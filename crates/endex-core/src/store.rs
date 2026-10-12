//! LanceDB table of searchable rows: one per document chunk, one per image.

use std::fmt;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use arrow_array::types::Float32Type;
use arrow_array::{
    Array, FixedSizeListArray, Float32Array, Float64Array, Int64Array, RecordBatch, StringArray,
};
use arrow_schema::{DataType, Field, Schema, SchemaRef};
use futures::TryStreamExt;
use lancedb::index::Index;
use lancedb::index::scalar::{FtsIndexBuilder, FtsQuery, FullTextSearchQuery, MultiMatchQuery};
use lancedb::query::{ExecutableQuery, QueryBase, Select};
use lancedb::table::OptimizeAction;
use lancedb::{DistanceType, Table};

use crate::Kind;
use crate::embed::DIM;

const TABLE: &str = "chunks";
/// Waits before re-trying a table that exists but cannot be loaded yet.
const OPEN_RETRY_DELAYS: [Duration; 3] = [
    Duration::from_millis(100),
    Duration::from_millis(200),
    Duration::from_millis(400),
];

/// The error inside `anyhow::Error` when the table exists but has no loadable version, as at
/// the start of `endex index --rebuild` in another process.
#[derive(Debug)]
pub struct TableUnavailable(pub String);

impl fmt::Display for TableUnavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the index table cannot be loaded yet: {}", self.0)
    }
}

impl std::error::Error for TableUnavailable {}

#[derive(Debug, Clone)]
pub struct Row {
    pub id: String,
    pub path: String,
    pub kind: Kind,
    pub title: String,
    pub location: String,
    pub text: String,
    /// Output of [`crate::terms::Analyzer::joined`] for `text`.
    pub terms: String,
    /// Output of [`crate::terms::Analyzer::name_terms`] for the parent directory and file name.
    pub name: String,
    pub vector: Vec<f32>,
    pub mtime: i64,
}

#[derive(Debug, Clone)]
pub struct StoredHit {
    pub id: String,
    pub path: String,
    pub kind: Kind,
    pub title: String,
    pub location: String,
    pub text: String,
    pub mtime: i64,
    pub score: f32,
}

pub struct Store {
    table: Table,
}

impl Store {
    pub async fn open(dir: &Path) -> Result<Self> {
        Self::connect(dir, false).await
    }

    /// The table if an index run has created it; never creates anything. A table that exists
    /// but does not load is retried briefly if `retry`, then reported as [`TableUnavailable`].
    pub async fn open_existing(dir: &Path, retry: bool) -> Result<Option<Self>> {
        if !dir.is_dir() {
            return Ok(None);
        }
        let db = lancedb::connect(dir.to_str().context("index path is not UTF-8")?)
            .execute()
            .await?;
        let delays: &[Duration] = if retry { &OPEN_RETRY_DELAYS } else { &[] };
        let mut delays = delays.iter();
        loop {
            if !db.table_names().execute().await?.iter().any(|t| t == TABLE) {
                return Ok(None);
            }
            match db.open_table(TABLE).execute().await {
                Ok(table) => return Ok(Some(Self { table })),
                Err(error) => match delays.next() {
                    Some(&delay) => tokio::time::sleep(delay).await,
                    None => return Err(TableUnavailable(error.to_string()).into()),
                },
            }
        }
    }

    /// Moves a long-lived reader to the newest version written by another process.
    pub async fn checkout_latest(&self) -> Result<()> {
        Ok(self.table.checkout_latest().await?)
    }

    /// Drops the endex table (only that) and starts a new, empty one.
    pub async fn reset(dir: &Path) -> Result<Self> {
        Self::connect(dir, true).await
    }

    async fn connect(dir: &Path, reset: bool) -> Result<Self> {
        let uri = dir.to_str().context("index path is not UTF-8")?;
        let db = lancedb::connect(uri).execute().await?;
        let exists = db.table_names().execute().await?.iter().any(|t| t == TABLE);
        if exists && reset {
            db.drop_table(TABLE, &[]).await?;
        }
        let table = if exists && !reset {
            db.open_table(TABLE).execute().await?
        } else {
            db.create_empty_table(TABLE, schema()).execute().await?
        };
        Ok(Self { table })
    }

    /// Deletes every row of `paths`, then inserts `rows` (which may cover only some of them).
    pub async fn replace(&self, paths: &[String], rows: &[Row]) -> Result<()> {
        if !paths.is_empty() {
            let filter = path_filter(paths);
            // A delete commits a new table version even when nothing matches.
            if self.table.count_rows(Some(filter.clone())).await? > 0 {
                self.table.delete(&filter).await?;
            }
        }
        if !rows.is_empty() {
            self.table.add(record_batch(rows)?).execute().await?;
        }
        Ok(())
    }

    /// Compacts fragments, rebuilds the keyword indexes and drops old versions so that deleted
    /// content does not linger on disk.
    pub async fn finish(&self) -> Result<()> {
        self.compact().await?;
        if self.table.count_rows(None).await? > 0 {
            self.build_keyword_index().await?;
        }
        self.prune().await
    }

    pub async fn compact(&self) -> Result<()> {
        self.table
            .optimize(OptimizeAction::Compact {
                options: Default::default(),
                remap_options: None,
            })
            .await?;
        Ok(())
    }

    /// Deletes all versions but the newest.
    pub async fn prune(&self) -> Result<()> {
        self.table
            .optimize(OptimizeAction::Prune {
                older_than: chrono::Duration::try_seconds(0),
                delete_unverified: Some(true),
                error_if_tagged_old_versions: Some(false),
            })
            .await?;
        Ok(())
    }

    /// Builds both keyword indexes from scratch.
    pub async fn build_keyword_index(&self) -> Result<()> {
        // Both columns hold pre-analyzed terms; Lance only lowercases and stems them and
        // drops English stop words.
        for column in ["terms", "name"] {
            let params = FtsIndexBuilder::default().base_tokenizer("whitespace".into());
            self.table
                .create_index(&[column], Index::FTS(params))
                .replace(true)
                .execute()
                .await
                .with_context(|| format!("building the {column} keyword index"))?;
        }
        Ok(())
    }

    pub async fn has_keyword_index(&self) -> Result<bool> {
        Ok(!self.table.list_indices().await?.is_empty())
    }

    /// Adds rows written since the keyword indexes were built to them. Until then keyword
    /// search scans those rows (with the same analyzer and the indexed BM25 statistics).
    pub async fn update_keyword_index(&self) -> Result<()> {
        self.table
            .optimize(OptimizeAction::Index(Default::default()))
            .await?;
        Ok(())
    }

    /// Rows not covered by the keyword indexes yet.
    pub async fn unindexed_rows(&self) -> Result<usize> {
        let mut most = 0;
        for index in self.table.list_indices().await? {
            if let Some(stats) = self.table.index_stats(&index.name).await? {
                most = most.max(stats.num_unindexed_rows);
            }
        }
        Ok(most)
    }

    pub async fn count(&self, kind: Kind) -> Result<usize> {
        Ok(self.table.count_rows(Some(kind_filter(kind))).await?)
    }

    pub async fn row(&self, id: &str) -> Result<Option<StoredHit>> {
        let batches: Vec<RecordBatch> = self
            .table
            .query()
            .only_if(format!("id = {}", quote(id)))
            .select(Select::columns(HIT_COLUMNS))
            .limit(1)
            .execute()
            .await?
            .try_collect()
            .await?;
        Ok(hits(&batches, None, |s| s)?.into_iter().next())
    }

    /// Ids of all rows of one file, in no particular order.
    pub async fn ids_of(&self, path: &str) -> Result<Vec<String>> {
        let batches: Vec<RecordBatch> = self
            .table
            .query()
            .only_if(format!("path = {}", quote(path)))
            .select(Select::columns(&["id"]))
            .execute()
            .await?
            .try_collect()
            .await?;
        let mut ids = Vec::new();
        for batch in &batches {
            ids.extend(
                string_column(batch, "id")?
                    .iter()
                    .flatten()
                    .map(str::to_owned),
            );
        }
        Ok(ids)
    }

    /// Exact (flat) cosine search; score = cosine similarity.
    pub async fn vector_search(
        &self,
        vector: &[f32],
        kind: Kind,
        limit: usize,
    ) -> Result<Vec<StoredHit>> {
        let batches: Vec<RecordBatch> = self
            .table
            .query()
            .nearest_to(vector)?
            .distance_type(DistanceType::Cosine)
            .only_if(kind_filter(kind))
            .select(Select::columns(HIT_COLUMNS))
            .limit(limit)
            .execute()
            .await?
            .try_collect()
            .await?;
        hits(&batches, Some("_distance"), |d| 1.0 - d)
    }

    /// BM25 of the query terms over the `terms` and `name` columns; a row scores the better
    /// of the two.
    pub async fn keyword_search(
        &self,
        terms: &str,
        kind: Kind,
        limit: usize,
    ) -> Result<Vec<StoredHit>> {
        if terms.trim().is_empty() || self.table.list_indices().await?.is_empty() {
            return Ok(Vec::new());
        }
        let query =
            MultiMatchQuery::try_new(terms.to_owned(), vec!["terms".into(), "name".into()])?;
        let batches: Vec<RecordBatch> = self
            .table
            .query()
            .full_text_search(FullTextSearchQuery {
                query: FtsQuery::MultiMatch(query),
                limit: None,
                wand_factor: None,
            })
            .only_if(kind_filter(kind))
            .select(Select::columns(HIT_COLUMNS))
            .limit(limit)
            .execute()
            .await?
            .try_collect()
            .await?;
        hits(&batches, Some("_score"), |s| s)
    }
}

const HIT_COLUMNS: &[&str] = &["id", "path", "kind", "title", "location", "text", "mtime"];

fn schema() -> SchemaRef {
    let text = |name| Field::new(name, DataType::Utf8, false);
    Arc::new(Schema::new(vec![
        text("id"),
        text("path"),
        text("kind"),
        text("title"),
        text("location"),
        text("text"),
        text("terms"),
        text("name"),
        Field::new(
            "vector",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                DIM as i32,
            ),
            false,
        ),
        Field::new("mtime", DataType::Int64, false),
    ]))
}

fn record_batch(rows: &[Row]) -> Result<RecordBatch> {
    let strings = |f: fn(&Row) -> &str| Arc::new(StringArray::from_iter_values(rows.iter().map(f)));
    let vectors = FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
        rows.iter()
            .map(|r| Some(r.vector.iter().copied().map(Some).collect::<Vec<_>>())),
        DIM as i32,
    );
    Ok(RecordBatch::try_new(
        schema(),
        vec![
            strings(|r| &r.id),
            strings(|r| &r.path),
            strings(|r| r.kind.as_str()),
            strings(|r| &r.title),
            strings(|r| &r.location),
            strings(|r| &r.text),
            strings(|r| &r.terms),
            strings(|r| &r.name),
            Arc::new(vectors),
            Arc::new(Int64Array::from_iter_values(rows.iter().map(|r| r.mtime))),
        ],
    )?)
}

/// Rows of `HIT_COLUMNS`; without a score column every score is 0.
fn hits(
    batches: &[RecordBatch],
    score_column: Option<&str>,
    score: fn(f32) -> f32,
) -> Result<Vec<StoredHit>> {
    let mut out = Vec::new();
    for batch in batches {
        let string = |name| string_column(batch, name);
        let (id, path, kind, title, location, text) = (
            string("id")?,
            string("path")?,
            string("kind")?,
            string("title")?,
            string("location")?,
            string("text")?,
        );
        let mtime = batch
            .column_by_name("mtime")
            .and_then(|c| c.as_any().downcast_ref::<Int64Array>())
            .context("missing column mtime")?;
        let scores: Vec<f32> = match score_column {
            None => vec![0.0; batch.num_rows()],
            Some(name) => {
                let scores = batch
                    .column_by_name(name)
                    .with_context(|| format!("missing column {name}"))?;
                if let Some(a) = scores.as_any().downcast_ref::<Float32Array>() {
                    a.values().to_vec()
                } else if let Some(a) = scores.as_any().downcast_ref::<Float64Array>() {
                    a.values().iter().map(|&v| v as f32).collect()
                } else {
                    anyhow::bail!("unexpected type of {name}: {}", scores.data_type());
                }
            }
        };
        for (i, &raw) in scores.iter().enumerate() {
            out.push(StoredHit {
                id: id.value(i).to_owned(),
                path: path.value(i).to_owned(),
                kind: kind.value(i).parse()?,
                title: title.value(i).to_owned(),
                location: location.value(i).to_owned(),
                text: text.value(i).to_owned(),
                mtime: mtime.value(i),
                score: score(raw),
            });
        }
    }
    Ok(out)
}

fn string_column<'a>(batch: &'a RecordBatch, name: &str) -> Result<&'a StringArray> {
    batch
        .column_by_name(name)
        .and_then(|c| c.as_any().downcast_ref::<StringArray>())
        .with_context(|| format!("missing column {name}"))
}

fn kind_filter(kind: Kind) -> String {
    format!("kind = '{}'", kind.as_str())
}

fn path_filter(paths: &[String]) -> String {
    let quoted: Vec<String> = paths.iter().map(|p| quote(p)).collect();
    format!("path IN ({})", quoted.join(", "))
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

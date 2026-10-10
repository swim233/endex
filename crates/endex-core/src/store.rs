//! LanceDB table of searchable rows: one per document chunk, one per image.

use std::path::Path;
use std::sync::Arc;

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
    pub path: String,
    pub kind: Kind,
    pub title: String,
    pub location: String,
    pub text: String,
    pub score: f32,
}

pub struct Store {
    table: Table,
}

impl Store {
    pub async fn open(dir: &Path) -> Result<Self> {
        Self::connect(dir, false).await
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
        self.table
            .optimize(OptimizeAction::Compact {
                options: Default::default(),
                remap_options: None,
            })
            .await?;
        if self.table.count_rows(None).await? > 0 {
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
        }
        self.table
            .optimize(OptimizeAction::Prune {
                older_than: chrono::Duration::try_seconds(0),
                delete_unverified: Some(true),
                error_if_tagged_old_versions: Some(false),
            })
            .await?;
        Ok(())
    }

    pub async fn count(&self, kind: Kind) -> Result<usize> {
        Ok(self.table.count_rows(Some(kind_filter(kind))).await?)
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
        hits(&batches, "_distance", |d| 1.0 - d)
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
        hits(&batches, "_score", |s| s)
    }
}

const HIT_COLUMNS: &[&str] = &["path", "kind", "title", "location", "text"];

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

fn hits(
    batches: &[RecordBatch],
    score_column: &str,
    score: fn(f32) -> f32,
) -> Result<Vec<StoredHit>> {
    let mut out = Vec::new();
    for batch in batches {
        let string = |name: &str| -> Result<&StringArray> {
            batch
                .column_by_name(name)
                .and_then(|c| c.as_any().downcast_ref::<StringArray>())
                .with_context(|| format!("missing column {name}"))
        };
        let (path, kind, title, location, text) = (
            string("path")?,
            string("kind")?,
            string("title")?,
            string("location")?,
            string("text")?,
        );
        let scores = batch
            .column_by_name(score_column)
            .with_context(|| format!("missing column {score_column}"))?;
        let scores: Vec<f32> = if let Some(a) = scores.as_any().downcast_ref::<Float32Array>() {
            a.values().to_vec()
        } else if let Some(a) = scores.as_any().downcast_ref::<Float64Array>() {
            a.values().iter().map(|&v| v as f32).collect()
        } else {
            anyhow::bail!("unexpected type of {score_column}: {}", scores.data_type());
        };
        for (i, &raw) in scores.iter().enumerate() {
            out.push(StoredHit {
                path: path.value(i).to_owned(),
                kind: kind.value(i).parse()?,
                title: title.value(i).to_owned(),
                location: location.value(i).to_owned(),
                text: text.value(i).to_owned(),
                score: score(raw),
            });
        }
    }
    Ok(out)
}

fn kind_filter(kind: Kind) -> String {
    format!("kind = '{}'", kind.as_str())
}

fn path_filter(paths: &[String]) -> String {
    let quoted: Vec<String> = paths
        .iter()
        .map(|p| format!("'{}'", p.replace('\'', "''")))
        .collect();
    format!("path IN ({})", quoted.join(", "))
}

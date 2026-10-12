//! `endex eval --sets`: every public set that `just eval` prepared under `<data_dir>/eval/`,
//! searched without writing anything there. The results go to
//! `<data_dir>/eval-results/last.json`, which the settings window shows.

use std::fmt::Write as _;
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use endex_core::catalog::Catalog;
use endex_core::config::Config;
use endex_core::search::Searcher;
use endex_core::{Kind, embed, eval};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Saved {
    /// Unix seconds.
    pub time: u64,
    pub model: String,
    pub limit: usize,
    pub sets: Vec<SetResult>,
}

#[derive(Serialize, Deserialize)]
pub struct SetResult {
    /// The folder under `eval/`, e.g. `xm3600`.
    pub set: String,
    /// The query file's name without `.toml`, e.g. `queries.en`.
    pub queries: String,
    /// What the set's files are: `doc` or `image`.
    pub kind: String,
    pub count: usize,
    pub modes: Vec<ModeResult>,
}

#[derive(Serialize, Deserialize)]
pub struct ModeResult {
    pub mode: String,
    pub recall: f64,
    pub mrr: f64,
}

struct Set {
    name: String,
    queries: PathBuf,
    db: PathBuf,
}

pub fn results_path(config: &Config) -> PathBuf {
    config.data_dir.join("eval-results/last.json")
}

pub async fn run(config: &Config, limit: usize) -> Result<Saved> {
    let sets = find(&config.data_dir.join("eval"))?;
    let mut results = Vec::new();
    for (i, set) in sets.iter().enumerate() {
        let queries_name = stem(&set.queries);
        let _ = writeln!(
            io::stderr(),
            "[{}/{}] {} {queries_name}",
            i + 1,
            sets.len(),
            set.name
        );
        let config = config.clone().with_index_root(Some(set.db.clone()));
        let counts = Catalog::open_untouched(&config.catalog_path())?.counts()?;
        let files = |kind| {
            counts
                .ok
                .iter()
                .filter(|(k, _)| *k == kind)
                .map(|(_, n)| n)
                .sum::<usize>()
        };
        let queries = eval::load(&set.queries)?;
        let mut searcher = Searcher::untouched(&config);
        let reports = eval::run(&mut searcher, &queries, limit)
            .await
            .with_context(|| format!("evaluating {} {queries_name}", set.name))?;
        let kind = if files(Kind::Image) > files(Kind::Doc) {
            Kind::Image
        } else {
            Kind::Doc
        };
        results.push(SetResult {
            set: set.name.clone(),
            queries: queries_name,
            kind: kind.as_str().to_owned(),
            count: queries.len(),
            modes: reports
                .iter()
                .map(|r| ModeResult {
                    mode: r.mode.as_str().to_owned(),
                    recall: r.overall.recall(),
                    mrr: r.overall.mrr(),
                })
                .collect(),
        });
    }
    Ok(Saved {
        time: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        model: embed::model_identity(config)?,
        limit,
        sets: results,
    })
}

pub fn save(config: &Config, saved: &Saved) -> Result<PathBuf> {
    let path = results_path(config);
    fs::create_dir_all(path.parent().expect("a file in a folder"))?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(saved)? + "\n")?;
    fs::rename(&tmp, &path)?;
    Ok(path)
}

pub fn load(config: &Config) -> Result<Option<Saved>> {
    let path = results_path(config);
    match fs::read_to_string(&path) {
        Ok(text) => Ok(Some(
            serde_json::from_str(&text).with_context(|| format!("reading {}", path.display()))?,
        )),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

pub fn table(saved: &Saved) -> Result<String> {
    let mut out = String::new();
    writeln!(
        out,
        "{:<16} {:<12} {:<6} {:>14} {:>14} {:>14}",
        "set", "queries", "kind", "vector R/MRR", "keyword R/MRR", "hybrid R/MRR"
    )?;
    for set in &saved.sets {
        write!(out, "{:<16} {:<12} {:<6}", set.set, set.queries, set.kind)?;
        for mode in ["vector", "keyword", "hybrid"] {
            let cell = set
                .modes
                .iter()
                .find(|m| m.mode == mode)
                .map_or("-".to_owned(), |m| format!("{:.3}/{:.3}", m.recall, m.mrr));
            write!(out, " {cell:>14}")?;
        }
        writeln!(out)?;
    }
    writeln!(
        out,
        "\nRecall@{} / MRR over {} queries per set; model {}.",
        saved.limit,
        saved.sets.first().map_or(0, |s| s.count),
        saved.model
    )?;
    Ok(out)
}

/// `<set>/queries*.toml` next to a `<set>/db` index, in name order.
fn find(dir: &Path) -> Result<Vec<Set>> {
    let mut sets = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            bail!(
                "no public eval sets in {}; prepare them with `just eval`",
                dir.display()
            )
        }
        Err(e) => return Err(e).with_context(|| format!("reading {}", dir.display())),
    };
    for entry in entries {
        let folder = entry?.path();
        let db = folder.join("db");
        if !db.join("catalog.sqlite").is_file() {
            continue;
        }
        let name = folder
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        for file in fs::read_dir(&folder)? {
            let queries = file?.path();
            let file_name = queries.file_name().unwrap_or_default().to_string_lossy();
            if file_name.starts_with("queries") && file_name.ends_with(".toml") {
                sets.push(Set {
                    name: name.clone(),
                    queries,
                    db: db.clone(),
                });
            }
        }
    }
    if sets.is_empty() {
        bail!(
            "no public eval sets in {}; prepare them with `just eval`",
            dir.display()
        );
    }
    sets.sort_by(|a, b| (&a.name, &a.queries).cmp(&(&b.name, &b.queries)));
    Ok(sets)
}

fn stem(path: &Path) -> String {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    name.strip_suffix(".toml").unwrap_or(&name).to_owned()
}

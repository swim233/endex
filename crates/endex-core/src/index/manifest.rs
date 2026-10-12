//! `endex index`: indexing the files listed in manifests.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};

use super::{Event, Indexer, Report};
use crate::catalog::{Catalog, Identity};
use crate::config::{Config, expand_home};
use crate::embed::Device;

pub struct Options {
    pub manifests: Vec<PathBuf>,
    pub device: Device,
    pub rebuild: bool,
}

/// Indexes the files listed in manifests; files that left a manifest are removed.
pub async fn run(config: &Config, options: &Options, on: &mut dyn FnMut(Event)) -> Result<Report> {
    let started = Instant::now();
    let mut indexer = Indexer::open(config, options.rebuild).await?;
    indexer.catalog.ensure_identity(&Identity::of(config)?)?;
    indexer.set_device(options.device);

    let (entries, skipped) = read_manifests(&options.manifests)?;
    for s in &skipped {
        on(Event::Skipped {
            manifest: &s.manifest,
            line: s.line,
            reason: s.reason,
        });
    }
    indexer.report.skipped = skipped.len();
    // A skipped line can name a file that is still there (a relative path, a name in another
    // encoding), so with any line skipped nothing counts as removed in this run.
    if skipped.is_empty() {
        let removed = removed_paths(&indexer.catalog, &options.manifests, &entries)?;
        indexer.remove(&removed).await?;
    }
    for (i, entry) in entries.iter().enumerate() {
        on(Event::Progress {
            done: i,
            total: entries.len(),
        });
        indexer.update(&entry.path, &entry.manifest, on).await?;
    }
    indexer.flush().await?;
    on(Event::Progress {
        done: entries.len(),
        total: entries.len(),
    });
    // Release the GPU before the (CPU-only) index build.
    indexer.release_models();
    indexer.finish().await?;
    let mut report = std::mem::take(&mut indexer.report);
    report.total = started.elapsed();
    Ok(report)
}

struct Entry {
    path: String,
    manifest: String,
}

struct SkippedLine {
    manifest: PathBuf,
    line: usize,
    reason: &'static str,
}

/// Lines are read as bytes: one non-UTF-8 or relative line (a file name with a newline, say)
/// is skipped instead of failing the whole run.
fn read_manifests(manifests: &[PathBuf]) -> Result<(Vec<Entry>, Vec<SkippedLine>)> {
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    let mut skipped = Vec::new();
    for manifest in manifests {
        let key = manifest_key(manifest)?;
        let bytes = fs::read(manifest)
            .with_context(|| format!("reading manifest {}", manifest.display()))?;
        for (i, line) in bytes.split(|&b| b == b'\n').enumerate() {
            let line = line.trim_ascii();
            if line.is_empty() || line.starts_with(b"#") {
                continue;
            }
            let skip = |reason| SkippedLine {
                manifest: manifest.clone(),
                line: i + 1,
                reason,
            };
            let Ok(line) = std::str::from_utf8(line) else {
                skipped.push(skip("not UTF-8"));
                continue;
            };
            let path = expand_home(Path::new(line));
            if !path.is_absolute() {
                skipped.push(skip("not an absolute path"));
                continue;
            }
            let path = path.to_string_lossy().into_owned();
            if seen.insert(path.clone()) {
                entries.push(Entry {
                    path,
                    manifest: key.clone(),
                });
            }
        }
    }
    Ok((entries, skipped))
}

/// Catalog entries of the given manifests that the manifests no longer list.
fn removed_paths(
    catalog: &Catalog,
    manifests: &[PathBuf],
    entries: &[Entry],
) -> Result<Vec<String>> {
    let listed: HashSet<&str> = entries.iter().map(|e| e.path.as_str()).collect();
    let mut removed = Vec::new();
    for manifest in manifests {
        for path in catalog.paths_of_owner(&manifest_key(manifest)?)? {
            if !listed.contains(path.as_str()) {
                removed.push(path);
            }
        }
    }
    Ok(removed)
}

fn manifest_key(manifest: &Path) -> Result<String> {
    Ok(fs::canonicalize(manifest)
        .with_context(|| format!("manifest {}", manifest.display()))?
        .to_string_lossy()
        .into_owned())
}

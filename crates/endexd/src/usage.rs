//! The local usage log, `<data_dir>/usage.jsonl`: one line per popup session (a picked result,
//! or the popup hidden after a non-empty query). It feeds the popup's recent searches.

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Past this size the oldest half of the entries is dropped (about 3,000 entries remain).
const MAX_BYTES: u64 = 1 << 20;

/// One writer at a time: an append can rewrite the file.
static WRITING: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    /// Local time, RFC 3339.
    pub t: String,
    pub q: String,
    pub mode: String,
    /// Results shown.
    pub n: u32,
    /// Search latency in milliseconds.
    pub ms: u32,
    /// The path of the picked result, if any.
    pub pick: Option<String>,
}

pub struct UsageLog {
    path: PathBuf,
}

impl UsageLog {
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_owned(),
        }
    }

    pub fn append(&self, mut entry: Entry) -> Result<()> {
        entry.t = chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let mut line = serde_json::to_string(&entry)?;
        line.push('\n');
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let _writing = WRITING.lock().unwrap_or_else(|e| e.into_inner());
        let mut file = private(OpenOptions::new().append(true)).open(&self.path)?;
        file.write_all(line.as_bytes())?;
        if file.metadata()?.len() > MAX_BYTES {
            let lines = self.lines();
            let keep = lines[lines.len() / 2..].join("\n") + "\n";
            let tmp = self.path.with_extension("jsonl.tmp");
            private(OpenOptions::new().write(true).truncate(true))
                .open(&tmp)?
                .write_all(keep.as_bytes())?;
            fs::rename(&tmp, &self.path)?;
        }
        Ok(())
    }

    /// The last `limit` lines, oldest first.
    pub fn tail(&self, limit: usize) -> Vec<String> {
        let lines = self.lines();
        lines[lines.len().saturating_sub(limit)..].to_vec()
    }

    /// Distinct queries, the most recent first.
    pub fn recent_queries(&self, limit: usize) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        self.lines()
            .iter()
            .rev()
            .filter_map(|line| serde_json::from_str::<Entry>(line).ok())
            .map(|e| e.q.trim().to_owned())
            .filter(|q| !q.is_empty() && seen.insert(q.clone()))
            .take(limit)
            .collect()
    }

    pub fn clear(&self) -> Result<()> {
        let _writing = WRITING.lock().unwrap_or_else(|e| e.into_inner());
        match fs::remove_file(&self.path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }

    fn lines(&self) -> Vec<String> {
        fs::read_to_string(&self.path)
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(str::to_owned)
            .collect()
    }
}

/// Queries and opened paths are private: readable by the user only.
fn private(options: &mut OpenOptions) -> &mut OpenOptions {
    options.create(true).mode(0o600)
}

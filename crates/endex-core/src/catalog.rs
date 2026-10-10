//! SQLite catalog: what was indexed, from which manifest, and with which model.

use std::path::Path;

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};

use crate::Kind;

pub const STATUS_OK: &str = "ok";
pub const STATUS_ERROR: &str = "error";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRecord {
    pub path: String,
    pub kind: Kind,
    pub size: u64,
    pub mtime: i64,
    pub hash: String,
    /// [`STATUS_OK`] or [`STATUS_ERROR`].
    pub status: String,
    pub error: Option<String>,
    pub indexed_at: i64,
    pub manifest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    New {
        hash: String,
    },
    Modified {
        hash: String,
    },
    /// Same content, new mtime: refresh the catalog, keep the rows.
    Touched {
        hash: String,
    },
    Unchanged,
}

/// Size and mtime short-circuit; otherwise the content hash decides. Files whose last attempt
/// failed are always reprocessed.
pub fn detect_change(
    prev: Option<&FileRecord>,
    size: u64,
    mtime: i64,
    hash: impl FnOnce() -> Result<String>,
) -> Result<Change> {
    let Some(prev) = prev else {
        return Ok(Change::New { hash: hash()? });
    };
    let failed = prev.status != STATUS_OK;
    if !failed && prev.size == size && prev.mtime == mtime {
        return Ok(Change::Unchanged);
    }
    let hash = hash()?;
    Ok(if !failed && hash == prev.hash {
        Change::Touched { hash }
    } else {
        Change::Modified { hash }
    })
}

pub struct Catalog {
    conn: Connection,
}

#[derive(Debug, Default)]
pub struct Counts {
    pub ok: Vec<(Kind, usize)>,
    pub errors: usize,
}

impl Catalog {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS files (
                 path TEXT PRIMARY KEY,
                 kind TEXT NOT NULL,
                 size INTEGER NOT NULL,
                 mtime INTEGER NOT NULL,
                 hash TEXT NOT NULL,
                 status TEXT NOT NULL,
                 error TEXT,
                 indexed_at INTEGER NOT NULL,
                 manifest TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )?;
        Ok(Self { conn })
    }

    pub fn get(&self, path: &str) -> Result<Option<FileRecord>> {
        self.conn
            .query_row(
                "SELECT path, kind, size, mtime, hash, status, error, indexed_at, manifest
                 FROM files WHERE path = ?1",
                [path],
                |r| {
                    Ok(FileRecord {
                        path: r.get(0)?,
                        kind: r.get::<_, String>(1)?.parse().unwrap_or(Kind::Doc),
                        size: r.get::<_, i64>(2)? as u64,
                        mtime: r.get(3)?,
                        hash: r.get(4)?,
                        status: r.get(5)?,
                        error: r.get(6)?,
                        indexed_at: r.get(7)?,
                        manifest: r.get(8)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn upsert(&mut self, records: &[FileRecord]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO files
                 (path, kind, size, mtime, hash, status, error, indexed_at, manifest)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )?;
            for r in records {
                stmt.execute(params![
                    r.path,
                    r.kind.as_str(),
                    r.size as i64,
                    r.mtime,
                    r.hash,
                    r.status,
                    r.error,
                    r.indexed_at,
                    r.manifest
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn paths_of_manifest(&self, manifest: &str) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT path FROM files WHERE manifest = ?1")?;
        let rows = stmt.query_map([manifest], |r| r.get(0))?;
        rows.collect::<rusqlite::Result<_>>().map_err(Into::into)
    }

    pub fn remove(&mut self, paths: &[String]) -> Result<()> {
        let tx = self.conn.transaction()?;
        for path in paths {
            tx.execute("DELETE FROM files WHERE path = ?1", [path])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn clear(&mut self) -> Result<()> {
        self.conn
            .execute_batch("DELETE FROM files; DELETE FROM meta;")?;
        Ok(())
    }

    pub fn meta(&self, key: &str) -> Result<Option<String>> {
        self.conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
            .optional()
            .map_err(Into::into)
    }

    pub fn set_meta(&self, key: &str, value: Option<&str>) -> Result<()> {
        match value {
            Some(value) => self.conn.execute(
                "INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)",
                [key, value],
            )?,
            None => self
                .conn
                .execute("DELETE FROM meta WHERE key = ?1", [key])?,
        };
        Ok(())
    }

    /// Fails if the index was built with another model or prefix style: vectors must not mix.
    pub fn check_embedding_identity(&self, model: &str, prefix_style: &str) -> Result<()> {
        for (key, value) in [("model", model), ("prefix_style", prefix_style)] {
            if let Some(stored) = self.meta(key)?.filter(|stored| stored != value) {
                bail!(
                    "the index was built with {key} `{stored}` but the config now says `{value}`; \
                     run `endex index --rebuild ...` to re-index"
                );
            }
        }
        Ok(())
    }

    /// Checks the identity and records it for a new index.
    pub fn ensure_embedding_identity(&self, model: &str, prefix_style: &str) -> Result<()> {
        self.check_embedding_identity(model, prefix_style)?;
        for (key, value) in [("model", model), ("prefix_style", prefix_style)] {
            self.conn.execute(
                "INSERT OR IGNORE INTO meta (key, value) VALUES (?1, ?2)",
                [key, value],
            )?;
        }
        Ok(())
    }

    pub fn counts(&self) -> Result<Counts> {
        let mut stmt = self.conn.prepare(
            "SELECT kind, COUNT(*) FROM files WHERE status = 'ok' GROUP BY kind ORDER BY kind",
        )?;
        let ok = stmt
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize))
            })?
            .filter_map(|r| r.ok())
            .filter_map(|(kind, n)| Some((kind.parse().ok()?, n)))
            .collect();
        let errors: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM files WHERE status != 'ok'", [], |r| {
                    r.get(0)
                })?;
        let errors = errors as usize;
        Ok(Counts { ok, errors })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(size: u64, mtime: i64, hash: &str) -> FileRecord {
        FileRecord {
            path: "/a".into(),
            kind: Kind::Doc,
            size,
            mtime,
            hash: hash.into(),
            status: STATUS_OK.into(),
            error: None,
            indexed_at: 0,
            manifest: "m".into(),
        }
    }

    #[test]
    fn change_detection() {
        let prev = record(10, 100, "h1");
        let hash = |h: &'static str| move || Ok(h.to_string());
        let no_hash = || -> Result<String> { panic!("hashed an unchanged file") };
        assert_eq!(
            detect_change(None, 10, 100, hash("h1")).unwrap(),
            Change::New { hash: "h1".into() }
        );
        assert_eq!(
            detect_change(Some(&prev), 10, 100, no_hash).unwrap(),
            Change::Unchanged
        );
        assert_eq!(
            detect_change(Some(&prev), 10, 200, hash("h1")).unwrap(),
            Change::Touched { hash: "h1".into() }
        );
        assert_eq!(
            detect_change(Some(&prev), 12, 200, hash("h2")).unwrap(),
            Change::Modified { hash: "h2".into() }
        );
        let failed = FileRecord {
            status: STATUS_ERROR.into(),
            ..prev
        };
        assert_eq!(
            detect_change(Some(&failed), 10, 100, hash("h1")).unwrap(),
            Change::Modified { hash: "h1".into() }
        );
    }
}

//! SQLite catalog: what was indexed, who listed it (a manifest or a source folder), and with
//! which model.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use crate::Kind;
use crate::config::Config;
use crate::embed;

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
    /// The canonical path of the manifest (`endex index`) or the source folder (endexd) that
    /// lists the file.
    pub owner: String,
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

/// What a scan compares a file on disk with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Known {
    pub size: u64,
    pub mtime: i64,
    pub ok: bool,
}

/// The vector space and text extraction an index was built with; rows of different
/// identities must not be mixed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub model: String,
    pub prefix_style: String,
    pub ocr: String,
}

impl Identity {
    pub fn of(config: &Config) -> Result<Self> {
        Ok(Self {
            model: embed::model_identity(config)?,
            prefix_style: config.embedding.prefix_style.as_str().to_owned(),
            ocr: if config.ocr.enabled {
                "pp-ocrv6_small"
            } else {
                "off"
            }
            .to_owned(),
        })
    }

    fn entries(&self) -> [(&'static str, &str); 3] {
        [
            ("model", &self.model),
            ("prefix_style", &self.prefix_style),
            ("ocr", &self.ocr),
        ]
    }
}

/// One differing part of an [`Identity`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mismatch {
    pub key: &'static str,
    pub stored: String,
    pub wanted: String,
}

#[derive(Debug, Default)]
pub struct Counts {
    pub ok: Vec<(Kind, usize)>,
    pub errors: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerCounts {
    pub owner: String,
    pub files: usize,
    pub errors: usize,
}

pub struct Catalog {
    conn: Connection,
}

impl Catalog {
    pub fn open(path: &Path) -> Result<Self> {
        let mut conn = Connection::open(path)
            .map_err(|e| anyhow::anyhow!("opening {}: {e}", path.display()))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
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
                 owner TEXT NOT NULL,
                 ext TEXT NOT NULL DEFAULT ''
             );
             CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )?;
        migrate(&mut conn)?;
        conn.execute_batch("CREATE INDEX IF NOT EXISTS files_owner ON files (owner);")?;
        Ok(Self { conn })
    }

    /// For readers next to a running indexer: no schema changes, never creates the file.
    pub fn open_read_only(path: &Path) -> Result<Self> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| anyhow::anyhow!("opening {}: {e}", path.display()))?;
        conn.busy_timeout(std::time::Duration::from_secs(2))?;
        Ok(Self { conn })
    }

    /// Reads without writing anything, not even SQLite's shared memory next to the file: for
    /// catalogs that must stay as they are, such as the public eval sets.
    pub fn open_untouched(path: &Path) -> Result<Self> {
        let text = path.to_str().context("catalog path is not UTF-8")?;
        let mut uri = String::from("file:");
        for c in text.chars() {
            match c {
                '%' | '?' | '#' => uri.push_str(&format!("%{:02X}", c as u32)),
                c => uri.push(c),
            }
        }
        // Without pending WAL frames the file is all there is: `immutable` opens nothing else
        // (SQLite would open, and maybe create, the -wal file read-write otherwise).
        let wal = path.with_extension("sqlite-wal");
        let pending = std::fs::metadata(&wal).is_ok_and(|m| m.len() > 0);
        uri.push_str(match pending {
            true => "?mode=ro&readonly_shm=1",
            false => "?mode=ro&immutable=1",
        });
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI;
        let conn = Connection::open_with_flags(&uri, flags)
            .map_err(|e| anyhow::anyhow!("opening {}: {e}", path.display()))?;
        conn.busy_timeout(std::time::Duration::from_secs(2))?;
        Ok(Self { conn })
    }

    pub fn get(&self, path: &str) -> Result<Option<FileRecord>> {
        self.conn
            .query_row(
                "SELECT path, kind, size, mtime, hash, status, error, indexed_at, owner
                 FROM files WHERE path = ?1",
                [path],
                record,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn upsert(&mut self, records: &[FileRecord]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO files
                 (path, kind, size, mtime, hash, status, error, indexed_at, owner, ext)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
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
                    r.owner,
                    extension(&r.path)
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn paths_of_owner(&self, owner: &str) -> Result<Vec<String>> {
        self.strings("SELECT path FROM files WHERE owner = ?1", [owner])
    }

    /// Files listed by none of `owners`.
    pub fn paths_not_owned_by(&self, owners: &[String]) -> Result<Vec<String>> {
        let marks = vec!["?"; owners.len()].join(", ");
        self.strings(
            &format!("SELECT path FROM files WHERE owner NOT IN ({marks})"),
            rusqlite::params_from_iter(owners),
        )
    }

    /// Whether the catalog has any file at or below `dir`.
    pub fn has_files_under(&self, dir: &Path) -> Result<bool> {
        let dir = dir.to_string_lossy();
        let low = format!("{}/", dir.trim_end_matches('/'));
        let high = format!("{}0", dir.trim_end_matches('/'));
        let found: Option<i64> = self
            .conn
            .query_row(
                "SELECT 1 FROM files WHERE (path >= ?1 AND path < ?2) OR path = ?3 LIMIT 1",
                params![low, high, dir.as_ref()],
                |r| r.get(0),
            )
            .optional()?;
        Ok(found.is_some())
    }

    /// Catalog entries at or below `dir`, by path.
    pub fn under(&self, dir: &Path) -> Result<BTreeMap<String, Known>> {
        let dir = dir.to_string_lossy();
        // Paths below `dir/` sort between `dir/` and `dir0` ('0' follows '/').
        let low = format!("{}/", dir.trim_end_matches('/'));
        let high = format!("{}0", dir.trim_end_matches('/'));
        let mut stmt = self.conn.prepare(
            "SELECT path, size, mtime, status FROM files
             WHERE (path >= ?1 AND path < ?2) OR path = ?3",
        )?;
        let rows = stmt.query_map(params![low, high, dir.as_ref()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                Known {
                    size: r.get::<_, i64>(1)? as u64,
                    mtime: r.get(2)?,
                    ok: r.get::<_, String>(3)? == STATUS_OK,
                },
            ))
        })?;
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

    /// Forgets every file but keeps the meta flags.
    pub fn clear_files(&mut self) -> Result<()> {
        self.conn.execute_batch("DELETE FROM files;")?;
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

    /// The first part of `identity` that differs from what the index was built with. Parts
    /// not recorded yet (older indexes have no `ocr`) count as matching.
    pub fn identity_mismatch(&self, identity: &Identity) -> Result<Option<Mismatch>> {
        for (key, wanted) in identity.entries() {
            if let Some(stored) = self.meta(key)?.filter(|stored| stored != wanted) {
                return Ok(Some(Mismatch {
                    key,
                    stored,
                    wanted: wanted.to_owned(),
                }));
            }
        }
        Ok(None)
    }

    /// Fails if the index was built with another identity; records the missing parts.
    pub fn ensure_identity(&self, identity: &Identity) -> Result<()> {
        if let Some(m) = self.identity_mismatch(identity)? {
            bail!(
                "the index was built with {} `{}` but the config now says `{}`; \
                 run `endex index --rebuild ...` to re-index",
                m.key,
                m.stored,
                m.wanted
            );
        }
        for (key, value) in identity.entries() {
            self.conn.execute(
                "INSERT OR IGNORE INTO meta (key, value) VALUES (?1, ?2)",
                [key, value],
            )?;
        }
        Ok(())
    }

    /// Records `identity` for an index that has no files yet.
    pub fn set_identity(&self, identity: &Identity) -> Result<()> {
        for (key, value) in identity.entries() {
            self.set_meta(key, Some(value))?;
        }
        Ok(())
    }

    /// Unix time of the most recent indexing attempt.
    pub fn last_indexed(&self) -> Result<Option<i64>> {
        self.conn
            .query_row("SELECT MAX(indexed_at) FROM files", [], |r| r.get(0))
            .map_err(Into::into)
    }

    pub fn file_count(&self) -> Result<usize> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?;
        Ok(n as usize)
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

    /// Indexed files per extension (lowercase, without the dot).
    pub fn formats(&self) -> Result<Vec<(String, usize)>> {
        let mut stmt = self.conn.prepare(
            "SELECT ext, COUNT(*) FROM files WHERE status = 'ok' GROUP BY ext ORDER BY ext",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? as usize)))?;
        rows.collect::<rusqlite::Result<_>>().map_err(Into::into)
    }

    pub fn owner_counts(&self) -> Result<Vec<OwnerCounts>> {
        let mut stmt = self.conn.prepare(
            "SELECT owner, SUM(status = 'ok'), SUM(status != 'ok') FROM files GROUP BY owner",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(OwnerCounts {
                owner: r.get(0)?,
                files: r.get::<_, i64>(1)? as usize,
                errors: r.get::<_, i64>(2)? as usize,
            })
        })?;
        rows.collect::<rusqlite::Result<_>>().map_err(Into::into)
    }

    /// Files whose last attempt failed, the newest first.
    pub fn failed(&self, limit: usize) -> Result<Vec<FileRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT path, kind, size, mtime, hash, status, error, indexed_at, owner
             FROM files WHERE status != 'ok' ORDER BY indexed_at DESC, path LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit as i64], record)?;
        rows.collect::<rusqlite::Result<_>>().map_err(Into::into)
    }

    fn strings(&self, sql: &str, params: impl rusqlite::Params) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(params, |r| r.get(0))?;
        rows.collect::<rusqlite::Result<_>>().map_err(Into::into)
    }
}

fn record(r: &rusqlite::Row) -> rusqlite::Result<FileRecord> {
    Ok(FileRecord {
        path: r.get(0)?,
        kind: r.get::<_, String>(1)?.parse().unwrap_or(Kind::Doc),
        size: r.get::<_, i64>(2)? as u64,
        mtime: r.get(3)?,
        hash: r.get(4)?,
        status: r.get(5)?,
        error: r.get(6)?,
        indexed_at: r.get(7)?,
        owner: r.get(8)?,
    })
}

fn extension(path: &str) -> String {
    Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// Catalogs written before M3 call the owner `manifest` and have no `ext`.
fn migrate(conn: &mut Connection) -> Result<()> {
    let columns: Vec<String> = {
        let mut stmt = conn.prepare("SELECT name FROM pragma_table_info('files')")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    let has = |name: &str| columns.iter().any(|c| c == name);
    let tx = conn.transaction()?;
    if has("manifest") {
        tx.execute_batch("ALTER TABLE files RENAME COLUMN manifest TO owner;")?;
    }
    if !has("ext") {
        tx.execute_batch("ALTER TABLE files ADD COLUMN ext TEXT NOT NULL DEFAULT '';")?;
        let paths: Vec<String> = {
            let mut stmt = tx.prepare("SELECT path FROM files")?;
            let rows = stmt.query_map([], |r| r.get(0))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        let mut update = tx.prepare("UPDATE files SET ext = ?1 WHERE path = ?2")?;
        for path in &paths {
            update.execute([extension(path), path.clone()])?;
        }
    }
    tx.commit()?;
    Ok(())
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
            owner: "m".into(),
        }
    }

    #[test]
    fn untouched_reads_pending_wal_frames_and_writes_nothing() {
        let dir = std::env::temp_dir().join(format!("endex-catalog-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("catalog.sqlite");
        let listing = || {
            let mut names: Vec<_> = std::fs::read_dir(&dir)
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect();
            names.sort();
            names
        };
        let mut writer = Catalog::open(&path).unwrap();
        writer.upsert(&[record(10, 100, "h1")]).unwrap();
        let before = listing();
        assert!(
            Catalog::open_untouched(&path)
                .unwrap()
                .get("/a")
                .unwrap()
                .is_some()
        );
        assert_eq!(listing(), before);
        drop(writer);
        let before = listing();
        assert!(
            Catalog::open_untouched(&path)
                .unwrap()
                .get("/a")
                .unwrap()
                .is_some()
        );
        assert_eq!(listing(), before);
        std::fs::remove_dir_all(&dir).unwrap();
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

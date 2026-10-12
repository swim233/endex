//! The writer lock: one process at a time writes an index (endexd, or `endex index` on an
//! index no daemon owns).

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::config::Config;

/// Holds the index's writer lock (`flock` on `<index root>/endex.lock`) while alive.
pub struct WriterLock(#[allow(dead_code)] fs::File);

/// The error inside `anyhow::Error` when another process writes the index.
#[derive(Debug)]
pub struct Locked {
    pub dir: PathBuf,
    /// The holder's process id, as it recorded it.
    pub pid: Option<u32>,
}

impl std::fmt::Display for Locked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the index in {} is locked by another process",
            self.dir.display()
        )?;
        if let Some(pid) = self.pid {
            write!(f, " (pid {pid})")?;
        }
        f.write_str(": endexd or `endex index` is writing it")
    }
}

impl std::error::Error for Locked {}

impl WriterLock {
    /// Fails with [`Locked`] if another process holds the lock.
    pub fn acquire(config: &Config) -> Result<Self> {
        use std::io::{Read, Seek, Write};
        fs::create_dir_all(config.index_root())?;
        let path = config.lock_path();
        let mut file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .with_context(|| format!("opening {}", path.display()))?;
        match file.try_lock() {
            Ok(()) => {
                file.set_len(0)?;
                file.rewind()?;
                write!(file, "{}", std::process::id())?;
                Ok(Self(file))
            }
            Err(fs::TryLockError::WouldBlock) => {
                let mut pid = String::new();
                let _ = file.read_to_string(&mut pid);
                Err(Locked {
                    dir: config.index_root().to_owned(),
                    pid: pid.trim().parse().ok(),
                }
                .into())
            }
            Err(fs::TryLockError::Error(e)) => {
                Err(anyhow::Error::new(e).context(format!("locking {}", path.display())))
            }
        }
    }
}

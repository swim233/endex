//! Coalescing of inotify events per path until writes settle.

use std::collections::BTreeMap;
use std::ops::Bound;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// Reconcile one file.
    Path(PathBuf),
    /// Rescan a folder and everything below it (created, deleted or moved).
    Tree(PathBuf),
}

struct Pending {
    first: Instant,
    last: Instant,
    tree: bool,
}

/// Collects events per path and releases each path once no event arrived for `settle` (or
/// after `max_wait`). A pending folder rescan absorbs the paths below it.
pub struct Coalescer {
    pending: BTreeMap<PathBuf, Pending>,
    settle: Duration,
    max_wait: Duration,
}

impl Coalescer {
    pub fn new(settle: Duration, max_wait: Duration) -> Self {
        Self {
            pending: BTreeMap::new(),
            settle,
            max_wait,
        }
    }

    pub fn add(&mut self, path: PathBuf, tree: bool, now: Instant) {
        let covering = path
            .ancestors()
            .skip(1)
            .find(|dir| self.pending.get(*dir).is_some_and(|p| p.tree))
            .map(Path::to_owned);
        if let Some(dir) = covering {
            self.pending.get_mut(&dir).expect("found above").last = now;
            return;
        }
        if tree {
            let below: Vec<PathBuf> = self
                .pending
                .range::<Path, _>((Bound::Excluded(path.as_path()), Bound::Unbounded))
                .map(|(p, _)| p)
                .take_while(|p| p.starts_with(&path))
                .cloned()
                .collect();
            for p in below {
                self.pending.remove(&p);
            }
        }
        let entry = self.pending.entry(path).or_insert(Pending {
            first: now,
            last: now,
            tree,
        });
        entry.last = now;
        entry.tree |= tree;
    }

    fn due_at(&self, p: &Pending) -> Instant {
        (p.last + self.settle).min(p.first + self.max_wait)
    }

    /// When the next path settles.
    pub fn deadline(&self) -> Option<Instant> {
        self.pending.values().map(|p| self.due_at(p)).min()
    }

    pub fn take_due(&mut self, now: Instant) -> Vec<Change> {
        let due: Vec<PathBuf> = self
            .pending
            .iter()
            .filter(|(_, p)| self.due_at(p) <= now)
            .map(|(path, _)| path.clone())
            .collect();
        due.into_iter()
            .map(|path| match self.pending.remove(&path) {
                Some(Pending { tree: true, .. }) => Change::Tree(path),
                _ => Change::Path(path),
            })
            .collect()
    }

    pub fn clear(&mut self) {
        self.pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coalesces_until_settled_and_folds_paths_into_trees() {
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let mut c = Coalescer::new(Duration::from_millis(500), Duration::from_secs(5));
        // A save: create, several writes, close.
        for t in [0, 10, 20, 30] {
            c.add("/s/a.md".into(), false, ms(t));
        }
        c.add("/s/sub/b.md".into(), false, ms(100));
        assert!(c.take_due(ms(400)).is_empty());
        assert_eq!(c.take_due(ms(530)), [Change::Path("/s/a.md".into())]);
        // A new folder absorbs the pending path below it and later events below it.
        c.add("/s/sub".into(), true, ms(200));
        c.add("/s/sub/c.md".into(), false, ms(300));
        assert_eq!(c.deadline(), Some(ms(800)));
        assert_eq!(c.take_due(ms(800)), [Change::Tree("/s/sub".into())]);
        // A file written without pause is still released after max_wait.
        for t in (1000..7000).step_by(100) {
            c.add("/s/log.txt".into(), false, ms(t));
        }
        assert_eq!(c.take_due(ms(6000)), [Change::Path("/s/log.txt".into())]);
        assert!(c.deadline().is_none());
    }
}

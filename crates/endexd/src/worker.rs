//! The search thread: owns the searcher and its warm CPU embedder, runs one job at a time.

use std::io::{self, Write as _};
use std::panic::{self, AssertUnwindSafe};
use std::process;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use endex_core::Kind;
use endex_core::catalog::Catalog;
use endex_core::config::Config;
use endex_core::embed;
use endex_core::search::{ChunkView, Limits, Mode, Results, Searcher};
use endex_core::store::TableUnavailable;
use tokio::runtime::Handle;
use tokio::sync::oneshot;

pub type CancelCheck = Box<dyn Fn() -> bool + Send>;

/// A table that stays unloadable this long is reported as an error rather than a rebuild.
const REBUILD_GRACE: Duration = Duration::from_secs(60);

enum Job {
    Search {
        query: String,
        mode: Mode,
        limits: Limits,
        cancelled: CancelCheck,
        reply: oneshot::Sender<Result<Results>>,
    },
    Preview {
        id: String,
        query: String,
        reply: oneshot::Sender<Result<Option<Preview>>>,
    },
    Status {
        reply: oneshot::Sender<Status>,
    },
    Configure(Box<Config>),
}

/// A chunk and the analyzed query, for highlighting.
pub struct Preview {
    pub view: ChunkView,
    pub terms: Vec<String>,
}

pub struct Status {
    /// `None` before the first index run.
    pub index: Option<Result<Counts>>,
    /// The table exists but has not loaded for a short while: `endex index --rebuild` is most
    /// likely replacing it.
    pub rebuilding: bool,
    pub model: String,
}

#[derive(Default)]
pub struct Counts {
    pub doc_files: usize,
    pub image_files: usize,
    pub doc_chunks: usize,
    pub image_chunks: usize,
    pub errors: usize,
    pub last_indexed: Option<i64>,
}

/// The embedder is not `Send`, so the searcher lives on one thread for its whole life and
/// drives LanceDB's futures on the daemon's runtime.
pub struct Worker {
    jobs: mpsc::Sender<Job>,
}

impl Worker {
    /// Loads the query embedder and returns once it has embedded a first query.
    pub async fn start(config: Config, runtime: Handle) -> Result<Self> {
        let (jobs, inbox) = mpsc::channel();
        let (ready, started) = oneshot::channel();
        thread::Builder::new()
            .name("search".into())
            .spawn(move || {
                let mut searcher = Searcher::new(&config);
                let warm = searcher.warm_up();
                let ok = warm.is_ok();
                let _ = ready.send(warm);
                if ok {
                    let served = panic::catch_unwind(AssertUnwindSafe(|| {
                        run(config, &mut searcher, &runtime, inbox)
                    }));
                    if served.is_err() {
                        // Nothing can be answered without this thread: exit, so that systemd
                        // restarts the service instead of keeping a daemon that only fails.
                        let _ =
                            writeln!(io::stderr(), "endexd: the search thread panicked; exiting");
                        process::exit(1);
                    }
                }
            })?;
        started.await.context("search thread exited")??;
        Ok(Self { jobs })
    }

    pub async fn search(
        &self,
        query: String,
        mode: Mode,
        limits: Limits,
        cancelled: CancelCheck,
    ) -> Result<Results> {
        let (reply, answer) = oneshot::channel();
        self.send(Job::Search {
            query,
            mode,
            limits,
            cancelled,
            reply,
        })?;
        answer.await.context("search thread exited")?
    }

    pub async fn preview(&self, id: String, query: String) -> Result<Option<Preview>> {
        let (reply, answer) = oneshot::channel();
        self.send(Job::Preview { id, query, reply })?;
        answer.await.context("search thread exited")?
    }

    pub async fn status(&self) -> Result<Status> {
        let (reply, answer) = oneshot::channel();
        self.send(Job::Status { reply })?;
        answer.await.context("search thread exited")
    }

    pub fn configure(&self, config: Config) {
        let _ = self.send(Job::Configure(Box::new(config)));
    }

    fn send(&self, job: Job) -> Result<()> {
        self.jobs.send(job).ok().context("search thread exited")
    }
}

fn run(mut config: Config, searcher: &mut Searcher, runtime: &Handle, inbox: mpsc::Receiver<Job>) {
    let mut unavailable_since: Option<Instant> = None;
    for job in inbox {
        match job {
            Job::Search {
                query,
                mode,
                limits,
                cancelled,
                reply,
            } => {
                let results =
                    runtime.block_on(searcher.search_until(&query, mode, limits, &cancelled));
                let _ = reply.send(results);
            }
            Job::Preview { id, query, reply } => {
                let preview = runtime.block_on(searcher.chunk(&id)).map(|view| {
                    view.map(|view| Preview {
                        view,
                        terms: searcher.terms(&query),
                    })
                });
                let _ = reply.send(preview);
            }
            Job::Configure(new) => {
                searcher.set_config(&new);
                config = *new;
            }
            Job::Status { reply } => {
                let index = runtime.block_on(counts(&config, searcher)).transpose();
                let unavailable = matches!(&index, Some(Err(e)) if e.is::<TableUnavailable>());
                if !unavailable {
                    unavailable_since = None;
                }
                let rebuilding = unavailable
                    && unavailable_since.get_or_insert_with(Instant::now).elapsed() < REBUILD_GRACE;
                let _ = reply.send(Status {
                    index,
                    rebuilding,
                    model: embed::model_identity(&config).unwrap_or_default(),
                });
            }
        }
    }
}

async fn counts(config: &Config, searcher: &mut Searcher) -> Result<Option<Counts>> {
    let path = config.catalog_path();
    if !path.exists() {
        return Ok(None);
    }
    let catalog = Catalog::open_read_only(&path)?;
    let files = catalog.counts()?;
    let files_of = |kind| {
        files
            .ok
            .iter()
            .find(|(k, _)| *k == kind)
            .map_or(0, |(_, n)| *n)
    };
    Ok(Some(Counts {
        doc_files: files_of(Kind::Doc),
        image_files: files_of(Kind::Image),
        doc_chunks: searcher.count(Kind::Doc).await?,
        image_chunks: searcher.count(Kind::Image).await?,
        errors: files.errors,
        last_indexed: catalog.last_indexed()?,
    }))
}

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::{self, IsTerminal, Write as _};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use endex_core::catalog::Catalog;
use endex_core::config::{Config, expand_home};
use endex_core::embed::{self, Device};
use endex_core::index::{self, Event, Stage};
use endex_core::search::{Hit, Mode, Searcher};
use endex_core::store::Store;
use endex_core::{Kind, eval, lock, sample};
use zbus::zvariant::{OwnedValue, Value};

mod eval_sets;

#[derive(Parser)]
#[command(
    name = "endex",
    version,
    about = "Local search for documents and images"
)]
struct Cli {
    /// Config file [default: ~/.config/endex/config.toml, created on first run]
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    /// Keep the index and catalog in this directory instead of the configured data_dir
    /// (OCR models and caches stay shared)
    #[arg(long, global = true, env = "ENDEX_DATA_DIR")]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Write test-set manifests (paths only) to <data_dir>/testset.
    Sample {
        /// Overrides `sample.seed` from the config.
        #[arg(long)]
        seed: Option<u64>,
        #[arg(long, default_value = "~/Pictures")]
        pictures: PathBuf,
        #[arg(long, default_value = "~/Documents")]
        documents: PathBuf,
    },
    /// Index the files listed in manifests (one absolute path per line).
    Index {
        #[arg(long = "manifest", required = true, num_args = 1..)]
        manifests: Vec<PathBuf>,
        /// Embedding device for indexing; falls back to cpu if the GPU is unavailable.
        #[arg(long, default_value = "gpu")]
        device: Device,
        /// Drop the existing index and catalog first (needed after changing model or prefixes).
        #[arg(long)]
        rebuild: bool,
    },
    /// Search the index; documents and images are listed separately.
    Search {
        query: String,
        #[arg(long, default_value = "hybrid")]
        mode: Mode,
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// Ask the running daemon (endexd), which searches its own index.
        #[arg(long)]
        daemon: bool,
    },
    /// Report Recall@k and MRR of every mode for a labelled query file, or for the public sets.
    /// Never writes into the evaluated index.
    Eval {
        #[arg(required_unless_present = "sets")]
        queries: Option<PathBuf>,
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// Evaluate every public set prepared by `just eval` under <data_dir>/eval/ and save
        /// the results to <data_dir>/eval-results/last.json.
        #[arg(long, conflicts_with = "queries")]
        sets: bool,
        /// With --sets: show the saved results of the last run instead of running.
        #[arg(long, requires = "sets")]
        saved: bool,
        /// With --sets: print the results as JSON (`null` if --saved finds none).
        #[arg(long, requires = "sets")]
        json: bool,
    },
    /// Show index counts, model identity and locations.
    Status,
    /// Print the resolved configuration, or one value of it (e.g. `endex config data_dir`).
    Config { key: Option<String> },
    /// Show or hide the search popup; D-Bus starts endex-ui if it is not running.
    Toggle,
    /// Open the settings window, optionally on one page.
    Settings {
        #[arg(value_enum)]
        page: Option<SettingsPage>,
    },
}

/// The settings window's pages, in its order.
#[derive(Clone, Copy, ValueEnum)]
enum SettingsPage {
    Dirs,
    Exclude,
    Watch,
    Model,
    Status,
    Shortcut,
    Resource,
    Eval,
    Appearance,
}

#[tokio::main]
async fn main() -> ExitCode {
    let result = match run(Cli::parse()).await {
        // Output is written once at the end, so a closed pipe (`| head`) cannot interrupt work.
        Ok(output) => io::stdout().lock().write_all(output.as_bytes()),
        Err(error) => {
            let _ = writeln!(io::stderr(), "Error: {error:#}");
            return ExitCode::FAILURE;
        }
    };
    match result {
        Err(error) if error.kind() != io::ErrorKind::BrokenPipe => {
            let _ = writeln!(io::stderr(), "Error: {error}");
            ExitCode::FAILURE
        }
        _ => ExitCode::SUCCESS,
    }
}

async fn run(cli: Cli) -> Result<String> {
    match cli.command {
        Command::Toggle => return call_ui("Toggle", &()).await,
        Command::Settings { page } => {
            let page = page.map(|p| p.to_possible_value().expect("no skipped pages"));
            let page = page.as_ref().map_or("", |p| p.get_name());
            // Started from the app menu, the launcher's token lets the window come to the front.
            return match std::env::var("XDG_ACTIVATION_TOKEN") {
                Ok(token) if !token.is_empty() => call_ui("ActivateSettings", &(page, token)).await,
                _ => call_ui("ShowSettings", &(page,)).await,
            };
        }
        Command::Search {
            query,
            mode,
            limit,
            daemon: true,
        } => {
            if cli.config.is_some() || cli.data_dir.is_some() {
                let _ = writeln!(
                    io::stderr(),
                    "warning: --config and --data-dir (ENDEX_DATA_DIR) are ignored with --daemon; \
                     the daemon searches its own index"
                );
            }
            return cmd_search_daemon(&query, mode, limit).await;
        }
        _ => {}
    }
    let config = match &cli.config {
        Some(path) => Config::from_file(path)?,
        None => Config::load()?,
    }
    .with_index_root(cli.data_dir);
    match cli.command {
        Command::Sample {
            seed,
            pictures,
            documents,
        } => cmd_sample(&config, seed, pictures, documents),
        Command::Index {
            manifests,
            device,
            rebuild,
        } => cmd_index(&config, manifests, device, rebuild).await,
        Command::Search {
            query, mode, limit, ..
        } => cmd_search(&config, &query, mode, limit).await,
        Command::Eval {
            sets: true,
            limit,
            saved,
            json,
            ..
        } => cmd_eval_sets(&config, limit, saved, json).await,
        Command::Eval { queries, limit, .. } => {
            cmd_eval(&config, &queries.expect("required"), limit).await
        }
        Command::Status => cmd_status(&config).await,
        Command::Config { key } => cmd_config(&config, key.as_deref()),
        Command::Toggle | Command::Settings { .. } => unreachable!("handled above"),
    }
}

/// Progress and diagnostics: best effort, never fatal (stderr may be a closed pipe).
macro_rules! note {
    ($($arg:tt)*) => {{
        let _ = write!(io::stderr(), $($arg)*);
    }};
}

fn cmd_sample(
    config: &Config,
    seed: Option<u64>,
    pictures: PathBuf,
    documents: PathBuf,
) -> Result<String> {
    let report = sample::run(&sample::Spec {
        pictures: expand_home(&pictures),
        documents: expand_home(&documents),
        out_dir: config.testset_dir(),
        seed: seed.unwrap_or(config.sample.seed),
    })?;
    let mut out = String::new();
    writeln!(
        out,
        "images: {} screenshots over {} months + {} other images from {} folders -> {}",
        report.screenshots,
        report.screenshot_months,
        report.other_images,
        report.other_groups,
        report.images_manifest.display()
    )?;
    writeln!(
        out,
        "texts:  {} markdown + {} docx -> {}",
        report.markdown,
        report.docx,
        report.texts_manifest.display()
    )?;
    writeln!(
        out,
        "Review both lists and remove anything sensitive before indexing them."
    )?;
    Ok(out)
}

async fn cmd_index(
    config: &Config,
    manifests: Vec<PathBuf>,
    device: Device,
    rebuild: bool,
) -> Result<String> {
    let tty = io::stderr().is_terminal();
    let mut on_event = |event: Event| match event {
        Event::GpuFallback(error) => {
            note!("GPU unavailable ({error:#}); embedding on CPU instead\n")
        }
        Event::Loaded { what, took } => note!("loaded {what} in {}\n", secs(took)),
        Event::Failed { path, error } => note!("failed: {path}: {error:#}\n"),
        Event::Progress { done, total } if tty => {
            note!(
                "\r[{done}/{total}]{}",
                if done == total { "\n" } else { "" }
            )
        }
        Event::Progress { .. } => {}
        Event::Skipped {
            manifest,
            line,
            reason,
        } => note!(
            "skipped {}:{line}: {reason}; nothing is removed from the index in this run\n",
            manifest.display()
        ),
    };
    // The data dir's own index belongs to endexd, also while it is not running: its next
    // start would drop rows of no configured folder.
    if config.index_root.is_none() {
        bail!(
            "endex index 不写入 {} 里的索引：那是 endexd 的索引，由它按设置窗口里的目录维护。\
             要用命令行建索引，请用 --data-dir 指定另一个目录。",
            config.data_dir.display()
        );
    }
    let _lock = lock::WriterLock::acquire(config).map_err(|error| match error.downcast::<lock::Locked>() {
        Ok(locked) => anyhow::anyhow!(
            "索引目录 {} 正被另一个进程写入（{}通常是 endexd）。endexd 运行时只有它能写入自己的索引：\
             请在设置窗口里管理索引目录或重建索引；要用命令行建索引，请用 --data-dir 指定另一个目录。",
            locked.dir.display(),
            locked.pid.map_or(String::new(), |pid| format!("进程号 {pid}，"))
        ),
        Err(error) => error,
    })?;
    let options = index::Options {
        manifests,
        device,
        rebuild,
    };
    let r = index::run(config, &options, &mut on_event).await?;
    let mut out = String::new();
    writeln!(
        out,
        "files: {} new, {} modified, {} touched, {} unchanged, {} removed, {} failed, {} unsupported",
        r.new, r.modified, r.touched, r.unchanged, r.removed, r.failed, r.unsupported
    )?;
    if r.skipped > 0 {
        writeln!(
            out,
            "{} manifest lines skipped (see the warnings above); nothing removed this run",
            r.skipped
        )?;
    }
    if let Some(device) = r.device {
        writeln!(out, "embedding device: {device}")?;
    }
    writeln!(
        out,
        "{:<12} {:>9} {:>8} {:>10}",
        "stage", "time", "items", "per item"
    )?;
    writeln!(out, "{:<12} {:>9}", "model load", secs(r.load))?;
    for (name, stage, unit) in [
        ("parse", &r.parse, "chunks"),
        ("ocr", &r.ocr, "images"),
        ("embed text", &r.embed_text, "chunks"),
        ("embed image", &r.embed_image, "images"),
        ("write", &r.write, "rows"),
    ] {
        write_stage(&mut out, name, stage, unit)?;
    }
    writeln!(out, "{:<12} {:>9}", "total", secs(r.total))?;
    Ok(out)
}

fn write_stage(out: &mut String, name: &str, stage: &Stage, unit: &str) -> std::fmt::Result {
    let per_item = if stage.items > 0 && unit != "rows" {
        format!(
            "{:.1} ms",
            stage.time.as_secs_f64() * 1000.0 / stage.items as f64
        )
    } else {
        String::new()
    };
    writeln!(
        out,
        "{name:<12} {:>9} {:>8} {per_item:>10}",
        secs(stage.time),
        format!("{} {unit}", stage.items)
    )
}

async fn cmd_search(config: &Config, query: &str, mode: Mode, limit: usize) -> Result<String> {
    let mut searcher = Searcher::new(config);
    let results = searcher.search(query, mode, limit).await?;
    let mut out = String::new();
    write_group(&mut out, "Documents", &results.docs)?;
    write_group(&mut out, "Images", &results.images)?;
    Ok(out)
}

async fn cmd_search_daemon(query: &str, mode: Mode, limit: usize) -> Result<String> {
    type Dict = HashMap<String, OwnedValue>;
    let bus = zbus::Connection::session().await?;
    let limit = Value::from(u32::try_from(limit)?);
    let opts = HashMap::from([("limit_docs", &limit), ("limit_images", &limit)]);
    let reply = bus
        .call_method(
            Some("org.endex.Daemon1"),
            "/org/endex/Daemon",
            Some("org.endex.Daemon1"),
            "Search",
            &(query, mode.as_str(), opts),
        )
        .await?;
    let (_, docs, images): (u32, Vec<Dict>, Vec<Dict>) = reply.body().deserialize()?;
    let hits = |dicts: Vec<Dict>| -> Result<Vec<Hit>> {
        dicts
            .iter()
            .map(|d| {
                let string = |key| -> Result<String> {
                    Ok(d.get(key).context(key)?.downcast_ref::<&str>()?.to_owned())
                };
                Ok(Hit {
                    id: string("id")?,
                    path: string("path")?,
                    kind: string("kind")?.parse()?,
                    title: string("title")?,
                    location: string("location")?,
                    snippet: string("snippet")?,
                    score: d.get("score").context("score")?.downcast_ref::<f64>()? as f32,
                    vector_score: None,
                    keyword: None,
                    mtime: 0,
                })
            })
            .collect()
    };
    let mut out = String::new();
    write_group(&mut out, "Documents", &hits(docs)?)?;
    write_group(&mut out, "Images", &hits(images)?)?;
    Ok(out)
}

fn write_group(out: &mut String, label: &str, hits: &[Hit]) -> std::fmt::Result {
    writeln!(out, "{label} ({})", hits.len())?;
    for (i, hit) in hits.iter().enumerate() {
        let location = if hit.location.is_empty() {
            String::new()
        } else {
            format!(" — {}", hit.location)
        };
        writeln!(
            out,
            "{:>3}. {:.4}  {}{location}",
            i + 1,
            hit.score,
            hit.title
        )?;
        writeln!(out, "     {}", hit.path)?;
        if !hit.snippet.is_empty() {
            writeln!(out, "     {}", hit.snippet)?;
        }
    }
    Ok(())
}

async fn cmd_eval(config: &Config, queries: &std::path::Path, limit: usize) -> Result<String> {
    let queries = eval::load(queries)?;
    let mut searcher = Searcher::untouched(config);
    let reports = eval::run(&mut searcher, &queries, limit).await?;
    let kinds: Vec<String> = reports
        .first()
        .map(|r| r.by_kind.keys().cloned().collect())
        .unwrap_or_default();
    let mut out = String::new();
    write!(
        out,
        "{:<8} {:>10} {:>6}",
        "mode",
        format!("recall@{limit}"),
        "MRR"
    )?;
    for kind in &kinds {
        write!(out, "  {:>16}", format!("{kind} R/MRR"))?;
    }
    writeln!(out)?;
    for r in &reports {
        write!(
            out,
            "{:<8} {:>10.3} {:>6.3}",
            r.mode,
            r.overall.recall(),
            r.overall.mrr()
        )?;
        for kind in &kinds {
            let m = r.by_kind[kind];
            write!(
                out,
                "  {:>16}",
                format!("{:.3}/{:.3} ({})", m.recall(), m.mrr(), m.queries)
            )?;
        }
        writeln!(out)?;
    }
    for r in &reports {
        if r.misses.is_empty() {
            continue;
        }
        writeln!(out, "\nmisses ({}):", r.mode)?;
        for miss in &r.misses {
            let best = miss.best_rank.map_or(String::new(), |rank| {
                format!(" (another expected hit at rank {rank})")
            });
            writeln!(out, "  {:?}{best}", miss.query)?;
            for path in &miss.missing {
                writeln!(out, "    - {path}")?;
            }
        }
    }
    writeln!(
        out,
        "\n{} queries; ranks are counted within the matching group (documents or images).",
        queries.len()
    )?;
    Ok(out)
}

async fn cmd_eval_sets(config: &Config, limit: usize, saved: bool, json: bool) -> Result<String> {
    let results = if saved {
        eval_sets::load(config)?
    } else {
        let results = eval_sets::run(config, limit).await?;
        let path = eval_sets::save(config, &results)?;
        note!("saved to {}\n", path.display());
        Some(results)
    };
    match (results, json) {
        (results, true) => Ok(serde_json::to_string(&results)? + "\n"),
        (Some(results), false) => eval_sets::table(&results),
        (None, false) => Ok(format!(
            "No saved results yet ({}).\n",
            eval_sets::results_path(config).display()
        )),
    }
}

async fn cmd_status(config: &Config) -> Result<String> {
    let mut out = String::new();
    writeln!(out, "data dir      {}", config.data_dir.display())?;
    writeln!(out, "index         {}", config.index_root().display())?;
    writeln!(out, "model         {}", embed::model_identity(config)?)?;
    writeln!(
        out,
        "prefix style  {} (config)",
        config.embedding.prefix_style.as_str()
    )?;
    if !config.catalog_path().exists() {
        writeln!(
            out,
            "              empty (run `endex index --manifest <file>`)"
        )?;
        return Ok(out);
    }
    // Read-only: endexd may be writing this index right now.
    let catalog = Catalog::open_read_only(&config.catalog_path())?;
    if let (Some(model), Some(prefix)) = (catalog.meta("model")?, catalog.meta("prefix_style")?) {
        writeln!(out, "index built   {model}, prefix style {prefix}")?;
    }
    let pipeline = catalog.text_pipeline()?;
    let note = if pipeline == index::TEXT_PIPELINE {
        String::new()
    } else {
        format!(
            " (current {}: endexd processes its documents again, `endex index` with --rebuild)",
            index::TEXT_PIPELINE
        )
    };
    writeln!(out, "text pipeline {pipeline}{note}")?;
    let counts = catalog.counts()?;
    let files: Vec<String> = counts.ok.iter().map(|(k, n)| format!("{n} {k}")).collect();
    let files = if files.is_empty() {
        "0".to_owned()
    } else {
        files.join(", ")
    };
    writeln!(
        out,
        "files         {files} ok, {} with errors",
        counts.errors
    )?;
    if let Some(store) = Store::open_existing(&config.index_dir(), true).await? {
        writeln!(
            out,
            "rows          {} document chunks, {} images",
            store.count(Kind::Doc).await?,
            store.count(Kind::Image).await?
        )?;
    }
    Ok(out)
}

fn cmd_config(config: &Config, key: Option<&str>) -> Result<String> {
    let e = &config.embedding;
    let values = [
        ("data_dir", config.data_dir.display().to_string()),
        ("index_root", config.index_root().display().to_string()),
        ("ocr_models", config.ocr_model_dir().display().to_string()),
        ("embedding.library", e.library.display().to_string()),
        ("embedding.model", e.model.display().to_string()),
        ("embedding.tokenizer", e.tokenizer.display().to_string()),
        ("embedding.prefix_style", e.prefix_style.as_str().to_owned()),
        (
            "index.max_document_mb",
            config.index.max_document_mb.to_string(),
        ),
        ("sample.seed", config.sample.seed.to_string()),
    ];
    let mut out = String::new();
    match key {
        Some(key) => match values.iter().find(|(k, _)| *k == key) {
            Some((_, value)) => writeln!(out, "{value}")?,
            None => bail!("unknown key {key:?}"),
        },
        None => {
            for (k, value) in &values {
                writeln!(out, "{k} = {value}")?;
            }
        }
    }
    Ok(out)
}

/// A method of the popup's `org.endex.Ui1`; D-Bus starts endex-ui if it is not running.
async fn call_ui<B>(method: &str, body: &B) -> Result<String>
where
    B: serde::Serialize + zbus::zvariant::DynamicType,
{
    zbus::Connection::session()
        .await?
        .call_method(
            Some("org.endex.Ui1"),
            "/org/endex/Ui",
            Some("org.endex.Ui1"),
            method,
            body,
        )
        .await?;
    Ok(String::new())
}

fn secs(d: Duration) -> String {
    format!("{:.2}s", d.as_secs_f64())
}

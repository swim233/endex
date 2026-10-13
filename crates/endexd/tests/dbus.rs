//! endexd over a private session bus: read-only on the synthetic smoke-fixture index, and as
//! the writer keeping a temporary folder indexed. Run with `just dbus-test` (needs the index
//! from `just smoke`). Every daemon gets its own config and data_dir, under target/tmp or
//! `$ENDEX_TEST_DIR`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use endex_core::config::default_toml;
use endex_core::config::edit::{self, ConfigDoc};
use endex_core::config::{Source, SourceKind};
use zbus::zvariant::{OwnedValue, Value};

type Dict = HashMap<String, OwnedValue>;

const NAME: &str = "org.endex.Daemon1";
const PATH: &str = "/org/endex/Daemon";
const QUERY: &str = "怎么做红烧肉";

struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A fresh folder for this test: in `$ENDEX_TEST_DIR` if set, else in target/tmp.
fn test_dir(name: &str) -> Result<PathBuf> {
    let base = std::env::var_os("ENDEX_TEST_DIR")
        .map_or_else(|| PathBuf::from(env!("CARGO_TARGET_TMPDIR")), PathBuf::from);
    let dir = base.join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// A config of its own, with `data_dir` next to it.
fn temp_config(name: &str, sources: &[Source]) -> Result<(PathBuf, PathBuf)> {
    let dir = test_dir(name)?;
    let mut doc = ConfigDoc::parse(&default_toml())?;
    doc.set(
        "data_dir",
        dir.join("data").to_string_lossy().as_ref().into(),
    )?;
    doc.set("index.sources", edit::sources(sources))?;
    let config = dir.join("config.toml");
    fs::write(&config, doc.to_string())?;
    Ok((dir, config))
}

async fn start(config: &Path, data_dir: Option<&Path>) -> Result<(Daemon, zbus::Proxy<'static>)> {
    ensure!(
        std::env::var_os("ENDEX_PRIVATE_BUS").is_some(),
        "run under dbus-run-session via `just dbus-test`, not on the desktop's bus"
    );
    let mut command = Command::new(env!("CARGO_BIN_EXE_endexd"));
    command
        .arg("--config")
        .arg(config)
        .env_remove("ENDEX_DATA_DIR")
        .env("ENDEX_DEBUG_BATTERY", "ac");
    if let Some(dir) = data_dir {
        command.arg("--data-dir").arg(dir);
    }
    let daemon = Daemon(command.spawn()?);
    let bus = zbus::Connection::session().await?;
    let dbus = zbus::fdo::DBusProxy::new(&bus).await?;
    let started = Instant::now();
    while !dbus.name_has_owner(NAME.try_into()?).await? {
        ensure!(
            started.elapsed() < Duration::from_secs(60),
            "endexd did not start"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let proxy = zbus::Proxy::new(&bus, NAME, PATH, NAME).await?;
    let version: u32 = proxy.get_property("Version").await?;
    assert_eq!(version, 2);
    Ok((daemon, proxy))
}

#[tokio::test]
#[ignore = "needs the smoke-fixture index and a private bus: just dbus-test"]
async fn read_only_search_preview_and_status() -> Result<()> {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/generated")
        .canonicalize()
        .context("no smoke fixtures: run `just smoke`")?;
    let (_, config) = temp_config("dbus-read-only", &[])?;
    let (_daemon, proxy) = start(&config, Some(&fixtures.join("db"))).await?;

    let status: Dict = proxy.call("Status", &()).await?;
    assert_eq!(string(&status, "state")?, "ready");
    assert_eq!(string(&status, "mode")?, "read_only");
    assert_eq!(number(&status, "doc_files")?, 14);
    assert_eq!(number(&status, "image_files")?, 18);
    assert!(number(&status, "doc_chunks")? >= 14);
    assert_eq!(string(&status, "query_device")?, "cpu");
    let error = proxy
        .call::<_, _, ()>("Rescan", &())
        .await
        .expect_err("read-only");
    assert!(error.to_string().contains("ReadOnly"), "{error}");

    let recipe = fixtures.join("docs/recipes/hongshaorou.md");
    let screenshot = fixtures.join("images/屏幕截图_20260222_123000_recipe.png");
    for (i, mode) in ["hybrid", "vector", "keyword"].into_iter().enumerate() {
        let id = Value::from(i as u32 + 1);
        let limit = Value::from(5u32);
        let opts = HashMap::from([("query_id", &id), ("limit_docs", &limit)]);
        let (query_id, docs, images): (u32, Vec<Dict>, Vec<Dict>) =
            proxy.call("Search", &(QUERY, mode, opts)).await?;
        assert_eq!(query_id, i as u32 + 1);
        assert!(
            !docs.is_empty() && docs.len() <= 5,
            "{mode}: {} docs",
            docs.len()
        );
        for hit in docs.iter().chain(&images) {
            for key in ["id", "kind", "path", "title", "location", "snippet"] {
                string(hit, key)?;
            }
            hit["mtime"].downcast_ref::<i64>()?;
            hit["size"].downcast_ref::<u64>()?;
            hit["score"].downcast_ref::<f64>()?;
            spans(hit)?;
        }
        assert_eq!(string(&docs[0], "path")?, recipe.to_string_lossy());
        assert_eq!(string(&images[0], "path")?, screenshot.to_string_lossy());
        let top = &docs[0];
        match mode {
            "vector" => assert!(top.contains_key("vec_score") && !top.contains_key("kw_rank")),
            "keyword" => assert!(!top.contains_key("vec_score") && number(top, "kw_rank")? == 1),
            _ => assert!(top.contains_key("vec_score") && number(top, "kw_rank")? == 1),
        }
        assert!(images[0]["ocr"].downcast_ref::<bool>()?);
        assert_eq!(number(&images[0], "width")?, 2560);
    }

    let (_, docs, images): (u32, Vec<Dict>, Vec<Dict>) = proxy
        .call("Search", &(QUERY, "hybrid", HashMap::<&str, Value>::new()))
        .await?;
    let doc: Dict = proxy
        .call("Preview", &(string(&docs[0], "id")?, QUERY))
        .await?;
    assert!(string(&doc, "text")?.contains("五花肉"));
    assert!(!spans(&doc)?.is_empty());
    assert!(number(&doc, "index")? < number(&doc, "count")?);
    string(&doc, "next")?;
    let image: Dict = proxy
        .call("Preview", &(string(&images[0], "id")?, QUERY))
        .await?;
    assert!(string(&image, "text")?.contains("红烧肉"));
    assert_eq!(number(&image, "height")?, 1600);

    let error = proxy
        .call::<_, _, Dict>("Preview", &("/no/such/file#0", QUERY))
        .await
        .expect_err("unknown id");
    assert!(error.to_string().contains("NotFound"), "{error}");
    Ok(())
}

#[tokio::test]
#[ignore = "needs the embedding model and a private bus: just dbus-test"]
async fn writer_keeps_a_folder_indexed() -> Result<()> {
    let dir = test_dir("dbus-writer-src")?;
    fs::create_dir_all(dir.join("notes"))?;
    let (notes, todo) = (dir.join("notes/redis.md"), dir.join("todo.txt"));
    fs::write(&notes, "# Redis\n\n缓存过期策略和内存淘汰的配置笔记。\n")?;
    fs::write(&todo, "Buy oat milk and renew the library card.\n")?;
    let source = Source {
        path: dir.clone(),
        kind: SourceKind::All,
    };
    let (_, config) = temp_config("dbus-writer", &[source])?;
    let (_daemon, proxy) = start(&config, None).await?;

    // Initial index, then search.
    let state = wait_state(&proxy, |s| s == "idle").await?;
    let status: Dict = proxy.call("Status", &()).await?;
    assert_eq!(string(&status, "mode")?, "writer");
    assert_eq!(number(&status, "doc_files")?, 2, "{state:?}");
    assert_eq!(
        top(&proxy, "Redis 内存淘汰", "hybrid").await?,
        Some(notes.clone())
    );

    // An edit becomes searchable through the watch; a deleted file disappears.
    fs::write(&notes, "# Redis\n\n缓存过期策略。新增：zebrafinch 实验。\n")?;
    wait_for(|| async { Ok(top(&proxy, "zebrafinch", "keyword").await? == Some(notes.clone())) })
        .await?;
    fs::remove_file(&todo)?;
    wait_for(|| async {
        Ok(top(&proxy, "oat milk library card", "keyword")
            .await?
            .is_none())
    })
    .await?;

    // Settings round trip: applied to the file with its comments, then in effect.
    let rules = Value::from(vec!["notes/"]);
    let (font_size, accent) = (Value::from(10.5), Value::from("#8E44AD"));
    let reply: Dict = proxy
        .call(
            "ApplySettings",
            &(HashMap::from([
                ("exclude.rules", &rules),
                ("appearance.font_size", &font_size),
                ("appearance.accent", &accent),
            ]),),
        )
        .await?;
    assert!(reply["applied"].downcast_ref::<bool>()?);
    let text = fs::read_to_string(&config)?;
    assert!(text.contains("# Extra rules in .gitignore syntax") && text.contains("\"notes/\""));
    assert!(text.contains("[appearance]") && text.contains("font_size = 10.5"));
    let settings: Dict = proxy.call("GetSettings", &()).await?;
    assert_eq!(
        Vec::<String>::try_from(settings["exclude.rules"].try_clone()?)?,
        ["notes/"]
    );
    assert_eq!(
        settings["appearance.font_size"].downcast_ref::<f64>()?,
        10.5
    );
    assert_eq!(string(&settings, "appearance.accent")?, "#8e44ad");
    wait_for(|| async { Ok(top(&proxy, "zebrafinch", "keyword").await?.is_none()) }).await?;
    let (bad, bad_accent) = (Value::from(0u32), Value::from("blue"));
    let reply: Dict = proxy
        .call(
            "ApplySettings",
            &(HashMap::from([
                ("resources.threads.mid", &bad),
                ("appearance.accent", &bad_accent),
            ]),),
        )
        .await?;
    assert!(!reply["applied"].downcast_ref::<bool>()?);
    let errors = HashMap::<String, String>::try_from(reply["errors"].try_clone()?)?;
    assert!(errors.contains_key("resources.threads.mid"), "{errors:?}");
    assert!(errors.contains_key("appearance.accent"), "{errors:?}");

    // Pause, then resume.
    proxy.call::<_, _, ()>("Pause", &()).await?;
    let state = wait_state(&proxy, |s| s == "paused").await?;
    assert_eq!(string(&state, "reason")?, "user");
    proxy.call::<_, _, ()>("Resume", &()).await?;
    wait_state(&proxy, |s| s == "idle").await?;
    Ok(())
}

/// The top document for `query`, if any.
async fn top(proxy: &zbus::Proxy<'_>, query: &str, mode: &str) -> Result<Option<PathBuf>> {
    let (_, docs, _): (u32, Vec<Dict>, Vec<Dict>) = proxy
        .call("Search", &(query, mode, HashMap::<&str, Value>::new()))
        .await?;
    docs.first()
        .map(|d| string(d, "path").map(PathBuf::from))
        .transpose()
}

async fn wait_state(proxy: &zbus::Proxy<'_>, wanted: impl Fn(&str) -> bool) -> Result<Dict> {
    let started = Instant::now();
    loop {
        let state: Dict = proxy.get_property("IndexState").await?;
        if wanted(&string(&state, "state")?) {
            return Ok(state);
        }
        if started.elapsed() > Duration::from_secs(60) {
            bail!("IndexState stayed {state:?}");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn wait_for<F: Future<Output = Result<bool>>>(check: impl Fn() -> F) -> Result<()> {
    let started = Instant::now();
    while !check().await? {
        ensure!(
            started.elapsed() < Duration::from_secs(30),
            "condition not met within 30 s"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Ok(())
}

fn string(d: &Dict, key: &str) -> Result<String> {
    Ok(d.get(key)
        .with_context(|| format!("missing {key}"))?
        .downcast_ref::<&str>()?
        .to_owned())
}

fn spans(d: &Dict) -> Result<Vec<(u32, u32)>> {
    let value = d.get("highlights").context("missing highlights")?;
    Ok(Vec::try_from(value.try_clone()?)?)
}

fn number(d: &Dict, key: &str) -> Result<u32> {
    Ok(d.get(key)
        .with_context(|| format!("missing {key}"))?
        .downcast_ref::<u32>()?)
}

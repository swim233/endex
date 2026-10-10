//! endexd on the synthetic smoke-fixture index, called over a private session bus.
//! Run with `just dbus-test` (needs the index from `just smoke`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, ensure};
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

#[tokio::test]
#[ignore = "needs the smoke-fixture index and a private bus: just dbus-test"]
async fn search_preview_and_status() -> Result<()> {
    ensure!(
        std::env::var_os("ENDEX_PRIVATE_BUS").is_some(),
        "run under dbus-run-session via `just dbus-test`, not on the desktop's bus"
    );
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/generated")
        .canonicalize()
        .context("no smoke fixtures: run `just smoke`")?;
    let _daemon = Daemon(
        Command::new(env!("CARGO_BIN_EXE_endexd"))
            .env("ENDEX_DATA_DIR", fixtures.join("db"))
            .spawn()?,
    );
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
    assert_eq!(version, 1);

    let status: Dict = proxy.call("Status", &()).await?;
    assert_eq!(string(&status, "state")?, "ready");
    assert_eq!(number(&status, "doc_files")?, 14);
    assert_eq!(number(&status, "image_files")?, 18);
    assert!(number(&status, "doc_chunks")? >= 14);
    assert_eq!(string(&status, "query_device")?, "cpu");

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

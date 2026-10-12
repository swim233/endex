# Endex

Local search over documents and images (including text inside screenshots) for one
Linux desktop. One query, Chinese or English. M1: Rust core + CLI. M2: a search daemon
(`endexd`, D-Bus) and a popup (`endex-ui`) for KDE Plasma 6 on Wayland. M3: endexd keeps the
chosen folders indexed by itself, and a settings window replaces editing TOML.

## Setup

Requirements: Rust ≥ 1.95, `just`, `curl`, the EmbeddingGemma 2 LiteRT-LM model, the
`litert-lm` Python wheel (only for its `liblitert-lm.so`), and the HF `tokenizer.json` of the
same model. Paths live in `~/.config/endex/config.toml`, written with defaults on first run.

The popup also needs Qt ≥ 6.8 (base, declarative, wayland), LayerShellQt, KF6 Kirigami, KIO
and KWindowSystem, qqc2-desktop-style, CMake and Ninja.

```sh
just build      # release build of target/release/{endex,endexd}
just models     # PP-OCRv6 small det/rec + dictionary (Apache-2.0) → <data_dir>/models/ocr
just check      # rustfmt --check, clippy -D warnings, unit tests
just smoke      # synthetic fixtures → isolated index → index (GPU) → eval → status
just eval       # public datasets: prepare once, index, Recall@10 / MRR per mode (needs uv)
just ui         # build target/ui/endex-ui and run its unit test
just dbus-test  # endexd on the smoke-fixture index, called over a private session bus
```

Results and dataset details: [docs/m1-eval.md](docs/m1-eval.md).

## Install (popup and daemon)

```sh
just install                          # ~/.local/bin, D-Bus service files, endexd user unit
systemctl --user enable --now endexd  # optional: start at login, model warm for the first search
```

`just install` touches only user-level paths: `~/.local/bin/{endex,endexd,endex-ui}`,
`~/.local/share/dbus-1/services/org.endex.{Daemon1,Ui1}.service`,
`~/.local/share/applications/endex-ui.desktop` (「Endex 设置」 in the app menu, runs
`endex settings`) and `~/.config/systemd/user/endexd.service`. Without the `enable`, D-Bus starts
the daemon on the first search. `just uninstall` removes these files again (config and index stay).

The popup opens with `endex toggle` (D-Bus starts `endex-ui` if needed). Endex does not write KDE
shortcut settings; bind a key by hand:

1. System Settings → Keyboard → Shortcuts → Add New → Command or Script.
2. Command: the absolute path that `echo "$HOME/.local/bin/endex toggle"` prints (it works even if
   `~/.local/bin` is not on the session's `PATH`; do not rely on `~` being expanded there). Name it
   e.g. "Endex".
3. Set the shortcut. Plasma gives Alt+Space to KRunner; either pick another combination such as
   Meta+Space, or first open Shortcuts → KRunner → "Activate KRunner" and remove Alt+Space there
   (KRunner keeps Alt+F2), then assign Alt+Space to Endex.

In the popup: type to search (120 ms debounce), Ctrl+1/2/3 for hybrid / vector / keyword, ↑↓ to
select, Tab to switch between documents and images, PgUp/PgDn to page each group, Enter to open,
Ctrl+Enter to show in Dolphin, Ctrl+C to copy the path (a normal copy when you selected text in the
search field), Ctrl+Shift+C to copy the chunk or OCR text, Esc to close a menu and then the popup.
←→ move the text cursor until you move through the results (↑↓, PgUp/PgDn, Tab or a click); then
they move in the image grid, until you type again. Clicking outside closes the popup too. The gear
and Ctrl+, open the settings window.

With an empty query the popup lists the recent searches from the usage log (↑↓ to select, Enter
runs one again), the key tips and the indexed counts. With no folders configured it shows a short
guide whose button opens the settings on 「索引目录」 with a welcome. A banner above the results
shows a first build, a rebuild or a large batch in progress (x / y files, ETA, 「搜索结果可能不完整」,
and with many images on the light or mid profile a hint that 「重」 is faster), indexing paused on
battery (「本次继续」, 「资源档位」), paused by the user or a rebuild waiting for that pause (「继续」 /
「本次继续」), an indexing error (the reason and 「继续」; there is no automatic retry), and a model
setting that needs a rebuild. The status line shows the index state (索引就绪 · counts, 索引中 x / y
· GPU 批量, 索引已暂停 · 电池供电, 尚未配置索引目录, …) and, when there is one, a broken config file
(with its path) or a change that waits for a restart. A summon reports at most one usage-log
entry to endexd, and only when the user acted: a result opened, or the popup hidden after a
query the user searched (typed, a recent search, another mode). The automatic re-run of the
previous query, a cleared field or a gear click log nothing; nothing is logged per keystroke.

## Settings window

`endex settings [page]`, the popup's gear or Ctrl+, and the app-menu entry open it (one window per
session; opening it again brings it to the front and switches the page). Pages: `dirs`
(索引目录), `exclude` (排除规则), `watch` (增量与监听), `model` (模型与设备), `status` (索引状态),
`shortcut` (快捷键指引), `resource` (资源档位), `eval` (评测与日志). It follows the Plasma theme.

- **Saving:** edits stay in the window until 「应用」 (`ApplySettings`, which writes `config.toml`
  keeping its comments; endexd reloads it) or 「还原」. The footer counts the changed settings and
  the sidebar marks the pages with edits or errors; invalid values are reported per field and
  nothing is written. Closing with unapplied edits asks: apply, discard or cancel.
- **索引目录:** folders with their kind (文档和图片 / 仅文档 / 仅图片), file counts and 移除;
  「需要你决定」 rows for the XDG Documents and Pictures folders (纳入索引 / 不索引, undo); 添加目录… (the
  standard folder dialog, through the KDE portal). A folder that is unplugged or gone shows
  「暂时不可用（文件仍保留在索引中）」.
- **排除规则:** the four switches, the builtin and sensitive lists (the saved-page rule below is
  shown as 「网页「另存为」生成的 *_files 文件夹里的 .js .css .html .htm」), the document size limit,
  extra rules (one per line) and 检查一个路径 (`CheckPath` against the applied settings).
- **增量与监听:** scan and watch switches, the scan interval, inotify watches in use, the four
  combinations with warnings (watch off: changes wait for the next scan or start; watch only:
  changes while endexd is stopped are missed; both off: nothing updates), and 「立即扫描」 (`Rescan`).
- **模型与设备:** model path (checked), devices for queries, bulk and incremental work, prefix
  style, OCR, whether the GPU engine is loaded; a model, prefix or OCR change warns that the index
  must be rebuilt. The reranker row is an M4 placeholder.
- **索引状态:** progress with 暂停 / 继续 / 本次继续, the error with 「继续」, counts by format, disk
  size, failed files with per-file and 「全部重试」, and 「重建索引…」 with a confirmation that says
  the index is emptied and refilled (partial results meanwhile), that it waits for AC on battery
  (or 「本次继续」), and which unavailable folders are re-read only when they return.
- **快捷键指引:** the manual steps above with a copy button for `<bindir>/endex toggle`, and the
  popup's keys. Endex never writes KDE shortcut settings.
- **资源档位:** pause on battery (and the current power state), low priority, the light / mid /
  heavy profile with its thread counts, GPU release minutes.
- **评测与日志:** the last `endex eval --sets` results (R@10 / MRR per set and mode) and 「运行评测」,
  which runs it in the background; the usage-log switch, its last entries and 「清除日志…」.

Notices on every page: endexd unreachable (with 「启动」), read-only daemon, a broken
`config.toml` (the error and its path; endexd keeps the last good settings), and a change that
needs a restart (`data_dir`, `embedding.library`, low priority turned off). That notice has
「立即重启」, which runs `systemctl --user restart endexd` the way 「启动」 runs `start` (and
`ENDEX_SYSTEMCTL` overrides the program for tests), shows 「正在重启…」 and disappears once the
restarted daemon no longer needs it; unapplied edits are kept. If the restart fails, the error
is shown and the command stays there to copy. A read-only daemon gets only the command.

## Commands

```sh
endex sample [--seed N]                       # private test-set manifests (paths only; postponed)
endex --data-dir D index --manifest a.txt b.txt [--device gpu|cpu] [--rebuild]
endex search "查询 query" [--mode hybrid|vector|keyword] [--limit 10]
endex eval queries.toml [--limit 10]          # Recall@k and MRR per mode and per query kind
endex eval --sets [--saved] [--json]          # every public set under <data_dir>/eval (see below)
endex status
endex config [KEY]                            # resolved settings, e.g. `endex config data_dir`
endex search "…" --daemon                     # ask the running daemon instead (its own index)
endex toggle                                  # show or hide the popup
endex settings [dirs|exclude|watch|model|status|shortcut|resource|eval]   # the settings window
endexd [--config F]                           # the daemon, in the foreground (normally systemd)
endexd [--config F] --data-dir D              # serve the index in D read-only (as in M2)
```

All commands accept `--config <file>` and `--data-dir <dir>` (or `ENDEX_DATA_DIR`), which keeps
the index and catalog in `<dir>` while OCR models and caches stay shared.

`endex index` needs `--data-dir` (isolated indexes such as the eval sets): the data dir's own
index belongs to endexd, and an index another process is writing is refused too.

`endex eval` never writes into the index it evaluates: no lock, no migration, and SQLite opens
the catalog without its shared-memory file (`immutable` when no WAL frames are pending). With
`--sets` it evaluates every `<data_dir>/eval/<set>/queries*.toml` against `<set>/db` (as
prepared by `just eval`), prints a table (or JSON with `--json`) and saves the results to
`<data_dir>/eval-results/last.json`; `--saved` prints that file instead of running. The
settings window's 「运行评测」 runs it with the daemon's `--config`.

A manifest lists one absolute path per line (`#` comments allowed). Each indexed file remembers
the manifest it was last listed in (by absolute path); files that disappear from that manifest
are removed on its next run. Renaming or moving a manifest file therefore orphans its entries:
index the new manifest and `--rebuild`, or keep the old path. Files that failed are retried on
every run; failing to load a model aborts the run. Documents over `index.max_document_mb`
(default 10) are skipped and reported as errors.

`queries.toml`:

```toml
[[query]]
text = "去上海虹桥的火车票"
expect = ["/abs/path/screenshot.png"]
kind = "visual"          # optional; metrics are also reported per kind
```

Ranks are counted inside the result group (documents or images) that contains the expected path.

## Layout

```
crates/endex-core   library
  embed/            Embedder trait, prefixes, LiteRT-LM FFI (libloading, RTLD_LOCAL)
  ocr.rs            Ocr trait, PP-OCRv6 via oar-ocr (ONNX Runtime, CPU), noise filter
  parse.rs          Markdown / plain text / docx → sections of numbered lines or paragraphs
  packed.rs         packed binary data (lookup tables, base64) dropped from document lines
  chunk.rs          sections → chunks of ≤ 480 model tokens
  terms.rs          keyword analysis (jieba + identifier splitting + file-name prefixes)
  catalog.rs        SQLite: files and their owner (manifest or source), change detection, identity
  store.rs          LanceDB: rows, flat vector search, BM25, keyword index upkeep
  index/            Indexer: batched writes, per-stage timings; pipeline.rs (one file through
                    parse/OCR/embed), manifest.rs (`endex index`)
  lock.rs           the writer lock
  exclude.rs        which files under the sources are indexed (rules, .gitignore, sensitive)
  scan.rs           walking a source folder; comparing it with the catalog
  config/edit.rs    config.toml edits that keep comments (toml_edit)
  search.rs         vector / keyword / hybrid (RRF k = 60), grouped results
  eval.rs, sample.rs
crates/endex-cli    the `endex` binary (eval_sets.rs: `endex eval --sets`)
crates/endexd       the daemon
  service/          D-Bus interface org.endex.Daemon1 (mod.rs) and its reply dictionaries
  worker.rs         search thread (warm query model)
  indexer/          indexer thread: loop and state (mod.rs), scans, reconciles, devices and
                    writes (files.rs), queue and ETA (queue.rs), config and identity changes
                    (reconfigure.rs), idle release, upkeep and polling (timers.rs)
  watch/            inotify watches, missing source folders (mod.rs), event coalescing
                    (coalescer.rs)
  settings.rs       GetSettings / ApplySettings keys and checks
  power.rs, usage.rs, app.rs, state.rs, priority.rs
ui/                 endex-ui (C++/QML): the popup (layer shell, daemon client, paged results,
                    navigation) and the settings window (settings.cpp: GetSettings/ApplySettings
                    draft; evalrun.cpp: `endex eval --sets`; settingswindow.cpp: one window,
                    activation; qml/settings/: the window and its eight pages)
dist/               systemd user unit, D-Bus service files and the settings' .desktop entry
                    (templates for `just install`)
scripts/gen_fixtures.py   synthetic smoke-test data
tools/datasets/           public evaluation sets (prepare.py via `uv run`, embed_litert.py)
```

Data: `~/.local/share/endex/{index/, catalog.sqlite, endex.lock, usage.jsonl,
config.last-good.toml, models/ocr/, cache/litert/, eval/, eval-results/, testset/}`
(`$XDG_DATA_HOME/endex` unless `data_dir` says otherwise).

## Keeping the index fresh (endexd)

Run normally (no `--data-dir`), endexd is the **writer** of the index in `data_dir`: it holds
`flock` on `<data_dir>/endex.lock` for its lifetime, so a second endexd refuses to start.
`endex index` never writes that index (it needs `--data-dir`), even while endexd is stopped.
Nothing is read until folders are configured:

```toml
[index]
sources = [
  { path = "~/Notes", kind = "all" },      # all | docs | images
]
declined = ["~/Pictures"]                   # suggestions the user turned down
max_document_mb = 10
```

- **Exclusions** (scan, watch and check-path use the same rules, `[exclude]`): `.gitignore` and
  `.ignore` files in the source and, inside a git repository, in its parent folders up to the
  repository's top (`.ignore` outranks `.gitignore`; the innermost folder decides); a builtin
  list of build output and caches, plus the scripts, styles and frames (`.js`, `.css`, `.html`,
  `.htm`, in any case) at any depth below a folder named `*_files`, which a browser's "Save page
  as, complete" writes next to the page (its images stay; CheckPath names the rule
  `**/*_files/**/*.{js,css,html,htm}`); a sensitive list (keys, credentials, password stores,
  keyrings, browser profiles) whose files are never opened; hidden files and folders unless
  `hidden = true`; extra `rules` in gitignore syntax relative to each source. Parent ignore
  files never come from the home folder or above (a dot-file repository there often ignores
  `*`). Symlinks and special files inside a source are skipped, and files are opened without
  following symlinks; a source folder that is itself a symlink (say, to a second disk) is
  resolved, and its files are indexed under their real path. Excluded folders are not entered
  and not watched. Changing the rules or a source's kind rescans and re-judges files already
  queued, so files enter and leave the index accordingly; removing a source removes its files.
- **Unavailable sources:** a source folder that is missing, unreadable, or empty while it is a
  mount point or on another device than when it last had files (an unplugged drive) keeps its
  files indexed; Status shows `exists = false`. Its return is noticed through a watch on its
  parent folder, or within 30 s (mounting a drive gives no inotify event), and it is rescanned
  without embedding unchanged files again. A normal folder that was really emptied loses its
  rows; so do files deleted one by one inside an available folder.
- **Updates** (`[update]`): `scan` compares the sources with the catalog (mtime + size, then
  BLAKE3) at `scan_interval` = `startup`, `hourly` or `daily`; `watch` follows changes with
  inotify. Events are coalesced per path until 0.5 s pass without one (at most 60 s), then the
  indexer reconciles each path with the catalog, so creates, edits, renames and deletes are all
  handled the same way; a new or moved folder is rescanned as a whole. A full inotify queue
  triggers a rescan. If `fs.inotify.max_user_watches` is reached, all watches are dropped,
  Status says `watch = limit`, and sources are rescanned at least hourly instead. Otherwise
  `startup` means at startup only, also with `watch` off (changes then wait for the next
  start; the settings window warns about that combination); with both off nothing updates.
- **Devices and resources** (`[devices]`, `[resources]`): queries embed on `devices.query`
  (CPU). A change set expected to need ≥ 20 s of CPU embedding (first build, rebuild, a large
  copy) uses `devices.bulk` (GPU); smaller ones use `devices.incremental` (CPU) unless the bulk
  engine is still loaded. The GPU engine is released after `gpu_idle_release_min` idle minutes;
  OCR, the tokenizer and a CPU embedder after one idle minute. With `low_priority` the indexer
  thread and the thread pools it creates (OCR, CPU embedding, the GPU driver's threads) run at
  nice 19 and idle IO priority; turning it off takes effect after a restart. LanceDB's own
  worker threads, which do part of each write, the compaction and the keyword index builds,
  are shared with search and run at normal priority. `profile` (`light` / `mid` / `heavy`) picks `threads`,
  the cap for OCR and CPU-embedding threads. With `pause_on_battery`, indexing pauses while
  UPower reports `OnBattery` (read from the system bus, never written); `ContinueOnBattery`
  overrides that until the power state changes or endexd restarts. Search works throughout.
- **Model identity:** the catalog records model, prefix style and OCR setting. Changing one
  stops indexing (`IndexState.state = needs_rebuild`) until `Rebuild`, which clears the index and
  fills it again; meanwhile search answers from what is there (queries use the index's prefix
  style; with another model, searches fail until the rebuild starts). A change during a
  rebuild restarts it. `Rebuild` while indexing is paused (battery, the user) waits, with the
  old index searchable, until the pause ends or 「本次继续」 (`IndexState.rebuild_pending`).
- **Text pipeline version:** the catalog also records the version of the document text
  pipeline (`text_pipeline` in its meta table; 1 when missing, i.e. before M3 fix 1; now 2,
  which drops packed data, see Design notes). When it differs, the writer processes every
  indexed document again in the background after starting: it records the new version together
  with marks that make the next scan see each document as changed, so the usual scan, the usual
  bulk/incremental device choice and the usual write cadence do the work, and an interrupted
  run resumes instead of starting over. Search answers from the old rows until each file's are
  replaced; nothing is cleared and `needs_rebuild` is not set. Images are not processed again
  (OCR is slow). `endex index` records the current version on a new or `--rebuild` index and
  otherwise leaves the version and unchanged files alone, so an index built from manifests
  (the eval sets) changes only with `--rebuild`; `endex status` shows the version. A read-only
  endexd does not look at it.
- **Writes and keyword index:** small changes are written within a second, bulk work every
  10 s or every 4,096 staged rows (each write adds a table fragment; many small ones slow
  searches down), with a compaction every 30 bulk writes. A new index gets its keyword index
  after its first write. New rows are searchable right away: keyword search scans
  rows that the BM25 index does not cover yet, with the index's analyzer and statistics (about
  3 ms per 1,000 such rows on SciFact). They are folded into the index once 2,000 accumulate,
  two minutes after the last write (with compaction and pruning), and by a full rebuild at the
  end of bulk work.
- **Crash safety:** a catalog flag is set before the first write of a batch and cleared when the
  index is compacted and its keyword indexes are complete; another flag marks queued work. After
  a crash or `kill -9`, the next start finishes the index upkeep and rescans the sources.
- **Config:** `config.toml` stays the source of truth. endexd writes it (only through
  `ApplySettings`, with `toml_edit`, keeping comments and layout; tables the file lacks are
  added with the default config's comments) and follows edits by hand. Edits by hand get the
  same checks as the settings window (rules, sources); a file that does not load or pass keeps
  the last good config running and shows the error in `Status.config_error`. endexd keeps a
  copy of the last good file in its running data dir as `config.last-good.toml` (first line:
  the config file it belongs to). When the default config.toml is broken at startup, endexd
  starts from that file's own copy in the data dir it names, with `config_error` set; a config
  given with `--config` is never replaced: it loads or endexd does not start. Changing
  `data_dir` or `embedding.library` needs a restart.
- **Usage log:** with `log.usage` (default on), `<data_dir>/usage.jsonl` gets one line per popup
  session, reported by the popup: `{"t", "q", "mode", "n", "ms", "pick"}` (`pick`: the path of
  the opened result, `null` if the popup was hidden). Past 1 MiB the older half is dropped. It
  is readable by the user only and never leaves the machine; the popup's recent searches come
  from it.

With `--data-dir` (or `ENDEX_DATA_DIR`) endexd serves that index **read-only**, exactly as in
M2: no sources, scans, watches, settings writes or usage log; methods that would change
something fail with `org.endex.Daemon1.Error.ReadOnly`.

### D-Bus interface `org.endex.Daemon1` (version 2)

`Search`, `Preview` and `Status` are unchanged from version 1 (Status has more keys).

| Member | Signature | |
|---|---|---|
| `Search` | `(s query, s mode, a{sv} opts) → (u, aa{sv}, aa{sv})` | see Design notes |
| `Preview` | `(s id, s query) → a{sv}` | |
| `Status` | `() → a{sv}` | counts and state, keys below |
| `GetSettings` | `() → a{sv}` | flat keys, below |
| `ApplySettings` | `(a{sv} changes) → a{sv}` | `applied` b, `errors` a{ss}, `needs_rebuild` b, `restart_required` b |
| `CheckPath` | `(s path) → a{sv}` | `indexed` b, `reason` s, `rule` s, `file` s, `source` s, `kind` s |
| `FailedFiles` | `(u limit) → aa{sv}` | `path`, `error`, `time` (unix), newest first |
| `Retry` | `(as paths)` | reprocess failed files; all if empty |
| `Pause`, `Resume` | `()` | |
| `ContinueOnBattery` | `()` | 「本次继续」 |
| `Rescan` | `()` | compare all sources now |
| `Rebuild` | `()` | clear and refill the index |
| `ReportUsage` | `(a{sv} entry)` | `query` s, `mode` s, `results` u, `latency_ms` u, `picked` s (empty: none) |
| `RecentQueries` | `(u limit) → as` | distinct, newest first; empty if the log is off |
| `UsageLog` | `(u limit) → as` | the last lines (JSON), oldest first |
| `ClearUsageLog` | `()` | |
| `Version` | property `u` | 2 |
| `IndexState` | property `a{sv}`, emits change signals ≤ 2 per second | below |

`IndexState`: `state` (`idle`, `scanning`, `indexing`, `paused`, `needs_rebuild`, `error`),
`reason` (`user` or `battery` when paused; `model`, `prefix_style` or `ocr` for a rebuild; the
message on errors), `done` / `total` files, `images` (images among the queued files), `device` (`gpu`, `cpu` or empty), `bulk` (b),
`rebuilding` (b: results are partial), `eta` (seconds, -1 unknown), `gpu_loaded` (b),
`rebuild_pending` (b: a requested rebuild waits for the pause in `reason` to end).

`Status` keys of version 1: `version`, `model`, `query_device`, `index_path`, `config_path`,
`state` (`ready`, `empty`, `rebuilding`, `error`), `error`, `doc_files`, `image_files`,
`doc_chunks`, `image_chunks`, `errors`, `last_indexed`. Added in version 2: `mode` (`writer` or
`read_only`), `data_dir` (the config's, where the usage log and eval sets live),
`index_state` (as above), `config_error`, `restart_required`, `watch` (`off`, `on`, `limit`,
`error`), `watches`, `max_watches`, `on_battery`, `battery_known`, `battery_percent` (-1
unknown), `battery_override`, `pause_on_battery`, `usage_log`, `sources` (aa{sv}: `path`,
`kind`, `files`, `errors`, `exists`: false while the folder is unavailable), `formats` (a{su}:
indexed files per extension), `disk_bytes` (the table and the catalog).

`CheckPath` reasons: `outside_sources`, `internal` (endex's data), `symlink`, `special`,
`hidden`, `sensitive`, `builtin`, `rule`, `gitignore` (with `file`), `unsupported`, `kind` (not
taken by this source's kind), `encoding` (a file name that is not UTF-8), `too_large`.

Settings keys (`GetSettings` / `ApplySettings`; `ApplySettings` takes any subset, writes nothing
if one is invalid, and keys errors by name, `index.sources.<i>` and `exclude.rules.<i>` per
entry): `index.sources` (aa{sv}: `path`, `kind`), `index.declined` (as),
`index.max_document_mb` (u, 1–1024), `exclude.gitignore`, `exclude.builtin`,
`exclude.sensitive`, `exclude.hidden` (b), `exclude.rules` (as), `update.scan` (b),
`update.scan_interval` (s), `update.watch` (b), `embedding.model` (s, a `.litertlm` file),
`embedding.prefix_style` (s), `ocr.enabled` (b), `devices.query`, `devices.bulk`,
`devices.incremental` (s: `cpu` or `gpu`), `resources.pause_on_battery`,
`resources.low_priority` (b), `resources.profile` (s), `resources.threads.light|mid|heavy`
(u, 1–64), `resources.gpu_idle_release_min` (u, 0–1440), `log.usage` (b). Read-only:
`index.suggested` (as: `~/Documents` and `~/Pictures` unless already a source or declined),
`exclude.builtin_list`, `exclude.sensitive_list`, `config_path`.

The popup's interface `org.endex.Ui1` at `/org/endex/Ui` (endex-ui, D-Bus activated): `Toggle()`,
`Show(s query)`, `ShowSettings(s page)` (empty: the page it was on) and
`ActivateSettings(s page, s activation_token)` (the same with the caller's XDG activation token;
`endex settings` uses it when `XDG_ACTIVATION_TOKEN` is set, e.g. from the app menu). With
`ENDEX_UI_DEBUG=1`, `/org/endex/UiDebug` (`org.endex.UiDebug`) reports the popup's and the
settings window's state and stands in for clicks, typing and the folder dialog in the nested-KWin
tests.

For tests only: `ENDEX_DEBUG_BATTERY=ac|battery` (other values are ignored) replaces UPower with
a fake power state and adds `org.endex.DaemonDebug1.SetOnBattery(b)` on the same object;
`ENDEX_DEBUG_MAX_WATCHES=n` pretends the inotify limit is n.

## Design notes

- **Embeddings.** EmbeddingGemma 2 (768-d, text and images in one space) through the LiteRT-LM
  C API. Indexing runs on the GPU (WebGPU/Vulkan) and falls back to the CPU; queries always embed
  on the CPU. The model identity and prefix style are stored in the catalog; changing either
  requires a rebuild (「重建索引」 in the settings; `endex index --rebuild` for `--data-dir`
  indexes). Inputs over the model's largest signature (1,024 tokens) are
  truncated instead of failing. Images are embedded from their encoded bytes; formats other
  than PNG/JPEG (WebP) are re-encoded as PNG first because LiteRT only decodes those two.
- **One process.** LiteRT-LM (`dlopen`, `RTLD_LOCAL`) and ONNX Runtime (statically linked by
  `ort`) run side by side without symbol clashes, on CPU and GPU, in either load order.
- **Keyword search.** Text is pre-analyzed in Rust into whitespace-separated terms: jieba
  search-mode segmentation for CJK minus Chinese function words, identifiers kept whole and
  split (`load_user_settings`, `RetryingHttpClient`, `E0382`), lowercase. LanceDB indexes that
  `terms` column with its whitespace tokenizer plus English stemming and stop words. A second
  `name` column holds the terms of `parent/file name` and all their prefixes (edge n-grams), so
  partial names such as `router_man` match; infix n-grams were tried and dropped because they
  matched `the` inside `weather`. Queries go through the same analyzer; a row scores the better
  of both columns. Changing the analyzer requires `endex index --rebuild`.
- **Rows.** One row per document chunk (title, heading path + line/paragraph range as
  `location`), one row per image (image vector; OCR text after the noise filter as `text`).
  Results are de-duplicated per file and listed separately for documents and images.
- **Incremental.** Unchanged size + mtime skips a file; otherwise the BLAKE3 hash decides
  between "touched" (catalog only) and "modified" (rows replaced). After writes the table is
  compacted, both keyword indexes are rebuilt and old versions are pruned; a catalog flag set
  before the first write makes an interrupted run finish this on the next run. `--rebuild`
  drops only the endex table, never the directory around it.
- **Chunks.** At most 480 model tokens including the heading line (a long heading path is cut
  in the chunk text but kept whole in `location`); over-long lines are split at sentence or
  word breaks using one tokenization of the line.
- **Packed data.** Before chunking, each line (or docx paragraph) of a document is checked for
  packed binary data, such as the HTML-entity decode table that JS bundles inline as one string
  literal: its code points fall into letter ranges, so the analyzer found words such as `cute`
  between CJK characters and previews showed the string. A line is dropped when it has at
  least 200 characters (whitespace aside), its letters come from at least 6 scripts, and at
  least one letter in 8 is directly followed (no space or punctuation between) by a letter of
  another script; controls and unassigned code points count as one more script, private-use
  glyphs (icon fonts, Office bullets) as none. Han and kana count as one script; all Latin
  blocks as one, together with number forms, super- and subscripts and mathematical letters
  (第Ⅱ章, ℝⁿ, xₖ). Real text switches between two or three scripts (Chinese with English or
  pinyin, Greek in formulas), and text in many scripts (language lists, i18n bundles) switches
  at word boundaries, so prose in any language,
  formulas, emoji, box drawing and minified code stay; the public eval sets and 56 million
  lines of system text and code files lost none. A dropped line stays as an empty line, so
  locations keep their line numbers. Base64 runs of at least 256 characters that mix upper and
  lower case letters and digits (embedded images, fonts, wasm) are replaced by `…`, in
  headings and titles too, and the rest of the line stays: they are binary data, cost many chunks of embedding and match nothing
  a user types. Hex strings and long words are not touched. Known limit: a table that the
  bundler escaped to ASCII (`"\u4026cute\u803b…"`) is not recognized; dropping escape-heavy
  lines would also drop real JSON written with `\u` escapes.
- **Daemon.** `endexd` owns `org.endex.Daemon1` at `/org/endex/Daemon` (interface version in the
  `Version` property). It keeps the query embedder warm on one search thread and writes the
  index on another (see above). Before each search it moves to the newest table version, so
  its own writes and, in read-only mode, `endex index` runs (including `--rebuild`) show up
  without a restart. A `query_id` from a client cancels that client's older queries between
  search stages. Results are the CLI's: same ranking, same per-file dedup. Each hit carries the
  fused `score` plus `vec_score` (cosine) and `kw_score`/`kw_rank` (BM25) when that side ranked
  the file, and highlight ranges in Unicode characters. Queries are cut at 512 characters.
  SQLite (WAL mode) keeps `catalog.sqlite-wal` and `-shm` next to the catalog. A second endexd
  exits instead of taking over the bus name or the index; the unit does not restart on startup
  errors (exit status 2: config, model, index locked, name taken).
- **Popup.** A LayerShellQt surface on the *top* layer (on *overlay*, KWin stacks the fcitx5
  candidate window below it). Each show maps it with exclusive keyboard interactivity, so focus
  does not depend on an activation token; once focused it drops to on-demand, so clicking another
  window takes focus away and the popup hides.

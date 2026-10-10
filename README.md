# Endex

Local search over documents and images (including text inside screenshots) for one
Linux desktop. One query, Chinese or English. M1: Rust core + CLI. M2: a search daemon
(`endexd`, D-Bus) and a popup (`endex-ui`) for KDE Plasma 6 on Wayland.

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
`~/.local/share/dbus-1/services/org.endex.{Daemon1,Ui1}.service` and
`~/.config/systemd/user/endexd.service`. Without the `enable`, D-Bus starts the daemon on the first
search. `just uninstall` removes these files again (config and index stay).

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
opens `config.toml` in the default editor.

## Commands

```sh
endex sample [--seed N]                       # private test-set manifests (paths only; postponed)
endex index --manifest a.txt b.txt [--device gpu|cpu] [--rebuild]
endex search "查询 query" [--mode hybrid|vector|keyword] [--limit 10]
endex eval queries.toml [--limit 10]          # Recall@k and MRR per mode and per query kind
endex status
endex config [KEY]                            # resolved settings, e.g. `endex config data_dir`
endex search "…" --daemon                     # ask the running daemon instead (its own index)
endex toggle                                  # show or hide the popup
endexd [--config F] [--data-dir D]            # the daemon, in the foreground (normally systemd)
```

All commands accept `--config <file>` and `--data-dir <dir>` (or `ENDEX_DATA_DIR`), which keeps
the index and catalog in `<dir>` while OCR models and caches stay shared.

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
  chunk.rs          sections → chunks of ≤ 480 model tokens
  terms.rs          keyword analysis (jieba + identifier splitting + file-name prefixes)
  catalog.rs        SQLite: files, change detection, model identity
  store.rs          LanceDB: rows, flat vector search, BM25
  index.rs          incremental pipeline with per-stage timings
  search.rs         vector / keyword / hybrid (RRF k = 60), grouped results
  eval.rs, sample.rs
crates/endex-cli    the `endex` binary
crates/endexd       the search daemon: D-Bus interface, search thread
ui/                 the popup (C++/QML): layer shell, daemon client, paged results, navigation
dist/               systemd user unit and D-Bus service files (templates for `just install`)
scripts/gen_fixtures.py   synthetic smoke-test data
tools/datasets/           public evaluation sets (prepare.py via `uv run`, embed_litert.py)
```

Data: `~/.local/share/endex/{index/, catalog.sqlite, models/ocr/, cache/litert/, testset/}`.

## Design notes

- **Embeddings.** EmbeddingGemma 2 (768-d, text and images in one space) through the LiteRT-LM
  C API. Indexing runs on the GPU (WebGPU/Vulkan) and falls back to the CPU; queries always embed
  on the CPU. The model identity and prefix style are stored in the catalog; changing either
  requires `endex index --rebuild`. Inputs over the model's largest signature (1,024 tokens) are
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
- **Daemon.** `endexd` owns `org.endex.Daemon1` at `/org/endex/Daemon` (interface version in the
  `Version` property): `Search(s query, s mode, a{sv} opts) → (u query_id, aa{sv} docs,
  aa{sv} images)`, `Preview(s id, s query) → a{sv}` and `Status() → a{sv}`. It is read-only and
  keeps the CPU query embedder warm on one search thread. Before each call it moves to the newest
  table version, so `endex index` runs (including `--rebuild`) show up without a restart. A
  `query_id` from a client cancels that client's older queries between search stages. Results are
  the CLI's: same ranking, same per-file dedup. Each hit carries the fused `score` plus
  `vec_score` (cosine) and `kw_score`/`kw_rank` (BM25) when that side ranked the file, and
  highlight ranges in Unicode characters. Queries are cut at 512 characters. The daemon opens the
  catalog read-only, yet SQLite (WAL mode) still keeps `catalog.sqlite-wal` and `-shm` next to it;
  they are expected. A second `endexd` exits instead of taking over the bus name; the unit does
  not restart on startup errors (exit status 2: config, model, name taken).
- **Popup.** A LayerShellQt surface on the *top* layer (on *overlay*, KWin stacks the fcitx5
  candidate window below it). Each show maps it with exclusive keyboard interactivity, so focus
  does not depend on an activation token; once focused it drops to on-demand, so clicking another
  window takes focus away and the popup hides.

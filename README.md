# Endex

Local search over documents and images (including text inside screenshots) for one
Linux desktop. One query, Chinese or English. M1: Rust core + CLI.

## Setup

Requirements: Rust ≥ 1.95, `just`, `curl`, the EmbeddingGemma 2 LiteRT-LM model, the
`litert-lm` Python wheel (only for its `liblitert-lm.so`), and the HF `tokenizer.json` of the
same model. Paths live in `~/.config/endex/config.toml`, written with defaults on first run.

```sh
just build     # release build of target/release/endex
just models    # PP-OCRv6 small det/rec + dictionary (Apache-2.0) → <data_dir>/models/ocr
just check     # rustfmt --check, clippy -D warnings, unit tests
just smoke     # synthetic fixtures → isolated index → index (GPU) → eval → status
just eval      # public datasets: prepare once, index, Recall@10 / MRR per mode (needs uv)
```

Results and dataset details: [docs/m1-eval.md](docs/m1-eval.md).

## Commands

```sh
endex sample [--seed N]                       # private test-set manifests (paths only; postponed)
endex index --manifest a.txt b.txt [--device gpu|cpu] [--rebuild]
endex search "查询 query" [--mode hybrid|vector|keyword] [--limit 10]
endex eval queries.toml [--limit 10]          # Recall@k and MRR per mode and per query kind
endex status
endex config [KEY]                            # resolved settings, e.g. `endex config data_dir`
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

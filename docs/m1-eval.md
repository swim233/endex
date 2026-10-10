# M1 evaluation on public datasets

Run on 2026-10-10 with `just eval` (RTX 4060 Laptop, Ryzen 9 7940HX). Every set has its own
isolated index (`<data_dir>/eval/<set>/db`). Queries embed on the CPU; indexing embeds on the
GPU, and OCR runs on the CPU. Recall@10 counts the fraction of a query's relevant files that
appear in the top 10 of their result group (documents or images). MRR uses the best-ranked
relevant file. Each set has 100 queries.

## Results

| Set | Corpus | Vector R@10 / MRR | Keyword R@10 / MRR | Hybrid R@10 / MRR |
|---|---|---|---|---|
| XM3600 zh, caption → photo | 500 images | **1.000 / 0.924** | 0.000 / 0.000 | 1.000 / 0.910 |
| XM3600 en, caption → photo | 500 images | **0.960 / 0.835** | 0.000 / 0.000 | 0.960 / 0.795 |
| MIRACL-VISION zh, question → page image | 300 images | 0.939 / 0.776 | 0.930 / 0.812 | **0.964 / 0.850** |
| T2Retrieval zh, question → passage | 2,000 passages (3,874 chunks) | 0.767 / 0.833 | 0.716 / 0.769 | **0.818 / 0.888** |
| SciFact en, claim → abstract | 5,183 abstracts (6,092 chunks) | 0.925 / 0.803 | 0.862 / 0.709 | **0.940 / 0.803** |

T2Retrieval has 11 queries with more than 10 relevant passages (up to 22), so its best possible
Recall@10 is 0.972, not 1.0. On the other sets it is 1.0.

### Indexing

| Set | Total | OCR / image | Embed / item (GPU) | Write + index build |
|---|---|---|---|---|
| XM3600 | 56.0 s | 37 ms (photos, mostly no text) | 70 ms per image | 0.1 s |
| MIRACL-VISION | 241.6 s | 695 ms (980×980 text pages) | 96 ms per image | 0.2 s |
| T2Retrieval | 64.4 s | — | 15 ms per chunk | 0.8 s |
| SciFact | 92.9 s | — | 14 ms per chunk | 1.7 s |

Every timing includes 1.4–1.5 s of model loading. On a 6k-chunk index, a hybrid search run from
the command line takes about 0.41 s end to end: process start, CPU model load, query embedding,
two LanceDB queries per group.

### History

| Run | Change | XM3600 hybrid MRR zh / en | MIRACL hybrid | T2 keyword | T2 hybrid | SciFact hybrid |
|---|---|---|---|---|---|---|
| 1 | first run | 0.519 / 0.391 | 0.954 / 0.822 | 0.695 / 0.752 | 0.815 / 0.891 | 0.940 / 0.797 |
| 2 | Chinese stop words, no 1-char name fragments | 0.865 / 0.634 | 0.964 / 0.822 | 0.716 / 0.769 | 0.818 / 0.887 | 0.940 / 0.797 |
| 3 | RRF ties go to the vector hit; chunker, OCR-filter and retry fixes | 0.910 / 0.795 | 0.964 / 0.850 | 0.716 / 0.769 | 0.818 / 0.888 | 0.940 / 0.803 |

Vector and keyword numbers did not change between runs, with two exceptions: T2 keyword in
run 2, and SciFact vector (0.915 / 0.797 → 0.925 / 0.803) in run 3, where the new splitter
cuts long abstracts at different points.

## Observations

- **Vector search carries image retrieval.** XM3600 photos contain almost no text, so keyword
  search has nothing to match. Hybrid search then still loses some MRR on photos (0.910 vs
  0.924 zh, 0.795 vs 0.835 en): RRF gives an incidental OCR match, such as a menu that says
  "plate", the same weight as the top vector hit. Once ties go to the vector rank, the loss is
  small. For text and text-heavy images, hybrid search is the best mode on every set.
- **Chinese stop words mattered.** Function words such as 的 / 在 / 有 matched any OCR text
  until the analyzer dropped them (run 1 → 2).
- **OCR makes text-heavy images searchable by keyword.** On MIRACL-VISION (Wikipedia page
  screenshots), OCR keyword search reaches 0.930 recall@10, even though the pages mix Traditional
  and Simplified Chinese and the queries are Simplified.
- **Inputs longer than 1,024 tokens fail.** LiteRT-LM rejects such inputs by default, because the
  model's largest loaded text signature is 1,024 tokens. Endex chunks stay at 480 tokens or
  fewer, but the embedder sets the overflow strategy to *truncate* so long queries cannot fail.

## M4 follow-ups

- **Weighted fusion or a minimum keyword score** (minimum-should-match / BM25 floor), so a
  one-word OCR match cannot rival a strong vector hit on photos. M1 keeps plain RRF with k = 60.
- **Traditional/Simplified folding** (e.g. OpenCC) for OCR text and queries.

## How the sets are built

`tools/datasets/prepare.py` uses a fixed seed (20261010) and writes corpus files, `manifest.txt`
and `queries*.toml` to `<data_dir>/eval/<set>/`. Raw downloads come from Hugging Face at
pinned dataset revisions, are checked against pinned SHA-256 sums (about 3 GB), and are cached
in `eval/raw/`.

- **xm3600**: from `mteb/XM3600T2IRetrieval`. 100 target images with one zh caption
  (`queries.toml`) and one en caption (`queries.en.toml`) each, plus 400 random distractor
  images.
- **miracl_vision**: from `nvidia/miracl-vision` (zh). 100 queries, all 140 of their positive
  page images, filled to 300 with random zh pages.
- **t2retrieval**: from `C-MTEB/T2Retrieval` dev. 100 queries with all 535 of their positives.
  For each query, the set adds the top-5 non-relevant BM25 hits (jieba, full 118k corpus) and the
  top-5 non-relevant vector hits. The vector hits come from a random pool of 20k passages,
  embedded by `embed_litert.py` with the same model. Overlap between the two kinds of negative
  is 14 of 1,000. The set is then filled to 2,000 with random passages. `<br>` becomes a line
  break and `<img>` is removed. Per-query negatives are recorded in `negatives.json`.
- **scifact**: from `BeIR/scifact`. The full corpus (title + abstract per file) and 100 sampled
  test queries.

Commands:

- `just eval`: prepares any missing sets, indexes incrementally, evaluates all four sets and
  logs to `<set>/index.log` and `<set>/eval-*.log`.
- `just eval scifact`: one set only.
- To re-index from scratch, delete `<set>/db`.
- `prepare.py --force <set>` rebuilds a set and deletes its index.

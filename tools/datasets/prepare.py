#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["pyarrow>=17", "numpy>=2", "jieba>=0.42"]
# ///
"""Build fixed-seed evaluation sets from public Hugging Face datasets for `endex eval`.

Each set is written to <data-dir>/eval/<set>/ as corpus files under files/, manifest.txt
(absolute paths) and queries*.toml (`expect` = absolute paths). Raw downloads are cached in
<data-dir>/eval/raw/.

  xm3600         mteb/XM3600T2IRetrieval: 100 target images (one zh and one en caption each)
                 + 400 random distractor images.
  miracl_vision  nvidia/miracl-vision zh: 100 queries, all their positive page images, filled to
                 300 with random zh page images.
  t2retrieval    C-MTEB/T2Retrieval dev: 100 queries, all positives, per query the top-5
                 non-relevant BM25 hits (jieba, full corpus) and top-5 non-relevant vector hits
                 (EmbeddingGemma over a 20k random pool), filled to 2,000 passages.
  scifact        BeIR/scifact: full corpus (5,183 abstracts), 100 sampled test queries.

Usage: prepare.py [--data-dir DIR] [--model FILE] [--force] [SET ...]
Sets go to <data-dir>/eval/ (default data dir: $XDG_DATA_HOME/endex). Files are downloaded at
pinned dataset revisions and checked against pinned SHA-256 sums. Downloads honour HF_ENDPOINT
(default https://huggingface.co) and fall back to hf-mirror.com. Vector negatives run
tools/datasets/embed_litert.py with $ENDEX_LITERT_PYTHON (default ~/.venvs/litert-lm/bin/python)
and --model (default: embedding.model from the endex config).
"""

import argparse
import hashlib
import json
import os
import random
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib
import urllib.error
import urllib.request
from collections import defaultdict
from pathlib import Path

import numpy as np
import pyarrow.parquet as pq

SEED = 20261010
USER_AGENT = "endex-eval-prep"
HERE = Path(__file__).resolve().parent

# Dataset revisions and the SHA-256 of every file used (checked 2026-10-10).
REVISIONS = {
    "mteb/XM3600T2IRetrieval": "963e5fcdc6b2f222360cec3df3869cc9b8343cb0",
    "nvidia/miracl-vision": "309e1696433408fbd555959cf1da968f3814f8b6",
    "C-MTEB/T2Retrieval": "8731a845f1bf500a4f111cf1070785c793d10e64",
    "C-MTEB/T2Retrieval-qrels": "1c83b8d1544e529875e3f6930f3a1fcf749a8e97",
    "BeIR/scifact": "b3b5335604bf5ee3c4447671af975ea25143d4f5",
    "BeIR/scifact-qrels": "2938d17dc3b09882fdb8c12bbbe2e2dc0e75a029",
}
SHA256 = {
    "mteb/XM3600T2IRetrieval/zh-queries/test-00000-of-00001.parquet": "3d35effa908f95eeaa109dbd672a5560bb742efc848c8cedf94c4e605ac108fd",
    "mteb/XM3600T2IRetrieval/zh-qrels/test-00000-of-00001.parquet": "ddf4584d8606a144c7f03d20e032a7a6890940424c5bea09d99912fe52478996",
    "mteb/XM3600T2IRetrieval/en-queries/test-00000-of-00001.parquet": "c8d0660daf632009bbca916ed87896b934e550731454459fcae7b63d8ac79502",
    "mteb/XM3600T2IRetrieval/en-qrels/test-00000-of-00001.parquet": "d6bce87d3afc6633c0879164efb493464f23c9fa091a5e5711809fdee460136e",
    "mteb/XM3600T2IRetrieval/zh-corpus/test-00000-of-00001.parquet": "937d3b1e9ddac5727678310919ec9e8ec004e529528538b83e760eed9ecf6881",
    "nvidia/miracl-vision/zh/queries.parquet": "ad11e8a880c0f871dcae8a4e354a43937156e6d84e55b0ba7eab56f359d8d60e",
    "nvidia/miracl-vision/zh/qrels.parquet": "65c5138e9931d47d5dbe19e3063a082a97e4e332fae6800574242ebc652d0ad0",
    "nvidia/miracl-vision/zh/corpus.parquet": "84cc09adfe3dc5b823bf027eff61f82fc55c351df08ce286d952be8c6bc438cb",
    "nvidia/miracl-vision/zh/images.parquet": "1d5ee37fbf8429c79fc0f7f318e3ed00c463812c600b8a4e362508e85cb8b234",
    "C-MTEB/T2Retrieval/data/queries-00000-of-00001-930bf3b805a80dd9.parquet": "47a5866f1e668700420f44b200f9808c9e799208a46da147dfe380b40fb406ec",
    "C-MTEB/T2Retrieval/data/corpus-00000-of-00001-8afe7b7a7eca49e3.parquet": "d4a207a9277eb1cf31f68ab6a7236cbf3e8fbf9493e0e2b577a34660929fcd17",
    "C-MTEB/T2Retrieval-qrels/data/dev-00000-of-00001-92ed0416056ff7e1.parquet": "a283ce5c8bd739e7f5524107586eb21e68d7736172524b79eb5bd844c4b78c00",
    "BeIR/scifact/corpus/corpus-00000-of-00001.parquet": "243324b35f03d82bd6d98a5f575966876e86cad7ce16e5333a35b1b793dc4f45",
    "BeIR/scifact/queries/queries-00000-of-00001.parquet": "1c37956c5dc8b810b60302323c24d1a9e79e26411ba8f5ad9d0888642e2a9034",
    "BeIR/scifact-qrels/test.tsv": "0864bb985e0ca2367ba217977e72004d549054b2b06666ed9d4825ac7c21284c",
}


def log(message):
    print(message, file=sys.stderr, flush=True)


# ---------------------------------------------------------------- downloads


def endpoints():
    primary = os.environ.get("HF_ENDPOINT", "https://huggingface.co").rstrip("/")
    return list(dict.fromkeys([primary, "https://hf-mirror.com"]))


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as f:
        while chunk := f.read(1 << 22):
            digest.update(chunk)
    return digest.hexdigest()


def fetch(raw: Path, repo: str, path: str) -> Path:
    """Downloads a pinned dataset file once (resuming partial downloads), verifies its SHA-256
    and returns the local path."""
    dest = raw / repo / path
    expected = SHA256[f"{repo}/{path}"]
    stamp = dest.with_name(dest.name + ".sha256")
    if dest.exists():
        if stamp.exists() and stamp.read_text().strip() == expected:
            return dest
        if sha256(dest) == expected:
            stamp.write_text(expected)
            return dest
        log(f"{dest}: checksum mismatch, downloading again")
        dest.unlink()
    dest.parent.mkdir(parents=True, exist_ok=True)
    part = dest.with_name(dest.name + ".part")
    errors = []
    for endpoint in endpoints():
        url = f"{endpoint}/datasets/{repo}/resolve/{REVISIONS[repo]}/{path}"
        done = part.stat().st_size if part.exists() else 0
        request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
        if done:
            request.add_header("Range", f"bytes={done}-")
        try:
            log(f"download {url}")
            with urllib.request.urlopen(request, timeout=60) as response:
                if response.headers.get_content_type() == "text/html":
                    raise OSError("got an HTML page instead of the file")
                mode = "ab" if done and response.status == 206 else "wb"
                with open(part, mode) as out:
                    shutil.copyfileobj(response, out, length=1 << 20)
        except (urllib.error.URLError, OSError) as error:
            errors.append(f"{endpoint}: {error}")
            continue
        if sha256(part) != expected:
            part.unlink()
            errors.append(f"{endpoint}: checksum mismatch")
            continue
        part.rename(dest)
        stamp.write_text(expected)
        return dest
    sys.exit(f"download failed for {repo}/{path}: {'; '.join(errors)}")


def read(raw, repo, path, columns=None, filters=None):
    return pq.read_table(fetch(raw, repo, path), columns=columns, filters=filters).to_pylist()


# ---------------------------------------------------------------- output


class SetWriter:
    """Builds a set in a temporary directory and moves it into place when complete."""

    def __init__(self, root: Path, name: str):
        self.final = root / name
        self.dir = root / f".{name}.tmp"
        shutil.rmtree(self.dir, ignore_errors=True)
        (self.dir / "files").mkdir(parents=True)
        self.paths = []

    def file(self, rel: str, content) -> str:
        """Writes a corpus file and returns its final absolute path."""
        path = self.dir / "files" / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        if isinstance(content, str):
            path.write_text(content, encoding="utf-8")
        else:
            path.write_bytes(content)
        final = str(self.final / "files" / rel)
        self.paths.append(final)
        return final

    def queries(self, filename: str, queries):
        """`queries`: iterable of (text, [absolute paths], kind)."""
        with open(self.dir / filename, "w", encoding="utf-8") as out:
            for text, expect, kind in queries:
                out.write("[[query]]\n")
                out.write(f"text = {json.dumps(text, ensure_ascii=False)}\n")
                out.write(f"expect = {json.dumps(expect, ensure_ascii=False)}\n")
                out.write(f'kind = "{kind}"\n\n')

    def finish(self):
        (self.dir / "manifest.txt").write_text("".join(p + "\n" for p in self.paths), encoding="utf-8")
        shutil.rmtree(self.final, ignore_errors=True)
        self.dir.rename(self.final)
        log(f"{self.final.name}: {len(self.paths)} files")


def image_ext(data: bytes) -> str:
    if data.startswith(b"\x89PNG"):
        return "png"
    if data.startswith(b"\xff\xd8"):
        return "jpg"
    if data[8:12] == b"WEBP":
        return "webp"
    raise ValueError("unknown image format")


# ---------------------------------------------------------------- sets


def xm3600(raw, out, _opts):
    repo = "mteb/XM3600T2IRetrieval"
    rng = random.Random(f"{SEED}-xm3600")
    captions = {}
    for lang in ("zh", "en"):
        texts = {q["id"]: q["text"] for q in read(raw, repo, f"{lang}-queries/test-00000-of-00001.parquet")}
        by_image = defaultdict(list)
        for qrel in read(raw, repo, f"{lang}-qrels/test-00000-of-00001.parquet"):
            by_image[qrel["corpus-id"]].append(texts[qrel["query-id"]])
        captions[lang] = by_image
    corpus = read(raw, repo, "zh-corpus/test-00000-of-00001.parquet", columns=["id", "image"])
    images = {row["id"]: row["image"]["bytes"] for row in corpus}
    candidates = sorted(i for i in images if captions["zh"].get(i) and captions["en"].get(i))
    targets = rng.sample(candidates, 100)
    others = rng.sample(sorted(set(images) - set(targets)), 400)

    writer = SetWriter(out, "xm3600")
    paths = {}
    for image_id in sorted(targets + others):
        data = images[image_id]
        paths[image_id] = writer.file(f"images/{image_id.removeprefix('corpus-')}.{image_ext(data)}", data)
    for lang, filename in (("zh", "queries.toml"), ("en", "queries.en.toml")):
        writer.queries(
            filename,
            ((rng.choice(sorted(captions[lang][i])), [paths[i]], lang) for i in targets),
        )
    writer.finish()


def miracl_vision(raw, out, _opts):
    repo = "nvidia/miracl-vision"
    rng = random.Random(f"{SEED}-miracl_vision")
    queries = {q["_id"]: q["text"] for q in read(raw, repo, "zh/queries.parquet")}
    positives = defaultdict(set)
    for qrel in read(raw, repo, "zh/qrels.parquet"):
        if qrel["score"] > 0:
            positives[str(qrel["query-id"])].add(str(qrel["corpus-id"]))
    corpus = {row["_id"]: row["image_id"] for row in read(raw, repo, "zh/corpus.parquet", columns=["_id", "image_id"])}
    sampled = rng.sample(sorted(q for q in positives if q in queries), 100)
    keep = set().union(*(positives[q] for q in sampled))
    keep |= set(rng.sample(sorted(set(corpus) - keep), 300 - len(keep)))

    wanted = {corpus[doc]: doc for doc in keep}
    rows = read(raw, repo, "zh/images.parquet", filters=[("file_name", "in", sorted(wanted))])
    writer = SetWriter(out, "miracl_vision")
    paths = {}
    for row in sorted(rows, key=lambda r: wanted[r["file_name"]]):
        doc, data = wanted[row["file_name"]], row["image"]["bytes"]
        paths[doc] = writer.file(f"pages/{doc}.{image_ext(data)}", data)
    writer.queries(
        "queries.toml",
        ((queries[q], sorted(paths[d] for d in positives[q]), "zh") for q in sampled),
    )
    writer.finish()


def clean_passage(text):
    text = re.sub(r"<br\s*/?>", "\n", text, flags=re.IGNORECASE)
    text = re.sub(r"<img[^>]*>", "", text, flags=re.IGNORECASE)
    return re.sub(r"\n{3,}", "\n\n", text).strip() + "\n"


def bm25_top(corpus_ids, corpus_texts, query_texts, exclude, k=5, k1=1.2, b=0.75):
    """Top-k non-excluded corpus ids per query under BM25 with jieba tokens."""
    import jieba

    jieba.setLogLevel(60)

    def tokens(text):
        return [t for t in jieba.lcut(text.lower()) if t.strip() and any(c.isalnum() for c in t)]

    query_tokens = [tokens(q) for q in query_texts]
    vocab = {t for qt in query_tokens for t in qt}
    postings = defaultdict(lambda: ([], []))
    lengths = np.zeros(len(corpus_ids), dtype=np.float32)
    for i, text in enumerate(corpus_texts):
        if i % 20000 == 0:
            log(f"bm25: tokenized {i}/{len(corpus_ids)}")
        doc = tokens(text)
        lengths[i] = len(doc)
        counts = defaultdict(int)
        for t in doc:
            if t in vocab:
                counts[t] += 1
        for t, c in counts.items():
            postings[t][0].append(i)
            postings[t][1].append(c)
    n, avgdl = len(corpus_ids), lengths.mean()
    results = []
    for qt, skip in zip(query_tokens, exclude):
        scores = np.zeros(n, dtype=np.float32)
        for t in qt:
            if t not in postings:
                continue
            docs, tfs = (np.asarray(a) for a in postings[t])
            idf = np.log(1 + (n - len(docs) + 0.5) / (len(docs) + 0.5))
            tf = tfs.astype(np.float32)
            scores[docs] += idf * tf * (k1 + 1) / (tf + k1 * (1 - b + b * lengths[docs] / avgdl))
        top = [corpus_ids[i] for i in np.argsort(-scores)[: k + len(skip) + 50] if scores[i] > 0]
        results.append([d for d in top if d not in skip][:k])
    return results


def embed(texts, model: str, cache_dir: Path):
    python = os.path.expanduser(os.environ.get("ENDEX_LITERT_PYTHON", "~/.venvs/litert-lm/bin/python"))
    cache_dir.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        src, dst = Path(tmp) / "in.jsonl", Path(tmp) / "out.npy"
        src.write_text("".join(json.dumps({"text": t}, ensure_ascii=False) + "\n" for t in texts), encoding="utf-8")
        subprocess.run(
            [python, "-I", str(HERE / "embed_litert.py"), model, str(cache_dir), str(src), str(dst)], check=True
        )
        return np.load(dst)


def t2retrieval(raw, out, opts, pool_size=20_000, total=2_000):
    if not opts.model:
        sys.exit("t2retrieval: pass --model (the .litertlm file) or create the endex config first")
    rng = random.Random(f"{SEED}-t2retrieval")
    queries = {
        q["id"]: q["text"]
        for q in read(raw, "C-MTEB/T2Retrieval", "data/queries-00000-of-00001-930bf3b805a80dd9.parquet")
    }
    positives = defaultdict(set)
    for qrel in read(raw, "C-MTEB/T2Retrieval-qrels", "data/dev-00000-of-00001-92ed0416056ff7e1.parquet"):
        if qrel["score"] > 0:
            positives[qrel["qid"]].add(qrel["pid"])
    table = pq.read_table(fetch(raw, "C-MTEB/T2Retrieval", "data/corpus-00000-of-00001-8afe7b7a7eca49e3.parquet"))
    ids, texts = table.column("id").to_pylist(), table.column("text").to_pylist()
    text_of = dict(zip(ids, texts))
    sampled = rng.sample(sorted(q for q in positives if q in queries), 100)
    exclude = [positives[q] for q in sampled]
    keep = set().union(*exclude)

    log("t2retrieval: BM25 negatives over the full corpus")
    bm25 = bm25_top(ids, texts, [queries[q] for q in sampled], exclude)

    log(f"t2retrieval: vector negatives from a {pool_size} passage pool")
    pool = rng.sample(ids, pool_size)
    vectors = embed(
        [f"task: search result | text: {clean_passage(text_of[p])[:1200]}" for p in pool]
        + [f"task: search query | text: {queries[q]}" for q in sampled],
        opts.model,
        opts.data_dir / "cache/litert",
    )
    pool_vectors, query_vectors = vectors[: len(pool)], vectors[len(pool) :]
    vector_negs = []
    for qv, skip in zip(query_vectors, exclude):
        order = np.argsort(-(pool_vectors @ qv))
        vector_negs.append([pool[i] for i in order[:100] if pool[i] not in skip][:5])

    for negs in bm25 + vector_negs:
        keep.update(negs)
    log(f"t2retrieval: {len(set().union(*exclude))} positives, {len(keep)} with negatives")
    keep |= set(rng.sample(sorted(set(ids) - keep), max(0, total - len(keep))))

    writer = SetWriter(out, "t2retrieval")
    paths = {pid: writer.file(f"passages/{pid}.txt", clean_passage(text_of[pid])) for pid in sorted(keep)}
    writer.queries("queries.toml", ((queries[q], sorted(paths[p] for p in positives[q]), "zh") for q in sampled))
    (writer.dir / "negatives.json").write_text(
        json.dumps({q: {"bm25": b, "vector": v} for q, b, v in zip(sampled, bm25, vector_negs)}, indent=1)
    )
    writer.finish()


def scifact(raw, out, _opts):
    rng = random.Random(f"{SEED}-scifact")
    corpus = read(raw, "BeIR/scifact", "corpus/corpus-00000-of-00001.parquet")
    queries = {q["_id"]: q["text"] for q in read(raw, "BeIR/scifact", "queries/queries-00000-of-00001.parquet")}
    positives = defaultdict(set)
    lines = fetch(raw, "BeIR/scifact-qrels", "test.tsv").read_text().splitlines()[1:]
    for qid, doc, score in (line.split("\t") for line in lines):
        if int(score) > 0:
            positives[qid].add(doc)
    sampled = rng.sample(sorted(q for q in positives if q in queries), 100)

    writer = SetWriter(out, "scifact")
    paths = {
        row["_id"]: writer.file(f"abstracts/{row['_id']}.txt", f"{row['title']}\n\n{row['text']}\n") for row in corpus
    }
    writer.queries("queries.toml", ((queries[q], sorted(paths[d] for d in positives[q]), "en") for q in sampled))
    writer.finish()


SETS = {"xm3600": xm3600, "miracl_vision": miracl_vision, "t2retrieval": t2retrieval, "scifact": scifact}


def xdg(variable, fallback):
    return Path(os.environ.get(variable) or Path.home() / fallback)


def configured_model():
    """embedding.model from the endex config, if there is one."""
    path = xdg("XDG_CONFIG_HOME", ".config") / "endex/config.toml"
    if not path.exists():
        return None
    return os.path.expanduser(tomllib.loads(path.read_text())["embedding"]["model"])


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("sets", nargs="*", metavar="SET", help=f"default: all of {', '.join(SETS)}")
    parser.add_argument("--data-dir", type=Path, default=xdg("XDG_DATA_HOME", ".local/share") / "endex")
    parser.add_argument("--model", help="EmbeddingGemma .litertlm file (for t2retrieval negatives)")
    parser.add_argument("--force", action="store_true", help="rebuild sets that already exist")
    args = parser.parse_args()
    unknown = set(args.sets) - set(SETS)
    if unknown:
        parser.error(f"unknown set(s): {', '.join(sorted(unknown))}")
    args.model = args.model or configured_model()
    root = args.data_dir / "eval"
    for name in args.sets or SETS:
        if (root / name / "manifest.txt").exists() and not args.force:
            log(f"{name}: ready")
            continue
        log(f"{name}: preparing")
        SETS[name](root / "raw", root, args)


if __name__ == "__main__":
    main()

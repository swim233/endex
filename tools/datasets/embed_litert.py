"""Embed JSON-lines texts with LiteRT-LM; run with the interpreter that has `litert_lm` installed.

Usage: python -I embed_litert.py MODEL CACHE_DIR IN.jsonl OUT.npy
Each input line is {"text": "..."} with the task prefix already applied. Writes L2-normalized
float32 vectors, shape (n, 768); items the engine rejects are reported and left as zeros.
Tries the GPU first and falls back to the CPU.
"""

import json
import sys
import time

import numpy as np
from litert_lm import (
    Backend,
    EmbeddingEngine,
    EmbeddingOptions,
    InputOverflowStrategy,
    LogSeverity,
    set_min_log_severity,
)

# The engine rejects inputs over its largest loaded signature (1024 tokens) unless told to cut.
OPTIONS = EmbeddingOptions(input_overflow_strategy=InputOverflowStrategy.TRUNCATE)


def open_engine(model, cache_dir):
    for backend in (Backend.GPU(), Backend.CPU()):
        try:
            return EmbeddingEngine(model, backend=backend, vision_backend=backend, cache_dir=cache_dir)
        except RuntimeError as error:
            print(f"embed: {backend.get_name()} unavailable ({error})", file=sys.stderr)
    sys.exit("embed: no usable backend")


def main():
    model, cache_dir, src, dst = sys.argv[1:5]
    set_min_log_severity(LogSeverity.ERROR)
    with open(src, encoding="utf-8") as lines:
        texts = [json.loads(line)["text"] for line in lines]
    vectors = np.zeros((len(texts), 768), dtype=np.float32)
    started = time.perf_counter()
    with open_engine(model, cache_dir) as engine:
        for i, text in enumerate(texts):
            try:
                vector = np.asarray(engine.compute_embedding(text, OPTIONS).embedding, dtype=np.float32)
            except RuntimeError as error:
                print(f"embed: item {i} failed ({error}): {text[:200]!r}", file=sys.stderr)
                continue  # stays a zero vector
            vectors[i] = vector / np.linalg.norm(vector)
            if (i + 1) % 1000 == 0:
                rate = (i + 1) / (time.perf_counter() - started)
                print(f"embed: {i + 1}/{len(texts)} ({rate:.0f}/s)", file=sys.stderr)
    np.save(dst, vectors)


if __name__ == "__main__":
    main()

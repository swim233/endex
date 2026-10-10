set shell := ["bash", "-euo", "pipefail", "-c"]

fixtures := justfile_directory() / "tests/fixtures/generated"
endex := justfile_directory() / "target/release/endex"
ocr_release := "https://github.com/GreatV/oar-ocr/releases/download/v0.7.0"

default:
    @just --list

build:
    cargo build --release

test:
    cargo test --workspace

lint:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings

# Everything CI would check.
check: lint test

# Download the PP-OCRv6 small models (Apache-2.0) once into the configured data dir,
# verifying their SHA-256.
models: build
    #!/usr/bin/env bash
    set -euo pipefail
    dir="$("{{endex}}" config ocr_models)"  # also writes the default config on first run
    mkdir -p "$dir"
    while read -r sum name; do
        if ! echo "$sum  $dir/$name" | sha256sum --check --status 2>/dev/null; then
            echo "downloading $name"
            curl -fsSL --retry 3 -o "$dir/$name.part" "{{ocr_release}}/$name"
            echo "$sum  $dir/$name.part" | sha256sum --check --quiet
            mv "$dir/$name.part" "$dir/$name"
        fi
    done <<'EOF'
    d73e0058b7a8086bbd57f3d10b8bcd4ff95363f67e06e2762b5e814fe9c9410e pp-ocrv6_small_det.onnx
    5435fd747c9e0efe15a96d0b378d5bd157e9492ed8fd80edf08f30d02fa24634 pp-ocrv6_small_rec.onnx
    b5f2bfe2bdd9448429e3e82b51c789775d9b42f2403d082b00662eb77e401c5d ppocrv6_dict.txt
    EOF
    echo "OCR models ready in $dir"

# Generate synthetic screenshots, photos, notes and docx files plus eval queries.
fixtures:
    python3 -I scripts/gen_fixtures.py "{{fixtures}}"

# End-to-end run on synthetic fixtures with an isolated index: index, eval, status.
smoke device="gpu": build models fixtures
    "{{endex}}" --data-dir "{{fixtures}}/db" index --rebuild --device {{device}} \
        --manifest "{{fixtures}}/images.txt" "{{fixtures}}/texts.txt"
    "{{endex}}" --data-dir "{{fixtures}}/db" eval "{{fixtures}}/queries.toml"
    "{{endex}}" --data-dir "{{fixtures}}/db" status

# Search the synthetic fixture index built by `just smoke`.
smoke-search query mode="hybrid":
    "{{endex}}" --data-dir "{{fixtures}}/db" search "{{query}}" --mode {{mode}}

# Prepare (once) and evaluate public sets (default: all four), each in its own index.
eval *sets: build models
    #!/usr/bin/env bash
    set -euo pipefail
    sets="{{sets}}"
    sets="${sets:-xm3600 miracl_vision t2retrieval scifact}"
    data_dir="$("{{endex}}" config data_dir)"
    uv run tools/datasets/prepare.py --data-dir "$data_dir" \
        --model "$("{{endex}}" config embedding.model)" $sets
    for set in $sets; do
        dir="$data_dir/eval/$set"
        echo "=== $set: index"
        # LiteRT's OpenCL probe prints "INFO: Loaded OpenCL library with dlopen." on GPU start;
        # neither litert_lm_set_min_log_level nor LiteRT's or TFLite's logger severity hides it.
        "{{endex}}" --data-dir "$dir/db" index --manifest "$dir/manifest.txt" 2>&1 \
            | sed '/^INFO: /d' | tee -a "$dir/index.log"
        for queries in "$dir"/queries*.toml; do
            name="$(basename "$queries" .toml)"
            echo "=== $set: $name"
            "{{endex}}" --data-dir "$dir/db" eval "$queries" | tee "$dir/eval-$name.log"
        done
    done

# Build the popup (target/ui/endex-ui) and run its unit test.
ui:
    cmake -S ui -B target/ui -G Ninja -DCMAKE_BUILD_TYPE=RelWithDebInfo
    cmake --build target/ui
    ctest --test-dir target/ui --output-on-failure

# D-Bus integration test: endexd on the smoke-fixture index, on a private session bus.
dbus-test:
    test -d "{{fixtures}}/db/index" || just smoke
    dbus-run-session -- env ENDEX_PRIVATE_BUS=1 cargo test -p endexd --test dbus -- --ignored

# Install for this user only: binaries to ~/.local/bin, D-Bus services, the endexd user unit.
install: build ui
    #!/usr/bin/env bash
    set -euo pipefail
    bin="$HOME/.local/bin"
    services="${XDG_DATA_HOME:-$HOME/.local/share}/dbus-1/services"
    units="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
    mkdir -p "$bin" "$services" "$units"
    install -m 755 target/release/endex target/release/endexd target/ui/endex-ui "$bin/"
    for f in org.endex.Daemon1.service org.endex.Ui1.service; do
        sed "s|@BINDIR@|$bin|" "dist/$f" > "$services/$f"
    done
    sed "s|@BINDIR@|$bin|" dist/endexd.service > "$units/endexd.service"
    systemctl --user daemon-reload
    systemctl --user try-restart endexd.service
    # A running popup keeps the old binary; the next `endex toggle` starts the new one.
    pkill -x endex-ui || true
    echo "Installed to $bin. Start the daemon at login: systemctl --user enable --now endexd"
    echo "(without that, D-Bus starts it on the first search). Shortcut: see README."

# Remove what `just install` added; config and index stay.
uninstall:
    #!/usr/bin/env bash
    set -euo pipefail
    systemctl --user disable --now endexd.service 2>/dev/null || true
    pkill -x endex-ui || true
    rm -f "$HOME/.local/bin/"{endex,endexd,endex-ui}
    rm -f "${XDG_DATA_HOME:-$HOME/.local/share}/dbus-1/services/"org.endex.{Daemon1,Ui1}.service
    rm -f "${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/endexd.service"
    systemctl --user daemon-reload
    echo "Removed. Config (~/.config/endex) and index (~/.local/share/endex) are kept."

# Evaluate one query file against an index directory.
eval-file queries data_dir: build
    "{{endex}}" --data-dir "{{data_dir}}" eval "{{queries}}"

search query mode="hybrid" limit="10": build
    "{{endex}}" search "{{query}}" --mode {{mode}} --limit {{limit}}

status: build
    "{{endex}}" status

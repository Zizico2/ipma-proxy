set positional-arguments

# List available recipes.
default:
    @just --list

# Run the server, forwarding arguments to the CLI.
run *args:
    cargo run --locked -p ipma-proxy -- "$@"

# Build the development binary and regenerate protobuf bindings.
build:
    cargo build --locked --workspace

# Build the optimized release binary.
release:
    cargo build --locked --workspace --release

# Check all targets without producing binaries.
check:
    cargo check --locked --workspace --all-targets

# Run tests.
test:
    cargo test --locked --workspace --all-targets
    cargo test --locked --workspace --doc

# Format Rust sources.
fmt:
    cargo fmt --all

# Check Rust formatting.
fmt-check:
    cargo fmt --all --check

# Run Clippy with warnings treated as errors.
lint:
    cargo clippy --locked --workspace --all-targets -- -D warnings

# Refresh the vendored Geobuf schema/license from a branch, tag or commit.
update-geobuf ref='master':
    #!/usr/bin/env bash
    set -euo pipefail
    geobuf_tmp=$(mktemp -d)
    trap 'rm -rf -- "$geobuf_tmp"' EXIT
    git init --quiet "$geobuf_tmp/repo"
    git -C "$geobuf_tmp/repo" fetch --quiet --depth=1 -- https://github.com/mapbox/geobuf.git "$1"
    geobuf_revision=$(git -C "$geobuf_tmp/repo" rev-parse FETCH_HEAD)
    git -C "$geobuf_tmp/repo" show "$geobuf_revision:geobuf.proto" > "$geobuf_tmp/geobuf.proto"
    git -C "$geobuf_tmp/repo" show "$geobuf_revision:LICENSE" > "$geobuf_tmp/LICENSE"
    printf '%s\n' "$geobuf_revision" > "$geobuf_tmp/UPSTREAM_REVISION"
    buf build "$geobuf_tmp/geobuf.proto" -o /dev/null
    geobuf_dir=crates/geojson-geobuf/third_party/geobuf
    mkdir -p "$geobuf_dir"
    mv -- "$geobuf_tmp/geobuf.proto" "$geobuf_tmp/LICENSE" "$geobuf_tmp/UPSTREAM_REVISION" "$geobuf_dir/"
    printf 'Updated Geobuf to %s\n' "$geobuf_revision"

# Call Ping on a running server using reflection.
ping address='127.0.0.1:50051':
    buf curl --protocol grpc --http2-prior-knowledge --data '{}' "http://$1/ipma.proxy.v1.ProxyService/Ping"

# Check all supported selections against a running proxy (contacts upstream).
smoke address='http://127.0.0.1:50051':
    cargo run --locked -p ipma-proxy --example smoke -- "$1"

# Verify Geobuf output with Mapbox's pinned JavaScript decoder.
interop:
    cargo build --locked -p geojson-geobuf --example encode_geobuf
    npm --prefix crates/geojson-geobuf/tests/interop ci --ignore-scripts --no-audit --no-fund
    npm --prefix crates/geojson-geobuf/tests/interop test

# Run the local validation suite.
verify: fmt-check check lint test

# Remove Cargo build artifacts.
clean:
    cargo clean

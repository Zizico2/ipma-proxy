set positional-arguments

# List available recipes.
default:
    @just --list

# Run the server, forwarding arguments to the CLI.
run *args:
    cargo run -- "$@"

# Build the development binary and regenerate protobuf bindings.
build:
    cargo build --locked

# Build the optimized release binary.
release:
    cargo build --locked --release

# Check all targets without producing binaries.
check:
    cargo check --locked --all-targets

# Run tests.
test:
    cargo test --locked --all-targets

# Format Rust and protobuf sources.
fmt:
    cargo fmt --all
    buf format -w

# Check Rust and protobuf formatting.
fmt-check:
    cargo fmt --all --check
    buf format --diff --exit-code

# Run Clippy with warnings treated as errors.
lint:
    cargo clippy --locked --all-targets -- -D warnings

# Lint and compile protobuf schemas.
proto-check:
    buf lint
    buf build -o /dev/null

# Check schema compatibility against a Git baseline.
proto-breaking against='.git#branch=main':
    buf breaking --against "$1"

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
    mkdir -p third_party/geobuf
    mv -- "$geobuf_tmp/geobuf.proto" "$geobuf_tmp/LICENSE" "$geobuf_tmp/UPSTREAM_REVISION" third_party/geobuf/
    printf 'Updated Geobuf to %s\n' "$geobuf_revision"

# Call the example Ping RPC on a running server.
ping address='127.0.0.1:50051':
    buf curl --schema proto --protocol grpc --http2-prior-knowledge --data '{}' "http://$1/ipma.proxy.v1.ProxyService/Ping"

# Check all supported selections against a running proxy (contacts upstream).
smoke address='http://127.0.0.1:50051':
    cargo run --locked --example smoke -- "$1"

# Verify Geobuf output with Mapbox's pinned JavaScript decoder.
interop:
    cargo build --locked --example encode_geobuf
    npm --prefix tests/interop ci --ignore-scripts --no-audit --no-fund
    npm --prefix tests/interop test

# Run the local validation suite.
verify: fmt-check check lint test proto-check
    buf breaking --against '.git#ref=HEAD'

# Remove Cargo build artifacts.
clean:
    cargo clean

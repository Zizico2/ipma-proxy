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

# Call the example Ping RPC on a running server.
ping address='127.0.0.1:50051':
    buf curl --schema proto --protocol grpc --http2-prior-knowledge --data '{}' "http://$1/ipma.proxy.v1.ProxyService/Ping"

# Run the local validation suite.
verify: fmt-check check lint test proto-check

# Remove Cargo build artifacts.
clean:
    cargo clean

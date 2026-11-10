# ipma-proxy

Rust gRPC proxy scaffold. Rust is pinned in `rust-toolchain.toml`.

With `just` and the Buf CLI installed, run `just` to list common tasks.
Use `just run` to start the server, `just run --help` for CLI options,
`just ping` to call the running server, and `just verify` for all local checks.
`just fmt` formats Rust and protobuf sources; `just release` builds the optimized binary.

```sh
cp .env.example .env
cargo run
cargo run -- --help
```

Configuration is read from `.env` if present, then from the environment; command-line arguments take precedence. `IPMA_PROXY_LISTEN_ADDR` (`--listen-addr`) sets the gRPC listen address, and `RUST_LOG` (`--log-filter`) controls tracing filters. Existing environment variables take precedence over `.env` values.

The example `ipma.proxy.v1.ProxyService/Ping` RPC returns `pong`. It is a placeholder for the proxy API. For example, with the Buf CLI installed:

```sh
buf curl --schema proto --protocol grpc --http2-prior-knowledge \
  --data '{}' http://127.0.0.1:50051/ipma.proxy.v1.ProxyService/Ping
```

Schemas live under `proto/`. `build.rs` compiles them with protox and generates tonic client and server code during Cargo builds, without requiring `protoc` or the Buf CLI. Add new root schemas to the list in `build.rs`. Buf is used separately to validate schemas:

```sh
buf lint
buf build
# Compare against a committed baseline when one exists:
# buf breaking --against '.git#branch=main'
```

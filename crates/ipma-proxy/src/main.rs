use std::{io::ErrorKind, net::SocketAddr, time::Duration};

use anyhow::Context;
use clap::Parser;
use ipma_proto::{
    FILE_DESCRIPTOR_SET, MAX_MESSAGE_BYTES, v1::proxy_service_server::ProxyServiceServer,
};
use ipma_service::{Proxy, ProxyOptions};
use tonic::{codec::CompressionEncoding, transport::Server};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(version, about = "Cached gRPC proxy for IPMA's documented public API")]
struct Config {
    /// Address on which the gRPC server listens.
    #[arg(
        long,
        env = "IPMA_PROXY_LISTEN_ADDR",
        default_value = "127.0.0.1:50051"
    )]
    listen_addr: SocketAddr,
    /// Absolute upstream directory URL; HTTP is allowed only on loopback.
    #[arg(
        long,
        env = "IPMA_PROXY_UPSTREAM_BASE_URL",
        default_value = "https://api.ipma.pt/open-data/"
    )]
    upstream_base_url: reqwest::Url,
    /// Total timeout for each upstream HTTP request, in seconds.
    #[arg(long, env = "IPMA_PROXY_UPSTREAM_TIMEOUT_SECONDS", default_value = "15", value_parser = clap::value_parser!(u64).range(1..=300))]
    upstream_timeout_seconds: u64,
    /// Approximate cache budget in MiB, including a heap-overhead allowance.
    #[arg(long, env = "IPMA_PROXY_CACHE_MIB", default_value = "64", value_parser = clap::value_parser!(u64).range(1..=65536))]
    cache_mib: u64,
    /// Maximum simultaneous HTTP fetches and CPU conversions.
    #[arg(long, env = "IPMA_PROXY_MAX_CONCURRENT_FETCHES", default_value = "8", value_parser = clap::value_parser!(u32).range(1..=256))]
    max_concurrent_fetches: u32,
    /// Maximum decompressed HTTP body size in MiB (1..32).
    #[arg(long, env = "IPMA_PROXY_MAX_BODY_MIB", default_value = "32", value_parser = clap::value_parser!(u32).range(1..=32))]
    max_body_mib: u32,
    /// Optional upper bound on all dataset cache lifetimes, in seconds.
    #[arg(long, env = "IPMA_PROXY_CACHE_TTL_CAP_SECONDS", value_parser = clap::value_parser!(u64).range(1..=86400))]
    cache_ttl_cap_seconds: Option<u64>,
    /// Tracing filter directives.
    #[arg(
        long,
        env = "RUST_LOG",
        default_value = "ipma_proxy=info,ipma_service=info"
    )]
    log_filter: EnvFilter,
}

fn load_dotenv() -> anyhow::Result<()> {
    match dotenvy::dotenv() {
        Ok(_) => Ok(()),
        Err(dotenvy::Error::Io(error)) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).context("failed to load .env"),
    }
}

async fn shutdown() {
    #[cfg(unix)]
    {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut terminate) => {
                tokio::select! {
                    result = tokio::signal::ctrl_c() => if let Err(error) = result { tracing::error!(%error, "Ctrl-C listener failed"); },
                    _ = terminate.recv() => {},
                }
            }
            Err(error) => {
                tracing::error!(%error, "SIGTERM listener failed");
            }
        }
    }
    #[cfg(not(unix))]
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(%error, "Ctrl-C listener failed");
    }
    tracing::info!("shutting down gRPC server");
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    load_dotenv()?;
    let config = Config::parse();
    tracing_subscriber::fmt()
        .with_env_filter(config.log_filter)
        .init();
    let proxy = Proxy::new(ProxyOptions {
        upstream_base_url: config.upstream_base_url,
        upstream_timeout: Duration::from_secs(config.upstream_timeout_seconds),
        cache_capacity_bytes: config.cache_mib * 1024 * 1024,
        max_concurrent_fetches: config.max_concurrent_fetches as usize,
        max_body_bytes: config.max_body_mib as usize * 1024 * 1024,
        cache_ttl_cap: config.cache_ttl_cap_seconds.map(Duration::from_secs),
    })?;
    let service = ProxyServiceServer::new(proxy)
        .max_decoding_message_size(64 * 1024)
        .max_encoding_message_size(MAX_MESSAGE_BYTES)
        .accept_compressed(CompressionEncoding::Gzip)
        .send_compressed(CompressionEncoding::Gzip);
    let (health, health_service) = tonic_health::server::health_reporter();
    health.set_serving::<ProxyServiceServer<Proxy>>().await;
    let reflection = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)
        .register_encoded_file_descriptor_set(tonic_health::pb::FILE_DESCRIPTOR_SET)
        .build_v1()?;
    tracing::info!(address = %config.listen_addr, "starting gRPC server");
    Server::builder()
        .concurrency_limit_per_connection(64)
        .timeout(Duration::from_secs(config.upstream_timeout_seconds + 15))
        .tcp_nodelay(true)
        .add_service(health_service)
        .add_service(reflection)
        .add_service(service)
        .serve_with_shutdown(config.listen_addr, shutdown())
        .await
        .context("gRPC server failed")?;
    Ok(())
}

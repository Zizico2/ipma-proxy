use std::{io::ErrorKind, net::SocketAddr};

use anyhow::Context;
use clap::Parser;
use tonic::{Request, Response, Status, transport::Server};
use tracing_subscriber::EnvFilter;

mod proto {
    tonic::include_proto!("ipma.proxy.v1");
}

use proto::{
    PingRequest, PingResponse,
    proxy_service_server::{ProxyService, ProxyServiceServer},
};

#[derive(Debug, Parser)]
#[command(version, about = "IPMA gRPC proxy")]
struct Config {
    /// Address on which the gRPC server listens.
    #[arg(
        long,
        env = "IPMA_PROXY_LISTEN_ADDR",
        default_value = "127.0.0.1:50051"
    )]
    listen_addr: SocketAddr,
}

#[derive(Debug, thiserror::Error)]
#[error("failed to load .env: {0}")]
struct DotenvError(#[source] dotenvy::Error);

fn load_dotenv() -> Result<(), DotenvError> {
    match dotenvy::dotenv() {
        Ok(_) => Ok(()),
        Err(dotenvy::Error::Io(error)) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(DotenvError(error)),
    }
}

#[derive(Default)]
struct Proxy;

#[tonic::async_trait]
impl ProxyService for Proxy {
    async fn ping(&self, _request: Request<PingRequest>) -> Result<Response<PingResponse>, Status> {
        Ok(Response::new(PingResponse {
            message: "pong".into(),
        }))
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    load_dotenv()?;
    let config = Config::parse();
    let filter = match std::env::var("RUST_LOG") {
        Ok(value) => EnvFilter::try_new(value).context("invalid RUST_LOG")?,
        Err(std::env::VarError::NotPresent) => EnvFilter::new("ipma_proxy=info"),
        Err(error) => return Err(error).context("invalid RUST_LOG"),
    };
    tracing_subscriber::fmt().with_env_filter(filter).init();

    tracing::info!(address = %config.listen_addr, "starting gRPC server");
    Server::builder()
        .add_service(ProxyServiceServer::new(Proxy))
        .serve_with_shutdown(config.listen_addr, async {
            if let Err(error) = tokio::signal::ctrl_c().await {
                tracing::error!(%error, "failed to listen for Ctrl-C");
            }
        })
        .await
        .context("gRPC server failed")?;
    Ok(())
}

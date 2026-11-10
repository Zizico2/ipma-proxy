//! Transparent gRPC access to IPMA's documented public datasets.
//!
//! Construct [`Proxy`] from [`ProxyOptions`] and register it with
//! [`ipma_proto::v1::proxy_service_server::ProxyServiceServer`]. The caller owns
//! runtime setup, logging configuration, listeners and shutdown.

mod classification;
mod convert;
mod proxy;

pub use proxy::{Proxy, ProxyOptions};

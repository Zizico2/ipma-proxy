//! Protobuf messages, gRPC clients/servers and descriptors for the IPMA API.
//!
//! Consumers can depend on this crate to call the service without linking the
//! upstream HTTP client, cache or GeoJSON encoder.

/// Maximum encoded dataset response size supported by the API (32 MiB).
pub const MAX_MESSAGE_BYTES: usize = 32 * 1024 * 1024;

/// Serialized descriptors for reflection and schema-aware tooling.
pub const FILE_DESCRIPTOR_SET: &[u8] = tonic::include_file_descriptor_set!("ipma_descriptor");

/// The `ipma.proxy.v1` wire contract.
pub mod v1 {
    tonic::include_proto!("ipma.proxy.v1");
}

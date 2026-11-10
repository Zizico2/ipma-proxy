//! Bounded, cached gRPC access to IPMA's documented public datasets.

mod convert;
pub mod geobuf;
pub mod proxy;

pub mod proto {
    tonic::include_proto!("ipma.proxy.v1");
    pub const FILE_DESCRIPTOR_SET: &[u8] = tonic::include_file_descriptor_set!("ipma_descriptor");
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    use prost::Message;
    // Track the directory so changes to imported schemas also trigger a rebuild.
    println!("cargo:rerun-if-changed=proto");
    let descriptors = protox::compile(["proto/ipma/proxy/v1/proxy.proto"], ["proto"])?;
    std::fs::write(
        std::path::PathBuf::from(std::env::var("OUT_DIR")?).join("ipma_descriptor.bin"),
        descriptors.encode_to_vec(),
    )?;
    tonic_prost_build::configure()
        .bytes(".ipma.proxy.v1.GetGeoDatasetResponse.geobuf")
        .compile_fds(descriptors)?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Track the directory so changes to imported schemas also trigger a rebuild.
    println!("cargo:rerun-if-changed=proto");
    let descriptors = protox::compile(["proto/ipma/proxy/v1/proxy.proto"], ["proto"])?;
    tonic_prost_build::configure().compile_fds(descriptors)?;
    Ok(())
}

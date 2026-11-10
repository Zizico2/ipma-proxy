//! Encode GeoJSON from stdin as raw Mapbox Geobuf on stdout.
use std::io::{Read, Write};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let value = serde_json::from_str(&input)?;
    std::io::stdout().write_all(&geojson_geobuf::encode(&value)?)?;
    Ok(())
}

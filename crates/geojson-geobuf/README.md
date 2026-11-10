# geojson-geobuf

Encode a GeoJSON `serde_json::Value` as standalone Mapbox Geobuf bytes. This
library owns coordinate quantization, geometry validation and protobuf wire
encoding. Its only runtime dependencies are `serde_json` and `thiserror`.

```rust
let point = serde_json::json!({
    "type": "Point",
    "coordinates": [-9.1286, 38.7660]
});
let bytes = geojson_geobuf::encode(&point)?;
# Ok::<(), geojson_geobuf::Error>(())
```

Decode the result with Mapbox's JavaScript library:
`geobuf.decode(new Pbf(bytes))`.

The encoder accepts GeoJSON geometry objects, Features and FeatureCollections.
It preserves properties, feature IDs and foreign members. Coordinates use six
decimal places without reprojection, simplification or tiling. Malformed shapes,+mixed coordinate dimensions, empty coordinate arrays, unclosed rings and
coordinates beyond JavaScript's safe integer range return `Error`. Empty
FeatureCollections and GeometryCollections are supported.

From the workspace root, encode stdin to stdout with:

```sh
cargo run -p geojson-geobuf --example encode_geobuf < input.geojson > output.pbf
```

`just interop` checks the output with Mapbox's pinned JavaScript decoder.
`GEOBUF_ENCODER` can select a different encoder executable when running that
JavaScript suite directly (for example, with a custom `CARGO_TARGET_DIR`).

The upstream proto2 schema, ISC license and source revision are in
[`third_party/geobuf`](third_party/geobuf/README.md). Run `just update-geobuf`
from the workspace to refresh them, or pass a tag or commit to pin the update.

# Mapbox Geobuf

`geobuf.proto` and `LICENSE` are vendored unchanged from
https://github.com/mapbox/geobuf. `UPSTREAM_REVISION` records the exact source
commit.

Run `just update-geobuf` to fetch the current upstream `master`, or
`just update-geobuf <tag-or-commit>` for a reproducible revision. The recipe
fetches both files from the same resolved commit, compiles the schema with Buf,
then replaces the vendored files and revision record. Failed downloads or schema
validation leave the existing files intact. Requires Git, Bash and Buf.

Review upstream wire changes against the Rust encoder and run `just interop`.
The interoperability check uses the JavaScript decoder version pinned in
`crates/geojson-geobuf/tests/interop/package.json` from the workspace root.

The schema is an external proto2 wire contract. Keep its tags, defaults, enum
numbers and packed fields intact; it is intentionally outside our Buf module.
The Rust encoder follows the Mapbox coordinate/delta and property encoding
algorithms. The ISC notice applies to those derived portions as well.

Mapbox's encoder omits `Feature.geometry` for null geometries despite the
schema's `required` declaration. Our wire encoder preserves that behavior.
Explicit null feature properties are retained as a custom member, which the
Mapbox decoder supports. Mixed coordinate dimensions, empty coordinate arrays,
unclosed rings, unsafe integer coordinates and unsupported shapes return errors
instead of silently corrupting geometry. Empty FeatureCollections and empty
GeometryCollections are supported. Coordinates use six decimal places; there
is no reprojection, simplification or tiling.

The converter crate's `tests/interop` verifies compatibility with the pinned
Mapbox JavaScript decoder.

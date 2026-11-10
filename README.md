# ipma-proxy

A Rust gRPC proxy for [IPMA's documented public API](https://api.ipma.pt/).
Forecasts, measurements, warnings, seismic events, reference data and climate
CSV files become typed protobuf messages. GeoJSON becomes standalone
[Mapbox Geobuf](https://github.com/mapbox/geobuf) bytes.

Run `just run` (or `cargo run --locked`) to listen on `127.0.0.1:50051`.
Rust is pinned in `rust-toolchain.toml`; protobuf generation uses protox and
requires neither protoc nor Buf. `just release` builds with thin LTO and one
codegen unit. The server supports graceful SIGINT/SIGTERM shutdown, standard
gRPC health checks and v1 server reflection. Configure TLS/authentication at
your ingress when serving beyond a trusted network.

```sh
cp .env.example .env
just run
# In another terminal:
just ping
buf curl --schema proto --protocol grpc --http2-prior-knowledge \
  --data '{"locationId":1110600}' \
  http://127.0.0.1:50051/ipma.proxy.v1.ProxyService/GetWeatherForecast
```

The [versioned schema](proto/ipma/proxy/v1/proxy.proto) defines the complete
contract. Generated Rust clients are exposed through `ipma_proxy::proto`.
Existing `Ping` clients remain compatible.

| RPC | Source / selection |
| --- | --- |
| `GetWeatherForecast` | Five-day city forecast by `location_id`, or all cities for `day_offset` 0..2 |
| `GetWarnings` | All published weather warnings, including upstream green entries |
| `GetObservations` | Rolling hourly station observations, approximately 24 hours |
| `GetSeaForecast` | Coastal forecasts, `day_offset` 0..2 |
| `GetFireRisk` | Municipal fire danger, `day_offset` 0..2 |
| `GetUvForecast` | All UV forecast rows published by IPMA |
| `GetSeismicEvents` | Rolling 30-day catalogue: Azores or mainland/Madeira |
| `GetLocations` | Weather or sea location identifiers and coordinates |
| `GetCodeTable` | Weather types, wind-speed classes or precipitation classes |
| `GetGeoDataset` | Recent observations, station catalogue or bivalve interdictions as Geobuf |
| `GetClimate` | Rolling 20-day municipal evapotranspiration, precipitation or temperature statistics |

Weather requests require a `oneof` selector. Sea/fire requests require explicit
presence of `day_offset`, including zero for today. Unknown and unspecified
request enums return `INVALID_ARGUMENT`. The proxy uses fixed endpoint paths;
clients cannot request arbitrary URLs. Website-only feeds are outside its scope.
The auxiliary weather-icon ZIP remains available directly from IPMA's API page.

Every dataset response carries source attribution, fetch time, expiry time and
an upstream update timestamp when supplied. Fetch time describes the proxy's
retrieval, not the age of the observation. Warning/event times use
`google.protobuf.Timestamp` in UTC; forecast dates use a civil `Date` message.

Numeric strings and JSON numbers map to finite numeric fields with documented
units. Missing/null measurements, empty numeric strings and the `-99` sentinel
become absent optional fields; real zeroes and negative temperatures survive.
Null station records are omitted. Reference tables retain the `-99` code.
Municipality DICO identifiers retain leading zeroes as strings. UV rows with
IPMA's unmapped location ID `0` are retained. Unknown upstream warning categories
remain strings so new categories do not break decoding.

`GetGeoDatasetResponse.geobuf` contains a raw Geobuf `Data` message, not JSON
wrapped in protobuf. Decode it with `geobuf.decode(new Pbf(response.geobuf))`.
The station array is wrapped as a FeatureCollection. Coordinates are rounded to
six decimal places; properties, nested values, feature IDs and foreign members
(including source metadata and missing-value sentinels) are preserved. CRS metadata
is retained without reprojection. The [vendored wire contract and ISC notice](third_party/geobuf/README.md)
describe format constraints. Malformed or unsupported upstream data fails with
`DATA_LOSS` rather than silently changing its meaning.

Run `just update-geobuf` to refresh the vendored schema and license from upstream,
or `just update-geobuf <tag-or-commit>` to select a revision. The resolved commit
is recorded in `third_party/geobuf/UPSTREAM_REVISION`; review changes against the
encoder and run `just interop` afterward.

The shared reqwest client pools HTTPS connections, verifies certificates,
negotiates gzip and refuses redirects. Downloads enforce a size limit while
reading decompressed chunks, including responses without Content-Length.
A semaphore bounds combined HTTP/conversion work, with immediate
`RESOURCE_EXHAUSTED` responses when distinct misses exceed capacity. JSON/CSV
conversion and Geobuf encoding run on blocking workers; cancelling an RPC does
not release a conversion's permit before that worker finishes.

Moka coalesces concurrent misses for the same dataset. Converted responses are
cached, avoiding repeat HTTP calls, JSON/CSV parsing and Geobuf encoding.
Cache weights include a conservative heap-overhead allowance (four times encoded
size plus 1 KiB); the configured budget is approximate, not a process RSS limit.
Entries larger than that budget are served without admission. Errors are never
cached. Eviction and restart may cause earlier refreshes.

| Dataset | Maximum cache lifetime |
| --- | --- |
| Warnings | 60 seconds |
| Location/code tables and station catalogue | 24 hours |
| Climate statistics | 1 hour |
| Other datasets | 10 minutes |

Day-relative forecasts also expire by the next midnight UTC. Lifetimes can be
shortened globally with `--cache-ttl-cap-seconds`; expiry metadata reflects the
actual policy. Requests after expiry fetch fresh data; stale data is not served
on upstream failure. Timeouts return `DEADLINE_EXCEEDED`, upstream 404 returns
`NOT_FOUND`, and upstream rate limiting/server failures return `UNAVAILABLE`.

Configuration comes from CLI arguments, then environment, then optional `.env`.
Existing environment variables take precedence over `.env` values. Run
`just run --help` for options.

| Environment variable | Default |
| --- | --- |
| `IPMA_PROXY_LISTEN_ADDR` | `127.0.0.1:50051` |
| `IPMA_PROXY_UPSTREAM_BASE_URL` | `https://api.ipma.pt/open-data/` |
| `IPMA_PROXY_UPSTREAM_TIMEOUT_SECONDS` | `15` |
| `IPMA_PROXY_CACHE_MIB` | `64` |
| `IPMA_PROXY_MAX_CONCURRENT_FETCHES` | `8` |
| `IPMA_PROXY_MAX_BODY_MIB` | `32` (maximum 32) |
| `IPMA_PROXY_CACHE_TTL_CAP_SECONDS` | Unset; dataset defaults apply |
| `RUST_LOG` | `ipma_proxy=info` |

The upstream base must be an HTTPS directory URL without credentials, query or
fragment. Loopback HTTP is allowed for local tests. gRPC requests are limited to
64 KiB, responses to 32 MiB and concurrent requests per connection to 64. The
server applies a request timeout of upstream timeout plus 15 seconds. gRPC gzip
is available when negotiated by clients.

`just verify` runs formatting, Clippy with warnings denied, all Rust tests, Buf
lint/build and a protobuf compatibility check against committed HEAD. The tests
use local HTTP and real gRPC servers: every dataset mapping, missing-value
semantics, validation, coalescing, cache expiry/admission, cancellation,
upstream failures, timeouts and size limits are checked without IPMA access.

`just interop` builds the encoder example, installs pinned test-only npm packages
with scripts disabled, and checks its output with Mapbox's official decoder.
It exercises every geometry type, holes, 3D coordinates, null geometries,
feature IDs, properties and invalid geometry rejection. Node/npm are only needed
for this interoperability check, not to build or run the proxy.

`just smoke` calls all dataset selections against a running proxy and reports
protobuf sizes plus timings for 100 warm-cache Lisbon RPCs. It contacts the
configured upstream on cache misses, so keep it as an explicit integration check.
The fixtures are small excerpts from IPMA responses retrieved on 2026-11-10,
with synthetic cases added for missing values and validation.

IPMA's API page provides its current usage conditions, attribution requirements
and request to notify `webmaster@ipma.pt` about intended use. Consult those terms
for your application. The proxy does not contact that address automatically.

use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use ipma_proxy::{
    proto::{
        proxy_service_client::ProxyServiceClient, proxy_service_server::ProxyServiceServer, *,
    },
    proxy::{Proxy, ProxyOptions},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::Code;

#[derive(Clone)]
struct Upstream {
    count: Arc<AtomicUsize>,
    status: StatusCode,
    body: Option<&'static str>,
    delay: Duration,
}
struct Harness {
    client: ProxyServiceClient<tonic::transport::Channel>,
    count: Arc<AtomicUsize>,
    tasks: Vec<JoinHandle<()>>,
}
impl Drop for Harness {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

async fn handler(State(state): State<Upstream>, Path(path): Path<String>) -> impl IntoResponse {
    state.count.fetch_add(1, Ordering::SeqCst);
    tokio::time::sleep(state.delay).await;
    let body = state.body.unwrap_or_else(|| match path.as_str() {
        "forecast/meteorology/cities/daily/1110600.json" => include_str!("fixtures/weather-location.json"),
        "forecast/meteorology/cities/daily/hp-daily-forecast-day0.json" => include_str!("fixtures/weather-day.json"),
        "forecast/warnings/warnings_www.json" => include_str!("fixtures/warnings.json"),
        "observation/meteorology/stations/observations.json" => include_str!("fixtures/observations.json"),
        "forecast/oceanography/daily/hp-daily-sea-forecast-day0.json" => include_str!("fixtures/sea.json"),
        "forecast/meteorology/rcm/rcm-d0.json" => include_str!("fixtures/fire.json"),
        "forecast/meteorology/uv/uv.json" => include_str!("fixtures/uv.json"),
        "observation/seismic/7.json" | "observation/seismic/3.json" => include_str!("fixtures/seismic.json"),
        "distrits-islands.json" => include_str!("fixtures/locations.json"),
        "sea-locations.json" => include_str!("fixtures/sea-locations.json"),
        "weather-type-classe.json" => include_str!("fixtures/weather-codes.json"),
        "wind-speed-daily-classe.json" => include_str!("fixtures/wind-codes.json"),
        "precipitation-classe.json" => include_str!("fixtures/precipitation-codes.json"),
        "observation/meteorology/stations/obs-surface.geojson" => include_str!("fixtures/observations.geojson"),
        "observation/meteorology/stations/stations.json" => include_str!("fixtures/stations.json"),
        "observation/biology/bivalves/CI_SNMB.geojson" => r#"{"type":"FeatureCollection","publication_date":"2026-11-09","features":[{"type":"Feature","properties":{"status":"PARTIAL_OPEN","interdictions":{"open":[{"specie_c":"Lapa"}]}},"geometry":{"type":"Polygon","coordinates":[[[-9,38],[-8,38],[-8,39],[-9,38]]]}}]}"#,
        "observation/climate/temperature/t2m-p1d-continental-obssup-idw-concelhos-20d.csv" => include_str!("fixtures/temperature.csv"),
        "observation/climate/evapotranspiration/et0-p1d-continental-obssup-idw-concelhos-20d.csv" => "time,zname,zid,mean_et0,std_et0,min_et0,max_et0,percentile_25_et0,median_et0,percentile_75_et0\n2026-11-09 00:00:00+00:00,Águeda,0101,0,0.2,-99,1.2,0.5,0.8,1\n",
        "observation/climate/precipitation/prec-p1d-continental-obssup-idw-concelhos-20d.csv" => "time,zname,zid,mean_maxprate,std_maxprate,min_maxprate,max_maxprate,percentile_25_maxprate,median_maxprate,percentile_75_maxprate,mean_tp,std_tp,min_tp,max_tp,percentile_25_tp,median_tp,percentile_75_tp\n2026-11-09 00:00:00+00:00,Águeda,0101,1,1,1,1,1,1,1,2,2,2,2,2,2,2\n",
        _ => panic!("unexpected upstream path: {path}"),
    });
    (state.status, body)
}

async fn harness(
    status: StatusCode,
    body: Option<&'static str>,
    delay: Duration,
    limit: usize,
    timeout: Duration,
) -> Harness {
    configured_harness(
        status,
        body,
        delay,
        ProxyOptions {
            max_body_bytes: limit,
            upstream_timeout: timeout,
            max_concurrent_fetches: 1,
            ..Default::default()
        },
    )
    .await
}

async fn configured_harness(
    status: StatusCode,
    body: Option<&'static str>,
    delay: Duration,
    mut options: ProxyOptions,
) -> Harness {
    let count = Arc::new(AtomicUsize::new(0));
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = upstream.local_addr().unwrap();
    let app = Router::new()
        .route("/open-data/{*path}", get(handler))
        .with_state(Upstream {
            count: count.clone(),
            status,
            body,
            delay,
        });
    let http = tokio::spawn(async move {
        axum::serve(upstream, app).await.unwrap();
    });
    options.upstream_base_url = format!("http://{addr}/open-data/").parse().unwrap();
    let proxy = Proxy::new(options).unwrap();
    let grpc_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let grpc_addr = grpc_listener.local_addr().unwrap();
    let grpc = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(ProxyServiceServer::new(proxy))
            .serve_with_incoming(TcpListenerStream::new(grpc_listener))
            .await
            .unwrap();
    });
    let client = ProxyServiceClient::connect(format!("http://{grpc_addr}"))
        .await
        .unwrap();
    Harness {
        client,
        count,
        tasks: vec![http, grpc],
    }
}
async fn normal() -> Harness {
    harness(
        StatusCode::OK,
        None,
        Duration::ZERO,
        4 * 1024 * 1024,
        Duration::from_secs(2),
    )
    .await
}

#[tokio::test]
async fn every_documented_dataset_has_typed_values_and_metadata() {
    let mut h = normal().await;
    assert_eq!(
        h.client
            .ping(PingRequest {})
            .await
            .unwrap()
            .into_inner()
            .message,
        "pong"
    );
    let f = h
        .client
        .get_weather_forecast(GetWeatherForecastRequest {
            target: Some(get_weather_forecast_request::Target::LocationId(1110600)),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(f.forecasts[0].location_id, 1110600);
    assert_eq!(f.forecasts[0].minimum_temperature_celsius, Some(16.5));
    let meta = f.metadata.unwrap();
    assert!(meta.source_url.ends_with("1110600.json"));
    assert_eq!(
        meta.expires_at.unwrap().seconds - meta.fetched_at.unwrap().seconds,
        600
    );
    assert!(meta.upstream_updated_at.is_some());
    let f = h
        .client
        .get_weather_forecast(GetWeatherForecastRequest {
            target: Some(get_weather_forecast_request::Target::DayOffset(0)),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(f.forecasts[0].minimum_temperature_celsius, Some(15.0));
    assert_eq!(f.forecasts[0].date.unwrap().day, 10);
    let warnings = h
        .client
        .get_warnings(GetWarningsRequest {})
        .await
        .unwrap()
        .into_inner();
    assert_eq!(warnings.warnings[0].level, "orange");
    let observations = h
        .client
        .get_observations(GetObservationsRequest {})
        .await
        .unwrap()
        .into_inner()
        .observations;
    assert_eq!(observations.len(), 2);
    assert_eq!(observations[0].temperature_celsius, Some(0.0));
    assert_eq!(observations[0].wind_direction_code, Some(0));
    assert_eq!(observations[0].precipitation_millimeters, None);
    assert_eq!(observations[0].relative_humidity_percent, None);
    assert_eq!(observations[1].temperature_celsius, Some(-2.5));
    let sea = h
        .client
        .get_sea_forecast(GetSeaForecastRequest {
            day_offset: Some(0),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(
        sea.forecasts[0].maximum_significant_wave_height_meters,
        Some(2.5)
    );
    let fire = h
        .client
        .get_fire_risk(GetFireRiskRequest {
            day_offset: Some(0),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(fire.risks[0].municipality_id, "0101");
    assert_eq!(fire.risks[0].risk_class, Some(1));
    assert!(
        !h.client
            .get_uv_forecast(GetUvForecastRequest {})
            .await
            .unwrap()
            .into_inner()
            .forecasts
            .is_empty()
    );
    for region in [1, 2] {
        assert!(
            !h.client
                .get_seismic_events(GetSeismicEventsRequest { region })
                .await
                .unwrap()
                .into_inner()
                .events
                .is_empty()
        );
    }
    for kind in [1, 2] {
        assert!(
            !h.client
                .get_locations(GetLocationsRequest { kind })
                .await
                .unwrap()
                .into_inner()
                .locations
                .is_empty()
        );
    }
    for kind in [1, 2, 3] {
        let codes = h
            .client
            .get_code_table(GetCodeTableRequest { kind })
            .await
            .unwrap()
            .into_inner();
        assert_eq!(codes.entries[0].code, -99);
    }
    for dataset in [1, 2, 3] {
        assert!(
            !h.client
                .get_geo_dataset(GetGeoDatasetRequest { dataset })
                .await
                .unwrap()
                .into_inner()
                .geobuf
                .is_empty()
        );
    }
    for dataset in [1, 2, 3] {
        let climate = h
            .client
            .get_climate(GetClimateRequest { dataset })
            .await
            .unwrap()
            .into_inner();
        assert_eq!(climate.records[0].municipality_id, "0101");
        assert!(!climate.records[0].statistics.is_empty());
        if dataset == 1 {
            assert_eq!(climate.records[0].statistics[0].mean, Some(0.0));
            assert_eq!(climate.records[0].statistics[0].minimum, None);
        }
    }
}

#[tokio::test]
async fn validation_happens_before_any_upstream_fetch() {
    let mut h = normal().await;
    assert_eq!(
        h.client
            .get_weather_forecast(GetWeatherForecastRequest { target: None })
            .await
            .unwrap_err()
            .code(),
        Code::InvalidArgument
    );
    assert_eq!(
        h.client
            .get_weather_forecast(GetWeatherForecastRequest {
                target: Some(get_weather_forecast_request::Target::LocationId(0))
            })
            .await
            .unwrap_err()
            .code(),
        Code::InvalidArgument
    );
    for value in [None, Some(3), Some(u32::MAX)] {
        assert_eq!(
            h.client
                .get_sea_forecast(GetSeaForecastRequest { day_offset: value })
                .await
                .unwrap_err()
                .code(),
            Code::InvalidArgument
        );
        assert_eq!(
            h.client
                .get_fire_risk(GetFireRiskRequest { day_offset: value })
                .await
                .unwrap_err()
                .code(),
            Code::InvalidArgument
        );
    }
    for value in [0, -1, 99] {
        assert_eq!(
            h.client
                .get_locations(GetLocationsRequest { kind: value })
                .await
                .unwrap_err()
                .code(),
            Code::InvalidArgument
        );
        assert_eq!(
            h.client
                .get_code_table(GetCodeTableRequest { kind: value })
                .await
                .unwrap_err()
                .code(),
            Code::InvalidArgument
        );
        assert_eq!(
            h.client
                .get_seismic_events(GetSeismicEventsRequest { region: value })
                .await
                .unwrap_err()
                .code(),
            Code::InvalidArgument
        );
        assert_eq!(
            h.client
                .get_geo_dataset(GetGeoDatasetRequest { dataset: value })
                .await
                .unwrap_err()
                .code(),
            Code::InvalidArgument
        );
        assert_eq!(
            h.client
                .get_climate(GetClimateRequest { dataset: value })
                .await
                .unwrap_err()
                .code(),
            Code::InvalidArgument
        );
    }
    assert_eq!(h.count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn concurrent_cache_misses_coalesce_and_hits_preserve_fetch_time() {
    let h = harness(
        StatusCode::OK,
        None,
        Duration::from_millis(100),
        4096,
        Duration::from_secs(2),
    )
    .await;
    let mut tasks = Vec::new();
    for _ in 0..20 {
        let mut client = h.client.clone();
        tasks.push(tokio::spawn(async move {
            client
                .get_weather_forecast(GetWeatherForecastRequest {
                    target: Some(get_weather_forecast_request::Target::LocationId(1110600)),
                })
                .await
                .unwrap()
                .into_inner()
        }));
    }
    let mut timestamps = Vec::new();
    for task in tasks {
        timestamps.push(task.await.unwrap().metadata.unwrap().fetched_at);
    }
    assert!(timestamps.iter().all(|t| t == &timestamps[0]));
    let mut client = h.client.clone();
    let cached = client
        .get_weather_forecast(GetWeatherForecastRequest {
            target: Some(get_weather_forecast_request::Target::LocationId(1110600)),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(cached.metadata.unwrap().fetched_at, timestamps[0]);
    assert_eq!(h.count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn failed_responses_are_not_cached_and_errors_have_grpc_codes() {
    for (status, body, expected) in [
        (StatusCode::NOT_FOUND, None, Code::NotFound),
        (StatusCode::TOO_MANY_REQUESTS, None, Code::Unavailable),
        (StatusCode::BAD_GATEWAY, None, Code::Unavailable),
        (StatusCode::FOUND, None, Code::Unavailable),
        (
            StatusCode::OK,
            Some("<html>not JSON</html>"),
            Code::DataLoss,
        ),
        (StatusCode::OK, Some("{}"), Code::DataLoss),
        (
            StatusCode::OK,
            Some(
                r#"[{"idAreaAviso":"A","awarenessTypeName":"Rain","awarenessLevelID":"yellow","startTime":"bad","endTime":"bad"}]"#,
            ),
            Code::DataLoss,
        ),
    ] {
        let mut h = harness(status, body, Duration::ZERO, 4096, Duration::from_secs(2)).await;
        for _ in 0..2 {
            assert_eq!(
                h.client
                    .get_warnings(GetWarningsRequest {})
                    .await
                    .unwrap_err()
                    .code(),
                expected
            );
        }
        assert_eq!(h.count.load(Ordering::SeqCst), 2);
    }
}

#[tokio::test]
async fn body_size_and_time_limits_are_enforced() {
    let mut h = harness(
        StatusCode::OK,
        Some("1234567890"),
        Duration::ZERO,
        4,
        Duration::from_secs(2),
    )
    .await;
    assert_eq!(
        h.client
            .get_warnings(GetWarningsRequest {})
            .await
            .unwrap_err()
            .code(),
        Code::ResourceExhausted
    );
    let mut h = harness(
        StatusCode::OK,
        None,
        Duration::from_millis(200),
        4096,
        Duration::from_millis(30),
    )
    .await;
    assert_eq!(
        h.client
            .get_warnings(GetWarningsRequest {})
            .await
            .unwrap_err()
            .code(),
        Code::DeadlineExceeded
    );
}

#[tokio::test]
async fn distinct_misses_over_capacity_fail_without_fetching() {
    let h = harness(
        StatusCode::OK,
        None,
        Duration::from_millis(200),
        4096,
        Duration::from_secs(2),
    )
    .await;
    let mut client = h.client.clone();
    let pending = tokio::spawn(async move {
        client.get_warnings(GetWarningsRequest {}).await.unwrap();
    });
    tokio::time::timeout(Duration::from_secs(1), async {
        while h.count.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let mut client = h.client.clone();
    assert_eq!(
        client
            .get_uv_forecast(GetUvForecastRequest {})
            .await
            .unwrap_err()
            .code(),
        Code::ResourceExhausted
    );
    pending.await.unwrap();
    assert_eq!(h.count.load(Ordering::SeqCst), 1);
}

#[test]
fn upstream_configuration_rejects_credentials_redirect_prone_and_remote_http_urls() {
    for url in [
        "http://api.ipma.pt/open-data/",
        "https://user:secret@api.ipma.pt/open-data/",
        "https://api.ipma.pt/open-data?key=secret",
        "https://api.ipma.pt/open-data/#x",
    ] {
        assert!(
            Proxy::new(ProxyOptions {
                upstream_base_url: url.parse().unwrap(),
                ..Default::default()
            })
            .is_err()
        );
    }
}

#[tokio::test]
async fn expired_entries_are_refetched_instead_of_serving_stale_data() {
    let mut h = configured_harness(
        StatusCode::OK,
        None,
        Duration::ZERO,
        ProxyOptions {
            cache_ttl_cap: Some(Duration::from_millis(50)),
            ..Default::default()
        },
    )
    .await;
    let first = h
        .client
        .get_warnings(GetWarningsRequest {})
        .await
        .unwrap()
        .into_inner();
    tokio::time::sleep(Duration::from_millis(80)).await;
    let second = h
        .client
        .get_warnings(GetWarningsRequest {})
        .await
        .unwrap()
        .into_inner();
    assert_ne!(
        first.metadata.unwrap().fetched_at,
        second.metadata.unwrap().fetched_at
    );
    assert_eq!(h.count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn an_oversize_cache_entry_is_served_but_not_retained() {
    let mut h = configured_harness(
        StatusCode::OK,
        None,
        Duration::ZERO,
        ProxyOptions {
            cache_capacity_bytes: 1,
            ..Default::default()
        },
    )
    .await;
    for _ in 0..2 {
        h.client.get_warnings(GetWarningsRequest {}).await.unwrap();
    }
    assert_eq!(h.count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn cancelled_fetch_does_not_leave_the_key_or_capacity_stuck() {
    let h = harness(
        StatusCode::OK,
        None,
        Duration::from_millis(100),
        4096,
        Duration::from_secs(2),
    )
    .await;
    let mut client = h.client.clone();
    let task = tokio::spawn(async move { client.get_warnings(GetWarningsRequest {}).await });
    tokio::time::timeout(Duration::from_secs(1), async {
        while h.count.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    task.abort();
    let mut client = h.client.clone();
    tokio::time::timeout(
        Duration::from_secs(1),
        client.get_warnings(GetWarningsRequest {}),
    )
    .await
    .unwrap()
    .unwrap();
    tokio::time::timeout(
        Duration::from_secs(1),
        client.get_uv_forecast(GetUvForecastRequest {}),
    )
    .await
    .unwrap()
    .unwrap();
}

#[tokio::test]
async fn chunked_and_gzipped_bodies_cannot_bypass_the_decompressed_size_limit() {
    use axum::{body::Body, http::Response};
    use ipma_proxy::proto::proxy_service_server::ProxyService;
    use std::io::Write;
    for compressed in [false, true] {
        let input = vec![b' '; 10_000];
        let body = if compressed {
            let mut encoder =
                flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(&input).unwrap();
            let encoded = encoder.finish().unwrap();
            assert!(encoded.len() < 128);
            encoded
        } else {
            input
        };
        let app = Router::new().route(
            "/open-data/forecast/warnings/warnings_www.json",
            get(move || {
                let body = body.clone();
                async move {
                    let chunks = body
                        .chunks(8)
                        .map(|v| {
                            Ok::<_, std::convert::Infallible>(bytes::Bytes::copy_from_slice(v))
                        })
                        .collect::<Vec<_>>();
                    let mut response = Response::builder();
                    if compressed {
                        response = response.header("content-encoding", "gzip");
                    }
                    response
                        .body(Body::from_stream(tokio_stream::iter(chunks)))
                        .unwrap()
                }
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let proxy = Proxy::new(ProxyOptions {
            upstream_base_url: format!("http://{address}/open-data/").parse().unwrap(),
            max_body_bytes: 128,
            ..Default::default()
        })
        .unwrap();
        let result = proxy
            .get_warnings(tonic::Request::new(GetWarningsRequest {}))
            .await;
        task.abort();
        assert_eq!(result.unwrap_err().code(), Code::ResourceExhausted);
    }
}

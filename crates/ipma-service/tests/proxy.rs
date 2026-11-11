use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use ipma_proto::v1::{
    proxy_service_client::ProxyServiceClient, proxy_service_server::ProxyServiceServer, *,
};
use ipma_service::{Proxy, ProxyOptions};
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
    assert_eq!(
        f.forecasts[0].location.as_ref().unwrap().value(),
        ForecastLocation::Lisboa1110600
    );
    assert_eq!(
        f.forecasts[0].location.as_ref().unwrap().source_code,
        1110600
    );
    assert_eq!(f.forecasts[0].minimum_temperature_celsius, Some(16.5));
    let first = &f.forecasts[0];
    let weather = first.weather_type.as_ref().unwrap();
    assert_eq!(weather.value(), WeatherType::RainWithPossibleThunderstorms);
    assert_eq!(Some(weather.source_code), first.weather_type_code);
    let wind = first.wind_speed_classification.as_ref().unwrap();
    assert_eq!(wind.value(), WindSpeedClass::Moderate);
    assert_eq!(Some(wind.source_code), first.wind_speed_class);
    let rain = first.precipitation_intensity.as_ref().unwrap();
    assert_eq!(rain.value(), PrecipitationIntensity::Strong);
    assert_eq!(Some(rain.source_code), first.precipitation_intensity_class);
    let direction = first.wind_direction.as_ref().unwrap();
    assert_eq!(direction.value(), CompassDirection::SouthWest);
    assert_eq!(
        direction.source,
        Some(direction_value::Source::Text(
            first.prevailing_wind_direction.clone()
        ))
    );
    let meta = f.metadata.unwrap();
    assert!(meta.source_url.ends_with("1110600.json"));
    assert!(meta.expires_at.is_none());
    assert!(meta.fetched_at.is_some());
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
    assert_eq!(
        warnings.warnings[0].area.as_ref().unwrap().source_value,
        warnings.warnings[0].area_id
    );
    let warning = &warnings.warnings[0];
    assert_eq!(
        warning.warning_level.as_ref().unwrap().value(),
        WarningLevel::Orange
    );
    assert_eq!(
        warning.warning_level.as_ref().unwrap().source_value,
        warning.level
    );
    assert_eq!(
        warning.warning_type.as_ref().unwrap().value(),
        WarningType::Precipitation
    );
    assert_eq!(
        warning.warning_type.as_ref().unwrap().source_value,
        warning.type_name
    );
    let observations = h
        .client
        .get_observations(GetObservationsRequest {})
        .await
        .unwrap()
        .into_inner()
        .observations;
    assert_eq!(observations.len(), 2);
    for observation in &observations {
        assert_eq!(
            observation.station.as_ref().unwrap().source_code,
            observation.station_id
        );
    }
    assert_eq!(observations[0].temperature_celsius, Some(0.0));
    assert_eq!(observations[0].wind_direction_code, Some(0));
    assert_eq!(
        observations[0].wind_direction.as_ref().unwrap().value(),
        CompassDirection::NoDirection
    );
    assert_eq!(
        observations[0].wind_direction.as_ref().unwrap().source,
        Some(direction_value::Source::Code(0))
    );
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
    assert_eq!(
        fire.risks[0].municipality.as_ref().unwrap().value(),
        Municipality::Agueda0101
    );
    assert_eq!(
        fire.risks[0].municipality.as_ref().unwrap().source_code,
        "0101"
    );
    assert_eq!(fire.risks[0].risk_class, Some(1));
    let risk = fire.risks[0].classification.as_ref().unwrap();
    assert_eq!(risk.value(), FireRiskClass::Reduced);
    assert_eq!(Some(risk.source_code), fire.risks[0].risk_class);
    let uv = h
        .client
        .get_uv_forecast(GetUvForecastRequest {})
        .await
        .unwrap()
        .into_inner();
    assert_eq!(
        uv.forecasts[0].period.as_ref().unwrap().value(),
        UvPeriod::Code10
    );
    assert_eq!(
        uv.forecasts[2].location.as_ref().unwrap().value(),
        ForecastLocation::Unspecified
    );
    assert_eq!(uv.forecasts[2].location.as_ref().unwrap().source_code, 0);
    for region in [1, 2] {
        let events = h
            .client
            .get_seismic_events(GetSeismicEventsRequest { region })
            .await
            .unwrap()
            .into_inner()
            .events;
        assert_eq!(
            events[0].magnitude_classification.as_ref().unwrap().value(),
            SeismicMagnitudeType::CodeL
        );
        assert_eq!(
            events[0].reporting_source.as_ref().unwrap().value(),
            SeismicSource::Ipma
        );
        assert!(events[0].intensity.is_none());
    }
    for kind in [1, 2] {
        let locations = h
            .client
            .get_locations(GetLocationsRequest { kind })
            .await
            .unwrap()
            .into_inner()
            .locations;
        assert_eq!(
            locations[0].location.as_ref().unwrap().source_code,
            locations[0].location_id
        );
        assert_eq!(
            locations[0].region.as_ref().unwrap().value(),
            GeographicRegion::Mainland
        );
        if kind == 1 {
            assert_eq!(
                locations[0].municipality.as_ref().unwrap().value(),
                Municipality::Aveiro0105
            );
            assert_eq!(
                locations[0]
                    .municipality
                    .as_ref()
                    .unwrap()
                    .source_local_code,
                Some(5)
            );
            assert_eq!(
                locations[0]
                    .municipality
                    .as_ref()
                    .unwrap()
                    .district
                    .as_ref()
                    .unwrap()
                    .value(),
                District::Aveiro
            );
        } else {
            assert!(locations[0].municipality.is_none());
            assert!(locations[0].coastal_location.is_some());
        }
    }
    assert_eq!(
        sea.forecasts[0].wave_direction.as_ref().unwrap().value(),
        CompassDirection::West
    );
    for kind in [1, 2, 3] {
        let codes = h
            .client
            .get_code_table(GetCodeTableRequest { kind })
            .await
            .unwrap()
            .into_inner();
        assert_eq!(codes.entries[0].code, -99);
        for entry in &codes.entries {
            match (&entry.classification, kind) {
                (Some(code_entry::Classification::WeatherType(value)), 1) => {
                    assert_ne!(*value, WeatherType::Other as i32);
                    assert_eq!(*value == WeatherType::Unspecified as i32, entry.code == -99);
                }
                (Some(code_entry::Classification::WindSpeedClass(value)), 2) => {
                    assert_ne!(*value, WindSpeedClass::Other as i32);
                    assert_eq!(
                        *value == WindSpeedClass::Unspecified as i32,
                        entry.code == -99
                    );
                }
                (Some(code_entry::Classification::PrecipitationIntensity(value)), 3) => {
                    assert_ne!(*value, PrecipitationIntensity::Other as i32);
                    assert_eq!(
                        *value == PrecipitationIntensity::Unspecified as i32,
                        entry.code == -99
                    );
                    if entry.code == 0 {
                        assert_eq!(*value, PrecipitationIntensity::NoPrecipitation as i32);
                    }
                }
                _ => panic!("reference row has wrong categorical kind"),
            }
            assert!(!entry.description_portuguese.is_empty());
            assert!(!entry.description_english.is_empty());
        }
    }
    for dataset in [1, 2, 3] {
        let response = h
            .client
            .get_geo_dataset(GetGeoDatasetRequest { dataset })
            .await
            .unwrap()
            .into_inner();
        assert!(!response.geobuf.is_empty());
        assert!(!response.feature_categories.is_empty());
        assert_eq!(response.feature_categories[0].feature_index, 0);
        if dataset != 3 {
            let Some(geo_feature_categories::Categories::Station(station)) =
                &response.feature_categories[0].categories
            else {
                panic!("lost station reference");
            };
            assert_ne!(station.station.as_ref().unwrap().value(), Station::Other);
            assert!(station.name.is_some());
        }
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
        assert_eq!(
            climate.records[0].municipality.as_ref().unwrap().value(),
            Municipality::Agueda0101
        );
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
async fn repeated_and_concurrent_calls_each_fetch_upstream() {
    let mut h = configured_harness(
        StatusCode::OK,
        None,
        Duration::from_millis(20),
        ProxyOptions {
            max_concurrent_fetches: 32,
            ..Default::default()
        },
    )
    .await;
    for _ in 0..2 {
        let response = h
            .client
            .get_warnings(GetWarningsRequest {})
            .await
            .unwrap()
            .into_inner();
        assert!(response.metadata.unwrap().expires_at.is_none());
    }
    let mut tasks = Vec::new();
    for _ in 0..20 {
        let mut client = h.client.clone();
        tasks.push(tokio::spawn(async move {
            client.get_warnings(GetWarningsRequest {}).await.unwrap();
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    assert_eq!(h.count.load(Ordering::SeqCst), 22);
}

#[tokio::test]
async fn upstream_errors_are_mapped_and_each_call_is_forwarded() {
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
async fn over_capacity_calls_fail_without_fetching() {
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
async fn cancelled_fetch_releases_capacity() {
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
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            match client.get_warnings(GetWarningsRequest {}).await {
                Ok(_) => break,
                Err(status) if status.code() == Code::ResourceExhausted => {
                    tokio::time::sleep(Duration::from_millis(5)).await
                }
                Err(status) => panic!("unexpected status: {status}"),
            }
        }
    })
    .await
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
    use ipma_proto::v1::proxy_service_server::ProxyService;
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

#[tokio::test]
async fn unknown_and_missing_categories_are_lossless_over_grpc() {
    let mut h = harness(StatusCode::OK, Some(r#"{"globalIdLocal":1110600,"forecastDate":"2026-11-10","data":[{"idWeatherType":777,"classWindSpeed":"999","classPrecInt":0,"predWindDir":"VARIABLE"},{"idWeatherType":null,"classWindSpeed":-99,"classPrecInt":null,"predWindDir":""},{"idWeatherType":0}]}"#), Duration::ZERO, 4096, Duration::from_secs(2)).await;
    let forecasts = h
        .client
        .get_weather_forecast(GetWeatherForecastRequest {
            target: Some(get_weather_forecast_request::Target::LocationId(1110600)),
        })
        .await
        .unwrap()
        .into_inner()
        .forecasts;
    let future = &forecasts[0];
    assert_eq!(future.weather_type_code, Some(777));
    assert_eq!(
        future.weather_type.as_ref().unwrap().value(),
        WeatherType::Other
    );
    assert_eq!(future.weather_type.as_ref().unwrap().source_code, 777);
    assert_eq!(future.wind_speed_class, Some(999));
    assert_eq!(
        future.wind_speed_classification.as_ref().unwrap().value(),
        WindSpeedClass::Other
    );
    assert_eq!(
        future
            .wind_speed_classification
            .as_ref()
            .unwrap()
            .source_code,
        999
    );
    assert_eq!(future.precipitation_intensity_class, Some(0));
    assert_eq!(
        future.precipitation_intensity.as_ref().unwrap().value(),
        PrecipitationIntensity::NoPrecipitation
    );
    assert_eq!(
        future.wind_direction.as_ref().unwrap().value(),
        CompassDirection::Other
    );
    assert_eq!(
        future.wind_direction.as_ref().unwrap().source,
        Some(direction_value::Source::Text("VARIABLE".into()))
    );
    assert!(forecasts[1].weather_type.is_none());
    assert!(forecasts[1].wind_speed_classification.is_none());
    assert!(forecasts[1].precipitation_intensity.is_none());
    assert!(forecasts[1].wind_direction.is_none());
    assert_eq!(
        forecasts[2].weather_type.as_ref().unwrap().value(),
        WeatherType::NoInformation
    );

    let mut h = harness(StatusCode::OK, Some(r#"[{"awarenessTypeName":"Novo fenómeno","awarenessLevelID":"purple","idAreaAviso":"FUT","text":"Future alert","startTime":"2026-11-10T12:00:00","endTime":"2026-11-10T13:00:00"}]"#), Duration::ZERO, 4096, Duration::from_secs(2)).await;
    let warnings = h
        .client
        .get_warnings(GetWarningsRequest {})
        .await
        .unwrap()
        .into_inner()
        .warnings;
    assert_eq!(
        warnings[0].warning_type.as_ref().unwrap().value(),
        WarningType::Other
    );
    assert_eq!(
        warnings[0].warning_type.as_ref().unwrap().source_value,
        "Novo fenómeno"
    );
    assert_eq!(
        warnings[0].warning_level.as_ref().unwrap().value(),
        WarningLevel::Other
    );
    assert_eq!(
        warnings[0].warning_level.as_ref().unwrap().source_value,
        "purple"
    );
    assert_eq!(warnings[0].area_id, "FUT");

    let mut h = harness(StatusCode::OK, Some(r#"{"dataPrev":"2026-11-10","dataRun":"2026-11-09","local":{"0101":{"data":{"rcm":999}}}}"#), Duration::ZERO, 4096, Duration::from_secs(2)).await;
    let risks = h
        .client
        .get_fire_risk(GetFireRiskRequest {
            day_offset: Some(0),
        })
        .await
        .unwrap()
        .into_inner()
        .risks;
    assert_eq!(risks[0].municipality_id, "0101");
    assert_eq!(
        risks[0].classification.as_ref().unwrap().value(),
        FireRiskClass::Other
    );
    assert_eq!(risks[0].classification.as_ref().unwrap().source_code, 999);
}

#[tokio::test]
async fn station_direction_aliases_keep_their_original_codes() {
    let mut h = harness(StatusCode::OK, Some(r#"{"2026-11-10T12:00":{"1001":{"idDireccVento":1},"1002":{"idDireccVento":9},"1003":{"idDireccVento":99},"1004":{"idDireccVento":-99}}}"#), Duration::ZERO, 4096, Duration::from_secs(2)).await;
    let observations = h
        .client
        .get_observations(GetObservationsRequest {})
        .await
        .unwrap()
        .into_inner()
        .observations;
    for (index, code) in [(0, 1), (1, 9)] {
        let direction = observations[index].wind_direction.as_ref().unwrap();
        assert_eq!(direction.value(), CompassDirection::North);
        assert_eq!(direction.source, Some(direction_value::Source::Code(code)));
    }
    assert_eq!(
        observations[2].wind_direction.as_ref().unwrap().value(),
        CompassDirection::Other
    );
    assert_eq!(
        observations[2].wind_direction.as_ref().unwrap().source,
        Some(direction_value::Source::Code(99))
    );
    assert!(observations[3].wind_direction.is_none());
}

#[tokio::test]
async fn municipality_components_stay_scoped_and_unknown_identifiers_survive() {
    let mut h = harness(StatusCode::OK, Some(r#"{"data":[
      {"globalIdLocal":1010100,"local":"Águeda","idDistrito":1,"idConcelho":1},
      {"globalIdLocal":1020100,"local":"Aljustrel","idDistrito":2,"idConcelho":1},
      {"globalIdLocal":9999999,"local":"Future","idDistrito":99,"idConcelho":1,"idRegiao":99,"idAreaAviso":"FUTURE"},
      {"globalIdLocal":9999998,"local":"Partial","idConcelho":1}
    ]}"#), Duration::ZERO, 10000, Duration::from_secs(2)).await;
    let rows = h
        .client
        .get_locations(GetLocationsRequest { kind: 1 })
        .await
        .unwrap()
        .into_inner()
        .locations;
    let first = rows[0].municipality.as_ref().unwrap();
    let second = rows[1].municipality.as_ref().unwrap();
    assert_eq!(first.value(), Municipality::Agueda0101);
    assert_eq!(second.value(), Municipality::Aljustrel0201);
    assert_eq!(first.source_local_code, second.source_local_code);
    assert_ne!(first.source_code, second.source_code);
    let future = &rows[2];
    assert_eq!(
        future.location.as_ref().unwrap().value(),
        ForecastLocation::Other
    );
    assert_eq!(future.location.as_ref().unwrap().source_code, 9999999);
    assert_eq!(
        future.municipality.as_ref().unwrap().value(),
        Municipality::Other
    );
    assert_eq!(future.municipality.as_ref().unwrap().source_code, "9901");
    assert_eq!(future.district.as_ref().unwrap().source_code, 99);
    assert_eq!(
        future.region.as_ref().unwrap().value(),
        GeographicRegion::Other
    );
    assert_eq!(future.warning_area.as_ref().unwrap().source_value, "FUTURE");
    let partial = rows[3].municipality.as_ref().unwrap();
    assert_eq!(partial.source_code, "");
    assert_eq!(partial.source_local_code, Some(1));
    assert!(partial.district.is_none());
    assert_eq!(h.count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn uv_internal_codes_keep_zero_unknown_and_missing_distinct() {
    let mut h = harness(StatusCode::OK, Some(r#"[
      {"globalIdLocal":0,"data":"2026-11-10","idPeriodo":0,"intervaloHora":"14h-14h"},
      {"globalIdLocal":9999999,"data":"2026-11-10","idPeriodo":999,"intervaloHora":"future interval"},
      {"globalIdLocal":0,"data":"2026-11-10","idPeriodo":-99}
    ]"#), Duration::ZERO, 10000, Duration::from_secs(2)).await;
    let rows = h
        .client
        .get_uv_forecast(GetUvForecastRequest {})
        .await
        .unwrap()
        .into_inner()
        .forecasts;
    assert_eq!(rows[0].period.as_ref().unwrap().value(), UvPeriod::Code0);
    assert_eq!(rows[0].period.as_ref().unwrap().source_code, 0);
    assert_eq!(
        rows[0].period.as_ref().unwrap().peak_hour_interval,
        "14h-14h"
    );
    assert_eq!(rows[1].period.as_ref().unwrap().value(), UvPeriod::Other);
    assert_eq!(rows[1].period.as_ref().unwrap().source_code, 999);
    assert_eq!(
        rows[1].location.as_ref().unwrap().value(),
        ForecastLocation::Other
    );
    assert!(rows[2].period.is_none());
}

#[tokio::test]
async fn geobuf_companions_preserve_indices_source_tokens_and_original_bytes() {
    const BODY: &str = r#"{"type":"FeatureCollection","features":[
      {"type":"Feature","geometry":null,"properties":null},
      {"type":"Feature","geometry":null,"properties":{"samp":"L1","zone_type":"LITORAL","region_name":"Norte","status":"CLOSED","interdictions":{"open":[{"specie_s":"Patella spp.","specie_c":"Lapa","classification":"B*"}]}}},
      {"type":"Feature","geometry":null,"properties":{"code":"FUTURE","status":"CLOSE","zone_type":"FUTURE","region_name":"New region","extra":{"untouched":true},"interdictions":{"pending":[{"specie_s":"Future taxon","specie_c":"Future label","classification":"D"}]}}}
    ]}"#;
    let mut h = harness(
        StatusCode::OK,
        Some(BODY),
        Duration::ZERO,
        10000,
        Duration::from_secs(2),
    )
    .await;
    let result = h
        .client
        .get_geo_dataset(GetGeoDatasetRequest { dataset: 3 })
        .await
        .unwrap()
        .into_inner();
    let root: serde_json::Value = serde_json::from_str(BODY).unwrap();
    assert_eq!(
        result.geobuf.as_ref(),
        geojson_geobuf::encode(&root).unwrap()
    );
    assert_eq!(result.feature_categories.len(), 2);
    assert_eq!(result.feature_categories[0].feature_index, 1);
    assert_eq!(result.feature_categories[1].feature_index, 2);
    let zone = |entry: &GeoFeatureCategories| match entry.categories.as_ref().unwrap() {
        geo_feature_categories::Categories::BivalveZone(zone) => zone.clone(),
        _ => panic!("wrong feature category"),
    };
    let first = zone(&result.feature_categories[0]);
    assert_eq!(first.zone.as_ref().unwrap().value(), BivalveZone::L1);
    assert_eq!(
        first.status.as_ref().unwrap().value(),
        BivalveZoneStatus::Closed
    );
    let species = &first.species_permissions[0];
    assert_eq!(
        species.species.as_ref().unwrap().value(),
        BivalveSpecies::PatellaSpp
    );
    assert_eq!(species.species.as_ref().unwrap().common_name, "Lapa");
    assert_eq!(
        species.sanitary_class.as_ref().unwrap().value(),
        BivalveSanitaryClass::BStar
    );
    assert_eq!(species.sanitary_class.as_ref().unwrap().source_value, "B*");
    let future = zone(&result.feature_categories[1]);
    assert_eq!(
        future.status.as_ref().unwrap().value(),
        BivalveZoneStatus::Close
    );
    assert_eq!(future.zone.as_ref().unwrap().value(), BivalveZone::Other);
    assert_eq!(future.zone.as_ref().unwrap().source_value, "FUTURE");
    assert_eq!(
        future.species_permissions[0]
            .permission
            .as_ref()
            .unwrap()
            .value(),
        BivalveHarvestPermission::Other
    );
    assert_eq!(
        future.species_permissions[0]
            .permission
            .as_ref()
            .unwrap()
            .source_value,
        "pending"
    );
    assert_eq!(
        future.species_permissions[0]
            .species
            .as_ref()
            .unwrap()
            .source_value,
        "Future taxon"
    );
    assert_eq!(
        future.species_permissions[0]
            .sanitary_class
            .as_ref()
            .unwrap()
            .source_value,
        "D"
    );
    assert_eq!(h.count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn seismic_reference_tokens_are_lossless_without_semantic_inference() {
    let mut h = harness(StatusCode::OK, Some(r#"{"data":[
      {"time":"2026-11-10T00:00:00","magType":"W","source":"GFZ","degree":"III/IV"},
      {"time":"2026-11-10T01:00:00","magType":"FUTURE","source":"New source","degree":"Future intensity"},
      {"time":"2026-11-10T02:00:00","magType":"","source":"","degree":null}
    ]}"#), Duration::ZERO, 10000, Duration::from_secs(2)).await;
    let rows = h
        .client
        .get_seismic_events(GetSeismicEventsRequest { region: 1 })
        .await
        .unwrap()
        .into_inner()
        .events;
    assert_eq!(
        rows[0].magnitude_classification.as_ref().unwrap().value(),
        SeismicMagnitudeType::CodeW
    );
    assert_eq!(
        rows[0].reporting_source.as_ref().unwrap().value(),
        SeismicSource::Gfz
    );
    assert_eq!(
        rows[0].intensity.as_ref().unwrap().value(),
        SeismicDegree::IiiToIv
    );
    assert_eq!(rows[0].intensity.as_ref().unwrap().source_value, "III/IV");
    assert_eq!(
        rows[1].magnitude_classification.as_ref().unwrap().value(),
        SeismicMagnitudeType::Other
    );
    assert_eq!(
        rows[1]
            .magnitude_classification
            .as_ref()
            .unwrap()
            .source_value,
        "FUTURE"
    );
    assert_eq!(
        rows[1].reporting_source.as_ref().unwrap().value(),
        SeismicSource::Other
    );
    assert_eq!(
        rows[1].reporting_source.as_ref().unwrap().source_value,
        "New source"
    );
    assert_eq!(
        rows[1].intensity.as_ref().unwrap().value(),
        SeismicDegree::Other
    );
    assert_eq!(
        rows[1].intensity.as_ref().unwrap().source_value,
        "Future intensity"
    );
    assert!(rows[2].magnitude_classification.is_none());
    assert!(rows[2].reporting_source.is_none());
    assert!(rows[2].intensity.is_none());
    assert_eq!(h.count.load(Ordering::SeqCst), 1);
}

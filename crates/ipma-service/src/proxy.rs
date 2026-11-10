use std::{
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};

use bytes::BytesMut;
use ipma_proto::{
    MAX_MESSAGE_BYTES,
    v1::{self as pb, proxy_service_server::ProxyService},
};
use moka::{Expiry, future::Cache};
use reqwest::{Client, Url};
use tokio::sync::Semaphore;
use tonic::{Request, Response, Status};

use crate::convert::{self, Payload};

#[derive(Debug, Clone)]
pub struct ProxyOptions {
    pub upstream_base_url: Url,
    pub upstream_timeout: Duration,
    pub max_body_bytes: usize,
    pub cache_capacity_bytes: u64,
    pub max_concurrent_fetches: usize,
    /// Optional upper bound on dataset-specific cache lifetimes.
    pub cache_ttl_cap: Option<Duration>,
}

impl Default for ProxyOptions {
    fn default() -> Self {
        Self {
            upstream_base_url: Url::parse("https://api.ipma.pt/open-data/").expect("static URL"),
            upstream_timeout: Duration::from_secs(15),
            max_body_bytes: MAX_MESSAGE_BYTES,
            cache_capacity_bytes: 64 * 1024 * 1024,
            max_concurrent_fetches: 8,
            cache_ttl_cap: None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("upstream request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("upstream returned HTTP {0}")]
    HttpStatus(u16),
    #[error("upstream body or converted response exceeds size limit")]
    TooLarge,
    #[error("proxy fetch capacity exhausted")]
    Busy,
    #[error("invalid upstream data: {0}")]
    Invalid(String),
    #[error("conversion worker failed: {0}")]
    Worker(#[from] tokio::task::JoinError),
}

impl Error {
    fn status(&self) -> Status {
        match self {
            Self::Http(e) if e.is_timeout() => Status::deadline_exceeded("IPMA request timed out"),
            Self::Http(_) | Self::HttpStatus(429 | 500..=599) => {
                Status::unavailable("IPMA is unavailable")
            }
            Self::HttpStatus(404) => Status::not_found("IPMA dataset was not found"),
            Self::HttpStatus(_) => Status::unavailable("IPMA rejected the upstream request"),
            Self::Busy | Self::TooLarge => Status::resource_exhausted(self.to_string()),
            Self::Invalid(_) => Status::data_loss("IPMA returned invalid or unsupported data"),
            Self::Worker(_) => Status::internal("dataset conversion failed"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Key {
    WeatherLocation(u32),
    WeatherDay(u32),
    Warnings,
    Observations,
    Sea(u32),
    Fire(u32),
    Uv,
    Seismic(u32),
    Locations(i32),
    Codes(i32),
    Geo(i32),
    Climate(i32),
}

impl Key {
    pub(crate) fn path(self) -> String {
        match self {
            Self::WeatherLocation(id) => format!("forecast/meteorology/cities/daily/{id}.json"),
            Self::WeatherDay(day) => format!("forecast/meteorology/cities/daily/hp-daily-forecast-day{day}.json"),
            Self::Warnings => "forecast/warnings/warnings_www.json".into(),
            Self::Observations => "observation/meteorology/stations/observations.json".into(),
            Self::Sea(day) => format!("forecast/oceanography/daily/hp-daily-sea-forecast-day{day}.json"),
            Self::Fire(day) => format!("forecast/meteorology/rcm/rcm-d{day}.json"),
            Self::Uv => "forecast/meteorology/uv/uv.json".into(),
            Self::Seismic(area) => format!("observation/seismic/{area}.json"),
            Self::Locations(1) => "distrits-islands.json".into(),
            Self::Locations(_) => "sea-locations.json".into(),
            Self::Codes(1) => "weather-type-classe.json".into(),
            Self::Codes(2) => "wind-speed-daily-classe.json".into(),
            Self::Codes(_) => "precipitation-classe.json".into(),
            Self::Geo(1) => "observation/meteorology/stations/obs-surface.geojson".into(),
            Self::Geo(2) => "observation/meteorology/stations/stations.json".into(),
            Self::Geo(_) => "observation/biology/bivalves/CI_SNMB.geojson".into(),
            Self::Climate(1) => "observation/climate/evapotranspiration/et0-p1d-continental-obssup-idw-concelhos-20d.csv".into(),
            Self::Climate(2) => "observation/climate/precipitation/prec-p1d-continental-obssup-idw-concelhos-20d.csv".into(),
            Self::Climate(_) => "observation/climate/temperature/t2m-p1d-continental-obssup-idw-concelhos-20d.csv".into(),
        }
    }

    pub(crate) fn ttl(self) -> Duration {
        Duration::from_secs(match self {
            Self::Warnings => 60,
            Self::Locations(_) | Self::Codes(_) | Self::Geo(2) => 86400,
            Self::Climate(_) => 3600,
            _ => 600,
        })
    }
}

struct CacheExpiry;

#[derive(Debug)]
enum LoadError {
    Failed(Error),
    // Share the initializer's result with waiters without admitting a jumbo entry.
    Uncacheable(Arc<Payload>),
}
impl Expiry<Key, Arc<Payload>> for CacheExpiry {
    fn expire_after_create(&self, _: &Key, value: &Arc<Payload>, _: Instant) -> Option<Duration> {
        // Use the advertised absolute expiry; conversion time never extends TTL.
        Some(value.remaining_ttl())
    }
}

/// Clones share the HTTP connection pool, bounded cache and fetch permits.
#[derive(Clone)]
pub struct Proxy {
    client: Client,
    options: ProxyOptions,
    cache: Cache<Key, Arc<Payload>>,
    permits: Arc<Semaphore>,
}

impl Proxy {
    pub fn new(options: ProxyOptions) -> anyhow::Result<Self> {
        let url = &options.upstream_base_url;
        anyhow::ensure!(
            url.scheme() == "https" || (url.scheme() == "http" && is_loopback(url)),
            "upstream must use HTTPS (HTTP allowed only for loopback tests)"
        );
        anyhow::ensure!(
            url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none()
                && url.path().ends_with('/'),
            "upstream base must be an absolute directory URL without credentials, query or fragment"
        );
        anyhow::ensure!(
            options.max_body_bytes > 0 && options.max_body_bytes <= MAX_MESSAGE_BYTES,
            "body limit must be in 1..={MAX_MESSAGE_BYTES}"
        );
        anyhow::ensure!(
            options.max_concurrent_fetches > 0
                && options.cache_capacity_bytes > 0
                && !options.upstream_timeout.is_zero(),
            "resource limits and timeout must be positive"
        );
        anyhow::ensure!(
            options.max_concurrent_fetches <= 256
                && options.upstream_timeout <= Duration::from_secs(300),
            "fetch concurrency must not exceed 256 and timeout must not exceed 300 seconds"
        );
        anyhow::ensure!(
            !options.cache_ttl_cap.is_some_and(|ttl| ttl.is_zero()),
            "cache TTL cap must be positive"
        );
        let client = Client::builder()
            .user_agent(concat!(
                env!("CARGO_PKG_NAME"),
                "/",
                env!("CARGO_PKG_VERSION")
            ))
            .connect_timeout(options.upstream_timeout.min(Duration::from_secs(5)))
            .timeout(options.upstream_timeout)
            .redirect(reqwest::redirect::Policy::none())
            .pool_max_idle_per_host(options.max_concurrent_fetches)
            .tcp_nodelay(true)
            .gzip(true)
            .build()?;
        // Encoded size is an approximation; account conservatively for heap structures.
        let cache = Cache::builder()
            .max_capacity(options.cache_capacity_bytes)
            .weigher(|_: &Key, value: &Arc<Payload>| value.weight())
            .expire_after(CacheExpiry)
            .build();
        let permits = Arc::new(Semaphore::new(options.max_concurrent_fetches));
        Ok(Self {
            client,
            options,
            cache,
            permits,
        })
    }

    async fn get(&self, key: Key) -> Result<Arc<Payload>, Status> {
        // Moka runs one initializer per key; concurrent misses share its result.
        match self
            .cache
            .try_get_with(key, async {
                let payload = Arc::new(self.fetch(key).await.map_err(LoadError::Failed)?);
                if u64::from(payload.weight()) > self.options.cache_capacity_bytes {
                    Err(LoadError::Uncacheable(payload))
                } else {
                    Ok(payload)
                }
            })
            .await
        {
            Ok(payload) => Ok(payload),
            Err(error) => match error.as_ref() {
                LoadError::Uncacheable(payload) => Ok(payload.clone()),
                LoadError::Failed(error) => {
                    tracing::warn!(path = %key.path(), %error, "dataset request failed");
                    Err(error.status())
                }
            },
        }
    }

    async fn fetch(&self, key: Key) -> Result<Payload, Error> {
        let permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)?;
        let url = self
            .options
            .upstream_base_url
            .join(&key.path())
            .map_err(|e| Error::Invalid(e.to_string()))?;
        let started = Instant::now();
        let mut response = self.client.get(url.clone()).send().await?;
        if !response.status().is_success() {
            return Err(Error::HttpStatus(response.status().as_u16()));
        }
        let limit = self.options.max_body_bytes;
        if response.content_length().is_some_and(|n| n > limit as u64) {
            return Err(Error::TooLarge);
        }
        let mut body = BytesMut::with_capacity(
            response.content_length().unwrap_or(8192).min(limit as u64) as usize,
        );
        while let Some(chunk) = response.chunk().await? {
            if chunk.len() > limit - body.len() {
                return Err(Error::TooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        let fetched = SystemTime::now();
        let mut ttl = key.ttl();
        if let Some(cap) = self.options.cache_ttl_cap {
            ttl = ttl.min(cap);
        }
        if matches!(key, Key::WeatherDay(_) | Key::Sea(_) | Key::Fire(_)) {
            // A cached 'today' must not become yesterday after midnight UTC.
            let since_epoch = fetched
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_err(|e| Error::Invalid(e.to_string()))?;
            let until_midnight = Duration::from_secs(86400)
                - Duration::new(since_epoch.as_secs() % 86400, since_epoch.subsec_nanos());
            ttl = ttl.min(until_midnight);
        }
        let metadata = pb::SourceMetadata {
            source_url: url.into(),
            fetched_at: Some(fetched.into()),
            expires_at: Some((fetched + ttl).into()),
            upstream_updated_at: None,
            attribution: "IPMA, I.P. — https://www.ipma.pt".into(),
        };
        let body = body.freeze();
        let payload = tokio::task::spawn_blocking(move || {
            // Hold the permit during CPU work, even if the requesting RPC is cancelled.
            let _permit = permit;
            let payload = convert::decode(key, &body, metadata)?;
            if payload.encoded_len() > MAX_MESSAGE_BYTES {
                return Err(Error::TooLarge);
            }
            Ok(payload)
        })
        .await??;
        tracing::info!(path = %key.path(), elapsed_ms = started.elapsed().as_millis(), response_bytes = payload.encoded_len(), "dataset refreshed");
        Ok(payload)
    }
}

fn is_loopback(url: &Url) -> bool {
    url.host_str().is_some_and(|host| {
        host == "localhost"
            || host
                .trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    })
}

fn day(value: Option<u32>) -> Result<u32, Status> {
    value
        .filter(|n| *n <= 2)
        .ok_or_else(|| Status::invalid_argument("day_offset is required and must be 0..2"))
}
fn choice(value: i32, max: i32, field: &str) -> Result<i32, Status> {
    if (1..=max).contains(&value) {
        Ok(value)
    } else {
        Err(Status::invalid_argument(format!(
            "{field} must be a specified, supported enum value"
        )))
    }
}

// Keep cached payload types and RPC responses checked together at compile time.
macro_rules! respond {
    ($self:ident, $key:expr, $variant:ident) => {
        match $self.get($key).await?.as_ref() {
            Payload::$variant(value) => Ok(Response::new(value.clone())),
            _ => Err(Status::internal("cache payload type mismatch")),
        }
    };
}

#[tonic::async_trait]
impl ProxyService for Proxy {
    async fn ping(
        &self,
        _: Request<pb::PingRequest>,
    ) -> Result<Response<pb::PingResponse>, Status> {
        Ok(Response::new(pb::PingResponse {
            message: "pong".into(),
        }))
    }
    async fn get_weather_forecast(
        &self,
        req: Request<pb::GetWeatherForecastRequest>,
    ) -> Result<Response<pb::GetWeatherForecastResponse>, Status> {
        use pb::get_weather_forecast_request::Target;
        let key = match req.into_inner().target {
            Some(Target::LocationId(id)) if id > 0 => Key::WeatherLocation(id),
            Some(Target::DayOffset(n)) => Key::WeatherDay(day(Some(n))?),
            _ => {
                return Err(Status::invalid_argument(
                    "select a positive location_id or day_offset 0..2",
                ));
            }
        };
        respond!(self, key, Weather)
    }
    async fn get_warnings(
        &self,
        _: Request<pb::GetWarningsRequest>,
    ) -> Result<Response<pb::GetWarningsResponse>, Status> {
        respond!(self, Key::Warnings, Warnings)
    }
    async fn get_observations(
        &self,
        _: Request<pb::GetObservationsRequest>,
    ) -> Result<Response<pb::GetObservationsResponse>, Status> {
        respond!(self, Key::Observations, Observations)
    }
    async fn get_sea_forecast(
        &self,
        req: Request<pb::GetSeaForecastRequest>,
    ) -> Result<Response<pb::GetSeaForecastResponse>, Status> {
        respond!(self, Key::Sea(day(req.into_inner().day_offset)?), Sea)
    }
    async fn get_fire_risk(
        &self,
        req: Request<pb::GetFireRiskRequest>,
    ) -> Result<Response<pb::GetFireRiskResponse>, Status> {
        respond!(self, Key::Fire(day(req.into_inner().day_offset)?), Fire)
    }
    async fn get_uv_forecast(
        &self,
        _: Request<pb::GetUvForecastRequest>,
    ) -> Result<Response<pb::GetUvForecastResponse>, Status> {
        respond!(self, Key::Uv, Uv)
    }
    async fn get_seismic_events(
        &self,
        req: Request<pb::GetSeismicEventsRequest>,
    ) -> Result<Response<pb::GetSeismicEventsResponse>, Status> {
        let region = choice(req.into_inner().region, 2, "region")?;
        respond!(self, Key::Seismic(if region == 1 { 3 } else { 7 }), Seismic)
    }
    async fn get_locations(
        &self,
        req: Request<pb::GetLocationsRequest>,
    ) -> Result<Response<pb::GetLocationsResponse>, Status> {
        respond!(
            self,
            Key::Locations(choice(req.into_inner().kind, 2, "kind")?),
            Locations
        )
    }
    async fn get_code_table(
        &self,
        req: Request<pb::GetCodeTableRequest>,
    ) -> Result<Response<pb::GetCodeTableResponse>, Status> {
        respond!(
            self,
            Key::Codes(choice(req.into_inner().kind, 3, "kind")?),
            Codes
        )
    }
    async fn get_geo_dataset(
        &self,
        req: Request<pb::GetGeoDatasetRequest>,
    ) -> Result<Response<pb::GetGeoDatasetResponse>, Status> {
        respond!(
            self,
            Key::Geo(choice(req.into_inner().dataset, 3, "dataset")?),
            Geo
        )
    }
    async fn get_climate(
        &self,
        req: Request<pb::GetClimateRequest>,
    ) -> Result<Response<pb::GetClimateResponse>, Status> {
        respond!(
            self,
            Key::Climate(choice(req.into_inner().dataset, 3, "dataset")?),
            Climate
        )
    }
}

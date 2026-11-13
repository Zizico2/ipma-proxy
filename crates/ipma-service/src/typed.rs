//! Typed generated upstream models mapped explicitly into the stable gRPC contract.
use crate::{
    classification,
    convert::{Payload, parse_date, parse_timestamp},
    proxy::{Error, Key},
};
use ipma_open_data_client::types as up;
use ipma_proto::v1::*;
type Result<T> = std::result::Result<T, Error>;
fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}
fn parse<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T> {
    serde_json::from_slice(body).map_err(|e| invalid(e.to_string()))
}
trait SourceNumber {
    fn source_number(&self) -> Result<Option<f64>>;
}
fn numeric_string(s: &str) -> Result<Option<f64>> {
    if s.trim().is_empty() {
        return Ok(None);
    }
    s.trim()
        .parse()
        .map(Some)
        .map_err(|_| invalid("numeric string"))
}
impl SourceNumber for up::NumericValue {
    fn source_number(&self) -> Result<Option<f64>> {
        match self {
            Self::Variant0(n) => Ok(*n),
            Self::Variant1(s) => numeric_string(s),
        }
    }
}
impl SourceNumber for up::CodeValue {
    fn source_number(&self) -> Result<Option<f64>> {
        match self {
            Self::Variant0(n) => Ok(n.map(|n| n as f64)),
            Self::Variant1(s) => numeric_string(s),
        }
    }
}
fn number<T: SourceNumber>(value: &Option<T>) -> Result<Option<f64>> {
    let n = value
        .as_ref()
        .map(SourceNumber::source_number)
        .transpose()?
        .flatten();
    if n.is_some_and(|n| !n.is_finite()) {
        return Err(invalid("non-finite number"));
    }
    Ok(n.filter(|n| *n != -99.0))
}
fn integer(value: &Option<up::CodeValue>) -> Result<Option<i32>> {
    number(value)?
        .map(|n| {
            if n.fract() == 0.0 && n >= i32::MIN as f64 && n <= i32::MAX as f64 {
                Ok(n as i32)
            } else {
                Err(invalid("integer"))
            }
        })
        .transpose()
}
fn unsigned(value: &Option<up::CodeValue>) -> Result<Option<u32>> {
    number(value)?
        .map(|n| {
            if n.fract() == 0.0 && n >= 0.0 && n <= u32::MAX as f64 {
                Ok(n as u32)
            } else {
                Err(invalid("identifier"))
            }
        })
        .transpose()
}
trait SourceId {
    fn source_id(&self) -> Option<u64>;
}
impl SourceId for u64 {
    fn source_id(&self) -> Option<u64> {
        Some(*self)
    }
}
impl SourceId for Option<u64> {
    fn source_id(&self) -> Option<u64> {
        *self
    }
}
impl SourceId for std::num::NonZeroU64 {
    fn source_id(&self) -> Option<u64> {
        Some(self.get())
    }
}
fn id(n: &impl SourceId) -> Result<u32> {
    n.source_id()
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| *n > 0)
        .ok_or_else(|| invalid("invalid identifier"))
}
fn text(value: &Option<String>) -> String {
    value.clone().unwrap_or_default()
}
trait SourceText {
    fn source_text(&self) -> Option<String>;
}
impl SourceText for up::SeismicEventDegree {
    fn source_text(&self) -> Option<String> {
        match self {
            Self::Variant0(v) => v.clone(),
            Self::Variant1(v) => Some(v.to_string()),
        }
    }
}
impl SourceText for up::SeismicEventLocal {
    fn source_text(&self) -> Option<String> {
        match self {
            Self::Variant0(v) => v.clone(),
            Self::Variant1(v) => Some(v.to_string()),
        }
    }
}
fn optional_text<T: SourceText>(v: &Option<T>) -> Option<String> {
    v.as_ref().and_then(SourceText::source_text)
}
fn timestamp(value: Option<&str>) -> Result<Option<prost_types::Timestamp>> {
    value
        .filter(|s| !s.is_empty())
        .map(parse_timestamp)
        .transpose()
}
fn required_timestamp(value: Option<&str>) -> Result<Option<prost_types::Timestamp>> {
    timestamp(value)?
        .map(Some)
        .ok_or_else(|| invalid("missing timestamp"))
}
fn metadata(mut metadata: SourceMetadata, update: Option<&str>) -> Result<Option<SourceMetadata>> {
    metadata.upstream_updated_at = timestamp(update)?;
    Ok(Some(metadata))
}
fn coordinates(
    lat: &Option<up::NumericValue>,
    lon: &Option<up::NumericValue>,
) -> Result<Option<Coordinates>> {
    match (number(lat)?, number(lon)?) {
        (Some(latitude), Some(longitude))
            if (-90.0..=90.0).contains(&latitude) && (-180.0..=180.0).contains(&longitude) =>
        {
            Ok(Some(Coordinates {
                latitude,
                longitude,
            }))
        }
        (None, None) => Ok(None),
        _ => Err(invalid("invalid coordinates")),
    }
}
pub(crate) fn decode(key: Key, body: &[u8], metadata_value: SourceMetadata) -> Result<Payload> {
    Ok(match key {
        Key::WeatherLocation(_) | Key::WeatherDay(_) => {
            let (rows, location_id, fallback_date, update) = match key {
                Key::WeatherLocation(_) => {
                    let root: up::CityDailyForecastResponse = parse(body)?;
                    let fallback = root
                        .extra
                        .get("forecastDate")
                        .and_then(|v| v.as_str())
                        .map(str::to_owned);
                    (
                        root.data,
                        Some(root.global_id_local.get()),
                        fallback,
                        root.data_update,
                    )
                }
                _ => {
                    let root: up::DailyForecastResponse = parse(body)?;
                    (root.data, None, Some(root.forecast_date), root.data_update)
                }
            };
            let meta = metadata(metadata_value, update.as_deref())?;
            let mut forecasts = Vec::new();
            for v in &rows {
                let weather_type_code = integer(&v.id_weather_type)?;
                let wind_speed_class = integer(&v.class_wind_speed)?;
                let precipitation_intensity_class = integer(&v.class_prec_int)?;
                let prevailing_wind_direction = text(&v.pred_wind_dir);
                let location_id = id(&location_id.or(v.global_id_local))?;
                forecasts.push(WeatherForecast {
                    location: classification::location(Some(location_id)),
                    location_id,
                    date: Some(parse_date(
                        v.forecast_date
                            .as_deref()
                            .or(fallback_date.as_deref())
                            .ok_or_else(|| invalid("missing forecast date"))?,
                    )?),
                    coordinates: coordinates(&v.latitude, &v.longitude)?,
                    minimum_temperature_celsius: number(&v.t_min)?,
                    maximum_temperature_celsius: number(&v.t_max)?,
                    precipitation_probability_percent: number(&v.precipita_prob)?,
                    weather_type: classification::weather_type(weather_type_code),
                    wind_speed_classification: classification::wind_speed(wind_speed_class),
                    precipitation_intensity: classification::precipitation(
                        precipitation_intensity_class,
                    ),
                    wind_direction: classification::direction_text(&prevailing_wind_direction),
                    prevailing_wind_direction,
                    weather_type_code,
                    wind_speed_class,
                    precipitation_intensity_class,
                });
            }
            Payload::Weather(GetWeatherForecastResponse {
                metadata: meta,
                forecasts,
            })
        }
        Key::Warnings => {
            let root: Vec<up::Warning> = parse(body)?;
            let meta = metadata(metadata_value, None)?;
            let warnings = root
                .iter()
                .map(|v| {
                    let type_name = v.awareness_type_name.clone();
                    let level = v.awareness_level_id.clone();
                    let area_id = v.id_area_aviso.clone();
                    Ok(Warning {
                        area: classification::warning_area(&area_id),
                        area_id,
                        warning_type: classification::warning_type(&type_name),
                        warning_level: classification::warning_level(&level),
                        type_name,
                        level,
                        text: text(&v.text),
                        starts_at: required_timestamp(Some(&v.start_time))?,
                        ends_at: required_timestamp(Some(&v.end_time))?,
                    })
                })
                .collect::<Result<_>>()?;
            Payload::Warnings(GetWarningsResponse {
                metadata: meta,
                warnings,
            })
        }
        Key::Observations => {
            let root: up::ObservationsResponse = parse(body)?;
            let meta = metadata(metadata_value, None)?;
            let mut observations = Vec::new();
            let times: std::collections::BTreeMap<_, _> = root.iter().collect();
            for (time, stations) in times {
                let observed_at = Some(parse_timestamp(time)?);
                let stations: std::collections::BTreeMap<_, _> = stations.iter().collect();
                for (station, row) in stations {
                    let Some(v) = row.0.as_ref() else { continue };
                    let station_id = station
                        .parse::<u32>()
                        .ok()
                        .filter(|id| *id > 0)
                        .ok_or_else(|| invalid("invalid station ID"))?;
                    let wind_direction_code = integer(&v.id_direcc_vento)?;
                    observations.push(Observation {
                        station: classification::station(Some(station_id)),
                        station_id,
                        observed_at,
                        temperature_celsius: number(&v.temperatura)?,
                        wind_speed_meters_per_second: number(&v.intensidade_vento)?,
                        wind_speed_kilometers_per_hour: number(&v.intensidade_vento_km)?,
                        wind_direction: classification::direction_code(wind_direction_code),
                        wind_direction_code,
                        precipitation_millimeters: number(&v.prec_acumulada)?,
                        relative_humidity_percent: number(&v.humidade)?,
                        sea_level_pressure_hectopascals: number(&v.pressao)?,
                        solar_radiation_kilojoules_per_square_meter: number(&v.radiacao)?,
                    });
                }
            }
            Payload::Observations(GetObservationsResponse {
                metadata: meta,
                observations,
            })
        }
        Key::Sea(_) => {
            let root: up::SeaForecastResponse = parse(body)?;
            let meta = metadata(metadata_value, root.data_update.as_deref())?;
            let forecasts = root
                .data
                .iter()
                .map(|v| {
                    let prevailing_wave_direction = text(&v.pred_wave_dir);
                    let location_id = id(&v.global_id_local)?;
                    Ok(SeaForecast {
                        location: classification::location(Some(location_id)),
                        location_id,
                        coordinates: coordinates(&v.latitude, &v.longitude)?,
                        minimum_wave_period_seconds: number(&v.wave_period_min)?,
                        maximum_wave_period_seconds: number(&v.wave_period_max)?,
                        minimum_swell_height_meters: number(&v.wave_high_min)?,
                        maximum_swell_height_meters: number(&v.wave_high_max)?,
                        minimum_significant_wave_height_meters: number(&v.total_sea_min)?,
                        maximum_significant_wave_height_meters: number(&v.total_sea_max)?,
                        minimum_sea_surface_temperature_celsius: number(&v.sst_min)?,
                        maximum_sea_surface_temperature_celsius: number(&v.sst_max)?,
                        wave_direction: classification::direction_text(&prevailing_wave_direction),
                        prevailing_wave_direction,
                    })
                })
                .collect::<Result<_>>()?;
            Payload::Sea(GetSeaForecastResponse {
                metadata: meta,
                date: Some(parse_date(&root.forecast_date)?),
                forecasts,
            })
        }
        Key::Fire(_) => {
            let root: up::FireRiskResponse = parse(body)?;
            let meta = metadata(metadata_value, root.file_date.as_deref())?;
            let municipalities: std::collections::BTreeMap<_, _> = root.local.iter().collect();
            let risks = municipalities
                .into_iter()
                .map(|(municipality_id, v)| {
                    let risk_class = integer(&Some(v.data.rcm.clone()))?;
                    Ok(FireRisk {
                        municipality: classification::municipality(municipality_id, None, None),
                        municipality_id: municipality_id.clone(),
                        coordinates: coordinates(&v.latitude, &v.longitude)?,
                        classification: classification::fire_risk(risk_class),
                        risk_class,
                    })
                })
                .collect::<Result<_>>()?;
            Payload::Fire(GetFireRiskResponse {
                metadata: meta,
                forecast_date: Some(parse_date(&root.data_prev)?),
                model_run_date: Some(parse_date(&root.data_run)?),
                risks,
            })
        }
        Key::Uv => {
            let root: Vec<up::UvForecast> = parse(body)?;
            let meta = metadata(metadata_value, None)?;
            let forecasts = root
                .iter()
                .map(|v| {
                    let location_id = u32::try_from(v.global_id_local)
                        .map_err(|_| invalid("invalid UV location"))?;
                    let period_code = integer(&v.id_periodo)?;
                    let peak_hour_interval = text(&v.intervalo_hora);
                    Ok(UvForecast {
                        location: classification::location(Some(location_id)),
                        period: classification::uv_period(period_code, &peak_hour_interval),
                        location_id,
                        date: Some(parse_date(&v.data)?),
                        index: number(&v.i_uv)?,
                        peak_hour_interval,
                        period_code,
                    })
                })
                .collect::<Result<_>>()?;
            Payload::Uv(GetUvForecastResponse {
                metadata: meta,
                forecasts,
            })
        }
        Key::Seismic(_) => {
            let root: up::SeismicResponse = parse(body)?;
            let meta = metadata(
                metadata_value,
                root.data_update.as_deref().or(root.update_date.as_deref()),
            )?;
            let events = root
                .data
                .iter()
                .map(|v| {
                    let sensed = v.sensed;
                    let magnitude_type = text(&v.mag_type);
                    let degree = optional_text(&v.degree);
                    let source = text(&v.source);
                    Ok(SeismicEvent {
                        magnitude_classification: classification::magnitude_type(&magnitude_type),
                        intensity: classification::seismic_degree(
                            degree.as_deref().unwrap_or_default(),
                        ),
                        reporting_source: classification::seismic_source(&source),
                        occurred_at: required_timestamp(v.time.as_deref())?,
                        coordinates: coordinates(&v.lat, &v.lon)?,
                        magnitude: number(&v.magnitud)?,
                        magnitude_type,
                        depth_kilometers: number(&v.depth)?,
                        observation_region: text(&v.obs_region),
                        locality: optional_text(&v.local),
                        degree,
                        source,
                        shakemap_id: text(&v.shakemapid),
                        updated_at: timestamp(v.data_update.as_deref())?,
                        event_id: v.sismo_id.clone(),
                        sensed,
                        google_map_reference: v.googlemapref.clone(),
                        tensor_reference: v.tensor_ref.clone(),
                    })
                })
                .collect::<Result<_>>()?;
            Payload::Seismic(GetSeismicEventsResponse {
                metadata: meta,
                events,
            })
        }
        Key::Locations(kind) => {
            let (rows, update) = if kind == 1 {
                let root: up::LocationsResponse = parse(body)?;
                (root.data, root.data_update)
            } else {
                (parse::<Vec<up::Location>>(body)?, None)
            };
            let meta = metadata(metadata_value, update.as_deref())?;
            let locations = rows
                .iter()
                .map(|v| {
                    let district_id = unsigned(&v.id_distrito)?;
                    let municipality_code = unsigned(&v.id_concelho)?;
                    // idConcelho is scoped by idDistrito. Only compose a DICO
                    // when both components are present; retain partial components.
                    let dico = match (district_id, municipality_code) {
                        (Some(d), Some(c)) => format!("{d:02}{c:02}"),
                        _ => String::new(),
                    };
                    Ok(Location {
                        location: classification::location(Some(id(&v.global_id_local)?)),
                        warning_area: classification::warning_area(&text(&v.id_area_aviso)),
                        region: classification::region(unsigned(&v.id_regiao)?),
                        district: classification::district(district_id),
                        municipality: classification::municipality(
                            &dico,
                            district_id,
                            municipality_code,
                        ),
                        coastal_location: classification::coastal_location(unsigned(&v.id_local)?),
                        location_id: id(&v.global_id_local)?,
                        name: v.local.clone(),
                        coordinates: coordinates(&v.latitude, &v.longitude)?,
                        warning_area_id: text(&v.id_area_aviso),
                        region_id: unsigned(&v.id_regiao)?,
                        district_id,
                        municipality_code,
                        local_code: unsigned(&v.id_local)?,
                    })
                })
                .collect::<Result<_>>()?;
            Payload::Locations(GetLocationsResponse {
                metadata: meta,
                locations,
            })
        }
        Key::Codes(kind) => {
            let (rows, update) = match kind {
                1 => {
                    let root: up::WeatherCodeResponse = parse(body)?;
                    (
                        root.data
                            .into_iter()
                            .map(|v| {
                                (
                                    v.id_weather_type,
                                    v.desc_weather_type_pt,
                                    v.desc_weather_type_en,
                                )
                            })
                            .collect::<Vec<_>>(),
                        root.data_update,
                    )
                }
                2 => {
                    let root: up::WindCodeResponse = parse(body)?;
                    (
                        root.data
                            .into_iter()
                            .map(|v| {
                                (
                                    v.class_wind_speed,
                                    v.desc_class_wind_speed_daily_pt
                                        .or(v.desc_class_wind_speed_pt),
                                    v.desc_class_wind_speed_daily_en
                                        .or(v.desc_class_wind_speed_en),
                                )
                            })
                            .collect::<Vec<_>>(),
                        root.data_update,
                    )
                }
                _ => {
                    let root: up::PrecipitationCodeResponse = parse(body)?;
                    (
                        root.data
                            .into_iter()
                            .map(|v| {
                                (
                                    v.class_prec_int,
                                    v.desc_class_prec_int_pt,
                                    v.desc_class_prec_int_en,
                                )
                            })
                            .collect::<Vec<_>>(),
                        root.data_update,
                    )
                }
            };
            let entries = rows
                .into_iter()
                .map(|(code, pt, en)| {
                    let n = code
                        .source_number()?
                        .ok_or_else(|| invalid("missing reference code"))?;
                    if n.fract() != 0.0 || n < i32::MIN as f64 || n > i32::MAX as f64 {
                        return Err(invalid("invalid reference code"));
                    }
                    Ok(CodeEntry {
                        code: n as i32,
                        classification: Some(classification::code_entry_classification(
                            kind, n as i32,
                        )),
                        description_portuguese: pt
                            .ok_or_else(|| invalid("missing Portuguese code description"))?,
                        description_english: en
                            .ok_or_else(|| invalid("missing English code description"))?,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            Payload::Codes(GetCodeTableResponse {
                metadata: metadata(metadata_value, update.as_deref())?,
                entries,
            })
        }
        Key::Geo(_) | Key::Climate(_) => unreachable!("handled by the raw-data converter"),
    })
}

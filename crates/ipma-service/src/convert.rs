use chrono::{Datelike, NaiveDate, NaiveDateTime};
use ipma_proto::v1::*;
use prost::Message;
use serde_json::Value;

use crate::proxy::{Error, Key};

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub(crate) enum Payload {
    Weather(GetWeatherForecastResponse),
    Warnings(GetWarningsResponse),
    Observations(GetObservationsResponse),
    Sea(GetSeaForecastResponse),
    Fire(GetFireRiskResponse),
    Uv(GetUvForecastResponse),
    Seismic(GetSeismicEventsResponse),
    Locations(GetLocationsResponse),
    Codes(GetCodeTableResponse),
    Geo(GetGeoDatasetResponse),
    Climate(GetClimateResponse),
}
impl Payload {
    fn metadata(&self) -> Option<&SourceMetadata> {
        match self {
            Self::Weather(v) => v.metadata.as_ref(),
            Self::Warnings(v) => v.metadata.as_ref(),
            Self::Observations(v) => v.metadata.as_ref(),
            Self::Sea(v) => v.metadata.as_ref(),
            Self::Fire(v) => v.metadata.as_ref(),
            Self::Uv(v) => v.metadata.as_ref(),
            Self::Seismic(v) => v.metadata.as_ref(),
            Self::Locations(v) => v.metadata.as_ref(),
            Self::Codes(v) => v.metadata.as_ref(),
            Self::Geo(v) => v.metadata.as_ref(),
            Self::Climate(v) => v.metadata.as_ref(),
        }
    }
    pub(crate) fn remaining_ttl(&self) -> std::time::Duration {
        self.metadata()
            .and_then(|m| m.expires_at)
            .and_then(|t| std::time::SystemTime::try_from(t).ok())
            .and_then(|t| t.duration_since(std::time::SystemTime::now()).ok())
            .unwrap_or_default()
    }
    pub(crate) fn encoded_len(&self) -> usize {
        match self {
            Self::Weather(v) => v.encoded_len(),
            Self::Warnings(v) => v.encoded_len(),
            Self::Observations(v) => v.encoded_len(),
            Self::Sea(v) => v.encoded_len(),
            Self::Fire(v) => v.encoded_len(),
            Self::Uv(v) => v.encoded_len(),
            Self::Seismic(v) => v.encoded_len(),
            Self::Locations(v) => v.encoded_len(),
            Self::Codes(v) => v.encoded_len(),
            Self::Geo(v) => v.encoded_len(),
            Self::Climate(v) => v.encoded_len(),
        }
    }
    pub(crate) fn weight(&self) -> u32 {
        self.encoded_len()
            .saturating_mul(4)
            .saturating_add(1024)
            .min(u32::MAX as usize) as u32
    }
}

fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}
fn array(v: &Value) -> Result<&Vec<Value>> {
    v.as_array().ok_or_else(|| invalid("expected array"))
}
fn object(v: &Value) -> Result<&serde_json::Map<String, Value>> {
    v.as_object().ok_or_else(|| invalid("expected object"))
}
fn string(v: &Value) -> Result<String> {
    v.as_str()
        .map(str::to_owned)
        .ok_or_else(|| invalid("expected string"))
}
fn text(v: &Value, key: &str) -> Result<String> {
    if v[key].is_null() {
        Ok(String::new())
    } else {
        string(&v[key])
    }
}
fn optional_text(v: &Value, key: &str) -> Result<Option<String>> {
    match &v[key] {
        Value::Null => Ok(None),
        Value::String(s) => Ok(Some(s.clone())),
        Value::Number(n) => Ok(Some(n.to_string())),
        _ => Err(invalid(format!("invalid text field {key}"))),
    }
}
fn numeric(v: &Value) -> Result<Option<f64>> {
    let n = match v {
        Value::Null => return Ok(None),
        Value::String(s) if s.trim().is_empty() => return Ok(None),
        Value::String(s) => s
            .parse::<f64>()
            .map_err(|_| invalid("invalid numeric string"))?,
        Value::Number(n) => n.as_f64().ok_or_else(|| invalid("invalid number"))?,
        _ => return Err(invalid("expected number or numeric string")),
    };
    if !n.is_finite() {
        return Err(invalid("non-finite number"));
    }
    Ok(Some(n))
}
fn number(v: &Value, key: &str) -> Result<Option<f64>> {
    Ok(numeric(&v[key])?.filter(|n| *n != -99.0))
}
fn integer(v: &Value, key: &str) -> Result<Option<i32>> {
    number(v, key)?
        .map(|n| {
            if n.fract() == 0.0 && n >= f64::from(i32::MIN) && n <= f64::from(i32::MAX) {
                Ok(n as i32)
            } else {
                Err(invalid(format!("invalid integer field {key}")))
            }
        })
        .transpose()
}
fn unsigned(v: &Value, key: &str) -> Result<Option<u32>> {
    number(v, key)?
        .map(|n| {
            if n.fract() == 0.0 && n >= 0.0 && n <= f64::from(u32::MAX) {
                Ok(n as u32)
            } else {
                Err(invalid(format!("invalid unsigned field {key}")))
            }
        })
        .transpose()
}
fn id(v: &Value, key: &str) -> Result<u32> {
    unsigned(v, key)?
        .filter(|n| *n > 0)
        .ok_or_else(|| invalid(format!("missing identifier {key}")))
}
fn coordinates(v: &Value, lat: &str, lon: &str) -> Result<Option<Coordinates>> {
    match (number(v, lat)?, number(v, lon)?) {
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
fn date(v: &Value) -> Result<Date> {
    parse_date(v.as_str().ok_or_else(|| invalid("missing date"))?)
}
fn parse_date(s: &str) -> Result<Date> {
    let d = NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| invalid("invalid date"))?;
    Ok(Date {
        year: d.year(),
        month: d.month(),
        day: d.day(),
    })
}
fn timestamp(v: &Value) -> Result<Option<prost_types::Timestamp>> {
    match v {
        Value::Null => Ok(None),
        Value::String(s) if s.is_empty() => Ok(None),
        Value::String(s) => parse_timestamp(s).map(Some),
        _ => Err(invalid("invalid timestamp")),
    }
}
fn parse_timestamp(s: &str) -> Result<prost_types::Timestamp> {
    if let Ok(t) = chrono::DateTime::parse_from_rfc3339(s) {
        return Ok(prost_types::Timestamp {
            seconds: t.timestamp(),
            nanos: t.timestamp_subsec_nanos() as i32,
        });
    }
    for format in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M:%S",
    ] {
        if let Ok(t) = NaiveDateTime::parse_from_str(s, format) {
            let t = t.and_utc();
            return Ok(prost_types::Timestamp {
                seconds: t.timestamp(),
                nanos: t.timestamp_subsec_nanos() as i32,
            });
        }
    }
    Err(invalid("invalid timestamp"))
}
fn required_timestamp(v: &Value) -> Result<Option<prost_types::Timestamp>> {
    timestamp(v)?
        .map(Some)
        .ok_or_else(|| invalid("missing timestamp"))
}

pub(crate) fn decode(key: Key, body: &[u8], mut metadata: SourceMetadata) -> Result<Payload> {
    if let Key::Climate(dataset) = key {
        return climate(dataset, body, metadata).map(Payload::Climate);
    }
    let mut root: Value = serde_json::from_slice(body).map_err(|e| invalid(e.to_string()))?;
    if matches!(key, Key::Geo(2)) {
        // IPMA serves the station list as an array of GeoJSON Features.
        array(&root)?;
        root = serde_json::json!({"type": "FeatureCollection", "features": root});
    }
    for field in ["dataUpdate", "updateDate", "fileDate"] {
        if !root[field].is_null() {
            metadata.upstream_updated_at = timestamp(&root[field])?;
            break;
        }
    }
    let meta = Some(metadata);
    Ok(match key {
        Key::WeatherLocation(_) | Key::WeatherDay(_) => {
            let mut forecasts = Vec::new();
            for v in array(&root["data"])? {
                object(v)?;
                forecasts.push(WeatherForecast {
                    location_id: if matches!(key, Key::WeatherLocation(_)) {
                        id(&root, "globalIdLocal")?
                    } else {
                        id(v, "globalIdLocal")?
                    },
                    date: Some(date(if v["forecastDate"].is_null() {
                        &root["forecastDate"]
                    } else {
                        &v["forecastDate"]
                    })?),
                    coordinates: coordinates(v, "latitude", "longitude")?,
                    minimum_temperature_celsius: number(v, "tMin")?,
                    maximum_temperature_celsius: number(v, "tMax")?,
                    precipitation_probability_percent: number(v, "precipitaProb")?,
                    prevailing_wind_direction: text(v, "predWindDir")?,
                    weather_type_code: integer(v, "idWeatherType")?,
                    wind_speed_class: integer(v, "classWindSpeed")?,
                    precipitation_intensity_class: integer(v, "classPrecInt")?,
                });
            }
            Payload::Weather(GetWeatherForecastResponse {
                metadata: meta,
                forecasts,
            })
        }
        Key::Warnings => {
            let warnings = array(&root)?
                .iter()
                .map(|v| {
                    object(v)?;
                    Ok(Warning {
                        area_id: string(&v["idAreaAviso"])?,
                        type_name: string(&v["awarenessTypeName"])?,
                        level: string(&v["awarenessLevelID"])?,
                        text: text(v, "text")?,
                        starts_at: required_timestamp(&v["startTime"])?,
                        ends_at: required_timestamp(&v["endTime"])?,
                    })
                })
                .collect::<Result<_>>()?;
            Payload::Warnings(GetWarningsResponse {
                metadata: meta,
                warnings,
            })
        }
        Key::Observations => {
            let mut observations = Vec::new();
            for (time, stations) in object(&root)? {
                let observed_at = Some(parse_timestamp(time)?);
                for (station, v) in object(stations)? {
                    if v.is_null() {
                        continue;
                    }
                    object(v)?;
                    let station_id = station
                        .parse::<u32>()
                        .ok()
                        .filter(|id| *id > 0)
                        .ok_or_else(|| invalid("invalid station ID"))?;
                    observations.push(Observation {
                        station_id,
                        observed_at,
                        temperature_celsius: number(v, "temperatura")?,
                        wind_speed_meters_per_second: number(v, "intensidadeVento")?,
                        wind_speed_kilometers_per_hour: number(v, "intensidadeVentoKM")?,
                        wind_direction_code: integer(v, "idDireccVento")?,
                        precipitation_millimeters: number(v, "precAcumulada")?,
                        relative_humidity_percent: number(v, "humidade")?,
                        sea_level_pressure_hectopascals: number(v, "pressao")?,
                        solar_radiation_kilojoules_per_square_meter: number(v, "radiacao")?,
                    });
                }
            }
            Payload::Observations(GetObservationsResponse {
                metadata: meta,
                observations,
            })
        }
        Key::Sea(_) => {
            let forecasts = array(&root["data"])?
                .iter()
                .map(|v| {
                    object(v)?;
                    Ok(SeaForecast {
                        location_id: id(v, "globalIdLocal")?,
                        coordinates: coordinates(v, "latitude", "longitude")?,
                        minimum_wave_period_seconds: number(v, "wavePeriodMin")?,
                        maximum_wave_period_seconds: number(v, "wavePeriodMax")?,
                        minimum_swell_height_meters: number(v, "waveHighMin")?,
                        maximum_swell_height_meters: number(v, "waveHighMax")?,
                        minimum_significant_wave_height_meters: number(v, "totalSeaMin")?,
                        maximum_significant_wave_height_meters: number(v, "totalSeaMax")?,
                        minimum_sea_surface_temperature_celsius: number(v, "sstMin")?,
                        maximum_sea_surface_temperature_celsius: number(v, "sstMax")?,
                        prevailing_wave_direction: text(v, "predWaveDir")?,
                    })
                })
                .collect::<Result<_>>()?;
            Payload::Sea(GetSeaForecastResponse {
                metadata: meta,
                date: Some(date(&root["forecastDate"])?),
                forecasts,
            })
        }
        Key::Fire(_) => {
            let risks = object(&root["local"])?
                .iter()
                .map(|(municipality_id, v)| {
                    object(v)?;
                    object(&v["data"])?;
                    Ok(FireRisk {
                        municipality_id: municipality_id.clone(),
                        coordinates: coordinates(v, "latitude", "longitude")?,
                        risk_class: integer(&v["data"], "rcm")?,
                    })
                })
                .collect::<Result<_>>()?;
            Payload::Fire(GetFireRiskResponse {
                metadata: meta,
                forecast_date: Some(date(&root["dataPrev"])?),
                model_run_date: Some(date(&root["dataRun"])?),
                risks,
            })
        }
        Key::Uv => {
            let forecasts = array(&root)?
                .iter()
                .map(|v| {
                    object(v)?;
                    Ok(UvForecast {
                        location_id: unsigned(v, "globalIdLocal")?
                            .ok_or_else(|| invalid("missing UV location ID"))?,
                        date: Some(date(&v["data"])?),
                        index: number(v, "iUv")?,
                        peak_hour_interval: text(v, "intervaloHora")?,
                        period_code: integer(v, "idPeriodo")?,
                    })
                })
                .collect::<Result<_>>()?;
            Payload::Uv(GetUvForecastResponse {
                metadata: meta,
                forecasts,
            })
        }
        Key::Seismic(_) => {
            let events = array(&root["data"])?
                .iter()
                .map(|v| {
                    object(v)?;
                    let sensed = match &v["sensed"] {
                        Value::Null => None,
                        Value::Bool(b) => Some(*b),
                        _ => return Err(invalid("invalid sensed flag")),
                    };
                    Ok(SeismicEvent {
                        occurred_at: required_timestamp(&v["time"])?,
                        coordinates: coordinates(v, "lat", "lon")?,
                        magnitude: number(v, "magnitud")?,
                        magnitude_type: text(v, "magType")?,
                        depth_kilometers: number(v, "depth")?,
                        observation_region: text(v, "obsRegion")?,
                        locality: optional_text(v, "local")?,
                        degree: optional_text(v, "degree")?,
                        source: text(v, "source")?,
                        shakemap_id: text(v, "shakemapid")?,
                        updated_at: timestamp(&v["dataUpdate"])?,
                        event_id: optional_text(v, "sismoId")?,
                        sensed,
                        google_map_reference: optional_text(v, "googlemapref")?,
                        tensor_reference: optional_text(v, "tensorRef")?,
                    })
                })
                .collect::<Result<_>>()?;
            Payload::Seismic(GetSeismicEventsResponse {
                metadata: meta,
                events,
            })
        }
        Key::Locations(kind) => {
            let locations = array(if kind == 1 { &root["data"] } else { &root })?
                .iter()
                .map(|v| {
                    object(v)?;
                    Ok(Location {
                        location_id: id(v, "globalIdLocal")?,
                        name: string(&v["local"])?,
                        coordinates: coordinates(v, "latitude", "longitude")?,
                        warning_area_id: text(v, "idAreaAviso")?,
                        region_id: unsigned(v, "idRegiao")?,
                        district_id: unsigned(v, "idDistrito")?,
                        municipality_code: unsigned(v, "idConcelho")?,
                        local_code: unsigned(v, "idLocal")?,
                    })
                })
                .collect::<Result<_>>()?;
            Payload::Locations(GetLocationsResponse {
                metadata: meta,
                locations,
            })
        }
        Key::Codes(kind) => {
            let (code, pt, en) = match kind {
                1 => ("idWeatherType", "descWeatherTypePT", "descWeatherTypeEN"),
                2 => (
                    "classWindSpeed",
                    "descClassWindSpeedDailyPT",
                    "descClassWindSpeedDailyEN",
                ),
                _ => ("classPrecInt", "descClassPrecIntPT", "descClassPrecIntEN"),
            };
            let entries = array(&root["data"])?
                .iter()
                .map(|v| {
                    object(v)?;
                    let n = numeric(&v[code])?.ok_or_else(|| invalid("missing reference code"))?;
                    if n.fract() != 0.0 || n < f64::from(i32::MIN) || n > f64::from(i32::MAX) {
                        return Err(invalid("invalid reference code"));
                    }
                    Ok(CodeEntry {
                        code: n as i32,
                        description_portuguese: string(&v[pt])?,
                        description_english: string(&v[en])?,
                    })
                })
                .collect::<Result<_>>()?;
            Payload::Codes(GetCodeTableResponse {
                metadata: meta,
                entries,
            })
        }
        Key::Geo(_) => Payload::Geo(GetGeoDatasetResponse {
            metadata: meta,
            geobuf: geojson_geobuf::encode(&root)
                .map_err(|e| invalid(e.to_string()))?
                .into(),
        }),
        Key::Climate(_) => unreachable!("handled before JSON parsing"),
    })
}

fn climate(dataset: i32, body: &[u8], metadata: SourceMetadata) -> Result<GetClimateResponse> {
    let columns: &[(ClimateMetric, &str)] = match dataset {
        1 => &[(ClimateMetric::Evapotranspiration, "et0")],
        2 => &[
            (ClimateMetric::MaximumPrecipitationRate, "maxprate"),
            (ClimateMetric::TotalPrecipitation, "tp"),
        ],
        _ => &[
            (ClimateMetric::MaximumTemperature, "maxt2m"),
            (ClimateMetric::MeanTemperature, "meant2m"),
            (ClimateMetric::MinimumTemperature, "mint2m"),
        ],
    };
    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(body);
    let headers = reader
        .headers()
        .map_err(|e| invalid(e.to_string()))?
        .clone();
    let column = |name: &str| {
        headers
            .iter()
            .position(|s| s == name)
            .ok_or_else(|| invalid(format!("missing CSV column {name}")))
    };
    let time = column("time")?;
    let municipality = column("zid")?;
    let name = column("zname")?;
    let stats_columns = columns
        .iter()
        .map(|(metric, suffix)| {
            let indexes = [
                "mean",
                "std",
                "min",
                "max",
                "percentile_25",
                "median",
                "percentile_75",
            ]
            .map(|prefix| column(&format!("{prefix}_{suffix}")));
            Ok((*metric, indexes.into_iter().collect::<Result<Vec<_>>>()?))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut records = Vec::new();
    for row in reader.records() {
        let row = row.map_err(|e| invalid(e.to_string()))?;
        let time = row.get(time).ok_or_else(|| invalid("missing CSV time"))?;
        // CSV dates are timezone-qualified midnight UTC.
        let t = chrono::DateTime::parse_from_str(time, "%Y-%m-%d %H:%M:%S%:z")
            .map_err(|_| invalid("invalid CSV time"))?
            .with_timezone(&chrono::Utc);
        let mut statistics = Vec::with_capacity(stats_columns.len());
        for (metric, indexes) in &stats_columns {
            let values = indexes
                .iter()
                .map(|i| {
                    let value = row.get(*i).ok_or_else(|| invalid("missing CSV value"))?;
                    Ok(numeric(&Value::String(value.to_owned()))?.filter(|n| *n != -99.0))
                })
                .collect::<Result<Vec<_>>>()?;
            statistics.push(ClimateStatistics {
                metric: *metric as i32,
                mean: values[0],
                standard_deviation: values[1],
                minimum: values[2],
                maximum: values[3],
                percentile_25: values[4],
                median: values[5],
                percentile_75: values[6],
            });
        }
        records.push(ClimateRecord {
            date: Some(Date {
                year: t.year(),
                month: t.month(),
                day: t.day(),
            }),
            municipality_id: row[municipality].into(),
            municipality_name: row[name].into(),
            statistics,
        });
    }
    Ok(GetClimateResponse {
        metadata: Some(metadata),
        records,
    })
}

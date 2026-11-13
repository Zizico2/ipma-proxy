use chrono::{Datelike, NaiveDate, NaiveDateTime};
use ipma_proto::v1::*;
use prost::Message;
use serde_json::Value;

use crate::{
    classification,
    proxy::{Error, Key},
};

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

pub(crate) fn parse_date(s: &str) -> Result<Date> {
    let d = NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| invalid("invalid date"))?;
    Ok(Date {
        year: d.year(),
        month: d.month(),
        day: d.day(),
    })
}

pub(crate) fn parse_timestamp(s: &str) -> Result<prost_types::Timestamp> {
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

pub(crate) async fn fetch(
    client: &ipma_open_data_client::Client,
    key: Key,
    mut metadata: SourceMetadata,
) -> Result<Payload> {
    match key {
        Key::Climate(dataset) => {
            let response = match dataset {
                1 => client.get_climate_evapotranspiration().await?,
                2 => client.get_climate_precipitation().await?,
                _ => client.get_climate_temperature().await?,
            };
            let body = binary_body(response).await?;
            metadata.fetched_at = Some(std::time::SystemTime::now().into());
            climate(dataset, &body, metadata).map(Payload::Climate)
        }
        Key::Geo(dataset) => {
            let root = match dataset {
                2 => {
                    serde_json::json!({"type":"FeatureCollection", "features": client.get_stations().await?.into_inner()})
                }
                _ => {
                    let response = if dataset == 1 {
                        client.get_recent_observations_geojson().await?
                    } else {
                        client.get_bivalve_zones().await?
                    };
                    serde_json::from_slice::<Value>(&binary_body(response).await?)
                        .map_err(|e| invalid(e.to_string()))?
                }
            };
            metadata.fetched_at = Some(std::time::SystemTime::now().into());
            for field in ["dataUpdate", "updateDate", "fileDate"] {
                if !root[field].is_null() {
                    metadata.upstream_updated_at = match root[field].as_str() {
                        Some("") => None,
                        Some(value) => Some(parse_timestamp(value)?),
                        None => return Err(invalid("invalid timestamp")),
                    };
                    break;
                }
            }
            Ok(Payload::Geo(GetGeoDatasetResponse {
                feature_categories: geo_categories(dataset, &root)?,
                metadata: Some(metadata),
                geobuf: geojson_geobuf::encode(&root)
                    .map_err(|e| invalid(e.to_string()))?
                    .into(),
            }))
        }
        _ => crate::typed::fetch(client, key, metadata).await,
    }
}

// Progenitor returns a ByteStream for non-JSON content. Only these formats
// need collecting before the CSV parser or Geobuf encoder can consume them.
async fn binary_body(
    response: ipma_open_data_client::ResponseValue<ipma_open_data_client::ByteStream>,
) -> Result<Vec<u8>> {
    use futures_util::TryStreamExt;
    Ok(response
        .into_inner_stream()
        .try_collect::<Vec<_>>()
        .await?
        .concat())
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
            municipality: classification::municipality(&row[municipality], None, None),
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

/// Build typed companions without changing the feature sequence or properties
/// passed to Geobuf. Only information present in this response is used.
fn geo_categories(dataset: i32, root: &Value) -> Result<Vec<GeoFeatureCategories>> {
    array(&root["features"])?
        .iter()
        .enumerate()
        .map(|(index, feature)| {
            let properties = &feature["properties"];
            // GeoJSON permits null properties; there are no categories to map.
            if properties.is_null() {
                return Ok(None);
            }
            object(properties)?;
            let categories = if dataset == 3 {
                let mut species_permissions = Vec::new();
                let interdictions = &properties["interdictions"];
                if !interdictions.is_null() {
                    for (group, species) in object(interdictions)? {
                        // Preserve new grouping names through OTHER rather than
                        // assuming only the published open/close groups exist.
                        if species.is_null() {
                            continue;
                        }
                        for species in array(species)? {
                            object(species)?;
                            species_permissions.push(BivalveSpeciesPermission {
                                species: classification::species(
                                    &text(species, "specie_s")?,
                                    &text(species, "specie_c")?,
                                ),
                                sanitary_class: classification::sanitary_class(&text(
                                    species,
                                    "classification",
                                )?),
                                permission: classification::harvest_permission(group),
                            });
                        }
                    }
                }
                // The documentation uses samp; the current feed uses code.
                // Both originals remain available in Geobuf if both are supplied.
                let zone = if properties["code"].is_null() {
                    text(properties, "samp")?
                } else {
                    text(properties, "code")?
                };
                geo_feature_categories::Categories::BivalveZone(BivalveZoneCategories {
                    zone: classification::bivalve_zone(&zone),
                    zone_type: classification::bivalve_zone_type(&text(properties, "zone_type")?),
                    status: classification::bivalve_zone_status(&text(properties, "status")?),
                    region: classification::bivalve_region(&text(properties, "region_name")?),
                    species_permissions,
                })
            } else {
                geo_feature_categories::Categories::Station(StationReference {
                    station: classification::station(unsigned(properties, "idEstacao")?),
                    name: optional_text(properties, "localEstacao")?,
                })
            };
            Ok(Some(GeoFeatureCategories {
                feature_index: u32::try_from(index)
                    .map_err(|_| invalid("too many GeoJSON features"))?,
                categories: Some(categories),
            }))
        })
        .filter_map(|entry| entry.transpose())
        .collect()
}

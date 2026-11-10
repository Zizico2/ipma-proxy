//! Run against a live proxy: cargo run --example smoke -- http://127.0.0.1:50051
use ipma_proxy::{
    proto::{proxy_service_client::ProxyServiceClient, *},
    proxy::MAX_MESSAGE_BYTES,
};
use prost::Message;
use std::time::Instant;
use tonic::{Response, Status};

fn report<T: Message>(name: &str, result: Result<Response<T>, Status>) -> anyhow::Result<T> {
    let value = result
        .map_err(|error| anyhow::anyhow!("{name}: {error}"))?
        .into_inner();
    println!("{name}: {} protobuf bytes", value.encoded_len());
    Ok(value)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let address = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "http://127.0.0.1:50051".into());
    let mut client = ProxyServiceClient::connect(address)
        .await?
        .max_decoding_message_size(MAX_MESSAGE_BYTES);
    report("ping", client.ping(PingRequest {}).await)?;
    report(
        "Lisbon forecast",
        client
            .get_weather_forecast(GetWeatherForecastRequest {
                target: Some(get_weather_forecast_request::Target::LocationId(1110600)),
            })
            .await,
    )?;
    for day in 0..=2 {
        report(
            &format!("weather day {day}"),
            client
                .get_weather_forecast(GetWeatherForecastRequest {
                    target: Some(get_weather_forecast_request::Target::DayOffset(day)),
                })
                .await,
        )?;
        report(
            &format!("sea day {day}"),
            client
                .get_sea_forecast(GetSeaForecastRequest {
                    day_offset: Some(day),
                })
                .await,
        )?;
        report(
            &format!("fire day {day}"),
            client
                .get_fire_risk(GetFireRiskRequest {
                    day_offset: Some(day),
                })
                .await,
        )?;
    }
    report("warnings", client.get_warnings(GetWarningsRequest {}).await)?;
    report(
        "24-hour observations",
        client.get_observations(GetObservationsRequest {}).await,
    )?;
    report(
        "UV forecasts",
        client.get_uv_forecast(GetUvForecastRequest {}).await,
    )?;
    for region in [1, 2] {
        report(
            &format!("seismic region {region}"),
            client
                .get_seismic_events(GetSeismicEventsRequest { region })
                .await,
        )?;
    }
    for kind in [1, 2] {
        report(
            &format!("locations {kind}"),
            client.get_locations(GetLocationsRequest { kind }).await,
        )?;
    }
    for kind in [1, 2, 3] {
        report(
            &format!("code table {kind}"),
            client.get_code_table(GetCodeTableRequest { kind }).await,
        )?;
    }
    for dataset in [1, 2, 3] {
        report(
            &format!("Geobuf dataset {dataset}"),
            client
                .get_geo_dataset(GetGeoDatasetRequest { dataset })
                .await,
        )?;
    }
    for dataset in [1, 2, 3] {
        report(
            &format!("climate dataset {dataset}"),
            client.get_climate(GetClimateRequest { dataset }).await,
        )?;
    }
    // This measures local transport + cache + protobuf processing, not upstream latency.
    let start = Instant::now();
    for _ in 0..100 {
        client
            .get_weather_forecast(GetWeatherForecastRequest {
                target: Some(get_weather_forecast_request::Target::LocationId(1110600)),
            })
            .await?;
    }
    println!(
        "100 sequential warm Lisbon RPCs: {:?} total, {:?} mean",
        start.elapsed(),
        start.elapsed() / 100
    );
    Ok(())
}

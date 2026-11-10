//! Faithful IPMA vocabulary mapping. Definitions live in ipma-proto; mapping
//! happens once at the adapter boundary and requires no extra upstream fetches.
use ipma_proto::v1::*;

pub(crate) fn warning_level(value: &str) -> Option<WarningLevelValue> {
    if value.trim().is_empty() {
        return None;
    }
    let enum_value = match value.trim().to_ascii_lowercase().as_str() {
        "green" => WarningLevel::Green,
        "yellow" => WarningLevel::Yellow,
        "orange" => WarningLevel::Orange,
        "red" => WarningLevel::Red,
        _ => WarningLevel::Other,
    };
    Some(WarningLevelValue {
        value: enum_value as i32,
        source_value: value.into(),
    })
}

pub(crate) fn warning_type(value: &str) -> Option<WarningTypeValue> {
    if value.trim().is_empty() {
        return None;
    }
    let enum_value = match value.trim().to_lowercase().as_str() {
        "trovoada" => WarningType::Thunderstorms,
        "agitação marítima" => WarningType::RoughSeas,
        "precipitação" => WarningType::Precipitation,
        "vento" => WarningType::Wind,
        "nevoeiro" => WarningType::Fog,
        "neve" => WarningType::Snow,
        "tempo frio" => WarningType::ColdWeather,
        "tempo quente" => WarningType::HotWeather,
        _ => WarningType::Other,
    };
    Some(WarningTypeValue {
        value: enum_value as i32,
        source_value: value.into(),
    })
}

pub(crate) fn weather_type_enum(code: i32) -> WeatherType {
    match code {
        -99 => WeatherType::Unspecified,
        0 => WeatherType::NoInformation,
        1 => WeatherType::ClearSky,
        2 => WeatherType::FewClouds,
        3 => WeatherType::PartlyCloudy,
        4 => WeatherType::Overcast,
        5 => WeatherType::HighClouds,
        6 => WeatherType::ShowersOrRain,
        7 => WeatherType::LightShowersOrRain,
        8 => WeatherType::HeavyShowersOrRain,
        9 => WeatherType::RainOrShowers,
        10 => WeatherType::LightRainOrDrizzle,
        11 => WeatherType::HeavyRainOrShowers,
        12 => WeatherType::IntermittentRain,
        13 => WeatherType::IntermittentLightRain,
        14 => WeatherType::IntermittentHeavyRain,
        15 => WeatherType::Drizzle,
        16 => WeatherType::Mist,
        17 => WeatherType::FogOrLowCloud,
        18 => WeatherType::Snow,
        19 => WeatherType::Thunderstorms,
        20 => WeatherType::ShowersWithPossibleThunderstorms,
        21 => WeatherType::Hail,
        22 => WeatherType::Frost,
        23 => WeatherType::RainWithPossibleThunderstorms,
        24 => WeatherType::ConvectiveClouds,
        25 => WeatherType::CloudyIntervals,
        26 => WeatherType::Fog,
        27 => WeatherType::Cloudy,
        28 => WeatherType::SnowShowers,
        29 => WeatherType::RainAndSnowCode29,
        30 => WeatherType::RainAndSnowCode30,
        _ => WeatherType::Other,
    }
}

pub(crate) fn wind_speed_enum(code: i32) -> WindSpeedClass {
    match code {
        -99 => WindSpeedClass::Unspecified,
        1 => WindSpeedClass::Weak,
        2 => WindSpeedClass::Moderate,
        3 => WindSpeedClass::Strong,
        4 => WindSpeedClass::VeryStrong,
        _ => WindSpeedClass::Other,
    }
}

pub(crate) fn precipitation_enum(code: i32) -> PrecipitationIntensity {
    match code {
        -99 => PrecipitationIntensity::Unspecified,
        0 => PrecipitationIntensity::NoPrecipitation,
        1 => PrecipitationIntensity::Weak,
        2 => PrecipitationIntensity::Moderate,
        3 => PrecipitationIntensity::Strong,
        _ => PrecipitationIntensity::Other,
    }
}

fn fire_risk_enum(code: i32) -> FireRiskClass {
    match code {
        -99 => FireRiskClass::Unspecified,
        1 => FireRiskClass::Reduced,
        2 => FireRiskClass::Moderate,
        3 => FireRiskClass::High,
        4 => FireRiskClass::VeryHigh,
        5 => FireRiskClass::Maximum,
        _ => FireRiskClass::Other,
    }
}

pub(crate) fn weather_type(code: Option<i32>) -> Option<WeatherTypeValue> {
    code.filter(|code| *code != -99)
        .map(|source_code| WeatherTypeValue {
            value: weather_type_enum(source_code) as i32,
            source_code,
        })
}
pub(crate) fn wind_speed(code: Option<i32>) -> Option<WindSpeedClassValue> {
    code.filter(|code| *code != -99)
        .map(|source_code| WindSpeedClassValue {
            value: wind_speed_enum(source_code) as i32,
            source_code,
        })
}
pub(crate) fn precipitation(code: Option<i32>) -> Option<PrecipitationIntensityValue> {
    code.filter(|code| *code != -99)
        .map(|source_code| PrecipitationIntensityValue {
            value: precipitation_enum(source_code) as i32,
            source_code,
        })
}
pub(crate) fn fire_risk(code: Option<i32>) -> Option<FireRiskClassValue> {
    code.filter(|code| *code != -99)
        .map(|source_code| FireRiskClassValue {
            value: fire_risk_enum(source_code) as i32,
            source_code,
        })
}

pub(crate) fn direction_text(value: &str) -> Option<DirectionValue> {
    if value.trim().is_empty() {
        return None;
    }
    let enum_value = match value.trim().to_ascii_uppercase().as_str() {
        "N" => CompassDirection::North,
        "NE" => CompassDirection::NorthEast,
        "E" => CompassDirection::East,
        "SE" => CompassDirection::SouthEast,
        "S" => CompassDirection::South,
        "SW" => CompassDirection::SouthWest,
        "W" => CompassDirection::West,
        "NW" => CompassDirection::NorthWest,
        _ => CompassDirection::Other,
    };
    Some(DirectionValue {
        value: enum_value as i32,
        source: Some(direction_value::Source::Text(value.into())),
    })
}

pub(crate) fn direction_code(code: Option<i32>) -> Option<DirectionValue> {
    code.filter(|code| *code != -99).map(|code| {
        let enum_value = match code {
            0 => CompassDirection::NoDirection,
            1 | 9 => CompassDirection::North,
            2 => CompassDirection::NorthEast,
            3 => CompassDirection::East,
            4 => CompassDirection::SouthEast,
            5 => CompassDirection::South,
            6 => CompassDirection::SouthWest,
            7 => CompassDirection::West,
            8 => CompassDirection::NorthWest,
            _ => CompassDirection::Other,
        };
        DirectionValue {
            value: enum_value as i32,
            source: Some(direction_value::Source::Code(code)),
        }
    })
}

pub(crate) fn code_entry_classification(table: i32, code: i32) -> code_entry::Classification {
    match table {
        1 => code_entry::Classification::WeatherType(weather_type_enum(code) as i32),
        2 => code_entry::Classification::WindSpeedClass(wind_speed_enum(code) as i32),
        _ => code_entry::Classification::PrecipitationIntensity(precipitation_enum(code) as i32),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn warning_levels_and_types_preserve_original_labels() {
        for (source, expected) in [
            ("green", WarningLevel::Green),
            ("yellow", WarningLevel::Yellow),
            ("orange", WarningLevel::Orange),
            ("red", WarningLevel::Red),
            (" GREEN ", WarningLevel::Green),
            ("purple", WarningLevel::Other),
        ] {
            let value = warning_level(source).unwrap();
            assert_eq!(value.value(), expected);
            assert_eq!(value.source_value, source);
        }
        for (source, expected) in [
            ("Trovoada", WarningType::Thunderstorms),
            ("Agitação Marítima", WarningType::RoughSeas),
            ("Precipitação", WarningType::Precipitation),
            ("Vento", WarningType::Wind),
            ("Nevoeiro", WarningType::Fog),
            ("Neve", WarningType::Snow),
            ("Tempo Frio", WarningType::ColdWeather),
            ("Tempo Quente", WarningType::HotWeather),
            ("AGITAÇÃO MARÍTIMA", WarningType::RoughSeas),
            ("new source category", WarningType::Other),
        ] {
            let value = warning_type(source).unwrap();
            assert_eq!(value.value(), expected);
            assert_eq!(value.source_value, source);
        }
        for source in ["", " \t"] {
            assert!(warning_level(source).is_none());
            assert!(warning_type(source).is_none());
        }
    }

    #[test]
    fn missing_and_meaningful_zeroes_have_different_representations() {
        for source in [None, Some(-99)] {
            assert!(weather_type(source).is_none());
            assert!(wind_speed(source).is_none());
            assert!(precipitation(source).is_none());
            assert!(fire_risk(source).is_none());
            assert!(direction_code(source).is_none());
        }
        let weather = weather_type(Some(0)).unwrap();
        assert_eq!(weather.value(), WeatherType::NoInformation);
        assert_eq!(weather.source_code, 0);
        let rain = precipitation(Some(0)).unwrap();
        assert_eq!(rain.value(), PrecipitationIntensity::NoPrecipitation);
        assert_eq!(rain.source_code, 0);
        assert_eq!(
            direction_code(Some(0)).unwrap().value(),
            CompassDirection::NoDirection
        );
        // Zero is not a documented wind-speed or fire-risk class.
        assert_eq!(wind_speed(Some(0)).unwrap().value(), WindSpeedClass::Other);
        assert_eq!(fire_risk(Some(0)).unwrap().value(), FireRiskClass::Other);
    }

    #[test]
    fn unknown_codes_are_retained_instead_of_becoming_unspecified() {
        for source in [i32::MIN, -1, 999, i32::MAX] {
            let weather = weather_type(Some(source)).unwrap();
            assert_eq!(weather.value(), WeatherType::Other);
            assert_eq!(weather.source_code, source);
            let wind = wind_speed(Some(source)).unwrap();
            assert_eq!(wind.value(), WindSpeedClass::Other);
            assert_eq!(wind.source_code, source);
            let rain = precipitation(Some(source)).unwrap();
            assert_eq!(rain.value(), PrecipitationIntensity::Other);
            assert_eq!(rain.source_code, source);
            let fire = fire_risk(Some(source)).unwrap();
            assert_eq!(fire.value(), FireRiskClass::Other);
            assert_eq!(fire.source_code, source);
        }
    }

    #[test]
    fn weather_catalogue_keeps_all_source_distinctions() {
        let catalogue: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/weather-codes.json")).unwrap();
        let mut kinds = HashSet::new();
        let mut codes = HashSet::new();
        for row in catalogue["data"].as_array().unwrap() {
            let code = row["idWeatherType"].as_i64().unwrap() as i32;
            if code == -99 {
                assert_eq!(weather_type_enum(code), WeatherType::Unspecified);
                continue;
            }
            let value = weather_type(Some(code)).unwrap();
            assert_ne!(value.value(), WeatherType::Other);
            assert_ne!(value.value(), WeatherType::Unspecified);
            assert_eq!(value.source_code, code);
            assert!(kinds.insert(value.value));
            codes.insert(code);
        }
        assert_eq!(codes, (0..=30).collect());
        assert_eq!(weather_type_enum(1), WeatherType::ClearSky);
        assert_eq!(
            weather_type_enum(23),
            WeatherType::RainWithPossibleThunderstorms
        );
        // Captions can overlap; their source concepts must stay separate.
        assert_ne!(weather_type_enum(2), weather_type_enum(25));
        assert_ne!(weather_type_enum(17), weather_type_enum(26));
        assert_eq!(weather_type_enum(29), WeatherType::RainAndSnowCode29);
        assert_eq!(weather_type_enum(30), WeatherType::RainAndSnowCode30);
    }

    #[test]
    fn published_wind_rain_and_fire_classes_are_mapped() {
        for (code, expected) in [
            (1, WindSpeedClass::Weak),
            (2, WindSpeedClass::Moderate),
            (3, WindSpeedClass::Strong),
            (4, WindSpeedClass::VeryStrong),
        ] {
            assert_eq!(wind_speed(Some(code)).unwrap().value(), expected);
        }
        for (code, expected) in [
            (0, PrecipitationIntensity::NoPrecipitation),
            (1, PrecipitationIntensity::Weak),
            (2, PrecipitationIntensity::Moderate),
            (3, PrecipitationIntensity::Strong),
        ] {
            assert_eq!(precipitation(Some(code)).unwrap().value(), expected);
        }
        for (code, expected) in [
            (1, FireRiskClass::Reduced),
            (2, FireRiskClass::Moderate),
            (3, FireRiskClass::High),
            (4, FireRiskClass::VeryHigh),
            (5, FireRiskClass::Maximum),
        ] {
            let value = fire_risk(Some(code)).unwrap();
            assert_eq!(value.value(), expected);
            assert_eq!(value.source_code, code);
        }
    }

    #[test]
    fn direction_aliases_and_source_formats_survive_normalisation() {
        for (code, expected) in [
            (0, CompassDirection::NoDirection),
            (1, CompassDirection::North),
            (2, CompassDirection::NorthEast),
            (3, CompassDirection::East),
            (4, CompassDirection::SouthEast),
            (5, CompassDirection::South),
            (6, CompassDirection::SouthWest),
            (7, CompassDirection::West),
            (8, CompassDirection::NorthWest),
            (9, CompassDirection::North),
            (99, CompassDirection::Other),
        ] {
            let value = direction_code(Some(code)).unwrap();
            assert_eq!(value.value(), expected);
            assert_eq!(value.source, Some(direction_value::Source::Code(code)));
        }
        for (text, expected) in [
            ("N", CompassDirection::North),
            ("NE", CompassDirection::NorthEast),
            ("E", CompassDirection::East),
            ("SE", CompassDirection::SouthEast),
            ("S", CompassDirection::South),
            ("SW", CompassDirection::SouthWest),
            ("W", CompassDirection::West),
            ("NW", CompassDirection::NorthWest),
            (" sw ", CompassDirection::SouthWest),
            ("VARIABLE", CompassDirection::Other),
        ] {
            let value = direction_text(text).unwrap();
            assert_eq!(value.value(), expected);
            assert_eq!(
                value.source,
                Some(direction_value::Source::Text(text.into()))
            );
        }
        assert!(direction_text("").is_none());
    }
}

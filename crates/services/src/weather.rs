//! Current conditions and an hourly strip from Open-Meteo (no API key).

use domain::config::WeatherLocation;
pub use domain::data::weather::*;
use serde::Deserialize;

#[derive(Deserialize)]
struct Response {
    current: Current,
    hourly: Hourly,
    daily: Daily,
}

#[derive(Deserialize)]
struct Current {
    temperature_2m: f64,
    weather_code: u32,
    is_day: u8,
}

#[derive(Deserialize)]
struct Hourly {
    time: Vec<String>,
    temperature_2m: Vec<f64>,
    weather_code: Vec<u32>,
}

#[derive(Deserialize)]
struct Daily {
    temperature_2m_max: Vec<f64>,
    temperature_2m_min: Vec<f64>,
}

pub fn url(location: &WeatherLocation) -> String {
    format!(
        "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}\
         &current=temperature_2m,weather_code,is_day\
         &hourly=temperature_2m,weather_code&forecast_hours=6\
         &daily=temperature_2m_max,temperature_2m_min&forecast_days=1&timezone=auto",
        location.latitude, location.longitude
    )
}

pub fn parse(json: &str) -> Result<Weather, String> {
    let response: Response = serde_json::from_str(json).map_err(|err| err.to_string())?;
    let hours = response
        .hourly
        .time
        .iter()
        .zip(&response.hourly.temperature_2m)
        .zip(&response.hourly.weather_code)
        .filter_map(|((time, &temperature), &code)| {
            // Times look like "2026-09-28T22:00".
            let hour = time.get(11..13)?.parse().ok()?;
            Some(Hour {
                hour,
                temperature,
                condition: Condition::from_wmo(code),
            })
        })
        .collect();
    let first = |values: &[f64]| values.first().copied().ok_or("missing daily forecast");
    Ok(Weather {
        temperature: response.current.temperature_2m,
        high: first(&response.daily.temperature_2m_max)?,
        low: first(&response.daily.temperature_2m_min)?,
        condition: Condition::from_wmo(response.current.weather_code),
        is_day: response.current.is_day == 1,
        hours,
    })
}

/// Blocking fetch; call it from a background task.
pub fn fetch(location: &WeatherLocation) -> Result<Weather, String> {
    let body = ureq::get(&url(location))
        .timeout(std::time::Duration::from_secs(15))
        .call()
        .map_err(|err| format!("Couldn't reach Open-Meteo ({err})"))?
        .into_string()
        .map_err(|err| err.to_string())?;
    parse(&body)
}

#[derive(Deserialize)]
struct Places {
    #[serde(default)]
    results: Vec<Place>,
}

pub fn search_url(query: &str) -> String {
    let encoded: String = query
        .trim()
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect();
    format!("https://geocoding-api.open-meteo.com/v1/search?name={encoded}&count=6&format=json")
}

pub fn parse_places(json: &str) -> Result<Vec<Place>, String> {
    let places: Places = serde_json::from_str(json).map_err(|err| err.to_string())?;
    Ok(places.results)
}

/// Blocking place search; call it from a background task.
pub fn search(query: &str) -> Result<Vec<Place>, String> {
    let body = ureq::get(&search_url(query))
        .timeout(std::time::Duration::from_secs(10))
        .call()
        .map_err(|err| format!("Couldn't reach Open-Meteo ({err})"))?
        .into_string()
        .map_err(|err| err.to_string())?;
    parse_places(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_open_meteo_response() {
        let json = r#"{
            "current": {"time": "2026-09-28T21:45", "temperature_2m": 13.2, "weather_code": 3, "is_day": 0},
            "hourly": {
                "time": ["2026-09-28T22:00", "2026-09-28T23:00"],
                "temperature_2m": [12.8, 12.1],
                "weather_code": [3, 61]
            },
            "daily": {"temperature_2m_max": [16.0], "temperature_2m_min": [11.4]}
        }"#;
        let weather = parse(json).unwrap();
        assert_eq!(weather.condition, Condition::Cloudy);
        assert!(!weather.is_day);
        assert_eq!(weather.high, 16.0);
        assert_eq!(weather.hours.len(), 2);
        assert_eq!(weather.hours[1].hour, 23);
        assert_eq!(weather.hours[1].condition, Condition::Rain);
    }

    #[test]
    fn rejects_malformed_payloads() {
        assert!(parse("{}").is_err());
    }

    #[test]
    fn parses_places_and_describes_them() {
        let json = r#"{"results":[
            {"id":1,"name":"Aalborg","latitude":57.048,"longitude":9.9187,"country":"Denmark","admin1":"North Denmark"},
            {"id":2,"name":"Singapore","latitude":1.29,"longitude":103.85,"country":"Singapore","admin1":"Singapore"}
        ],"generationtime_ms":0.5}"#;
        let places = parse_places(json).unwrap();
        assert_eq!(places.len(), 2);
        assert_eq!(places[0].detail(), "North Denmark, Denmark");
        assert_eq!(places[1].detail(), "");
        assert_eq!(places[0].location().name, "Aalborg");
        assert!(
            parse_places(r#"{"generationtime_ms":0.1}"#)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn search_urls_escape_the_query() {
        assert!(search_url(" São Paulo ").contains("name=S%C3%A3o%20Paulo&"));
    }
}

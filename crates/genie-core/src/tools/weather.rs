use anyhow::Result;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

/// Weather via Open-Meteo API (free, no API key required).
///
/// Open-Meteo provides current weather and 7-day forecast.
/// We use their geocoding API to resolve city names → coordinates,
/// then fetch weather for those coordinates.
///
/// All requests go through raw TCP+TLS-free HTTP to api.open-meteo.com.
/// Note: Open-Meteo supports HTTP (no TLS required for the free tier).

// ── Public API ──────────────────────────────────────────────

/// Get current weather for a location.
pub async fn get_weather(location: &str) -> Result<String> {
    // Step 1: Geocode the location name → lat/lon.
    let (lat, lon, resolved_name) = geocode(location).await?;

    // Step 2: Fetch current weather.
    let weather = fetch_weather(lat, lon).await?;

    Ok(format!(
        "Weather in {}: {}°C (feels like {}°C), {}. Wind: {} km/h. Humidity: {}%.",
        resolved_name,
        weather.temperature,
        weather.feels_like,
        weather.description,
        weather.wind_speed,
        weather.humidity,
    ))
}

/// Get weather forecast for a location.
pub async fn get_forecast(location: &str) -> Result<String> {
    let (lat, lon, resolved_name) = geocode(location).await?;
    let forecast = fetch_forecast(lat, lon).await?;

    let mut lines = vec![format!("Forecast for {}:", resolved_name)];
    for day in &forecast {
        lines.push(format!(
            "  {} — {}°C to {}°C, {}",
            day.date, day.temp_min, day.temp_max, day.description
        ));
    }

    Ok(lines.join("\n"))
}

struct CurrentWeather {
    temperature: f64,
    feels_like: f64,
    wind_speed: f64,
    humidity: f64,
    description: String,
}

struct ForecastDay {
    date: String,
    temp_min: f64,
    temp_max: f64,
    description: String,
}

/// Geocode a location name using Open-Meteo's geocoding API.
async fn geocode(location: &str) -> Result<(f64, f64, String)> {
    let encoded = location.replace(' ', "+");
    let path = format!(
        "/v1/search?name={}&count=1&language=en&format=json",
        encoded
    );

    let body = http_get("geocoding-api.open-meteo.com", &path).await?;
    let data: serde_json::Value = serde_json::from_str(&body)?;

    let results = data
        .get("results")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("location '{}' not found", location))?;

    let first = results
        .first()
        .ok_or_else(|| anyhow::anyhow!("location '{}' not found", location))?;

    let lat = first
        .get("latitude")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let lon = first
        .get("longitude")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let name = first
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(location)
        .to_string();

    Ok((lat, lon, name))
}

/// Fetch current weather from Open-Meteo.
async fn fetch_weather(lat: f64, lon: f64) -> Result<CurrentWeather> {
    let path = format!(
        "/v1/forecast?latitude={}&longitude={}&current=temperature_2m,relative_humidity_2m,apparent_temperature,weather_code,wind_speed_10m&timezone=auto",
        lat, lon
    );

    let body = http_get("api.open-meteo.com", &path).await?;
    let data: serde_json::Value = serde_json::from_str(&body)?;

    let current = data
        .get("current")
        .ok_or_else(|| anyhow::anyhow!("no current weather data"))?;

    let temperature = current
        .get("temperature_2m")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let feels_like = current
        .get("apparent_temperature")
        .and_then(|v| v.as_f64())
        .unwrap_or(temperature);
    let humidity = current
        .get("relative_humidity_2m")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let wind_speed = current
        .get("wind_speed_10m")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let weather_code = current
        .get("weather_code")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    Ok(CurrentWeather {
        temperature,
        feels_like,
        wind_speed,
        humidity,
        description: wmo_code_to_description(weather_code),
    })
}

/// Fetch 7-day forecast from Open-Meteo.
async fn fetch_forecast(lat: f64, lon: f64) -> Result<Vec<ForecastDay>> {
    let path = format!(
        "/v1/forecast?latitude={}&longitude={}&daily=temperature_2m_max,temperature_2m_min,weather_code&timezone=auto&forecast_days=7",
        lat, lon
    );

    let body = http_get("api.open-meteo.com", &path).await?;
    let data: serde_json::Value = serde_json::from_str(&body)?;

    let daily = data
        .get("daily")
        .ok_or_else(|| anyhow::anyhow!("no forecast data"))?;

    let dates = daily.get("time").and_then(|v| v.as_array());
    let maxs = daily.get("temperature_2m_max").and_then(|v| v.as_array());
    let mins = daily.get("temperature_2m_min").and_then(|v| v.as_array());
    let codes = daily.get("weather_code").and_then(|v| v.as_array());

    let mut forecast = Vec::new();
    if let (Some(dates), Some(maxs), Some(mins), Some(codes)) = (dates, maxs, mins, codes) {
        for i in 0..dates.len().min(7) {
            forecast.push(ForecastDay {
                date: dates[i].as_str().unwrap_or("").to_string(),
                temp_max: maxs[i].as_f64().unwrap_or(0.0),
                temp_min: mins[i].as_f64().unwrap_or(0.0),
                description: wmo_code_to_description(codes[i].as_u64().unwrap_or(0)),
            });
        }
    }

    Ok(forecast)
}

/// Raw HTTP GET (no TLS — Open-Meteo supports plain HTTP).
async fn http_get(host: &str, path: &str) -> Result<String> {
    let addr = format!("{}:80", host);
    let stream = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        TcpStream::connect(&addr),
    )
    .await
    .map_err(|_| anyhow::anyhow!("connection timeout"))??;

    let (reader, mut writer) = stream.into_split();

    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: GeniePod/0.2\r\nAccept: application/json\r\nConnection: close\r\n\r\n",
        path, host
    );
    writer.write_all(request.as_bytes()).await?;

    let mut buf_reader = BufReader::new(reader);
    let mut body = String::new();
    let mut in_body = false;

    loop {
        let mut line = String::new();
        let n = buf_reader.read_line(&mut line).await?;
        if n == 0 {
            break;
        }
        if in_body {
            body.push_str(&line);
        } else if line.trim().is_empty() {
            in_body = true;
        }
    }

    // Handle chunked transfer encoding (simple: just strip chunk headers).
    if body.contains("\r\n") && body.starts_with(|c: char| c.is_ascii_hexdigit()) {
        let mut decoded = String::new();
        for line in body.split("\r\n") {
            if !line.is_empty() && !line.chars().all(|c| c.is_ascii_hexdigit()) {
                decoded.push_str(line);
            }
        }
        body = decoded;
    }

    Ok(body.trim().to_string())
}

/// WMO weather interpretation codes → human-readable description.
fn wmo_code_to_description(code: u64) -> String {
    match code {
        0 => "clear sky",
        1 => "mainly clear",
        2 => "partly cloudy",
        3 => "overcast",
        45 | 48 => "foggy",
        51 | 53 | 55 => "drizzle",
        56 | 57 => "freezing drizzle",
        61 | 63 | 65 => "rain",
        66 | 67 => "freezing rain",
        71 | 73 | 75 => "snow",
        77 => "snow grains",
        80..=82 => "rain showers",
        85 | 86 => "snow showers",
        95 => "thunderstorm",
        96 | 99 => "thunderstorm with hail",
        _ => "unknown conditions",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wmo_codes() {
        assert_eq!(wmo_code_to_description(0), "clear sky");
        assert_eq!(wmo_code_to_description(61), "rain");
        assert_eq!(wmo_code_to_description(95), "thunderstorm");
        assert_eq!(wmo_code_to_description(999), "unknown conditions");
    }
}

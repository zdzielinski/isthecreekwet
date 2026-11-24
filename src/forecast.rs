//! The /api/forecast endpoint: the National Weather Service 7-day forecast for the station.

use std::collections::BTreeMap;

use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;
use time::{Date, OffsetDateTime};
use worker::{Fetch, Headers, Request, RequestInit};

use crate::error::{Error, Result};
use crate::json::{round2, truthy};
use crate::types::DebugLog;

// NWS forecast for the station's location (38.104, -109.595), found via
// https://api.weather.gov/points/38.104,-109.595. NWS asks callers to identify themselves.
const FORECAST_URL: &str = "https://api.weather.gov/gridpoints/GJT/54,63/forecast";
const GRIDPOINT_URL: &str = "https://api.weather.gov/gridpoints/GJT/54,63"; // raw data, incl. expected precip amounts
const NWS_USER_AGENT: &str = "isthecreekwet.com (hello@isthecreekwet.com)";

/// GET a National Weather Service API URL.
async fn nws_json(url: &str, debug: &DebugLog) -> Result<Value> {
    let headers = Headers::new();
    headers.set("User-Agent", NWS_USER_AGENT)?;
    headers.set("Accept", "application/geo+json")?;
    let mut init = RequestInit::new();
    init.with_headers(headers);
    let mut resp = Fetch::Request(Request::new_with_init(url, &init)?)
        .send()
        .await?;
    debug.log(json!({"msg": "fetched", "url": url, "status": resp.status_code()}));
    if !(200..300).contains(&resp.status_code()) {
        return Err(Error::NwsApi {
            status: resp.status_code(),
        });
    }
    Ok(resp.json().await?)
}

/// The NWS 7-day forecast for the station, one entry per day, and when NWS issued it.
///
/// See [`build_forecast`] for the shape of the result.
pub async fn get_forecast(debug: &DebugLog) -> Result<Value> {
    let forecast = nws_json(FORECAST_URL, debug).await?;
    let grid = nws_json(GRIDPOINT_URL, debug).await?;
    build_forecast(&forecast, &grid)
}

/// Combine the NWS forecast and gridpoint responses into one entry per Utah day.
///
/// Returns `{"updated": ISO time or null, "days": [{"name", "date", "high", "low", "precip_pct",
/// "precip_in", "short"}]}`; name is the weekday. high, low or `precip_in` is null when the
/// forecast doesn't cover that part of the day.
fn build_forecast(forecast: &Value, grid: &Value) -> Result<Value> {
    let periods = forecast["properties"]["periods"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    let start_time = |p: &Value| parse_iso(p["startTime"].as_str().unwrap_or_default());

    // Expected precip comes in blocks like "2026-11-23T12:00:00+00:00/PT6H" (UTC, mm).
    // Add each block to the Utah date it starts on, using the forecast's own UTC offset.
    let first = periods.first().ok_or(Error::NoForecastPeriods)?;
    let utah = start_time(first)?.offset();
    let mut precip_mm: BTreeMap<Date, f64> = BTreeMap::new();
    for v in grid["properties"]["quantitativePrecipitation"]["values"]
        .as_array()
        .into_iter()
        .flatten()
    {
        if let Some(value) = v["value"].as_f64() {
            let valid_time = v["validTime"].as_str().unwrap_or_default();
            let start = parse_iso(valid_time.split('/').next().unwrap_or_default())?;
            *precip_mm.entry(start.to_offset(utah).date()).or_insert(0.0) += value;
        }
    }

    // Group NWS's day/night periods by the Utah date each one starts on.
    let mut days: Vec<(Date, Value)> = Vec::new();
    for p in periods {
        let start = start_time(p)?; // already Utah local time
        let date = start.date();
        let pct = p
            .get("probabilityOfPrecipitation")
            .and_then(|o| o.get("value"));
        let pct = pct
            .filter(|v| truthy(Some(v)))
            .cloned()
            .unwrap_or_else(|| json!(0));
        let i = days
            .iter()
            .position(|(d, _)| *d == date)
            .unwrap_or_else(|| {
                days.push((
                    date,
                    json!({
                        "name": start.weekday().to_string(),
                        "date": format!("{}/{}", u8::from(start.month()), start.day()),
                        "high": null,
                        "low": null,
                        "precip_pct": 0,
                        "precip_in": precip_mm.get(&date).map(|mm| round2(mm / 25.4)),
                        "short": p["shortForecast"],
                    }),
                ));
                days.len() - 1
            });
        let day = &mut days[i].1;
        if pct.as_f64() > day["precip_pct"].as_f64() {
            day["precip_pct"] = pct;
        }
        if p["isDaytime"].as_bool() == Some(true) {
            day["high"] = p["temperature"].clone();
            day["short"] = p["shortForecast"].clone(); // prefer the daytime description
        } else if day["low"].is_null() || p["temperature"].as_f64() < day["low"].as_f64() {
            day["low"] = p["temperature"].clone();
        }
    }
    Ok(json!({
        "updated": forecast["properties"]["updateTime"],
        "days": days.into_iter().map(|(_, d)| d).collect::<Vec<_>>(),
    }))
}

fn parse_iso(s: &str) -> Result<OffsetDateTime> {
    OffsetDateTime::parse(s, &Rfc3339).map_err(|e| Error::Time {
        input: s.to_string(),
        source: e,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn period(start: &str, is_daytime: bool, temperature: i64, pop: &Value, short: &str) -> Value {
        json!({
            "startTime": start,
            "isDaytime": is_daytime,
            "temperature": temperature,
            "probabilityOfPrecipitation": {"value": pop},
            "shortForecast": short,
        })
    }

    fn forecast(periods: &[Value]) -> Value {
        json!({"properties": {"updateTime": "2026-11-23T17:00:00+00:00", "periods": periods}})
    }

    fn grid(values: &[Value]) -> Value {
        json!({"properties": {"quantitativePrecipitation": {"values": values}}})
    }

    #[test]
    fn test_parse_iso_keeps_offset() -> Result<()> {
        let t = parse_iso("2026-11-23T06:00:00-07:00")?;
        assert_eq!(t.offset().whole_seconds(), -7 * 3600);
        Ok(())
    }

    #[test]
    fn test_parse_iso_error_names_input() {
        let result = parse_iso("nope");
        assert!(matches!(&result, Err(e) if e.to_string().ends_with(": nope")));
    }

    #[test]
    fn test_build_forecast_groups_periods_by_utah_day() -> Result<()> {
        let forecast = forecast(&[
            period("2026-11-23T18:00:00-07:00", false, 20, &json!(10), "Clear"),
            period("2026-11-24T06:00:00-07:00", true, 45, &json!(null), "Sunny"),
            period("2026-11-24T18:00:00-07:00", false, 25, &json!(30), "Snow"),
        ]);
        let grid = grid(&[
            // 17:00 on the 23rd in Utah
            json!({"validTime": "2026-11-24T00:00:00+00:00/PT6H", "value": 2.54}),
            // 05:00 and 23:00 on the 24th in Utah
            json!({"validTime": "2026-11-24T12:00:00+00:00/PT6H", "value": 12.7}),
            json!({"validTime": "2026-11-25T06:00:00+00:00/PT6H", "value": 2.54}),
            json!({"validTime": "2026-11-25T12:00:00+00:00/PT6H", "value": null}),
        ]);
        assert_eq!(
            build_forecast(&forecast, &grid)?,
            json!({
                "updated": "2026-11-23T17:00:00+00:00",
                "days": [
                    {"name": "Monday", "date": "11/23", "high": null, "low": 20,
                     "precip_pct": 10, "precip_in": 0.1, "short": "Clear"},
                    {"name": "Tuesday", "date": "11/24", "high": 45, "low": 25,
                     "precip_pct": 30, "precip_in": 0.6, "short": "Sunny"},
                ],
            })
        );
        Ok(())
    }

    #[test]
    fn test_build_forecast_keeps_lowest_night_temperature() -> Result<()> {
        let forecast = forecast(&[
            period("2026-11-24T00:00:00-07:00", false, 30, &json!(0), "Cloudy"),
            period("2026-11-24T06:00:00-07:00", true, 50, &json!(0), "Sunny"),
            period("2026-11-24T18:00:00-07:00", false, 22, &json!(0), "Clear"),
        ]);
        let result = build_forecast(&forecast, &grid(&[]))?;
        assert_eq!(result["days"][0]["low"], json!(22));
        assert_eq!(result["days"][0]["precip_in"], Value::Null);
        Ok(())
    }

    #[test]
    fn test_build_forecast_without_periods_is_an_error() {
        let result = build_forecast(&forecast(&[]), &grid(&[]));
        assert!(matches!(result, Err(Error::NoForecastPeriods)));
        let result = build_forecast(&json!({}), &json!({}));
        assert!(matches!(result, Err(Error::NoForecastPeriods)));
    }

    #[test]
    fn test_build_forecast_rejects_bad_start_time() {
        let forecast = forecast(&[period("tomorrow", true, 50, &json!(0), "Sunny")]);
        let result = build_forecast(&forecast, &grid(&[]));
        assert!(matches!(result, Err(Error::Time { .. })));
    }
}

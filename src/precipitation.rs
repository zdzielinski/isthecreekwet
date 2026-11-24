//! The /api/precipitation endpoint: recent precipitation from the Utah Climate Center (USU) API.

use std::collections::{BTreeMap, HashSet};

use serde_json::{json, Value};
use time::macros::format_description;
use time::{Date, Duration, OffsetDateTime, PrimitiveDateTime};
use worker::{Env, Fetch};

use crate::error::{Error, Result};
use crate::json::{round2, truthy};
use crate::types::DebugLog;

const API_BASE: &str = "https://climate.usu.edu/api/v4";
const DAYS: u8 = 14;
const RECENT_HOURS: [i64; 3] = [24, 48, 72];
// Precip in an hour averaging this cold or colder (airt2_avg, °F) is charted as snow.
const SNOW_MAX_F: f64 = 34.0;

// Find station ids via /networks/ucc/stations/byNetwork.php?network=agwx
const STATION_NAME: &str = "Dugout Ranch";
const STATION_NETWORK: &str = "agwx";
const STATION_ID: &str = "1349823";

/// Hourly rows for a station. Dates in and out are Utah local time.
///
/// Date-only start/end values (YYYY-MM-DD) return every hour of those days.
async fn get_hourly(
    api_key: &str,
    start_date: &str,
    end_date: &str,
    debug: &DebugLog,
) -> Result<Vec<Value>> {
    let params = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("network", STATION_NETWORK)
        .append_pair("station_id", STATION_ID)
        .append_pair("start_date", start_date)
        .append_pair("end_date", end_date)
        .append_pair("units", "e")
        .finish();
    let url = format!("{API_BASE}/networks/ucc/hourlyData.php?{params}"); // logged, so no API key in it
    let key = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("key", api_key)
        .finish();
    let mut resp = Fetch::Url(format!("{url}&{key}").parse()?).send().await?;
    debug.log(json!({"msg": "fetched", "url": url, "status": resp.status_code()}));
    let data: Value = resp.json().await?;
    if !truthy(data.get("success")) {
        return Err(Error::UsuApi {
            station: STATION_ID,
            body: data.to_string(),
        });
    }
    Ok(payload(data))
}

fn payload(mut data: Value) -> Vec<Value> {
    match data.get_mut("payload").map(Value::take) {
        Some(Value::Array(rows)) => rows,
        _ => Vec::new(),
    }
}

/// An hourly row's `precip_tb` (inches), or None if the station didn't report it.
fn precip_tb(row: &Value) -> Result<Option<f64>> {
    let not_a_number = |v: &Value| Error::NotANumber {
        field: "precip_tb",
        value: v.to_string(),
    };
    match row.get("precip_tb") {
        None | Some(Value::Null) => Ok(None),
        Some(v @ Value::String(s)) => s.trim().parse().map(Some).map_err(|_| not_a_number(v)),
        Some(v) => v.as_f64().map(Some).ok_or_else(|| not_a_number(v)),
    }
}

fn date_time(row: &Value) -> &str {
    row["date_time"].as_str().unwrap_or_default()
}

fn parse_date_time(s: &str) -> Result<PrimitiveDateTime> {
    let format = format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");
    PrimitiveDateTime::parse(s, format).map_err(|e| Error::Time {
        input: s.to_string(),
        source: e,
    })
}

/// An hourly row's `airt2_avg` (°F, the hour's average air temperature), or None if not reported.
///
/// An unparseable temperature also counts as not reported rather than failing the request: it
/// only decides whether precip is charted as snow, and such hours already count as rain.
fn airt2_avg(row: &Value) -> Option<f64> {
    match row.get("airt2_avg")? {
        Value::String(s) => s.trim().parse().ok(),
        v => v.as_f64(),
    }
}

/// Sum hourly precip (`precip_tb`) by local date. Returns the newest DAYS days, oldest first.
///
/// Each value is (date, `precip_in`, `snow_in`, hours). `snow_in` is the part of `precip_in` that fell in
/// hours averaging `SNOW_MAX_F` or colder (hours with no temperature count as rain), and hours is
/// how many hourly rows the station reported that day (today will be partial).
fn daily_totals(rows: &[Value]) -> Result<Vec<(String, f64, f64, i64)>> {
    let mut totals: BTreeMap<String, (f64, f64, i64)> = BTreeMap::new();
    for row in rows {
        let date = parse_date_time(date_time(row))?.date().to_string();
        let t = totals.entry(date).or_insert((0.0, 0.0, 0));
        t.2 += 1;
        if let Some(p) = precip_tb(row)? {
            t.0 += p;
            if airt2_avg(row).is_some_and(|f| f <= SNOW_MAX_F) {
                t.1 += p;
            }
        }
    }
    let skip = totals.len().saturating_sub(usize::from(DAYS));
    Ok(totals
        .into_iter()
        .skip(skip)
        .map(|(d, (p, s, h))| (d, round2(p), round2(s), h))
        .collect())
}

/// Rolling precip totals over the last N hours, ending at the newest reading.
///
/// Returns [(hours, `precip_in`, `snow_in`)], e.g. [(24, 0.1, 0), (48, 0.3, 0.1), (72, 0.3, 0.1)],
/// where `snow_in` is the part of `precip_in` that fell in hours averaging `SNOW_MAX_F` or colder.
/// Like Python's `sum()`, a total with no readings to add is the integer 0.
fn recent_totals(rows: &[Value]) -> Result<Vec<(i64, Value, Value)>> {
    let times = rows
        .iter()
        .map(|r| {
            let is_snow = airt2_avg(r).is_some_and(|f| f <= SNOW_MAX_F);
            Ok((parse_date_time(date_time(r))?, precip_tb(r)?, is_snow))
        })
        .collect::<Result<Vec<_>>>()?;
    let Some(latest) = times.iter().map(|(t, _, _)| *t).max() else {
        return Ok(RECENT_HOURS
            .iter()
            .map(|&h| (h, Value::Null, Value::Null))
            .collect());
    };
    let total = |amounts: &[f64]| {
        if amounts.is_empty() {
            json!(0)
        } else {
            json!(round2(amounts.iter().sum()))
        }
    };
    Ok(RECENT_HOURS
        .iter()
        .map(|&h| {
            let window: Vec<(f64, bool)> = times
                .iter()
                .filter(|(t, _, _)| *t > latest - Duration::hours(h))
                .filter_map(|&(_, p, is_snow)| Some((p?, is_snow)))
                .collect();
            let precip: Vec<f64> = window.iter().map(|&(p, _)| p).collect();
            let snow: Vec<f64> = window.iter().filter(|(_, s)| *s).map(|&(p, _)| p).collect();
            (h, total(&precip), total(&snow))
        })
        .collect())
}

/// Most recent hour with precip > 0 within `days` (a set of YYYY-MM-DD dates).
///
/// Returns `{"date_time": str, "hours_ago": int}` measured back from the newest
/// reading, or null if there was no precipitation in that window.
fn last_precipitation(rows: &[Value], days: &HashSet<&str>) -> Result<Value> {
    if rows.is_empty() {
        return Ok(Value::Null);
    }
    let mut latest = parse_date_time(date_time(&rows[0]))?;
    let mut last: Option<&str> = None;
    for r in rows {
        let t = parse_date_time(date_time(r))?;
        latest = latest.max(t);
        if days.contains(t.date().to_string().as_str()) && precip_tb(r)?.is_some_and(|p| p > 0.0) {
            last = last.max(Some(date_time(r)));
        }
    }
    let Some(last) = last else {
        return Ok(Value::Null);
    };
    let hours = (latest - parse_date_time(last)?)
        .whole_seconds()
        .div_euclid(3600);
    Ok(json!({"date_time": last, "hours_ago": hours}))
}

/// Hourly rows covering the last DAYS local days (plus a bit extra, trimmed later).
async fn precipitation_rows(api_key: &str, debug: &DebugLog) -> Result<Vec<Value>> {
    // Utah is UTC-6/-7, so UTC "tomorrow" always covers local today; the extra
    // leading day is trimmed by daily_totals.
    let now_utc = OffsetDateTime::now_utc().date();
    let start = (now_utc - Duration::days(i64::from(DAYS))).to_string();
    let end = (now_utc + Duration::days(1)).to_string();
    get_hourly(api_key, &start, &end, debug).await
}

/// The /api/precipitation body: daily totals, recent totals and the last precipitation.
pub async fn get_precipitation(env: &Env, debug: &DebugLog) -> Result<Value> {
    let rows = precipitation_rows(&env.secret("USU_API_KEY")?.to_string(), debug).await?;
    let totals = daily_totals(&rows)?;
    let days: HashSet<&str> = totals.iter().map(|(d, _, _, _)| d.as_str()).collect();
    let daily = totals
        .iter()
        .map(|(d, p, s, h)| {
            Ok(json!({
                "date": d,
                "weekday": weekday(d)?,
                "precip_in": p,
                "snow_in": s,
                "hours": h,
            }))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({
        "station": STATION_NAME,
        "units": "in",
        "days": daily,
        "recent": recent_totals(&rows)?.into_iter().map(|(h, p, s)| json!({
            "hours": h,
            "precip_in": p,
            "snow_in": s,
        })).collect::<Vec<_>>(),
        "last_precipitation": last_precipitation(&rows, &days)?,
    }))
}

fn weekday(day: &str) -> Result<String> {
    let date =
        Date::parse(day, format_description!("[year]-[month]-[day]")).map_err(|e| Error::Time {
            input: day.to_string(),
            source: e,
        })?;
    Ok(date.format(format_description!("[weekday repr:short]"))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(date_time: &str, precip: &Value, temp: &Value) -> Value {
        json!({"date_time": date_time, "precip_tb": precip, "airt2_avg": temp})
    }

    #[test]
    fn test_precip_tb_reads_numbers_strings_and_null() -> Result<()> {
        assert_eq!(precip_tb(&json!({"precip_tb": 0.1}))?, Some(0.1));
        assert_eq!(precip_tb(&json!({"precip_tb": " 0.2 "}))?, Some(0.2));
        assert_eq!(precip_tb(&json!({"precip_tb": null}))?, None);
        assert_eq!(precip_tb(&json!({}))?, None);
        Ok(())
    }

    #[test]
    fn test_precip_tb_rejects_non_numbers() {
        assert!(matches!(
            precip_tb(&json!({"precip_tb": "abc"})),
            Err(Error::NotANumber { .. })
        ));
        assert!(matches!(
            precip_tb(&json!({"precip_tb": true})),
            Err(Error::NotANumber { .. })
        ));
    }

    #[test]
    fn test_daily_totals_splits_rain_and_snow() -> Result<()> {
        let rows = [
            row("2026-11-20 01:00:00", &json!(0.1), &json!(30.0)),
            row("2026-11-20 02:00:00", &json!("0.2"), &json!(40.0)),
            row("2026-11-20 03:00:00", &json!(0.05), &json!(null)),
            row("2026-11-21 01:00:00", &json!(null), &json!(50.0)),
        ];
        assert_eq!(
            daily_totals(&rows)?,
            vec![
                ("2026-11-20".to_string(), 0.35, 0.1, 3),
                ("2026-11-21".to_string(), 0.0, 0.0, 1)
            ]
        );
        Ok(())
    }

    #[test]
    fn test_daily_totals_keeps_newest_days() -> Result<()> {
        let rows: Vec<Value> = (1..=20)
            .map(|d| row(&format!("2026-11-{d:02} 00:00:00"), &json!(0), &json!(60)))
            .collect();
        let totals = daily_totals(&rows)?;
        assert_eq!(totals.len(), usize::from(DAYS));
        assert_eq!(totals[0].0, "2026-11-07");
        Ok(())
    }

    #[test]
    fn test_recent_totals_empty_is_null() -> Result<()> {
        assert_eq!(
            recent_totals(&[])?,
            vec![
                (24, Value::Null, Value::Null),
                (48, Value::Null, Value::Null),
                (72, Value::Null, Value::Null)
            ]
        );
        Ok(())
    }

    #[test]
    fn test_recent_totals_windows() -> Result<()> {
        let rows = [
            row("2026-11-20 12:00:00", &json!(0.3), &json!(30)), // 48h before latest: outside 48h window
            row("2026-11-21 13:00:00", &json!(0.2), &json!(null)), // 23h before latest
            row("2026-11-22 11:00:00", &json!(0.1), &json!(32)), // snow
            row("2026-11-22 12:00:00", &json!(null), &json!(null)),
        ];
        assert_eq!(
            recent_totals(&rows)?,
            vec![
                (24, json!(0.3), json!(0.1)),
                (48, json!(0.3), json!(0.1)),
                (72, json!(0.6), json!(0.4))
            ]
        );
        Ok(())
    }

    #[test]
    fn test_recent_totals_no_readings_is_integer_zero() -> Result<()> {
        let rows = [row("2026-11-22 12:00:00", &json!(null), &json!(null))];
        assert_eq!(recent_totals(&rows)?[0], (24, json!(0), json!(0)));
        Ok(())
    }

    #[test]
    fn test_last_precipitation_finds_latest_wet_hour() -> Result<()> {
        let rows = [
            row("2026-11-20 05:00:00", &json!(0.1), &json!(null)),
            row("2026-11-21 08:00:00", &json!(0.2), &json!(null)),
            row("2026-11-22 10:30:00", &json!(0), &json!(null)),
        ];
        let days = HashSet::from(["2026-11-20", "2026-11-21", "2026-11-22"]);
        assert_eq!(
            last_precipitation(&rows, &days)?,
            json!({"date_time": "2026-11-21 08:00:00", "hours_ago": 26})
        );
        Ok(())
    }

    #[test]
    fn test_last_precipitation_ignores_days_outside_window() -> Result<()> {
        let rows = [row("2026-11-20 05:00:00", &json!(0.1), &json!(null))];
        assert_eq!(
            last_precipitation(&rows, &HashSet::from(["2026-11-21"]))?,
            Value::Null
        );
        assert_eq!(last_precipitation(&[], &HashSet::new())?, Value::Null);
        Ok(())
    }

    #[test]
    fn test_weekday_formats_short_name() -> Result<()> {
        assert_eq!(weekday("2026-11-23")?, "Mon");
        assert!(weekday("not a date").is_err());
        Ok(())
    }

    #[test]
    fn test_daily_totals_rejects_short_date_time() {
        let rows = [row("2026-11", &json!(0.1), &json!(null))];
        assert!(matches!(daily_totals(&rows), Err(Error::Time { .. })));
        let rows = [json!({"precip_tb": 0.1})]; // no date_time at all
        assert!(matches!(daily_totals(&rows), Err(Error::Time { .. })));
    }

    #[test]
    fn test_last_precipitation_rejects_short_date_time() {
        let rows = [row("2026-11", &json!(0.1), &json!(null))];
        let days = HashSet::from(["2026-11-20"]);
        assert!(matches!(
            last_precipitation(&rows, &days),
            Err(Error::Time { .. })
        ));
    }
}

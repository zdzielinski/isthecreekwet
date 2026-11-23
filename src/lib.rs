//! Is the creek wet? Recent rainfall from the Utah Climate Center API.
//!
//! API docs: https://logansw.atlassian.net/wiki/spaces/UCCA/overview

use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::future::Future;
use std::io;

use chrono::{DateTime, Duration, FixedOffset, NaiveDate, NaiveDateTime, Utc};
use serde_json::{json, Value};
use worker::*;

const API_BASE: &str = "https://climate.usu.edu/api/v4";
const CACHE_SECONDS: u32 = 3600; // the station reports hourly
const DAYS: usize = 14;
const RECENT_HOURS: [i64; 3] = [24, 48, 72];

// Find station ids via /networks/ucc/stations/byNetwork.php?network=agwx
const STATION_NAME: &str = "Dugout Ranch";
const STATION_NETWORK: &str = "agwx";
const STATION_ID: &str = "1349823";

// NWS forecast for the station's location (38.104, -109.595), found via
// https://api.weather.gov/points/38.104,-109.595. NWS asks callers to identify themselves.
const FORECAST_URL: &str = "https://api.weather.gov/gridpoints/GJT/54,63/forecast";
const GRIDPOINT_URL: &str = "https://api.weather.gov/gridpoints/GJT/54,63"; // raw data, incl. expected precip amounts
const NWS_USER_AGENT: &str = "isthecreekwet.com (hello@isthecreekwet.com)";

/// One request's debug log, on when LOG_LEVEL=debug (previews and `wrangler dev`; see wrangler.toml).
///
/// Each entry goes to the Workers console (Workers Logs indexes its fields) and back to the
/// page in the X-Debug-Log header, which index.html prints to the browser console.
struct DebugLog(Option<RefCell<Vec<Value>>>);

impl DebugLog {
    fn log(&self, entry: Value) {
        if let Some(entries) = &self.0 {
            console_debug!("{entry}");
            entries.borrow_mut().push(entry);
        }
    }
}

/// Hourly rows for a station. Dates in and out are Utah local time.
///
/// Date-only start/end values (YYYY-MM-DD) return every hour of those days.
async fn get_hourly(api_key: &str, start_date: &str, end_date: &str, debug: &DebugLog) -> Result<Vec<Value>> {
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
        return Err(Error::RustError(format!("USU API error for station {STATION_ID}: {data}")));
    }
    Ok(payload(data))
}

fn payload(data: Value) -> Vec<Value> {
    match data.get("payload") {
        Some(Value::Array(rows)) => rows.clone(),
        _ => Vec::new(),
    }
}

/// Python-style truthiness for a JSON value.
fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64() != Some(0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// GET a National Weather Service API URL.
async fn nws_json(url: &str, debug: &DebugLog) -> Result<Value> {
    let headers = Headers::new();
    headers.set("User-Agent", NWS_USER_AGENT)?;
    headers.set("Accept", "application/geo+json")?;
    let mut init = RequestInit::new();
    init.with_headers(headers);
    let mut resp = Fetch::Request(Request::new_with_init(url, &init)?).send().await?;
    debug.log(json!({"msg": "fetched", "url": url, "status": resp.status_code()}));
    if !(200..300).contains(&resp.status_code()) {
        return Err(Error::RustError(format!("NWS API error: {}", resp.status_code())));
    }
    resp.json().await
}

/// The NWS 7-day forecast for the station, one entry per day.
///
/// Returns [{"name", "date", "high", "low", "precip_pct", "precip_in", "short"}]; name is the
/// weekday. high, low or precip_in is null when the forecast doesn't cover that part of the day.
async fn get_forecast(debug: &DebugLog) -> Result<Vec<Value>> {
    let forecast = nws_json(FORECAST_URL, debug).await?;
    let grid = nws_json(GRIDPOINT_URL, debug).await?;
    let periods = forecast["properties"]["periods"].as_array().cloned().unwrap_or_default();
    let start_time = |p: &Value| parse_iso(p["startTime"].as_str().unwrap_or_default());

    // Expected precip comes in blocks like "2026-11-23T12:00:00+00:00/PT6H" (UTC, mm).
    // Add each block to the Utah date it starts on, using the forecast's own UTC offset.
    let utah = *start_time(&periods[0])?.offset();
    let mut precip_mm: BTreeMap<NaiveDate, f64> = BTreeMap::new();
    for v in grid["properties"]["quantitativePrecipitation"]["values"].as_array().into_iter().flatten() {
        if let Some(value) = v["value"].as_f64() {
            let valid_time = v["validTime"].as_str().unwrap_or_default();
            let start = parse_iso(valid_time.split('/').next().unwrap_or_default())?;
            *precip_mm.entry(start.with_timezone(&utah).date_naive()).or_insert(0.0) += value;
        }
    }

    // Group NWS's day/night periods by the Utah date each one starts on.
    let mut days: Vec<(NaiveDate, Value)> = Vec::new();
    for p in &periods {
        let start = start_time(p)?; // already Utah local time
        let date = start.date_naive();
        let pct = p.get("probabilityOfPrecipitation").and_then(|o| o.get("value"));
        let pct = if truthy(pct) { pct.unwrap().clone() } else { json!(0) };
        let i = match days.iter().position(|(d, _)| *d == date) {
            Some(i) => i,
            None => {
                days.push((
                    date,
                    json!({
                        "name": start.format("%A").to_string(),
                        "date": format!("{}/{}", start.format("%-m"), start.format("%-d")),
                        "high": null,
                        "low": null,
                        "precip_pct": 0,
                        "precip_in": precip_mm.get(&date).map(|mm| round2(mm / 25.4)),
                        "short": p["shortForecast"],
                    }),
                ));
                days.len() - 1
            }
        };
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
    Ok(days.into_iter().map(|(_, d)| d).collect())
}

fn parse_iso(s: &str) -> Result<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc3339(s).map_err(|e| Error::RustError(format!("{e}: {s}")))
}

/// Round to 2 decimal places like Python's round(x, 2).
fn round2(x: f64) -> f64 {
    format!("{x:.2}").parse().unwrap()
}

/// An hourly row's precip_tb (inches), or None if the station didn't report it.
fn precip_tb(row: &Value) -> Option<f64> {
    match row.get("precip_tb")? {
        Value::Null => None,
        Value::String(s) => Some(s.trim().parse().expect("precip_tb is not a number")),
        v => Some(v.as_f64().expect("precip_tb is not a number")),
    }
}

fn date_time(row: &Value) -> &str {
    row["date_time"].as_str().unwrap_or_default()
}

fn parse_date_time(s: &str) -> Result<NaiveDateTime> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").map_err(|e| Error::RustError(format!("{e}: {s}")))
}

/// Sum hourly precip (precip_tb) by local date. Returns the newest DAYS days, oldest first.
///
/// Each value is (precip_in, hours) where hours is how many hourly rows
/// the station reported that day (today will be partial).
fn daily_totals(rows: &[Value]) -> Vec<(String, f64, i64)> {
    let mut totals: BTreeMap<String, (f64, i64)> = BTreeMap::new();
    for row in rows {
        let t = totals.entry(date_time(row)[..10].to_string()).or_insert((0.0, 0));
        t.1 += 1;
        if let Some(p) = precip_tb(row) {
            t.0 += p;
        }
    }
    let skip = totals.len().saturating_sub(DAYS);
    totals.into_iter().skip(skip).map(|(d, (p, h))| (d, round2(p), h)).collect()
}

/// Rolling precip totals over the last N hours, ending at the newest reading.
///
/// Returns [(hours, precip_in)], e.g. [(24, 0.1), (48, 0.3), (72, 0.3)]. Like Python's sum(),
/// a window with no reported readings totals the integer 0.
fn recent_totals(rows: &[Value]) -> Result<Vec<(i64, Value)>> {
    if rows.is_empty() {
        return Ok(RECENT_HOURS.iter().map(|&h| (h, Value::Null)).collect());
    }
    let times = rows
        .iter()
        .map(|r| Ok((parse_date_time(date_time(r))?, precip_tb(r))))
        .collect::<Result<Vec<_>>>()?;
    let latest = times.iter().map(|(t, _)| *t).max().unwrap();
    Ok(RECENT_HOURS
        .iter()
        .map(|&h| {
            let window: Vec<f64> = times
                .iter()
                .filter(|(t, _)| *t > latest - Duration::hours(h))
                .filter_map(|(_, p)| *p)
                .collect();
            let total = if window.is_empty() { json!(0) } else { json!(round2(window.iter().sum())) };
            (h, total)
        })
        .collect())
}

/// Most recent hour with precip > 0 within `days` (a set of YYYY-MM-DD dates).
///
/// Returns {"date_time": str, "hours_ago": int} measured back from the newest
/// reading, or null if there was no rain in that window.
fn last_rain(rows: &[Value], days: &HashSet<&str>) -> Result<Value> {
    if rows.is_empty() {
        return Ok(Value::Null);
    }
    let mut latest = parse_date_time(date_time(&rows[0]))?;
    for r in rows {
        latest = latest.max(parse_date_time(date_time(r))?);
    }
    let last = rows
        .iter()
        .filter(|r| days.contains(&date_time(r)[..10]) && precip_tb(r).is_some_and(|p| p > 0.0))
        .map(date_time)
        .max();
    let Some(last) = last else { return Ok(Value::Null) };
    let hours = (latest - parse_date_time(last)?).num_seconds().div_euclid(3600);
    Ok(json!({"date_time": last, "hours_ago": hours}))
}

/// Hourly rows covering the last DAYS local days (plus a bit extra, trimmed later).
async fn rainfall_rows(api_key: &str, debug: &DebugLog) -> Result<Vec<Value>> {
    // Utah is UTC-6/-7, so UTC "tomorrow" always covers local today; the extra
    // leading day is trimmed by daily_totals.
    let now_utc = Utc::now().date_naive();
    let start = (now_utc - Duration::days(DAYS as i64)).to_string();
    let end = (now_utc + Duration::days(1)).to_string();
    get_hourly(api_key, &start, &end, debug).await
}

/// The /api/rainfall body: daily totals, recent totals and the last rain.
async fn get_rainfall(env: &Env, debug: &DebugLog) -> Result<Value> {
    let rows = rainfall_rows(&env.secret("USU_API_KEY")?.to_string(), debug).await?;
    let totals = daily_totals(&rows);
    let days: HashSet<&str> = totals.iter().map(|(d, _, _)| d.as_str()).collect();
    Ok(json!({
        "station": STATION_NAME,
        "units": "in",
        "days": totals.iter().map(|(d, p, h)| json!({
            "date": d,
            "weekday": weekday(d),
            "precip_in": p,
            "hours": h,
        })).collect::<Vec<_>>(),
        "recent": recent_totals(&rows)?.into_iter().map(|(h, p)| json!({
            "hours": h,
            "precip_in": p,
        })).collect::<Vec<_>>(),
        "last_rain": last_rain(&rows, &days)?,
    }))
}

fn weekday(day: &str) -> String {
    NaiveDate::parse_from_str(day, "%Y-%m-%d").unwrap().format("%a").to_string()
}

/// Serializes like Python's json.dumps defaults: ", " and ": " separators, non-ASCII escaped.
struct PythonJson;

impl serde_json::ser::Formatter for PythonJson {
    fn begin_array_value<W: ?Sized + io::Write>(&mut self, w: &mut W, first: bool) -> io::Result<()> {
        if first { Ok(()) } else { w.write_all(b", ") }
    }
    fn begin_object_key<W: ?Sized + io::Write>(&mut self, w: &mut W, first: bool) -> io::Result<()> {
        if first { Ok(()) } else { w.write_all(b", ") }
    }
    fn begin_object_value<W: ?Sized + io::Write>(&mut self, w: &mut W) -> io::Result<()> {
        w.write_all(b": ")
    }
    fn write_string_fragment<W: ?Sized + io::Write>(&mut self, w: &mut W, fragment: &str) -> io::Result<()> {
        for c in fragment.chars() {
            if (' '..='~').contains(&c) {
                w.write_all(&[c as u8])?;
            } else {
                for unit in c.encode_utf16(&mut [0; 2]) {
                    write!(w, "\\u{unit:04x}")?;
                }
            }
        }
        Ok(())
    }
}

fn respond(body: String, status: u16, content_type: &str) -> Result<Response> {
    let headers = Headers::new();
    headers.set("Content-Type", content_type)?;
    Ok(Response::ok(body)?.with_status(status).with_headers(headers))
}

/// Serve a JSON body from Cloudflare's cache, or build it, cache it for CACHE_SECONDS and serve it.
///
/// Each endpoint caches its finished body, so a cache hit makes no upstream API calls.
/// `key` must be a URL; the cache is per Cloudflare data center.
async fn cached(key: &str, debug: &DebugLog, build: impl Future<Output = Result<Value>>) -> Result<Response> {
    let cache = Cache::default();
    let now = Date::now().as_millis() / 1000;
    if let Some(mut hit) = cache.get(key, false).await? {
        // Entries without X-Stored-At (cached before it existed) count as fully aged.
        let stored_at = hit.headers().get("X-Stored-At")?.and_then(|s| s.parse().ok()).unwrap_or(0);
        let age = now.saturating_sub(stored_at).min(CACHE_SECONDS as u64);
        debug.log(json!({"msg": "cache hit", "key": key, "age_seconds": age}));
        return for_browser(hit.text().await?, CACHE_SECONDS as u64 - age);
    }
    debug.log(json!({"msg": "cache miss", "key": key}));
    let mut body = Vec::new();
    serde::Serialize::serialize(&build.await?, &mut serde_json::Serializer::with_formatter(&mut body, PythonJson))?;
    let body = String::from_utf8(body).unwrap();
    let mut resp = respond(body.clone(), 200, "application/json")?;
    resp.headers_mut().set("Cache-Control", &format!("max-age={CACHE_SECONDS}"))?;
    resp.headers_mut().set("X-Stored-At", &now.to_string())?;
    cache.put(key, resp).await?;
    for_browser(body, CACHE_SECONDS as u64)
}

/// A JSON response the visitor's browser may reuse for `max_age` seconds: the time left before
/// our cached copy expires, so browser copies never outlive it and the data is no staler.
fn for_browser(body: String, max_age: u64) -> Result<Response> {
    let mut resp = respond(body, 200, "application/json")?;
    resp.headers_mut().set("Cache-Control", &format!("private, max-age={max_age}"))?;
    Ok(resp)
}

// Static files in public/ (the landing page) are served by Cloudflare before
// this runs; only paths with no matching file reach fetch().
#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let on = env.var("LOG_LEVEL").is_ok_and(|v| v.to_string() == "debug");
    let debug = DebugLog(on.then(RefCell::default));
    let (method, path) = (req.method().to_string(), req.path());
    match route(req, env, &debug).await {
        Ok(mut resp) => {
            debug.log(json!({"msg": "response", "method": method, "path": path, "status": resp.status_code()}));
            if let Some(entries) = debug.0 {
                resp.headers_mut().set("X-Debug-Log", &serde_json::to_string(&entries.into_inner())?)?;
            }
            Ok(resp)
        }
        Err(e) => {
            debug.log(json!({"msg": "error", "method": method, "path": path, "error": e.to_string()}));
            Err(e)
        }
    }
}

/// True when the request came from our own page. Browsers send Sec-Fetch-Site on every fetch;
/// older ones without it still send a Referer. Both can be faked, so this only deters casual use.
fn from_our_page(req: &Request, url: &Url) -> Result<bool> {
    if let Some(site) = req.headers().get("Sec-Fetch-Site")? {
        return Ok(site == "same-origin");
    }
    let referer = req.headers().get("Referer")?.and_then(|r| Url::parse(&r).ok());
    Ok(referer.is_some_and(|r| r.origin() == url.origin()))
}

async fn route(req: Request, env: Env, debug: &DebugLog) -> Result<Response> {
    // Cache by URL without the query string, so query strings can't bypass the cache.
    let mut key = req.url()?;
    key.set_query(None);

    if req.method() != Method::Get {
        return respond("Method not allowed".to_string(), 405, "text/plain;charset=UTF-8");
    }
    if !from_our_page(&req, &key)? {
        return respond("Forbidden".to_string(), 403, "text/plain;charset=UTF-8");
    }

    match req.path().as_str() {
        "/api/rainfall" => return cached(key.as_str(), debug, get_rainfall(&env, debug)).await,
        "/api/forecast" => return cached(key.as_str(), debug, async { Ok(json!({"days": get_forecast(debug).await?})) }).await,
        _ => {}
    }

    respond("Not found".to_string(), 404, "text/plain;charset=UTF-8")
}

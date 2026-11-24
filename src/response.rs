//! Building responses, and caching endpoint bodies in Cloudflare's cache.

use std::future::Future;

use serde_json::{json, Value};
use worker::{Cache, Date, Headers, Response};

use crate::error::Result;
use crate::json::PythonJson;
use crate::types::DebugLog;

const CACHE_SECONDS: u32 = 3600; // the station reports hourly

/// A response with the given body, status and Content-Type.
pub fn respond(body: String, status: u16, content_type: &str) -> Result<Response> {
    let headers = Headers::new();
    headers.set("Content-Type", content_type)?;
    Ok(Response::ok(body)?
        .with_status(status)
        .with_headers(headers))
}

/// Serve a JSON body from Cloudflare's cache, or build it, cache it for `CACHE_SECONDS` and serve it.
///
/// Each endpoint caches its finished body, so a cache hit makes no upstream API calls.
/// `key` must be a URL; the cache is per Cloudflare data center. With `use_cache` false (local dev,
/// see `DISABLE_CACHE` in .dev.vars.example) every request builds a fresh body that nothing stores.
pub async fn cached(
    key: &str,
    use_cache: bool,
    debug: &DebugLog,
    build: impl Future<Output = Result<Value>>,
) -> Result<Response> {
    let cache = Cache::default();
    let now = Date::now().as_millis() / 1000;
    if !use_cache {
        debug.log(json!({"msg": "cache disabled", "key": key}));
    } else if let Some(mut hit) = cache.get(key, false).await? {
        // Entries without X-Stored-At (cached before it existed) count as fully aged.
        let stored_at = hit
            .headers()
            .get("X-Stored-At")?
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let age = now.saturating_sub(stored_at).min(u64::from(CACHE_SECONDS));
        debug.log(json!({"msg": "cache hit", "key": key, "age_seconds": age}));
        return for_browser(hit.text().await?, u64::from(CACHE_SECONDS) - age);
    } else {
        debug.log(json!({"msg": "cache miss", "key": key}));
    }
    let mut body = Vec::new();
    serde::Serialize::serialize(
        &build.await?,
        &mut serde_json::Serializer::with_formatter(&mut body, PythonJson),
    )?;
    let body = String::from_utf8(body)?;
    if !use_cache {
        let mut resp = respond(body, 200, "application/json")?;
        resp.headers_mut().set("Cache-Control", "no-store")?; // the browser mustn't keep it either
        return Ok(resp);
    }
    let mut resp = respond(body.clone(), 200, "application/json")?;
    resp.headers_mut()
        .set("Cache-Control", &format!("max-age={CACHE_SECONDS}"))?;
    resp.headers_mut().set("X-Stored-At", &now.to_string())?;
    cache.put(key, resp).await?;
    for_browser(body, u64::from(CACHE_SECONDS))
}

/// A JSON response the visitor's browser may reuse for `max_age` seconds: the time left before
/// our cached copy expires, so browser copies never outlive it and the data is no staler.
fn for_browser(body: String, max_age: u64) -> Result<Response> {
    let mut resp = respond(body, 200, "application/json")?;
    resp.headers_mut()
        .set("Cache-Control", &format!("private, max-age={max_age}"))?;
    Ok(resp)
}

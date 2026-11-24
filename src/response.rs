//! Building responses, and caching endpoint bodies in Cloudflare's cache.

use std::future::Future;

use serde_json::{json, Value};
use worker::{Cache, Headers, Response};

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
    if !use_cache {
        debug.log(json!({"msg": "cache disabled", "key": key}));
    } else if let Some(mut hit) = cache.get(key, false).await? {
        debug.log(json!({"msg": "cache hit", "key": key}));
        return for_browser(hit.text().await?);
    } else {
        debug.log(json!({"msg": "cache miss", "key": key}));
    }
    let mut body = Vec::new();
    serde::Serialize::serialize(
        &build.await?,
        &mut serde_json::Serializer::with_formatter(&mut body, PythonJson),
    )?;
    let body = String::from_utf8(body)?;
    if use_cache {
        let mut resp = respond(body.clone(), 200, "application/json")?;
        resp.headers_mut()
            .set("Cache-Control", &format!("max-age={CACHE_SECONDS}"))?;
        cache.put(key, resp).await?;
    }
    for_browser(body)
}

/// A JSON response the visitor's browser mustn't store, so every page load gets the current data
/// from our cache (or a fresh build).
fn for_browser(body: String) -> Result<Response> {
    let mut resp = respond(body, 200, "application/json")?;
    resp.headers_mut().set("Cache-Control", "no-store")?;
    Ok(resp)
}

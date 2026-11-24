//! Request routing: which requests reach which endpoint.

use worker::{Env, Method, Request, Response, Url};

use crate::error::Result;
use crate::forecast::get_forecast;
use crate::precipitation::get_precipitation;
use crate::response::{cached, respond};
use crate::types::DebugLog;

/// True when the request came from our own page. Browsers send Sec-Fetch-Site on every fetch;
/// older ones without it still send a Referer. Both can be faked, so this only deters casual use.
fn from_our_page(req: &Request, url: &Url) -> Result<bool> {
    if let Some(site) = req.headers().get("Sec-Fetch-Site")? {
        return Ok(site == "same-origin");
    }
    let referer = req
        .headers()
        .get("Referer")?
        .and_then(|r| Url::parse(&r).ok());
    Ok(referer.is_some_and(|r| r.origin() == url.origin()))
}

/// Answer a request: a cached API endpoint for GETs from our own page, or a 403/404/405.
pub async fn route(req: Request, env: Env, debug: &DebugLog) -> Result<Response> {
    // Cache by URL without the query string, so query strings can't bypass the cache.
    let mut key = req.url()?;
    key.set_query(None);

    if req.method() != Method::Get {
        return respond(
            "Method not allowed".to_string(),
            405,
            "text/plain;charset=UTF-8",
        );
    }
    if !from_our_page(&req, &key)? {
        return respond("Forbidden".to_string(), 403, "text/plain;charset=UTF-8");
    }

    // Only .dev.vars sets DISABLE_CACHE, so staging and production always cache.
    let use_cache = !env
        .var("DISABLE_CACHE")
        .is_ok_and(|v| v.to_string() == "true");
    match req.path().as_str() {
        "/api/precipitation" => {
            return cached(
                key.as_str(),
                use_cache,
                debug,
                get_precipitation(&env, debug),
            )
            .await
        }
        "/api/forecast" => {
            return cached(key.as_str(), use_cache, debug, get_forecast(debug)).await
        }
        _ => {}
    }

    respond("Not found".to_string(), 404, "text/plain;charset=UTF-8")
}

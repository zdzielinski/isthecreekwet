//! Request routing: which requests reach which endpoint.

use worker::{Env, Method, Request, Response, Url};

use crate::error::{Error, Result};
use crate::forecast::get_forecast;
use crate::precipitation::get_precipitation;
use crate::response::{cached, respond};
use crate::types::DebugLog;

/// The Worker's config, embedded at build time for its `CACHE_VERSION`.
const WRANGLER_TOML: &str = include_str!("../wrangler.toml");

/// `CACHE_VERSION` from wrangler.toml's `[vars]`, the one place it's set. Increasing it gives
/// every edge cache key a new version, so responses cached by the previous release aren't served.
fn cache_version(wrangler_toml: &str) -> Result<&str> {
    wrangler_toml
        .lines()
        .find_map(|l| l.strip_prefix("CACHE_VERSION = \"")?.strip_suffix('"'))
        .filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
        .ok_or(Error::MissingCacheVersion)
}

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
    // Cache by URL with our own query string, so visitors' query strings can't bypass the cache.
    let mut key = req.url()?;
    key.set_query(Some(&format!("v={}", cache_version(WRANGLER_TOML)?)));

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_version_reads_wrangler_toml() {
        assert!(cache_version(WRANGLER_TOML).is_ok_and(|v| v.parse::<u32>().is_ok()));
    }

    #[test]
    fn test_cache_version_finds_line() {
        let toml = "[vars]\nCACHE_VERSION = \"42\"\nLOG_LEVEL = \"debug\"";
        assert!(cache_version(toml).is_ok_and(|v| v == "42"));
    }

    #[test]
    fn test_cache_version_rejects_missing_or_bad_value() {
        assert!(matches!(
            cache_version("[vars]"),
            Err(Error::MissingCacheVersion)
        ));
        assert!(matches!(
            cache_version("CACHE_VERSION = 3"),
            Err(Error::MissingCacheVersion)
        ));
        assert!(matches!(
            cache_version("CACHE_VERSION = \"\""),
            Err(Error::MissingCacheVersion)
        ));
    }
}

//! Is the creek wet? Recent precipitation from the Utah Climate Center API.
//!
//! API docs: <https://logansw.atlassian.net/wiki/spaces/UCCA/overview>

mod error;
mod forecast;
mod json;
mod precipitation;
mod response;
mod routes;
mod types;

use std::cell::RefCell;

use serde_json::json;
use worker::{event, Context, Env, Request, Response};

use crate::types::DebugLog;

// Static files in public/ (the landing page) are served by Cloudflare before
// this runs; only paths with no matching file reach fetch().
#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> worker::Result<Response> {
    let on = env.var("LOG_LEVEL").is_ok_and(|v| v.to_string() == "debug");
    let debug = DebugLog(on.then(RefCell::default));
    let (method, path) = (req.method().to_string(), req.path());
    match routes::route(req, env, &debug).await {
        Ok(mut resp) => {
            debug.log(json!({"msg": "response", "method": method, "path": path, "status": resp.status_code()}));
            if let Some(entries) = debug.0 {
                resp.headers_mut().set(
                    "X-Debug-Log",
                    &serde_json::to_string(&entries.into_inner())?,
                )?;
            }
            Ok(resp)
        }
        Err(e) => {
            debug.log(
                json!({"msg": "error", "method": method, "path": path, "error": e.to_string()}),
            );
            Err(e.into())
        }
    }
}

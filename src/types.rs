//! Types shared across modules.

use std::cell::RefCell;

use serde_json::Value;
use worker::console_debug;

/// One request's debug log, on when `LOG_LEVEL=debug` (previews and `wrangler dev`; see wrangler.toml).
///
/// Each entry goes to the Workers console (Workers Logs indexes its fields) and back to the
/// page in the X-Debug-Log header, which index.html prints to the browser console.
pub struct DebugLog(pub Option<RefCell<Vec<Value>>>);

impl DebugLog {
    /// Record `entry` when debug logging is on; does nothing otherwise.
    pub fn log(&self, entry: Value) {
        if let Some(entries) = &self.0 {
            console_debug!("{entry}");
            entries.borrow_mut().push(entry);
        }
    }
}

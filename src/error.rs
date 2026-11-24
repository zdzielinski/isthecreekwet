//! The crate's error type.

/// Everything that can go wrong while handling a request.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A Workers runtime call failed (fetch, cache, headers, secrets).
    #[error("Workers runtime error: {0}")]
    Worker(#[from] worker::Error),

    /// A JSON body couldn't be serialized.
    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    /// An upstream URL couldn't be built.
    #[error("invalid upstream URL: {0}")]
    Url(#[from] url::ParseError),

    /// A date or time from an upstream API didn't parse.
    #[error("{source}: {input}")]
    Time {
        input: String,
        source: time::error::Parse,
    },

    /// A date couldn't be formatted for display.
    #[error("date formatting error: {0}")]
    Format(#[from] time::error::Format),

    /// The USU API answered without `"success": true`.
    #[error("USU API error for station {station}: {body}")]
    UsuApi { station: &'static str, body: String },

    /// The NWS API answered with a non-2xx status.
    #[error("NWS API error: {status}")]
    NwsApi { status: u16 },

    /// The NWS forecast came back without any forecast periods.
    #[error("NWS forecast has no periods")]
    NoForecastPeriods,

    /// wrangler.toml has no `CACHE_VERSION = "<number>"` line to build cache keys from.
    #[error("CACHE_VERSION not found in wrangler.toml")]
    MissingCacheVersion,

    /// A numeric field from an upstream API held something else.
    #[error("{field} is not a number: {value}")]
    NotANumber { field: &'static str, value: String },

    /// A serialized body wasn't valid UTF-8.
    #[error("response body is not UTF-8: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),
}

/// `Result` with the crate's [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

impl From<Error> for worker::Error {
    fn from(e: Error) -> Self {
        match e {
            Error::Worker(e) => e,
            e => Self::RustError(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_error_returns_worker_errors_unchanged() {
        let e: worker::Error = Error::Worker(worker::Error::RustError("boom".to_string())).into();
        assert!(matches!(e, worker::Error::RustError(s) if s == "boom"));
    }

    #[test]
    fn test_from_error_wraps_other_errors_with_their_message() {
        let e: worker::Error = Error::NwsApi { status: 503 }.into();
        assert!(matches!(e, worker::Error::RustError(s) if s == "NWS API error: 503"));
    }

    #[test]
    fn test_error_display_describes_wrapped_errors() {
        let json = serde_json::from_str::<u8>("x").map_err(Error::from).err();
        assert!(json.is_some_and(|e| e.to_string().starts_with("JSON serialization error: ")));
        let url = url::Url::parse("not a url").map_err(Error::from).err();
        assert!(url
            .is_some_and(|e| e.to_string() == "invalid upstream URL: relative URL without a base"));
    }
}

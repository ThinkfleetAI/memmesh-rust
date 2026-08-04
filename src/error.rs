//! Error type for the MemMesh client.

use std::time::Duration;

/// Anything that can go wrong talking to the MemMesh API.
///
/// Non-2xx responses are mapped to a granular variant by status code (see
/// [`Error::from_status`]). Anything the mapping doesn't recognise falls back
/// to [`Error::Api`], so callers can always inspect `status` + `body`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Transport failure (connection, TLS) surfaced directly by reqwest.
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    /// Response body didn't match the expected shape.
    #[error("decode error: {0}")]
    Decode(#[from] serde_json::Error),
    /// 401 — invalid or missing API key.
    #[error("authentication failed (401): {message}")]
    Authentication { message: String },
    /// 403 — authenticated but not permitted.
    #[error("authorization failed (403): {message}")]
    Authorization { message: String },
    /// 404 — resource not found.
    #[error("not found (404): {message}")]
    NotFound { message: String },
    /// 400 / 422 — request failed validation.
    #[error("validation failed ({status}): {message}")]
    Validation { status: u16, message: String },
    /// 429 — rate limited. `retry_after` is the server's `Retry-After`, if sent.
    #[error("rate limited (429): {message}")]
    RateLimit {
        message: String,
        retry_after: Option<Duration>,
    },
    /// 5xx — server-side failure.
    #[error("server error ({status}): {message}")]
    Server { status: u16, message: String },
    /// The request exceeded its configured timeout.
    #[error("request timed out")]
    Timeout,
    /// A transport-level failure (connect/read) after retries were exhausted.
    #[error("network error: {0}")]
    Network(String),
    /// A non-2xx response not covered by a more specific variant.
    #[error("memmesh api error {status}: {body}")]
    Api { status: u16, body: String },
}

impl Error {
    /// Map an HTTP status + response body to the most specific variant.
    ///
    /// `retry_after` is only meaningful for 429 and is ignored otherwise.
    pub(crate) fn from_status(status: u16, body: String, retry_after: Option<Duration>) -> Error {
        let message = extract_message(&body).unwrap_or_else(|| {
            if body.is_empty() {
                format!("HTTP {status}")
            } else {
                body.clone()
            }
        });
        match status {
            401 => Error::Authentication { message },
            403 => Error::Authorization { message },
            404 => Error::NotFound { message },
            400 | 422 => Error::Validation { status, message },
            429 => Error::RateLimit {
                message,
                retry_after,
            },
            s if s >= 500 => Error::Server { status, message },
            _ => Error::Api { status, body },
        }
    }
}

/// Pull a human message out of a JSON error body (`message` or `error` key).
fn extract_message(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    v.get("message")
        .or_else(|| v.get("error"))
        .and_then(|m| m.as_str())
        .map(|s| s.to_string())
}

//! Error type for the MemMesh client.

/// Anything that can go wrong talking to the MemMesh API.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Transport failure (connection, timeout, TLS).
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    /// Response body didn't match the expected shape.
    #[error("decode error: {0}")]
    Decode(#[from] serde_json::Error),
    /// A non-2xx response from the API.
    #[error("memmesh api error {status}: {body}")]
    Api { status: u16, body: String },
}

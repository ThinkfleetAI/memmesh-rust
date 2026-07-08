//! Official Rust SDK for [MemMesh](https://memmesh.ai) — memory + prediction
//! for AI agents.
//!
//! ```no_run
//! # async fn run() -> Result<(), memmesh::Error> {
//! use memmesh::{MemMesh, Subject, Observe};
//!
//! let mm = MemMesh::new("sk-...", "proj_...");
//!
//! mm.memory().observe(Observe {
//!     subject: Some(Subject::new("contact", "sarah")),
//!     content: "Prefers email over phone.".into(),
//!     ..Default::default()
//! }).await?;
//!
//! let hits = mm.memory().search("how to reach sarah", 5).await?;
//! let insights = mm.memory().reflect(Default::default()).await?;
//! # Ok(()) }
//! ```

use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde::Serialize;

mod error;
mod types;

pub mod context;
pub mod lattice;
pub mod memory;
pub mod resources;

pub use context::Context;
pub use error::Error;
pub use lattice::Lattice;
pub use memory::{Memory, Observe, ReflectOpts};
pub use types::*;

const DEFAULT_BASE_URL: &str = "https://app.memmesh.ai";

pub(crate) struct Inner {
    http: reqwest::Client,
    base: String,
    key: String,
    project: String,
}

/// The MemMesh client. Cheap to clone (`Arc` inside); construct once and share.
#[derive(Clone)]
pub struct MemMesh {
    inner: Arc<Inner>,
}

impl MemMesh {
    /// Create a client with your `sk-...` API key and default project id.
    pub fn new(api_key: impl Into<String>, project_id: impl Into<String>) -> Self {
        Self::with_base_url(api_key, project_id, DEFAULT_BASE_URL)
    }

    /// Create a client pointed at a custom base URL (self-hosted / region).
    pub fn with_base_url(
        api_key: impl Into<String>,
        project_id: impl Into<String>,
        base_url: impl Into<String>,
    ) -> Self {
        MemMesh {
            inner: Arc::new(Inner {
                http: reqwest::Client::new(),
                base: base_url.into().trim_end_matches('/').to_string(),
                key: api_key.into(),
                project: project_id.into(),
            }),
        }
    }

    pub fn memory(&self) -> Memory {
        Memory { c: self.inner.clone() }
    }
    pub fn lattice(&self) -> Lattice {
        Lattice { c: self.inner.clone() }
    }
    pub fn context(&self) -> Context {
        Context { c: self.inner.clone() }
    }
    pub fn events(&self) -> resources::Events {
        resources::Events { c: self.inner.clone() }
    }
    pub fn alerts(&self) -> resources::Alerts {
        resources::Alerts { c: self.inner.clone() }
    }
    pub fn learning(&self) -> resources::Learning {
        resources::Learning { c: self.inner.clone() }
    }
    pub fn compliance(&self) -> resources::Compliance {
        resources::Compliance { c: self.inner.clone() }
    }
    pub fn health(&self) -> resources::Health {
        resources::Health { c: self.inner.clone() }
    }
}

impl Inner {
    fn url(&self, path: &str) -> String {
        format!("{}/api/v1/projects/{}{}", self.base, self.project, path)
    }

    pub(crate) async fn send<B: Serialize, T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&B>,
    ) -> Result<T, Error> {
        let mut req = self
            .http
            .request(method, self.url(path))
            .bearer_auth(&self.key);
        if let Some(b) = body {
            req = req.json(b);
        }
        let resp = req.send().await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(Error::Api { status: status.as_u16(), body: text });
        }
        if text.is_empty() {
            // Caller expects T; empty body only valid for unit-like T.
            return serde_json::from_str("null").map_err(Into::into);
        }
        serde_json::from_str(&text).map_err(Into::into)
    }
}

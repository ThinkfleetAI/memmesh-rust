//! Official Rust SDK for [MemMesh](https://memmesh.ai) — memory + prediction
//! for AI agents.
//!
//! ```no_run
//! # async fn run() -> Result<(), memmesh::Error> {
//! use memmesh::{MemMesh, Observe};
//!
//! let mm = MemMesh::new("sk-...", "proj_...");
//!
//! // Hand the engine the raw turn — it extracts what's worth keeping and
//! // returns { saved, candidate_count }; filler comes back as saved: [].
//! let res = mm.memory().observe(Observe {
//!     text: Some("Sarah prefers email over phone.".into()),
//!     ..Default::default()
//! }).await?;
//! println!("kept {} of {}", res.saved.len(), res.candidate_count);
//!
//! let hits = mm.memory().search("how to reach sarah", 5).await?;
//! let insights = mm.memory().reflect(Default::default()).await?;
//! # Ok(()) }
//! ```

use std::sync::Arc;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Serialize;

mod error;
mod interceptor;
mod pagination;
#[cfg(test)]
mod test_support;
mod types;

pub mod brains;
pub mod consent;
pub mod context;
pub mod financial;
pub mod lattice;
pub mod memory;
pub mod resources;
pub mod typed;

pub use brains::Brains;
pub use consent::Consent;
pub use context::Context;
pub use error::Error;
pub use financial::Financial;
pub use interceptor::{BoxFuture, RequestInterceptor, ResponseInterceptor};
pub use lattice::Lattice;
pub use memory::{
    render_procedure_content, ConsolidateOpts, DedupOpts, IngestMedia, ListParams, Memory, Observe,
    ObserveAttachment, Procedure, ReflectOpts, SubmitFeedback, UpdateMemory,
};
pub use typed::{
    Accumulator, AccumulatorParams, AttributeDataType, AttributeDef, EnqueueReport, IngestReport,
    ListAttributesParams, ObservationStatus, QueryObservationsParams, RegisterAttributeRequest,
    TypedAttributes, TypedObservation, TypedObservationInput,
};
pub use pagination::{list_all, SeekPage};
pub use types::*;

const DEFAULT_BASE_URL: &str = "https://app.memmesh.ai";
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_MAX_RETRIES: u32 = 2;
/// Backoff is capped at this ceiling before jitter, matching the TS SDK.
const MAX_BACKOFF_MS: u64 = 10_000;

/// Per-call overrides threaded through [`Inner::send_with`]. Cheap to clone;
/// build with `..Default::default()`.
#[derive(Debug, Default, Clone)]
pub struct RequestOptions {
    /// Override the client's default project id for this one call.
    pub project_id: Option<String>,
    /// Override the client's default request timeout for this one call.
    pub timeout: Option<Duration>,
}

pub(crate) struct Inner {
    http: reqwest::Client,
    base: String,
    key: String,
    project: String,
    timeout: Duration,
    max_retries: u32,
    request_interceptors: Vec<RequestInterceptor>,
    response_interceptors: Vec<ResponseInterceptor>,
}

/// Builder for [`MemMesh`] — the escape hatch for tuning timeout, retries, a
/// custom `reqwest::Client`, and request/response interceptors. The positional
/// [`MemMesh::new`] / [`MemMesh::with_base_url`] constructors cover the common
/// case; reach for the builder when you need more.
///
/// ```no_run
/// # use std::time::Duration;
/// use memmesh::MemMesh;
///
/// let mm = MemMesh::builder("sk-...", "proj_...")
///     .base_url("https://memory.internal")
///     .timeout(Duration::from_secs(10))
///     .max_retries(4)
///     .build();
/// # let _ = mm;
/// ```
pub struct MemMeshBuilder {
    key: String,
    project: String,
    base: String,
    timeout: Duration,
    max_retries: u32,
    http: Option<reqwest::Client>,
    request_interceptors: Vec<RequestInterceptor>,
    response_interceptors: Vec<ResponseInterceptor>,
}

impl MemMeshBuilder {
    fn new(api_key: impl Into<String>, project_id: impl Into<String>) -> Self {
        MemMeshBuilder {
            key: api_key.into(),
            project: project_id.into(),
            base: DEFAULT_BASE_URL.to_string(),
            timeout: DEFAULT_TIMEOUT,
            max_retries: DEFAULT_MAX_RETRIES,
            http: None,
            request_interceptors: Vec::new(),
            response_interceptors: Vec::new(),
        }
    }

    /// Point the client at a custom base URL (self-hosted / region).
    pub fn base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base = base_url.into().trim_end_matches('/').to_string();
        self
    }

    /// Default request timeout (overridable per-call via [`RequestOptions`]).
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Max retries for 429 + 5xx (and transient network errors). Default 2.
    pub fn max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// Supply a pre-configured `reqwest::Client` (proxies, connection pools,
    /// custom TLS). Per-request timeouts are still applied on top.
    pub fn http_client(mut self, client: reqwest::Client) -> Self {
        self.http = Some(client);
        self
    }

    /// Add a hook that rewrites each outgoing request (e.g. swap the bearer key
    /// for a Cognito JWT). Interceptors run in the order added, on every attempt.
    pub fn request_interceptor(mut self, interceptor: RequestInterceptor) -> Self {
        self.request_interceptors.push(interceptor);
        self
    }

    /// Add a hook that observes each response before its body is read.
    pub fn response_interceptor(mut self, interceptor: ResponseInterceptor) -> Self {
        self.response_interceptors.push(interceptor);
        self
    }

    /// Finalize into a ready-to-use [`MemMesh`].
    pub fn build(self) -> MemMesh {
        MemMesh {
            inner: Arc::new(Inner {
                http: self.http.unwrap_or_default(),
                base: self.base,
                key: self.key,
                project: self.project,
                timeout: self.timeout,
                max_retries: self.max_retries,
                request_interceptors: self.request_interceptors,
                response_interceptors: self.response_interceptors,
            }),
        }
    }
}

/// The MemMesh client. Cheap to clone (`Arc` inside); construct once and share.
#[derive(Clone)]
pub struct MemMesh {
    inner: Arc<Inner>,
}

impl MemMesh {
    /// Create a client with your `sk-...` API key and default project id.
    pub fn new(api_key: impl Into<String>, project_id: impl Into<String>) -> Self {
        MemMeshBuilder::new(api_key, project_id).build()
    }

    /// Create a client pointed at a custom base URL (self-hosted / region).
    pub fn with_base_url(
        api_key: impl Into<String>,
        project_id: impl Into<String>,
        base_url: impl Into<String>,
    ) -> Self {
        MemMeshBuilder::new(api_key, project_id).base_url(base_url).build()
    }

    /// Start a [`MemMeshBuilder`] for full control over timeout, retries, a
    /// custom HTTP client, and interceptors.
    pub fn builder(
        api_key: impl Into<String>,
        project_id: impl Into<String>,
    ) -> MemMeshBuilder {
        MemMeshBuilder::new(api_key, project_id)
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
    pub fn behaviors(&self) -> resources::Behaviors {
        resources::Behaviors { c: self.inner.clone() }
    }
    pub fn compliance(&self) -> resources::Compliance {
        resources::Compliance { c: self.inner.clone() }
    }
    pub fn health(&self) -> resources::Health {
        resources::Health { c: self.inner.clone() }
    }
    /// The financial vertical — market-data ingestion, indicators, portfolio
    /// risk, and the self-calibrating directional prediction loop.
    pub fn financial(&self) -> Financial {
        Financial { c: self.inner.clone() }
    }
    /// The brains marketplace registry (create / list / get / update / delete).
    pub fn brains(&self) -> Brains {
        Brains { c: self.inner.clone() }
    }
    /// Subject-level consent / opt-out, recorded client-side as `consent` memories.
    pub fn consent(&self) -> Consent {
        Consent { c: self.inner.clone() }
    }
    /// Typed attributes — structured/numeric observations + running accumulators.
    pub fn typed(&self) -> TypedAttributes {
        TypedAttributes { c: self.inner.clone() }
    }
}

/// Exponential backoff with full-range jitter, capped at [`MAX_BACKOFF_MS`].
///
/// `attempt` is the 1-based retry number (1 for the first retry). `rand_factor`
/// is a value in `[0, 1)`; the jittered delay lands in `[base/2, base]`. Pure
/// so it can be unit-tested without a clock.
fn backoff_delay(attempt: u32, rand_factor: f64) -> Duration {
    let exp = 2u64.saturating_pow(attempt.saturating_sub(1));
    let base = 500u64.saturating_mul(exp).min(MAX_BACKOFF_MS);
    let factor = 0.5 + 0.5 * rand_factor.clamp(0.0, 1.0);
    Duration::from_millis((base as f64 * factor) as u64)
}

/// Cheap jitter source in `[0, 1)` — avoids pulling in the `rand` crate.
fn jitter() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    (nanos % 1000) as f64 / 1000.0
}

/// Parse a `Retry-After` header expressed in whole seconds.
fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

impl Inner {
    fn url(&self, path: &str, opts: &RequestOptions) -> String {
        let project = opts.project_id.as_deref().unwrap_or(&self.project);
        format!("{}/api/v1/projects/{}{}", self.base, project, path)
    }

    /// Issue a request with default options. Kept as the ergonomic entry point
    /// every resource calls.
    pub(crate) async fn send<B: Serialize, T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&B>,
    ) -> Result<T, Error> {
        self.send_with(method, path, body, &RequestOptions::default()).await
    }

    /// Issue a request, applying per-call [`RequestOptions`], timeout, retry
    /// with backoff on 429/5xx honoring `Retry-After`, and interceptors.
    pub(crate) async fn send_with<B: Serialize, T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&B>,
        opts: &RequestOptions,
    ) -> Result<T, Error> {
        let url = self.url(path, opts);
        let timeout = opts.timeout.unwrap_or(self.timeout);
        let mut attempt: u32 = 0;

        loop {
            // Rebuild per attempt: sending consumes the builder, and the body
            // must be re-serialized for each retry.
            let mut req = self
                .http
                .request(method.clone(), &url)
                .bearer_auth(&self.key)
                .timeout(timeout);
            if let Some(b) = body {
                req = req.json(b);
            }
            for ic in &self.request_interceptors {
                req = ic(req).await?;
            }

            let resp = match req.send().await {
                Ok(r) => r,
                Err(e) => {
                    if e.is_timeout() {
                        return Err(Error::Timeout);
                    }
                    // Transient transport failure: retry, then give up.
                    if attempt < self.max_retries {
                        tokio::time::sleep(backoff_delay(attempt + 1, jitter())).await;
                        attempt += 1;
                        continue;
                    }
                    return Err(Error::Network(e.to_string()));
                }
            };

            for ic in &self.response_interceptors {
                ic(&resp);
            }

            let status = resp.status();
            if status.is_success() {
                let text = resp.text().await?;
                if text.is_empty() {
                    // Caller expects T; empty body only valid for unit-like T.
                    return serde_json::from_str("null").map_err(Into::into);
                }
                return serde_json::from_str(&text).map_err(Into::into);
            }

            let code = status.as_u16();
            let retry_after = parse_retry_after(resp.headers());
            let text = resp.text().await.unwrap_or_default();

            // Retry 429 + 5xx; honor Retry-After over computed backoff.
            if (code == 429 || code >= 500) && attempt < self.max_retries {
                let delay = retry_after.unwrap_or_else(|| backoff_delay(attempt + 1, jitter()));
                tokio::time::sleep(delay).await;
                attempt += 1;
                continue;
            }

            return Err(Error::from_status(code, text, retry_after));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_and_caps() {
        // First retry (attempt=1): base 500ms. Full jitter -> exactly base.
        assert_eq!(backoff_delay(1, 1.0), Duration::from_millis(500));
        // Zero jitter -> half of base.
        assert_eq!(backoff_delay(1, 0.0), Duration::from_millis(250));
        // Doubles each attempt: 500 -> 1000 -> 2000.
        assert_eq!(backoff_delay(2, 1.0), Duration::from_millis(1000));
        assert_eq!(backoff_delay(3, 1.0), Duration::from_millis(2000));
        // Capped at MAX_BACKOFF_MS regardless of attempt count.
        assert_eq!(backoff_delay(30, 1.0), Duration::from_millis(MAX_BACKOFF_MS));
    }

    #[test]
    fn backoff_stays_in_half_to_full_range() {
        for attempt in 1..=8u32 {
            let exp = 2u64.saturating_pow(attempt - 1);
            let base = (500 * exp).min(MAX_BACKOFF_MS);
            for &f in &[0.0, 0.25, 0.5, 0.9999] {
                let d = backoff_delay(attempt, f).as_millis() as u64;
                assert!(d >= base / 2 && d <= base, "attempt={attempt} f={f} d={d} base={base}");
            }
        }
    }

    #[test]
    fn jitter_is_unit_range() {
        for _ in 0..100 {
            let j = jitter();
            assert!((0.0..1.0).contains(&j));
        }
    }

    #[test]
    fn from_status_maps_variants() {
        assert!(matches!(
            Error::from_status(401, String::new(), None),
            Error::Authentication { .. }
        ));
        assert!(matches!(
            Error::from_status(422, r#"{"message":"bad"}"#.into(), None),
            Error::Validation { status: 422, .. }
        ));
        assert!(matches!(
            Error::from_status(503, String::new(), None),
            Error::Server { status: 503, .. }
        ));
        match Error::from_status(429, String::new(), Some(Duration::from_secs(5))) {
            Error::RateLimit { retry_after, .. } => {
                assert_eq!(retry_after, Some(Duration::from_secs(5)))
            }
            other => panic!("expected RateLimit, got {other:?}"),
        }
        // Unmapped status falls back to Api.
        assert!(matches!(
            Error::from_status(418, "teapot".into(), None),
            Error::Api { status: 418, .. }
        ));
    }
}

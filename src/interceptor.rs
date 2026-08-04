//! Request/response hooks — the Rust analogue of the TS SDK's
//! `requestInterceptors` / `responseInterceptors`.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::Error;

/// A boxed, `Send` future — the return type async hooks produce.
pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// Hook called before each request is sent (on every attempt, including
/// retries). Use it to customize the outgoing request — most usefully to swap
/// the static bearer key for a freshly-minted token:
///
/// ```no_run
/// use memmesh::{MemMesh, RequestInterceptor};
/// use std::sync::Arc;
///
/// // Replace the SDK's bearer auth with a Cognito JWT fetched per request.
/// let jwt: RequestInterceptor = Arc::new(|req: reqwest::RequestBuilder| {
///     Box::pin(async move {
///         let token = "cognito-jwt".to_string(); // e.g. await a token provider
///         Ok(req.bearer_auth(token))
///     })
/// });
///
/// let mm = MemMesh::builder("sk-...", "proj_...")
///     .request_interceptor(jwt)
///     .build();
/// # let _ = mm;
/// ```
pub type RequestInterceptor = Arc<
    dyn Fn(reqwest::RequestBuilder) -> BoxFuture<Result<reqwest::RequestBuilder, Error>>
        + Send
        + Sync,
>;

/// Hook called after each response arrives, before the body is read. Useful for
/// logging/metrics on status + headers. Purely observational.
pub type ResponseInterceptor = Arc<dyn Fn(&reqwest::Response) + Send + Sync>;

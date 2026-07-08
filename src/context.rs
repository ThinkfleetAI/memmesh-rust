//! Context — LLM-ready bundles + temporal knowledge-graph queries.

use std::sync::Arc;

use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{Error, GraphEdge, Inner, Subject};

/// Accessor for the context API. Get one via [`crate::MemMesh::context`].
pub struct Context {
    pub(crate) c: Arc<Inner>,
}

/// Point-in-time graph filter. `as_of` is RFC3339; `None` = current graph.
#[derive(Debug, Default)]
pub struct GraphQuery {
    pub subject_id: Option<String>,
    pub predicate: Option<String>,
    pub as_of: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Deserialize)]
struct Batch {
    bundles: Vec<Value>,
}
#[derive(Deserialize)]
struct Edges {
    edges: Vec<GraphEdge>,
}

impl Context {
    /// Unified, token-budgeted context bundle for one subject.
    pub async fn build(&self, subject: &Subject) -> Result<Value, Error> {
        let body = json!({"subject": subject});
        self.c.send(Method::POST, "/lattice/context", Some(&body)).await
    }

    /// Bundles for many subjects (<=500) in one call.
    pub async fn batch_build(&self, subjects: &[Subject]) -> Result<Vec<Value>, Error> {
        let body = json!({"subjects": subjects});
        let r: Batch = self.c.send(Method::POST, "/lattice/context/batch", Some(&body)).await?;
        Ok(r.bundles)
    }

    /// Edges valid AT `as_of` (or current), filtered by subject/predicate.
    pub async fn query_graph(&self, q: GraphQuery) -> Result<Vec<GraphEdge>, Error> {
        let mut body = json!({});
        if let Some(s) = q.subject_id { body["subjectId"] = json!(s); }
        if let Some(p) = q.predicate { body["predicate"] = json!(p); }
        if let Some(a) = q.as_of { body["asOf"] = json!(a); }
        if let Some(l) = q.limit { body["limit"] = json!(l); }
        let r: Edges = self.c.send(Method::POST, "/lattice/graph/query", Some(&body)).await?;
        Ok(r.edges)
    }
}

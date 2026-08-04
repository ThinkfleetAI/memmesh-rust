//! Context — LLM-ready bundles + temporal knowledge-graph queries.

use std::sync::Arc;

use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{ContextBuildRequest, ContextBundle, Error, GraphEdge, Inner, Subject};

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
    ///
    /// Pass a [`ContextBuildRequest`] to select sections (`include`), cap the
    /// budget (`max_tokens`), bound memories/predictions, or drop sensitive
    /// categories (`exclude_categories`). For the all-sections default,
    /// [`ContextBuildRequest::new`] / [`Context::build_for`] keep it terse.
    ///
    /// ```no_run
    /// # async fn run(mm: memmesh::MemMesh) -> Result<(), memmesh::Error> {
    /// use memmesh::{ContextBuildRequest, ContextSection, Subject};
    ///
    /// let ctx = mm.context().build(ContextBuildRequest {
    ///     subject: Subject::new("contact", "sarah-pizza"),
    ///     include: Some(vec![ContextSection::Profile, ContextSection::Predictions]),
    ///     max_tokens: Some(1500),
    ///     ..Default::default()
    /// }).await?;
    /// println!("{}", ctx.tokens_estimate);
    /// # Ok(()) }
    /// ```
    pub async fn build(&self, body: ContextBuildRequest) -> Result<ContextBundle, Error> {
        self.c.send(Method::POST, "/lattice/context", Some(&body)).await
    }

    /// Terse helper: an all-sections, default-budget bundle for one subject.
    /// Equivalent to `build(ContextBuildRequest::new(subject))`.
    pub async fn build_for(&self, subject: &Subject) -> Result<ContextBundle, Error> {
        self.build(ContextBuildRequest::new(subject.clone())).await
    }

    /// Bundles for many subjects (<=500) in one call.
    pub async fn batch_build(&self, subjects: &[Subject]) -> Result<Vec<Value>, Error> {
        let body = json!({ "subjects": subjects });
        let r: Batch = self.c.send(Method::POST, "/lattice/context/batch", Some(&body)).await?;
        Ok(r.bundles)
    }

    /// Edges valid AT `as_of` (or current), filtered by subject/predicate.
    pub async fn query_graph(&self, q: GraphQuery) -> Result<Vec<GraphEdge>, Error> {
        let mut body = json!({});
        if let Some(s) = q.subject_id {
            body["subjectId"] = json!(s);
        }
        if let Some(p) = q.predicate {
            body["predicate"] = json!(p);
        }
        if let Some(a) = q.as_of {
            body["asOf"] = json!(a);
        }
        if let Some(l) = q.limit {
            body["limit"] = json!(l);
        }
        let r: Edges = self.c.send(Method::POST, "/lattice/graph/query", Some(&body)).await?;
        Ok(r.edges)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{client, mock_server};
    use crate::ContextSection;

    #[tokio::test]
    async fn build_posts_options_and_parses_bundle() {
        let body = json!({
            "subject": { "kind": "contact", "externalId": "sarah" },
            "profile": {
                "rfmSegment": "at_risk_high_value", "recencyScore": 0.3,
                "frequencyScore": null, "monetaryScore": null,
                "topEntity": "Tony's", "cadenceSummary": "weekly",
                "risks": [{
                    "kind": "declining_engagement", "description": "slowing",
                    "severity": 0.6, "sourcePatternId": "p9",
                }],
            },
            "patterns": [{
                "id": "p1", "patternKind": "recurring_event", "summary": "weekly pizza",
                "confidence": 0.8, "nextExpectedAt": "2026-02-01T00:00:00Z",
            }],
            "predictions": [],
            "memories": [{
                "id": "m1", "type": "fact", "content": "likes pizza",
                "importance": 5.0, "created": "2026-01-01T00:00:00Z",
            }],
            "observations": [],
            "provenance": { "memoryIds": ["m1"], "patternIds": ["p1"], "observationIds": [] },
            "tokensEstimate": 1342,
            "truncated": ["memories"],
            "generatedAt": "2026-01-01T00:00:00Z", "durationMs": 11,
        })
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let ctx = mm
            .context()
            .build(ContextBuildRequest {
                subject: Subject::new("contact", "sarah"),
                include: Some(vec![ContextSection::Profile, ContextSection::Predictions, ContextSection::Memories]),
                max_tokens: Some(1500),
                memory_limit: Some(10),
                exclude_categories: Some(vec!["medical".into()]),
                ..Default::default()
            })
            .await
            .unwrap();

        assert_eq!(ctx.profile.as_ref().unwrap().rfm_segment.as_deref(), Some("at_risk_high_value"));
        assert_eq!(ctx.patterns.len(), 1);
        assert_eq!(ctx.memories[0].id, "m1");
        assert_eq!(ctx.tokens_estimate, 1342);
        assert_eq!(ctx.truncated, vec![ContextSection::Memories]);
        assert_eq!(ctx.provenance.memory_ids, vec!["m1"]);

        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        assert!(req.path.ends_with("/lattice/context"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["subject"]["externalId"], "sarah");
        assert_eq!(b["maxTokens"], 1500);
        assert_eq!(b["memoryLimit"], 10);
        assert_eq!(b["include"], json!(["profile", "predictions", "memories"]));
        assert_eq!(b["excludeCategories"], json!(["medical"]));
        // Unset options omitted, not sent as null.
        assert!(b.get("predictionLimit").is_none());
    }

    #[tokio::test]
    async fn build_for_sends_only_subject() {
        let body = json!({
            "subject": { "kind": "contact", "externalId": "sarah" },
            "profile": null,
            "patterns": [], "predictions": [], "memories": [], "observations": [],
            "provenance": { "memoryIds": [], "patternIds": [], "observationIds": [] },
            "tokensEstimate": 0, "truncated": [],
            "generatedAt": "2026-01-01T00:00:00Z", "durationMs": 2,
        })
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let ctx = mm.context().build_for(&Subject::new("contact", "sarah")).await.unwrap();
        assert!(ctx.profile.is_none());
        let req = rx.recv().unwrap();
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["subject"]["externalId"], "sarah");
        // Backward-compatible default: no options on the wire.
        assert!(b.get("include").is_none());
        assert!(b.get("maxTokens").is_none());
    }
}

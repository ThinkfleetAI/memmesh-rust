//! Knowledge graph — the structural half of memory.
//!
//! Observing text doesn't only produce embeddable rows; extraction also
//! resolves entities and writes typed edges between them. That graph is what
//! reaches a fact no single memory states outright ("who does Sarah report
//! to?" answered from `sarah -[member_of]-> team` plus `team -[led_by]-> priya`).
//!
//! Entities and edges are bi-temporal: `valid_from` / `valid_to` say when the
//! fact was TRUE in the world, which is not the same as when we believed it.
//! The read routes return only currently-believed edges, so a row superseded by
//! a contradicting one simply stops appearing.
//!
//! Read-only by design. Entities and edges are written by extraction when you
//! [`Memory::observe`](crate::memory::Memory::observe); the server's manual
//! create/retire routes exist for annotation tooling, and exposing them here
//! would invite hand-maintained graphs — the work the engine exists to do.

use std::collections::HashMap;
use std::sync::Arc;

use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{Error, Inner};

/// Accessor for the knowledge-graph API. Get one via [`crate::MemMesh::graph`].
pub struct Graph {
    pub(crate) c: Arc<Inner>,
}

/// A resolved thing — person, org, product, concept — filed under
/// `canonical_name`, with `aliases` resolving to it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryEntity {
    pub id: String,
    #[serde(default)]
    pub project_id: Option<String>,
    /// The brain that first created this entity. Entities dedupe per project,
    /// so this is provenance, NOT an isolation key — brain-scoped graph work
    /// filters on the edge's brain, which the read routes apply server-side.
    #[serde(default)]
    pub brain_id: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default, rename = "type")]
    pub type_: Option<String>,
    pub canonical_name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub metadata: Option<Value>,
    #[serde(default)]
    pub valid_from: Option<String>,
    /// `None` while the entity is still current.
    #[serde(default)]
    pub valid_to: Option<String>,
    #[serde(default)]
    pub superseded_by_id: Option<String>,
}

/// An edge as the READ routes return it — hydrated, not the raw `memory_edge`
/// row. `subject` and `object` are resolved entities rather than ids, and `hop`
/// says how far from the seed the walk found it.
///
/// This is the server's `GraphTraversalEdge`, returned by
/// [`Graph::list_edges`], [`Graph::traverse`], and the `edges` of
/// [`Graph::get_entity`]. The raw row shape (`subjectId` / `objectId` /
/// `brainId`) is not exposed by any read route, so it is deliberately not
/// modelled here — a type nothing returns is a trap.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphTraversalEdge {
    /// Edge primary key — needed for invalidation.
    pub id: String,
    /// The entity this edge starts from, hydrated.
    pub subject: MemoryEntity,
    /// The relationship — `works_at`, `owns`, `located_in`, ...
    pub predicate: String,
    /// The target entity, hydrated. `None` when `object_literal` carries the value.
    #[serde(default)]
    pub object: Option<MemoryEntity>,
    /// Literal value, when the object is not an entity (a date, a price).
    #[serde(default)]
    pub object_literal: Option<String>,
    /// Confidence, 0..1.
    #[serde(default)]
    pub weight: f64,
    #[serde(default)]
    pub valid_from: Option<String>,
    /// `None` while the edge is still valid.
    #[serde(default)]
    pub valid_to: Option<String>,
    /// The memory this edge was extracted from.
    #[serde(default)]
    pub source_memory_id: Option<String>,
    /// Distance from the seed entity on a `traverse` — 1 for a direct
    /// neighbour. `list_edges` has no seed, so every edge comes back `hop: 0`.
    #[serde(default)]
    pub hop: i64,
}

/// Whether KG extraction is on, platform-wide and for this project.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractionState {
    #[serde(default)]
    pub platform_enabled: bool,
    #[serde(default)]
    pub project_enabled: bool,
}

/// Aggregate graph counts.
///
/// `memories_with_edges` against your total memory count is the useful ratio:
/// it says how much of what you remember made it into the graph rather than
/// remaining an isolated embedding. A low ratio usually means extraction is
/// off — check [`extraction`](Self::extraction) before concluding the corpus
/// simply had no relations in it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphStats {
    #[serde(default)]
    pub entity_count: i64,
    #[serde(default)]
    pub edge_count: i64,
    /// Distinct memories that produced at least one edge.
    #[serde(default)]
    pub memories_with_edges: i64,
    #[serde(default)]
    pub retired_entities: i64,
    #[serde(default)]
    pub retired_edges: i64,
    /// Live entity counts keyed by entity type.
    #[serde(default)]
    pub entities_by_type: HashMap<String, i64>,
    #[serde(default)]
    pub extraction: Option<ExtractionState>,
}

/// An entity plus its 1-hop neighbourhood.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityWithEdges {
    #[serde(default)]
    pub entity: Option<MemoryEntity>,
    #[serde(default)]
    pub edges: Vec<GraphTraversalEdge>,
}

/// Filters for [`Graph::list_entities`]. Fill what you need; the rest default.
#[derive(Debug, Default)]
pub struct ListEntities {
    pub type_: Option<String>,
    pub scope: Option<String>,
    /// Substring match against `canonical_name` and every alias.
    pub search: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

/// Options for [`Graph::traverse`].
#[derive(Debug, Default)]
pub struct Traverse {
    /// How many hops out from the seed entity. 1-3.
    pub hops: Option<u32>,
    /// Restrict the walk to these predicates, e.g. `["member_of", "led_by"]`.
    pub predicates: Option<Vec<String>>,
    pub as_of: Option<String>,
}

impl Graph {
    /// Aggregate counts for the whole graph.
    ///
    /// Prefer this over `list_entities(...).len()` for any "how big is it"
    /// question: these are SQL `COUNT(*)`s over the full table, where the list
    /// routes page and would report the page size as the total.
    pub async fn stats(&self) -> Result<GraphStats, Error> {
        self.c.send::<Value, _>(Method::GET, "/admin/memory/graph/stats", None).await
    }

    /// Entities, filtered by type/scope or a substring of name or alias.
    pub async fn list_entities(&self, p: ListEntities) -> Result<Vec<MemoryEntity>, Error> {
        let mut q: Vec<(String, String)> = Vec::new();
        if let Some(t) = p.type_ {
            q.push(("type".into(), t));
        }
        if let Some(s) = p.scope {
            q.push(("scope".into(), s));
        }
        if let Some(s) = p.search {
            q.push(("search".into(), s));
        }
        if let Some(n) = p.limit {
            q.push(("limit".into(), n.to_string()));
        }
        if let Some(n) = p.offset {
            q.push(("offset".into(), n.to_string()));
        }
        self.c
            .send::<Value, _>(Method::GET, &with_query("/admin/memory/entities", &q), None)
            .await
    }

    /// One entity plus its 1-hop neighbourhood.
    pub async fn get_entity(
        &self,
        entity_id: &str,
        as_of: Option<&str>,
    ) -> Result<EntityWithEdges, Error> {
        let q: Vec<(String, String)> = as_of
            .map(|t| vec![("asOf".to_string(), t.to_string())])
            .unwrap_or_default();
        let path = format!("/admin/memory/entities/{entity_id}");
        self.c.send::<Value, _>(Method::GET, &with_query(&path, &q), None).await
    }

    /// Every currently-valid edge.
    ///
    /// Use for rendering a whole small graph; for a large one, seed from an
    /// entity and [`traverse`](Self::traverse) instead.
    pub async fn list_edges(
        &self,
        as_of: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Vec<GraphTraversalEdge>, Error> {
        let mut q: Vec<(String, String)> = Vec::new();
        if let Some(t) = as_of {
            q.push(("asOf".into(), t.to_string()));
        }
        if let Some(n) = limit {
            q.push(("limit".into(), n.to_string()));
        }
        self.c
            .send::<Value, _>(Method::GET, &with_query("/admin/memory/graph/edges", &q), None)
            .await
    }

    /// Walk out from a seed entity.
    ///
    /// This is the multi-hop path: the edges returned here connect facts no
    /// single memory states together, which is how a question gets answered
    /// from a chain rather than from one lucky vector hit.
    pub async fn traverse(&self, entity_id: &str, p: Traverse) -> Result<Vec<GraphTraversalEdge>, Error> {
        let mut body = json!({ "entityId": entity_id });
        if let Some(h) = p.hops {
            body["hops"] = json!(h);
        }
        if let Some(preds) = p.predicates {
            body["predicates"] = json!(preds);
        }
        if let Some(t) = p.as_of {
            body["asOf"] = json!(t);
        }
        self.c
            .send(Method::POST, "/admin/memory/graph/traverse", Some(&body))
            .await
    }
}

/// Append `q` to `path` as a query string, percent-encoding each value.
///
/// Values reach here from user input (`search`), so they are encoded rather
/// than interpolated — an unescaped `&` would otherwise silently truncate the
/// filter and return the wrong page.
fn with_query(path: &str, q: &[(String, String)]) -> String {
    if q.is_empty() {
        return path.to_string();
    }
    let pairs: Vec<String> = q
        .iter()
        .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
        .collect();
    format!("{path}?{}", pairs.join("&"))
}

fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{client, mock_server};

    #[test]
    fn with_query_omits_empty() {
        assert_eq!(with_query("/x", &[]), "/x");
    }

    #[test]
    fn with_query_encodes_values() {
        // An unescaped `&` here would silently truncate the filter server-side
        // and return the wrong page, so this is a correctness test, not style.
        let q = vec![("search".to_string(), "a&b c".to_string())];
        assert_eq!(with_query("/x", &q), "/x?search=a%26b%20c");
    }

    #[tokio::test]
    async fn stats_parses_camel_case_counts() {
        let body = json!({
            "entityCount": 12,
            "edgeCount": 34,
            "memoriesWithEdges": 7,
            "retiredEntities": 0,
            "retiredEdges": 1,
            "entitiesByType": { "person": 3 },
            "extraction": { "platformEnabled": true, "projectEnabled": false }
        })
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let st = client(&base).graph().stats().await.unwrap();
        assert_eq!(st.entity_count, 12);
        assert_eq!(st.edge_count, 34);
        assert_eq!(st.memories_with_edges, 7);
        assert_eq!(st.entities_by_type.get("person"), Some(&3));
        assert!(!st.extraction.unwrap().project_enabled);
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "GET");
        assert!(req.path.ends_with("/admin/memory/graph/stats"));
    }

    #[tokio::test]
    async fn list_entities_sends_only_set_filters() {
        let (base, rx) = mock_server(vec!["[]".into()]);
        client(&base)
            .graph()
            .list_entities(ListEntities {
                search: Some("Sarah".into()),
                limit: Some(5),
                ..Default::default()
            })
            .await
            .unwrap();
        let req = rx.recv().unwrap();
        assert!(req.path.contains("search=Sarah"));
        assert!(req.path.contains("limit=5"));
        assert!(!req.path.contains("scope="));
        assert!(!req.path.contains("offset="));
    }

    #[tokio::test]
    async fn list_entities_without_filters_sends_no_query() {
        let (base, rx) = mock_server(vec!["[]".into()]);
        client(&base).graph().list_entities(ListEntities::default()).await.unwrap();
        let req = rx.recv().unwrap();
        assert!(req.path.ends_with("/admin/memory/entities"), "got {}", req.path);
    }

    #[tokio::test]
    async fn traverse_posts_entity_id_and_omits_unset() {
        let (base, rx) = mock_server(vec!["[]".into()]);
        client(&base)
            .graph()
            .traverse(
                "e1",
                Traverse { hops: Some(2), predicates: Some(vec!["member_of".into()]), ..Default::default() },
            )
            .await
            .unwrap();
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        assert!(req.path.ends_with("/admin/memory/graph/traverse"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["entityId"], "e1");
        assert_eq!(b["hops"], 2);
        assert_eq!(b["predicates"][0], "member_of");
        assert!(b.get("asOf").is_none());
    }

    #[tokio::test]
    async fn list_edges_decodes_hydrated_traversal_shape() {
        // Regression: the read routes return GraphTraversalEdge, not the raw
        // memory_edge row. Decoding against the row shape failed live with
        // `missing field 'subjectId'`.
        let body = json!([{
            "id": "g1",
            "subject": { "id": "e1", "canonicalName": "NVIDIA CORP" },
            "predicate": "reported_metric",
            "object": { "id": "e2", "canonicalName": "Cost of Revenue" },
            "objectLiteral": null,
            "weight": 0.85,
            "validFrom": "2026-07-24T19:45:04.420Z",
            "validTo": null,
            "sourceMemoryId": "m1",
            "hop": 0
        }])
        .to_string();
        let (base, _rx) = mock_server(vec![body]);
        let edges = client(&base).graph().list_edges(None, Some(1)).await.unwrap();
        assert_eq!(edges[0].subject.canonical_name, "NVIDIA CORP");
        assert_eq!(edges[0].object.as_ref().unwrap().canonical_name, "Cost of Revenue");
        assert_eq!(edges[0].hop, 0);
        assert!((edges[0].weight - 0.85).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn list_edges_decodes_literal_object() {
        // `object` is null when the value is a literal rather than an entity.
        let body = json!([{
            "id": "g2",
            "subject": { "id": "e1", "canonicalName": "NVIDIA CORP" },
            "predicate": "ticker_symbol",
            "object": null,
            "objectLiteral": "NVDA",
            "weight": 0.85,
            "hop": 0
        }])
        .to_string();
        let (base, _rx) = mock_server(vec![body]);
        let edges = client(&base).graph().list_edges(None, None).await.unwrap();
        assert!(edges[0].object.is_none());
        assert_eq!(edges[0].object_literal.as_deref(), Some("NVDA"));
    }

    #[tokio::test]
    async fn get_entity_returns_entity_with_edges() {
        let body = json!({
            "entity": { "id": "e1", "canonicalName": "Sarah" },
            "edges": [{
                "id": "g1",
                "subject": { "id": "e1", "canonicalName": "Sarah" },
                "predicate": "works_at",
                "object": { "id": "e2", "canonicalName": "Acme" },
                "weight": 0.9,
                "hop": 1
            }]
        })
        .to_string();
        let (base, _rx) = mock_server(vec![body]);
        let out = client(&base).graph().get_entity("e1", None).await.unwrap();
        assert_eq!(out.entity.unwrap().canonical_name, "Sarah");
        assert_eq!(out.edges[0].predicate, "works_at");
        // The read routes hydrate both ends — ids alone would not decode.
        assert_eq!(out.edges[0].subject.canonical_name, "Sarah");
        assert_eq!(out.edges[0].object.as_ref().unwrap().canonical_name, "Acme");
        assert_eq!(out.edges[0].hop, 1);
    }
}

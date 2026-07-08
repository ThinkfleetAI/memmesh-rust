//! Shared serde types.

use serde::{Deserialize, Serialize};

/// Who/what a memory or prediction is about.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subject {
    pub kind: String,
    #[serde(rename = "externalId")]
    pub external_id: String,
}

impl Subject {
    pub fn new(kind: impl Into<String>, external_id: impl Into<String>) -> Self {
        Subject { kind: kind.into(), external_id: external_id.into() }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryItem {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub content: String,
    pub importance: f64,
    pub scope: String,
    pub status: String,
    #[serde(default)]
    pub confidence: f64,
    #[serde(default)]
    pub superseded_by_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchResult {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub content: String,
    pub similarity: f64,
    pub scope: String,
    pub status: String,
    pub importance: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Insight {
    pub id: String,
    pub content: String,
    pub source_ids: Vec<String>,
    pub confidence: f64,
}

/// Outcome of ingesting one media item: the memories extracted from it plus the
/// text the model read and where the raw bytes were kept.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IngestMediaResult {
    #[serde(default)]
    pub saved: Vec<MemoryItem>,
    #[serde(default)]
    pub candidate_count: u32,
    #[serde(default)]
    pub extracted_text: String,
    #[serde(default)]
    pub modality: String,
    #[serde(default)]
    pub blob_uri: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReflectResult {
    pub insights: Vec<Insight>,
    pub sources_considered: u32,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphEdge {
    pub id: String,
    pub subject_id: String,
    pub predicate: String,
    pub object_id: Option<String>,
    pub object_literal: Option<String>,
    pub weight: f64,
    pub valid_from: String,
    pub valid_to: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DedupResult {
    pub scanned: u32,
    pub groups: u32,
    pub superseded: u32,
}

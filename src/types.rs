//! Shared serde types.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Who/what a memory or prediction is about.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
    /// Free-form structured metadata. Carries `sourceMemoryIds` on mined
    /// patterns (see [`Explanation`]) and the structured shape on procedures.
    #[serde(default)]
    pub metadata: Option<Value>,
    /// ISO-8601 ingest timestamp. Present on rows served by the list/get routes;
    /// used to order sibling records (e.g. picking the active consent row).
    #[serde(default)]
    pub created: Option<String>,
}

/// A review-queue row: a memory plus why it needs a steward's attention.
/// `review_reason` is one of `pending` / `flagged` / `low_confidence` / `stale`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewQueueItem {
    #[serde(flatten)]
    pub memory: MemoryItem,
    pub review_reason: String,
}

/// One step of a procedure. `pitfall` is an optional inline warning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcedureStep {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pitfall: Option<String>,
}

/// Category-level precedence exception: for this category, this tier wins.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrecedenceOverride {
    pub category: String,
    pub winning_tier: String,
}

/// Which memory wins when two disagree. Default ladder: human_verified > local
/// > licensed_brain > base.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrecedencePolicy {
    pub default_order: Vec<String>,
    pub scope_nearest_wins: bool,
    pub overrides: Vec<PrecedenceOverride>,
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

/// Aggregate counts for the admin dashboard.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryStats {
    pub total: i64,
    pub pending_review: i64,
    pub flagged: i64,
    #[serde(default)]
    pub by_scope: HashMap<String, i64>,
    #[serde(default)]
    pub by_status: HashMap<String, i64>,
}

/// Outcome of an LLM deductive-consolidation pass.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsolidateResult {
    pub subjects_considered: i64,
    pub observations_created: i64,
    pub observations_updated: i64,
    pub observations_superseded: i64,
    pub duration_ms: i64,
}

/// How many items were vectorized on a single backfill pass. Call
/// `backfill_embeddings` repeatedly until `embedded` is 0 to drain a corpus.
#[derive(Debug, Clone, Deserialize)]
pub struct BackfillEmbeddingsResult {
    pub embedded: i64,
}

/// A feedback record attached to a memory item. `rating` is `positive` or
/// `negative`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryFeedback {
    pub id: String,
    pub memory_id: String,
    #[serde(default)]
    pub response_id: Option<String>,
    pub rating: String,
    #[serde(default)]
    pub comment: Option<String>,
    #[serde(default)]
    pub created_by_user_id: Option<String>,
    #[serde(default)]
    pub created: Option<String>,
}

/// Right-to-explanation result: a memory item paired with the raw source
/// memories that produced it (empty for anything but a mined pattern).
/// Built client-side by resolving the pattern's `metadata.sourceMemoryIds`.
#[derive(Debug, Clone)]
pub struct Explanation {
    pub memory: MemoryItem,
    pub source_memories: Vec<MemoryItem>,
}

// ── Lattice: behavioral pattern intelligence ─────────────────────────────
//
// Mirrors thinkfleet-memory-sdk/src/types/lattice.ts. Response structs are
// `Deserialize` (camelCase); request structs are `Serialize` with
// `skip_serializing_if` so unset fields are omitted, not sent as null.

/// Approximate inter-event cadence a pattern fires on.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cadence {
    /// Approximate inter-event interval in days (e.g. 7 for weekly).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period_days: Option<f64>,
    /// 0 = Sunday … 6 = Saturday.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub day_of_week: Option<i64>,
    /// Local time of day in "HH:MM" format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_of_day_local: Option<String>,
    /// IANA timezone the local time refers to (e.g. "America/Chicago").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
}

/// Structured metadata carried on a mined behavior pattern.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BehaviorPatternMetadata {
    pub pattern_kind: String,
    pub contact_id: String,
    #[serde(default)]
    pub entity_external_ids: Option<Vec<String>>,
    #[serde(default)]
    pub entity_kind: Option<String>,
    #[serde(default)]
    pub event_type: Option<String>,
    #[serde(default)]
    pub cadence: Option<Cadence>,
    /// Confidence 0..1.
    pub confidence: f64,
    pub observation_count: i64,
    pub observation_window_days: i64,
    pub last_observed_at: String,
    #[serde(default)]
    pub next_expected_at: Option<String>,
    #[serde(default)]
    pub tolerance_minutes: Option<i64>,
    pub active: bool,
}

/// Bulk (re-)extraction request. Omit `contact_id` for a project-wide run.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractPatternsRequest {
    /// Restrict extraction to a single contact. Omit for project-wide bulk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact_id: Option<String>,
    /// Look-back window in days (7–730). Default 90.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_days: Option<u32>,
    /// Force re-extraction even if recent patterns exist.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force: Option<bool>,
    /// Activity source: `memories` (default, Rust engine) or `contact_events`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Restrict mining to one subject (only meaningful for source=`memories`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<Subject>,
}

/// Subject-agnostic corpus mining request — thin sibling of
/// [`ExtractPatternsRequest`] without the legacy `contact_id`/`source` fields.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MineMemoriesRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<Subject>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_days: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force: Option<bool>,
}

/// A per-contact extraction failure.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactExtractError {
    pub contact_id: String,
    pub event_type: String,
    pub error: String,
}

/// Outcome of a pattern-extraction run.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractPatternsResult {
    pub contacts_processed: i64,
    pub patterns_created: i64,
    pub patterns_refreshed: i64,
    pub patterns_deactivated: i64,
    pub duration_ms: i64,
    #[serde(default)]
    pub errors: Vec<ContactExtractError>,
}

/// A single stored behavior pattern.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BehaviorPatternRecord {
    pub id: String,
    pub project_id: Option<String>,
    pub contact_id: String,
    /// Free-text summary; mirrors the underlying memory item's `content`.
    pub summary: String,
    pub metadata: BehaviorPatternMetadata,
    pub active: bool,
    pub confidence: f64,
    pub created: String,
    pub updated: String,
}

/// Query params for [`crate::Lattice::list_patterns`]. All optional.
#[derive(Debug, Clone, Default)]
pub struct ListPatternsParams {
    /// Default true. Set false to include retired patterns.
    pub active_only: Option<bool>,
    /// Page size, 1–100. Default 50.
    pub limit: Option<u32>,
    /// Opaque cursor from a prior response's `next_cursor`.
    pub cursor: Option<String>,
}

impl ListPatternsParams {
    /// Render the set fields as a `?a=b&c=d` query string (empty when none set).
    pub(crate) fn query(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(v) = self.active_only {
            parts.push(format!("activeOnly={v}"));
        }
        if let Some(v) = self.limit {
            parts.push(format!("limit={v}"));
        }
        if let Some(v) = &self.cursor {
            parts.push(format!("cursor={v}"));
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!("?{}", parts.join("&"))
        }
    }
}

/// Cursor-paginated list of a contact's patterns.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListContactPatternsResponse {
    pub data: Vec<BehaviorPatternRecord>,
    pub next_cursor: Option<String>,
}

/// Query params for [`crate::Lattice::get_context`]. All optional.
#[derive(Debug, Clone, Default)]
pub struct GetContextParams {
    /// Bi-temporal query: bundle as it was at this ISO-8601 timestamp.
    pub as_of: Option<String>,
    /// Recent events to include (1–200). Default 25.
    pub events_limit: Option<u32>,
    /// Recent memories to include (1–100). Default 25.
    pub memories_limit: Option<u32>,
    /// Graph traversal depth (1–3). Default 1.
    pub graph_hops: Option<u32>,
}

impl GetContextParams {
    pub(crate) fn query(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(v) = &self.as_of {
            parts.push(format!("asOf={v}"));
        }
        if let Some(v) = self.events_limit {
            parts.push(format!("eventsLimit={v}"));
        }
        if let Some(v) = self.memories_limit {
            parts.push(format!("memoriesLimit={v}"));
        }
        if let Some(v) = self.graph_hops {
            parts.push(format!("graphHops={v}"));
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!("?{}", parts.join("&"))
        }
    }
}

/// The contact facet of a [`LatticeContextBundle`].
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LatticeContextContact {
    pub id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub segment: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub lifetime_value: Option<f64>,
    #[serde(default)]
    pub last_interaction_at: Option<String>,
}

/// A recent activity event in a [`LatticeContextBundle`].
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LatticeContextEvent {
    pub id: String,
    pub event_type: String,
    pub title: String,
    pub occurred_at: String,
    #[serde(default)]
    pub data: Option<Value>,
}

/// A recent memory in a [`LatticeContextBundle`].
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LatticeContextMemory {
    pub id: String,
    pub content: String,
    pub importance: f64,
    pub scope: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub created: String,
}

/// A graph entity node in a [`LatticeContextBundle`].
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LatticeContextEntity {
    pub id: String,
    pub kind: String,
    pub name: String,
    #[serde(default)]
    pub metadata: Value,
}

/// A graph edge in a [`LatticeContextBundle`].
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LatticeContextEdge {
    pub id: String,
    pub source_entity_id: String,
    pub target_entity_id: String,
    pub kind: String,
    #[serde(default)]
    pub weight: Option<f64>,
}

/// Full retrieval bundle for a contact — profile, patterns, events, memories,
/// and optional entity/edge graph.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LatticeContextBundle {
    pub contact_id: String,
    pub contact: LatticeContextContact,
    pub active_patterns: Vec<BehaviorPatternRecord>,
    pub recent_events: Vec<LatticeContextEvent>,
    pub recent_memories: Vec<LatticeContextMemory>,
    #[serde(default)]
    pub entities: Option<Vec<LatticeContextEntity>>,
    #[serde(default)]
    pub edges: Option<Vec<LatticeContextEdge>>,
}

/// A single monitor-tick failure.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorFailure {
    pub pattern_id: String,
    pub error: String,
}

/// Outcome of a pattern-break monitor tick.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorTickResult {
    pub patterns_checked: i64,
    pub patterns_broken: i64,
    pub breaks_emitted: i64,
    pub duration_ms: i64,
    pub capped: bool,
    #[serde(default)]
    pub failures: Vec<MonitorFailure>,
}

/// Monitor health snapshot.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorStatus {
    pub last_tick_at: Option<String>,
    pub last_tick_duration_ms: Option<i64>,
    pub patterns_due: i64,
    pub active_pattern_count: i64,
}

/// A declaratively-specified prediction target (v2). Declare *what* to predict;
/// the engine selects the model family from `kind`
/// (`event_occurrence` | `numeric` | `event_time` | `anomaly`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PredictionTarget {
    /// Target type — drives model selection.
    pub kind: String,
    /// For event_occurrence / event_time: the activity event to predict.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_type: Option<String>,
    /// For numeric / anomaly: the typed-observation attribute to predict.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribute_key: Option<String>,
    /// Days of history to learn from. Default 365, clamped [1, 3650].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lookback_days: Option<u32>,
}

impl PredictionTarget {
    /// Shorthand for a target with just a `kind`.
    pub fn new(kind: impl Into<String>) -> Self {
        PredictionTarget { kind: kind.into(), ..Default::default() }
    }
}

/// Predict request. Omit `target` for pattern projection; set it for v2
/// general prediction (`target_prediction` in the result).
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PredictRequest {
    pub subject: Subject,
    /// How far forward to project (days). Default 30, clamped [1, 365].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub horizon_days: Option<u32>,
    /// Max predictions to return. Default 20, max 200.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// Minimum confidence (0..1) to surface. Default 0.5.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_confidence: Option<f64>,
    /// Future occurrences to project per fixed-cadence pattern. Default 1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub occurrences_per_pattern: Option<u32>,
    /// Emit a `prediction.imminent` event per prediction due within the window.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emit_events: Option<bool>,
    /// Imminence window for event emission, in hours. Default 48, clamped [1, 720].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imminent_within_hours: Option<u32>,
    /// v2 general prediction: predict this declared target instead of projecting
    /// mined patterns. Result arrives in `target_prediction`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<PredictionTarget>,
}

impl PredictRequest {
    /// Shorthand for a pattern-projection request for one subject.
    pub fn new(subject: Subject) -> Self {
        PredictRequest { subject, ..Default::default() }
    }
}

/// One projected event derived from one active behavior pattern.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PredictedEvent {
    pub pattern_id: String,
    pub pattern_kind: String,
    pub description: String,
    pub expected_at: String,
    /// 0..1 confidence inherited from the pattern's dominance score.
    pub confidence: f64,
    #[serde(default)]
    pub confidence_lower: Option<f64>,
    #[serde(default)]
    pub confidence_upper: Option<f64>,
    pub window_minutes: i64,
    /// Provenance — raw memories that produced the source pattern.
    #[serde(default)]
    pub source_memory_ids: Vec<String>,
}

/// The single calibrated estimate for a declared `target`. Check `abstained`
/// first — when true, treat the estimate as unknown, never as low risk.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetPrediction {
    pub target_kind: String,
    pub event_type: String,
    /// event_occurrence: P(event within horizon), calibrated.
    pub probability: f64,
    pub probability_lower: f64,
    pub probability_upper: f64,
    /// numeric: predicted next value + 95% interval.
    pub value: f64,
    pub value_lower: f64,
    pub value_upper: f64,
    /// event_time: when the next occurrence is expected (ISO-8601) + interval.
    pub expected_at: String,
    pub expected_at_lower: String,
    pub expected_at_upper: String,
    pub days_until: f64,
    /// anomaly: |z| from baseline, and whether it crosses the threshold.
    pub anomaly_score: f64,
    pub is_anomaly: bool,
    /// First-class abstention — true when there isn't enough signal.
    pub abstained: bool,
    pub abstention_reason: String,
    pub explanation: String,
    #[serde(default)]
    pub evidence_memory_ids: Vec<String>,
}

/// Result of [`crate::Lattice::predict`].
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PredictResult {
    pub subject: Subject,
    #[serde(default)]
    pub predictions: Vec<PredictedEvent>,
    pub active_pattern_count: i64,
    #[serde(default)]
    pub events_emitted: Option<i64>,
    pub generated_at: String,
    pub duration_ms: i64,
    /// First-class abstention: treat as "unknown", never "no/low risk".
    #[serde(default)]
    pub abstained: Option<bool>,
    #[serde(default)]
    pub abstention_reason: Option<String>,
    /// Set instead of `predictions` when the request carried a `target`.
    #[serde(default)]
    pub target_prediction: Option<TargetPrediction>,
}

/// A behavioral risk signal on a [`SubjectProfile`].
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiskIndicator {
    pub kind: String,
    pub description: String,
    /// 0..1 severity.
    pub severity: f64,
    /// Pattern memory id that produced the signal.
    pub source_pattern_id: String,
}

/// Behavioral profile snapshot — the non-temporal "who is this subject" view.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubjectProfile {
    pub subject: Subject,
    pub rfm_segment: Option<String>,
    pub recency_score: Option<f64>,
    pub frequency_score: Option<f64>,
    pub monetary_score: Option<f64>,
    pub top_entity: Option<String>,
    pub cadence_summary: Option<String>,
    #[serde(default)]
    pub risks: Vec<RiskIndicator>,
    #[serde(default)]
    pub contributing_pattern_ids: Vec<String>,
    pub generated_at: String,
    pub duration_ms: i64,
}

/// Request for [`crate::Lattice::get_cohort`].
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetCohortRequest {
    pub subject: Subject,
    /// Nearest neighbors to return. Server clamps to [1, 50]. Default 10.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub k: Option<u32>,
    /// Minimum similarity (0..1) for inclusion. Default 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_similarity: Option<f64>,
}

/// One member of a cohort.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CohortMember {
    pub subject: Subject,
    pub similarity: f64,
    pub rfm_segment: String,
    #[serde(default)]
    pub pattern_kinds: Vec<String>,
}

/// Result of [`crate::Lattice::get_cohort`].
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetCohortResponse {
    pub target: Subject,
    #[serde(default)]
    pub members: Vec<CohortMember>,
    pub candidate_count: i64,
    pub generated_at: String,
    pub duration_ms: i64,
}

/// Request for [`crate::Lattice::predict_by_cohort`].
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PredictByCohortRequest {
    pub subject: Subject,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cohort_k: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prediction_limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_similarity: Option<f64>,
}

/// A cohort-aggregated prediction — "people like the target also did X."
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CohortPrediction {
    pub pattern_kind: String,
    pub description: String,
    pub expected_at: String,
    pub confidence: f64,
    pub window_minutes: i64,
    #[serde(default)]
    pub supporting_subjects: Vec<Subject>,
    #[serde(default)]
    pub source_memory_ids: Vec<String>,
}

/// Result of [`crate::Lattice::predict_by_cohort`].
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PredictByCohortResponse {
    pub target: Subject,
    #[serde(default)]
    pub cohort: Vec<CohortMember>,
    #[serde(default)]
    pub predictions: Vec<CohortPrediction>,
    pub generated_at: String,
    pub duration_ms: i64,
}

/// Request for [`crate::Lattice::estimate`] — a deterministic wellness estimator.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EstimateRequest {
    pub subject: Subject,
    /// Which estimator to run. v1: "phenoage".
    pub estimator_id: String,
    /// Persist the score as a memory so it builds a trajectory. Default false.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persist: Option<bool>,
}

/// A per-signal contribution to an [`EstimateResult`].
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoreContributor {
    pub signal: String,
    /// Signed contribution to the score (positive pushed it up).
    pub contribution: f64,
}

/// Result of a deterministic estimator run — an estimate, never a diagnosis.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EstimateResult {
    pub subject: Subject,
    pub estimator_id: String,
    /// True when every required signal was present and a score was produced.
    pub ok: bool,
    pub value: f64,
    pub unit: String,
    #[serde(default)]
    pub contributors: Vec<ScoreContributor>,
    pub confidence: f64,
    #[serde(default)]
    pub provenance: Vec<String>,
    pub framing: String,
    pub disclaimer: String,
    #[serde(default)]
    pub missing_signals: Vec<String>,
}

/// One confidence band of a [`CalibrationReport`].
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationBucket {
    pub lower: f64,
    pub upper: f64,
    pub patterns: i64,
    pub predictions: i64,
    pub hits: i64,
    pub misses: i64,
    /// hits / predictions; only meaningful when `has_data` is true.
    pub realized_hit_rate: f64,
    pub has_data: bool,
}

/// Prediction calibration report: confidence buckets → realized hit-rates.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationReport {
    #[serde(default)]
    pub buckets: Vec<CalibrationBucket>,
    pub total_patterns: i64,
    pub total_predictions: i64,
}

// ── Context builder ──────────────────────────────────────────────────────
//
// Mirrors thinkfleet-memory-sdk/src/resources/context.ts.

/// A section of a [`ContextBundle`], selectable via
/// [`ContextBuildRequest::include`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContextSection {
    Profile,
    Patterns,
    Predictions,
    Memories,
    Observations,
}

/// Options for [`crate::Context::build`]. Build with `..Default::default()`;
/// unset fields fall back to the server defaults.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextBuildRequest {
    pub subject: Subject,
    /// Which sections to include. Default: all five.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<ContextSection>>,
    /// Hard cap on bundle size. Sections truncate to fit. Default 2000.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Max raw memories. Default 10.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_limit: Option<u32>,
    /// Max predictions. Default 5.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prediction_limit: Option<u32>,
    /// Drop memories whose `category` is in this list (min-necessary access).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude_categories: Option<Vec<String>>,
}

impl ContextBuildRequest {
    /// Shorthand for an all-sections, default-budget bundle for one subject.
    pub fn new(subject: Subject) -> Self {
        ContextBuildRequest { subject, ..Default::default() }
    }
}

/// The profile facet of a [`ContextBundle`].
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextProfile {
    pub rfm_segment: Option<String>,
    pub recency_score: Option<f64>,
    pub frequency_score: Option<f64>,
    pub monetary_score: Option<f64>,
    pub top_entity: Option<String>,
    pub cadence_summary: Option<String>,
    #[serde(default)]
    pub risks: Vec<RiskIndicator>,
}

/// An active pattern in a [`ContextBundle`].
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPattern {
    pub id: String,
    pub pattern_kind: String,
    pub summary: String,
    pub confidence: f64,
    pub next_expected_at: Option<String>,
}

/// A prediction in a [`ContextBundle`].
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPrediction {
    pub pattern_id: String,
    pub pattern_kind: String,
    pub description: String,
    pub expected_at: String,
    pub confidence: f64,
    #[serde(default)]
    pub source_memory_ids: Vec<String>,
}

/// A raw memory in a [`ContextBundle`].
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextMemory {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub content: String,
    pub importance: f64,
    pub created: String,
}

/// A consolidated observation in a [`ContextBundle`].
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextObservation {
    pub id: String,
    pub content: String,
    pub proof_count: i64,
    #[serde(default)]
    pub source_memory_ids: Vec<String>,
    pub created: String,
}

/// Provenance index for a [`ContextBundle`] — every id that fed the bundle.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextProvenance {
    #[serde(default)]
    pub memory_ids: Vec<String>,
    #[serde(default)]
    pub pattern_ids: Vec<String>,
    #[serde(default)]
    pub observation_ids: Vec<String>,
}

/// Token-budgeted, provenanced context bundle — the unified "everything the LLM
/// needs in one call" payload returned by [`crate::Context::build`].
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextBundle {
    pub subject: Subject,
    pub profile: Option<ContextProfile>,
    #[serde(default)]
    pub patterns: Vec<ContextPattern>,
    #[serde(default)]
    pub predictions: Vec<ContextPrediction>,
    #[serde(default)]
    pub memories: Vec<ContextMemory>,
    #[serde(default)]
    pub observations: Vec<ContextObservation>,
    #[serde(default)]
    pub provenance: ContextProvenance,
    /// Honest within ±15%.
    pub tokens_estimate: i64,
    /// Sections that were truncated to fit the `max_tokens` budget.
    #[serde(default)]
    pub truncated: Vec<ContextSection>,
    pub generated_at: String,
    pub duration_ms: i64,
}

// ── Learning: the decision → action → outcome loop ───────────────────────
//
// Mirrors thinkfleet-memory-sdk/src/resources/learning.ts. Request structs are
// `Serialize` with `skip_serializing_if` (unset fields omitted, not null);
// response structs are `Deserialize` (camelCase).

/// A causal input to a decision — the pattern/prediction/observation the actor
/// was reacting to. `weight` splits credit across multiple inputs. Serialized on
/// the way in and deserialized back on a [`DecisionRecord`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvenanceRef {
    /// Memory id of the pattern/prediction/observation that informed the decision.
    pub memory_id: String,
    /// What kind of memory this ref points at. Defaults to "pattern".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ref_type: Option<String>,
    /// Credit-assignment weight; 0/unset is treated as 1.0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<f64>,
}

/// Input to [`crate::resources::Learning::record_decision`]. Build with
/// `..Default::default()`; `subject` is required.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordDecisionInput {
    /// Who/what the decision is about.
    pub subject: Subject,
    /// Who made it: "agent:copilot" | "human:owner" | "policy:winback-v1".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    /// Generic label: "offer" | "outreach" | "escalation".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision_type: Option<String>,
    /// Optional policy id/version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy: Option<String>,
    /// The patterns/predictions that informed the decision (credit assignment).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub informed_by: Option<Vec<ProvenanceRef>>,
    /// The executed effect: "send_message" | "apply_discount" | ...
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_type: Option<String>,
    /// Opaque action params.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<HashMap<String, String>>,
    /// "proposed" | "executed" | "skipped". Defaults to "executed".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// Client RFC3339 timestamp; the engine fills now() when omitted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, String>>,
    /// Re-sending the same key returns the existing decision, no duplicate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

/// A recorded decision.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionRecord {
    pub decision_id: String,
    #[serde(default)]
    pub subject: Option<Subject>,
    pub actor: String,
    pub decision_type: String,
    pub policy: String,
    #[serde(default)]
    pub informed_by: Vec<ProvenanceRef>,
    pub action_type: String,
    pub status: String,
    pub occurred_at: String,
    pub created: String,
}

/// Result of [`crate::resources::Learning::record_decision`].
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordDecisionResult {
    #[serde(default)]
    pub decision: Option<DecisionRecord>,
}

/// Input to [`crate::resources::Learning::record_outcome`]. Build with
/// `..Default::default()`; `decision_id` + `result` are required.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordOutcomeInput {
    /// The decision this outcome resulted from.
    pub decision_id: String,
    /// Optional — defaults to the linked decision's subject.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<Subject>,
    /// "conversion" | "engagement" | "no_response".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome_type: Option<String>,
    /// "success" | "failure" | "partial".
    pub result: String,
    /// Numeric reward signal (revenue, 0/1, points…).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reward: Option<f64>,
    /// Client RFC3339 timestamp; the engine fills now() when omitted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub realized_at: Option<String>,
    /// How long after the decision this outcome still counts, in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attribution_window_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, String>>,
    /// Exactly-once guard: a replayed key won't double-count calibration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

/// One informing ref re-weighted by an outcome (before/after confidence).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationUpdate {
    pub ref_id: String,
    pub ref_type: String,
    pub prior_confidence: f64,
    pub posterior_confidence: f64,
    pub hits: i64,
    pub misses: i64,
}

/// Result of [`crate::resources::Learning::record_outcome`].
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordOutcomeResult {
    pub outcome_id: String,
    /// Which informing refs were re-weighted, and by how much.
    #[serde(default)]
    pub updates: Vec<CalibrationUpdate>,
}

/// A recorded outcome, as returned by [`crate::resources::Learning::get_outcomes`].
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutcomeRecord {
    pub outcome_id: String,
    pub decision_id: String,
    #[serde(default)]
    pub subject: Option<Subject>,
    pub decision_type: String,
    pub action_type: String,
    pub outcome_type: String,
    pub result: String,
    pub reward: f64,
    pub occurred_at: String,
    pub realized_at: String,
}

/// Query params for [`crate::resources::Learning::get_outcomes`]. All optional.
#[derive(Debug, Clone, Default)]
pub struct GetOutcomesParams {
    /// Restrict to one subject; omit for scope-wide.
    pub subject: Option<Subject>,
    pub decision_type: Option<String>,
    pub action_type: Option<String>,
    /// Default 100, clamped [1, 1000].
    pub limit: Option<u32>,
}

impl GetOutcomesParams {
    /// Render the set fields as a `?a=b&c=d` query string (empty when none set).
    pub(crate) fn query(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(s) = &self.subject {
            parts.push(format!("subjectKind={}", s.kind));
            parts.push(format!("subjectExternalId={}", s.external_id));
        }
        if let Some(v) = &self.decision_type {
            parts.push(format!("decisionType={v}"));
        }
        if let Some(v) = &self.action_type {
            parts.push(format!("actionType={v}"));
        }
        if let Some(v) = self.limit {
            parts.push(format!("limit={v}"));
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!("?{}", parts.join("&"))
        }
    }
}

/// Query params for [`crate::resources::Learning::get_effectiveness`]. All optional.
#[derive(Debug, Clone, Default)]
pub struct GetEffectivenessParams {
    /// Dimension to roll up by: `action_type` (default) | `decision_type` |
    /// `policy` | `pattern_kind`.
    pub group_by: Option<String>,
    /// Return only groups with at least this many outcomes.
    pub min_support: Option<u32>,
}

impl GetEffectivenessParams {
    /// Render the set fields as a `?a=b&c=d` query string (empty when none set).
    pub(crate) fn query(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(v) = &self.group_by {
            parts.push(format!("groupBy={v}"));
        }
        if let Some(v) = self.min_support {
            parts.push(format!("minSupport={v}"));
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!("?{}", parts.join("&"))
        }
    }
}

/// One "what worked" aggregation row from
/// [`crate::resources::Learning::get_effectiveness`].
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectivenessRow {
    pub group_key: String,
    /// Support: outcome count in this group.
    pub n: i64,
    /// Fraction with result="success".
    pub success_rate: f64,
    pub avg_reward: f64,
    /// Beta-Binomial posterior mean of the success rate.
    pub confidence: f64,
}

// ── Behaviors: emergent behavior discovery ───────────────────────────────
//
// Mirrors thinkfleet-memory-sdk/src/resources/behaviors.ts.

/// Tuning for a discovery run. All optional; the engine clamps to safe ranges.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverParams {
    /// Minimum similarity [0,1] for a subject to join a cluster. Default 0.75.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sim_threshold: Option<f64>,
    /// A cluster smaller than this is noise, not a behavior. Default 3.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_cluster_size: Option<u32>,
    /// Drop clusters whose cohesion (mean intra-cluster similarity) is below
    /// this. Default 0.6.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_stability: Option<f64>,
    /// Cap on member subjects returned per behavior. Default 50.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_members: Option<u32>,
}

/// One emergent behavior — a cohesive cluster of subjects the engine grouped
/// together because they behave alike, with the statistics that justify treating
/// it as real.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredBehavior {
    /// Rule-based description (RFM band + frequency + dominant pattern + entity).
    pub label: String,
    /// Fraction of the analyzed cohort in this cluster, [0,1] — how common it is.
    pub prevalence: f64,
    /// Cohesion: mean pairwise similarity within the cluster, [0,1].
    pub stability: f64,
    /// Total subjects in the cluster (may exceed `member_subjects` when capped).
    pub size: i64,
    /// Up to `max_members` subjects, medoid first — provenance for who exhibits it.
    #[serde(default)]
    pub member_subjects: Vec<Subject>,
    /// Human-readable signals behind the label (e.g. "pattern: recurring_event").
    #[serde(default)]
    pub exemplar_evidence: Vec<String>,
}

/// Result of [`crate::resources::Behaviors::discover`].
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverResult {
    /// Discovered behaviors, sorted by prevalence then stability. Empty when
    /// there isn't enough signal — discovery abstains rather than inventing
    /// weak behaviors.
    #[serde(default)]
    pub behaviors: Vec<DiscoveredBehavior>,
    /// Total subjects analyzed (the prevalence denominator).
    pub subjects_analyzed: i64,
    pub generated_at: String,
    pub duration_ms: i64,
}

// ── Brains — the marketplace registry ────────────────────────────────────
//
// A Brain is a publishable/consumable unit of memory: a Brain Card manifest
// plus a stable `external_id` slug the Mesh Router addresses it by. This SDK
// covers the registry (create / list / get / update / delete). Consumption of a
// published brain happens over the hosted MCP endpoint, which an MCP client
// connects to directly — it is not a REST call, so it lives outside this SDK.

/// Provenance of the facts in a brain — where they came from and under what license.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrainProvenance {
    pub source: String,
    pub license: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Coverage the induced reasoning layer advertises on a Brain Card — the
/// procedure/checklist/decomposition memories that make a brain worth more than
/// a plain dataset. A facts-only brain reports `total: 0`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrainReasoningCoverage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub procedures: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checklists: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decompositions: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
}

/// The `coverage` block on a Brain Card.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrainCoverage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subjects: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub facts: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<BrainReasoningCoverage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freshness: Option<String>,
}

/// The `evaluation` block on a Brain Card.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrainEvaluation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub benchmark: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
}

/// The `pricing` block on a Brain Card.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrainPricing {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

/// The Brain Card manifest (stored on the brain, surfaced in the catalog).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrainCard {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<Vec<BrainProvenance>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changelog_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage: Option<BrainCoverage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluation: Option<BrainEvaluation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predict_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pricing: Option<BrainPricing>,
}

/// A brain in the project's catalog. `visibility` is one of `PUBLIC` /
/// `UNLISTED` / `PRIVATE`; `status` is one of `DRAFT` / `PUBLISHED` /
/// `ARCHIVED` (kept as strings to stay forward-compatible with new values).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Brain {
    pub id: String,
    pub created: String,
    pub updated: String,
    pub project_id: String,
    /// Stable slug the Router addresses the brain by (unique per project).
    pub external_id: String,
    pub name: String,
    #[serde(default)]
    pub domain: Option<String>,
    /// Brain Interface contract version the brain conforms to (e.g. "v1").
    pub brain_interface: String,
    pub version: String,
    pub visibility: String,
    pub status: String,
    pub rights_attested: bool,
    #[serde(default)]
    pub card: Option<BrainCard>,
}

/// Body for [`crate::Brains::create`]. Build with `..Default::default()`.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBrainRequest {
    pub external_id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rights_attested: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub card: Option<BrainCard>,
}

/// Body for [`crate::Brains::update`]. Only set fields change.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBrainRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rights_attested: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub card: Option<BrainCard>,
}

/// Query params for [`crate::Brains::list`] / [`crate::Brains::list_all`].
#[derive(Debug, Clone, Default)]
pub struct ListBrainsParams {
    pub limit: Option<u32>,
    /// Opaque cursor from a prior [`SeekPage`](crate::SeekPage)'s `next`.
    pub cursor: Option<String>,
}

impl ListBrainsParams {
    /// Render the set fields as a `?a=b&c=d` query string (empty when none set).
    pub(crate) fn query(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(v) = self.limit {
            parts.push(format!("limit={v}"));
        }
        if let Some(v) = &self.cursor {
            parts.push(format!("cursor={v}"));
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!("?{}", parts.join("&"))
        }
    }
}

/// High-level options for [`crate::Brains::create_from_project`] — the easy path
/// to turning a project's memory into a brain. A minimal set of fields; the
/// Brain Card (provenance/coverage) is filled in for you.
#[derive(Debug, Clone, Default)]
pub struct CreateBrainFromProjectOptions {
    /// Stable slug the Router addresses the brain by (unique per project).
    pub external_id: String,
    pub name: String,
    /// Domain the brain covers, e.g. "finance". Required before publishing PUBLIC.
    pub domain: Option<String>,
    /// Semantic version. Defaults to "1.0.0".
    pub version: Option<String>,
    /// Defaults to "PRIVATE". A brain is only consumable once separately PUBLISHED.
    pub visibility: Option<String>,
}

// ── Consent — subject-level opt-out ──────────────────────────────────────
//
// Consent is recorded client-side as `type='consent'` memory items (see
// [`crate::Consent`]); these types describe the request/response surface.

/// Subject of a consent decision — a person / team / workspace, identified by
/// `kind` + `external_id`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsentSubject {
    pub kind: String,
    #[serde(rename = "externalId")]
    pub external_id: String,
}

impl ConsentSubject {
    pub fn new(kind: impl Into<String>, external_id: impl Into<String>) -> Self {
        ConsentSubject { kind: kind.into(), external_id: external_id.into() }
    }
}

/// Request to opt a subject out of mining.
#[derive(Debug, Clone, Default)]
pub struct OptOutRequest {
    pub subject: ConsentSubject,
    /// Free-text reason for the audit log (GDPR Art. 17 request id, etc.).
    pub reason: Option<String>,
}

/// Request to opt a subject back in.
#[derive(Debug, Clone, Default)]
pub struct OptInRequest {
    pub subject: ConsentSubject,
}

/// Current consent status for a subject. `opted_out` defaults to `false` when no
/// consent record exists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsentStatus {
    pub subject: ConsentSubject,
    pub opted_out: bool,
    /// ISO timestamp of the opt-out, or `None` if never opted out.
    pub opted_out_at: Option<String>,
    pub reason: Option<String>,
    /// Id of the underlying consent memory item (for audit linking).
    pub memory_id: Option<String>,
}

// ── Events — the durable memory event log ────────────────────────────────
//
// Mirrors thinkfleet-memory-sdk/src/resources/events.ts and
// src/types/lattice.ts (EmitEventRequest / EmitEventResult).

/// Severity tier emitted on every event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EventSeverity {
    Info,
    Warn,
    Critical,
}

/// One row from the memory event log. Provenance pointers
/// (`source_memory_ids` / `source_pattern_id`) let consumers call
/// `explain()` to drill down to the raw memories that produced the event.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryEvent {
    pub id: String,
    pub event_type: String,
    #[serde(default)]
    pub subject: Option<Subject>,
    pub severity: EventSeverity,
    /// Free-form JSON payload.
    #[serde(default)]
    pub payload: Value,
    #[serde(default)]
    pub source_memory_ids: Vec<String>,
    #[serde(default)]
    pub source_pattern_id: Option<String>,
    #[serde(default)]
    pub emitted_by_pack: Option<String>,
    pub occurred_at: String,
}

/// Query params for [`crate::resources::Events::poll`] /
/// [`crate::resources::Events::subscribe`]. All optional.
#[derive(Debug, Clone, Default)]
pub struct PollEventsParams {
    /// ISO timestamp of the last event you saw. Returns events newer than this.
    pub since: Option<String>,
    /// Max events to return per call. Default 100, max 1000.
    pub limit: Option<u32>,
    /// Restrict to specific event types.
    pub event_types: Option<Vec<String>>,
}

impl PollEventsParams {
    /// Render the set fields as a `?a=b&c=d` query string (empty when none set).
    pub(crate) fn query(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(v) = &self.since {
            parts.push(format!("since={v}"));
        }
        if let Some(v) = self.limit {
            parts.push(format!("limit={v}"));
        }
        if let Some(v) = &self.event_types {
            if !v.is_empty() {
                parts.push(format!("eventTypes={}", v.join(",")));
            }
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!("?{}", parts.join("&"))
        }
    }
}

/// Request body for [`crate::resources::Events::emit`] — appends an event to the
/// durable log. Matching alert rules fire synchronously.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmitEventRequest {
    pub event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<Subject>,
    /// `info` | `warn` | `critical`. Defaults to `info` server-side.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<EventSeverity>,
    /// Free-form JSON payload, serialized (can include "value", "channel", etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_json: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_memory_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_pattern_id: Option<String>,
}

/// The event summary echoed back by [`EmitEventResult`].
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmittedEvent {
    pub id: String,
    pub event_type: String,
    pub severity: String,
    pub occurred_at: String,
}

/// Result of [`crate::resources::Events::emit`].
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmitEventResult {
    /// False when a dedupe collision suppressed the insert.
    pub emitted: bool,
    /// Present only when `emitted` is true.
    #[serde(default)]
    pub event: Option<EmittedEvent>,
    /// Number of alert rules that matched + dispatched.
    pub alert_dispatches: i64,
}

// ── Alerts — user-defined alert rules (Phase 3j) ─────────────────────────
//
// Mirrors thinkfleet-memory-sdk/src/resources/alerts.ts.

/// What makes an alert rule fire — hooks into the engine event stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum AlertTrigger {
    /// Fire on one of the given engine event types.
    EngineEvent {
        #[serde(rename = "eventTypes")]
        event_types: Vec<String>,
    },
    /// Fire when a subject's segment changes into `to` (optionally from `from`).
    SegmentChange {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<String>,
        to: String,
    },
    /// Fire when a new pattern of the given kind emerges.
    PatternEmerged {
        #[serde(rename = "patternKind")]
        pattern_kind: String,
    },
}

/// Narrows which events an [`AlertTrigger`] actually fires on.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertFilter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_kind: Option<String>,
    /// Glob pattern. Use `*` for wildcards: `vip-*` matches any externalId
    /// starting with `vip-`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_external_id_pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub categories: Option<Vec<String>>,
    /// Dot-path keys; equality match against event payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata_match: Option<Value>,
}

/// The `writeAs` config for a [`NotificationChannel::Memory`] channel.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryWriteAs {
    /// Template with `{{event.eventType}}`, `{{event.severity}}`,
    /// `{{subject.kind}}`, `{{subject.externalId}}`, `{{rule.name}}`.
    pub content: String,
    /// `project` | `platform` | `user`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

/// How a firing alert is delivered.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum NotificationChannel {
    /// Deliver over an HTTP webhook.
    Webhook {
        url: String,
        /// Shared secret for HMAC-SHA256 signing of the body. Sent as
        /// `X-ThinkFleet-Signature`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        secret: Option<String>,
    },
    /// Self-documenting alert: write the firing event back as an OBSERVATION
    /// memory item the LLM surfaces on the next `context.build()`.
    Memory {
        #[serde(rename = "writeAs")]
        write_as: MemoryWriteAs,
    },
}

/// Rate-limiting for a rule's deliveries.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThrottleConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_per_hour: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cooldown_minutes: Option<u32>,
    /// `subject` | `subject+rule` | `rule`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedup_on: Option<String>,
}

/// A stored alert rule.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertRule {
    pub id: String,
    pub project_id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub enabled: bool,
    pub trigger: AlertTrigger,
    #[serde(default)]
    pub filter: Option<AlertFilter>,
    #[serde(default)]
    pub notify: Vec<NotificationChannel>,
    #[serde(default)]
    pub throttle: Option<ThrottleConfig>,
    pub created: String,
    pub updated: String,
}

/// Body for [`crate::resources::Alerts::create`].
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAlertRuleRequest {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    pub trigger: AlertTrigger,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<AlertFilter>,
    pub notify: Vec<NotificationChannel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub throttle: Option<ThrottleConfig>,
}

// A default AlertTrigger is required so CreateAlertRuleRequest can derive
// Default; a struct-args builder still forces the caller to set `trigger`.
impl Default for AlertTrigger {
    fn default() -> Self {
        AlertTrigger::EngineEvent { event_types: Vec::new() }
    }
}

/// Body for [`crate::resources::Alerts::update`] — every field optional (PATCH).
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAlertRuleRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger: Option<AlertTrigger>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<AlertFilter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notify: Option<Vec<NotificationChannel>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub throttle: Option<ThrottleConfig>,
}

/// One outcome of a delivery attempt within an [`AlertFire`].
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryResult {
    pub channel: String,
    pub ok: bool,
    #[serde(default)]
    pub error: Option<String>,
}

/// A single firing of an alert rule.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertFire {
    pub id: String,
    pub alert_rule_id: String,
    #[serde(default)]
    pub event_id: Option<String>,
    pub dedupe_key: String,
    #[serde(default)]
    pub delivery_results: Vec<DeliveryResult>,
    pub fired_at: String,
}

// ── Health vertical (@thinkfleet/pack-healthcare) ──────────────────────────

/// Per-call options for [`crate::resources::Health::record_biomarker`]. Send the
/// unit the lab reported; the engine normalizes it.
#[derive(Debug, Clone, Default)]
pub struct RecordBiomarkerOpts {
    pub unit: Option<String>,
    pub observed_at: Option<String>,
}

/// Record/refresh a subject's demographics. Latest values win.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DemographicsInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub age_years: Option<f64>,
    /// "male" | "female" | "unknown".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight_kg: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height_cm: Option<f64>,
    /// "sedentary" | "low" | "moderate" | "high".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity: Option<String>,
}

/// An ICD-10 diagnosis to record for a subject.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConditionInput {
    /// ICD-10 code, e.g. "E11.9".
    pub icd10: String,
    /// "active" | "resolved" | "historical".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// ISO timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub onset_at: Option<String>,
}

/// One labelled contribution to a biological-age estimate.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthAgeComponent {
    pub label: String,
    pub years_delta: f64,
}

/// Biological-age estimate derived from a subject's recorded health data.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BiologicalAge {
    pub biological_age_years: f64,
    pub chronological_age_years: f64,
    pub delta_years: f64,
    /// "phenoage_hybrid" | "composite".
    pub method: String,
    pub confidence: f64,
    #[serde(default)]
    pub components: Vec<HealthAgeComponent>,
    /// 10-year mortality score (0..1) from PhenoAge, when available.
    #[serde(default)]
    pub mortality_score: Option<f64>,
}

/// A predicted (not yet diagnosed) condition for a subject.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PredictedHealthCondition {
    /// Canonical key, e.g. "type2_diabetes".
    pub condition: String,
    pub label: String,
    /// "above_threshold_now" | "threshold_projection".
    pub basis: String,
    pub biomarker: String,
    pub current_value: f64,
    pub threshold: f64,
    /// ISO timestamp; set only for threshold_projection.
    #[serde(default)]
    pub projected_onset_at: Option<String>,
    pub confidence: f64,
    pub rationale: String,
    #[serde(default)]
    pub source_memory_ids: Vec<String>,
}

/// The latest reading for one biomarker.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BiomarkerReading {
    pub biomarker: String,
    pub value: f64,
    pub unit: String,
    pub observed_at: String,
}

/// Derived health profile for a subject — biological age + condition
/// predictions + latest biomarkers.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthProfile {
    pub subject: Subject,
    #[serde(default)]
    pub biological_age: Option<BiologicalAge>,
    #[serde(default)]
    pub predicted_conditions: Vec<PredictedHealthCondition>,
    /// ICD-10 codes already diagnosed (active) on record.
    #[serde(default)]
    pub diagnosed_conditions: Vec<String>,
    #[serde(default)]
    pub latest_biomarkers: Vec<BiomarkerReading>,
    /// Always populated — screening indicators, not a diagnosis.
    pub disclaimer: String,
    pub generated_at: String,
}

/// Condition prevalence within a subject's nearest-neighbour cohort.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CohortConditionRisk {
    pub condition: String,
    /// Fraction of the cohort carrying this condition (0..1).
    pub cohort_prevalence: f64,
    pub cohort_size: u64,
    pub count_with: u64,
    pub mean_similarity: f64,
    pub confidence: f64,
    pub rationale: String,
}

/// Cohort outcomes — epidemiological base rates for patients like this one.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CohortHealthRisk {
    pub subject: Subject,
    pub cohort_size: u64,
    pub population_size: u64,
    #[serde(default)]
    pub risks: Vec<CohortConditionRisk>,
    pub disclaimer: String,
    pub generated_at: String,
}

// ── Compliance (GDPR-grade subject rights + pack enablement) ───────────────

/// Per-class counts returned alongside a subject export.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportCounts {
    pub memories: u64,
    pub patterns: u64,
    pub observations: u64,
    pub events: u64,
    pub alert_fires: u64,
}

/// Response of [`crate::resources::Compliance::export_subject`] (GDPR Art. 15).
/// `export` is the full bundle the controller hands to the data subject; its
/// nested arrays are opaque records, so it is surfaced as raw JSON.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSubjectResponse {
    pub subject: Subject,
    #[serde(default)]
    pub export: Option<Value>,
    pub counts: ExportCounts,
    pub generated_at: String,
    pub duration_ms: u64,
}

/// Response of [`crate::resources::Compliance::hard_delete_subject`]
/// (GDPR Art. 17). When `dry_run`, no rows are touched and the counts are a
/// preview.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HardDeleteSubjectResponse {
    pub subject: Subject,
    pub memories_deleted: u64,
    pub patterns_deleted: u64,
    pub observations_deleted: u64,
    pub events_deleted: u64,
    pub alert_fires_deleted: u64,
    pub dry_run: bool,
    #[serde(default)]
    pub audit_event_id: Option<String>,
    pub generated_at: String,
    pub duration_ms: u64,
}

/// Filters for [`crate::resources::Compliance::list_audit_events`]. Build with
/// `..Default::default()`.
#[derive(Debug, Clone, Default)]
pub struct ListAuditParams {
    /// Restrict to events touching this subject.
    pub subject: Option<Subject>,
    /// Restrict to a specific actor (user or service key id).
    pub actor: Option<String>,
    /// Event types to include. Empty/None = all.
    pub event_types: Option<Vec<String>>,
    /// ISO-8601 lower bound. Newer events only.
    pub since: Option<String>,
    /// Max events to return. Default 100, max 1000.
    pub limit: Option<u32>,
}

impl ListAuditParams {
    /// Render the set fields as a `?a=b&c=d` query string (empty when none set).
    pub(crate) fn query(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(s) = &self.subject {
            parts.push(format!("subjectKind={}", s.kind));
            parts.push(format!("subjectExternalId={}", s.external_id));
        }
        if let Some(v) = &self.actor {
            parts.push(format!("actor={v}"));
        }
        if let Some(v) = &self.since {
            parts.push(format!("since={v}"));
        }
        if let Some(v) = self.limit {
            parts.push(format!("limit={v}"));
        }
        if let Some(types) = &self.event_types {
            if !types.is_empty() {
                parts.push(format!("eventTypes={}", types.join(",")));
            }
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!("?{}", parts.join("&"))
        }
    }
}

/// One row of the memory audit log. `event_type` is e.g. "read.search",
/// "read.export", "subject.hard_delete".
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEvent {
    pub id: String,
    pub created: String,
    pub actor: String,
    pub event_type: String,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub memory_ids: Option<String>,
    pub result_count: u64,
    #[serde(default)]
    pub metadata: Value,
}

/// An installed compliance pack — the redaction / consent / retention rules a
/// tenant's data is enforced against on every read.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompliancePack {
    /// Stable pack id — e.g. "hipaa", "gdpr".
    pub id: String,
    pub version: String,
    pub description: String,
    /// Memory classes the pack claims jurisdiction over (e.g. "phi", "pii").
    #[serde(default)]
    pub owns_classes: Vec<String>,
    /// Regulatory tags this pack maps to (e.g. "HIPAA", "GDPR-Art-9").
    #[serde(default)]
    pub regulatory_tags: Vec<String>,
}

/// A per-project pack enablement row.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPackEnablement {
    pub id: String,
    pub pack_id: String,
    pub enabled: bool,
    #[serde(default)]
    pub config: Value,
    #[serde(default)]
    pub enabled_by_user_id: Option<String>,
    pub created: String,
    pub updated: String,
}

/// Body for [`crate::resources::Compliance::upsert_project_pack`]. Idempotent on
/// `pack_id`.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpsertProjectPackRequest {
    pub pack_id: String,
    pub enabled: bool,
    /// Pack-owned opaque config. Schema is the pack's responsibility.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<Value>,
}

//! Lattice — behavioral intelligence: mine, forecast, profile, calibrate.
//!
//! Full parity with `thinkfleet-memory-sdk/src/resources/lattice.ts`: pattern
//! extraction + corpus mining, single-pattern / per-contact reads, the retrieval
//! context bundle, the pattern-break monitor, v1 pattern-projection + v2
//! declared-target prediction, behavioral profiles, cohorts, deterministic
//! estimators, and the calibration report.

use std::sync::Arc;

use reqwest::Method;
use serde_json::json;

use crate::{
    BehaviorPatternRecord, CalibrationReport, Error, EstimateRequest, EstimateResult,
    ExtractPatternsRequest, ExtractPatternsResult, GetCohortRequest, GetCohortResponse,
    GetContextParams, Inner, LatticeContextBundle, ListContactPatternsResponse, ListPatternsParams,
    MineMemoriesRequest, MonitorStatus, MonitorTickResult, PredictByCohortRequest,
    PredictByCohortResponse, PredictRequest, PredictResult, PredictionTarget, SubjectProfile,
    Subject, TargetPrediction,
};

/// Accessor for the lattice API. Get one via [`crate::MemMesh::lattice`].
pub struct Lattice {
    pub(crate) c: Arc<Inner>,
}

impl Lattice {
    /// Force pattern (re-)extraction. Omit `contact_id` for a project-wide bulk
    /// run; set it (or `subject`) to limit scope. Defaults to mining the memory
    /// corpus via the Rust engine.
    pub async fn extract_patterns(
        &self,
        body: ExtractPatternsRequest,
    ) -> Result<ExtractPatternsResult, Error> {
        self.c.send(Method::POST, "/lattice/patterns/extract", Some(&body)).await
    }

    /// Mine behavioral patterns from the memory corpus. Subject-agnostic thin
    /// wrapper over [`extract_patterns`](Self::extract_patterns) with
    /// `source="memories"`.
    pub async fn mine_memories(
        &self,
        body: MineMemoriesRequest,
    ) -> Result<ExtractPatternsResult, Error> {
        let req = ExtractPatternsRequest {
            subject: body.subject,
            window_days: body.window_days,
            force: body.force,
            source: Some("memories".into()),
            ..Default::default()
        };
        self.c.send(Method::POST, "/lattice/patterns/extract", Some(&req)).await
    }

    /// Inspect a single pattern by id. 404 if it doesn't exist or belongs to a
    /// different project.
    pub async fn get_pattern(&self, pattern_id: &str) -> Result<BehaviorPatternRecord, Error> {
        let path = format!("/lattice/patterns/{pattern_id}");
        self.c.send::<(), _>(Method::GET, &path, None).await
    }

    /// List behavior patterns learned for a contact. Cursor-paginated — pass the
    /// response's `next_cursor` back via [`ListPatternsParams::cursor`].
    pub async fn list_patterns(
        &self,
        contact_id: &str,
        params: ListPatternsParams,
    ) -> Result<ListContactPatternsResponse, Error> {
        let path = format!("/lattice/contacts/{contact_id}/patterns{}", params.query());
        self.c.send::<(), _>(Method::GET, &path, None).await
    }

    /// Full retrieval bundle for a contact — profile, active patterns, recent
    /// events, recent memories, and optional entity/edge graph. Supports
    /// bi-temporal replay via [`GetContextParams::as_of`].
    pub async fn get_context(
        &self,
        contact_id: &str,
        params: GetContextParams,
    ) -> Result<LatticeContextBundle, Error> {
        let path = format!("/lattice/contacts/{contact_id}/context{}", params.query());
        self.c.send::<(), _>(Method::GET, &path, None).await
    }

    /// Manually run the pattern-break monitor tick (the platform runs it on a
    /// cron; manual triggers exist for debugging + tests).
    pub async fn run_monitor_tick(&self) -> Result<MonitorTickResult, Error> {
        let body = json!({});
        self.c.send(Method::POST, "/lattice/monitor/tick", Some(&body)).await
    }

    /// Monitor health: last-tick timestamp + count of patterns due for the next
    /// check. Useful as a liveness probe.
    pub async fn get_monitor_status(&self) -> Result<MonitorStatus, Error> {
        self.c.send::<(), _>(Method::GET, "/lattice/monitor/status", None).await
    }

    /// Predict for a subject. Two modes: omit `target` (on the request) to
    /// project active behavior patterns forward; set it for v2 general
    /// prediction (the estimate lands in `result.target_prediction`). For the
    /// declared-target case prefer the typed [`predict_target`](Self::predict_target).
    pub async fn predict(&self, body: PredictRequest) -> Result<PredictResult, Error> {
        self.c.send(Method::POST, "/lattice/predict", Some(&body)).await
    }

    /// v2 general prediction, typed and ergonomic: declare *what* to predict and
    /// get back the single calibrated [`TargetPrediction`] (or an abstention).
    /// Thin wrapper over [`predict`](Self::predict) with a `target`. Always check
    /// `.abstained` before reading a value.
    ///
    /// Mirrors the TS helper: when the engine returns no target estimate, a
    /// synthetic abstention is returned so callers never have to null-check.
    pub async fn predict_target(
        &self,
        subject: Subject,
        target: PredictionTarget,
        horizon_days: Option<u32>,
    ) -> Result<TargetPrediction, Error> {
        let event_type = target
            .event_type
            .clone()
            .or_else(|| target.attribute_key.clone())
            .unwrap_or_default();
        let target_kind = target.kind.clone();
        let result = self
            .predict(PredictRequest { subject, target: Some(target), horizon_days, ..Default::default() })
            .await?;
        Ok(result.target_prediction.unwrap_or_else(|| TargetPrediction {
            target_kind,
            event_type,
            probability: 0.0,
            probability_lower: 0.0,
            probability_upper: 0.0,
            value: 0.0,
            value_lower: 0.0,
            value_upper: 0.0,
            expected_at: String::new(),
            expected_at_lower: String::new(),
            expected_at_upper: String::new(),
            days_until: 0.0,
            anomaly_score: 0.0,
            is_anomaly: false,
            abstained: true,
            abstention_reason: {
                let r = result.abstention_reason.unwrap_or_default();
                if r.is_empty() {
                    "insufficient_signal: engine returned no target estimate".into()
                } else {
                    r
                }
            },
            explanation: String::new(),
            evidence_memory_ids: Vec::new(),
        }))
    }

    /// Behavioral profile snapshot for a subject — RFM segment + top entity +
    /// cadence + risks. The non-temporal counterpart to
    /// [`predict`](Self::predict): "who are they?" vs "what will they do?".
    pub async fn get_profile(&self, subject: &Subject) -> Result<SubjectProfile, Error> {
        let body = json!({ "subject": subject });
        self.c.send(Method::POST, "/lattice/profile", Some(&body)).await
    }

    /// Find subjects whose behavior looks like the target's — top-K nearest
    /// neighbors ranked by a blended similarity score.
    pub async fn get_cohort(&self, body: GetCohortRequest) -> Result<GetCohortResponse, Error> {
        self.c.send(Method::POST, "/lattice/cohort", Some(&body)).await
    }

    /// Cohort-aware predictions — "people like the target also did X." Every
    /// prediction carries `supporting_subjects` + `source_memory_ids` for full
    /// traceability.
    pub async fn predict_by_cohort(
        &self,
        body: PredictByCohortRequest,
    ) -> Result<PredictByCohortResponse, Error> {
        self.c.send(Method::POST, "/lattice/cohort/predict", Some(&body)).await
    }

    /// Run a deterministic estimator (e.g. PhenoAge biological age) over a
    /// subject's biomarker signals. Returns a wellness estimate with a
    /// not-a-diagnosis disclaimer — never a medical verdict.
    pub async fn estimate(&self, body: EstimateRequest) -> Result<EstimateResult, Error> {
        self.c.send(Method::POST, "/lattice/estimate", Some(&body)).await
    }

    /// Prediction calibration report: confidence buckets mapped to realized
    /// hit-rates. `bucket_count` = number of equal-width buckets over [0,1]
    /// (default 5, clamped [1,20]); `None` uses the server default.
    pub async fn get_calibration(
        &self,
        bucket_count: Option<u32>,
    ) -> Result<CalibrationReport, Error> {
        let path = match bucket_count {
            Some(n) => format!("/lattice/calibration?bucketCount={n}"),
            None => "/lattice/calibration".to_string(),
        };
        self.c.send::<(), _>(Method::GET, &path, None).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{client, mock_server};
    use serde_json::{json, Value};

    #[tokio::test]
    async fn extract_patterns_posts_camel_body() {
        let body = json!({
            "contactsProcessed": 3, "patternsCreated": 2, "patternsRefreshed": 1,
            "patternsDeactivated": 0, "durationMs": 12,
        })
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm
            .lattice()
            .extract_patterns(ExtractPatternsRequest {
                contact_id: Some("c1".into()),
                window_days: Some(90),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(out.patterns_created, 2);
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        assert!(req.path.ends_with("/lattice/patterns/extract"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["contactId"], "c1");
        assert_eq!(b["windowDays"], 90);
        // Unset fields omitted, not sent as null.
        assert!(b.get("source").is_none());
    }

    #[tokio::test]
    async fn mine_memories_injects_source() {
        let body = json!({
            "contactsProcessed": 1, "patternsCreated": 1, "patternsRefreshed": 0,
            "patternsDeactivated": 0, "durationMs": 5,
        })
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        mm.lattice()
            .mine_memories(MineMemoriesRequest {
                subject: Some(Subject::new("contact", "sarah")),
                window_days: Some(30),
                ..Default::default()
            })
            .await
            .unwrap();
        let req = rx.recv().unwrap();
        assert!(req.path.ends_with("/lattice/patterns/extract"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["source"], "memories");
        assert_eq!(b["subject"]["externalId"], "sarah");
        assert_eq!(b["windowDays"], 30);
    }

    #[tokio::test]
    async fn get_pattern_hits_pattern_route() {
        let body = json!({
            "id": "p1", "projectId": "proj", "contactId": "c1", "summary": "weekly pizza",
            "metadata": {
                "patternKind": "recurring_event", "contactId": "c1", "confidence": 0.8,
                "observationCount": 10, "observationWindowDays": 90,
                "lastObservedAt": "2026-01-01T00:00:00Z", "active": true,
            },
            "active": true, "confidence": 0.8,
            "created": "2026-01-01T00:00:00Z", "updated": "2026-01-02T00:00:00Z",
        })
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm.lattice().get_pattern("p1").await.unwrap();
        assert_eq!(out.id, "p1");
        assert_eq!(out.metadata.pattern_kind, "recurring_event");
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "GET");
        assert!(req.path.ends_with("/lattice/patterns/p1"));
    }

    #[tokio::test]
    async fn list_patterns_builds_query() {
        let body = json!({ "data": [], "nextCursor": null }).to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm
            .lattice()
            .list_patterns(
                "c1",
                ListPatternsParams { active_only: Some(false), limit: Some(25), ..Default::default() },
            )
            .await
            .unwrap();
        assert!(out.data.is_empty());
        assert!(out.next_cursor.is_none());
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "GET");
        assert!(req.path.contains("/lattice/contacts/c1/patterns?"));
        assert!(req.path.contains("activeOnly=false"));
        assert!(req.path.contains("limit=25"));
    }

    #[tokio::test]
    async fn get_context_bundle_parses() {
        let body = json!({
            "contactId": "c1",
            "contact": { "id": "c1", "displayName": "Sarah" },
            "activePatterns": [],
            "recentEvents": [],
            "recentMemories": [],
        })
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm
            .lattice()
            .get_context("c1", GetContextParams { events_limit: Some(50), ..Default::default() })
            .await
            .unwrap();
        assert_eq!(out.contact_id, "c1");
        assert_eq!(out.contact.display_name.as_deref(), Some("Sarah"));
        let req = rx.recv().unwrap();
        assert!(req.path.contains("/lattice/contacts/c1/context?"));
        assert!(req.path.contains("eventsLimit=50"));
    }

    #[tokio::test]
    async fn run_monitor_tick_posts_empty_body() {
        let body = json!({
            "patternsChecked": 4, "patternsBroken": 1, "breaksEmitted": 1,
            "durationMs": 9, "capped": false,
        })
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm.lattice().run_monitor_tick().await.unwrap();
        assert_eq!(out.patterns_broken, 1);
        assert!(!out.capped);
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        assert!(req.path.ends_with("/lattice/monitor/tick"));
    }

    #[tokio::test]
    async fn get_monitor_status_parses() {
        let body = json!({
            "lastTickAt": "2026-01-01T00:00:00Z", "lastTickDurationMs": 7,
            "patternsDue": 3, "activePatternCount": 12,
        })
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm.lattice().get_monitor_status().await.unwrap();
        assert_eq!(out.patterns_due, 3);
        assert_eq!(out.active_pattern_count, 12);
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "GET");
        assert!(req.path.ends_with("/lattice/monitor/status"));
    }

    #[tokio::test]
    async fn predict_pattern_projection_omits_target() {
        let body = json!({
            "subject": { "kind": "contact", "externalId": "sarah" },
            "predictions": [{
                "patternId": "p1", "patternKind": "recurring_event",
                "description": "pizza order", "expectedAt": "2026-02-01T00:00:00Z",
                "confidence": 0.7, "windowMinutes": 120, "sourceMemoryIds": ["m1"],
            }],
            "activePatternCount": 1, "generatedAt": "2026-01-01T00:00:00Z", "durationMs": 8,
        })
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm
            .lattice()
            .predict(PredictRequest {
                subject: Subject::new("contact", "sarah"),
                horizon_days: Some(30),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(out.predictions.len(), 1);
        assert_eq!(out.predictions[0].pattern_id, "p1");
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        assert!(req.path.ends_with("/lattice/predict"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["horizonDays"], 30);
        // Pattern-projection mode: no target on the wire.
        assert!(b.get("target").is_none());
    }

    #[tokio::test]
    async fn predict_target_returns_estimate() {
        let body = json!({
            "subject": { "kind": "customer", "externalId": "acct-42" },
            "predictions": [], "activePatternCount": 0,
            "generatedAt": "2026-01-01T00:00:00Z", "durationMs": 8,
            "targetPrediction": {
                "targetKind": "event_occurrence", "eventType": "subscription_cancelled",
                "probability": 0.62, "probabilityLower": 0.5, "probabilityUpper": 0.74,
                "value": 0.0, "valueLower": 0.0, "valueUpper": 0.0,
                "expectedAt": "", "expectedAtLower": "", "expectedAtUpper": "",
                "daysUntil": 0.0, "anomalyScore": 0.0, "isAnomaly": false,
                "abstained": false, "abstentionReason": "",
                "explanation": "3 of 5 similar cancelled", "evidenceMemoryIds": ["m1", "m2"],
            },
        })
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let p = mm
            .lattice()
            .predict_target(
                Subject::new("customer", "acct-42"),
                PredictionTarget {
                    kind: "event_occurrence".into(),
                    event_type: Some("subscription_cancelled".into()),
                    ..Default::default()
                },
                Some(90),
            )
            .await
            .unwrap();
        assert!(!p.abstained);
        assert_eq!(p.probability, 0.62);
        assert_eq!(p.evidence_memory_ids, vec!["m1", "m2"]);
        let req = rx.recv().unwrap();
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["target"]["kind"], "event_occurrence");
        assert_eq!(b["target"]["eventType"], "subscription_cancelled");
        assert_eq!(b["horizonDays"], 90);
    }

    #[tokio::test]
    async fn predict_target_synthesizes_abstention() {
        // Engine returns no targetPrediction → helper synthesizes an abstention.
        let body = json!({
            "subject": { "kind": "customer", "externalId": "acct-42" },
            "predictions": [], "activePatternCount": 0,
            "generatedAt": "2026-01-01T00:00:00Z", "durationMs": 8,
            "abstained": true, "abstentionReason": "cold_start",
        })
        .to_string();
        let (base, _rx) = mock_server(vec![body]);
        let mm = client(&base);
        let p = mm
            .lattice()
            .predict_target(
                Subject::new("customer", "acct-42"),
                PredictionTarget::new("numeric"),
                None,
            )
            .await
            .unwrap();
        assert!(p.abstained);
        assert_eq!(p.abstention_reason, "cold_start");
        assert_eq!(p.target_kind, "numeric");
    }

    #[tokio::test]
    async fn get_profile_posts_subject() {
        let body = json!({
            "subject": { "kind": "contact", "externalId": "sarah" },
            "rfmSegment": "at_risk_high_value", "recencyScore": 0.3,
            "frequencyScore": null, "monetaryScore": null,
            "topEntity": "Tony's", "cadenceSummary": "weekly",
            "risks": [{
                "kind": "declining_engagement", "description": "slowing down",
                "severity": 0.6, "sourcePatternId": "p9",
            }],
            "contributingPatternIds": ["p9"],
            "generatedAt": "2026-01-01T00:00:00Z", "durationMs": 4,
        })
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm.lattice().get_profile(&Subject::new("contact", "sarah")).await.unwrap();
        assert_eq!(out.rfm_segment.as_deref(), Some("at_risk_high_value"));
        assert_eq!(out.risks.len(), 1);
        assert_eq!(out.risks[0].source_pattern_id, "p9");
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        assert!(req.path.ends_with("/lattice/profile"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["subject"]["externalId"], "sarah");
    }

    #[tokio::test]
    async fn get_cohort_posts_and_parses() {
        let body = json!({
            "target": { "kind": "contact", "externalId": "sarah" },
            "members": [{
                "subject": { "kind": "contact", "externalId": "bob" },
                "similarity": 0.82, "rfmSegment": "loyal", "patternKinds": ["recurring_event"],
            }],
            "candidateCount": 40, "generatedAt": "2026-01-01T00:00:00Z", "durationMs": 15,
        })
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm
            .lattice()
            .get_cohort(GetCohortRequest {
                subject: Subject::new("contact", "sarah"),
                k: Some(10),
                min_similarity: Some(0.5),
            })
            .await
            .unwrap();
        assert_eq!(out.members.len(), 1);
        assert_eq!(out.members[0].subject.external_id, "bob");
        let req = rx.recv().unwrap();
        assert!(req.path.ends_with("/lattice/cohort"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["k"], 10);
        assert_eq!(b["minSimilarity"], 0.5);
    }

    #[tokio::test]
    async fn predict_by_cohort_hits_route() {
        let body = json!({
            "target": { "kind": "contact", "externalId": "sarah" },
            "cohort": [],
            "predictions": [{
                "patternKind": "recurring_event", "description": "reorder",
                "expectedAt": "2026-02-01T00:00:00Z", "confidence": 0.66,
                "windowMinutes": 60,
                "supportingSubjects": [{ "kind": "contact", "externalId": "bob" }],
                "sourceMemoryIds": ["m1"],
            }],
            "generatedAt": "2026-01-01T00:00:00Z", "durationMs": 22,
        })
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm
            .lattice()
            .predict_by_cohort(PredictByCohortRequest {
                subject: Subject::new("contact", "sarah"),
                cohort_k: Some(10),
                prediction_limit: Some(5),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(out.predictions.len(), 1);
        assert_eq!(out.predictions[0].supporting_subjects.len(), 1);
        let req = rx.recv().unwrap();
        assert!(req.path.ends_with("/lattice/cohort/predict"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["cohortK"], 10);
        assert_eq!(b["predictionLimit"], 5);
    }

    #[tokio::test]
    async fn estimate_posts_and_parses() {
        let body = json!({
            "subject": { "kind": "patient", "externalId": "p-123" },
            "estimatorId": "phenoage", "ok": true, "value": 42.3, "unit": "years",
            "contributors": [{ "signal": "crp", "contribution": 1.2 }],
            "confidence": 0.9, "provenance": ["m1"],
            "framing": "estimate", "disclaimer": "not a diagnosis", "missingSignals": [],
        })
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm
            .lattice()
            .estimate(EstimateRequest {
                subject: Subject::new("patient", "p-123"),
                estimator_id: "phenoage".into(),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(out.ok);
        assert_eq!(out.value, 42.3);
        assert_eq!(out.contributors[0].signal, "crp");
        let req = rx.recv().unwrap();
        assert!(req.path.ends_with("/lattice/estimate"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["estimatorId"], "phenoage");
    }

    #[tokio::test]
    async fn get_calibration_builds_query() {
        let body = json!({
            "buckets": [{
                "lower": 0.0, "upper": 0.5, "patterns": 3, "predictions": 10,
                "hits": 4, "misses": 6, "realizedHitRate": 0.4, "hasData": true,
            }],
            "totalPatterns": 3, "totalPredictions": 10,
        })
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm.lattice().get_calibration(Some(5)).await.unwrap();
        assert_eq!(out.total_predictions, 10);
        assert_eq!(out.buckets.len(), 1);
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "GET");
        assert!(req.path.contains("/lattice/calibration?bucketCount=5"));
    }
}

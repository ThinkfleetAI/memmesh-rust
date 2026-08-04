//! Remaining resources: events, alerts, learning, compliance, health.

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use reqwest::Method;
use serde::Deserialize;
use serde_json::json;

use crate::{
    AlertFire, AlertRule, AuditEvent, CohortHealthRisk, CompliancePack, ConditionInput,
    CreateAlertRuleRequest, DemographicsInput, DiscoverParams, DiscoverResult, EffectivenessRow,
    EmitEventRequest, EmitEventResult, Error, ExportSubjectResponse, GetEffectivenessParams,
    GetOutcomesParams, HardDeleteSubjectResponse, HealthProfile, Inner, ListAuditParams, MemoryEvent,
    MemoryItem, OutcomeRecord, PollEventsParams, ProjectPackEnablement, RecordBiomarkerOpts,
    RecordDecisionInput, RecordDecisionResult, RecordOutcomeInput, RecordOutcomeResult, Subject,
    UpdateAlertRuleRequest, UpsertProjectPackRequest,
};

macro_rules! service {
    ($name:ident) => {
        pub struct $name {
            pub(crate) c: Arc<Inner>,
        }
    };
}

service!(Events);
service!(Alerts);
service!(Learning);
service!(Behaviors);
service!(Compliance);
service!(Health);

/// A running background subscription started by [`Events::subscribe`].
///
/// The spawned polling loop runs until [`stop`](Self::stop) is called (or the
/// handle is dropped after calling `stop`). `stop` is cooperative — the loop
/// exits at the next poll/handler boundary. Prefer [`stop_and_join`] when you
/// need to be sure the task has fully wound down.
pub struct Subscription {
    stop: Arc<AtomicBool>,
    handle: tokio::task::JoinHandle<()>,
}

impl Subscription {
    /// Signal the polling loop to stop. Returns immediately; the loop exits at
    /// its next boundary.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    /// Signal the loop to stop and await its full shutdown.
    pub async fn stop_and_join(self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.handle.await;
    }

    /// True once the loop's task has finished.
    pub fn is_finished(&self) -> bool {
        self.handle.is_finished()
    }

    /// Forcibly abort the background task without waiting for a clean boundary.
    pub fn abort(&self) {
        self.handle.abort();
    }
}

impl Events {
    /// Pull events newer than `since`. Single round-trip; use the last event's
    /// `occurred_at` as the next `since` to walk the log.
    pub async fn poll(&self, params: PollEventsParams) -> Result<Vec<MemoryEvent>, Error> {
        let path = format!("/memory-events{}", params.query());
        self.c.send::<(), _>(Method::GET, &path, None).await
    }

    /// Append an event to the durable log. Matching alert rules fire
    /// synchronously. The write-side counterpart to [`poll`](Self::poll).
    pub async fn emit(&self, body: EmitEventRequest) -> Result<EmitEventResult, Error> {
        self.c.send(Method::POST, "/lattice/events/emit", Some(&body)).await
    }

    /// Poll in a background loop and invoke `handler` for each event, returning a
    /// [`Subscription`] handle you call [`stop`](Subscription::stop) on to cancel.
    ///
    /// The loop advances its cursor by each event's `occurred_at`, sleeps
    /// `interval` (clamped to a 500ms floor) between rounds, and swallows both
    /// network blips and handler errors so a transient failure never kills the
    /// subscription — matching the TS `subscribe()` semantics.
    pub fn subscribe<F, Fut>(
        &self,
        params: PollEventsParams,
        interval: Duration,
        handler: F,
    ) -> Subscription
    where
        F: Fn(MemoryEvent) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let events = Events { c: self.c.clone() };
        let stop = Arc::new(AtomicBool::new(false));
        let stop_loop = stop.clone();
        let interval = interval.max(Duration::from_millis(500));
        let mut cursor = params.since.clone();

        let handle = tokio::spawn(async move {
            while !stop_loop.load(Ordering::SeqCst) {
                let p = PollEventsParams { since: cursor.clone(), ..params.clone() };
                // A poll error is a network blip — swallow it and retry on the
                // next tick so a transient failure never kills the loop.
                if let Ok(batch) = events.poll(p).await {
                    for e in batch {
                        if stop_loop.load(Ordering::SeqCst) {
                            break;
                        }
                        cursor = Some(e.occurred_at.clone());
                        handler(e).await;
                    }
                }
                tokio::time::sleep(interval).await;
            }
        });

        Subscription { stop, handle }
    }
}

impl Alerts {
    /// List all alert rules for the project.
    pub async fn list(&self) -> Result<Vec<AlertRule>, Error> {
        self.c.send::<(), _>(Method::GET, "/memory-alerts", None).await
    }

    /// Fetch a single alert rule by id.
    pub async fn get(&self, alert_id: &str) -> Result<AlertRule, Error> {
        self.c.send::<(), _>(Method::GET, &format!("/memory-alerts/{alert_id}"), None).await
    }

    /// Create a new alert rule.
    pub async fn create(&self, body: CreateAlertRuleRequest) -> Result<AlertRule, Error> {
        self.c.send(Method::POST, "/memory-alerts", Some(&body)).await
    }

    /// Patch an existing alert rule (only the fields you set are sent).
    pub async fn update(
        &self,
        alert_id: &str,
        body: UpdateAlertRuleRequest,
    ) -> Result<AlertRule, Error> {
        self.c.send(Method::PATCH, &format!("/memory-alerts/{alert_id}"), Some(&body)).await
    }

    /// Delete an alert rule.
    pub async fn delete(&self, alert_id: &str) -> Result<(), Error> {
        self.c.send::<(), ()>(Method::DELETE, &format!("/memory-alerts/{alert_id}"), None).await
    }

    /// Convenience — patch only the `enabled` flag to `true`.
    pub async fn enable(&self, alert_id: &str) -> Result<AlertRule, Error> {
        self.update(alert_id, UpdateAlertRuleRequest { enabled: Some(true), ..Default::default() })
            .await
    }

    /// Convenience — patch only the `enabled` flag to `false`.
    pub async fn disable(&self, alert_id: &str) -> Result<AlertRule, Error> {
        self.update(alert_id, UpdateAlertRuleRequest { enabled: Some(false), ..Default::default() })
            .await
    }

    /// Last ~100 fires for an alert rule, newest first.
    pub async fn list_fires(&self, alert_id: &str) -> Result<Vec<AlertFire>, Error> {
        self.c
            .send::<(), _>(Method::GET, &format!("/memory-alerts/{alert_id}/fires"), None)
            .await
    }
}

/// Server envelope for `GET /lattice/outcomes` — `{ "outcomes": [...] }`.
#[derive(Deserialize)]
struct OutcomesResponse {
    #[serde(default)]
    outcomes: Vec<OutcomeRecord>,
}

/// Server envelope for `GET /lattice/effectiveness` — `{ "rows": [...] }`.
#[derive(Deserialize)]
struct EffectivenessResponse {
    #[serde(default)]
    rows: Vec<EffectivenessRow>,
}

/// Learning — the closed-loop **decision → action → outcome** primitive.
///
/// Where [`crate::Lattice::predict`] answers "what will happen?", learning
/// answers "did acting on it work?". Record a decision (with links to the
/// patterns/predictions that informed it), record its realized outcome, and
/// every informing pattern's calibrated confidence moves toward what actually
/// happened. [`get_effectiveness`](Self::get_effectiveness) rolls "what worked"
/// up per action_type / decision_type / policy / pattern_kind.
impl Learning {
    /// Record a decision and its causal provenance.
    pub async fn record_decision(
        &self,
        body: RecordDecisionInput,
    ) -> Result<RecordDecisionResult, Error> {
        self.c.send(Method::POST, "/lattice/decisions", Some(&body)).await
    }

    /// Record the realized outcome of a decision. Folds the result into the
    /// online calibrated confidence of every pattern the decision was
    /// `informed_by`, and returns the before/after for each.
    pub async fn record_outcome(
        &self,
        body: RecordOutcomeInput,
    ) -> Result<RecordOutcomeResult, Error> {
        self.c.send(Method::POST, "/lattice/outcomes", Some(&body)).await
    }

    /// List recorded outcomes for a subject (or the whole scope), newest first.
    pub async fn get_outcomes(
        &self,
        params: GetOutcomesParams,
    ) -> Result<Vec<OutcomeRecord>, Error> {
        let path = format!("/lattice/outcomes{}", params.query());
        let r: OutcomesResponse = self.c.send::<(), _>(Method::GET, &path, None).await?;
        Ok(r.outcomes)
    }

    /// "What worked" roll-up — success rate, average reward, and posterior
    /// confidence per group. Per-scope only.
    pub async fn get_effectiveness(
        &self,
        params: GetEffectivenessParams,
    ) -> Result<Vec<EffectivenessRow>, Error> {
        let path = format!("/lattice/effectiveness{}", params.query());
        let r: EffectivenessResponse = self.c.send::<(), _>(Method::GET, &path, None).await?;
        Ok(r.rows)
    }
}

/// Behaviors — emergent behavior discovery.
///
/// Where [`crate::Lattice::predict`] answers "what will this subject do?" and
/// `get_profile` answers "who is this subject?", `discover` answers a
/// project-wide question: **"what behaviors exist in my data that nobody
/// defined?"** It clusters subjects by their feature vectors and surfaces the
/// dense, cohesive groups as behaviors — each with prevalence, stability,
/// members, and explainable evidence.
impl Behaviors {
    /// Discover emergent behaviors across the project. Returns clusters of
    /// like-behaving subjects, sorted most-common-and-cohesive first. An empty
    /// result means the engine abstained — not enough signal to assert any
    /// behavior — never "there are no behaviors".
    pub async fn discover(&self, params: DiscoverParams) -> Result<DiscoverResult, Error> {
        self.c.send(Method::POST, "/lattice/discover", Some(&params)).await
    }
}

/// Compliance — GDPR-grade subject rights + compliance-pack enablement.
///
/// Two subject-scoped operations back the data-subject rights:
///  - [`export_subject`](Self::export_subject) (Art. 15) — return every memory,
///    pattern, observation, event, and alert fire for the subject in one bundle.
///  - [`hard_delete_subject`](Self::hard_delete_subject) (Art. 17) —
///    cascade-delete the same set and write a tombstone audit row.
///
/// [`list_audit_events`](Self::list_audit_events) reads the access log ("who
/// accessed my data and when"); the pack methods surface and configure which
/// compliance packs are enforced on the project's data.
impl Compliance {
    /// GDPR Art. 15 subject-access request — every record held for the subject,
    /// plus per-class counts.
    pub async fn export_subject(&self, subject: &Subject) -> Result<ExportSubjectResponse, Error> {
        let body = json!({ "subject": subject });
        self.c.send(Method::POST, "/memory-compliance/export", Some(&body)).await
    }

    /// GDPR Art. 17 right-to-erasure — cascade-delete the subject's records.
    /// `reason` is required (it lands in the audit log). Pass `dry_run` to
    /// preview counts without touching any rows.
    pub async fn hard_delete_subject(
        &self,
        subject: &Subject,
        reason: &str,
        dry_run: bool,
    ) -> Result<HardDeleteSubjectResponse, Error> {
        let body = json!({ "subject": subject, "reason": reason, "dryRun": dry_run });
        self.c.send(Method::POST, "/memory-compliance/hard-delete", Some(&body)).await
    }

    /// Read the memory audit log. Pass a subject to narrow to one person's
    /// access log; omit for a project-wide audit.
    pub async fn list_audit_events(
        &self,
        params: ListAuditParams,
    ) -> Result<Vec<AuditEvent>, Error> {
        let path = format!("/memory-compliance/audit{}", params.query());
        self.c.send::<(), _>(Method::GET, &path, None).await
    }

    /// List installed compliance packs — the redaction / consent / retention
    /// rules enforced on every read.
    pub async fn list_packs(&self) -> Result<Vec<CompliancePack>, Error> {
        self.c.send::<(), _>(Method::GET, "/memory-compliance/packs", None).await
    }

    /// List which compliance packs are enabled (or explicitly disabled) for the
    /// current project. Packs absent from the list fall back to the platform
    /// default set.
    pub async fn list_project_packs(&self) -> Result<Vec<ProjectPackEnablement>, Error> {
        self.c.send::<(), _>(Method::GET, "/memory-compliance-packs", None).await
    }

    /// Enable, disable, or reconfigure a compliance pack for the current
    /// project. Idempotent on `pack_id`.
    pub async fn upsert_project_pack(
        &self,
        body: UpsertProjectPackRequest,
    ) -> Result<ProjectPackEnablement, Error> {
        self.c.send(Method::POST, "/memory-compliance-packs", Some(&body)).await
    }

    /// Remove the per-project enablement row for a pack, reverting it to the
    /// platform default set. Use `upsert_project_pack` with `enabled: false` to
    /// explicitly disable instead.
    pub async fn remove_project_pack(&self, pack_id: &str) -> Result<(), Error> {
        let path = format!("/memory-compliance-packs/{}", urlencode(pack_id));
        self.c.send::<(), ()>(Method::DELETE, &path, None).await
    }
}

/// Percent-encode a path segment (RFC 3986 unreserved chars pass through).
/// Mirrors `encodeURIComponent` closely enough for pack ids like
/// `@thinkfleet/pack-healthcare`.
fn urlencode(s: &str) -> String {
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

/// Health — biological ("health") age + condition prediction for the memory
/// engine's health vertical (gated behind `@thinkfleet/pack-healthcare`).
///
/// Health data is just memory data: record biomarkers, demographics, and
/// ICD-10 diagnoses as memory items via the `record_*` methods, and the engine
/// derives a biological age and condition predictions
/// ([`get_profile`](Self::get_profile)) plus cohort base rates
/// ([`cohort_risk`](Self::cohort_risk)). Recorded items are stored as
/// non-activity `fact` memories, so they feed the health engine without being
/// mined as behavioral patterns.
impl Health {
    /// Record a biomarker reading. Send whatever unit the lab reported via
    /// `opts.unit`; the engine normalizes it.
    pub async fn record_biomarker(
        &self,
        subject: &Subject,
        biomarker: &str,
        value: f64,
        opts: RecordBiomarkerOpts,
    ) -> Result<MemoryItem, Error> {
        let mut health = json!({ "biomarker": biomarker, "value": value });
        if let Some(unit) = &opts.unit {
            health["unit"] = json!(unit);
        }
        if let Some(observed_at) = &opts.observed_at {
            health["observedAt"] = json!(observed_at);
        }
        let content = match &opts.unit {
            Some(unit) => format!("{biomarker} = {value} {unit}"),
            None => format!("{biomarker} = {value}"),
        };
        let body = json!({
            "content": content,
            "type": "fact",
            "scope": "project",
            "category": "health",
            "source": "sdk:health",
            "metadata": { "subject": subject, "health": health },
        });
        self.c.send(Method::POST, "/admin/memory", Some(&body)).await
    }

    /// Record/refresh a subject's demographics. Latest values win.
    pub async fn record_demographics(
        &self,
        subject: &Subject,
        demographics: DemographicsInput,
    ) -> Result<MemoryItem, Error> {
        let body = json!({
            "content": "Demographics update",
            "type": "fact",
            "scope": "project",
            "category": "health",
            "source": "sdk:health",
            "metadata": { "subject": subject, "demographic": demographics },
        });
        self.c.send(Method::POST, "/admin/memory", Some(&body)).await
    }

    /// Record an ICD-10 diagnosis.
    pub async fn record_condition(
        &self,
        subject: &Subject,
        condition: ConditionInput,
    ) -> Result<MemoryItem, Error> {
        let body = json!({
            "content": format!("Diagnosis {}", condition.icd10),
            "type": "fact",
            "scope": "project",
            "category": "health",
            "source": "sdk:health",
            "metadata": { "subject": subject, "condition": condition },
        });
        self.c.send(Method::POST, "/admin/memory", Some(&body)).await
    }

    /// Biological-age estimate + condition predictions + latest biomarkers for a
    /// subject, derived from their recorded health data.
    pub async fn get_profile(&self, subject: &Subject) -> Result<HealthProfile, Error> {
        let body = json!({ "subject": subject });
        self.c.send(Method::POST, "/lattice/health/profile", Some(&body)).await
    }

    /// Cohort outcomes — condition prevalence among the patients most similar to
    /// this subject. `k` is the cohort size (nearest patients); default 25.
    pub async fn cohort_risk(
        &self,
        subject: &Subject,
        k: Option<u32>,
    ) -> Result<CohortHealthRisk, Error> {
        let body = match k {
            Some(k) => json!({ "subject": subject, "k": k }),
            None => json!({ "subject": subject }),
        };
        self.c.send(Method::POST, "/lattice/health/cohort-risk", Some(&body)).await
    }
}

#[cfg(test)]
mod tests {
    use crate::test_support::{client, mock_server};
    use crate::{
        AlertTrigger, ConditionInput, CreateAlertRuleRequest, DemographicsInput, DiscoverParams,
        EmitEventRequest, EventSeverity, GetEffectivenessParams, GetOutcomesParams, ListAuditParams,
        NotificationChannel, PollEventsParams, ProvenanceRef, RecordBiomarkerOpts,
        RecordDecisionInput, RecordOutcomeInput, Subject, UpdateAlertRuleRequest,
        UpsertProjectPackRequest,
    };
    use serde_json::{json, Value};

    // ── Events ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn emit_posts_lattice_events_route() {
        let body = json!({
            "emitted": true,
            "event": {
                "id": "e1", "eventType": "cart.abandoned",
                "severity": "warn", "occurredAt": "2026-01-01T00:00:00Z",
            },
            "alertDispatches": 2,
        })
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm
            .events()
            .emit(EmitEventRequest {
                event_type: "cart.abandoned".into(),
                subject: Some(Subject::new("contact", "sarah")),
                severity: Some(EventSeverity::Warn),
                payload_json: Some(r#"{"cartValue":84}"#.into()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(out.emitted);
        assert_eq!(out.alert_dispatches, 2);
        assert_eq!(out.event.unwrap().id, "e1");
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        // Canonical /lattice/events/emit route, not the stale /events one.
        assert!(req.path.ends_with("/lattice/events/emit"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["eventType"], "cart.abandoned");
        assert_eq!(b["severity"], "warn");
        assert_eq!(b["subject"]["externalId"], "sarah");
        // Unset field omitted, not null.
        assert!(b.get("sourcePatternId").is_none());
    }

    #[tokio::test]
    async fn poll_gets_memory_events_route_with_filters() {
        let body = json!([
            {
                "id": "e1", "eventType": "risk.fired",
                "subject": { "kind": "contact", "externalId": "sarah" },
                "severity": "critical", "payload": { "riskKind": "churn" },
                "sourceMemoryIds": ["m1"], "sourcePatternId": null,
                "emittedByPack": null, "occurredAt": "2026-01-01T00:00:00Z",
            }
        ])
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm
            .events()
            .poll(PollEventsParams {
                since: Some("2026-01-01T00:00:00Z".into()),
                limit: Some(100),
                event_types: Some(vec!["risk.fired".into(), "segment.changed".into()]),
            })
            .await
            .unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].event_type, "risk.fired");
        assert_eq!(out[0].severity, EventSeverity::Critical);
        assert_eq!(out[0].source_memory_ids, vec!["m1".to_string()]);
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "GET");
        assert!(req.path.contains("/memory-events?"));
        assert!(req.path.contains("since=2026-01-01T00:00:00Z"));
        assert!(req.path.contains("limit=100"));
        assert!(req.path.contains("eventTypes=risk.fired,segment.changed"));
    }

    #[tokio::test]
    async fn subscribe_starts_delivers_and_stops() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        // One canned page of two events; every later poll returns [] so the loop
        // keeps ticking without re-delivering.
        let page = json!([
            {
                "id": "e1", "eventType": "risk.fired", "subject": null,
                "severity": "info", "payload": {}, "sourceMemoryIds": [],
                "sourcePatternId": null, "emittedByPack": null,
                "occurredAt": "2026-01-01T00:00:00Z",
            },
            {
                "id": "e2", "eventType": "risk.fired", "subject": null,
                "severity": "info", "payload": {}, "sourceMemoryIds": [],
                "sourcePatternId": null, "emittedByPack": null,
                "occurredAt": "2026-01-01T00:00:01Z",
            }
        ])
        .to_string();
        let empty = "[]".to_string();
        let mut responses = vec![page];
        responses.extend(std::iter::repeat(empty).take(20));
        let (base, _rx) = mock_server(responses);
        let mm = client(&base);

        let seen = Arc::new(AtomicUsize::new(0));
        let seen2 = seen.clone();
        let sub = mm.events().subscribe(
            PollEventsParams::default(),
            std::time::Duration::from_millis(500),
            move |_e| {
                let seen = seen2.clone();
                async move {
                    seen.fetch_add(1, Ordering::SeqCst);
                }
            },
        );

        // Give the loop time to pull the first page and deliver both events.
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        assert_eq!(seen.load(Ordering::SeqCst), 2, "both events delivered");

        // Stopping halts the loop and the task winds down.
        sub.stop_and_join().await;
        assert_eq!(seen.load(Ordering::SeqCst), 2, "no further deliveries after stop");
    }

    // ── Alerts ───────────────────────────────────────────────────────────

    fn alert_rule_json(enabled: bool) -> Value {
        json!({
            "id": "al1", "projectId": "proj", "name": "VIP at risk",
            "description": null, "enabled": enabled,
            "trigger": { "kind": "engine-event", "eventTypes": ["risk.fired"] },
            "filter": null, "notify": [{ "kind": "webhook", "url": "https://hooks.slack.com/x" }],
            "throttle": null,
            "created": "2026-01-01T00:00:00Z", "updated": "2026-01-01T00:00:00Z",
        })
    }

    #[tokio::test]
    async fn alerts_list_gets_memory_alerts_route() {
        let body = json!([alert_rule_json(true)]).to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm.alerts().list().await.unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "VIP at risk");
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "GET");
        // Corrected /memory-alerts route, not the stale /alerts one.
        assert!(req.path.ends_with("/memory-alerts"));
    }

    #[tokio::test]
    async fn alerts_get_gets_memory_alerts_by_id() {
        let body = alert_rule_json(true).to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm.alerts().get("al1").await.unwrap();
        assert_eq!(out.id, "al1");
        matches!(out.trigger, AlertTrigger::EngineEvent { .. });
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "GET");
        assert!(req.path.ends_with("/memory-alerts/al1"));
    }

    #[tokio::test]
    async fn alerts_create_posts_memory_alerts_route() {
        let body = alert_rule_json(true).to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm
            .alerts()
            .create(CreateAlertRuleRequest {
                name: "VIP at risk".into(),
                trigger: AlertTrigger::EngineEvent { event_types: vec!["risk.fired".into()] },
                notify: vec![NotificationChannel::Webhook {
                    url: "https://hooks.slack.com/x".into(),
                    secret: None,
                }],
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(out.id, "al1");
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        assert!(req.path.ends_with("/memory-alerts"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["name"], "VIP at risk");
        // Discriminated unions serialize with the `kind` tag + camelCase fields.
        assert_eq!(b["trigger"]["kind"], "engine-event");
        assert_eq!(b["trigger"]["eventTypes"][0], "risk.fired");
        assert_eq!(b["notify"][0]["kind"], "webhook");
        assert_eq!(b["notify"][0]["url"], "https://hooks.slack.com/x");
        // Unset optionals omitted, not null.
        assert!(b.get("throttle").is_none());
        assert!(b["notify"][0].get("secret").is_none());
    }

    #[tokio::test]
    async fn alerts_update_patches_memory_alerts_route() {
        let body = alert_rule_json(true).to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let _ = mm
            .alerts()
            .update("al1", UpdateAlertRuleRequest { name: Some("renamed".into()), ..Default::default() })
            .await
            .unwrap();
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "PATCH");
        assert!(req.path.ends_with("/memory-alerts/al1"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["name"], "renamed");
        assert!(b.get("enabled").is_none());
    }

    #[tokio::test]
    async fn alerts_enable_disable_patch_enabled_flag() {
        let (base, rx) = mock_server(vec![
            alert_rule_json(true).to_string(),
            alert_rule_json(false).to_string(),
        ]);
        let mm = client(&base);

        let _ = mm.alerts().enable("al1").await.unwrap();
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "PATCH");
        assert!(req.path.ends_with("/memory-alerts/al1"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["enabled"], true);

        let _ = mm.alerts().disable("al1").await.unwrap();
        let req = rx.recv().unwrap();
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["enabled"], false);
    }

    #[tokio::test]
    async fn alerts_delete_hits_memory_alerts_route() {
        let (base, rx) = mock_server(vec![String::new()]);
        let mm = client(&base);
        mm.alerts().delete("al1").await.unwrap();
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "DELETE");
        assert!(req.path.ends_with("/memory-alerts/al1"));
    }

    #[tokio::test]
    async fn alerts_list_fires_gets_nested_fires_route() {
        let body = json!([
            {
                "id": "f1", "alertRuleId": "al1", "eventId": "e1",
                "dedupeKey": "sarah:risk", "deliveryResults": [{ "channel": "webhook", "ok": true }],
                "firedAt": "2026-01-01T00:00:00Z",
            }
        ])
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm.alerts().list_fires("al1").await.unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "f1");
        assert!(out[0].delivery_results[0].ok);
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "GET");
        assert!(req.path.ends_with("/memory-alerts/al1/fires"));
    }

    #[tokio::test]
    async fn record_decision_hits_lattice_route() {
        let body = json!({
            "decision": {
                "decisionId": "d1",
                "subject": { "kind": "contact", "externalId": "sarah" },
                "actor": "policy:winback-v1", "decisionType": "offer", "policy": "winback-v1",
                "informedBy": [{ "memoryId": "p1", "refType": "pattern", "weight": 1.0 }],
                "actionType": "apply_discount", "status": "executed",
                "occurredAt": "2026-01-01T00:00:00Z", "created": "2026-01-01T00:00:00Z",
            }
        })
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm
            .learning()
            .record_decision(RecordDecisionInput {
                subject: Subject::new("contact", "sarah"),
                actor: Some("policy:winback-v1".into()),
                decision_type: Some("offer".into()),
                action_type: Some("apply_discount".into()),
                informed_by: Some(vec![ProvenanceRef {
                    memory_id: "p1".into(),
                    ref_type: Some("pattern".into()),
                    ..Default::default()
                }]),
                ..Default::default()
            })
            .await
            .unwrap();
        let decision = out.decision.unwrap();
        assert_eq!(decision.decision_id, "d1");
        assert_eq!(decision.informed_by[0].memory_id, "p1");
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        // Canonical /lattice route, not the stale /learning one.
        assert!(req.path.ends_with("/lattice/decisions"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["subject"]["externalId"], "sarah");
        assert_eq!(b["actionType"], "apply_discount");
        assert_eq!(b["informedBy"][0]["memoryId"], "p1");
        // Unset fields omitted, not null.
        assert!(b.get("policy").is_none());
    }

    #[tokio::test]
    async fn record_outcome_hits_lattice_route() {
        let body = json!({
            "outcomeId": "o1",
            "updates": [{
                "refId": "p1", "refType": "pattern",
                "priorConfidence": 0.5, "posteriorConfidence": 0.62,
                "hits": 3, "misses": 1,
            }],
        })
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm
            .learning()
            .record_outcome(RecordOutcomeInput {
                decision_id: "d1".into(),
                outcome_type: Some("conversion".into()),
                result: "success".into(),
                reward: Some(84.0),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(out.outcome_id, "o1");
        assert_eq!(out.updates[0].posterior_confidence, 0.62);
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        assert!(req.path.ends_with("/lattice/outcomes"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["decisionId"], "d1");
        assert_eq!(b["result"], "success");
        assert_eq!(b["reward"], 84.0);
    }

    #[tokio::test]
    async fn get_outcomes_gets_lattice_route_and_unwraps() {
        let body = json!({
            "outcomes": [{
                "outcomeId": "o1", "decisionId": "d1",
                "subject": { "kind": "contact", "externalId": "sarah" },
                "decisionType": "offer", "actionType": "apply_discount",
                "outcomeType": "conversion", "result": "success", "reward": 84.0,
                "occurredAt": "2026-01-01T00:00:00Z", "realizedAt": "2026-01-02T00:00:00Z",
            }]
        })
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm
            .learning()
            .get_outcomes(GetOutcomesParams {
                subject: Some(Subject::new("contact", "sarah")),
                action_type: Some("apply_discount".into()),
                limit: Some(50),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].outcome_id, "o1");
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "GET");
        assert!(req.path.contains("/lattice/outcomes?"));
        assert!(req.path.contains("subjectKind=contact"));
        assert!(req.path.contains("subjectExternalId=sarah"));
        assert!(req.path.contains("actionType=apply_discount"));
        assert!(req.path.contains("limit=50"));
    }

    #[tokio::test]
    async fn get_effectiveness_gets_lattice_route_and_unwraps() {
        let body = json!({
            "rows": [{
                "groupKey": "apply_discount", "n": 12,
                "successRate": 0.75, "avgReward": 40.0, "confidence": 0.7,
            }]
        })
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm
            .learning()
            .get_effectiveness(GetEffectivenessParams {
                group_by: Some("action_type".into()),
                min_support: Some(5),
            })
            .await
            .unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].group_key, "apply_discount");
        assert_eq!(out[0].n, 12);
        let req = rx.recv().unwrap();
        // GET, not the stale POST.
        assert_eq!(req.method, "GET");
        assert!(req.path.contains("/lattice/effectiveness?"));
        assert!(req.path.contains("groupBy=action_type"));
        assert!(req.path.contains("minSupport=5"));
    }

    #[tokio::test]
    async fn discover_posts_lattice_route() {
        let body = json!({
            "behaviors": [{
                "label": "weekly pizza loyalists", "prevalence": 0.4, "stability": 0.82,
                "size": 20,
                "memberSubjects": [{ "kind": "contact", "externalId": "sarah" }],
                "exemplarEvidence": ["pattern: recurring_event"],
            }],
            "subjectsAnalyzed": 50,
            "generatedAt": "2026-01-01T00:00:00Z", "durationMs": 15,
        })
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm
            .behaviors()
            .discover(DiscoverParams { min_cluster_size: Some(3), ..Default::default() })
            .await
            .unwrap();
        assert_eq!(out.subjects_analyzed, 50);
        assert_eq!(out.behaviors.len(), 1);
        assert_eq!(out.behaviors[0].label, "weekly pizza loyalists");
        assert_eq!(out.behaviors[0].member_subjects[0].external_id, "sarah");
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        assert!(req.path.ends_with("/lattice/discover"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["minClusterSize"], 3);
        // Unset fields omitted.
        assert!(b.get("simThreshold").is_none());
    }

    // ── Compliance ───────────────────────────────────────────────────────

    fn memory_item_json() -> Value {
        json!({
            "id": "m1", "type": "fact", "content": "hba1c = 6.2 %",
            "importance": 5, "scope": "project", "status": "confirmed",
        })
    }

    #[tokio::test]
    async fn compliance_export_posts_memory_compliance_route() {
        let body = json!({
            "subject": { "kind": "contact", "externalId": "sarah" },
            "export": null,
            "counts": {
                "memories": 3, "patterns": 1, "observations": 0,
                "events": 2, "alertFires": 0,
            },
            "generatedAt": "2026-01-01T00:00:00Z", "durationMs": 12,
        })
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm
            .compliance()
            .export_subject(&Subject::new("contact", "sarah"))
            .await
            .unwrap();
        assert_eq!(out.subject.external_id, "sarah");
        assert_eq!(out.counts.memories, 3);
        assert_eq!(out.counts.events, 2);
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        // Corrected /memory-compliance route, not the stale /admin/memory one.
        assert!(req.path.ends_with("/memory-compliance/export"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["subject"]["externalId"], "sarah");
    }

    #[tokio::test]
    async fn compliance_hard_delete_posts_hard_delete_route() {
        let body = json!({
            "subject": { "kind": "contact", "externalId": "sarah" },
            "memoriesDeleted": 3, "patternsDeleted": 1, "observationsDeleted": 0,
            "eventsDeleted": 2, "alertFiresDeleted": 0,
            "dryRun": true, "auditEventId": null,
            "generatedAt": "2026-01-01T00:00:00Z", "durationMs": 20,
        })
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm
            .compliance()
            .hard_delete_subject(&Subject::new("contact", "sarah"), "GDPR Art. 17 case A", true)
            .await
            .unwrap();
        assert_eq!(out.memories_deleted, 3);
        assert!(out.dry_run);
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        // Corrected /hard-delete route, not the stale /erase one.
        assert!(req.path.ends_with("/memory-compliance/hard-delete"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["reason"], "GDPR Art. 17 case A");
        assert_eq!(b["dryRun"], true);
    }

    #[tokio::test]
    async fn compliance_list_audit_events_builds_query() {
        let body = json!([{
            "id": "a1", "created": "2026-01-01T00:00:00Z", "actor": "svc-key-1",
            "eventType": "read.export", "query": null, "memoryIds": null,
            "resultCount": 3, "metadata": {},
        }])
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm
            .compliance()
            .list_audit_events(ListAuditParams {
                subject: Some(Subject::new("contact", "sarah")),
                event_types: Some(vec!["read.context".into(), "read.export".into()]),
                since: Some("2026-05-01T00:00:00Z".into()),
                limit: Some(50),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].event_type, "read.export");
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "GET");
        assert!(req.path.contains("/memory-compliance/audit?"));
        assert!(req.path.contains("subjectKind=contact"));
        assert!(req.path.contains("subjectExternalId=sarah"));
        assert!(req.path.contains("since=2026-05-01T00:00:00Z"));
        assert!(req.path.contains("limit=50"));
        assert!(req.path.contains("eventTypes=read.context,read.export"));
    }

    #[tokio::test]
    async fn compliance_list_packs_gets_packs_route() {
        let body = json!([{
            "id": "hipaa", "version": "1.0.0", "description": "HIPAA de-id",
            "ownsClasses": ["phi"], "regulatoryTags": ["HIPAA"],
        }])
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm.compliance().list_packs().await.unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "hipaa");
        assert_eq!(out[0].owns_classes, vec!["phi".to_string()]);
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "GET");
        // Corrected /memory-compliance/packs route, not the stale /admin one.
        assert!(req.path.ends_with("/memory-compliance/packs"));
    }

    fn project_pack_json() -> Value {
        json!({
            "id": "pp1", "packId": "@thinkfleet/pack-healthcare", "enabled": true,
            "config": { "deidentificationMode": "safe-harbor" },
            "enabledByUserId": "u1",
            "created": "2026-01-01T00:00:00Z", "updated": "2026-01-01T00:00:00Z",
        })
    }

    #[tokio::test]
    async fn compliance_list_project_packs_gets_packs_route() {
        let body = json!([project_pack_json()]).to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm.compliance().list_project_packs().await.unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].pack_id, "@thinkfleet/pack-healthcare");
        assert!(out[0].enabled);
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "GET");
        assert!(req.path.ends_with("/memory-compliance-packs"));
    }

    #[tokio::test]
    async fn compliance_upsert_project_pack_posts_packs_route() {
        let body = project_pack_json().to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm
            .compliance()
            .upsert_project_pack(UpsertProjectPackRequest {
                pack_id: "@thinkfleet/pack-healthcare".into(),
                enabled: true,
                config: Some(json!({ "deidentificationMode": "safe-harbor" })),
            })
            .await
            .unwrap();
        assert_eq!(out.pack_id, "@thinkfleet/pack-healthcare");
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        assert!(req.path.ends_with("/memory-compliance-packs"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["packId"], "@thinkfleet/pack-healthcare");
        assert_eq!(b["enabled"], true);
        assert_eq!(b["config"]["deidentificationMode"], "safe-harbor");
    }

    #[tokio::test]
    async fn compliance_remove_project_pack_deletes_encoded_route() {
        let (base, rx) = mock_server(vec![String::new()]);
        let mm = client(&base);
        mm.compliance().remove_project_pack("@thinkfleet/pack-healthcare").await.unwrap();
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "DELETE");
        // pack id is percent-encoded into the path segment.
        assert!(req.path.ends_with("/memory-compliance-packs/%40thinkfleet%2Fpack-healthcare"));
    }

    // ── Health ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn health_record_biomarker_posts_admin_memory() {
        let (base, rx) = mock_server(vec![memory_item_json().to_string()]);
        let mm = client(&base);
        let out = mm
            .health()
            .record_biomarker(
                &Subject::new("patient", "p-123"),
                "hba1c",
                6.2,
                RecordBiomarkerOpts { unit: Some("%".into()), ..Default::default() },
            )
            .await
            .unwrap();
        assert_eq!(out.id, "m1");
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        // Signals are recorded as fact memories via /admin/memory (no /health/biomarkers).
        assert!(req.path.ends_with("/admin/memory"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["type"], "fact");
        assert_eq!(b["category"], "health");
        assert_eq!(b["source"], "sdk:health");
        assert_eq!(b["content"], "hba1c = 6.2 %");
        assert_eq!(b["metadata"]["subject"]["externalId"], "p-123");
        assert_eq!(b["metadata"]["health"]["biomarker"], "hba1c");
        assert_eq!(b["metadata"]["health"]["value"], 6.2);
        assert_eq!(b["metadata"]["health"]["unit"], "%");
    }

    #[tokio::test]
    async fn health_record_demographics_posts_admin_memory() {
        let (base, rx) = mock_server(vec![memory_item_json().to_string()]);
        let mm = client(&base);
        mm.health()
            .record_demographics(
                &Subject::new("patient", "p-123"),
                DemographicsInput {
                    age_years: Some(54.0),
                    sex: Some("female".into()),
                    weight_kg: Some(82.0),
                    height_cm: Some(170.0),
                    activity: Some("low".into()),
                },
            )
            .await
            .unwrap();
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        assert!(req.path.ends_with("/admin/memory"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["content"], "Demographics update");
        assert_eq!(b["metadata"]["demographic"]["ageYears"], 54.0);
        assert_eq!(b["metadata"]["demographic"]["sex"], "female");
        // Unset fields omitted, not null.
        assert!(b["metadata"]["demographic"].get("bmi").is_none());
    }

    #[tokio::test]
    async fn health_record_condition_posts_admin_memory() {
        let (base, rx) = mock_server(vec![memory_item_json().to_string()]);
        let mm = client(&base);
        mm.health()
            .record_condition(
                &Subject::new("patient", "p-123"),
                ConditionInput { icd10: "I10".into(), status: Some("active".into()), ..Default::default() },
            )
            .await
            .unwrap();
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        assert!(req.path.ends_with("/admin/memory"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["content"], "Diagnosis I10");
        assert_eq!(b["metadata"]["condition"]["icd10"], "I10");
        assert_eq!(b["metadata"]["condition"]["status"], "active");
        assert!(b["metadata"]["condition"].get("onsetAt").is_none());
    }

    #[tokio::test]
    async fn health_get_profile_posts_lattice_route() {
        let body = json!({
            "subject": { "kind": "patient", "externalId": "p-123" },
            "biologicalAge": {
                "biologicalAgeYears": 58.0, "chronologicalAgeYears": 54.0,
                "deltaYears": 4.0, "method": "phenoage_hybrid", "confidence": 0.8,
                "components": [{ "label": "hba1c", "yearsDelta": 2.0 }],
                "mortalityScore": 0.12,
            },
            "predictedConditions": [{
                "condition": "type2_diabetes", "label": "Type 2 Diabetes",
                "basis": "above_threshold_now", "biomarker": "hba1c",
                "currentValue": 6.2, "threshold": 6.5, "projectedOnsetAt": null,
                "confidence": 0.7, "rationale": "trending", "sourceMemoryIds": ["m1"],
            }],
            "diagnosedConditions": ["I10"],
            "latestBiomarkers": [{
                "biomarker": "hba1c", "value": 6.2, "unit": "%",
                "observedAt": "2026-01-01T00:00:00Z",
            }],
            "disclaimer": "Screening only.",
            "generatedAt": "2026-01-01T00:00:00Z",
        })
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm.health().get_profile(&Subject::new("patient", "p-123")).await.unwrap();
        assert_eq!(out.subject.external_id, "p-123");
        assert_eq!(out.biological_age.unwrap().biological_age_years, 58.0);
        assert_eq!(out.predicted_conditions[0].condition, "type2_diabetes");
        assert_eq!(out.diagnosed_conditions, vec!["I10".to_string()]);
        assert_eq!(out.latest_biomarkers[0].unit, "%");
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        // Corrected /lattice/health route, not the stale /health one.
        assert!(req.path.ends_with("/lattice/health/profile"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["subject"]["externalId"], "p-123");
    }

    #[tokio::test]
    async fn health_cohort_risk_posts_lattice_route_with_k() {
        let body = json!({
            "subject": { "kind": "patient", "externalId": "p-123" },
            "cohortSize": 25, "populationSize": 400,
            "risks": [{
                "condition": "type2_diabetes", "cohortPrevalence": 0.32,
                "cohortSize": 25, "countWith": 8, "meanSimilarity": 0.88,
                "confidence": 0.6, "rationale": "8 of 25 similar patients",
            }],
            "disclaimer": "Base rates, not a diagnosis.",
            "generatedAt": "2026-01-01T00:00:00Z",
        })
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm
            .health()
            .cohort_risk(&Subject::new("patient", "p-123"), Some(25))
            .await
            .unwrap();
        assert_eq!(out.cohort_size, 25);
        assert_eq!(out.risks[0].count_with, 8);
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        assert!(req.path.ends_with("/lattice/health/cohort-risk"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["subject"]["externalId"], "p-123");
        assert_eq!(b["k"], 25);
    }
}

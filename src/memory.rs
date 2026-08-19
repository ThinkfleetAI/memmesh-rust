//! Memory — the primary surface: ingest, recall, admin + SOTA ops.

use std::sync::Arc;

use base64::Engine as _;
use reqwest::Method;
use serde::Serialize;
use serde_json::{json, Value};

use crate::{
    BackfillEmbeddingsResult, ConsolidateResult, DedupResult, Error, Explanation,
    IngestMediaResult, Inner, MemoryFeedback, MemoryItem, MemoryStats, ObserveResponse,
    PrecedencePolicy, ProcedureStep, ReflectResult, ReviewQueueItem, SearchResult, Subject,
};

/// Largest page `GET /admin/memory` will serve — the route rejects anything
/// bigger. Keep in sync with the admin list route on the server.
const MAX_PAGE_SIZE: u32 = 500;

/// Accessor for the memory API. Get one via [`crate::MemMesh::memory`].
pub struct Memory {
    pub(crate) c: Arc<Inner>,
}

/// An event to observe. Fill the fields you need; the rest default.
///
/// `metadata` carries any structured fields the mining engine reads off the
/// event. Notably, the RFM **Monetary** score sums a numeric `amount` (or
/// `value` / `total`, or a `lineItems` array) — a price written only into
/// `content` is not parsed, so set it here:
///
/// ```no_run
/// # use memmesh::{memory::Observe, Subject};
/// # use serde_json::json;
/// Observe {
///     subject: Some(Subject::new("contact", "sarah")),
///     content: "Order — pizza".into(),
///     activity_type: Some("order_placed".into()),
///     metadata: Some(json!({ "amount": 42.0 })),
///     ..Default::default()
/// };
/// ```
#[derive(Debug, Default)]
pub struct Observe {
    /// Raw message text — the PRIMARY field. Send the whole turn verbatim; the
    /// engine runs extraction (heuristic + optional LLM) and keeps only what's
    /// worth remembering, dropping filler. Prefer this over `content`. When set,
    /// `observe` posts to `/memory/observe` and the rest of the structured
    /// fields (`subject`, `type_`, `scope`, ...) are ignored — the engine
    /// resolves them during extraction.
    pub text: Option<String>,
    /// Who said `text` — defaults to `"user"`. Only used on the `text` path.
    pub role: Option<String>,
    /// The end user this turn belongs to — your own identifier, not a MemMesh
    /// one. Recorded as provenance on whatever the engine keeps.
    ///
    /// NOT a tenancy boundary: search filters `chatIdentityId IS NULL OR = $1`,
    /// permissively by design, so project-wide memories stay visible to every
    /// caller. Isolating one end user's memories needs a project per tenant.
    pub user_id: Option<String>,
    /// The agent or assistant that produced this turn. Provenance only.
    pub agent_id: Option<String>,
    /// Conversation/thread id, so turns from one session stay linkable.
    pub session_id: Option<String>,
    pub subject: Option<Subject>,
    /// DEPRECATED — a pre-decided fact stored verbatim, bypassing extraction.
    /// Prefer `text` and let the engine decide what to keep.
    pub content: String,
    pub type_: Option<String>,
    pub scope: Option<String>,
    pub importance: Option<i64>,
    pub category: Option<String>,
    pub activity_type: Option<String>,
    pub occurred_at: Option<String>,
    /// Extra structured fields merged into the event metadata (e.g.
    /// `{ "amount": 42.0 }` for RFM monetary, `entityIds`, `lineItems`).
    pub metadata: Option<Value>,
}

/// A media item to ingest. Fill `media` + `mime_type`; the rest are optional
/// attribution recorded on the resulting memories' provenance.
#[derive(Debug, Default)]
pub struct IngestMedia {
    pub media: Vec<u8>,
    pub mime_type: String,
    pub user_id: Option<String>,
    pub agent_id: Option<String>,
    pub session_id: Option<String>,
    pub source: Option<String>,
}

/// Options for a reflection pass.
#[derive(Debug, Default)]
pub struct ReflectOpts {
    pub user_id: Option<String>,
    pub max_sources: Option<u32>,
    pub max_insights: Option<u32>,
    pub dry_run: bool,
}

/// Filters for [`Memory::list`] / [`Memory::list_all`]. All fields optional;
/// build with `..Default::default()`. `limit` maxes out at 500.
#[derive(Debug, Default, Clone)]
pub struct ListParams {
    pub type_: Option<String>,
    pub scope: Option<String>,
    pub status: Option<String>,
    pub source: Option<String>,
    pub chatbot_id: Option<String>,
    pub chat_identity_id: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

impl ListParams {
    /// Render the set fields as a `?a=b&c=d` query string (empty when none set).
    fn query(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(v) = &self.type_ {
            parts.push(format!("type={v}"));
        }
        if let Some(v) = &self.scope {
            parts.push(format!("scope={v}"));
        }
        if let Some(v) = &self.status {
            parts.push(format!("status={v}"));
        }
        if let Some(v) = &self.source {
            parts.push(format!("source={v}"));
        }
        if let Some(v) = &self.chatbot_id {
            parts.push(format!("chatbotId={v}"));
        }
        if let Some(v) = &self.chat_identity_id {
            parts.push(format!("chatIdentityId={v}"));
        }
        if let Some(v) = self.limit {
            parts.push(format!("limit={v}"));
        }
        if let Some(v) = self.offset {
            parts.push(format!("offset={v}"));
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!("?{}", parts.join("&"))
        }
    }
}

/// Fields to change on a memory item — `None` leaves a field untouched.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMemory {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub importance: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

/// An image / audio / document attachment to record as a memory. Fill `data` +
/// `mime_type`; the rest are optional. `content` is the searchable caption /
/// transcript / extracted text.
#[derive(Debug, Default)]
pub struct ObserveAttachment {
    pub subject: Option<Subject>,
    /// Raw bytes of the image, audio clip, or document.
    pub data: Vec<u8>,
    pub mime_type: String,
    pub file_name: Option<String>,
    /// Caption (image) / transcript (audio) / extracted text (document).
    pub content: Option<String>,
    pub activity_type: Option<String>,
    pub occurred_at: Option<String>,
    pub importance: Option<i64>,
    pub metadata: Option<Value>,
}

/// Options for a semantic dedup pass. Build with `..Default::default()`; unset
/// fields fall back to the server defaults.
#[derive(Debug, Default)]
pub struct DedupOpts {
    /// Cosine threshold for "same memory". Default 0.92 (server-side).
    pub threshold: Option<f64>,
    /// Max items to scan this pass. Default 1000, clamped [1, 10000].
    pub scan_limit: Option<u32>,
}

/// Options for an LLM consolidation pass. Omit `subject` for a project-wide run.
#[derive(Debug, Default)]
pub struct ConsolidateOpts {
    pub subject: Option<Subject>,
    /// How far back to scan for new activity. Default 30, max 365 (server-side).
    pub window_days: Option<u32>,
}

/// Feedback to submit on a memory item. `rating` is `positive` or `negative`.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitFeedback {
    pub memory_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_id: Option<String>,
    pub rating: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

/// A procedure to author — "how this job is done here."
#[derive(Debug, Default)]
pub struct Procedure {
    pub goal: String,
    pub when_to_use: Option<String>,
    pub steps: Vec<ProcedureStep>,
    pub failure_modes: Vec<String>,
    /// Heading used when the procedure is injected.
    pub category: Option<String>,
    pub scope: Option<String>,
    pub importance: Option<i64>,
}

/// Render a procedure into the injectable `content` string — identical to the
/// engine-side renderer, so client-authored content matches the server.
pub fn render_procedure_content(p: &Procedure) -> String {
    let mut lines = vec![format!("Goal: {}", p.goal.trim())];
    if let Some(w) = p.when_to_use.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        lines.push(format!("When: {w}"));
    }
    lines.push("Steps:".to_string());
    for (i, s) in p.steps.iter().enumerate() {
        let suffix = match s.pitfall.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
            Some(p) => format!(" (watch out: {p})"),
            None => String::new(),
        };
        lines.push(format!("{}. {}{}", i + 1, s.text.trim(), suffix));
    }
    let failures: Vec<&str> = p.failure_modes.iter().map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    if !failures.is_empty() {
        lines.push("Avoid:".to_string());
        for f in failures {
            lines.push(format!("- {f}"));
        }
    }
    lines.join("\n")
}

impl Memory {
    /// Record that something happened (the primary agent ingestion call).
    ///
    /// PRIMARY path — set [`Observe::text`]: the raw turn is POSTed to
    /// `/memory/observe`, where the engine runs the Observe pipeline (extract →
    /// dedupe → budget) and returns only what's worth keeping. Filler comes back
    /// as `saved: []` (`candidate_count: 0`) — that's success, not an error.
    ///
    /// LEGACY path — leave `text` empty and set [`Observe::content`]: the fact is
    /// stored verbatim via the admin-create route (no extraction) and wrapped as
    /// a single-item `ObserveResponse`.
    pub async fn observe(&self, o: Observe) -> Result<ObserveResponse, Error> {
        // PRIMARY path: hand the engine the raw turn and let it decide what to
        // keep. `/memory/observe` returns { saved, candidateCount } directly.
        if let Some(text) = o.text.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            let mut body = json!({
                "text": text,
                "role": o.role.clone().unwrap_or_else(|| "user".into()),
            });
            if let Some(t) = &o.occurred_at {
                body["occurredAt"] = json!(t);
            }
            // Provenance, all optional server-side. Inserted only when set, so a
            // turn without them is indistinguishable from one made by an older
            // client rather than carrying explicit nulls.
            if let Some(v) = &o.user_id {
                body["userId"] = json!(v);
            }
            if let Some(v) = &o.agent_id {
                body["agentId"] = json!(v);
            }
            if let Some(v) = &o.session_id {
                body["sessionId"] = json!(v);
            }
            return self.c.send(Method::POST, "/memory/observe", Some(&body)).await;
        }
        // LEGACY path: caller handed a pre-decided fact. Require it to be
        // non-empty — with neither `text` nor `content` there's nothing to store.
        if o.content.trim().is_empty() {
            return Err(Error::Validation {
                status: 400,
                message: "observe requires `text` (preferred) or `content`".into(),
            });
        }
        let mut md = json!({});
        if let Some(s) = &o.subject {
            md["subject"] = json!(s);
        }
        if let Some(a) = &o.activity_type {
            md["eventType"] = json!(a);
        }
        if let Some(t) = &o.occurred_at {
            md["occurredAt"] = json!(t);
        }
        // Merge caller-supplied metadata (amount, entityIds, lineItems, ...).
        // Applied last so caller keys win, matching the Go / Python / .NET / TS
        // SDKs' observe merge order.
        if let (Some(extra), Some(dst)) = (
            o.metadata.as_ref().and_then(Value::as_object),
            md.as_object_mut(),
        ) {
            for (k, v) in extra {
                dst.insert(k.clone(), v.clone());
            }
        }
        let mut body = json!({
            "content": o.content,
            "type": o.type_.unwrap_or_else(|| "event".into()),
            "scope": o.scope.unwrap_or_else(|| "project".into()),
            "importance": o.importance.unwrap_or(5),
            "source": "admin_created",
            "metadata": md,
        });
        if let Some(t) = &o.occurred_at {
            // Event time, not ingest time. `validFrom` is the field behavior
            // mining buckets day-of-week / hour-of-day on, so this is what makes
            // a backfill work: without it every historical row lands at the
            // moment of import and the mined patterns describe the import job
            // rather than the data. The metadata copy above is kept only for
            // readers that already look for it.
            body["validFrom"] = json!(t);
        }
        if let Some(cat) = o.category {
            body["category"] = json!(cat);
        }
        let item: MemoryItem = self.c.send(Method::POST, "/admin/memory", Some(&body)).await?;
        Ok(ObserveResponse { saved: vec![item], candidate_count: 1 })
    }

    /// Ingest an image / audio / document. The engine extracts text (vision,
    /// transcription, or OCR via LiteLLM) and runs it through the observe
    /// pipeline, so the result is real memories — not just a stored file.
    /// Requires multimodal to be enabled on the engine.
    pub async fn ingest_media(&self, m: IngestMedia) -> Result<IngestMediaResult, Error> {
        let mut body = json!({
            "dataBase64": base64::engine::general_purpose::STANDARD.encode(&m.media),
            "mimeType": m.mime_type,
        });
        if let Some(u) = m.user_id {
            body["userId"] = json!(u);
        }
        if let Some(a) = m.agent_id {
            body["agentId"] = json!(a);
        }
        if let Some(s) = m.session_id {
            body["sessionId"] = json!(s);
        }
        if let Some(s) = m.source {
            body["source"] = json!(s);
        }
        self.c.send(Method::POST, "/memory/media", Some(&body)).await
    }

    /// Seed a memory directly.
    pub async fn create(&self, content: &str, type_: &str) -> Result<MemoryItem, Error> {
        let body = json!({"content": content, "type": type_, "scope": "project", "importance": 5});
        self.c.send(Method::POST, "/admin/memory", Some(&body)).await
    }

    /// Seed a memory that became true at a specific point in the past.
    ///
    /// `occurred_at` (RFC3339) sets event time. Behavior mining buckets patterns
    /// by it, so any back-dated seed must go through here rather than `create`,
    /// which defaults event time to now.
    pub async fn create_at(
        &self,
        content: &str,
        type_: &str,
        occurred_at: &str,
    ) -> Result<MemoryItem, Error> {
        let body = json!({
            "content": content,
            "type": type_,
            "scope": "project",
            "importance": 5,
            "validFrom": occurred_at,
        });
        self.c.send(Method::POST, "/admin/memory", Some(&body)).await
    }

    /// Fetch a single memory by id.
    ///
    /// The point-lookup counterpart to `list`/`search`: without it a caller
    /// holding a memory id (from a pattern's `sourceMemoryIds`, an audit log, a
    /// webhook) had no way to resolve it and had to page `list` hoping the row
    /// was still on one.
    pub async fn get(&self, id: &str) -> Result<MemoryItem, Error> {
        self.c
            .send::<Value, MemoryItem>(Method::GET, &format!("/admin/memory/{id}"), None)
            .await
    }

    /// Author a procedure ("how this job is done here"). Stored as a
    /// `procedure` memory: the structured shape on `metadata` and the rendered
    /// how-to on `content`, so retrieval injects it as an explicit exemplar.
    pub async fn create_procedure(&self, p: Procedure) -> Result<MemoryItem, Error> {
        let mut metadata = json!({ "goal": p.goal, "steps": p.steps });
        if let Some(w) = &p.when_to_use {
            metadata["whenToUse"] = json!(w);
        }
        if !p.failure_modes.is_empty() {
            metadata["failureModes"] = json!(p.failure_modes);
        }
        let mut body = json!({
            "content": render_procedure_content(&p),
            "type": "procedure",
            "scope": p.scope.clone().unwrap_or_else(|| "project".into()),
            "importance": p.importance.unwrap_or(7),
            "metadata": metadata,
        });
        if let Some(cat) = &p.category {
            body["category"] = json!(cat);
        }
        self.c.send(Method::POST, "/admin/memory", Some(&body)).await
    }

    /// The adjudication queue — everything the system is unsure about. Each row
    /// carries a `review_reason` (pending / flagged / low_confidence / stale).
    pub async fn list_pending_review(
        &self,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<ReviewQueueItem>, Error> {
        let path = format!("/admin/memory/review?limit={limit}&offset={offset}");
        self.c.send::<Value, _>(Method::GET, &path, None).await
    }

    /// Get the project's memory precedence policy — which memory wins when two
    /// disagree. Falls back to the default ladder when unset.
    pub async fn get_precedence(&self) -> Result<PrecedencePolicy, Error> {
        self.c.send::<Value, _>(Method::GET, "/admin/memory/precedence", None).await
    }

    /// Save the precedence policy. Requires the Memory Steward role.
    pub async fn set_precedence(&self, policy: &PrecedencePolicy) -> Result<PrecedencePolicy, Error> {
        self.c.send(Method::PUT, "/admin/memory/precedence", Some(policy)).await
    }

    /// Hybrid semantic + keyword search.
    pub async fn search(&self, query: &str, limit: u32) -> Result<Vec<SearchResult>, Error> {
        self.search_paged(query, limit, 0).await
    }

    /// Search a specific page of results — bump `offset` to page through.
    pub async fn search_paged(
        &self,
        query: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<SearchResult>, Error> {
        let mut body = json!({"query": query, "limit": limit});
        if offset > 0 {
            body["offset"] = json!(offset);
        }
        self.c.send(Method::POST, "/admin/memory/search", Some(&body)).await
    }

    /// Delete a memory via the admin route (`DELETE /admin/memory/{id}`).
    /// Requires WRITE_MEMORY permission. To delete one of your own items, use
    /// [`delete_mine`](Self::delete_mine).
    pub async fn delete(&self, id: &str) -> Result<(), Error> {
        self.c
            .send::<Value, ()>(Method::DELETE, &format!("/admin/memory/{id}"), None)
            .await
    }

    /// Delete one of your own memory items via the user route
    /// (`DELETE /memory/{id}`) — no admin permission required.
    pub async fn delete_mine(&self, id: &str) -> Result<(), Error> {
        self.c
            .send::<Value, ()>(Method::DELETE, &format!("/memory/{id}"), None)
            .await
    }

    /// Approve / reject a review-queue item.
    pub async fn confirm(&self, id: &str, status: &str) -> Result<MemoryItem, Error> {
        let body = json!({"status": status});
        self.c
            .send(Method::POST, &format!("/admin/memory/{id}/confirm"), Some(&body))
            .await
    }

    /// Collapse near-duplicate memories (cosine ≥ threshold): keep the
    /// strongest, supersede the rest. Tune the cosine `threshold` and per-pass
    /// `scan_limit` via [`DedupOpts`]; pass `Default::default()` for the server
    /// defaults.
    pub async fn dedup(&self, opts: DedupOpts) -> Result<DedupResult, Error> {
        let mut body = json!({});
        if let Some(t) = opts.threshold {
            body["threshold"] = json!(t);
        }
        if let Some(n) = opts.scan_limit {
            body["scanLimit"] = json!(n);
        }
        self.c
            .send::<Value, DedupResult>(Method::POST, "/admin/memory/dedup", Some(&body))
            .await
    }

    /// Synthesize higher-order insight memories, each provenanced to its sources.
    pub async fn reflect(&self, o: ReflectOpts) -> Result<ReflectResult, Error> {
        let mut body = json!({"dryRun": o.dry_run});
        if let Some(u) = o.user_id {
            body["userId"] = json!(u);
        }
        if let Some(n) = o.max_sources {
            body["maxSources"] = json!(n);
        }
        if let Some(n) = o.max_insights {
            body["maxInsights"] = json!(n);
        }
        self.c.send(Method::POST, "/admin/memory/reflect", Some(&body)).await
    }

    /// Memories linked to the same graph entities as the seeds (spreading activation).
    pub async fn prefetch_related(
        &self,
        seed_memory_ids: &[String],
        limit: u32,
    ) -> Result<Vec<MemoryItem>, Error> {
        let body = json!({"seedMemoryIds": seed_memory_ids, "limit": limit});
        self.c
            .send(Method::POST, "/admin/memory/prefetch-related", Some(&body))
            .await
    }

    /// List memories in the project, newest first. `params.limit` maxes out at
    /// 500 (default 50 server-side); page by bumping `params.offset`. A page
    /// shorter than `limit` means the end. Use [`list_all`](Self::list_all) to
    /// walk everything.
    pub async fn list(&self, params: ListParams) -> Result<Vec<MemoryItem>, Error> {
        let path = format!("/admin/memory{}", params.query());
        self.c.send::<Value, _>(Method::GET, &path, None).await
    }

    /// Walk every memory matching `params`, paging under the hood via the shared
    /// [`crate::list_all`] helper, and collect them into one `Vec`. `params.offset`
    /// is ignored (the walk manages it); `limit` is clamped to 500.
    pub async fn list_all(&self, params: ListParams) -> Result<Vec<MemoryItem>, Error> {
        let limit = params.limit.unwrap_or(MAX_PAGE_SIZE).min(MAX_PAGE_SIZE);
        let base = ListParams {
            limit: Some(limit),
            offset: None,
            ..params
        };
        crate::list_all(|cursor: Option<String>| {
            let mut page_params = base.clone();
            let offset: u32 = cursor.and_then(|c| c.parse().ok()).unwrap_or(0);
            page_params.offset = Some(offset);
            async move {
                let items = self.list(page_params).await?;
                let n = items.len() as u32;
                // A short page is the last one; otherwise advance the offset.
                let next = if n < limit {
                    None
                } else {
                    Some((offset + n).to_string())
                };
                Ok(crate::SeekPage {
                    data: items,
                    next,
                    previous: None,
                })
            }
        })
        .await
    }

    /// List platform-level memories (shared across every project on the platform).
    pub async fn list_platform(
        &self,
        status: Option<&str>,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<Vec<MemoryItem>, Error> {
        let mut parts: Vec<String> = Vec::new();
        if let Some(s) = status {
            parts.push(format!("status={s}"));
        }
        if let Some(l) = limit {
            parts.push(format!("limit={l}"));
        }
        if let Some(o) = offset {
            parts.push(format!("offset={o}"));
        }
        let qs = if parts.is_empty() {
            String::new()
        } else {
            format!("?{}", parts.join("&"))
        };
        let path = format!("/admin/memory/platform{qs}");
        self.c.send::<Value, _>(Method::GET, &path, None).await
    }

    /// List the current user's own memories across all scopes.
    pub async fn mine(
        &self,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<Vec<MemoryItem>, Error> {
        let mut parts: Vec<String> = Vec::new();
        if let Some(l) = limit {
            parts.push(format!("limit={l}"));
        }
        if let Some(o) = offset {
            parts.push(format!("offset={o}"));
        }
        let qs = if parts.is_empty() {
            String::new()
        } else {
            format!("?{}", parts.join("&"))
        };
        let path = format!("/memory/mine{qs}");
        self.c.send::<Value, _>(Method::GET, &path, None).await
    }

    /// Aggregate counts for the admin dashboard (totals, by-scope, by-status,
    /// pending-review, flagged).
    pub async fn stats(&self) -> Result<MemoryStats, Error> {
        self.c.send::<Value, _>(Method::GET, "/admin/memory/stats", None).await
    }

    /// Update any memory item — only the fields set on `body` change.
    pub async fn update(&self, id: &str, body: UpdateMemory) -> Result<MemoryItem, Error> {
        self.c
            .send(Method::PATCH, &format!("/admin/memory/{id}"), Some(&body))
            .await
    }

    /// Copy a memory to another scope. The original is preserved; the new row is
    /// a confirmed sibling at `target_scope`.
    pub async fn promote(&self, id: &str, target_scope: &str) -> Result<MemoryItem, Error> {
        let body = json!({ "targetScope": target_scope });
        self.c
            .send(Method::POST, &format!("/admin/memory/{id}/promote"), Some(&body))
            .await
    }

    /// Right-to-explanation: for a `behavior_pattern` item, resolve the raw
    /// source memories that produced it (from `metadata.sourceMemoryIds`). For
    /// any other item, `source_memories` is empty. Sources that have since been
    /// deleted are skipped rather than failing the whole call.
    pub async fn explain(&self, id: &str) -> Result<Explanation, Error> {
        let memory = self.get(id).await?;
        let ids = source_memory_ids(&memory.metadata);
        let mut source_memories = Vec::with_capacity(ids.len());
        for sid in ids {
            if let Ok(m) = self.get(&sid).await {
                source_memories.push(m);
            }
        }
        Ok(Explanation { memory, source_memories })
    }

    /// Submit feedback on a memory item — `positive` reinforces, `negative`
    /// counts toward the auto-flag threshold (3 negatives → review queue).
    pub async fn submit_feedback(&self, feedback: SubmitFeedback) -> Result<(), Error> {
        self.c
            .send::<SubmitFeedback, ()>(Method::POST, "/memory/feedback", Some(&feedback))
            .await
    }

    /// List the feedback records attached to a memory item.
    pub async fn list_feedback(&self, id: &str) -> Result<Vec<MemoryFeedback>, Error> {
        self.c
            .send::<Value, _>(Method::GET, &format!("/admin/memory/{id}/feedback"), None)
            .await
    }

    /// LLM-driven deductive consolidation: distil recent activity into
    /// one-facet-per-row observations with provenance. The deductive counterpart
    /// to Lattice's inductive pattern miner.
    pub async fn consolidate(&self, o: ConsolidateOpts) -> Result<ConsolidateResult, Error> {
        let mut body = json!({});
        if let Some(s) = &o.subject {
            body["subject"] = json!(s);
        }
        if let Some(w) = o.window_days {
            body["windowDays"] = json!(w);
        }
        self.c
            .send(Method::POST, "/admin/memory/llm-consolidate", Some(&body))
            .await
    }

    /// Vectorize memory items that have no embedding yet. Idempotent; call
    /// repeatedly until `embedded` is 0 to drain a large corpus.
    pub async fn backfill_embeddings(
        &self,
        batch: Option<u32>,
    ) -> Result<BackfillEmbeddingsResult, Error> {
        let mut body = json!({});
        if let Some(b) = batch {
            body["batch"] = json!(b);
        }
        self.c
            .send(Method::POST, "/admin/memory/embeddings/backfill", Some(&body))
            .await
    }

    /// Record an image as a memory item (stored as a MEMORY_ATTACHMENT file).
    /// Pass `content` for the searchable caption.
    pub async fn observe_image(&self, a: ObserveAttachment) -> Result<MemoryItem, Error> {
        self.upload_attachment(a).await
    }

    /// Record a voice clip / audio file as a memory item. Pass `content` for the
    /// searchable transcript.
    pub async fn observe_voice(&self, a: ObserveAttachment) -> Result<MemoryItem, Error> {
        self.upload_attachment(a).await
    }

    /// Record a document (PDF, Word, Markdown, plain text, …) as a memory item.
    /// Pass `content` for the searchable text.
    pub async fn observe_document(&self, a: ObserveAttachment) -> Result<MemoryItem, Error> {
        self.upload_attachment(a).await
    }

    /// Shared attachment upload for image / voice / document — base64s the bytes
    /// and posts to `/memory/attachments`.
    async fn upload_attachment(&self, a: ObserveAttachment) -> Result<MemoryItem, Error> {
        let mut body = json!({
            "dataBase64": base64::engine::general_purpose::STANDARD.encode(&a.data),
            "mimeType": a.mime_type,
        });
        if let Some(s) = &a.subject {
            body["subject"] = json!(s);
        }
        if let Some(v) = &a.file_name {
            body["fileName"] = json!(v);
        }
        if let Some(v) = &a.content {
            body["content"] = json!(v);
        }
        if let Some(v) = &a.activity_type {
            body["activityType"] = json!(v);
        }
        if let Some(v) = &a.occurred_at {
            body["occurredAt"] = json!(v);
        }
        if let Some(v) = a.importance {
            body["importance"] = json!(v);
        }
        if let Some(v) = &a.metadata {
            body["metadata"] = v.clone();
        }
        self.c.send(Method::POST, "/memory/attachments", Some(&body)).await
    }
}

/// Pull source-memory ids off a pattern item's metadata. Accepts both the
/// camelCase (`sourceMemoryIds`, emitted by the Rust engine) and legacy
/// snake_case (`source_memory_ids`) spellings.
fn source_memory_ids(metadata: &Option<Value>) -> Vec<String> {
    let md = match metadata {
        Some(v) => v,
        None => return Vec::new(),
    };
    let raw = md
        .get("sourceMemoryIds")
        .or_else(|| md.get("source_memory_ids"));
    match raw.and_then(Value::as_array) {
        Some(arr) => arr.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MemMesh, ProcedureStep};
    use serde_json::json;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;

    #[test]
    fn renders_procedure() {
        let p = Procedure {
            goal: "Refund a charge".into(),
            when_to_use: Some("double charge".into()),
            steps: vec![
                ProcedureStep { text: "Find it".into(), pitfall: None },
                ProcedureStep { text: "Refund".into(), pitfall: Some("never twice".into()) },
            ],
            failure_modes: vec!["wrong card".into(), "  ".into()],
            ..Default::default()
        };
        let want = "Goal: Refund a charge\nWhen: double charge\nSteps:\n1. Find it\n2. Refund (watch out: never twice)\nAvoid:\n- wrong card";
        assert_eq!(render_procedure_content(&p), want);
    }

    #[test]
    fn source_memory_ids_reads_both_spellings() {
        // camelCase (Rust engine)
        let camel = Some(json!({ "sourceMemoryIds": ["a", "b"] }));
        assert_eq!(source_memory_ids(&camel), vec!["a".to_string(), "b".to_string()]);
        // snake_case (legacy)
        let snake = Some(json!({ "source_memory_ids": ["c"] }));
        assert_eq!(source_memory_ids(&snake), vec!["c".to_string()]);
        // non-strings filtered out
        let mixed = Some(json!({ "sourceMemoryIds": ["x", 1, null] }));
        assert_eq!(source_memory_ids(&mixed), vec!["x".to_string()]);
        // absent / null metadata → empty
        assert!(source_memory_ids(&None).is_empty());
        assert!(source_memory_ids(&Some(json!({}))).is_empty());
    }

    // ── Dep-free HTTP mock server ────────────────────────────────────────
    //
    // Answers `responses.len()` sequential requests, one per connection
    // (Connection: close forces reqwest to reconnect), replying with the
    // matching canned body and recording each request's method/path/body.

    #[derive(Debug)]
    struct Captured {
        method: String,
        path: String,
        body: String,
    }

    fn mock_server(responses: Vec<String>) -> (String, mpsc::Receiver<Captured>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            for body_out in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut data = Vec::new();
                let mut buf = [0u8; 4096];
                loop {
                    let n = stream.read(&mut buf).unwrap();
                    if n == 0 {
                        break;
                    }
                    data.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&data);
                    if let Some(idx) = text.find("\r\n\r\n") {
                        let header = text[..idx].to_string();
                        let content_len = header
                            .lines()
                            .find_map(|l| {
                                let low = l.to_ascii_lowercase();
                                low.strip_prefix("content-length:")
                                    .and_then(|v| v.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        let body_start = idx + 4;
                        if data.len() >= body_start + content_len {
                            let first = header.lines().next().unwrap_or("");
                            let mut it = first.split_whitespace();
                            let method = it.next().unwrap_or("").to_string();
                            let path = it.next().unwrap_or("").to_string();
                            let req_body =
                                String::from_utf8_lossy(&data[body_start..body_start + content_len])
                                    .to_string();
                            tx.send(Captured { method, path, body: req_body }).unwrap();
                            let resp = format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                body_out.len(),
                                body_out
                            );
                            stream.write_all(resp.as_bytes()).unwrap();
                            stream.flush().unwrap();
                            break;
                        }
                    }
                }
            }
        });
        (format!("http://{addr}"), rx)
    }

    fn item_json(id: &str) -> String {
        json!({
            "id": id,
            "type": "fact",
            "content": "c",
            "importance": 5,
            "scope": "project",
            "status": "confirmed",
        })
        .to_string()
    }

    fn client(base: &str) -> MemMesh {
        MemMesh::with_base_url("sk-test", "proj", base)
    }

    #[tokio::test]
    async fn list_builds_query_and_hits_admin_route() {
        let (base, rx) = mock_server(vec!["[]".into()]);
        let mm = client(&base);
        let out = mm
            .memory()
            .list(ListParams {
                scope: Some("project".into()),
                status: Some("confirmed".into()),
                limit: Some(10),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(out.is_empty());
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "GET");
        assert!(req.path.contains("/api/v1/projects/proj/admin/memory?"));
        assert!(req.path.contains("scope=project"));
        assert!(req.path.contains("status=confirmed"));
        assert!(req.path.contains("limit=10"));
    }

    #[tokio::test]
    async fn list_all_pages_by_offset() {
        // Page 1 is full (2 == limit) → continue; page 2 is short → stop.
        let (base, rx) = mock_server(vec![
            format!("[{},{}]", item_json("a"), item_json("b")),
            format!("[{}]", item_json("c")),
        ]);
        let mm = client(&base);
        let all = mm
            .memory()
            .list_all(ListParams { limit: Some(2), ..Default::default() })
            .await
            .unwrap();
        assert_eq!(all.len(), 3);
        let r1 = rx.recv().unwrap();
        let r2 = rx.recv().unwrap();
        assert!(r1.path.contains("offset=0"));
        assert!(r2.path.contains("offset=2"));
        assert!(r1.path.contains("limit=2"));
    }

    #[tokio::test]
    async fn stats_parses_typed_result() {
        let body = json!({
            "total": 5,
            "pendingReview": 2,
            "flagged": 1,
            "byScope": { "project": 5 },
            "byStatus": { "confirmed": 4, "pending": 1 },
        })
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let stats = mm.memory().stats().await.unwrap();
        assert_eq!(stats.total, 5);
        assert_eq!(stats.pending_review, 2);
        assert_eq!(stats.by_scope.get("project"), Some(&5));
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "GET");
        assert!(req.path.ends_with("/admin/memory/stats"));
    }

    #[tokio::test]
    async fn update_patches_with_camel_case_body() {
        let (base, rx) = mock_server(vec![item_json("m1")]);
        let mm = client(&base);
        let out = mm
            .memory()
            .update(
                "m1",
                UpdateMemory {
                    content: Some("new".into()),
                    type_: Some("preference".into()),
                    importance: Some(9),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(out.id, "m1");
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "PATCH");
        assert!(req.path.ends_with("/admin/memory/m1"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["content"], "new");
        assert_eq!(b["type"], "preference");
        assert_eq!(b["importance"], 9);
        // Unset fields are omitted, not sent as null.
        assert!(b.get("scope").is_none());
    }

    #[tokio::test]
    async fn promote_posts_target_scope() {
        let (base, rx) = mock_server(vec![item_json("m1")]);
        let mm = client(&base);
        mm.memory().promote("m1", "platform").await.unwrap();
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        assert!(req.path.ends_with("/admin/memory/m1/promote"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["targetScope"], "platform");
    }

    #[tokio::test]
    async fn mine_hits_user_route() {
        let (base, rx) = mock_server(vec!["[]".into()]);
        let mm = client(&base);
        mm.memory().mine(Some(20), None).await.unwrap();
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "GET");
        assert!(req.path.contains("/api/v1/projects/proj/memory/mine?"));
        assert!(req.path.contains("limit=20"));
    }

    #[tokio::test]
    async fn list_platform_hits_platform_route() {
        let (base, rx) = mock_server(vec!["[]".into()]);
        let mm = client(&base);
        mm.memory().list_platform(Some("confirmed"), Some(50), None).await.unwrap();
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "GET");
        assert!(req.path.contains("/admin/memory/platform?"));
        assert!(req.path.contains("status=confirmed"));
    }

    #[tokio::test]
    async fn submit_feedback_posts_to_feedback_route() {
        let (base, rx) = mock_server(vec![String::new()]);
        let mm = client(&base);
        mm.memory()
            .submit_feedback(SubmitFeedback {
                memory_id: "m1".into(),
                rating: "negative".into(),
                comment: Some("wrong".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        assert!(req.path.ends_with("/memory/feedback"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["memoryId"], "m1");
        assert_eq!(b["rating"], "negative");
        assert_eq!(b["comment"], "wrong");
    }

    #[tokio::test]
    async fn list_feedback_parses_records() {
        let body = json!([{
            "id": "f1",
            "memoryId": "m1",
            "rating": "positive",
        }])
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let fb = mm.memory().list_feedback("m1").await.unwrap();
        assert_eq!(fb.len(), 1);
        assert_eq!(fb[0].rating, "positive");
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "GET");
        assert!(req.path.ends_with("/admin/memory/m1/feedback"));
    }

    #[tokio::test]
    async fn consolidate_posts_subject_and_window() {
        let body = json!({
            "subjectsConsidered": 3,
            "observationsCreated": 2,
            "observationsUpdated": 1,
            "observationsSuperseded": 0,
            "durationMs": 42,
        })
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let res = mm
            .memory()
            .consolidate(ConsolidateOpts {
                subject: Some(Subject::new("contact", "sarah")),
                window_days: Some(30),
            })
            .await
            .unwrap();
        assert_eq!(res.observations_created, 2);
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        assert!(req.path.ends_with("/admin/memory/llm-consolidate"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["windowDays"], 30);
        assert_eq!(b["subject"]["externalId"], "sarah");
    }

    #[tokio::test]
    async fn backfill_embeddings_posts_batch() {
        let (base, rx) = mock_server(vec![json!({ "embedded": 7 }).to_string()]);
        let mm = client(&base);
        let res = mm.memory().backfill_embeddings(Some(500)).await.unwrap();
        assert_eq!(res.embedded, 7);
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        assert!(req.path.ends_with("/admin/memory/embeddings/backfill"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["batch"], 500);
    }

    #[tokio::test]
    async fn observe_image_uploads_attachment() {
        let (base, rx) = mock_server(vec![item_json("m1")]);
        let mm = client(&base);
        mm.memory()
            .observe_image(ObserveAttachment {
                subject: Some(Subject::new("contact", "sarah")),
                data: vec![1, 2, 3],
                mime_type: "image/png".into(),
                content: Some("receipt".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        assert!(req.path.ends_with("/memory/attachments"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["mimeType"], "image/png");
        assert_eq!(b["content"], "receipt");
        assert_eq!(b["subject"]["kind"], "contact");
        // bytes [1,2,3] base64
        assert_eq!(b["dataBase64"], "AQID");
    }

    #[tokio::test]
    async fn delete_mine_hits_user_route() {
        let (base, rx) = mock_server(vec![String::new()]);
        let mm = client(&base);
        mm.memory().delete_mine("m1").await.unwrap();
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "DELETE");
        // User route, not the admin route.
        assert!(req.path.ends_with("/memory/m1"));
        assert!(!req.path.contains("/admin/memory/m1"));
    }

    #[tokio::test]
    async fn delete_hits_admin_route() {
        let (base, rx) = mock_server(vec![String::new()]);
        let mm = client(&base);
        mm.memory().delete("m1").await.unwrap();
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "DELETE");
        assert!(req.path.ends_with("/admin/memory/m1"));
    }

    #[tokio::test]
    async fn dedup_sends_threshold_and_scan_limit() {
        let body = json!({ "scanned": 100, "groups": 4, "superseded": 7 }).to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let res = mm
            .memory()
            .dedup(DedupOpts { threshold: Some(0.95), scan_limit: Some(500) })
            .await
            .unwrap();
        assert_eq!(res.superseded, 7);
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        assert!(req.path.ends_with("/admin/memory/dedup"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["threshold"], 0.95);
        assert_eq!(b["scanLimit"], 500);
    }

    #[tokio::test]
    async fn dedup_default_omits_params() {
        let body = json!({ "scanned": 0, "groups": 0, "superseded": 0 }).to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        mm.memory().dedup(DedupOpts::default()).await.unwrap();
        let req = rx.recv().unwrap();
        let b: Value = serde_json::from_str(&req.body).unwrap();
        // Unset options are omitted, not sent as null.
        assert!(b.get("threshold").is_none());
        assert!(b.get("scanLimit").is_none());
    }

    #[tokio::test]
    async fn observe_text_posts_to_observe_route() {
        // PRIMARY path: raw text goes to the engine's Observe pipeline.
        let body = format!(r#"{{"saved":[{}],"candidateCount":3}}"#, item_json("m1"));
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let res = mm
            .memory()
            .observe(Observe {
                text: Some("Sarah prefers email over phone.".into()),
                occurred_at: Some("2024-01-02T00:00:00Z".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(res.saved.len(), 1);
        assert_eq!(res.saved[0].id, "m1");
        assert_eq!(res.candidate_count, 3);
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        assert!(req.path.ends_with("/memory/observe"));
        // Not the verbatim admin-create route.
        assert!(!req.path.contains("/admin/memory"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["text"], "Sarah prefers email over phone.");
        assert_eq!(b["role"], "user"); // defaulted
        assert_eq!(b["occurredAt"], "2024-01-02T00:00:00Z");
    }

    #[tokio::test]
    async fn observe_text_forwards_identity_provenance() {
        let body = format!(r#"{{"saved":[{}],"candidateCount":1}}"#, item_json("m1"));
        let (base, rx) = mock_server(vec![body]);
        client(&base)
            .memory()
            .observe(Observe {
                text: Some("I just moved to Denver.".into()),
                user_id: Some("user-123".into()),
                agent_id: Some("agent-9".into()),
                session_id: Some("thread-456".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        let b: Value = serde_json::from_str(&rx.recv().unwrap().body).unwrap();
        assert_eq!(b["userId"], "user-123");
        assert_eq!(b["agentId"], "agent-9");
        assert_eq!(b["sessionId"], "thread-456");
    }

    #[tokio::test]
    async fn observe_text_omits_identity_when_unset() {
        // An older call site must produce the request it always did — the
        // fields are absent, not explicit nulls.
        let body = format!(r#"{{"saved":[{}],"candidateCount":1}}"#, item_json("m1"));
        let (base, rx) = mock_server(vec![body]);
        client(&base)
            .memory()
            .observe(Observe { text: Some("hello".into()), ..Default::default() })
            .await
            .unwrap();
        let b: Value = serde_json::from_str(&rx.recv().unwrap().body).unwrap();
        assert!(b.get("userId").is_none());
        assert!(b.get("agentId").is_none());
        assert!(b.get("sessionId").is_none());
    }

    #[tokio::test]
    async fn observe_filler_returns_empty_saved() {
        // Filler: the engine kept nothing — success with saved: [], not an error.
        let body = json!({ "saved": [], "candidateCount": 0 }).to_string();
        let (base, _rx) = mock_server(vec![body]);
        let mm = client(&base);
        let res = mm
            .memory()
            .observe(Observe { text: Some("ok".into()), ..Default::default() })
            .await
            .unwrap();
        assert!(res.saved.is_empty());
        assert_eq!(res.candidate_count, 0);
    }

    #[tokio::test]
    async fn observe_content_wraps_single_item() {
        // LEGACY path: pre-decided fact stored verbatim, wrapped as one item.
        let (base, rx) = mock_server(vec![item_json("m1")]);
        let mm = client(&base);
        let res = mm
            .memory()
            .observe(Observe {
                content: "Prefers email.".into(),
                occurred_at: Some("2024-01-02T00:00:00Z".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(res.saved.len(), 1);
        assert_eq!(res.saved[0].id, "m1");
        assert_eq!(res.candidate_count, 1);
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        assert!(req.path.ends_with("/admin/memory"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["content"], "Prefers email.");
        assert_eq!(b["source"], "admin_created");
        assert_eq!(b["validFrom"], "2024-01-02T00:00:00Z");
    }

    #[tokio::test]
    async fn observe_requires_text_or_content() {
        // Neither text nor content: a clear client-side error, no request sent.
        let (base, _rx) = mock_server(vec![]);
        let mm = client(&base);
        let err = mm.memory().observe(Observe::default()).await.unwrap_err();
        assert!(matches!(err, Error::Validation { status: 400, .. }));
    }

    #[tokio::test]
    async fn explain_resolves_source_memories() {
        // First GET returns a pattern carrying sourceMemoryIds; second GET
        // resolves the single source.
        let pattern = json!({
            "id": "p1",
            "type": "behavior_pattern",
            "content": "orders pizza on fridays",
            "importance": 7,
            "scope": "project",
            "status": "confirmed",
            "metadata": { "sourceMemoryIds": ["s1"] },
        })
        .to_string();
        let (base, rx) = mock_server(vec![pattern, item_json("s1")]);
        let mm = client(&base);
        let ex = mm.memory().explain("p1").await.unwrap();
        assert_eq!(ex.memory.id, "p1");
        assert_eq!(ex.source_memories.len(), 1);
        assert_eq!(ex.source_memories[0].id, "s1");
        let r1 = rx.recv().unwrap();
        let r2 = rx.recv().unwrap();
        assert!(r1.path.ends_with("/admin/memory/p1"));
        assert!(r2.path.ends_with("/admin/memory/s1"));
    }
}

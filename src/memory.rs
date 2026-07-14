//! Memory — the primary surface: ingest, recall, admin + SOTA ops.

use std::sync::Arc;

use base64::Engine as _;
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    DedupResult, Error, IngestMediaResult, Inner, MemoryItem, ReflectResult, SearchResult, Subject,
};

/// Accessor for the memory API. Get one via [`crate::MemMesh::memory`].
pub struct Memory {
    pub(crate) c: Arc<Inner>,
}

/// An event to observe. Fill the fields you need; the rest default.
#[derive(Debug, Default)]
pub struct Observe {
    pub subject: Option<Subject>,
    pub content: String,
    pub type_: Option<String>,
    pub scope: Option<String>,
    pub importance: Option<i64>,
    pub category: Option<String>,
    pub activity_type: Option<String>,
    pub occurred_at: Option<String>,
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

impl Memory {
    /// Record that something happened (the primary agent ingestion call).
    pub async fn observe(&self, o: Observe) -> Result<MemoryItem, Error> {
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
        self.c.send(Method::POST, "/admin/memory", Some(&body)).await
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

    /// Delete a memory.
    pub async fn delete(&self, id: &str) -> Result<(), Error> {
        self.c
            .send::<Value, ()>(Method::DELETE, &format!("/admin/memory/{id}"), None)
            .await
    }

    /// Approve / reject a review-queue item.
    pub async fn confirm(&self, id: &str, status: &str) -> Result<MemoryItem, Error> {
        let body = json!({"status": status});
        self.c
            .send(Method::POST, &format!("/admin/memory/{id}/confirm"), Some(&body))
            .await
    }

    /// Collapse near-duplicate memories.
    pub async fn dedup(&self) -> Result<DedupResult, Error> {
        self.c
            .send::<Value, DedupResult>(Method::POST, "/admin/memory/dedup", Some(&json!({})))
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
}

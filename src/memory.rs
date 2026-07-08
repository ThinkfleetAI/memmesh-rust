//! Memory — the primary surface: ingest, recall, admin + SOTA ops.

use std::sync::Arc;

use reqwest::Method;
use serde_json::{json, Value};

use crate::{DedupResult, Error, Inner, MemoryItem, ReflectResult, SearchResult, Subject};

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
        if let Some(cat) = o.category {
            body["category"] = json!(cat);
        }
        self.c.send(Method::POST, "/admin/memory", Some(&body)).await
    }

    /// Seed a memory directly.
    pub async fn create(&self, content: &str, type_: &str) -> Result<MemoryItem, Error> {
        let body = json!({"content": content, "type": type_, "scope": "project", "importance": 5});
        self.c.send(Method::POST, "/admin/memory", Some(&body)).await
    }

    /// Hybrid semantic + keyword search.
    pub async fn search(&self, query: &str, limit: u32) -> Result<Vec<SearchResult>, Error> {
        let body = json!({"query": query, "limit": limit});
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

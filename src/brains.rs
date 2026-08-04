//! Brains — the marketplace registry.
//!
//! Register, version, and manage the brains a project publishes. A brain carries
//! a Brain Card manifest (ontology, provenance, coverage, eval, pricing) and a
//! stable `external_id` slug. Once a brain is `PUBLISHED` + `PUBLIC`, any caller
//! can consume it over the hosted MCP endpoint; consumption is an MCP
//! connection, not a REST call, so it lives outside this resource.
//!
//! ```no_run
//! # async fn run() -> Result<(), memmesh::Error> {
//! use memmesh::{MemMesh, CreateBrainFromProjectOptions, ListBrainsParams, UpdateBrainRequest};
//!
//! let mm = MemMesh::new("sk-...", "proj_...");
//!
//! let brain = mm.brains().create_from_project(CreateBrainFromProjectOptions {
//!     external_id: "my-support-playbook".into(),
//!     name: "Support Playbook".into(),
//!     domain: Some("support".into()),
//!     ..Default::default()
//! }).await?;
//!
//! // Publish it later, once you're ready:
//! mm.brains().update(&brain.id, UpdateBrainRequest {
//!     visibility: Some("PUBLIC".into()),
//!     status: Some("PUBLISHED".into()),
//!     ..Default::default()
//! }).await?;
//!
//! let page = mm.brains().list(ListBrainsParams { limit: Some(20), ..Default::default() }).await?;
//! for b in &page.data { println!("{} {}", b.external_id, b.status); }
//! # Ok(()) }
//! ```

use std::sync::Arc;

use reqwest::Method;
use serde_json::Value;

use crate::{
    Brain, BrainCard, CreateBrainFromProjectOptions, CreateBrainRequest, Error, Inner,
    ListBrainsParams, SeekPage, UpdateBrainRequest,
};

/// Accessor for the brains API. Get one via [`crate::MemMesh::brains`].
pub struct Brains {
    pub(crate) c: Arc<Inner>,
}

impl Brains {
    /// Register a new brain in the project's catalog.
    pub async fn create(&self, body: CreateBrainRequest) -> Result<Brain, Error> {
        self.c.send(Method::POST, "/brains", Some(&body)).await
    }

    /// Create a brain from the calling project's memory — the easy, high-level
    /// path. Builds a sensible [`CreateBrainRequest`] from just a slug + name
    /// (plus optional domain / version / visibility) and an empty-but-valid
    /// Brain Card. Coverage is computed server-side from the project's memory.
    /// The brain is created as a `DRAFT` + `PRIVATE`; publishing is a separate
    /// step via [`update`](Self::update).
    pub async fn create_from_project(
        &self,
        opts: CreateBrainFromProjectOptions,
    ) -> Result<Brain, Error> {
        let body = CreateBrainRequest {
            external_id: opts.external_id,
            name: opts.name,
            domain: opts.domain,
            version: Some(opts.version.unwrap_or_else(|| "1.0.0".into())),
            visibility: Some(opts.visibility.unwrap_or_else(|| "PRIVATE".into())),
            rights_attested: None,
            // Empty-but-valid card: an empty provenance list and empty coverage.
            // The server recomputes coverage from the project's memory; a real
            // licensed provenance source is only required to publish PUBLIC.
            card: Some(BrainCard {
                provenance: Some(Vec::new()),
                coverage: Some(Default::default()),
                ..Default::default()
            }),
        };
        self.create(body).await
    }

    /// List the project's brains (cursor-paginated). Pass the returned page's
    /// `next` back via [`ListBrainsParams::cursor`], or use
    /// [`list_all`](Self::list_all) to walk everything.
    pub async fn list(&self, params: ListBrainsParams) -> Result<SeekPage<Brain>, Error> {
        let path = format!("/brains{}", params.query());
        self.c.send::<(), _>(Method::GET, &path, None).await
    }

    /// Walk every brain in the catalog, paging under the hood via the shared
    /// [`crate::list_all`] cursor helper, and collect them into one `Vec`.
    /// `params.cursor` is ignored (the walk manages it); `limit` is the page size.
    pub async fn list_all(&self, params: ListBrainsParams) -> Result<Vec<Brain>, Error> {
        let limit = params.limit;
        crate::list_all(|cursor: Option<String>| {
            let page_params = ListBrainsParams { limit, cursor };
            async move { self.list(page_params).await }
        })
        .await
    }

    /// Fetch one brain by id.
    pub async fn get(&self, brain_id: &str) -> Result<Brain, Error> {
        self.c
            .send::<(), _>(Method::GET, &format!("/brains/{brain_id}"), None)
            .await
    }

    /// Update / version a brain (name, version, visibility, status, card, …).
    pub async fn update(&self, brain_id: &str, body: UpdateBrainRequest) -> Result<Brain, Error> {
        self.c
            .send(Method::PATCH, &format!("/brains/{brain_id}"), Some(&body))
            .await
    }

    /// Delete a brain from the catalog.
    pub async fn delete(&self, brain_id: &str) -> Result<(), Error> {
        self.c
            .send::<Value, ()>(Method::DELETE, &format!("/brains/{brain_id}"), None)
            .await
    }
}

#[cfg(test)]
mod tests {
    use crate::test_support::{client, mock_server};
    use crate::{
        CreateBrainFromProjectOptions, CreateBrainRequest, ListBrainsParams, UpdateBrainRequest,
    };
    use serde_json::{json, Value};

    fn brain_json(id: &str, external_id: &str) -> Value {
        json!({
            "id": id,
            "created": "2026-01-01T00:00:00.000Z",
            "updated": "2026-01-01T00:00:00.000Z",
            "projectId": "proj",
            "externalId": external_id,
            "name": "SEC EDGAR Financials",
            "domain": "finance",
            "brainInterface": "v1",
            "version": "2026.07.0",
            "visibility": "PRIVATE",
            "status": "DRAFT",
            "rightsAttested": false,
            "card": { "provenance": [], "coverage": {} },
        })
    }

    #[tokio::test]
    async fn create_posts_brain() {
        let (base, rx) = mock_server(vec![brain_json("b1", "sec-edgar").to_string()]);
        let mm = client(&base);
        let brain = mm
            .brains()
            .create(CreateBrainRequest {
                external_id: "sec-edgar".into(),
                name: "SEC EDGAR Financials".into(),
                domain: Some("finance".into()),
                version: Some("2026.07.0".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(brain.id, "b1");
        assert_eq!(brain.external_id, "sec-edgar");
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        assert!(req.path.ends_with("/brains"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["externalId"], "sec-edgar");
        assert_eq!(b["domain"], "finance");
        // Unset fields are omitted, not sent as null.
        assert!(b.get("card").is_none());
        assert!(b.get("rightsAttested").is_none());
    }

    #[tokio::test]
    async fn create_from_project_defaults_draft_private() {
        let (base, rx) = mock_server(vec![brain_json("b2", "my-playbook").to_string()]);
        let mm = client(&base);
        mm.brains()
            .create_from_project(CreateBrainFromProjectOptions {
                external_id: "my-playbook".into(),
                name: "Support Playbook".into(),
                domain: Some("support".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        assert!(req.path.ends_with("/brains"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["externalId"], "my-playbook");
        // Defaults filled in for the caller.
        assert_eq!(b["version"], "1.0.0");
        assert_eq!(b["visibility"], "PRIVATE");
        // Empty-but-valid card: empty provenance list + empty coverage object.
        assert_eq!(b["card"]["provenance"], json!([]));
        assert_eq!(b["card"]["coverage"], json!({}));
    }

    #[tokio::test]
    async fn get_fetches_by_id() {
        let (base, rx) = mock_server(vec![brain_json("b1", "sec-edgar").to_string()]);
        let mm = client(&base);
        let brain = mm.brains().get("b1").await.unwrap();
        assert_eq!(brain.id, "b1");
        assert_eq!(brain.brain_interface, "v1");
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "GET");
        assert!(req.path.ends_with("/brains/b1"));
    }

    #[tokio::test]
    async fn update_patches_with_camel_case_body() {
        let (base, rx) = mock_server(vec![brain_json("b1", "sec-edgar").to_string()]);
        let mm = client(&base);
        mm.brains()
            .update(
                "b1",
                UpdateBrainRequest {
                    visibility: Some("PUBLIC".into()),
                    status: Some("PUBLISHED".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "PATCH");
        assert!(req.path.ends_with("/brains/b1"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["visibility"], "PUBLIC");
        assert_eq!(b["status"], "PUBLISHED");
        // Unset fields omitted.
        assert!(b.get("name").is_none());
    }

    #[tokio::test]
    async fn delete_hits_brain_route() {
        let (base, rx) = mock_server(vec![String::new()]);
        let mm = client(&base);
        mm.brains().delete("b1").await.unwrap();
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "DELETE");
        assert!(req.path.ends_with("/brains/b1"));
    }

    #[tokio::test]
    async fn list_all_walks_cursor_pages() {
        // Page 1 carries a `next` cursor; page 2 has none → stop.
        let page1 = json!({
            "data": [brain_json("b1", "one"), brain_json("b2", "two")],
            "next": "cursor-2",
        })
        .to_string();
        let page2 = json!({
            "data": [brain_json("b3", "three")],
            "next": null,
        })
        .to_string();
        let (base, rx) = mock_server(vec![page1, page2]);
        let mm = client(&base);
        let all = mm
            .brains()
            .list_all(ListBrainsParams { limit: Some(2), ..Default::default() })
            .await
            .unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[2].external_id, "three");
        let r1 = rx.recv().unwrap();
        let r2 = rx.recv().unwrap();
        assert_eq!(r1.method, "GET");
        // First page: no cursor; second page threads the returned `next`.
        assert!(r1.path.contains("/brains?"));
        assert!(r1.path.contains("limit=2"));
        assert!(!r1.path.contains("cursor="));
        assert!(r2.path.contains("cursor=cursor-2"));
    }
}

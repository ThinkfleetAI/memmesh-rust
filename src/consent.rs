//! Subject-level consent / opt-out.
//!
//! Records consent decisions as memory items of `type='consent'` so the audit
//! log captures every change. The Rust mining engine honors opt-outs at the
//! start of every mine pass — opted-out subjects are skipped, and their behavior
//! patterns aren't generated.
//!
//! This resource has no dedicated server endpoints: it is implemented entirely
//! **client-side over the admin-memory CRUD surface** — exactly as the
//! TypeScript SDK does. An opt-out is a `consent` memory; a lookup is a filtered
//! list; superseding a prior decision is a hard delete of the old row.
//!
//! Foundations of the EU AI Act / GDPR Art. 22 compliance story:
//!   - subject-level: per-person (or per-team / per-workspace) opt-out
//!   - audit-traceable: every opt-out is a confirmed memory item
//!   - reversible: opt-in re-enables mining (without restoring prior patterns —
//!     those must be re-mined to honor the gap)

use std::sync::Arc;

use reqwest::Method;
use serde_json::{json, Value};

use crate::{ConsentStatus, ConsentSubject, Error, Inner, MemoryItem, OptInRequest, OptOutRequest};

/// Memory-item type used for consent records.
const CONSENT_TYPE: &str = "consent";

/// Accessor for the consent API. Get one via [`crate::MemMesh::consent`].
pub struct Consent {
    pub(crate) c: Arc<Inner>,
}

impl Consent {
    /// Mark a subject as opted-out. Mining and recall must honor this: the
    /// engine skips opted-out subjects at mine time.
    ///
    /// ```no_run
    /// # async fn run(mm: memmesh::MemMesh) -> Result<(), memmesh::Error> {
    /// use memmesh::{ConsentSubject, OptOutRequest};
    /// mm.consent().opt_out(OptOutRequest {
    ///     subject: ConsentSubject::new("contact", "sarah-pizza"),
    ///     reason: Some("GDPR Art. 17 request 2026-05-25".into()),
    /// }).await?;
    /// # Ok(()) }
    /// ```
    pub async fn opt_out(&self, body: OptOutRequest) -> Result<ConsentStatus, Error> {
        let OptOutRequest { subject, reason } = body;
        // Supersede any prior consent record so the audit log shows the history
        // but only one row is "active".
        self.supersede_prior_consent(&subject).await?;

        let now = now_iso();
        let content = format!("[consent] {}:{} opted out", subject.kind, subject.external_id);
        let metadata = json!({
            "subject": &subject,
            "optedOut": true,
            "optedOutAt": &now,
            "reason": &reason,
            "recordKind": "consent",
        });
        let memory = self.create_consent_memory(&content, metadata).await?;

        Ok(ConsentStatus {
            subject,
            opted_out: true,
            opted_out_at: Some(now),
            reason,
            memory_id: Some(memory.id),
        })
    }

    /// Restore consent for a subject. Mining resumes from the next pass. Prior
    /// patterns are NOT auto-restored — they must be re-mined so the gap during
    /// opt-out is honored.
    pub async fn opt_in(&self, body: OptInRequest) -> Result<ConsentStatus, Error> {
        let OptInRequest { subject } = body;
        self.supersede_prior_consent(&subject).await?;

        let content = format!("[consent] {}:{} opted in", subject.kind, subject.external_id);
        let metadata = json!({
            "subject": &subject,
            "optedOut": false,
            "optedOutAt": Value::Null,
            "reason": Value::Null,
            "recordKind": "consent",
        });
        let memory = self.create_consent_memory(&content, metadata).await?;

        Ok(ConsentStatus {
            subject,
            opted_out: false,
            opted_out_at: None,
            reason: None,
            memory_id: Some(memory.id),
        })
    }

    /// Read the current consent status for a subject. Returns `opted_out: false`
    /// (default) if no consent record exists.
    pub async fn get_status(&self, subject: ConsentSubject) -> Result<ConsentStatus, Error> {
        match self.find_active_consent(&subject).await? {
            None => Ok(ConsentStatus {
                subject,
                opted_out: false,
                opted_out_at: None,
                reason: None,
                memory_id: None,
            }),
            Some(active) => {
                let md = active.metadata.unwrap_or(Value::Null);
                Ok(ConsentStatus {
                    subject,
                    opted_out: md.get("optedOut").and_then(Value::as_bool).unwrap_or(false),
                    opted_out_at: md.get("optedOutAt").and_then(Value::as_str).map(String::from),
                    reason: md.get("reason").and_then(Value::as_str).map(String::from),
                    memory_id: Some(active.id),
                })
            }
        }
    }

    // ── private helpers ──────────────────────────────────────────────────

    /// The single active (newest) consent record for a subject, if any.
    async fn find_active_consent(
        &self,
        subject: &ConsentSubject,
    ) -> Result<Option<MemoryItem>, Error> {
        // The engine caps `limit` at 500 (querystring/limit must be <= 500);
        // 1000 hard-fails every consent lookup.
        let all: Vec<MemoryItem> = self
            .c
            .send::<(), Vec<MemoryItem>>(Method::GET, "/admin/memory?limit=500", None)
            .await?;
        let mut matches: Vec<MemoryItem> = all
            .into_iter()
            .filter(|m| m.type_ == CONSENT_TYPE && subject_matches(m, subject))
            .collect();
        // Newest first — ISO-8601 timestamps order lexicographically.
        matches.sort_by(|a, b| created_key(b).cmp(created_key(a)));
        Ok(matches.into_iter().next())
    }

    /// Hard-delete the prior active consent row, if any. The audit log keeps the
    /// historical trail; the old row is just no longer in the active set.
    async fn supersede_prior_consent(&self, subject: &ConsentSubject) -> Result<(), Error> {
        if let Some(prior) = self.find_active_consent(subject).await? {
            self.c
                .send::<Value, ()>(
                    Method::DELETE,
                    &format!("/admin/memory/{}", prior.id),
                    None,
                )
                .await?;
        }
        Ok(())
    }

    /// Create the underlying `consent` memory item via the admin-memory route.
    async fn create_consent_memory(
        &self,
        content: &str,
        metadata: Value,
    ) -> Result<MemoryItem, Error> {
        let body = json!({
            "content": content,
            "type": CONSENT_TYPE,
            "scope": "project",
            "importance": 10,
            "category": "consent",
            "metadata": metadata,
        });
        self.c.send(Method::POST, "/admin/memory", Some(&body)).await
    }
}

/// Sort key for ordering consent rows newest-first (absent `created` sorts last).
fn created_key(item: &MemoryItem) -> &str {
    item.created.as_deref().unwrap_or("")
}

/// True when a memory item's `metadata.subject` matches `subject` on kind + id.
fn subject_matches(item: &MemoryItem, subject: &ConsentSubject) -> bool {
    let s = match item.metadata.as_ref().and_then(|md| md.get("subject")) {
        Some(v) => v,
        None => return false,
    };
    s.get("kind").and_then(Value::as_str) == Some(subject.kind.as_str())
        && s.get("externalId").and_then(Value::as_str) == Some(subject.external_id.as_str())
}

/// Current UTC time as an RFC-3339 / ISO-8601 string (`YYYY-MM-DDTHH:MM:SS.mmmZ`),
/// matching TypeScript's `new Date().toISOString()`. Implemented without pulling
/// in a date crate.
fn now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let dur = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = dur.as_secs() as i64;
    let millis = dur.subsec_millis();
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let (hh, mm, ss) = (tod / 3600, (tod % 3600) / 60, tod % 60);
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}.{millis:03}Z")
}

/// Howard Hinnant's `civil_from_days`: turn a count of days since the Unix epoch
/// into a `(year, month, day)` Gregorian date.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::civil_from_days;
    use crate::test_support::{client, mock_server};
    use crate::{ConsentSubject, OptInRequest, OptOutRequest};
    use serde_json::{json, Value};

    fn consent_item(id: &str, kind: &str, external_id: &str, opted_out: bool, created: &str) -> Value {
        json!({
            "id": id,
            "type": "consent",
            "content": "[consent] record",
            "importance": 10,
            "scope": "project",
            "status": "confirmed",
            "created": created,
            "metadata": {
                "subject": { "kind": kind, "externalId": external_id },
                "optedOut": opted_out,
                "optedOutAt": if opted_out { Value::String("2026-05-25T00:00:00.000Z".into()) } else { Value::Null },
                "reason": if opted_out { Value::String("gdpr".into()) } else { Value::Null },
                "recordKind": "consent",
            },
        })
    }

    #[test]
    fn civil_from_days_known_dates() {
        // Unix epoch and a couple of reference dates.
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(18_262), (2020, 1, 1));
        assert_eq!(civil_from_days(20_089), (2025, 1, 1));
    }

    #[tokio::test]
    async fn opt_out_lists_then_creates_consent_memory() {
        // No prior consent (empty list) → then POST the new record.
        let created = consent_item("c1", "contact", "sarah", true, "2026-05-25T00:00:00.000Z");
        let (base, rx) = mock_server(vec!["[]".into(), created.to_string()]);
        let mm = client(&base);
        let status = mm
            .consent()
            .opt_out(OptOutRequest {
                subject: ConsentSubject::new("contact", "sarah"),
                reason: Some("gdpr".into()),
            })
            .await
            .unwrap();
        assert!(status.opted_out);
        assert_eq!(status.reason.as_deref(), Some("gdpr"));
        assert_eq!(status.memory_id.as_deref(), Some("c1"));

        // First: the list lookup (capped at limit=500).
        let list_req = rx.recv().unwrap();
        assert_eq!(list_req.method, "GET");
        assert!(list_req.path.contains("/admin/memory?limit=500"));

        // Then: the create, written through the admin-memory CRUD surface.
        let create_req = rx.recv().unwrap();
        assert_eq!(create_req.method, "POST");
        assert!(create_req.path.ends_with("/admin/memory"));
        let b: Value = serde_json::from_str(&create_req.body).unwrap();
        assert_eq!(b["type"], "consent");
        assert_eq!(b["importance"], 10);
        assert_eq!(b["category"], "consent");
        assert_eq!(b["metadata"]["optedOut"], true);
        assert_eq!(b["metadata"]["reason"], "gdpr");
        assert_eq!(b["metadata"]["subject"]["externalId"], "sarah");
        assert_eq!(b["metadata"]["recordKind"], "consent");
    }

    #[tokio::test]
    async fn opt_in_supersedes_prior_then_creates() {
        // A prior opted-out record exists → it is hard-deleted, then the opt-in
        // record is created.
        let prior = consent_item("old", "contact", "sarah", true, "2026-05-25T00:00:00.000Z");
        let fresh = consent_item("new", "contact", "sarah", false, "2026-06-01T00:00:00.000Z");
        let (base, rx) = mock_server(vec![
            json!([prior]).to_string(), // list → finds prior
            String::new(),              // delete prior
            fresh.to_string(),          // create opt-in
        ]);
        let mm = client(&base);
        let status = mm
            .consent()
            .opt_in(OptInRequest { subject: ConsentSubject::new("contact", "sarah") })
            .await
            .unwrap();
        assert!(!status.opted_out);
        assert!(status.opted_out_at.is_none());
        assert_eq!(status.memory_id.as_deref(), Some("new"));

        let list_req = rx.recv().unwrap();
        assert_eq!(list_req.method, "GET");
        // The prior row is hard-deleted so only one stays active.
        let del_req = rx.recv().unwrap();
        assert_eq!(del_req.method, "DELETE");
        assert!(del_req.path.ends_with("/admin/memory/old"));
        // The opt-in record is written with optedOut=false and null fields.
        let create_req = rx.recv().unwrap();
        assert_eq!(create_req.method, "POST");
        let b: Value = serde_json::from_str(&create_req.body).unwrap();
        assert_eq!(b["metadata"]["optedOut"], false);
        assert_eq!(b["metadata"]["optedOutAt"], Value::Null);
    }

    #[tokio::test]
    async fn get_status_filters_and_parses_active_record() {
        // The list carries an unrelated item and two consent rows for the
        // subject; the newest (by `created`) wins.
        let unrelated = json!({
            "id": "x", "type": "fact", "content": "c", "importance": 5,
            "scope": "project", "status": "confirmed",
            "metadata": { "subject": { "kind": "contact", "externalId": "sarah" } },
        });
        let older = consent_item("older", "contact", "sarah", false, "2026-04-01T00:00:00.000Z");
        let newer = consent_item("newer", "contact", "sarah", true, "2026-05-25T00:00:00.000Z");
        let (base, rx) = mock_server(vec![json!([unrelated, older, newer]).to_string()]);
        let mm = client(&base);
        let status = mm
            .consent()
            .get_status(ConsentSubject::new("contact", "sarah"))
            .await
            .unwrap();
        // Newest consent row wins → opted out.
        assert!(status.opted_out);
        assert_eq!(status.memory_id.as_deref(), Some("newer"));
        assert_eq!(status.opted_out_at.as_deref(), Some("2026-05-25T00:00:00.000Z"));
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "GET");
        assert!(req.path.contains("/admin/memory?limit=500"));
    }
}

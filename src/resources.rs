//! Remaining resources: events, alerts, learning, compliance, health.

use std::sync::Arc;

use reqwest::Method;
use serde_json::{json, Value};

use crate::{Error, Inner, Subject};

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
service!(Compliance);
service!(Health);

impl Events {
    /// Append an event (idempotent on projectId + dedupeKey).
    pub async fn emit(&self, event: Value) -> Result<Value, Error> {
        self.c.send(Method::POST, "/events", Some(&event)).await
    }
}

impl Alerts {
    pub async fn create(&self, rule: Value) -> Result<Value, Error> {
        self.c.send(Method::POST, "/alerts", Some(&rule)).await
    }
    pub async fn list(&self) -> Result<Value, Error> {
        self.c.send::<Value, Value>(Method::GET, "/alerts", None).await
    }
    pub async fn delete(&self, id: &str) -> Result<(), Error> {
        self.c.send::<Value, ()>(Method::DELETE, &format!("/alerts/{id}"), None).await
    }
}

impl Learning {
    pub async fn record_decision(&self, decision: Value) -> Result<Value, Error> {
        self.c.send(Method::POST, "/learning/decisions", Some(&decision)).await
    }
    pub async fn record_outcome(&self, outcome: Value) -> Result<Value, Error> {
        self.c.send(Method::POST, "/learning/outcomes", Some(&outcome)).await
    }
    pub async fn effectiveness(&self, query: Value) -> Result<Value, Error> {
        self.c.send(Method::POST, "/learning/effectiveness", Some(&query)).await
    }
}

impl Compliance {
    pub async fn export_subject(&self, subject: &Subject) -> Result<Value, Error> {
        let body = json!({"subject": subject});
        self.c.send(Method::POST, "/admin/memory/compliance/export", Some(&body)).await
    }
    pub async fn hard_delete_subject(&self, subject: &Subject, reason: &str, dry_run: bool) -> Result<Value, Error> {
        let body = json!({"subject": subject, "reason": reason, "dryRun": dry_run});
        self.c.send(Method::POST, "/admin/memory/compliance/erase", Some(&body)).await
    }
    pub async fn list_packs(&self) -> Result<Value, Error> {
        self.c.send::<Value, Value>(Method::GET, "/admin/memory/compliance/packs", None).await
    }
}

impl Health {
    pub async fn get_profile(&self, subject: &Subject) -> Result<Value, Error> {
        let body = json!({"subject": subject});
        self.c.send(Method::POST, "/health/profile", Some(&body)).await
    }
    pub async fn cohort_risk(&self, subject: &Subject) -> Result<Value, Error> {
        let body = json!({"subject": subject});
        self.c.send(Method::POST, "/health/cohort-risk", Some(&body)).await
    }
}

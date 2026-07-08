//! Lattice — behavioral intelligence: mine, forecast, profile.

use std::sync::Arc;

use reqwest::Method;
use serde_json::{json, Value};

use crate::{Error, Inner, Subject};

/// Accessor for the lattice API. Get one via [`crate::MemMesh::lattice`].
pub struct Lattice {
    pub(crate) c: Arc<Inner>,
}

impl Lattice {
    /// Forecast a target for a subject — calibrated, provenanced, abstaining
    /// when signal is thin. `target` e.g. `json!({"kind":"event_occurrence","event":"churn"})`.
    pub async fn predict(&self, subject: &Subject, target: Value) -> Result<Value, Error> {
        let body = json!({"subject": subject, "target": target});
        self.c.send(Method::POST, "/lattice/predict", Some(&body)).await
    }

    /// Run pattern extraction over recent activity.
    pub async fn mine(&self, subject: Option<&Subject>) -> Result<Value, Error> {
        let body = match subject {
            Some(s) => json!({"subject": s}),
            None => json!({}),
        };
        self.c.send(Method::POST, "/lattice/patterns/extract", Some(&body)).await
    }

    /// Behavioral snapshot (RFM, top entity, cadence, risks).
    pub async fn profile(&self, subject: &Subject) -> Result<Value, Error> {
        let body = json!({"subject": subject});
        self.c.send(Method::POST, "/lattice/profile", Some(&body)).await
    }

    /// How honest the engine's confidence is (buckets -> hit rate).
    pub async fn calibration(&self) -> Result<Value, Error> {
        self.c.send::<Value, Value>(Method::GET, "/lattice/calibration", None).await
    }
}

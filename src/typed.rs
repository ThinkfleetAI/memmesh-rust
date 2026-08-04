//! Typed attributes — structured/numeric data the engine reasons over
//! (credit scores, sensor readings, balances) instead of opaque metadata.
//!
//! Register an attribute's schema once, then ingest observations: each is
//! validated against the definition (accepted or quarantined) and accepted
//! numeric values are folded into per-subject accumulators you can read back
//! with running mean/variance/min/max/cumulative. Pair with a `memory-value`
//! alert rule (see [`crate::resources::Alerts`]) to fire on a threshold/range.
//!
//! Mirrors thinkfleet-memory-sdk/src/resources/typed.ts.
//!
//! ```no_run
//! # async fn run() -> Result<(), memmesh::Error> {
//! use memmesh::{MemMesh, RegisterAttributeRequest, TypedObservationInput, AccumulatorParams};
//!
//! let mm = MemMesh::new("sk-...", "proj_...");
//!
//! mm.typed().register_attribute(RegisterAttributeRequest {
//!     attribute_key: "credit_score".into(),
//!     data_type: memmesh::AttributeDataType::Numeric,
//!     min_valid: Some(300.0),
//!     max_valid: Some(850.0),
//!     ..Default::default()
//! }).await?;
//!
//! let report = mm.typed().ingest(vec![TypedObservationInput {
//!     attribute_key: "credit_score".into(),
//!     subject_kind: "contact".into(),
//!     subject_external_id: "sarah".into(),
//!     value_numeric: Some(650.0),
//!     observed_at: "2026-01-01T00:00:00Z".into(),
//!     ..Default::default()
//! }]).await?;
//! println!("accepted {}", report.accepted);
//!
//! let acc = mm.typed().accumulator(AccumulatorParams {
//!     subject_kind: "contact".into(),
//!     subject_external_id: "sarah".into(),
//!     attribute_key: "credit_score".into(),
//! }).await?;
//! println!("mean {:?}", acc.mean);
//! # Ok(()) }
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use reqwest::Method;
use serde::{Deserialize, Serialize};

use crate::{Error, Inner};

/// The declared type of an attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AttributeDataType {
    #[default]
    Numeric,
    Categorical,
    Temporal,
    Boolean,
}

/// Acceptance status of an ingested observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ObservationStatus {
    Accepted,
    Quarantined,
}

/// A registered attribute definition — drives input validation on ingest.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttributeDef {
    pub id: String,
    #[serde(default)]
    pub platform_id: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    pub attribute_key: String,
    pub data_type: AttributeDataType,
    #[serde(default)]
    pub unit: Option<String>,
    /// Inclusive plausibility bounds; values outside are quarantined.
    #[serde(default)]
    pub min_valid: Option<f64>,
    #[serde(default)]
    pub max_valid: Option<f64>,
    pub required: bool,
    #[serde(default)]
    pub metadata_json: Option<String>,
}

/// Input shape for registering/updating an attribute definition.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterAttributeRequest {
    pub attribute_key: String,
    pub data_type: AttributeDataType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_valid: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_valid: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// One typed measurement of an attribute for a subject at a point in time.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypedObservationInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub attribute_key: String,
    pub subject_kind: String,
    pub subject_external_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_numeric: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_bool: Option<bool>,
    /// ISO-8601 for temporal values.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_ts: Option<String>,
    /// ISO-8601 observation time.
    pub observed_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Source-trust weight in 0..1 (default 1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trust: Option<f64>,
}

/// A stored typed observation (input fields plus server-assigned metadata).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypedObservation {
    pub id: String,
    #[serde(default)]
    pub platform_id: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    pub attribute_key: String,
    pub subject_kind: String,
    pub subject_external_id: String,
    #[serde(default)]
    pub value_numeric: Option<f64>,
    #[serde(default)]
    pub value_text: Option<String>,
    #[serde(default)]
    pub value_bool: Option<bool>,
    #[serde(default)]
    pub value_ts: Option<String>,
    pub observed_at: String,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub trust: Option<f64>,
    #[serde(default)]
    pub quality_score: Option<f64>,
    #[serde(default)]
    pub status: Option<ObservationStatus>,
}

/// Outcome of a batch ingest.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IngestReport {
    pub accepted: i64,
    pub quarantined: i64,
    pub duplicates: i64,
    /// observationId -> quarantine reason.
    #[serde(default)]
    pub quarantine_reasons: HashMap<String, String>,
}

/// Result of [`TypedAttributes::enqueue`].
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueReport {
    pub enqueued: i64,
}

/// Per-(subject, attribute) running statistics.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Accumulator {
    pub subject_kind: String,
    pub subject_external_id: String,
    pub attribute_key: String,
    pub count: i64,
    pub sum: f64,
    pub sum_sq: f64,
    #[serde(default)]
    pub min_val: Option<f64>,
    #[serde(default)]
    pub max_val: Option<f64>,
    #[serde(default)]
    pub last_val: Option<f64>,
    #[serde(default)]
    pub last_observed_at: Option<String>,
    pub cumulative: f64,
    #[serde(default)]
    pub ewma: Option<f64>,
    #[serde(default)]
    pub ewma_var: Option<f64>,
    /// Derived on read.
    #[serde(default)]
    pub mean: Option<f64>,
    #[serde(default)]
    pub variance: Option<f64>,
    #[serde(default)]
    pub stddev: Option<f64>,
}

/// Query params for [`TypedAttributes::query_observations`]. All optional.
#[derive(Debug, Clone, Default)]
pub struct QueryObservationsParams {
    pub subject_kind: Option<String>,
    pub subject_external_id: Option<String>,
    pub attribute_key: Option<String>,
    /// ISO-8601 inclusive bounds on observedAt.
    pub since: Option<String>,
    pub until: Option<String>,
    pub min_value: Option<f64>,
    pub max_value: Option<f64>,
    pub status: Option<ObservationStatus>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

impl QueryObservationsParams {
    fn query(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(v) = &self.subject_kind {
            parts.push(format!("subjectKind={v}"));
        }
        if let Some(v) = &self.subject_external_id {
            parts.push(format!("subjectExternalId={v}"));
        }
        if let Some(v) = &self.attribute_key {
            parts.push(format!("attributeKey={v}"));
        }
        if let Some(v) = &self.since {
            parts.push(format!("since={v}"));
        }
        if let Some(v) = &self.until {
            parts.push(format!("until={v}"));
        }
        if let Some(v) = self.min_value {
            parts.push(format!("minValue={v}"));
        }
        if let Some(v) = self.max_value {
            parts.push(format!("maxValue={v}"));
        }
        if let Some(v) = self.status {
            let s = match v {
                ObservationStatus::Accepted => "accepted",
                ObservationStatus::Quarantined => "quarantined",
            };
            parts.push(format!("status={s}"));
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

/// Params for [`TypedAttributes::list_attributes`]. All optional.
#[derive(Debug, Clone, Default)]
pub struct ListAttributesParams {
    pub attribute_key: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

impl ListAttributesParams {
    fn query(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(v) = &self.attribute_key {
            parts.push(format!("attributeKey={v}"));
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

/// Params for [`TypedAttributes::accumulator`] — the (subject, attribute) key.
#[derive(Debug, Clone, Default)]
pub struct AccumulatorParams {
    pub subject_kind: String,
    pub subject_external_id: String,
    pub attribute_key: String,
}

impl AccumulatorParams {
    fn query(&self) -> String {
        format!(
            "?subjectKind={}&subjectExternalId={}&attributeKey={}",
            self.subject_kind, self.subject_external_id, self.attribute_key
        )
    }
}

/// The observations batch body posted to the ingest/enqueue routes.
#[derive(Serialize)]
struct ObservationsBody<'a> {
    observations: &'a [TypedObservationInput],
}

/// Accessor for the typed-attributes API. Get one via [`crate::MemMesh::typed`].
pub struct TypedAttributes {
    pub(crate) c: Arc<Inner>,
}

impl TypedAttributes {
    /// Register or update an attribute definition (type + plausibility range).
    pub async fn register_attribute(
        &self,
        body: RegisterAttributeRequest,
    ) -> Result<AttributeDef, Error> {
        self.c.send(Method::POST, "/memory-typed/attributes", Some(&body)).await
    }

    /// List registered attribute definitions for the project.
    pub async fn list_attributes(
        &self,
        params: ListAttributesParams,
    ) -> Result<Vec<AttributeDef>, Error> {
        let path = format!("/memory-typed/attributes{}", params.query());
        self.c.send::<(), _>(Method::GET, &path, None).await
    }

    /// Ingest a batch of typed observations synchronously and return the report
    /// (accepted / quarantined / duplicate counts + quarantine reasons).
    pub async fn ingest(
        &self,
        observations: Vec<TypedObservationInput>,
    ) -> Result<IngestReport, Error> {
        let body = ObservationsBody { observations: &observations };
        self.c.send(Method::POST, "/memory-typed/observations", Some(&body)).await
    }

    /// Queue a batch for asynchronous ingest (the scalable path for high volume).
    /// Returns the count accepted onto the queue; results are folded in by a
    /// background worker.
    pub async fn enqueue(
        &self,
        observations: Vec<TypedObservationInput>,
    ) -> Result<EnqueueReport, Error> {
        let body = ObservationsBody { observations: &observations };
        self.c.send(Method::POST, "/memory-typed/observations/enqueue", Some(&body)).await
    }

    /// Query raw observations by subject, attribute, time window, and value range.
    pub async fn query_observations(
        &self,
        params: QueryObservationsParams,
    ) -> Result<Vec<TypedObservation>, Error> {
        let path = format!("/memory-typed/observations{}", params.query());
        self.c.send::<(), _>(Method::GET, &path, None).await
    }

    /// Read the running statistics for a subject + attribute.
    pub async fn accumulator(&self, params: AccumulatorParams) -> Result<Accumulator, Error> {
        let path = format!("/memory-typed/accumulator{}", params.query());
        self.c.send::<(), _>(Method::GET, &path, None).await
    }
}

#[cfg(test)]
mod tests {
    use crate::test_support::{client, mock_server};
    use crate::{
        AccumulatorParams, AttributeDataType, ListAttributesParams, QueryObservationsParams,
        RegisterAttributeRequest, TypedObservationInput,
    };
    use serde_json::{json, Value};

    #[tokio::test]
    async fn register_attribute_posts_typed_route() {
        let body = json!({
            "id": "a1", "attributeKey": "credit_score", "dataType": "numeric",
            "minValid": 300, "maxValid": 850, "required": false,
        })
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm
            .typed()
            .register_attribute(RegisterAttributeRequest {
                attribute_key: "credit_score".into(),
                data_type: AttributeDataType::Numeric,
                min_valid: Some(300.0),
                max_valid: Some(850.0),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(out.attribute_key, "credit_score");
        assert_eq!(out.data_type, AttributeDataType::Numeric);
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        assert!(req.path.ends_with("/memory-typed/attributes"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["attributeKey"], "credit_score");
        assert_eq!(b["dataType"], "numeric");
        assert_eq!(b["minValid"], 300.0);
        // Unset optional omitted, not null.
        assert!(b.get("unit").is_none());
    }

    #[tokio::test]
    async fn list_attributes_gets_typed_route() {
        let body = json!([
            { "id": "a1", "attributeKey": "credit_score", "dataType": "numeric", "required": false }
        ])
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm
            .typed()
            .list_attributes(ListAttributesParams { limit: Some(10), ..Default::default() })
            .await
            .unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].attribute_key, "credit_score");
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "GET");
        assert!(req.path.contains("/memory-typed/attributes?"));
        assert!(req.path.contains("limit=10"));
    }

    #[tokio::test]
    async fn ingest_posts_observations_route() {
        let body = json!({
            "accepted": 1, "quarantined": 0, "duplicates": 0, "quarantineReasons": {}
        })
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm
            .typed()
            .ingest(vec![TypedObservationInput {
                attribute_key: "credit_score".into(),
                subject_kind: "contact".into(),
                subject_external_id: "sarah".into(),
                value_numeric: Some(650.0),
                observed_at: "2026-01-01T00:00:00Z".into(),
                ..Default::default()
            }])
            .await
            .unwrap();
        assert_eq!(out.accepted, 1);
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        assert!(req.path.ends_with("/memory-typed/observations"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["observations"][0]["attributeKey"], "credit_score");
        assert_eq!(b["observations"][0]["valueNumeric"], 650.0);
    }

    #[tokio::test]
    async fn enqueue_posts_enqueue_route() {
        let body = json!({ "enqueued": 2 }).to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm
            .typed()
            .enqueue(vec![
                TypedObservationInput {
                    attribute_key: "credit_score".into(),
                    subject_kind: "contact".into(),
                    subject_external_id: "sarah".into(),
                    value_numeric: Some(650.0),
                    observed_at: "2026-01-01T00:00:00Z".into(),
                    ..Default::default()
                },
                TypedObservationInput {
                    attribute_key: "credit_score".into(),
                    subject_kind: "contact".into(),
                    subject_external_id: "joe".into(),
                    value_numeric: Some(700.0),
                    observed_at: "2026-01-01T00:00:00Z".into(),
                    ..Default::default()
                },
            ])
            .await
            .unwrap();
        assert_eq!(out.enqueued, 2);
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        assert!(req.path.ends_with("/memory-typed/observations/enqueue"));
    }

    #[tokio::test]
    async fn query_observations_gets_route_with_filters() {
        let body = json!([
            {
                "id": "o1", "attributeKey": "credit_score",
                "subjectKind": "contact", "subjectExternalId": "sarah",
                "valueNumeric": 650, "observedAt": "2026-01-01T00:00:00Z",
                "status": "accepted",
            }
        ])
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm
            .typed()
            .query_observations(QueryObservationsParams {
                subject_kind: Some("contact".into()),
                subject_external_id: Some("sarah".into()),
                attribute_key: Some("credit_score".into()),
                limit: Some(50),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "o1");
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "GET");
        assert!(req.path.contains("/memory-typed/observations?"));
        assert!(req.path.contains("subjectKind=contact"));
        assert!(req.path.contains("subjectExternalId=sarah"));
        assert!(req.path.contains("attributeKey=credit_score"));
        assert!(req.path.contains("limit=50"));
    }

    #[tokio::test]
    async fn accumulator_gets_route() {
        let body = json!({
            "subjectKind": "contact", "subjectExternalId": "sarah",
            "attributeKey": "credit_score", "count": 1, "sum": 650.0, "sumSq": 422500.0,
            "cumulative": 650.0, "mean": 650.0,
        })
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm
            .typed()
            .accumulator(AccumulatorParams {
                subject_kind: "contact".into(),
                subject_external_id: "sarah".into(),
                attribute_key: "credit_score".into(),
            })
            .await
            .unwrap();
        assert_eq!(out.count, 1);
        assert_eq!(out.mean, Some(650.0));
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "GET");
        assert!(req.path.contains("/memory-typed/accumulator?"));
        assert!(req.path.contains("subjectKind=contact"));
        assert!(req.path.contains("attributeKey=credit_score"));
    }
}

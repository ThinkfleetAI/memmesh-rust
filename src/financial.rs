//! Financial — technical indicators, portfolio risk, and a self-calibrating
//! directional prediction loop for the memory engine's financial vertical
//! (gated behind the `@thinkfleet/pack-financial` pack).
//!
//! Financial data is just memory data: you ingest price bars, fundamentals,
//! holdings, and news as memory items, and the engine derives indicators
//! (SMA/EMA, RSI, MACD, Bollinger, volatility, drawdown, Sharpe, beta),
//! portfolio risk (VaR, weighted beta, HHI concentration, allocation), and
//! buy/sell/hold calls whose reported confidence is structural agreement times
//! the strategy's *realized* hit-rate. [`reconcile`](Financial::reconcile)
//! scores due calls against actual prices, so the engine gets more honest over
//! time, not just louder.
//!
//! Everything here is informational only — NOT investment advice.
//!
//! ```no_run
//! # async fn run() -> Result<(), memmesh::Error> {
//! use memmesh::{MemMesh, Subject};
//! use memmesh::financial::{HoldingInput, PriceInput};
//!
//! let mm = MemMesh::new("sk-...", "proj_...");
//!
//! // Backfill price history (market data — no subject).
//! mm.financial().ingest_price(PriceInput { ticker: "AAPL".into(), close: 187.0, ..Default::default() }).await?;
//!
//! // Record a portfolio position (subject-private).
//! let portfolio = Subject::new("portfolio", "acct-123");
//! mm.financial().ingest_holding(&portfolio, HoldingInput { ticker: "AAPL".into(), shares: 100.0, ..Default::default() }).await?;
//!
//! // Read indicators + risk, and generate calibrated calls.
//! let profile = mm.financial().get_profile(&portfolio).await?;
//! let calls = mm.financial().predict(&Subject::new("ticker", "AAPL"), Default::default()).await?;
//! let _ = (profile, calls);
//! # Ok(()) }
//! ```

use std::sync::Arc;

use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{Error, Inner, MemoryItem, Subject};

/// Accessor for the financial API. Get one via [`crate::MemMesh::financial`].
pub struct Financial {
    pub(crate) c: Arc<Inner>,
}

// ── Ingestion inputs ───────────────────────────────────────────────────────

/// One price bar (daily close). Emit one per trading day to build history.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceInput {
    pub ticker: String,
    pub close: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<f64>,
    /// ISO timestamp of the bar; defaults to ingestion time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub as_of: Option<String>,
}

/// Latest-wins fundamentals for a ticker.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FundamentalInput {
    pub ticker: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pe_ratio: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market_cap: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dividend_yield: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eps: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub debt_to_equity: Option<f64>,
    /// Vendor-reported beta. The engine also computes beta from price history.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub beta: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub as_of: Option<String>,
}

/// A portfolio position. Restated, not summed — the latest record wins.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HoldingInput {
    pub ticker: String,
    pub shares: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_basis: Option<f64>,
    /// "equity" | "bond" | "cash" | "crypto" | ... Defaults to "equity".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_class: Option<String>,
}

/// A news event. Tag one ticker or many; supply a sentiment in [-1, 1] if you
/// have a vendor/LLM score, otherwise the engine falls back to a lexicon.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewsInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ticker: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tickers: Option<Vec<String>>,
    pub headline: String,
    /// [-1, 1]; omit to let the engine score the headline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sentiment: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
}

// ── Read shapes — profile ──────────────────────────────────────────────────

/// Technical indicators derived from a ticker's price history.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TechnicalIndicators {
    pub ticker: String,
    pub last_close: f64,
    pub as_of: String,
    #[serde(default)]
    pub sma20: Option<f64>,
    #[serde(default)]
    pub sma50: Option<f64>,
    #[serde(default)]
    pub sma200: Option<f64>,
    #[serde(default)]
    pub ema12: Option<f64>,
    #[serde(default)]
    pub ema26: Option<f64>,
    #[serde(default)]
    pub rsi14: Option<f64>,
    #[serde(default)]
    pub macd: Option<f64>,
    #[serde(default)]
    pub macd_signal: Option<f64>,
    #[serde(default)]
    pub macd_histogram: Option<f64>,
    #[serde(default)]
    pub bollinger_upper: Option<f64>,
    #[serde(default)]
    pub bollinger_mid: Option<f64>,
    #[serde(default)]
    pub bollinger_lower: Option<f64>,
    #[serde(default)]
    pub bollinger_pct_b: Option<f64>,
    #[serde(default)]
    pub annualized_volatility: Option<f64>,
    #[serde(default)]
    pub trailing_return: Option<f64>,
    /// Negative fraction, e.g. -0.25 for a 25% peak-to-trough decline.
    #[serde(default)]
    pub max_drawdown: Option<f64>,
    #[serde(default)]
    pub sharpe: Option<f64>,
    #[serde(default)]
    pub beta: Option<f64>,
    /// "none" | "computed" (from price history) | "reported" (vendor).
    pub beta_source: String,
    pub sample_size: u64,
    #[serde(default)]
    pub source_memory_ids: Vec<String>,
}

/// The latest fundamentals snapshot for a ticker.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FundamentalSnapshot {
    pub ticker: String,
    #[serde(default)]
    pub pe_ratio: Option<f64>,
    #[serde(default)]
    pub market_cap: Option<f64>,
    #[serde(default)]
    pub dividend_yield: Option<f64>,
    #[serde(default)]
    pub eps: Option<f64>,
    #[serde(default)]
    pub debt_to_equity: Option<f64>,
    #[serde(default)]
    pub beta: Option<f64>,
    pub as_of: String,
    pub source_memory_id: String,
}

/// One priced position within a portfolio profile.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortfolioPosition {
    pub ticker: String,
    pub shares: f64,
    #[serde(default)]
    pub cost_basis: Option<f64>,
    pub last_close: f64,
    pub market_value: f64,
    /// Fraction of total portfolio value.
    pub weight: f64,
    #[serde(default)]
    pub unrealized_pnl: Option<f64>,
    pub asset_class: String,
}

/// Value + weight for one asset class in a portfolio.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetAllocation {
    pub asset_class: String,
    pub value: f64,
    pub weight: f64,
}

/// Portfolio-level risk rollup.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortfolioRisk {
    pub total_value: f64,
    #[serde(default)]
    pub weighted_beta: Option<f64>,
    #[serde(default)]
    pub weighted_annualized_volatility: Option<f64>,
    /// Parametric 1-day 95% VaR in currency units (ignores cross-asset
    /// correlation; see `var_method`).
    #[serde(rename = "valueAtRisk95_1d", default)]
    pub value_at_risk_95_1d: Option<f64>,
    /// Herfindahl index of position weights, [0, 1]. 1 = single name.
    pub concentration_hhi: f64,
    #[serde(default)]
    pub allocations: Vec<AssetAllocation>,
    pub var_method: String,
}

/// Read-only, forecast-free profile: indicators + fundamentals + (for a
/// portfolio subject) positions and risk.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinancialProfile {
    pub subject: Subject,
    #[serde(default)]
    pub indicators: Vec<TechnicalIndicators>,
    #[serde(default)]
    pub fundamentals: Vec<FundamentalSnapshot>,
    /// Empty in ticker mode (subject.kind == "ticker").
    #[serde(default)]
    pub positions: Vec<PortfolioPosition>,
    /// Absent in ticker mode or when the portfolio has no priced value.
    #[serde(default)]
    pub portfolio_risk: Option<PortfolioRisk>,
    /// Held tickers with no market data in the corpus (couldn't be priced).
    #[serde(default)]
    pub unpriced_holdings: Vec<String>,
    /// Always populated — informational only, not investment advice.
    pub disclaimer: String,
    pub generated_at: String,
}

// ── Read shapes — prediction loop ──────────────────────────────────────────

/// One directional call.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinancialSignal {
    pub ticker: String,
    pub strategy: String,
    /// "buy" | "sell" | "hold".
    pub direction: String,
    /// Blended sub-signal score in [-1, 1], bullish positive.
    pub score: f64,
    /// Raw model agreement before calibration.
    pub structural_confidence: f64,
    /// The number to trust: structural × the strategy's realized reliability.
    pub reported_confidence: f64,
    pub expected_return: f64,
    pub horizon_days: u32,
    pub basis_close: f64,
    /// ISO timestamp when the call becomes scoreable.
    pub due_at: String,
    #[serde(default)]
    pub rationale: Vec<String>,
    pub news_used: bool,
    /// Set when the call was persisted for later scoring.
    #[serde(default)]
    pub prediction_id: Option<String>,
    #[serde(default)]
    pub source_memory_ids: Vec<String>,
}

/// Result of [`Financial::predict`].
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PredictFinancialResult {
    #[serde(default)]
    pub signals: Vec<FinancialSignal>,
    pub strategy: String,
    /// Reliability multiplier applied this run.
    pub strategy_reliability: f64,
    /// How many resolved calls the multiplier was computed from (0 = untested).
    pub resolved_sample: u64,
    pub disclaimer: String,
    pub generated_at: String,
}

/// Result of [`Financial::reconcile`].
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReconcileFinancialResult {
    /// Newly resolved this pass.
    pub scored: u64,
    pub hits: u64,
    pub misses: u64,
    /// Due-or-not, not yet scoreable.
    pub still_pending: u64,
    pub generated_at: String,
}

/// One confidence band in a calibration report.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinancialCalibrationBucket {
    pub lower: f64,
    pub upper: f64,
    pub predictions: u64,
    pub hits: u64,
    pub misses: u64,
    pub realized_hit_rate: f64,
    pub has_data: bool,
}

/// Result of [`Financial::get_calibration`] — the honesty proof.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinancialCalibrationReport {
    #[serde(default)]
    pub buckets: Vec<FinancialCalibrationBucket>,
    /// "all" when unfiltered.
    pub strategy: String,
    pub strategy_reliability: f64,
    pub total_resolved: u64,
    pub generated_at: String,
}

// ── Method option bags ─────────────────────────────────────────────────────

/// Options for [`Financial::predict`]. Build with `..Default::default()`.
#[derive(Debug, Clone, Default)]
pub struct PredictOptions {
    /// Horizon in days; default 30, clamped [1, 365].
    pub horizon_days: Option<u32>,
    /// Persist each call for later scoring; default true.
    pub persist: Option<bool>,
}

/// Options for [`Financial::get_calibration`]. Build with `..Default::default()`.
#[derive(Debug, Clone, Default)]
pub struct CalibrationOptions {
    /// Number of confidence bands; default 5, clamped [1, 20].
    pub bucket_count: Option<u32>,
    /// Filter to one strategy; omit for all.
    pub strategy: Option<String>,
}

impl Financial {
    // ── Input — ingest market data + positions (stored as memory items) ──

    /// Ingest a single price bar. Market data — not subject-attributed.
    pub async fn ingest_price(&self, price: PriceInput) -> Result<MemoryItem, Error> {
        let content = match &price.as_of {
            Some(as_of) => format!("{} close {} @ {}", price.ticker, price.close, as_of),
            None => format!("{} close {}", price.ticker, price.close),
        };
        let body = json!({
            "content": content,
            "type": "fact",
            "scope": "project",
            "category": "financial",
            "source": "sdk:financial",
            "metadata": { "price": price },
        });
        self.c.send(Method::POST, "/admin/memory", Some(&body)).await
    }

    /// Ingest many price bars (e.g. a backfill). Issued sequentially; resolves
    /// once all are stored. For very large histories, batch in chunks yourself.
    pub async fn ingest_prices(&self, prices: Vec<PriceInput>) -> Result<Vec<MemoryItem>, Error> {
        let mut out = Vec::with_capacity(prices.len());
        for p in prices {
            out.push(self.ingest_price(p).await?);
        }
        Ok(out)
    }

    /// Ingest/refresh a ticker's fundamentals. Latest values win.
    pub async fn ingest_fundamentals(
        &self,
        fundamental: FundamentalInput,
    ) -> Result<MemoryItem, Error> {
        let body = json!({
            "content": format!("Fundamentals {}", fundamental.ticker),
            "type": "fact",
            "scope": "project",
            "category": "financial",
            "source": "sdk:financial",
            "metadata": { "fundamental": fundamental },
        });
        self.c.send(Method::POST, "/admin/memory", Some(&body)).await
    }

    /// Record a portfolio position. Subject-private — attributed to the owner
    /// (use a `{ kind: "portfolio", externalId }` subject). Restated, not
    /// summed: re-recording a ticker replaces the prior position.
    pub async fn ingest_holding(
        &self,
        subject: &Subject,
        holding: HoldingInput,
    ) -> Result<MemoryItem, Error> {
        let body = json!({
            "content": format!("Holding {} {}", holding.shares, holding.ticker),
            "type": "fact",
            "scope": "project",
            "category": "financial",
            "source": "sdk:financial",
            "metadata": { "subject": subject, "holding": holding },
        });
        self.c.send(Method::POST, "/admin/memory", Some(&body)).await
    }

    /// Ingest a news event. Market data — tag one or many tickers.
    pub async fn ingest_news(&self, news: NewsInput) -> Result<MemoryItem, Error> {
        let label = news
            .ticker
            .clone()
            .or_else(|| news.tickers.as_ref().map(|t| t.join(",")))
            .unwrap_or_else(|| "news".into());
        let body = json!({
            "content": format!("News [{}]: {}", label, news.headline),
            "type": "fact",
            "scope": "project",
            "category": "financial",
            "source": "sdk:financial",
            "metadata": { "newsEvent": news },
        });
        self.c.send(Method::POST, "/admin/memory", Some(&body)).await
    }

    // ── Read — indicators, risk, calibrated predictions ──

    /// Technical indicators + (for a portfolio subject) a risk rollup, derived
    /// from ingested market data and holdings. Read-only and forecast-free.
    ///
    /// `subject.kind == "ticker"` → single-name analysis (external_id is the
    /// ticker). Any other kind → portfolio mode over the subject's holdings.
    pub async fn get_profile(&self, subject: &Subject) -> Result<FinancialProfile, Error> {
        let body = json!({ "subject": subject });
        self.c.send(Method::POST, "/lattice/financial/profile", Some(&body)).await
    }

    /// Generate directional buy/sell/hold calls. Reported confidence =
    /// structural agreement × the strategy's realized reliability. By default
    /// each call is persisted so it can be scored at horizon by `reconcile`.
    pub async fn predict(
        &self,
        subject: &Subject,
        opts: PredictOptions,
    ) -> Result<PredictFinancialResult, Error> {
        let mut body = json!({ "subject": subject });
        if let Some(h) = opts.horizon_days {
            body["horizonDays"] = json!(h);
        }
        if let Some(p) = opts.persist {
            body["persist"] = json!(p);
        }
        self.c.send(Method::POST, "/lattice/financial/predict", Some(&body)).await
    }

    /// Run the feedback loop: score every persisted prediction whose horizon has
    /// elapsed against the realized close, and mark it resolved. Idempotent;
    /// safe to run on a schedule.
    pub async fn reconcile(&self) -> Result<ReconcileFinancialResult, Error> {
        let body = json!({});
        self.c.send(Method::POST, "/lattice/financial/reconcile", Some(&body)).await
    }

    /// Resolved predictions bucketed by the confidence we reported, with the
    /// realized hit-rate per band.
    pub async fn get_calibration(
        &self,
        opts: CalibrationOptions,
    ) -> Result<FinancialCalibrationReport, Error> {
        let mut body: Value = json!({});
        if let Some(n) = opts.bucket_count {
            body["bucketCount"] = json!(n);
        }
        if let Some(s) = &opts.strategy {
            body["strategy"] = json!(s);
        }
        self.c.send(Method::POST, "/lattice/financial/calibration", Some(&body)).await
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CalibrationOptions, FundamentalInput, HoldingInput, NewsInput, PredictOptions, PriceInput,
    };
    use crate::test_support::{client, mock_server};
    use crate::Subject;
    use serde_json::{json, Value};

    fn memory_item_json() -> Value {
        json!({
            "id": "m1", "type": "fact", "content": "AAPL close 187",
            "importance": 5, "scope": "project", "status": "confirmed",
        })
    }

    #[tokio::test]
    async fn ingest_price_posts_admin_memory() {
        let (base, rx) = mock_server(vec![memory_item_json().to_string()]);
        let mm = client(&base);
        let out = mm
            .financial()
            .ingest_price(PriceInput {
                ticker: "AAPL".into(),
                close: 187.0,
                as_of: Some("2026-01-02".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(out.id, "m1");
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        assert!(req.path.ends_with("/admin/memory"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["type"], "fact");
        assert_eq!(b["category"], "financial");
        assert_eq!(b["source"], "sdk:financial");
        assert_eq!(b["content"], "AAPL close 187 @ 2026-01-02");
        assert_eq!(b["metadata"]["price"]["ticker"], "AAPL");
        assert_eq!(b["metadata"]["price"]["close"], 187.0);
        assert_eq!(b["metadata"]["price"]["asOf"], "2026-01-02");
        // Unset fields omitted, not null.
        assert!(b["metadata"]["price"].get("volume").is_none());
    }

    #[tokio::test]
    async fn ingest_prices_posts_each_bar() {
        let (base, rx) = mock_server(vec![
            memory_item_json().to_string(),
            memory_item_json().to_string(),
        ]);
        let mm = client(&base);
        let out = mm
            .financial()
            .ingest_prices(vec![
                PriceInput { ticker: "AAPL".into(), close: 187.0, ..Default::default() },
                PriceInput { ticker: "AAPL".into(), close: 188.0, ..Default::default() },
            ])
            .await
            .unwrap();
        assert_eq!(out.len(), 2);
        let r1 = rx.recv().unwrap();
        let r2 = rx.recv().unwrap();
        assert!(r1.path.ends_with("/admin/memory"));
        assert!(r2.path.ends_with("/admin/memory"));
        // No as_of → content omits the "@ ..." suffix.
        let b1: Value = serde_json::from_str(&r1.body).unwrap();
        assert_eq!(b1["content"], "AAPL close 187");
    }

    #[tokio::test]
    async fn ingest_fundamentals_posts_admin_memory() {
        let (base, rx) = mock_server(vec![memory_item_json().to_string()]);
        let mm = client(&base);
        mm.financial()
            .ingest_fundamentals(FundamentalInput {
                ticker: "AAPL".into(),
                pe_ratio: Some(29.5),
                beta: Some(1.2),
                ..Default::default()
            })
            .await
            .unwrap();
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        assert!(req.path.ends_with("/admin/memory"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["content"], "Fundamentals AAPL");
        assert_eq!(b["metadata"]["fundamental"]["peRatio"], 29.5);
        assert_eq!(b["metadata"]["fundamental"]["beta"], 1.2);
        assert!(b["metadata"]["fundamental"].get("eps").is_none());
    }

    #[tokio::test]
    async fn ingest_holding_posts_admin_memory_with_subject() {
        let (base, rx) = mock_server(vec![memory_item_json().to_string()]);
        let mm = client(&base);
        mm.financial()
            .ingest_holding(
                &Subject::new("portfolio", "acct-123"),
                HoldingInput { ticker: "AAPL".into(), shares: 100.0, cost_basis: Some(150.0), ..Default::default() },
            )
            .await
            .unwrap();
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        assert!(req.path.ends_with("/admin/memory"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["content"], "Holding 100 AAPL");
        assert_eq!(b["metadata"]["subject"]["externalId"], "acct-123");
        assert_eq!(b["metadata"]["holding"]["shares"], 100.0);
        assert_eq!(b["metadata"]["holding"]["costBasis"], 150.0);
    }

    #[tokio::test]
    async fn ingest_news_labels_with_ticker() {
        let (base, rx) = mock_server(vec![memory_item_json().to_string()]);
        let mm = client(&base);
        mm.financial()
            .ingest_news(NewsInput {
                ticker: Some("AAPL".into()),
                headline: "Apple beats earnings".into(),
                sentiment: Some(0.7),
                ..Default::default()
            })
            .await
            .unwrap();
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        assert!(req.path.ends_with("/admin/memory"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["content"], "News [AAPL]: Apple beats earnings");
        assert_eq!(b["metadata"]["newsEvent"]["sentiment"], 0.7);
    }

    #[tokio::test]
    async fn ingest_news_falls_back_to_joined_tickers() {
        let (base, rx) = mock_server(vec![memory_item_json().to_string()]);
        let mm = client(&base);
        mm.financial()
            .ingest_news(NewsInput {
                tickers: Some(vec!["AAPL".into(), "MSFT".into()]),
                headline: "Big tech rallies".into(),
                ..Default::default()
            })
            .await
            .unwrap();
        let req = rx.recv().unwrap();
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["content"], "News [AAPL,MSFT]: Big tech rallies");
    }

    #[tokio::test]
    async fn get_profile_posts_lattice_route() {
        let body = json!({
            "subject": { "kind": "ticker", "externalId": "AAPL" },
            "indicators": [{
                "ticker": "AAPL", "lastClose": 187.0, "asOf": "2026-01-02",
                "rsi14": 61.0, "betaSource": "computed", "sampleSize": 200,
                "sourceMemoryIds": ["m1"],
            }],
            "fundamentals": [],
            "positions": [],
            "portfolioRisk": null,
            "unpricedHoldings": [],
            "disclaimer": "Not investment advice.",
            "generatedAt": "2026-01-02T00:00:00Z",
        })
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm.financial().get_profile(&Subject::new("ticker", "AAPL")).await.unwrap();
        assert_eq!(out.subject.external_id, "AAPL");
        assert_eq!(out.indicators[0].rsi14, Some(61.0));
        assert_eq!(out.indicators[0].beta_source, "computed");
        assert_eq!(out.indicators[0].sample_size, 200);
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        assert!(req.path.ends_with("/lattice/financial/profile"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["subject"]["externalId"], "AAPL");
    }

    #[tokio::test]
    async fn get_profile_parses_portfolio_risk_var_field() {
        let body = json!({
            "subject": { "kind": "portfolio", "externalId": "acct-1" },
            "indicators": [],
            "fundamentals": [],
            "positions": [{
                "ticker": "AAPL", "shares": 100.0, "costBasis": 150.0,
                "lastClose": 187.0, "marketValue": 18700.0, "weight": 1.0,
                "unrealizedPnl": 3700.0, "assetClass": "equity",
            }],
            "portfolioRisk": {
                "totalValue": 18700.0, "weightedBeta": 1.2,
                "weightedAnnualizedVolatility": 0.25,
                "valueAtRisk95_1d": 480.0, "concentrationHhi": 1.0,
                "allocations": [{ "assetClass": "equity", "value": 18700.0, "weight": 1.0 }],
                "varMethod": "parametric",
            },
            "unpricedHoldings": [],
            "disclaimer": "Not investment advice.",
            "generatedAt": "2026-01-02T00:00:00Z",
        })
        .to_string();
        let (base, _rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm.financial().get_profile(&Subject::new("portfolio", "acct-1")).await.unwrap();
        let risk = out.portfolio_risk.unwrap();
        assert_eq!(risk.value_at_risk_95_1d, Some(480.0));
        assert_eq!(risk.concentration_hhi, 1.0);
        assert_eq!(out.positions[0].market_value, 18700.0);
    }

    #[tokio::test]
    async fn predict_posts_lattice_route_with_opts() {
        let body = json!({
            "signals": [{
                "ticker": "AAPL", "strategy": "trend_momentum", "direction": "buy",
                "score": 0.4, "structuralConfidence": 0.6, "reportedConfidence": 0.55,
                "expectedReturn": 0.03, "horizonDays": 30, "basisClose": 187.0,
                "dueAt": "2026-02-01T00:00:00Z", "rationale": ["RSI rising"],
                "newsUsed": true, "predictionId": "pred-1", "sourceMemoryIds": ["m1"],
            }],
            "strategy": "trend_momentum",
            "strategyReliability": 0.9, "resolvedSample": 12,
            "disclaimer": "Not investment advice.",
            "generatedAt": "2026-01-02T00:00:00Z",
        })
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm
            .financial()
            .predict(
                &Subject::new("ticker", "AAPL"),
                PredictOptions { horizon_days: Some(30), persist: Some(false) },
            )
            .await
            .unwrap();
        assert_eq!(out.signals[0].direction, "buy");
        assert_eq!(out.signals[0].prediction_id, Some("pred-1".to_string()));
        assert_eq!(out.strategy_reliability, 0.9);
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        assert!(req.path.ends_with("/lattice/financial/predict"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["subject"]["externalId"], "AAPL");
        assert_eq!(b["horizonDays"], 30);
        assert_eq!(b["persist"], false);
    }

    #[tokio::test]
    async fn predict_omits_unset_opts() {
        let body = json!({
            "signals": [], "strategy": "trend_momentum",
            "strategyReliability": 1.0, "resolvedSample": 0,
            "disclaimer": "Not investment advice.",
            "generatedAt": "2026-01-02T00:00:00Z",
        })
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        mm.financial().predict(&Subject::new("ticker", "AAPL"), PredictOptions::default()).await.unwrap();
        let req = rx.recv().unwrap();
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert!(b.get("horizonDays").is_none());
        assert!(b.get("persist").is_none());
    }

    #[tokio::test]
    async fn reconcile_posts_lattice_route() {
        let body = json!({
            "scored": 5, "hits": 3, "misses": 2, "stillPending": 4,
            "generatedAt": "2026-01-02T00:00:00Z",
        })
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm.financial().reconcile().await.unwrap();
        assert_eq!(out.scored, 5);
        assert_eq!(out.hits, 3);
        assert_eq!(out.still_pending, 4);
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        assert!(req.path.ends_with("/lattice/financial/reconcile"));
        // Empty object body.
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b, json!({}));
    }

    #[tokio::test]
    async fn get_calibration_posts_lattice_route_with_opts() {
        let body = json!({
            "buckets": [{
                "lower": 0.6, "upper": 0.8, "predictions": 10, "hits": 7,
                "misses": 3, "realizedHitRate": 0.7, "hasData": true,
            }],
            "strategy": "trend_momentum", "strategyReliability": 0.9,
            "totalResolved": 10, "generatedAt": "2026-01-02T00:00:00Z",
        })
        .to_string();
        let (base, rx) = mock_server(vec![body]);
        let mm = client(&base);
        let out = mm
            .financial()
            .get_calibration(CalibrationOptions {
                bucket_count: Some(5),
                strategy: Some("trend_momentum".into()),
            })
            .await
            .unwrap();
        assert_eq!(out.buckets[0].realized_hit_rate, 0.7);
        assert_eq!(out.total_resolved, 10);
        let req = rx.recv().unwrap();
        assert_eq!(req.method, "POST");
        assert!(req.path.ends_with("/lattice/financial/calibration"));
        let b: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(b["bucketCount"], 5);
        assert_eq!(b["strategy"], "trend_momentum");
    }
}

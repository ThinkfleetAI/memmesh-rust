# memmesh (Rust)

Official **Rust** SDK for **[MemMesh](https://memmesh.ai)** — memory + prediction
for AI agents. Async (`reqwest`/`tokio`), semantic recall, a bi-temporal
knowledge graph, belief revision, reflection, and calibrated forecasting.

Add it to your `Cargo.toml`:

```toml
[dependencies]
memmesh = "0.2"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

or:

```bash
cargo add memmesh
```

## Quickstart

```rust
use memmesh::{MemMesh, Subject, Observe, ReflectOpts};

#[tokio::main]
async fn main() -> Result<(), memmesh::Error> {
    let mm = MemMesh::new("sk-...", "proj_...");

    // Remember something — hand the engine the raw turn and let it extract
    // what's worth keeping (returns { saved, candidate_count }).
    mm.memory().observe(Observe {
        text: Some("Sarah prefers email over phone.".into()),
        ..Default::default()
    }).await?;

    // Recall it, semantically
    for hit in mm.memory().search("how to reach sarah", 5).await? {
        println!("{}", hit.content);
    }

    // Synthesize higher-order insights, with provenance
    let res = mm.memory().reflect(ReflectOpts { max_insights: Some(3), ..Default::default() }).await?;
    for i in res.insights { println!("{} ({:.0}%)", i.content, i.confidence * 100.0); }

    // Point-in-time knowledge graph
    use memmesh::context::GraphQuery;
    let edges = mm.context().query_graph(GraphQuery {
        as_of: Some("2026-03-01T00:00:00Z".into()), ..Default::default()
    }).await?;
    let _ = edges;
    Ok(())
}
```

## Surface

`mm.memory()` (observe/create/search/delete/confirm/**reflect**/**prefetch_related**/dedup) ·
`mm.lattice()` (extract_patterns/mine_memories/get_pattern/list_patterns/get_context/run_monitor_tick/get_monitor_status/predict/predict_target/get_profile/get_cohort/predict_by_cohort/estimate/get_calibration) ·
`mm.context()` (build/**build_for**/batch_build/query_graph) ·
`mm.events()` · `mm.alerts()` · `mm.learning()` (record_decision/record_outcome/get_outcomes/get_effectiveness) · `mm.behaviors()` (discover) · `mm.compliance()` · `mm.health()`.

Errors are [`memmesh::Error`] (`Http`, `Decode`, `Api { status, body }`).

Apache-2.0 · [memmesh.ai](https://memmesh.ai) · [docs](https://docs.memmesh.ai)

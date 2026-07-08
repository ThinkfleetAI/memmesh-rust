# memmesh (Rust)

Official **Rust** SDK for **[MemMesh](https://memmesh.ai)** — memory + prediction
for AI agents. Async (`reqwest`/`tokio`), semantic recall, a bi-temporal
knowledge graph, belief revision, reflection, and calibrated forecasting.

```bash
cargo add memmesh
```

## Quickstart

```rust
use memmesh::{MemMesh, Subject, Observe, ReflectOpts};

#[tokio::main]
async fn main() -> Result<(), memmesh::Error> {
    let mm = MemMesh::new("sk-...", "proj_...");

    // Remember something
    mm.memory().observe(Observe {
        subject: Some(Subject::new("contact", "sarah")),
        content: "Prefers email over phone.".into(),
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
`mm.lattice()` (predict/mine/profile/calibration) ·
`mm.context()` (build/**batch_build**/**query_graph**) ·
`mm.events()` · `mm.alerts()` · `mm.learning()` · `mm.compliance()` · `mm.health()`.

Errors are [`memmesh::Error`] (`Http`, `Decode`, `Api { status, body }`).

Apache-2.0 · [memmesh.ai](https://memmesh.ai) · [docs](https://docs.memmesh.ai)

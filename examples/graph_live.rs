//! Live check of the graph surface. Read-only — no observe() call, so it
//! never writes into the project's corpus.
//!
//!   cargo run --example graph_live -- <api-key>
use memmesh::{graph::ListEntities, graph::Traverse, MemMesh};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = std::env::args().nth(1).expect("usage: graph_live <api-key>");
    let mm = MemMesh::new(key, "YIPKT3NV8RR3UxFfq0PI5");

    let st = mm.graph().stats().await?;
    println!("stats: entities={} edges={} withEdges={}", st.entity_count, st.edge_count, st.memories_with_edges);
    println!("  extraction: {:?}", st.extraction);

    let ents = mm.graph().list_entities(ListEntities { limit: Some(3), ..Default::default() }).await?;
    println!("entities({}): {:?}", ents.len(), ents.iter().map(|e| &e.canonical_name).collect::<Vec<_>>());

    let edges = mm.graph().list_edges(None, Some(3)).await?;
    println!("edges({}):", edges.len());
    for e in &edges {
        let obj = e
            .object
            .as_ref()
            .map(|o| o.canonical_name.clone())
            .or_else(|| e.object_literal.clone())
            .unwrap_or_default();
        println!("  hop={} {} -[{}]-> {} (w={})", e.hop, e.subject.canonical_name, e.predicate, obj, e.weight);
    }

    if let Some(first) = ents.first() {
        let hood = mm.graph().get_entity(&first.id, None).await?;
        println!("get_entity: {} -> {} edges", hood.entity.map(|e| e.canonical_name).unwrap_or_default(), hood.edges.len());
        let walk = mm.graph().traverse(&first.id, Traverse { hops: Some(2), ..Default::default() }).await?;
        println!("traverse(2 hops): {} edges", walk.len());
    }
    println!("OK — all five graph methods work live");
    Ok(())
}

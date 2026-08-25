//! Pipeline graph repository — nodes and edges as edge tables, DAG traversal
//! by recursive CTE (ADR-2). A published version is immutable (INV-DATA-3):
//! this module inserts whole graphs and reads them; it never updates one.

use anyhow::Context;
use sqlx::SqlitePool;
use surge_domain::pipeline::{Edge, EdgeTrigger, Node, NodeConfig, Pipeline};

/// Insert a published pipeline version with its whole graph in one
/// transaction (INV-DATA-8: no half-landed graphs).
pub async fn insert_graph(
    pool: &SqlitePool,
    pipeline: &Pipeline,
    nodes: &[Node],
    edges: &[Edge],
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    let blessed = pipeline.blessed as i64;
    sqlx::query!(
        "INSERT INTO pipeline (id, name, version, content_hash, blessed, forked_from, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        pipeline.id,
        pipeline.name,
        pipeline.version,
        pipeline.content_hash,
        blessed,
        pipeline.forked_from,
        pipeline.created_at
    )
    .execute(&mut *tx)
    .await?;
    for n in nodes {
        let kind = n.config.kind();
        let config = serde_json::to_string(&n.config)?;
        let human_gate = n.human_gate as i64;
        let emits_span = n.emits_span as i64;
        sqlx::query!(
            "INSERT INTO node (id, pipeline_id, label, x, y, human_gate, emits_span,
                               metric_binding, metric_note, kind, config)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            n.id,
            n.pipeline_id,
            n.label,
            n.x,
            n.y,
            human_gate,
            emits_span,
            n.metric_binding,
            n.metric_note,
            kind,
            config
        )
        .execute(&mut *tx)
        .await?;
    }
    for e in edges {
        let trigger = e.trigger.as_str();
        let gate_required = e.gate_required as i64;
        sqlx::query!(
            "INSERT INTO edge (id, pipeline_id, from_node, to_node, trigger, gate_required)
             VALUES (?, ?, ?, ?, ?, ?)",
            e.id,
            e.pipeline_id,
            e.from_node,
            e.to_node,
            trigger,
            gate_required
        )
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn exists(pool: &SqlitePool, id: &str) -> anyhow::Result<bool> {
    let row = sqlx::query!("SELECT COUNT(*) AS n FROM pipeline WHERE id = ?", id)
        .fetch_one(pool)
        .await?;
    Ok(row.n > 0)
}

/// Load one published pipeline version and its whole graph.
pub async fn load_graph(
    pool: &SqlitePool,
    pipeline_id: &str,
) -> anyhow::Result<(Pipeline, Vec<Node>, Vec<Edge>)> {
    let p = sqlx::query!(
        "SELECT id, name, version, content_hash, blessed, forked_from, created_at
         FROM pipeline WHERE id = ?",
        pipeline_id
    )
    .fetch_one(pool)
    .await
    .context("pipeline not found")?;
    let pipeline = Pipeline {
        id: p.id,
        name: p.name,
        version: p.version,
        content_hash: p.content_hash,
        blessed: p.blessed != 0,
        forked_from: p.forked_from,
        created_at: p.created_at,
    };

    let nodes = sqlx::query!(
        "SELECT id, pipeline_id, label, x, y, human_gate, emits_span,
                metric_binding, metric_note, config
         FROM node WHERE pipeline_id = ? ORDER BY id",
        pipeline_id
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|r| {
        let config: NodeConfig = serde_json::from_str(&r.config)?;
        Ok(Node {
            id: r.id,
            pipeline_id: r.pipeline_id,
            label: r.label,
            x: r.x,
            y: r.y,
            human_gate: r.human_gate != 0,
            emits_span: r.emits_span != 0,
            metric_binding: r.metric_binding,
            metric_note: r.metric_note,
            config,
        })
    })
    .collect::<anyhow::Result<Vec<_>>>()?;

    let edges = sqlx::query!(
        "SELECT id, pipeline_id, from_node, to_node, trigger, gate_required
         FROM edge WHERE pipeline_id = ? ORDER BY id",
        pipeline_id
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|r| Edge {
        id: r.id,
        pipeline_id: r.pipeline_id,
        from_node: r.from_node,
        to_node: r.to_node,
        trigger: EdgeTrigger::parse(&r.trigger),
        gate_required: r.gate_required != 0,
    })
    .collect();

    Ok((pipeline, nodes, edges))
}

/// Node ids reachable from `from_node` by following edges forward — the DAG
/// traversal the dispatcher and compiler build on. Recursive CTE (ADR-2).
pub async fn reachable_nodes(
    pool: &SqlitePool,
    pipeline_id: &str,
    from_node: &str,
) -> anyhow::Result<Vec<String>> {
    let rows = sqlx::query!(
        r#"WITH RECURSIVE reach(node_id) AS (
               SELECT ?2
               UNION
               SELECT e.to_node FROM edge e
               JOIN reach r ON e.from_node = r.node_id
               WHERE e.pipeline_id = ?1
           )
           SELECT node_id AS "node_id!: String" FROM reach ORDER BY node_id"#,
        pipeline_id,
        from_node
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| r.node_id).collect())
}

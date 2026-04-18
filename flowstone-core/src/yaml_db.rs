//! Populate cozo from parsed YAML nodes + schemas.
//!
//! Two classes of relation:
//!
//! - A single meta relation `yaml_nodes { path => schema, kind, attrs_json }`
//!   stores every node from every schema.
//! - Per-(schema,edge-kind) relations of the form `<prefix>_<edge>` where
//!   `<prefix>` comes from the schema filename with `.schema` stripped and
//!   non-alphanumerics replaced with `_`. E.g. `infra.schema` + `reads`
//!   → `infra_reads { source: String, target: String }`.
//!
//! This keeps schemas disjoint at the relation level and matches how a
//! user would write queries: `?[s, t] := *infra_reads[s, t]`.

use std::collections::BTreeMap;

use cozo::{DataValue, Db, ScriptMutability, Storage};

use crate::yaml::{RawNode, Schema, YamlEdge, YamlNode, resolve_node};

pub struct YamlLoadStats {
    pub nodes: usize,
    pub edges: usize,
    pub warnings: Vec<String>,
}

/// Turn "infra.schema" → "infra"; non-alphanumerics become `_` so the
/// result is a safe cozo identifier.
pub fn schema_prefix(schema_name: &str) -> String {
    let stem = schema_name.strip_suffix(".schema").unwrap_or(schema_name);
    stem.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

pub fn edge_relation_name(schema_name: &str, edge_kind: &str) -> String {
    format!("{}_{}", schema_prefix(schema_name), edge_kind)
}

pub fn create_yaml_schema<S>(db: &Db<S>)
where
    S: for<'s> Storage<'s> + 'static,
{
    let _ = db.run_default("::remove yaml_nodes");
    db.run_default(
        ":create yaml_nodes { path: String => schema: String, kind: String, attrs_json: String }",
    )
    .expect("create yaml_nodes");
}

fn create_edge_relation<S>(db: &Db<S>, name: &str)
where
    S: for<'s> Storage<'s> + 'static,
{
    let _ = db.run_default(&format!("::remove {name}"));
    db.run_default(&format!(
        ":create {name} {{ source: String, target: String }}"
    ))
    .unwrap_or_else(|e| panic!("create {name}: {e}"));
}

pub fn load_yaml_nodes<S>(db: &Db<S>, nodes: &[YamlNode])
where
    S: for<'s> Storage<'s> + 'static,
{
    if nodes.is_empty() {
        return;
    }
    let rows: Vec<DataValue> = nodes
        .iter()
        .map(|n| {
            let attrs_json = serde_json::to_string(&n.attrs).unwrap_or_else(|_| "{}".into());
            DataValue::List(vec![
                DataValue::Str(n.path.as_str().into()),
                DataValue::Str(n.schema.as_str().into()),
                DataValue::Str(n.kind.as_deref().unwrap_or("").into()),
                DataValue::Str(attrs_json.into()),
            ])
        })
        .collect();

    let mut params = BTreeMap::new();
    params.insert("data".to_string(), DataValue::List(rows));

    if let Err(e) = db.run_script(
        "?[path, schema, kind, attrs_json] <- $data \
         :put yaml_nodes {path => schema, kind, attrs_json}",
        params,
        ScriptMutability::Mutable,
    ) {
        eprintln!("Warning: failed to insert yaml_nodes: {e}");
    }
}

fn load_edges_into<S>(db: &Db<S>, relation: &str, edges: &[(String, String)])
where
    S: for<'s> Storage<'s> + 'static,
{
    if edges.is_empty() {
        return;
    }
    let rows: Vec<DataValue> = edges
        .iter()
        .map(|(s, t)| {
            DataValue::List(vec![
                DataValue::Str(s.as_str().into()),
                DataValue::Str(t.as_str().into()),
            ])
        })
        .collect();

    let mut params = BTreeMap::new();
    params.insert("data".to_string(), DataValue::List(rows));

    let script = format!("?[source, target] <- $data :put {relation} {{source, target}}");
    if let Err(e) = db.run_script(&script, params, ScriptMutability::Mutable) {
        eprintln!("Warning: failed to insert edges into {relation}: {e}");
    }
}

/// One-shot pipeline: resolve each raw node against its schema, create
/// the meta + per-edge relations, then bulk-insert.
pub fn build_yaml<S>(
    db: &Db<S>,
    schemas: &BTreeMap<String, Schema>,
    raw_nodes: Vec<RawNode>,
) -> YamlLoadStats
where
    S: for<'s> Storage<'s> + 'static,
{
    let mut warnings = Vec::new();
    let mut nodes: Vec<YamlNode> = Vec::with_capacity(raw_nodes.len());

    // edges grouped by target relation name (e.g. "infra_reads")
    let mut edges_by_relation: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();

    for raw in raw_nodes {
        let schema = schemas.get(&raw.schema);
        if schema.is_none() {
            warnings.push(format!(
                "{}: schema `{}` not found in corpus; all keys treated as attrs",
                raw.path, raw.schema
            ));
        }
        let schema_name = raw.schema.clone();
        let (node, mut warns) = resolve_node(raw, schema);
        warnings.append(&mut warns);

        for YamlEdge { kind, target } in &node.edges {
            let rel = edge_relation_name(&schema_name, kind);
            edges_by_relation
                .entry(rel)
                .or_default()
                .push((node.path.clone(), target.clone()));
        }
        nodes.push(node);
    }

    // Create relations
    create_yaml_schema(db);
    for (schema_name, schema) in schemas {
        for edge_kind in schema.edges.keys() {
            create_edge_relation(db, &edge_relation_name(schema_name, edge_kind));
        }
    }

    // Seed rows
    load_yaml_nodes(db, &nodes);
    let mut edge_count = 0;
    for (rel, edges) in &edges_by_relation {
        edge_count += edges.len();
        load_edges_into(db, rel, edges);
    }

    YamlLoadStats {
        nodes: nodes.len(),
        edges: edge_count,
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::yaml::{parse_node, parse_schema};
    use cozo::new_cozo_mem;

    const INFRA_SCHEMA: &str = include_str!("../tests/fixtures/infra.schema");
    const X3CUSTOMERPULL: &str = include_str!("../tests/fixtures/nodes/x3customerpull.yaml");
    const RIVSPROD02: &str = include_str!("../tests/fixtures/nodes/rivsprod02.yaml");
    const POSTGRESQL: &str = include_str!("../tests/fixtures/nodes/postgresql.yaml");
    const X3ROCS_CUSTOMER: &str = include_str!("../tests/fixtures/nodes/x3rocs-customer.yaml");

    fn corpus() -> (BTreeMap<String, Schema>, Vec<RawNode>) {
        let mut schemas = BTreeMap::new();
        schemas.insert(
            "infra.schema".to_string(),
            parse_schema(INFRA_SCHEMA).unwrap(),
        );
        let raws = vec![
            parse_node(X3CUSTOMERPULL, "x3customerpull".into()).unwrap(),
            parse_node(RIVSPROD02, "rivsprod02".into()).unwrap(),
            parse_node(POSTGRESQL, "postgresql".into()).unwrap(),
            parse_node(X3ROCS_CUSTOMER, "x3rocs-customer".into()).unwrap(),
        ];
        (schemas, raws)
    }

    #[test]
    fn prefix_strips_schema_suffix() {
        assert_eq!(schema_prefix("infra.schema"), "infra");
        assert_eq!(schema_prefix("my-domain.schema"), "my_domain");
        assert_eq!(schema_prefix("weird.name.schema"), "weird_name");
    }

    #[test]
    fn build_populates_nodes_and_per_kind_edges() {
        let db = new_cozo_mem().unwrap();
        let (schemas, raws) = corpus();
        let stats = build_yaml(&db, &schemas, raws);
        assert!(stats.warnings.is_empty(), "warns: {:?}", stats.warnings);
        assert_eq!(stats.nodes, 4);
        assert!(stats.edges >= 4); // reads, writes, hosts, serves

        // yaml_nodes count
        let r = db
            .run_default("?[count(path)] := *yaml_nodes{path}")
            .unwrap();
        assert_eq!(r.rows[0][0].get_int().unwrap(), 4);

        // infra_reads has one row: x3customerpull -> x3-customer-api
        let r = db
            .run_default("?[s, t] := *infra_reads[s, t]")
            .unwrap();
        assert_eq!(r.rows.len(), 1);
        assert_eq!(r.rows[0][0].get_str().unwrap(), "x3customerpull");
        assert_eq!(r.rows[0][1].get_str().unwrap(), "x3-customer-api");

        // infra_writes has one row
        let r = db.run_default("?[s, t] := *infra_writes[s, t]").unwrap();
        assert_eq!(r.rows.len(), 1);

        // infra_hosts has rivsprod02 → postgresql
        let r = db.run_default("?[s, t] := *infra_hosts[s, t]").unwrap();
        assert_eq!(r.rows.len(), 1);
        assert_eq!(r.rows[0][0].get_str().unwrap(), "rivsprod02");
    }

    #[test]
    fn attrs_serialise_as_json() {
        let db = new_cozo_mem().unwrap();
        let (schemas, raws) = corpus();
        build_yaml(&db, &schemas, raws);

        let r = db
            .run_default(
                "?[attrs_json] := *yaml_nodes{path, attrs_json}, path = 'x3customerpull'",
            )
            .unwrap();
        let json = r.rows[0][0].get_str().unwrap();
        assert!(json.contains("cadence"), "attrs json: {json}");
        assert!(json.contains("hourly"), "attrs json: {json}");
    }
}

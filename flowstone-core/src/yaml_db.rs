//! Populate cozo from parsed YAML schemas + nodes-documents.
//!
//! Relations:
//!
//! - `yaml_docs { doc => schema }` — one row per nodes-document, so
//!   the UI can enumerate tabs.
//! - `yaml_nodes { id, doc => kind, attrs_json }` — every node from
//!   every document.
//! - Per-(doc, edge-kind) relations `<prefix>_<edge>` where `<prefix>`
//!   is the doc stem with non-alphanumerics replaced with `_`.
//!   E.g. doc `infrastack` + edge `reads` → `infrastack_reads
//!   { source: String, target: String }`.
//!
//! Scoping by document (not schema) means each nodes-document becomes
//! its own tab with its own relations — no accidental cross-file edges.

use std::collections::BTreeMap;

use cozo::{DataValue, Db, ScriptMutability, Storage};

use crate::yaml::{NodeDoc, Schema, YamlEdge, YamlNode, resolve_node};

pub struct YamlLoadStats {
    pub nodes: usize,
    pub edges: usize,
    pub warnings: Vec<String>,
}

/// Normalise a doc name into a safe cozo relation prefix —
/// non-alphanumerics become `_`. e.g. `my-stack` → `my_stack`.
pub fn doc_prefix(doc_id: &str) -> String {
    doc_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

pub fn edge_relation_name(doc_id: &str, edge_kind: &str) -> String {
    format!("{}_{}", doc_prefix(doc_id), edge_kind)
}

fn create_meta<S>(db: &Db<S>)
where
    S: for<'s> Storage<'s> + 'static,
{
    let _ = db.run_default("::remove yaml_docs");
    db.run_default(":create yaml_docs { doc: String => schema: String }")
        .expect("create yaml_docs");

    let _ = db.run_default("::remove yaml_nodes");
    db.run_default(
        ":create yaml_nodes { id: String, doc: String => kind: String, attrs_json: String }",
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

fn load_docs<S>(db: &Db<S>, docs: &[(String, String)])
where
    S: for<'s> Storage<'s> + 'static,
{
    if docs.is_empty() {
        return;
    }
    let rows: Vec<DataValue> = docs
        .iter()
        .map(|(doc, schema)| {
            DataValue::List(vec![
                DataValue::Str(doc.as_str().into()),
                DataValue::Str(schema.as_str().into()),
            ])
        })
        .collect();
    let mut params = BTreeMap::new();
    params.insert("data".to_string(), DataValue::List(rows));
    if let Err(e) = db.run_script(
        "?[doc, schema] <- $data :put yaml_docs {doc => schema}",
        params,
        ScriptMutability::Mutable,
    ) {
        eprintln!("Warning: failed to insert yaml_docs: {e}");
    }
}

fn load_nodes<S>(db: &Db<S>, doc_id: &str, nodes: &[YamlNode])
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
                DataValue::Str(n.id.as_str().into()),
                DataValue::Str(doc_id.into()),
                DataValue::Str(n.kind.as_deref().unwrap_or("").into()),
                DataValue::Str(attrs_json.into()),
            ])
        })
        .collect();

    let mut params = BTreeMap::new();
    params.insert("data".to_string(), DataValue::List(rows));

    if let Err(e) = db.run_script(
        "?[id, doc, kind, attrs_json] <- $data \
         :put yaml_nodes {id, doc => kind, attrs_json}",
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

/// One-shot pipeline: resolve every node in every doc against its
/// schema, create the meta + per-edge relations, then bulk-insert.
pub fn build_yaml<S>(
    db: &Db<S>,
    schemas: &BTreeMap<String, Schema>,
    docs: Vec<NodeDoc>,
) -> YamlLoadStats
where
    S: for<'s> Storage<'s> + 'static,
{
    let mut warnings = Vec::new();

    // Resolve every doc's nodes so we know what edges exist before
    // creating relations. `resolved[doc_id] = (schema_name, [YamlNode])`.
    let mut resolved: Vec<(String, String, Vec<YamlNode>)> = Vec::with_capacity(docs.len());
    // edges grouped by target relation name (e.g. "infrastack_reads")
    let mut edges_by_relation: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();

    for doc in docs {
        let NodeDoc {
            doc_id,
            schema: schema_name,
            nodes: raw_nodes,
        } = doc;
        let schema = schemas.get(&schema_name);
        if schema.is_none() {
            warnings.push(format!(
                "{doc_id}: schema `{schema_name}` not found in corpus; all keys treated as attrs",
            ));
        }

        let mut resolved_nodes = Vec::with_capacity(raw_nodes.len());
        for raw in raw_nodes {
            let (node, mut warns) = resolve_node(raw, schema);
            warnings.append(&mut warns);
            for YamlEdge { kind, target } in &node.edges {
                let rel = edge_relation_name(&doc_id, kind);
                // If the schema declares this node's kind as the edge's
                // `to`, treat the declaring node as the target and the
                // listed value as the source. Lets a task write
                // `selects: [some_record]` while the schema reads
                // `selects: from: recordset, to: task` — the arrow
                // renders records → task, matching the data-flow.
                let edge_spec = schema.and_then(|s| s.edges.get(kind));
                let flip = matches!(
                    (edge_spec, node.kind.as_deref()),
                    (Some(spec), Some(k))
                        if spec.to.as_deref() == Some(k)
                        && spec.from.as_deref() != Some(k)
                );
                let (src, tgt) = if flip {
                    (target.clone(), node.id.clone())
                } else {
                    (node.id.clone(), target.clone())
                };
                edges_by_relation.entry(rel).or_default().push((src, tgt));
            }
            resolved_nodes.push(node);
        }

        resolved.push((doc_id, schema_name, resolved_nodes));
    }

    // Create meta relations and one edge relation per (doc, edge_kind)
    // that the doc's schema declares.
    create_meta(db);
    for (doc_id, schema_name, _) in &resolved {
        if let Some(schema) = schemas.get(schema_name) {
            for edge_kind in schema.edges.keys() {
                create_edge_relation(db, &edge_relation_name(doc_id, edge_kind));
            }
        }
    }

    // Seed rows
    let doc_rows: Vec<(String, String)> = resolved
        .iter()
        .map(|(doc_id, schema_name, _)| (doc_id.clone(), schema_name.clone()))
        .collect();
    load_docs(db, &doc_rows);

    let mut node_count = 0;
    for (doc_id, _, nodes) in &resolved {
        node_count += nodes.len();
        load_nodes(db, doc_id, nodes);
    }

    let mut edge_count = 0;
    for (rel, edges) in &edges_by_relation {
        edge_count += edges.len();
        load_edges_into(db, rel, edges);
    }

    YamlLoadStats {
        nodes: node_count,
        edges: edge_count,
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::yaml::{parse_node_doc, parse_schema};
    use cozo::new_cozo_mem;

    const INFRA_SCHEMA: &str = include_str!("../tests/fixtures/infra.yaml");
    const INFRASTACK: &str = include_str!("../tests/fixtures/infrastack.yaml");

    fn corpus() -> (BTreeMap<String, Schema>, Vec<NodeDoc>) {
        let mut schemas = BTreeMap::new();
        schemas.insert("infra".to_string(), parse_schema(INFRA_SCHEMA).unwrap());
        let docs = vec![parse_node_doc(INFRASTACK, "infrastack".to_string()).unwrap()];
        (schemas, docs)
    }

    #[test]
    fn prefix_sanitises_doc_id() {
        assert_eq!(doc_prefix("infrastack"), "infrastack");
        assert_eq!(doc_prefix("my-stack"), "my_stack");
        assert_eq!(doc_prefix("weird.name"), "weird_name");
    }

    #[test]
    fn build_populates_docs_nodes_and_per_kind_edges() {
        let db = new_cozo_mem().unwrap();
        let (schemas, docs) = corpus();
        let stats = build_yaml(&db, &schemas, docs);
        assert!(stats.warnings.is_empty(), "warns: {:?}", stats.warnings);
        assert_eq!(stats.nodes, 4);
        assert!(stats.edges >= 4);

        // yaml_docs has one row
        let r = db.run_default("?[doc, schema] := *yaml_docs{doc, schema}").unwrap();
        assert_eq!(r.rows.len(), 1);
        assert_eq!(r.rows[0][0].get_str().unwrap(), "infrastack");
        assert_eq!(r.rows[0][1].get_str().unwrap(), "infra");

        // yaml_nodes count
        let r = db.run_default("?[count(id)] := *yaml_nodes{id}").unwrap();
        assert_eq!(r.rows[0][0].get_int().unwrap(), 4);

        // infrastack_reads has one row: x3customerpull -> x3-customer-api
        let r = db.run_default("?[s, t] := *infrastack_reads[s, t]").unwrap();
        assert_eq!(r.rows.len(), 1);
        assert_eq!(r.rows[0][0].get_str().unwrap(), "x3customerpull");
        assert_eq!(r.rows[0][1].get_str().unwrap(), "x3-customer-api");

        // infrastack_writes has one row
        let r = db.run_default("?[s, t] := *infrastack_writes[s, t]").unwrap();
        assert_eq!(r.rows.len(), 1);

        // infrastack_hosts has rivsprod02 → postgresql
        let r = db.run_default("?[s, t] := *infrastack_hosts[s, t]").unwrap();
        assert_eq!(r.rows.len(), 1);
        assert_eq!(r.rows[0][0].get_str().unwrap(), "rivsprod02");
    }

    #[test]
    fn attrs_serialise_as_json() {
        let db = new_cozo_mem().unwrap();
        let (schemas, docs) = corpus();
        build_yaml(&db, &schemas, docs);

        let r = db
            .run_default(
                "?[attrs_json] := *yaml_nodes{id, doc, attrs_json}, id = 'x3customerpull'",
            )
            .unwrap();
        let json = r.rows[0][0].get_str().unwrap();
        assert!(json.contains("cadence"), "attrs json: {json}");
        assert!(json.contains("hourly"), "attrs json: {json}");
    }
}

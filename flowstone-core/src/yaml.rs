//! Parse schema and nodes-document YAML for Flowstone's directed-graph
//! tab.
//!
//! A corpus may carry two kinds of YAML document alongside its markdown.
//! They are discriminated by contents, not filename:
//!
//! - A **schema** declares an ontology — which edge kinds exist, which
//!   node kinds exist, and optional render hints. It has `edges:` or
//!   `nodes:` at the top level.
//! - A **nodes-document** declares a set of related nodes all obeying
//!   the same schema. It has `schema: <name>` at the top level, and
//!   every other top-level key is a node id whose value is the node's
//!   body (`kind:`, attrs, and out-edges).
//!
//! Schema lookup is by flat filename stem (no extension, no path), so
//! physical location in the corpus is irrelevant. Each nodes-document
//! becomes its own tab in the UI.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct Schema {
    #[serde(default)]
    pub edges: BTreeMap<String, EdgeSpec>,
    #[serde(default)]
    pub nodes: BTreeMap<String, NodeSpec>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct EdgeSpec {
    pub from: Option<String>,
    pub to: Option<String>,
    pub colour: Option<String>,
    #[serde(default)]
    pub directed: bool,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct NodeSpec {
    pub shape: Option<String>,
}

/// A whole nodes-document: its stem (used as the doc id and relation
/// prefix), the schema it refers to, and every node it declares.
#[derive(Debug, Clone)]
pub struct NodeDoc {
    pub doc_id: String,
    pub schema: String,
    pub nodes: Vec<RawNode>,
}

/// One unresolved node — the map value under a node-id key inside a
/// nodes-document. `kind` has been peeled off; everything else stays
/// raw until the schema is consulted.
#[derive(Debug, Clone)]
pub struct RawNode {
    pub id: String,
    pub kind: Option<String>,
    pub other: BTreeMap<String, serde_yml::Value>,
}

/// A node after its schema has been applied: edge-shaped keys are
/// peeled off into `edges`, everything else lands in `attrs`.
#[derive(Debug, Clone)]
pub struct YamlNode {
    pub id: String,
    pub kind: Option<String>,
    pub attrs: BTreeMap<String, serde_yml::Value>,
    pub edges: Vec<YamlEdge>,
}

#[derive(Debug, Clone)]
pub struct YamlEdge {
    pub kind: String,
    pub target: String,
}

pub fn parse_schema(source: &str) -> Result<Schema, String> {
    serde_yml::from_str(source).map_err(|e| format!("schema parse error: {e}"))
}

/// Sort a YAML document into either a schema or a nodes-document by
/// examining its top-level keys. A nodes-document has `schema:` at the
/// top; a schema has `edges:` or `nodes:`. An empty document or one
/// with neither clue is ambiguous.
#[derive(Debug)]
pub enum YamlDocument {
    Schema(Schema),
    Nodes(NodeDoc),
}

pub fn parse_document(source: &str, doc_id: String) -> Result<YamlDocument, String> {
    let val: serde_yml::Value =
        serde_yml::from_str(source).map_err(|e| format!("{doc_id}: {e}"))?;
    let map = match &val {
        serde_yml::Value::Mapping(m) => m,
        _ => return Err(format!("{doc_id}: top-level YAML must be a mapping")),
    };
    let has_schema_key = map.keys().any(|k| k.as_str() == Some("schema"));
    let has_ontology_key = map
        .keys()
        .any(|k| matches!(k.as_str(), Some("edges") | Some("nodes")));

    if has_schema_key {
        parse_node_doc(source, doc_id).map(YamlDocument::Nodes)
    } else if has_ontology_key {
        parse_schema(source).map(YamlDocument::Schema)
    } else {
        Err(format!(
            "{doc_id}: cannot tell schema from nodes-document — needs a top-level `schema:`, `edges:`, or `nodes:` clause"
        ))
    }
}

/// Parse a nodes-document: expects a top-level `schema:` plus one key
/// per node id, each mapping to a node body (itself a YAML mapping).
pub fn parse_node_doc(source: &str, doc_id: String) -> Result<NodeDoc, String> {
    let val: serde_yml::Value =
        serde_yml::from_str(source).map_err(|e| format!("{doc_id}: {e}"))?;
    let map = match val {
        serde_yml::Value::Mapping(m) => m,
        _ => return Err(format!("{doc_id}: top-level YAML must be a mapping")),
    };

    let mut schema: Option<String> = None;
    let mut nodes: Vec<RawNode> = Vec::new();

    for (k, v) in map {
        let key = match k.as_str() {
            Some(s) => s.to_string(),
            None => return Err(format!("{doc_id}: non-string top-level key")),
        };
        if key == "schema" {
            schema = Some(
                v.as_str()
                    .ok_or_else(|| format!("{doc_id}: schema: must be a string"))?
                    .to_string(),
            );
            continue;
        }
        let body = match v {
            serde_yml::Value::Mapping(m) => m,
            _ => {
                return Err(format!(
                    "{doc_id}: node `{key}` must be a mapping (got a scalar or list)"
                ));
            }
        };
        nodes.push(build_raw_node(&doc_id, key, body)?);
    }

    let schema = schema.ok_or_else(|| format!("{doc_id}: missing `schema:` clause"))?;

    Ok(NodeDoc {
        doc_id,
        schema,
        nodes,
    })
}

fn build_raw_node(
    doc_id: &str,
    node_id: String,
    body: serde_yml::Mapping,
) -> Result<RawNode, String> {
    let mut kind: Option<String> = None;
    let mut other: BTreeMap<String, serde_yml::Value> = BTreeMap::new();

    for (k, v) in body {
        let key = match k.as_str() {
            Some(s) => s.to_string(),
            None => return Err(format!("{doc_id}/{node_id}: non-string key in node body")),
        };
        if key == "kind" {
            kind = v.as_str().map(str::to_string);
            continue;
        }
        other.insert(key, v);
    }

    Ok(RawNode {
        id: node_id,
        kind,
        other,
    })
}

/// Apply a schema to a raw node: keys whose names match an edge kind in
/// the schema are extracted as `YamlEdge`s; everything else stays in
/// `attrs`. Returns the resolved node plus any non-fatal warnings.
pub fn resolve_node(raw: RawNode, schema: Option<&Schema>) -> (YamlNode, Vec<String>) {
    let mut attrs = BTreeMap::new();
    let mut edges = Vec::new();
    let mut warnings = Vec::new();

    for (key, value) in raw.other {
        let is_edge = schema.map(|s| s.edges.contains_key(&key)).unwrap_or(false);
        if !is_edge {
            attrs.insert(key, value);
            continue;
        }
        match value {
            serde_yml::Value::String(s) => {
                edges.push(YamlEdge {
                    kind: key,
                    target: s,
                });
            }
            serde_yml::Value::Sequence(seq) => {
                for item in seq {
                    match item {
                        serde_yml::Value::String(s) => edges.push(YamlEdge {
                            kind: key.clone(),
                            target: s,
                        }),
                        other => warnings.push(format!(
                            "{}: edge `{}` entry must be a string, got {:?}",
                            raw.id, key, other
                        )),
                    }
                }
            }
            other => warnings.push(format!(
                "{}: edge `{}` must be a string or list, got {:?}",
                raw.id, key, other
            )),
        }
    }

    (
        YamlNode {
            id: raw.id,
            kind: raw.kind,
            attrs,
            edges,
        },
        warnings,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const INFRA_SCHEMA: &str = include_str!("../tests/fixtures/infra.yaml");
    const INFRASTACK: &str = include_str!("../tests/fixtures/infrastack.yaml");

    fn infra() -> Schema {
        parse_schema(INFRA_SCHEMA).expect("infra.yaml parses")
    }

    #[test]
    fn schema_has_expected_edge_kinds() {
        let s = infra();
        for k in ["hosts", "serves", "table", "api", "reads", "writes"] {
            assert!(s.edges.contains_key(k), "missing edge kind: {k}");
        }
        assert_eq!(s.edges["reads"].colour.as_deref(), Some("#4a9eff"));
        assert!(s.edges["reads"].directed);
    }

    #[test]
    fn schema_has_node_shapes() {
        let s = infra();
        assert_eq!(s.nodes["task"].shape.as_deref(), Some("diamond"));
        assert_eq!(s.nodes["table"].shape.as_deref(), Some("cylinder"));
    }

    #[test]
    fn doc_parses_many_nodes() {
        let doc = parse_node_doc(INFRASTACK, "infrastack".to_string()).unwrap();
        assert_eq!(doc.doc_id, "infrastack");
        assert_eq!(doc.schema, "infra");
        let ids: Vec<_> = doc.nodes.iter().map(|n| n.id.as_str()).collect();
        assert!(ids.contains(&"x3customerpull"));
        assert!(ids.contains(&"rivsprod02"));
        assert!(ids.contains(&"postgresql"));
    }

    #[test]
    fn node_splits_edges_from_attrs() {
        let doc = parse_node_doc(INFRASTACK, "infrastack".to_string()).unwrap();
        let raw = doc
            .nodes
            .into_iter()
            .find(|n| n.id == "x3customerpull")
            .unwrap();
        assert_eq!(raw.kind.as_deref(), Some("task"));

        let (node, warns) = resolve_node(raw, Some(&infra()));
        assert!(warns.is_empty(), "unexpected warnings: {:?}", warns);

        let reads: Vec<_> = node.edges.iter().filter(|e| e.kind == "reads").collect();
        assert_eq!(reads.len(), 1);
        assert_eq!(reads[0].target, "x3-customer-api");
        let writes: Vec<_> = node.edges.iter().filter(|e| e.kind == "writes").collect();
        assert_eq!(writes[0].target, "x3rocs-customer");

        assert!(node.attrs.contains_key("cadence"));
        assert!(node.attrs.contains_key("owner"));
    }

    #[test]
    fn node_without_schema_match_keeps_all_as_attrs() {
        let doc = parse_node_doc(INFRASTACK, "infrastack".to_string()).unwrap();
        let raw = doc
            .nodes
            .into_iter()
            .find(|n| n.id == "rivsprod02")
            .unwrap();
        let (node, _warns) = resolve_node(raw, None);
        assert!(node.edges.is_empty());
        assert!(node.attrs.contains_key("hosts"));
        assert!(node.attrs.contains_key("fqdn"));
    }

    #[test]
    fn missing_schema_clause_is_an_error() {
        let err = parse_node_doc(
            "x3customerpull:\n  kind: task\n",
            "x".to_string(),
        )
        .unwrap_err();
        assert!(err.contains("missing `schema:`"), "got: {err}");
    }

    #[test]
    fn parse_document_routes_schema_and_nodes() {
        let doc = parse_document(INFRA_SCHEMA, "infra".to_string()).unwrap();
        assert!(matches!(doc, YamlDocument::Schema(_)), "infra should parse as Schema");

        let doc = parse_document(INFRASTACK, "infrastack".to_string()).unwrap();
        assert!(matches!(doc, YamlDocument::Nodes(_)), "infrastack should parse as Nodes");
    }

    #[test]
    fn parse_document_rejects_ambiguous_yaml() {
        let err = parse_document("greeting: hello\n", "x".to_string()).unwrap_err();
        assert!(err.contains("cannot tell schema from nodes-document"), "got: {err}");
    }
}

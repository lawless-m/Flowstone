//! Parse schema and node YAML for Flowstone's directed-graph tab.
//!
//! A corpus may carry two kinds of YAML document alongside its markdown.
//! They are discriminated by contents, not filename:
//!
//! - A **schema** declares an ontology — which edge kinds exist, which
//!   node kinds exist, and optional render hints. It has `edges:` or
//!   `nodes:` at the top level.
//! - A **node** declares a thing in the graph. Its first clause is
//!   `schema: <name>`, which picks the vocabulary. Keys matching an
//!   edge name in that schema are treated as out-edges (string or list
//!   of target ids); any other key is kept as an attribute.
//!
//! Schema lookup is by flat filename stem (no extension, no path), so
//! physical location in the corpus is irrelevant. Cross-schema edges
//! are not supported — a node under one schema cannot target a node
//! under another.

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

/// A parsed node YAML, with the `schema:` + `kind:` spine separated out
/// and the remaining keys left raw until a schema is known.
#[derive(Debug, Clone)]
pub struct RawNode {
    pub path: String,
    pub schema: String,
    pub kind: Option<String>,
    pub other: BTreeMap<String, serde_yml::Value>,
}

/// A node YAML after the schema has been applied: edge-shaped keys are
/// peeled off into `edges`, everything else lands in `attrs`.
#[derive(Debug, Clone)]
pub struct YamlNode {
    pub path: String,
    pub schema: String,
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

/// Sort a YAML document into either a schema or a node by examining its
/// top-level keys. A node is anything with a top-level `schema:` clause;
/// a schema is anything with a top-level `edges:` or `nodes:` mapping.
/// An empty document or one with neither clue is ambiguous.
#[derive(Debug)]
pub enum YamlDocument {
    Schema(Schema),
    Node(RawNode),
}

pub fn parse_document(source: &str, path: String) -> Result<YamlDocument, String> {
    let val: serde_yml::Value =
        serde_yml::from_str(source).map_err(|e| format!("{path}: {e}"))?;
    let map = match &val {
        serde_yml::Value::Mapping(m) => m,
        _ => return Err(format!("{path}: top-level YAML must be a mapping")),
    };
    let has_schema_key = map.keys().any(|k| k.as_str() == Some("schema"));
    let has_ontology_key = map
        .keys()
        .any(|k| matches!(k.as_str(), Some("edges") | Some("nodes")));

    if has_schema_key {
        parse_node(source, path).map(YamlDocument::Node)
    } else if has_ontology_key {
        parse_schema(source).map(YamlDocument::Schema)
    } else {
        Err(format!(
            "{path}: cannot tell schema from node — needs a top-level `schema:`, `edges:`, or `nodes:` clause"
        ))
    }
}

pub fn parse_node(source: &str, path: String) -> Result<RawNode, String> {
    let val: serde_yml::Value =
        serde_yml::from_str(source).map_err(|e| format!("{path}: {e}"))?;
    let map = match val {
        serde_yml::Value::Mapping(m) => m,
        _ => return Err(format!("{path}: top-level YAML must be a mapping")),
    };

    let mut schema: Option<String> = None;
    let mut kind: Option<String> = None;
    let mut other: BTreeMap<String, serde_yml::Value> = BTreeMap::new();

    for (k, v) in map {
        let key = match k.as_str() {
            Some(s) => s.to_string(),
            None => return Err(format!("{path}: non-string key in mapping")),
        };
        match key.as_str() {
            "schema" => {
                schema = Some(
                    v.as_str()
                        .ok_or_else(|| format!("{path}: schema: must be a string"))?
                        .to_string(),
                );
            }
            "kind" => {
                kind = v.as_str().map(str::to_string);
            }
            _ => {
                other.insert(key, v);
            }
        }
    }

    let schema = schema.ok_or_else(|| format!("{path}: missing `schema:` clause"))?;

    Ok(RawNode {
        path,
        schema,
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
                            raw.path, key, other
                        )),
                    }
                }
            }
            other => warnings.push(format!(
                "{}: edge `{}` must be a string or list, got {:?}",
                raw.path, key, other
            )),
        }
    }

    (
        YamlNode {
            path: raw.path,
            schema: raw.schema,
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
    const X3CUSTOMERPULL: &str = include_str!("../tests/fixtures/nodes/x3customerpull.yaml");
    const RIVSPROD02: &str = include_str!("../tests/fixtures/nodes/rivsprod02.yaml");

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
    fn node_splits_edges_from_attrs() {
        let raw = parse_node(X3CUSTOMERPULL, "x3customerpull".to_string()).unwrap();
        assert_eq!(raw.schema, "infra");
        assert_eq!(raw.kind.as_deref(), Some("task"));

        let (node, warns) = resolve_node(raw, Some(&infra()));
        assert!(warns.is_empty(), "unexpected warnings: {:?}", warns);

        // Edges
        let reads: Vec<_> = node.edges.iter().filter(|e| e.kind == "reads").collect();
        assert_eq!(reads.len(), 1);
        assert_eq!(reads[0].target, "x3-customer-api");
        let writes: Vec<_> = node.edges.iter().filter(|e| e.kind == "writes").collect();
        assert_eq!(writes[0].target, "x3rocs-customer");

        // Non-edge keys stay as attrs
        assert!(node.attrs.contains_key("cadence"));
        assert!(node.attrs.contains_key("owner"));
    }

    #[test]
    fn node_without_schema_match_keeps_all_as_attrs() {
        let raw = parse_node(RIVSPROD02, "rivsprod02".to_string()).unwrap();
        // With no schema, nothing is recognised as an edge.
        let (node, _warns) = resolve_node(raw, None);
        assert!(node.edges.is_empty());
        assert!(node.attrs.contains_key("hosts"));
        assert!(node.attrs.contains_key("fqdn"));
    }

    #[test]
    fn missing_schema_clause_is_an_error() {
        let err = parse_node("kind: task\n", "x".to_string()).unwrap_err();
        assert!(err.contains("missing `schema:`"), "got: {err}");
    }

    #[test]
    fn parse_document_routes_schema_and_node() {
        let doc = parse_document(INFRA_SCHEMA, "infra".to_string()).unwrap();
        assert!(matches!(doc, YamlDocument::Schema(_)), "infra should parse as Schema");

        let doc = parse_document(X3CUSTOMERPULL, "x3customerpull".to_string()).unwrap();
        assert!(matches!(doc, YamlDocument::Node(_)), "node should parse as Node");
    }

    #[test]
    fn parse_document_rejects_ambiguous_yaml() {
        let err = parse_document("greeting: hello\n", "x".to_string()).unwrap_err();
        assert!(err.contains("cannot tell schema from node"), "got: {err}");
    }
}

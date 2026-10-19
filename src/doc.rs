//! The workspace features document: one per workspace root, read by every
//! `features` line and by `annotate`, written by `init`.
//!
//! It is a 1o document (`v1.workspace.1o.features`), its shape declared once
//! in [`SCHEMA_V1`] (`schema/features/v1.schema.json`) and checked by
//! core-1o's validator. `features.toml` is accepted for the same document:
//! both twins read into the one in-memory form, [`FeaturesDoc`], and a TOML
//! document is checked against the same schema by its 1o rendering. The
//! README's table of keys is rendered from the schema ([`schema_table`]).
//!
//! ```text
//! {
//!   features {
//!     default {
//!       feature "log"
//!     }
//!     feature {
//!       name "log"
//!       dep "app:tracing"
//!       dep "lib:log"
//!     }
//!     crate {
//!       name "app"
//!       optional "extra"
//!       feature {
//!         name "std"
//!         enable "lib/std"
//!       }
//!     }
//!   }
//! }
//! ```
//!
//! What core-1o 0.1.1 cannot say, and this module checks itself: that a key
//! appears at most once (`name`), that feature and crate names are unique,
//! and that a document has no comment lines (its lexer has none).

use crate::error::{PassengerError, Result};
use core_1o::engine::{json_schema_validator::validate_ast, Lexer, Node, NodeValue, Parser};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The 1o twin's file name at the workspace root: version, name, `1o`,
/// modifier, as core-1o and its language server read file names.
pub const DOC_1O: &str = "v1.workspace.1o.features";
/// The TOML twin's file name at the workspace root.
pub const DOC_TOML: &str = "features.toml";
/// The 1o modifier and schema version of the document.
pub const MODIFIER: &str = "features";
pub const SCHEMA_VERSION: &str = "v1";
/// The schema, declared once (a JSON schema in core-1o's subset; `x-toml`
/// names each key's TOML spelling for the README table).
pub const SCHEMA_V1: &str = include_str!("../schema/features/v1.schema.json");

/// The document, as both twins read.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeaturesDoc {
    /// Workspace features on by default in every crate that has them;
    /// `None` leaves each crate's default list as it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub features: BTreeMap<String, Feature>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub crates: BTreeMap<String, CrateLines>,
}

/// One workspace feature.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Feature {
    /// `crate:dependency`, in order.
    #[serde(default)]
    pub deps: Vec<String>,
}

/// One crate's own lines.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrateLines {
    /// The crate's default list, verbatim, replacing the derived one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Vec<String>>,
    /// Dependencies made optional with no workspace feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub optional: Vec<String>,
    /// A feature's whole list in this crate, verbatim, replacing the derived one.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub features: BTreeMap<String, Vec<String>>,
}

/// Which twin a document was read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Twin {
    OneO,
    Toml,
}

/// A document found at a workspace root.
#[derive(Debug, Clone)]
pub struct Found {
    pub path: PathBuf,
    pub twin: Twin,
    pub doc: FeaturesDoc,
}

/// `crate:dependency` split, or why not.
pub fn split_dep(entry: &str) -> Result<(String, String)> {
    match entry.split_once(':') {
        Some((c, d)) if !c.trim().is_empty() && !d.trim().is_empty() && !d.contains(':') => Ok((c.trim().replace('-', "_"), d.trim().replace('-', "_"))),
        _ => Err(PassengerError::FeaturesDoc { detail: format!("`{entry}` is not `crate:dependency`") }),
    }
}

fn schema() -> serde_json::Value {
    serde_json::from_str(SCHEMA_V1).expect("the declared schema is JSON")
}

/// 1-based line of a character index.
fn line_of(text: &str, char_index: usize) -> usize {
    text.chars().take(char_index).filter(|c| *c == '\n').count() + 1
}

fn doc_err(text: &str, at: usize, what: impl std::fmt::Display) -> PassengerError {
    PassengerError::FeaturesDoc { detail: format!("line {}: {what}", line_of(text, at)) }
}

/// The 1o twin parsed and checked against the schema.
pub fn parse_1o(text: &str) -> Result<FeaturesDoc> {
    let nodes = Parser::new(Lexer::new(text).tokenize()).parse().map_err(|e| PassengerError::FeaturesDoc { detail: format!("{e:?}") })?;
    let mut diags = Vec::new();
    validate_ast(&nodes, &schema(), &mut diags);
    if !diags.is_empty() {
        let all: Vec<String> = diags.iter().map(|d| format!("line {}: {}", line_of(text, d.span.start), d.message)).collect();
        return Err(PassengerError::FeaturesDoc { detail: all.join("; ") });
    }
    let root = nodes.iter().find(|n| n.key == "features").ok_or_else(|| PassengerError::FeaturesDoc { detail: "no `features` block".into() })?;
    read_features(text, root)
}

fn children(n: &Node) -> &[Node] {
    match &n.value {
        NodeValue::Block(c) => c,
        _ => &[],
    }
}

fn string(n: &Node) -> Option<&str> {
    match &n.value {
        NodeValue::String(s) => Some(s),
        _ => None,
    }
}

/// The one `name` of a block.
fn name_of(text: &str, block: &Node) -> Result<String> {
    let names: Vec<&Node> = children(block).iter().filter(|c| c.key == "name").collect();
    match names.as_slice() {
        [one] => Ok(string(one).unwrap_or_default().to_string()),
        [] => Err(doc_err(text, block.span.start, format!("`{}` has no `name`", block.key))),
        [_, second, ..] => Err(doc_err(text, second.span.start, format!("`{}` has two `name` lines", block.key))),
    }
}

fn strings(block: &Node, key: &str) -> Vec<String> {
    children(block).iter().filter(|c| c.key == key).filter_map(string).map(str::to_string).collect()
}

/// The `default { feature ... }` block, at most one.
fn default_of(text: &str, block: &Node) -> Result<Option<Vec<String>>> {
    let defaults: Vec<&Node> = children(block).iter().filter(|c| c.key == "default").collect();
    match defaults.as_slice() {
        [] => Ok(None),
        [one] => Ok(Some(strings(one, "feature"))),
        [_, second, ..] => Err(doc_err(text, second.span.start, format!("`{}` has two `default` blocks", block.key))),
    }
}

fn read_features(text: &str, root: &Node) -> Result<FeaturesDoc> {
    let mut doc = FeaturesDoc { default: default_of(text, root)?, ..Default::default() };
    for n in children(root) {
        match n.key.as_str() {
            "feature" => {
                let name = name_of(text, n)?;
                if doc.features.contains_key(&name) {
                    return Err(doc_err(text, n.span.start, format!("feature `{name}` is declared twice")));
                }
                doc.features.insert(name, Feature { deps: strings(n, "dep") });
            }
            "crate" => {
                let name = name_of(text, n)?;
                if doc.crates.contains_key(&name) {
                    return Err(doc_err(text, n.span.start, format!("crate `{name}` is declared twice")));
                }
                let mut lines = CrateLines { default: default_of(text, n)?, optional: strings(n, "optional"), features: BTreeMap::new() };
                for f in children(n).iter().filter(|c| c.key == "feature") {
                    let fname = name_of(text, f)?;
                    if lines.features.contains_key(&fname) {
                        return Err(doc_err(text, f.span.start, format!("crate `{name}` declares feature `{fname}` twice")));
                    }
                    lines.features.insert(fname, strings(f, "enable"));
                }
                doc.crates.insert(name, lines);
            }
            _ => {}
        }
    }
    doc.check_entries()?;
    Ok(doc)
}

fn quoted(s: &str) -> Result<String> {
    if s.contains('"') {
        return Err(PassengerError::FeaturesDoc { detail: format!("`{s}`: a 1o string cannot hold a double quote") });
    }
    Ok(format!("\"{s}\""))
}

impl FeaturesDoc {
    /// The TOML twin parsed, then checked against the schema through its 1o rendering.
    pub fn from_toml(text: &str) -> Result<FeaturesDoc> {
        let doc: FeaturesDoc = toml::from_str(text).map_err(|e| PassengerError::FeaturesDoc { detail: e.to_string() })?;
        doc.check_entries()?;
        parse_1o(&doc.to_1o()?)?;
        Ok(doc)
    }

    pub fn from_1o(text: &str) -> Result<FeaturesDoc> {
        parse_1o(text)
    }

    /// Every `crate:dependency` well formed.
    fn check_entries(&self) -> Result<()> {
        for f in self.features.values() {
            for d in &f.deps {
                split_dep(d)?;
            }
        }
        Ok(())
    }

    /// The 1o twin: sorted, two-space indented, one value per line.
    pub fn to_1o(&self) -> Result<String> {
        let mut s = String::from("{\n  features {\n");
        if let Some(def) = &self.default {
            s.push_str("    default {\n");
            for f in def {
                s.push_str(&format!("      feature {}\n", quoted(f)?));
            }
            s.push_str("    }\n");
        }
        for (name, f) in &self.features {
            s.push_str(&format!("    feature {{\n      name {}\n", quoted(name)?));
            for d in &f.deps {
                s.push_str(&format!("      dep {}\n", quoted(d)?));
            }
            s.push_str("    }\n");
        }
        for (name, c) in &self.crates {
            s.push_str(&format!("    crate {{\n      name {}\n", quoted(name)?));
            if let Some(def) = &c.default {
                s.push_str("      default {\n");
                for f in def {
                    s.push_str(&format!("        feature {}\n", quoted(f)?));
                }
                s.push_str("      }\n");
            }
            for o in &c.optional {
                s.push_str(&format!("      optional {}\n", quoted(o)?));
            }
            for (fname, list) in &c.features {
                s.push_str(&format!("      feature {{\n        name {}\n", quoted(fname)?));
                for e in list {
                    s.push_str(&format!("        enable {}\n", quoted(e)?));
                }
                s.push_str("      }\n");
            }
            s.push_str("    }\n");
        }
        s.push_str("  }\n}\n");
        Ok(s)
    }

    /// The TOML twin.
    pub fn to_toml(&self) -> Result<String> {
        let body = toml::to_string(self).map_err(|e| PassengerError::FeaturesDoc { detail: e.to_string() })?;
        Ok(format!("# code_passenger workspace features (schema: {MODIFIER} {SCHEMA_VERSION}; the 1o twin is {DOC_1O})\n{body}"))
    }
}

/// The document at `root`: the 1o twin, or the TOML twin; both is a refusal.
pub fn find(root: &Path) -> Result<Option<Found>> {
    let one = root.join(DOC_1O);
    let toml_path = root.join(DOC_TOML);
    match (one.is_file(), toml_path.is_file()) {
        (true, true) => Err(PassengerError::TwoFeaturesDocs { detail: root.display().to_string() }),
        (true, false) => {
            let text = std::fs::read_to_string(&one)?;
            let doc = FeaturesDoc::from_1o(&text).map_err(|e| PassengerError::FeaturesDoc { detail: format!("{}: {e}", one.display()) })?;
            Ok(Some(Found { path: one, twin: Twin::OneO, doc }))
        }
        (false, true) => {
            let text = std::fs::read_to_string(&toml_path)?;
            let doc = FeaturesDoc::from_toml(&text).map_err(|e| PassengerError::FeaturesDoc { detail: format!("{}: {e}", toml_path.display()) })?;
            Ok(Some(Found { path: toml_path, twin: Twin::Toml, doc }))
        }
        (false, false) => Ok(None),
    }
}

/// The document at `root`, or the refusal that there is none.
pub fn require(root: &Path) -> Result<Found> {
    find(root)?.ok_or_else(|| PassengerError::NoFeaturesDoc { detail: root.display().to_string() })
}

/// The README's table of keys, rendered from the schema: one row per key,
/// its 1o path, its TOML spelling, its value and what it means.
pub fn schema_table() -> String {
    fn walk(path: &str, v: &serde_json::Value, rows: &mut Vec<String>) {
        let Some(props) = v.get("properties").and_then(|p| p.as_object()) else { return };
        let required: Vec<&str> = v.get("required").and_then(|r| r.as_array()).map(|a| a.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
        for (key, p) in props {
            let here = if path.is_empty() { key.clone() } else { format!("{path} {key}") };
            let kind = match p.get("type").and_then(|t| t.as_str()) {
                Some("object") => "block",
                Some("string") => "string",
                Some(other) => other,
                None => "",
            };
            let toml = p.get("x-toml").and_then(|t| t.as_str()).unwrap_or("");
            let desc = p.get("description").and_then(|t| t.as_str()).unwrap_or("");
            let req = if required.contains(&key.as_str()) { ", required" } else { "" };
            rows.push(format!("| `{here}` | `{toml}` | {kind}{req} | {desc} |"));
            walk(&here, p, rows);
        }
    }
    let mut rows = vec!["| 1o key | TOML | value | meaning |".to_string(), "|---|---|---|---|".to_string()];
    walk("", &schema(), &mut rows);
    rows.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE_O: &str = r#"{
  features {
    default {
      feature "log"
    }
    feature {
      name "log"
      dep "app:tracing"
      dep "lib:log"
    }
    crate {
      name "app"
      default {
        feature "std"
      }
      optional "extra"
      feature {
        name "std"
        enable "lib/std"
        enable "dep:extra"
      }
    }
  }
}
"#;

    const TOML: &str = r#"default = ["log"]

[features.log]
deps = ["app:tracing", "lib:log"]

[crates.app]
default = ["std"]
optional = ["extra"]

[crates.app.features]
std = ["lib/std", "dep:extra"]
"#;

    #[test]
    fn the_twins_read_into_one_model() {
        let a = FeaturesDoc::from_1o(ONE_O).unwrap();
        let b = FeaturesDoc::from_toml(TOML).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.features["log"].deps, vec!["app:tracing".to_string(), "lib:log".into()]);
        assert_eq!(a.crates["app"].features["std"], vec!["lib/std".to_string(), "dep:extra".into()]);
        // each twin renders to the other and back
        assert_eq!(FeaturesDoc::from_1o(&b.to_1o().unwrap()).unwrap(), a);
        assert_eq!(FeaturesDoc::from_toml(&a.to_toml().unwrap()).unwrap(), a);
        assert_eq!(a.to_1o().unwrap(), ONE_O, "the 1o rendering is the canonical form");
    }

    #[test]
    fn the_schema_refuses_what_it_does_not_declare() {
        let e = FeaturesDoc::from_1o("{ features { feature { name \"x\" deps \"a:b\" } } }").unwrap_err().to_string();
        assert!(e.contains("[CP0010]") && e.contains("Unknown key 'deps'"), "{e}");
        let e = FeaturesDoc::from_1o("{ features { feature { dep \"a:b\" } } }").unwrap_err().to_string();
        assert!(e.contains("Missing required key 'name'"), "{e}");
        let e = FeaturesDoc::from_1o("{ features { feature { name \"x\" } feature { name \"x\" } } }").unwrap_err().to_string();
        assert!(e.contains("declared twice"), "{e}");
        let e = FeaturesDoc::from_1o("{ features { feature { name \"x\" dep \"nocolon\" } } }").unwrap_err().to_string();
        assert!(e.contains("is not `crate:dependency`"), "{e}");
        let e = FeaturesDoc::from_toml("[features.x]\ndep = [\"a:b\"]\n").unwrap_err().to_string();
        assert!(e.contains("[CP0010]") && e.contains("unknown field"), "{e}");
    }

    #[test]
    fn the_readme_table_is_the_schemas() {
        let readme = include_str!("../README.md");
        let table = schema_table();
        assert!(readme.contains(&table), "README.md must carry the table rendered from the schema:\n{table}");
    }
}

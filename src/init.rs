//! `init` and `agent init`.
//!
//! `init` writes the workspace features document at the workspace root (and
//! its schema where the 1o language server reads it): every crate's
//! features as its feature model has them, so
//! planning it changes nothing. A feature whose list is only `dep:`
//! entries becomes a workspace feature with those dependencies; any other
//! list is kept as the crate's own, verbatim (its `dep:` entries also named
//! under the workspace feature); an optional dependency no `dep:` entry
//! names is the crate's `optional`; a declared `default` list is the
//! crate's own default.
//!
//! `agent init` makes custodian_kit's agent layout (`.custodian/`: the
//! journal, receipts, jobs as folders queued → running → done) and writes
//! the job templates code-passenger runs (`crate::agent`).

use crate::cargo::enable_text;
use crate::doc::{self, CrateLines, Feature, FeaturesDoc, Twin};
use crate::error::{PassengerError, Result};
use crate::gate::workspace_root;
use klepto::{FeatureEnable, Klepto};
use std::path::{Path, PathBuf};

/// The document that round-trips to no change: the current feature models.
pub fn seed(k: &Klepto) -> FeaturesDoc {
    let mut d = FeaturesDoc::default();
    for (pkg, model) in &k.features {
        let mut lines = CrateLines::default();
        let mut named_by_dep: Vec<String> = Vec::new();
        for (name, info) in model.features.iter().filter(|(_, i)| !i.implicit) {
            if name == "default" {
                lines.default = Some(info.enables.iter().map(enable_text).collect());
                continue;
            }
            let deps: Vec<&String> = info
                .enables
                .iter()
                .filter_map(|e| match e {
                    FeatureEnable::Dep(dep) if model.dep(dep).is_some_and(|x| x.optional) => Some(dep),
                    _ => None,
                })
                .collect();
            named_by_dep.extend(deps.iter().map(|s| (*s).clone()));
            let only_deps = !info.enables.is_empty() && deps.len() == info.enables.len();
            if !deps.is_empty() {
                let f = d.features.entry(name.clone()).or_insert_with(Feature::default);
                for dep in &deps {
                    f.deps.push(format!("{pkg}:{dep}"));
                }
            }
            if !only_deps {
                lines.features.insert(name.clone(), info.enables.iter().map(enable_text).collect());
            }
        }
        let mut seen = std::collections::BTreeSet::new();
        for dep in model.dependencies.iter().filter(|x| x.optional) {
            if !named_by_dep.contains(&dep.name) && seen.insert(dep.name.clone()) {
                lines.optional.push(dep.name.clone());
            }
        }
        if lines != CrateLines::default() {
            d.crates.insert(pkg.clone(), lines);
        }
    }
    d
}

/// What `init` wrote.
#[derive(Debug, Clone)]
pub struct Written {
    /// The features document.
    pub doc: PathBuf,
    /// The schema's copy, where core-1o's workspace reads it.
    pub schema: PathBuf,
    /// `oneo.project.json`, listing the schema folder.
    pub project: PathBuf,
}

/// `init [--toml]`: the seeded document written at the workspace root, and
/// its schema where the 1o language server finds it (custodian's way: a copy
/// under the tool's folder, listed in `oneo.project.json`); an existing
/// document (either twin) is refused.
pub fn init(root: &Path, twin: Twin) -> Result<Written> {
    let root = workspace_root(root)?;
    for name in [doc::DOC_1O, doc::DOC_TOML] {
        let p = root.join(name);
        if p.exists() {
            return Err(PassengerError::FeaturesDocExists { detail: p.display().to_string() });
        }
    }
    let k = crate::commands::klepto_for(&root)?;
    let d = seed(&k);
    let (name, text) = match twin {
        Twin::OneO => (doc::DOC_1O, d.to_1o()?),
        Twin::Toml => (doc::DOC_TOML, d.to_toml()?),
    };
    let path = root.join(name);
    std::fs::write(&path, text)?;
    let store = crate::passenger::PassengerStore::at(&root)?;
    let schema = store.path(&crate::passenger::store::named::SCHEMA, &[("modifier", doc::MODIFIER), ("version", doc::SCHEMA_VERSION)])?;
    if let Some(d) = schema.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(&schema, doc::SCHEMA_V1)?;
    let project = custodian_kit::oneo::declare_schema_dir(&store.root, crate::passenger::store::SCHEMA_DIR)?;
    Ok(Written { doc: path, schema, project })
}

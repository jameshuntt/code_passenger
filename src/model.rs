use klepto::{UseSite, UseSitesCount};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ManifestInfo {
    pub crate_name: String,
    /// feature -> raw members (strings) as in Cargo.toml
    pub features_raw: BTreeMap<String, Vec<String>>,
    /// all dependency keys (normalized: '-' -> '_')
    pub all_deps: BTreeSet<String>,
    /// optional dependency keys (as used in code: '-' normalized to '_')
    pub optional_deps: BTreeSet<String>,
    /// feature -> enabled deps (subset of optional_deps), normalized to code idents
    pub feature_deps: BTreeMap<String, BTreeSet<String>>,
    /// dep -> features that enable it
    pub dep_features: BTreeMap<String, BTreeSet<String>>,
}

impl ManifestInfo {
    pub fn empty() -> Self {
        Self::default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentDetails {
    pub filename: String,
    /// path relative to the scanned directory (`src/net/http.rs` from a
    /// package root, `app/src/lib.rs` from a workspace root)
    pub relative_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UsedSymbols {
    pub packages: std::collections::BTreeSet<String>, // deps like tokio, regex, serde_json
    pub modules: std::collections::BTreeSet<String>,  // optional (crate::foo, std::fs, etc.)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureNote {
    pub feature_name: String,
    /// The dependencies the feature turns on that this file uses.
    pub deps: Vec<String>,
    /// The units of this file behind the feature: `file <path>`,
    /// `module <key>`, `function <id>`, `item <id>`.
    pub scope: Vec<String>,
    /// Every item of the file is behind the feature: the file's `#![cfg]`.
    pub corpus: bool,
    pub used_in_file: bool,
    /// The file's items unavailable with the feature off.
    #[serde(default)]
    pub unavailable: Vec<String>,
    /// Callers in other crates that lose items of this file.
    #[serde(default)]
    pub lost_by: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileReport {
    pub document: DocumentDetails,
    pub used: UsedSymbols,
    pub notes: Vec<FeatureNote>,
    pub corpus_features: Vec<String>,

    pub external_use_sites: Vec<UseSite>,
    pub internal_use_sites: Vec<UseSite>,

    pub external_dep_symbol_counts: UseSitesCount,
    pub internal_dep_symbol_counts: UseSitesCount,

}

#[derive(Debug, Clone, Copy)]
pub enum PlanSection {
    DocumentDetails,
    FeatureNotes,
    CorpusGates,
    Custom(&'static str),
}



#[derive(Debug, Clone, Copy)]
pub enum ScaffoldKind {
    Module,
    Component,
    Service,
    Test,
}

impl ScaffoldKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "module" => Some(Self::Module),
            "component" => Some(Self::Component),
            "service" => Some(Self::Service),
            "test" => Some(Self::Test),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScaffoldRequest {
    pub kind: ScaffoldKind,
    pub name: String,                 // e.g. "thread_pool_manager"
    pub module_path: Vec<String>,      // e.g. ["net", "http"]
}


#[derive(Debug, Clone)]
pub struct ScaffoldOutput {
    pub files: Vec<(std::path::PathBuf, String)>, // path + content
}
impl ScaffoldOutput {
    pub fn unsupported() -> Self { Self { files: vec![] } }
}


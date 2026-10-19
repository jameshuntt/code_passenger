//! What code_passenger reads of a package's manifest, and where it finds it.
//!
//! Nothing here parses a `Cargo.toml`: the features, the dependencies and
//! which features enable which optional dependency are klepto's feature model
//! (cargo metadata underneath); the packages and their manifest paths are
//! klepto's `PackageInfo`. The only manifest code_passenger touches as TOML is
//! the one it edits (`gate.rs`, through `toml_edit`).

use crate::error::{PassengerError, Result};
use crate::model::ManifestInfo;
use klepto::{FeatureEnable, FeatureModel, Klepto, KleptoBuilder, PackageInfo};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// A feature entry as Cargo spells it: `other`, `dep:beta`, `beta/feat`, `beta?/feat`.
pub fn enable_text(e: &FeatureEnable) -> String {
    match e {
        FeatureEnable::Feature(f) => f.clone(),
        FeatureEnable::Dep(d) => format!("dep:{d}"),
        FeatureEnable::DepFeature { dep, feature, weak } => format!("{dep}{}/{feature}", if *weak { "?" } else { "" }),
    }
}

/// One package's manifest facts, from its feature model.
pub fn manifest_info(model: &FeatureModel) -> ManifestInfo {
    let mut features_raw: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut feature_deps: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (name, info) in model.features.iter().filter(|(_, i)| !i.implicit) {
        features_raw.insert(name.clone(), info.enables.iter().map(enable_text).collect());
        feature_deps.insert(name.clone(), model.deps_enabled_by(name));
    }
    let mut dep_features: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (feat, deps) in &feature_deps {
        for dep in deps {
            dep_features.entry(dep.clone()).or_default().insert(feat.clone());
        }
    }
    ManifestInfo {
        crate_name: model.package.clone(),
        features_raw,
        all_deps: model.dependencies.iter().map(|d| d.name.clone()).collect(),
        optional_deps: model.dependencies.iter().filter(|d| d.optional).map(|d| d.name.clone()).collect(),
        feature_deps,
        dep_features,
    }
}

/// The builder over every workspace member of `root`, generated files left
/// out: what every command scans (cargo metadata has run; nothing is parsed).
pub fn builder(root: &Path) -> Result<KleptoBuilder> {
    Klepto::new("workspace")
        .scan_workspace_root(root)
        .map_err(|e| PassengerError::unsupported(e.to_string()))?
        .exclude_generated()
        .map_err(|e| PassengerError::unsupported(e.to_string()))
}

fn same_file(a: &Path, b: &Path) -> bool {
    a == b || std::fs::canonicalize(a).ok().is_some_and(|ca| std::fs::canonicalize(b).ok().is_some_and(|cb| ca == cb))
}

/// The package whose manifest is `manifest`, among `packages`.
pub fn package_with_manifest<'p>(packages: &'p BTreeMap<String, PackageInfo>, manifest: &Path) -> Option<&'p PackageInfo> {
    packages.values().find(|p| same_file(&p.manifest_path, manifest))
}

/// The manifest of `package` (`-` or `_` spelled), from cargo metadata.
pub fn manifest_of(packages: &BTreeMap<String, PackageInfo>, package: &str) -> Option<PathBuf> {
    packages.get(&package.replace('-', "_")).map(|p| p.manifest_path.clone())
}

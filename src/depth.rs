//! With a feature off: what goes, how deep it sits, and who in other crates
//! loses what. Computed on the planned tree (the tree with the plan's edits
//! written into a scratch copy, or the tree itself when the plan is empty),
//! so a plan's report and a header describe the gates as they will be.
//!
//! - The unavailable items of crate P with feature F off are klepto's
//!   `affected_by_feature(P, F)`: the items whose own cfg cannot hold
//!   without F and the items using a dependency F turns off.
//! - Their units (crate, whole file, whole module, function, item) are
//!   custodian_kit's scope (`reading::scope::unavailable`): a whole file is
//!   a file every item of which is unavailable, by the module tree.
//! - Each use site of an optional dependency F turns on is labelled with
//!   the unit that holds it.
//! - The callers in other crates are `affected_by_feature(P, F).transitive`
//!   outside P, each with the items of P it reaches that go.

use crate::error::{PassengerError, Result};
use crate::gate::{scratch_copy, EditSet};
use custodian_kit::reading::facts::Facts;
use custodian_kit::reading::scope::{self, Unavailable};
use custodian_kit::reading::spec::Depth;
use klepto::{FeatureModel, Klepto, Receipt};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// One use site of a dependency the feature turns off, and the unit holding it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SiteUnit {
    /// Relative to the workspace root.
    pub file: String,
    /// The line in the tree as it is (before the plan's edits).
    pub line: Option<u32>,
    pub dep: String,
    /// The enclosing function, else the module.
    pub scope: String,
    /// `crate`, `file`, `module`, `function` or `item`.
    pub unit: String,
    /// The unit's name: the crate, the file, the module key or the item id.
    pub unit_name: String,
}

/// An item of another crate that loses items of this one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Caller {
    pub caller: String,
    pub package: String,
    pub loses: Vec<String>,
}

/// One workspace feature off, in one crate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureOff {
    pub feature: String,
    pub package: String,
    /// The optional dependencies the feature turns on in this crate (klepto's `deps_enabled_by`).
    pub deps: Vec<String>,
    /// The units, their depth and the files wholly or partly behind it.
    pub scope: Unavailable,
    pub sites: Vec<SiteUnit>,
    /// From `affected_by_feature(..).transitive`, outside this crate.
    pub callers_elsewhere: Vec<Caller>,
}

impl FeatureOff {
    pub fn depth(&self) -> &'static str {
        self.scope.depth.map(Depth::as_str).unwrap_or("none")
    }

    pub fn files_wholly(&self) -> Vec<&str> {
        self.scope.files.iter().filter(|p| p.wholly).map(|p| p.name.as_str()).collect()
    }

    /// Whether a file (relative) is wholly behind the feature.
    pub fn whole_file(&self, file: &str) -> bool {
        self.scope.files.iter().any(|p| p.wholly && p.name == file)
    }
}

/// Per edited file (relative), each planned line's line in the tree as it
/// is (`None` for a line the plan adds): the plan only adds and replaces
/// attribute lines, so a code line keeps its place in the diff.
fn line_maps(root: &Path, edits: &EditSet) -> BTreeMap<String, Vec<Option<u32>>> {
    let mut out = BTreeMap::new();
    for e in &edits.files {
        let Ok(rel) = crate::gate::relative(root, &e.path) else { continue };
        let diff = similar::TextDiff::from_lines(&e.before, &e.after);
        let mut map: Vec<Option<u32>> = vec![None; e.after.lines().count() + 1];
        for op in diff.ops() {
            if let similar::DiffOp::Equal { old_index, new_index, len } = *op {
                for i in 0..len {
                    if let Some(slot) = map.get_mut(new_index + i) {
                        *slot = Some((old_index + i + 1) as u32);
                    }
                }
            }
        }
        out.insert(rel.to_string_lossy().replace('\\', "/"), map);
    }
    out
}

/// The unit a site's carrier sits in.
fn unit_of(u: &Unavailable, package: &str, file: &str, scope_id: &str, module: Option<&str>) -> (String, String) {
    if u.units.iter().any(|x| x.depth == Depth::Crate) {
        return ("crate".into(), package.to_string());
    }
    if u.units.iter().any(|x| x.depth == Depth::File && x.name == file) {
        return ("file".into(), file.to_string());
    }
    if let Some(m) = module
        && let Some(x) = u.units.iter().find(|x| x.depth == Depth::Module && (m == x.name || m.starts_with(&format!("{}::", x.name)))) {
            return ("module".into(), x.name.clone());
        }
    match u.units.iter().find(|x| x.name == scope_id) {
        Some(x) => (x.depth.as_str().to_string(), x.name.clone()),
        // a module-level use (its scope is the module) or an item nested in a gated one
        None => ("item".into(), scope_id.to_string()),
    }
}

/// Each feature off, in every crate whose desired model declares it (its
/// own features, not `default` and not Cargo's implicit ones); a feature
/// that leaves nothing unavailable is left out. `desired` is every crate's
/// desired model; `edits` the plan's edits (empty: the tree is the planned tree).
pub fn feature_off(root: &Path, desired: &BTreeMap<String, FeatureModel>, k: &Klepto, edits: &EditSet) -> Result<Vec<FeatureOff>> {
    let wanted: Vec<(String, String)> = desired
        .iter()
        .flat_map(|(p, m)| m.features.iter().filter(|(f, i)| !i.implicit && f.as_str() != "default").map(move |(f, _)| (p.clone(), f.clone())))
        .collect();
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    // the planned tree
    let scratch;
    let planned_root;
    let owned;
    let k2: &Klepto = if edits.is_empty() {
        planned_root = root.canonicalize()?;
        k
    } else {
        scratch = scratch_copy(root)?;
        edits.write_to(root, scratch.path())?;
        planned_root = scratch.path().canonicalize()?;
        owned = crate::commands::klepto_for(&planned_root)?;
        &owned
    };
    let receipt = Receipt::from_klepto(k2);
    let lines = line_maps(root, edits);
    struct Pending {
        package: String,
        feature: String,
        deps: Vec<String>,
        direct: BTreeSet<String>,
        transitive: Vec<String>,
    }
    let mut pending = Vec::new();
    for (p, f) in wanted {
        let a = k2.affected_by_feature(&p, &f);
        let deps: Vec<String> = k2.features.get(&p).map(|m| m.deps_enabled_by(&f).into_iter().collect()).unwrap_or_default();
        let direct: BTreeSet<String> = a.gated.iter().chain(a.uses_lost_dep.iter()).cloned().collect();
        pending.push(Pending { package: p, feature: f, deps, direct, transitive: a.transitive });
    }
    let graph = k2.graph.clone();
    let modules = k2.modules.clone();
    let k_owned: Klepto = k2.clone();
    let facts = Facts::from_klepto(planned_root.clone(), k_owned, custodian_kit::api::Resolving::Free).map_err(PassengerError::from)?;
    let mut out = Vec::new();
    for w in pending {
        if w.direct.is_empty() {
            continue;
        }
        let u = scope::unavailable(&facts, &w.package, &w.feature, &w.direct);
        let gone: BTreeSet<&String> = u.items.iter().collect();
        let mut sites = Vec::new();
        for d in receipt.dep_uses.iter().filter(|d| d.package == w.package && w.deps.contains(&d.site.dep)) {
            let file = facts.rel(&d.site.location.path);
            let module = modules.for_file(&d.site.location.path).map(|m| m.path.clone()).or_else(|| facts.item(&d.site.scope).map(|i| i.module.clone()));
            let (unit, unit_name) = unit_of(&u, &w.package, &file, &d.site.scope, module.as_deref());
            // the line in the tree as it is
            let line = match (lines.get(&file), d.site.location.line) {
                (Some(map), Some(l)) => map.get(l as usize - 1).copied().flatten().or(Some(l)),
                (_, l) => l,
            };
            sites.push(SiteUnit { file, line, dep: d.site.dep.clone(), scope: d.site.scope.clone(), unit, unit_name });
        }
        sites.sort_by(|a, b| (&a.file, a.line, &a.dep).cmp(&(&b.file, b.line, &b.dep)));
        sites.dedup();
        let mut callers = Vec::new();
        for y in &w.transitive {
            let Some(pkg) = graph.node(y).and_then(|n| n.package.clone()) else { continue };
            if pkg == w.package {
                continue;
            }
            let loses: Vec<String> = graph.transitive_dependencies(y).into_iter().filter(|d| gone.contains(d)).collect();
            if !loses.is_empty() {
                callers.push(Caller { caller: y.clone(), package: pkg, loses });
            }
        }
        callers.sort_by(|a, b| a.caller.cmp(&b.caller));
        out.push(FeatureOff { feature: w.feature, package: w.package, deps: w.deps, scope: u, sites, callers_elsewhere: callers });
    }
    out.sort_by(|a, b| (&a.feature, &a.package).cmp(&(&b.feature, &b.package)));
    Ok(out)
}

/// The lines a report carries: per feature and crate, the depth, the
/// units, the use sites with their unit, and the callers elsewhere.
pub fn render(off: &[FeatureOff]) -> String {
    let mut s = String::new();
    for o in off {
        s.push_str(&format!("with {} off in {} (depth: {}):\n", o.feature, o.package, o.depth()));
        s.push_str(&format!("  unavailable: {}\n", o.scope.items.join(", ")));
        let whole = o.files_wholly();
        if !whole.is_empty() {
            s.push_str(&format!("  files wholly gated: {}\n", whole.join(", ")));
        }
        for x in &o.scope.units {
            s.push_str(&format!("  unit {} {} ({} item(s))\n", x.depth.as_str(), x.name, x.items));
        }
        for site in &o.sites {
            let line = site.line.map(|l| l.to_string()).unwrap_or_else(|| "?".into());
            s.push_str(&format!("  site {}:{line} {} in {} — {} {}\n", site.file, site.dep, site.scope, site.unit, site.unit_name));
        }
        for c in &o.callers_elsewhere {
            s.push_str(&format!("  {} ({}) loses {}\n", c.caller, c.package, c.loses.join(", ")));
        }
    }
    s
}

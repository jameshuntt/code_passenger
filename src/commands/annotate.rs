//! `annotate`: a header per file whose content is the features plan's.
//!
//! The plan is the workspace features document's (or, without one, the
//! document `init` would seed from the current feature models, which plans
//! to nothing). A header describes gates the code has, so a plan with work
//! left is refused (`features apply --write` first). For each file and each
//! feature with an item of the file behind it, the header's note names the
//! dependencies the file uses that the feature turns on, the units of the
//! file behind it, the items unavailable with it off and the callers in
//! other crates that lose them; a file wholly behind a feature carries the
//! feature as a corpus `#![cfg(...)]` (the plan's `select_gates`).

use crate::depth::FeatureOff;
use crate::engine::{run_scan_on, RunContext};
use crate::error::{PassengerError, Result};
use crate::gate::{relative, workspace_root};
use crate::model::{FeatureNote, FileReport};
use crate::{doc, packs, plans};
use klepto::Klepto;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct AnnotateOptions {
    pub write: bool,
    pub check: bool,
    /// `verbose` or `compact`.
    pub plan: String,
    pub lang: String,
}

impl Default for AnnotateOptions {
    fn default() -> Self {
        Self { write: false, check: false, plan: "verbose".into(), lang: "rust".into() }
    }
}

#[derive(Debug)]
pub struct AnnotateOutcome {
    pub root: PathBuf,
    /// Every file (relative) and whether its header changes.
    pub files: Vec<(String, bool)>,
    /// The headers as rendered, by file.
    pub headers: BTreeMap<String, String>,
    pub written: bool,
    pub exit_code: i32,
}

fn rel(root: &Path, p: &Path) -> String {
    relative(root, p).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_else(|_| p.display().to_string())
}

/// The notes of one file, from the plan's features-off lists.
pub fn notes_for(root: &Path, k: &Klepto, report: &FileReport, package: Option<&str>, off: &[FeatureOff]) -> Vec<FeatureNote> {
    let file = &report.document.relative_path;
    let item_file: BTreeMap<&str, String> = k.items.iter().map(|d| (d.id.as_str(), rel(root, &d.location.path))).collect();
    let mut notes = Vec::new();
    for o in off.iter().filter(|o| Some(o.package.as_str()) == package) {
        let here: Vec<String> = o.scope.items.iter().filter(|id| item_file.get(id.as_str()) == Some(file)).cloned().collect();
        if here.is_empty() {
            continue;
        }
        let here_set: BTreeSet<&String> = here.iter().collect();
        let deps: Vec<String> = o.deps.iter().filter(|d| report.used.packages.contains(*d)).cloned().collect();
        let mut scope = Vec::new();
        for u in &o.scope.units {
            let touches = match u.depth.as_str() {
                "crate" => true,
                "file" => &u.name == file,
                "module" => k.items.iter().any(|d| (d.module == u.name || d.module.starts_with(&format!("{}::", u.name))) && item_file.get(d.id.as_str()) == Some(file)),
                _ => here_set.contains(&u.name),
            };
            if touches {
                scope.push(format!("{} {}", u.depth.as_str(), u.name));
            }
        }
        let lost_by: Vec<String> = o
            .callers_elsewhere
            .iter()
            .filter_map(|c| {
                let l: Vec<&String> = c.loses.iter().filter(|x| here_set.contains(x)).collect();
                (!l.is_empty()).then(|| format!("{} ({}) loses {}", c.caller, c.package, l.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")))
            })
            .collect();
        notes.push(FeatureNote { feature_name: o.feature.clone(), deps, scope, corpus: o.whole_file(file), used_in_file: true, unavailable: here, lost_by });
    }
    notes
}

/// The document planned for headers: the root's, or the seeded one.
fn doc_at(root: &Path, k: &Klepto) -> Result<(doc::FeaturesDoc, Option<PathBuf>)> {
    Ok(match doc::find(root)? {
        Some(f) => (f.doc, Some(f.path)),
        None => (crate::init::seed(k), None),
    })
}

/// `annotate [--write] [--check]` over every file of the workspace.
pub fn run(root: &Path, opts: &AnnotateOptions) -> Result<AnnotateOutcome> {
    let root = workspace_root(root)?.canonicalize()?;
    let k = crate::commands::klepto_for(&root)?;
    let (d, doc_path) = doc_at(&root, &k)?;
    let wplan = crate::workspace::plan_static(&root, &d, doc_path, &k)?;
    if !wplan.is_empty() {
        return Err(PassengerError::PlanNotApplied { detail: format!("{} gate(s), {} manifest(s) to go", wplan.gates(), wplan.manifests()) });
    }
    let pack = packs::get_pack(&opts.lang)?;
    let plan = plans::get_plan(&opts.plan)?;
    let select = plans::get_plan_v2(&opts.plan)?;
    let ctx = RunContext { root: root.clone(), src_rel: String::new(), manifest_path: root.join("Cargo.toml"), lang: opts.lang.clone() };
    let scan = run_scan_on(&ctx, &k)?;
    let mut files = Vec::new();
    let mut headers = BTreeMap::new();
    let mut changed_any = false;
    let mut writes: Vec<(PathBuf, String)> = Vec::new();
    for mut r in scan.reports {
        let abs = root.join(&r.document.relative_path);
        let package = k.modules.for_file(&abs).map(|m| m.package.clone()).or_else(|| {
            let c = abs.canonicalize().ok()?;
            k.modules.modules.values().find(|m| !m.inline && m.file.canonicalize().ok().as_ref() == Some(&c)).map(|m| m.package.clone())
        });
        r.notes = notes_for(&root, &k, &r, package.as_deref(), &wplan.off);
        r.corpus_features = select.select_gates(&r.notes);
        let header = pack.render_header(plan.as_ref(), &r);
        let original = std::fs::read_to_string(&abs)?;
        let updated = pack.apply_header(&original, &header);
        let changed = updated != original;
        changed_any |= changed;
        if changed {
            writes.push((abs.clone(), updated));
        }
        headers.insert(r.document.relative_path.clone(), header);
        files.push((r.document.relative_path.clone(), changed));
    }
    let written = opts.write && !opts.check && changed_any;
    if written {
        for (p, text) in &writes {
            std::fs::write(p, text)?;
        }
    }
    let exit_code = if opts.check && changed_any { 1 } else { 0 };
    Ok(AnnotateOutcome { root, files, headers, written, exit_code })
}

pub fn render(out: &AnnotateOutcome) -> String {
    let mut s = String::from("CODE PASSENGER :: ANNOTATE\n──────────────────────────\n");
    s.push_str(&format!("root: {}\n", out.root.display()));
    for (f, changed) in &out.files {
        s.push_str(&format!("  {} {f}\n", if *changed { "changes" } else { "same   " }));
    }
    s.push_str(&format!("written: {}\nexit: {}\n", if out.written { "yes" } else { "no" }, out.exit_code));
    s
}

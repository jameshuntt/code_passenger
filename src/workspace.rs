//! `features plan | apply | check`: the workspace features document planned
//! for every crate in one run.
//!
//! 1. Each crate's klepto spec (`PackageSpec`) is derived from the document
//!    ([`derive`]): a workspace feature's `crate:dependency` entries make the
//!    dependency optional behind the feature in that crate; a crate's own
//!    lines replace a list verbatim; the workspace default turns its features
//!    on in every crate that has them.
//! 2. klepto plans each crate's gates against its desired model.
//! 3. The cross-crate cascade ([`cascade`]): an item of crate P left gated
//!    behind workspace features is used by items of crate Q (klepto's item
//!    graph, Reference / ReExport / Import edges, workspace members only).
//!    Q's same-named features forward to P's (`p/log`), and Q's users are
//!    gated with the same condition: an item through klepto's forced gates
//!    (its own cascade then runs inside Q), a module-level `use` as a `use`
//!    gate. Repeated until nothing new is gated or forwarded.
//! 4. The cargo check: every crate the plan touches, and every crate using
//!    one, is checked with every feature combination klepto's matrix names,
//!    on one scratch copy holding the whole workspace's edits. A compiler
//!    error klepto maps to an item becomes a forced gate and the workspace
//!    is planned again; one it cannot map is for a person.
//! 5. With the feature off ([`crate::depth`]): the items unavailable, the
//!    units they make up and the callers in other crates that lose them,
//!    computed on the planned tree.
//!
//! Refusals (exit 2): a carrier klepto cannot judge, a compiler error no
//! gate answers, and a write the cargo check did not prove unless
//! `--unproven` says so. `check` exits 1 while there is work and never
//! writes. `apply --write` writes every crate's edits and checkpoints the
//! tree before and after into the passenger store.

use crate::cargo::enable_text;
use crate::depth::{self, FeatureOff};
use crate::doc::{self, split_dep, FeaturesDoc};
use crate::error::{PassengerError, Result};
use crate::gate::{apply_with, relative, workspace_root, EditSet, FileEdit};
use crate::oracle::CargoOracle;
use crate::words::PlanWord;
use klepto::graph::NodeKind;
use klepto::{CfgPredicate, CheckJob, ConvergeStatus, EdgeKind, FeatureModel, ForcedGate, GateEdit, GateKind, GatePlan, GateReason, Implication, Klepto, ManifestEdit, MissedGate, PackageSpec, UnknownGate};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub const PLAN_SCHEMA: &str = "code_passenger.features_plan.v1";

/// Which `features` line runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    /// Plan and prove; never writes.
    Plan,
    /// Plan, prove and (with `--write`) write and checkpoint.
    Apply,
    /// Exit 1 while there is work; never writes, never runs cargo.
    Check,
}

impl Action {
    pub fn word(self) -> &'static str {
        match self {
            Action::Plan => "plan",
            Action::Apply => "apply",
            Action::Check => "check",
        }
    }

    pub fn of_word(w: &str) -> Option<Action> {
        [Action::Plan, Action::Apply, Action::Check].into_iter().find(|a| a.word() == w)
    }
}

#[derive(Debug, Clone)]
pub struct FeaturesOptions {
    pub word: PlanWord,
    pub max_rounds: usize,
    /// Where the plan is written as JSON (a job step's reading).
    pub out: Option<PathBuf>,
    /// A prior step's receipt: apply takes the plan it names instead of planning again.
    pub after: Option<PathBuf>,
}

impl Default for FeaturesOptions {
    fn default() -> Self {
        Self { word: PlanWord::from_raw(0), max_rounds: 3, out: None, after: None }
    }
}

/// One crate's part of the plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackagePlan {
    pub package: String,
    pub manifest_path: PathBuf,
    /// The klepto spec derived from the document (after the cascade's forwards).
    pub spec: PackageSpec,
    pub plan: GatePlan,
}

/// A use across crates the cascade followed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrossLink {
    /// The gated item, in `from_package`.
    pub from: String,
    pub from_package: String,
    /// The user in another crate (an item, or the module of a `use`).
    pub user: String,
    pub package: String,
    /// The condition the user needs.
    pub needs: String,
    /// `item` (a forced gate), `use` (a use gate) or `already` (it holds already).
    pub how: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobResult {
    pub label: String,
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsRound {
    pub jobs: Vec<JobResult>,
    pub missed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsConvergence {
    pub status: ConvergeStatus,
    pub rounds: Vec<WsRound>,
    /// `(package, gate)` the compiler insisted on.
    pub forced: Vec<(String, ForcedGate)>,
    /// `(package, diagnostic)` no gate answers: a person decides.
    pub human: Vec<(String, MissedGate)>,
}

/// The whole workspace's plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspacePlan {
    pub schema: String,
    pub root: PathBuf,
    /// The document planned (its path), or `None` for the document seeded from the current models.
    pub doc: Option<PathBuf>,
    /// klepto's fingerprint of the tree the plan was made on.
    pub fingerprint: String,
    pub packages: Vec<PackagePlan>,
    pub cross: Vec<CrossLink>,
    pub convergence: Option<WsConvergence>,
    /// With each workspace feature off, per crate.
    pub off: Vec<FeatureOff>,
}

impl WorkspacePlan {
    pub fn is_empty(&self) -> bool {
        self.packages.iter().all(|p| p.plan.is_empty())
    }

    pub fn unknown(&self) -> Vec<(&str, &UnknownGate)> {
        self.packages.iter().flat_map(|p| p.plan.unknown.iter().map(move |u| (p.package.as_str(), u))).collect()
    }

    pub fn gates(&self) -> usize {
        self.packages.iter().map(|p| p.plan.gates.len()).sum()
    }

    pub fn manifests(&self) -> usize {
        self.packages.iter().filter(|p| !p.plan.manifest.is_empty()).count()
    }
}

#[derive(Debug)]
pub struct FeaturesOutcome {
    pub action: Action,
    pub word: PlanWord,
    pub plan: WorkspacePlan,
    pub edits: EditSet,
    pub written: bool,
    /// The passenger snapshots taken: before and after the write.
    pub checkpoints: Vec<String>,
    /// The refusal behind an exit 2, rendered with its code.
    pub refused: Option<String>,
    pub exit_code: i32,
}

// ---------------------------------------------------------------------------
// 1. the spec of every crate, from the document

/// The packages of the scan: every workspace member with a feature model.
fn members(k: &Klepto) -> Vec<String> {
    k.features.keys().cloned().collect()
}

fn feature_list(model: &FeatureModel, name: &str) -> Option<Vec<String>> {
    model.features.get(name).filter(|i| !i.implicit).map(|i| i.enables.iter().map(enable_text).collect())
}

/// Every crate's spec, before the cascade.
pub fn derive(doc: &FeaturesDoc, k: &Klepto) -> Result<BTreeMap<String, PackageSpec>> {
    let names = members(k);
    let mut specs: BTreeMap<String, PackageSpec> = names.iter().map(|n| (n.clone(), PackageSpec::default())).collect();
    for (feature, f) in &doc.features {
        for entry in &f.deps {
            let (krate, dep) = split_dep(entry)?;
            let model = k.features.get(&krate).ok_or_else(|| PassengerError::UnknownName { detail: format!("feature `{feature}`: `{entry}`: no workspace member `{krate}` (members: {})", names.join(", ")) })?;
            if model.dep(&dep).is_none() {
                return Err(PassengerError::UnknownName { detail: format!("feature `{feature}`: `{entry}`: `{krate}` declares no dependency `{dep}`") });
            }
            let spec = specs.get_mut(&krate).expect("a member");
            spec.optional.insert(dep.clone());
            let list = spec.features.entry(feature.clone()).or_default();
            let want = format!("dep:{dep}");
            if !list.contains(&want) {
                list.push(want);
            }
        }
    }
    for (krate, lines) in &doc.crates {
        let key = krate.replace('-', "_");
        let model = k.features.get(&key).ok_or_else(|| PassengerError::UnknownName { detail: format!("crate `{krate}`: no workspace member of that name (members: {})", names.join(", ")) })?;
        let spec = specs.get_mut(&key).expect("a member");
        for o in &lines.optional {
            let o = o.replace('-', "_");
            if model.dep(&o).is_none() {
                return Err(PassengerError::UnknownName { detail: format!("crate `{krate}`: optional `{o}`: no such dependency") });
            }
            spec.optional.insert(o);
        }
        for (fname, list) in &lines.features {
            // the crate's own list, verbatim; the workspace's dependencies kept in it
            let derived = spec.features.get(fname).cloned().unwrap_or_default();
            let mut verbatim = list.clone();
            for d in derived {
                if !verbatim.contains(&d) {
                    verbatim.push(d);
                }
            }
            spec.features.insert(fname.clone(), verbatim);
        }
        if let Some(def) = &lines.default {
            spec.default = Some(def.clone());
        }
    }
    Ok(specs)
}

/// The workspace default resolved per crate (crates with their own default keep it).
fn resolve_defaults(doc: &FeaturesDoc, k: &Klepto, specs: &mut BTreeMap<String, PackageSpec>) {
    let Some(ws_default) = &doc.default else { return };
    for (name, spec) in specs.iter_mut() {
        if doc.crates.get(name).is_some_and(|c| c.default.is_some()) {
            continue;
        }
        let Some(model) = k.features.get(name) else { continue };
        let current = feature_list(model, "default").unwrap_or_default();
        let mut def: Vec<String> = current.iter().filter(|e| !doc.features.contains_key(e.as_str())).cloned().collect();
        for f in ws_default {
            let has = spec.features.contains_key(f) || model.features.get(f).is_some_and(|i| !i.implicit);
            if has && !def.contains(f) {
                def.push(f.clone());
            }
        }
        spec.default = Some(def);
    }
}

// ---------------------------------------------------------------------------
// 2–3. per-crate plans and the cross-crate cascade

#[derive(Default)]
struct Work {
    specs: BTreeMap<String, PackageSpec>,
    forced: BTreeMap<String, Vec<ForcedGate>>,
    uses: BTreeMap<String, Vec<GateEdit>>,
    cross: Vec<CrossLink>,
}

struct Planned {
    desired: BTreeMap<String, FeatureModel>,
    plans: BTreeMap<String, GatePlan>,
}

/// A gate's condition without the features that only reach another one it
/// names: `any(feature = "default", feature = "log")` where `default`
/// turns on `log` is `feature = "log"` (cargo turns `log` on with
/// `default`, so the two are the same condition).
pub fn minimize(p: CfgPredicate, m: &FeatureModel) -> CfgPredicate {
    match p {
        CfgPredicate::Any(v) => {
            let v: Vec<CfgPredicate> = v.into_iter().map(|x| minimize(x, m)).collect();
            let feats: Vec<String> = v.iter().filter_map(|x| if let CfgPredicate::Feature(f) = x { Some(f.clone()) } else { None }).collect();
            let reaches = |a: &str, b: &str| m.active_features(&[a], false).contains(b);
            let keep: Vec<CfgPredicate> = v
                .into_iter()
                .filter(|x| match x {
                    CfgPredicate::Feature(f) => !feats.iter().any(|g| g != f && reaches(f, g) && (!reaches(g, f) || g < f)),
                    _ => true,
                })
                .collect();
            CfgPredicate::Any(keep).simplify()
        }
        CfgPredicate::All(v) => CfgPredicate::All(v.into_iter().map(|x| minimize(x, m)).collect()).simplify(),
        CfgPredicate::Not(b) => CfgPredicate::Not(Box::new(minimize(*b, m))),
        other => other,
    }
}

fn plan_all(k: &Klepto, work: &Work) -> Planned {
    let mut desired = BTreeMap::new();
    let mut plans = BTreeMap::new();
    for (name, current) in &k.features {
        let spec = work.specs.get(name).cloned().unwrap_or_default();
        let d = current.apply_spec(&spec);
        let mut plan = k.plan_gates_with(name, &d, work.forced.get(name).map(Vec::as_slice).unwrap_or(&[]));
        for g in &mut plan.gates {
            g.predicate = minimize(g.predicate.clone(), &d);
        }
        for u in work.uses.get(name).into_iter().flatten() {
            if !plan.gates.iter().any(|g| g.kind == GateKind::Use && g.location == u.location) {
                plan.gates.push(u.clone());
            }
        }
        plan.gates.sort_by(|a, b| (&a.location.path, a.location.line, a.location.column).cmp(&(&b.location.path, b.location.line, b.location.column)));
        desired.insert(name.clone(), d);
        plans.insert(name.clone(), plan);
    }
    Planned { desired, plans }
}

/// An item's cfg once its crate's plan is applied.
fn effective(k: &Klepto, plan: Option<&GatePlan>, id: &str) -> Option<CfgPredicate> {
    let node = k.graph.node(id)?;
    let gate = plan.and_then(|p| p.gates.iter().find(|g| g.kind == GateKind::Item && g.item == id));
    Some(match gate {
        Some(g) => g.existing_cfg.clone().and(g.predicate.clone()).simplify(),
        None => node.cfg.clone(),
    })
}

/// Q's name for its dependency on workspace member P.
fn dep_name_on(q: &FeatureModel, p_cargo: &str) -> Option<String> {
    q.dependencies.iter().find(|d| d.package.replace('-', "_") == p_cargo.replace('-', "_")).map(|d| d.name.clone())
}

/// One round of the cross-crate cascade; whether anything new was added.
fn cascade(k: &Klepto, doc_features: &BTreeSet<String>, work: &mut Work, planned: &Planned) -> bool {
    let mut changed = false;
    for (p, plan_p) in &planned.plans {
        let p_cargo = k.packages.get(p).map(|i| i.cargo_name.clone()).unwrap_or_else(|| p.clone());
        let nodes: Vec<&klepto::ItemNode> = k.graph.nodes.values().filter(|n| n.package.as_deref() == Some(p.as_str()) && n.kind != NodeKind::Module && n.kind != NodeKind::External).collect();
        for x in nodes {
            let Some(eff_x) = effective(k, Some(plan_p), &x.id) else { continue };
            let feats = eff_x.features();
            if feats.is_empty() || eff_x.is_unknown() || !feats.iter().all(|f| doc_features.contains(f)) {
                continue;
            }
            for e in k.graph.edges.iter().filter(|e| e.to == x.id && matches!(e.kind, EdgeKind::Reference | EdgeKind::ReExport | EdgeKind::Import)) {
                let Some(user) = k.graph.node(&e.from) else { continue };
                let Some(q) = user.package.clone() else { continue };
                if &q == p || !k.features.contains_key(&q) {
                    continue;
                }
                let Some(q_model) = k.features.get(&q) else { continue };
                let Some(dep) = dep_name_on(q_model, &p_cargo) else { continue };
                // Q's features of the same names forward to P's
                for f in &feats {
                    let spec = work.specs.entry(q.clone()).or_default();
                    let list = spec.features.entry(f.clone()).or_insert_with(|| feature_list(q_model, f).unwrap_or_default());
                    let forward = format!("{dep}/{f}");
                    if !list.contains(&forward) {
                        list.push(forward);
                        changed = true;
                    }
                }
                let desired_q = q_model.apply_spec(work.specs.get(&q).expect("just set"));
                let needs = eff_x.clone().simplify();
                let module_level = user.kind == NodeKind::Module;
                let have = if module_level { e.cfg.clone() } else { effective(k, planned.plans.get(&q), &user.id).unwrap_or_else(|| user.cfg.clone()) };
                let holds = !have.is_unknown() && desired_q.closed_implies(&have, &needs) == Implication::Always;
                let how = if holds {
                    "already"
                } else if module_level {
                    let uses = work.uses.entry(q.clone()).or_default();
                    if !uses.iter().any(|u| u.location == e.location) {
                        uses.push(GateEdit { item: user.id.clone(), kind: GateKind::Use, location: e.location.clone(), existing_cfg: e.cfg.clone(), predicate: needs.clone(), reasons: vec![GateReason::Cascade { from: x.id.clone() }] });
                        changed = true;
                    }
                    "use"
                } else {
                    let forced = work.forced.entry(q.clone()).or_default();
                    if !forced.iter().any(|g| g.item == user.id && g.predicate == needs) {
                        forced.push(ForcedGate { item: user.id.clone(), predicate: needs.clone(), reason: GateReason::Cascade { from: x.id.clone() } });
                        changed = true;
                    }
                    "item"
                };
                let link = CrossLink { from: x.id.clone(), from_package: p.clone(), user: user.id.clone(), package: q.clone(), needs: needs.render(), how: how.to_string() };
                if let Some(old) = work.cross.iter_mut().find(|c| c.from == link.from && c.user == link.user) {
                    // the first verdict is the one the tree had; a later round sees the gate it added
                    if old.how == "already" && link.how != "already" {
                        *old = link;
                    }
                } else {
                    work.cross.push(link);
                }
            }
        }
    }
    changed
}

/// The cascade to its fixpoint, the defaults resolved: every crate's plan.
fn plan_to_fixpoint(doc: &FeaturesDoc, k: &Klepto, work: &mut Work) -> Planned {
    let doc_features: BTreeSet<String> = doc.features.keys().cloned().collect();
    resolve_defaults(doc, k, &mut work.specs);
    let mut planned = plan_all(k, work);
    // bounded: every round adds a forward, a forced gate or a use gate, all finite
    for _ in 0..64 {
        if !cascade(k, &doc_features, work, &planned) {
            break;
        }
        resolve_defaults(doc, k, &mut work.specs);
        planned = plan_all(k, work);
    }
    work.cross.sort_by(|a, b| (&a.from, &a.user).cmp(&(&b.from, &b.user)));
    planned
}

fn package_plans(k: &Klepto, work: &Work, planned: &Planned) -> Vec<PackagePlan> {
    planned
        .plans
        .iter()
        .map(|(name, plan)| PackagePlan {
            package: name.clone(),
            manifest_path: k.packages.get(name).map(|p| p.manifest_path.clone()).unwrap_or_default(),
            spec: work.specs.get(name).cloned().unwrap_or_default(),
            plan: plan.clone(),
        })
        .collect()
}

/// Every crate's edits as one set: source files and manifests together.
pub fn edits_of(packages: &[PackagePlan]) -> Result<EditSet> {
    let mut all = EditSet::default();
    for p in packages.iter().filter(|p| !p.plan.is_empty()) {
        let manifest = if p.plan.manifest.is_empty() { None } else { Some(p.manifest_path.as_path()) };
        let set = apply_with(&p.plan, manifest)?;
        for e in set.files.into_iter().chain(set.manifest) {
            if all.files.iter().any(|f: &FileEdit| f.path == e.path) {
                return Err(PassengerError::PersonDecides { detail: format!("two crates' plans edit {}", e.path.display()) });
            }
            all.files.push(e);
        }
    }
    all.files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(all)
}

// ---------------------------------------------------------------------------
// 4. the cargo check across the workspace

fn depends_on(k: &Klepto, q: &str, p: &str) -> bool {
    let Some(p_cargo) = k.packages.get(p).map(|i| i.cargo_name.clone()) else { return false };
    k.features.get(q).is_some_and(|m| m.dependencies.iter().any(|d| d.kind == klepto::DepKind::Normal && d.package.replace('-', "_") == p_cargo.replace('-', "_")))
}

/// The crates to check: every crate the plan touches, and every crate using one.
fn check_set(k: &Klepto, planned: &Planned) -> Vec<String> {
    let touched: BTreeSet<&String> = planned.plans.iter().filter(|(_, p)| !p.is_empty()).map(|(n, _)| n).collect();
    let mut out: BTreeSet<String> = touched.iter().map(|s| (*s).clone()).collect();
    for q in k.features.keys() {
        if touched.iter().any(|p| depends_on(k, q, p)) {
            out.insert(q.clone());
        }
    }
    out.into_iter().collect()
}

fn package_of_file(k: &Klepto, root: &Path, file: Option<&PathBuf>) -> Option<String> {
    let f = file?;
    let abs = std::fs::canonicalize(root.join(f)).ok()?;
    k.modules.modules.values().filter(|m| std::fs::canonicalize(&m.file).ok().as_ref() == Some(&abs)).map(|m| m.package.clone()).next()
}

fn converge(root: &Path, doc: &FeaturesDoc, k: &Klepto, base: &Work, max_rounds: usize) -> Result<(Work, Planned, WsConvergence)> {
    let mut oracle = CargoOracle::new(root);
    let mut learned: BTreeMap<String, Vec<ForcedGate>> = BTreeMap::new();
    let mut human: Vec<(String, MissedGate)> = Vec::new();
    let mut rounds = Vec::new();
    loop {
        let mut work = Work { specs: base.specs.clone(), forced: base.forced.clone(), uses: base.uses.clone(), cross: base.cross.clone() };
        for (p, gates) in &learned {
            work.forced.entry(p.clone()).or_default().extend(gates.iter().cloned());
        }
        let planned = plan_to_fixpoint(doc, k, &mut work);
        let edits = edits_of(&package_plans(k, &work, &planned))?;
        let mut jobs = Vec::new();
        let mut missed: Vec<(String, MissedGate)> = Vec::new();
        for p in check_set(k, &planned) {
            let Some(desired) = planned.desired.get(&p) else { continue };
            for job in klepto::check_matrix(desired) {
                let res = oracle.check_edits(&job, &edits);
                for d in &res.diagnostics {
                    let owner = package_of_file(k, root, d.file.as_ref()).unwrap_or_else(|| p.clone());
                    let model = planned.desired.get(&owner).unwrap_or(desired);
                    missed.push((owner.clone(), k.map_diagnostic(root, &owner, model, d)));
                }
                jobs.push((job, res.ok));
            }
        }
        let all_ok = jobs.iter().all(|(_, ok)| *ok);
        rounds.push(WsRound { jobs: jobs.iter().map(|(j, ok): &(CheckJob, bool)| JobResult { label: j.label(), ok: *ok }).collect(), missed: missed.len() });
        let forced_all = |learned: &BTreeMap<String, Vec<ForcedGate>>| learned.iter().flat_map(|(p, gs)| gs.iter().map(move |g| (p.clone(), g.clone()))).collect::<Vec<_>>();
        if all_ok {
            let conv = WsConvergence { status: ConvergeStatus::Converged, rounds, forced: forced_all(&learned), human };
            return Ok((work, planned, conv));
        }
        let mut added = false;
        for (owner, m) in missed {
            let (Some(item), Some(p)) = (&m.item, &m.predicate) else {
                if !human.iter().any(|(o, h)| o == &owner && h.diagnostic == m.diagnostic) {
                    human.push((owner, m));
                }
                continue;
            };
            let already_forced = learned.get(&owner).is_some_and(|gs| gs.iter().any(|g| &g.item == item && &g.predicate == p));
            let model = planned.desired.get(&owner);
            let already_planned = planned.plans.get(&owner).and_then(|pl| pl.gate_for(item)).is_some_and(|g| {
                model.is_some_and(|m| m.closed_implies(&g.existing_cfg.clone().and(g.predicate.clone()).simplify(), p) == Implication::Always)
            });
            if already_forced || already_planned {
                continue;
            }
            learned.entry(owner).or_default().push(ForcedGate {
                item: item.clone(),
                predicate: p.clone(),
                reason: GateReason::Forced { code: m.diagnostic.code.clone().unwrap_or_default(), mention: m.diagnostic.mention.clone().unwrap_or_default() },
            });
            added = true;
        }
        if !added || rounds.len() >= max_rounds.max(1) {
            let conv = WsConvergence { status: ConvergeStatus::GaveUp, rounds, forced: forced_all(&learned), human };
            return Ok((work, planned, conv));
        }
    }
}

// ---------------------------------------------------------------------------
// the plan, the run

fn fingerprint(root: &Path) -> Result<String> {
    Ok(crate::cargo::builder(root)?.fingerprint()?.digest)
}

fn assemble(root: &Path, doc_path: Option<PathBuf>, k: &Klepto, work: Work, planned: &Planned, convergence: Option<WsConvergence>) -> Result<WorkspacePlan> {
    let packages = package_plans(k, &work, planned);
    let edits = edits_of(&packages)?;
    let off = depth::feature_off(root, &planned.desired, k, &edits)?;
    Ok(WorkspacePlan { schema: PLAN_SCHEMA.into(), root: root.to_path_buf(), doc: doc_path, fingerprint: fingerprint(root)?, packages, cross: work.cross, convergence, off })
}

/// The workspace planned statically (no cargo): every crate's plan after the
/// cascade, and with each feature off. `doc_path` is `None` for a seeded document.
pub fn plan_static(root: &Path, doc: &FeaturesDoc, doc_path: Option<PathBuf>, k: &Klepto) -> Result<WorkspacePlan> {
    let mut work = Work { specs: derive(doc, k)?, ..Default::default() };
    let planned = plan_to_fixpoint(doc, k, &mut work);
    assemble(root, doc_path, k, work, &planned, None)
}

/// The workspace planned and proven: the static plan, then the cargo check
/// rounds; a plan with nothing to do needs no check.
pub fn plan_proven(root: &Path, doc: &FeaturesDoc, doc_path: Option<PathBuf>, k: &Klepto, max_rounds: usize) -> Result<WorkspacePlan> {
    let base = Work { specs: derive(doc, k)?, ..Default::default() };
    let mut probe = Work { specs: base.specs.clone(), ..Default::default() };
    let planned = plan_to_fixpoint(doc, k, &mut probe);
    if planned.plans.values().all(|p| p.is_empty()) {
        return assemble(root, doc_path, k, probe, &planned, None);
    }
    let (work, planned, conv) = converge(root, doc, k, &base, max_rounds)?;
    assemble(root, doc_path, k, work, &planned, Some(conv))
}

fn exit_of(action: Action, plan: &WorkspacePlan) -> (i32, Option<String>) {
    if action == Action::Check {
        return (if plan.is_empty() { 0 } else { 1 }, None);
    }
    let unknown = plan.unknown();
    if !unknown.is_empty() {
        let what: Vec<String> = unknown.iter().map(|(p, u)| format!("{p}: {} ({})", u.item, u.why)).collect();
        return (2, Some(PassengerError::PersonDecides { detail: format!("klepto cannot judge {}", what.join("; ")) }.to_string()));
    }
    if let Some(c) = &plan.convergence {
        if !c.human.is_empty() {
            let what: Vec<String> = c.human.iter().map(|(p, h)| format!("{p}: {} {}", h.diagnostic.code.as_deref().unwrap_or("-"), h.diagnostic.message)).collect();
            return (2, Some(PassengerError::PersonDecides { detail: format!("compiler errors no gate answers: {}", what.join("; ")) }.to_string()));
        }
        if c.status == ConvergeStatus::GaveUp {
            return (1, None);
        }
    }
    (0, None)
}

fn load_prior_plan(after: &Path, root: &Path) -> Result<WorkspacePlan> {
    let text = std::fs::read_to_string(after).map_err(|e| PassengerError::PriorReceipt { detail: format!("{}: {e}", after.display()) })?;
    let r: custodian_kit::store::Receipt = serde_json::from_str(&text).map_err(|e| PassengerError::PriorReceipt { detail: format!("{}: {e}", after.display()) })?;
    if !r.accepted {
        return Err(PassengerError::PriorReceipt { detail: format!("run #{} ({}) was not accepted: {}", r.seq, r.line, r.reason) });
    }
    let plan_file = r.outputs.iter().find(|o| o.ends_with(".json")).ok_or_else(|| PassengerError::PriorReceipt { detail: format!("run #{} names no plan", r.seq) })?;
    let p = if Path::new(plan_file).is_absolute() { PathBuf::from(plan_file) } else { root.join(plan_file) };
    let text = std::fs::read_to_string(&p).map_err(|e| PassengerError::PriorReceipt { detail: format!("{}: {e}", p.display()) })?;
    let plan: WorkspacePlan = serde_json::from_str(&text).map_err(|e| PassengerError::PriorReceipt { detail: format!("{}: {e}", p.display()) })?;
    if plan.schema != PLAN_SCHEMA {
        return Err(PassengerError::PriorReceipt { detail: format!("{} is not a {PLAN_SCHEMA}", p.display()) });
    }
    Ok(plan)
}

/// `features plan | apply | check` at `root` (a member directory planns its whole workspace).
pub fn run(root: &Path, action: Action, opts: &FeaturesOptions) -> Result<FeaturesOutcome> {
    let word = opts.word.checked()?;
    if word.writes() && action != Action::Apply {
        return Err(PassengerError::PlanFlags { detail: format!("`features {}` never writes; `features apply --write` does", action.word()) });
    }
    let root = workspace_root(root)?;
    let found = doc::require(&root)?;

    let plan = match (&opts.after, action) {
        (Some(after), Action::Apply) => {
            let plan = load_prior_plan(after, &root)?;
            let now = fingerprint(&root)?;
            if plan.fingerprint != now {
                return Err(PassengerError::StalePlan { detail: format!("planned on {}, the tree is {now}", plan.fingerprint) });
            }
            if plan.convergence.is_none() && !plan.is_empty() && word.writes() && !(word.skips_cargo() && word.has_plan_unproven()) {
                return Err(PassengerError::Unproven);
            }
            plan
        }
        (Some(_), _) => return Err(PassengerError::PlanFlags { detail: "--after is for `features apply`".into() }),
        (None, _) => {
            let k = crate::commands::klepto_for(&root)?;
            let prove = action != Action::Check && !word.skips_cargo() && !word.checks_only();
            if prove {
                plan_proven(&root, &found.doc, Some(found.path.clone()), &k, opts.max_rounds)?
            } else {
                plan_static(&root, &found.doc, Some(found.path.clone()), &k)?
            }
        }
    };

    let edits = edits_of(&plan.packages)?;
    let (exit_code, refused) = exit_of(action, &plan);
    if let Some(out) = &opts.out {
        if let Some(d) = out.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(out, serde_json::to_string_pretty(&plan)? + "\n")?;
    }
    let mut checkpoints = Vec::new();
    let can_write = action == Action::Apply && word.writes() && !plan.is_empty() && exit_code == 0;
    if can_write {
        let store = crate::passenger::PassengerStore::open_or_init(&root)?;
        let before = store.checkpoint_tree(&format!("before features apply ({} gates, {} manifests)", plan.gates(), plan.manifests()), None)?;
        checkpoints.push(before.id);
        edits.write()?;
        let after = store.checkpoint_tree(&format!("features apply: {} gates, {} manifests", plan.gates(), plan.manifests()), Some(&plan))?;
        checkpoints.push(after.id);
    }
    Ok(FeaturesOutcome { action, word, plan, edits, written: can_write, checkpoints, refused, exit_code })
}

// ---------------------------------------------------------------------------
// the report

fn reason_text(r: &GateReason) -> String {
    match r {
        GateReason::UngatedDep { dep } => format!("ungated dep {dep}"),
        GateReason::ExistingViolation { to } => format!("references gated {to}"),
        GateReason::Cascade { from } => format!("cascade from {from}"),
        GateReason::Forced { code, mention } => format!("forced by {code} on {mention}"),
    }
}

fn manifest_text(e: &ManifestEdit) -> String {
    match e {
        ManifestEdit::MakeOptional { dep } => format!("optional {dep}"),
        ManifestEdit::SetFeature { name, enables } => format!("feature {name} = {enables:?}"),
        ManifestEdit::SetDefault { features } => format!("default = {features:?}"),
    }
}

/// The human rendering.
pub fn render(out: &FeaturesOutcome) -> String {
    let root = &out.plan.root;
    let rel = |p: &Path| relative(root, p).map(|r| r.to_string_lossy().into_owned()).unwrap_or_else(|_| p.display().to_string());
    let mut s = format!("CODE PASSENGER :: FEATURES {}\n─────────────────────────────\n", out.action.word().to_uppercase());
    s.push_str(&format!("root: {}\n", root.display()));
    s.push_str(&format!("document: {}\n", out.plan.doc.as_deref().map(rel).unwrap_or_else(|| "(seeded from the current feature models)".into())));
    s.push_str(&format!("flags: [{}]\n", out.word.names().join(", ")));
    if out.plan.is_empty() {
        s.push_str("plan: nothing to do\n");
    }
    for p in out.plan.packages.iter().filter(|p| !p.plan.is_empty()) {
        s.push_str(&format!("CRATE {}\n", p.package));
        if !p.plan.manifest.is_empty() {
            s.push_str("  manifest:\n");
            for e in &p.plan.manifest {
                s.push_str(&format!("    {}\n", manifest_text(e)));
            }
        }
        if !p.plan.gates.is_empty() {
            s.push_str("  gates:\n");
            for g in &p.plan.gates {
                let line = g.location.line.map(|l| l.to_string()).unwrap_or_else(|| "?".into());
                let reasons: Vec<String> = g.reasons.iter().map(reason_text).collect();
                let attr = if g.existing_cfg == CfgPredicate::True { g.attribute_text() } else { crate::gate::merged_attribute(&g.existing_cfg, &g.predicate) };
                s.push_str(&format!("    {}:{}  {}  {}  ({})\n", rel(&g.location.path), line, attr, g.item, reasons.join("; ")));
            }
        }
        if !p.plan.reaches_public.is_empty() {
            s.push_str(&format!("  reaches_public: {}\n", p.plan.reaches_public.join(", ")));
        }
        for u in &p.plan.unknown {
            let line = u.location.line.map(|l| l.to_string()).unwrap_or_else(|| "?".into());
            s.push_str(&format!("  unknown (a person decides): {}:{}  {}  {}\n", rel(&u.location.path), line, u.item, u.why));
        }
    }
    if !out.plan.cross.is_empty() {
        s.push_str("across crates:\n");
        for c in &out.plan.cross {
            s.push_str(&format!("  {} ({}) uses {} ({}): needs {} — {}\n", c.user, c.package, c.from, c.from_package, c.needs, c.how));
        }
    }
    match &out.plan.convergence {
        None => s.push_str(&format!("cargo check: {}\n", if out.action == Action::Check || out.plan.is_empty() { "not needed" } else { "skipped (unproven)" })),
        Some(c) => {
            let status = match c.status {
                ConvergeStatus::Converged => "converged",
                ConvergeStatus::GaveUp => "gave up",
            };
            let jobs: usize = c.rounds.iter().map(|r| r.jobs.len()).sum();
            s.push_str(&format!("cargo check: {status} after {} round(s), {jobs} job(s), {} forced gate(s)\n", c.rounds.len(), c.forced.len()));
            for (p, h) in &c.human {
                s.push_str(&format!("  human: {p} {} {} {}\n", h.diagnostic.code.as_deref().unwrap_or("-"), h.item.as_deref().unwrap_or("?"), h.diagnostic.message));
            }
        }
    }
    s.push_str(&depth::render(&out.plan.off));
    if !out.edits.is_empty() {
        s.push_str("diff:\n");
        s.push_str(&out.edits.diff_from(root));
    }
    if let Some(r) = &out.refused {
        s.push_str(&format!("refused: {r}\n"));
    }
    if !out.checkpoints.is_empty() {
        s.push_str(&format!("passenger: {}\n", out.checkpoints.join(" → ")));
    }
    s.push_str(&format!("written: {}\nexit: {}\n", if out.written { "yes" } else { "no" }, out.exit_code));
    s
}

//! Workspace features: one document planned for every crate (the cross-crate
//! cascade, the cargo check across the workspace), the unavailable list, the
//! headers from the plan, init's round trip, the passenger history, agent
//! mode's job, and the declared lines. Fixture: tests/fixtures/ws_features/
//! (`store` and `app`, feature `log` spanning both, `logsink` the dependency
//! it turns off). The cargo tests run real cargo, offline.

mod common;

use code_passenger::commands::{annotate, features};
use code_passenger::doc::{self, FeaturesDoc, Twin};
use code_passenger::error::PassengerError;
use code_passenger::gate::EditSet;
use code_passenger::oracle::CargoOracle;
use code_passenger::passenger::PassengerStore;
use code_passenger::words::PlanWord;
use code_passenger::workspace::{self, Action, FeaturesOptions};
use common::*;
use klepto::{ConvergeStatus, GateReason, ManifestEdit};
use std::ffi::OsString;
use std::path::Path;

const STORE_LIB: &str = "store/src/lib.rs";
const STORE_SINK: &str = "store/src/sink.rs";
const STORE_PLAIN: &str = "store/src/plain.rs";
const APP_LIB: &str = "app/src/lib.rs";

fn opts(write: bool, no_cargo: bool, unproven: bool) -> FeaturesOptions {
    FeaturesOptions { word: PlanWord::of(write, false, no_cargo, unproven), ..Default::default() }
}

fn gates_of(plan: &workspace::WorkspacePlan, package: &str) -> Vec<(String, u32, String, Vec<GateReason>)> {
    let p = plan.packages.iter().find(|p| p.package == package).expect("the crate is planned");
    p.plan.gates.iter().map(|g| (g.item.clone(), g.location.line.unwrap_or(0), g.attribute_text(), g.reasons.clone())).collect()
}

fn manifest_of(plan: &workspace::WorkspacePlan, package: &str) -> Vec<ManifestEdit> {
    plan.packages.iter().find(|p| p.package == package).map(|p| p.plan.manifest.clone()).unwrap_or_default()
}

fn rel_files(root: &Path) -> Vec<String> {
    let mut v: Vec<String> = walk(root).into_iter().map(|p| p.strip_prefix(root).unwrap().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        if name == "target" || name == "Cargo.lock" {
            continue;
        }
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

// ---------------------------------------------------------------- plan

#[test]
fn one_plan_for_every_crate_with_the_cascade_across_crates_proven_by_cargo() {
    let copy = fresh_copy("ws_features");
    let root = copy.path();
    let before = read(&root.join(APP_LIB));
    let out = workspace::run(root, Action::Plan, &FeaturesOptions::default()).expect("plan");

    assert_eq!(
        manifest_of(&out.plan, "store"),
        vec![
            ManifestEdit::MakeOptional { dep: "logsink".into() },
            ManifestEdit::SetFeature { name: "log".into(), enables: vec!["dep:logsink".into()] },
            ManifestEdit::SetDefault { features: vec!["log".into()] },
        ]
    );
    assert_eq!(
        manifest_of(&out.plan, "app"),
        vec![
            ManifestEdit::MakeOptional { dep: "logsink".into() },
            ManifestEdit::SetFeature { name: "log".into(), enables: vec!["dep:logsink".into(), "store/log".into()] },
            ManifestEdit::SetDefault { features: vec!["log".into()] },
        ],
        "app's log turns on store's"
    );
    let log = "#[cfg(feature = \"log\")]".to_string();
    assert_eq!(
        gates_of(&out.plan, "store"),
        vec![
            ("store::record".into(), 7, log.clone(), vec![GateReason::UngatedDep { dep: "logsink".into() }]),
            ("store::sink::flush".into(), 1, log.clone(), vec![GateReason::UngatedDep { dep: "logsink".into() }]),
            ("store::sink::flush_twice".into(), 6, log.clone(), vec![GateReason::Cascade { from: "store::sink::flush".into() }]),
        ]
    );
    assert_eq!(
        gates_of(&out.plan, "app"),
        vec![
            ("app::audit".into(), 4, log.clone(), vec![GateReason::Cascade { from: "store::record".into() }]),
            ("app::trace".into(), 9, log.clone(), vec![GateReason::UngatedDep { dep: "logsink".into() }]),
        ],
        "app::audit is gated because store::record is: the cascade crossed crates"
    );
    assert_eq!(out.plan.cross.len(), 1);
    assert_eq!((out.plan.cross[0].from.as_str(), out.plan.cross[0].user.as_str(), out.plan.cross[0].how.as_str()), ("store::record", "app::audit", "item"));

    let conv = out.plan.convergence.as_ref().expect("the cargo check ran");
    assert_eq!(conv.status, ConvergeStatus::Converged);
    assert_eq!(conv.rounds.len(), 1);
    let labels: Vec<&str> = conv.rounds[0].jobs.iter().map(|j| j.label.as_str()).collect();
    assert_eq!(labels, vec!["app[no-default]", "app[default]", "app[no-default+log]", "store[no-default]", "store[default]", "store[no-default+log]"]);
    assert!(conv.rounds[0].jobs.iter().all(|j| j.ok));
    assert!(conv.forced.is_empty() && conv.human.is_empty());
    assert_eq!(out.exit_code, 0);
    assert!(!out.written);
    assert_eq!(read(&root.join(APP_LIB)), before, "plan never writes");
    let text = workspace::render(&out);
    assert!(text.starts_with("CODE PASSENGER :: FEATURES PLAN\n"), "{text}");
    assert!(text.contains("app::audit (app) uses store::record (store): needs feature = \"log\" — item"), "{text}");
    assert!(text.contains("cargo check: converged after 1 round(s), 6 job(s), 0 forced gate(s)"), "{text}");
}

#[test]
fn without_the_cross_crate_gate_app_does_not_compile() {
    // without a workspace default, store's `log` is off when app's is: the gate on app::audit is what compiles
    let copy = fresh_copy("ws_features");
    let root = copy.path().to_path_buf();
    std::fs::write(root.join(doc::DOC_1O), "{\n  features {\n    feature {\n      name \"log\"\n      dep \"store:logsink\"\n      dep \"app:logsink\"\n    }\n  }\n}\n").unwrap();
    let k = code_passenger::commands::klepto_for(&root).unwrap();
    let found = doc::require(&root).unwrap();
    let plan = workspace::plan_static(&root, &found.doc, Some(found.path), &k).unwrap();
    assert!(plan.packages.iter().all(|p| !p.plan.manifest.iter().any(|m| matches!(m, ManifestEdit::SetDefault { .. }))), "no default in the document, no default edited");
    let job = klepto::CheckJob { package: "app".into(), features: vec![], no_default: true };
    let mut oracle = CargoOracle::new(&root);
    let full = workspace::edits_of(&plan.packages).unwrap();
    assert!(oracle.check_edits(&job, &full).ok, "the plan compiles with log off");
    let mut without = plan.packages.clone();
    for p in without.iter_mut().filter(|p| p.package == "app") {
        p.plan.gates.retain(|g| g.item != "app::audit");
    }
    let partial = workspace::edits_of(&without).unwrap();
    let bad = oracle.check_edits(&job, &partial);
    assert!(!bad.ok, "store::record is gone with log off, so an ungated app::audit cannot compile");
    assert!(bad.diagnostics.iter().any(|d| d.message.contains("record")), "{:?}", bad.diagnostics);
}

// ---------------------------------------------------------------- depth, the unavailable list

#[test]
fn the_unavailable_list_is_exact() {
    let root = fixture("ws_features");
    let k = code_passenger::commands::klepto_for(&root).unwrap();
    let found = doc::require(&root).unwrap();
    let plan = workspace::plan_static(&root, &found.doc, Some(found.path), &k).unwrap();
    let keys: Vec<(&str, &str)> = plan.off.iter().map(|o| (o.feature.as_str(), o.package.as_str())).collect();
    assert_eq!(keys, vec![("log", "app"), ("log", "store")]);

    let store = &plan.off[1];
    assert_eq!(store.depth(), "file");
    assert_eq!(store.deps, vec!["logsink".to_string()]);
    assert_eq!(store.scope.items, vec!["store::record".to_string(), "store::sink::flush".into(), "store::sink::flush_twice".into()]);
    let units: Vec<(&str, &str, usize)> = store.scope.units.iter().map(|u| (u.depth.as_str(), u.name.as_str(), u.items)).collect();
    assert_eq!(units, vec![("file", STORE_SINK, 2), ("function", "store::record", 1)]);
    assert_eq!(store.files_wholly(), vec![STORE_SINK]);
    let sites: Vec<(&str, Option<u32>, &str, &str, &str)> = store.sites.iter().map(|s| (s.file.as_str(), s.line, s.dep.as_str(), s.unit.as_str(), s.unit_name.as_str())).collect();
    assert_eq!(sites, vec![(STORE_LIB, Some(9), "logsink", "function", "store::record"), (STORE_SINK, Some(3), "logsink", "file", STORE_SINK)], "lines are the tree's as it is");
    let callers: Vec<(&str, &str, Vec<String>)> = store.callers_elsewhere.iter().map(|c| (c.caller.as_str(), c.package.as_str(), c.loses.clone())).collect();
    assert_eq!(callers, vec![("app::audit", "app", vec!["store::record".to_string()])], "the caller in the other crate, from affected_by_feature(..).transitive");

    let app = &plan.off[0];
    assert_eq!(app.depth(), "function");
    assert_eq!(app.scope.items, vec!["app::audit".to_string(), "app::trace".into()]);
    let units: Vec<(&str, &str)> = app.scope.units.iter().map(|u| (u.depth.as_str(), u.name.as_str())).collect();
    assert_eq!(units, vec![("function", "app::audit"), ("function", "app::trace")]);
    assert!(app.files_wholly().is_empty());
    let sites: Vec<(&str, Option<u32>, &str)> = app.sites.iter().map(|s| (s.file.as_str(), s.line, s.unit.as_str())).collect();
    assert_eq!(sites, vec![(APP_LIB, Some(11), "function")]);
    assert!(app.callers_elsewhere.is_empty());

    // feature-probe shows the same list, reviewing against the document
    let probe = features::feature_probe(&root, None, "log", None).expect("a document's feature is a feature");
    assert_eq!(probe.kind, "feature");
    assert_eq!(probe.off, plan.off);
    let proposed: Vec<&str> = probe.proposed.iter().map(|g| g.item.as_str()).collect();
    assert_eq!(proposed, vec!["app::audit", "app::trace", "store::record", "store::sink::flush", "store::sink::flush_twice"]);
    let text = features::render_probe(&probe);
    assert!(text.contains("with log off in store (depth: file):\n  unavailable: store::record, store::sink::flush, store::sink::flush_twice\n  files wholly gated: store/src/sink.rs\n"), "{text}");
    assert!(text.contains("  app::audit (app) loses store::record\n"), "{text}");
    assert!(text.ends_with("writes: false\n"));
}

// ---------------------------------------------------------------- apply, check, headers

const SINK_HEADER: &str = r#"//! code_passenger:begin
//! ----------------------------------------------
//! DOCUMENT DETAILS -----------------------------
//!
//! filename:sink.rs
//! description:
//! usages:["logsink"]
//!
//! ----------------------------------------------
//! FEATURE NOTES --------------------------------
//!
//! feature_name:log
//! deps:["logsink"]
//! scope:["file store/src/sink.rs"]
//! corpus:true
//! unavailable:["store::sink::flush", "store::sink::flush_twice"]
//!
//! ----------------------------------------------
//! CORPUS FEATURES ------------------------------
//!
#![cfg(feature = "log")]
//! code_passenger:end
"#;

const STORE_LIB_HEADER: &str = r#"//! code_passenger:begin
//! ----------------------------------------------
//! DOCUMENT DETAILS -----------------------------
//!
//! filename:lib.rs
//! description:
//! usages:["logsink"]
//!
//! ----------------------------------------------
//! FEATURE NOTES --------------------------------
//!
//! feature_name:log
//! deps:["logsink"]
//! scope:["function store::record"]
//! corpus:false
//! unavailable:["store::record"]
//! lost_by:["app::audit (app) loses store::record"]
//!
//! ----------------------------------------------
//! CORPUS FEATURES ------------------------------
//!
//! code_passenger:end
"#;

const APP_LIB_HEADER: &str = r#"//! code_passenger:begin
//! ----------------------------------------------
//! DOCUMENT DETAILS -----------------------------
//!
//! filename:lib.rs
//! description:
//! usages:["logsink", "store"]
//!
//! ----------------------------------------------
//! FEATURE NOTES --------------------------------
//!
//! feature_name:log
//! deps:["logsink"]
//! scope:["function app::audit", "function app::trace"]
//! corpus:false
//! unavailable:["app::audit", "app::trace"]
//!
//! ----------------------------------------------
//! CORPUS FEATURES ------------------------------
//!
//! code_passenger:end
"#;

const PLAIN_HEADER: &str = r#"//! code_passenger:begin
//! ----------------------------------------------
//! DOCUMENT DETAILS -----------------------------
//!
//! filename:plain.rs
//! description:
//! usages:none
//!
//! ----------------------------------------------
//! FEATURE NOTES --------------------------------
//!
//! ----------------------------------------------
//! CORPUS FEATURES ------------------------------
//!
//! code_passenger:end
"#;

#[test]
fn apply_writes_every_crate_check_is_idempotent_and_the_headers_are_the_plans() {
    let copy = fresh_copy("ws_features");
    let root = copy.path();

    // annotate before the plan is applied: refused, the headers would describe gates the code lacks
    let e = annotate::run(root, &annotate::AnnotateOptions::default()).unwrap_err();
    assert!(matches!(e, PassengerError::PlanNotApplied { .. }), "{e}");
    assert!(e.to_string().starts_with("[CP0023]"), "{e}");

    let check = workspace::run(root, Action::Check, &FeaturesOptions::default()).unwrap();
    assert_eq!(check.exit_code, 1, "check: work to do");
    assert!(check.plan.convergence.is_none(), "check never runs cargo");

    let out = workspace::run(root, Action::Apply, &opts(true, false, false)).expect("apply --write");
    assert!(out.written);
    assert_eq!(out.exit_code, 0);
    assert_eq!(out.checkpoints, vec!["S000001".to_string(), "S000002".into()]);
    assert_eq!(out.plan.convergence.as_ref().unwrap().status, ConvergeStatus::Converged);
    let app = read(&root.join(APP_LIB));
    assert!(app.contains("#[cfg(feature = \"log\")]\npub fn audit(x: u32)"), "{app}");
    assert!(app.contains("#[cfg(feature = \"log\")]\npub fn trace()"), "{app}");
    assert!(!app.contains("#[cfg(feature = \"log\")]\npub fn run()"));
    let app_manifest = read(&root.join("app/Cargo.toml"));
    assert!(app_manifest.contains("logsink = { path = \"../deps/logsink\" , optional = true }"), "{app_manifest}");
    assert!(app_manifest.contains("log = [\"dep:logsink\", \"store/log\"]"), "{app_manifest}");
    assert!(app_manifest.contains("default = [\"log\"]"), "{app_manifest}");
    assert!(read(&root.join(STORE_SINK)).contains("#[cfg(feature = \"log\")]\npub fn flush_twice()"));

    // idempotent: nothing left to plan, and a second write writes nothing
    let again = workspace::run(root, Action::Check, &FeaturesOptions::default()).unwrap();
    assert!(again.plan.is_empty(), "{:?}", again.plan.packages.iter().map(|p| &p.plan).collect::<Vec<_>>());
    assert_eq!(again.exit_code, 0);
    assert!(workspace::render(&again).contains("plan: nothing to do"));
    let twice = workspace::run(root, Action::Apply, &opts(true, false, false)).unwrap();
    assert!(!twice.written && twice.checkpoints.is_empty() && twice.plan.convergence.is_none());

    // the headers, from the same plan
    let c = annotate::run(root, &annotate::AnnotateOptions { check: true, ..Default::default() }).unwrap();
    assert_eq!(c.exit_code, 1);
    let w = annotate::run(root, &annotate::AnnotateOptions { write: true, ..Default::default() }).unwrap();
    assert!(w.written);
    assert_eq!(w.headers[STORE_SINK], SINK_HEADER);
    assert_eq!(w.headers[STORE_LIB], STORE_LIB_HEADER);
    assert_eq!(w.headers[APP_LIB], APP_LIB_HEADER);
    assert_eq!(w.headers[STORE_PLAIN], PLAIN_HEADER);
    assert!(read(&root.join(STORE_SINK)).starts_with(SINK_HEADER), "the corpus #![cfg] sits in the file's header");
    let c = annotate::run(root, &annotate::AnnotateOptions { check: true, ..Default::default() }).unwrap();
    assert_eq!(c.exit_code, 0, "--check after --write: nothing changes");
    assert_eq!(workspace::run(root, Action::Check, &FeaturesOptions::default()).unwrap().exit_code, 0, "the headers' #![cfg] changes no plan");

    // the written, annotated workspace compiles in every combination the matrix names
    let mut oracle = CargoOracle::new(root);
    for package in ["store", "app"] {
        for (no_default, features) in [(true, vec![]), (false, vec![]), (true, vec!["log".to_string()])] {
            let job = klepto::CheckJob { package: package.into(), features, no_default };
            let r = oracle.check_edits(&job, &EditSet::default());
            assert!(r.ok, "{}: {:?}", job.label(), r.diagnostics);
        }
    }
}

// ---------------------------------------------------------------- refusals

#[test]
fn refusals_carry_their_codes() {
    let copy = fresh_copy("ws_features");
    let root = copy.path();
    let e = workspace::run(root, Action::Apply, &opts(true, true, false)).unwrap_err();
    assert!(matches!(e, PassengerError::Unproven) && e.exit_code() == 2, "{e}");
    assert!(e.to_string().starts_with("[CP0021]"), "{e}");
    assert!(!read(&root.join(APP_LIB)).contains("#[cfg("), "nothing written");

    // said on purpose, an unproven plan is written
    let out = workspace::run(root, Action::Apply, &opts(true, true, true)).unwrap();
    assert!(out.written && out.plan.convergence.is_none());

    let e = workspace::run(root, Action::Plan, &opts(true, false, false)).unwrap_err();
    assert!(matches!(e, PassengerError::PlanFlags { .. }) && e.to_string().contains("never writes"), "{e}");

    // a plan read from a prior receipt is refused once the code changed since it was made
    let staled = fresh_copy("ws_features");
    let plan_file = staled.path().join("plan.json");
    workspace::run(staled.path(), Action::Plan, &FeaturesOptions { word: PlanWord::of(false, false, true, false), out: Some(plan_file.clone()), ..Default::default() }).unwrap();
    let receipt = serde_json::json!({
        "schema": "custodian.receipt.v1", "seq": 1, "utc": "2026-10-19T00:00:00Z", "command": "code-passenger", "name": "plan",
        "spec": null, "spec_hash": null, "code_digest": null, "index_key": null, "plan_id": "p", "plan_digest": "d", "operation_digest": "d",
        "receipt_digest": "d", "line": "code-passenger features plan", "exit_code": 0, "accepted": true, "disposition": "Continue", "reason": "ok",
        "outputs": [plan_file.to_string_lossy()], "job": null, "reused_seq": null, "millis": 0
    });
    let receipt_file = staled.path().join("1.json");
    std::fs::write(&receipt_file, receipt.to_string()).unwrap();
    std::fs::write(staled.path().join(STORE_PLAIN), "pub fn add(a: u32, b: u32) -> u32 { b + a }\n").unwrap();
    let after = FeaturesOptions { word: PlanWord::of(true, false, true, true), after: Some(receipt_file), ..Default::default() };
    let e = workspace::run(staled.path(), Action::Apply, &after).unwrap_err();
    assert!(matches!(e, PassengerError::StalePlan { .. }) && e.to_string().starts_with("[CP0025]"), "{e}");

    // the gate line keeps 0.2.0's loop and takes the same plan word
    let gate = code_passenger::lines::run_argv(&["gate", "--spec", "klepto.toml", "--write", "--no-cargo"].map(OsString::from), Path::new("code-passenger"));
    assert_eq!(gate.code, 2);
    assert!(gate.stderr.contains("[CP0021]"), "{}", gate.stderr);

    let other = fresh_copy("ws_features");
    std::fs::write(other.path().join(doc::DOC_1O), "{\n  features {\n    feature {\n      name \"log\"\n      dep \"nosuch:logsink\"\n    }\n  }\n}\n").unwrap();
    let e = workspace::run(other.path(), Action::Plan, &FeaturesOptions::default()).unwrap_err();
    assert!(matches!(e, PassengerError::UnknownName { .. }) && e.to_string().starts_with("[CP0014]"), "{e}");
    std::fs::write(other.path().join(doc::DOC_TOML), "").unwrap();
    let e = workspace::run(other.path(), Action::Plan, &FeaturesOptions::default()).unwrap_err();
    assert!(matches!(e, PassengerError::TwoFeaturesDocs { .. }), "{e}");
}

// ---------------------------------------------------------------- init

#[test]
fn init_writes_the_current_models_and_planning_it_changes_nothing() {
    // the fixture before the feature exists, and after it is applied
    let pristine = fresh_copy("ws_features");
    let applied = fresh_copy("ws_features");
    workspace::run(applied.path(), Action::Apply, &opts(true, true, true)).unwrap();
    for (copy, expect_crates) in [(pristine, 0usize), (applied, 2)] {
        let root = copy.path();
        std::fs::rename(root.join(doc::DOC_1O), root.join("features.1o.was")).unwrap();
        let before = rel_files(root);
        let w = code_passenger::init::init(root, Twin::OneO).expect("init");
        let mut after = rel_files(root);
        after.retain(|f| !before.contains(f));
        assert_eq!(after, vec![".passenger/config/schema/features/v1.schema.json".to_string(), "oneo.project.json".into(), doc::DOC_1O.into()], "init writes the document and its schema's place, nothing else");
        assert_eq!(read(&w.schema), doc::SCHEMA_V1);
        let seeded = FeaturesDoc::from_1o(&read(&w.doc)).unwrap();
        assert_eq!(seeded.crates.len(), expect_crates);
        let check = workspace::run(root, Action::Check, &FeaturesOptions::default()).unwrap();
        assert!(check.plan.is_empty(), "round trip: {:?}", check.plan.packages.iter().map(|p| &p.plan).collect::<Vec<_>>());
        assert_eq!(check.exit_code, 0);
        let e = code_passenger::init::init(root, Twin::Toml).unwrap_err();
        assert!(matches!(e, PassengerError::FeaturesDocExists { .. }), "{e}");

        // the language server's view: core-1o's workspace finds the schema and checks the document live
        let ws = core_1o::engine::Workspace::new(root);
        assert!(ws.schema_registry.get(doc::MODIFIER, doc::SCHEMA_VERSION).is_some(), "oneo.project.json lists the schema folder");
        assert!(core_1o::engine::diagnostics_for_text_workspace(&ws, doc::MODIFIER, doc::SCHEMA_VERSION, &read(&w.doc)).is_empty());
        let bad = core_1o::engine::diagnostics_for_text_workspace(&ws, doc::MODIFIER, doc::SCHEMA_VERSION, "{ features { feature { name \"x\" deps \"a:b\" } } }");
        assert!(bad.iter().any(|d| d.message.contains("Unknown key 'deps'")), "{:?}", bad.iter().map(|d| &d.message).collect::<Vec<_>>());
    }

    // the TOML twin of the applied tree reads into the same model
    let one = fresh_copy("ws_features");
    let two = fresh_copy("ws_features");
    for c in [&one, &two] {
        workspace::run(c.path(), Action::Apply, &opts(true, true, true)).unwrap();
        std::fs::rename(c.path().join(doc::DOC_1O), c.path().join("features.1o.was")).unwrap();
    }
    let a = code_passenger::init::init(one.path(), Twin::OneO).unwrap();
    let b = code_passenger::init::init(two.path(), Twin::Toml).unwrap();
    assert_eq!(FeaturesDoc::from_1o(&read(&a.doc)).unwrap(), FeaturesDoc::from_toml(&read(&b.doc)).unwrap());
    assert_eq!(
        read(&a.doc),
        "{\n  features {\n    feature {\n      name \"log\"\n      dep \"app:logsink\"\n      dep \"store:logsink\"\n    }\n    crate {\n      name \"app\"\n      default {\n        feature \"log\"\n      }\n      feature {\n        name \"log\"\n        enable \"dep:logsink\"\n        enable \"store/log\"\n      }\n    }\n    crate {\n      name \"store\"\n      default {\n        feature \"log\"\n      }\n    }\n  }\n}\n"
    );
    assert_eq!(workspace::run(two.path(), Action::Check, &FeaturesOptions::default()).unwrap().exit_code, 0);
}

// ---------------------------------------------------------------- history

#[test]
fn apply_checkpoints_and_checkout_writes_the_snapshot_back() {
    let copy = fresh_copy("ws_features");
    let root = copy.path();
    let original: Vec<(String, String)> = [APP_LIB, STORE_LIB, STORE_SINK, "app/Cargo.toml", "store/Cargo.toml"].iter().map(|f| (f.to_string(), read(&root.join(f)))).collect();
    let out = workspace::run(root, Action::Apply, &opts(true, true, true)).unwrap();
    assert_eq!(out.checkpoints, vec!["S000001".to_string(), "S000002".into()]);

    // the store is the workspace root's, whichever member it is opened from
    let store = PassengerStore::open(root.join("app")).unwrap();
    assert_eq!(store.root, root.canonicalize().unwrap());
    let head = store.resolve_head().unwrap();
    assert_eq!(head.head_commit.as_deref(), Some("S000002"));
    let after = store.read_commit(&head.passenger_version, "S000002").unwrap();
    assert_eq!(after.parents, vec!["S000001".to_string()]);
    let tracked: Vec<&String> = after.manifest.files.keys().filter(|f| f.as_str() != "Cargo.lock").collect();
    assert_eq!(tracked, vec!["Cargo.toml", "app/Cargo.toml", APP_LIB, "store/Cargo.toml", STORE_LIB, STORE_PLAIN, STORE_SINK, doc::DOC_1O], "what klepto's scan reads, and the document");
    let art = root.join(".passenger/data/V0.1.0/artifacts/S000002");
    for f in ["scan.json", "analysis.json", "plan.json", doc::DOC_1O] {
        assert!(art.join(f).is_file(), "{f} beside the snapshot");
    }

    // back to before the apply: every gated file written back
    let r = store.checkout("S000001", false).unwrap();
    assert_eq!(r.commit.as_deref(), Some("S000001"));
    assert_eq!(r.written, vec!["app/Cargo.toml".to_string(), APP_LIB.into(), "store/Cargo.toml".into(), STORE_LIB.into(), STORE_SINK.into()]);
    for (f, text) in &original {
        assert_eq!(&read(&root.join(f)), text, "{f} restored");
    }
    assert_eq!(store.resolve_head().unwrap().head_commit.as_deref(), Some("S000001"), "detached at the snapshot");

    // a change since the snapshot HEAD names is refused, unless forced
    std::fs::write(root.join(STORE_PLAIN), "pub fn add(a: u32, b: u32) -> u32 { a * b }\n").unwrap();
    let e = store.checkout("main", false).unwrap_err();
    assert!(matches!(e, PassengerError::Dirty { .. }) && e.to_string().contains(STORE_PLAIN), "{e}");
    let r = store.checkout("main", true).unwrap();
    assert_eq!(r.commit.as_deref(), Some("S000002"));
    assert!(read(&root.join(APP_LIB)).contains("#[cfg(feature = \"log\")]\npub fn audit"));
    assert!(read(&root.join(STORE_PLAIN)).contains("a + b"));
}

// ---------------------------------------------------------------- agent mode

#[test]
fn agent_init_creates_the_job_layout_and_runs_the_features_apply_job() {
    let copy = fresh_copy("ws_features");
    let root = copy.path();
    let (store, made) = code_passenger::agent::init(root).expect("agent init");
    for dir in [".custodian/state/jobs/queued", ".custodian/state/jobs/running", ".custodian/state/jobs/done", ".custodian/data/receipts"] {
        assert!(root.join(dir).is_dir(), "{dir}");
    }
    let template = root.join(".custodian/config/jobs/features-apply.toml");
    assert!(made.contains(&template.canonicalize().unwrap()) || made.iter().any(|p| p.ends_with("config/jobs/features-apply.toml")));
    // `features` is custodian_kit's reading job, its template the kit's: left as the kit wrote it
    let kits = read(&root.join(".custodian/config/jobs/features.toml"));
    assert_eq!(kits, custodian_kit::jobs::FEATURES, "the kit's features template, untouched");
    let t: code_passenger::agent::JobTemplate = toml::from_str(&read(&template)).unwrap();
    let steps: Vec<(&str, &str, Option<&str>)> = t.step.iter().map(|s| (s.name.as_str(), s.line.as_str(), s.reads.as_deref())).collect();
    assert_eq!(steps, vec![("plan", "features-plan", None), ("apply", "features-apply", Some("plan")), ("annotate", "annotate", Some("apply"))]);
    assert!(root.join(".custodian/state/journal.sqlite").is_file());

    let queued = code_passenger::agent::queue(root, "features-apply", Some("gate-log")).unwrap();
    assert!(queued.join("job.toml").is_file());
    let exe = Path::new(env!("CARGO_BIN_EXE_code-passenger"));
    let (closing, done) = code_passenger::agent::run(root, Some("gate-log"), exe).expect("agent run");
    assert!(closing.done, "{closing:?}");
    let names: Vec<(&str, bool, Option<i32>)> = closing.steps.iter().map(|s| (s.name.as_str(), s.accepted, s.exit_code)).collect();
    assert_eq!(names, vec![("plan", true, Some(0)), ("apply", true, Some(0)), ("annotate", true, Some(0))]);
    assert!(closing.steps[1].line.contains(" apply --write --after "), "apply reads the plan step's receipt: {}", closing.steps[1].line);
    assert!(closing.steps[2].line.contains(" --write --after "), "{}", closing.steps[2].line);
    assert!(done.ends_with(".custodian/state/jobs/done/gate-log"));
    assert!(done.join("closing.json").is_file() && done.join("closing.md").is_file() && done.join("readings/plan.json").is_file());
    for s in &closing.steps {
        let r: custodian_kit::store::Receipt = serde_json::from_str(&read(&store.root.join(&s.receipt))).unwrap();
        assert_eq!(r.job.as_deref(), Some(closing.id.as_str()));
        assert!(r.accepted);
    }
    assert!(read(&root.join(APP_LIB)).starts_with(APP_LIB_HEADER), "the job's last step wrote the headers");
    assert!(read(&root.join(APP_LIB)).contains("#[cfg(feature = \"log\")]\npub fn audit"));
    assert_eq!(workspace::run(root, Action::Check, &FeaturesOptions::default()).unwrap().exit_code, 0);
}

// ---------------------------------------------------------------- the declared lines

fn words(s: &str) -> Vec<OsString> {
    s.split_whitespace().map(OsString::from).collect()
}

#[test]
fn every_command_is_a_declared_line() {
    use code_passenger::lines;
    let a = lines::read(&words("features plan --root ws --no-cargo --json")).unwrap();
    let b = lines::read(&words("--root ws features plan --json --no-cargo")).unwrap();
    assert_eq!(a.display(), "code-passenger features --root ws plan --no-cargo --json");
    assert_eq!(a.display(), b.display(), "the root-first spelling is the same line");
    // custodian's facade writes `--root ROOT gate --spec SPEC --write`
    let g = lines::read(&words("--root ws gate --spec klepto.toml --write")).unwrap();
    assert_eq!(g.display(), "code-passenger gate --root ws --spec klepto.toml --write");

    let e = lines::read(&words("features plan --frobnicate")).unwrap_err();
    assert!(e.to_string().starts_with("[CP0030]") && e.exit_code() == 2, "{e}");
    let e = lines::read(&words("features deploy")).unwrap_err();
    assert!(e.to_string().contains("\"deploy\" is not a action: plan, apply, check"), "{e}");
    assert!(lines::read(&words("annotate --write --check")).is_err(), "write conflicts with check");

    let job = klepto::CheckJob { package: "app".into(), features: vec!["log".into()], no_default: true };
    let line = lines::cargo_check(&job, true);
    assert_eq!(xccute_contract::CompositeShellCommand::build_display(&line), "cargo check -p app --message-format=json --no-default-features --features log --offline");
    assert_eq!(xccute_contract::CompositeShellCommand::exit_status_of(&line, Some(101)).map(|s| s.word), Some("does not compile"));

    // the binary: the refusal of an unproven write, its code and its status
    let copy = fresh_copy("ws_features");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_code-passenger")).args(["features", "apply", "--root"]).arg(copy.path()).args(["--write", "--no-cargo"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("[CP0021]") && err.contains("refused: a person decides (exit 2), receipt sha256:"), "{err}");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_code-passenger")).arg("--root").arg(copy.path()).args(["features", "check"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1), "check: work to do");
    assert!(String::from_utf8_lossy(&out.stdout).contains("CODE PASSENGER :: FEATURES CHECK"));
}

#[test]
fn a_checkpoint_keeps_a_file_klepto_cannot_parse_by_digest_and_says_so() {
    let copy = fresh_copy("ws_unparsed");
    let root = copy.path();
    let exe = env!("CARGO_BIN_EXE_code-passenger");
    let init = std::process::Command::new(exe).args(["passenger", "init", "--root"]).arg(root).output().unwrap();
    assert!(init.status.success(), "{}", String::from_utf8_lossy(&init.stderr));
    let out = std::process::Command::new(exe).args(["passenger", "checkpoint", "--root"]).arg(root).output().unwrap();
    let said = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "{said}{}", String::from_utf8_lossy(&out.stderr));
    assert!(said.contains("checkpoint S000001 on main"), "{said}");
    assert!(said.contains("note: klepto could not parse app/src/broken.rs; kept by digest, marked unparsed"), "{said}");

    let store = PassengerStore::open(root).unwrap();
    let head = store.resolve_head().unwrap();
    let c = store.read_commit(&head.passenger_version, "S000001").unwrap();
    let broken = &c.manifest.files["app/src/broken.rs"];
    assert!(broken.unparsed.is_some(), "marked unparsed");
    let bytes = std::fs::read(root.join("app/src/broken.rs")).unwrap();
    assert_eq!(broken.hash, stable_digest::StableDigest::sha256(&bytes).as_str(), "its bytes kept by digest");
    assert!(c.manifest.files["app/src/lib.rs"].unparsed.is_none());
    let scan = read(&root.join(".passenger/data/V0.1.0/artifacts/S000001/scan.json"));
    assert!(scan.contains("app/src/lib.rs") || scan.contains("lib.rs"), "the parsed file is in the scan");
    assert!(!scan.contains("broken.rs"), "the unparsed one is not");
}

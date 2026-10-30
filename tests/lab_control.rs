// Copyright © 2026 1o. All rights reserved.

//! The laboratory-control repairs (2026-10-30), on a crate made here (`lab`: a generic registry and a test) inside a
//! project custodian keeps (a `.custodian` folder, a lock on the copy):
//!
//! - CP-1: `replace_body`'s text fact is proved on the proof's copy, with the root spelled `.` as a runner spells it;
//! - INT-1: a write goes only on the preflight that authorized this exact spec over this exact code: authorize A,
//!   alter the spec or the code, and A's receipt writes nothing; no preflight at all writes nothing; a dry run proves;
//! - INT-2: the record binds the preflight, the spec's digest, the code digest before and after, the lock the proof
//!   built on and the writer, and the file written is the file proved;
//! - CP-2: the knowledge graph's item `target` is a change spec's target as it is;
//! - the `version` line names the writer.

use code_passenger::transform::{self, Options};
use custodian_kit::api::{Resolving, Scope, Workspace};
use custodian_kit::runs::{Ctx, Runner};
use custodian_kit::store::{Mode, Store};
use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_code-passenger");

const LIB: &str = "pub struct Registry<const WORKS: usize, const SUBSCRIBERS: usize> {\n    slots: [Option<u64>; WORKS],\n}\n\nimpl<const WORKS: usize, const SUBSCRIBERS: usize> Registry<WORKS, SUBSCRIBERS> {\n    pub fn new() -> Self {\n        Registry { slots: [None; WORKS] }\n    }\n\n    pub fn first_free(&self) -> Option<usize> {\n        self.slots.iter().position(|s| s.is_none())\n    }\n}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn empty_has_room() {\n        assert_eq!(super::Registry::<4, 2>::new().first_free(), Some(0));\n    }\n}\n";

const ID: &str = "lab::Registry<WORKS,SUBSCRIBERS>::first_free";

const SPEC: &str = "name = \"first-free\"\nobjective = \"one pass, the same answer\"\noperation = \"replace_body\"\ntarget = \"lab::Registry<WORKS,SUBSCRIBERS>::first_free\"\ncompatibility = \"preserve\"\n\n[function]\nname = \"first_free\"\n\n[[function.body]]\npiece = \"raw\"\ncode = '''\nlet mut found = None;\nfor (index, slot) in self.slots.iter().enumerate() {\n    if slot.is_none() && found.is_none() {\n        found = Some(index);\n    }\n}\nfound\n'''\n";

/// A copy under the system's temporary directory (on macOS `/var/...`, which klepto reads as `/private/var/...`).
fn workspace(governed: bool) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"lab\"\nversion = \"0.1.0\"\nedition = \"2021\"\n").unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/lib.rs"), LIB).unwrap();
    custodian_kit::exec::lock_crate(root).unwrap();
    if governed {
        Store::init(root, Mode::Agent).unwrap();
    }
    dir
}

fn spec(root: &Path, text: &str) -> PathBuf {
    let p = root.join("first-free.toml");
    std::fs::write(&p, text).unwrap();
    p
}

/// custodian's preflight of the spec over the code as it is (custodian_kit's, in this process): its receipt.
fn preflight(root: &Path, spec: &Path) -> (i64, String) {
    let store = Store::find(root).unwrap();
    let ctx = Ctx { root: store.root.clone(), store: Some(store.clone()), exe: PathBuf::from("custodian") };
    let w = Workspace::open_with(&store.root, &Scope::default(), Resolving::Free).unwrap();
    let ran = custodian_kit::preflight::run_preflight(&ctx, spec, None, Runner::InProcess(&w)).unwrap();
    assert!(ran.preflight.authorized, "{:?} {:?}", ran.preflight.why, ran.preflight.preconditions);
    (ran.receipt.unwrap().seq, ran.preflight.code_digest)
}

fn write_opts(spec: &Path, preflight: Option<i64>) -> Options {
    Options { spec: spec.to_path_buf(), write: true, preflight, ..Options::default() }
}

/// INT-1 (1) and INT-2: the exact spec over the exact code, on its preflight's receipt, is proved and written; the
/// record binds that receipt, the spec's digest, the code digest the preflight read and the one after, the lock the
/// proof built on, and the writer; the file written is the file the proof held.
#[test]
fn an_authorized_exact_spec_over_exact_code_is_written_and_bound_to_its_preflight() {
    let dir = workspace(true);
    let root = dir.path();
    let s = spec(root, SPEC);
    let (a, code) = preflight(root, &s);
    let o = transform::run(root, &write_opts(&s, Some(a))).unwrap();
    assert!(o.record.written && o.exit_code == 0, "{}", o.said.join("\n"));
    let r = &o.record;
    assert_eq!(r.preflight_seq, Some(a));
    assert_eq!(r.spec_digest, custodian_kit::change::ChangeSpec::from_path(&s).unwrap().digest());
    assert_eq!(r.code_digest.as_deref(), Some(code.as_str()), "the code the preflight read is the code it ran over");
    let after = r.code_digest_after.clone().expect("the digest after the write");
    assert_ne!(after, code);
    assert_eq!(after, custodian_kit::code_digest(root, &Scope { tests: true, ..Default::default() }.scan_options()).unwrap());
    assert_eq!(r.lock.as_ref().map(|l| l.digest.clone()), Some(custodian_kit::digest::tagged(std::fs::read(root.join("Cargo.lock")).unwrap())));
    assert_eq!(r.writer.as_deref(), Some(code_passenger::lines::writer().as_str()));
    let written = std::fs::read_to_string(root.join("src/lib.rs")).unwrap();
    assert_eq!(r.files.len(), 1);
    assert_eq!(r.files[0].after, stable_digest::StableDigest::sha256(written.as_bytes()).as_str());
    assert!(written.contains("        let mut found = None;\n") && written.lines().all(|l| l == l.trim_end()), "{written}");
}

/// INT-1 (2): authorize A; alter the spec; A's receipt writes nothing.
#[test]
fn an_altered_spec_with_the_old_receipt_is_refused_and_nothing_is_written() {
    let dir = workspace(true);
    let root = dir.path();
    let s = spec(root, SPEC);
    let (a, _) = preflight(root, &s);
    std::fs::write(&s, SPEC.replace("found = Some(index);", "found = Some(index.min(WORKS));")).unwrap();
    let e = transform::run(root, &write_opts(&s, Some(a))).unwrap_err().to_string();
    assert!(e.contains("[CUST0047]") && e.contains(&format!("preflight #{a} authorized another version of the spec")), "{e}");
    assert_eq!(std::fs::read_to_string(root.join("src/lib.rs")).unwrap(), LIB);
    // the newest preflight is no help either: none covers the spec as it is now
    let e = transform::run(root, &write_opts(&s, None)).unwrap_err().to_string();
    assert!(e.contains("[CUST0047]") && e.contains("the spec changed since preflight"), "{e}");
    assert_eq!(std::fs::read_to_string(root.join("src/lib.rs")).unwrap(), LIB);
}

/// INT-1 (3): authorize A; alter the code; A's receipt writes nothing.
#[test]
fn altered_code_with_the_old_receipt_is_refused_and_nothing_is_written() {
    let dir = workspace(true);
    let root = dir.path();
    let s = spec(root, SPEC);
    let (a, _) = preflight(root, &s);
    let moved = LIB.replace("    pub fn new()", "    /// Made empty.\n    pub fn new()");
    std::fs::write(root.join("src/lib.rs"), &moved).unwrap();
    let e = transform::run(root, &write_opts(&s, Some(a))).unwrap_err().to_string();
    assert!(e.contains("[CUST0047]") && e.contains(&format!("preflight #{a} read other code")), "{e}");
    assert_eq!(std::fs::read_to_string(root.join("src/lib.rs")).unwrap(), moved);
}

/// In a project custodian keeps, a write with no preflight writes nothing, and a receipt number that is no preflight
/// of the spec is refused; a dry run (nothing written) proves without one.
#[test]
fn a_governed_write_needs_a_preflight_and_a_dry_run_does_not() {
    let dir = workspace(true);
    let root = dir.path();
    let s = spec(root, SPEC);
    let e = transform::run(root, &write_opts(&s, None)).unwrap_err().to_string();
    assert!(e.contains("[CUST0047]") && e.contains("no preflight of this spec"), "{e}");
    let e = transform::run(root, &write_opts(&s, Some(999))).unwrap_err().to_string();
    assert!(e.contains("receipt #999 is no preflight of this spec"), "{e}");
    assert_eq!(std::fs::read_to_string(root.join("src/lib.rs")).unwrap(), LIB);
    let dry = transform::run(root, &Options { spec: s.clone(), ..Options::default() }).unwrap();
    assert!(dry.record.proved() && !dry.record.written, "{}", dry.said.join("\n"));
    assert_eq!(dry.record.preflight_seq, None);
}

/// Outside a project custodian keeps, code_passenger is its own tool: it writes a proved change with no preflight, and
/// refuses a receipt number it has no journal to check.
#[test]
fn outside_a_governed_project_a_named_receipt_is_refused_and_a_proved_write_is_made() {
    let dir = workspace(false);
    let root = dir.path();
    let s = spec(root, SPEC);
    let e = transform::run(root, &write_opts(&s, Some(4))).unwrap_err().to_string();
    assert!(e.contains("--preflight 4 names a receipt, and no .custodian folder keeps receipts"), "{e}");
    let o = transform::run(root, &write_opts(&s, None)).unwrap();
    assert!(o.record.written && o.record.preflight_seq.is_none());
}

/// CP-1, as the runner ran it: `cd <candidate> && code-passenger --root . transform --spec FILE`. The landing is the
/// crate's own `src/lib.rs`, the text fact names it relative to the root, and the fact holds on the proof's copy.
#[test]
fn replace_bodys_text_fact_is_proved_on_the_proofs_copy_with_the_root_spelled_dot() {
    let dir = workspace(false);
    let root = dir.path();
    let s = spec(root, SPEC);
    let out = root.join("record.json");
    let o = Command::new(BIN).current_dir(root).args(["--root", ".", "transform", "--spec"]).arg(&s).arg("--out").arg(&out).output().unwrap();
    let said = String::from_utf8_lossy(&o.stdout);
    assert_eq!(o.status.code(), Some(0), "{said}\n{}", String::from_utf8_lossy(&o.stderr));
    assert!(said.contains(&format!("lands {ID} at src/lib.rs:")), "{said}");
    let r: custodian_kit::transform::TransformRecord = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert!(r.postconditions.contains(&("text src/lib.rs let mut found = None;".to_string(), true)), "{:?}", r.postconditions);
    assert_eq!(std::fs::read_to_string(root.join("src/lib.rs")).unwrap(), LIB, "a dry run writes nothing");
}

/// CP-2: the knowledge graph's item record carries `target`, the change spec's target as it is; taken verbatim it
/// lands, while the same name rebuilt without its generics is refused naming that target.
#[test]
fn the_knowledge_graphs_target_is_a_change_specs_target_as_it_is() {
    let dir = workspace(false);
    let root = dir.path();
    let kg = root.join("kg");
    code_passenger::commands::receipt::write(root, &root.join("receipt.json"), Some(&kg)).unwrap();
    let items: Vec<serde_json::Value> = std::fs::read_to_string(kg.join("knowledge.ndjson")).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let item = items.iter().find(|r| r["kind"] == "item" && r["name"] == "first_free").expect("the method's record");
    assert_eq!(item["target"], ID);
    let s = spec(root, &SPEC.replace(ID, item["target"].as_str().unwrap()));
    let o = transform::run(root, &Options { spec: s.clone(), no_cargo: true, ..Options::default() }).unwrap();
    assert!(o.said[0].contains(&format!("lands {ID} at src/lib.rs:")), "{:?}", o.said);
    let near = spec(root, &SPEC.replace(ID, "lab::Registry::first_free"));
    let e = transform::run(root, &Options { spec: near, no_cargo: true, ..Options::default() }).unwrap_err().to_string();
    assert!(e.contains(&format!("the scan's id for it is `{ID}`")), "{e}");
}

/// The writer's name: what `version` (and `--version`) prints, and what every record keeps.
#[test]
fn the_version_line_names_the_writer() {
    for word in ["version", "--version"] {
        let o = Command::new(BIN).arg(word).output().unwrap();
        assert_eq!(o.status.code(), Some(0), "{word}: {}", String::from_utf8_lossy(&o.stderr));
        let said = String::from_utf8_lossy(&o.stdout);
        assert_eq!(said.trim(), code_passenger::lines::writer());
        assert!(said.starts_with(&format!("code_passenger {} (custodian_kit ", env!("CARGO_PKG_VERSION"))), "{said}");
    }
}

/// Audit #1: in a project custodian keeps, a revert puts back only what a recorded transform wrote: a record no receipt
/// names is refused, and so is one naming a file outside the tree; nothing is written or deleted. A real record (the
/// program's own receipt names it) is put back.
#[test]
fn a_revert_puts_back_only_a_recorded_transform_inside_the_tree() {
    let dir = workspace(true);
    let root = dir.path();
    let outside = tempfile::tempdir().unwrap();
    let victim = outside.path().join("victim.txt");
    std::fs::write(&victim, "keep me\n").unwrap();
    let now = stable_digest::StableDigest::sha256(LIB.as_bytes()).as_str().to_string();
    let forged = serde_json::json!({"schema": "custodian.transform.v1", "spec_name": "x", "spec": "x.toml", "spec_digest": "d", "operation": "replace_body", "target": "t", "files": [{"path": "src/lib.rs", "before": "none", "after": now, "lines": []}], "checks": [], "postconditions": [], "written": true});
    let rec = root.join("forged.json");
    std::fs::write(&rec, forged.to_string()).unwrap();
    let e = transform::revert(root, &rec).unwrap_err().to_string();
    assert!(e.contains("no receipt in the journal names this record"), "{e}");
    assert_eq!(std::fs::read_to_string(root.join("src/lib.rs")).unwrap(), LIB);
    // a real write, through the program (its receipt names the record), then its revert
    let s = spec(root, SPEC);
    let (a, _) = preflight(root, &s);
    let out = root.join("written.json");
    let o = Command::new(BIN).args(["--root"]).arg(root).args(["transform", "--spec"]).arg(&s).args(["--preflight", &a.to_string(), "--write", "--out"]).arg(&out).output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    assert_ne!(std::fs::read_to_string(root.join("src/lib.rs")).unwrap(), LIB);
    let original = std::fs::read(&out).unwrap();
    // a folder named in place of a record: its records are named by no receipt, and nothing is put back
    std::fs::write(root.join("evil.json"), forged.to_string()).unwrap();
    let e = transform::revert(root, root).unwrap_err().to_string();
    assert!(e.contains("no receipt in the journal names this record"), "{e}");
    // the real record, edited after its write to name a file the transform never wrote: not the copy kept with it
    std::fs::write(root.join("src/extra.rs"), "pub fn extra() {}\n").unwrap();
    let mut edited: serde_json::Value = serde_json::from_slice(&original).unwrap();
    edited["files"][0] = serde_json::json!({"path": "src/extra.rs", "before": "none", "after": stable_digest::StableDigest::sha256(b"pub fn extra() {}\n").as_str(), "lines": []});
    std::fs::write(&out, serde_json::to_string_pretty(&edited).unwrap() + "\n").unwrap();
    let e = transform::revert(root, &out).unwrap_err().to_string();
    assert!(e.contains("the record is not the one code_passenger kept with its write"), "{e}");
    assert!(root.join("src/extra.rs").is_file());
    // the same record with a file outside the tree slipped in: refused before anything is put back
    let mut evil: serde_json::Value = serde_json::from_slice(&original).unwrap();
    let victim_now = stable_digest::StableDigest::sha256(b"keep me\n").as_str().to_string();
    evil["files"].as_array_mut().unwrap().push(serde_json::json!({"path": victim.display().to_string(), "before": "none", "after": victim_now, "lines": []}));
    std::fs::write(&out, evil.to_string()).unwrap();
    let e = transform::revert(root, &out).unwrap_err().to_string();
    assert!(e.contains("the record is not the one code_passenger kept with its write"), "{e}");
    assert!(victim.is_file(), "the file outside the tree is untouched");
    // the record as it was written is put back
    std::fs::write(&out, &original).unwrap();
    transform::revert(root, &out).unwrap();
    assert_eq!(std::fs::read_to_string(root.join("src/lib.rs")).unwrap(), LIB);
}

/// Audit #2: in a project custodian keeps, `--no-cargo` and `--unproven` prove nothing and write nothing, even on an
/// authorizing receipt.
#[test]
fn a_governed_write_is_a_proved_one() {
    let dir = workspace(true);
    let root = dir.path();
    let s = spec(root, SPEC);
    let (a, _) = preflight(root, &s);
    let e = transform::run(root, &Options { spec: s.clone(), write: true, no_cargo: true, unproven: true, preflight: Some(a), ..Options::default() }).unwrap_err().to_string();
    assert!(e.contains("[CUST0047]") && e.contains("--no-cargo and --unproven write nothing here"), "{e}");
    assert_eq!(std::fs::read_to_string(root.join("src/lib.rs")).unwrap(), LIB);
}

/// Audit #4: a member crate custodian keeps inside a larger cargo workspace is governed: its own `.custodian` is found
/// from the root the line gives, not only from the workspace root cargo names.
#[test]
fn a_member_crates_own_custodian_folder_governs_it() {
    let parent = tempfile::tempdir().unwrap();
    std::fs::write(parent.path().join("Cargo.toml"), "[workspace]\nresolver = \"2\"\nmembers = [\"lab\"]\n").unwrap();
    let lab = parent.path().join("lab");
    std::fs::create_dir_all(lab.join("src")).unwrap();
    std::fs::write(lab.join("Cargo.toml"), "[package]\nname = \"lab\"\nversion = \"0.1.0\"\nedition = \"2021\"\n").unwrap();
    std::fs::write(lab.join("src/lib.rs"), LIB).unwrap();
    custodian_kit::exec::lock_crate(parent.path()).unwrap();
    Store::init(&lab, Mode::Agent).unwrap();
    let s = spec(&lab, SPEC);
    let e = transform::run(&lab, &write_opts(&s, None)).unwrap_err().to_string();
    assert!(e.contains("[CUST0047]"), "{e}");
    // the root named is the parent workspace: the member's own folder still governs the file the write would touch
    let e = transform::run(parent.path(), &write_opts(&s, None)).unwrap_err().to_string();
    assert!(e.contains("[CUST0047]"), "{e}");
    assert_eq!(std::fs::read_to_string(lab.join("src/lib.rs")).unwrap(), LIB);
    // and an experiment open in the member refuses a line that writes without a change spec, from the parent's root too
    let folder = lab.join(".custodian/state/jobs/running/exp");
    std::fs::create_dir_all(&folder).unwrap();
    custodian_kit::provenance::open(&folder, &lab, "job-exp", &lab).unwrap();
    let e = code_passenger::commands::edit::dep_add(parent.path(), "lab", "itoa@1", None, &[], false).unwrap_err().to_string();
    assert!(e.contains("experiment job-exp is open"), "{e}");
}

/// Audit #7: each change of a batch keeps its record by its spec's name, so two members of one name are refused.
#[test]
fn a_batch_of_two_specs_of_one_name_is_refused() {
    let dir = workspace(false);
    let root = dir.path();
    let one = root.join("one.toml");
    let two = root.join("two.toml");
    std::fs::write(&one, SPEC).unwrap();
    std::fs::write(&two, SPEC.replace("found = Some(index);", "found = Some(index.min(WORKS));")).unwrap();
    let e = transform::run_batch(root, &transform::BatchOptions { specs: vec![one, two], write: true, out_dir: None, preflights: Default::default(), after_proof: None, tests: false }).unwrap_err().to_string();
    assert!(e.contains("two members of the batch are named first-free"), "{e}");
    assert_eq!(std::fs::read_to_string(root.join("src/lib.rs")).unwrap(), LIB);
}

/// While an experiment is open, its tree is written only by `transform` on an authorized preflight: `dep add` (and the
/// other lines that write without a change spec) write nothing.
#[test]
fn an_open_experiments_tree_is_written_only_by_an_authorized_transform() {
    let dir = workspace(true);
    let root = dir.path();
    let specimen = tempfile::tempdir().unwrap();
    std::fs::write(specimen.path().join("Cargo.toml"), "[package]\nname = \"lab\"\nversion = \"0.1.0\"\nedition = \"2021\"\n").unwrap();
    let folder = root.join(".custodian/state/jobs/running/exp");
    std::fs::create_dir_all(&folder).unwrap();
    custodian_kit::provenance::open(&folder, root, "job-exp", specimen.path()).unwrap();
    let before = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
    let e = code_passenger::commands::edit::dep_add(root, "lab", "itoa@1", None, &[], false).unwrap_err().to_string();
    assert!(e.contains("[CUST0047]") && e.contains("experiment job-exp is open"), "{e}");
    assert_eq!(std::fs::read_to_string(root.join("Cargo.toml")).unwrap(), before);
    // with the experiment closed (its folder no longer running), the line is the tool's again
    std::fs::remove_dir_all(&folder).unwrap();
    code_passenger::commands::edit::dep_add(root, "lab", "itoa@1", None, &[], false).unwrap();
}

/// Outside a governed project too, a revert never reaches outside the tree: a record naming a file by an absolute path
/// (or one that climbs out) is refused before anything is put back.
#[test]
fn a_revert_never_reaches_outside_the_tree() {
    let dir = workspace(false);
    let root = dir.path();
    let s = spec(root, SPEC);
    let out = root.join("written.json");
    transform::run(root, &Options { spec: s, write: true, out: Some(out.clone()), ..Options::default() }).unwrap();
    let outside = tempfile::tempdir().unwrap();
    let victim = outside.path().join("victim.txt");
    std::fs::write(&victim, "keep me\n").unwrap();
    let mut evil: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    evil["files"].as_array_mut().unwrap().push(serde_json::json!({"path": victim.display().to_string(), "before": "none", "after": stable_digest::StableDigest::sha256(b"keep me\n").as_str(), "lines": []}));
    std::fs::write(&out, evil.to_string()).unwrap();
    let e = transform::revert(root, &out).unwrap_err().to_string();
    assert!(e.contains("which is not a file inside"), "{e}");
    assert!(victim.is_file());
}

/// The runner's own form, governed: `cd <candidate> && code-passenger --root . transform --spec FILE --preflight N
/// --write`. The code digest read from `.` is the one the preflight read from the resolved root, so the write goes on
/// its receipt.
#[test]
fn a_governed_write_from_root_dot_goes_on_its_preflight() {
    let dir = workspace(true);
    let root = dir.path();
    let s = spec(root, SPEC);
    let (a, code) = preflight(root, &s);
    let out = root.join("written.json");
    let o = Command::new(BIN).current_dir(root).args(["--root", ".", "transform", "--spec"]).arg(&s).args(["--preflight", &a.to_string(), "--write", "--out"]).arg(&out).output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    let r: custodian_kit::transform::TransformRecord = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!((r.preflight_seq, r.code_digest.as_deref(), r.written), (Some(a), Some(code.as_str()), true));
}

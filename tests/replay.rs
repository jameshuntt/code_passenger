// Copyright © 2026 1o. All rights reserved.

//! `transform` of a `replay` spec: an exact transition of whole files, any file type, made only after the whole of it
//! is checked. One file replaced; several made, replaced and deleted at once with their modes; a dry run that changes
//! nothing; each way a transition does not hold refused with nothing written; a transition whose fifth file is not as
//! declared, or whose late payload is not the bytes declared, writes none of the five; a staging that fails takes back
//! what it staged; the record names exactly the files touched; in a project custodian keeps, a write goes only on the
//! preflight of this spec over this code; a batch leaves a replay out and says why.

use code_passenger::transform::{self, Options};
use custodian_kit::api::{Resolving, Scope, Workspace};
use custodian_kit::change::{self, ChangeSpec, Operation, ReplayFile};
use custodian_kit::runs::{Ctx, Runner};
use custodian_kit::store::{Mode, Store};
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const LIB: &[u8] = b"pub fn answer() -> u32 {\n    41\n}\n";

/// A crate with a lock, a script, notes and a binary file; governed when asked.
fn workspace(governed: bool) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    put(root, "Cargo.toml", b"[package]\nname = \"lab\"\nversion = \"0.1.0\"\nedition = \"2021\"\n");
    put(root, "src/lib.rs", LIB);
    put(root, "scripts/run.sh", b"#!/bin/sh\necho old\n");
    set_mode(root, "scripts/run.sh", 0o755);
    put(root, "notes.txt", b"old notes\n");
    put(root, "data/table.bin", &[1, 2, 3, 0, 255]);
    custodian_kit::exec::lock_crate(root).unwrap();
    if governed {
        Store::init(root, Mode::Agent).unwrap();
    }
    dir
}

fn put(root: &Path, rel: &str, bytes: &[u8]) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, bytes).unwrap();
}

fn set_mode(root: &Path, rel: &str, bits: u32) {
    std::fs::set_permissions(root.join(rel), std::fs::Permissions::from_mode(bits)).unwrap();
}

fn mode(root: &Path, rel: &str) -> u32 {
    std::fs::metadata(root.join(rel)).unwrap().permissions().mode() & 0o777
}

fn sha(bytes: &[u8]) -> String {
    change::sha256_hex(bytes)
}

/// Every file and folder of the tree under `root` (the passenger's and custodian's stores aside): its bytes (a
/// folder none) and its mode.
fn snapshot(root: &Path) -> BTreeMap<String, (Option<Vec<u8>>, u32)> {
    let mut out = BTreeMap::new();
    for e in walkdir::WalkDir::new(root).min_depth(1).into_iter().filter_entry(|e| ![".passenger", ".custodian", "target"].contains(&e.file_name().to_string_lossy().as_ref())) {
        let e = e.unwrap();
        let rel = e.path().strip_prefix(root).unwrap().to_string_lossy().into_owned();
        let bytes = e.file_type().is_file().then(|| std::fs::read(e.path()).unwrap());
        out.insert(rel, (bytes, e.metadata().unwrap().permissions().mode() & 0o7777));
    }
    out
}

/// A spec folder outside the tree: the spec and its payloads.
struct Spec {
    _dir: tempfile::TempDir,
    path: PathBuf,
    spec: ChangeSpec,
}

/// The replay of `files` (`(entry, payload bytes)`), written as a spec with its payloads as `payloads/<n>`.
fn replay(name: &str, files: Vec<(ReplayFile, Option<&[u8]>)>) -> Spec {
    let dir = tempfile::tempdir().unwrap();
    let mut entries = Vec::new();
    for (n, (mut f, payload)) in files.into_iter().enumerate() {
        if let Some(bytes) = payload {
            let rel = format!("payloads/{n}");
            put(dir.path(), &rel, bytes);
            if f.payload.is_none() {
                f.payload = Some(rel);
            }
        }
        entries.push(f);
    }
    let spec = ChangeSpec { name: name.into(), objective: "the kept state".into(), operation: Some(Operation::Replay), target: "*".into(), replay: entries, ..ChangeSpec::default() };
    let path = dir.path().join(format!("{name}.toml"));
    std::fs::write(&path, spec.to_toml().unwrap()).unwrap();
    Spec { _dir: dir, path, spec }
}

/// `path` written with `after` over `before` (`None`: absent).
fn writes(path: &str, before: Option<&[u8]>, after: &[u8]) -> ReplayFile {
    ReplayFile { path: path.into(), before_sha256: before.map(sha), absent_before: before.is_none(), after_sha256: Some(sha(after)), ..ReplayFile::default() }
}

fn deletes(path: &str, before: &[u8]) -> ReplayFile {
    ReplayFile { path: path.into(), before_sha256: Some(sha(before)), delete: true, ..ReplayFile::default() }
}

fn with_mode(f: ReplayFile, m: &str) -> ReplayFile {
    ReplayFile { mode: Some(m.into()), ..f }
}

fn opts(s: &Spec, write: bool) -> Options {
    Options { spec: s.path.clone(), write, ..Options::default() }
}

#[test]
fn one_file_is_replaced_byte_for_byte_and_the_record_names_it() {
    let d = workspace(false);
    let root = d.path();
    let after = b"pub fn answer() -> u32 {\n    42\n}\n";
    let s = replay("one", vec![(writes("src/lib.rs", Some(LIB), after), Some(after))]);
    let o = transform::run(root, &Options { out: Some(root.join(".records/one.json")), ..opts(&s, true) }).unwrap();
    assert_eq!(o.exit_code, 0, "{}", o.said.join("\n"));
    assert_eq!(std::fs::read(root.join("src/lib.rs")).unwrap(), after);
    let r = &o.record;
    assert!(r.written && r.proved() && r.checks.is_empty(), "no cargo: {r:?}");
    assert_eq!((r.operation.as_str(), r.target.as_str(), r.spec_digest.as_str()), ("replay", "*", s.spec.digest().as_str()));
    assert_eq!(r.files.len(), 1);
    assert_eq!((r.files[0].path.as_str(), r.files[0].before.as_str(), r.files[0].after.as_str()), ("src/lib.rs", sha(LIB).as_str(), sha(after).as_str()));
    assert_eq!(r.files[0].lines, [(2, 2)], "the line written");
    assert!(r.checkpoint_before.is_some() && r.checkpoint_after.is_some() && r.code_digest_after.is_some() && r.code_digest_after != r.code_digest);
    assert_eq!(r.postconditions, [(format!("replay src/lib.rs: sha256 {}, mode 644", sha(after)), true)]);
    // the record kept is the record returned
    let kept: custodian_kit::transform::TransformRecord = serde_json::from_str(&std::fs::read_to_string(o.record_file.as_ref().unwrap()).unwrap()).unwrap();
    assert_eq!(&kept, r);
    assert!(transform::render(&o).contains("-    41\n+    42\n"), "the diff a person reads");
}

#[test]
fn a_mixed_transition_makes_replaces_and_deletes_with_its_modes_and_the_record_names_exactly_those_files() {
    let d = workspace(false);
    let root = d.path();
    set_mode(root, "notes.txt", 0o600);
    let run = b"#!/bin/sh\necho new\n";
    let table: &[u8] = &[9, 8, 7, 0, 0, 254];
    let notes = b"new notes\n";
    let tool = b"#!/bin/sh\nexit 0\n";
    let readme = b"read me\n";
    let s = replay(
        "mixed",
        vec![
            (writes("scripts/run.sh", Some(b"#!/bin/sh\necho old\n"), run), Some(run)),
            (writes("data/table.bin", Some(&[1, 2, 3, 0, 255]), table), Some(table)),
            (writes("notes.txt", Some(b"old notes\n"), notes), Some(notes)),
            (with_mode(writes("tools/bin/check", None, tool), "755"), Some(tool)),
            (with_mode(writes("docs/README", None, readme), "0640"), Some(readme)),
            (writes("docs/plain.txt", None, readme), Some(readme)),
            (with_mode(writes("src/lib.rs", Some(LIB), LIB), "600"), Some(LIB)),
            (deletes("Cargo.toml.orig", b"x"), None),
        ],
    );
    put(root, "Cargo.toml.orig", b"x");
    let o = transform::run(root, &opts(&s, true)).unwrap();
    assert_eq!(o.exit_code, 0, "{}", o.said.join("\n"));
    for (rel, bytes, bits) in [("scripts/run.sh", run.as_slice(), 0o755), ("data/table.bin", table, 0o644), ("notes.txt", notes, 0o600), ("tools/bin/check", tool, 0o755), ("docs/README", readme, 0o640), ("docs/plain.txt", readme, 0o644), ("src/lib.rs", LIB, 0o600)] {
        assert_eq!(std::fs::read(root.join(rel)).unwrap(), bytes, "{rel}: the bytes after, exactly");
        assert_eq!(mode(root, rel), bits, "{rel}: a declared mode, a replaced file's own, a made file's 644");
    }
    assert!(!root.join("Cargo.toml.orig").exists(), "deleted");
    let touched: Vec<(&str, &str, &str)> = o.record.files.iter().map(|f| (f.path.as_str(), f.before.as_str(), f.after.as_str())).collect();
    let (s_run, s_tab, s_notes, s_tool, s_readme, s_lib, s_x) = (sha(run), sha(table), sha(notes), sha(tool), sha(readme), sha(LIB), sha(b"x"));
    assert_eq!(
        touched,
        [
            ("Cargo.toml.orig", s_x.as_str(), "none"),
            ("data/table.bin", sha(&[1, 2, 3, 0, 255]).as_str(), s_tab.as_str()),
            ("docs/README", "none", s_readme.as_str()),
            ("docs/plain.txt", "none", s_readme.as_str()),
            ("notes.txt", sha(b"old notes\n").as_str(), s_notes.as_str()),
            ("scripts/run.sh", sha(b"#!/bin/sh\necho old\n").as_str(), s_run.as_str()),
            ("src/lib.rs", s_lib.as_str(), s_lib.as_str()),
            ("tools/bin/check", "none", s_tool.as_str()),
        ],
        "every file the replay names, in path order, and no other"
    );
    assert!(o.record.files.iter().find(|f| f.path == "data/table.bin").unwrap().lines.is_empty(), "no lines for bytes that are not text");
    assert!(o.record.postconditions.iter().all(|(_, h)| *h) && o.record.postconditions.len() == 8, "{:?}", o.record.postconditions);
    assert!(o.record.postconditions.iter().any(|(f, _)| f == &format!("replay tools/bin/check: sha256 {s_tool}, mode 755")));
    assert!(o.record.postconditions.iter().any(|(f, _)| f == "replay Cargo.toml.orig: absent"));
    // nothing staged is left beside a file
    assert!(snapshot(root).keys().all(|k| !k.contains(".replay-")), "{:?}", snapshot(root).keys().collect::<Vec<_>>());
}

#[test]
fn a_dry_run_proves_the_transition_and_changes_nothing() {
    let d = workspace(false);
    let root = d.path();
    let before = snapshot(root);
    let s = replay("dry", vec![(writes("src/lib.rs", Some(LIB), b"x"), Some(b"x")), (with_mode(writes("new.sh", None, b"y"), "755"), Some(b"y")), (deletes("notes.txt", b"old notes\n"), None)]);
    let o = transform::run(root, &opts(&s, false)).unwrap();
    assert_eq!(o.exit_code, 0, "{}", o.said.join("\n"));
    assert!(!o.record.written && o.record.proved() && o.record_file.is_none());
    assert!(o.said.iter().any(|l| l.starts_with("proved on a scratch copy, not written (--write writes): 3 files")), "{:?}", o.said);
    assert_eq!(snapshot(root), before, "no byte, mode or file changed");
    assert!(!root.join(".passenger").exists(), "no checkpoint taken");
    // its record, asked for, is the plan; the tree is still as it was
    let o = transform::run(root, &Options { out: Some(d.path().join("plan.json")), ..opts(&s, false) }).unwrap();
    assert_eq!(o.record.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), ["new.sh", "notes.txt", "src/lib.rs"]);
    let mut now = snapshot(root);
    now.remove("plan.json");
    assert_eq!(now, before);
}

/// The refusal of `files` over a fresh workspace: nothing in the tree changed, and the refusal names `why`.
fn refused(tag: &str, files: Vec<(ReplayFile, Option<&[u8]>)>, why: &str) {
    let d = workspace(false);
    let root = d.path();
    let before = snapshot(root);
    let s = replay(tag, files);
    for write in [false, true] {
        let e = transform::run(root, &opts(&s, write)).unwrap_err().to_string();
        assert!(e.contains("the replay does not hold, so nothing is written") && e.contains(why), "{tag}: {e}");
        assert_eq!(snapshot(root), before, "{tag}: nothing written");
        assert!(!root.join(".passenger").exists(), "{tag}: no checkpoint");
    }
}

#[test]
fn each_way_a_transition_does_not_hold_is_refused_before_anything_is_written() {
    let ok = || (writes("src/lib.rs", Some(LIB), b"x"), Some(b"x".as_slice()));
    refused("wrong-before", vec![ok(), (writes("notes.txt", Some(b"other"), b"n"), Some(b"n"))], "notes.txt: as declared before: sha256 ");
    refused("absent-but-there", vec![ok(), (writes("notes.txt", None, b"n"), Some(b"n"))], "notes.txt: as declared before: there (sha256 ");
    refused("there-but-absent", vec![ok(), (writes("gone.txt", Some(b"g"), b"n"), Some(b"n"))], "gone.txt: as declared before: absent, declared sha256 ");
    refused("no-payload", vec![ok(), (ReplayFile { payload: Some("payloads/none".into()), ..writes("notes.txt", Some(b"old notes\n"), b"n") }, None)], "notes.txt: its payload holds the bytes after: ");
    refused("payload-hash", vec![ok(), (writes("notes.txt", Some(b"old notes\n"), b"declared"), Some(b"other bytes"))], &format!("declared {}", sha(b"declared")));
    refused("absolute", vec![ok(), (writes("/tmp/replay-escape", None, b"n"), Some(b"n"))], "is absolute");
    refused("escape", vec![ok(), (writes("src/../../escape", None, b"n"), Some(b"n"))], "climbs out of the root");
    refused("duplicate", vec![ok(), (writes("./src//lib.rs", Some(LIB), b"x"), Some(b"x"))], "src/lib.rs: named once: named twice (written, then written)");
    refused("write-delete", vec![ok(), (deletes("src/lib.rs", LIB), None)], "src/lib.rs: named once: named twice (written, then deleted)");
    refused("mode", vec![ok(), (with_mode(writes("run.sh", None, b"n"), "rwxr-xr-x"), Some(b"n"))], "run.sh: its mode: mode `rwxr-xr-x` is not three octal digits");
    refused("governance", vec![ok(), (writes(".passenger/HEAD", None, b"n"), Some(b"n"))], "where the tree digest does not read");
}

/// Atomicity: five files, the fifth in path order not as declared before. With --write nothing is written: the first
/// four are byte for byte and mode for mode as they were, the one to be made is not made.
#[test]
fn a_five_file_replay_whose_fifth_file_is_not_as_declared_writes_none_of_them() {
    let d = workspace(false);
    let root = d.path();
    let before = snapshot(root);
    let s = replay(
        "five",
        vec![
            (writes("Cargo.toml", Some(&std::fs::read(root.join("Cargo.toml")).unwrap()), b"[package]\nname = \"lab\"\nversion = \"0.2.0\"\nedition = \"2021\"\n"), Some(b"[package]\nname = \"lab\"\nversion = \"0.2.0\"\nedition = \"2021\"\n")),
            (with_mode(writes("data/table.bin", Some(&[1, 2, 3, 0, 255]), &[0]), "600"), Some(&[0])),
            (deletes("notes.txt", b"old notes\n"), None),
            (writes("scripts/new.sh", None, b"#!/bin/sh\n"), Some(b"#!/bin/sh\n")),
            (writes("src/lib.rs", Some(b"not what is there"), b"x"), Some(b"x")),
        ],
    );
    let e = transform::run(root, &opts(&s, true)).unwrap_err().to_string();
    assert!(e.contains("src/lib.rs: as declared before: sha256 ") && e.contains(&format!("declared {}", sha(b"not what is there"))), "{e}");
    assert_eq!(snapshot(root), before, "files 1-5 byte for byte and mode for mode as they were");
    assert!(!root.join("scripts/new.sh").exists() && root.join("notes.txt").exists());
    assert!(!root.join(".passenger").exists(), "no checkpoint, so no write began");
}

/// Atomicity: a payload late in path order that is not the bytes declared after: zero writes.
#[test]
fn a_late_payload_that_is_not_the_bytes_declared_writes_nothing() {
    let d = workspace(false);
    let root = d.path();
    let before = snapshot(root);
    let s = replay(
        "late",
        vec![
            (writes("a/first.txt", None, b"1"), Some(b"1")),
            (writes("notes.txt", Some(b"old notes\n"), b"2"), Some(b"2")),
            (writes("scripts/run.sh", Some(b"#!/bin/sh\necho old\n"), b"3"), Some(b"3")),
            (writes("zz/last.txt", None, b"declared last"), Some(b"the payload is other bytes")),
        ],
    );
    let e = transform::run(root, &opts(&s, true)).unwrap_err().to_string();
    assert!(e.contains("zz/last.txt: its payload holds the bytes after") && e.contains(&format!("declared {}", sha(b"declared last"))), "{e}");
    assert_eq!(snapshot(root), before, "zero writes");
    assert!(!root.join("a").exists() && !root.join("zz").exists());
}

/// A write that cannot be staged (a folder it cannot write in) takes back every file it staged and every folder it
/// made: the tree is as it was.
#[test]
fn a_staging_that_fails_takes_back_what_it_staged() {
    let d = workspace(false);
    let root = d.path();
    put(root, "locked/held.txt", b"held");
    let s = replay(
        "staging",
        vec![
            (writes("a.txt", None, b"a"), Some(b"a")),
            (writes("b/new/deep.txt", None, b"b"), Some(b"b")),
            (writes("locked/held.txt", Some(b"held"), b"c"), Some(b"c")),
        ],
    );
    std::fs::set_permissions(root.join("locked"), std::fs::Permissions::from_mode(0o555)).unwrap();
    let before = snapshot(root);
    let e = transform::run(root, &opts(&s, true)).unwrap_err().to_string();
    std::fs::set_permissions(root.join("locked"), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(e.to_lowercase().contains("permission denied"), "{e}");
    let mut now = snapshot(root);
    now.insert("locked".into(), before["locked"].clone());
    assert_eq!(now, before, "nothing staged is left, no folder made stays");
    assert!(!root.join("a.txt").exists() && !root.join("b").exists());
}

// ---------------------------------------------------------------------------
// in a project custodian keeps

fn preflight(root: &Path, spec: &Path) -> (i64, bool) {
    let store = Store::find(root).unwrap();
    let ctx = Ctx { root: store.root.clone(), store: Some(store.clone()), exe: PathBuf::from("custodian") };
    let w = Workspace::open_with(&store.root, &Scope::default(), Resolving::Free).unwrap();
    let ran = custodian_kit::preflight::run_preflight(&ctx, spec, None, Runner::InProcess(&w)).unwrap();
    (ran.receipt.unwrap().seq, ran.preflight.authorized)
}

#[test]
fn in_a_governed_project_a_replay_is_written_only_on_the_preflight_of_this_spec_over_this_code() {
    let d = workspace(true);
    let root = d.path();
    let after = b"pub fn answer() -> u32 {\n    42\n}\n";
    let s = replay("governed", vec![(writes("src/lib.rs", Some(LIB), after), Some(after)), (with_mode(writes("scripts/check.sh", None, b"#!/bin/sh\n"), "755"), Some(b"#!/bin/sh\n"))]);
    let before = snapshot(root);
    // no preflight: refused, nothing written
    let e = transform::run(root, &opts(&s, true)).unwrap_err().to_string();
    assert!(e.contains("CUST0047") || e.contains("no preflight"), "{e}");
    assert_eq!(snapshot(root), before);
    // --no-cargo proves nothing in a governed project: no write
    assert!(transform::run(root, &Options { no_cargo: true, unproven: true, ..opts(&s, true) }).is_err());
    // the preflight authorizes it; a spec altered since is refused on that receipt
    let (seq, authorized) = preflight(root, &s.path);
    assert!(authorized);
    let mut altered = s.spec.clone();
    altered.objective = "another".into();
    std::fs::write(&s.path, altered.to_toml().unwrap()).unwrap();
    let e = transform::run(root, &Options { preflight: Some(seq), ..opts(&s, true) }).unwrap_err().to_string();
    assert!(e.contains("another version of the spec"), "{e}");
    assert_eq!(snapshot(root), before);
    std::fs::write(&s.path, s.spec.to_toml().unwrap()).unwrap();
    // the code moved since the preflight: refused
    put(root, "src/extra.rs", b"// moved\n");
    let e = transform::run(root, &Options { preflight: Some(seq), ..opts(&s, true) }).unwrap_err().to_string();
    assert!(e.contains("the code moved since"), "{e}");
    std::fs::remove_file(root.join("src/extra.rs")).unwrap();
    // on its receipt: written, the record bound to it
    let o = transform::run(root, &Options { preflight: Some(seq), ..opts(&s, true) }).unwrap();
    assert_eq!(o.exit_code, 0, "{}", o.said.join("\n"));
    assert_eq!(o.record.preflight_seq, Some(seq));
    assert_eq!(std::fs::read(root.join("src/lib.rs")).unwrap(), after);
    assert_eq!(mode(root, "scripts/check.sh"), 0o755);
    assert!(o.record_file.as_ref().is_some_and(|f| f.starts_with(root.canonicalize().unwrap().join(".custodian"))), "the project's transforms folder: {:?}", o.record_file);
    assert_eq!(o.record.lock.as_ref().map(|l| l.path.as_str()), Some("Cargo.lock"));
}

#[test]
fn a_batch_leaves_a_replay_out_and_says_why() {
    let d = workspace(false);
    let root = d.path();
    let before = snapshot(root);
    let s = replay("in-a-batch", vec![(writes("src/lib.rs", Some(LIB), b"x"), Some(b"x"))]);
    let said = match transform::run_batch(root, &transform::BatchOptions { specs: vec![s.path.clone()], write: true, ..Default::default() }) {
        Ok(o) => o.excluded.iter().map(|x| format!("{}: {}", x.spec, x.why)).collect::<Vec<_>>().join("; "),
        Err(e) => e.to_string(),
    };
    assert!(said.contains("a replay is made alone (transform --spec), never in a batch"), "{said}");
    assert_eq!(snapshot(root), before);
}

/// The line as a runner runs it, on a spec written by hand: a dry run proves and changes nothing (exit 0); the write
/// writes; a refusal is exit 2 with the reason, and nothing written.
#[test]
fn the_transform_line_runs_a_replay_spec_written_by_hand() {
    const BIN: &str = env!("CARGO_BIN_EXE_code-passenger");
    let d = workspace(false);
    let root = d.path();
    let specs = tempfile::tempdir().unwrap();
    put(specs.path(), "payloads/run.sh", b"#!/bin/sh\necho new\n");
    let text = format!(
        "name = \"by-hand\"\nobjective = \"the kept state\"\noperation = \"replay\"\ntarget = \"*\"\n\n[[replay]]\npath = \"scripts/run.sh\"\nbefore_sha256 = \"{}\"\nafter_sha256 = \"{}\"\npayload = \"payloads/run.sh\"\nmode = \"750\"\n\n[[replay]]\npath = \"notes.txt\"\nbefore_sha256 = \"{}\"\ndelete = true\n",
        sha(b"#!/bin/sh\necho old\n"),
        sha(b"#!/bin/sh\necho new\n"),
        sha(b"old notes\n")
    );
    let spec = specs.path().join("by-hand.toml");
    std::fs::write(&spec, &text).unwrap();
    let before = snapshot(root);
    let run = |extra: &[&str]| std::process::Command::new(BIN).current_dir(root).args(["--root", ".", "transform", "--spec"]).arg(&spec).args(extra).output().unwrap();
    let o = run(&[]);
    assert_eq!(o.status.code(), Some(0), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    assert!(String::from_utf8_lossy(&o.stdout).contains("replay by-hand: 2 files (1 written, 0 of them made; 1 deleted)"), "{}", String::from_utf8_lossy(&o.stdout));
    assert_eq!(snapshot(root), before, "a dry run changes nothing");
    let o = run(&["--write"]);
    assert_eq!(o.status.code(), Some(0), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    assert_eq!(std::fs::read(root.join("scripts/run.sh")).unwrap(), b"#!/bin/sh\necho new\n");
    assert_eq!(mode(root, "scripts/run.sh"), 0o750);
    assert!(!root.join("notes.txt").exists());
    // run again: the files are no longer as declared before
    let after = snapshot(root);
    let o = run(&["--write"]);
    assert_eq!(o.status.code(), Some(2), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    assert!(String::from_utf8_lossy(&o.stderr).contains("scripts/run.sh: as declared before: sha256 "), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(snapshot(root), after);
}

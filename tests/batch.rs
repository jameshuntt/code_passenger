// Copyright © 2026 1o. All rights reserved.

//! The proof's shape (tests/fixtures/batch_ws: `rules`, which `user` depends on, which `far` depends on; four bodies
//! of one shape in rules; one rules test that fails before any change).
//!
//! - The proof compiles the crate a change lands in (its tests too) and each crate that depends on it directly, never
//!   a dependent's dependent: a body swapped in rules is checked in rules and user, not far.
//! - One proof place per workspace: the copy keeps its path and the build is kept, and a sync writes only the files
//!   whose bytes differ, so cargo compiles again only what moved.
//! - A swap's expansion before and after is a postcondition of the transform; the crate's tests are compared with the
//!   same tests before the change: a test failing the same before does not stop it, one it breaks does.
//! - James's rule for a repeated shape: one member proved alone, the rest as one batch (one check, one expansion per
//!   crate, one record per member); a member that is not the same tokens is left out and named, the rest land; a
//!   member is reverted alone.

mod common;

use code_passenger::proof_dir::ProofDir;
use code_passenger::transform::{self, BatchOptions, Options};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn spec(root: &Path, name: &str, function: &str, condition: &str, ret: &str, value: &str, same: bool) -> PathBuf {
    let p = root.join(".specs").join(format!("{name}.toml"));
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    let text = format!(
        "name = \"{name}\"\nobjective = \"o\"\noperation = \"replace_body\"\ntarget = \"rules::{function}\"\nsame_expansion = {same}\n\n[scope]\nwrite = [\"crate:rules\"]\n\n[function]\nname = \"{function}\"\n\n[[function.body]]\npiece = \"return_if\"\ncondition = {condition:?}\nreturn_value = {ret:?}\n\n[[function.body]]\npiece = \"value\"\nvalue = {value:?}\n"
    );
    std::fs::write(&p, text).unwrap();
    p
}

fn lib(root: &Path) -> String {
    common::read(&root.join("rules/src/lib.rs"))
}

#[test]
fn one_swap_is_proved_on_the_crate_and_its_direct_dependents_with_its_expansion_and_the_tests_against_the_base() {
    let d = common::fresh_copy("batch_ws");
    let root = d.path();
    let s = spec(root, "port", "port", "n == 0", "Err(\"no port\")", "Ok(n)", true);
    let out = root.join(".records/port.json");
    let o = transform::run(root, &Options { spec: s, write: true, tests: true, out: Some(out.clone()), ..Options::default() }).unwrap();
    assert_eq!(o.exit_code, 0, "{}", o.said.join("\n"));
    let checked: std::collections::BTreeSet<&str> = o.record.checks.iter().map(|c| c.package.as_str()).collect();
    assert_eq!(checked, ["rules", "user"].into_iter().collect(), "the crate and its direct dependent, never far: {:?}", o.record.checks);
    assert!(o.record.postconditions.iter().any(|(p, h)| p == "expands rules::port the same" && *h), "{:?}", o.record.postconditions);
    assert!(o.record.postconditions.iter().any(|(p, h)| p == "tests rules as before" && *h), "{:?}", o.record.postconditions);
    let tests: code_passenger::proof_dir::Compared = serde_json::from_str(&common::read(&transform::tests_file(&out))).unwrap();
    assert_eq!(tests.failing_at_base, ["tests::needs_what_a_laptop_has_not"], "the test failing before the change is not its");
    assert!(tests.new_failures.is_empty());
    let e: custodian_kit::transform::Expansion = serde_json::from_str(&common::read(&transform::expansion_file(&out))).unwrap();
    assert!(e.same && !e.before.is_empty());
    assert!(lib(root).contains("    simple_impl::return_if!(n == 0 => Err(\"no port\"));\n    Ok(n)\n"), "{}", lib(root));

    // a change that is not the same tokens is refused by its postcondition, and nothing is written
    let s = spec(root, "level", "level", "n >= 2", "Err(\"over two\")", "Ok(n)", true);
    let o = transform::run(root, &Options { spec: s, write: true, tests: true, ..Options::default() }).unwrap();
    assert_eq!(o.exit_code, 1);
    assert!(o.record.postconditions.iter().any(|(p, h)| p == "expands rules::level the same" && !*h), "{:?}", o.record.postconditions);
    assert!(lib(root).contains("    if n > 2 {\n"), "left as it was");

    // a change that breaks a test that passed before is refused by the tests (it keeps no expansion)
    let s = spec(root, "size", "size", "n > 9", "Err(\"too large\")", "Ok(n)", false);
    let o = transform::run(root, &Options { spec: s, write: true, tests: true, ..Options::default() }).unwrap();
    assert_eq!(o.exit_code, 1, "{}", o.said.join("\n"));
    assert!(o.record.postconditions.iter().any(|(p, h)| p == "tests rules as before" && !*h), "{:?}", o.record.postconditions);
    assert!(lib(root).contains("        return Err(\"too big\");\n"), "left as it was");
}

#[test]
fn the_proof_place_is_kept_and_a_sync_writes_only_what_differs() {
    let d = common::fresh_copy("batch_ws");
    let root = d.path();
    let far = {
        let p = ProofDir::take(root).unwrap();
        assert!(p.root().join("far/src/lib.rs").is_file());
        p.root().join("far/src/lib.rs")
    };
    let first = std::fs::metadata(&far).unwrap().modified().unwrap();
    std::fs::write(root.join("user/src/lib.rs"), "//! moved\n\npub fn open(n: u16) -> Result<u16, &'static str> {\n    rules::port(n)\n}\n").unwrap();
    std::fs::write(root.join("user/src/extra.rs"), "").unwrap();
    let p = ProofDir::take(root).unwrap();
    assert_eq!(p.root().join("far/src/lib.rs"), far, "the same place");
    assert_eq!(std::fs::metadata(&far).unwrap().modified().unwrap(), first, "an unchanged file is not written again");
    assert!(common::read(&p.root().join("user/src/lib.rs")).starts_with("//! moved"));
    drop(p);
    std::fs::remove_file(root.join("user/src/extra.rs")).unwrap();
    let p = ProofDir::take(root).unwrap();
    assert!(!p.root().join("user/src/extra.rs").exists(), "a file the workspace no longer has goes from the copy");
    assert!(p.target().ends_with("target"));
}

#[test]
fn a_shape_proved_once_goes_as_one_batch_and_a_member_not_the_same_is_left_out() {
    let d = common::fresh_copy("batch_ws");
    let root = d.path();
    // the one member of the shape alone, fully proved
    let s = spec(root, "port", "port", "n == 0", "Err(\"no port\")", "Ok(n)", true);
    let o = transform::run(root, &Options { spec: s, write: true, tests: true, out: Some(root.join(".records/port.json")), ..Options::default() }).unwrap();
    assert_eq!(o.exit_code, 0, "{}", o.said.join("\n"));
    // the rest as one batch, one member written wrong
    let specs = vec![
        spec(root, "level", "level", "n > 2", "Err(\"over two\")", "Ok(n)", true),
        spec(root, "size", "size", "n >= 9", "Err(\"too big\")", "Ok(n)", true),
        spec(root, "name", "name", "s.is_empty()", "Err(\"no name\")", "Ok(s)", true),
    ];
    let out_dir = root.join(".records/batch");
    let b = transform::run_batch(root, &BatchOptions { specs, write: true, out_dir: Some(out_dir.clone()), preflights: BTreeMap::new(), after_proof: Some(7), tests: true }).unwrap();
    assert_eq!(b.exit_code, 0, "{}", b.said.join("\n"));
    assert!(b.said[0].starts_with("batch of 3 after proof #7"), "{:?}", b.said);
    assert_eq!(b.excluded.len(), 1, "{:?}", b.excluded);
    assert_eq!(b.excluded[0].spec, "size");
    assert!(b.excluded[0].why.contains("expands rules::size the same"), "{}", b.excluded[0].why);
    let written: Vec<&str> = b.records.iter().filter(|(r, _)| r.written).map(|(r, _)| r.spec_name.as_str()).collect();
    assert_eq!(written, ["level", "name"]);
    assert!(b.records.iter().all(|(r, f)| r.proved() && f.as_ref().is_some_and(|f| f.is_file() && transform::expansion_file(f).is_file())));
    let t = lib(root);
    assert!(t.contains("simple_impl::return_if!(n > 2 => Err(\"over two\"));") && t.contains("simple_impl::return_if!(s.is_empty() => Err(\"no name\"));"), "{t}");
    assert!(t.contains("    if n > 9 {\n"), "the member left out is as it was: {t}");
    // one member reverted alone, the batch's other member and the proof's swap kept
    let r = transform::revert(root, &out_dir.join("level.json")).unwrap();
    assert_eq!(r.exit_code, 0);
    let t = lib(root);
    assert!(t.contains("    if n > 2 {\n        return Err(\"over two\");\n    }\n"), "{t}");
    assert!(t.contains("simple_impl::return_if!(s.is_empty() => Err(\"no name\"));") && t.contains("simple_impl::return_if!(n == 0 => Err(\"no port\"));"), "{t}");
}

#[test]
fn the_toolkit_batch_line_finds_each_member_again_and_swaps_the_shape_under_one_proof() {
    let d = common::fresh_copy("batch_ws");
    let root = d.path();
    let words = |w: &[&str]| w.iter().map(std::ffi::OsString::from).collect::<Vec<_>>();
    let exe = Path::new(env!("CARGO_BIN_EXE_code-passenger"));
    let r = root.display().to_string();
    // the one function of the shape alone (to-toolkit: the matcher's spec, the transform's proof)
    let one = code_passenger::lines::run_argv(&words(&["--root", &r, "to-toolkit", "--function", "rules::port", "--write"]), exe);
    assert_eq!(one.code, 0, "{}{}", one.stdout, one.stderr);
    assert!(one.stdout.contains("cargo expand: the function after is the same tokens as before"), "{}", one.stdout);
    // the rest, one function a line; a near-only function and one not in the code are left out and named
    let list = root.join(".batch");
    std::fs::write(&list, "rules::level\nrules::size\nrules::name\nrules::nowhere\n").unwrap();
    let out = root.join(".records/rules-return_if");
    let l = list.display().to_string();
    let o = out.display().to_string();
    let b = code_passenger::lines::run_argv(&words(&["--root", &r, "transform", "--batch", &l, "--toolkit", "--after-proof", "12", "--out", &o, "--write"]), exe);
    assert_eq!(b.code, 0, "{}{}", b.stdout, b.stderr);
    assert!(b.stdout.contains("to-toolkit batch: 3 members swapped"), "{}", b.stdout);
    assert!(b.stdout.contains("left out: rules::nowhere"), "{}", b.stdout);
    let rec: serde_json::Value = serde_json::from_str(&common::read(&out.join("to_toolkit_batch.json"))).unwrap();
    assert_eq!(rec["after_proof"], 12);
    assert_eq!(rec["members"].as_array().unwrap().len(), 3);
    assert!(rec["members"].as_array().unwrap().iter().all(|m| m["same_expansion"] == true));
    assert_eq!(std::fs::read_dir(out.join("transforms")).unwrap().filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().ends_with(".expansion.json")).count(), 3, "one expansion record per member");
    let t = lib(root);
    assert_eq!(t.matches("simple_impl::return_if!(").count(), 4, "{t}");
}

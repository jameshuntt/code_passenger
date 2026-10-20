// Copyright © 2026 1o. All rights reserved.

//! The manifest operations a bounded job needs for crate features (tests/fixtures/transform_more): `declare_feature`
//! writes `[features]`, and a `dep:` entry makes its dependency optional in the same step (cargo takes `dep:` only
//! for an optional one), so a feature named like its dependency (`shop-marks = ["dep:shop-marks"]`) is written;
//! `optional` on `add_dependency` and `change_dependency`; an inline entry keeps cargo's spacing when a key is added.

mod common;

use code_passenger::commands::edit;
use code_passenger::transform::{self, Options};
use std::path::{Path, PathBuf};

fn spec(root: &Path, name: &str, text: &str) -> PathBuf {
    let p = root.join(format!("{name}.toml"));
    std::fs::write(&p, text).unwrap();
    p
}

fn apply(root: &Path, spec: &Path) -> transform::Outcome {
    let out = Some(root.join(".records").join(format!("{}.json", spec.file_stem().unwrap().to_string_lossy())));
    let o = transform::run(root, &Options { spec: spec.to_path_buf(), write: true, out, ..Options::default() }).unwrap_or_else(|e| panic!("{}: {e}", spec.display()));
    assert_eq!(o.exit_code, 0, "{}", o.said.join("\n"));
    assert!(o.record.written && o.record.proved(), "{:?}", o.record);
    o
}

fn manifest(root: &Path) -> String {
    std::fs::read_to_string(root.join("shop/Cargo.toml")).unwrap()
}

#[test]
fn a_feature_is_declared_and_its_dependency_made_optional_in_one_step() {
    let d = common::fresh_copy("transform_more");
    let root = d.path();
    apply(root, &spec(root, "marks", "name = \"marks\"\nobjective = \"o\"\noperation = \"add_dependency\"\ntarget = \"crate:shop\"\n\n[dependency]\nname = \"shop-marks\"\npath = \"../marks\"\n"));
    // a feature that only gates code, then the same feature turning the dependency on: its list replaced
    let o = apply(root, &spec(root, "gates", "name = \"gates\"\nobjective = \"o\"\noperation = \"declare_feature\"\ntarget = \"crate:shop\"\nfeature = \"marked\"\n"));
    assert!(manifest(root).ends_with("[features]\nmarked = []\n"), "{}", manifest(root));
    assert!(o.record.postconditions.iter().all(|(_, h)| *h), "{:?}", o.record.postconditions);
    apply(root, &spec(root, "turns", "name = \"turns\"\nobjective = \"o\"\noperation = \"declare_feature\"\ntarget = \"crate:shop\"\nfeature = \"marked\"\nenables = [\"dep:shop-marks\"]\n"));
    assert!(manifest(root).contains("shop-marks = { path = \"../marks\", optional = true }\n"), "{}", manifest(root));
    assert!(manifest(root).ends_with("[features]\nmarked = [\"dep:shop-marks\"]\n"), "{}", manifest(root));
}

#[test]
fn a_feature_named_like_the_dependency_it_turns_on_is_written() {
    let d = common::fresh_copy("transform_more");
    let root = d.path();
    apply(root, &spec(root, "marks", "name = \"marks\"\nobjective = \"o\"\noperation = \"add_dependency\"\ntarget = \"crate:shop\"\n\n[dependency]\nname = \"shop-marks\"\npath = \"../marks\"\n"));
    let o = apply(root, &spec(root, "named", "name = \"named\"\nobjective = \"o\"\noperation = \"declare_feature\"\ntarget = \"crate:shop\"\nfeature = \"shop-marks\"\nenables = [\"dep:shop-marks\"]\n"));
    assert!(manifest(root).contains("shop-marks = { path = \"../marks\", optional = true }\n[features]\nshop-marks = [\"dep:shop-marks\"]\n") || manifest(root).contains("shop-marks = { path = \"../marks\", optional = true }\n\n[features]\nshop-marks = [\"dep:shop-marks\"]\n"), "{}", manifest(root));
    assert!(o.record.postconditions.iter().any(|(p, h)| p == "optional shop shop_marks true" && *h), "{:?}", o.record.postconditions);
    // refused: a feature turning on a dependency the crate does not have
    let s = spec(root, "none", "name = \"none\"\nobjective = \"o\"\noperation = \"declare_feature\"\ntarget = \"crate:shop\"\nfeature = \"json\"\nenables = [\"dep:serde_json\"]\n");
    let e = transform::run(root, &Options { spec: s, ..Options::default() }).map(|o| o.exit_code).unwrap_err();
    assert!(e.to_string().contains("has no dependency serde_json"), "{e}");
}

#[test]
fn optional_on_add_dependency_and_change_dependency() {
    let d = common::fresh_copy("transform_more");
    let root = d.path();
    let lock_before = std::fs::read_to_string(root.join("Cargo.lock")).unwrap_or_default();
    let o = apply(root, &spec(root, "marks", "name = \"marks\"\nobjective = \"o\"\noperation = \"add_dependency\"\ntarget = \"crate:shop\"\n\n[dependency]\nname = \"shop-marks\"\npath = \"../marks\"\noptional = true\n"));
    assert!(manifest(root).contains("shop-marks = { path = \"../marks\", optional = true }\n"), "{}", manifest(root));
    // the lock cargo wrote in the proof is written with the manifest: the tree's lock names the new dependency, so a
    // frozen `cargo metadata` (how a scan finds dependency macros) reads it
    let lock = std::fs::read_to_string(root.join("Cargo.lock")).unwrap();
    assert!(lock != lock_before && lock.contains("[[package]]\nname = \"shop\"\nversion = \"0.1.0\"\ndependencies = [\n \"shop-marks\",\n]"), "{lock}");
    assert!(o.record.files.iter().any(|f| f.path == "Cargo.lock"), "{:?}", o.record.files);
    let frozen = std::process::Command::new("cargo").args(["metadata", "--frozen", "--format-version", "1"]).current_dir(root).output().unwrap();
    assert!(frozen.status.success(), "{}", String::from_utf8_lossy(&frozen.stderr));
    let o = apply(root, &spec(root, "required", "name = \"required\"\nobjective = \"o\"\noperation = \"change_dependency\"\ntarget = \"crate:shop\"\n\n[dependency]\nname = \"shop-marks\"\noptional = false\n"));
    assert!(manifest(root).contains("shop-marks = { path = \"../marks\" }\n"), "the source kept, optional dropped: {}", manifest(root));
    assert!(o.record.postconditions.iter().any(|(p, h)| p == "optional shop shop_marks false" && *h), "{:?}", o.record.postconditions);
}

#[test]
fn a_key_added_to_an_inline_entry_keeps_cargos_spacing() {
    let text = "[package]\nname = \"pantry\"\n\n[dependencies]\nserde = { version = \"1\", features = [\"derive\"] }\nchrono = \"0.4\"\n\n[features]\nchrono = []\n";
    let after = edit::feature_declare_text(text, "json", &["dep:serde".into()], &["serde".into()]).unwrap();
    assert!(after.contains("serde = { version = \"1\", features = [\"derive\"], optional = true }\n"), "{after}");
    let after = edit::feature_declare_text(&after, "chrono", &["dep:chrono".into()], &["chrono".into()]).unwrap();
    assert!(after.contains("chrono = { version = \"0.4\", optional = true }\n"), "{after}");
    assert!(after.ends_with("[features]\nchrono = [\"dep:chrono\"]\njson = [\"dep:serde\"]\n"), "{after}");
}

#[test]
fn make_lib_gives_a_binary_only_crate_its_library() {
    let d = common::fresh_copy("transform_more");
    let root = d.path();
    let o = apply(root, &spec(root, "lib", "name = \"lib\"\nobjective = \"o\"\noperation = \"make_lib\"\ntarget = \"crate:cli\"\n\n[module]\nname = \"lib\"\ndoc = \"What the shop's command line does.\"\n"));
    assert_eq!(std::fs::read_to_string(root.join("cli/src/lib.rs")).unwrap(), "//! What the shop's command line does.\n");
    assert!(o.record.postconditions.iter().any(|(p, h)| p == "library cli" && *h), "{:?}", o.record.postconditions);
    assert!(std::fs::read_to_string(root.join("cli/src/main.rs")).unwrap().starts_with("fn main()") || !std::fs::read_to_string(root.join("cli/src/main.rs")).unwrap().contains("mod lib"), "the binary gets no `mod lib;`");
    // reversible: the file it made is taken away again
    transform::revert(root, o.record_file.as_ref().unwrap()).unwrap();
    assert!(!root.join("cli/src/lib.rs").exists());
}

#[test]
fn change_signature_with_a_body_writes_both_in_one_step() {
    // the pantry's `main` that now returns `Result`: its old body does not compile against the new signature, so the
    // two are one change
    let d = common::fresh_copy("transform_more");
    let root = d.path();
    apply(root, &spec(root, "fallible", "name = \"fallible\"\nobjective = \"o\"\noperation = \"change_signature\"\ntarget = \"shopcli::main\"\n\n[function]\nname = \"main\"\nreturns = \"Result<(), String>\"\nbody = [\n  { piece = \"let\", bind = \"v\", value = \"shop::version()\" },\n  { piece = \"ensure\", condition = \"v > 0\", error = \"\\\"no version\\\".to_string()\" },\n  { piece = \"value\", value = \"Ok(())\" },\n]\n"));
    let main = std::fs::read_to_string(root.join("cli/src/main.rs")).unwrap();
    assert_eq!(main, "fn main() -> Result<(), String> {\n    let v = shop::version();\n    simple_impl::ensure!(v > 0, \"no version\".to_string());\n    Ok(())\n}\n", "{main}");
}

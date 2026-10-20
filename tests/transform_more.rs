// Copyright © 2026 1o. All rights reserved.

//! `transform` on the cases the exercise found (tests/fixtures/transform_more: `shop` with a module that has its own
//! file and a depot that imports from it, `cli` with only a binary, `packed` with a struct under an attribute macro
//! from `shop-marks`): a new module's folder made, a binary checked and its calls inside macros reached, a method's
//! calls found by their receivers, a moved item's imports dropped where it lands and carried from where it was, a
//! dependency's name read with `-` and `_` as one, an attribute macro refused by name, the tests compiled with the
//! proof, a dry run's receipt kept in the open job; and the two operations `inline` and `wrap`, each reversible.

mod common;

use code_passenger::transform::{self, Options};
use std::path::{Path, PathBuf};

fn spec(root: &Path, name: &str, text: &str) -> PathBuf {
    let p = root.join(format!("{name}.toml"));
    std::fs::write(&p, text).unwrap();
    p
}

fn opts(root: &Path, spec: &Path, write: bool) -> Options {
    Options { spec: spec.to_path_buf(), write, out: Some(root.join(".records").join(format!("{}.json", spec.file_stem().unwrap().to_string_lossy()))), ..Options::default() }
}

fn apply(root: &Path, spec: &Path) -> transform::Outcome {
    let o = transform::run(root, &opts(root, spec, true)).unwrap_or_else(|e| panic!("{}: {e}", spec.display()));
    assert_eq!(o.exit_code, 0, "{}", o.said.join("\n"));
    assert!(o.record.written && o.record.proved(), "{:?}", o.record);
    o
}

fn read(root: &Path, rel: &str) -> String {
    std::fs::read_to_string(root.join(rel)).unwrap()
}

#[test]
fn a_module_of_a_module_with_its_own_file_gets_its_folder() {
    let d = common::fresh_copy("transform_more");
    let root = d.path();
    assert!(!root.join("shop/src/stock").exists());
    let o = apply(root, &spec(root, "shelf", "name = \"shelf\"\nobjective = \"o\"\noperation = \"add_module\"\ntarget = \"crate:shop/src/stock.rs\"\n\n[module]\nname = \"shelf\"\nvis = \"pub\"\ndoc = \"Where items sit.\"\n"));
    assert_eq!(read(root, "shop/src/stock/shelf.rs"), "//! Where items sit.\n");
    assert!(read(root, "shop/src/stock.rs").contains("pub mod shelf;\n"));
    assert!(o.record.files.iter().any(|f| f.path == "shop/src/stock/shelf.rs" && f.before == "none"));
    // extract into the same folder
    let x = apply(root, &spec(root, "having", "name = \"having\"\nobjective = \"o\"\noperation = \"extract\"\ntarget = \"crate:shop/src/stock.rs\"\nitems = [\"shop::stock::has\"]\n\n[module]\nname = \"having\"\n"));
    assert!(read(root, "shop/src/stock/having.rs").contains("pub fn has(i: &Item, key: &str) -> bool {"));
    // both reversible, the last first: the files they made are taken away again
    transform::revert(root, x.record_file.as_ref().unwrap()).unwrap();
    transform::revert(root, o.record_file.as_ref().unwrap()).unwrap();
    assert!(!root.join("shop/src/stock/shelf.rs").exists() && !root.join("shop/src/stock/having.rs").exists());
    assert_eq!(read(root, "shop/src/stock.rs"), std::fs::read_to_string(common::fixture("transform_more").join("shop/src/stock.rs")).unwrap());
}

#[test]
fn a_crate_with_only_a_binary_is_checked_and_a_call_inside_a_macro_gets_the_new_argument() {
    let d = common::fresh_copy("transform_more");
    let root = d.path();
    let o = apply(root, &spec(root, "quiet", "name = \"quiet\"\nobjective = \"o\"\noperation = \"add_attribute\"\ntarget = \"shopcli::main\"\nattribute = \"#[allow(dead_code)]\"\n"));
    assert!(o.record.checks.iter().any(|c| c.package == "cli" && c.ok), "the binary's package is checked: {:?}", o.record.checks);
    assert!(read(root, "cli/src/main.rs").starts_with("#[allow(dead_code)]\nfn main() {"));
    let o = apply(root, &spec(root, "versioned", "name = \"versioned\"\nobjective = \"o\"\noperation = \"change_signature\"\ntarget = \"shop::version\"\n\n[function]\nname = \"version\"\nvis = \"pub\"\nparams = [{ name = \"minor\", ty = \"u32\" }]\nreturns = \"u32\"\n\n[call_sites]\npass = \"0\"\n"));
    assert!(read(root, "cli/src/main.rs").contains("println!(\"shop {}\", shop::version(0));"), "{}", read(root, "cli/src/main.rs"));
    assert!(o.record.checks.iter().any(|c| c.package == "cli"));
}

#[test]
fn a_tree_nothing_checked_is_never_proved() {
    let d = common::fresh_copy("transform_more");
    let root = d.path();
    // --no-cargo without --unproven: the postconditions are read on a written copy, and nothing is proved
    let s = spec(root, "quiet", "name = \"quiet\"\nobjective = \"o\"\noperation = \"add_attribute\"\ntarget = \"shopcli::main\"\nattribute = \"#[allow(dead_code)]\"\n");
    let o = transform::run(root, &Options { no_cargo: true, ..opts(root, &s, false) }).unwrap();
    assert!(o.record.postconditions.iter().all(|(_, h)| *h), "read on the copy the edits went to: {:?}", o.record.postconditions);
    assert_eq!(o.exit_code, 1);
}

#[test]
fn a_methods_calls_are_found_by_their_receivers_inside_macros_too() {
    let d = common::fresh_copy("transform_more");
    let root = d.path();
    let o = apply(root, &spec(root, "count-at", "name = \"count-at\"\nobjective = \"o\"\noperation = \"change_signature\"\ntarget = \"shop::stock::Item::count\"\n\n[function]\nname = \"count\"\nvis = \"pub\"\nreceiver = \"&self\"\nparams = [{ name = \"key\", ty = \"&str\" }, { name = \"floor\", ty = \"u32\" }]\nreturns = \"u32\"\n\n[call_sites]\npass = \"0\"\n"));
    let t = read(root, "shop/src/stock.rs");
    assert!(t.contains("    i.count(key, 0) > 0\n"), "{t}");
    assert!(t.contains("assert_eq!(i.count(\"a\", 0), 0);"), "the call in the test's macro: {t}");
    assert!(o.record.postconditions.contains(&("args shop::stock::Item::count 3".to_string(), true)), "{:?}", o.record.postconditions);
}

#[test]
fn a_moved_item_drops_the_import_where_it_lands_and_takes_the_ones_it_used() {
    let d = common::fresh_copy("transform_more");
    let root = d.path();
    let o = apply(root, &spec(root, "mv", "name = \"mv\"\nobjective = \"o\"\noperation = \"move\"\ntarget = \"shop::stock::Item\"\nto = \"crate:shop/src/depot.rs\"\n"));
    let depot = read(root, "shop/src/depot.rs");
    assert!(!depot.contains("use crate::stock::Item;"), "{depot}");
    assert!(depot.contains("use std::collections::BTreeMap;") && depot.contains("pub struct Item {"), "{depot}");
    assert!(read(root, "shop/src/stock.rs").contains("pub use crate::depot::Item;"));
    assert!(o.said.iter().any(|l| l.contains("its import dropped")), "{:?}", o.said);
}

#[test]
fn a_dependency_named_with_a_dash_is_the_one_cargo_names_with_an_underscore() {
    let d = common::fresh_copy("transform_more");
    let root = d.path();
    let o = apply(root, &spec(root, "marks", "name = \"marks\"\nobjective = \"o\"\noperation = \"add_dependency\"\ntarget = \"crate:shop\"\n\n[dependency]\nname = \"shop-marks\"\npath = \"../marks\"\n"));
    assert!(o.record.postconditions.contains(&("dependency shop shop-marks".to_string(), true)), "{:?}", o.record.postconditions);
}

#[test]
fn a_struct_under_an_attribute_macro_is_refused_by_the_macros_name() {
    let d = common::fresh_copy("transform_more");
    let root = d.path();
    let field = spec(root, "wide", "name = \"wide\"\nobjective = \"o\"\noperation = \"add_field\"\ntarget = \"packed::Packed\"\n\n[field]\nname = \"more\"\nty = \"u8\"\ndefault = \"0\"\n");
    let e = transform::run(root, &opts(root, &field, true)).unwrap_err().to_string();
    assert!(e.contains("[CUST0040]") && e.contains("#[keep] (shop_marks::keep)"), "{e}");
    let ctor = spec(root, "ctor", "name = \"ctor\"\nobjective = \"o\"\noperation = \"add_constructor\"\ntarget = \"packed::Packed\"\n\n[function]\nname = \"of_bits\"\nvis = \"pub\"\nparams = [{ name = \"bits\", ty = \"u8\" }]\n");
    assert!(transform::run(root, &opts(root, &ctor, true)).unwrap_err().to_string().contains("[CUST0040]"));
    // a method is not a field: it is written as it would be anywhere
    apply(root, &spec(root, "m", "name = \"m\"\nobjective = \"o\"\noperation = \"add_method\"\ntarget = \"packed::Packed\"\n\n[function]\nname = \"bits\"\nvis = \"pub\"\nreceiver = \"&self\"\nreturns = \"u8\"\n\n[[function.body]]\npiece = \"value\"\nvalue = \"self.bits\"\n"));
}

#[test]
fn the_proof_compiles_the_tests_too() {
    let d = common::fresh_copy("transform_more");
    let root = d.path();
    let s = spec(root, "bad-test", "name = \"bad-test\"\nobjective = \"o\"\noperation = \"add_test\"\ntarget = \"shop::version\"\n\n[function]\nname = \"wrong_type\"\n\n[[function.body]]\npiece = \"raw\"\ncode = \"let _n: u32 = \\\"one\\\";\"\n");
    let o = transform::run(root, &opts(root, &s, true)).unwrap();
    assert_eq!(o.exit_code, 1, "a test that does not compile is not proved: {}", o.said.join("\n"));
    assert!(!o.record.written);
    assert!(o.said.iter().any(|l| l.contains("mismatched types")), "{:?}", o.said);
}

#[test]
fn a_dry_run_in_an_open_job_keeps_its_receipt_under_the_job() {
    use custodian_kit::store::{JobState, Mode, Store};
    let d = common::fresh_copy("transform_more");
    let root = d.path().canonicalize().unwrap();
    let (store, _) = Store::init(&root, Mode::Agent).unwrap();
    let folder = store.job_dir(JobState::Running, "j").unwrap();
    std::fs::create_dir_all(&folder).unwrap();
    let jf = custodian_kit::jobs::JobFile { id: Some("job-open".into()), name: Some("j".into()), ..Default::default() };
    std::fs::write(folder.join(custodian_kit::jobs::job_file_name()), custodian_kit::oneo::render(custodian_kit::oneo::Doc::Job, &jf).unwrap()).unwrap();
    let s = spec(&root, "quiet", "name = \"quiet\"\nobjective = \"o\"\noperation = \"add_attribute\"\ntarget = \"shopcli::main\"\nattribute = \"#[allow(dead_code)]\"\n");
    let args: Vec<std::ffi::OsString> = vec!["--root".into(), root.clone().into(), "transform".into(), "--spec".into(), s.into()];
    let out = code_passenger::lines::run_argv(&args, Path::new("code-passenger"));
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    let kept = out.kept.expect("kept in the journal");
    let r: custodian_kit::store::Receipt = serde_json::from_str(&std::fs::read_to_string(kept).unwrap()).unwrap();
    assert_eq!(r.job.as_deref(), Some("job-open"));
}

#[test]
fn inline_puts_the_body_at_every_call_and_removes_the_private_function() {
    let d = common::fresh_copy("transform_more");
    let root = d.path();
    let before = read(root, "shop/src/lib.rs");
    let o = apply(root, &spec(root, "in", "name = \"in\"\nobjective = \"o\"\noperation = \"inline\"\ntarget = \"shop::double\"\n"));
    let t = read(root, "shop/src/lib.rs");
    assert!(!t.contains("fn double"), "{t}");
    assert!(t.contains("{ let n: u32 = o; let twice = n * 2;\n    twice }"), "{t}");
    assert!(t.contains("assert_eq!({ let n: u32 = 2; let twice = n * 2;\n    twice }, 4);"), "the call in the test's macro: {t}");
    assert!(o.record.postconditions.contains(&("calls shop::double 0".to_string(), true)) && o.record.postconditions.contains(&("absent shop::double".to_string(), true)), "{:?}", o.record.postconditions);
    transform::revert(root, o.record_file.as_ref().unwrap()).unwrap();
    assert_eq!(read(root, "shop/src/lib.rs"), before);
}

#[test]
fn inline_refuses_what_would_leave_the_caller_and_keeps_a_pub_function() {
    let d = common::fresh_copy("transform_more");
    let root = d.path();
    let lib = root.join("shop/src/lib.rs");
    std::fs::write(&lib, read(root, "shop/src/lib.rs") + "\nfn early(n: u32) -> u32 {\n    if n > 3 {\n        return 3;\n    }\n    n\n}\n\npub fn capped() -> u32 {\n    early(9)\n}\n\npub fn one() -> u32 {\n    1\n}\n\npub fn two() -> u32 {\n    one() + one()\n}\n").unwrap();
    let e = transform::run(root, &opts(root, &spec(root, "e", "name = \"e\"\nobjective = \"o\"\noperation = \"inline\"\ntarget = \"shop::early\"\n"), true)).unwrap_err().to_string();
    assert!(e.contains("returns early"), "{e}");
    let o = apply(root, &spec(root, "one", "name = \"one\"\nobjective = \"o\"\noperation = \"inline\"\ntarget = \"shop::one\"\n"));
    let t = read(root, "shop/src/lib.rs");
    assert!(t.contains("pub fn one() -> u32 {") && t.contains("({ 1 }) + { 1 }"), "{t}");
    assert!(o.said.iter().any(|l| l.contains("kept: it is pub")), "{:?}", o.said);
}

#[test]
fn wrap_runs_the_pieces_first_and_keeps_the_body_as_its_value() {
    let d = common::fresh_copy("transform_more");
    let root = d.path();
    let before = read(root, "shop/src/lib.rs");
    let o = apply(root, &spec(root, "w", "name = \"w\"\nobjective = \"o\"\noperation = \"wrap\"\ntarget = \"shop::twice_total\"\n\n[function]\nname = \"twice_total\"\n\n[[function.body]]\npiece = \"let\"\nbind = \"_seen\"\nvalue = \"o\"\n"));
    let t = read(root, "shop/src/lib.rs");
    assert!(t.contains("pub fn twice_total(o: u32) -> u32 {\n    let _seen = o;\n    {\n        double(o) + double(1)\n    }\n}\n"), "{t}");
    let refused = spec(root, "w2", "name = \"w2\"\nobjective = \"o\"\noperation = \"wrap\"\ntarget = \"shop::version\"\n\n[function]\nname = \"version\"\n\n[[function.body]]\npiece = \"value\"\nvalue = \"2\"\n");
    assert!(transform::run(root, &opts(root, &refused, true)).unwrap_err().to_string().contains("a wrapped body's value is the body it wraps"));
    transform::revert(root, o.record_file.as_ref().unwrap()).unwrap();
    assert_eq!(read(root, "shop/src/lib.rs"), before);
}

/// shop's tests call `version` inside a function-like proc macro's arguments (`shop_marks::same!`), which klepto
/// cannot expand: it names the paths there and resolves none. custodian_kit's `change::macro_calls` resolves them from
/// the test module (`use super::*`, `crate::`), and the new argument reaches both calls, one of them over two lines.
fn calls_in_a_proc_macro(root: &Path) {
    let marks = read(root, "marks/src/lib.rs");
    std::fs::write(root.join("marks/src/lib.rs"), format!("{marks}\n/// A function-like macro: its tokens back as they are, which klepto cannot expand.\n#[proc_macro]\npub fn same(input: TokenStream) -> TokenStream {{\n    input\n}}\n")).unwrap();
    let manifest = read(root, "shop/Cargo.toml");
    std::fs::write(root.join("shop/Cargo.toml"), format!("{manifest}\n[dev-dependencies]\nshop-marks = {{ path = \"../marks\" }}\n")).unwrap();
    let lib = read(root, "shop/src/lib.rs").replace(
        "        assert_eq!(twice_total(3), 8);\n    }\n",
        "        assert_eq!(twice_total(3), 8);\n    }\n\n    #[test]\n    fn the_version_through_a_macro() {\n        assert!(shop_marks::same!(version() == 1));\n        assert!(shop_marks::same!(\n            crate::version() > 0\n        ));\n    }\n",
    );
    std::fs::write(root.join("shop/src/lib.rs"), lib).unwrap();
}

#[test]
fn a_call_inside_a_macro_klepto_cannot_expand_gets_the_new_argument() {
    let d = common::fresh_copy("transform_more");
    let root = d.path();
    calls_in_a_proc_macro(root);
    let o = apply(root, &spec(root, "versioned", "name = \"versioned\"\nobjective = \"o\"\noperation = \"change_signature\"\ntarget = \"shop::version\"\n\n[function]\nname = \"version\"\nvis = \"pub\"\nparams = [{ name = \"minor\", ty = \"u32\" }]\nreturns = \"u32\"\n\n[call_sites]\npass = \"0\"\n"));
    let lib = read(root, "shop/src/lib.rs");
    assert!(lib.contains("assert!(shop_marks::same!(version(0) == 1));"), "{lib}");
    assert!(lib.contains("            crate::version(0) > 0\n"), "{lib}");
    assert!(read(root, "cli/src/main.rs").contains("shop::version(0)"));
    assert!(o.record.checks.iter().all(|c| c.ok) && o.record.files.iter().any(|f| f.path == "shop/src/lib.rs"), "{:?}", o.record);
}

/// A dry transform that does not compile says it is not proved and why; code naming an optional dependency with no
/// feature gate is told the spec's fix (custodian_kit's `change::optional_uses`, the rule the preflight refuses by).
#[test]
fn a_dry_run_that_does_not_compile_says_not_proved_and_names_the_attrs_fix() {
    let d = common::fresh_copy("feature_forms");
    let root = d.path();
    let s = spec(root, "from-b", "name = \"from-b\"\nobjective = \"o\"\noperation = \"make_fn\"\ntarget = \"crate:p\"\n\n[function]\nname = \"from_b\"\nvis = \"pub\"\nreturns = \"u8\"\nbody = [{ piece = \"value\", value = \"b::b()\" }]\n");
    let o = transform::run(root, &opts(root, &s, false)).unwrap();
    assert_eq!(o.exit_code, 1, "{}", o.said.join("\n"));
    let last = o.said.last().unwrap();
    assert!(last.starts_with("not proved: p --no-default-features does not compile: ") && last.ends_with("; nothing written"), "{last}");
    assert!(!o.said.iter().any(|l| l.starts_with("proved on a scratch copy")), "{:?}", o.said);
    assert!(o.said.iter().any(|l| l == "the fix: b is an optional dependency of p (turned on by b): give the code `attrs = ['#[cfg(feature = \"b\")]']`, or gate the place it lands"), "{:?}", o.said);
    // with the attribute it is proved, and the dry line says so
    let s = spec(root, "from-b", "name = \"from-b\"\nobjective = \"o\"\noperation = \"make_fn\"\ntarget = \"crate:p\"\n\n[function]\nname = \"from_b\"\nvis = \"pub\"\nreturns = \"u8\"\nattrs = ['#[cfg(feature = \"b\")]']\nbody = [{ piece = \"value\", value = \"b::b()\" }]\n");
    let o = transform::run(root, &opts(root, &s, false)).unwrap();
    assert_eq!(o.exit_code, 0, "{}", o.said.join("\n"));
    assert!(o.said.last().unwrap().starts_with("proved on a scratch copy, not written (--write writes): 1 files"), "{:?}", o.said);
}

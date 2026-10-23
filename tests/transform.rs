// Copyright © 2026 1o. All rights reserved.

//! `transform`: every operation of the change vocabulary applied to a copy of the fixture workspace
//! (tests/fixtures/transform_ws: a `shop` library and an `app` that uses it), each proved by cargo over the check
//! matrix of the crate it lands in and the crates that depend on it, its facts shown by a fresh klepto scan, written
//! with its record, and put back by `--revert`. A function's body is simple_impl 0.1.12's toolkit pieces; the
//! generated function compiles in the fixture and its declared test passes.

mod common;

use code_passenger::transform::{self, Options};
use std::path::{Path, PathBuf};

/// Each test's checks build in its own scratch copy's target: cargo hashes a path package by its place in the
/// workspace, so two copies sharing one target directory would read each other's builds as fresh.
fn warm() {}

fn spec(root: &Path, name: &str, text: &str) -> PathBuf {
    let p = root.join(format!("{name}.toml"));
    std::fs::write(&p, text).unwrap();
    p
}

fn apply(root: &Path, spec: &Path) -> transform::Outcome {
    warm();
    let opts = Options { spec: spec.to_path_buf(), write: true, out: Some(root.join(".records").join(format!("{}.json", spec.file_stem().unwrap().to_string_lossy()))), ..Options::default() };
    let o = transform::run(root, &opts).unwrap_or_else(|e| panic!("{}: {e}", spec.display()));
    assert_eq!(o.exit_code, 0, "{}", o.said.join("\n"));
    assert!(o.record.written && o.record.proved(), "{:?}", o.record);
    assert!(!o.record.checks.is_empty(), "cargo checked it");
    o
}

fn read(root: &Path, rel: &str) -> String {
    std::fs::read_to_string(root.join(rel)).unwrap()
}

#[test]
fn make_struct_writes_the_declaration_and_revert_puts_the_file_back() {
    let d = common::fresh_copy("transform_ws");
    let root = d.path();
    let before = read(root, "shop/src/lib.rs");
    let s = spec(root, "receipt", "name = \"receipt\"\nobjective = \"a receipt per order\"\noperation = \"make_struct\"\ntarget = \"crate:shop\"\n\n[item]\nname = \"Receipt\"\nvis = \"pub\"\ndoc = \"What an order paid.\"\nderives = [\"Debug\", \"Clone\"]\n\n[[item.fields]]\nname = \"order\"\nty = \"u32\"\nvis = \"pub\"\n\n[[item.fields]]\nname = \"paid\"\nty = \"u64\"\nvis = \"pub\"\n");
    let o = apply(root, &s);
    let after = read(root, "shop/src/lib.rs");
    assert!(after.contains("/// What an order paid.\n#[derive(Debug, Clone)]\npub struct Receipt {\n    pub order: u32,\n    pub paid: u64,\n}\n"), "{after}");
    assert!(after.find("pub struct Receipt").unwrap() < after.find("#[cfg(test)]").unwrap(), "before the tests module");
    assert_eq!(o.record.files.len(), 1);
    assert!(o.record.postconditions.iter().all(|(_, h)| *h) && o.record.postconditions.iter().any(|(f, _)| f == "exists shop::Receipt"));
    assert!(o.record.checkpoint_before.is_some() && o.record.checkpoint_after.is_some());
    assert!(!o.record.files[0].lines.is_empty());
    // reversible
    let r = transform::revert(root, o.record_file.as_ref().unwrap()).unwrap();
    assert_eq!(read(root, "shop/src/lib.rs"), before);
    assert!(r.record.reverts.is_some());
}

#[test]
fn make_enum_and_add_derive_merge_into_one_derive_line() {
    let d = common::fresh_copy("transform_ws");
    let root = d.path();
    apply(root, &spec(root, "kind", "name = \"kind\"\nobjective = \"o\"\noperation = \"make_enum\"\ntarget = \"crate:shop\"\n\n[item]\nname = \"Kind\"\nvis = \"pub\"\nderives = [\"Debug\"]\n\n[[item.variants]]\nname = \"Retail\"\n\n[[item.variants]]\nname = \"Bulk\"\ntuple = [\"u32\"]\n"));
    assert!(read(root, "shop/src/lib.rs").contains("#[derive(Debug)]\npub enum Kind {\n    Retail,\n    Bulk(u32),\n}\n"));
    apply(root, &spec(root, "d1", "name = \"d1\"\nobjective = \"o\"\noperation = \"add_derive\"\ntarget = \"shop::Order\"\nderive = [\"Debug\"]\n"));
    apply(root, &spec(root, "d2", "name = \"d2\"\nobjective = \"o\"\noperation = \"add_derive\"\ntarget = \"shop::Order\"\nderive = [\"Clone\", \"PartialEq\"]\n"));
    let t = read(root, "shop/src/lib.rs");
    assert!(t.contains("#[derive(Debug, Clone, PartialEq)]\npub struct Order {"), "{t}");
}

#[test]
fn a_method_whose_body_is_toolkit_pieces_compiles_and_its_declared_test_passes() {
    let d = common::fresh_copy("transform_ws");
    let root = d.path();
    let o = apply(root, &spec(root, "discount", r#"name = "discount"
objective = "take a percentage off an order"
operation = "add_method"
target = "shop::Order"

[function]
name = "discounted"
vis = "pub"
receiver = "&self"
params = [{ name = "pct", ty = "u64" }]
returns = "Result<u64, &'static str>"
doc = "The total with `pct` percent off."

[[function.body]]
piece = "ensure"
condition = "pct <= 100"
error = "\"over a hundred percent\""

[[function.body]]
piece = "let"
bind = "off"
value = "self.total * pct / 100"

[[function.body]]
piece = "value"
value = "Ok(self.total - off)"
"#));
    let t = read(root, "shop/src/lib.rs");
    assert!(t.contains("    /// The total with `pct` percent off.\n    pub fn discounted(&self, pct: u64) -> Result<u64, &'static str> {\n        simple_impl::ensure!(pct <= 100, \"over a hundred percent\");\n        let off = self.total * pct / 100;\n        Ok(self.total - off)\n    }\n"), "{t}");
    assert!(read(root, "shop/Cargo.toml").contains("simple_impl = \"0.1.12\""), "the toolkit added to the crate");
    assert!(o.record.files.iter().any(|f| f.path == "shop/Cargo.toml"));
    assert!(o.record.listed.iter().any(|l| l.contains("gets the dependency simple_impl")));
    // its test, declared as a spec too, then run
    apply(root, &spec(root, "discount-test", r#"name = "discount-test"
objective = "prove the discount"
operation = "add_test"
target = "shop::Order::discounted"

[function]
name = "ten_percent_off_a_hundred_is_ninety"
returns = "Result<(), &'static str>"

[[function.body]]
piece = "let"
bind = "o"
value = "Order::new(1, 100)"

[[function.body]]
piece = "ensure_eq"
left = "o.discounted(10)?"
right = "90"
error = "\"ten percent off\""

[[function.body]]
piece = "ensure"
condition = "o.discounted(101).is_err()"
error = "\"more than all of it\""

[[function.body]]
piece = "value"
value = "Ok(())"
"#));
    let out = std::process::Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["test", "-q", "--offline", "-p", "shop", "--", "ten_percent_off_a_hundred_is_ninety"])
        .current_dir(root)
        .output()
        .unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && text.contains("1 passed"), "{text}");
}

#[test]
fn add_field_with_a_default_fills_every_struct_literal_across_crates() {
    let d = common::fresh_copy("transform_ws");
    let root = d.path();
    let o = apply(root, &spec(root, "note", "name = \"note\"\nobjective = \"o\"\noperation = \"add_field\"\ntarget = \"shop::Order\"\n\n[field]\nname = \"note\"\nty = \"String\"\nvis = \"pub\"\ndefault = \"String::new()\"\n"));
    assert!(read(root, "shop/src/lib.rs").contains("    pub total: u64,\n    pub note: String,\n}"));
    assert!(read(root, "shop/src/lib.rs").contains("Order { id, total, note: String::new() }"), "{}", read(root, "shop/src/lib.rs"));
    assert!(read(root, "app/src/lib.rs").contains("shop::Order { id: 1, total: 10, note: String::new() }"), "{}", read(root, "app/src/lib.rs"));
    assert_eq!(o.record.files.len(), 2);
    assert!(o.record.checks.iter().any(|c| c.package == "app"), "the dependent crate checked: {:?}", o.record.checks);
}

#[test]
fn add_variant_lists_the_match_it_breaks_and_writes_nothing_unproved() {
    warm();
    let d = common::fresh_copy("transform_ws");
    let root = d.path();
    let before = read(root, "shop/src/lib.rs");
    let s = spec(root, "paused", "name = \"paused\"\nobjective = \"o\"\noperation = \"add_variant\"\ntarget = \"shop::State\"\n\n[variant]\nname = \"Paused\"\n");
    let o = transform::run(root, &Options { spec: s, write: true, ..Options::default() }).unwrap();
    assert_eq!(o.exit_code, 1, "{}", o.said.join("\n"));
    assert!(o.record.listed.iter().any(|l| l.contains("match without a catch-all") && l.contains("Paused")));
    assert!(o.record.checks.iter().any(|c| !c.ok), "cargo says the match is not exhaustive");
    assert!(!o.record.written);
    assert_eq!(read(root, "shop/src/lib.rs"), before, "nothing written");
}

#[test]
fn add_attribute_gate_and_anchor_go_above_the_item() {
    let d = common::fresh_copy("transform_ws");
    let root = d.path();
    apply(root, &spec(root, "must", "name = \"must\"\nobjective = \"o\"\noperation = \"add_attribute\"\ntarget = \"shop::total_of\"\nattribute = \"#[must_use]\"\n"));
    // `label`: nothing outside it uses it (`version` is used by the golden tests, which the proof compiles too)
    apply(root, &spec(root, "gate", "name = \"gate\"\nobjective = \"o\"\noperation = \"gate\"\ntarget = \"shop::label\"\nfeature = \"audit\"\n"));
    apply(root, &spec(root, "anchor", "name = \"anchor\"\nobjective = \"o\"\noperation = \"anchor\"\ntarget = \"shop::Order\"\nlinkage = \"order-and-receipt\"\n"));
    let t = read(root, "shop/src/lib.rs");
    assert!(t.contains("#[must_use]\npub fn total_of"));
    assert!(t.contains("#[cfg(feature = \"audit\")]\npub fn label"));
    assert!(t.contains("// linkage: order-and-receipt (.custodian/data/linkage/v1.order-and-receipt.1o.linkage)\npub struct Order"), "{t}");
}

#[test]
fn add_module_and_add_dependency_and_a_trait_impl_from_a_toolkit_generator() {
    let d = common::fresh_copy("transform_ws");
    let root = d.path();
    apply(root, &spec(root, "money", "name = \"money\"\nobjective = \"o\"\noperation = \"add_module\"\ntarget = \"crate:shop\"\n\n[module]\nname = \"money\"\nvis = \"pub\"\ndoc = \"Prices and their arithmetic.\"\n"));
    assert_eq!(read(root, "shop/src/money.rs"), "//! Prices and their arithmetic.\n");
    assert!(read(root, "shop/src/lib.rs").contains("pub mod money;\n"));
    apply(root, &spec(root, "dep", "name = \"dep\"\nobjective = \"o\"\noperation = \"add_dependency\"\ntarget = \"crate:app\"\n\n[dependency]\nname = \"stable_digest\"\nversion = \"0.1.1\"\n"));
    assert!(read(root, "app/Cargo.toml").contains("stable_digest = \"0.1.1\""));
    apply(root, &spec(root, "debug", "name = \"debug\"\nobjective = \"o\"\noperation = \"add_trait_impl\"\ntarget = \"shop::Order\"\n\n[generator]\npiece = \"impl_debug_redacted\"\nty = \"Order\"\ntext = \"\\\"Order(..)\\\"\"\n"));
    assert!(read(root, "shop/src/lib.rs").contains("simple_impl::impl_debug_redacted!(Order, \"Order(..)\");\n"));
    apply(root, &spec(root, "default", "name = \"default\"\nobjective = \"o\"\noperation = \"add_trait_impl\"\ntarget = \"shop::Order\"\ntrait = \"Default\"\n\n[[functions]]\nname = \"default\"\nreturns = \"Self\"\n\n[[functions.body]]\npiece = \"value\"\nvalue = \"Order::new(0, 0)\"\n"));
    assert!(read(root, "shop/src/lib.rs").contains("impl Default for Order {\n    fn default() -> Self {\n        Order::new(0, 0)\n    }\n}\n"));
}

#[test]
fn a_getter_and_a_constructor_from_the_toolkit_and_from_the_fields() {
    let d = common::fresh_copy("transform_ws");
    let root = d.path();
    apply(root, &spec(root, "id", "name = \"id\"\nobjective = \"o\"\noperation = \"add_method\"\ntarget = \"shop::Order\"\n\n[function]\nname = \"id\"\nvis = \"pub\"\nreceiver = \"&self\"\nreturns = \"u32\"\n\n[function.generator]\npiece = \"getter\"\nfield = \"id\"\n"));
    assert!(read(root, "shop/src/lib.rs").contains("    simple_impl::getter!(pub fn id(&self) -> u32 => id);\n"));
    apply(root, &spec(root, "ctor", "name = \"ctor\"\nobjective = \"o\"\noperation = \"add_constructor\"\ntarget = \"shop::Order\"\n\n[function]\nname = \"from_parts\"\nvis = \"pub\"\nparams = [{ name = \"id\", ty = \"u32\" }, { name = \"total\", ty = \"u64\" }]\n"));
    assert!(read(root, "shop/src/lib.rs").contains("    pub fn from_parts(id: u32, total: u64) -> Self {\n        Self { id, total }\n    }\n"));
}

#[test]
fn change_signature_passes_the_new_argument_at_every_call_and_rename_follows_every_use() {
    let d = common::fresh_copy("transform_ws");
    let root = d.path();
    apply(root, &spec(root, "sig", "name = \"sig\"\nobjective = \"o\"\noperation = \"change_signature\"\ntarget = \"shop::total_of\"\n\n[function]\nname = \"total_of\"\nparams = [{ name = \"o\", ty = \"&Order\" }, { name = \"extra\", ty = \"u64\" }]\nreturns = \"u64\"\n\n[call_sites]\npass = \"0\"\n"));
    assert!(read(root, "shop/src/lib.rs").contains("pub fn total_of(o: &Order, extra: u64) -> u64 {"), "{}", read(root, "shop/src/lib.rs"));
    assert!(read(root, "app/src/lib.rs").contains("shop::total_of(o, 0) + 1"), "{}", read(root, "app/src/lib.rs"));
    let o = apply(root, &spec(root, "rename", "name = \"rename\"\nobjective = \"o\"\noperation = \"rename\"\ntarget = \"shop::total_of\"\nnew_name = \"order_total\"\n"));
    assert!(read(root, "shop/src/lib.rs").contains("pub fn order_total(o: &Order, extra: u64)"));
    assert!(read(root, "app/src/lib.rs").contains("shop::order_total(o, 0) + 1"));
    assert!(o.record.postconditions.contains(&("absent shop::total_of".to_string(), true)));
}

#[test]
fn move_and_extract_keep_the_old_paths_through_a_re_export() {
    let d = common::fresh_copy("transform_ws");
    let root = d.path();
    apply(root, &spec(root, "meta", "name = \"meta\"\nobjective = \"o\"\noperation = \"extract\"\ntarget = \"crate:shop\"\nitems = [\"shop::version\"]\n\n[module]\nname = \"meta\"\n"));
    let lib = read(root, "shop/src/lib.rs");
    assert!(!lib.contains("pub fn version()") && lib.contains("mod meta;\npub use meta::{version};"), "{lib}");
    assert!(read(root, "shop/src/meta.rs").contains("pub fn version() -> u32 {"));
    // move it on into a module of its own, the old path re-exported
    apply(root, &spec(root, "money", "name = \"money\"\nobjective = \"o\"\noperation = \"add_module\"\ntarget = \"crate:shop\"\n\n[module]\nname = \"money\"\nvis = \"pub\"\n"));
    apply(root, &spec(root, "mv", "name = \"mv\"\nobjective = \"o\"\noperation = \"move\"\ntarget = \"shop::label\"\nto = \"crate:shop/src/money.rs\"\n"));
    let lib = read(root, "shop/src/lib.rs");
    assert!(lib.contains("pub use crate::money::label;") && !lib.contains("pub fn label"), "{lib}");
}

#[test]
fn every_operation_of_the_vocabulary_has_its_test_here() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let here = ["transform.rs", "transform_more.rs", "manifest_ops.rs"].iter().map(|f| std::fs::read_to_string(dir.join(f)).unwrap()).collect::<String>();
    for op in custodian_kit::change::Operation::ALL {
        assert!(here.contains(&format!("operation = \\\"{}\\\"", op.as_str())) || here.contains(&format!("operation = \"{}\"", op.as_str())), "no test applies {}", op.as_str());
    }
}

#[test]
fn make_fn_and_make_impl_from_pieces() {
    let d = common::fresh_copy("transform_ws");
    let root = d.path();
    apply(root, &spec(root, "big", "name = \"big\"\nobjective = \"o\"\noperation = \"make_fn\"\ntarget = \"crate:shop\"\n\n[function]\nname = \"is_big\"\nvis = \"pub\"\nparams = [{ name = \"o\", ty = \"&Order\" }]\nreturns = \"bool\"\n\n[[function.body]]\npiece = \"in_range\"\nvalue = \"o.total\"\nminimum = \"1000\"\nmaximum = \"u64::MAX\"\n"));
    assert!(read(root, "shop/src/lib.rs").contains("pub fn is_big(o: &Order) -> bool {\n    simple_impl::in_range!(o.total, 1000, u64::MAX)\n}\n"), "{}", read(root, "shop/src/lib.rs"));
    apply(root, &spec(root, "state", "name = \"state\"\nobjective = \"o\"\noperation = \"make_impl\"\ntarget = \"shop::State\"\n\n[[functions]]\nname = \"is_open\"\nvis = \"pub\"\nreceiver = \"&self\"\nreturns = \"bool\"\n\n[[functions.body]]\npiece = \"value\"\nvalue = \"matches!(self, State::Open)\"\n"));
    assert!(read(root, "shop/src/lib.rs").contains("impl State {\n    pub fn is_open(&self) -> bool {\n        matches!(self, State::Open)\n    }\n}\n"));
}

#[test]
fn replace_body_keeps_the_signature_and_docs_and_add_test_lands_in_the_module_named() {
    let d = common::fresh_copy("transform_ws");
    let root = d.path();
    let before = read(root, "shop/src/lib.rs");
    let o = apply(root, &spec(root, "new-checked", r#"name = "new-checked"
objective = "an order's total kept as given, through a named binding"
operation = "replace_body"
target = "shop::Order::new"

[function]
name = "new"

[[function.body]]
piece = "let"
bind = "order"
value = "Order { id, total }"

[[function.body]]
piece = "value"
value = "order"
"#));
    let t = read(root, "shop/src/lib.rs");
    assert!(t.contains("    pub fn new(id: u32, total: u64) -> Self {\n        let order = Order { id, total };\n        order\n    }\n"), "{t}");
    assert_eq!(t.lines().count(), before.lines().count() + 1, "one line more, nothing else moved");
    assert!(o.record.postconditions.contains(&("args shop::Order::new 2".to_string(), true)), "{:?}", o.record.postconditions);
    // a free function's body: a toolkit piece, simple_impl added to the crate it lands in
    let o = apply(root, &spec(root, "version-checked", r#"name = "version-checked"
objective = "o"
operation = "replace_body"
target = "shop::version"

[function]
name = "version"

[[function.body]]
piece = "let"
bind = "one"
value = "1"

[[function.body]]
piece = "push_if"
collection = "&mut Vec::<u32>::new()"
condition = "one > 0"
value = "one"

[[function.body]]
piece = "value"
value = "one"
"#));
    let t = read(root, "shop/src/lib.rs");
    assert!(t.contains("pub fn version() -> u32 {\n    let one = 1;\n    simple_impl::push_if!(&mut Vec::<u32>::new(), one > 0 => one);\n    one\n}\n"), "{t}");
    assert!(read(root, "shop/Cargo.toml").contains("simple_impl = \"0.1.12\""));
    assert_eq!(o.record.files.len(), 2);
    // the spec's function must be the target's own
    let other = spec(root, "other", "name = \"other\"\nobjective = \"o\"\noperation = \"replace_body\"\ntarget = \"shop::version\"\n\n[function]\nname = \"label\"\n\n[[function.body]]\npiece = \"value\"\nvalue = \"2\"\n");
    assert!(transform::run(root, &Options { spec: other, write: true, ..Options::default() }).is_err());
    // a test in the inline module the spec names
    let o = apply(root, &spec(root, "golden-two", "name = \"golden-two\"\nobjective = \"o\"\noperation = \"add_test\"\ntarget = \"shop::version\"\nto = \"golden\"\n\n[function]\nname = \"version_is_still_one\"\n\n[[function.body]]\npiece = \"raw\"\ncode = \"assert_eq!(version(), 1);\"\n"));
    let t = read(root, "shop/src/lib.rs");
    let golden = t.find("mod golden {").unwrap();
    assert!(t[golden..].contains("    #[test]\n    fn version_is_still_one() {\n        assert_eq!(version(), 1);\n    }\n"), "{t}");
    assert!(o.record.postconditions.contains(&("exists shop::golden::version_is_still_one".to_string(), true)), "{:?}", o.record.postconditions);
    // a module the file does not have is made (code_passenger 0.4.4; it was refused)
    apply(root, &spec(root, "nowhere", "name = \"nowhere\"\nobjective = \"o\"\noperation = \"add_test\"\ntarget = \"shop::version\"\nto = \"absent\"\n\n[function]\nname = \"t\"\n\n[[function.body]]\npiece = \"raw\"\ncode = \"assert_eq!(version(), 1);\"\n"));
    assert!(read(root, "shop/src/lib.rs").contains("#[cfg(test)]\nmod absent {\n    use super::*;\n"));
}

#[test]
fn change_dependency_moves_every_crate_naming_it_and_keeps_what_else_the_entry_says() {
    let d = common::fresh_copy("transform_ws");
    let root = d.path();
    let o = apply(root, &spec(root, "shop-versioned", "name = \"shop-versioned\"\nobjective = \"o\"\noperation = \"change_dependency\"\ntarget = \"*\"\n\n[[dependencies]]\nname = \"shop\"\nversion = \"0.1.0\"\npath = \"../shop\"\n"));
    assert!(read(root, "app/Cargo.toml").contains("shop = { path = \"../shop\", version = \"0.1.0\" }"), "{}", read(root, "app/Cargo.toml"));
    assert!(o.record.postconditions.contains(&("source app/Cargo.toml shop 0.1.0 ../shop".to_string(), true)), "{:?}", o.record.postconditions);
    // the manifest, and the lock file when cargo's proof wrote it anew (code_passenger 0.4.4)
    assert_eq!(o.record.files.iter().filter(|f| f.path != "Cargo.lock").count(), 1, "{:?}", o.record.files);
    // the text rule: a version alone is the registry's, the path dropped, a bare string when nothing else is on it;
    // features given replace the entry's; every dependency table, a target's too, and a renamed package
    let text = "[dependencies]\nx = { path = \"../x\" } # the copy\ny = { path = \"../y\", features = [\"a\"], optional = true }\n\n[dev-dependencies]\nx = { version = \"0.1\", path = \"../x\" }\n\n[target.'cfg(unix)'.dependencies]\nz = { package = \"x\", path = \"../x\" }\n";
    let (after, n) = code_passenger::commands::edit::dep_change_text(text, "x", Some("0.3.1"), None, &[]).unwrap();
    assert_eq!(n, 3);
    assert_eq!(after, "[dependencies]\nx = \"0.3.1\" # the copy\ny = { path = \"../y\", features = [\"a\"], optional = true }\n\n[dev-dependencies]\nx = \"0.3.1\"\n\n[target.'cfg(unix)'.dependencies]\nz = { package = \"x\", version = \"0.3.1\" }\n");
    let (after, _) = code_passenger::commands::edit::dep_change_text(text, "y", Some("0.2"), None, &["b".into()]).unwrap();
    assert!(after.contains("y = { features = [\"b\"], optional = true , version = \"0.2\" }") || after.contains("version = \"0.2\""), "{after}");
    assert!(!after.contains("../y"));
    assert!(code_passenger::commands::edit::dep_change_text("[dependencies]\nx = { workspace = true }\n", "x", Some("1"), None, &[]).is_err());
    assert_eq!(code_passenger::commands::edit::dep_source(&after, "x"), Some((None, Some("../x".into()))));
}

/// W8's merge by hand, as operations: `remove_dependency` refused while a source file still names the crate (each
/// place said); `rename_paths` moves every `old_kit::` path to `new_kit::`, proved by the compile; then the line drops.
#[test]
fn rename_paths_then_remove_dependency_with_the_use_site_check() {
    let d = common::fresh_copy("deps_ws");
    let root = d.path();
    let remove = spec(root, "drop-old", "name = \"drop-old\"\nobjective = \"o\"\noperation = \"remove_dependency\"\ntarget = \"crate:user_a\"\n\n[dependency]\nname = \"old_kit\"\n");
    let early = transform::run(root, &Options { spec: remove.clone(), write: true, ..Options::default() });
    let e = early.map(|o| o.said.join("\n")).unwrap_err().to_string();
    assert!(e.contains("user_a still names old_kit") && e.contains("crates/user_a/src/lib.rs:3") && e.contains("crates/user_a/src/lib.rs:6"), "{e}");
    let rename = spec(root, "to-new", "name = \"to-new\"\nobjective = \"o\"\noperation = \"rename_paths\"\ntarget = \"crate:user_a\"\nnew_name = \"new_kit\"\n\n[dependency]\nname = \"old_kit\"\n");
    apply(root, &rename);
    let lib = read(root, "crates/user_a/src/lib.rs");
    assert!(lib.contains("use new_kit::greet;") && lib.contains("new_kit::greet()") && !lib.contains("old_kit"), "{lib}");
    apply(root, &remove);
    let m = read(root, "crates/user_a/Cargo.toml");
    assert!(!m.contains("old_kit") && m.contains("new_kit"), "{m}");
    // user_b untouched
    assert!(read(root, "crates/user_b/src/lib.rs").contains("old_kit::greet()"));
}

/// Manifest changes batch like code changes: two callers' dependency lines in one batch, one scan, one check,
/// a record each.
#[test]
fn a_batch_of_dependency_changes_is_one_transform_for_every_caller() {
    let d = common::fresh_copy("deps_ws");
    let root = d.path();
    let change = |name: &str, krate: &str| spec(root, name, &format!("name = \"{name}\"\nobjective = \"o\"\noperation = \"change_dependency\"\ntarget = \"crate:{krate}\"\n\n[dependency]\nname = \"old_kit\"\nversion = \"0.1\"\npath = \"../old_kit\"\n"));
    let specs = vec![change("a-old", "user_a"), change("b-old", "user_b")];
    let o = transform::run_batch(root, &transform::BatchOptions { specs, write: true, out_dir: Some(root.join(".records")), ..Default::default() }).unwrap();
    assert_eq!(o.exit_code, 0, "{}", o.said.join("\n"));
    assert_eq!(o.records.len(), 2, "{}", o.said.join("\n"));
    for k in ["user_a", "user_b"] {
        let m = read(root, &format!("crates/{k}/Cargo.toml"));
        assert!(m.contains("version = \"0.1\""), "{m}");
    }
}

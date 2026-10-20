// Copyright © 2026 1o. All rights reserved.

//! `to-toolkit`: a fixture crate (tests/fixtures/toolkit_ws: `guards`, which does not depend on simple_impl) with
//! three hand-written bodies the toolkit writes exactly (an `ensure`, a `require_some`, a `continue_if` inside a
//! loop) and one it writes nearly. Each exact one is swapped: the first swap's preflight adds the dependency as its
//! own step; each is proved on a scratch copy (cargo expand of the function before and after the same in the normal
//! form, the crate's tests passing) and written through `transform`. The near one is refused with its difference
//! named.

mod common;

use code_passenger::to_toolkit::{self, Options};
use std::path::Path;

fn swap(root: &Path, function: &str) -> to_toolkit::Outcome {
    let out = root.join(".records").join(function.replace("::", "_"));
    let o = to_toolkit::run(root, &Options { function: function.into(), write: true, out: Some(out) }).unwrap_or_else(|e| panic!("{function}: {e}"));
    assert!(o.record.same_expansion, "{}\nbefore: {}\nafter:  {}", function, o.record.expanded_before, o.record.expanded_after);
    assert_eq!(o.exit_code, 0, "{}", to_toolkit::render(&o));
    assert_eq!(o.record.tests_exit, Some(0), "the crate's tests pass on the copy: {:?}", o.record.tests);
    assert!(o.record.written && o.record.transforms.iter().all(|t| t.written && t.proved()), "{:?}", o.record.transforms);
    o
}

#[test]
fn three_exact_bodies_are_swapped_and_a_near_one_is_refused() {
    let d = common::fresh_copy("toolkit_ws");
    let root = d.path();
    let lib = |r: &Path| std::fs::read_to_string(r.join("guards/src/lib.rs")).unwrap();

    let first = swap(root, "guards::qos");
    assert_eq!(first.record.pieces, ["ensure"]);
    assert_eq!(first.record.steps.len(), 2, "the dependency first, as its own step");
    assert!(first.record.preflight.iter().any(|c| !c.holds && c.what.contains("depends on simple_impl")));
    assert!(std::fs::read_to_string(root.join("guards/Cargo.toml")).unwrap().contains("simple_impl = \"0.1.12\""));
    assert!(lib(root).contains("pub fn qos(level: u8) -> Result<u8, &'static str> {\n    simple_impl::ensure!(level <= 2, \"qos is 0, 1 or 2\");\n    let checked = level;\n    Ok(checked)\n}"), "{}", lib(root));

    let second = swap(root, "guards::first_port");
    assert_eq!(second.record.steps.len(), 1, "the crate depends on the toolkit now");
    assert!(lib(root).contains("    let port = simple_impl::require_some!(ports.first(), \"no port\".to_string());\n    Ok(*port)\n"), "{}", lib(root));

    swap(root, "guards::names");
    assert!(lib(root).contains("simple_impl::continue_if!(name.is_empty());"), "{}", lib(root));

    // a whole method from a generator: its docs and attributes go inside the call, which writes them back
    let getter = swap(root, "guards::Port::number");
    assert_eq!(getter.record.pieces, ["getter"]);
    assert!(lib(root).contains("    simple_impl::getter!(#[doc = \" The port's number.\"] #[must_use] pub fn number(&self) -> u16 => number);\n"), "{}", lib(root));

    let near = to_toolkit::run(root, &Options { function: "guards::same_size".into(), write: true, out: None }).unwrap_err();
    let why = near.to_string();
    assert!(why.contains("near only") && why.contains("without its `;`"), "{why}");
    assert!(lib(root).contains("        return Err(\"sizes differ\")\n"), "the near one is left as it is");
}

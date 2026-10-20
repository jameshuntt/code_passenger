// Copyright © 2026 1o. All rights reserved.

//! `to-toolkit --function F [--write] [--out DIR]`: one function's body swapped for the simple_impl toolkit calls
//! whose expansion it already is (custodian_kit's `patterns` reading finds them; `toolkit::matcher::function_at`
//! matches the one function again here, over the code as it is now).
//!
//! The preflight: the function matches the toolkit exactly (a near match is refused, its difference named), and
//! its crate depends on simple_impl, or the first step is an `add_dependency` spec that makes it so. Each step is a
//! change spec `transform` makes (`replace_body` for the swap).
//!
//! The proof, on a scratch copy before anything is written: the steps applied there, `cargo expand` of the crate
//! before and after (a declared line), the function found in both and compared in custodian_kit's proof form
//! (`toolkit::normal::proof_form`: its locals named by where they are bound, since a macro's own names are hygienic,
//! then the normal form: the prelude's names written short, parentheses around an atomic expression dropped): the
//! same is the postcondition; then the crate's tests (`cargo test -p <crate>`, a declared line) pass on the copy.
//! With `--write` each step is then made by `transform` (proved again by cargo over the check matrix, written,
//! checkpointed, recorded).

use crate::error::{PassengerError, Result};
use crate::gate::workspace_root;
use crate::transform;
use custodian_kit::change::{self, ChangeSpec, DependencySpec, Operation};
use custodian_kit::toolkit::matcher;
use custodian_kit::toolkit::normal::{self, Level};
use custodian_kit::transform::TransformRecord;
use quote::ToTokens;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const TO_TOOLKIT_SCHEMA: &str = "code_passenger.to_toolkit.v1";

/// What `to-toolkit` was asked.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// The function's path, as klepto names it (`shop::Order::total`).
    pub function: String,
    pub write: bool,
    /// Where the specs and the record go (else a scratch directory, and the record nowhere).
    pub out: Option<PathBuf>,
}

/// One check of the preflight.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    pub what: String,
    pub holds: bool,
    pub detail: String,
}

/// What a to-toolkit run did and proved.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolkitRecord {
    pub schema: String,
    pub function: String,
    pub package: String,
    pub file: String,
    /// The pieces swapped in.
    pub pieces: Vec<String>,
    pub preflight: Vec<Check>,
    /// The steps, in order: each a change spec.
    pub steps: Vec<ChangeSpec>,
    /// The function as cargo expand gives it before and after, in the normal form.
    pub expanded_before: String,
    pub expanded_after: String,
    pub same_expansion: bool,
    /// `cargo test -p <crate>` on the copy: its exit code, and its `test result:` lines.
    pub tests_exit: Option<i32>,
    pub tests: Vec<String>,
    /// Each step's transform record (proved; written with `--write`).
    pub transforms: Vec<TransformRecord>,
    pub written: bool,
}

/// What a to-toolkit run did.
#[derive(Debug)]
pub struct Outcome {
    pub record: ToolkitRecord,
    pub record_file: Option<PathBuf>,
    pub said: Vec<String>,
    pub exit_code: i32,
}

fn refused(detail: impl std::fmt::Display) -> PassengerError {
    PassengerError::TransformRefused { detail: detail.to_string() }
}

/// A path split at `::` outside angle brackets (`m::<A as m::P>::id` is `m`, `<A as m::P>`, `id`).
fn segments(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    let mut chars = path.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '<' => depth += 1,
            '>' => depth -= 1,
            ':' if depth == 0 && chars.peek() == Some(&':') => {
                chars.next();
                out.push(std::mem::take(&mut cur));
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    out.push(cur);
    out
}

/// A type's name without its generics or path (`Lens<S, A>` is `Lens`, `crate::m::T` is `T`).
fn base_name(t: &str) -> String {
    let t = t.trim().trim_start_matches('&').trim();
    let t = t.split('<').next().unwrap_or(t);
    t.rsplit("::").next().unwrap_or(t).trim().to_string()
}

/// The function `segs` names (module path, then a type or `<Type as Trait>`, then the function) among `items`.
fn find_fn(items: &[syn::Item], segs: &[String]) -> Option<syn::ImplItemFn> {
    match segs {
        [name] => items.iter().find_map(|i| match i {
            syn::Item::Fn(f) if f.sig.ident == name => Some(syn::ImplItemFn { attrs: f.attrs.clone(), vis: f.vis.clone(), defaultness: None, sig: f.sig.clone(), block: (*f.block).clone() }),
            _ => None,
        }),
        [ty, name] => {
            let (self_ty, trait_name) = match ty.strip_prefix('<').and_then(|t| t.strip_suffix('>')).and_then(|t| t.split_once(" as ")) {
                Some((s, t)) => (base_name(s), Some(base_name(t))),
                None => (base_name(ty), None),
            };
            let in_impl = items.iter().find_map(|i| match i {
                syn::Item::Impl(imp)
                    if base_name(&imp.self_ty.to_token_stream().to_string().replace(' ', "")) == self_ty
                        && trait_name.as_ref().is_none_or(|t| imp.trait_.as_ref().and_then(|(_, p, _)| p.segments.last()).is_some_and(|s| s.ident == t)) =>
                {
                    imp.items.iter().find_map(|x| match x {
                        syn::ImplItem::Fn(f) if f.sig.ident == name => Some(f.clone()),
                        _ => None,
                    })
                }
                _ => None,
            });
            in_impl.or_else(|| {
                items.iter().find_map(|i| match i {
                    syn::Item::Trait(t) if t.ident == self_ty => t.items.iter().find_map(|x| match x {
                        syn::TraitItem::Fn(f) if f.sig.ident == name => f.default.clone().map(|b| syn::ImplItemFn { attrs: f.attrs.clone(), vis: syn::Visibility::Inherited, defaultness: None, sig: f.sig.clone(), block: b }),
                        _ => None,
                    }),
                    _ => None,
                })
            })
            .or_else(|| module_then(items, segs))
        }
        _ => module_then(items, segs),
    }
}

fn module_then(items: &[syn::Item], segs: &[String]) -> Option<syn::ImplItemFn> {
    let (first, rest) = segs.split_first()?;
    items.iter().find_map(|i| match i {
        syn::Item::Mod(m) if m.ident == first => m.content.as_ref().and_then(|(_, items)| find_fn(items, rest)),
        _ => None,
    })
}

/// The function `item` of `package` as `cargo expand` gives it in the crate at `dir`, in the normal form.
fn expanded_fn(dir: &Path, package: &str, item: &str) -> Result<String> {
    let text = custodian_kit::exec::expand_crate(dir, package).map_err(|e| refused(format!("cargo expand: {e}")))?;
    let file = syn::parse_file(&text).map_err(|e| refused(format!("cargo expand's output for {package} does not parse: {e}")))?;
    let segs = segments(item);
    let segs = match segs.split_first() {
        Some((first, rest)) if first.replace('-', "_") == package.replace('-', "_") => rest.to_vec(),
        _ => segs,
    };
    let f = find_fn(&file.items, &segs).ok_or_else(|| refused(format!("{item}: not found in cargo expand's output")))?;
    Ok(normal::proof_form(&f))
}

/// `cargo test -p <package>` in the crate at `dir`, a declared line: its exit code and its `test result:` lines.
fn tests_in(dir: &Path, package: &str) -> Result<(Option<i32>, Vec<String>)> {
    use custodian_kit::exec::{line, Cargo, Plan, Watch};
    let l = Cargo::test(dir.join("Cargo.toml").as_os_str(), package);
    let walked = Plan::new("to-toolkit-tests").then(line("tests", &l).map_err(refused)?.going_on()).walk(Watch::Quiet, |_, _| {}).map_err(refused)?;
    let w = walked.into_iter().next().ok_or_else(|| refused("cargo test did not run"))?;
    let results = w.stdout.lines().chain(w.stderr.lines()).filter(|l| l.starts_with("test result:")).map(str::to_string).collect();
    Ok((w.code, results))
}

fn spec_file(dir: &Path, spec: &ChangeSpec) -> Result<PathBuf> {
    let p = dir.join(format!("{}.toml", spec.name));
    std::fs::create_dir_all(dir)?;
    std::fs::write(&p, spec.to_toml().map_err(refused)?)?;
    Ok(p)
}

/// `to-toolkit`: the preflight, the proof on a scratch copy, and (with `write`) each step made by `transform`.
pub fn run(root: &Path, opts: &Options) -> Result<Outcome> {
    let root = workspace_root(root)?;
    let k = crate::cargo::builder(&root)?.ignore_parse_errors(true).parse().map_err(|e| PassengerError::Scan { detail: e.to_string() })?;
    let f = k.functions.iter().find(|x| x.fq_name == opts.function && x.generated.is_none()).ok_or_else(|| refused(format!("{}: not a function klepto captured in the source", opts.function)))?;
    let (Some(line), Some(column)) = (f.location.line, f.location.column) else { return Err(refused(format!("{}: klepto has no place for it", opts.function))) };
    let text = std::fs::read_to_string(&f.location.path)?;
    let at = matcher::function_at(&text, line, column, &opts.function).ok_or_else(|| refused(format!("{}: no function at {}:{line}", opts.function, f.location.path.display())))?;
    let mut said = Vec::new();
    let mut preflight = Vec::new();
    let pieces: Vec<String> = at.matched.whole.iter().filter(|w| w.level == Level::Exact).map(|w| w.shape().piece.clone()).chain(at.matched.steps.iter().filter(|s| s.matched.level == Level::Exact && !s.comment).map(|s| s.matched.shape().piece.clone())).collect();
    let exact = at.spec.is_some();
    let detail = if exact {
        format!("{} (the call expands to the same tokens)", pieces.join(", "))
    } else if at.near_spec.is_some() {
        format!("near only: {}", at.matched.differences().join("; "))
    } else {
        "no toolkit shape in its body".to_string()
    };
    preflight.push(Check { what: "the function's body matches the toolkit exactly".into(), holds: exact, detail: detail.clone() });
    let Some(swap) = at.spec.clone() else {
        return Err(refused(format!("{}: {detail}", opts.function)));
    };
    let land = change::landing(&k, &root, &swap).map_err(refused)?;
    let has = k.features.get(&land.package).is_some_and(|m| m.dependencies.iter().any(|d| d.name == change::TOOLKIT.0));
    let mut steps = Vec::new();
    if !has {
        steps.push(ChangeSpec {
            name: format!("to-toolkit-dependency-{}", land.package),
            objective: format!("{} uses the simple_impl toolkit", land.package),
            operation: Some(Operation::AddDependency),
            target: format!("crate:{}", land.package),
            dependency: Some(DependencySpec { name: change::TOOLKIT.0.into(), version: Some(change::TOOLKIT.1.into()), ..DependencySpec::default() }),
            ..ChangeSpec::default()
        });
    }
    preflight.push(Check {
        what: format!("{} depends on {}", land.package, change::TOOLKIT.0),
        holds: has,
        detail: if has { "it does".into() } else { format!("not yet: step 1 is add_dependency {} = \"{}\"", change::TOOLKIT.0, change::TOOLKIT.1) },
    });
    steps.push(swap);
    for (i, s) in steps.iter().enumerate() {
        said.push(format!("step {}: {} {} ({})", i + 1, s.op().as_str(), s.target, s.name));
    }

    // the proof on a scratch copy: the steps applied, the function expanded before and after, the tests
    let held = tempfile::tempdir()?;
    let specs_dir = opts.out.clone().unwrap_or_else(|| held.path().join("specs"));
    let copy = crate::gate::scratch_copy(&root)?;
    let before = expanded_fn(copy.path(), &land.package, &opts.function)?;
    for s in &steps {
        let path = spec_file(&specs_dir, s)?;
        let o = transform::run(copy.path(), &transform::Options { spec: path, no_cargo: true, ..transform::Options::default() })?;
        o.edits.write_to(&o.root, copy.path())?;
    }
    let after = expanded_fn(copy.path(), &land.package, &opts.function)?;
    let same = before == after;
    said.push(if same { "cargo expand: the function after is the same tokens as before (in the normal form)".into() } else { "cargo expand: the function after differs from before".into() });
    let (tests_exit, tests) = if same { tests_in(copy.path(), &land.package)? } else { (None, Vec::new()) };
    if same {
        said.push(format!("cargo test -p {} on the copy: exit {:?}; {}", land.package, tests_exit, tests.join(" | ")));
    }
    let proved = same && tests_exit == Some(0);

    let mut transforms = Vec::new();
    let mut written = false;
    if proved {
        for s in &steps {
            let path = spec_file(&specs_dir, s)?;
            let o = transform::run(&root, &transform::Options { spec: path, write: opts.write, out: opts.out.as_ref().map(|d| d.join(format!("{}.record.json", s.name))), ..transform::Options::default() })?;
            said.extend(o.said.iter().map(|l| format!("  {l}")));
            let ok = o.exit_code == 0;
            written |= o.record.written;
            transforms.push(o.record);
            if !ok {
                break;
            }
        }
    } else {
        said.push("nothing written: not proved".into());
    }
    let all_proved = proved && transforms.len() == steps.len() && transforms.iter().all(|t| t.proved());
    let record = ToolkitRecord {
        schema: TO_TOOLKIT_SCHEMA.into(),
        function: opts.function.clone(),
        package: land.package.clone(),
        file: land.file.clone(),
        pieces,
        preflight,
        steps,
        expanded_before: before,
        expanded_after: after,
        same_expansion: same,
        tests_exit,
        tests,
        transforms,
        written,
    };
    let record_file = opts.out.as_ref().map(|d| d.join("to_toolkit.json"));
    if let Some(p) = &record_file {
        std::fs::create_dir_all(p.parent().unwrap_or(Path::new(".")))?;
        std::fs::write(p, serde_json::to_string_pretty(&record)? + "\n")?;
    }
    let exit_code = if all_proved && (written || !opts.write) { 0 } else { 1 };
    Ok(Outcome { record, record_file, said, exit_code })
}

/// What `to-toolkit` prints.
pub fn render(o: &Outcome) -> String {
    let mut s = String::new();
    s.push_str(&format!("to-toolkit {} ({}, {})\n", o.record.function, o.record.package, o.record.file));
    for c in &o.record.preflight {
        s.push_str(&format!("preflight: {} — {} ({})\n", c.what, if c.holds { "holds" } else { "does not hold" }, c.detail));
    }
    for l in &o.said {
        s.push_str(&format!("{l}\n"));
    }
    if let Some(f) = &o.record_file {
        s.push_str(&format!("record: {}\n", f.display()));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_splits_outside_angle_brackets() {
        assert_eq!(segments("m::<A as m::P>::id"), ["m", "<A as m::P>", "id"]);
        assert_eq!(segments("shop::Order::total"), ["shop", "Order", "total"]);
        assert_eq!(base_name("Lens<S, A>"), "Lens");
    }
}

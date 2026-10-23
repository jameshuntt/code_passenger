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
//! same is the postcondition; then the crate's tests (`cargo test -p <crate>`, a declared line) on the copy: when some
//! fail, the same tests run again with the copy's files put back as they are in the tree, and only a test that fails
//! after and passed before stops the swap (a test that fails the same way at the base is not the change's: on the Mac a
//! crate whose tests need root or systemd always has some).
//! With `--write` each step is then made by `transform` (proved again by cargo over the check matrix, written,
//! checkpointed, recorded).
//!
//! `--batch LIST` (through `transform --batch LIST --toolkit`): James's rule for a repeated shape. One function of a
//! shape is swapped alone first, fully proved; the rest of that shape in the crate go as one batch: each matched again
//! here, their edits placed together on one scratch copy, one `cargo expand` per crate before and after compared
//! function by function (a member not the same tokens is left out and named, the rest go on), the crate's tests once
//! against the base, then `transform --batch` (one cargo check over the check matrix, one record per function, each
//! reverted alone).

use crate::error::{PassengerError, Result};
use crate::gate::workspace_root;
use crate::proof_dir::Compared;
use crate::transform;
use custodian_kit::change::{self, ChangeSpec, DependencySpec, Operation};
use custodian_kit::toolkit::matcher;
use custodian_kit::toolkit::normal::Level;
use custodian_kit::transform::{Expansion, TransformRecord};
use quote::ToTokens;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
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
    /// The tests failing on the copy after that fail the same at the base (the tree as it is): not the change's.
    #[serde(default)]
    pub failing_at_base: Vec<String>,
    /// The tests failing after that passed at the base: any one stops the swap.
    #[serde(default)]
    pub new_failures: Vec<String>,
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

/// The function `item` of `package` among the items of the crate's expansion (`package::` dropped from its path).
pub(crate) fn fn_in(items: &[syn::Item], package: &str, item: &str) -> Option<syn::ImplItemFn> {
    // a route that names its impl (`path[impl From<i64> for Level]`): found in the impl whose header is that one
    let (path, _) = custodian_kit::route::split(item);
    let segs = segments(path);
    let segs = match segs.split_first() {
        Some((first, rest)) if first.replace('-', "_") == package.replace('-', "_") => rest.to_vec(),
        _ => segs,
    };
    match custodian_kit::route::named_impl(item) {
        Some(imp) => in_named_impl(items, &segs, &imp),
        None => find_fn(items, &segs),
    }
}

/// The function `segs` names inside the impl `imp` (its trait as written, generic arguments and all, and its type),
/// in the module the path leads to.
fn in_named_impl(items: &[syn::Item], segs: &[String], imp: &(Option<String>, String)) -> Option<syn::ImplItemFn> {
    let flat = |t: String| t.split_whitespace().collect::<String>();
    match segs {
        [_, name] => items.iter().find_map(|i| match i {
            syn::Item::Impl(x)
                if flat(x.self_ty.to_token_stream().to_string()) == flat(imp.1.clone())
                    && x.trait_.as_ref().map(|(_, p, _)| flat(p.to_token_stream().to_string())) == imp.0.clone().map(flat) =>
            {
                x.items.iter().find_map(|f| match f {
                    syn::ImplItem::Fn(f) if f.sig.ident == name => Some(f.clone()),
                    _ => None,
                })
            }
            _ => None,
        }),
        [first, rest @ ..] if rest.len() > 1 => items.iter().find_map(|i| match i {
            syn::Item::Mod(m) if m.ident == first => m.content.as_ref().and_then(|(_, items)| in_named_impl(items, rest, imp)),
            _ => None,
        }),
        _ => None,
    }
}

/// The proof a transform kept beside its record: the expansion, the tests against the base.
fn proof_of(record_file: Option<&PathBuf>) -> (Option<Expansion>, Option<Compared>) {
    let Some(f) = record_file else { return (None, None) };
    let e = std::fs::read_to_string(transform::expansion_file(f)).ok().and_then(|t| serde_json::from_str(&t).ok());
    let c = std::fs::read_to_string(transform::tests_file(f)).ok().and_then(|t| serde_json::from_str(&t).ok());
    (e, c)
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
    let f = custodian_kit::route::find_fn(&k.functions, &opts.function).map_err(|e| refused(e.to_string()))?;
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

    // the proof is the transform's: each step proved on the workspace's proof place (the crate it lands in with its
    // tests and its direct dependents compiled, the swap's expansion the same before and after, the crate's tests
    // against the same tests before), written with --write
    let held = tempfile::tempdir()?;
    let specs_dir = opts.out.clone().unwrap_or_else(|| held.path().join("specs"));
    let mut transforms = Vec::new();
    let mut written = false;
    let mut proved = true;
    let (mut expansion, mut compared) = (None, None);
    if !has && !opts.write {
        proved = false;
        said.push(format!("not proved: {} does not depend on {} yet; --write makes step 1 first, then proves the swap", land.package, change::TOOLKIT.0));
    } else {
        for s in &steps {
            let path = spec_file(&specs_dir, s)?;
            let swap = s.op() == Operation::ReplaceBody;
            let record_out = specs_dir.join(format!("{}.record.json", s.name));
            let o = transform::run(&root, &transform::Options { spec: path, write: opts.write, tests: swap, out: Some(record_out), ..transform::Options::default() })?;
            said.extend(o.said.iter().map(|l| format!("  {l}")));
            if swap {
                (expansion, compared) = proof_of(o.record_file.as_ref());
            }
            let ok = o.exit_code == 0;
            written |= o.record.written;
            transforms.push(o.record);
            if !ok {
                proved = false;
                break;
            }
        }
    }
    if !proved && !written {
        said.push("nothing written: not proved".into());
    }
    let same = expansion.as_ref().is_some_and(|e| e.same);
    let (before, after) = expansion.map(|e| (e.before, e.after)).unwrap_or_default();
    let (tests_exit, tests, failing_at_base, new_failures) = match compared {
        Some(c) => (c.exit, c.results, c.failing_at_base, c.new_failures),
        None => (None, Vec::new(), Vec::new(), Vec::new()),
    };
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
        failing_at_base,
        new_failures,
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

pub const TO_TOOLKIT_BATCH_SCHEMA: &str = "code_passenger.to_toolkit_batch.v1";

/// What `transform --batch LIST --toolkit` was asked.
#[derive(Debug, Clone, Default)]
pub struct BatchOptions {
    /// Each member: a function's path (its spec is the matcher's), or a change spec file (custodian's, preflighted):
    /// its target is the function, and the matcher must write the same body now.
    pub members: Vec<String>,
    pub write: bool,
    /// Where the specs, each member's transform record and the batch's record go.
    pub out: Option<PathBuf>,
    /// The preflight receipt each member runs on, by spec name.
    pub preflights: BTreeMap<String, i64>,
    /// The receipt of the one function of this shape swapped alone first.
    pub after_proof: Option<i64>,
}

/// One member of a batch as the proof saw it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchMember {
    pub function: String,
    pub package: String,
    pub spec: String,
    pub pieces: Vec<String>,
    pub expanded_before: String,
    pub expanded_after: String,
    pub same_expansion: bool,
}

/// What a batch did and proved.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolkitBatchRecord {
    pub schema: String,
    pub after_proof: Option<i64>,
    /// The members swapped (written with `--write`).
    pub members: Vec<BatchMember>,
    pub excluded: Vec<transform::Excluded>,
    /// Per crate, the `test result:` lines on the copy after.
    pub tests: BTreeMap<String, Vec<String>>,
    pub failing_at_base: Vec<String>,
    pub new_failures: Vec<String>,
    /// Each member's transform record, in the order written.
    pub records: Vec<String>,
    pub written: bool,
}

/// What a batch did.
#[derive(Debug)]
pub struct BatchOutcome {
    pub record: ToolkitBatchRecord,
    pub record_file: Option<PathBuf>,
    pub transform: Option<transform::BatchOutcome>,
    pub said: Vec<String>,
    pub exit_code: i32,
}

/// The same change: what the matcher writes now and what the spec says (its name and objective aside).
fn same_change(a: &ChangeSpec, b: &ChangeSpec) -> bool {
    let body = |s: &ChangeSpec| serde_json::to_value(&s.function).unwrap_or_default();
    a.op() == b.op() && a.target == b.target && body(a) == body(b)
}

/// `transform --batch LIST --toolkit`: the members matched again, placed together on one copy, one expansion per
/// crate before and after, the tests once against the base, then the batch made by `transform --batch`.
pub fn run_batch(root: &Path, opts: &BatchOptions) -> Result<BatchOutcome> {
    let root = workspace_root(root)?;
    let k = crate::cargo::builder(&root)?.ignore_parse_errors(true).parse().map_err(|e| PassengerError::Scan { detail: e.to_string() })?;
    let held = tempfile::tempdir()?;
    let specs_dir = opts.out.clone().unwrap_or_else(|| held.path().to_path_buf()).join("specs");
    let mut said = Vec::new();
    let mut excluded: Vec<transform::Excluded> = Vec::new();
    // (function, package, spec name, spec file, pieces)
    let mut members: Vec<(String, String, String, PathBuf, Vec<String>)> = Vec::new();
    for entry in &opts.members {
        let given = Path::new(entry);
        let given_spec = if entry.ends_with(".toml") || entry.ends_with(".change") || given.is_file() {
            let p = if given.is_absolute() { given.to_path_buf() } else { root.join(given) };
            Some((ChangeSpec::from_path(&p).map_err(refused)?, p))
        } else {
            None
        };
        let function = given_spec.as_ref().map(|(s, _)| s.target.clone()).unwrap_or_else(|| entry.clone());
        let name = given_spec.as_ref().map(|(s, _)| s.name.clone()).unwrap_or_else(|| function.clone());
        let mut out = |why: String| excluded.push(transform::Excluded { spec: name.clone(), why });
        let f = match custodian_kit::route::find_fn(&k.functions, &function) {
            Ok(f) => f,
            Err(e) => {
                out(e.to_string());
                continue;
            }
        };
        let (Some(line), Some(column)) = (f.location.line, f.location.column) else {
            out(format!("{function}: klepto has no place for it"));
            continue;
        };
        let text = std::fs::read_to_string(&f.location.path)?;
        let Some(at) = matcher::function_at(&text, line, column, &function) else {
            out(format!("{function}: no function at {}:{line}", f.location.path.display()));
            continue;
        };
        let Some(swap) = at.spec.clone() else {
            out(if at.near_spec.is_some() { format!("near only: {}", at.matched.differences().join("; ")) } else { "no toolkit shape in its body".into() });
            continue;
        };
        let land = match change::landing(&k, &root, &swap) {
            Ok(l) => l,
            Err(e) => {
                out(e.to_string());
                continue;
            }
        };
        if !k.features.get(&land.package).is_some_and(|m| m.dependencies.iter().any(|d| d.name == change::TOOLKIT.0)) {
            out(format!("{} does not depend on {}: swap one function of this shape alone first, its first step adds it", land.package, change::TOOLKIT.0));
            continue;
        }
        let pieces = at.matched.whole.iter().filter(|w| w.level == Level::Exact).map(|w| w.shape().piece.clone()).chain(at.matched.steps.iter().filter(|s| s.matched.level == Level::Exact && !s.comment).map(|s| s.matched.shape().piece.clone())).collect();
        let (spec_name, spec_path) = match given_spec {
            Some((s, p)) => {
                if !same_change(&s, &swap) {
                    out("the code moved since the spec was written: the matcher writes another body now".into());
                    continue;
                }
                (s.name, p)
            }
            None => (swap.name.clone(), spec_file(&specs_dir, &swap)?),
        };
        members.push((function, land.package.clone(), spec_name, spec_path, pieces));
    }
    said.push(format!("batch of {}{}: {} matched exactly now, {} left out", opts.members.len(), opts.after_proof.map(|s| format!(" after proof #{s}")).unwrap_or_default(), members.len(), excluded.len()));

    // the proof is the batch transform's: the members placed together on the proof place, one cargo check, one
    // expansion per crate before and after (a member not the same tokens left out and named), the crates' tests once
    // against the same tests before
    let t = transform::run_batch(
        &root,
        &transform::BatchOptions {
            specs: members.iter().map(|m| m.3.clone()).collect(),
            write: opts.write,
            out_dir: opts.out.as_ref().map(|d| d.join("transforms")),
            preflights: opts.preflights.clone(),
            after_proof: opts.after_proof,
            tests: true,
        },
    )?;
    said.extend(t.said.iter().map(|l| format!("  {l}")));
    excluded.extend(t.excluded.iter().cloned());
    let made: BTreeSet<&String> = t.records.iter().map(|(r, _)| &r.spec_name).collect();
    let proof: Vec<BatchMember> = members
        .iter()
        .filter(|m| made.contains(&m.2))
        .map(|(function, package, spec, _, pieces)| {
            let e = t.expansions.get(function);
            BatchMember {
                function: function.clone(),
                package: package.clone(),
                spec: spec.clone(),
                pieces: pieces.clone(),
                expanded_before: e.map(|e| e.before.clone()).unwrap_or_default(),
                expanded_after: e.map(|e| e.after.clone()).unwrap_or_default(),
                same_expansion: e.is_some_and(|e| e.same),
            }
        })
        .collect();
    let tests: BTreeMap<String, Vec<String>> = t.tests.iter().map(|(p, c)| (p.clone(), c.results.clone())).collect();
    let failing_at_base: Vec<String> = t.tests.iter().flat_map(|(p, c)| c.failing_at_base.iter().map(move |x| format!("{p} {x}"))).collect();
    let new_failures: Vec<String> = t.tests.iter().flat_map(|(p, c)| c.new_failures.iter().map(move |x| format!("{p} {x}"))).collect();
    let written = t.records.iter().any(|(r, _)| r.written);
    let records: Vec<String> = t.records.iter().filter_map(|(_, f)| f.as_ref().map(|f| f.display().to_string())).collect();
    let exit_code = t.exit_code;
    let made = Some(t);
    let record = ToolkitBatchRecord { schema: TO_TOOLKIT_BATCH_SCHEMA.into(), after_proof: opts.after_proof, members: proof, excluded, tests, failing_at_base, new_failures, records, written };
    let record_file = opts.out.as_ref().map(|d| d.join("to_toolkit_batch.json"));
    if let Some(p) = &record_file {
        std::fs::create_dir_all(p.parent().unwrap_or(Path::new(".")))?;
        std::fs::write(p, serde_json::to_string_pretty(&record)? + "\n")?;
    }
    Ok(BatchOutcome { record, record_file, transform: made, said, exit_code })
}

/// What a batch prints: what it did, the members left out, the diff, each member's record.
pub fn render_batch(o: &BatchOutcome) -> String {
    let mut s = String::new();
    s.push_str(&format!("to-toolkit batch: {} members swapped{}\n", o.record.members.len(), if o.record.written { "" } else { " (not written)" }));
    for l in &o.said {
        s.push_str(&format!("{l}\n"));
    }
    for e in &o.record.excluded {
        s.push_str(&format!("left out: {} — {}\n", e.spec, e.why));
    }
    if let Some(t) = &o.transform {
        s.push_str(&t.edits.diff_from(&t.root));
        for (r, f) in &t.records {
            if let Some(f) = f {
                s.push_str(&format!("record: {} {}\n", r.spec_name, f.display()));
            }
        }
    }
    if let Some(f) = &o.record_file {
        s.push_str(&format!("batch record: {}\n", f.display()));
    }
    s
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

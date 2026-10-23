// Copyright © 2026 1o. All rights reserved.

//! `transform --spec S [--write]`: one bounded change spec (custodian_kit's
//! `change::ChangeSpec`) made in the code.
//!
//! The spec says what to make or change; custodian_kit says where it lands
//! (`change::landing`, the one rule the preflight reads too) and renders what
//! is written from the spec's fields (`change::render_*`: bodies are
//! simple_impl 0.1.12 toolkit pieces, whole functions and trait impls its
//! generators). This module places the text at the place syn finds in the
//! file (the item's span from klepto's line: its closing brace, the line
//! after it, the attributes above it), cascades where the operation says so
//! (every struct literal gets a new field's default; every call a new
//! parameter's argument; every resolved occurrence a new name), and never
//! edits what it only lists (a match without a catch-all, an occurrence
//! inside macro tokens).
//!
//! Nothing is written before it is proved: the edits are checked by cargo
//! on a scratch copy over the check matrix of the crate it lands in and of
//! every crate that depends on it (`CargoOracle`), and a fresh klepto scan of
//! the copy must show the facts the operation promises and the spec's own
//! postconditions. With `--write` the tree is checkpointed in the passenger
//! store before and after, and the transform's record (custodian_kit's
//! `transform::TransformRecord`: every file's digest before and after, the
//! lines written, the checks, the postconditions) is kept; `--revert RECORD`
//! puts every file it wrote back as it was.

use crate::error::{PassengerError, Result};
use crate::gate::{relative, workspace_root, EditSet, FileEdit};
use custodian_kit::change::{self, At, ChangeSpec, Landing, Operation};
use custodian_kit::transform::{Checked, Expansion, FileWrite, TransformRecord, TRANSFORM_SCHEMA};
use proc_macro2::LineColumn;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use syn::spanned::Spanned;
use syn::visit::Visit;

/// What `transform` was asked.
#[derive(Debug, Clone, Default)]
pub struct Options {
    pub spec: PathBuf,
    pub write: bool,
    /// Skip cargo; with `write`, only with `unproven`.
    pub no_cargo: bool,
    pub unproven: bool,
    /// Where the record goes (else the project's `.custodian/data/transforms/`, else nowhere).
    pub out: Option<PathBuf>,
    /// The preflight receipt it runs on.
    pub preflight: Option<i64>,
    /// The landed crate's tests run on the proof, compared with the same tests before the change.
    pub tests: bool,
}

/// What a transform did.
#[derive(Debug)]
pub struct Outcome {
    pub root: PathBuf,
    pub record: TransformRecord,
    pub record_file: Option<PathBuf>,
    pub edits: EditSet,
    pub exit_code: i32,
    pub said: Vec<String>,
}

fn refused(detail: impl std::fmt::Display) -> PassengerError {
    PassengerError::TransformRefused { detail: detail.to_string() }
}

/// The working set: each file's text before, and as the edits leave it.
#[derive(Default)]
struct Tree {
    files: BTreeMap<PathBuf, (Option<String>, String)>,
}

/// One spelling per file (`/var/...` and `/private/var/...` are one file): the canonical path, or its parent's.
fn canon(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| match (p.parent().and_then(|d| d.canonicalize().ok()), p.file_name()) {
        (Some(d), Some(n)) => d.join(n),
        _ => p.to_path_buf(),
    })
}

impl Tree {
    fn text(&mut self, p: &Path) -> Result<&mut String> {
        let p = &canon(p);
        if !self.files.contains_key(p) {
            let t = std::fs::read_to_string(p).map_err(|e| PassengerError::Io { detail: format!("{}: {e}", p.display()) })?;
            self.files.insert(p.to_path_buf(), (Some(t.clone()), t));
        }
        Ok(&mut self.files.get_mut(p).expect("just inserted").1)
    }

    fn create(&mut self, p: &Path, text: String) -> Result<()> {
        let p = &canon(p);
        if p.exists() || self.files.contains_key(p) {
            return Err(refused(format!("{} already exists", p.display())));
        }
        self.files.insert(p.to_path_buf(), (None, text));
        Ok(())
    }

    fn edits(&self) -> EditSet {
        EditSet { files: self.files.iter().map(|(p, (b, a))| FileEdit { path: p.clone(), before: b.clone().unwrap_or_default(), after: a.clone() }).collect(), manifest: None }
    }
}

// ---------------------------------------------------------------------------
// places in a file, from syn's spans

/// A line and column (proc_macro2's: line 1-based, column in characters) as a byte offset in `text`.
fn offset(text: &str, at: LineColumn) -> usize {
    let mut off = 0;
    for (i, line) in text.split_inclusive('\n').enumerate() {
        if i + 1 == at.line {
            return off + line.chars().take(at.column).map(char::len_utf8).sum::<usize>();
        }
        off += line.len();
    }
    text.len()
}

/// The byte offset of the start of `line` (1-based).
fn line_start(text: &str, line: usize) -> usize {
    offset(text, LineColumn { line, column: 0 })
}

/// The byte offset just past the end of `line` (its newline included).
fn line_end(text: &str, line: usize) -> usize {
    line_start(text, line + 1)
}

fn indent_of(text: &str, line: usize) -> String {
    text.lines().nth(line.saturating_sub(1)).map(|l| l.chars().take_while(|c| c.is_whitespace()).collect()).unwrap_or_default()
}

fn parse(text: &str, path: &Path) -> Result<syn::File> {
    syn::parse_file(text).map_err(|e| refused(format!("{}: syn cannot parse it: {e}", path.display())))
}

/// An item's span (attributes and docs included) and the span of its body's closing brace, when it has one.
#[derive(Debug, Clone, Copy)]
struct Spans {
    start: LineColumn,
    end: LineColumn,
    /// The closing `}` of a struct's fields, an enum's variants, an impl's items, a module's items.
    close: Option<LineColumn>,
    /// The opening `{` of a function's body.
    body_open: Option<LineColumn>,
}

struct FindItem {
    line: usize,
    found: Option<Spans>,
}

impl FindItem {
    fn consider(&mut self, start: LineColumn, end: LineColumn, close: Option<LineColumn>, body_open: Option<LineColumn>) {
        if start.line <= self.line && self.line <= end.line {
            // the innermost item holding the line wins, and one starting at it over one around it
            let better = match &self.found {
                None => true,
                Some(f) => start.line == self.line && f.start.line != self.line || (start.line >= f.start.line && end.line <= f.end.line),
            };
            if better {
                self.found = Some(Spans { start, end, close, body_open });
            }
        }
    }
}

impl<'ast> Visit<'ast> for FindItem {
    fn visit_item(&mut self, i: &'ast syn::Item) {
        let sp = i.span();
        let close = match i {
            syn::Item::Struct(s) => match &s.fields {
                syn::Fields::Named(n) => Some(n.brace_token.span.close().start()),
                _ => None,
            },
            syn::Item::Enum(e) => Some(e.brace_token.span.close().start()),
            syn::Item::Impl(x) => Some(x.brace_token.span.close().start()),
            syn::Item::Mod(m) => m.content.as_ref().map(|(b, _)| b.span.close().start()),
            syn::Item::Trait(t) => Some(t.brace_token.span.close().start()),
            _ => None,
        };
        let body = match i {
            syn::Item::Fn(f) => Some(f.block.brace_token.span.open().start()),
            _ => None,
        };
        self.consider(sp.start(), sp.end(), close, body);
        syn::visit::visit_item(self, i);
    }

    fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
        let sp = f.span();
        self.consider(sp.start(), sp.end(), None, Some(f.block.brace_token.span.open().start()));
        syn::visit::visit_impl_item_fn(self, f);
    }

    fn visit_trait_item_fn(&mut self, f: &'ast syn::TraitItemFn) {
        let sp = f.span();
        self.consider(sp.start(), sp.end(), None, f.default.as_ref().map(|b| b.brace_token.span.open().start()));
        syn::visit::visit_trait_item_fn(self, f);
    }
}

/// The item at klepto's line in `text`.
fn item_at(text: &str, path: &Path, line: u32) -> Result<Spans> {
    let file = parse(text, path)?;
    let mut f = FindItem { line: line as usize, found: None };
    f.visit_file(&file);
    f.found.ok_or_else(|| refused(format!("{}:{line}: no item there", path.display())))
}

/// One replacement in a file's text: bytes `start..end` of the text it was computed on become `text`.
type Splice = (usize, usize, String);

/// Splices computed on one text, applied together (the last first, so no offset moves under another).
fn splice(text: &mut String, mut edits: Vec<Splice>) {
    edits.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    for (a, b, t) in edits {
        text.replace_range(a..b, &t);
    }
}

/// The splices that put text just before a closing brace: on lines of their own when the brace starts its line,
/// else inline; the item before it gets a comma when it lacks one and `comma` is asked.
fn before_close(text: &str, close: LineColumn, lines: &str, comma: bool) -> Vec<Splice> {
    let at = offset(text, close);
    let before = &text[..at];
    let own_line = before.rsplit('\n').next().is_some_and(|l| l.trim().is_empty());
    let last = before.trim_end().chars().last();
    let needs_comma = comma && !matches!(last, Some(',') | Some('{') | None);
    let mut out = Vec::new();
    if own_line {
        if needs_comma {
            let end_of_prev = before.trim_end().len();
            out.push((end_of_prev, end_of_prev, ",".to_string()));
        }
        let line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
        out.push((line_start, line_start, lines.to_string()));
    } else {
        let inline = lines.lines().map(str::trim).collect::<Vec<_>>().join(" ");
        let inline = inline.trim_end_matches(',');
        let end_of_prev = before.trim_end().len();
        let pre = if needs_comma { ", " } else { " " };
        out.push((end_of_prev, end_of_prev, format!("{pre}{inline}")));
    }
    out
}

/// [`before_close`], applied.
fn insert_before_close(text: &mut String, close: LineColumn, lines: &str, comma: bool) {
    let e = before_close(text, close, lines, comma);
    splice(text, e);
}

/// The top-level `#[cfg(test)] mod tests` of a file, by its first line.
fn test_module_line(file: &syn::File) -> Option<usize> {
    file.items.iter().find_map(|i| match i {
        syn::Item::Mod(m) if m.ident == "tests" && m.attrs.iter().any(|a| a.path().is_ident("cfg")) => Some(i.span().start().line),
        _ => None,
    })
}

/// Text placed at a file's end, before its `#[cfg(test)] mod tests` when it has one.
fn place_at_end(text: &mut String, path: &Path, block: &str) -> Result<()> {
    let file = parse(text, path)?;
    let at = match test_module_line(&file) {
        Some(l) => line_start(text, l),
        None => {
            if !text.ends_with('\n') {
                text.push('\n');
            }
            text.len()
        }
    };
    let mut ins = String::new();
    if at > 0 && !text[..at].ends_with("\n\n") {
        ins.push('\n');
    }
    ins.push_str(block);
    if at < text.len() {
        ins.push('\n');
    }
    text.insert_str(at, &ins);
    Ok(())
}

/// Text placed on the lines after an item.
fn place_after(text: &mut String, sp: Spans, block: &str) {
    let at = line_end(text, sp.end.line);
    let mut ins = String::from("\n");
    ins.push_str(block);
    text.insert_str(at, &ins);
}

/// One attribute line placed above an item: after its doc lines, above its other attributes; a `#[derive(..)]`
/// merged into the one it has.
fn place_above(text: &mut String, sp: Spans, attr: &str) {
    let lines: Vec<String> = text.lines().map(str::to_string).collect();
    let lead = crate::gate::leading(&lines, sp.start.line - 1);
    if let Some(inner) = attr.trim().strip_prefix("#[derive(").and_then(|r| r.strip_suffix(")]"))
        && let Some((a, _)) = lead.attrs.iter().find(|(a, b)| b - a == 1 && lines[*a].trim().starts_with("#[derive(")) {
            let line = &lines[*a];
            if let Some(close) = line.rfind(")]") {
                let have: Vec<&str> = line.trim()[9..].trim_end_matches(")]").split(',').map(str::trim).collect();
                let add: Vec<&str> = inner.split(',').map(str::trim).filter(|d| !have.contains(d)).collect();
                if add.is_empty() {
                    return;
                }
                let at = line_start(text, a + 1) + close;
                text.insert_str(at, &format!(", {}", add.join(", ")));
                return;
            }
        }
    let at_line = lead.attrs.first().map(|(a, _)| *a).unwrap_or(lead.decl);
    let indent = crate::gate::indent_at(&lines, at_line);
    let at = line_start(text, at_line + 1);
    text.insert_str(at, &format!("{indent}{}\n", attr.trim()));
}

// ---------------------------------------------------------------------------
// cascades over the uses klepto resolves

struct Literals {
    want: BTreeSet<(usize, usize)>,
    found: Vec<(LineColumn, bool)>,
}

impl<'ast> Visit<'ast> for Literals {
    fn visit_expr_struct(&mut self, e: &'ast syn::ExprStruct) {
        let s = e.path.span().start();
        if self.want.contains(&(s.line, s.column)) {
            self.found.push((e.brace_token.span.close().start(), e.rest.is_some()));
        }
        syn::visit::visit_expr_struct(self, e);
    }
}

struct Calls {
    want: BTreeSet<(usize, usize)>,
    /// The function's name: a call is one of its calls only when it names it.
    name: String,
    /// The closing paren of each call found, and whether it has arguments.
    found: Vec<(LineColumn, bool)>,
    /// The wanted places met.
    met: BTreeSet<(usize, usize)>,
}

impl Calls {
    fn new(want: BTreeSet<(usize, usize)>, name: &str) -> Calls {
        Calls { want, name: name.to_string(), found: Vec::new(), met: BTreeSet::new() }
    }

    fn meet(&mut self, at: &[LineColumn]) -> bool {
        let hits: Vec<(usize, usize)> = at.iter().map(|s| (s.line, s.column)).filter(|k| self.want.contains(k)).collect();
        self.met.extend(hits.iter().copied());
        !hits.is_empty()
    }
}

impl<'ast> Visit<'ast> for Calls {
    fn visit_expr_call(&mut self, e: &'ast syn::ExprCall) {
        let names_it = matches!(&*e.func, syn::Expr::Path(p) if p.path.segments.last().is_some_and(|s| s.ident == self.name.as_str()));
        if names_it && self.meet(&[e.func.span().start()]) {
            self.found.push((e.paren_token.span.close().start(), !e.args.is_empty()));
        }
        syn::visit::visit_expr_call(self, e);
    }

    fn visit_expr_method_call(&mut self, e: &'ast syn::ExprMethodCall) {
        // klepto places a method call at its receiver; the method's own name is the other place a call is named at
        if e.method == self.name.as_str() && self.meet(&[e.receiver.span().start(), e.method.span().start()]) {
            self.found.push((e.paren_token.span.close().start(), !e.args.is_empty()));
        }
        syn::visit::visit_expr_method_call(self, e);
    }
}

/// The byte offset of the parenthesis that closes the one at `open`, strings and characters skipped.
fn matching_paren(text: &str, open: usize) -> Option<usize> {
    matching_close(text, open).filter(|i| text.as_bytes()[*i] == b')')
}

/// The byte offset of the bracket (`)`, `]` or `}`) that closes the one at `open`, strings and characters skipped.
fn matching_close(text: &str, open: usize) -> Option<usize> {
    let b = text.as_bytes();
    let mut depth = 0i32;
    let mut i = open;
    while i < b.len() {
        match b[i] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            b'"' => {
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b'\'' if i + 2 < b.len() && (b[i + 2] == b'\'' || b[i + 1] == b'\\') => {
                i += 1;
                while i < b.len() && b[i] != b'\'' {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// The arguments between `open` and `close` (the call's parentheses), split at the commas outside any bracket or
/// string, each as its text, trimmed.
fn arguments(text: &str, open: usize, close: usize) -> Vec<String> {
    let inner = &text[open + 1..close];
    let mut out = Vec::new();
    let (mut depth, mut start, mut in_str, mut esc) = (0i32, 0usize, false, false);
    for (i, c) in inner.char_indices() {
        if in_str {
            match (esc, c) {
                (true, _) => esc = false,
                (false, '\\') => esc = true,
                (false, '"') => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                out.push(inner[start..i].trim().to_string());
                start = i + 1;
            }
            _ => {}
        }
    }
    let last = inner[start..].trim();
    if !last.is_empty() {
        out.push(last.to_string());
    }
    out
}

/// A call klepto found where syn does not look (inside a macro's tokens): from klepto's place for it (the callee's
/// path, or a method call's receiver), the first `name` after it on its line that is a whole word followed by its
/// argument list. Its `(` and `)` byte offsets.
fn call_in_text(text: &str, line: u32, column: Option<u32>, name: &str) -> Option<(usize, usize)> {
    let (l, c) = syn_at(line, column);
    let starts = [offset(text, LineColumn { line: l, column: c }), offset(text, LineColumn { line: l, column: c + 1 })];
    let from = starts.into_iter().min()?;
    let end = text[from..].find('\n').map(|e| from + e).unwrap_or(text.len());
    let mut at = from;
    while let Some(i) = text[at..end].find(name) {
        let a = at + i;
        let before = text[..a].chars().last();
        let mut rest = a + name.len();
        let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
        if !word(before) && !word(text[rest..].chars().next()) {
            // a turbofish between the name and its arguments
            let tail = text[rest..].trim_start();
            rest = text.len() - tail.len();
            if tail.starts_with("::<") {
                let mut depth = 0;
                for (j, ch) in tail.char_indices() {
                    match ch {
                        '<' => depth += 1,
                        '>' => {
                            depth -= 1;
                            if depth == 0 {
                                rest += j + 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                rest = text.len() - text[rest..].trim_start().len();
            }
            if text[rest..].starts_with('(') {
                return matching_paren(text, rest).map(|close| (rest, close));
            }
        }
        at = a + name.len();
    }
    None
}

/// The calls a macro's arguments make by one of `paths` (custodian_kit's `change::macro_calls`: the paths in its tokens
/// that resolve to the function): from the macro's place (its path, as klepto records it) to the bracket closing its
/// arguments, each place a path is written as a whole word (not after `.` or another path segment), followed by its
/// argument list. Each call's `(` and `)` byte offsets.
fn calls_in_macro(text: &str, line: u32, column: Option<u32>, paths: &[String]) -> Vec<(usize, usize)> {
    let (l, c) = syn_at(line, column);
    let Some(from) = [offset(text, LineColumn { line: l, column: c }), offset(text, LineColumn { line: l, column: c + 1 })].into_iter().min() else { return Vec::new() };
    // the macro's `!`, then the bracket its arguments open with
    let Some(bang) = text[from..].find('!').map(|i| from + i) else { return Vec::new() };
    let Some(open) = text[bang + 1..].find(|ch: char| !ch.is_whitespace()).map(|i| bang + 1 + i).filter(|i| matches!(text.as_bytes()[*i], b'(' | b'[' | b'{')) else { return Vec::new() };
    let Some(close) = matching_close(text, open) else { return Vec::new() };
    let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    let mut out = Vec::new();
    for p in paths {
        let mut at = open + 1;
        while let Some(i) = text[at..close].find(p.as_str()) {
            let a = at + i;
            let before = text[..a].trim_end_matches(' ').chars().last();
            let end = a + p.len();
            at = end;
            if word(text[..a].chars().last()) || word(text[end..].chars().next()) || matches!(before, Some('.' | ':')) {
                continue;
            }
            let rest = end + (text[end..close].len() - text[end..close].trim_start().len());
            if text[rest..].starts_with('(')
                && let Some(c) = matching_paren(text, rest).filter(|c| *c < close)
            {
                out.push((rest, c));
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// klepto's `(file, line, column)` (column 1-based or 0-based as it records) as syn's `(line, column 0-based)`.
fn syn_at(line: u32, column: Option<u32>) -> (usize, usize) {
    (line as usize, column.map(|c| c.saturating_sub(1) as usize).unwrap_or(0))
}

/// Both spellings of a klepto column, so a literal is found whichever klepto records.
fn both(line: u32, column: Option<u32>) -> [(usize, usize); 2] {
    let (l, c) = syn_at(line, column);
    [(l, c), (l, c + 1)]
}

// ---------------------------------------------------------------------------
// the operations

/// What planning an operation made: the edits, the source generated, the sites listed and not edited, and what a
/// fresh scan must show.
/// A spec that keeps what its function compiles to: a body swapped for toolkit calls whose expansion it already is.
fn keeps_expansion(spec: &ChangeSpec) -> bool {
    spec.same_expansion && spec.op() == Operation::ReplaceBody
}

/// The packages a proof compiles, for the packages a change lands in (custodian_kit's rule).
fn proof_packages_of(k: &klepto::Klepto, spec: &ChangeSpec, land: &Landing, landed: &[String]) -> Vec<change::ProofPackage> {
    let mut out: Vec<change::ProofPackage> = Vec::new();
    for p in landed {
        let at = Landing { package: p.clone(), ..land.clone() };
        for pp in change::proof_packages(k, spec, &at) {
            match out.iter_mut().find(|x| x.package == pp.package) {
                Some(x) => x.tests |= pp.tests,
                None => out.push(pp),
            }
        }
    }
    out
}

/// What a proof compiles, in one line.
fn proof_line(packages: &[change::ProofPackage]) -> String {
    let lands: Vec<&str> = packages.iter().filter(|p| p.tests).map(|p| p.package.as_str()).collect();
    let direct = packages.iter().filter(|p| p.why == "direct dependent").count();
    let readers = packages.iter().filter(|p| p.why.starts_with("reads ")).count();
    format!(
        "cargo check over {} packages: {} with its tests, {} that depend on it directly{}",
        packages.len(),
        lands.join(", "),
        direct,
        if readers > 0 { format!(", {readers} that read what it changes") } else { String::new() }
    )
}

/// The tests' verdict, in one line.
fn tests_line(package: &str, c: &crate::proof_dir::Compared) -> String {
    let mut l = format!("cargo test -p {package} on the copy: exit {:?}; {}", c.exit, c.results.join(" | "));
    if !c.failing_at_base.is_empty() {
        l.push_str(&format!("; {} failing the same before the change, not its", c.failing_at_base.len()));
    }
    for t in &c.new_failures {
        l.push_str(&format!("; fails after and passed before: {t}"));
    }
    l
}

/// The tests' side record beside a transform's record.
pub fn tests_file(record: &Path) -> PathBuf {
    record.with_file_name(format!("{}.tests.json", record.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()))
}

/// The expansion's side record beside a transform's record.
pub fn expansion_file(record: &Path) -> PathBuf {
    record.with_file_name(format!("{}.expansion.json", record.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()))
}

struct Planned {
    tree: Tree,
    generated: Vec<String>,
    listed: Vec<String>,
    /// What else the edits do, said for a person (an import dropped or carried, a function removed).
    also: Vec<String>,
    /// `exists <id>`, `field <type> <name>`, `variant <enum> <name>`, `method <type> <name>`, `absent <id>`, `text <file> <line>`.
    facts: Vec<String>,
}

fn abs(root: &Path, rel: &str) -> PathBuf {
    root.join(rel)
}

fn ty_name(id: &str) -> &str {
    // a route that names its impl (`path[impl From<i64> for Level]`): the path's last segment
    let id = custodian_kit::route::split(id).0;
    id.rsplit("::").next().unwrap_or(id)
}

fn module_path_of(id: &str) -> &str {
    id.rsplit_once("::").map(|(m, _)| m).unwrap_or(id)
}

/// The edits an operation makes, unwritten.
fn plan(k: &klepto::Klepto, root: &Path, spec: &ChangeSpec, land: &Landing) -> Result<Planned> {
    use Operation::*;
    let mut p = Planned { tree: Tree::default(), generated: Vec::new(), listed: Vec::new(), also: Vec::new(), facts: Vec::new() };
    // an attribute macro that rewrites the item: the operation writes it as declared, which the macro does not keep
    if let Some(why) = land.item.as_deref().and_then(|i| change::rewritten_by(k, spec.op(), i)) {
        return Err(refused(why));
    }
    let file = abs(root, &land.file);
    let item = land.item.clone().unwrap_or_default();
    let line_of = |at: &At| -> u32 {
        match at {
            At::Inside { line } | At::After { line } | At::Above { line } | At::At { line } => *line,
            _ => 1,
        }
    };
    match spec.op() {
        MakeStruct => {
            let i = spec.item.as_ref().ok_or_else(|| refused("make_struct needs item"))?;
            let text = change::render_struct(i).map_err(refused)?;
            place_at_end(p.tree.text(&file)?, &file, &text)?;
            p.generated.push(text);
            p.facts.push(format!("exists {}::{}", land.module, i.name));
        }
        MakeEnum => {
            let i = spec.item.as_ref().ok_or_else(|| refused("make_enum needs item"))?;
            let text = change::render_enum(i);
            place_at_end(p.tree.text(&file)?, &file, &text)?;
            p.generated.push(text);
            p.facts.push(format!("exists {}::{}", land.module, i.name));
        }
        MakeFn => {
            let f = spec.function.as_ref().ok_or_else(|| refused("make_fn needs function"))?;
            let text = change::render_fn(f, "").map_err(refused)?;
            place_at_end(p.tree.text(&file)?, &file, &text)?;
            let fact = if f.generator.is_some() { text_fact(&land.file, &text) } else { format!("exists {}::{}", land.module, f.name) };
            p.generated.push(text);
            p.facts.push(fact);
        }
        MakeImpl | AddTraitImpl => {
            let tname = ty_name(&item).to_string();
            let text = match (&spec.generator, spec.op()) {
                (Some(g), AddTraitImpl) => change::render_impl_piece(g).map_err(refused)?,
                _ => change::render_impl(&tname, if spec.op() == AddTraitImpl { spec.trait_name.as_deref() } else { None }, &spec.assoc, &spec.functions, None).map_err(refused)?,
            };
            let t = p.tree.text(&file)?;
            let sp = item_at(t, &file, line_of(&land.at))?;
            place_after(t, sp, &text);
            if spec.generator.is_some() {
                // a toolkit impl generator: its line is the fact, the compiler the proof
                p.facts.push(text_fact(&land.file, &text));
            } else if spec.op() == AddTraitImpl {
                p.facts.push(format!("impl {item} {}", spec.trait_name.clone().unwrap_or_default()));
            }
            for f in &spec.functions {
                p.facts.push(if f.generator.is_some() || spec.op() == AddTraitImpl { text_fact(&land.file, &change::render_fn(f, "    ").unwrap_or_default()) } else { format!("method {item} {}", f.name) });
            }
            p.generated.push(text);
        }
        AddMethod | AddConstructor => {
            let mut f = spec.function.clone().ok_or_else(|| refused("add_method needs function"))?;
            if spec.op() == AddConstructor && f.body.is_empty() && f.generator.is_none() {
                // `Self { field: param, .. }`: each parameter its field
                if f.ret.is_none() {
                    f.ret = Some("Self".into());
                }
                let names: Vec<String> = f.params.iter().map(|x| x.name.clone()).collect();
                f.body = vec![change::Piece { piece: "value".into(), value: Some(format!("Self {{ {} }}", names.join(", "))), ..Default::default() }];
            }
            let t = p.tree.text(&file)?;
            match &land.at {
                At::Inside { line } => {
                    let sp = item_at(t, &file, *line)?;
                    let close = sp.close.ok_or_else(|| refused(format!("{}:{line}: the impl has no body", land.file)))?;
                    let indent = format!("{}    ", indent_of(t, sp.start.line));
                    let text = change::render_fn(&f, &indent).map_err(refused)?;
                    insert_before_close(t, close, &format!("\n{text}"), false);
                    p.generated.push(text);
                }
                _ => {
                    // no inherent impl yet: one made after the type
                    let text = change::render_impl(ty_name(&item), None, &[], std::slice::from_ref(&f), None).map_err(refused)?;
                    let sp = item_at(t, &file, line_of(&land.at))?;
                    place_after(t, sp, &text);
                    p.generated.push(text);
                }
            }
            p.facts.push(fn_fact(&f, &item, &land.file, p.generated.last()));
        }
        AddField => {
            let fl = spec.field.as_ref().ok_or_else(|| refused("add_field needs field"))?;
            // every splice is computed on the file as it is, then applied together
            let mut splices: BTreeMap<PathBuf, Vec<Splice>> = BTreeMap::new();
            let t = p.tree.text(&file)?.clone();
            let sp = item_at(&t, &file, line_of(&land.at))?;
            let close = sp.close.ok_or_else(|| refused(format!("{item}: not a struct with named fields")))?;
            let indent = format!("{}    ", indent_of(&t, sp.start.line));
            let text = change::render_field(fl, &indent);
            splices.entry(canon(&file)).or_default().extend(before_close(&t, close, &text, true));
            p.generated.push(text);
            p.facts.push(format!("field {item} {}", fl.name));
            // every struct literal of the type: the default, or listed
            let mut by_file: BTreeMap<PathBuf, BTreeSet<(usize, usize)>> = BTreeMap::new();
            for o in k.paths.iter().filter(|o| o.role == klepto::PathRole::StructLiteral) {
                let resolves = matches!(&o.resolution, Some(klepto::Resolution::Local(id) | klepto::Resolution::External(id)) if *id == item);
                if let (true, Some(l)) = (resolves, o.location.line) {
                    by_file.entry(canon(&o.location.path)).or_default().extend(both(l, o.location.column));
                }
            }
            for (f, want) in by_file {
                let t = p.tree.text(&f)?.clone();
                let parsed = parse(&t, &f)?;
                let mut v = Literals { want, found: Vec::new() };
                v.visit_file(&parsed);
                for (close, rest) in v.found {
                    let at = format!("{}:{}", relative(root, &f).map(|r| r.display().to_string()).unwrap_or_default(), close.line);
                    match (&fl.default, rest) {
                        (_, true) => p.listed.push(format!("struct literal {at}: `..` fills the new field")),
                        (Some(d), false) => {
                            let indent = format!("{}    ", indent_of(&t, close.line));
                            splices.entry(f.clone()).or_default().extend(before_close(&t, close, &format!("{indent}{}: {d},\n", fl.name), true));
                        }
                        (None, false) => p.listed.push(format!("struct literal {at}: needs the new field")),
                    }
                }
            }
            for (f, e) in splices {
                splice(p.tree.text(&f)?, e);
            }
        }
        AddVariant => {
            let v = spec.variant.as_ref().ok_or_else(|| refused("add_variant needs variant"))?;
            let t = p.tree.text(&file)?;
            let sp = item_at(t, &file, line_of(&land.at))?;
            let close = sp.close.ok_or_else(|| refused(format!("{item}: not an enum")))?;
            let indent = format!("{}    ", indent_of(t, sp.start.line));
            let text = change::render_variant(v, &indent);
            insert_before_close(t, close, &text, true);
            p.generated.push(text);
            p.facts.push(format!("variant {item} {}", v.name));
            for m in &k.matches {
                let names_it = m.arms.iter().flat_map(|a| a.paths.iter()).any(|x| matches!(&x.resolution, Some(klepto::Resolution::Local(id) | klepto::Resolution::External(id)) if id.starts_with(&format!("{item}::"))));
                if names_it && !m.arms.iter().any(|a| a.catch_all) {
                    p.listed.push(format!("match without a catch-all {}:{}: needs an arm for {}", relative(root, &m.location.path).map(|r| r.display().to_string()).unwrap_or_default(), m.location.line.unwrap_or(0), v.name));
                }
            }
        }
        AddDerive | AddAttribute | Gate | Anchor => {
            let line = match spec.op() {
                AddDerive => format!("#[derive({})]", spec.derive.join(", ")),
                AddAttribute => spec.attribute.clone().unwrap_or_default(),
                Gate => format!("#[cfg(feature = \"{}\")]", spec.feature.clone().unwrap_or_default()),
                _ => format!("{}{} (.custodian/data/linkage/v1.{}.1o.linkage)", custodian_kit::linkage::ANCHOR, spec.linkage.clone().unwrap_or_default(), spec.linkage.clone().unwrap_or_default()),
            };
            let t = p.tree.text(&file)?;
            let sp = item_at(t, &file, line_of(&land.at))?;
            place_above(t, sp, &line);
            p.generated.push(line.clone());
            match spec.op() {
                AddDerive => spec.derive.iter().for_each(|d| p.facts.push(format!("derive {item} {d}"))),
                _ => p.facts.push(format!("text {} {line}", land.file)),
            }
        }
        AddModule | Extract => {
            let m = spec.module.as_ref().ok_or_else(|| refused("add_module needs module"))?;
            let At::NewFile { parent } = &land.at else { return Err(refused("a module lands in a new file")) };
            let parent_file = abs(root, parent);
            let mut body = m.doc.as_ref().map(|d| d.lines().map(|l| format!("//! {l}\n")).collect::<String>()).unwrap_or_default();
            // extract: the items move out of the parent into the new file, re-exported where they were
            let mut moved = Vec::new();
            if spec.op() == Extract {
                let mut spans = Vec::new();
                for id in &spec.items {
                    let d = change::item_named(k, id).ok_or_else(|| refused(format!("{id}: no such item")))?;
                    if abs(root, &relative(root, &d.location.path).map(|r| r.display().to_string()).unwrap_or_default()) != parent_file {
                        return Err(refused(format!("{id} is not in {parent}")));
                    }
                    let t = p.tree.text(&parent_file)?;
                    spans.push((item_at(t, &parent_file, d.location.line.unwrap_or(1))?, d.name.clone()));
                }
                spans.sort_by_key(|b| std::cmp::Reverse(b.0.start.line));
                let t = p.tree.text(&parent_file)?;
                let mut texts = Vec::new();
                for (sp, name) in &spans {
                    let (a, b) = (line_start(t, sp.start.line), line_end(t, sp.end.line));
                    texts.push(t[a..b].to_string());
                    t.replace_range(a..b, "");
                    moved.push(name.clone());
                }
                texts.reverse();
                if !body.is_empty() {
                    body.push('\n');
                }
                body.push_str("use super::*;\n\n");
                body.push_str(&texts.join("\n"));
            }
            let recipe = file_recipe::FileRecipe {
                recipe_id: "code_passenger.rust_module.v1".into(),
                extension: ".rs".into(),
                mime_type: "text/x-rust".into(),
                filename_template: "{name}".into(),
                storage_class: file_recipe::StorageClass::InlineBytes,
                read_modes: vec![file_recipe::ReadMode::Utf8Text],
                validation: file_recipe::ValidationRuleSet { min_bytes: None, max_bytes: None, required_fields: vec!["name".into()], require_utf8: true },
                projections: Vec::new(),
            };
            let made = file_recipe::materialize_file_object(&recipe, file_recipe::FileRecipeInput { source_kind: "change_spec".into(), fields: [("name".to_string(), m.name.clone())].into(), bytes: body.into_bytes() })
                .map_err(|e| refused(format!("the module file: {e:?}")))?;
            let new = abs(root, &land.file);
            if new.file_name().map(|n| n.to_string_lossy().into_owned()) != Some(made.filename.clone()) {
                return Err(refused(format!("the module file is {} where the landing says {}", made.filename, land.file)));
            }
            let text = String::from_utf8(made.bytes).map_err(refused)?;
            p.tree.create(&new, text.clone())?;
            p.generated.push(text);
            // its `mod` line in the parent, after the last mod line (else after the inner docs and attributes)
            let vis = m.vis.as_ref().map(|v| format!("{v} ")).unwrap_or_default();
            let t = p.tree.text(&parent_file)?;
            let parsed = parse(t, &parent_file)?;
            let last_mod = parsed.items.iter().filter(|i| matches!(i, syn::Item::Mod(m) if m.content.is_none())).map(|i| i.span().end().line).max();
            let first_item = parsed.items.first().map(|i| i.span().start().line);
            let mut decl = format!("{vis}mod {};\n", m.name);
            if !moved.is_empty() {
                decl.push_str(&format!("pub use {}::{{{}}};\n", m.name, moved.join(", ")));
            }
            match (last_mod, first_item) {
                (Some(l), _) => {
                    let at = line_end(t, l);
                    t.insert_str(at, &decl);
                }
                (None, Some(l)) => {
                    let at = line_start(t, l);
                    t.insert_str(at, &format!("{decl}\n"));
                }
                (None, None) => t.push_str(&decl),
            }
            p.facts.push(format!("module {}", land.module));
            for n in &moved {
                p.facts.push(format!("exists {}::{n}", land.module));
            }
        }
        MakeLib => {
            let new = abs(root, &land.file);
            if new.exists() {
                return Err(refused(format!("{} already exists", land.file)));
            }
            let text = spec.module.as_ref().and_then(|m| m.doc.as_ref()).map(|d| d.lines().map(|l| format!("//! {l}\n")).collect::<String>()).unwrap_or_default();
            p.tree.create(&new, text.clone())?;
            p.generated.push(text);
            p.facts.push(format!("library {}", land.package));
        }
        AddDependency => {
            let d = spec.dependency.as_ref().ok_or_else(|| refused("add_dependency needs dependency"))?;
            let dep = match &d.version {
                Some(v) => format!("{}@{v}", d.name),
                None => d.name.clone(),
            };
            let e = crate::commands::edit::dep_add_plan(root, &land.package, &dep, d.path.as_deref(), &d.features, false)?;
            for f in e.edits.files {
                let after = match d.optional {
                    Some(o) => crate::commands::edit::dep_change_text_with(&f.after, &d.name, None, None, &[], Some(o))?.0,
                    None => f.after,
                };
                p.tree.files.insert(f.path.clone(), (Some(f.before), after));
            }
            p.facts.push(format!("dependency {} {}", land.package, d.name));
            if let Some(o) = d.optional {
                p.facts.push(format!("optional {} {} {o}", land.package, d.name));
            }
        }
        AddTest => {
            let f = spec.function.as_ref().ok_or_else(|| refused("add_test needs function"))?;
            let mut f = f.clone();
            if !f.attrs.iter().any(|a| a.trim() == "#[test]") {
                f.attrs.insert(0, "#[test]".into());
            }
            let module = spec.to.clone().unwrap_or_else(|| "tests".into());
            let t = p.tree.text(&file)?;
            let parsed = parse(t, &file)?;
            let tests = parsed.items.iter().find_map(|i| match i {
                syn::Item::Mod(m) if m.ident == module.as_str() && m.content.is_some() => Some((i.span(), m.content.as_ref().map(|(b, _)| b.span.close().start()))),
                _ => None,
            });
            // a module the file does not have yet (`to`, else `tests`) is made: `#[cfg(test)] mod <name>` at its end
            match tests {
                Some((sp, Some(close))) => {
                    let indent = format!("{}    ", indent_of(t, sp.start().line));
                    let text = change::render_fn(&f, &indent).map_err(refused)?;
                    insert_before_close(t, close, &format!("\n{text}"), false);
                    p.generated.push(text);
                }
                _ => {
                    let text = format!("#[cfg(test)]\nmod {module} {{\n    use super::*;\n\n{}}}\n", change::render_fn(&f, "    ").map_err(refused)?);
                    if !t.ends_with('\n') {
                        t.push('\n');
                    }
                    t.push('\n');
                    t.push_str(&text);
                    p.generated.push(text);
                }
            }
            p.facts.push(format!("exists {}::{module}::{}", land.module, f.name));
        }
        ReplaceBody => {
            let f = spec.function.as_ref().ok_or_else(|| refused("replace_body needs function"))?;
            let old = custodian_kit::route::find_fn(&k.functions, &item).map_err(|e| refused(e.to_string()))?;
            if f.name != ty_name(&item) {
                return Err(refused(format!("{item}: the spec's function is {}, not this one", f.name)));
            }
            let t = p.tree.text(&file)?;
            let sp = item_at(t, &file, line_of(&land.at))?;
            if f.generator.is_some() {
                // a toolkit method generator writes the whole function from its signature: the function (its docs and
                // attributes, which the spec carries into the call) becomes the call
                let lines: Vec<String> = t.lines().map(str::to_string).collect();
                let lead = crate::gate::leading(&lines, sp.start.line - 1);
                let indent = indent_of(t, lead.decl + 1);
                let call = change::render_fn(f, &indent).map_err(refused)?;
                let (a, b) = (line_start(t, sp.start.line), offset(t, sp.end));
                t.replace_range(a..b, call.trim_end());
                p.generated.push(call.clone());
                p.facts.push(text_fact(&land.file, &call));
                let _ = old;
            } else {
            let open = sp.body_open.ok_or_else(|| refused(format!("{item}: no body")))?;
            // the body from its opening brace to the item's end (its closing brace); the signature, docs and
            // attributes above it stay as they are
            let lines: Vec<String> = t.lines().map(str::to_string).collect();
            let lead = crate::gate::leading(&lines, sp.start.line - 1);
            let body = change::render_body(f, &indent_of(t, lead.decl + 1)).map_err(refused)?;
            let (a, b) = (offset(t, open), offset(t, sp.end));
            if !t[a..b].ends_with('}') {
                return Err(refused(format!("{item}: its body does not end at the item's end")));
            }
            t.replace_range(a..b, &body);
            let first = body.lines().nth(1).unwrap_or("").trim().to_string();
            p.generated.push(body);
            p.facts.push(format!("args {item} {}", old.args.len()));
            if !first.is_empty() {
                p.facts.push(format!("text {} {first}", land.file));
            }
            }
        }
        Inline => {
            let blockers = change::inline_blockers(k, &item);
            if !blockers.is_empty() {
                return Err(refused(blockers.join("; ")));
            }
            let f = custodian_kit::route::find_fn(&k.functions, &item).map_err(|e| refused(e.to_string()))?;
            let name = ty_name(&item).to_string();
            // its parameters and body, as they are written
            let decl = canon(&f.location.path);
            let dtext = p.tree.text(&decl)?.clone();
            let parsed = parse(&dtext, &decl)?;
            let mut fv = FnText { name: name.clone(), line: f.location.line.unwrap_or(1) as usize, found: None };
            fv.visit_file(&parsed);
            let (params, body_span, item_span) = fv.found.ok_or_else(|| refused(format!("{item}: no body at {}:{}", land.file, f.location.line.unwrap_or(0))))?;
            let params: Vec<(String, String)> = params.iter().map(|(a, b, c, d)| (dtext[offset(&dtext, *a)..offset(&dtext, *b)].to_string(), dtext[offset(&dtext, *c)..offset(&dtext, *d)].to_string())).collect();
            let body = dtext[offset(&dtext, body_span.0)..offset(&dtext, body_span.1)].to_string();
            let mut splices: BTreeMap<PathBuf, Vec<Splice>> = BTreeMap::new();
            let mut by_file: BTreeMap<PathBuf, Vec<(u32, Option<u32>)>> = BTreeMap::new();
            for c in &k.calls {
                if matches!(&c.resolution, Some(klepto::Resolution::Local(id) | klepto::Resolution::External(id)) if *id == item)
                    && let Some(l) = c.location.line {
                        by_file.entry(canon(&c.location.path)).or_default().push((l, c.location.column));
                    }
            }
            let mut calls = 0;
            for (cf, places) in by_file {
                let t = p.tree.text(&cf)?.clone();
                // each call: from the callee's path to its closing parenthesis, its arguments as written
                let mut at: Vec<(usize, usize, Vec<String>)> = Vec::new();
                for (l, c) in places {
                    let (open, close) = call_in_text(&t, l, c, &name).ok_or_else(|| refused(format!("{}:{l}: klepto names a call of {name} there and none is in the text", relative(root, &cf).map(|r| r.display().to_string()).unwrap_or_default())))?;
                    // the call starts at its path: back from the name over `seg::` and whitespace
                    let name_at = t[..open].trim_end().len() - name.len();
                    let mut start = name_at;
                    loop {
                        let before = t[..start].trim_end();
                        if let Some(b) = before.strip_suffix("::") {
                            let b = b.trim_end();
                            let seg = b.len() - b.chars().rev().take_while(|c| c.is_alphanumeric() || *c == '_').map(char::len_utf8).sum::<usize>();
                            start = seg;
                        } else {
                            break;
                        }
                    }
                    at.push((start, close + 1, arguments(&t, open, close)));
                }
                at.sort();
                at.dedup();
                if at.windows(2).any(|w| w[1].0 < w[0].1) {
                    return Err(refused(format!("{}: a call of {name} sits inside another's arguments; inline it in two specs", relative(root, &cf).map(|r| r.display().to_string()).unwrap_or_default())));
                }
                for (a, b, args) in at {
                    let mut block = change::render_inlined(&params, &args, &body).map_err(|e| refused(format!("{}: {e}", relative(root, &cf).map(|r| r.display().to_string()).unwrap_or_default())))?;
                    // a block where a statement starts, with more of the expression after it, would end there: in
                    // parentheses it stays the expression's first operand
                    let before = t[..a].trim_end();
                    let after = t[b..].trim_start();
                    let starts_statement = before.is_empty() || before.ends_with(['{', ';', '}']) || before.ends_with("=>");
                    if starts_statement && !after.starts_with([';', '}', ',', ')']) {
                        block = format!("({block})");
                    }
                    p.generated.push(block.clone());
                    splices.entry(cf.clone()).or_default().push((a, b, block));
                    calls += 1;
                }
            }
            // the function goes when nothing outside the crate can name it and every use was a call
            if !f.is_public {
                let lines: Vec<String> = dtext.lines().map(str::to_string).collect();
                let lead = crate::gate::leading(&lines, item_span.0.line - 1);
                let first = lead.attrs.first().map(|(a, _)| *a).unwrap_or(lead.decl).min(item_span.0.line - 1);
                // its doc lines above it go with it
                let mut from = first;
                while from > 0 && lines[from - 1].trim_start().starts_with("///") {
                    from -= 1;
                }
                let (a, mut b) = (line_start(&dtext, from + 1), line_end(&dtext, item_span.1.line));
                // and one blank line after it, so no two blank lines are left
                if dtext[b..].starts_with('\n') && (a == 0 || dtext[..a].ends_with("\n\n")) {
                    b += 1;
                }
                splices.entry(decl.clone()).or_default().push((a, b, String::new()));
                p.facts.push(format!("absent {item}"));
                p.also.push(format!("{item} removed: not pub, and its {calls} calls are inlined"));
            } else {
                p.also.push(format!("{item} kept: it is pub, and other crates may name it"));
            }
            for (f, e) in splices {
                splice(p.tree.text(&f)?, e);
            }
            p.facts.push(format!("calls {item} 0"));
        }
        Wrap => {
            let f = spec.function.as_ref().ok_or_else(|| refused("wrap needs function"))?;
            custodian_kit::route::find_fn(&k.functions, &item).map_err(|e| refused(e.to_string()))?;
            if f.name != ty_name(&item) {
                return Err(refused(format!("{item}: the spec's function is {}, not this one", f.name)));
            }
            let t = p.tree.text(&file)?;
            let sp = item_at(t, &file, line_of(&land.at))?;
            let open = sp.body_open.ok_or_else(|| refused(format!("{item}: no body")))?;
            let (a, b) = (offset(t, open), offset(t, sp.end));
            if !t[a..b].ends_with('}') {
                return Err(refused(format!("{item}: its body does not end at the item's end")));
            }
            let lines: Vec<String> = t.lines().map(str::to_string).collect();
            let lead = crate::gate::leading(&lines, sp.start.line - 1);
            let indent = indent_of(t, lead.decl + 1);
            // the old body moves one level in, unless a string in it runs over lines (its text would change)
            let old = t[a..b].to_string();
            let old = if spans_lines_in_a_string(&old) { old } else { old.lines().enumerate().map(|(i, l)| if i == 0 || l.trim().is_empty() { l.to_string() } else { format!("    {l}") }).collect::<Vec<_>>().join("\n") };
            let body = change::render_wrapped(f, &indent, &old).map_err(refused)?;
            t.replace_range(a..b, &body);
            let first = body.lines().nth(1).unwrap_or("").trim().to_string();
            p.generated.push(body);
            if !first.is_empty() {
                p.facts.push(format!("text {} {first}", land.file));
            }
            p.facts.push(format!("exists {item}"));
        }
        ChangeDependency => {
            for c in change::dependency_changes(k, root, spec).map_err(refused)? {
                let path = abs(root, &c.manifest);
                for d in spec.dependencies_named().into_iter().filter(|d| d.name.replace('-', "_") == c.name) {
                    let (after, n) = crate::commands::edit::dep_change_text_with(p.tree.text(&path)?, &d.name, d.version.as_deref(), d.path.as_deref(), &d.features, d.optional)?;
                    if n == 0 {
                        return Err(refused(format!("{}: names no {} to change", c.manifest, d.name)));
                    }
                    *p.tree.text(&path)? = after;
                    // the manifest as the root names it, so the fact reads the scratch copy's
                    let manifest = relative(root, &path).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_else(|_| c.manifest.clone());
                    if d.version.is_some() || d.path.is_some() {
                        p.facts.push(format!("source {manifest} {} {} {}", d.name, d.version.as_deref().unwrap_or("-"), d.path.as_deref().unwrap_or("-")));
                    }
                    p.facts.push(format!("dependency {} {}", c.package, c.name));
                    if let Some(o) = d.optional {
                        p.facts.push(format!("optional {} {} {o}", c.package, c.name));
                    }
                }
            }
        }
        RemoveDependency => {
            let dep = spec.dependency.as_ref().map(|d| d.name.clone()).ok_or_else(|| refused("remove_dependency needs dependency"))?;
            let path = abs(root, &land.file);
            let crate_dir = path.parent().map(Path::to_path_buf).unwrap_or_else(|| root.to_path_buf());
            // the use-site check: nothing in the crate's sources still names the dependency's paths
            let left = crate_names(&crate_dir, &dep.replace('-', "_"));
            if !left.is_empty() {
                return Err(refused(format!("{} still names {dep}: {}", land.package, left.iter().map(|(f, l)| format!("{}:{l}", relative(root, f).unwrap_or_else(|_| f.clone()).display())).collect::<Vec<_>>().join(", "))));
            }
            let (after, n) = crate::commands::edit::dep_remove_text(p.tree.text(&path)?, &dep)?;
            if n == 0 {
                return Err(refused(format!("{}: names no {dep} to remove", land.file)));
            }
            *p.tree.text(&path)? = after;
            p.also.push(format!("{dep} dropped from {} ({n} line(s))", land.file));
        }
        RenamePaths => {
            let old = spec.dependency.as_ref().map(|d| d.name.replace('-', "_")).ok_or_else(|| refused("rename_paths needs dependency (the crate the paths name now)"))?;
            let new = spec.new_name.clone().map(|n| n.replace('-', "_")).ok_or_else(|| refused("rename_paths needs new_name (the crate they name after)"))?;
            let crate_dir = abs(root, &land.file).parent().map(Path::to_path_buf).unwrap_or_else(|| root.to_path_buf());
            let mut changed = 0;
            for (file, _) in crate_names(&crate_dir, &old).into_iter().map(|(f, _)| (f, ())).collect::<BTreeMap<_, _>>() {
                let (after, n) = rename_crate_paths(&p.tree.text(&file)?.clone(), &old, &new);
                if n > 0 {
                    *p.tree.text(&file)? = after;
                    changed += n;
                }
            }
            if changed == 0 {
                return Err(refused(format!("{}: no source file names {old}", land.package)));
            }
            p.also.push(format!("{changed} path(s) through {old} now through {new} in {}", land.package));
        }
        DeclareFeature => {
            let d = change::feature_declaration(k, root, spec).map_err(refused)?;
            if !d.problems.is_empty() {
                return Err(refused(d.problems.join("; ")));
            }
            let path = abs(root, &land.file);
            let after = crate::commands::edit::feature_declare_text(p.tree.text(&path)?, &d.feature, &d.enables, &d.made_optional)?;
            *p.tree.text(&path)? = after;
            p.facts.push(format!("feature {} {} {}", d.package, d.feature, d.enables.join(",")));
            for n in &d.made_optional {
                p.facts.push(format!("optional {} {n} true", d.package));
            }
        }
        ChangeSignature => {
            let f = spec.function.as_ref().ok_or_else(|| refused("change_signature needs function"))?;
            let old = custodian_kit::route::find_fn(&k.functions, &item).map_err(|e| refused(e.to_string()))?;
            let mut splices: BTreeMap<PathBuf, Vec<Splice>> = BTreeMap::new();
            let t = p.tree.text(&file)?.clone();
            let sp = item_at(&t, &file, line_of(&land.at))?;
            let open = sp.body_open.ok_or_else(|| refused(format!("{item}: no body")))?;
            // the signature: from the `fn` line (after docs and attributes) to the body's brace
            let lines: Vec<String> = t.lines().map(str::to_string).collect();
            let lead = crate::gate::leading(&lines, sp.start.line - 1);
            let (a, b) = (line_start(&t, lead.decl + 1) + indent_of(&t, lead.decl + 1).len(), offset(&t, open));
            let mut sig = f.clone();
            sig.vis = sig.vis.or_else(|| old.is_public.then(|| "pub".to_string()));
            let new_sig = change::signature(&sig);
            splices.entry(canon(&file)).or_default().push((a, b, format!("{new_sig} ")));
            p.generated.push(new_sig.clone());
            // with a body: written in the same step, so a body that compiles only against the new signature (a
            // `main` that now returns `Result`) is proved with it
            if !f.body.is_empty() {
                let body = change::render_body(f, &indent_of(&t, lead.decl + 1)).map_err(refused)?;
                let end = offset(&t, sp.end);
                if !t[offset(&t, open)..end].ends_with('}') {
                    return Err(refused(format!("{item}: its body does not end at the item's end")));
                }
                splices.entry(canon(&file)).or_default().push((offset(&t, open), end, body.clone()));
                if let Some(first) = body.lines().nth(1).map(str::trim).filter(|l| !l.is_empty()) {
                    p.facts.push(format!("text {} {first}", land.file));
                }
                p.generated.push(body);
            }
            // parameters, the receiver aside: klepto's args hold `self` too
            let added = f.params.len() > old.args.iter().filter(|a| !change::is_receiver(a)).count();
            let name = f.name.clone();
            // its calls: the new argument at the end, when the spec gives it
            if added {
                // every call klepto resolves to it, wherever it is (a binary's too, and a method's by its receiver);
                // one inside a macro's tokens, where syn does not look, found in the text from klepto's place
                let old_name = ty_name(&item).to_string();
                let mut by_file: BTreeMap<PathBuf, Vec<(u32, Option<u32>)>> = BTreeMap::new();
                for c in &k.calls {
                    if matches!(&c.resolution, Some(klepto::Resolution::Local(id) | klepto::Resolution::External(id)) if *id == item)
                        && let Some(l) = c.location.line {
                            by_file.entry(canon(&c.location.path)).or_default().push((l, c.location.column));
                        }
                }
                // the calls inside a macro's arguments whose path resolves to it (custodian_kit's rule, the one the
                // preflight counts by): klepto names their paths and resolves none
                let mut in_macros: BTreeMap<PathBuf, Vec<change::MacroCall>> = BTreeMap::new();
                for m in change::macro_calls(k, &item) {
                    by_file.entry(canon(&m.file)).or_default();
                    in_macros.entry(canon(&m.file)).or_default().push(m);
                }
                for (cf, places) in by_file {
                    let t = p.tree.text(&cf)?.clone();
                    let parsed = parse(&t, &cf)?;
                    let mut v = Calls::new(places.iter().flat_map(|(l, c)| both(*l, *c)).collect(), &old_name);
                    v.visit_file(&parsed);
                    let mut closes: Vec<(usize, bool)> = v.found.iter().map(|(close, has)| (offset(&t, *close), *has)).collect();
                    for (l, c) in places.iter().filter(|(l, c)| !both(*l, *c).iter().any(|k| v.met.contains(k))) {
                        match call_in_text(&t, *l, *c, &old_name) {
                            Some((open, close)) => closes.push((close, !t[open + 1..close].trim().is_empty())),
                            None => p.listed.push(format!("call {}:{l}: klepto names it, and no call of {old_name} is there to give the new argument", relative(root, &cf).map(|r| r.display().to_string()).unwrap_or_default())),
                        }
                    }
                    for m in in_macros.get(&cf).into_iter().flatten() {
                        let found = calls_in_macro(&t, m.line, m.column, &m.paths);
                        if found.is_empty() {
                            p.listed.push(format!("call in {}! at {}:{}: its tokens name {}, and no call of it is there to give the new argument", m.macro_path, relative(root, &cf).map(|r| r.display().to_string()).unwrap_or_default(), m.line, m.paths.join(", ")));
                        }
                        closes.extend(found.into_iter().map(|(open, close)| (close, !t[open + 1..close].trim().is_empty())));
                    }
                    closes.sort();
                    closes.dedup();
                    for (at, has_args) in closes {
                        match spec.call_sites.as_ref().and_then(|c| c.pass.clone()) {
                            Some(pass) => {
                                splices.entry(cf.clone()).or_default().push((at, at, if has_args { format!(", {pass}") } else { pass.clone() }));
                            }
                            None => p.listed.push(format!("call {}:{}: needs the new argument", relative(root, &cf).map(|r| r.display().to_string()).unwrap_or_default(), t[..at].matches('\n').count() + 1)),
                        }
                    }
                }
            }
            for (f, e) in splices {
                splice(p.tree.text(&f)?, e);
            }
            let new_id = format!("{}::{name}", module_path_of(&item));
            p.facts.push(format!("args {new_id} {}", f.params.len() + usize::from(f.receiver.is_some() || old.args.iter().any(|a| change::is_receiver(a)))));
            if name != ty_name(&item) {
                p.facts.push(format!("absent {item}"));
            }
        }
        Rename => {
            let new = spec.new_name.clone().unwrap_or_default();
            let old = ty_name(&item).to_string();
            // the declaration and every occurrence klepto resolves to the item, at its own place
            let mut at: BTreeMap<PathBuf, BTreeSet<usize>> = BTreeMap::new();
            let d = change::item_named(k, &item).ok_or_else(|| refused(format!("{item}: no such item")))?;
            let mut push = |path: &Path, line: Option<u32>| {
                if let Some(l) = line {
                    at.entry(path.to_path_buf()).or_default().insert(l as usize);
                }
            };
            let t0 = std::fs::read_to_string(&d.location.path)?;
            let sp = item_at(&t0, &d.location.path, d.location.line.unwrap_or(1))?;
            let lines0: Vec<String> = t0.lines().map(str::to_string).collect();
            let decl = crate::gate::leading(&lines0, sp.start.line - 1).decl + 1;
            push(&d.location.path, Some(decl as u32));
            let resolves = |r: &Option<klepto::Resolution>| matches!(r, Some(klepto::Resolution::Local(id) | klepto::Resolution::External(id)) if *id == item || id.starts_with(&format!("{item}::")));
            for o in &k.paths {
                if resolves(&o.resolution) {
                    push(&o.location.path, o.location.line);
                }
            }
            for c in &k.calls {
                if resolves(&c.resolution) {
                    push(&c.location.path, c.location.line);
                }
            }
            for i in &k.imports {
                if resolves(&i.resolution) {
                    push(&i.location.path, i.location.line);
                }
            }
            let in_macros = k.macros_inv.iter().filter(|m| m.token_paths.iter().any(|p| p.rsplit("::").next() == Some(old.as_str()))).count();
            if in_macros > 0 {
                p.listed.push(format!("{in_macros} macro invocations name {old} inside their tokens: klepto resolves no paths there; not renamed"));
            }
            for (f, lines) in at {
                let t = p.tree.text(&f)?;
                let mut out = String::new();
                for (i, l) in t.split_inclusive('\n').enumerate() {
                    if lines.contains(&(i + 1)) {
                        out.push_str(&rename_word(l, &old, &new));
                    } else {
                        out.push_str(l);
                    }
                }
                *t = out;
            }
            p.facts.push(format!("exists {}::{new}", module_path_of(&item)));
            p.facts.push(format!("absent {item}"));
        }
        Move => {
            let to = spec.to.clone().unwrap_or_default();
            let (_, dest) = change::route_file(k, root, &to).ok_or_else(|| refused(format!("{to}: not a crate:<name>/<file>.rs route")))?;
            let dest_module = change::module_of_file(k, &dest).map(|m| m.path.clone()).ok_or_else(|| refused(format!("{}: no module of the scan", dest.display())))?;
            let d = change::item_named(k, &item).ok_or_else(|| refused(format!("{item}: no such item")))?;
            let t = p.tree.text(&file)?;
            let sp = item_at(t, &file, d.location.line.unwrap_or(1))?;
            let (a, b) = (line_start(t, sp.start.line), line_end(t, sp.end.line));
            let moved = t[a..b].to_string();
            let source_text = t.clone();
            let vis = if d.is_public { "pub " } else { "pub(crate) " };
            let target_path = format!("crate::{}", dest_module.split_once("::").map(|(_, r)| r).unwrap_or(""));
            t.replace_range(a..b, &format!("{vis}use {}::{};\n", target_path.trim_end_matches("::"), d.name));
            // the destination's own imports of the item go: the item is declared there now
            let dest_c = canon(&dest);
            let import_lines: BTreeSet<usize> = k
                .imports
                .iter()
                .filter(|i| canon(&i.location.path) == dest_c && matches!(&i.resolution, Some(klepto::Resolution::Local(id) | klepto::Resolution::External(id)) if *id == item))
                .filter_map(|i| i.location.line.map(|l| l as usize))
                .collect();
            if !import_lines.is_empty() {
                let dt = p.tree.text(&dest)?;
                let dropped = drop_imports(dt, &dest, &import_lines, &d.name)?;
                for l in dropped {
                    p.also.push(format!("import {}:{l}: {} is declared there now, its import dropped", to, d.name));
                }
            }
            let dt = p.tree.text(&dest)?;
            // the moved item keeps resolving the names of the module it left
            let old_module = format!("crate::{}", d.module.split_once("::").map(|(_, r)| r).unwrap_or("")).trim_end_matches("::").to_string();
            let glob = format!("use {old_module}::*;");
            // and the names the module it left imports privately (a glob does not carry those) that the item uses
            let declared: BTreeSet<String> = k.modules.get(&dest_module).map(|m| m.items.keys().cloned().collect()).unwrap_or_default();
            let carried = imports_carried(&source_text, &file, &d.module, &moved, dt, &dest, &dest_module, &declared)?;
            let mut head: Vec<String> = carried.clone();
            if !dt.lines().any(|l| l.trim() == glob) {
                head.insert(0, glob.clone());
            }
            if !head.is_empty() {
                let parsed = parse(dt, &dest)?;
                let at = parsed.items.first().map(|i| line_start(dt, i.span().start().line)).unwrap_or(dt.len());
                dt.insert_str(at, &format!("{}\n\n", head.join("\n")));
            }
            for c in &carried {
                p.also.push(format!("{to} gets `{c}`: {} uses the name, imported where it was", d.name));
            }
            if !dt.ends_with('\n') {
                dt.push('\n');
            }
            dt.push('\n');
            dt.push_str(&moved);
            p.generated.push(moved);
            p.facts.push(format!("exists {dest_module}::{}", d.name));
        }
    }
    // a body written with toolkit pieces needs the toolkit in its crate
    if change::uses_toolkit(spec) {
        let has = k.features.get(&land.package).is_some_and(|m| m.dependencies.iter().any(|d| d.name == change::TOOLKIT.0));
        if !has {
            let e = crate::commands::edit::dep_add_plan(root, &land.package, &format!("{}@{}", change::TOOLKIT.0, change::TOOLKIT.1), None, &[], false)?;
            for f in e.edits.files {
                let before = p.tree.files.get(&f.path).map(|(b, _)| b.clone()).unwrap_or(Some(f.before.clone()));
                p.tree.files.insert(f.path.clone(), (before, f.after));
            }
            p.listed.push(format!("{} gets the dependency {} = \"{}\" (its body is toolkit pieces)", land.package, change::TOOLKIT.0, change::TOOLKIT.1));
        }
    }
    for c in &spec.postconditions {
        p.facts.push(c.clone());
    }
    Ok(p)
}

/// The fact a placed function shows: its method in the scan, or (made by a toolkit generator, which the scan may
/// not expand) its first line in the file.
fn fn_fact(f: &change::FnSpec, item: &str, file: &str, generated: Option<&String>) -> String {
    match (&f.generator, generated) {
        (Some(_), Some(text)) => text_fact(file, text),
        _ => format!("method {item} {}", f.name),
    }
}

/// `text <file> <the first line of what was placed>`.
fn text_fact(file: &str, text: &str) -> String {
    let first = text.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with("///") && !l.starts_with("#[")).unwrap_or("");
    format!("text {file} {first}")
}

/// `old` replaced by `new` where it is a whole word on the line.
fn rename_word(line: &str, old: &str, new: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(i) = rest.find(old) {
        let before = rest[..i].chars().last();
        let after = rest[i + old.len()..].chars().next();
        let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
        out.push_str(&rest[..i]);
        if !word(before) && !word(after) {
            out.push_str(new);
        } else {
            out.push_str(old);
        }
        rest = &rest[i + old.len()..];
    }
    out.push_str(rest);
    out
}

/// A function's text by its name and line: each parameter's pattern and type (their spans) and its body's span
/// (`{` to `}`), and the item's own span.
#[allow(clippy::type_complexity)]
struct FnText {
    name: String,
    line: usize,
    found: Option<(Vec<(LineColumn, LineColumn, LineColumn, LineColumn)>, (LineColumn, LineColumn), (LineColumn, LineColumn))>,
}

impl FnText {
    fn take(&mut self, sig: &syn::Signature, block: &syn::Block, span: proc_macro2::Span) {
        if sig.ident != self.name.as_str() || !(span.start().line..=span.end().line).contains(&self.line) {
            return;
        }
        let params = sig
            .inputs
            .iter()
            .filter_map(|a| match a {
                syn::FnArg::Typed(t) => Some((t.pat.span().start(), t.pat.span().end(), t.ty.span().start(), t.ty.span().end())),
                syn::FnArg::Receiver(_) => None,
            })
            .collect();
        self.found = Some((params, (block.brace_token.span.open().start(), block.brace_token.span.close().end()), (span.start(), span.end())));
    }
}

impl<'ast> Visit<'ast> for FnText {
    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
        self.take(&f.sig, &f.block, f.span());
        syn::visit::visit_item_fn(self, f);
    }

    fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
        self.take(&f.sig, &f.block, f.span());
        syn::visit::visit_impl_item_fn(self, f);
    }
}

/// Whether a string literal in `block` (a body's text) runs over more than one line.
fn spans_lines_in_a_string(block: &str) -> bool {
    struct Lits(bool);
    impl<'ast> Visit<'ast> for Lits {
        fn visit_lit(&mut self, l: &'ast syn::Lit) {
            if l.span().start().line != l.span().end().line {
                self.0 = true;
            }
        }
        fn visit_macro(&mut self, m: &'ast syn::Macro) {
            if m.tokens.clone().into_iter().any(|t| t.span().start().line != t.span().end().line) {
                self.0 = true;
            }
        }
    }
    match syn::parse_str::<syn::Block>(block) {
        Ok(b) => {
            let mut v = Lits(false);
            v.visit_block(&b);
            v.0
        }
        Err(_) => true,
    }
}

/// A `use` tree's leaves: each name's path from the tree's root and the name it binds (`None` for a glob).
fn use_leaves(t: &syn::UseTree, prefix: &mut Vec<String>, out: &mut Vec<(Vec<String>, Option<String>)>) {
    match t {
        syn::UseTree::Path(p) => {
            prefix.push(p.ident.to_string());
            use_leaves(&p.tree, prefix, out);
            prefix.pop();
        }
        syn::UseTree::Name(n) if n.ident == "self" => out.push((prefix.clone(), prefix.last().cloned())),
        syn::UseTree::Name(n) => out.push(([prefix.clone(), vec![n.ident.to_string()]].concat(), Some(n.ident.to_string()))),
        syn::UseTree::Rename(r) => {
            let path = if r.ident == "self" { prefix.clone() } else { [prefix.clone(), vec![r.ident.to_string()]].concat() };
            out.push((path, Some(r.rename.to_string())))
        }
        syn::UseTree::Glob(_) => out.push((prefix.clone(), None)),
        syn::UseTree::Group(g) => g.items.iter().for_each(|i| use_leaves(i, prefix, out)),
    }
}

/// The names a moved item takes with it: each private `use` of the module it left (`module`, klepto's path, its
/// first segment the crate) that binds a name the item's text uses, and that the destination neither declares nor
/// imports, as a `use` line written from the crate's root.
#[allow(clippy::too_many_arguments)]
fn imports_carried(source: &str, source_path: &Path, module: &str, moved: &str, dest: &str, dest_path: &Path, dest_module: &str, declared: &BTreeSet<String>) -> Result<Vec<String>> {
    let src = parse(source, source_path)?;
    let dst = parse(dest, dest_path)?;
    // what the destination declares (klepto's, which sees what its macros make) and imports
    let mut taken: BTreeSet<String> = declared.clone();
    let dest_abs: Vec<String> = std::iter::once("crate".to_string()).chain(dest_module.split("::").skip(1).map(str::to_string)).collect();
    for i in &dst.items {
        let declared = match i {
            syn::Item::Use(u) => {
                let mut leaves = Vec::new();
                use_leaves(&u.tree, &mut Vec::new(), &mut leaves);
                taken.extend(leaves.into_iter().filter_map(|(_, n)| n));
                None
            }
            syn::Item::Struct(x) => Some(&x.ident),
            syn::Item::Enum(x) => Some(&x.ident),
            syn::Item::Fn(x) => Some(&x.sig.ident),
            syn::Item::Trait(x) => Some(&x.ident),
            syn::Item::Type(x) => Some(&x.ident),
            syn::Item::Const(x) => Some(&x.ident),
            syn::Item::Static(x) => Some(&x.ident),
            syn::Item::Mod(x) => Some(&x.ident),
            _ => None,
        };
        taken.extend(declared.map(|i| i.to_string()));
    }
    let here: Vec<String> = module.split("::").skip(1).map(str::to_string).collect();
    let word = |name: &str| {
        let mut rest = moved;
        while let Some(i) = rest.find(name) {
            let w = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
            if !w(rest[..i].chars().last()) && !w(rest[i + name.len()..].chars().next()) {
                return true;
            }
            rest = &rest[i + name.len()..];
        }
        false
    };
    let mut out = Vec::new();
    for i in &src.items {
        let syn::Item::Use(u) = i else { continue };
        if !matches!(u.vis, syn::Visibility::Inherited) {
            continue;
        }
        let mut leaves = Vec::new();
        use_leaves(&u.tree, &mut Vec::new(), &mut leaves);
        for (path, name) in leaves {
            let Some(name) = name else { continue };
            if taken.contains(&name) || !word(&name) {
                continue;
            }
            // from the crate's root: `self::` and `super::` read from the module it was in
            let abs: Vec<String> = match path.first().map(String::as_str) {
                Some("self") => [vec!["crate".to_string()], here.clone(), path[1..].to_vec()].concat(),
                Some("super") => {
                    let ups = path.iter().take_while(|s| *s == "super").count();
                    [vec!["crate".to_string()], here[..here.len().saturating_sub(ups)].to_vec(), path[ups..].to_vec()].concat()
                }
                _ => path.clone(),
            };
            // a name of the destination itself needs no import there
            if abs.is_empty() || abs[..abs.len() - 1] == dest_abs[..] {
                continue;
            }
            let last = abs.last().cloned().unwrap_or_default();
            let line = if last == name { format!("use {};", abs.join("::")) } else { format!("use {} as {name};", abs.join("::")) };
            taken.insert(name);
            out.push(line);
        }
    }
    Ok(out)
}

/// A `use` tree with the leaves that import `name` taken out; `None` when nothing is left.
fn use_without(t: &syn::UseTree, name: &str) -> Option<String> {
    match t {
        syn::UseTree::Path(p) => use_without(&p.tree, name).map(|rest| format!("{}::{rest}", p.ident)),
        syn::UseTree::Name(n) => (n.ident != name).then(|| n.ident.to_string()),
        syn::UseTree::Rename(r) => Some(format!("{} as {}", r.ident, r.rename)),
        syn::UseTree::Glob(_) => Some("*".into()),
        syn::UseTree::Group(g) => {
            let kept: Vec<String> = g.items.iter().filter_map(|i| use_without(i, name)).collect();
            (!kept.is_empty()).then(|| format!("{{{}}}", kept.join(", ")))
        }
    }
}

/// The `use` items at `lines` of a file lose their import of `name`; a `use` left importing nothing is removed. The
/// lines whose imports were dropped.
fn drop_imports(text: &mut String, path: &Path, lines: &BTreeSet<usize>, name: &str) -> Result<Vec<usize>> {
    let parsed = parse(text, path)?;
    let mut edits: Vec<Splice> = Vec::new();
    let mut dropped = Vec::new();
    for i in &parsed.items {
        let syn::Item::Use(u) = i else { continue };
        let (a, b) = (u.span().start().line, u.span().end().line);
        if !lines.iter().any(|l| (a..=b).contains(l)) {
            continue;
        }
        match use_without(&u.tree, name) {
            Some(kept) => {
                let (x, y) = (offset(text, u.tree.span().start()), offset(text, u.tree.span().end()));
                if text[x..y].replace(char::is_whitespace, "") != kept.replace(char::is_whitespace, "") {
                    edits.push((x, y, kept));
                    dropped.push(a);
                }
            }
            None => {
                edits.push((line_start(text, i.span().start().line), line_end(text, b), String::new()));
                dropped.push(a);
            }
        }
    }
    splice(text, edits);
    Ok(dropped)
}

/// Whether a fact holds in a scan.
fn holds(k: &klepto::Klepto, root: &Path, fact: &str) -> bool {
    let w: Vec<&str> = fact.splitn(3, ' ').collect();
    let item = |id: &str| k.items.iter().any(|d| d.id == id);
    match w.as_slice() {
        ["exists", id] | ["resolves", id] => item(id) || k.graph.resolve_id(id).is_some(),
        ["absent", id] => !item(id),
        ["field", t, name] => k.type_decl(t).is_some_and(|d| d.fields.iter().any(|f| f.name.as_deref() == Some(*name))),
        ["variant", t, name] => k.type_decl(t).is_some_and(|d| d.variants.iter().any(|v| v.name == *name)),
        ["method", t, name] => k.impls_of(t).iter().flat_map(|i| i.methods.iter()).any(|m| m.rsplit("::").next() == Some(*name)) || item(&format!("{t}::{name}")),
        ["impl", t, tr] => k.impls_of(t).iter().any(|i| i.trait_.as_ref().is_some_and(|x| x.raw.rsplit("::").next().is_some_and(|n| tr.contains(n) || n.contains(tr.trim_start_matches("impl_")))) ),
        ["derive", t, d] => k.type_decl(t).is_some_and(|x| x.derives.iter().any(|r| matches!(r, klepto::Resolution::Local(p) | klepto::Resolution::External(p) if p.rsplit("::").next() == Some(d.rsplit("::").next().unwrap_or(d))) || matches!(r, klepto::Resolution::Unresolved { raw, .. } if raw.ends_with(*d)))),
        ["module", m] => k.modules.get(m).is_some(),
        // `library <package>`: cargo builds a library for it
        ["library", p] => k.packages.get(&p.replace('-', "_")).is_some_and(|x| x.targets.iter().any(|t| t.kind == "lib")),
        // a dependency's name as cargo reads it: `big-bad-bool` and `big_bad_bool` are one name
        ["dependency", p, d] => k.features.get(&p.replace('-', "_")).or_else(|| k.features.get(*p)).is_some_and(|m| m.dependencies.iter().any(|x| x.name.replace('-', "_") == d.replace('-', "_"))),
        // `feature <package> <name> <enables,...>`: the crate declares it with that list
        ["feature", p, rest] => {
            let (name, list) = rest.split_once(' ').unwrap_or((rest, ""));
            let want: Vec<String> = list.split(',').filter(|x| !x.is_empty()).map(|x| x.replace('-', "_")).collect();
            k.features.get(&p.replace('-', "_")).and_then(|m| m.features.get(name)).is_some_and(|i| i.enables.iter().map(|e| enable_text(e).replace('-', "_")).collect::<Vec<_>>() == want)
        }
        // `optional <package> <dependency> <true|false>`
        ["optional", p, rest] => {
            let (d, o) = rest.split_once(' ').unwrap_or((rest, "true"));
            k.features.get(&p.replace('-', "_")).is_some_and(|m| m.dependencies.iter().any(|x| x.name.replace('-', "_") == d.replace('-', "_") && x.optional.to_string() == o))
        }
        // `calls <id> <n>`: how many calls klepto resolves to it
        ["calls", id, n] => k.calls.iter().filter(|c| matches!(&c.resolution, Some(klepto::Resolution::Local(x) | klepto::Resolution::External(x)) if x == id)).count().to_string() == *n,
        ["args", id, n] => k.functions.iter().find(|f| f.fq_name == *id).is_some_and(|f| f.args.len().to_string() == *n),
        ["text", f, line] => std::fs::read_to_string(root.join(f)).is_ok_and(|t| t.lines().any(|l| l.trim() == line.trim())),
        // `source <manifest> <dependency> <version|-> <path|->`: the manifest names it from there
        ["source", f, rest] => {
            let w: Vec<&str> = rest.split(' ').collect();
            let want = |x: &str| (x != "-").then(|| x.to_string());
            match w.as_slice() {
                [name, version, path] => std::fs::read_to_string(root.join(f)).ok().and_then(|t| crate::commands::edit::dep_source(&t, name)) == Some((want(version), want(path))),
                _ => false,
            }
        }
        _ => false,
    }
}

fn enable_text(e: &klepto::FeatureEnable) -> String {
    match e {
        klepto::FeatureEnable::Feature(f) => f.clone(),
        klepto::FeatureEnable::Dep(d) => format!("dep:{d}"),
        klepto::FeatureEnable::DepFeature { dep, feature, weak } => format!("{dep}{}/{feature}", if *weak { "?" } else { "" }),
    }
}

fn hex(bytes: &[u8]) -> String {
    stable_digest::StableDigest::sha256(bytes).as_str().to_string()
}

/// The lines written, as 1-based inclusive ranges of the file after.
fn written_lines(before: &str, after: &str) -> Vec<(u32, u32)> {
    let d = similar::TextDiff::from_lines(before, after);
    let mut out = Vec::new();
    for op in d.ops() {
        let r = op.new_range();
        if !matches!(op.tag(), similar::DiffTag::Equal) && !r.is_empty() {
            out.push((r.start as u32 + 1, r.end as u32));
        }
    }
    out
}

/// A record's path: the one asked, else the project's `.custodian/data/transforms/<spec>/<stamp>.json`.
fn record_path(root: &Path, opts: &Options, spec: &ChangeSpec) -> Option<PathBuf> {
    if let Some(o) = &opts.out {
        return Some(o.clone());
    }
    let store = custodian_kit::store::Store::find(root)?;
    store.path(&custodian_kit::store::named::TRANSFORM, &[("name", &spec.name), ("stamp", &custodian_kit::runs::stamp())]).ok()
}

/// `transform`: the spec planned, proved and (with `write`) written and recorded.
pub fn run(root: &Path, opts: &Options) -> Result<Outcome> {
    let root = workspace_root(root)?;
    let spec_path = if opts.spec.is_absolute() { opts.spec.clone() } else { root.join(&opts.spec) };
    let spec = ChangeSpec::from_path(&spec_path).map_err(refused)?;
    let k = crate::cargo::builder(&root)?.ignore_parse_errors(true).parse().map_err(|e| PassengerError::Scan { detail: e.to_string() })?;
    let land = change::landing(&k, &root, &spec).map_err(refused)?;
    let mut said = vec![format!("{} {}: lands {}", spec.op().as_str(), spec.name, land.line())];
    let planned = plan(&k, &root, &spec, &land)?;
    let mut edits = planned.tree.edits();
    if edits.is_empty() {
        return Err(refused(format!("{}: nothing to write", spec.name)));
    }
    // the proof: cargo over the check matrix of the crate it lands in (its tests too) and each crate that depends on
    // it directly (custodian_kit's proof_packages), on the workspace's proof place: one copy at a path that does not
    // change and one build kept between proofs; then a fresh scan
    let mut checks = Vec::new();
    let mut oracle: crate::oracle::CargoOracle;
    let mut expansion: Option<Expansion> = None;
    let mut compared: Option<crate::proof_dir::Compared> = None;
    let scratch_root: PathBuf;
    let _held;
    if opts.no_cargo {
        let dir = crate::gate::scratch_copy(&root)?;
        edits.write_to(&root, dir.path())?;
        scratch_root = dir.path().to_path_buf();
        _held = Some(dir);
        said.push("cargo not run (--no-cargo)".into());
    } else {
        let landed: Vec<String> = if spec.op() == Operation::ChangeDependency {
            change::dependency_changes(&k, &root, &spec).map_err(refused)?.into_iter().map(|c| c.package).collect::<BTreeSet<_>>().into_iter().collect()
        } else {
            vec![land.package.clone()]
        };
        let packages = proof_packages_of(&k, &spec, &land, &landed);
        said.push(proof_line(&packages));
        let proof = crate::proof_dir::ProofDir::take(&root)?;
        // a swap that keeps what the function compiles to: its expansion before, on the copy as the tree is
        let before = if keeps_expansion(&spec) { Some(proof.expand_fns(&land.package, std::slice::from_ref(&spec.target))?) } else { None };
        oracle = crate::oracle::CargoOracle::with_proof(&root, proof);
        oracle.tests = true;
        for pp in &packages {
            let Some(model) = k.features.get(&pp.package) else { continue };
            // the configurations are the edited manifest's: a feature declared, a dependency made optional or
            // required again, as the change leaves them
            let desired = change::model_after(&k, &root, &spec, &pp.package).unwrap_or_else(|| model.clone());
            for job in klepto::check_matrix(&desired) {
                let r = oracle.check_edits_tests(&job, &edits, pp.tests);
                let error = r.diagnostics.iter().find(|d| d.level == "error").map(|d| d.message.clone());
                checks.push(Checked { package: pp.package.clone(), features: job.features.clone(), no_default: job.no_default, ok: r.ok, error });
                if !r.ok {
                    break;
                }
            }
            if checks.iter().any(|c| !c.ok) {
                break;
            }
        }
        let compiled = !checks.is_empty() && checks.iter().all(|c| c.ok);
        if let (Some((before, line, ms)), true) = (before, compiled) {
            // the edits are on the copy as the last check left them
            let proof = oracle.proof().ok_or_else(|| refused("the proof place was let go"))?;
            let (after, _, ms2) = proof.expand_fns(&land.package, std::slice::from_ref(&spec.target))?;
            let (b, a) = (before.get(&spec.target).cloned().flatten(), after.get(&spec.target).cloned().flatten());
            let same = b.is_some() && a == b;
            said.push(match (&b, &a) {
                (None, _) | (_, None) => format!("cargo expand: {} is not in the expansion of {}'s library", spec.target, land.package),
                _ if same => "cargo expand: the function after is the same tokens as before (in the normal form)".into(),
                _ => "cargo expand: the function after is not the same tokens as before".into(),
            });
            expansion = Some(Expansion { function: spec.target.clone(), package: land.package.clone(), before: b.unwrap_or_default(), after: a.unwrap_or_default(), same, line, millis: ms + ms2 });
        }
        if opts.tests && compiled && expansion.as_ref().is_none_or(|e| e.same) {
            let proof = oracle.proof().ok_or_else(|| refused("the proof place was let go"))?;
            let c = proof.tests_compared(&land.package, &edits)?;
            said.push(tests_line(&land.package, &c));
            compared = Some(c);
        }
        match oracle.scratch_dir() {
            Some(d) => {
                let ws = oracle.workspace().map(Path::to_path_buf).unwrap_or_else(|| root.clone());
                scratch_root = d.join(relative(&ws, &root).unwrap_or_default());
                _held = None;
                // a changed manifest changes the lock file: cargo's, from the proof, is written with the change, so
                // the tree's lock names what its manifests name (a scan reads dependency macros through it, frozen)
                if matches!(spec.op(), Operation::AddDependency | Operation::ChangeDependency | Operation::DeclareFeature)
                    && let Ok(after) = std::fs::read_to_string(d.join("Cargo.lock"))
                {
                    let lock = ws.join("Cargo.lock");
                    let before = std::fs::read_to_string(&lock).unwrap_or_default();
                    if after != before && !edits.files.iter().any(|f| f.path == lock) {
                        edits.files.push(crate::gate::FileEdit { path: lock, before, after });
                    }
                }
            }
            None => {
                // nothing was checked: the postconditions are still read on a copy the edits are written to, never
                // on the tree as it is, and the change is not proved
                let dir = crate::gate::scratch_copy(&root)?;
                edits.write_to(&root, dir.path())?;
                scratch_root = dir.path().to_path_buf();
                _held = Some(dir);
                said.push("no package was checked: the change is not proved".into());
            }
        }
    }
    let after_scan = crate::cargo::builder(&scratch_root)?.ignore_parse_errors(true).parse().map_err(|e| PassengerError::Scan { detail: e.to_string() })?;
    let mut postconditions: Vec<(String, bool)> = planned.facts.iter().map(|f| (f.clone(), holds(&after_scan, &scratch_root, f))).collect();
    if keeps_expansion(&spec) && !opts.no_cargo {
        postconditions.push((format!("expands {} the same", spec.target), expansion.as_ref().is_some_and(|e| e.same)));
    }
    if opts.tests && !opts.no_cargo {
        postconditions.push((format!("tests {} as before", land.package), compared.as_ref().is_some_and(crate::proof_dir::Compared::ok)));
    }
    let checks_ok = checks.iter().all(|c| c.ok);
    let posts_ok = postconditions.iter().all(|(_, h)| *h);
    said.push(format!("cargo check: {} of {} configurations compile; postconditions: {} of {} hold", checks.iter().filter(|c| c.ok).count(), checks.len(), postconditions.iter().filter(|(_, h)| *h).count(), postconditions.len()));
    for c in checks.iter().filter(|c| !c.ok) {
        said.push(format!("does not compile: {} {}{}: {}", c.package, if c.no_default { "--no-default-features " } else { "" }, c.features.join(","), c.error.clone().unwrap_or_default()));
    }
    // code naming an optional dependency with no feature gate: the fix is the spec's (custodian_kit's rule, the one
    // the preflight refuses by)
    if !checks.iter().all(|c| c.ok) {
        for u in change::optional_uses(&k, &spec, &land).into_iter().filter(|u| !u.gated) {
            said.push(format!("the fix: {}", u.fix()));
        }
    }
    for (f, h) in &postconditions {
        if !h {
            said.push(format!("postcondition does not hold: {f}"));
        }
    }
    let proved = checks_ok && posts_ok && (if opts.no_cargo { opts.unproven } else { !checks.is_empty() });
    let mut files: Vec<FileWrite> = edits
        .files
        .iter()
        .filter(|f| f.before != f.after || !f.path.exists())
        .map(|f| {
            let created = match planned.tree.files.get(&f.path) {
                Some((b, _)) => b.is_none(),
                // the lock file cargo wrote in the proof
                None => !f.path.exists(),
            };
            FileWrite {
                path: relative(&root, &f.path).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_default(),
                before: if created { "none".into() } else { hex(f.before.as_bytes()) },
                after: hex(f.after.as_bytes()),
                lines: written_lines(if created { "" } else { &f.before }, &f.after),
            }
        })
        .collect();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let mut record = TransformRecord {
        schema: TRANSFORM_SCHEMA.into(),
        spec_name: spec.name.clone(),
        spec: relative(&root, &spec_path).map(|r| r.display().to_string()).unwrap_or_else(|_| spec_path.display().to_string()),
        spec_digest: spec.digest(),
        operation: spec.op().as_str().into(),
        target: spec.target.clone(),
        preflight_seq: opts.preflight,
        code_digest: custodian_kit::code_digest(&root, &custodian_kit::reading::spec::Scope { tests: true, ..Default::default() }.scan_options()).ok(),
        files,
        checkpoint_before: None,
        checkpoint_after: None,
        checks,
        postconditions,
        listed: planned.listed.clone(),
        generated: planned.generated.clone(),
        written: false,
        reverts: None,
    };
    for l in &planned.also {
        said.push(format!("also: {l}"));
    }
    for l in &planned.listed {
        said.push(format!("listed, not edited: {l}"));
    }
    let mut exit_code = if proved { 0 } else { 1 };
    if opts.write {
        if !proved {
            // the reason with it: the first check that failed and cargo's first error line for it
            let why = record.checks.iter().find(|c| !c.ok).map(|c| format!(": {} does not compile: {}", c.package, c.error.clone().unwrap_or_else(|| "cargo gave no error line".into()))).unwrap_or_default();
            said.push(format!("nothing written: the change is not proved{why}"));
        } else {
            let store = crate::passenger::PassengerStore::open_or_init(&root)?;
            let before = store.checkpoint_tree(&format!("before transform {}", spec.name), None)?;
            edits.write()?;
            let after = store.checkpoint_tree(&format!("transform {} ({})", spec.name, spec.op().as_str()), None)?;
            record.checkpoint_before = Some(before.id.clone());
            record.checkpoint_after = Some(after.id.clone());
            record.written = true;
            said.push(format!("written: {} files; passenger {} → {}", record.files.len(), before.id, after.id));
        }
    } else {
        exit_code = if proved { 0 } else { 1 };
        said.push(dry_line(proved, &record, &said));
    }
    let record_file = if record.written || opts.out.is_some() { record_path(&root, opts, &spec) } else { None };
    if let Some(p) = &record_file {
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(p, serde_json::to_string_pretty(&record)? + "\n")?;
        if let Some(e) = &expansion {
            std::fs::write(expansion_file(p), serde_json::to_string_pretty(e)? + "\n")?;
        }
        if let Some(c) = &compared {
            std::fs::write(tests_file(p), serde_json::to_string_pretty(c)? + "\n")?;
        }
    }
    Ok(Outcome { root, record, record_file, edits, exit_code, said })
}

// ---------------------------------------------------------------------------------------------------------------------
// `transform --batch LIST`: the rule for repeated shapes (James, 2026-10-20). One member of a shape is made alone first,
// fully proved; the rest of that shape go as one batch: every member planned over one scan, the edits placed together,
// one cargo check over the check matrix, one scan of the copy for every member's postconditions, one checkpoint before
// and after, and one record per member (each file's digest before and after that member, so each is reverted alone).
// A member whose edit overlaps another's, whose code does not compile, or whose postcondition does not hold is
// excluded and named; the rest land.
// ---------------------------------------------------------------------------------------------------------------------

/// What `transform --batch` was asked.
#[derive(Debug, Clone, Default)]
pub struct BatchOptions {
    /// The members' change specs.
    pub specs: Vec<PathBuf>,
    pub write: bool,
    /// The folder each member's record goes into (`<spec name>.json`); else the project's transforms folder.
    pub out_dir: Option<PathBuf>,
    /// The preflight receipt each member runs on, by spec name.
    pub preflights: BTreeMap<String, i64>,
    /// The receipt of the one member of this shape made alone and proved first.
    pub after_proof: Option<i64>,
    /// The landed crates' tests run once, compared with the same tests before the batch.
    pub tests: bool,
}

/// A member left out of a batch, and why.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Excluded {
    pub spec: String,
    pub why: String,
}

/// What a batch did: one record per member written (or proved), the members left out.
#[derive(Debug)]
pub struct BatchOutcome {
    pub root: PathBuf,
    pub records: Vec<(TransformRecord, Option<PathBuf>)>,
    pub excluded: Vec<Excluded>,
    /// The batch's edits together.
    pub edits: EditSet,
    /// Each swap's expansion before and after, by its function.
    pub expansions: BTreeMap<String, Expansion>,
    /// Each landed crate's tests against the same tests before the batch.
    pub tests: BTreeMap<String, crate::proof_dir::Compared>,
    pub said: Vec<String>,
    pub exit_code: i32,
}

/// One member: its spec, where it lands, its plan, and the spans it replaces (in the file as it is).
struct Member {
    spec: ChangeSpec,
    spec_path: PathBuf,
    land: Landing,
    planned: Planned,
    spans: Vec<Span>,
}

/// One replaced span of a file: bytes `start..end` of the text as it is become `text`.
#[derive(Clone)]
struct Span {
    file: PathBuf,
    start: usize,
    end: usize,
    text: String,
}

/// The span one edit replaces: the text between the longest common prefix and suffix of before and after.
fn span_of(file: &Path, before: &str, after: &str) -> Span {
    let (b, a) = (before.as_bytes(), after.as_bytes());
    let mut p = 0;
    while p < b.len() && p < a.len() && b[p] == a[p] {
        p += 1;
    }
    while !(before.is_char_boundary(p) && after.is_char_boundary(p)) {
        p -= 1;
    }
    let mut s = 0;
    while s < b.len() - p && s < a.len() - p && b[b.len() - 1 - s] == a[a.len() - 1 - s] {
        s += 1;
    }
    while !(before.is_char_boundary(b.len() - s) && after.is_char_boundary(a.len() - s)) {
        s -= 1;
    }
    Span { file: file.to_path_buf(), start: p, end: b.len() - s, text: after[p..a.len() - s].to_string() }
}

/// Two spans of one file that cannot both be placed: they overlap, or both insert at one place (their order unknown).
fn overlaps(x: &Span, y: &Span) -> bool {
    x.file == y.file && (x.start.max(y.start) < x.end.min(y.end) || x.start == y.start)
}

/// The members placed in order over the texts as they are: each member's file writes (before and after that member)
/// and, for every member, the lines its spans cover in the text after the whole batch.
struct Placed {
    /// path → (text as it is, text after the batch)
    files: BTreeMap<PathBuf, (String, String)>,
    /// per member: (path, text before the member, text after it)
    steps: Vec<Vec<(PathBuf, String, String)>>,
    /// per member: (path, first line, last line) in the text after the batch
    lines: Vec<Vec<(PathBuf, u32, u32)>>,
}

fn place(members: &[Member]) -> Result<Placed> {
    let mut files: BTreeMap<PathBuf, (String, String)> = BTreeMap::new();
    for m in members {
        for s in &m.spans {
            if !files.contains_key(&s.file) {
                let t = std::fs::read_to_string(&s.file)?;
                files.insert(s.file.clone(), (t.clone(), t));
            }
        }
    }
    // a span's place now: its place in the text as it is, moved by every span placed before it in the file
    // (start, end, length change) of each span placed; a span starting where `at` is is the one asked about (two
    // spans starting at one place are never in one batch)
    let mut placed: BTreeMap<PathBuf, Vec<(usize, usize, isize)>> = BTreeMap::new();
    let shift = |placed: &BTreeMap<PathBuf, Vec<(usize, usize, isize)>>, file: &Path, at: usize| -> usize {
        let d: isize = placed.get(file).map(|v| v.iter().filter(|(st, e, _)| *e <= at && *st != at).map(|(_, _, d)| *d).sum()).unwrap_or(0);
        (at as isize + d) as usize
    };
    let mut steps = Vec::new();
    for m in members {
        let mut step = Vec::new();
        for s in &m.spans {
            let cur = files.get(&s.file).map(|(_, now)| now.clone()).unwrap_or_default();
            let (a, b) = (shift(&placed, &s.file, s.start), shift(&placed, &s.file, s.end));
            let next = format!("{}{}{}", &cur[..a], s.text, &cur[b..]);
            step.push((s.file.clone(), cur, next.clone()));
            if let Some(f) = files.get_mut(&s.file) {
                f.1 = next;
            }
            placed.entry(s.file.clone()).or_default().push((s.start, s.end, s.text.len() as isize - (s.end - s.start) as isize));
        }
        steps.push(step);
    }
    // the lines each member's spans cover after the whole batch
    let mut lines = Vec::new();
    for m in members {
        let mut l = Vec::new();
        for s in &m.spans {
            let after = &files[&s.file].1;
            let begin = shift(&placed, &s.file, s.start).min(after.len());
            let first = after[..begin].matches('\n').count() as u32 + 1;
            let last = first + s.text.matches('\n').count() as u32;
            l.push((s.file.clone(), first, last));
        }
        lines.push(l);
    }
    Ok(Placed { files, steps, lines })
}

/// The members of a batch planned over one scan: each spec's plan and the spans it replaces; a member that changes a
/// manifest, cannot be planned, writes nothing, or overlaps a member before it, left out and named.
fn members_of(k: &klepto::Klepto, root: &Path, specs: &[PathBuf]) -> Result<(Vec<Member>, Vec<Excluded>)> {
    let mut excluded = Vec::new();
    let mut members: Vec<Member> = Vec::new();
    for path in specs {
        let spec_path = if path.is_absolute() { path.clone() } else { root.join(path) };
        let spec = ChangeSpec::from_path(&spec_path).map_err(refused)?;
        // a change to a manifest is a member like a code change: the lock file cargo writes in the proof is carried
        // with the batch's write (`batch_lock`), one preflight, one transform, one check for every caller
        let planned_member = change::landing(k, root, &spec).map_err(refused).and_then(|land| plan(k, root, &spec, &land).map(|p| (land, p)));
        let (land, planned) = match planned_member {
            Ok(x) => x,
            Err(e) => {
                excluded.push(Excluded { spec: spec.name.clone(), why: e.to_string() });
                continue;
            }
        };
        let spans: Vec<Span> = planned.tree.edits().files.iter().filter(|f| f.before != f.after).map(|f| span_of(&f.path, &f.before, &f.after)).collect();
        if spans.is_empty() {
            excluded.push(Excluded { spec: spec.name.clone(), why: "nothing to write".into() });
            continue;
        }
        if let Some(other) = members.iter().find(|m| m.spans.iter().any(|x| spans.iter().any(|y| overlaps(x, y)))) {
            excluded.push(Excluded { spec: spec.name.clone(), why: format!("its edit overlaps {}'s: made alone, after this batch", other.spec.name) });
            continue;
        }
        members.push(Member { spec, spec_path, land, planned, spans });
    }
    Ok((members, excluded))
}

/// A change spec that edits a manifest (a dependency added, changed or removed, a feature declared).
fn is_manifest_change(spec: &ChangeSpec) -> bool {
    matches!(spec.op(), Operation::AddDependency | Operation::ChangeDependency | Operation::DeclareFeature | Operation::RemoveDependency)
}

/// The lock file cargo wrote in the proof's copy for the batch's manifests, as an edit of the tree's, when it moved.
fn batch_lock(oracle: &crate::oracle::CargoOracle, root: &Path, edits: &EditSet) -> Option<FileEdit> {
    let d = oracle.scratch_dir()?;
    let ws = oracle.workspace().map(Path::to_path_buf).unwrap_or_else(|| root.to_path_buf());
    let after = std::fs::read_to_string(d.join("Cargo.lock")).ok()?;
    let lock = ws.join("Cargo.lock");
    let before = std::fs::read_to_string(&lock).unwrap_or_default();
    (after != before && !edits.files.iter().any(|f| f.path == lock)).then_some(FileEdit { path: lock, before, after })
}

/// `transform --batch`: the members planned over one scan, placed together, proved once, written once, recorded one
/// by one.
pub fn run_batch(root: &Path, opts: &BatchOptions) -> Result<BatchOutcome> {
    let root = workspace_root(root)?;
    if opts.specs.is_empty() {
        return Err(refused("a batch needs at least one spec"));
    }
    let k = crate::cargo::builder(&root)?.ignore_parse_errors(true).parse().map_err(|e| PassengerError::Scan { detail: e.to_string() })?;
    let mut said = Vec::new();
    let (mut members, mut excluded) = members_of(&k, &root, &opts.specs)?;
    said.push(format!("batch of {}{}: {} planned over one scan, {} left out", opts.specs.len(), opts.after_proof.map(|s| format!(" after proof #{s}")).unwrap_or_default(), members.len(), excluded.len()));

    // the proof, on the workspace's proof place (one copy, one kept build): each swap's expansion before, as the tree
    // is; then the members placed together, one cargo check over the check matrix of the packages they land in (their
    // tests too) and each that depends on one of them directly; a member the errors point into, whose postcondition
    // does not hold, or whose expansion is not the same, is left out and named, and the rest checked again
    let proof = crate::proof_dir::ProofDir::take(&root)?;
    let mut before: BTreeMap<String, Option<String>> = BTreeMap::new();
    let mut expand_line = String::new();
    let mut expand_ms = 0i64;
    for p in members.iter().filter(|m| keeps_expansion(&m.spec)).map(|m| m.land.package.clone()).collect::<BTreeSet<_>>() {
        let fns: Vec<String> = members.iter().filter(|m| keeps_expansion(&m.spec) && m.land.package == p).map(|m| m.spec.target.clone()).collect();
        let (found, line, ms) = proof.expand_fns(&p, &fns)?;
        before.extend(found);
        expand_line = line;
        expand_ms += ms;
    }
    let mut oracle = crate::oracle::CargoOracle::with_proof(&root, proof);
    oracle.tests = true;
    let mut checks: Vec<Checked> = Vec::new();
    let mut placed = place(&members)?;
    let mut after_scan = None;
    let mut postconditions: Vec<Vec<(String, bool)>> = Vec::new();
    let mut expansions: BTreeMap<String, Expansion> = BTreeMap::new();
    let mut packages_line = String::new();
    for _round in 0..8 {
        if members.is_empty() {
            break;
        }
        let edits = EditSet { files: placed.files.iter().map(|(p, (b, a))| FileEdit { path: p.clone(), before: b.clone(), after: a.clone() }).collect(), manifest: None };
        let mut packages: Vec<change::ProofPackage> = Vec::new();
        for m in &members {
            for pp in proof_packages_of(&k, &m.spec, &m.land, std::slice::from_ref(&m.land.package)) {
                match packages.iter_mut().find(|x| x.package == pp.package) {
                    Some(x) => x.tests |= pp.tests,
                    None => packages.push(pp),
                }
            }
        }
        packages_line = proof_line(&packages);
        checks.clear();
        let mut errors: Vec<klepto::Diagnostic> = Vec::new();
        'packages: for pp in &packages {
            let Some(model) = k.features.get(&pp.package) else { continue };
            // the configurations as the member that changes this package's manifest leaves them (else the first's)
            let spec_of = members.iter().find(|m| m.land.package == pp.package && is_manifest_change(&m.spec)).unwrap_or(&members[0]);
            let desired = change::model_after(&k, &root, &spec_of.spec, &pp.package).unwrap_or_else(|| model.clone());
            for job in klepto::check_matrix(&desired) {
                let r = oracle.check_edits_tests(&job, &edits, pp.tests);
                let error = r.diagnostics.iter().find(|d| d.level == "error").map(|d| d.message.clone());
                checks.push(Checked { package: pp.package.clone(), features: job.features.clone(), no_default: job.no_default, ok: r.ok, error });
                if !r.ok {
                    errors = r.diagnostics.into_iter().filter(|d| d.level == "error").collect();
                    break 'packages;
                }
            }
        }
        if !errors.is_empty() {
            // the members whose lines an error names
            let mut blamed: BTreeSet<usize> = BTreeSet::new();
            for d in &errors {
                let (Some(f), Some(line)) = (&d.file, d.line) else { continue };
                for (i, spans) in placed.lines.iter().enumerate() {
                    if spans.iter().any(|(p, a, b)| relative(&root, p).is_ok_and(|r| r == *f || f.ends_with(&r)) && (*a..=*b).contains(&line)) {
                        blamed.insert(i);
                    }
                }
            }
            if blamed.is_empty() {
                said.push(format!("cargo check: an error no member's lines hold: {}", errors[0].message));
                break;
            }
            for i in blamed.iter().rev() {
                let m = members.remove(*i);
                let why = errors.iter().find(|d| d.line.is_some_and(|l| placed.lines[*i].iter().any(|(_, a, b)| (*a..=*b).contains(&l)))).map(|d| d.message.clone()).unwrap_or_default();
                excluded.push(Excluded { spec: m.spec.name.clone(), why: format!("does not compile with the batch: {why}") });
            }
            placed = place(&members)?;
            continue;
        }
        // one scan of the copy for every member's postconditions
        let scratch_root = match oracle.scratch_dir() {
            Some(d) => d.join(relative(oracle.workspace().unwrap_or(&root), &root).unwrap_or_default()),
            None => {
                said.push("no package was checked: the batch is not proved".into());
                break;
            }
        };
        let scan = crate::cargo::builder(&scratch_root)?.ignore_parse_errors(true).parse().map_err(|e| PassengerError::Scan { detail: e.to_string() })?;
        postconditions = members.iter().map(|m| m.planned.facts.iter().map(|f| (f.clone(), holds(&scan, &scratch_root, f))).collect()).collect();
        // one expansion per crate after, each swap's function compared with its own before
        expansions.clear();
        let proof = oracle.proof().ok_or_else(|| refused("the proof place was let go"))?;
        for p in members.iter().filter(|m| keeps_expansion(&m.spec)).map(|m| m.land.package.clone()).collect::<BTreeSet<_>>() {
            let fns: Vec<String> = members.iter().filter(|m| keeps_expansion(&m.spec) && m.land.package == p).map(|m| m.spec.target.clone()).collect();
            let (after, _, ms) = proof.expand_fns(&p, &fns)?;
            expand_ms += ms;
            for f in fns {
                let (b, a) = (before.get(&f).cloned().flatten(), after.get(&f).cloned().flatten());
                let same = b.is_some() && a == b;
                expansions.insert(f.clone(), Expansion { function: f, package: p.clone(), before: b.unwrap_or_default(), after: a.unwrap_or_default(), same, line: expand_line.clone(), millis: expand_ms });
            }
        }
        for (i, m) in members.iter().enumerate() {
            if keeps_expansion(&m.spec) {
                postconditions[i].push((format!("expands {} the same", m.spec.target), expansions.get(&m.spec.target).is_some_and(|e| e.same)));
            }
        }
        let failing: Vec<usize> = postconditions.iter().enumerate().filter(|(_, p)| p.iter().any(|(_, h)| !h)).map(|(i, _)| i).collect();
        if failing.is_empty() {
            after_scan = Some(scan);
            break;
        }
        for i in failing.iter().rev() {
            let m = members.remove(*i);
            let f = postconditions[*i].iter().find(|(_, h)| !h).map(|(f, _)| f.clone()).unwrap_or_default();
            excluded.push(Excluded { spec: m.spec.name.clone(), why: format!("the postcondition {f} does not hold") });
        }
        placed = place(&members)?;
    }
    said.push(packages_line);
    // the landed crates' tests, once each, against the same tests before the batch
    let mut tests_ok = true;
    let mut compared: BTreeMap<String, crate::proof_dir::Compared> = BTreeMap::new();
    if opts.tests && after_scan.is_some() {
        let edits = EditSet { files: placed.files.iter().map(|(p, (b, a))| FileEdit { path: p.clone(), before: b.clone(), after: a.clone() }).collect(), manifest: None };
        let proof = oracle.proof().ok_or_else(|| refused("the proof place was let go"))?;
        for p in members.iter().map(|m| m.land.package.clone()).collect::<BTreeSet<_>>() {
            let c = proof.tests_compared(&p, &edits)?;
            said.push(tests_line(&p, &c));
            tests_ok &= c.ok();
            compared.insert(p, c);
        }
        for (i, m) in members.iter().enumerate() {
            if let Some(post) = postconditions.get_mut(i) {
                post.push((format!("tests {} as before", m.land.package), compared.get(&m.land.package).is_some_and(crate::proof_dir::Compared::ok)));
            }
        }
    }
    if !expansions.is_empty() {
        said.push(format!("cargo expand: {} of {} swaps the same tokens before and after (in the normal form)", expansions.values().filter(|e| e.same).count(), expansions.len()));
    }
    let proved = after_scan.is_some() && tests_ok && !members.is_empty() && !checks.is_empty() && checks.iter().all(|c| c.ok);
    said.push(format!(
        "cargo check: {} of {} configurations compile with the batch's {} members together{}",
        checks.iter().filter(|c| c.ok).count(),
        checks.len(),
        members.len(),
        if proved { "; every member's postconditions hold" } else { "" }
    ));
    for e in &excluded {
        said.push(format!("left out: {} — {}", e.spec, e.why));
    }
    let mut edits = EditSet { files: placed.files.iter().map(|(p, (b, a))| FileEdit { path: p.clone(), before: b.clone(), after: a.clone() }).collect(), manifest: None };
    if members.iter().any(|m| is_manifest_change(&m.spec)) {
        if let Some(lock) = batch_lock(&oracle, &root, &edits) {
            said.push("Cargo.lock: cargo's, from the proof, written with the batch's manifests".into());
            edits.files.push(lock);
        }
    }
    let code_digest = custodian_kit::code_digest(&root, &custodian_kit::reading::spec::Scope { tests: true, ..Default::default() }.scan_options()).ok();
    let mut records: Vec<TransformRecord> = members
        .iter()
        .zip(placed.steps.iter())
        .enumerate()
        .map(|(i, (m, step))| TransformRecord {
            schema: TRANSFORM_SCHEMA.into(),
            spec_name: m.spec.name.clone(),
            spec: relative(&root, &m.spec_path).map(|r| r.display().to_string()).unwrap_or_else(|_| m.spec_path.display().to_string()),
            spec_digest: m.spec.digest(),
            operation: m.spec.op().as_str().into(),
            target: m.spec.target.clone(),
            preflight_seq: opts.preflights.get(&m.spec.name).copied(),
            code_digest: code_digest.clone(),
            files: step
                .iter()
                .map(|(p, b, a)| FileWrite { path: relative(&root, p).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_default(), before: hex(b.as_bytes()), after: hex(a.as_bytes()), lines: written_lines(b, a) })
                .collect(),
            checkpoint_before: None,
            checkpoint_after: None,
            checks: checks.clone(),
            postconditions: postconditions.get(i).cloned().unwrap_or_default(),
            listed: m.planned.listed.clone(),
            generated: m.planned.generated.clone(),
            written: false,
            reverts: None,
        })
        .collect();
    let mut exit_code = if proved { 0 } else { 1 };
    if opts.write {
        if !proved {
            said.push("nothing written: the batch is not proved".into());
            exit_code = 1;
        } else {
            let store = crate::passenger::PassengerStore::open_or_init(&root)?;
            let before = store.checkpoint_tree(&format!("before a batch of {}", members.len()), None)?;
            // each member's texts kept, so a member's record reverts alone
            for step in &placed.steps {
                for (_, b, a) in step {
                    store.keep(b.as_bytes())?;
                    store.keep(a.as_bytes())?;
                }
            }
            edits.write()?;
            let after = store.checkpoint_tree(&format!("a batch of {}{}", members.len(), opts.after_proof.map(|s| format!(" after proof #{s}")).unwrap_or_default()), None)?;
            for r in &mut records {
                r.checkpoint_before = Some(before.id.clone());
                r.checkpoint_after = Some(after.id.clone());
                r.written = true;
            }
            said.push(format!("written: {} members in {} files; passenger {} → {}", records.len(), placed.files.len(), before.id, after.id));
        }
    } else if proved {
        said.push(format!("proved on a scratch copy, not written (--write writes): {} members in {} files", records.len(), placed.files.len()));
    } else {
        said.push("not proved: nothing written".into());
    }
    let mut out = Vec::new();
    for r in records {
        let file = if r.written || opts.out_dir.is_some() {
            match &opts.out_dir {
                Some(d) => Some(d.join(format!("{}.json", r.spec_name))),
                None => custodian_kit::store::Store::find(&root).and_then(|s| s.path(&custodian_kit::store::named::TRANSFORM, &[("name", &r.spec_name), ("stamp", &custodian_kit::runs::stamp())]).ok()),
            }
        } else {
            None
        };
        if let Some(p) = &file {
            if let Some(d) = p.parent() {
                std::fs::create_dir_all(d)?;
            }
            std::fs::write(p, serde_json::to_string_pretty(&r)? + "\n")?;
            if let Some(e) = expansions.get(&r.target) {
                std::fs::write(expansion_file(p), serde_json::to_string_pretty(e)? + "\n")?;
            }
            if let Some(c) = members.iter().find(|m| m.spec.name == r.spec_name).and_then(|m| compared.get(&m.land.package)) {
                std::fs::write(tests_file(p), serde_json::to_string_pretty(c)? + "\n")?;
            }
        }
        out.push((r, file));
    }
    Ok(BatchOutcome { root, records: out, excluded, edits, expansions, tests: compared, said, exit_code })
}

/// What `transform --batch` prints: what it did, the members left out, the diff, each member's record.
pub fn render_batch(o: &BatchOutcome) -> String {
    let mut s = String::new();
    for l in &o.said {
        s.push_str(&format!("{l}\n"));
    }
    s.push_str(&o.edits.diff_from(&o.root));
    for (r, f) in &o.records {
        if let Some(f) = f {
            s.push_str(&format!("record: {} {}\n", r.spec_name, f.display()));
        }
    }
    s
}

/// The line a transform without `--write` ends with: proved on a scratch copy and not written, or not proved and
/// why (the first configuration that does not compile, the first postcondition that does not hold, or nothing checked).
fn dry_line(proved: bool, record: &TransformRecord, said: &[String]) -> String {
    if proved {
        return format!("proved on a scratch copy, not written (--write writes): {} files", record.files.len());
    }
    let why = record
        .checks
        .iter()
        .find(|c| !c.ok)
        .map(|c| format!("{} {}{} does not compile: {}", c.package, if c.no_default { "--no-default-features" } else { "" }, if c.features.is_empty() { String::new() } else { format!(" {}", c.features.join(",")) }, c.error.clone().unwrap_or_default()))
        .or_else(|| record.postconditions.iter().find(|(_, h)| !h).map(|(f, _)| format!("the postcondition {f} does not hold")))
        .or_else(|| said.iter().find(|l| l.contains("not proved")).cloned())
        .unwrap_or_else(|| "no configuration was checked".into());
    format!("not proved: {why}; nothing written")
}

/// `transform --revert RECORD`: every file the record wrote put back as it was, when it is still as the record left
/// it (a file the transform made is taken away again); the revert's own record beside it. RECORD may be a batch's
/// folder: its members' records chain within a file (each member's before is the one before it's after), so the batch
/// is reverted in one step, each file put back to what it was before the batch's first member. A revert builds no
/// proof: the files come back from the record's snapshots, then one `cargo check` of the packages they sit in, on the
/// kept build, says whether the tree compiles; its record holds each file's digest now and after, so a job's close
/// covers its lines as the transform's own.
pub fn revert(root: &Path, record_file: &Path) -> Result<Outcome> {
    let root = workspace_root(root)?;
    let (rec, chains, file) = if record_file.is_dir() {
        let members = batch_members(record_file)?;
        let first = members.first().cloned().ok_or_else(|| refused(format!("{}: no written member record in the batch", record_file.display())))?;
        let name = format!("{} (batch of {})", record_file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), members.len());
        let file = record_file.with_file_name(format!("{}.revert.json", record_file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
        (TransformRecord { spec_name: name, ..first }, chained(&members), file)
    } else {
        let text = std::fs::read_to_string(record_file).map_err(|e| PassengerError::Io { detail: format!("{}: {e}", record_file.display()) })?;
        let rec: TransformRecord = serde_json::from_str(&text)?;
        if !rec.written {
            return Err(refused(format!("{}: it wrote nothing", record_file.display())));
        }
        let chains = rec.files.iter().map(|f| (f.path.clone(), f.before.clone(), f.after.clone())).collect();
        let file = record_file.with_file_name(format!("{}.revert.json", record_file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()));
        (rec, chains, file)
    };
    let store = crate::passenger::PassengerStore::open(&root)?;
    let mut files = Vec::new();
    let mut edits = EditSet::default();
    let mut removed = Vec::new();
    for (path, was, wrote) in &chains {
        let p = root.join(path);
        let now = std::fs::read(&p).ok();
        let now_text = String::from_utf8(now.clone().unwrap_or_default()).unwrap_or_default();
        let now_digest = now.as_deref().map(hex).unwrap_or_else(|| "none".into());
        let back = if &now_digest == wrote {
            if was == "none" { None } else { Some(String::from_utf8(store.object(was)?).map_err(refused)?) }
        } else {
            // the file moved since (the members of a batch after this one): this change alone taken out, when its
            // text is still in the file once, as it wrote it
            let dirty = || PassengerError::Dirty { detail: format!("{path} changed since the transform wrote it, and its change is not found in it once as written") };
            let (Ok(b), Ok(a)) = (store.object(was), store.object(wrote)) else { return Err(dirty()) };
            let (b, a) = (String::from_utf8(b).map_err(refused)?, String::from_utf8(a).map_err(refused)?);
            Some(take_out(&now_text, &b, &a).ok_or_else(dirty)?)
        };
        // the revert's own write: from the file as it is now, so the close's coverage chains through it
        files.push(FileWrite { path: path.clone(), before: now_digest, after: back.as_deref().map(|b| hex(b.as_bytes())).unwrap_or_else(|| "none".into()), lines: back.as_deref().map(|b| written_lines(&now_text, b)).unwrap_or_default() });
        match back {
            Some(b) => edits.files.push(FileEdit { path: p, before: now_text, after: b }),
            None => removed.push(p),
        }
    }
    // a snapshot of the files, not a scan of the tree: the record's digests are the proof of what came back
    let note = |n: String| crate::passenger::CheckpointOptions { note: Some(n), ..Default::default() };
    let before = store.checkpoint(None, None, note(format!("before reverting {}", rec.spec_name)))?;
    edits.write()?;
    for p in &removed {
        std::fs::remove_file(p)?;
    }
    let after = store.checkpoint(None, None, note(format!("reverted {}", rec.spec_name)))?;
    let checks = check_once(&root, &files);
    let out = TransformRecord { files, checkpoint_before: Some(before.id.clone()), checkpoint_after: Some(after.id.clone()), written: true, reverts: Some(relative(&root, record_file).map(|r| r.display().to_string()).unwrap_or_default()), checks, postconditions: Vec::new(), listed: Vec::new(), generated: Vec::new(), ..rec.clone() };
    std::fs::write(&file, serde_json::to_string_pretty(&out)? + "\n")?;
    let compiled = out.checks.iter().all(|c| c.ok);
    let mut said = vec![format!("reverted {}: {} files back as they were; passenger {} → {}", rec.spec_name, out.files.len(), before.id, after.id)];
    said.push(format!("cargo check once after the revert: {} of {} packages compile{}", out.checks.iter().filter(|c| c.ok).count(), out.checks.len(), out.checks.iter().find_map(|c| c.error.as_ref()).map(|e| format!(" ({e})")).unwrap_or_default()));
    Ok(Outcome { root, record: out, record_file: Some(file), edits, exit_code: if compiled { 0 } else { 1 }, said })
}

/// A batch folder's member records, written ones only, in the order they were written (their chain within each file).
fn batch_members(dir: &Path) -> Result<Vec<TransformRecord>> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json") && !p.to_string_lossy().ends_with(".revert.json")).collect();
    paths.sort();
    Ok(paths.iter().filter_map(|p| std::fs::read_to_string(p).ok()).filter_map(|t| serde_json::from_str::<TransformRecord>(&t).ok()).filter(|r| r.written && r.reverts.is_none()).collect())
}

/// Each file the members wrote, as the batch took it: (path, its digest before the first member that wrote it, its
/// digest after the last). Within a file the members chain: one's before is another's after.
fn chained(members: &[TransformRecord]) -> Vec<(String, String, String)> {
    let mut by: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for m in members {
        for f in &m.files {
            by.entry(f.path.clone()).or_default().push((f.before.clone(), f.after.clone()));
        }
    }
    by.into_iter()
        .map(|(path, writes)| {
            let afters: BTreeSet<&String> = writes.iter().map(|(_, a)| a).collect();
            let befores: BTreeSet<&String> = writes.iter().map(|(b, _)| b).collect();
            let first = writes.iter().find(|(b, _)| !afters.contains(b)).map(|(b, _)| b.clone()).unwrap_or_else(|| writes[0].0.clone());
            let last = writes.iter().rev().find(|(_, a)| !befores.contains(a)).map(|(_, a)| a.clone()).unwrap_or_else(|| writes[writes.len() - 1].1.clone());
            (path, first, last)
        })
        .collect()
}

/// One `cargo check` of each package the files sit in (the nearest manifest's `[package] name`), on the kept build:
/// what a revert pays instead of a proof.
fn check_once(root: &Path, files: &[FileWrite]) -> Vec<Checked> {
    let mut packages = BTreeSet::new();
    for f in files {
        let mut dir = root.join(&f.path).parent().map(Path::to_path_buf);
        while let Some(d) = dir {
            if let Some(name) = std::fs::read_to_string(d.join("Cargo.toml")).ok().and_then(|t| t.parse::<toml::Table>().ok()).and_then(|t| t.get("package")?.get("name")?.as_str().map(str::to_string)) {
                packages.insert(name);
                break;
            }
            if d == root {
                break;
            }
            dir = d.parent().map(Path::to_path_buf);
        }
    }
    let mut oracle = crate::oracle::CargoOracle::new(root);
    packages
        .into_iter()
        .map(|package| {
            let job = klepto::CheckJob { package: package.clone(), features: Vec::new(), no_default: false };
            let r = oracle.check_edits_tests(&job, &EditSet::default(), false);
            let error = r.diagnostics.iter().find(|d| d.level == "error").map(|d| d.message.clone());
            Checked { package, features: Vec::new(), no_default: false, ok: r.ok, error }
        })
        .collect()
}

/// `now` with one change taken out: the span `after` wrote over `before` (their common prefix and suffix aside) found
/// in `now` exactly once with the text before it as `after` has it, and put back as `before` had it.
fn take_out(now: &str, before: &str, after: &str) -> Option<String> {
    let sp = span_of(Path::new(""), before, after);
    let a_end = after.len() - (before.len() - sp.end);
    let written = &after[sp.start..a_end];
    let was = &before[sp.start..sp.end];
    // the text before the span, as far back as needed to make it one place
    for ctx in [80usize, 400, 2000, usize::MAX] {
        let from = sp.start.saturating_sub(ctx);
        let from = (0..=from).rev().find(|i| after.is_char_boundary(*i)).unwrap_or(0);
        let needle = format!("{}{}", &after[from..sp.start], written);
        let at: Vec<usize> = now.match_indices(&needle).map(|(i, _)| i).collect();
        if at.len() == 1 {
            let i = at[0] + (sp.start - from);
            return Some(format!("{}{}{}", &now[..i], was, &now[i + written.len()..]));
        }
        if at.is_empty() {
            return None;
        }
    }
    None
}

/// What `transform` prints: what it did, the diff, the record.
pub fn render(o: &Outcome) -> String {
    let mut s = String::new();
    for l in &o.said {
        s.push_str(&format!("{l}\n"));
    }
    s.push_str(&o.edits.diff_from(&o.root));
    if let Some(f) = &o.record_file {
        s.push_str(&format!("record: {}\n", f.display()));
    }
    s
}

/// Whether `b` may sit just before a crate's name in a path that starts with it (not inside another name, not after
/// another path's `::`).
fn starts_a_path(text: &str, at: usize) -> bool {
    let before = &text[..at];
    let prev = before.chars().last();
    !prev.is_some_and(|c| c.is_alphanumeric() || c == '_') && !before.ends_with("::")
}

/// The places (file, line) in the `.rs` files under `dir` (its `target` aside) that name the crate `name`: a path
/// `name::`, `use name`, `extern crate name`.
fn crate_names(dir: &Path, name: &str) -> Vec<(PathBuf, usize)> {
    let mut out = Vec::new();
    for e in walkdir::WalkDir::new(dir).into_iter().filter_entry(|e| e.file_name() != "target" && e.file_name() != ".git").flatten() {
        if e.path().extension().is_none_or(|x| x != "rs") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(e.path()) else { continue };
        for (i, line) in text.lines().enumerate() {
            let named = line.match_indices(name).any(|(at, _)| {
                let rest = &line[at + name.len()..];
                let next = rest.chars().next();
                starts_a_path(line, at) && (rest.starts_with("::") || ((line[..at].trim_end().ends_with("use") || line[..at].trim_end().ends_with("crate")) && !next.is_some_and(|c| c.is_alphanumeric() || c == '_')))
            });
            if named {
                out.push((e.path().to_path_buf(), i + 1));
            }
        }
    }
    out
}

/// `text` with every path through the crate `old` made a path through `new` (`old::` → `new::`, `use old` and
/// `extern crate old` too), and how many.
fn rename_crate_paths(text: &str, old: &str, new: &str) -> (String, usize) {
    let mut out = String::with_capacity(text.len());
    let mut n = 0;
    let mut last = 0;
    for (at, _) in text.match_indices(old) {
        if at < last {
            continue;
        }
        let rest = &text[at + old.len()..];
        let ends = !rest.chars().next().is_some_and(|c| c.is_alphanumeric() || c == '_');
        let head = text[..at].trim_end();
        let path = rest.starts_with("::") || (ends && (head.ends_with("use") || head.ends_with("crate")));
        if starts_a_path(text, at) && path {
            out.push_str(&text[last..at]);
            out.push_str(new);
            last = at + old.len();
            n += 1;
        }
    }
    out.push_str(&text[last..]);
    (out, n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_replaced_only_where_it_is_a_whole_word() {
        assert_eq!(rename_word("let c = Config::new(Configs, config);", "Config", "Settings"), "let c = Settings::new(Configs, config);");
        assert_eq!(written_lines("a\nb\n", "a\nx\nb\n"), [(2, 2)]);
        let t = "struct A {\n    x: u8\n}\n".to_string();
        let mut t2 = t.clone();
        insert_before_close(&mut t2, LineColumn { line: 3, column: 0 }, "    y: u16,\n", true);
        assert_eq!(t2, "struct A {\n    x: u8,\n    y: u16,\n}\n");
        let mut one = "struct B { x: u8 }\n".to_string();
        insert_before_close(&mut one, LineColumn { line: 1, column: 17 }, "    y: u16,\n", true);
        assert_eq!(one, "struct B { x: u8, y: u16 }\n");
    }

    #[test]
    fn rename_paths_moves_every_path_through_one_crate_and_nothing_else() {
        let text = "use old_kit::a;\nextern crate old_kit;\nfn f() { old_kit::b(); my_old_kit::c(); x::old_kit::d(); }\n";
        let (after, n) = rename_crate_paths(text, "old_kit", "new_kit");
        assert_eq!(n, 3);
        assert_eq!(after, "use new_kit::a;\nextern crate new_kit;\nfn f() { new_kit::b(); my_old_kit::c(); x::old_kit::d(); }\n");
        let dir = std::env::temp_dir().join(format!("cp-names-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), text).unwrap();
        assert_eq!(crate_names(&dir, "old_kit").iter().map(|(_, l)| *l).collect::<Vec<_>>(), vec![1, 2, 3]);
        assert!(crate_names(&dir, "kit").is_empty(), "a name inside another is not the crate");
    }

    #[test]
    fn a_batchs_members_chain_within_a_file_so_the_batch_reverts_to_before_its_first() {
        let rec = |writes: &[(&str, &str, &str)]| -> TransformRecord {
            let files: Vec<serde_json::Value> = writes.iter().map(|(p, b, a)| serde_json::json!({"path": p, "before": b, "after": a, "lines": []})).collect();
            serde_json::from_value(serde_json::json!({"schema": TRANSFORM_SCHEMA, "spec_name": "m", "spec": "s", "spec_digest": "d", "operation": "replace_body", "target": "t", "files": files, "checks": [], "postconditions": [], "written": true})).unwrap()
        };
        // three members in one file (d0 -> d1 -> d2 -> d3), one of them in a second file too
        let members = vec![rec(&[("src/a.rs", "d0", "d1")]), rec(&[("src/a.rs", "d1", "d2"), ("src/b.rs", "e0", "e1")]), rec(&[("src/a.rs", "d2", "d3")])];
        assert_eq!(chained(&members), vec![("src/a.rs".to_string(), "d0".to_string(), "d3".to_string()), ("src/b.rs".to_string(), "e0".to_string(), "e1".to_string())]);
    }
}

//! `transform --spec S [--write]`: one bounded change spec (custodian_kit's
//! `change::ChangeSpec`) made in the code.
//!
//! The spec says what to make or change; custodian_kit says where it lands
//! (`change::landing`, the one rule the preflight reads too) and renders what
//! is written from the spec's fields (`change::render_*`: bodies are
//! simple_impl 0.1.11 toolkit pieces, whole functions and trait impls its
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
use custodian_kit::transform::{Checked, FileWrite, TransformRecord, TRANSFORM_SCHEMA};
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
    /// The closing paren of each call found, and whether it has arguments.
    found: Vec<(LineColumn, bool)>,
}

impl<'ast> Visit<'ast> for Calls {
    fn visit_expr_call(&mut self, e: &'ast syn::ExprCall) {
        let s = e.func.span().start();
        if self.want.contains(&(s.line, s.column)) {
            self.found.push((e.paren_token.span.close().start(), !e.args.is_empty()));
        }
        syn::visit::visit_expr_call(self, e);
    }

    fn visit_expr_method_call(&mut self, e: &'ast syn::ExprMethodCall) {
        let s = e.method.span().start();
        if self.want.contains(&(s.line, s.column)) {
            self.found.push((e.paren_token.span.close().start(), !e.args.is_empty()));
        }
        syn::visit::visit_expr_method_call(self, e);
    }
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
struct Planned {
    tree: Tree,
    generated: Vec<String>,
    listed: Vec<String>,
    /// `exists <id>`, `field <type> <name>`, `variant <enum> <name>`, `method <type> <name>`, `absent <id>`, `text <file> <line>`.
    facts: Vec<String>,
}

fn abs(root: &Path, rel: &str) -> PathBuf {
    root.join(rel)
}

fn ty_name(id: &str) -> &str {
    id.rsplit("::").next().unwrap_or(id)
}

fn module_path_of(id: &str) -> &str {
    id.rsplit_once("::").map(|(m, _)| m).unwrap_or(id)
}

/// The edits an operation makes, unwritten.
fn plan(k: &klepto::Klepto, root: &Path, spec: &ChangeSpec, land: &Landing) -> Result<Planned> {
    use Operation::*;
    let mut p = Planned { tree: Tree::default(), generated: Vec::new(), listed: Vec::new(), facts: Vec::new() };
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
        AddDependency => {
            let d = spec.dependency.as_ref().ok_or_else(|| refused("add_dependency needs dependency"))?;
            let dep = match &d.version {
                Some(v) => format!("{}@{v}", d.name),
                None => d.name.clone(),
            };
            let e = crate::commands::edit::dep_add_plan(root, &land.package, &dep, d.path.as_deref(), &d.features, false)?;
            for f in e.edits.files {
                p.tree.files.insert(f.path.clone(), (Some(f.before), f.after));
            }
            p.facts.push(format!("dependency {} {}", land.package, d.name));
        }
        AddTest => {
            let f = spec.function.as_ref().ok_or_else(|| refused("add_test needs function"))?;
            let mut f = f.clone();
            if !f.attrs.iter().any(|a| a.trim() == "#[test]") {
                f.attrs.insert(0, "#[test]".into());
            }
            let t = p.tree.text(&file)?;
            let parsed = parse(t, &file)?;
            let tests = parsed.items.iter().find_map(|i| match i {
                syn::Item::Mod(m) if m.ident == "tests" && m.content.is_some() => Some((i.span(), m.content.as_ref().map(|(b, _)| b.span.close().start()))),
                _ => None,
            });
            match tests {
                Some((sp, Some(close))) => {
                    let indent = format!("{}    ", indent_of(t, sp.start().line));
                    let text = change::render_fn(&f, &indent).map_err(refused)?;
                    insert_before_close(t, close, &format!("\n{text}"), false);
                    p.generated.push(text);
                }
                _ => {
                    let text = format!("#[cfg(test)]\nmod tests {{\n    use super::*;\n\n{}}}\n", change::render_fn(&f, "    ").map_err(refused)?);
                    if !t.ends_with('\n') {
                        t.push('\n');
                    }
                    t.push('\n');
                    t.push_str(&text);
                    p.generated.push(text);
                }
            }
            p.facts.push(format!("exists {}::tests::{}", land.module, f.name));
        }
        ChangeSignature => {
            let f = spec.function.as_ref().ok_or_else(|| refused("change_signature needs function"))?;
            let old = k.functions.iter().find(|x| x.fq_name == item).ok_or_else(|| refused(format!("{item}: not a function klepto captured")))?;
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
            let added = f.params.len() > old.args.len();
            let name = f.name.clone();
            // its calls: the new argument at the end, when the spec gives it
            if added {
                let mut by_file: BTreeMap<PathBuf, BTreeSet<(usize, usize)>> = BTreeMap::new();
                for c in &k.calls {
                    if matches!(&c.resolution, Some(klepto::Resolution::Local(id) | klepto::Resolution::External(id)) if *id == item)
                        && let Some(l) = c.location.line {
                            by_file.entry(c.location.path.clone()).or_default().extend(both(l, c.location.column));
                        }
                }
                for (cf, want) in by_file {
                    let cf = canon(&cf);
                    let t = p.tree.text(&cf)?.clone();
                    let parsed = parse(&t, &cf)?;
                    let mut v = Calls { want, found: Vec::new() };
                    v.visit_file(&parsed);
                    for (close, has_args) in v.found {
                        match spec.call_sites.as_ref().and_then(|c| c.pass.clone()) {
                            Some(pass) => {
                                let at = offset(&t, close);
                                splices.entry(cf.clone()).or_default().push((at, at, if has_args { format!(", {pass}") } else { pass.clone() }));
                            }
                            None => p.listed.push(format!("call {}:{}: needs the new argument", relative(root, &cf).map(|r| r.display().to_string()).unwrap_or_default(), close.line)),
                        }
                    }
                }
            }
            for (f, e) in splices {
                splice(p.tree.text(&f)?, e);
            }
            let new_id = format!("{}::{name}", module_path_of(&item));
            p.facts.push(format!("args {new_id} {}", f.params.len()));
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
            let vis = if d.is_public { "pub " } else { "pub(crate) " };
            let target_path = format!("crate::{}", dest_module.split_once("::").map(|(_, r)| r).unwrap_or(""));
            t.replace_range(a..b, &format!("{vis}use {}::{};\n", target_path.trim_end_matches("::"), d.name));
            let dt = p.tree.text(&dest)?;
            // the moved item keeps resolving the names of the module it left
            let old_module = format!("crate::{}", d.module.split_once("::").map(|(_, r)| r).unwrap_or("")).trim_end_matches("::").to_string();
            let glob = format!("use {old_module}::*;");
            if !dt.lines().any(|l| l.trim() == glob) {
                let parsed = parse(dt, &dest)?;
                let at = parsed.items.first().map(|i| line_start(dt, i.span().start().line)).unwrap_or(dt.len());
                dt.insert_str(at, &format!("{glob}\n\n"));
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

/// The packages a check covers: the one it lands in and every one that depends on it.
fn checked_packages(k: &klepto::Klepto, package: &str) -> Vec<String> {
    let mut out = BTreeSet::from([package.to_string()]);
    let mut work = vec![package.to_string()];
    while let Some(p) = work.pop() {
        for (dependent, deps) in &k.package_deps {
            if deps.contains(&p) && out.insert(dependent.clone()) {
                work.push(dependent.clone());
            }
        }
    }
    out.into_iter().filter(|p| k.features.contains_key(p)).collect()
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
        ["dependency", p, d] => k.features.get(*p).is_some_and(|m| m.dependencies.iter().any(|x| x.name == *d)),
        ["args", id, n] => k.functions.iter().find(|f| f.fq_name == *id).is_some_and(|f| f.args.len().to_string() == *n),
        ["text", f, line] => std::fs::read_to_string(root.join(f)).is_ok_and(|t| t.lines().any(|l| l.trim() == line.trim())),
        _ => false,
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
    let edits = planned.tree.edits();
    if edits.is_empty() {
        return Err(refused(format!("{}: nothing to write", spec.name)));
    }
    // the proof: cargo over the check matrix of the crate and its dependents, on a scratch copy; then a fresh scan
    let mut checks = Vec::new();
    let mut oracle = crate::oracle::CargoOracle::new(&root);
    let scratch_root: PathBuf;
    let _held;
    if opts.no_cargo {
        let dir = crate::gate::scratch_copy(&root)?;
        edits.write_to(&root, dir.path())?;
        scratch_root = dir.path().to_path_buf();
        _held = Some(dir);
        said.push("cargo not run (--no-cargo)".into());
    } else {
        _held = None;
        for pkg in checked_packages(&k, &land.package) {
            let Some(model) = k.features.get(&pkg) else { continue };
            let mut desired = model.clone();
            // the new dependency is in the edited manifest, not in the model read before the edit
            let _ = &mut desired;
            for job in klepto::check_matrix(&desired) {
                let r = oracle.check_edits(&job, &edits);
                let error = r.diagnostics.iter().find(|d| d.level == "error").map(|d| d.message.clone());
                checks.push(Checked { package: pkg.clone(), features: job.features.clone(), no_default: job.no_default, ok: r.ok, error });
                if !r.ok {
                    break;
                }
            }
            if checks.iter().any(|c| !c.ok) {
                break;
            }
        }
        let ws = oracle.workspace().map(Path::to_path_buf).unwrap_or_else(|| root.clone());
        scratch_root = oracle.scratch_dir().map(|d| d.join(relative(&ws, &root).unwrap_or_default())).unwrap_or_else(|| root.clone());
    }
    let after_scan = crate::cargo::builder(&scratch_root)?.ignore_parse_errors(true).parse().map_err(|e| PassengerError::Scan { detail: e.to_string() })?;
    let postconditions: Vec<(String, bool)> = planned.facts.iter().map(|f| (f.clone(), holds(&after_scan, &scratch_root, f))).collect();
    let checks_ok = checks.iter().all(|c| c.ok);
    let posts_ok = postconditions.iter().all(|(_, h)| *h);
    said.push(format!("cargo check: {} of {} configurations compile; postconditions: {} of {} hold", checks.iter().filter(|c| c.ok).count(), checks.len(), postconditions.iter().filter(|(_, h)| *h).count(), postconditions.len()));
    for c in checks.iter().filter(|c| !c.ok) {
        said.push(format!("does not compile: {} {}{}: {}", c.package, if c.no_default { "--no-default-features " } else { "" }, c.features.join(","), c.error.clone().unwrap_or_default()));
    }
    for (f, h) in &postconditions {
        if !h {
            said.push(format!("postcondition does not hold: {f}"));
        }
    }
    let proved = checks_ok && posts_ok && (!opts.no_cargo || opts.unproven);
    let mut files: Vec<FileWrite> = edits
        .files
        .iter()
        .filter(|f| f.before != f.after || !f.path.exists())
        .map(|f| {
            let created = !planned.tree.files.get(&f.path).is_some_and(|(b, _)| b.is_some());
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
    for l in &planned.listed {
        said.push(format!("listed, not edited: {l}"));
    }
    let mut exit_code = if proved { 0 } else { 1 };
    if opts.write {
        if !proved {
            said.push("nothing written: the change is not proved".into());
        } else {
            let store = crate::passenger::PassengerStore::open_or_init(&root)?;
            let before = store.checkpoint_tree(&format!("before transform {}", spec.name), None)?;
            edits.write()?;
            for f in edits.files.iter() {
                if !f.path.exists() {
                    if let Some(d) = f.path.parent() {
                        std::fs::create_dir_all(d)?;
                    }
                    std::fs::write(&f.path, &f.after)?;
                }
            }
            let after = store.checkpoint_tree(&format!("transform {} ({})", spec.name, spec.op().as_str()), None)?;
            record.checkpoint_before = Some(before.id.clone());
            record.checkpoint_after = Some(after.id.clone());
            record.written = true;
            said.push(format!("written: {} files; passenger {} → {}", record.files.len(), before.id, after.id));
        }
    } else {
        exit_code = if proved { 0 } else { 1 };
        said.push(format!("proved on a scratch copy, not written (--write writes): {} files", record.files.len()));
    }
    let record_file = if record.written || opts.out.is_some() { record_path(&root, opts, &spec) } else { None };
    if let Some(p) = &record_file {
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(p, serde_json::to_string_pretty(&record)? + "\n")?;
    }
    Ok(Outcome { root, record, record_file, edits, exit_code, said })
}

/// `transform --revert RECORD`: every file the record wrote put back as it was, when it is still as the record left
/// it (a file the transform made is taken away again); the revert's own record beside it.
pub fn revert(root: &Path, record_file: &Path) -> Result<Outcome> {
    let root = workspace_root(root)?;
    let text = std::fs::read_to_string(record_file).map_err(|e| PassengerError::Io { detail: format!("{}: {e}", record_file.display()) })?;
    let rec: TransformRecord = serde_json::from_str(&text)?;
    if !rec.written {
        return Err(refused(format!("{}: it wrote nothing", record_file.display())));
    }
    let store = crate::passenger::PassengerStore::open(&root)?;
    let mut files = Vec::new();
    let mut edits = EditSet::default();
    for f in &rec.files {
        let p = root.join(&f.path);
        let now = std::fs::read(&p).ok();
        if now.as_deref().map(hex) != Some(f.after.clone()) {
            return Err(PassengerError::Dirty { detail: format!("{} changed since the transform wrote it", f.path) });
        }
        let back = if f.before == "none" { None } else { Some(String::from_utf8(store.object(&f.before)?).map_err(refused)?) };
        let now_text = String::from_utf8(now.unwrap_or_default()).unwrap_or_default();
        files.push(FileWrite { path: f.path.clone(), before: f.after.clone(), after: back.as_deref().map(|b| hex(b.as_bytes())).unwrap_or_else(|| "none".into()), lines: back.as_deref().map(|b| written_lines(&now_text, b)).unwrap_or_default() });
        match back {
            Some(b) => edits.files.push(FileEdit { path: p, before: now_text, after: b }),
            None => {
                std::fs::remove_file(&p)?;
            }
        }
    }
    let before = store.checkpoint_tree(&format!("before reverting {}", rec.spec_name), None)?;
    edits.write()?;
    let after = store.checkpoint_tree(&format!("reverted {}", rec.spec_name), None)?;
    let out = TransformRecord { files, checkpoint_before: Some(before.id.clone()), checkpoint_after: Some(after.id.clone()), written: true, reverts: Some(relative(&root, record_file).map(|r| r.display().to_string()).unwrap_or_default()), checks: Vec::new(), postconditions: Vec::new(), listed: Vec::new(), generated: Vec::new(), ..rec.clone() };
    let file = record_file.with_file_name(format!("{}.revert.json", record_file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()));
    std::fs::write(&file, serde_json::to_string_pretty(&out)? + "\n")?;
    let said = vec![format!("reverted {}: {} files back as they were; passenger {} → {}", rec.spec_name, out.files.len(), before.id, after.id)];
    Ok(Outcome { root, record: out, record_file: Some(file), edits, exit_code: 0, said })
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
}

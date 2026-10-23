//! Applying a klepto `GatePlan` to a tree (rules A1–A4 in `GATE_RULES.md`).
//! Pure until `EditSet::write`; the oracle uses `write_to` on a scratch copy.
//!
//! Out of scope: edits inside macro bodies; carriers the planner could not
//! name (they are listed under `unknown`, a person decides).

use crate::error::{PassengerError, Result};
use klepto::{GateEdit, GatePlan, ManifestEdit};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEdit {
    /// Absolute path in the tree the plan was computed on.
    pub path: PathBuf,
    pub before: String,
    pub after: String,
}

/// Everything a plan changes (rule A3).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EditSet {
    pub files: Vec<FileEdit>,
    pub manifest: Option<FileEdit>,
}

impl EditSet {
    pub fn is_empty(&self) -> bool {
        self.files.iter().all(|f| f.before == f.after) && self.manifest.as_ref().is_none_or(|m| m.before == m.after)
    }

    fn all(&self) -> impl Iterator<Item = &FileEdit> {
        self.files.iter().chain(self.manifest.iter())
    }

    /// Unified diffs, one per changed file, in path order, with absolute names.
    pub fn diff(&self) -> String {
        self.diff_named(|p| p.to_string_lossy().into_owned())
    }

    /// Unified diffs with names relative to `root` (`a/app/src/lib.rs`).
    pub fn diff_from(&self, root: &Path) -> String {
        self.diff_named(|p| {
            relative(root, p)
                .map(|r| format!("a/{}", r.to_string_lossy()))
                .unwrap_or_else(|_| p.to_string_lossy().into_owned())
        })
    }

    fn diff_named(&self, name: impl Fn(&Path) -> String) -> String {
        let mut edits: Vec<&FileEdit> = self.all().filter(|f| f.before != f.after).collect();
        edits.sort_by(|a, b| a.path.cmp(&b.path));
        let mut out = String::new();
        for e in edits {
            let n = name(&e.path);
            let d = similar::TextDiff::from_lines(&e.before, &e.after);
            out.push_str(&d.unified_diff().header(&n, &n.replacen("a/", "b/", 1)).to_string());
        }
        out
    }

    /// Writes every `after` in place.
    pub fn write(&self) -> Result<()> {
        for e in self.all() {
            // a file a change makes may sit in a folder no file has made yet (`src/<module>/<name>.rs`)
            if e.before != e.after || !e.path.exists() {
                if let Some(d) = e.path.parent().filter(|d| !d.as_os_str().is_empty()) {
                    std::fs::create_dir_all(d)?;
                }
                std::fs::write(&e.path, &e.after)?;
            }
        }
        Ok(())
    }

    /// Writes the same contents into another tree with the same layout;
    /// returns the relative paths written.
    pub fn write_to(&self, from_root: &Path, to_root: &Path) -> Result<Vec<PathBuf>> {
        let mut written = Vec::new();
        for e in self.all() {
            let rel = relative(from_root, &e.path)?;
            let target = to_root.join(&rel);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&target, &e.after)?;
            written.push(rel);
        }
        Ok(written)
    }
}

/// `path` relative to `root`, trying the root as given and canonicalised.
pub(crate) fn relative(root: &Path, path: &Path) -> Result<PathBuf> {
    if let Ok(r) = path.strip_prefix(root) {
        return Ok(r.to_path_buf());
    }
    std::fs::canonicalize(root)
        .ok()
        .and_then(|c| path.strip_prefix(&c).ok().map(Path::to_path_buf))
        .ok_or_else(|| PassengerError::path(format!("{} is not under {}", path.display(), root.display())))
}

fn skip_dir(name: &OsStr) -> bool {
    name == "target" || name == ".git" || name == ".passenger"
}

/// Rule A2: the manifest of `package` in the workspace at `root`, as cargo
/// metadata names it (klepto's `PackageInfo::manifest_path`).
pub fn find_manifest(root: &Path, package: &str) -> Result<PathBuf> {
    let b = crate::cargo::builder(root)?;
    crate::cargo::manifest_of(b.packages(), package)
        .ok_or_else(|| PassengerError::unsupported(format!("no workspace member named `{package}` under {}", root.display())))
}

/// Rules A1–A2.
pub fn apply(root: &Path, plan: &GatePlan) -> Result<EditSet> {
    let manifest = if plan.manifest.is_empty() { None } else { Some(find_manifest(root, &plan.package)?) };
    apply_with(plan, manifest.as_deref())
}

/// Rules A1–A2 with the package's manifest already known (`None` when the
/// plan edits none).
pub fn apply_with(plan: &GatePlan, manifest: Option<&Path>) -> Result<EditSet> {
    let mut by_file: BTreeMap<PathBuf, Vec<&GateEdit>> = BTreeMap::new();
    for g in &plan.gates {
        by_file.entry(g.location.path.clone()).or_default().push(g);
    }
    let mut files = Vec::new();
    for (path, mut gates) in by_file {
        let before = std::fs::read_to_string(&path)?;
        let after = gate_file(&before, &mut gates);
        files.push(FileEdit { path, before, after });
    }
    let manifest = match (plan.manifest.is_empty(), manifest) {
        (true, _) => None,
        (false, None) => return Err(PassengerError::unsupported(format!("the plan edits the manifest of `{}`, and none is known", plan.package))),
        (false, Some(path)) => {
            let before = std::fs::read_to_string(path)?;
            let after = edit_manifest(&before, &plan.manifest)?;
            Some(FileEdit { path: path.to_path_buf(), before, after })
        }
    };
    Ok(EditSet { files, manifest })
}

fn is_doc_line(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("///") || t.starts_with("//!")
}

/// Rule A1: the attribute that replaces a carrier's own `#[cfg]` — the
/// planner's predicate is what to *add*, so the existing condition is kept
/// by conjunction (`#[cfg(unix)]` + `feature = "b"` → `all(unix, feature = "b")`).
pub fn merged_attribute(existing: &klepto::CfgPredicate, predicate: &klepto::CfgPredicate) -> String {
    let merged = existing.clone().and(predicate.clone()).simplify();
    format!("#[cfg({})]", merged.render())
}

/// A carrier's leading lines from its start (0-based indexes): its doc
/// comments and attributes, multi-line attributes tracked by bracket
/// balance, up to the declaration line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Leading {
    /// Every attribute, as `[start, end)` line ranges, in order.
    pub attrs: Vec<(usize, usize)>,
    /// The first `#[cfg(` attribute's line.
    pub cfg: Option<usize>,
    /// The declaration line (where an attribute is inserted above).
    pub decl: usize,
}

/// The one placement rule (A1): `gate_file` and `attr place` both walk a
/// carrier this way.
pub fn leading(lines: &[String], start: usize) -> Leading {
    let mut out = Leading { attrs: Vec::new(), cfg: None, decl: start };
    let mut j = start;
    while j < lines.len() {
        let t = lines[j].trim_start();
        if is_doc_line(t) {
            j += 1;
            continue;
        }
        if !t.starts_with("#[") {
            break;
        }
        if t.starts_with("#[cfg(") && out.cfg.is_none() {
            out.cfg = Some(j);
        }
        // a (possibly multi-line) attribute by bracket balance
        let begin = j;
        let mut depth: i32 = 0;
        loop {
            for ch in lines[j].chars() {
                match ch {
                    '[' => depth += 1,
                    ']' => depth -= 1,
                    _ => {}
                }
            }
            j += 1;
            if depth <= 0 || j >= lines.len() {
                break;
            }
        }
        out.attrs.push((begin, j));
    }
    out.decl = j.min(lines.len());
    out
}

/// The indentation of line `at` (the line inserted there takes it).
pub fn indent_at(lines: &[String], at: usize) -> String {
    lines.get(at).map(|l| l.chars().take_while(|c| c.is_whitespace()).collect()).unwrap_or_default()
}

/// Rule A1. The carrier's leading lines are its doc comments and
/// attributes ([`leading`]); its first `#[cfg(` is replaced by the merged
/// condition, otherwise the attribute is inserted right above the
/// declaration line.
fn gate_file(before: &str, gates: &mut [&GateEdit]) -> String {
    let had_trailing_newline = before.ends_with('\n');
    let mut lines: Vec<String> = before.lines().map(str::to_string).collect();
    gates.sort_by(|a, b| b.location.line.cmp(&a.location.line).then_with(|| a.item.cmp(&b.item)));
    let mut done: BTreeSet<u32> = BTreeSet::new();
    for g in gates.iter() {
        let Some(line) = g.location.line else { continue };
        if !done.insert(line) {
            continue;
        }
        let start = (line as usize).saturating_sub(1);
        if start >= lines.len() {
            continue;
        }
        let lead = leading(&lines, start);
        match lead.cfg {
            Some(c) => {
                let indent = indent_at(&lines, c);
                lines[c] = format!("{indent}{}", merged_attribute(&g.existing_cfg, &g.predicate));
            }
            None => {
                let indent = indent_at(&lines, lead.decl);
                lines.insert(lead.decl, format!("{indent}{}", g.attribute_text()));
            }
        }
    }
    let mut out = lines.join("\n");
    if had_trailing_newline {
        out.push('\n');
    }
    out
}

fn string_array(items: &[String]) -> toml_edit::Array {
    let mut a = toml_edit::Array::new();
    for s in items {
        a.push(s.as_str());
    }
    a
}

/// Rule A2.
fn edit_manifest(before: &str, edits: &[ManifestEdit]) -> Result<String> {
    let mut doc: toml_edit::DocumentMut = before.parse().map_err(|e: toml_edit::TomlError| PassengerError::toml(e.to_string()))?;
    for e in edits {
        match e {
            ManifestEdit::MakeOptional { dep } => {
                if crate::commands::edit::set_optional_in(&mut doc, dep, true) == 0 {
                    return Err(PassengerError::unsupported(format!("dependency `{dep}` is not declared in the manifest")));
                }
            }
            ManifestEdit::SetFeature { name, enables } => crate::commands::edit::set_feature_in(&mut doc, name, enables)?,
            ManifestEdit::SetDefault { features } => {
                if !doc.contains_key("features") {
                    doc["features"] = toml_edit::table();
                }
                let feats = doc["features"]
                    .as_table_like_mut()
                    .ok_or_else(|| PassengerError::toml("[features] is not a table"))?;
                feats.insert("default", toml_edit::value(string_array(features)));
            }
        }
    }
    Ok(doc.to_string())
}

/// Rule O1: the cargo workspace `root` belongs to — `root` itself, or the
/// parent workspace when `root` is a member package (`cargo locate-project
/// --workspace`). The scratch copy must hold the whole workspace or cargo
/// refuses to build the member.
pub fn workspace_root(root: &Path) -> Result<PathBuf> {
    // James's battery rule, before any cargo line
    custodian_kit::power::cargo_may_start()?;
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = std::process::Command::new(cargo)
        .args(["locate-project", "--workspace", "--message-format", "plain"])
        .current_dir(root)
        .output()?;
    if !out.status.success() {
        // cargo could not load the workspace (a manifest mid-change): the governing root by cargo's rule, as
        // custodian_kit's requirements reading reads it, so a proof's copy carries the root cargo will look for
        return Ok(custodian_kit::reading::requirements::manifest::governing_root_of(root).unwrap_or_else(|| root.to_path_buf()));
    }
    let manifest = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let ws = Path::new(&manifest).parent().map(Path::to_path_buf).unwrap_or_else(|| root.to_path_buf());
    // prefer the spelling the caller used when it is the same directory
    let same = std::fs::canonicalize(&ws).ok() == std::fs::canonicalize(root).ok();
    Ok(if same { root.to_path_buf() } else { ws })
}

/// Rule A4: the tree minus `target`, `.git` and `.passenger` at any depth;
/// symlinks are not followed.
pub fn scratch_copy(root: &Path) -> Result<tempfile::TempDir> {
    let dir = tempfile::tempdir()?;
    let walk = WalkDir::new(root).into_iter().filter_entry(|e| !(e.depth() > 0 && e.file_type().is_dir() && skip_dir(e.file_name())));
    for entry in walk.filter_map(|e| e.ok()) {
        if entry.depth() == 0 {
            continue;
        }
        let ft = entry.file_type();
        if ft.is_symlink() {
            continue;
        }
        let rel = entry.path().strip_prefix(root).map_err(|e| PassengerError::path(e.to_string()))?;
        let target = dir.path().join(rel);
        if ft.is_dir() {
            std::fs::create_dir_all(&target)?;
        } else if ft.is_file() {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(dir)
}

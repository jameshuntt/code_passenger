//! `dep add | remove` and `attr place | remove`: the two writers custodian
//! routes to (custodian never edits a manifest or a source file itself).
//!
//! `dep add <crate> <dep>[@version] [--path P] [--features a,b] [--workspace-table]`
//! adds the dependency to the crate's `[dependencies]` through toml_edit (the
//! formatting kept), or updates it: the version replaced, the features
//! added. With `--workspace-table` the version (or path) goes into the root
//! manifest's `[workspace.dependencies]` and the crate names it
//! `{ workspace = true }`, its features at the crate. `dep remove <crate>
//! <dep> [--workspace-table]` takes it out of the crate (and out of the
//! workspace table once no member names it); a feature that still names the
//! dependency refuses. The crate's manifest is cargo metadata's (klepto's
//! packages).
//!
//! `attr place --file F --item ID --line '#[attr(...)]'` places one
//! attribute line on an item at klepto's location of it, by the placement
//! rule the gates use (`crate::gate::leading`): a `#[cfg(...)]` merges into
//! the item's own cfg (`all(old, new)`) or goes right above the
//! declaration; any other attribute goes above the item's first attribute
//! (after its doc comments). The same line already there is nothing to do.
//! `attr remove` takes that exact line out of the item's attributes. An item
//! klepto does not name in that file refuses.

use crate::error::{PassengerError, Result};
use crate::gate::{indent_at, leading, relative, workspace_root, EditSet, FileEdit};
use std::path::{Path, PathBuf};

/// What an edit did: the file it changed (before and after), or nothing.
#[derive(Debug)]
pub struct Edited {
    pub root: PathBuf,
    pub edits: EditSet,
    pub written: bool,
    /// One line on what happened.
    pub said: String,
}

impl Edited {
    pub fn render(&self) -> String {
        let mut s = format!("{}\n", self.said);
        if !self.edits.is_empty() {
            s.push_str(&self.edits.diff_from(&self.root));
        }
        s.push_str(&format!("written: {}\n", if self.written { "yes" } else { "no" }));
        s
    }
}

fn finish(root: &Path, path: PathBuf, before: String, after: String, said: String) -> Result<Edited> {
    let edit = FileEdit { path, before, after };
    let edits = EditSet { files: vec![edit], manifest: None };
    let written = !edits.is_empty();
    if written {
        edits.write()?;
    }
    Ok(Edited { root: root.to_path_buf(), edits, written, said })
}

// ---------------------------------------------------------------------------
// dep

/// `<dep>[@version]`.
pub fn split_spec(spec: &str) -> (String, Option<String>) {
    match spec.split_once('@') {
        Some((d, v)) if !v.is_empty() => (d.to_string(), Some(v.to_string())),
        _ => (spec.trim_end_matches('@').to_string(), None),
    }
}

fn manifest_of(root: &Path, krate: &str) -> Result<PathBuf> {
    let b = crate::cargo::builder(root)?;
    crate::cargo::manifest_of(b.packages(), krate).ok_or_else(|| {
        let names: Vec<&String> = b.packages().keys().collect();
        PassengerError::UnknownName { detail: format!("no workspace member `{krate}` (members: {names:?})") }
    })
}

fn parse(path: &Path) -> Result<(String, toml_edit::DocumentMut)> {
    let text = std::fs::read_to_string(path)?;
    let doc = text.parse::<toml_edit::DocumentMut>().map_err(|e| PassengerError::toml(format!("{}: {e}", path.display())))?;
    Ok((text, doc))
}

fn features_array(t: &mut dyn toml_edit::TableLike, features: &[String]) {
    if features.is_empty() {
        return;
    }
    let mut arr = t.get("features").and_then(|f| f.as_array()).cloned().unwrap_or_default();
    for f in features {
        if !arr.iter().any(|x| x.as_str() == Some(f.as_str())) {
            arr.push(f.as_str());
        }
    }
    t.insert("features", toml_edit::value(arr));
}

/// An inline entry's spacing written as cargo writes it after a key is added or set: `{ version = "1", features =
/// ["derive"], optional = true }`, never the `["derive"] , optional` a key appended after the last value's own
/// spacing reads as. A table entry (`[dependencies.x]`) keeps its lines as they are.
pub fn tidy_entry(item: &mut toml_edit::Item) {
    if let Some(t) = item.as_inline_table_mut() {
        t.fmt();
    }
}

/// The entry as an inline table, a bare version string turned into `{ version = .. }`.
fn as_table(item: &mut toml_edit::Item) -> &mut dyn toml_edit::TableLike {
    if let Some(v) = item.as_str().map(str::to_string) {
        let mut t = toml_edit::InlineTable::new();
        t.insert("version", toml_edit::Value::from(v.as_str()));
        *item = toml_edit::value(t);
    }
    if item.is_none() {
        *item = toml_edit::value(toml_edit::InlineTable::new());
    }
    item.as_table_like_mut().expect("a dependency entry is a table here")
}

fn set_source(t: &mut dyn toml_edit::TableLike, version: Option<&str>, path: Option<&str>) {
    if let Some(v) = version {
        t.insert("version", toml_edit::value(v));
    }
    if let Some(p) = path {
        t.insert("path", toml_edit::value(p));
    }
}

fn deps_table(doc: &mut toml_edit::DocumentMut) -> &mut dyn toml_edit::TableLike {
    if !doc.contains_key("dependencies") {
        doc["dependencies"] = toml_edit::table();
    }
    doc["dependencies"].as_table_like_mut().expect("[dependencies] is a table")
}

/// A bare `dep = "1.0"` stays bare when only its version is set.
fn plain_version(item: &toml_edit::Item) -> bool {
    item.as_str().is_some()
}

/// `dep add`.
pub fn dep_add(root: &Path, krate: &str, spec: &str, path_dep: Option<&str>, features: &[String], workspace_table: bool) -> Result<Edited> {
    let e = dep_add_plan(root, krate, spec, path_dep, features, workspace_table)?;
    if e.written {
        e.edits.write()?;
    }
    Ok(e)
}

/// `dep add`'s edits, not written (`written` says whether there are any): what a transform proves before it writes.
pub fn dep_add_plan(root: &Path, krate: &str, spec: &str, path_dep: Option<&str>, features: &[String], workspace_table: bool) -> Result<Edited> {
    let root = workspace_root(root)?;
    let (name, version) = split_spec(spec);
    if name.is_empty() {
        return Err(PassengerError::DepVersion { detail: format!("`{spec}` names no dependency") });
    }
    let manifest = manifest_of(&root, krate)?;
    if workspace_table {
        let root_manifest = root.join("Cargo.toml");
        let (rbefore, mut rdoc) = parse(&root_manifest)?;
        let Some(ws) = rdoc.get_mut("workspace").and_then(|w| w.as_table_like_mut()) else {
            return Err(PassengerError::NoWorkspaceTable { detail: root_manifest.display().to_string() });
        };
        if !ws.contains_key("dependencies") {
            ws.insert("dependencies", toml_edit::table());
        }
        let wdeps = ws.get_mut("dependencies").and_then(|d| d.as_table_like_mut()).expect("[workspace.dependencies] is a table");
        let had = wdeps.contains_key(&name);
        if !had && version.is_none() && path_dep.is_none() {
            return Err(PassengerError::DepVersion { detail: format!("`{name}` is not in [workspace.dependencies]; give {name}@<version> or --path") });
        }
        if version.is_some() || path_dep.is_some() {
            let entry = wdeps.entry(&name).or_insert(toml_edit::Item::None);
            if path_dep.is_none() && (entry.is_none() || plain_version(entry)) {
                *entry = toml_edit::value(version.as_deref().unwrap_or_default());
            } else {
                set_source(as_table(entry), version.as_deref(), path_dep);
            }
        }
        let rafter = rdoc.to_string();
        let (before, mut doc) = parse(&manifest)?;
        let deps = deps_table(&mut doc);
        let entry = deps.entry(&name).or_insert(toml_edit::Item::None);
        let t = as_table(entry);
        t.remove("version");
        t.remove("path");
        t.insert("workspace", toml_edit::value(true));
        features_array(t, features);
        tidy_entry(entry);
        let after = doc.to_string();
        let mut files = Vec::new();
        if rafter != rbefore {
            files.push(FileEdit { path: root_manifest, before: rbefore, after: rafter });
        }
        if after != before {
            files.push(FileEdit { path: manifest, before, after });
        }
        let edits = EditSet { files, manifest: None };
        let written = !edits.is_empty();
        let said = if written { format!("added {name} to {krate} through [workspace.dependencies]") } else { format!("{name} is already in {krate} through [workspace.dependencies]: nothing to do") };
        return Ok(Edited { root, edits, written, said });
    }
    if version.is_none() && path_dep.is_none() {
        let (_, doc) = parse(&manifest)?;
        let present = doc.get("dependencies").and_then(|d| d.as_table_like()).is_some_and(|d| d.contains_key(&name));
        if !present {
            return Err(PassengerError::DepVersion { detail: format!("give {name}@<version> or --path") });
        }
    }
    let (before, mut doc) = parse(&manifest)?;
    let deps = deps_table(&mut doc);
    let existed = deps.contains_key(&name);
    let entry = deps.entry(&name).or_insert(toml_edit::Item::None);
    if path_dep.is_none() && features.is_empty() && (entry.is_none() || plain_version(entry)) {
        if let Some(v) = &version {
            *entry = toml_edit::value(v.as_str());
        }
    } else {
        let t = as_table(entry);
        set_source(t, version.as_deref(), path_dep);
        features_array(t, features);
        tidy_entry(entry);
    }
    let after = doc.to_string();
    let said = match (existed, after != before) {
        (_, false) => format!("{name} is already in {krate} as asked: nothing to do"),
        (false, true) => format!("added {name} to {krate}"),
        (true, true) => format!("updated {name} in {krate}"),
    };
    let edits = EditSet { files: vec![FileEdit { path: manifest, before, after }], manifest: None };
    let written = !edits.is_empty();
    Ok(Edited { root, edits, written, said })
}

/// The dependency tables of a manifest that can name a dependency: `[dependencies]`, `[dev-dependencies]`,
/// `[build-dependencies]`, and each `[target.<cfg>.*]` of them.
const DEP_TABLES: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

fn same_crate(a: &str, b: &str) -> bool {
    a.replace('-', "_") == b.replace('-', "_")
}

/// One entry changed to the source asked: a version alone is the registry's (the path dropped; a bare `"x.y"` when
/// nothing else is on the entry), a path with or without a version; features given replace its features. Refused
/// for an entry that takes its source from `[workspace.dependencies]`.
fn change_entry(entry: &mut toml_edit::Item, version: Option<&str>, path: Option<&str>, features: &[String]) -> std::result::Result<(), String> {
    if entry.as_table_like().is_some_and(|t| t.get("workspace").and_then(|w| w.as_bool()) == Some(true)) {
        return Err("its source is the workspace table's (`workspace = true`): change it there".into());
    }
    let others = entry.as_table_like().map(|t| t.iter().filter(|(k, _)| !matches!(*k, "version" | "path")).count()).unwrap_or(0);
    if let (None, true, 0, Some(v)) = (path, features.is_empty(), others, version) {
        let decor = entry.as_value().map(|x| x.decor().clone());
        *entry = toml_edit::value(v);
        if let (Some(d), Some(x)) = (decor, entry.as_value_mut()) {
            *x.decor_mut() = d;
        }
        return Ok(());
    }
    let t = as_table(entry);
    match version {
        Some(v) => {
            t.insert("version", toml_edit::value(v));
        }
        None => {
            t.remove("version");
        }
    }
    match path {
        Some(p) => {
            t.insert("path", toml_edit::value(p));
        }
        None => {
            t.remove("path");
        }
    }
    if !features.is_empty() {
        let mut arr = toml_edit::Array::new();
        for f in features {
            arr.push(f.as_str());
        }
        t.insert("features", toml_edit::value(arr));
    }
    tidy_entry(entry);
    Ok(())
}

/// A manifest's text with every entry naming `name` (its key, or its `package = ..`) in its dependency tables
/// changed to the source asked ([`change_entry`]); the text after and how many entries it changed.
pub fn dep_change_text(text: &str, name: &str, version: Option<&str>, path: Option<&str>, features: &[String]) -> Result<(String, usize)> {
    if version.is_none() && path.is_none() {
        return Err(PassengerError::DepVersion { detail: format!("change {name} to what: give its version, its path, or both") });
    }
    dep_change_text_with(text, name, version, path, features, None)
}

/// [`dep_change_text`], with `optional` set too (`Some(true)` optional, `Some(false)` required again); a change of
/// `optional` alone keeps the entry's source.
pub fn dep_change_text_with(text: &str, name: &str, version: Option<&str>, path: Option<&str>, features: &[String], optional: Option<bool>) -> Result<(String, usize)> {
    if version.is_none() && path.is_none() && optional.is_none() {
        return Err(PassengerError::DepVersion { detail: format!("change {name} to what: give its version, its path, optional, or more than one") });
    }
    let mut doc = text.parse::<toml_edit::DocumentMut>().map_err(|e| PassengerError::toml(e.to_string()))?;
    let mut changed = 0;
    let mut change_in = |deps: &mut dyn toml_edit::TableLike| -> Result<()> {
        let keys: Vec<String> = deps.iter().filter(|(k, v)| same_crate(k, name) || v.as_table_like().and_then(|t| t.get("package")).and_then(|p| p.as_str()).is_some_and(|p| same_crate(p, name))).map(|(k, _)| k.to_string()).collect();
        for k in keys {
            let entry = deps.get_mut(&k).expect("the key was just listed");
            if version.is_some() || path.is_some() {
                change_entry(entry, version, path, features).map_err(|why| PassengerError::DepVersion { detail: format!("{k}: {why}") })?;
            }
            changed += 1;
        }
        Ok(())
    };
    for table in DEP_TABLES {
        if let Some(deps) = doc.get_mut(table).and_then(|d| d.as_table_like_mut()) {
            change_in(deps)?;
        }
    }
    if let Some(targets) = doc.get_mut("target").and_then(|t| t.as_table_like_mut()) {
        for (_, cfg) in targets.iter_mut() {
            for table in DEP_TABLES {
                if let Some(deps) = cfg.as_table_like_mut().and_then(|c| c.get_mut(table)).and_then(|d| d.as_table_like_mut()) {
                    change_in(deps)?;
                }
            }
        }
    }
    if let Some(o) = optional {
        set_optional_in(&mut doc, name, o);
    }
    Ok((doc.to_string(), changed))
}

/// Every entry naming `dep` (its key, `-` and `_` one) in a manifest's dependency tables and their `[target.*]`
/// forms made optional, or required again (`optional = false` drops the key); how many entries it set.
pub fn set_optional_in(doc: &mut toml_edit::DocumentMut, dep: &str, optional: bool) -> usize {
    let mut hit = 0;
    let mut visit = |tbl: &mut dyn toml_edit::TableLike| {
        let keys: Vec<String> = tbl.iter().map(|(k, _)| k.to_string()).filter(|k| same_crate(k, dep)).collect();
        for key in keys {
            let Some(item) = tbl.get_mut(&key) else { continue };
            if optional {
                let t = as_table(item);
                t.insert("optional", toml_edit::value(true));
            } else if let Some(t) = item.as_table_like_mut() {
                t.remove("optional");
            }
            tidy_entry(item);
            hit += 1;
        }
    };
    for table in DEP_TABLES {
        if let Some(t) = doc.get_mut(table).and_then(|i| i.as_table_like_mut()) {
            visit(t);
        }
    }
    if let Some(targets) = doc.get_mut("target").and_then(|i| i.as_table_like_mut()) {
        for (_, cfg) in targets.iter_mut() {
            for table in DEP_TABLES {
                if let Some(t) = cfg.as_table_like_mut().and_then(|c| c.get_mut(table)).and_then(|d| d.as_table_like_mut()) {
                    visit(t);
                }
            }
        }
    }
    hit
}

/// `[features] name = [enables]` set in a manifest (the table made when the crate has none, the feature's list
/// replaced when it has one).
pub fn set_feature_in(doc: &mut toml_edit::DocumentMut, name: &str, enables: &[String]) -> Result<()> {
    if !doc.contains_key("features") {
        doc["features"] = toml_edit::table();
    }
    let feats = doc["features"].as_table_like_mut().ok_or_else(|| PassengerError::toml("[features] is not a table"))?;
    let mut arr = toml_edit::Array::new();
    for e in enables {
        arr.push(e.as_str());
    }
    feats.insert(name, toml_edit::value(arr));
    Ok(())
}

/// `declare_feature`'s manifest: each dependency in `optional` made optional (a `dep:` entry takes only an optional
/// dependency), then `[features] name = [enables]`; the text after.
pub fn feature_declare_text(text: &str, name: &str, enables: &[String], optional: &[String]) -> Result<String> {
    let mut doc = text.parse::<toml_edit::DocumentMut>().map_err(|e| PassengerError::toml(e.to_string()))?;
    for d in optional {
        if set_optional_in(&mut doc, d, true) == 0 {
            return Err(PassengerError::unsupported(format!("dependency `{d}` is not declared in the manifest")));
        }
    }
    set_feature_in(&mut doc, name, enables)?;
    Ok(doc.to_string())
}

/// A dependency's source as a manifest names it in its first dependency table that has it: (version, path).
pub fn dep_source(text: &str, name: &str) -> Option<(Option<String>, Option<String>)> {
    let doc = text.parse::<toml_edit::DocumentMut>().ok()?;
    let tables = DEP_TABLES.iter().filter_map(|t| doc.get(t).and_then(|d| d.as_table_like())).collect::<Vec<_>>();
    for deps in tables {
        for (k, v) in deps.iter() {
            let pkg = v.as_table_like().and_then(|t| t.get("package")).and_then(|p| p.as_str());
            if same_crate(k, name) || pkg.is_some_and(|p| same_crate(p, name)) {
                return Some(match v.as_str() {
                    Some(ver) => (Some(ver.to_string()), None),
                    None => {
                        let t = v.as_table_like()?;
                        (t.get("version").and_then(|x| x.as_str()).map(str::to_string), t.get("path").and_then(|x| x.as_str()).map(str::to_string))
                    }
                });
            }
        }
    }
    None
}

/// Whether a feature list names `dep` (`dep:x`, `x/f`, `x?/f`, `x`).
fn named_by_features(doc: &toml_edit::DocumentMut, dep: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(feats) = doc.get("features").and_then(|f| f.as_table_like()) {
        for (fname, list) in feats.iter() {
            let names = list.as_array().map(|a| a.iter().filter_map(|v| v.as_str()).any(|e| {
                let e = e.strip_prefix("dep:").unwrap_or(e);
                let head = e.split('/').next().unwrap_or(e).trim_end_matches('?');
                head == dep
            }));
            if names == Some(true) {
                out.push(fname.to_string());
            }
        }
    }
    out
}

/// `dep remove`.
pub fn dep_remove(root: &Path, krate: &str, spec: &str, workspace_table: bool) -> Result<Edited> {
    let root = workspace_root(root)?;
    let (name, _) = split_spec(spec);
    let manifest = manifest_of(&root, krate)?;
    let (before, mut doc) = parse(&manifest)?;
    let naming = named_by_features(&doc, &name);
    if !naming.is_empty() {
        return Err(PassengerError::DepInUse { detail: format!("{krate}: feature(s) {} name {name}", naming.join(", ")) });
    }
    let removed = doc.get_mut("dependencies").and_then(|d| d.as_table_like_mut()).and_then(|d| d.remove(&name)).is_some();
    let after = doc.to_string();
    let mut files = Vec::new();
    if after != before {
        files.push(FileEdit { path: manifest.clone(), before, after });
    }
    let mut said = if removed { format!("removed {name} from {krate}") } else { format!("{krate} has no dependency {name}: nothing to do") };
    if workspace_table {
        // out of the workspace table once no other member names it
        let b = crate::cargo::builder(&root)?;
        let mut still = Vec::new();
        for p in b.packages().values().filter(|p| p.manifest_path != manifest) {
            let (_, other) = parse(&p.manifest_path)?;
            if other.get("dependencies").and_then(|d| d.as_table_like()).is_some_and(|d| d.contains_key(&name)) {
                still.push(p.name.clone());
            }
        }
        let root_manifest = root.join("Cargo.toml");
        let (rbefore, mut rdoc) = parse(&root_manifest)?;
        if still.is_empty() {
            if let Some(w) = rdoc.get_mut("workspace").and_then(|w| w.get_mut("dependencies")).and_then(|d| d.as_table_like_mut()) {
                w.remove(&name);
            }
            let rafter = rdoc.to_string();
            if rafter != rbefore {
                files.push(FileEdit { path: root_manifest, before: rbefore, after: rafter });
                said.push_str(", and from [workspace.dependencies]");
            }
        } else {
            said.push_str(&format!("; [workspace.dependencies] keeps it ({} still name it)", still.join(", ")));
        }
    }
    let edits = EditSet { files, manifest: None };
    let written = !edits.is_empty();
    if written {
        edits.write()?;
    }
    Ok(Edited { root, edits, written, said })
}

// ---------------------------------------------------------------------------
// attr

/// One attribute on one line: `#[...]` balanced.
fn check_line(line: &str) -> Result<String> {
    let t = line.trim();
    let balanced = t.chars().fold(0i32, |d, c| d + match c {
        '[' => 1,
        ']' => -1,
        _ => 0,
    }) == 0;
    if !(t.starts_with("#[") && t.ends_with(']')) || !balanced || t.contains('\n') {
        return Err(PassengerError::AttrLine { detail: format!("`{line}`") });
    }
    Ok(t.to_string())
}

/// The inner condition of `#[cfg(X)]`.
fn cfg_inner(line: &str) -> Option<&str> {
    line.trim().strip_prefix("#[cfg(")?.strip_suffix(")]")
}

/// The item's 0-based start line in `file`, as klepto locates it.
fn locate(root: &Path, file: &Path, item: &str) -> Result<(PathBuf, usize)> {
    let k = crate::commands::klepto_for(root)?;
    let abs = if file.is_absolute() { file.to_path_buf() } else { root.join(file) };
    let want = abs.canonicalize().map_err(|e| PassengerError::AttrItem { detail: format!("{}: {e}", abs.display()) })?;
    let in_file = |p: &Path| p.canonicalize().ok().as_ref() == Some(&want);
    // `crate::x` names an item of the file's own package
    let package = k.modules.modules.values().find(|m| !m.inline && in_file(&m.file)).map(|m| m.package.clone());
    let id = match (item.strip_prefix("crate::"), &package) {
        (Some(rest), Some(p)) => format!("{p}::{rest}"),
        _ => item.to_string(),
    };
    let found = k.items.iter().find(|d| d.id == id && in_file(&d.location.path));
    match found.and_then(|d| d.location.line) {
        Some(line) => Ok((want, line.saturating_sub(1) as usize)),
        None => {
            let elsewhere: Vec<String> = k.items.iter().filter(|d| d.id == id).map(|d| relative(root, &d.location.path).map(|r| r.display().to_string()).unwrap_or_default()).collect();
            let hint = if elsewhere.is_empty() { "klepto names no such item".to_string() } else { format!("it is in {}", elsewhere.join(", ")) };
            Err(PassengerError::AttrItem { detail: format!("`{id}` in {}: {hint}", file.display()) })
        }
    }
}

fn join(lines: &[String], trailing: bool) -> String {
    let mut out = lines.join("\n");
    if trailing {
        out.push('\n');
    }
    out
}

/// `attr place`.
pub fn attr_place(root: &Path, file: &Path, item: &str, line: &str) -> Result<Edited> {
    let root = workspace_root(root)?;
    let attr = check_line(line)?;
    let (path, start) = locate(&root, file, item)?;
    let before = std::fs::read_to_string(&path)?;
    let mut lines: Vec<String> = before.lines().map(str::to_string).collect();
    if start >= lines.len() {
        return Err(PassengerError::AttrItem { detail: format!("{item}: line {} is past the end of {}", start + 1, path.display()) });
    }
    let lead = leading(&lines, start);
    let already = lead.attrs.iter().any(|(a, b)| b - a == 1 && lines[*a].trim() == attr);
    let said;
    if already {
        said = format!("{item} already carries {attr}: nothing to do");
    } else if let Some(new) = cfg_inner(&attr) {
        match lead.cfg {
            Some(c) if lead.attrs.iter().any(|(a, b)| *a == c && b - a == 1) => {
                let old = cfg_inner(&lines[c]).unwrap_or_default().to_string();
                let indent = indent_at(&lines, c);
                lines[c] = format!("{indent}#[cfg(all({old}, {new}))]");
                said = format!("merged {attr} into {item}'s cfg");
            }
            Some(c) => return Err(PassengerError::AttrLine { detail: format!("{item}'s cfg at line {} spans lines; merge it by hand", c + 1) }),
            None => {
                let indent = indent_at(&lines, lead.decl);
                lines.insert(lead.decl, format!("{indent}{attr}"));
                said = format!("placed {attr} on {item}");
            }
        }
    } else {
        let at = lead.attrs.first().map(|(a, _)| *a).unwrap_or(lead.decl);
        let indent = indent_at(&lines, at);
        lines.insert(at, format!("{indent}{attr}"));
        said = format!("placed {attr} on {item}");
    }
    let after = join(&lines, before.ends_with('\n'));
    finish(&root, path, before, after, said)
}

/// `attr remove`.
pub fn attr_remove(root: &Path, file: &Path, item: &str, line: &str) -> Result<Edited> {
    let root = workspace_root(root)?;
    let attr = check_line(line)?;
    let (path, start) = locate(&root, file, item)?;
    let before = std::fs::read_to_string(&path)?;
    let mut lines: Vec<String> = before.lines().map(str::to_string).collect();
    let lead = leading(&lines, start.min(lines.len()));
    let hit = lead.attrs.iter().find(|(a, b)| b - a == 1 && lines[*a].trim() == attr).map(|(a, _)| *a);
    let said = match hit {
        Some(at) => {
            lines.remove(at);
            format!("removed {attr} from {item}")
        }
        None => format!("{item} carries no {attr}: nothing to do"),
    };
    let after = join(&lines, before.ends_with('\n'));
    finish(&root, path, before, after, said)
}

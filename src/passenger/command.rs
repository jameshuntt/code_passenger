//! `passenger init | branch | checkout | checkpoint | log`, on the store of
//! the workspace `--root` belongs to.

use crate::error::{PassengerError, Result};
use crate::lines::{CodePassenger, PassengerRooted};
use crate::passenger::{CheckpointOptions, PassengerStore};
use std::path::Path;

fn os(v: &Option<std::ffi::OsString>) -> Option<String> {
    v.as_ref().map(|s| s.to_string_lossy().into_owned())
}

/// Runs the line; what it says goes into `out`; returns the exit status.
pub fn run(l: &PassengerRooted<CodePassenger>, root: &Path, out: &mut String) -> Result<i32> {
    let l = l.leaf();
    let name = os(&l.name);
    match l.action.to_string_lossy().as_ref() {
        "init" => {
            let s = PassengerStore::init(root)?;
            out.push_str(&format!("initialized {}\n", s.root.join(crate::passenger::store::FOLDER).display()));
        }
        "branch" => {
            let s = PassengerStore::open(root)?;
            let name = name.ok_or_else(|| PassengerError::Line { detail: "passenger branch NAME".into() })?;
            s.create_branch(&name, os(&l.from).as_deref())?;
            out.push_str(&format!("created branch {name}\n"));
        }
        "checkout" => {
            let s = PassengerStore::open(root)?;
            let name = name.ok_or_else(|| PassengerError::Line { detail: "passenger checkout BRANCH|SNAPSHOT".into() })?;
            let r = s.checkout(&name, l.force)?;
            out.push_str(&format!("checked out {name} ({}): {} file(s) written", r.commit.as_deref().unwrap_or("no commits yet"), r.written.len()));
            for f in &r.written {
                out.push_str(&format!("\n  wrote {f}"));
            }
            for f in &r.left {
                out.push_str(&format!("\n  left {f} (not in the snapshot)"));
            }
            out.push('\n');
        }
        "checkpoint" => {
            let s = PassengerStore::open(root)?;
            let ctx = crate::engine::RunContext { root: s.root.clone(), src_rel: String::new(), manifest_path: s.root.join("Cargo.toml"), lang: "rust".into() };
            let (scan, unparsed) = crate::engine::run_scan_skipping(&ctx)?;
            let analysis = serde_json::to_value(crate::analysis::run(&scan))?;
            let c = s.checkpoint(Some(&scan), Some(&analysis), CheckpointOptions { note: os(&l.note), branch: os(&l.branch), include_artifacts: !l.no_artifacts, unparsed })?;
            out.push_str(&format!("checkpoint {} on {}\n", c.id, c.branch));
            for (f, e) in c.manifest.files.iter().filter_map(|(f, e)| e.unparsed.as_ref().map(|why| (f, why))) {
                out.push_str(&format!("note: klepto could not parse {f}; kept by digest, marked unparsed, left out of the scan: {e}\n"));
            }
        }
        "log" => {
            let s = PassengerStore::open(root)?;
            let n: usize = os(&l.n).map(|v| v.parse().map_err(|_| PassengerError::Line { detail: format!("--n takes a number, not `{v}`") })).transpose()?.unwrap_or(25);
            let head = s.resolve_head()?;
            let mut cur = head.head_commit;
            out.push_str(&format!("== log ({}) ==\n", head.branch));
            for _ in 0..n {
                let Some(id) = cur.take() else { break };
                let c = s.read_commit(&head.passenger_version, &id)?;
                let when = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(c.ts_ms).map(|t| t.to_rfc3339()).unwrap_or_default();
                out.push_str(&format!("{}  {}  changed_files={} +{} -{}  {}\n", c.id, when, c.stats.changed_files, c.stats.added_lines, c.stats.removed_lines, c.note.clone().unwrap_or_default()));
                cur = c.parents.first().cloned();
            }
        }
        "diff" => {
            // the lines of FILE that differ from the blob --from names: what a bounded job's close lists by line
            let s = PassengerStore::open(root)?;
            let from = os(&l.from).ok_or_else(|| PassengerError::Line { detail: "passenger diff --from SHA256 --file FILE".into() })?;
            let file = os(&l.file).ok_or_else(|| PassengerError::Line { detail: "passenger diff --from SHA256 --file FILE".into() })?;
            let old = String::from_utf8(s.object(from.trim_start_matches("sha256:"))?).unwrap_or_default();
            let now = std::fs::read_to_string(s.root.join(&file)).unwrap_or_default();
            let d = similar::TextDiff::from_lines(&old, &now);
            let mut lines = Vec::new();
            for op in d.ops() {
                if !matches!(op.tag(), similar::DiffTag::Equal) {
                    lines.extend(op.new_range().map(|n| n + 1));
                }
            }
            out.push_str(&format!("lines: {}\n", lines.iter().map(usize::to_string).collect::<Vec<_>>().join(",")));
        }
        other => return Err(PassengerError::Line { detail: format!("passenger {other}") }),
    }
    Ok(0)
}

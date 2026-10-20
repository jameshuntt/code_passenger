// Copyright © 2026 1o. All rights reserved.

//! The proof's one place per workspace: a copy of its source at a path that does not change, and one build folder
//! beside it, kept between proofs. A proof syncs the copy (only the files whose bytes differ are written, so cargo's
//! fingerprints of everything else hold), writes its edits there, and runs cargo with `CARGO_TARGET_DIR` on the kept
//! build: a crate whose source did not move is not compiled again. One proof at a time holds it (a lock file, let go
//! when the proof ends, by the OS if it dies).
//!
//! The place is `$CODE_PASSENGER_PROOF_DIR` when set, else `<temp>/code_passenger-proof/<name>-<digest of the
//! workspace's path>`: `src/` the copy, `target/` the build, `workspace` the path it copies. Never inside the workspace's
//! `target/`: klepto reads no source under a `target` folder, so the proof's scan would find nothing there. A place
//! whose workspace is gone (a test's fixture, a removed worktree) is taken away when another is taken.

use crate::error::{PassengerError, Result};
use crate::gate::EditSet;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// One run of `cargo test -p <package>` on the copy.
#[derive(Debug, Clone)]
pub struct TestRun {
    pub code: Option<i32>,
    /// Its `test result:` lines.
    pub results: Vec<String>,
    /// The tests it names `FAILED`.
    pub failing: Vec<String>,
    /// Its last words on stderr, for a run that built nothing to test.
    pub tail: String,
}

/// A crate's tests after a change, against the same tests before it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Compared {
    pub exit: Option<i32>,
    pub results: Vec<String>,
    /// Failing after, and the same before: not the change's (on the Mac, a crate whose tests need root or systemd).
    pub failing_at_base: Vec<String>,
    /// Failing after, passing before: any one stops the change.
    pub new_failures: Vec<String>,
}

impl Compared {
    pub fn ok(&self) -> bool {
        self.exit == Some(0) || (self.exit.is_some() && self.new_failures.is_empty())
    }
}

/// What every cargo line of a proof runs with besides its build folder: no debug info (a proof runs nothing under a
/// debugger, and debug info is most of a test build's bytes).
pub const PROOF_ENV: &[(&str, &str)] = &[("CARGO_PROFILE_DEV_DEBUG", "0"), ("CARGO_PROFILE_TEST_DEBUG", "0")];

/// The variable naming the proof's place.
pub const PROOF_DIR_ENV: &str = "CODE_PASSENGER_PROOF_DIR";

/// The proof's place for one workspace, held.
pub struct ProofDir {
    workspace: PathBuf,
    src: PathBuf,
    target: PathBuf,
    /// Held while the proof runs.
    _lock: std::fs::File,
}

/// The proof places under `all` whose workspace no longer exists, taken away (each held first, so one in use stays).
fn prune(all: &Path) {
    let Ok(entries) = std::fs::read_dir(all) else { return };
    for e in entries.flatten() {
        let dir = e.path();
        let Ok(ws) = std::fs::read_to_string(dir.join("workspace")) else { continue };
        if Path::new(ws.trim()).exists() {
            continue;
        }
        let Ok(lock) = std::fs::OpenOptions::new().write(true).open(dir.join("lock")) else { continue };
        if lock.try_lock().is_ok() {
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

/// What the copy leaves out: the builds, git, the passenger store.
fn skip_dir(name: &std::ffi::OsStr) -> bool {
    name == "target" || name == ".git" || name == ".passenger"
}

impl ProofDir {
    /// The place for `workspace` (its root), held until dropped, the copy synced with the workspace as it is.
    pub fn take(workspace: &Path) -> Result<Self> {
        let workspace = workspace.canonicalize().unwrap_or_else(|_| workspace.to_path_buf());
        let base = match std::env::var_os(PROOF_DIR_ENV) {
            Some(d) => PathBuf::from(d),
            None => {
                let all = std::env::temp_dir().join("code_passenger-proof");
                prune(&all);
                let key = stable_digest::StableDigest::sha256(workspace.to_string_lossy().as_bytes()).as_str().chars().take(16).collect::<String>();
                let name = workspace.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                all.join(format!("{name}-{key}"))
            }
        };
        std::fs::create_dir_all(&base)?;
        std::fs::write(base.join("workspace"), workspace.to_string_lossy().as_bytes())?;
        let lock = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(base.join("lock"))?;
        lock.lock().map_err(|e| PassengerError::Io { detail: format!("{}: {e}", base.join("lock").display()) })?;
        let me = ProofDir { workspace, src: base.join("src"), target: base.join("target"), _lock: lock };
        me.sync()?;
        Ok(me)
    }

    /// The copy of the workspace's root.
    pub fn root(&self) -> &Path {
        &self.src
    }

    /// The kept build folder: every cargo line of a proof runs with it as `CARGO_TARGET_DIR`.
    pub fn target(&self) -> &Path {
        &self.target
    }

    /// The copy made the same as the workspace: a file whose bytes differ written, a file the workspace no longer
    /// has removed, nothing else touched.
    pub fn sync(&self) -> Result<()> {
        let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
        let walk = WalkDir::new(&self.workspace).into_iter().filter_entry(|e| !(e.depth() > 0 && e.file_type().is_dir() && skip_dir(e.file_name())));
        for entry in walk.filter_map(|e| e.ok()) {
            if entry.depth() == 0 || entry.file_type().is_symlink() {
                continue;
            }
            let rel = entry.path().strip_prefix(&self.workspace).map_err(|e| PassengerError::path(e.to_string()))?.to_path_buf();
            let to = self.src.join(&rel);
            if entry.file_type().is_dir() {
                std::fs::create_dir_all(&to)?;
            } else if entry.file_type().is_file() {
                let bytes = std::fs::read(entry.path())?;
                if std::fs::read(&to).ok().as_deref() != Some(&bytes[..]) {
                    if let Some(d) = to.parent() {
                        std::fs::create_dir_all(d)?;
                    }
                    std::fs::write(&to, &bytes)?;
                }
            }
            seen.insert(rel);
        }
        // what the workspace no longer has goes from the copy too (a file a proof made, a file deleted since)
        let stale: Vec<PathBuf> = WalkDir::new(&self.src)
            .contents_first(true)
            .into_iter()
            .filter_entry(|e| !(e.depth() > 0 && e.file_type().is_dir() && skip_dir(e.file_name())))
            .filter_map(|e| e.ok())
            .filter(|e| e.depth() > 0)
            .filter_map(|e| e.path().strip_prefix(&self.src).ok().map(Path::to_path_buf))
            .filter(|rel| !seen.contains(rel))
            .collect();
        for rel in stale {
            let p = self.src.join(&rel);
            if p.is_dir() {
                let _ = std::fs::remove_dir(&p);
            } else {
                let _ = std::fs::remove_file(&p);
            }
        }
        Ok(())
    }

    /// Edits planned on the workspace written into the copy; the relative paths written.
    pub fn write(&self, edits: &EditSet) -> Result<Vec<PathBuf>> {
        edits.write_to(&self.workspace, &self.src)
    }

    /// Several functions of `package` as `cargo expand` gives them on the copy (one expansion, on the kept build),
    /// each in the toolkit's proof form, or `None` where the expansion has no such function; the line and its time.
    pub fn expand_fns(&self, package: &str, items: &[String]) -> Result<(BTreeMap<String, Option<String>>, String, i64)> {
        use custodian_kit::exec::{line, Cargo, Plan, Watch};
        let l = Cargo::expand(self.src.join("Cargo.toml").as_os_str(), package).lib().ugly();
        let mut ln = line("expand", &l).map_err(|e| PassengerError::TransformRefused { detail: e.to_string() })?;
        ln.env.push(("CARGO_TARGET_DIR".into(), self.target.display().to_string()));
        ln.env.extend(PROOF_ENV.iter().map(|(k, v)| (k.to_string(), v.to_string())));
        let shown = format!("cargo expand -p {package} --lib --ugly");
        let walked = Plan::new("proof-expand").then(ln).walk(Watch::Quiet, |_, _| {}).map_err(|e| PassengerError::TransformRefused { detail: e.to_string() })?;
        let w = walked.into_iter().next().ok_or_else(|| PassengerError::TransformRefused { detail: "cargo expand did not run".into() })?;
        if w.code != Some(0) {
            let tail = w.stderr.lines().rev().take(12).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
            return Err(PassengerError::TransformRefused { detail: format!("cargo expand -p {package}: exit {:?}: {tail}", w.code) });
        }
        let file = syn::parse_file(&w.stdout).map_err(|e| PassengerError::TransformRefused { detail: format!("cargo expand's output for {package} does not parse: {e}") })?;
        let found = items.iter().map(|item| (item.clone(), crate::to_toolkit::fn_in(&file.items, package, item).map(|f| custodian_kit::toolkit::normal::proof_form(&f)))).collect();
        Ok((found, shown, w.millis))
    }

    /// `cargo test -p <package>` on the copy, on the kept build.
    pub fn test(&self, package: &str) -> Result<TestRun> {
        use custodian_kit::exec::{line, Cargo, Plan, Watch};
        let l = Cargo::test(self.src.join("Cargo.toml").as_os_str(), package);
        let mut ln = line("tests", &l).map_err(|e| PassengerError::TransformRefused { detail: e.to_string() })?.going_on();
        ln.env.push(("CARGO_TARGET_DIR".into(), self.target.display().to_string()));
        ln.env.extend(PROOF_ENV.iter().map(|(k, v)| (k.to_string(), v.to_string())));
        let walked = Plan::new("proof-tests").then(ln).walk(Watch::Quiet, |_, _| {}).map_err(|e| PassengerError::TransformRefused { detail: e.to_string() })?;
        let w = walked.into_iter().next().ok_or_else(|| PassengerError::TransformRefused { detail: "cargo test did not run".into() })?;
        let results = w.stdout.lines().chain(w.stderr.lines()).filter(|l| l.starts_with("test result:")).map(str::to_string).collect();
        let failing = w.stdout.lines().filter_map(|l| l.strip_prefix("test ").and_then(|r| r.strip_suffix(" ... FAILED"))).map(str::to_string).collect();
        let tail = w.stderr.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or_default().to_string();
        Ok(TestRun { code: w.code, results, failing, tail })
    }

    /// The crate's tests on the copy as the edits leave it, compared with the copy as the workspace is: when some
    /// fail, the files the edits changed are put back and the tests run again; a test failing after that failed the
    /// same before is not the change's.
    pub fn tests_compared(&self, package: &str, edits: &EditSet) -> Result<Compared> {
        self.write(edits)?;
        let after = self.test(package)?;
        if after.code == Some(0) {
            return Ok(Compared { exit: after.code, results: after.results, failing_at_base: Vec::new(), new_failures: Vec::new() });
        }
        if after.failing.is_empty() {
            let why = format!("the tests did not run to a result: {}", after.tail);
            return Ok(Compared { exit: after.code, results: after.results, failing_at_base: Vec::new(), new_failures: vec![why] });
        }
        self.sync()?;
        let base = self.test(package);
        self.write(edits)?;
        let base = base?;
        let at_base: BTreeSet<&String> = base.failing.iter().collect();
        let (same, new): (Vec<String>, Vec<String>) = after.failing.into_iter().partition(|t| at_base.contains(t));
        Ok(Compared { exit: after.code, results: after.results, failing_at_base: same, new_failures: new })
    }

    /// A path of the workspace as it is in the copy.
    pub fn in_copy(&self, path: &Path) -> Result<PathBuf> {
        Ok(self.src.join(crate::gate::relative(&self.workspace, path)?))
    }
}

// Copyright © 2026 1o. All rights reserved.

//! The proof's one place per workspace (custodian_kit's `proof_place`, the place custodian's test runs use too): a copy of its source at a path that does not change, and one build folder
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

/// What every cargo line of a proof runs with besides its build folder: custodian_kit's, one for every line on the
/// kept build (`custodian_kit::proof_place::PROOF_ENV`).
pub const PROOF_ENV: &[(&str, &str)] = custodian_kit::proof_place::PROOF_ENV;

/// The variable naming the proof's place (`custodian_kit::proof_place::PROOF_DIR_ENV`).
pub const PROOF_DIR_ENV: &str = custodian_kit::proof_place::PROOF_DIR_ENV;

/// The proof's place for one workspace, held: custodian_kit's `ProofPlace`, the one place and kept build that
/// custodian's test runs use too.
pub struct ProofDir {
    place: custodian_kit::proof_place::ProofPlace,
}

impl ProofDir {
    /// The place for `workspace` (its root), held until dropped, the copy synced with the workspace as it is.
    pub fn take(workspace: &Path) -> Result<Self> {
        Ok(ProofDir { place: custodian_kit::proof_place::ProofPlace::take(workspace)? })
    }

    /// The copy of the workspace's root.
    pub fn root(&self) -> &Path {
        self.place.root()
    }

    /// The kept build folder: every cargo line of a proof runs with it as `CARGO_TARGET_DIR`.
    pub fn target(&self) -> &Path {
        self.place.target()
    }

    /// The copy made the same as the workspace: a file whose bytes differ written, a file the workspace no longer
    /// has removed, nothing else touched.
    pub fn sync(&self) -> Result<()> {
        Ok(self.place.sync()?)
    }

    /// Edits planned on the workspace written into the copy; the relative paths written.
    pub fn write(&self, edits: &EditSet) -> Result<Vec<PathBuf>> {
        edits.write_to(self.place.workspace(), self.place.root())
    }

    /// Several functions of `package` as `cargo expand` gives them on the copy (one expansion, on the kept build),
    /// each in the toolkit's proof form, or `None` where the expansion has no such function; the line and its time.
    pub fn expand_fns(&self, package: &str, items: &[String]) -> Result<(BTreeMap<String, Option<String>>, String, i64)> {
        use custodian_kit::exec::{line, Cargo, Plan, Watch};
        let l = Cargo::expand(self.root().join("Cargo.toml").as_os_str(), package).lib().ugly();
        let mut ln = line("expand", &l).map_err(|e| PassengerError::TransformRefused { detail: e.to_string() })?;
        ln.env.extend(self.place.env());
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
        let l = Cargo::test(self.root().join("Cargo.toml").as_os_str(), package);
        let mut ln = line("tests", &l).map_err(|e| PassengerError::TransformRefused { detail: e.to_string() })?.going_on();
        ln.env.extend(self.place.env());
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
        Ok(self.root().join(crate::gate::relative(self.place.workspace(), path)?))
    }
}

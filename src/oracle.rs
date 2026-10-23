//! The cargo oracle (rules O1–O4 in `GATE_RULES.md`): proves a plan with
//! real `cargo check` on a scratch copy. The real tree is never written.

use crate::gate::{apply_with, relative, workspace_root, EditSet};
use crate::proof_dir::ProofDir;
use klepto::{parse_cargo_json, CheckJob, Diagnostic, GatePlan, Oracle, OracleResult};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct CargoOracle {
    root: PathBuf,
    /// The workspace `root` belongs to (rule O1); the scratch copy is of this.
    workspace: Option<PathBuf>,
    /// The workspace's proof place (its copy and its kept build), held from the first check.
    proof: Option<ProofDir>,
    /// Files written into the scratch copy by the previous check, relative
    /// to the workspace root (rule O1).
    touched: BTreeSet<PathBuf>,
    /// Package → manifest, from cargo metadata once (rule A2).
    manifests: Option<BTreeMap<String, PathBuf>>,
    pub offline: bool,
    /// Check the tests too (`--tests`, with the library and binaries): a transform's proof.
    pub tests: bool,
    /// The packages with a library target, read with the manifests.
    libs: Option<BTreeSet<String>>,
}

fn internal(message: String) -> OracleResult {
    OracleResult {
        ok: false,
        diagnostics: vec![Diagnostic { code: None, level: "error".into(), message, mention: None, file: None, line: None, column: None }],
    }
}

impl CargoOracle {
    pub fn new(root: &Path) -> Self {
        Self { root: root.to_path_buf(), workspace: None, proof: None, touched: BTreeSet::new(), manifests: None, offline: true, tests: false, libs: None }
    }

    /// The workspace the scratch copy mirrors, once a check has run.
    pub fn workspace(&self) -> Option<&Path> {
        self.workspace.as_deref()
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn scratch_dir(&self) -> Option<&Path> {
        self.proof.as_ref().map(|d| d.root())
    }

    /// The proof place, once a check has taken it.
    pub fn proof(&self) -> Option<&ProofDir> {
        self.proof.as_ref()
    }

    /// The proof place given back (its lock let go), for a proof that runs more lines on it after the checks.
    pub fn into_proof(self) -> Option<ProofDir> {
        self.proof
    }

    pub fn touched(&self) -> &BTreeSet<PathBuf> {
        &self.touched
    }

    fn manifest_of(&mut self, package: &str) -> Result<PathBuf, String> {
        self.read_packages()?;
        self.manifests.as_ref().and_then(|m| m.get(&package.replace('-', "_")).cloned()).ok_or_else(|| format!("no workspace member named `{package}`"))
    }

    /// The packages' manifests and which of them have a library, from cargo metadata once.
    fn read_packages(&mut self) -> Result<(), String> {
        if self.manifests.is_none() {
            let b = crate::cargo::builder(&self.root).map_err(|e| e.to_string())?;
            self.manifests = Some(b.packages().iter().map(|(n, p)| (n.clone(), p.manifest_path.clone())).collect());
            self.libs = Some(b.packages().iter().filter(|(_, p)| p.targets.iter().any(|t| matches!(t.kind.as_str(), "lib" | "rlib" | "proc-macro" | "cdylib" | "dylib" | "staticlib"))).map(|(n, _)| n.clone()).collect());
        }
        Ok(())
    }

    /// Rule O1 for one package's plan.
    fn prepare(&mut self, plan: &GatePlan) -> Result<PathBuf, String> {
        let manifest = if plan.manifest.is_empty() { None } else { Some(self.manifest_of(&plan.package)?) };
        let edits = apply_with(plan, manifest.as_deref()).map_err(|e| e.to_string())?;
        self.prepare_edits(&edits)
    }

    /// Rule O1: pristine copy of the whole workspace on first use;
    /// previously touched files restored, then `edits` written. Returns the
    /// scratch directory that corresponds to `root` (cargo's working directory).
    fn prepare_edits(&mut self, edits: &EditSet) -> Result<PathBuf, String> {
        if self.workspace.is_none() {
            self.workspace = Some(workspace_root(&self.root).map_err(|e| e.to_string())?);
        }
        let ws = self.workspace.clone().ok_or("no workspace root")?;
        if self.proof.is_none() {
            self.proof = Some(ProofDir::take(&ws).map_err(|e| e.to_string())?);
        }
        let scratch = self.proof.as_ref().map(|d| d.root().to_path_buf()).ok_or("no proof place")?;
        for rel in std::mem::take(&mut self.touched) {
            let src = ws.join(&rel);
            let dst = scratch.join(&rel);
            if src.exists() {
                // read + write, not copy: a copied file keeps its old mtime and cargo would call the crate fresh
                let bytes = std::fs::read(&src).map_err(|e| format!("restore {}: {e}", rel.display()))?;
                std::fs::write(&dst, bytes).map_err(|e| format!("restore {}: {e}", rel.display()))?;
            } else {
                let _ = std::fs::remove_file(&dst);
            }
        }
        let written = edits.write_to(&ws, &scratch).map_err(|e| e.to_string())?;
        self.touched.extend(written);
        let inside = relative(&ws, &self.root).map_err(|e| e.to_string())?;
        Ok(scratch.join(inside))
    }
}

impl CargoOracle {
    /// Rules O2–O3 for a whole workspace's edits at once: every package's
    /// gates and manifest edits written into the scratch copy, then `job`.
    pub fn check_edits(&mut self, job: &CheckJob, edits: &EditSet) -> OracleResult {
        let tests = self.tests;
        self.check_edits_tests(job, edits, tests)
    }

    /// One check of the matrix with the edits written, its tests compiled or not (a transform compiles the tests of
    /// the crate it lands in only).
    pub fn check_edits_tests(&mut self, job: &CheckJob, edits: &EditSet, tests: bool) -> OracleResult {
        match self.prepare_edits(edits) {
            Ok(cwd) => self.run_tests(job, &cwd, tests),
            Err(e) => internal(format!("oracle setup failed: {e}")),
        }
    }

    /// The oracle on a proof place already held (a proof that ran a line on the copy before its checks).
    pub fn with_proof(root: &Path, proof: ProofDir) -> Self {
        let mut o = Self::new(root);
        o.proof = Some(proof);
        o
    }

    fn run(&mut self, job: &CheckJob, cwd: &Path) -> OracleResult {
        let tests = self.tests;
        self.run_tests(job, cwd, tests)
    }

    fn run_tests(&mut self, job: &CheckJob, cwd: &Path, tests: bool) -> OracleResult {
        let target_dir = self.proof.as_ref().map(|d| d.target().to_path_buf()).unwrap_or_else(|| cwd.join("target"));
        let cwd = cwd.to_path_buf();
        // the declared line (`crate::lines::Cargo`), run by the cargo that runs us
        let line = if tests {
            if let Err(e) = self.read_packages() {
                return internal(e);
            }
            let has_lib = self.libs.as_ref().is_some_and(|l| l.contains(&job.package.replace('-', "_")));
            crate::lines::cargo_check_with_tests(job, self.offline, has_lib)
        } else {
            crate::lines::cargo_check(job, self.offline)
        };
        if let Err(e) = xccute_contract::ValidatedCommand::validate(&line) {
            return internal(format!("the cargo check line was refused: {e}"));
        }
        // James's battery rule, before any cargo line
        if let Err(e) = custodian_kit::power::cargo_may_start() {
            return internal(e.to_string());
        }
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let mut cmd = Command::new(cargo);
        cmd.args(xccute_contract::CompositeShellCommand::argv(&line));
        cmd.current_dir(&cwd);
        cmd.env("CARGO_TARGET_DIR", target_dir);
        if self.proof.is_some() {
            cmd.envs(crate::proof_dir::PROOF_ENV.iter().copied());
        }
        let output = match cmd.output() {
            Ok(o) => o,
            Err(e) => return internal(format!("could not run cargo: {e}")),
        };
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut diagnostics = parse_cargo_json(&stdout);
        // a failure cargo gives no compiler message for (a dependency it cannot fetch offline, a manifest it refuses):
        // its own first error line is the reason
        if !output.status.success() && !diagnostics.iter().any(|d| d.level == "error") {
            let stderr = String::from_utf8_lossy(&output.stderr);
            diagnostics.push(Diagnostic {
                code: None,
                level: "error".into(),
                message: first_error_line(&stderr),
                mention: None,
                file: None,
                line: None,
                column: None,
            });
        }
        OracleResult { ok: output.status.success(), diagnostics }
    }
}

impl Oracle for CargoOracle {
    /// Rules O2–O3.
    fn check(&mut self, _root: &Path, job: &CheckJob, plan: &GatePlan) -> OracleResult {
        match self.prepare(plan) {
            Ok(cwd) => self.run(job, &cwd),
            Err(e) => internal(format!("oracle setup failed: {e}")),
        }
    }
}

/// Cargo's first error line in its stderr (`error: no matching package named ...`), with the line after it when that
/// carries the detail; else its last line that says anything; `cargo failed` when it said nothing.
pub fn first_error_line(stderr: &str) -> String {
    let lines: Vec<&str> = stderr.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    match lines.iter().position(|l| l.starts_with("error")) {
        Some(i) => {
            let l = lines[i];
            match lines.get(i + 1) {
                Some(next) if l.ends_with(':') && !next.starts_with("error") => format!("{l} {next}"),
                _ => l.to_string(),
            }
        }
        None => lines.last().map(|l| l.to_string()).unwrap_or_else(|| "cargo failed".into()),
    }
}

#[cfg(test)]
mod first_error_tests {
    #[test]
    fn a_failed_check_with_no_compiler_message_carries_cargos_first_error_line() {
        // a dependent's check offline, the registry not reachable: cargo's own error, no compiler message
        let stderr = "    Updating crates.io index\nwarning: spurious network error\nerror: failed to get `simple_impl` as a dependency of package `fleet_ops v0.1.0`\n\nCaused by:\n  attempting to make an HTTP request, but --offline was specified\n";
        assert_eq!(super::first_error_line(stderr), "error: failed to get `simple_impl` as a dependency of package `fleet_ops v0.1.0`");
        assert_eq!(super::first_error_line("error: no matching package named `x` found\nlocation searched: registry\n"), "error: no matching package named `x` found");
        assert_eq!(super::first_error_line("error: failed to select a version for the requirement:\n`simple_impl = \"^0.9\"`\n"), "error: failed to select a version for the requirement: `simple_impl = \"^0.9\"`");
        assert_eq!(super::first_error_line(""), "cargo failed");
    }
}

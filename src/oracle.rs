//! The cargo oracle (rules O1–O4 in `GATE_RULES.md`): proves a plan with
//! real `cargo check` on a scratch copy. The real tree is never written.

use crate::gate::{apply_with, relative, scratch_copy, workspace_root, EditSet};
use klepto::{parse_cargo_json, CheckJob, Diagnostic, GatePlan, Oracle, OracleResult};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct CargoOracle {
    root: PathBuf,
    /// The workspace `root` belongs to (rule O1); the scratch copy is of this.
    workspace: Option<PathBuf>,
    scratch: Option<tempfile::TempDir>,
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
        Self { root: root.to_path_buf(), workspace: None, scratch: None, touched: BTreeSet::new(), manifests: None, offline: true, tests: false, libs: None }
    }

    /// The workspace the scratch copy mirrors, once a check has run.
    pub fn workspace(&self) -> Option<&Path> {
        self.workspace.as_deref()
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn scratch_dir(&self) -> Option<&Path> {
        self.scratch.as_ref().map(|d| d.path())
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
        if self.scratch.is_none() {
            self.scratch = Some(scratch_copy(&ws).map_err(|e| e.to_string())?);
        }
        let scratch = self.scratch.as_ref().map(|d| d.path().to_path_buf()).ok_or("no scratch dir")?;
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
        match self.prepare_edits(edits) {
            Ok(cwd) => self.run(job, &cwd),
            Err(e) => internal(format!("oracle setup failed: {e}")),
        }
    }

    fn run(&mut self, job: &CheckJob, cwd: &Path) -> OracleResult {
        let target_dir = self.scratch.as_ref().map(|d| d.path().join("target")).unwrap_or_else(|| cwd.join("target"));
        let cwd = cwd.to_path_buf();
        // the declared line (`crate::lines::Cargo`), run by the cargo that runs us
        let line = if self.tests {
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
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let mut cmd = Command::new(cargo);
        cmd.args(xccute_contract::CompositeShellCommand::argv(&line));
        cmd.current_dir(&cwd);
        cmd.env("CARGO_TARGET_DIR", target_dir);
        let output = match cmd.output() {
            Ok(o) => o,
            Err(e) => return internal(format!("could not run cargo: {e}")),
        };
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut diagnostics = parse_cargo_json(&stdout);
        if !output.status.success() && diagnostics.is_empty() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            diagnostics.push(Diagnostic {
                code: None,
                level: "error".into(),
                message: stderr.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("cargo failed").to_string(),
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

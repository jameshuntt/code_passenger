//! The klepto-backed commands (rules G, F, R, X in `GATE_RULES.md`).
//! Each module exposes a library function the CLI and the tests call.

pub mod features;
pub mod gate;
pub mod lint;
pub mod patterns;
pub mod receipt;

use crate::error::{PassengerError, Result};
use klepto::Klepto;
use std::path::Path;

/// Rule G1: klepto over the whole workspace, as `scan` builds it.
pub fn klepto_for(root: &Path) -> Result<Klepto> {
    crate::cargo::builder(root)?.parse().map_err(|e| PassengerError::Unsupported(e.to_string()))
}

/// Rule G1: `--package`, else the package whose manifest is the root's
/// `Cargo.toml` (klepto's packages), else an error naming the scanned packages.
pub fn resolve_package(k: &Klepto, root: &Path, requested: Option<&str>) -> Result<String> {
    if let Some(p) = requested {
        return Ok(p.replace('-', "_"));
    }
    if let Some(p) = crate::cargo::package_with_manifest(&k.packages, &root.join("Cargo.toml")) {
        return Ok(p.name.clone());
    }
    let names: Vec<&String> = k.features.keys().collect();
    Err(PassengerError::Unsupported(format!("no --package given and the root manifest has no [package]; scanned packages: {names:?}")))
}

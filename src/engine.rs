//! `scan`: klepto's use sites grouped per file. The files are the ones the
//! scan read (`Klepto::files`), not a directory walk; the manifest facts are
//! klepto's feature model (`crate::cargo`).

use crate::{
    cargo::{manifest_info, package_with_manifest},
    error::{PassengerError, Result},
    model::{DocumentDetails, FileReport, ManifestInfo, UsedSymbols},
    packs,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

pub struct RunContext {
    pub root: PathBuf,
    /// The directory under `root` whose files are reported (`src`); empty:
    /// every file the scan read, every member's.
    pub src_rel: String,
    /// The package reported as `EngineOutput::manifest`; a virtual
    /// workspace's root manifest names none (`workspace`).
    pub manifest_path: PathBuf,
    pub lang: String,
}

#[derive(Serialize, Deserialize)]
pub struct EngineOutput {
    pub manifest: ManifestInfo,
    /// Every scanned package's manifest facts.
    #[serde(default)]
    pub packages: BTreeMap<String, ManifestInfo>,
    pub reports: Vec<FileReport>,
}

fn module_prefix(path: &str) -> Option<String> {
    let mut it = path.split("::");
    let a = it.next()?;
    let b = it.next()?;
    Some(format!("{a}::{b}"))
}

fn used_external_from_sites(sites: &[klepto::UseSite]) -> BTreeSet<String> {
    sites.iter().map(|u| u.dep.clone()).collect()
}

fn used_mods_from_internal_sites(sites: &[klepto::UseSite]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for u in sites {
        if let Some(m) = module_prefix(&u.path) {
            out.insert(m);
        } else {
            out.insert(u.dep.clone()); // fallback
        }
    }
    out
}

fn canon(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

/// `path` relative to `base`, `/`-separated.
fn rel_to(base: &Path, p: &Path) -> Result<String> {
    let rel = p.strip_prefix(base).or_else(|_| p.strip_prefix(canon(base))).map_err(|e| PassengerError::path(format!("{}: {e}", p.display())))?;
    Ok(rel.to_string_lossy().replace('\\', "/"))
}

pub fn run_scan(ctx: &RunContext) -> Result<EngineOutput> {
    let k = crate::cargo::builder(&ctx.root)?.parse().map_err(|e| PassengerError::unsupported(e.to_string()))?;
    run_scan_on(ctx, &k)
}

/// `scan` over a parse already made.
pub fn run_scan_on(ctx: &RunContext, k: &klepto::Klepto) -> Result<EngineOutput> {
    let packages: BTreeMap<String, ManifestInfo> = k.features.iter().map(|(n, m)| (n.clone(), manifest_info(m))).collect();
    let manifest = package_with_manifest(&k.packages, &ctx.manifest_path)
        .and_then(|p| packages.get(&p.name).cloned())
        .unwrap_or_else(|| ManifestInfo { crate_name: "workspace".into(), ..ManifestInfo::empty() });

    // every dependency of every scanned package
    let used_deps: BTreeSet<String> = packages.values().flat_map(|m| m.all_deps.iter().cloned()).collect();

    let mut ext_by_file: BTreeMap<PathBuf, Vec<klepto::UseSite>> = BTreeMap::new();
    for u in k.dep_use_sites(&used_deps) {
        ext_by_file.entry(canon(&u.location.path)).or_default().push(u.clone());
    }
    let mut int_by_file: BTreeMap<PathBuf, Vec<klepto::UseSite>> = BTreeMap::new();
    for u in k.internal_use_sites() {
        int_by_file.entry(canon(&u.location.path)).or_default().push(u.clone());
    }

    let pack = packs::get_pack(&ctx.lang)?;
    let base = if ctx.src_rel.is_empty() { ctx.root.clone() } else { ctx.root.join(&ctx.src_rel) };
    let base_c = canon(&base);
    let mut files: Vec<&klepto::ParsedFile> = k.files.iter().filter(|f| pack.matches_path(&f.path) && canon(&f.path).starts_with(&base_c)).collect();
    files.sort_by(|a, b| a.path.cmp(&b.path));

    let mut reports = Vec::new();
    for f in files {
        let abs = canon(&f.path);
        let rel = rel_to(&base_c, &abs)?;
        let filename = abs.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let external_use_sites = ext_by_file.get(&abs).cloned().unwrap_or_default();
        let internal_use_sites = int_by_file.get(&abs).cloned().unwrap_or_default();
        let external_dep_symbol_counts = count_use_sites(&external_use_sites);
        let internal_dep_symbol_counts = count_use_sites(&internal_use_sites);
        let used = UsedSymbols { packages: used_external_from_sites(&external_use_sites), modules: used_mods_from_internal_sites(&internal_use_sites) };
        reports.push(FileReport {
            document: DocumentDetails { filename, relative_path: rel },
            used,
            notes: vec![],
            corpus_features: vec![],
            internal_use_sites,
            internal_dep_symbol_counts,
            external_use_sites,
            external_dep_symbol_counts,
        });
    }

    Ok(EngineOutput { manifest, packages, reports })
}

fn count_use_sites(sites: &[klepto::UseSite]) -> BTreeMap<String, BTreeMap<String, usize>> {
    let mut out = BTreeMap::<String, BTreeMap<String, usize>>::new();
    for u in sites {
        out.entry(u.dep.clone()).or_default().entry(u.head.clone()).and_modify(|c| *c += 1).or_insert(1);
    }
    out
}

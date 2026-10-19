//! The passenger store: snapshots of a workspace's tracked files, kept per
//! workspace root (the root cargo names for `--root`, never the current
//! directory) in `<root>/.passenger/`.
//!
//! - Its paths are named once through pathgen (a dev policy whose base is the
//!   workspace root and whose app is `.passenger`; [`named`]).
//! - The tracked files are the ones klepto's scan reads (every member's `.rs`
//!   files, every member manifest, the root manifest and lockfile; klepto's
//!   fingerprint) and the features document: no directory walk.
//! - Every blob is stored once by its stable_digest (SHA-256, hex), zstd
//!   compressed; a commit names its parent's hash, so the history is a chain.
//! - `checkout` moves HEAD and writes the snapshot's files back; a tracked
//!   file changed since HEAD's snapshot is refused unless forced, and a file
//!   the snapshot does not hold is left as it is.

use crate::engine::EngineOutput;
use crate::error::{PassengerError, Result};
use pathgen::{AppPathRole, ArtifactEndpoint, ArtifactOutputDescriptor, PathMutability, PathOwner, PathPolicy, PathSensitivity, PathTemplateVars};
use stable_digest::StableDigest;
use std::{collections::BTreeMap, fs, io::Write, path::{Path, PathBuf}};

use super::types::*;

/// The store's folder name, and pathgen's app name for it.
pub const FOLDER: &str = ".passenger";

const fn file(route_id: &'static str, kind: &'static str, endpoint: ArtifactEndpoint, base_role: AppPathRole, relative_template: &'static str, mutability: PathMutability) -> ArtifactOutputDescriptor {
    ArtifactOutputDescriptor {
        route_id,
        schema: "code_passenger.passenger.v2",
        kind,
        version: 2,
        endpoint,
        base_role,
        relative_template,
        sensitivity: PathSensitivity::Internal,
        mutability,
        owner: PathOwner::CurrentUser,
    }
}

/// Every file the store keeps, named once.
pub mod named {
    use super::*;
    pub const CONFIG: ArtifactOutputDescriptor = file("passenger.config", "config", ArtifactEndpoint::FileToml, AppPathRole::Config, "passenger.toml", PathMutability::Writable);
    pub const STATE: ArtifactOutputDescriptor = file("passenger.state", "state", ArtifactEndpoint::FileJson, AppPathRole::State, "state.json", PathMutability::Writable);
    pub const HEAD: ArtifactOutputDescriptor = file("passenger.head", "head", ArtifactEndpoint::FileTxt, AppPathRole::State, "V{version}/HEAD", PathMutability::Writable);
    pub const REF: ArtifactOutputDescriptor = file("passenger.ref", "ref", ArtifactEndpoint::FileTxt, AppPathRole::State, "V{version}/refs/{branch}", PathMutability::Writable);
    pub const OBJECT: ArtifactOutputDescriptor = file("passenger.object", "object", ArtifactEndpoint::FileTxt, AppPathRole::Data, "objects/sha256/{a}/{b}/{hash}.zst", PathMutability::AppendOnly);
    pub const COMMIT: ArtifactOutputDescriptor = file("passenger.commit", "commit", ArtifactEndpoint::FileJson, AppPathRole::Data, "V{version}/commits/{id}.json", PathMutability::AppendOnly);
    pub const ARTIFACT: ArtifactOutputDescriptor = file("passenger.artifact", "artifact", ArtifactEndpoint::FileJson, AppPathRole::Data, "V{version}/artifacts/{id}/{name}", PathMutability::AppendOnly);
    /// A 1o schema copied where core-1o's workspace (and its language server) reads it.
    pub const SCHEMA: ArtifactOutputDescriptor = file("passenger.schema", "schema", ArtifactEndpoint::FileJson, AppPathRole::Config, "schema/{modifier}/{version}.schema.json", PathMutability::Writable);
}

/// The schema folder, relative to the workspace root, as `oneo.project.json` lists it.
pub const SCHEMA_DIR: &str = ".passenger/config/schema";

#[derive(Debug, Clone)]
pub struct PassengerStore {
    /// The workspace root the store belongs to.
    pub root: PathBuf,
    policy: PathPolicy,
}

#[derive(Debug, Clone)]
pub struct HeadInfo {
    pub passenger_version: String,
    pub head_kind: HeadKind,
    pub branch: String,
    pub head_commit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeadKind {
    Ref,
    Detached,
}

#[derive(Debug, Clone)]
pub struct CheckpointOptions {
    pub note: Option<String>,
    /// Override the target branch (otherwise HEAD's).
    pub branch: Option<String>,
    /// Write scan.json and analysis.json beside the commit.
    pub include_artifacts: bool,
    /// Files the scan skipped because klepto could not parse them (path, the
    /// parser's message): each is marked `unparsed` in the snapshot.
    pub unparsed: Vec<(PathBuf, String)>,
}

impl Default for CheckpointOptions {
    fn default() -> Self {
        Self { note: None, branch: None, include_artifacts: true, unparsed: Vec::new() }
    }
}

/// What a checkout did.
#[derive(Debug, Clone, Default)]
pub struct Restored {
    /// The commit whose files were written.
    pub commit: Option<String>,
    pub branch: String,
    /// Files written back (relative), those already equal to the snapshot left out.
    pub written: Vec<String>,
    /// Tracked files now that the snapshot does not hold: left as they are.
    pub left: Vec<String>,
}

fn store_err(path: &Path, e: impl std::fmt::Display) -> PassengerError {
    PassengerError::Io { detail: format!("{}: {e}", path.display()) }
}

impl PassengerStore {
    fn policy(root: &Path) -> Result<PathPolicy> {
        PathPolicy::dev(FOLDER).map(|p| p.with_dev_base(root)).map_err(|e| store_err(root, e))
    }

    /// The workspace root `start` belongs to (cargo's), or `start` itself.
    pub fn workspace_of(start: &Path) -> PathBuf {
        let root = crate::gate::workspace_root(start).unwrap_or_else(|_| start.to_path_buf());
        root.canonicalize().unwrap_or(root)
    }

    /// A named file, its template filled.
    pub fn path(&self, d: &ArtifactOutputDescriptor, vars: &[(&str, &str)]) -> Result<PathBuf> {
        let mut v = PathTemplateVars::new();
        for (k, val) in vars {
            v = v.with(k, val);
        }
        self.policy.artifact_output(d, &v).map(|r| r.path).map_err(|e| store_err(&self.root, e))
    }

    fn config_path(&self) -> Result<PathBuf> {
        self.path(&named::CONFIG, &[])
    }

    /// The store's paths for the workspace `start` belongs to, made or not.
    pub fn at(start: impl AsRef<Path>) -> Result<Self> {
        let root = Self::workspace_of(start.as_ref());
        Ok(Self { policy: Self::policy(&root)?, root })
    }

    /// The store of the workspace `start` belongs to.
    pub fn open(start: impl AsRef<Path>) -> Result<Self> {
        let root = Self::workspace_of(start.as_ref());
        let s = Self { policy: Self::policy(&root)?, root };
        let cfg = s.config_path()?;
        if !cfg.exists() {
            return Err(PassengerError::NoStore { detail: s.root.display().to_string() });
        }
        Ok(s)
    }

    /// Makes the store (or keeps an existing one) for the workspace `start` belongs to.
    pub fn init(start: impl AsRef<Path>) -> Result<Self> {
        let root = Self::workspace_of(start.as_ref());
        let store = Self { policy: Self::policy(&root)?, root };
        let cfg_path = store.config_path()?;
        if !cfg_path.exists() {
            write_toml_atomic(&cfg_path, &PassengerConfig::default())?;
        }
        let st_path = store.path(&named::STATE, &[])?;
        if !st_path.exists() {
            let mut st = PassengerState::default();
            st.next_seq.insert(store.read_config()?.passenger_version, 1);
            write_json_atomic(&st_path, &st)?;
        }
        store.ensure_version_layout()?;
        Ok(store)
    }

    pub fn open_or_init(start: impl AsRef<Path>) -> Result<Self> {
        match Self::open(start.as_ref()) {
            Ok(s) => Ok(s),
            Err(PassengerError::NoStore { .. }) => Self::init(start),
            Err(e) => Err(e),
        }
    }

    pub fn read_config(&self) -> Result<PassengerConfig> {
        read_toml(&self.config_path()?)
    }
    pub fn read_state(&self) -> Result<PassengerState> {
        read_json(&self.path(&named::STATE, &[])?)
    }
    pub fn write_state(&self, st: &PassengerState) -> Result<()> {
        write_json_atomic(&self.path(&named::STATE, &[])?, st)
    }

    fn head_path(&self, version: &str) -> Result<PathBuf> {
        self.path(&named::HEAD, &[("version", version)])
    }

    fn ref_path(&self, version: &str, branch: &str) -> Result<PathBuf> {
        if branch.is_empty() || branch.contains(['/', '\\']) || branch.starts_with('.') {
            return Err(PassengerError::NoSnapshot { detail: format!("`{branch}` is not a branch name") });
        }
        self.path(&named::REF, &[("version", version), ("branch", branch)])
    }

    pub fn ensure_version_layout(&self) -> Result<()> {
        let cfg = self.read_config()?;
        let r = self.ref_path(&cfg.passenger_version, &cfg.default_branch)?;
        if !r.exists() {
            write_text_atomic(&r, "")?; // empty means no commits yet
        }
        let head = self.head_path(&cfg.passenger_version)?;
        if !head.exists() {
            write_text_atomic(&head, format!("ref: refs/{}", cfg.default_branch))?;
        }
        Ok(())
    }

    pub fn resolve_head(&self) -> Result<HeadInfo> {
        let cfg = self.read_config()?;
        let head_txt = fs::read_to_string(self.head_path(&cfg.passenger_version)?)?.trim().to_string();
        if let Some(rest) = head_txt.strip_prefix("ref: ") {
            let branch = rest.trim().strip_prefix("refs/").unwrap_or(rest.trim()).to_string();
            let commit = fs::read_to_string(self.ref_path(&cfg.passenger_version, &branch)?).ok().map(|s| s.trim().to_string());
            Ok(HeadInfo { passenger_version: cfg.passenger_version, head_kind: HeadKind::Ref, branch, head_commit: commit.filter(|s| !s.is_empty()) })
        } else {
            Ok(HeadInfo {
                passenger_version: cfg.passenger_version,
                head_kind: HeadKind::Detached,
                branch: "(detached)".to_string(),
                head_commit: if head_txt.is_empty() { None } else { Some(head_txt) },
            })
        }
    }

    pub fn create_branch(&self, name: &str, from: Option<&str>) -> Result<()> {
        let head = self.resolve_head()?;
        let new_ref = self.ref_path(&head.passenger_version, name)?;
        if new_ref.exists() {
            return Err(PassengerError::path(format!("branch already exists: {name}")));
        }
        let base = match from {
            Some(id) => id.to_string(),
            None => head.head_commit.clone().unwrap_or_default(),
        };
        write_text_atomic(&new_ref, base)
    }

    pub fn read_commit(&self, passenger_version: &str, id: &str) -> Result<PassengerCommit> {
        let p = self.path(&named::COMMIT, &[("version", passenger_version), ("id", id)])?;
        if !p.exists() {
            return Err(PassengerError::NoSnapshot { detail: id.to_string() });
        }
        read_json(&p)
    }

    /// The tracked files: what klepto's scan reads, and the features document (relative, sorted).
    pub fn tracked(&self) -> Result<Vec<String>> {
        let fp = crate::cargo::builder(&self.root)?.fingerprint()?;
        let mut out: Vec<String> = fp.files.iter().filter_map(|(p, _)| self.rel(p)).collect();
        for name in [crate::doc::DOC_1O, crate::doc::DOC_TOML] {
            if self.root.join(name).is_file() {
                out.push(name.to_string());
            }
        }
        out.sort();
        out.dedup();
        Ok(out)
    }

    fn rel(&self, p: &Path) -> Option<String> {
        let c = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        c.strip_prefix(&self.root).ok().map(|r| r.to_string_lossy().replace('\\', "/"))
    }

    /// A snapshot of the tracked files, with scan / analysis artifacts.
    pub fn checkpoint(&self, raw: Option<&EngineOutput>, analysis_json: Option<&serde_json::Value>, opts: CheckpointOptions) -> Result<PassengerCommit> {
        self.ensure_version_layout()?;
        let cfg = self.read_config()?;
        let mut st = self.read_state()?;
        let head = self.resolve_head()?;
        let target_branch = opts.branch.clone().unwrap_or_else(|| if head.head_kind == HeadKind::Ref { head.branch.clone() } else { cfg.default_branch.clone() });
        let ref_path = self.ref_path(&cfg.passenger_version, &target_branch)?;
        if !ref_path.exists() {
            self.create_branch(&target_branch, head.head_commit.as_deref())?;
        }
        let parent_id = fs::read_to_string(&ref_path).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());

        let seq = st.next_seq.entry(cfg.passenger_version.clone()).or_insert(1);
        let id = format!("S{:06}", *seq);
        *seq += 1;
        self.write_state(&st)?;

        let mut manifest = self.build_full_manifest()?;
        for (p, why) in &opts.unparsed {
            if let Some(e) = self.rel(p).and_then(|r| manifest.files.get_mut(&r)) {
                e.unparsed = Some(why.clone());
            }
        }
        let stats = self.compute_stats_delta(&cfg, parent_id.as_deref(), &manifest)?;
        let mut commit = PassengerCommit {
            schema: 2,
            id: id.clone(),
            ts_ms: chrono::Utc::now().timestamp_millis(),
            passenger_version: cfg.passenger_version.clone(),
            branch: target_branch.clone(),
            parents: parent_id.clone().into_iter().collect(),
            manifest,
            stats,
            note: opts.note.clone(),
            prev_hash: None,
            hash: String::new(),
        };
        if let Some(pid) = &parent_id {
            commit.prev_hash = Some(self.read_commit(&cfg.passenger_version, pid)?.hash);
        }
        commit.hash = compute_commit_hash(&commit)?;
        write_json_atomic(&self.path(&named::COMMIT, &[("version", &cfg.passenger_version), ("id", &id)])?, &commit)?;
        write_text_atomic(&ref_path, id.clone())?;
        // a checkpoint lands on its branch: HEAD follows it
        write_text_atomic(&self.head_path(&cfg.passenger_version)?, format!("ref: refs/{target_branch}"))?;
        if opts.include_artifacts {
            if let Some(raw) = raw {
                self.write_artifact(&cfg.passenger_version, &id, "scan.json", &serde_json::to_vec_pretty(raw)?)?;
            }
            if let Some(aj) = analysis_json {
                self.write_artifact(&cfg.passenger_version, &id, "analysis.json", &serde_json::to_vec_pretty(aj)?)?;
            }
        }
        Ok(commit)
    }

    pub fn write_artifact(&self, version: &str, id: &str, name: &str, bytes: &[u8]) -> Result<PathBuf> {
        let p = self.path(&named::ARTIFACT, &[("version", version), ("id", id), ("name", name)])?;
        write_bytes_atomic(&p, bytes)?;
        Ok(p)
    }

    /// `checkpoint` of the whole workspace: scan.json and analysis.json of
    /// every member, and with a features plan, the plan and the document.
    pub fn checkpoint_tree(&self, note: &str, plan: Option<&crate::workspace::WorkspacePlan>) -> Result<PassengerCommit> {
        let ctx = crate::engine::RunContext { root: self.root.clone(), src_rel: String::new(), manifest_path: self.root.join("Cargo.toml"), lang: "rust".into() };
        let (out, unparsed) = crate::engine::run_scan_skipping(&ctx)?;
        let analysis = serde_json::to_value(crate::analysis::run(&out))?;
        let commit = self.checkpoint(Some(&out), Some(&analysis), CheckpointOptions { note: Some(note.to_string()), unparsed, ..Default::default() })?;
        if let Some(plan) = plan {
            self.write_artifact(&commit.passenger_version, &commit.id, "plan.json", &serde_json::to_vec_pretty(plan)?)?;
            if let Some(doc) = &plan.doc {
                let name = doc.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "features".into());
                self.write_artifact(&commit.passenger_version, &commit.id, &name, &fs::read(doc)?)?;
            }
        }
        Ok(commit)
    }

    /// `checkout <branch | snapshot>`: HEAD moved, the snapshot's files written back.
    pub fn checkout(&self, name: &str, force: bool) -> Result<Restored> {
        let head = self.resolve_head()?;
        let v = head.passenger_version.clone();
        let (target, branch) = match self.ref_path(&v, name) {
            Ok(r) if r.exists() => (fs::read_to_string(&r)?.trim().to_string(), Some(name.to_string())),
            _ => {
                self.read_commit(&v, name)?;
                (name.to_string(), None)
            }
        };
        if !force
            && let Some(hc) = &head.head_commit {
                let at_head = self.read_commit(&v, hc)?;
                let now = self.build_full_manifest()?;
                let changed: Vec<String> = now.files.iter().filter(|(p, e)| at_head.manifest.files.get(*p).is_some_and(|h| h.hash != e.hash)).map(|(p, _)| p.clone()).collect();
                if !changed.is_empty() {
                    return Err(PassengerError::Dirty { detail: changed.join(", ") });
                }
            }
        let mut restored = Restored { commit: None, branch: branch.clone().unwrap_or_else(|| "(detached)".into()), ..Default::default() };
        if !target.is_empty() {
            let c = self.read_commit(&v, &target)?;
            for (rel, entry) in &c.manifest.files {
                let dst = self.root.join(rel);
                if fs::read(&dst).ok().is_some_and(|b| sha256_hex(&b) == entry.hash) {
                    continue;
                }
                let bytes = self.read_object(&entry.hash)?;
                write_bytes_atomic(&dst, &bytes)?;
                restored.written.push(rel.clone());
            }
            restored.left = self.tracked()?.into_iter().filter(|p| !c.manifest.files.contains_key(p)).collect();
            restored.commit = Some(target.clone());
        }
        let head_text = match &branch {
            Some(b) => format!("ref: refs/{b}"),
            None => target.clone(),
        };
        write_text_atomic(&self.head_path(&v)?, head_text)?;
        Ok(restored)
    }

    /// Moves HEAD to a snapshot without touching files.
    pub fn detach_head(&self, snapshot_id: &str) -> Result<()> {
        let head = self.resolve_head()?;
        write_text_atomic(&self.head_path(&head.passenger_version)?, snapshot_id.to_string())
    }

    fn build_full_manifest(&self) -> Result<Manifest> {
        let mut files: BTreeMap<String, FileEntry> = BTreeMap::new();
        for rel in self.tracked()? {
            let entry = self.ingest_file(&self.root.join(&rel))?;
            files.insert(rel, entry);
        }
        Ok(Manifest { kind: ManifestKind::Full, base: None, files, deleted: vec![] })
    }

    fn ingest_file(&self, path: &Path) -> Result<FileEntry> {
        let bytes = fs::read(path)?;
        let hash = sha256_hex(&bytes);
        self.write_object_if_missing(&hash, &bytes)?;
        Ok(FileEntry { hash, bytes: bytes.len() as u64, lines: count_lines_bytes(&bytes) as u64, unparsed: None })
    }

    fn object_path(&self, hash: &str) -> Result<PathBuf> {
        self.path(&named::OBJECT, &[("a", &hash[0..2]), ("b", &hash[2..4]), ("hash", hash)])
    }

    fn write_object_if_missing(&self, hash: &str, bytes: &[u8]) -> Result<()> {
        let p = self.object_path(hash)?;
        if p.exists() {
            return Ok(());
        }
        let compressed = zstd::encode_all(bytes, 3)?;
        write_bytes_atomic(&p, &compressed)
    }

    fn read_object(&self, hash: &str) -> Result<Vec<u8>> {
        let compressed = fs::read(self.object_path(hash)?)?;
        Ok(zstd::decode_all(&compressed[..])?)
    }

    fn compute_stats_delta(&self, cfg: &PassengerConfig, parent_id: Option<&str>, new_manifest: &Manifest) -> Result<CommitStats> {
        let mut st = CommitStats::default();
        let Some(pid) = parent_id else {
            st.changed_files = new_manifest.files.len();
            return Ok(st);
        };
        let parent = self.read_commit(&cfg.passenger_version, pid)?;
        let old = &parent.manifest.files;
        let new = &new_manifest.files;
        let mut changed_paths: Vec<String> = vec![];
        for (p, e) in new {
            match old.get(p) {
                Some(oe) if oe.hash == e.hash => {}
                _ => changed_paths.push(p.clone()),
            }
        }
        for p in old.keys() {
            if !new.contains_key(p) {
                changed_paths.push(p.clone());
            }
        }
        changed_paths.sort();
        changed_paths.dedup();
        st.changed_files = changed_paths.len();
        for p in changed_paths {
            match (old.get(&p).map(|x| x.hash.clone()), new.get(&p).map(|x| x.hash.clone())) {
                (Some(oh), Some(nh)) => {
                    let old_txt = String::from_utf8_lossy(&self.read_object(&oh)?).to_string();
                    let new_txt = String::from_utf8_lossy(&self.read_object(&nh)?).to_string();
                    let (a, r) = diff_line_counts(&old_txt, &new_txt);
                    st.added_lines += a;
                    st.removed_lines += r;
                }
                (None, Some(nh)) => st.added_lines += String::from_utf8_lossy(&self.read_object(&nh)?).lines().count(),
                (Some(oh), None) => st.removed_lines += String::from_utf8_lossy(&self.read_object(&oh)?).lines().count(),
                (None, None) => {}
            }
        }
        st.paste_score = 0.0;
        Ok(st)
    }
}

/* ----------------------- helpers ----------------------- */

/// A blob's digest: stable_digest's SHA-256, lowercase hex.
fn sha256_hex(bytes: &[u8]) -> String {
    StableDigest::sha256(bytes).as_str().to_string()
}

fn count_lines_bytes(bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        return 0;
    }
    let mut n = bytes.iter().filter(|b| **b == b'\n').count();
    if bytes.last() != Some(&b'\n') {
        n += 1;
    }
    n
}

fn diff_line_counts(old_txt: &str, new_txt: &str) -> (usize, usize) {
    use similar::TextDiff;
    let diff = TextDiff::from_lines(old_txt, new_txt);
    let mut add = 0usize;
    let mut rem = 0usize;
    for op in diff.ops() {
        for change in diff.iter_changes(op) {
            match change.tag() {
                similar::ChangeTag::Insert => add += 1,
                similar::ChangeTag::Delete => rem += 1,
                similar::ChangeTag::Equal => {}
            }
        }
    }
    (add, rem)
}

/// The commit's hash: its JSON with `hash` blank, then the parent's hash.
fn compute_commit_hash(c: &PassengerCommit) -> Result<String> {
    let mut tmp = c.clone();
    tmp.hash.clear();
    let mut bytes = serde_json::to_vec(&tmp)?;
    if let Some(prev) = &c.prev_hash {
        bytes.extend_from_slice(prev.as_bytes());
    }
    Ok(sha256_hex(&bytes))
}

fn read_json<T: for<'de> serde::Deserialize<'de>>(p: &Path) -> Result<T> {
    let bytes = fs::read(p).map_err(|e| store_err(p, e))?;
    Ok(serde_json::from_slice(&bytes)?)
}
fn write_json_atomic<T: serde::Serialize>(p: &Path, v: &T) -> Result<()> {
    write_bytes_atomic(p, &serde_json::to_vec_pretty(v)?)
}

fn read_toml<T: for<'de> serde::Deserialize<'de>>(p: &Path) -> Result<T> {
    let s = fs::read_to_string(p).map_err(|e| store_err(p, e))?;
    Ok(toml::from_str(&s)?)
}
fn write_toml_atomic<T: serde::Serialize>(p: &Path, v: &T) -> Result<()> {
    let s = toml::to_string_pretty(v).map_err(PassengerError::toml)?;
    write_text_atomic(p, s)
}

fn write_text_atomic(p: &Path, s: impl Into<String>) -> Result<()> {
    write_bytes_atomic(p, s.into().as_bytes())
}

/// Written whole: a sibling temporary file, then a rename.
fn write_bytes_atomic(p: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(d) = p.parent() {
        fs::create_dir_all(d).map_err(|e| store_err(d, e))?;
    }
    let tmp = p.with_file_name(format!(".{}.{}.tmp", p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), std::process::id()));
    {
        let mut f = fs::File::create(&tmp).map_err(|e| store_err(&tmp, e))?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, p).map_err(|e| store_err(p, e))?;
    Ok(())
}

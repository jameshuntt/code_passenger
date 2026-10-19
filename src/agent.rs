//! Agent mode, on custodian_kit's job runtime: jobs as folders in
//! `.custodian/` (queued → running → done), every step a declared
//! code-passenger line started by custodian_kit's executor, its receipt kept
//! in the journal (`receipts/<seq>.json`), and a closing receipt in the
//! job's folder.
//!
//! - `agent init` makes custodian_kit's agent layout and writes the job
//!   templates code-passenger runs into `config/jobs/<kind>.toml`, where they
//!   are the user's to edit.
//! - `agent queue [NAME] [--kind features]` drops a job folder into
//!   `jobs/queued/` (its `job.toml` names the kind).
//! - `agent run [NAME]` moves the job to `jobs/running/`, runs the
//!   template's steps in order (each step that `reads` another is given that
//!   step's receipt as `--after`, and refuses if it was not accepted, or if
//!   the plan it names no longer matches the code), keeps each step's output
//!   in the folder, writes `closing.json` / `closing.md` and moves the folder
//!   to `jobs/done/`.

use crate::error::{PassengerError, Result};
use crate::lines::{AgentRooted, CodePassenger, JOB_ENV};
use custodian_kit::exec::{self, Plan, Watch};
use custodian_kit::runs::{keep, now_utc, receipt_of, stamp, InJob};
use custodian_kit::jobs::JobFile;
use custodian_kit::store::{named, write_whole, JobRow, JobState, Mode, Receipt, Store, RECEIPT_SCHEMA};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The job kinds code-passenger runs, written by `agent init`.
pub const TEMPLATES: &[(&str, &str)] = &[("features", FEATURES)];

/// `features`: the workspace features document planned for every crate,
/// the proven plan applied, the headers written.
pub const FEATURES: &str = r#"# features: plan every crate from the workspace features document (proven by cargo check),
# apply exactly that plan (refused if the code changed since), then write the headers it implies.
kind = "features"
summary = "planned for every crate and proven; applied as planned; headers written from the plan"

[[step]]
name = "plan"
line = "features-plan"

[[step]]
name = "apply"
line = "features-apply"
reads = "plan"

[[step]]
name = "annotate"
line = "annotate"
reads = "apply"
"#;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobStep {
    pub name: String,
    /// `features-plan`, `features-apply` or `annotate`.
    pub line: String,
    /// The earlier step whose receipt this step reads.
    #[serde(default)]
    pub reads: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobTemplate {
    pub kind: String,
    #[serde(default)]
    pub summary: String,
    pub step: Vec<JobStep>,
}

/// One step's part of the closing receipt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepClosed {
    pub name: String,
    pub line: String,
    pub seq: i64,
    pub exit_code: Option<i32>,
    pub accepted: bool,
    pub word: String,
    pub receipt: String,
    pub outputs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Closing {
    pub schema: String,
    pub id: String,
    pub name: String,
    pub kind: String,
    pub opened_utc: String,
    pub closed_utc: String,
    pub steps: Vec<StepClosed>,
    /// Every step accepted.
    pub done: bool,
    /// The step that stopped the job, if one did.
    pub stopped_at: Option<String>,
}

pub const CLOSING_SCHEMA: &str = "code_passenger.job_closing.v1";

fn custodian(e: custodian_kit::CustodianError) -> PassengerError {
    PassengerError::from(e)
}

fn store_at(root: &Path) -> Result<Store> {
    Store::find(root).ok_or_else(|| PassengerError::Custodian { detail: format!("no .custodian at {} or above (code-passenger agent init)", root.display()) })
}

/// `agent init`: custodian_kit's agent layout, and code-passenger's templates.
pub fn init(root: &Path) -> Result<(Store, Vec<PathBuf>)> {
    let root = crate::gate::workspace_root(root)?;
    let (store, mut made) = Store::init(&root, Mode::Agent).map_err(custodian)?;
    for (kind, text) in TEMPLATES {
        let p = store.path(&named::JOB_TEMPLATE, &[("kind", kind)]).map_err(custodian)?;
        if !p.exists() {
            write_whole(&p, text.as_bytes()).map_err(custodian)?;
            made.push(p);
        }
    }
    Ok((store, made))
}

fn template(store: &Store, kind: &str) -> Result<JobTemplate> {
    let p = store.path(&named::JOB_TEMPLATE, &[("kind", kind)]).map_err(custodian)?;
    let text = std::fs::read_to_string(&p)
        .ok()
        .or_else(|| TEMPLATES.iter().find(|(k, _)| *k == kind).map(|(_, t)| t.to_string()))
        .ok_or_else(|| PassengerError::NoJobKind { detail: format!("`{kind}` ({})", p.display()) })?;
    let t: JobTemplate = toml::from_str(&text).map_err(|e| PassengerError::NoJobKind { detail: format!("{}: {e}", p.display()) })?;
    if t.kind != kind {
        return Err(PassengerError::NoJobKind { detail: format!("{} declares kind `{}`", p.display(), t.kind) });
    }
    Ok(t)
}

fn job_file(dir: &Path) -> JobFile {
    std::fs::read_to_string(dir.join("job.toml")).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
}

/// `agent queue`: a job folder in `jobs/queued/`.
pub fn queue(root: &Path, kind: &str, name: Option<&str>) -> Result<PathBuf> {
    let store = store_at(root)?;
    template(&store, kind)?;
    let name = name.map(str::to_string).unwrap_or_else(|| format!("{kind}-{}", stamp()));
    let dir = store.job_dir(JobState::Queued, &name).map_err(custodian)?;
    if dir.exists() {
        return Err(PassengerError::Custodian { detail: format!("{} is already queued", dir.display()) });
    }
    let jf = JobFile { id: None, name: Some(name), kind: Some(kind.to_string()), target: None, origin: None, ..Default::default() };
    write_whole(&dir.join("job.toml"), toml::to_string(&jf).map_err(PassengerError::toml)?.as_bytes()).map_err(custodian)?;
    Ok(dir)
}

/// The declared line a step runs.
fn step_line(step: &JobStep, root: &Path, folder: &Path, prior: Option<&Path>) -> Result<StepLine> {
    let root_s = root.as_os_str();
    Ok(match step.line.as_str() {
        "features-plan" => StepLine::Features(CodePassenger::features("plan").root(root_s).out(folder.join("readings").join(format!("{}.json", step.name)).as_os_str())),
        "features-apply" => {
            let mut l = CodePassenger::features("apply").root(root_s).write();
            if let Some(p) = prior {
                l = l.after(p.as_os_str());
            }
            StepLine::Features(l)
        }
        "annotate" => {
            let mut l = CodePassenger::annotate().root(root_s).write();
            if let Some(p) = prior {
                l = l.after(p.as_os_str());
            }
            StepLine::Annotate(l)
        }
        other => return Err(PassengerError::NoJobKind { detail: format!("step `{}`: `{other}` is not a line a job runs (features-plan, features-apply, annotate)", step.name) }),
    })
}

enum StepLine {
    Features(crate::lines::FeaturesRooted<CodePassenger>),
    Annotate(crate::lines::AnnotateRooted<CodePassenger>),
}

fn rel(store: &Store, p: &Path) -> String {
    p.strip_prefix(&store.root).unwrap_or(p).to_string_lossy().replace('\\', "/")
}

/// `agent run`: the queued job run to its close.
pub fn run(root: &Path, which: Option<&str>, exe: &Path) -> Result<(Closing, PathBuf)> {
    let store = store_at(root)?;
    let queued_dir = store.jobs_dir(JobState::Queued).map_err(custodian)?;
    let mut queued: Vec<PathBuf> = std::fs::read_dir(&queued_dir).map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect()).unwrap_or_default();
    queued.sort();
    let ours = |p: &PathBuf| job_file(p).kind.is_some_and(|k| TEMPLATES.iter().any(|(t, _)| *t == k));
    let folder = match which {
        Some(w) => queued.into_iter().find(|p| p.file_name().is_some_and(|n| n == w) || job_file(p).name.as_deref() == Some(w)),
        None => queued.into_iter().find(ours),
    }
    .ok_or_else(|| PassengerError::Custodian { detail: format!("no queued job {} in {}", which.unwrap_or("of a code-passenger kind"), queued_dir.display()) })?;
    let mut jf = job_file(&folder);
    let kind = jf.kind.clone().ok_or_else(|| PassengerError::NoJobKind { detail: format!("{} names no kind", folder.display()) })?;
    let t = template(&store, &kind)?;
    let folder_name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let name = jf.name.clone().unwrap_or_else(|| folder_name.clone());
    let opened = now_utc();
    let id = jf.id.clone().unwrap_or_else(|| format!("job-{}", &stable_digest::StableDigest::sha256(format!("{name}\n{opened}")).as_str()[..12]));
    let running = store.job_dir(JobState::Running, &folder_name).map_err(custodian)?;
    if running.exists() {
        return Err(PassengerError::Custodian { detail: format!("{} is already running", running.display()) });
    }
    if let Some(p) = running.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::rename(&folder, &running)?;
    jf.id = Some(id.clone());
    jf.name = Some(name.clone());
    write_whole(&running.join("job.toml"), toml::to_string(&jf).map_err(PassengerError::toml)?.as_bytes()).map_err(custodian)?;
    let journal = store.journal().map_err(custodian)?;
    journal.put_job(&JobRow { id: id.clone(), name: name.clone(), kind: kind.clone(), state: "running".into(), folder: rel(&store, &running), opened_utc: Some(opened.clone()), closed_utc: None }).map_err(custodian)?;
    let in_job = InJob { id: id.clone(), readings: running.join("readings") };
    let ws_root = store.root.clone();

    let mut steps: Vec<StepClosed> = Vec::new();
    let mut receipts: Vec<(String, PathBuf)> = Vec::new();
    let mut stopped_at = None;
    for step in &t.step {
        let prior = match &step.reads {
            None => None,
            Some(r) => Some(receipts.iter().find(|(n, _)| n == r).map(|(_, f)| f.clone()).ok_or_else(|| PassengerError::PriorReceipt { detail: format!("step `{}` reads `{r}`, which has not run before it", step.name) })?),
        };
        let sl = step_line(step, &ws_root, &running, prior.as_deref())?;
        let mut l = match &sl {
            StepLine::Features(x) => exec::line(&step.name, &exec::at(exe, x.clone())),
            StepLine::Annotate(x) => exec::line(&step.name, &exec::at(exe, x.clone())),
        }
        .map_err(custodian)?;
        l.env.push((JOB_ENV.to_string(), id.clone()));
        let plan = Plan::new(format!("job:{id}:{}", step.name)).then(l);
        let walked = plan.walk(Watch::Quiet, |_, _| {}).map_err(custodian)?;
        let w = walked.first().ok_or_else(|| PassengerError::Custodian { detail: format!("step `{}` did not start", step.name) })?;
        let out_file = running.join("readings").join(format!("{}.txt", step.name));
        write_whole(&out_file, format!("{}{}", w.stdout, w.stderr).as_bytes()).map_err(custodian)?;
        let mut outputs = Vec::new();
        if step.line == "features-plan" {
            outputs.push(rel(&store, &running.join("readings").join(format!("{}.json", step.name))));
        }
        outputs.push(rel(&store, &out_file));
        let r = receipt_of(&plan, w, "code-passenger", &step.name, None, None, None, None, outputs.clone(), Some(&in_job));
        let (r, file) = keep(&store, r).map_err(custodian)?;
        let word = match &sl {
            StepLine::Features(x) => xccute_contract::CompositeShellCommand::exit_status_of(x, w.code).map(|s| s.word.to_string()),
            StepLine::Annotate(x) => xccute_contract::CompositeShellCommand::exit_status_of(x, w.code).map(|s| s.word.to_string()),
        }
        .unwrap_or_else(|| "unnamed".into());
        steps.push(StepClosed { name: step.name.clone(), line: r.line.clone(), seq: r.seq, exit_code: r.exit_code, accepted: r.accepted, word, receipt: rel(&store, &file), outputs });
        receipts.push((step.name.clone(), file));
        if !r.accepted {
            stopped_at = Some(step.name.clone());
            break;
        }
    }

    let closing = Closing { schema: CLOSING_SCHEMA.into(), id: id.clone(), name: name.clone(), kind: kind.clone(), opened_utc: opened.clone(), closed_utc: now_utc(), done: stopped_at.is_none(), stopped_at, steps };
    let json = serde_json::to_string_pretty(&closing)? + "\n";
    write_whole(&running.join("closing.json"), json.as_bytes()).map_err(custodian)?;
    write_whole(&running.join("closing.md"), render_closing(&closing).as_bytes()).map_err(custodian)?;
    let done = store.job_dir(JobState::Done, &folder_name).map_err(custodian)?;
    if let Some(p) = done.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::rename(&running, &done)?;
    journal.put_job(&JobRow { id: id.clone(), name, kind, state: "done".into(), folder: rel(&store, &done), opened_utc: Some(opened), closed_utc: Some(closing.closed_utc.clone()) }).map_err(custodian)?;
    let digest = format!("sha256:{}", stable_digest::StableDigest::sha256(&json).as_str());
    let r = Receipt {
        schema: RECEIPT_SCHEMA.into(),
        seq: 0,
        utc: closing.closed_utc.clone(),
        command: "job-close".into(),
        name: id.clone(),
        spec: None,
        spec_hash: None,
        code_digest: None,
        index_key: None,
        plan_id: format!("job:{id}"),
        plan_digest: digest.clone(),
        operation_digest: digest.clone(),
        receipt_digest: digest,
        line: format!("code-passenger agent run {}", folder_name),
        exit_code: Some(if closing.done { 0 } else { 1 }),
        accepted: closing.done,
        disposition: "Stop".into(),
        reason: if closing.done { "closed".into() } else { format!("stopped at {}", closing.stopped_at.clone().unwrap_or_default()) },
        outputs: vec![rel(&store, &done.join("closing.md")), rel(&store, &done.join("closing.json"))],
        job: Some(id),
        reused_seq: None,
        millis: 0,
    };
    keep(&store, r).map_err(custodian)?;
    Ok((closing, done))
}

pub fn render_closing(c: &Closing) -> String {
    let mut s = format!("# job {} — {}\n\n- kind: {} · opened {} · closed {}\n", c.id, c.name, c.kind, c.opened_utc, c.closed_utc);
    match &c.stopped_at {
        None => s.push_str("- every step accepted\n"),
        Some(st) => s.push_str(&format!("- stopped at `{st}`\n")),
    }
    s.push_str("\n## steps\n\n");
    for st in &c.steps {
        s.push_str(&format!("- {} #{}: `{}` → exit {} ({}), receipt {}; {}\n", st.name, st.seq, st.line, st.exit_code.map(|c| c.to_string()).unwrap_or_else(|| "-".into()), st.word, st.receipt, st.outputs.join(", ")));
    }
    s
}

/// The `agent` line.
pub fn command(l: &AgentRooted<CodePassenger>, root: &Path, exe: &Path, out: &mut String, outputs: &mut Vec<PathBuf>) -> Result<i32> {
    let l = l.leaf();
    let name = l.name.as_ref().map(|s| s.to_string_lossy().into_owned());
    match l.action.to_string_lossy().as_ref() {
        "init" => {
            let (store, made) = init(root)?;
            out.push_str(&format!("agent mode at {}\n", store.root.display()));
            for p in made {
                out.push_str(&format!("  {}\n", rel(&store, &p)));
            }
            Ok(0)
        }
        "queue" => {
            let kind = l.kind.as_ref().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "features".into());
            let dir = queue(root, &kind, name.as_deref())?;
            out.push_str(&format!("queued {}\n", dir.display()));
            outputs.push(dir.join("job.toml"));
            Ok(0)
        }
        "run" => {
            let (c, done) = run(root, name.as_deref(), exe)?;
            out.push_str(&render_closing(&c));
            out.push_str(&format!("closing receipt: {}\n", done.join("closing.md").display()));
            outputs.push(done.join("closing.json"));
            Ok(if c.done { 0 } else { 1 })
        }
        other => Err(PassengerError::Line { detail: format!("agent {other}") }),
    }
}

//! Every command code-passenger takes, declared once as an xccute line
//! (simple_impl_derive's `CompositeShell` root, a `SimpleSubCommand` leaf
//! per command, its exit statuses named), and the one dispatcher that reads
//! the words of a run back into a declared line, validates it, runs the
//! library behind it and decides its exit status by the line's own policy.
//!
//! ```text
//! code-passenger <command> [--root R] ...
//! code-passenger --root R <command> ...      (the form custodian's facade writes)
//! ```
//!
//! Every run ends with a receipt (xccute's `RuntimeOperationReceipt` over
//! the plan of one line, its operation and its decision). With an
//! initialized `.custodian/` at the root or above, the receipt is kept in its
//! journal (`receipts/<seq>.json`); otherwise its digest is printed on stderr
//! and nothing is stored. A job step (`CODE_PASSENGER_JOB` set) leaves the
//! keeping to the job runner, whose executor started it.
//!
//! The oracle's `cargo check` is declared here too ([`Cargo`]).

use crate::error::{PassengerError, Result};
use simple_impl_derive::{CompositeShell, SimpleSubCommand};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use xccute_contract::{CommandValidationError, CommandValidationResult, CompositeShellCommand, ValidatedCommand};
use xccute_runtime::{ExitStatusPolicy, RuntimeExitStatus, RuntimeOperationReceipt};

/// The environment variable a job runner sets for its steps.
pub const JOB_ENV: &str = "CODE_PASSENGER_JOB";

/// code-passenger.
#[derive(CompositeShell, Debug, Clone, Copy, Default)]
#[shell(program = "code-passenger")]
#[composite(command = "scan", ty = Scan)]
#[composite(command = "annotate", ty = Annotate)]
#[composite(command = "scaffold", ty = Scaffold, init = "kind,name")]
#[composite(command = "passenger", ty = Passenger, init = "action")]
#[composite(command = "gate", ty = Gate, init = "spec")]
#[composite(command = "features", ty = Features, init = "action")]
#[composite(command = "feature-map", method = "feature_map", ty = FeatureMap)]
#[composite(command = "feature-sites", method = "feature_sites", ty = FeatureSites, init = "target")]
#[composite(command = "feature-probe", method = "feature_probe", ty = FeatureProbe, init = "target")]
#[composite(command = "receipt", ty = ReceiptLine)]
#[composite(command = "lint", ty = Lint)]
#[composite(command = "patterns", ty = Patterns)]
#[composite(command = "init", ty = Init)]
#[composite(command = "agent", ty = Agent, init = "action")]
#[composite(command = "dep", ty = Dep, init = "action,krate,dep")]
#[composite(command = "attr", ty = Attr, init = "action,file,item,line")]
pub struct CodePassenger;

/// `code-passenger --root ROOT WORD...`: a line whose root comes first, as
/// custodian's facade writes it; its words are one of the lines below.
#[derive(CompositeShell, Debug, Clone, Copy, Default)]
#[shell(program = "code-passenger")]
#[composite(command = "", method = "at_root", ty = PassengerAt, init = "root")]
pub struct CodePassengerAt;

#[allow(clippy::duplicated_attributes)]
#[derive(SimpleSubCommand, Debug, Clone, Default)]
#[subcommand(no_segment)]
#[exit_status(code = 0, word = "done")]
#[exit_status(code = 1, word = "work found, failed or gave up")]
#[exit_status(code = 2, word = "refused: a person decides")]
pub struct PassengerAt {
    #[builder(init_required, into)]
    #[arg(kv = "--root")]
    pub root: OsString,
    #[arg(positional)]
    pub words: Vec<OsString>,
}

fn word_of(field: &str, word: &OsStr, words: &[&str]) -> CommandValidationResult {
    match word.to_str() {
        Some(w) if words.contains(&w) => Ok(()),
        _ => Err(CommandValidationError::structural(format!("{word:?} is not a {field}: {}", words.join(", "))).with_field(field)),
    }
}

fn features_action(field: &str, word: &OsStr) -> CommandValidationResult {
    word_of(field, word, &["plan", "apply", "check"])
}

fn passenger_action(field: &str, word: &OsStr) -> CommandValidationResult {
    word_of(field, word, &["init", "branch", "checkout", "checkpoint", "log"])
}

fn agent_action(field: &str, word: &OsStr) -> CommandValidationResult {
    word_of(field, word, &["init", "queue", "run"])
}

fn dep_action(field: &str, word: &OsStr) -> CommandValidationResult {
    word_of(field, word, &["add", "remove"])
}

fn attr_action(field: &str, word: &OsStr) -> CommandValidationResult {
    word_of(field, word, &["place", "remove"])
}

fn header_plan(field: &str, word: &OsStr) -> CommandValidationResult {
    word_of(field, word, &["verbose", "compact"])
}

fn nonempty(field: &str, word: &OsStr) -> CommandValidationResult {
    if word.is_empty() {
        Err(CommandValidationError::structural(format!("{field} is empty")).with_field(field))
    } else {
        Ok(())
    }
}

/// `scan [--root R] [--src DIR] [--json]`.
#[derive(SimpleSubCommand, Debug, Clone, Default)]
#[subcommand(segment = "scan")]
#[exit_status(code = 0, word = "done")]
pub struct Scan {
    #[builder(into)]
    #[arg(kv = "--root")]
    pub root: Option<OsString>,
    #[builder(into)]
    #[arg(kv = "--src")]
    pub src: Option<OsString>,
    #[arg(flag = "--json")]
    pub json: bool,
}

/// `annotate [--root R] [--write] [--check] [--plan verbose|compact] [--lang rust] [--after RECEIPT]`.
#[allow(clippy::duplicated_attributes)]
#[derive(SimpleSubCommand, Debug, Clone, Default)]
#[subcommand(segment = "annotate")]
#[exit_status(code = 0, word = "headers current or written")]
#[exit_status(code = 1, word = "headers would change")]
#[exit_status(code = 2, word = "refused: the features plan is not applied")]
pub struct Annotate {
    #[builder(into)]
    #[arg(kv = "--root")]
    pub root: Option<OsString>,
    #[arg(flag = "--write")]
    #[conflicts_with("check")]
    pub write: bool,
    #[arg(flag = "--check")]
    pub check: bool,
    #[builder(into)]
    #[arg(kv = "--plan", each = header_plan)]
    pub plan: Option<OsString>,
    #[builder(into)]
    #[arg(kv = "--lang")]
    pub lang: Option<OsString>,
    /// A prior step's receipt (a job's apply step): it must have been accepted.
    #[builder(into)]
    #[arg(kv = "--after")]
    pub after: Option<OsString>,
}

/// `scaffold --kind K --name N [--root R] [--path P] [--scaffold-plan default] [--write]`.
#[derive(SimpleSubCommand, Debug, Clone, Default)]
#[subcommand(segment = "scaffold")]
#[exit_status(code = 0, word = "done")]
pub struct Scaffold {
    #[builder(into)]
    #[arg(kv = "--root")]
    pub root: Option<OsString>,
    #[builder(init_required, into)]
    #[arg(kv = "--kind", each = nonempty)]
    pub kind: OsString,
    #[builder(init_required, into)]
    #[arg(kv = "--name", each = nonempty)]
    pub name: OsString,
    #[builder(into)]
    #[arg(kv = "--path")]
    pub path: Option<OsString>,
    #[builder(into)]
    #[arg(kv = "--scaffold-plan")]
    pub scaffold_plan: Option<OsString>,
    #[arg(flag = "--write")]
    pub write: bool,
}

/// `passenger init|branch|checkout|checkpoint|log [NAME] [--root R] [--from ID] [--note N] [--branch B] [--n N] [--no-artifacts] [--force]`.
#[allow(clippy::duplicated_attributes)]
#[derive(SimpleSubCommand, Debug, Clone, Default)]
#[subcommand(segment = "passenger")]
#[exit_status(code = 0, word = "done")]
#[exit_status(code = 2, word = "refused: files would be overwritten")]
pub struct Passenger {
    #[builder(into)]
    #[arg(kv = "--root")]
    pub root: Option<OsString>,
    #[builder(init_required, into)]
    #[arg(positional, each = passenger_action)]
    pub action: OsString,
    /// The branch or snapshot (`branch`, `checkout`).
    #[builder(into)]
    #[arg(positional)]
    pub name: Option<OsString>,
    #[builder(into)]
    #[arg(kv = "--from")]
    pub from: Option<OsString>,
    #[builder(into)]
    #[arg(kv = "--note")]
    pub note: Option<OsString>,
    #[builder(into)]
    #[arg(kv = "--branch")]
    pub branch: Option<OsString>,
    #[builder(into)]
    #[arg(kv = "--n")]
    pub n: Option<OsString>,
    #[arg(flag = "--no-artifacts")]
    pub no_artifacts: bool,
    #[arg(flag = "--force")]
    pub force: bool,
}

/// `gate --spec S [--root R] [--package P] [--write] [--check] [--max-rounds N] [--no-cargo] [--unproven] [--json]`:
/// one package, a klepto.toml spec (0.2.0's loop).
#[allow(clippy::duplicated_attributes)]
#[derive(SimpleSubCommand, Debug, Clone, Default)]
#[subcommand(segment = "gate")]
#[exit_status(code = 0, word = "nothing to do or converged")]
#[exit_status(code = 1, word = "work found or the oracle gave up")]
#[exit_status(code = 2, word = "a person has to decide")]
pub struct Gate {
    #[builder(into)]
    #[arg(kv = "--root")]
    pub root: Option<OsString>,
    #[builder(init_required, into)]
    #[arg(kv = "--spec", each = nonempty)]
    pub spec: OsString,
    #[builder(into)]
    #[arg(kv = "--package")]
    pub package: Option<OsString>,
    #[arg(flag = "--write")]
    pub write: bool,
    #[arg(flag = "--check")]
    pub check: bool,
    #[builder(into)]
    #[arg(kv = "--max-rounds")]
    pub max_rounds: Option<OsString>,
    #[arg(flag = "--no-cargo")]
    pub no_cargo: bool,
    #[arg(flag = "--unproven")]
    pub unproven: bool,
    #[arg(flag = "--json")]
    pub json: bool,
}

/// `features plan|apply|check [--root R] [--write] [--no-cargo] [--unproven] [--max-rounds N] [--out FILE] [--after RECEIPT] [--json]`.
#[allow(clippy::duplicated_attributes)]
#[derive(SimpleSubCommand, Debug, Clone, Default)]
#[subcommand(segment = "features")]
#[exit_status(code = 0, word = "nothing to do, converged or written")]
#[exit_status(code = 1, word = "work found or the cargo check gave up")]
#[exit_status(code = 2, word = "refused: a person decides")]
pub struct Features {
    #[builder(into)]
    #[arg(kv = "--root")]
    pub root: Option<OsString>,
    #[builder(init_required, into)]
    #[arg(positional, each = features_action)]
    pub action: OsString,
    #[arg(flag = "--write")]
    pub write: bool,
    #[arg(flag = "--no-cargo")]
    pub no_cargo: bool,
    #[arg(flag = "--unproven")]
    pub unproven: bool,
    #[builder(into)]
    #[arg(kv = "--max-rounds")]
    pub max_rounds: Option<OsString>,
    #[builder(into)]
    #[arg(kv = "--out")]
    pub out: Option<OsString>,
    #[builder(into)]
    #[arg(kv = "--after")]
    pub after: Option<OsString>,
    #[arg(flag = "--json")]
    pub json: bool,
}

/// `feature-map [--root R] [--package P] [--json]`.
#[derive(SimpleSubCommand, Debug, Clone, Default)]
#[subcommand(segment = "feature-map")]
#[exit_status(code = 0, word = "done")]
pub struct FeatureMap {
    #[builder(into)]
    #[arg(kv = "--root")]
    pub root: Option<OsString>,
    #[builder(into)]
    #[arg(kv = "--package")]
    pub package: Option<OsString>,
    #[arg(flag = "--json")]
    pub json: bool,
}

/// `feature-sites TARGET [--root R] [--package P] [--json]`.
#[derive(SimpleSubCommand, Debug, Clone, Default)]
#[subcommand(segment = "feature-sites")]
#[exit_status(code = 0, word = "done")]
pub struct FeatureSites {
    #[builder(into)]
    #[arg(kv = "--root")]
    pub root: Option<OsString>,
    #[builder(init_required, into)]
    #[arg(positional, each = nonempty)]
    pub target: OsString,
    #[builder(into)]
    #[arg(kv = "--package")]
    pub package: Option<OsString>,
    #[arg(flag = "--json")]
    pub json: bool,
}

/// `feature-probe TARGET [--root R] [--package P] [--spec S] [--json]`.
#[derive(SimpleSubCommand, Debug, Clone, Default)]
#[subcommand(segment = "feature-probe")]
#[exit_status(code = 0, word = "done")]
pub struct FeatureProbe {
    #[builder(into)]
    #[arg(kv = "--root")]
    pub root: Option<OsString>,
    #[builder(init_required, into)]
    #[arg(positional, each = nonempty)]
    pub target: OsString,
    #[builder(into)]
    #[arg(kv = "--package")]
    pub package: Option<OsString>,
    #[builder(into)]
    #[arg(kv = "--spec")]
    pub spec: Option<OsString>,
    #[arg(flag = "--json")]
    pub json: bool,
}

/// `receipt [--root R] [--out FILE] [--kg DIR]`.
#[derive(SimpleSubCommand, Debug, Clone, Default)]
#[subcommand(segment = "receipt")]
#[exit_status(code = 0, word = "written")]
pub struct ReceiptLine {
    #[builder(into)]
    #[arg(kv = "--root")]
    pub root: Option<OsString>,
    #[builder(into)]
    #[arg(kv = "--out")]
    pub out: Option<OsString>,
    #[builder(into)]
    #[arg(kv = "--kg")]
    pub kg: Option<OsString>,
}

/// `lint [--root R] [--policy FILE] [--json]`.
#[allow(clippy::duplicated_attributes)]
#[derive(SimpleSubCommand, Debug, Clone, Default)]
#[subcommand(segment = "lint")]
#[exit_status(code = 0, word = "clean")]
#[exit_status(code = 1, word = "findings")]
pub struct Lint {
    #[builder(into)]
    #[arg(kv = "--root")]
    pub root: Option<OsString>,
    #[builder(into)]
    #[arg(kv = "--policy")]
    pub policy: Option<OsString>,
    #[arg(flag = "--json")]
    pub json: bool,
}

/// `patterns [--root R] [--min-tokens N] [--top N] [--json]`.
#[derive(SimpleSubCommand, Debug, Clone, Default)]
#[subcommand(segment = "patterns")]
#[exit_status(code = 0, word = "done")]
pub struct Patterns {
    #[builder(into)]
    #[arg(kv = "--root")]
    pub root: Option<OsString>,
    #[builder(into)]
    #[arg(kv = "--min-tokens")]
    pub min_tokens: Option<OsString>,
    #[builder(into)]
    #[arg(kv = "--top")]
    pub top: Option<OsString>,
    #[arg(flag = "--json")]
    pub json: bool,
}

/// `init [--root R] [--toml]`: the features document (and its schema where the language server reads it).
#[allow(clippy::duplicated_attributes)]
#[derive(SimpleSubCommand, Debug, Clone, Default)]
#[subcommand(segment = "init")]
#[exit_status(code = 0, word = "written")]
#[exit_status(code = 2, word = "refused: a document is already there")]
pub struct Init {
    #[builder(into)]
    #[arg(kv = "--root")]
    pub root: Option<OsString>,
    #[arg(flag = "--toml")]
    pub toml: bool,
}

/// `agent init|queue|run [NAME] [--root R] [--kind K]`.
#[allow(clippy::duplicated_attributes)]
#[derive(SimpleSubCommand, Debug, Clone, Default)]
#[subcommand(segment = "agent")]
#[exit_status(code = 0, word = "done")]
#[exit_status(code = 1, word = "a step stopped the job")]
#[exit_status(code = 2, word = "refused")]
pub struct Agent {
    #[builder(into)]
    #[arg(kv = "--root")]
    pub root: Option<OsString>,
    #[builder(init_required, into)]
    #[arg(positional, each = agent_action)]
    pub action: OsString,
    /// The job (`queue`, `run`).
    #[builder(into)]
    #[arg(positional)]
    pub name: Option<OsString>,
    /// The job kind (`queue`); `features` when not given.
    #[builder(into)]
    #[arg(kv = "--kind")]
    pub kind: Option<OsString>,
}

/// `dep add|remove CRATE DEP[@VERSION] [--root R] [--path P] [--features A,B] [--workspace-table]`:
/// a dependency of a crate, through toml_edit (custodian's `init --macros`).
#[allow(clippy::duplicated_attributes)]
#[derive(SimpleSubCommand, Debug, Clone, Default)]
#[subcommand(segment = "dep")]
#[exit_status(code = 0, word = "written or nothing to do")]
#[exit_status(code = 2, word = "refused")]
pub struct Dep {
    #[builder(into)]
    #[arg(kv = "--root")]
    pub root: Option<OsString>,
    #[builder(init_required, into)]
    #[arg(positional, each = dep_action)]
    pub action: OsString,
    #[builder(init_required, into)]
    #[arg(positional, each = nonempty)]
    pub krate: OsString,
    /// `name` or `name@version`.
    #[builder(init_required, into)]
    #[arg(positional, each = nonempty)]
    pub dep: OsString,
    #[builder(into)]
    #[arg(kv = "--path")]
    pub path: Option<OsString>,
    #[builder(into)]
    #[arg(kv = "--features")]
    pub features: Option<OsString>,
    #[arg(flag = "--workspace-table")]
    pub workspace_table: bool,
}

/// `attr place|remove --file F --item ID --line '#[attr(...)]' [--root R]`:
/// one attribute line on an item at klepto's location (custodian's `annotate --apply`).
#[allow(clippy::duplicated_attributes)]
#[derive(SimpleSubCommand, Debug, Clone, Default)]
#[subcommand(segment = "attr")]
#[exit_status(code = 0, word = "written or nothing to do")]
#[exit_status(code = 2, word = "refused")]
pub struct Attr {
    #[builder(into)]
    #[arg(kv = "--root")]
    pub root: Option<OsString>,
    #[builder(init_required, into)]
    #[arg(positional, each = attr_action)]
    pub action: OsString,
    #[builder(init_required, into)]
    #[arg(kv = "--file", each = nonempty)]
    pub file: OsString,
    #[builder(init_required, into)]
    #[arg(kv = "--item", each = nonempty)]
    pub item: OsString,
    #[builder(init_required, into)]
    #[arg(kv = "--line", each = nonempty)]
    pub line: OsString,
}

/// cargo, as the oracle runs it.
#[derive(CompositeShell, Debug, Clone, Copy, Default)]
#[shell(program = "cargo")]
#[composite(command = "check", ty = CargoCheck, init = "package")]
pub struct Cargo;

/// `cargo check -p PACKAGE --message-format=json [--no-default-features] [--features A,B] [--offline]`.
#[allow(clippy::duplicated_attributes)]
#[derive(SimpleSubCommand, Debug, Clone, Default)]
#[subcommand(segment = "check")]
#[exit_status(code = 0, word = "compiles")]
#[exit_status(code = 101, word = "does not compile")]
pub struct CargoCheck {
    #[builder(init_required, into)]
    #[arg(kv = "-p", each = nonempty)]
    pub package: OsString,
    #[arg(flag = "--message-format=json")]
    pub json: bool,
    #[arg(flag = "--no-default-features")]
    pub no_default: bool,
    #[builder(into)]
    #[arg(kv = "--features")]
    pub features: Option<OsString>,
    #[arg(flag = "--offline")]
    pub offline: bool,
}

/// The declared line for one job of klepto's check matrix.
pub fn cargo_check(job: &klepto::CheckJob, offline: bool) -> CargoCheckRooted<Cargo> {
    let mut line = Cargo::check(job.package.as_str()).json();
    if job.no_default {
        line = line.no_default();
    }
    if !job.features.is_empty() {
        line = line.features(job.features.join(","));
    }
    if offline {
        line = line.offline();
    }
    line
}

macro_rules! leaves {
    ($($leaf:ident => $rooted:ident),* $(,)?) => {
        $(
            impl $rooted<CodePassenger> {
                /// The leaf's fields, as the line holds them.
                pub fn leaf(&self) -> &$leaf {
                    &self.inner
                }
            }
        )*
    };
}

leaves! {
    Scan => ScanRooted,
    Annotate => AnnotateRooted,
    Scaffold => ScaffoldRooted,
    Passenger => PassengerRooted,
    Gate => GateRooted,
    Features => FeaturesRooted,
    FeatureMap => FeatureMapRooted,
    FeatureSites => FeatureSitesRooted,
    FeatureProbe => FeatureProbeRooted,
    ReceiptLine => ReceiptLineRooted,
    Lint => LintRooted,
    Patterns => PatternsRooted,
    Init => InitRooted,
    Agent => AgentRooted,
    Dep => DepRooted,
    Attr => AttrRooted,
}

// ---------------------------------------------------------------------------
// reading the words back into a line

/// The words of a run, consumed by name: flags, `--key value` pairs, positionals.
struct Words {
    flags: Vec<String>,
    kvs: Vec<(String, String)>,
    positionals: Vec<String>,
}

impl Words {
    fn read(words: &[&str], flags: &[&str], kvs: &[&str]) -> Result<Words> {
        let mut out = Words { flags: Vec::new(), kvs: Vec::new(), positionals: Vec::new() };
        let mut it = words.iter();
        while let Some(w) = it.next() {
            if flags.contains(w) {
                if out.flags.iter().any(|f| f == w) {
                    return Err(PassengerError::Line { detail: format!("{w} given twice") });
                }
                out.flags.push(w.to_string());
            } else if kvs.contains(w) {
                let v = it.next().ok_or_else(|| PassengerError::Line { detail: format!("{w} takes a value") })?;
                if out.kvs.iter().any(|(k, _)| k == w) {
                    return Err(PassengerError::Line { detail: format!("{w} given twice") });
                }
                out.kvs.push((w.to_string(), v.to_string()));
            } else if w.starts_with("--") {
                let mut known: Vec<&str> = flags.iter().chain(kvs.iter()).copied().collect();
                known.sort();
                return Err(PassengerError::Line { detail: format!("{w} is not an option here ({})", known.join(" ")) });
            } else {
                out.positionals.push(w.to_string());
            }
        }
        Ok(out)
    }

    fn flag(&self, f: &str) -> bool {
        self.flags.iter().any(|x| x == f)
    }

    fn kv(&self, k: &str) -> Option<String> {
        self.kvs.iter().find(|(x, _)| x == k).map(|(_, v)| v.clone())
    }

    fn positionals(&self, at_most: usize, what: &str) -> Result<Vec<String>> {
        if self.positionals.len() > at_most {
            return Err(PassengerError::Line { detail: format!("{} too many words ({what})", self.positionals[at_most..].join(" ")) });
        }
        Ok(self.positionals.clone())
    }
}

/// One declared line, read.
#[derive(Debug, Clone)]
pub enum Line {
    Scan(ScanRooted<CodePassenger>),
    Annotate(AnnotateRooted<CodePassenger>),
    Scaffold(ScaffoldRooted<CodePassenger>),
    Passenger(PassengerRooted<CodePassenger>),
    Gate(GateRooted<CodePassenger>),
    Features(FeaturesRooted<CodePassenger>),
    FeatureMap(FeatureMapRooted<CodePassenger>),
    FeatureSites(FeatureSitesRooted<CodePassenger>),
    FeatureProbe(FeatureProbeRooted<CodePassenger>),
    Receipt(ReceiptLineRooted<CodePassenger>),
    Lint(LintRooted<CodePassenger>),
    Patterns(PatternsRooted<CodePassenger>),
    Init(InitRooted<CodePassenger>),
    Agent(AgentRooted<CodePassenger>),
    Dep(DepRooted<CodePassenger>),
    Attr(AttrRooted<CodePassenger>),
}

macro_rules! each_line {
    ($self:expr, $l:ident => $body:expr) => {
        match $self {
            Line::Scan($l) => $body,
            Line::Annotate($l) => $body,
            Line::Scaffold($l) => $body,
            Line::Passenger($l) => $body,
            Line::Gate($l) => $body,
            Line::Features($l) => $body,
            Line::FeatureMap($l) => $body,
            Line::FeatureSites($l) => $body,
            Line::FeatureProbe($l) => $body,
            Line::Receipt($l) => $body,
            Line::Lint($l) => $body,
            Line::Patterns($l) => $body,
            Line::Init($l) => $body,
            Line::Agent($l) => $body,
            Line::Dep($l) => $body,
            Line::Attr($l) => $body,
        }
    };
}

impl Line {
    pub fn command(&self) -> &'static str {
        match self {
            Line::Scan(_) => "scan",
            Line::Annotate(_) => "annotate",
            Line::Scaffold(_) => "scaffold",
            Line::Passenger(_) => "passenger",
            Line::Gate(_) => "gate",
            Line::Features(_) => "features",
            Line::FeatureMap(_) => "feature-map",
            Line::FeatureSites(_) => "feature-sites",
            Line::FeatureProbe(_) => "feature-probe",
            Line::Receipt(_) => "receipt",
            Line::Lint(_) => "lint",
            Line::Patterns(_) => "patterns",
            Line::Init(_) => "init",
            Line::Agent(_) => "agent",
            Line::Dep(_) => "dep",
            Line::Attr(_) => "attr",
        }
    }

    pub fn display(&self) -> String {
        each_line!(self, l => l.build_display())
    }

    pub fn argv(&self) -> Vec<OsString> {
        each_line!(self, l => l.argv())
    }

    pub fn validate(&self) -> CommandValidationResult {
        each_line!(self, l => l.validate())
    }

    /// The root the line names (`.` when it names none).
    pub fn root(&self) -> PathBuf {
        let r: Option<&OsString> = each_line!(self, l => l.inner.root.as_ref());
        r.map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
    }

    fn with_root(self, root: &str) -> Line {
        match self {
            Line::Scan(l) => Line::Scan(l.root(root)),
            Line::Annotate(l) => Line::Annotate(l.root(root)),
            Line::Scaffold(l) => Line::Scaffold(l.root(root)),
            Line::Passenger(l) => Line::Passenger(l.root(root)),
            Line::Gate(l) => Line::Gate(l.root(root)),
            Line::Features(l) => Line::Features(l.root(root)),
            Line::FeatureMap(l) => Line::FeatureMap(l.root(root)),
            Line::FeatureSites(l) => Line::FeatureSites(l.root(root)),
            Line::FeatureProbe(l) => Line::FeatureProbe(l.root(root)),
            Line::Receipt(l) => Line::Receipt(l.root(root)),
            Line::Lint(l) => Line::Lint(l.root(root)),
            Line::Patterns(l) => Line::Patterns(l.root(root)),
            Line::Init(l) => Line::Init(l.root(root)),
            Line::Agent(l) => Line::Agent(l.root(root)),
            Line::Dep(l) => Line::Dep(l.root(root)),
            Line::Attr(l) => Line::Attr(l.root(root)),
        }
    }
}

macro_rules! opt {
    ($line:ident, $w:ident, $flag:literal, $setter:ident) => {
        if let Some(v) = $w.kv($flag) {
            $line = $line.$setter(v);
        }
    };
}

macro_rules! flag {
    ($line:ident, $w:ident, $flag:literal, $setter:ident) => {
        if $w.flag($flag) {
            $line = $line.$setter();
        }
    };
}

/// The words after `code-passenger`, read into the declared line they spell.
pub fn read(args: &[OsString]) -> Result<Line> {
    let words: Vec<&str> = args.iter().map(|a| a.to_str()).collect::<Option<Vec<_>>>().ok_or_else(|| PassengerError::Line { detail: "a word is not UTF-8".into() })?;
    // the root-first form: `--root R <line>`
    if words.first() == Some(&"--root") {
        let root = *words.get(1).ok_or_else(|| PassengerError::Line { detail: "--root takes a value".into() })?;
        let inner: Vec<OsString> = args[2..].to_vec();
        let line = read(&inner)?;
        if line.root() != Path::new(".") || inner.iter().any(|w| w == "--root") {
            return Err(PassengerError::Line { detail: "--root is given twice".into() });
        }
        let at = CodePassengerAt::at_root(root).words(line.argv());
        at.validate().map_err(|e| PassengerError::Line { detail: e.to_string() })?;
        return Ok(line.with_root(root));
    }
    let (cmd, rest) = words.split_first().ok_or_else(|| PassengerError::Line { detail: "no command; the commands are scan, annotate, scaffold, passenger, gate, features, feature-map, feature-sites, feature-probe, receipt, lint, patterns, init, agent, dep, attr".into() })?;
    let line = match *cmd {
        "scan" => {
            let w = Words::read(rest, &["--json"], &["--root", "--src"])?;
            w.positionals(0, "scan")?;
            let mut l = CodePassenger::scan();
            opt!(l, w, "--root", root);
            opt!(l, w, "--src", src);
            flag!(l, w, "--json", json);
            Line::Scan(l)
        }
        "annotate" => {
            let w = Words::read(rest, &["--write", "--check"], &["--root", "--plan", "--lang", "--after"])?;
            w.positionals(0, "annotate")?;
            let mut l = CodePassenger::annotate();
            opt!(l, w, "--root", root);
            flag!(l, w, "--write", write);
            flag!(l, w, "--check", check);
            opt!(l, w, "--plan", plan);
            opt!(l, w, "--lang", lang);
            opt!(l, w, "--after", after);
            Line::Annotate(l)
        }
        "scaffold" => {
            let w = Words::read(rest, &["--write"], &["--root", "--kind", "--name", "--path", "--scaffold-plan"])?;
            w.positionals(0, "scaffold")?;
            let kind = w.kv("--kind").ok_or_else(|| PassengerError::Line { detail: "scaffold needs --kind".into() })?;
            let name = w.kv("--name").ok_or_else(|| PassengerError::Line { detail: "scaffold needs --name".into() })?;
            let mut l = CodePassenger::scaffold(kind, name);
            opt!(l, w, "--root", root);
            opt!(l, w, "--path", path);
            opt!(l, w, "--scaffold-plan", scaffold_plan);
            flag!(l, w, "--write", write);
            Line::Scaffold(l)
        }
        "passenger" => {
            let w = Words::read(rest, &["--no-artifacts", "--force"], &["--root", "--from", "--note", "--branch", "--n"])?;
            let mut p = w.positionals(3, "passenger ACTION [NAME]")?;
            // 0.2.0 spelled a new branch `branch create NAME`
            if p.first().map(String::as_str) == Some("branch") && p.get(1).map(String::as_str) == Some("create") {
                p.remove(1);
            }
            if p.len() > 2 {
                return Err(PassengerError::Line { detail: format!("{} too many words (passenger ACTION [NAME])", p[2..].join(" ")) });
            }
            let action = p.first().cloned().ok_or_else(|| PassengerError::Line { detail: "passenger needs an action: init, branch, checkout, checkpoint, log".into() })?;
            let mut l = CodePassenger::passenger(action);
            if let Some(n) = p.get(1) {
                l = l.name(n.clone());
            }
            opt!(l, w, "--root", root);
            opt!(l, w, "--from", from);
            opt!(l, w, "--note", note);
            opt!(l, w, "--branch", branch);
            opt!(l, w, "--n", n);
            flag!(l, w, "--no-artifacts", no_artifacts);
            flag!(l, w, "--force", force);
            Line::Passenger(l)
        }
        "gate" => {
            let w = Words::read(rest, &["--write", "--check", "--no-cargo", "--unproven", "--json"], &["--root", "--spec", "--package", "--max-rounds"])?;
            w.positionals(0, "gate")?;
            let spec = w.kv("--spec").ok_or_else(|| PassengerError::Line { detail: "gate needs --spec".into() })?;
            let mut l = CodePassenger::gate(spec);
            opt!(l, w, "--root", root);
            opt!(l, w, "--package", package);
            flag!(l, w, "--write", write);
            flag!(l, w, "--check", check);
            opt!(l, w, "--max-rounds", max_rounds);
            flag!(l, w, "--no-cargo", no_cargo);
            flag!(l, w, "--unproven", unproven);
            flag!(l, w, "--json", json);
            Line::Gate(l)
        }
        "features" => {
            let w = Words::read(rest, &["--write", "--no-cargo", "--unproven", "--json"], &["--root", "--max-rounds", "--out", "--after"])?;
            let p = w.positionals(1, "features plan|apply|check")?;
            let action = p.first().cloned().ok_or_else(|| PassengerError::Line { detail: "features needs plan, apply or check".into() })?;
            let mut l = CodePassenger::features(action);
            opt!(l, w, "--root", root);
            flag!(l, w, "--write", write);
            flag!(l, w, "--no-cargo", no_cargo);
            flag!(l, w, "--unproven", unproven);
            opt!(l, w, "--max-rounds", max_rounds);
            opt!(l, w, "--out", out);
            opt!(l, w, "--after", after);
            flag!(l, w, "--json", json);
            Line::Features(l)
        }
        "feature-map" => {
            let w = Words::read(rest, &["--json"], &["--root", "--package"])?;
            w.positionals(0, "feature-map")?;
            let mut l = CodePassenger::feature_map();
            opt!(l, w, "--root", root);
            opt!(l, w, "--package", package);
            flag!(l, w, "--json", json);
            Line::FeatureMap(l)
        }
        "feature-sites" | "feature-probe" => {
            let probe = *cmd == "feature-probe";
            let kvs: &[&str] = if probe { &["--root", "--package", "--spec"] } else { &["--root", "--package"] };
            let w = Words::read(rest, &["--json"], kvs)?;
            let p = w.positionals(1, "TARGET")?;
            let target = p.first().cloned().ok_or_else(|| PassengerError::Line { detail: format!("{cmd} needs a dependency or feature name") })?;
            if probe {
                let mut l = CodePassenger::feature_probe(target);
                opt!(l, w, "--root", root);
                opt!(l, w, "--package", package);
                opt!(l, w, "--spec", spec);
                flag!(l, w, "--json", json);
                Line::FeatureProbe(l)
            } else {
                let mut l = CodePassenger::feature_sites(target);
                opt!(l, w, "--root", root);
                opt!(l, w, "--package", package);
                flag!(l, w, "--json", json);
                Line::FeatureSites(l)
            }
        }
        "receipt" => {
            let w = Words::read(rest, &[], &["--root", "--out", "--kg"])?;
            w.positionals(0, "receipt")?;
            let mut l = CodePassenger::receipt();
            opt!(l, w, "--root", root);
            opt!(l, w, "--out", out);
            opt!(l, w, "--kg", kg);
            Line::Receipt(l)
        }
        "lint" => {
            let w = Words::read(rest, &["--json"], &["--root", "--policy"])?;
            w.positionals(0, "lint")?;
            let mut l = CodePassenger::lint();
            opt!(l, w, "--root", root);
            opt!(l, w, "--policy", policy);
            flag!(l, w, "--json", json);
            Line::Lint(l)
        }
        "patterns" => {
            let w = Words::read(rest, &["--json"], &["--root", "--min-tokens", "--top"])?;
            w.positionals(0, "patterns")?;
            let mut l = CodePassenger::patterns();
            opt!(l, w, "--root", root);
            opt!(l, w, "--min-tokens", min_tokens);
            opt!(l, w, "--top", top);
            flag!(l, w, "--json", json);
            Line::Patterns(l)
        }
        "init" => {
            let w = Words::read(rest, &["--toml"], &["--root"])?;
            w.positionals(0, "init")?;
            let mut l = CodePassenger::init();
            opt!(l, w, "--root", root);
            flag!(l, w, "--toml", toml);
            Line::Init(l)
        }
        "agent" => {
            let w = Words::read(rest, &[], &["--root", "--kind"])?;
            let p = w.positionals(2, "agent init|queue|run [NAME]")?;
            let action = p.first().cloned().ok_or_else(|| PassengerError::Line { detail: "agent needs init, queue or run".into() })?;
            let mut l = CodePassenger::agent(action);
            if let Some(n) = p.get(1) {
                l = l.name(n.clone());
            }
            opt!(l, w, "--root", root);
            opt!(l, w, "--kind", kind);
            Line::Agent(l)
        }
        "dep" => {
            let w = Words::read(rest, &["--workspace-table"], &["--root", "--path", "--features"])?;
            let p = w.positionals(3, "dep add|remove CRATE DEP[@VERSION]")?;
            let [action, krate, dep] = p.as_slice() else {
                return Err(PassengerError::Line { detail: "dep add|remove CRATE DEP[@VERSION]".into() });
            };
            let mut l = CodePassenger::dep(action.as_str(), krate.as_str(), dep.as_str());
            opt!(l, w, "--root", root);
            opt!(l, w, "--path", path);
            opt!(l, w, "--features", features);
            flag!(l, w, "--workspace-table", workspace_table);
            Line::Dep(l)
        }
        "attr" => {
            let w = Words::read(rest, &[], &["--root", "--file", "--item", "--line"])?;
            let p = w.positionals(1, "attr place|remove")?;
            let action = p.first().cloned().ok_or_else(|| PassengerError::Line { detail: "attr needs place or remove".into() })?;
            let need = |k: &str| w.kv(k).ok_or_else(|| PassengerError::Line { detail: format!("attr {action} needs {k}") });
            let mut l = CodePassenger::attr(action.as_str(), need("--file")?, need("--item")?, need("--line")?);
            opt!(l, w, "--root", root);
            Line::Attr(l)
        }
        other => return Err(PassengerError::Line { detail: format!("`{other}` is not a command") }),
    };
    line.validate().map_err(|e| PassengerError::Line { detail: e.to_string() })?;
    Ok(line)
}

// ---------------------------------------------------------------------------
// running a line

/// A line run: its exit status, the word its line names for it, what it printed, its receipt.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub code: i32,
    pub word: String,
    pub stdout: String,
    pub stderr: String,
    pub receipt: Option<RuntimeOperationReceipt>,
    /// The journal's receipt file, when the run was kept.
    pub kept: Option<PathBuf>,
}

fn os(v: &Option<OsString>) -> Option<String> {
    v.as_ref().map(|s| s.to_string_lossy().into_owned())
}

fn num(v: &Option<OsString>, default: usize, what: &str) -> Result<usize> {
    match os(v) {
        None => Ok(default),
        Some(s) => s.parse().map_err(|_| PassengerError::Line { detail: format!("{what} takes a number, not `{s}`") }),
    }
}

/// Runs a read line in this process; returns its exit status, what it
/// printed, and the files it wrote (for its receipt).
fn dispatch(line: &Line, exe: &Path) -> Result<(i32, String, Vec<PathBuf>)> {
    use crate::commands;
    let root = line.root();
    let mut out = String::new();
    let mut outputs = Vec::new();
    let code = match line {
        Line::Scan(l) => {
            let l = l.leaf();
            let ctx = crate::engine::RunContext { root: root.clone(), src_rel: os(&l.src).unwrap_or_default(), manifest_path: root.join("Cargo.toml"), lang: "rust".into() };
            let scan = crate::engine::run_scan(&ctx)?;
            if l.json {
                let file = root.join("report.json");
                std::fs::write(&file, serde_json::to_string_pretty(&crate::analysis::run(&scan))?)?;
                out.push_str(&format!("Saved report to {}\n", file.display()));
                outputs.push(file);
            } else {
                for r in &scan.reports {
                    out.push_str(&format!("== {} ==\nused_deps: {:?}\nused_mods: {:?}\nuse_sites: external={}, internal={}\n\n", r.document.relative_path, r.used.packages, r.used.modules, r.external_use_sites.len(), r.internal_use_sites.len()));
                }
            }
            0
        }
        Line::Annotate(l) => {
            let l = l.leaf();
            if let Some(after) = os(&l.after) {
                prior_accepted(Path::new(&after))?;
            }
            let opts = commands::annotate::AnnotateOptions { write: l.write, check: l.check, plan: os(&l.plan).unwrap_or_else(|| "verbose".into()), lang: os(&l.lang).unwrap_or_else(|| "rust".into()) };
            let o = commands::annotate::run(&root, &opts)?;
            out.push_str(&commands::annotate::render(&o));
            if o.written {
                outputs.extend(o.files.iter().filter(|(_, c)| *c).map(|(f, _)| o.root.join(f)));
            }
            o.exit_code
        }
        Line::Scaffold(l) => {
            let l = l.leaf();
            let pack = crate::packs::get_pack("rust")?;
            let plan = crate::scaffolds::get_scaffold_plan(&os(&l.scaffold_plan).unwrap_or_else(|| "default".into()))?;
            let kind_s = l.kind.to_string_lossy().into_owned();
            let kind = crate::model::ScaffoldKind::parse(&kind_s).ok_or_else(|| PassengerError::unsupported(format!("unknown scaffold kind '{kind_s}'")))?;
            let module_path: Vec<String> = os(&l.path).unwrap_or_default().split('/').filter(|s| !s.is_empty()).map(str::to_string).collect();
            let made = pack.scaffold(plan.as_ref(), crate::model::ScaffoldRequest { kind, name: l.name.to_string_lossy().into_owned(), module_path })?;
            for (rel, content) in made.files {
                let abs = root.join(rel);
                if l.write {
                    if let Some(parent) = abs.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::write(&abs, content)?;
                    outputs.push(abs);
                } else {
                    out.push_str(&format!("--- {}\n{content}\n", abs.display()));
                }
            }
            0
        }
        Line::Passenger(l) => crate::passenger::command::run(l, &root, &mut out)?,
        Line::Gate(l) => {
            let l = l.leaf();
            crate::words::PlanWord::of(l.write, l.check, l.no_cargo, l.unproven).checked()?;
            let opts = commands::gate::GateOptions { spec: PathBuf::from(&l.spec), package: os(&l.package), write: l.write, check: l.check, max_rounds: num(&l.max_rounds, 3, "--max-rounds")?, no_cargo: l.no_cargo };
            let o = commands::gate::run(&root, &opts)?;
            if l.json {
                out.push_str(&(serde_json::to_string_pretty(&o.plan)? + "\n"));
            } else {
                out.push_str(&commands::gate::render(&o));
            }
            o.exit_code
        }
        Line::Features(l) => {
            let l = l.leaf();
            let action = crate::workspace::Action::of_word(&l.action.to_string_lossy()).ok_or_else(|| PassengerError::Line { detail: "features plan|apply|check".into() })?;
            let opts = crate::workspace::FeaturesOptions {
                word: crate::words::PlanWord::of(l.write, action == crate::workspace::Action::Check, l.no_cargo, l.unproven),
                max_rounds: num(&l.max_rounds, 3, "--max-rounds")?,
                out: os(&l.out).map(PathBuf::from),
                after: os(&l.after).map(PathBuf::from),
            };
            let o = crate::workspace::run(&root, action, &opts)?;
            if l.json {
                out.push_str(&(serde_json::to_string_pretty(&o.plan)? + "\n"));
            } else {
                out.push_str(&crate::workspace::render(&o));
            }
            outputs.extend(opts.out.clone());
            if o.written {
                outputs.extend(o.edits.files.iter().filter(|f| f.before != f.after).map(|f| f.path.clone()));
            }
            o.exit_code
        }
        Line::FeatureMap(l) => {
            let l = l.leaf();
            let m = commands::features::feature_map(&root, os(&l.package).as_deref())?;
            out.push_str(&if l.json { serde_json::to_string_pretty(&m)? + "\n" } else { commands::features::render_map(&m) });
            0
        }
        Line::FeatureSites(l) => {
            let l = l.leaf();
            let s = commands::features::feature_sites(&root, os(&l.package).as_deref(), &l.target.to_string_lossy())?;
            out.push_str(&if l.json { serde_json::to_string_pretty(&s)? + "\n" } else { commands::features::render_sites(&s) });
            0
        }
        Line::FeatureProbe(l) => {
            let l = l.leaf();
            let spec = os(&l.spec).map(PathBuf::from);
            let p = commands::features::feature_probe(&root, os(&l.package).as_deref(), &l.target.to_string_lossy(), spec.as_deref())?;
            out.push_str(&if l.json { serde_json::to_string_pretty(&p)? + "\n" } else { commands::features::render_probe(&p) });
            0
        }
        Line::Receipt(l) => {
            let l = l.leaf();
            let file = os(&l.out).map(PathBuf::from).unwrap_or_else(|| root.join("receipt.json"));
            let w = commands::receipt::write(&root, &file, os(&l.kg).map(PathBuf::from).as_deref())?;
            out.push_str(&format!("wrote {}\n", w.receipt.display()));
            outputs.push(w.receipt.clone());
            for p in w.knowledge.iter().chain(w.challenges.iter()) {
                out.push_str(&format!("wrote {}\n", p.display()));
                outputs.push(p.clone());
            }
            0
        }
        Line::Lint(l) => {
            let l = l.leaf();
            let v = commands::lint::run(&root, os(&l.policy).map(PathBuf::from).as_deref())?;
            out.push_str(&if l.json { serde_json::to_string_pretty(&v)? + "\n" } else { v.summary() + "\n" });
            v.exit_code
        }
        Line::Patterns(l) => {
            let l = l.leaf();
            let r = commands::patterns::run(&root, &klepto::MiningOptions { min_tokens: num(&l.min_tokens, 16, "--min-tokens")?, min_members: 2 })?;
            out.push_str(&if l.json { serde_json::to_string_pretty(&r)? + "\n" } else { commands::patterns::render(&r, num(&l.top, 20, "--top")?) });
            0
        }
        Line::Init(l) => {
            let l = l.leaf();
            let w = crate::init::init(&root, if l.toml { crate::doc::Twin::Toml } else { crate::doc::Twin::OneO })?;
            for p in [&w.doc, &w.schema, &w.project] {
                out.push_str(&format!("wrote {}\n", p.display()));
                outputs.push(p.clone());
            }
            0
        }
        Line::Agent(l) => crate::agent::command(l, &root, exe, &mut out, &mut outputs)?,
        Line::Dep(l) => {
            let l = l.leaf();
            let features: Vec<String> = os(&l.features).map(|f| f.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect()).unwrap_or_default();
            let (krate, dep) = (l.krate.to_string_lossy().into_owned(), l.dep.to_string_lossy().into_owned());
            let e = match l.action.to_string_lossy().as_ref() {
                "add" => commands::edit::dep_add(&root, &krate, &dep, os(&l.path).as_deref(), &features, l.workspace_table)?,
                _ => commands::edit::dep_remove(&root, &krate, &dep, l.workspace_table)?,
            };
            out.push_str(&e.render());
            outputs.extend(e.edits.files.iter().filter(|f| f.before != f.after).map(|f| f.path.clone()));
            0
        }
        Line::Attr(l) => {
            let l = l.leaf();
            let (file, item, attr) = (PathBuf::from(&l.file), l.item.to_string_lossy().into_owned(), l.line.to_string_lossy().into_owned());
            let e = match l.action.to_string_lossy().as_ref() {
                "place" => commands::edit::attr_place(&root, &file, &item, &attr)?,
                _ => commands::edit::attr_remove(&root, &file, &item, &attr)?,
            };
            out.push_str(&e.render());
            outputs.extend(e.edits.files.iter().filter(|f| f.before != f.after).map(|f| f.path.clone()));
            0
        }
    };
    Ok((code, out, outputs))
}

/// A prior step's receipt (custodian's form) that must have been accepted.
pub fn prior_accepted(after: &Path) -> Result<custodian_kit::store::Receipt> {
    let text = std::fs::read_to_string(after).map_err(|e| PassengerError::PriorReceipt { detail: format!("{}: {e}", after.display()) })?;
    let r: custodian_kit::store::Receipt = serde_json::from_str(&text).map_err(|e| PassengerError::PriorReceipt { detail: format!("{}: {e}", after.display()) })?;
    if !r.accepted {
        return Err(PassengerError::PriorReceipt { detail: format!("run #{} ({}) was not accepted: {}", r.seq, r.line, r.reason) });
    }
    Ok(r)
}

/// The decision and receipt of a line that ran in this process, and the
/// journal's copy when the project keeps one.
fn receipt_for(line: &Line, code: i32, stdout: &str, outputs: &[PathBuf], millis: i64) -> (String, Option<RuntimeOperationReceipt>, Option<PathBuf>) {
    use custodian_kit::exec;
    let (word, decided) = each_line!(line, l => {
        let accepted = l.ok_codes().contains(&code);
        let decision = ExitStatusPolicy::of(l).decide(RuntimeExitStatus::new(Some(code), accepted));
        let word = l.exit_status_of(Some(code)).map(|s| s.word.to_string()).unwrap_or_else(|| "unnamed".into());
        let plan = exec::line(line.command(), l).map(|ln| exec::Plan::new(format!("code-passenger:{}", line.command())).then(ln));
        (word, plan.ok().map(|p| (p, decision)))
    });
    let Some((plan, decision)) = decided else { return (word, None, None) };
    let Some(op) = plan.plan.operations.first().cloned() else { return (word, None, None) };
    let receipt = RuntimeOperationReceipt::new(&plan.plan, &op, decision.clone());
    if std::env::var_os(JOB_ENV).is_some() {
        return (word, Some(receipt), None);
    }
    let root = line.root();
    let Some(store) = custodian_kit::store::Store::find(&root) else { return (word, Some(receipt), None) };
    let walked = exec::Walked { operation: op, receipt: receipt.clone(), decision, code: Some(code), stdout: stdout.to_string(), stderr: String::new(), millis };
    let rel = |p: &PathBuf| p.strip_prefix(&store.root).unwrap_or(p).to_string_lossy().replace('\\', "/");
    let r = custodian_kit::runs::receipt_of(&plan, &walked, "code-passenger", line.command(), None, None, None, None, outputs.iter().map(rel).collect(), None);
    match custodian_kit::runs::keep(&store, r) {
        Ok((_, file)) => (word, Some(receipt), Some(file)),
        Err(_) => (word, Some(receipt), None),
    }
}

/// The words after `code-passenger`, run: read, validated, dispatched,
/// decided. `exe` is this program (an agent job's steps run it).
pub fn run_argv(args: &[OsString], exe: &Path) -> Outcome {
    let started = std::time::Instant::now();
    let line = match read(args) {
        Ok(l) => l,
        Err(e) => return Outcome { code: e.exit_code(), word: "refused".into(), stdout: String::new(), stderr: format!("{e}\n"), receipt: None, kept: None },
    };
    let (code, stdout, stderr, outputs) = match dispatch(&line, exe) {
        Ok((code, out, outputs)) => (code, out, String::new(), outputs),
        Err(e) => (e.exit_code(), String::new(), format!("{e}\n"), Vec::new()),
    };
    let (word, receipt, kept) = receipt_for(&line, code, &stdout, &outputs, started.elapsed().as_millis() as i64);
    Outcome { code, word, stdout, stderr, receipt, kept }
}

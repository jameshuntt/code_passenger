//! Every error and refusal code_passenger gives, declared once as liaise
//! codes: `[CP0001]` and its sentence. The code is what a script matches on;
//! the sentence is what a person reads; the detail after it says what.
//!
//! A refusal ends a line with exit status 2 (`PassengerError::exit_code`); every
//! other error ends it with 1.

use liaise::{Liaise, LiaiseCodes};

#[derive(Debug, LiaiseCodes)]
#[liaise(prefix = "CP", domain = "code_passenger")]
pub enum PassengerError {
    #[liaise(code = 1, msg = "a file cannot be read or written: {detail}")]
    Io { detail: String },
    #[liaise(code = 2, msg = "a JSON document cannot be read or written: {detail}")]
    Json { detail: String },
    #[liaise(code = 3, msg = "a TOML document is not valid: {detail}")]
    Toml { detail: String },
    #[liaise(code = 4, msg = "a path is not where it must be: {detail}")]
    Path { detail: String },
    #[liaise(code = 5, msg = "not supported here: {detail}")]
    Unsupported { detail: String },
    #[liaise(code = 6, msg = "klepto could not scan the workspace: {detail}")]
    Scan { detail: String },
    #[liaise(code = 7, msg = "the headers are not written yet (run annotate --write)")]
    ChangesNeeded,

    // the workspace features document
    #[liaise(code = 10, msg = "the features document is not valid: {detail}")]
    FeaturesDoc { detail: String },
    #[liaise(code = 11, msg = "no features document at the workspace root (features.1o or features.toml; code-passenger init writes one): {detail}")]
    NoFeaturesDoc { detail: String },
    #[liaise(code = 12, msg = "the workspace root holds both features.1o and features.toml; keep one: {detail}")]
    TwoFeaturesDocs { detail: String },
    #[liaise(code = 13, msg = "a features document is already there; init never overwrites one: {detail}")]
    FeaturesDocExists { detail: String },
    #[liaise(code = 14, msg = "the features document names a crate or a dependency the workspace does not have: {detail}")]
    UnknownName { detail: String },

    // refusals of a plan (exit 2)
    #[liaise(code = 20, msg = "a person has to decide: {detail}")]
    PersonDecides { detail: String },
    #[liaise(code = 21, msg = "the plan was not proven by cargo check; --write --no-cargo writes it only with --unproven")]
    Unproven,
    #[liaise(code = 22, msg = "the plan flags break the word's own rules: {detail}")]
    PlanFlags { detail: String },
    #[liaise(code = 23, msg = "the features plan is not applied yet, so the headers would describe gates the code does not have (run features apply --write first): {detail}")]
    PlanNotApplied { detail: String },
    #[liaise(code = 24, msg = "the receipt this step reads is missing or does not hold what it needs: {detail}")]
    PriorReceipt { detail: String },
    #[liaise(code = 25, msg = "the code changed since the plan was made; plan again: {detail}")]
    StalePlan { detail: String },

    // the declared lines (exit 2)
    #[liaise(code = 30, msg = "the line is not a declared code-passenger line: {detail}")]
    Line { detail: String },

    // the passenger store
    #[liaise(code = 40, msg = "no passenger store for this workspace (code-passenger passenger init): {detail}")]
    NoStore { detail: String },
    #[liaise(code = 41, msg = "no such branch or snapshot: {detail}")]
    NoSnapshot { detail: String },
    #[liaise(code = 42, msg = "tracked files changed since the snapshot HEAD names; checkout would overwrite them (checkpoint first, or --force): {detail}")]
    Dirty { detail: String },

    // the writers custodian routes to (exit 2)
    #[liaise(code = 60, msg = "the dependency needs where it comes from: {detail}")]
    DepVersion { detail: String },
    #[liaise(code = 61, msg = "the root manifest has no [workspace] table, so there is no workspace dependency table: {detail}")]
    NoWorkspaceTable { detail: String },
    #[liaise(code = 62, msg = "a feature still names the dependency; change the feature first: {detail}")]
    DepInUse { detail: String },
    #[liaise(code = 63, msg = "the item is not one klepto names in that file: {detail}")]
    AttrItem { detail: String },
    #[liaise(code = 64, msg = "the line is not one attribute on one line (#[...]): {detail}")]
    AttrLine { detail: String },

    // agent mode, through custodian_kit
    #[liaise(code = 50, msg = "custodian_kit refused: {detail}")]
    Custodian { detail: String },
    #[liaise(code = 51, msg = "no such job kind; agent init writes the ones code-passenger runs: {detail}")]
    NoJobKind { detail: String },
}

impl PassengerError {
    /// Refusals end a line with 2, everything else with 1.
    pub fn exit_code(&self) -> i32 {
        match self {
            PassengerError::PersonDecides { .. }
            | PassengerError::Unproven
            | PassengerError::PlanFlags { .. }
            | PassengerError::PlanNotApplied { .. }
            | PassengerError::PriorReceipt { .. }
            | PassengerError::StalePlan { .. }
            | PassengerError::Line { .. }
            | PassengerError::FeaturesDoc { .. }
            | PassengerError::NoFeaturesDoc { .. }
            | PassengerError::TwoFeaturesDocs { .. }
            | PassengerError::FeaturesDocExists { .. }
            | PassengerError::UnknownName { .. }
            | PassengerError::Dirty { .. }
            | PassengerError::DepVersion { .. }
            | PassengerError::NoWorkspaceTable { .. }
            | PassengerError::DepInUse { .. }
            | PassengerError::AttrItem { .. }
            | PassengerError::AttrLine { .. } => 2,
            _ => 1,
        }
    }

    /// `CP0021`.
    pub fn code_text(&self) -> String {
        Liaise::code(self)
    }
}

pub type Result<T> = std::result::Result<T, PassengerError>;

impl From<std::io::Error> for PassengerError {
    fn from(e: std::io::Error) -> Self {
        PassengerError::Io { detail: e.to_string() }
    }
}

impl From<serde_json::Error> for PassengerError {
    fn from(e: serde_json::Error) -> Self {
        PassengerError::Json { detail: e.to_string() }
    }
}

impl From<toml::de::Error> for PassengerError {
    fn from(e: toml::de::Error) -> Self {
        PassengerError::Toml { detail: e.to_string() }
    }
}

impl From<klepto::KleptoError> for PassengerError {
    fn from(e: klepto::KleptoError) -> Self {
        PassengerError::Scan { detail: e.to_string() }
    }
}

impl From<custodian_kit::CustodianError> for PassengerError {
    fn from(e: custodian_kit::CustodianError) -> Self {
        PassengerError::Custodian { detail: e.to_string() }
    }
}

impl PassengerError {
    pub fn unsupported(detail: impl std::fmt::Display) -> Self {
        PassengerError::Unsupported { detail: detail.to_string() }
    }

    pub fn path(detail: impl std::fmt::Display) -> Self {
        PassengerError::Path { detail: detail.to_string() }
    }

    pub fn toml(detail: impl std::fmt::Display) -> Self {
        PassengerError::Toml { detail: detail.to_string() }
    }
}

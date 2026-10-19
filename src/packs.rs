use crate::{
    error::{PassengerError, Result},
    model::{FileReport, ScaffoldOutput, ScaffoldRequest},
    plans::HeaderPlan, scaffolds::ScaffoldPlan,
};
use std::path::Path;

pub trait LanguagePack: Send + Sync {
    fn id(&self) -> &'static str;

    /// Which files this pack owns.
    fn matches_path(&self, path: &Path) -> bool;

    /// Render the header (doc + notes + gates) using a plan.
    fn render_header(&self, plan: &dyn HeaderPlan, report: &FileReport) -> String;

    /// Insert/replace header block idempotently.
    fn apply_header(&self, original: &str, header: &str) -> String;

    /// Optional: generate scaffolding for a new file/module.
    fn scaffold(&self, _plan: &dyn ScaffoldPlan, _req: ScaffoldRequest) -> Result<ScaffoldOutput> {
        Err(PassengerError::Unsupported("scaffold not supported by this pack".into()))
    }
}


#[cfg(feature = "lang_rust")]
pub mod rust;

pub fn get_pack(lang: &str) -> Result<Box<dyn LanguagePack>> {
    match lang {
        #[cfg(feature = "lang_rust")]
        "rust" | "rs" => Ok(Box::new(rust::RustPack)),
        _ => Err(PassengerError::Unsupported(format!("language pack '{lang}' not available"))),
    }
}
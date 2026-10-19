use crate::model::{FileReport, ScaffoldKind, ScaffoldOutput, ScaffoldRequest};
use crate::packs::rust::helpers::to_type_name;
use crate::plans::HeaderPlan;
use crate::scaffolds::ScaffoldPlan;

#[derive(Default)]
pub struct RustPack;

impl RustPack {
    pub const BEGIN_MARK: &'static str = "//! code_passenger:begin";
    pub const END_MARK: &'static str = "//! code_passenger:end";

    fn replace_or_insert(original: &str, header: &str) -> String {
        // Replace existing marked region if present
        if let Some(beg) = original.find(Self::BEGIN_MARK)
            && let Some(end) = original.find(Self::END_MARK) {
                let end_line = original[end..].find('\n').map(|i| end + i + 1).unwrap_or(original.len());
                let mut out = String::new();
                out.push_str(&original[..beg]);
                out.push_str(header);
                out.push_str(&original[end_line..]);
                return out;
            }

        // Insert after leading inner attrs `#![...]` (keep them first)
        let mut idx = 0usize;
        for line in original.lines() {
            let len = line.len();
            let line_with_nl = len + 1; // assume '\n' (works fine for typical files)
            let t = line.trim_start();
            if t.starts_with("#![") {
                idx += line_with_nl;
                continue;
            }
            break;
        }

        let mut out = String::new();
        out.push_str(&original[..idx]);
        out.push_str(header);
        out.push_str(&original[idx..]);
        out
    }
}

impl super::LanguagePack for RustPack {
    fn id(&self) -> &'static str { "rust" }

    fn matches_path(&self, path: &std::path::Path) -> bool {
        path.extension().and_then(|s| s.to_str()) == Some("rs")
    }

    fn render_header(&self, plan: &dyn HeaderPlan, report: &FileReport) -> String {
        plan.render_for_rust(report, Self::BEGIN_MARK, Self::END_MARK)
    }

    fn apply_header(&self, original: &str, header: &str) -> String {
        Self::replace_or_insert(original, header)
    }


    // inside impl super::LanguagePack for RustPack:
    fn scaffold(&self, plan: &dyn ScaffoldPlan, req: ScaffoldRequest) -> crate::error::Result<ScaffoldOutput> {
        let snake = req.name;

        let mut path = if matches!(req.kind, ScaffoldKind::Test) {
            std::path::PathBuf::from("tests")
        } else {
            std::path::PathBuf::from("src")
        };

        for seg in req.module_path {
            path.push(seg);
        }
        path.push(format!("{snake}.rs"));

        let ty = to_type_name(&snake);

        let tpl = plan.template(req.kind);
        let content = tpl
            .replace("{snake}", &snake)
            .replace("{Ty}", &ty);

        Ok(ScaffoldOutput { files: vec![(path, content)] })
    }

}


pub mod helpers;
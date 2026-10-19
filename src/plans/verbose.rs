use crate::model::{FileReport, PlanSection};
use super::{HeaderPlan, HeaderPlanV2};

pub struct VerbosePlan;

impl HeaderPlan for VerbosePlan {
    fn id(&self) -> &'static str { "verbose" }

    fn render_for_rust(&self, report: &FileReport, begin: &str, end: &str) -> String {
        let mut out = String::new();
        out.push_str(begin);
        out.push('\n');

        out.push_str("//! ----------------------------------------------\n");
        out.push_str("//! DOCUMENT DETAILS -----------------------------\n");
        out.push_str("//!\n");
        out.push_str(&format!("//! filename:{}\n", report.document.filename));
        out.push_str("//! description:\n");
        if report.used.packages.is_empty() {
            out.push_str("//! usages:none\n");
        } else {
            out.push_str(&format!("//! usages:{:?}\n", report.used.packages.iter().collect::<Vec<_>>()));
        }
        out.push_str("//!\n");

        out.push_str("//! ----------------------------------------------\n");
        out.push_str("//! FEATURE NOTES --------------------------------\n");

        for n in &report.notes {
            out.push_str("//!\n");
            out.push_str(&format!("//! feature_name:{}\n", n.feature_name));
            out.push_str(&format!("//! deps:{:?}\n", n.deps));
            out.push_str(&format!("//! scope:{:?}\n", n.scope));
            out.push_str(&format!("//! corpus:{}\n", n.corpus));
            out.push_str(&format!("//! unavailable:{:?}\n", n.unavailable));
            if !n.lost_by.is_empty() {
                out.push_str(&format!("//! lost_by:{:?}\n", n.lost_by));
            }
        }

        out.push_str("//!\n");
        out.push_str("//! ----------------------------------------------\n");
        out.push_str("//! CORPUS FEATURES ------------------------------\n");
        out.push_str("//!\n");

        for f in &report.corpus_features {
            out.push_str(&format!("#![cfg(feature = \"{}\")]\n", f));
        }

        out.push_str(end);
        out.push('\n');
        out
    }
}

impl HeaderPlanV2 for VerbosePlan {
    fn id(&self) -> &'static str { "verbose" }

    fn sections(&self) -> &[PlanSection] {
        const SECTIONS: &[PlanSection] = &[
            PlanSection::DocumentDetails,
            PlanSection::FeatureNotes,
            PlanSection::CorpusGates,
        ];
        SECTIONS
    }
}

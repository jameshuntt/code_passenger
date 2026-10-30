//! `receipt` (rule R1 in `GATE_RULES.md`).

use crate::commands::klepto_for;
use crate::error::Result;
use klepto::receipt::kg;
use klepto::Receipt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Written {
    pub receipt: PathBuf,
    pub knowledge: Option<PathBuf>,
    pub challenges: Option<PathBuf>,
}

/// Each workspace item's record gains `target`: its id exactly as a change spec's `target` names it (the node's id
/// without its `item/` prefix), so a caller takes it as it is and never rebuilds it from the name, the module or the
/// type (a generic type's id carries its parameters: `Registry<WORKS,SUBSCRIBERS>::first_free`).
pub fn with_targets(knowledge: &mut [serde_json::Value]) {
    for r in knowledge.iter_mut() {
        let id = (r["kind"] == "item" && r["item_kind"] != "external").then(|| r["id"].as_str().and_then(|i| i.strip_prefix("item/")).map(str::to_string)).flatten();
        if let (Some(id), Some(o)) = (id, r.as_object_mut()) {
            o.insert("target".into(), serde_json::Value::String(id));
        }
    }
}

/// Rule R1.
pub fn write(root: &Path, out: &Path, kg_dir: Option<&Path>) -> Result<Written> {
    let k = klepto_for(root)?;
    let receipt = Receipt::from_klepto(&k);
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out, receipt.to_json_string())?;
    let mut written = Written { receipt: out.to_path_buf(), knowledge: None, challenges: None };
    if let Some(dir) = kg_dir {
        std::fs::create_dir_all(dir)?;
        let mut bundle = kg::export(&k);
        with_targets(&mut bundle.knowledge);
        let knowledge = dir.join("knowledge.ndjson");
        let challenges = dir.join("challenges.ndjson");
        std::fs::write(&knowledge, kg::to_ndjson(&bundle.knowledge))?;
        std::fs::write(&challenges, kg::to_ndjson(&bundle.challenges))?;
        written.knowledge = Some(knowledge);
        written.challenges = Some(challenges);
    }
    Ok(written)
}

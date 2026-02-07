use crate::analysis::run as r;
use crate::engine::{RunContext, run_scan};
use crate::error::{Result, PassengerError};
use crate::{packs, plans};
use clap::{Parser, Subcommand};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "code-passenger")]
#[command(about = "Scan a Rust crate for feature/dependency usage and generate feature gating headers.", long_about = None)]
pub struct Cli {
    /// Path to crate root (where Cargo.toml lives)
    #[arg(long, default_value = ".")]
    pub root: PathBuf,

    /// Relative source directory to scan
    #[arg(long, default_value = "src")]
    pub src: String,

    /// Manifest path (defaults to {root}/Cargo.toml)
    #[arg(long)]
    pub manifest: Option<PathBuf>,

    #[arg(long, default_value="rust")]
    pub lang: String,

    #[arg(long, default_value="verbose")]
    pub plan: String,

    /// Emit JSON instead of human output
    #[arg(long)]
    pub json: bool,

    #[command(subcommand)]
    pub cmd: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Scan and print reports
    Scan,
    /// Annotate files (insert/replace header blocks)
    Annotate {
        /// Actually write changes (otherwise dry-run)
        #[arg(long)]
        write: bool,

        /// Exit non-zero if changes would be made (CI mode)
        #[arg(long)]
        check: bool,
    },
    Scaffold {
        #[arg(long)]
        kind: String, // "module" etc

        #[arg(long)]
        name: String,

        /// like "net/http"
        #[arg(long, default_value="")]
        path: String,

        #[arg(long, default_value="default")]
        scaffold_plan: String,

        #[arg(long)]
        write: bool,
    }
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let manifest_path = cli.manifest.clone().unwrap_or_else(|| cli.root.join("Cargo.toml"));

    let ctx = RunContext {
        root: cli.root.clone(),
        src_rel: cli.src.clone(),
        manifest_path,
        lang: cli.lang.clone(),
    };

    match cli.cmd {
        Command::Scan => {
            let out = run_scan(&ctx)?;
            if cli.json {
                // println!("{}", serde_json::to_string_pretty(&out.reports).unwrap());
                
                let a = r(&out);
                let file = std::fs::File::create("report.json")?;
                let writer = std::io::BufWriter::new(file);

                let _ = serde_json::to_writer_pretty(writer, &a);
                println!("Saved report to report.json");
            } else {
                let top_n = 5;

                for r in &out.reports {
                    println!("== {} ==", r.document.relative_path);

                    print_set("used_deps", &r.used.packages);   // or r.used.packages
                    print_set("used_mods", &r.used.modules);    // or r.used.modules
                    print_set("corpus_features", &r.corpus_features);

                    // Optional: quick totals
                    println!(
                        "use_sites: external={}, internal={}",
                        r.external_use_sites.len(),
                        r.internal_use_sites.len()
                    );

                    print_counts("external_symbols", &r.external_dep_symbol_counts, top_n);
                    print_counts("internal_symbols", &r.internal_dep_symbol_counts, top_n);

                    println!();
                }
            }
        },

        Command::Annotate { write, check } => {
            let out = run_scan(&ctx)?;
            let pack = packs::get_pack(&cli.lang)?;
            let plan = plans::get_plan(&cli.plan)?;

            let mut changed_any = false;

            for r in &out.reports {
                let header = pack.render_header(plan.as_ref(), r);

                let file_path = cli.root.join(&cli.src).join(&r.document.relative_path);
                let original = fs::read_to_string(&file_path)?;
                let updated = pack.apply_header(&original, &header);

                if updated != original {
                    changed_any = true;
                    if write {
                        fs::write(&file_path, updated)?;
                    }
                }
            }

            if check && changed_any {
                return Err(PassengerError::ChangesNeeded);
            }
        },

        Command::Scaffold { kind, name, path, scaffold_plan, write } => {
            let pack = packs::get_pack(&cli.lang)?;
            let plan = crate::scaffolds::get_scaffold_plan(&scaffold_plan)?;

            let kind = crate::model::ScaffoldKind::parse(&kind)
                .ok_or_else(|| PassengerError::Unsupported(format!("unknown scaffold kind '{kind}'")))?;

            let module_path: Vec<String> = path
                .split('/')
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();

            let out = pack.scaffold(
                plan.as_ref(),
                crate::model::ScaffoldRequest { kind, name, module_path }
            )?;

            if out.files.is_empty() {
                return Err(PassengerError::Unsupported("scaffold produced no files".into()));
            }

            for (rel, content) in out.files {
                let abs = cli.root.join(rel);
                if write {
                    if let Some(parent) = abs.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::write(&abs, content)?;
                } else {
                    println!("--- {}", abs.display());
                    println!("{content}");
                }
            }
        }

    }

    Ok(())
}


use std::collections::BTreeMap;

fn preview_syms(syms: &BTreeMap<String, usize>, top_n: usize) -> String {
    let mut v: Vec<(&String, &usize)> = syms.iter().collect();
    v.sort_by(|(ka, ca), (kb, cb)| {
        // count desc, then name asc
        cb.cmp(ca).then_with(|| ka.cmp(kb))
    });

    v.into_iter()
        .take(top_n)
        .map(|(k, c)| format!("{k}({c})"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn print_counts(
    title: &str,
    counts: &BTreeMap<String, BTreeMap<String, usize>>,
    top_n: usize,
) {
    if counts.is_empty() {
        return;
    }

    println!("{title}:");
    for (dep, syms) in counts {
        if syms.is_empty() { continue; }
        let preview = preview_syms(syms, top_n);
        if preview.is_empty() { continue; }
        println!("  {dep}: {preview}");
    }
}

fn print_set<T: std::fmt::Debug>(label: &str, set: &T) {
    println!("{label}: {set:?}");
}

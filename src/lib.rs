//! code_passenger: scans a workspace with klepto, then does the writing
//! klepto never does: feature gates across every crate of a workspace from
//! one features document, Cargo.toml edits, file headers, reports, and the
//! history of it all.

pub mod cargo;
pub mod model;
pub mod error;
pub mod plans;
pub mod passenger;
pub mod proof_dir;
pub mod packs;
pub mod engine;
pub mod scaffolds;
pub mod analysis;
pub mod commands;
pub mod gate;
pub mod oracle;
pub mod words;
pub mod doc;
pub mod depth;
pub mod workspace;
pub mod init;
pub mod agent;
pub mod lines;
pub mod transform;
pub mod to_toolkit;

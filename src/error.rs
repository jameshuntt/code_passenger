use thiserror::Error;

#[derive(Debug, Error)]
pub enum PassengerError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid Cargo.toml: {0}")]
    Toml(String),

    #[error("missing [package].name in Cargo.toml")]
    MissingPackageName,

    #[error("path error: {0}")]
    Path(String),

    #[error("unsupported: {0}")]
    Unsupported(String),

    #[error("changes needed (run annotate --write)")]
    ChangesNeeded,
}

pub type Result<T> = std::result::Result<T, PassengerError>;
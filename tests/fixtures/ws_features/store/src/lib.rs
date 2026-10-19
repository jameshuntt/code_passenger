// code_passenger workspace-features fixture: `log` spans store and app.
// This workspace COMPILES (the cargo check runs on a scratch copy).

pub mod plain;
pub mod sink;

/// Writes through logsink: one function behind `log`.
pub fn record(x: u32) -> u32 {
    logsink::write(x)
}

/// Nothing to do with logsink.
pub fn keep(x: u32) -> u32 {
    plain::add(x, 1)
}

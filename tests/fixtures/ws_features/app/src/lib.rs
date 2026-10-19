// app uses store's function that goes behind `log` (the cross-crate cascade)
// and logsink itself.

/// Calls store::record: gated in app too, and app's `log` turns on store's.
pub fn audit(x: u32) -> u32 {
    store::record(x)
}

/// Uses logsink directly.
pub fn trace() -> u32 {
    logsink::write(1)
}

/// Untouched.
pub fn run() -> u32 {
    store::keep(1)
}

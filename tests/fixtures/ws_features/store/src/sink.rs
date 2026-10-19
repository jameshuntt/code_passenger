/// Every item of this file uses logsink: the whole file goes behind `log`.
pub fn flush() -> u32 {
    logsink::write(0)
}

/// Calls flush: the cascade.
pub fn flush_twice() -> u32 {
    flush() + flush()
}

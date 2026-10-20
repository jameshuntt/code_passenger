//! Depends on rules directly.

pub fn open(n: u16) -> Result<u16, &'static str> {
    rules::port(n)
}

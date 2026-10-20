//! A shop: its stock, its depot, and the sums the counter makes.

pub mod depot;
pub mod stock;
pub use stock::{has, Item};

/// Twice a number.
fn double(n: u32) -> u32 {
    let twice = n * 2;
    twice
}

pub fn twice_total(o: u32) -> u32 {
    double(o) + double(1)
}

pub fn version() -> u32 {
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_doubled_is_four() {
        assert_eq!(double(2), 4);
        assert_eq!(twice_total(3), 8);
    }
}

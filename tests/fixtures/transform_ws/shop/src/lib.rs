//! A shop: orders and their states.

pub struct Order {
    pub id: u32,
    pub total: u64,
}

impl Order {
    pub fn new(id: u32, total: u64) -> Self {
        Order { id, total }
    }
}

pub enum State {
    Open,
    Closed,
}

pub fn label(s: &State) -> &'static str {
    match s {
        State::Open => "open",
        State::Closed => "closed",
    }
}

pub fn total_of(o: &Order) -> u64 {
    o.total
}

pub fn version() -> u32 {
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_order_keeps_its_total() {
        assert_eq!(Order::new(1, 5).total, 5);
    }
}

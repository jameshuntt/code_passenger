//! Uses the shop's orders.

pub fn first() -> shop::Order {
    shop::Order { id: 1, total: 10 }
}

pub fn sum(o: &shop::Order) -> u64 {
    shop::total_of(o) + 1
}

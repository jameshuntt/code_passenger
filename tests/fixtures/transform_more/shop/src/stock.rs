//! What the shop holds.

use std::collections::BTreeMap;

pub struct Item {
    pub name: String,
    pub counts: BTreeMap<String, u32>,
}

impl Item {
    pub fn count(&self, key: &str) -> u32 {
        self.counts.get(key).copied().unwrap_or(0)
    }
}

pub fn has(i: &Item, key: &str) -> bool {
    i.count(key) > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_item_counts_nothing() {
        let i = Item { name: "a".into(), counts: BTreeMap::new() };
        assert_eq!(i.count("a"), 0);
    }
}

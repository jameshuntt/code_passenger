//! Where items wait.

use crate::stock::Item;

pub fn first(items: &[Item]) -> Option<&Item> {
    items.first()
}

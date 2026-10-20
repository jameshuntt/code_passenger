//! A struct under an attribute macro.

use shop_marks::keep;

#[keep]
#[derive(Debug)]
pub struct Packed {
    pub bits: u8,
}

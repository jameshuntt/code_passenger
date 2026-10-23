//! A caller of the kit.

use old_kit::greet;

pub fn hello() -> String {
    format!("{} {}", greet(), old_kit::greet())
}

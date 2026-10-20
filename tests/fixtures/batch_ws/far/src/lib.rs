//! Depends on rules only through user: a body changed in rules is never compiled here.

pub fn open_twice(n: u16) -> bool {
    user::open(n).is_ok() && user::open(n).is_ok()
}

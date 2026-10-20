//! Four bodies of one shape (an early return, then the value), which the toolkit's `return_if` writes exactly; a
//! test that fails before any change (as a test that needs root fails on a laptop); a test each change must keep.

/// A port that is not zero.
pub fn port(n: u16) -> Result<u16, &'static str> {
    if n == 0 {
        return Err("no port");
    }
    Ok(n)
}

/// A QoS level: 0, 1 or 2.
pub fn level(n: u8) -> Result<u8, &'static str> {
    if n > 2 {
        return Err("over two");
    }
    Ok(n)
}

/// A size of one digit.
pub fn size(n: usize) -> Result<usize, &'static str> {
    if n > 9 {
        return Err("too big");
    }
    Ok(n)
}

/// A name that is not empty.
pub fn name(s: &str) -> Result<&str, &'static str> {
    if s.is_empty() {
        return Err("no name");
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zero_port_is_refused() {
        assert_eq!(port(0), Err("no port"));
        assert_eq!(port(22), Ok(22));
    }

    #[test]
    fn each_rule_keeps_its_word() {
        assert_eq!(level(3), Err("over two"));
        assert_eq!(size(10), Err("too big"));
        assert_eq!(name(""), Err("no name"));
    }

    #[test]
    fn needs_what_a_laptop_has_not() {
        assert!(std::path::Path::new("/no/such/place/on/any/laptop").exists());
    }
}

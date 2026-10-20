//! Hand-written bodies the simple_impl toolkit writes exactly (three statements, one whole method), and one it
//! writes nearly.

/// A port and what it is for.
pub struct Port {
    number: u16,
}

impl Port {
    pub fn new(number: u16) -> Self {
        Self { number }
    }

    /// The port's number.
    #[must_use]
    pub fn number(&self) -> u16 {
        self.number
    }
}

/// A QoS level: 0, 1 or 2.
pub fn qos(level: u8) -> Result<u8, &'static str> {
    if !(level <= 2) {
        return Err("qos is 0, 1 or 2");
    }
    let checked = level;
    Ok(checked)
}

/// The first port given.
pub fn first_port(ports: &[u16]) -> Result<u16, String> {
    let port = match ports.first() {
        Some(port) => port,
        None => return Err("no port".to_string()),
    };
    Ok(*port)
}

/// Every name that is not empty.
pub fn names(all: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    for name in all {
        if name.is_empty() {
            continue;
        }
        out.push(name.to_string());
    }
    out
}

/// Two sizes that must agree: its `return` has no `;`, so the toolkit's `ensure` writes it nearly, not exactly.
pub fn same_size(a: usize, b: usize) -> Result<usize, &'static str> {
    if !(a == b) {
        return Err("sizes differ")
    }
    Ok(a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_level_over_two_is_refused() {
        assert_eq!(qos(1), Ok(1));
        assert_eq!(qos(3), Err("qos is 0, 1 or 2"));
    }

    #[test]
    fn the_first_port_or_why_not() {
        assert_eq!(first_port(&[22, 443]), Ok(22));
        assert_eq!(first_port(&[]), Err("no port".to_string()));
    }

    #[test]
    fn empty_names_are_left_out() {
        assert_eq!(names(&["a", "", "b"]), ["a", "b"]);
    }

    #[test]
    fn a_port_keeps_its_number() {
        assert_eq!(Port::new(22).number(), 22);
    }

    #[test]
    fn sizes_agree_or_not() {
        assert_eq!(same_size(2, 2), Ok(2));
        assert_eq!(same_size(2, 3), Err("sizes differ"));
    }
}

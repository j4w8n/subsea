pub fn is_register(name: &str) -> bool {
    matches!(name, "sp" | "wsp")
        || name
            .as_bytes()
            .split_first()
            .is_some_and(|(&prefix, index)| {
                matches!(
                    prefix,
                    b'x' | b'w' | b'v' | b'q' | b'd' | b's' | b'h' | b'b'
                ) && parse_index(index).is_some_and(|index| match prefix {
                    b'x' | b'w' => index <= 30,
                    _ => index <= 31,
                })
            })
}

pub(crate) fn is_vector(name: &str) -> bool {
    name.as_bytes()
        .split_first()
        .is_some_and(|(&prefix, index)| {
            matches!(prefix, b'v' | b'q' | b'd' | b's' | b'h' | b'b')
                && parse_index(index).is_some_and(|index| index <= 31)
        })
}

fn parse_index(index: &[u8]) -> Option<u8> {
    std::str::from_utf8(index).ok()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::{is_register, is_vector};

    #[test]
    fn malformed_and_non_ascii_names_are_not_registers() {
        for name in ["", "x", "x31", "v32", "é", "xé", "é0"] {
            assert!(!is_register(name), "{name:?}");
            assert!(!is_vector(name), "{name:?}");
        }
    }
}

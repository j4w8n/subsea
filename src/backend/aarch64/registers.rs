pub fn is_register(name: &str) -> bool {
    matches!(name, "sp" | "wsp")
        || (name.len() >= 2
            && matches!(&name[..1], "x" | "w" | "v" | "q" | "d" | "s" | "h" | "b")
            && name[1..].parse::<u8>().is_ok_and(|index| match &name[..1] {
                "x" | "w" => index <= 30,
                _ => index <= 31,
            }))
}

pub(crate) fn is_vector(name: &str) -> bool {
    name.len() >= 2
        && matches!(&name[..1], "v" | "q" | "d" | "s" | "h" | "b")
        && name[1..].parse::<u8>().is_ok_and(|index| index <= 31)
}

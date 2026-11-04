/// A client or profile name: lowercase letters, digits, and hyphens,
/// 1 to 64 characters, not starting with a hyphen.
pub fn valid_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0] != b'-'
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_the_prototype_rule() {
        assert!(valid_name("commit-scope"));
        assert!(valid_name("a1"));
        assert!(valid_name("cc-my-plugin"));
        assert!(valid_name(&"a".repeat(64)));
        assert!(!valid_name(""));
        assert!(!valid_name("Commit"));
        assert!(!valid_name("-x"));
        assert!(!valid_name("a b"));
        assert!(!valid_name(&"a".repeat(65)));
    }
}

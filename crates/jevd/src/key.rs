//! The key shape check (spec section 4, step 3).

use jev_proto::ErrorCode;

const MAX_KEY_LENGTH: usize = 512;

/// The trimmed key when it is usable, else `no-key` or `bad-key`. Spaces at
/// either end do not count. Use the returned value, not `raw`, wherever the key
/// is matched or sent, so the checked key and the used key are the same text.
///
/// Printable ASCII only: a control character in a header makes an HTTP
/// library put the key in its error text.
pub fn check_key(raw: &str) -> Result<&str, ErrorCode> {
    let key = raw.trim();
    if key.is_empty() {
        return Err(ErrorCode::NoKey);
    }
    if key.len() > MAX_KEY_LENGTH || !key.bytes().all(|b| (0x21..=0x7e).contains(&b)) {
        return Err(ErrorCode::BadKey);
    }
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_shapes() {
        assert_eq!(check_key("tsk-abc123"), Ok("tsk-abc123"));
        assert_eq!(check_key("  tsk-abc123\n"), Ok("tsk-abc123"));
        assert_eq!(check_key(""), Err(ErrorCode::NoKey));
        assert_eq!(check_key("   "), Err(ErrorCode::NoKey));
        assert_eq!(check_key("a b"), Err(ErrorCode::BadKey));
        assert_eq!(check_key("abc\u{7}"), Err(ErrorCode::BadKey));
        assert_eq!(check_key("clé"), Err(ErrorCode::BadKey));
        assert_eq!(check_key(&"k".repeat(512)), Ok("k".repeat(512).as_str()));
        assert_eq!(check_key(&"k".repeat(513)), Err(ErrorCode::BadKey));
    }

    #[test]
    fn key_character_range_edges() {
        assert_eq!(check_key("a!b"), Ok("a!b")); // 0x21, the first allowed
        assert_eq!(check_key("a~b"), Ok("a~b")); // 0x7e, the last allowed
        assert_eq!(check_key("a\u{7f}b"), Err(ErrorCode::BadKey)); // DEL
    }
}

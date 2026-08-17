/// Estimate token count from text. ~4 chars = 1 token.
/// Matches bash: `(len + 3) / 4`
pub fn estimate_tokens(s: &str) -> usize {
    s.len().div_ceil(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_string() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn one_char() {
        assert_eq!(estimate_tokens("a"), 1);
    }

    #[test]
    fn four_chars() {
        assert_eq!(estimate_tokens("abcd"), 1);
    }

    #[test]
    fn five_chars() {
        assert_eq!(estimate_tokens("abcde"), 2);
    }

    #[test]
    fn eight_chars() {
        assert_eq!(estimate_tokens("abcdefgh"), 2);
    }

    #[test]
    fn nine_chars() {
        assert_eq!(estimate_tokens("abcdefghi"), 3);
    }

    #[test]
    fn hundred_chars() {
        let s = "a".repeat(100);
        assert_eq!(estimate_tokens(&s), 25);
    }
}

/// Truncate string to at most `max_bytes`, ensuring UTF-8 char boundary.
/// Returns the truncated view (not a new allocation).
pub fn truncate(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        s
    } else {
        &s[..s.floor_char_boundary(max_bytes)]
    }
}

/// Truncate from the left, keeping the rightmost portion.
/// Prepends `"..."` if truncated. Respects UTF-8 char boundaries.
pub fn truncate_tail(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        s.to_string()
    } else {
        let ideal = s.len() - max_bytes + 3;
        let start = s
            .char_indices()
            .find(|&(i, _)| i >= ideal)
            .map(|(i, _)| i)
            .unwrap_or(s.len());
        let mut out = String::with_capacity(3 + s.len() - start);
        out.push_str("...");
        out.push_str(&s[start..]);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_ascii() {
        assert_eq!(truncate("hello world", 5), "hello");
        assert_eq!(truncate("hello world", 100), "hello world");
    }

    #[test]
    fn truncate_multi_byte() {
        let s = "智谱AI GLM";
        // 智=3, 谱=3, A=1, I=1, space=1, G=1, L=1, M=1 = 12 bytes
        assert_eq!(truncate(s, 6), "智谱");
        assert_eq!(truncate(s, 7), "智谱A");
    }

    #[test]
    fn truncate_tail_ascii() {
        assert_eq!(truncate_tail("hello world", 11), "hello world");
        assert_eq!(truncate_tail("hello world", 8), "...world");
    }

    #[test]
    fn truncate_tail_multi_byte() {
        let s = "智谱AI GLM";
        assert_eq!(truncate_tail(s, 6), "...GLM");
    }
}

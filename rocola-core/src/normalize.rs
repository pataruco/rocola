/// Canonical ISRC: 12 uppercase alphanumerics. Spotify data is inconsistently
/// cased and occasionally hyphenated, so normalise before any comparison.
pub fn normalize_isrc(raw: &str) -> Option<String> {
    let cleaned: String = raw
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_uppercase())
        .collect();
    (cleaned.len() == 12).then_some(cleaned)
}

/// Lowercase, drop bracketed/parenthesised segments, drop everything after
/// " - " (Spotify's suffix convention: "- Remastered", "- Live", …),
/// collapse whitespace.
#[must_use]
pub fn normalize_title(raw: &str) -> String {
    let no_dash_suffix = raw.split(" - ").next().unwrap_or(raw);
    let mut out = String::with_capacity(no_dash_suffix.len());
    let mut depth = 0u32;
    for c in no_dash_suffix.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.extend(c.to_lowercase()),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[must_use]
pub fn normalize_artist(raw: &str) -> String {
    raw.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isrc_uppercases_and_strips_hyphens() {
        assert_eq!(
            normalize_isrc("gb-arl-98-00212"),
            Some("GBARL9800212".into())
        );
        assert_eq!(normalize_isrc("USUM71703861"), Some("USUM71703861".into()));
    }

    #[test]
    fn isrc_rejects_wrong_shape() {
        assert_eq!(normalize_isrc(""), None);
        assert_eq!(normalize_isrc("TOO-SHORT"), None);
        assert_eq!(normalize_isrc("GBARL98002123456"), None);
    }

    #[test]
    fn title_strips_noise_and_case() {
        assert_eq!(
            normalize_title("Bohemian Rhapsody - Remastered 2011"),
            "bohemian rhapsody"
        );
        assert_eq!(normalize_title("Umbrella (feat. JAY-Z)"), "umbrella");
        assert_eq!(
            normalize_title("Como Te Extraño  [En Vivo]"),
            "como te extraño"
        );
    }

    #[test]
    fn artist_lowercases_and_trims() {
        assert_eq!(normalize_artist("  Café Tacvba "), "café tacvba");
    }
}

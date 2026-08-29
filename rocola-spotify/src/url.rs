use crate::SpotifyError;

/// A bare Spotify playlist ID (base62).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistRef(pub String);

/// Accepts `https://open.spotify.com/playlist/<id>[?…]`, its localized form
/// `https://open.spotify.com/intl-<locale>/playlist/<id>[?…]` (e.g.
/// `intl-es`, `intl-pt-br`), and `spotify:playlist:<id>`.
///
/// # Errors
///
/// Returns [`SpotifyError::BadUrl`] when `input` isn't a recognisable Spotify
/// playlist link or URI.
pub fn parse_playlist_url(input: &str) -> Result<PlaylistRef, SpotifyError> {
    let input = input.trim();
    let id = input
        .strip_prefix("spotify:playlist:")
        .map(ToOwned::to_owned)
        .or_else(|| {
            let after_host = input.split("open.spotify.com/").nth(1)?;
            let after_locale = after_host
                .strip_prefix("intl-")
                .and_then(|rest| rest.split_once('/'))
                .filter(|(locale, _)| !locale.is_empty())
                .map_or(after_host, |(_, tail)| tail);
            let after = after_locale.strip_prefix("playlist/")?;
            Some(after.split(['?', '/']).next().unwrap_or(after).to_owned())
        });
    match id {
        Some(id) if !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric()) => {
            Ok(PlaylistRef(id))
        }
        _ => Err(SpotifyError::BadUrl(
            "That doesn't look like a Spotify playlist link. Paste a link like \
             https://open.spotify.com/playlist/… (from Share → Copy link to playlist)."
                .into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_web_url_with_query() {
        let r =
            parse_playlist_url("https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M?si=abc")
                .unwrap();
        assert_eq!(r.0, "37i9dQZF1DXcBWIGoYBM5M");
    }

    #[test]
    fn parses_uri_form() {
        let r = parse_playlist_url("spotify:playlist:37i9dQZF1DXcBWIGoYBM5M").unwrap();
        assert_eq!(r.0, "37i9dQZF1DXcBWIGoYBM5M");
    }

    #[test]
    fn rejects_album_url_with_named_error() {
        let err = parse_playlist_url("https://open.spotify.com/album/xyz").unwrap_err();
        assert!(
            err.to_string().contains("playlist"),
            "error must tell the user it needs a playlist link"
        );
    }

    #[test]
    fn parses_localized_url_with_query() {
        let r = parse_playlist_url(
            "https://open.spotify.com/intl-es/playlist/37i9dQZF1DXcBWIGoYBM5M?si=abc",
        )
        .unwrap();
        assert_eq!(r.0, "37i9dQZF1DXcBWIGoYBM5M");
    }

    #[test]
    fn parses_localized_url_with_two_part_locale() {
        let r = parse_playlist_url(
            "https://open.spotify.com/intl-pt-br/playlist/37i9dQZF1DXcBWIGoYBM5M",
        )
        .unwrap();
        assert_eq!(r.0, "37i9dQZF1DXcBWIGoYBM5M");
    }
}

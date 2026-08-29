use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngExt as _;
use sha2::{Digest as _, Sha256};

pub const REDIRECT_URI: &str = "http://127.0.0.1:8888/callback";
const UNRESERVED: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";

#[derive(Debug, Clone)]
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    #[must_use]
    pub fn new() -> Self {
        let mut rng = rand::rng();
        let verifier: String = (0..64)
            .map(|_| UNRESERVED[rng.random_range(0..UNRESERVED.len())] as char)
            .collect();
        let challenge = challenge_for(&verifier);
        Self {
            verifier,
            challenge,
        }
    }
}

impl Default for Pkce {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn challenge_for(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

#[must_use]
pub fn authorize_url(client_id: &str, pkce: &Pkce, state: &str) -> String {
    let redirect = REDIRECT_URI.replace(':', "%3A").replace('/', "%2F");
    format!(
        "https://accounts.spotify.com/authorize?client_id={client_id}\
         &response_type=code&redirect_uri={redirect}\
         &code_challenge_method=S256&code_challenge={}\
         &state={state}&scope=playlist-read-private%20playlist-read-collaborative",
        pkce.challenge
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc7636_appendix_b_vector() {
        // Verifier and expected challenge from RFC 7636 Appendix B.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            challenge_for(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn verifier_is_64_unreserved_chars_and_random() {
        let a = Pkce::new();
        let b = Pkce::new();
        assert_eq!(a.verifier.len(), 64);
        assert!(
            a.verifier
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-._~".contains(c))
        );
        assert_ne!(a.verifier, b.verifier);
    }

    #[test]
    fn authorize_url_pins_loopback_redirect() {
        let url = authorize_url("myclientid", &Pkce::new(), "st4te");
        assert!(url.starts_with("https://accounts.spotify.com/authorize?"));
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A8888%2Fcallback"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state=st4te"));
    }
}

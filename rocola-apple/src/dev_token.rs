use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde::Serialize;

use crate::AppleError;

#[derive(Serialize)]
struct Claims {
    iss: String,
    iat: u64,
    exp: u64,
}

/// Apple developer token: ES256 JWT signed with the user's `MusicKit` .p8 key.
/// Short-lived and in-memory only — 12 hours covers any session.
///
/// # Errors
///
/// Returns [`AppleError::BadKey`] when `p8_pem` isn't a valid EC private key
/// or when signing fails.
///
/// # Panics
///
/// Panics if the system clock reads earlier than the Unix epoch.
pub fn mint_developer_token(
    p8_pem: &str,
    team_id: &str,
    key_id: &str,
) -> Result<String, AppleError> {
    let key = EncodingKey::from_ec_pem(p8_pem.as_bytes()).map_err(|_| {
        AppleError::BadKey(
            "couldn't read your Apple key. Check that apple.p8_path in \
             ~/.config/rocola/config.toml points at the AuthKey_….p8 file you \
             downloaded from Apple."
                .into(),
        )
    })?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before 1970")
        .as_secs();
    let mut header = Header::new(Algorithm::ES256);
    header.kid = Some(key_id.to_owned());
    let claims = Claims {
        iss: team_id.to_owned(),
        iat: now,
        exp: now + 12 * 60 * 60,
    };
    encode(&header, &claims, &key)
        .map_err(|e| AppleError::BadKey(format!("couldn't sign the Apple token: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    #[test]
    fn mints_a_jwt_with_kid_iss_and_12h_expiry() {
        let pem = include_str!("../tests/fixtures/test_key.p8");
        let token = mint_developer_token(pem, "TEAM123456", "KEY1234567").unwrap();
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);
        let header: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).unwrap()).unwrap();
        assert_eq!(header["alg"], "ES256");
        assert_eq!(header["kid"], "KEY1234567");
        let claims: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
        assert_eq!(claims["iss"], "TEAM123456");
        let lifetime = claims["exp"].as_u64().unwrap() - claims["iat"].as_u64().unwrap();
        assert_eq!(lifetime, 12 * 60 * 60);
    }

    #[test]
    fn bad_pem_reports_the_fix() {
        let err = mint_developer_token("not a key", "T", "K").unwrap_err();
        assert!(
            err.to_string().contains(".p8"),
            "error must mention the .p8 file: {err}"
        );
    }
}

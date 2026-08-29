use serde::Deserialize;

use crate::SpotifyError;
use crate::pkce::{Pkce, REDIRECT_URI, authorize_url};

#[derive(Debug, Clone, Deserialize)]
pub struct TokenSet {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    pub expires_in: u64,
}

/// Parse the query string Spotify sends to the loopback redirect.
///
/// # Errors
///
/// Returns [`SpotifyError::Auth`] when Spotify reported an error, when the
/// `state` doesn't match the one rocola sent, or when no code came back.
pub fn parse_callback_query(query: &str, expected_state: &str) -> Result<String, SpotifyError> {
    let mut code = None;
    let mut state = None;
    let mut error = None;
    for pair in query.split('&') {
        match pair.split_once('=') {
            Some(("code", v)) => code = Some(v.to_owned()),
            Some(("state", v)) => state = Some(v.to_owned()),
            Some(("error", v)) => error = Some(v.to_owned()),
            _ => {}
        }
    }
    if let Some(e) = error {
        return Err(SpotifyError::Auth(format!(
            "the sign-in didn't finish ({e}). Run rocola again to retry."
        )));
    }
    if state.as_deref() != Some(expected_state) {
        return Err(SpotifyError::Auth(
            "the sign-in reply didn't come from the request rocola made. Run rocola again to retry."
                .into(),
        ));
    }
    code.ok_or_else(|| {
        SpotifyError::Auth("Spotify sent no code. Run rocola again to retry.".into())
    })
}

/// Pick the callback query out of a raw HTTP request, but only when this is
/// the request we're waiting for: a `GET` of `/callback` carrying a query.
///
/// Browsers open speculative connections and probe for `/favicon.ico`, so the
/// loopback listener sees traffic that isn't the sign-in reply.
fn callback_query(request: &str) -> Option<String> {
    let mut request_line = request.lines().next()?.split_whitespace();
    if request_line.next()? != "GET" {
        return None;
    }
    let (path, query) = request_line.next()?.split_once('?')?;
    if !path.starts_with("/callback") || query.is_empty() {
        return None;
    }
    Some(query.to_owned())
}

/// Answer connections on the loopback listener until the browser delivers the
/// sign-in callback, then reply to it and return its authorization code.
async fn wait_for_callback(
    listener: &tokio::net::TcpListener,
    state: &str,
) -> Result<String, SpotifyError> {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    loop {
        let (mut stream, _) = listener
            .accept()
            .await
            .map_err(|e| SpotifyError::Auth(format!("couldn't accept the browser's reply: {e}")))?;
        let mut buf = vec![0u8; 8192];
        let request = match stream.read(&mut buf).await {
            Ok(n) => String::from_utf8_lossy(&buf[..n]).into_owned(),
            Err(_) => continue,
        };
        let Some(query) = callback_query(&request) else {
            let _ = stream
                .write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await;
            continue;
        };
        let result = parse_callback_query(&query, state);
        let body = if result.is_ok() {
            "Signed in. You can close this tab and return to the terminal."
        } else {
            "Something went wrong. Return to the terminal for what to do next."
        };
        let _ = stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .await;
        return result;
    }
}

/// Run the Authorization Code + PKCE flow: open the browser, catch the single
/// loopback callback, and exchange the code for a token set.
///
/// # Errors
///
/// Returns [`SpotifyError::Auth`] when port 8888 is taken, when the sign-in
/// isn't finished within five minutes, when the browser's reply fails the
/// state check, or when Spotify refuses the exchange; [`SpotifyError::Http`]
/// when the token request itself fails.
pub async fn run_auth_flow(client_id: &str) -> Result<TokenSet, SpotifyError> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8888")
        .await
        .map_err(|_| {
            SpotifyError::Auth(
                "port 8888 is in use. Close the other program using it and try again — \
                 Spotify only accepts this exact port."
                    .into(),
            )
        })?;
    let pkce = Pkce::new();
    let state: String = {
        use rand::RngExt as _;
        let mut rng = rand::rng();
        (0..16)
            .map(|_| char::from(rng.random_range(b'a'..=b'z')))
            .collect()
    };
    let url = authorize_url(client_id, &pkce, &state);
    if open::that(&url).is_err() {
        eprintln!("Open this link in your browser to sign in to Spotify:\n{url}");
    }

    let code = tokio::time::timeout(
        std::time::Duration::from_secs(300),
        wait_for_callback(&listener, &state),
    )
    .await
    .map_err(|_| {
        SpotifyError::Auth(
            "the Spotify sign-in didn't finish within 5 minutes. Run rocola again to retry.".into(),
        )
    })??;

    exchange(&[
        ("grant_type", "authorization_code"),
        ("code", &code),
        ("redirect_uri", REDIRECT_URI),
        ("client_id", client_id),
        ("code_verifier", &pkce.verifier),
    ])
    .await
}

/// Trade a stored refresh token for a fresh access token.
///
/// # Errors
///
/// Returns [`SpotifyError::Auth`] when Spotify refuses the refresh grant and
/// [`SpotifyError::Http`] when the request or its body fails.
pub async fn refresh(client_id: &str, refresh_token: &str) -> Result<TokenSet, SpotifyError> {
    exchange(&[
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", client_id),
    ])
    .await
}

async fn exchange(form: &[(&str, &str)]) -> Result<TokenSet, SpotifyError> {
    let response = reqwest::Client::new()
        .post("https://accounts.spotify.com/api/token")
        .form(form)
        .send()
        .await
        .map_err(|e| SpotifyError::Http(e.to_string()))?;
    if !response.status().is_success() {
        let status = response.status();
        return Err(SpotifyError::Auth(format!(
            "Spotify refused the sign-in ({status}). Check your client ID in ~/.config/rocola/config.toml."
        )));
    }
    response
        .json()
        .await
        .map_err(|e| SpotifyError::Http(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_code_when_state_matches() {
        let code = parse_callback_query("code=AQD123&state=xyz", "xyz").unwrap();
        assert_eq!(code, "AQD123");
    }

    #[test]
    fn rejects_mismatched_state() {
        assert!(parse_callback_query("code=AQD123&state=evil", "xyz").is_err());
    }

    #[test]
    fn picks_the_callback_request_out_of_the_raw_bytes() {
        let request =
            "GET /callback?code=AQD123&state=xyz HTTP/1.1\r\nHost: 127.0.0.1:8888\r\n\r\n";
        assert_eq!(
            callback_query(request).as_deref(),
            Some("code=AQD123&state=xyz")
        );
    }

    #[test]
    fn ignores_requests_that_are_not_the_callback() {
        // Favicon probes and stray paths must not be mistaken for the callback.
        assert_eq!(callback_query("GET /favicon.ico HTTP/1.1\r\n\r\n"), None);
        assert_eq!(callback_query("GET /callback HTTP/1.1\r\n\r\n"), None);
        assert_eq!(
            callback_query("POST /callback?code=x HTTP/1.1\r\n\r\n"),
            None
        );
    }

    #[test]
    fn ignores_a_connection_that_sent_nothing() {
        // Browsers speculatively pre-connect; that socket reads as zero bytes.
        assert_eq!(callback_query(""), None);
    }

    #[test]
    fn surfaces_spotify_denial_in_plain_english() {
        let err = parse_callback_query("error=access_denied&state=xyz", "xyz").unwrap_err();
        assert!(err.to_string().contains("didn't finish"), "got: {err}");
    }
}

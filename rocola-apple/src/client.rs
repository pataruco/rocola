//! Apple Music API client: storefront lookup, catalog resolve (ISRC batch and
//! search fallback), and library playlist write.

use std::time::Duration;

use rocola_core::{Candidate, SourceTrack};
use serde_json::json;

use crate::AppleError;
use crate::api_types::{SearchResponse, SongsResponse};

const BASE: &str = "https://api.music.apple.com";

/// Apple accepts at most 25 comma-separated values in `filter[isrc]`.
pub const ISRC_BATCH: usize = 25;

/// Fallback wait when Apple rate-limits without a usable `Retry-After`.
const DEFAULT_RETRY_AFTER_SECS: u64 = 3;

pub struct AppleClient {
    http: reqwest::Client,
    developer_token: String,
    user_token: String,
}

/// What one HTTP attempt produced: a usable response, or a rate-limit with the
/// number of seconds Apple asked us to wait.
enum Attempt {
    Response(reqwest::Response),
    RateLimited(u64),
}

impl AppleClient {
    #[must_use]
    pub fn new(developer_token: String, user_token: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            developer_token,
            user_token,
        }
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{BASE}{path}"))
            .bearer_auth(&self.developer_token)
            .header("Music-User-Token", &self.user_token)
    }

    /// Send one request, and on a 429 honour `Retry-After` and send it once
    /// more. A second 429 is reported rather than retried again.
    ///
    /// `build` is a closure so the request can be rebuilt for the retry —
    /// a `RequestBuilder` is consumed by `send`.
    async fn send(
        &self,
        build: impl Fn() -> reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, AppleError> {
        let wait = match Self::attempt(build()).await? {
            Attempt::Response(response) => return Ok(response),
            Attempt::RateLimited(wait) => wait,
        };
        tokio::time::sleep(Duration::from_secs(wait)).await;
        match Self::attempt(build()).await? {
            Attempt::Response(response) => Ok(response),
            Attempt::RateLimited(_) => Err(AppleError::Http(
                "Apple Music is rate-limiting; wait a minute and re-run".into(),
            )),
        }
    }

    async fn attempt(request: reqwest::RequestBuilder) -> Result<Attempt, AppleError> {
        let response = request
            .send()
            .await
            .map_err(|e| AppleError::Http(e.to_string()))?;
        let status = response.status();
        match status.as_u16() {
            429 => {
                let wait = response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok()?.parse().ok())
                    .unwrap_or(DEFAULT_RETRY_AFTER_SECS);
                Ok(Attempt::RateLimited(wait))
            }
            401 | 403 => Err(AppleError::Auth(
                "Apple Music rejected the sign-in. Run rocola again to reconnect.".into(),
            )),
            _ if status.is_success() => Ok(Attempt::Response(response)),
            s => Err(AppleError::Http(format!("Apple Music answered {s}"))),
        }
    }

    /// The signed-in listener's storefront, e.g. `"gb"`.
    ///
    /// # Errors
    ///
    /// Returns [`AppleError::Auth`] when Apple rejects the tokens,
    /// [`AppleError::Http`] for transport failures, rate limiting and any
    /// other status, and [`AppleError::NotInStorefront`] when Apple answers
    /// with no storefront at all.
    pub async fn storefront(&self) -> Result<String, AppleError> {
        #[derive(serde::Deserialize)]
        struct R {
            data: Vec<D>,
        }
        #[derive(serde::Deserialize)]
        struct D {
            id: String,
        }
        let r: R = self
            .send(|| self.request(reqwest::Method::GET, "/v1/me/storefront"))
            .await?
            .json()
            .await
            .map_err(|e| AppleError::Http(e.to_string()))?;
        r.data
            .into_iter()
            .next()
            .map(|d| d.id)
            .ok_or(AppleError::NotInStorefront)
    }

    /// Look up catalog songs by ISRC, 25 per request.
    ///
    /// Returns `(isrc, candidate)` pairs. An ISRC Apple doesn't know is simply
    /// absent from the result — the caller falls back to search.
    ///
    /// # Errors
    ///
    /// Returns [`AppleError::Auth`] when Apple rejects the tokens, and
    /// [`AppleError::Http`] for transport failures, rate limiting, any other
    /// status, and unreadable JSON.
    pub async fn resolve_by_isrc(
        &self,
        storefront: &str,
        isrcs: &[String],
    ) -> Result<Vec<(String, Candidate)>, AppleError> {
        let mut out = Vec::new();
        for chunk in isrcs.chunks(ISRC_BATCH) {
            let path = format!(
                "/v1/catalog/{storefront}/songs?filter[isrc]={}",
                chunk.join(",")
            );
            let r: SongsResponse = self
                .send(|| self.request(reqwest::Method::GET, &path))
                .await?
                .json()
                .await
                .map_err(|e| AppleError::Http(e.to_string()))?;
            out.extend(r.into_isrc_candidates());
        }
        Ok(out)
    }

    /// Search the catalog for a track, newest-ranked first, at most 5 hits.
    ///
    /// # Errors
    ///
    /// Returns [`AppleError::Auth`] when Apple rejects the tokens, and
    /// [`AppleError::Http`] for transport failures, rate limiting, any other
    /// status, and unreadable JSON.
    pub async fn search(
        &self,
        storefront: &str,
        track: &SourceTrack,
    ) -> Result<Vec<Candidate>, AppleError> {
        let term = format!("{} {}", track.title, track.artists.join(" "));
        let encoded: String = url_encode(&term);
        let path = format!("/v1/catalog/{storefront}/search?types=songs&limit=5&term={encoded}");
        let r: SearchResponse = self
            .send(|| self.request(reqwest::Method::GET, &path))
            .await?
            .json()
            .await
            .map_err(|e| AppleError::Http(e.to_string()))?;
        Ok(r.into_candidates())
    }

    /// Create an empty playlist in the listener's library, returning its id.
    ///
    /// # Errors
    ///
    /// Returns [`AppleError::Auth`] when Apple rejects the tokens, and
    /// [`AppleError::Http`] for transport failures, rate limiting, any other
    /// status, unreadable JSON, and a response carrying no playlist id.
    pub async fn create_playlist(
        &self,
        name: &str,
        description: &str,
    ) -> Result<String, AppleError> {
        #[derive(serde::Deserialize)]
        struct R {
            data: Vec<D>,
        }
        #[derive(serde::Deserialize)]
        struct D {
            id: String,
        }
        let body = json!({ "attributes": { "name": name, "description": description } });
        let r: R = self
            .send(|| {
                self.request(reqwest::Method::POST, "/v1/me/library/playlists")
                    .json(&body)
            })
            .await?
            .json()
            .await
            .map_err(|e| AppleError::Http(e.to_string()))?;
        r.data
            .into_iter()
            .next()
            .map(|d| d.id)
            .ok_or_else(|| AppleError::Http("Apple created the playlist but sent no id".into()))
    }

    /// Append catalog songs to a library playlist.
    ///
    /// # Errors
    ///
    /// Returns [`AppleError::Auth`] when Apple rejects the tokens, and
    /// [`AppleError::Http`] for transport failures, rate limiting and any
    /// other status.
    pub async fn add_tracks(
        &self,
        playlist_id: &str,
        catalog_ids: &[String],
    ) -> Result<(), AppleError> {
        let body = json!({
            "data": catalog_ids.iter().map(|id| json!({ "id": id, "type": "songs" })).collect::<Vec<_>>()
        });
        let path = format!("/v1/me/library/playlists/{playlist_id}/tracks");
        self.send(|| self.request(reqwest::Method::POST, &path).json(&body))
            .await?;
        Ok(())
    }
}

/// Minimal percent-encoding for a query value (space and reserved chars).
fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                char::from(b).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::url_encode;

    #[test]
    fn encodes_spaces_and_unicode() {
        assert_eq!(url_encode("a b"), "a%20b");
        assert_eq!(url_encode("café"), "caf%C3%A9");
    }
}

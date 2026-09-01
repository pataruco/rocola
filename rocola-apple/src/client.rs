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

/// Apple accepts at most 100 songs in one add-to-playlist request. A longer
/// playlist is added in several, in order.
pub const ADD_CHUNK: usize = 100;

/// Fallback wait when Apple rate-limits without a usable `Retry-After`.
const DEFAULT_RETRY_AFTER_SECS: u64 = 3;

pub struct AppleClient {
    http: reqwest::Client,
    developer_token: String,
    user_token: String,
}

/// A refused add, and how much of the playlist Apple already took.
///
/// The caller needs both: the count to tell the listener what is actually in
/// the playlist, the error to say why the rest isn't. Resuming from the count
/// is also the only way to retry without adding anything twice.
#[derive(Debug)]
pub struct AddFailure {
    /// Songs Apple accepted before the batch that failed.
    pub added: usize,
    pub error: AppleError,
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
            Attempt::RateLimited(_) => Err(AppleError::RateLimited),
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
            // Apple answers 401/403 both for a Music User Token it has stopped
            // accepting and for a developer token signed with the wrong team
            // or key, so the copy has to cover both.
            401 | 403 => Err(AppleError::Auth(
                "Apple Music rejected the request. If this keeps happening after signing in \
                 again, check team_id and key_id in ~/.config/rocola/config.toml."
                    .into(),
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
    /// [`AppleError::RateLimited`] when Apple is still rate-limiting after one
    /// retry, [`AppleError::Http`] for transport failures and any other
    /// status, and [`AppleError::NotInStorefront`] when Apple answers with no
    /// storefront at all.
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
    /// Returns [`AppleError::Auth`] when Apple rejects the tokens,
    /// [`AppleError::RateLimited`] when Apple is still rate-limiting after one
    /// retry, and [`AppleError::Http`] for transport failures, any other
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
    /// Returns [`AppleError::Auth`] when Apple rejects the tokens,
    /// [`AppleError::RateLimited`] when Apple is still rate-limiting after one
    /// retry, and [`AppleError::Http`] for transport failures, any other
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
    /// Returns [`AppleError::Auth`] when Apple rejects the tokens,
    /// [`AppleError::RateLimited`] when Apple is still rate-limiting after one
    /// retry, and [`AppleError::Http`] for transport failures, any other
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

    /// The names of the playlists already in the listener's library.
    ///
    /// One page of 100 is enough for the duplicate-run guard: a name past
    /// that is treated as absent, which costs a suffixed playlist, never a
    /// wrong write.
    ///
    /// # Errors
    ///
    /// Returns [`AppleError::Auth`] when Apple rejects the tokens,
    /// [`AppleError::RateLimited`] when Apple is still rate-limiting after one
    /// retry, and [`AppleError::Http`] for transport failures, any other
    /// status, and unreadable JSON.
    pub async fn list_playlist_names(&self) -> Result<Vec<String>, AppleError> {
        let r: crate::api_types::LibraryPlaylists = self
            .send(|| self.request(reqwest::Method::GET, "/v1/me/library/playlists?limit=100"))
            .await?
            .json()
            .await
            .map_err(|e| AppleError::Http(e.to_string()))?;
        Ok(r.names())
    }

    /// Append catalog songs to a library playlist, [`ADD_CHUNK`] at a time and
    /// in order, stopping at the first batch Apple refuses.
    ///
    /// # Errors
    ///
    /// Returns [`AddFailure`], carrying how many songs Apple had already
    /// accepted and why it refused the next batch: [`AppleError::Auth`] when
    /// Apple rejects the tokens, [`AppleError::RateLimited`] when Apple is
    /// still rate-limiting after one retry, and [`AppleError::Http`] for
    /// transport failures and any other status.
    pub async fn add_tracks(
        &self,
        playlist_id: &str,
        catalog_ids: &[String],
    ) -> Result<(), AddFailure> {
        let path = format!("/v1/me/library/playlists/{playlist_id}/tracks");
        let mut added = 0;
        for chunk in catalog_ids.chunks(ADD_CHUNK) {
            let body = json!({
                "data": chunk.iter().map(|id| json!({ "id": id, "type": "songs" })).collect::<Vec<_>>()
            });
            if let Err(error) = self
                .send(|| self.request(reqwest::Method::POST, &path).json(&body))
                .await
            {
                return Err(AddFailure { added, error });
            }
            added += chunk.len();
        }
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
    use super::{ADD_CHUNK, url_encode};

    #[test]
    fn encodes_spaces_and_unicode() {
        assert_eq!(url_encode("a b"), "a%20b");
        assert_eq!(url_encode("café"), "caf%C3%A9");
    }

    #[test]
    fn a_long_playlist_is_added_in_batches_of_a_hundred() {
        let ids: Vec<String> = (0..250).map(|i| i.to_string()).collect();
        let batches: Vec<usize> = ids.chunks(ADD_CHUNK).map(<[String]>::len).collect();
        assert_eq!(batches, vec![100, 100, 50]);
        // Exactly 100 is one request, not two — an empty second POST would be
        // a request Apple has no reason to accept.
        assert_eq!(ids[..100].chunks(ADD_CHUNK).count(), 1);
    }
}

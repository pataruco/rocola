use rocola_core::SourceTrack;
use serde::Deserialize;

use crate::SpotifyError;
use crate::api_types::PlaylistPage;
use crate::url::PlaylistRef;

#[derive(Debug, Deserialize)]
struct PlaylistMeta {
    name: String,
}

/// One playlist, as read from Spotify.
#[derive(Debug)]
pub struct Playlist {
    pub name: String,
    pub tracks: Vec<SourceTrack>,
    /// One line per item rocola can't look up on Apple Music — local files,
    /// podcast episodes, and tracks Spotify has removed — in playlist order.
    /// Reported at the end of the run, never dropped.
    pub skipped: Vec<String>,
}

/// Fetch the playlist name, every matchable track, and the names of the items
/// that can't be matched, following pagination.
///
/// # Errors
///
/// Returns [`SpotifyError::Auth`] when the token has expired,
/// [`SpotifyError::RestrictedPlaylist`] when Spotify answers 404 — deleted,
/// private, or one of Spotify's own playlists, which it hides from apps like
/// this one — [`SpotifyError::NotYourPlaylist`] when the signed-in user
/// neither owns nor collaborates on the playlist (it answers 403),
/// [`SpotifyError::RateLimited`] when Spotify asks rocola to slow down, and
/// [`SpotifyError::Http`] for transport failures and any other status.
pub async fn fetch_playlist(
    access_token: &str,
    playlist: &PlaylistRef,
) -> Result<Playlist, SpotifyError> {
    let client = reqwest::Client::new();
    let get = |url: String| {
        let client = client.clone();
        let token = access_token.to_owned();
        async move {
            let response = client
                .get(url)
                .bearer_auth(token)
                .send()
                .await
                .map_err(|e| SpotifyError::Http(e.to_string()))?;
            match response.status().as_u16() {
                200 => Ok(response),
                401 => Err(SpotifyError::Auth(
                    "your Spotify sign-in has expired. Run rocola again and it will sign you in."
                        .into(),
                )),
                403 => Err(SpotifyError::NotYourPlaylist),
                404 => Err(SpotifyError::RestrictedPlaylist),
                429 => {
                    // Spec risk 4: honour Retry-After once, then give up loudly.
                    let wait = response
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok()?.parse().ok())
                        .unwrap_or(3u64);
                    tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
                    Err(SpotifyError::RateLimited)
                }
                s => Err(SpotifyError::Http(format!("Spotify answered {s}"))),
            }
        }
    };

    let meta: PlaylistMeta = get(format!(
        "https://api.spotify.com/v1/playlists/{}?fields=name",
        playlist.0
    ))
    .await?
    .json()
    .await
    .map_err(|e| SpotifyError::Http(e.to_string()))?;

    let mut tracks = Vec::new();
    let mut skipped = Vec::new();
    let mut next = Some(format!(
        "https://api.spotify.com/v1/playlists/{}/items?limit=100&fields=next,items(is_local,item(name,duration_ms,is_local,type,album(name),artists(name),external_ids))",
        playlist.0
    ));
    while let Some(url) = next {
        let page: PlaylistPage = get(url)
            .await?
            .json()
            .await
            .map_err(|e| SpotifyError::Http(e.to_string()))?;
        let page_tracks = page.partition();
        tracks.extend(page_tracks.tracks);
        skipped.extend(page_tracks.skipped);
        next = page.next;
    }
    Ok(Playlist {
        name: meta.name,
        tracks,
        skipped,
    })
}

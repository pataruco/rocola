use rocola_core::SourceTrack;
use serde::Deserialize;

use crate::SpotifyError;
use crate::api_types::PlaylistPage;
use crate::url::PlaylistRef;

#[derive(Debug, Deserialize)]
struct PlaylistMeta {
    name: String,
}

/// Fetch playlist name and every track, following pagination.
///
/// # Errors
///
/// Returns [`SpotifyError::Auth`] when the token has expired,
/// [`SpotifyError::RestrictedPlaylist`] when Spotify hides the playlist from
/// apps like this one (it answers 404), [`SpotifyError::NotYourPlaylist`] when
/// the signed-in user neither owns nor collaborates on the playlist (it
/// answers 403), and [`SpotifyError::Http`] for transport failures, rate
/// limiting, and any other status.
pub async fn fetch_playlist(
    access_token: &str,
    playlist: &PlaylistRef,
) -> Result<(String, Vec<SourceTrack>), SpotifyError> {
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
                    "your Spotify sign-in has expired. rocola will sign you in again on the next run."
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
                    Err(SpotifyError::Http(
                        "Spotify is rate-limiting; wait a minute and re-run".into(),
                    ))
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
    let mut next = Some(format!(
        "https://api.spotify.com/v1/playlists/{}/items?limit=100&fields=next,items(is_local,item(name,duration_ms,is_local,album(name),artists(name),external_ids))",
        playlist.0
    ));
    while let Some(url) = next {
        let page: PlaylistPage = get(url)
            .await?
            .json()
            .await
            .map_err(|e| SpotifyError::Http(e.to_string()))?;
        tracks.extend(page.source_tracks());
        next = page.next;
    }
    Ok((meta.name, tracks))
}

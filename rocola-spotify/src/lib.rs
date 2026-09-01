//! Spotify source: Authorization Code + PKCE auth and playlist fetch.

pub mod api_types;
pub mod auth;
pub mod client;
pub mod pkce;
pub mod url;

pub use auth::{TokenSet, refresh, run_auth_flow};
pub use client::{Playlist, fetch_playlist};
pub use pkce::{Pkce, REDIRECT_URI, authorize_url};
pub use url::{PlaylistRef, parse_playlist_url};

#[derive(Debug, thiserror::Error)]
pub enum SpotifyError {
    #[error("{0}")]
    BadUrl(String),
    #[error("Spotify request failed: {0}. Check your connection and try again.")]
    Http(String),
    #[error("Spotify is asking rocola to slow down. Wait a minute, then run rocola again.")]
    RateLimited,
    #[error("Spotify sign-in problem: {0}")]
    Auth(String),
    #[error(
        "Spotify couldn't find that playlist. It may have been deleted, it may be private, \
         or it may be one of Spotify's own playlists — Discover Weekly, Release Radar, an \
         editorial mix — which Spotify blocks apps like rocola from reading. Check the link, \
         or try a playlist a person made."
    )]
    RestrictedPlaylist,
    #[error(
        "Spotify now only lets apps like rocola read playlists you own or collaborate on. \
         Open the playlist in Spotify, use 'Add to other playlist' to copy it into one of \
         yours, then run rocola on the copy."
    )]
    NotYourPlaylist,
}

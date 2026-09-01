use rocola_core::SourceTrack;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct PlaylistPage {
    pub next: Option<String>,
    pub items: Vec<PageItem>,
}

#[derive(Debug, Deserialize)]
pub struct PageItem {
    pub item: Option<ApiTrack>,
}

#[derive(Debug, Deserialize)]
pub struct ApiTrack {
    pub name: String,
    pub duration_ms: u32,
    pub album: ApiAlbum,
    pub artists: Vec<ApiArtist>,
    #[serde(default)]
    pub external_ids: ExternalIds,
    #[serde(default)]
    pub is_local: bool,
}

#[derive(Debug, Deserialize)]
pub struct ApiAlbum {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct ApiArtist {
    pub name: String,
}

#[derive(Debug, Default, Deserialize)]
pub struct ExternalIds {
    pub isrc: Option<String>,
}

/// Spotify sends no metadata at all for a track it has removed from its
/// catalog, so there is nothing to name it by.
const REMOVED_TRACK: &str = "(removed track)";

/// One page's items, split into the tracks rocola can look up on Apple Music
/// and the ones it can't.
#[derive(Debug, Default)]
pub struct PageTracks {
    pub tracks: Vec<SourceTrack>,
    /// One display line per left-out item, in playlist order.
    pub skipped: Vec<String>,
}

impl PlaylistPage {
    /// Split the page into matchable tracks and un-matchable items.
    ///
    /// Local files and removed (null) tracks have no counterpart in Apple
    /// Music's catalog. They are separated out here rather than dropped, so
    /// the caller can name every one of them in the final report — the spec's
    /// rule that nothing ever disappears in silence.
    #[must_use]
    pub fn partition(&self) -> PageTracks {
        let mut out = PageTracks::default();
        for item in &self.items {
            let Some(track) = item.item.as_ref() else {
                out.skipped.push(REMOVED_TRACK.to_owned());
                continue;
            };
            if track.is_local {
                out.skipped.push(display_line(track));
                continue;
            }
            out.tracks.push(SourceTrack {
                title: track.name.clone(),
                artists: track.artists.iter().map(|a| a.name.clone()).collect(),
                album: track.album.name.clone(),
                duration_ms: track.duration_ms,
                isrc: track
                    .external_ids
                    .isrc
                    .as_deref()
                    .and_then(rocola_core::normalize::normalize_isrc),
            });
        }
        out
    }
}

/// `"Title — Artist, Artist"`, matching the report lines the matched tracks
/// get. A local file can carry no artists at all, so the dash is conditional.
fn display_line(track: &ApiTrack) -> String {
    let artists: Vec<&str> = track.artists.iter().map(|a| a.name.as_str()).collect();
    if artists.is_empty() {
        track.name.clone()
    } else {
        format!(
            "{name} — {artists}",
            name = track.name,
            artists = artists.join(", ")
        )
    }
}

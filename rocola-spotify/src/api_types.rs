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

impl PlaylistPage {
    /// Local files and removed (null) tracks can't exist on Apple Music's
    /// catalog; they are skipped here and reported by the caller.
    #[must_use]
    pub fn source_tracks(&self) -> Vec<SourceTrack> {
        self.items
            .iter()
            .filter_map(|i| i.item.as_ref())
            .filter(|t| !t.is_local)
            .map(|t| SourceTrack {
                title: t.name.clone(),
                artists: t.artists.iter().map(|a| a.name.clone()).collect(),
                album: t.album.name.clone(),
                duration_ms: t.duration_ms,
                isrc: t
                    .external_ids
                    .isrc
                    .as_deref()
                    .and_then(rocola_core::normalize::normalize_isrc),
            })
            .collect()
    }
}

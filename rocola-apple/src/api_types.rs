//! Wire shapes for the Apple Music catalog responses rocola reads, and the
//! mapping from those shapes onto rocola-core's [`Candidate`].

use rocola_core::{Candidate, MatchedBy};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct SongsResponse {
    pub data: Vec<Song>,
}

#[derive(Debug, Deserialize)]
pub struct Song {
    pub id: String,
    pub attributes: SongAttributes,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SongAttributes {
    pub name: String,
    pub artist_name: String,
    pub album_name: String,
    pub duration_in_millis: u32,
    pub isrc: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SearchResponse {
    pub results: SearchResults,
}

#[derive(Debug, Default, Deserialize)]
pub struct SearchResults {
    #[serde(default)]
    pub songs: Option<SongsResponse>,
}

fn to_candidate(song: Song, matched_by: MatchedBy) -> Candidate {
    Candidate {
        catalog_id: song.id,
        title: song.attributes.name,
        // Apple returns one display string ("A & B"); keep it whole — the
        // scorer's overlap handles multi-artist sources against it.
        artists: vec![song.attributes.artist_name],
        album: song.attributes.album_name,
        duration_ms: song.attributes.duration_in_millis,
        matched_by,
    }
}

impl SongsResponse {
    /// Pair every song that carries an ISRC with its candidate.
    ///
    /// Songs without an ISRC are dropped: the caller keys results by ISRC, so
    /// there is nothing to key them on.
    #[must_use]
    pub fn into_isrc_candidates(self) -> Vec<(String, Candidate)> {
        self.data
            .into_iter()
            .filter_map(|song| {
                let isrc = song.attributes.isrc.clone()?;
                Some((isrc, to_candidate(song, MatchedBy::Isrc)))
            })
            .collect()
    }
}

impl SearchResponse {
    /// The song hits from a search, in Apple's ranking order.
    #[must_use]
    pub fn into_candidates(self) -> Vec<Candidate> {
        self.results.songs.map_or_else(Vec::new, |s| {
            s.data
                .into_iter()
                .map(|song| to_candidate(song, MatchedBy::Search))
                .collect()
        })
    }
}

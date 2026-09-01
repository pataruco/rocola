use std::collections::HashMap;

use crate::matching::classify;
use crate::types::{Candidate, SourceTrack, TrackMatch};

/// The write-side seam (spec §The seam that matters). Implemented by
/// rocola-apple; implemented by fakes in tests. Native async fn in trait —
/// used generically, never as `dyn`.
// Trait is only ever used generically (never `dyn`), so the missing auto
// trait bounds this lint warns about don't apply here.
#[allow(async_fn_in_trait)]
pub trait MusicTarget {
    type Error;

    async fn resolve_by_isrc(
        &self,
        isrcs: &[String],
    ) -> Result<Vec<(String, Candidate)>, Self::Error>;
    async fn search(&self, track: &SourceTrack) -> Result<Vec<Candidate>, Self::Error>;
}

/// Tier 1: one batched ISRC pass. Tier 2: per-track search for the rest.
/// Every input track appears in the output, classified, in input order.
///
/// # Errors
///
/// Returns `T::Error` if either `resolve_by_isrc` or `search` fails on the
/// underlying target.
// Never spawned across threads; single-threaded TUI runtime, so the
// resulting !Send future is fine.
#[allow(clippy::future_not_send)]
pub async fn match_tracks<T: MusicTarget>(
    target: &T,
    tracks: &[SourceTrack],
) -> Result<Vec<TrackMatch>, T::Error> {
    let isrcs: Vec<String> = tracks.iter().filter_map(|t| t.isrc.clone()).collect();
    let mut by_isrc: HashMap<String, Vec<Candidate>> = HashMap::new();
    for (isrc, candidate) in target.resolve_by_isrc(&isrcs).await? {
        by_isrc.entry(isrc).or_default().push(candidate);
    }

    let mut out = Vec::with_capacity(tracks.len());
    for track in tracks {
        let isrc_hits = track
            .isrc
            .as_ref()
            .and_then(|i| by_isrc.get(i))
            .cloned()
            .unwrap_or_default();
        let candidates = if isrc_hits.is_empty() {
            target.search(track).await?
        } else {
            isrc_hits
        };
        out.push(classify(track, candidates));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Candidate, Confidence, MatchedBy, SourceTrack};

    struct FakeTarget;

    // Fixtures below don't need to await anything; `async fn` still mirrors
    // the trait signature FakeTarget stands in for.
    #[allow(clippy::unused_async_trait_impl)]
    impl MusicTarget for FakeTarget {
        type Error = std::convert::Infallible;

        async fn resolve_by_isrc(
            &self,
            isrcs: &[String],
        ) -> Result<Vec<(String, Candidate)>, Self::Error> {
            // Knows exactly one recording by ISRC.
            Ok(isrcs
                .iter()
                .filter(|i| i.as_str() == "USUM70701234")
                .map(|i| {
                    (
                        i.clone(),
                        Candidate {
                            catalog_id: "900".into(),
                            title: "Umbrella".into(),
                            artists: vec!["Rihanna".into()],
                            album: "Good Girl Gone Bad".into(),
                            duration_ms: 275_000,
                            matched_by: MatchedBy::Isrc,
                        },
                    )
                })
                .collect())
        }

        async fn search(&self, track: &SourceTrack) -> Result<Vec<Candidate>, Self::Error> {
            if track.title == "Findable" {
                Ok(vec![Candidate {
                    catalog_id: "111".into(),
                    title: "Findable".into(),
                    artists: track.artists.clone(),
                    album: track.album.clone(),
                    duration_ms: track.duration_ms,
                    matched_by: MatchedBy::Search,
                }])
            } else {
                Ok(vec![])
            }
        }
    }

    fn track(title: &str, isrc: Option<&str>) -> SourceTrack {
        SourceTrack {
            title: title.into(),
            artists: vec!["Rihanna".into()],
            album: "Good Girl Gone Bad".into(),
            duration_ms: 275_000,
            isrc: isrc.map(Into::into),
        }
    }

    #[tokio::test]
    async fn isrc_hit_search_hit_and_miss_all_appear_in_input_order() {
        let tracks = vec![
            track("Umbrella", Some("USUM70701234")),
            track("Findable", None),
            track("Vanished", Some("XX0000000000")),
        ];
        let matches = match_tracks(&FakeTarget, &tracks).await.unwrap();
        assert_eq!(matches.len(), 3);
        assert_eq!(matches[0].confidence, Confidence::Exact);
        assert_eq!(matches[1].confidence, Confidence::High);
        assert_eq!(matches[2].confidence, Confidence::NotFound);
        assert_eq!(matches[2].source.title, "Vanished");
    }
}

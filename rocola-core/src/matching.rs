use crate::normalize::{normalize_artist, normalize_title};
use crate::types::{Candidate, Confidence, MatchedBy, SourceTrack, TrackMatch};

const DURATION_TIGHT_MS: u32 = 3_000;
const DURATION_LOOSE_MS: u32 = 10_000;
const HIGH_THRESHOLD: u32 = 85;
const AMBIGUOUS_THRESHOLD: u32 = 50;

/// 0–100. Weights: title 40, artists 30, duration 20 (tight) / 10 (loose), album 10.
#[must_use]
pub fn score(source: &SourceTrack, candidate: &Candidate) -> u32 {
    let mut total = 0;
    if normalize_title(&source.title) == normalize_title(&candidate.title) {
        total += 40;
    }
    total += artist_overlap(&source.artists, &candidate.artists);
    let delta = source.duration_ms.abs_diff(candidate.duration_ms);
    if delta <= DURATION_TIGHT_MS {
        total += 20;
    } else if delta <= DURATION_LOOSE_MS {
        total += 10;
    }
    if normalize_title(&source.album) == normalize_title(&candidate.album) {
        total += 10;
    }
    total
}

fn artist_overlap(a: &[String], b: &[String]) -> u32 {
    let norm = |xs: &[String]| xs.iter().map(|x| normalize_artist(x)).collect::<Vec<_>>();
    let (a, b) = (norm(a), norm(b));
    let shared = a.iter().filter(|x| b.contains(x)).count();
    let denom = a.len().max(b.len()).max(1);
    u32::try_from(30 * shared / denom).unwrap_or(0)
}

/// Sort candidates best-first and classify.
///
/// An ISRC-found candidate whose duration is within 3s is trusted as the
/// same recording: `Exact`. Score alone can rank a well-scoring search decoy
/// above such a candidate, so the ISRC-exact one (the best-scoring among any
/// that qualify) is promoted to the front of the list before it is used for
/// auto-accept downstream.
#[must_use]
pub fn classify(source: &SourceTrack, mut candidates: Vec<Candidate>) -> TrackMatch {
    candidates.sort_by_key(|c| std::cmp::Reverse(score(source, c)));

    // Sorted best-first, so the earliest match here is the best-scoring
    // ISRC-exact candidate.
    let isrc_exact_pos = candidates.iter().position(|c| {
        c.matched_by == MatchedBy::Isrc
            && source.duration_ms.abs_diff(c.duration_ms) <= DURATION_TIGHT_MS
    });
    if let Some(pos) = isrc_exact_pos {
        candidates.swap(0, pos);
    }

    let confidence = if isrc_exact_pos.is_some() {
        Confidence::Exact
    } else {
        candidates.first().map_or(Confidence::NotFound, |best| {
            let s = score(source, best);
            if s >= HIGH_THRESHOLD {
                Confidence::High
            } else if s >= AMBIGUOUS_THRESHOLD {
                Confidence::Ambiguous
            } else {
                Confidence::NotFound
            }
        })
    };

    TrackMatch {
        source: source.clone(),
        candidates,
        confidence,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::MatchedBy;

    fn src(title: &str, artist: &str, album: &str, ms: u32, isrc: Option<&str>) -> SourceTrack {
        SourceTrack {
            title: title.into(),
            artists: vec![artist.into()],
            album: album.into(),
            duration_ms: ms,
            isrc: isrc.map(Into::into),
        }
    }

    fn cand(title: &str, artist: &str, album: &str, ms: u32, by: MatchedBy) -> Candidate {
        Candidate {
            catalog_id: "123".into(),
            title: title.into(),
            artists: vec![artist.into()],
            album: album.into(),
            duration_ms: ms,
            matched_by: by,
        }
    }

    #[test]
    fn identical_track_scores_100() {
        let s = src("Umbrella", "Rihanna", "Good Girl Gone Bad", 275_000, None);
        let c = cand(
            "Umbrella",
            "Rihanna",
            "Good Girl Gone Bad",
            275_000,
            MatchedBy::Search,
        );
        assert_eq!(score(&s, &c), 100);
    }

    #[test]
    fn remaster_suffix_does_not_lower_title_score() {
        let s = src(
            "Bohemian Rhapsody - Remastered 2011",
            "Queen",
            "A Night at the Opera",
            354_000,
            None,
        );
        let c = cand(
            "Bohemian Rhapsody",
            "Queen",
            "A Night at the Opera (Deluxe)",
            355_000,
            MatchedBy::Search,
        );
        assert!(score(&s, &c) >= 85, "got {}", score(&s, &c));
    }

    #[test]
    fn isrc_candidate_with_close_duration_is_exact() {
        let s = src(
            "Umbrella",
            "Rihanna",
            "Good Girl Gone Bad",
            275_000,
            Some("USUM70701234"),
        );
        let c = cand(
            "Umbrella (feat. JAY-Z)",
            "Rihanna",
            "Good Girl Gone Bad",
            276_000,
            MatchedBy::Isrc,
        );
        let m = classify(&s, vec![c]);
        assert_eq!(m.confidence, Confidence::Exact);
    }

    #[test]
    fn no_candidates_is_not_found() {
        let s = src("Obscure B-side", "Nobody", "Nothing", 100_000, None);
        assert_eq!(classify(&s, vec![]).confidence, Confidence::NotFound);
    }

    #[test]
    fn weak_candidates_are_ambiguous_and_sorted_best_first() {
        let s = src(
            "Wish You Were Here",
            "Pink Floyd",
            "Wish You Were Here",
            334_000,
            None,
        );
        let weak = cand(
            "Wish You Were Here",
            "Avril Lavigne",
            "Goodbye Lullaby",
            225_000,
            MatchedBy::Search,
        );
        let close = cand(
            "Wish You Were Here - Live",
            "Pink Floyd",
            "Pulse",
            340_000,
            MatchedBy::Search,
        );
        let m = classify(&s, vec![weak, close]);
        assert_eq!(m.confidence, Confidence::Ambiguous);
        assert_eq!(m.candidates[0].artists[0], "Pink Floyd");
    }

    #[test]
    fn isrc_exact_beats_higher_scoring_search_decoy() {
        // Isrc candidate: title mismatch +0, artist match +30, duration exact
        // +20, album mismatch +0 = 50.
        let s = src("Umbrella", "Rihanna", "Good Girl Gone Bad", 275_000, None);
        let isrc_exact = cand("We Found Love", "Rihanna", "Loud", 275_000, MatchedBy::Isrc);
        // Search decoy: title +40, artist +30, duration within 10s +10, album
        // +10 = 90 — outscores the ISRC-exact candidate on `score` alone.
        let search_decoy = cand(
            "Umbrella",
            "Rihanna",
            "Good Girl Gone Bad",
            280_000,
            MatchedBy::Search,
        );
        assert_eq!(score(&s, &isrc_exact), 50);
        assert_eq!(score(&s, &search_decoy), 90);

        let m = classify(&s, vec![isrc_exact, search_decoy]);
        assert_eq!(m.confidence, Confidence::Exact);
        assert_eq!(m.candidates[0].matched_by, MatchedBy::Isrc);
    }
}

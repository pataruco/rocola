use serde::{Deserialize, Serialize};

/// A track as read from the source service (Spotify).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceTrack {
    pub title: String,
    pub artists: Vec<String>,
    pub album: String,
    pub duration_ms: u32,
    /// International Standard Recording Code, when the source provides one.
    pub isrc: Option<String>,
}

/// How a candidate was found on the target service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MatchedBy {
    Isrc,
    Search,
}

/// A possible counterpart on the target service (Apple Music).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    pub catalog_id: String,
    pub title: String,
    pub artists: Vec<String>,
    pub album: String,
    pub duration_ms: u32,
    pub matched_by: MatchedBy,
}

/// Outcome class for one source track. Exact and High are auto-accepted;
/// Ambiguous goes to the review queue; `NotFound` is reported, never dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Confidence {
    Exact,
    High,
    Ambiguous,
    NotFound,
}

/// One source track with its ranked candidates and classification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackMatch {
    pub source: SourceTrack,
    pub candidates: Vec<Candidate>,
    pub confidence: Confidence,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_track_roundtrips_through_json() {
        let track = SourceTrack {
            title: "Pienso en Ti".into(),
            artists: vec!["Chavela Vargas".into()],
            album: "Colección".into(),
            duration_ms: 187_000,
            isrc: Some("MXF049800212".into()),
        };
        let json = serde_json::to_string(&track).unwrap();
        let back: SourceTrack = serde_json::from_str(&json).unwrap();
        assert_eq!(track, back);
    }
}

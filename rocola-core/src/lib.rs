//! Domain types and the matching engine.
//!
//! This crate is deliberately free of I/O and network dependencies so the
//! whole matching pipeline can be developed and tested against JSON fixtures
//! with zero credentials.

pub mod matching;
pub mod normalize;
pub mod pipeline;
pub mod types;

pub use matching::{classify, score};
pub use pipeline::{MusicTarget, match_tracks};
pub use types::{Candidate, Confidence, MatchedBy, SourceTrack, TrackMatch};

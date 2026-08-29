//! Domain types and the matching engine.
//!
//! This crate is deliberately free of I/O and network dependencies so the
//! whole matching pipeline can be developed and tested against JSON fixtures
//! with zero credentials.

pub mod types;

pub use types::{Candidate, Confidence, MatchedBy, SourceTrack, TrackMatch};

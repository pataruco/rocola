# rocola Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Rust TUI that takes a Spotify playlist URL and recreates the playlist on the user's Apple Music account, with a credential-free matching core.

**Architecture:** Four-crate workspace (already scaffolded). `rocola-core` holds pure domain types and the matching engine — no I/O. `rocola-spotify` (PKCE auth + playlist fetch) and `rocola-apple` (tokens, ISRC resolve, playlist write) implement network providers. `rocola` is the binary: config, orchestration, ratatui TUI.

**Tech Stack:** Rust 2024 edition, tokio, reqwest (rustls), serde, thiserror, ratatui + crossterm, jsonwebtoken (ES256), axum (local auth callback server), hurl for live-network verification.

**Spec:** `docs/design/2026-08-29-spotify-to-apple-music.md` — the plan argues from that spec; read both.

## Global Constraints

- Workspace lints already set: `clippy::pedantic` + `nursery` warn, `unsafe_code = "forbid"`. All code must pass `just ci` (fmt --check, clippy -D warnings, tests).
- `rocola-core` must have **no I/O and no network dependencies** — serde/thiserror only.
- **No network in `cargo test`.** Unit tests use committed JSON fixtures. Live-network verification uses hurl files under `tests/hurl/`, run manually via `just hurl-*` recipes with secrets from env vars.
- Secrets never in the repo: hurl files reference `{{spotify_token}}` etc. from `--variables-file tests/hurl/vars.env` which is git-ignored.
- Config file `~/.config/rocola/config.toml` created with mode 0600 at open time.
- Spotify redirect URI is exactly `http://127.0.0.1:8888/callback` (loopback literal required; `localhost` disallowed by Spotify since Nov 2025).
- User-facing copy: plain English, every error names the fix (spec §Content design).
- **Git checkpoints:** Pedro performs ALL git operations. Every "Checkpoint" step means: STOP, report what changed, hand Pedro a suggested commit message, and wait. Never run `git add`/`commit`/`push`.
- Dependency versions (current as of 2026-08-29): tokio 1.53, reqwest 0.13 (default-tls off, rustls-tls on), serde 1, serde_json 1, thiserror 2, ratatui 0.30, crossterm 0.29, jsonwebtoken 11, axum 0.8, open 5, toml 1, dirs 6, sha2 0.11, base64 0.23, rand 0.10, tempfile 3 (dev).

---

## Task 0: M0 probe — MusicKit JS authorises from a loopback origin

The one unverified assumption that can invalidate the design (spec §Risks 1). Do it first; it needs Pedro's Apple Developer membership and ~30 minutes, and produces no production code.

**Files:**
- Create: `probes/musickit-loopback/index.html`
- Create: `probes/musickit-loopback/README.md`

**Interfaces:** none — throwaway probe; result recorded in the design doc.

- [ ] **Step 1: Write the probe page**

`probes/musickit-loopback/index.html`:

```html
<!doctype html>
<meta charset="utf-8">
<title>rocola M0 probe</title>
<script src="https://js-cdn.music.apple.com/musickit/v3/musickit.js" async></script>
<body>
<button id="go" disabled>Sign in to Apple Music</button>
<pre id="out">waiting for MusicKit…</pre>
<script>
const out = (m) => document.getElementById('out').textContent = m;
document.addEventListener('musickitloaded', async () => {
  try {
    await MusicKit.configure({
      developerToken: new URLSearchParams(location.search).get('devtoken'),
      app: { name: 'rocola-probe', build: '0' },
    });
    const btn = document.getElementById('go');
    btn.disabled = false;
    btn.onclick = async () => {
      try {
        const userToken = await MusicKit.getInstance().authorize();
        out('SUCCESS — user token (first 16 chars): ' + userToken.slice(0, 16) + '…');
      } catch (e) { out('AUTHORIZE FAILED: ' + e); }
    };
    out('MusicKit configured. Click the button.');
  } catch (e) { out('CONFIGURE FAILED: ' + e); }
});
</script>
```

- [ ] **Step 2: Write the probe README**

`probes/musickit-loopback/README.md`:

```markdown
# M0 probe: does MusicKit JS authorise from http://127.0.0.1?

Answers spec Risk 1. Needs an Apple Developer membership.

1. Mint a short-lived developer token (any tool; 1h expiry is fine).
2. `python3 -m http.server 8899 --bind 127.0.0.1` from this directory.
3. Open `http://127.0.0.1:8899/index.html?devtoken=<JWT>` in Safari, Chrome, Firefox.
4. Click the button, sign in.

Record PASS/FAIL per browser in `docs/design/2026-08-29-spotify-to-apple-music.md` §Risks 1.
PASS in at least one mainstream browser = design holds.
FAIL everywhere = STOP; the Apple auth flow needs a redesign (likely mkcert + https) before Tasks 8–11 are valid.
```

- [ ] **Step 3: Pedro runs the probe** (needs his .p8 key; agent cannot do this step)

- [ ] **Step 4: Record the result in the design doc Risks section**

- [ ] **Step 5: Checkpoint — Pedro commits**

Suggested: `docs: record M0 probe result — MusicKit JS on loopback origin`

---

## Task 1: rocola-core — domain types

**Files:**
- Modify: `rocola-core/Cargo.toml` (add serde, thiserror)
- Create: `rocola-core/src/types.rs`
- Modify: `rocola-core/src/lib.rs`

**Interfaces:**
- Produces: `SourceTrack { title: String, artists: Vec<String>, album: String, duration_ms: u32, isrc: Option<String> }`, `Candidate { catalog_id: String, title: String, artists: Vec<String>, album: String, duration_ms: u32, matched_by: MatchedBy }`, `MatchedBy { Isrc, Search }`, `Confidence { Exact, High, Ambiguous, NotFound }`, `TrackMatch { source: SourceTrack, candidates: Vec<Candidate>, confidence: Confidence }`. All derive `Debug, Clone, PartialEq, Serialize, Deserialize`; `Confidence`/`MatchedBy` also `Copy, Eq`.

- [ ] **Step 1: Add dependencies**

In `rocola-core/Cargo.toml` under `[dependencies]`:

```toml
serde = { version = "1", features = ["derive"] }
thiserror = "2"
```

- [ ] **Step 2: Write the failing test**

In `rocola-core/src/types.rs` (bottom of the new file, but write test first conceptually — the file must exist to hold it):

```rust
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
```

Add `serde_json = "1"` under `[dev-dependencies]` in `rocola-core/Cargo.toml`.

- [ ] **Step 3: Run to verify it fails**

Run: `just test rocola-core` — Expected: compile error, `SourceTrack` not defined.

- [ ] **Step 4: Write the types**

Top of `rocola-core/src/types.rs`:

```rust
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
/// Ambiguous goes to the review queue; NotFound is reported, never dropped.
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
```

Replace `rocola-core/src/lib.rs` contents with:

```rust
pub mod types;

pub use types::{Candidate, Confidence, MatchedBy, SourceTrack, TrackMatch};
```

- [ ] **Step 5: Run to verify it passes**

Run: `just test rocola-core` then `just lint rocola-core` — Expected: PASS, no clippy warnings.

- [ ] **Step 6: Checkpoint — Pedro commits**

Suggested: `feat(core): add domain types for tracks, candidates and match confidence`

---

## Task 2: rocola-core — normalisation (ISRC, titles, artists)

**Files:**
- Create: `rocola-core/src/normalize.rs`
- Modify: `rocola-core/src/lib.rs`

**Interfaces:**
- Produces: `normalize_isrc(&str) -> Option<String>` (12-char uppercase alphanumeric or None), `normalize_title(&str) -> String`, `normalize_artist(&str) -> String`.

- [ ] **Step 1: Write the failing tests**

`rocola-core/src/normalize.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isrc_uppercases_and_strips_hyphens() {
        assert_eq!(normalize_isrc("gb-arl-98-00212"), Some("GBARL9800212".into()));
        assert_eq!(normalize_isrc("USUM71703861"), Some("USUM71703861".into()));
    }

    #[test]
    fn isrc_rejects_wrong_shape() {
        assert_eq!(normalize_isrc(""), None);
        assert_eq!(normalize_isrc("TOO-SHORT"), None);
        assert_eq!(normalize_isrc("GBARL98002123456"), None);
    }

    #[test]
    fn title_strips_noise_and_case() {
        assert_eq!(normalize_title("Bohemian Rhapsody - Remastered 2011"), "bohemian rhapsody");
        assert_eq!(normalize_title("Umbrella (feat. JAY-Z)"), "umbrella");
        assert_eq!(normalize_title("Como Te Extraño  [En Vivo]"), "como te extraño");
    }

    #[test]
    fn artist_lowercases_and_trims() {
        assert_eq!(normalize_artist("  Café Tacvba "), "café tacvba");
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `just test rocola-core` — Expected: compile error, functions not defined.

- [ ] **Step 3: Implement**

Top of `rocola-core/src/normalize.rs`:

```rust
/// Canonical ISRC: 12 uppercase alphanumerics. Spotify data is inconsistently
/// cased and occasionally hyphenated, so normalise before any comparison.
pub fn normalize_isrc(raw: &str) -> Option<String> {
    let cleaned: String = raw
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_uppercase())
        .collect();
    (cleaned.len() == 12).then_some(cleaned)
}

/// Lowercase, drop bracketed/parenthesised segments, drop everything after
/// " - " (Spotify's suffix convention: "- Remastered", "- Live", …),
/// collapse whitespace.
pub fn normalize_title(raw: &str) -> String {
    let no_dash_suffix = raw.split(" - ").next().unwrap_or(raw);
    let mut out = String::with_capacity(no_dash_suffix.len());
    let mut depth = 0u32;
    for c in no_dash_suffix.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.extend(c.to_lowercase()),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn normalize_artist(raw: &str) -> String {
    raw.trim().to_lowercase()
}
```

Add to `rocola-core/src/lib.rs`:

```rust
pub mod normalize;
```

- [ ] **Step 4: Run to verify pass**

Run: `just test rocola-core && just lint rocola-core` — Expected: PASS.

- [ ] **Step 5: Checkpoint — Pedro commits**

Suggested: `feat(core): normalise ISRCs, titles and artists for matching`

---

## Task 3: rocola-core — scoring and classification

**Files:**
- Create: `rocola-core/src/matching.rs`
- Modify: `rocola-core/src/lib.rs`

**Interfaces:**
- Consumes: types from Task 1, normalisers from Task 2.
- Produces: `score(source: &SourceTrack, candidate: &Candidate) -> u32` (0–100), `classify(source: &SourceTrack, candidates: Vec<Candidate>) -> TrackMatch` (sorts candidates best-first, sets `confidence`).
- Thresholds (fixed constants, tested): ISRC match with duration within 3s ⇒ `Exact`. Otherwise score ≥ 85 ⇒ `High`; any candidate scoring ≥ 50 ⇒ `Ambiguous`; else `NotFound`.
- Score weights: title equality after normalisation 40; artist overlap up to 30 (`shared/max(len)` scaled); duration within 3s 20, within 10s 10; normalised album equality 10.

- [ ] **Step 1: Write the failing tests**

Bottom of `rocola-core/src/matching.rs`:

```rust
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
        let c = cand("Umbrella", "Rihanna", "Good Girl Gone Bad", 275_000, MatchedBy::Search);
        assert_eq!(score(&s, &c), 100);
    }

    #[test]
    fn remaster_suffix_does_not_lower_title_score() {
        let s = src("Bohemian Rhapsody - Remastered 2011", "Queen", "A Night at the Opera", 354_000, None);
        let c = cand("Bohemian Rhapsody", "Queen", "A Night at the Opera (Deluxe)", 355_000, MatchedBy::Search);
        assert!(score(&s, &c) >= 85, "got {}", score(&s, &c));
    }

    #[test]
    fn isrc_candidate_with_close_duration_is_exact() {
        let s = src("Umbrella", "Rihanna", "Good Girl Gone Bad", 275_000, Some("USUM70701234"));
        let c = cand("Umbrella (feat. JAY-Z)", "Rihanna", "Good Girl Gone Bad", 276_000, MatchedBy::Isrc);
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
        let s = src("Wish You Were Here", "Pink Floyd", "Wish You Were Here", 334_000, None);
        let weak = cand("Wish You Were Here", "Avril Lavigne", "Goodbye Lullaby", 225_000, MatchedBy::Search);
        let close = cand("Wish You Were Here - Live", "Pink Floyd", "Pulse", 340_000, MatchedBy::Search);
        let m = classify(&s, vec![weak, close]);
        assert_eq!(m.confidence, Confidence::Ambiguous);
        assert_eq!(m.candidates[0].artists[0], "Pink Floyd");
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `just test rocola-core` — Expected: compile error.

- [ ] **Step 3: Implement**

Top of `rocola-core/src/matching.rs`:

```rust
use crate::normalize::{normalize_artist, normalize_title};
use crate::types::{Candidate, Confidence, MatchedBy, SourceTrack, TrackMatch};

const DURATION_TIGHT_MS: u32 = 3_000;
const DURATION_LOOSE_MS: u32 = 10_000;
const HIGH_THRESHOLD: u32 = 85;
const AMBIGUOUS_THRESHOLD: u32 = 50;

/// 0–100. Weights: title 40, artists 30, duration 20 (tight) / 10 (loose), album 10.
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

/// Sort candidates best-first and classify. An ISRC-found candidate whose
/// duration is within 3s is trusted as the same recording: `Exact`.
pub fn classify(source: &SourceTrack, mut candidates: Vec<Candidate>) -> TrackMatch {
    candidates.sort_by_key(|c| std::cmp::Reverse(score(source, c)));
    let confidence = match candidates.first() {
        None => Confidence::NotFound,
        Some(best) => {
            let isrc_exact = best.matched_by == MatchedBy::Isrc
                && source.duration_ms.abs_diff(best.duration_ms) <= DURATION_TIGHT_MS;
            let s = score(source, best);
            if isrc_exact {
                Confidence::Exact
            } else if s >= HIGH_THRESHOLD {
                Confidence::High
            } else if s >= AMBIGUOUS_THRESHOLD {
                Confidence::Ambiguous
            } else {
                Confidence::NotFound
            }
        }
    };
    TrackMatch { source: source.clone(), candidates, confidence }
}
```

Add to `rocola-core/src/lib.rs`:

```rust
pub mod matching;

pub use matching::{classify, score};
```

- [ ] **Step 4: Run to verify pass**

Run: `just test rocola-core && just lint rocola-core` — Expected: PASS.

- [ ] **Step 5: Checkpoint — Pedro commits**

Suggested: `feat(core): score candidates and classify matches with fixed thresholds`

---

## Task 4: rocola-core — golden fixture corpus

**Files:**
- Create: `rocola-core/tests/fixtures/corpus.json`
- Create: `rocola-core/tests/corpus.rs`

**Interfaces:**
- Consumes: `classify`, types. Nothing downstream consumes this; it is the regression net for matching quality. Contributors extend `corpus.json` without touching Rust.

- [ ] **Step 1: Write the corpus fixture**

`rocola-core/tests/fixtures/corpus.json` — an array of cases. Start with 12 spanning the spec's hard categories (remasters, live versions, features, explicit/clean, non-Latin scripts, wrong-artist same-title); grow toward ~50 during M6. Shape:

```json
[
  {
    "name": "plain exact match",
    "source": { "title": "Umbrella", "artists": ["Rihanna"], "album": "Good Girl Gone Bad", "duration_ms": 275000, "isrc": null },
    "candidates": [
      { "catalog_id": "1", "title": "Umbrella", "artists": ["Rihanna"], "album": "Good Girl Gone Bad", "duration_ms": 275000, "matched_by": "Search" }
    ],
    "expected": "High"
  },
  {
    "name": "isrc hit close duration",
    "source": { "title": "Vámonos", "artists": ["Chavela Vargas"], "album": "Colección", "duration_ms": 187000, "isrc": "MXF049800212" },
    "candidates": [
      { "catalog_id": "2", "title": "Vámonos", "artists": ["Chavela Vargas"], "album": "Lo Esencial", "duration_ms": 188500, "matched_by": "Isrc" }
    ],
    "expected": "Exact"
  }
]
```

Write all 12 cases concretely in this shape (the two above plus: remaster suffix, live version vs studio, feat. credit differences, explicit/clean same duration, non-Latin script (e.g. さくら), same-title different artist, ISRC hit with 30s duration gap (must NOT be Exact), empty candidates, two near-equal candidates (Ambiguous), duration-only disagreement).

- [ ] **Step 2: Write the corpus test**

`rocola-core/tests/corpus.rs`:

```rust
use rocola_core::{classify, Candidate, Confidence, SourceTrack};
use serde::Deserialize;

#[derive(Deserialize)]
struct Case {
    name: String,
    source: SourceTrack,
    candidates: Vec<Candidate>,
    expected: Confidence,
}

#[test]
fn golden_corpus() {
    let raw = include_str!("fixtures/corpus.json");
    let cases: Vec<Case> = serde_json::from_str(raw).expect("corpus.json parses");
    let mut failures = Vec::new();
    for case in cases {
        let got = classify(&case.source, case.candidates.clone()).confidence;
        if got != case.expected {
            failures.push(format!("{}: expected {:?}, got {got:?}", case.name, case.expected));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
```

- [ ] **Step 3: Run; fix scoring or expectations until green**

Run: `just test rocola-core` — Expected: PASS. If a case fails, decide deliberately: is the corpus expectation wrong, or the scorer? Adjust the one that is wrong; never blindly relabel.

- [ ] **Step 4: Checkpoint — Pedro commits**

Suggested: `test(core): golden corpus for match classification`

---

## Task 5: rocola-spotify — playlist URL parsing and API types

**Files:**
- Modify: `rocola-spotify/Cargo.toml`
- Create: `rocola-spotify/src/url.rs`
- Create: `rocola-spotify/src/api_types.rs`
- Create: `rocola-spotify/tests/fixtures/playlist_page.json`
- Modify: `rocola-spotify/src/lib.rs`

**Interfaces:**
- Produces: `parse_playlist_url(&str) -> Result<PlaylistRef, SpotifyError>` where `PlaylistRef(pub String)` is the bare Spotify playlist ID; `PlaylistPage` deserialising Spotify's `GET /v1/playlists/{id}/tracks` response with `items[].track.{name, duration_ms, album.name, artists[].name, external_ids.isrc}` and `next: Option<String>`; `PlaylistPage::source_tracks(&self) -> Vec<SourceTrack>` (skips null/local tracks); `SpotifyError` enum (thiserror): `BadUrl(String)`, `Http(String)`, `Auth(String)`, `RestrictedPlaylist`.

- [ ] **Step 1: Add dependencies**

`rocola-spotify/Cargo.toml` `[dependencies]` (keep the existing rocola-core line):

```toml
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
reqwest = { version = "0.13", default-features = false, features = ["rustls-tls", "json"] }
tokio = { version = "1", features = ["net", "io-util", "macros", "time"] }
sha2 = "0.11"
base64 = "0.23"
rand = "0.10"
open = "5"
```

- [ ] **Step 2: Write the failing URL tests**

Bottom of `rocola-spotify/src/url.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_web_url_with_query() {
        let r = parse_playlist_url("https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M?si=abc").unwrap();
        assert_eq!(r.0, "37i9dQZF1DXcBWIGoYBM5M");
    }

    #[test]
    fn parses_uri_form() {
        let r = parse_playlist_url("spotify:playlist:37i9dQZF1DXcBWIGoYBM5M").unwrap();
        assert_eq!(r.0, "37i9dQZF1DXcBWIGoYBM5M");
    }

    #[test]
    fn rejects_album_url_with_named_error() {
        let err = parse_playlist_url("https://open.spotify.com/album/xyz").unwrap_err();
        assert!(err.to_string().contains("playlist"), "error must tell the user it needs a playlist link");
    }
}
```

- [ ] **Step 3: Run to verify failure** — `just test rocola-spotify`, expected compile error.

- [ ] **Step 4: Implement URL parsing and errors**

Top of `rocola-spotify/src/url.rs`:

```rust
use crate::SpotifyError;

/// A bare Spotify playlist ID (base62).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistRef(pub String);

/// Accepts `https://open.spotify.com/playlist/<id>[?…]` and `spotify:playlist:<id>`.
pub fn parse_playlist_url(input: &str) -> Result<PlaylistRef, SpotifyError> {
    let input = input.trim();
    let id = input
        .strip_prefix("spotify:playlist:")
        .map(ToOwned::to_owned)
        .or_else(|| {
            let after = input.split("open.spotify.com/playlist/").nth(1)?;
            Some(after.split(['?', '/']).next().unwrap_or(after).to_owned())
        });
    match id {
        Some(id) if !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric()) => Ok(PlaylistRef(id)),
        _ => Err(SpotifyError::BadUrl(
            "That doesn't look like a Spotify playlist link. Paste a link like \
             https://open.spotify.com/playlist/… (from Share → Copy link to playlist)."
                .into(),
        )),
    }
}
```

`rocola-spotify/src/lib.rs`:

```rust
pub mod api_types;
pub mod url;

pub use url::{parse_playlist_url, PlaylistRef};

#[derive(Debug, thiserror::Error)]
pub enum SpotifyError {
    #[error("{0}")]
    BadUrl(String),
    #[error("Spotify request failed: {0}. Check your connection and try again.")]
    Http(String),
    #[error("Spotify sign-in problem: {0}")]
    Auth(String),
    #[error(
        "Spotify blocks apps like this one from reading Spotify-made playlists \
         (Discover Weekly, editorial playlists). Try a playlist made by a person."
    )]
    RestrictedPlaylist,
}
```

- [ ] **Step 5: Write the failing API-types test with a fixture**

`rocola-spotify/tests/fixtures/playlist_page.json` — a realistic one-page response, 3 items: one full track with ISRC, one with `"track": null` (removed track), one local file (`"is_local": true`, no ISRC):

```json
{
  "next": "https://api.spotify.com/v1/playlists/xyz/tracks?offset=100&limit=100",
  "items": [
    {
      "is_local": false,
      "track": {
        "name": "Umbrella",
        "duration_ms": 275000,
        "album": { "name": "Good Girl Gone Bad" },
        "artists": [{ "name": "Rihanna" }, { "name": "JAY-Z" }],
        "external_ids": { "isrc": "USUM70701234" },
        "is_local": false
      }
    },
    { "is_local": false, "track": null },
    {
      "is_local": true,
      "track": {
        "name": "Home Recording",
        "duration_ms": 100000,
        "album": { "name": "" },
        "artists": [{ "name": "Me" }],
        "external_ids": {},
        "is_local": true
      }
    }
  ]
}
```

`rocola-spotify/tests/api_types.rs`:

```rust
use rocola_spotify::api_types::PlaylistPage;

#[test]
fn maps_page_to_source_tracks_skipping_null_and_local() {
    let page: PlaylistPage =
        serde_json::from_str(include_str!("fixtures/playlist_page.json")).unwrap();
    let tracks = page.source_tracks();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].title, "Umbrella");
    assert_eq!(tracks[0].artists, vec!["Rihanna".to_string(), "JAY-Z".to_string()]);
    assert_eq!(tracks[0].isrc.as_deref(), Some("USUM70701234"));
    assert!(page.next.is_some());
}
```

- [ ] **Step 6: Run to verify failure** — `just test rocola-spotify`, expected compile error.

- [ ] **Step 7: Implement API types**

`rocola-spotify/src/api_types.rs`:

```rust
use rocola_core::SourceTrack;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct PlaylistPage {
    pub next: Option<String>,
    pub items: Vec<PageItem>,
}

#[derive(Debug, Deserialize)]
pub struct PageItem {
    pub track: Option<ApiTrack>,
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
    pub fn source_tracks(&self) -> Vec<SourceTrack> {
        self.items
            .iter()
            .filter_map(|i| i.track.as_ref())
            .filter(|t| !t.is_local)
            .map(|t| SourceTrack {
                title: t.name.clone(),
                artists: t.artists.iter().map(|a| a.name.clone()).collect(),
                album: t.album.name.clone(),
                duration_ms: t.duration_ms,
                isrc: t.external_ids.isrc.as_deref().and_then(rocola_core::normalize::normalize_isrc),
            })
            .collect()
    }
}
```

- [ ] **Step 8: Run to verify pass** — `just test rocola-spotify && just lint rocola-spotify`, expected PASS.

- [ ] **Step 9: Checkpoint — Pedro commits**

Suggested: `feat(spotify): parse playlist URLs and map API pages to source tracks`

---

## Task 6: rocola-spotify — PKCE helpers

**Files:**
- Create: `rocola-spotify/src/pkce.rs`
- Modify: `rocola-spotify/src/lib.rs`

**Interfaces:**
- Produces: `Pkce { verifier: String, challenge: String }`, `Pkce::new() -> Self` (random 64-char verifier from `[A-Za-z0-9\-._~]`, challenge = BASE64URL-nopad(SHA256(verifier))), `authorize_url(client_id: &str, pkce: &Pkce, state: &str) -> String` (scope `playlist-read-private playlist-read-collaborative`, redirect `http://127.0.0.1:8888/callback`).

- [ ] **Step 1: Write the failing tests**

Bottom of `rocola-spotify/src/pkce.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc7636_appendix_b_vector() {
        // Verifier and expected challenge from RFC 7636 Appendix B.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(challenge_for(verifier), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn verifier_is_64_unreserved_chars_and_random() {
        let a = Pkce::new();
        let b = Pkce::new();
        assert_eq!(a.verifier.len(), 64);
        assert!(a.verifier.chars().all(|c| c.is_ascii_alphanumeric() || "-._~".contains(c)));
        assert_ne!(a.verifier, b.verifier);
    }

    #[test]
    fn authorize_url_pins_loopback_redirect() {
        let url = authorize_url("myclientid", &Pkce::new(), "st4te");
        assert!(url.starts_with("https://accounts.spotify.com/authorize?"));
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A8888%2Fcallback"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state=st4te"));
    }
}
```

- [ ] **Step 2: Run to verify failure** — `just test rocola-spotify`, expected compile error.

- [ ] **Step 3: Implement**

Top of `rocola-spotify/src/pkce.rs`:

```rust
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::Rng as _;
use sha2::{Digest as _, Sha256};

pub const REDIRECT_URI: &str = "http://127.0.0.1:8888/callback";
const UNRESERVED: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";

#[derive(Debug, Clone)]
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    pub fn new() -> Self {
        let mut rng = rand::rng();
        let verifier: String = (0..64)
            .map(|_| UNRESERVED[rng.random_range(0..UNRESERVED.len())] as char)
            .collect();
        let challenge = challenge_for(&verifier);
        Self { verifier, challenge }
    }
}

impl Default for Pkce {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn challenge_for(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

pub fn authorize_url(client_id: &str, pkce: &Pkce, state: &str) -> String {
    let redirect = REDIRECT_URI.replace(':', "%3A").replace('/', "%2F");
    format!(
        "https://accounts.spotify.com/authorize?client_id={client_id}\
         &response_type=code&redirect_uri={redirect}\
         &code_challenge_method=S256&code_challenge={}\
         &state={state}&scope=playlist-read-private%20playlist-read-collaborative",
        pkce.challenge
    )
}
```

Add to `rocola-spotify/src/lib.rs`:

```rust
pub mod pkce;

pub use pkce::{authorize_url, Pkce, REDIRECT_URI};
```

- [ ] **Step 4: Run to verify pass** — `just test rocola-spotify && just lint rocola-spotify`, expected PASS (the RFC vector proves the S256 pipeline end to end).

- [ ] **Step 5: Checkpoint — Pedro commits**

Suggested: `feat(spotify): PKCE verifier/challenge and authorize URL`

---

## Task 7: rocola-spotify — auth flow and playlist fetch (client)

The one task whose runtime behaviour is verified live with hurl rather than unit tests: token exchange and playlist paging against real Spotify. Unit tests cover the pure parts (callback query parsing, token deserialisation); hurl covers the wire.

**Files:**
- Create: `rocola-spotify/src/auth.rs`
- Create: `rocola-spotify/src/client.rs`
- Create: `rocola-spotify/tests/fixtures/token_response.json`
- Create: `tests/hurl/spotify_refresh.hurl`
- Create: `tests/hurl/spotify_playlist.hurl`
- Create: `tests/hurl/vars.env.example`
- Modify: `rocola-spotify/src/lib.rs`, `.gitignore`, `justfile`

**Interfaces:**
- Consumes: `Pkce`, `authorize_url`, `REDIRECT_URI`, `PlaylistPage`, `PlaylistRef`, `SpotifyError`.
- Produces:
  - `TokenSet { access_token: String, refresh_token: Option<String>, expires_in: u64 }` (Deserialize).
  - `parse_callback_query(query: &str, expected_state: &str) -> Result<String, SpotifyError>` → authorization code.
  - `async fn run_auth_flow(client_id: &str) -> Result<TokenSet, SpotifyError>` — binds `127.0.0.1:8888` (named error if taken: "Port 8888 is in use. Close the other program using it and try again — Spotify only accepts this exact port."), opens browser via `open::that`, accepts one `GET /callback`, replies with a plain "You can close this tab and return to the terminal." page, exchanges code at `https://accounts.spotify.com/api/token` (form: grant_type=authorization_code, code, redirect_uri, client_id, code_verifier).
  - `async fn refresh(client_id: &str, refresh_token: &str) -> Result<TokenSet, SpotifyError>`.
  - `async fn fetch_playlist(access_token: &str, playlist: &PlaylistRef) -> Result<(String, Vec<SourceTrack>), SpotifyError>` — returns (playlist name, tracks); follows `next` pages; maps HTTP 404 on a playlist that exists in the app to `RestrictedPlaylist` (Spotify serves editorial/algorithmic playlists as 404 to new apps).
- Implementation notes: use raw `tokio::net::TcpListener` + minimal HTTP/1.1 parsing for the single callback request (axum is not needed for one GET; fewer deps in this crate). Read request line, extract path+query, respond, close.

- [ ] **Step 1: Failing unit tests for the pure parts**

Bottom of `rocola-spotify/src/auth.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_code_when_state_matches() {
        let code = parse_callback_query("code=AQD123&state=xyz", "xyz").unwrap();
        assert_eq!(code, "AQD123");
    }

    #[test]
    fn rejects_mismatched_state() {
        assert!(parse_callback_query("code=AQD123&state=evil", "xyz").is_err());
    }

    #[test]
    fn surfaces_spotify_denial_in_plain_english() {
        let err = parse_callback_query("error=access_denied&state=xyz", "xyz").unwrap_err();
        assert!(err.to_string().contains("didn't finish"), "got: {err}");
    }
}
```

`rocola-spotify/tests/fixtures/token_response.json`:

```json
{ "access_token": "BQabc", "token_type": "Bearer", "expires_in": 3600, "refresh_token": "AQdef", "scope": "playlist-read-private" }
```

`rocola-spotify/tests/token_types.rs`:

```rust
use rocola_spotify::auth::TokenSet;

#[test]
fn token_response_deserialises() {
    let t: TokenSet = serde_json::from_str(include_str!("fixtures/token_response.json")).unwrap();
    assert_eq!(t.access_token, "BQabc");
    assert_eq!(t.refresh_token.as_deref(), Some("AQdef"));
    assert_eq!(t.expires_in, 3600);
}
```

- [ ] **Step 2: Run to verify failure** — `just test rocola-spotify`, expected compile error.

- [ ] **Step 3: Implement `auth.rs`**

```rust
use serde::Deserialize;

use crate::pkce::{authorize_url, Pkce, REDIRECT_URI};
use crate::SpotifyError;

#[derive(Debug, Clone, Deserialize)]
pub struct TokenSet {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    pub expires_in: u64,
}

/// Parse the query string Spotify sends to the loopback redirect.
pub fn parse_callback_query(query: &str, expected_state: &str) -> Result<String, SpotifyError> {
    let mut code = None;
    let mut state = None;
    let mut error = None;
    for pair in query.split('&') {
        match pair.split_once('=') {
            Some(("code", v)) => code = Some(v.to_owned()),
            Some(("state", v)) => state = Some(v.to_owned()),
            Some(("error", v)) => error = Some(v.to_owned()),
            _ => {}
        }
    }
    if let Some(e) = error {
        return Err(SpotifyError::Auth(format!(
            "the sign-in didn't finish ({e}). Run rocola again to retry."
        )));
    }
    if state.as_deref() != Some(expected_state) {
        return Err(SpotifyError::Auth(
            "the sign-in reply didn't come from the request rocola made. Run rocola again to retry.".into(),
        ));
    }
    code.ok_or_else(|| SpotifyError::Auth("Spotify sent no code. Run rocola again to retry.".into()))
}

pub async fn run_auth_flow(client_id: &str) -> Result<TokenSet, SpotifyError> {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:8888").await.map_err(|_| {
        SpotifyError::Auth(
            "port 8888 is in use. Close the other program using it and try again — \
             Spotify only accepts this exact port."
                .into(),
        )
    })?;
    let pkce = Pkce::new();
    let state: String = {
        use rand::Rng as _;
        let mut rng = rand::rng();
        (0..16).map(|_| char::from(rng.random_range(b'a'..=b'z'))).collect()
    };
    let url = authorize_url(client_id, &pkce, &state);
    if open::that(&url).is_err() {
        eprintln!("Open this link in your browser to sign in to Spotify:\n{url}");
    }

    let (mut stream, _) = listener
        .accept()
        .await
        .map_err(|e| SpotifyError::Auth(format!("couldn't accept the browser's reply: {e}")))?;
    let mut buf = vec![0u8; 8192];
    let n = stream
        .read(&mut buf)
        .await
        .map_err(|e| SpotifyError::Auth(format!("couldn't read the browser's reply: {e}")))?;
    let request = String::from_utf8_lossy(&buf[..n]);
    let query = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|path| path.split_once('?'))
        .map(|(_, q)| q.to_owned())
        .unwrap_or_default();
    let result = parse_callback_query(&query, &state);
    let body = match &result {
        Ok(_) => "Signed in. You can close this tab and return to the terminal.",
        Err(_) => "Something went wrong. Return to the terminal for what to do next.",
    };
    let _ = stream
        .write_all(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .await;
    let code = result?;

    exchange(&[
        ("grant_type", "authorization_code"),
        ("code", &code),
        ("redirect_uri", REDIRECT_URI),
        ("client_id", client_id),
        ("code_verifier", &pkce.verifier),
    ])
    .await
}

pub async fn refresh(client_id: &str, refresh_token: &str) -> Result<TokenSet, SpotifyError> {
    exchange(&[
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", client_id),
    ])
    .await
}

async fn exchange(form: &[(&str, &str)]) -> Result<TokenSet, SpotifyError> {
    let response = reqwest::Client::new()
        .post("https://accounts.spotify.com/api/token")
        .form(form)
        .send()
        .await
        .map_err(|e| SpotifyError::Http(e.to_string()))?;
    if !response.status().is_success() {
        let status = response.status();
        return Err(SpotifyError::Auth(format!(
            "Spotify refused the sign-in ({status}). Check your client ID in ~/.config/rocola/config.toml."
        )));
    }
    response.json().await.map_err(|e| SpotifyError::Http(e.to_string()))
}
```

- [ ] **Step 4: Implement `client.rs`**

```rust
use rocola_core::SourceTrack;
use serde::Deserialize;

use crate::api_types::PlaylistPage;
use crate::url::PlaylistRef;
use crate::SpotifyError;

#[derive(Debug, Deserialize)]
struct PlaylistMeta {
    name: String,
}

/// Fetch playlist name and every track, following pagination.
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
                    "your Spotify sign-in has expired. rocola will sign you in again on the next run.".into(),
                )),
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
        "https://api.spotify.com/v1/playlists/{}/tracks?limit=100&fields=next,items(is_local,track(name,duration_ms,is_local,album(name),artists(name),external_ids))",
        playlist.0
    ));
    while let Some(url) = next {
        let page: PlaylistPage = get(url)
            .await?
            .json()
            .await
            .map_err(|e| SpotifyError::Http(e.to_string()))?;
        tracks.extend(page.source_tracks());
        next = page.next.clone();
    }
    Ok((meta.name, tracks))
}
```

Add to `rocola-spotify/src/lib.rs`:

```rust
pub mod auth;
pub mod client;

pub use auth::{refresh, run_auth_flow, TokenSet};
pub use client::fetch_playlist;
```

- [ ] **Step 5: Run unit tests** — `just test rocola-spotify && just lint rocola-spotify`, expected PASS.

- [ ] **Step 6: Write the hurl files and wiring**

Append to `.gitignore`:

```
tests/hurl/vars.env
```

`tests/hurl/vars.env.example` (committed; the real `vars.env` is git-ignored):

```
spotify_token=PASTE_ACCESS_TOKEN
spotify_refresh_token=PASTE_REFRESH_TOKEN
spotify_client_id=PASTE_CLIENT_ID
spotify_playlist_id=37i9dQZF1DXcBWIGoYBM5M
```

`tests/hurl/spotify_playlist.hurl`:

```hurl
# Live check: playlist fetch shape matches what rocola-spotify deserialises.
GET https://api.spotify.com/v1/playlists/{{spotify_playlist_id}}/tracks?limit=2&fields=next,items(is_local,track(name,duration_ms,is_local,album(name),artists(name),external_ids))
Authorization: Bearer {{spotify_token}}
HTTP 200
[Asserts]
jsonpath "$.items" count > 0
jsonpath "$.items[0].track.name" isString
jsonpath "$.items[0].track.duration_ms" isInteger
jsonpath "$.items[0].track.album.name" isString
jsonpath "$.items[0].track.artists[0].name" isString
```

`tests/hurl/spotify_refresh.hurl`:

```hurl
# Live check: PKCE refresh grant returns a usable token set.
POST https://accounts.spotify.com/api/token
[Form]
grant_type: refresh_token
refresh_token: {{spotify_refresh_token}}
client_id: {{spotify_client_id}}
HTTP 200
[Asserts]
jsonpath "$.access_token" isString
jsonpath "$.expires_in" isInteger
```

Append to `justfile`:

```make
# Live network checks (never run in CI). Copy tests/hurl/vars.env.example
# to tests/hurl/vars.env and fill it in first.
hurl-spotify:
    hurl --variables-file tests/hurl/vars.env --test tests/hurl/spotify_playlist.hurl tests/hurl/spotify_refresh.hurl

hurl-apple:
    hurl --variables-file tests/hurl/vars.env --test tests/hurl/apple_isrc.hurl tests/hurl/apple_search.hurl tests/hurl/apple_storefront.hurl
```

- [ ] **Step 7: Pedro runs `just hurl-spotify`** once he has credentials (agent stops here; needs his Spotify app). Expected: 2 tests PASS.

- [ ] **Step 8: Checkpoint — Pedro commits**

Suggested: `feat(spotify): PKCE auth flow and paginated playlist fetch, hurl live checks`

---

## Task 8: rocola-apple — developer token (ES256 JWT)

**Files:**
- Modify: `rocola-apple/Cargo.toml`
- Create: `rocola-apple/src/dev_token.rs`
- Create: `rocola-apple/tests/fixtures/test_key.p8` (test-only P-256 key, generated for this repo, never a real Apple key)
- Modify: `rocola-apple/src/lib.rs`

**Interfaces:**
- Produces: `mint_developer_token(p8_pem: &str, team_id: &str, key_id: &str) -> Result<String, AppleError>` — ES256 JWT, `iss`=team_id, `iat`=now, `exp`=now+12h, header `kid`=key_id. Minted in memory each run, never persisted (spec §Auth). `AppleError` enum (thiserror): `BadKey(String)`, `Http(String)`, `Auth(String)`, `NotInStorefront`.

- [ ] **Step 1: Generate the test-only key** (safe: it is a random key with no Apple registration)

Run: `openssl ecparam -genkey -name prime256v1 -noout | openssl pkcs8 -topk8 -nocrypt -out rocola-apple/tests/fixtures/test_key.p8`

Add a first line comment is NOT possible in PEM — instead create `rocola-apple/tests/fixtures/README.md`:

```markdown
`test_key.p8` is a randomly generated P-256 key used only to test JWT minting.
It is not an Apple key and grants access to nothing.
```

- [ ] **Step 2: Add dependencies**

`rocola-apple/Cargo.toml` `[dependencies]` (keep rocola-core):

```toml
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
reqwest = { version = "0.13", default-features = false, features = ["rustls-tls", "json"] }
tokio = { version = "1", features = ["macros", "time"] }
jsonwebtoken = "11"
axum = "0.8"
open = "5"
```

`[dev-dependencies]`:

```toml
base64 = "0.23"
```

- [ ] **Step 3: Write the failing test**

Bottom of `rocola-apple/src/dev_token.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    #[test]
    fn mints_a_jwt_with_kid_iss_and_12h_expiry() {
        let pem = include_str!("../tests/fixtures/test_key.p8");
        let token = mint_developer_token(pem, "TEAM123456", "KEY1234567").unwrap();
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);
        let header: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).unwrap()).unwrap();
        assert_eq!(header["alg"], "ES256");
        assert_eq!(header["kid"], "KEY1234567");
        let claims: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
        assert_eq!(claims["iss"], "TEAM123456");
        let lifetime = claims["exp"].as_u64().unwrap() - claims["iat"].as_u64().unwrap();
        assert_eq!(lifetime, 12 * 60 * 60);
    }

    #[test]
    fn bad_pem_reports_the_fix() {
        let err = mint_developer_token("not a key", "T", "K").unwrap_err();
        assert!(err.to_string().contains(".p8"), "error must mention the .p8 file: {err}");
    }
}
```

- [ ] **Step 4: Run to verify failure** — `just test rocola-apple`, expected compile error.

- [ ] **Step 5: Implement**

Top of `rocola-apple/src/dev_token.rs`:

```rust
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::Serialize;

use crate::AppleError;

#[derive(Serialize)]
struct Claims {
    iss: String,
    iat: u64,
    exp: u64,
}

/// Apple developer token: ES256 JWT signed with the user's MusicKit .p8 key.
/// Short-lived and in-memory only — 12 hours covers any session.
pub fn mint_developer_token(p8_pem: &str, team_id: &str, key_id: &str) -> Result<String, AppleError> {
    let key = EncodingKey::from_ec_pem(p8_pem.as_bytes()).map_err(|_| {
        AppleError::BadKey(
            "couldn't read your Apple key. Check that apple.p8_path in \
             ~/.config/rocola/config.toml points at the AuthKey_….p8 file you \
             downloaded from Apple."
                .into(),
        )
    })?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before 1970")
        .as_secs();
    let mut header = Header::new(Algorithm::ES256);
    header.kid = Some(key_id.to_owned());
    let claims = Claims { iss: team_id.to_owned(), iat: now, exp: now + 12 * 60 * 60 };
    encode(&header, &claims, &key).map_err(|e| AppleError::BadKey(format!("couldn't sign the Apple token: {e}")))
}
```

`rocola-apple/src/lib.rs`:

```rust
pub mod dev_token;

pub use dev_token::mint_developer_token;

#[derive(Debug, thiserror::Error)]
pub enum AppleError {
    #[error("{0}")]
    BadKey(String),
    #[error("Apple Music request failed: {0}. Check your connection and try again.")]
    Http(String),
    #[error("Apple Music sign-in problem: {0}")]
    Auth(String),
    #[error("this song isn't available in your country's Apple Music catalog")]
    NotInStorefront,
}
```

- [ ] **Step 6: Run to verify pass** — `just test rocola-apple && just lint rocola-apple`, expected PASS.

- [ ] **Step 7: Checkpoint — Pedro commits**

Suggested: `feat(apple): mint ES256 developer token from a MusicKit .p8 key`

---

## Task 9: rocola-apple — Music User Token via local MusicKit page

**Files:**
- Create: `rocola-apple/src/user_token.rs`
- Create: `rocola-apple/src/musickit.html`
- Modify: `rocola-apple/src/lib.rs`

**Interfaces:**
- Consumes: `mint_developer_token` output (passed in as `&str`).
- Produces: `async fn run_user_auth(developer_token: &str) -> Result<String, AppleError>` — serves `musickit.html` (with the dev token substituted) on `127.0.0.1:8889` via axum, opens the browser, waits for the page to POST `/token` with JSON `{"userToken": "..."}`, returns the token. 5-minute timeout with a named error.
- Depends on Task 0's PASS. If Task 0 failed, this task's design is invalid — stop and revisit.

- [ ] **Step 1: Write the page**

`rocola-apple/src/musickit.html` (embedded via `include_str!`; `__DEV_TOKEN__` substituted at serve time):

```html
<!doctype html>
<meta charset="utf-8">
<title>rocola — connect Apple Music</title>
<script src="https://js-cdn.music.apple.com/musickit/v3/musickit.js" async></script>
<body style="font-family: system-ui; max-width: 34rem; margin: 4rem auto;">
<h1>Connect rocola to Apple Music</h1>
<p id="msg">Loading Apple's sign-in…</p>
<button id="go" hidden>Sign in to Apple Music</button>
<script>
const msg = (t) => document.getElementById('msg').textContent = t;
document.addEventListener('musickitloaded', async () => {
  try {
    await MusicKit.configure({ developerToken: '__DEV_TOKEN__', app: { name: 'rocola', build: '1' } });
    const btn = document.getElementById('go');
    btn.hidden = false;
    msg('Click the button to sign in. rocola only gets permission to create playlists for you.');
    btn.onclick = async () => {
      try {
        const userToken = await MusicKit.getInstance().authorize();
        await fetch('/token', {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify({ userToken }),
        });
        msg('Connected. You can close this tab and return to the terminal.');
        btn.hidden = true;
      } catch (e) { msg('Sign-in failed: ' + e + '. Close this tab and run rocola again.'); }
    };
  } catch (e) { msg('Apple rejected the app token: ' + e + '. Check team_id and key_id in ~/.config/rocola/config.toml.'); }
});
</script>
```

- [ ] **Step 2: Write the failing unit test** (pure part: token substitution)

Bottom of `rocola-apple/src/user_token.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_embeds_the_developer_token_and_posts_back() {
        let page = render_page("DEVTOKEN123");
        assert!(page.contains("developerToken: 'DEVTOKEN123'"));
        assert!(!page.contains("__DEV_TOKEN__"));
        assert!(page.contains("fetch('/token'"));
    }
}
```

- [ ] **Step 3: Run to verify failure** — `just test rocola-apple`, expected compile error.

- [ ] **Step 4: Implement**

Top of `rocola-apple/src/user_token.rs`:

```rust
use std::sync::Arc;

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use tokio::sync::mpsc;

use crate::AppleError;

fn render_page(developer_token: &str) -> String {
    include_str!("musickit.html").replace("__DEV_TOKEN__", developer_token)
}

#[derive(Deserialize)]
struct TokenPost {
    #[serde(rename = "userToken")]
    user_token: String,
}

/// Serve the MusicKit page on 127.0.0.1:8889, open the browser, and wait
/// (max 5 minutes) for the page to post back the Music User Token.
pub async fn run_user_auth(developer_token: &str) -> Result<String, AppleError> {
    let (tx, mut rx) = mpsc::channel::<String>(1);
    let page = Arc::new(render_page(developer_token));

    let app = Router::new()
        .route(
            "/",
            get({
                let page = Arc::clone(&page);
                move || async move { axum::response::Html(page.as_ref().clone()) }
            }),
        )
        .route(
            "/token",
            post(|State(tx): State<mpsc::Sender<String>>, Json(body): Json<TokenPost>| async move {
                let _ = tx.send(body.user_token).await;
                "ok"
            }),
        )
        .with_state(tx);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:8889").await.map_err(|_| {
        AppleError::Auth("port 8889 is in use. Close the other program using it and try again.".into())
    })?;
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    if open::that("http://127.0.0.1:8889/").is_err() {
        eprintln!("Open this link in your browser to connect Apple Music:\nhttp://127.0.0.1:8889/");
    }

    let token = tokio::time::timeout(std::time::Duration::from_secs(300), rx.recv())
        .await
        .map_err(|_| {
            AppleError::Auth(
                "the Apple Music sign-in didn't finish within 5 minutes. Run rocola again to retry.".into(),
            )
        })?
        .ok_or_else(|| AppleError::Auth("the sign-in page closed early. Run rocola again to retry.".into()))?;
    server.abort();
    Ok(token)
}
```

Add to `rocola-apple/src/lib.rs`:

```rust
pub mod user_token;

pub use user_token::run_user_auth;
```

- [ ] **Step 5: Run to verify pass** — `just test rocola-apple && just lint rocola-apple`, expected PASS. (The axum route-state wiring compiles or it doesn't — the type system is the test for the server half; the live flow is verified end-to-end in Task 12.)

- [ ] **Step 6: Checkpoint — Pedro commits**

Suggested: `feat(apple): obtain Music User Token via local MusicKit page`

---

## Task 10: rocola-apple — catalog resolve (ISRC batch + search fallback) and playlist write

**Files:**
- Create: `rocola-apple/src/api_types.rs`
- Create: `rocola-apple/src/client.rs`
- Create: `rocola-apple/tests/fixtures/isrc_response.json`
- Create: `rocola-apple/tests/fixtures/search_response.json`
- Create: `tests/hurl/apple_storefront.hurl`, `tests/hurl/apple_isrc.hurl`, `tests/hurl/apple_search.hurl`
- Modify: `rocola-apple/src/lib.rs`, `tests/hurl/vars.env.example`

**Interfaces:**
- Consumes: `SourceTrack`, `Candidate`, `MatchedBy` from rocola-core; `AppleError`.
- Produces `AppleClient`:
  - `AppleClient::new(developer_token: String, user_token: String) -> Self` (owns a reqwest client; sends `Authorization: Bearer <dev>` + `Music-User-Token: <user>` on every request).
  - `async fn storefront(&self) -> Result<String, AppleError>` — `GET /v1/me/storefront`, returns e.g. `"gb"`.
  - `async fn resolve_by_isrc(&self, storefront: &str, isrcs: &[String]) -> Result<Vec<(String, Candidate)>, AppleError>` — batches ≤25 per request to `GET /v1/catalog/{sf}/songs?filter[isrc]=a,b,…`; returns (isrc, candidate) pairs, `matched_by: Isrc`.
  - `async fn search(&self, storefront: &str, track: &SourceTrack) -> Result<Vec<Candidate>, AppleError>` — `GET /v1/catalog/{sf}/search?types=songs&limit=5&term=<title artists urlencoded>`; `matched_by: Search`.
  - `async fn create_playlist(&self, name: &str, description: &str) -> Result<String, AppleError>` — `POST /v1/me/library/playlists`, body `{"attributes": {"name": name, "description": description}}`, returns library playlist id.
  - `async fn add_tracks(&self, playlist_id: &str, catalog_ids: &[String]) -> Result<(), AppleError>` — `POST /v1/me/library/playlists/{id}/tracks`, body `{"data": [{"id": "...", "type": "songs"}, …]}` (spec-verified: `songs` = catalog ID accepted directly).
  - All requests: on 429, read `Retry-After` (default 3s), sleep, retry once; on second 429 return `Http("Apple Music is rate-limiting; wait a minute and re-run")`.

- [ ] **Step 1: Write fixtures**

`rocola-apple/tests/fixtures/isrc_response.json` (shape of `GET /catalog/{sf}/songs?filter[isrc]=`):

```json
{
  "data": [
    {
      "id": "900032829",
      "type": "songs",
      "attributes": {
        "name": "Umbrella (feat. JAY-Z)",
        "artistName": "Rihanna",
        "albumName": "Good Girl Gone Bad",
        "durationInMillis": 275986,
        "isrc": "USUM70701234"
      }
    }
  ]
}
```

`rocola-apple/tests/fixtures/search_response.json` (shape of `GET /catalog/{sf}/search?types=songs`):

```json
{
  "results": {
    "songs": {
      "data": [
        {
          "id": "1440783625",
          "type": "songs",
          "attributes": {
            "name": "Wish You Were Here",
            "artistName": "Pink Floyd",
            "albumName": "Wish You Were Here",
            "durationInMillis": 334743,
            "isrc": "GBN9Y1100088"
          }
        }
      ]
    }
  }
}
```

- [ ] **Step 2: Write the failing deserialisation/mapping tests**

`rocola-apple/tests/api_types.rs`:

```rust
use rocola_apple::api_types::{SearchResponse, SongsResponse};
use rocola_core::MatchedBy;

#[test]
fn isrc_response_maps_to_candidates_keyed_by_isrc() {
    let r: SongsResponse = serde_json::from_str(include_str!("fixtures/isrc_response.json")).unwrap();
    let pairs = r.into_isrc_candidates();
    assert_eq!(pairs.len(), 1);
    let (isrc, c) = &pairs[0];
    assert_eq!(isrc, "USUM70701234");
    assert_eq!(c.catalog_id, "900032829");
    assert_eq!(c.artists, vec!["Rihanna".to_string()]);
    assert_eq!(c.duration_ms, 275_986);
    assert_eq!(c.matched_by, MatchedBy::Isrc);
}

#[test]
fn search_response_maps_to_search_candidates() {
    let r: SearchResponse = serde_json::from_str(include_str!("fixtures/search_response.json")).unwrap();
    let cs = r.into_candidates();
    assert_eq!(cs.len(), 1);
    assert_eq!(cs[0].catalog_id, "1440783625");
    assert_eq!(cs[0].matched_by, MatchedBy::Search);
}
```

- [ ] **Step 3: Run to verify failure** — `just test rocola-apple`, expected compile error.

- [ ] **Step 4: Implement `api_types.rs`**

```rust
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
    pub fn into_candidates(self) -> Vec<Candidate> {
        self.results
            .songs
            .map(|s| s.data.into_iter().map(|song| to_candidate(song, MatchedBy::Search)).collect())
            .unwrap_or_default()
    }
}
```

- [ ] **Step 5: Run mapping tests** — `just test rocola-apple`, expected PASS.

- [ ] **Step 6: Implement `client.rs`**

```rust
use rocola_core::{Candidate, SourceTrack};
use serde_json::json;

use crate::api_types::{SearchResponse, SongsResponse};
use crate::AppleError;

const BASE: &str = "https://api.music.apple.com";
pub const ISRC_BATCH: usize = 25;

pub struct AppleClient {
    http: reqwest::Client,
    developer_token: String,
    user_token: String,
}

impl AppleClient {
    #[must_use]
    pub fn new(developer_token: String, user_token: String) -> Self {
        Self { http: reqwest::Client::new(), developer_token, user_token }
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{BASE}{path}"))
            .bearer_auth(&self.developer_token)
            .header("Music-User-Token", &self.user_token)
    }

    async fn send(&self, build: impl Fn() -> reqwest::RequestBuilder) -> Result<reqwest::Response, AppleError> {
        for attempt in 0..2 {
            let response = build().send().await.map_err(|e| AppleError::Http(e.to_string()))?;
            match response.status().as_u16() {
                429 if attempt == 0 => {
                    let wait = response
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok()?.parse().ok())
                        .unwrap_or(3u64);
                    tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
                }
                429 => {
                    return Err(AppleError::Http(
                        "Apple Music is rate-limiting; wait a minute and re-run".into(),
                    ))
                }
                401 | 403 => {
                    return Err(AppleError::Auth(
                        "Apple Music rejected the sign-in. Run rocola again to reconnect.".into(),
                    ))
                }
                s if response.status().is_success() => {
                    let _ = s;
                    return Ok(response);
                }
                s => return Err(AppleError::Http(format!("Apple Music answered {s}"))),
            }
        }
        unreachable!("loop returns on every branch by attempt 1")
    }

    pub async fn storefront(&self) -> Result<String, AppleError> {
        #[derive(serde::Deserialize)]
        struct R {
            data: Vec<D>,
        }
        #[derive(serde::Deserialize)]
        struct D {
            id: String,
        }
        let r: R = self
            .send(|| self.request(reqwest::Method::GET, "/v1/me/storefront"))
            .await?
            .json()
            .await
            .map_err(|e| AppleError::Http(e.to_string()))?;
        r.data.into_iter().next().map(|d| d.id).ok_or(AppleError::NotInStorefront)
    }

    pub async fn resolve_by_isrc(
        &self,
        storefront: &str,
        isrcs: &[String],
    ) -> Result<Vec<(String, Candidate)>, AppleError> {
        let mut out = Vec::new();
        for chunk in isrcs.chunks(ISRC_BATCH) {
            let path = format!("/v1/catalog/{storefront}/songs?filter[isrc]={}", chunk.join(","));
            let r: SongsResponse = self
                .send(|| self.request(reqwest::Method::GET, &path))
                .await?
                .json()
                .await
                .map_err(|e| AppleError::Http(e.to_string()))?;
            out.extend(r.into_isrc_candidates());
        }
        Ok(out)
    }

    pub async fn search(&self, storefront: &str, track: &SourceTrack) -> Result<Vec<Candidate>, AppleError> {
        let term = format!("{} {}", track.title, track.artists.join(" "));
        let encoded: String = url_encode(&term);
        let path = format!("/v1/catalog/{storefront}/search?types=songs&limit=5&term={encoded}");
        let r: SearchResponse = self
            .send(|| self.request(reqwest::Method::GET, &path))
            .await?
            .json()
            .await
            .map_err(|e| AppleError::Http(e.to_string()))?;
        Ok(r.into_candidates())
    }

    pub async fn create_playlist(&self, name: &str, description: &str) -> Result<String, AppleError> {
        #[derive(serde::Deserialize)]
        struct R {
            data: Vec<D>,
        }
        #[derive(serde::Deserialize)]
        struct D {
            id: String,
        }
        let body = json!({ "attributes": { "name": name, "description": description } });
        let r: R = self
            .send(|| self.request(reqwest::Method::POST, "/v1/me/library/playlists").json(&body))
            .await?
            .json()
            .await
            .map_err(|e| AppleError::Http(e.to_string()))?;
        r.data
            .into_iter()
            .next()
            .map(|d| d.id)
            .ok_or_else(|| AppleError::Http("Apple created the playlist but sent no id".into()))
    }

    pub async fn add_tracks(&self, playlist_id: &str, catalog_ids: &[String]) -> Result<(), AppleError> {
        let body = json!({
            "data": catalog_ids.iter().map(|id| json!({ "id": id, "type": "songs" })).collect::<Vec<_>>()
        });
        let path = format!("/v1/me/library/playlists/{playlist_id}/tracks");
        self.send(|| self.request(reqwest::Method::POST, &path).json(&body)).await?;
        Ok(())
    }
}

/// Minimal percent-encoding for a query value (space and reserved chars).
fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                char::from(b).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::url_encode;

    #[test]
    fn encodes_spaces_and_unicode() {
        assert_eq!(url_encode("a b"), "a%20b");
        assert_eq!(url_encode("café"), "caf%C3%A9");
    }
}
```

Add to `rocola-apple/src/lib.rs`:

```rust
pub mod api_types;
pub mod client;

pub use client::AppleClient;
```

- [ ] **Step 7: Run to verify pass** — `just test rocola-apple && just lint rocola-apple`, expected PASS.

- [ ] **Step 8: Write the hurl live checks** (read-only ones; playlist creation is exercised end-to-end in Task 12, not hurl — it mutates the library)

Append to `tests/hurl/vars.env.example`:

```
apple_dev_token=PASTE_DEVELOPER_JWT
apple_user_token=PASTE_MUSIC_USER_TOKEN
apple_storefront=gb
```

`tests/hurl/apple_storefront.hurl`:

```hurl
GET https://api.music.apple.com/v1/me/storefront
Authorization: Bearer {{apple_dev_token}}
Music-User-Token: {{apple_user_token}}
HTTP 200
[Asserts]
jsonpath "$.data[0].id" isString
```

`tests/hurl/apple_isrc.hurl`:

```hurl
# Umbrella (Rihanna) — a stable, globally released recording.
GET https://api.music.apple.com/v1/catalog/{{apple_storefront}}/songs?filter[isrc]=USUM70701234
Authorization: Bearer {{apple_dev_token}}
HTTP 200
[Asserts]
jsonpath "$.data" count > 0
jsonpath "$.data[0].type" == "songs"
jsonpath "$.data[0].attributes.durationInMillis" isInteger
```

`tests/hurl/apple_search.hurl`:

```hurl
GET https://api.music.apple.com/v1/catalog/{{apple_storefront}}/search?types=songs&limit=5&term=wish%20you%20were%20here%20pink%20floyd
Authorization: Bearer {{apple_dev_token}}
HTTP 200
[Asserts]
jsonpath "$.results.songs.data" count > 0
jsonpath "$.results.songs.data[0].attributes.artistName" isString
```

- [ ] **Step 9: Pedro runs `just hurl-apple`** (needs his dev + user tokens). Expected: 3 tests PASS.

- [ ] **Step 10: Checkpoint — Pedro commits**

Suggested: `feat(apple): catalog resolve by ISRC and search, playlist create/add`

---

## Task 11: rocola-core — MusicTarget trait and matching pipeline

The seam from the spec: the pipeline is generic over the target service, so the whole flow is unit-tested with a fake target — no credentials, no network.

**Files:**
- Create: `rocola-core/src/pipeline.rs`
- Modify: `rocola-core/src/lib.rs`, `rocola-core/Cargo.toml` (dev-dep tokio for async tests only)

**Interfaces:**
- Consumes: `classify`, types.
- Produces:

```rust
pub trait MusicTarget {
    type Error;
    async fn resolve_by_isrc(&self, isrcs: &[String]) -> Result<Vec<(String, Candidate)>, Self::Error>;
    async fn search(&self, track: &SourceTrack) -> Result<Vec<Candidate>, Self::Error>;
}

pub async fn match_tracks<T: MusicTarget>(target: &T, tracks: &[SourceTrack])
    -> Result<Vec<TrackMatch>, T::Error>;
```

(Deliberate deviation from the spec's trait sketch: the spec put `create_playlist`/`add_tracks` on `MusicTarget` too. Only the resolve side needs the seam — it is what the tested pipeline calls; the two write methods are called once each by `run.rs` and gain nothing from indirection today. A future AppleScript backend adds them to the trait when a second implementation actually exists — YAGNI. Storefront is the Apple client's own state, set at construction — the trait stays service-neutral. `rocola-apple` implements this trait in Task 14 with a thin wrapper struct holding client + storefront.)

- Behaviour: one ISRC batch pass for every track that has an ISRC; each ISRC hit becomes that track's candidate list; every track with no ISRC hit falls back to `search`; every track is classified; output order matches input order; nothing is dropped.

- [ ] **Step 1: Write the failing test with a fake target**

Bottom of `rocola-core/src/pipeline.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Candidate, Confidence, MatchedBy, SourceTrack};

    struct FakeTarget;

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
```

Add to `rocola-core/Cargo.toml`:

```toml
[dev-dependencies]
serde_json = "1"
tokio = { version = "1", features = ["macros", "rt"] }
```

(`serde_json` line already exists from Task 1 — keep one copy.)

- [ ] **Step 2: Run to verify failure** — `just test rocola-core`, expected compile error.

- [ ] **Step 3: Implement**

Top of `rocola-core/src/pipeline.rs`:

```rust
use std::collections::HashMap;

use crate::matching::classify;
use crate::types::{Candidate, SourceTrack, TrackMatch};

/// The write-side seam (spec §The seam that matters). Implemented by
/// rocola-apple; implemented by fakes in tests. Native async fn in trait —
/// used generically, never as `dyn`.
pub trait MusicTarget {
    type Error;

    async fn resolve_by_isrc(&self, isrcs: &[String]) -> Result<Vec<(String, Candidate)>, Self::Error>;
    async fn search(&self, track: &SourceTrack) -> Result<Vec<Candidate>, Self::Error>;
}

/// Tier 1: one batched ISRC pass. Tier 2: per-track search for the rest.
/// Every input track appears in the output, classified, in input order.
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
        let isrc_hits = track.isrc.as_ref().and_then(|i| by_isrc.get(i)).cloned().unwrap_or_default();
        let candidates = if isrc_hits.is_empty() {
            target.search(track).await?
        } else {
            isrc_hits
        };
        out.push(classify(track, candidates));
    }
    Ok(out)
}
```

Add to `rocola-core/src/lib.rs`:

```rust
pub mod pipeline;

pub use pipeline::{match_tracks, MusicTarget};
```

- [ ] **Step 4: Run to verify pass** — `just test rocola-core && just lint rocola-core`, expected PASS.

- [ ] **Step 5: Checkpoint — Pedro commits**

Suggested: `feat(core): MusicTarget seam and two-tier matching pipeline`

---

## Task 12: rocola binary — config file (0600)

**Files:**
- Modify: `rocola/Cargo.toml`
- Create: `rocola/src/config.rs`
- Modify: `rocola/src/main.rs` (add `mod config;` only for now)

**Interfaces:**
- Produces:

```rust
pub struct Config {
    pub spotify: SpotifyConfig,   // { client_id: String, refresh_token: Option<String> }
    pub apple: AppleConfig,       // { team_id: String, key_id: String, p8_path: PathBuf,
                                  //   music_user_token: Option<String>, storefront: Option<String> }
}
impl Config {
    pub fn load(path: &Path) -> Result<Option<Self>, ConfigError>;  // None = first run
    pub fn save(&self, path: &Path) -> Result<(), ConfigError>;     // 0600 at open time
    pub fn default_path() -> PathBuf;                                // ~/.config/rocola/config.toml
    pub fn p8_inside_git_worktree(&self) -> bool;                    // startup warning (spec §Storage)
}
```

- [ ] **Step 1: Add dependencies**

`rocola/Cargo.toml` `[dependencies]` (keep the three rocola-* lines):

```toml
serde = { version = "1", features = ["derive"] }
thiserror = "2"
toml = "1"
dirs = "6"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
ratatui = "0.30"
crossterm = "0.29"
```

`[dev-dependencies]`:

```toml
tempfile = "3"
```

- [ ] **Step 2: Write the failing tests**

Bottom of `rocola/src/config.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Config {
        Config {
            spotify: SpotifyConfig { client_id: "cid".into(), refresh_token: Some("rt".into()) },
            apple: AppleConfig {
                team_id: "TEAM".into(),
                key_id: "KEY".into(),
                p8_path: "/tmp/AuthKey_X.p8".into(),
                music_user_token: None,
                storefront: Some("gb".into()),
            },
        }
    }

    #[test]
    fn roundtrips_and_is_owner_only() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        sample().save(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "config must be owner-read/write only");
        let loaded = Config::load(&path).unwrap().expect("config exists");
        assert_eq!(loaded.spotify.client_id, "cid");
        assert_eq!(loaded.apple.storefront.as_deref(), Some("gb"));
    }

    #[test]
    fn missing_file_is_first_run_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Config::load(&dir.path().join("nope.toml")).unwrap().is_none());
    }

    #[test]
    fn corrupt_file_names_the_file_in_the_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "not [valid toml").unwrap();
        let err = Config::load(&path).unwrap_err();
        assert!(err.to_string().contains("config.toml"), "got: {err}");
    }
}
```

- [ ] **Step 3: Run to verify failure** — `just test rocola`, expected compile error.

- [ ] **Step 4: Implement**

Top of `rocola/src/config.rs`:

```rust
use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub spotify: SpotifyConfig,
    pub apple: AppleConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotifyConfig {
    pub client_id: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppleConfig {
    pub team_id: String,
    pub key_id: String,
    pub p8_path: PathBuf,
    #[serde(default)]
    pub music_user_token: Option<String>,
    #[serde(default)]
    pub storefront: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("couldn't read {path}: {source}. Fix or delete the file and run rocola again.")]
    Unreadable { path: String, source: std::io::Error },
    #[error("{path} isn't valid config: {source}. Fix or delete the file and run rocola again.")]
    Invalid { path: String, source: Box<toml::de::Error> },
    #[error("couldn't write {path}: {source}")]
    Unwritable { path: String, source: std::io::Error },
}

impl Config {
    pub fn default_path() -> PathBuf {
        dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("rocola/config.toml")
    }

    /// `Ok(None)` means first run — the caller starts setup, not an error path.
    pub fn load(path: &Path) -> Result<Option<Self>, ConfigError> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(ConfigError::Unreadable { path: path.display().to_string(), source: e }),
        };
        toml::from_str(&text)
            .map(Some)
            .map_err(|e| ConfigError::Invalid { path: path.display().to_string(), source: Box::new(e) })
    }

    /// Written 0600 at open time — never chmod-ed after the bytes land.
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        use std::os::unix::fs::OpenOptionsExt as _;
        let wrap = |source| ConfigError::Unwritable { path: path.display().to_string(), source };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(wrap)?;
        }
        let text = toml::to_string_pretty(self).expect("config serialises");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .map_err(wrap)?;
        file.write_all(text.as_bytes()).map_err(wrap)
    }

    /// Spec §Storage: warn when the .p8 sits inside a git working tree.
    pub fn p8_inside_git_worktree(&self) -> bool {
        self.apple
            .p8_path
            .ancestors()
            .any(|dir| dir.join(".git").exists())
    }
}
```

In `rocola/src/main.rs`, replace the scaffold contents with:

```rust
mod config;

fn main() {
    println!("rocola: TUI arrives in the next task");
}
```

- [ ] **Step 5: Run to verify pass** — `just test rocola && just lint rocola`, expected PASS.

- [ ] **Step 6: Checkpoint — Pedro commits**

Suggested: `feat(cli): config file with owner-only permissions and first-run detection`

---

## Task 13: rocola binary — TUI state machine (pure, fully unit-tested)

The review screen is the product (spec §The TUI). Model it as a pure reducer — `Screen` + `on_key` — so every interaction rule is a unit test. Rendering (Task 14) reads this state and draws; it contains no decisions.

**Files:**
- Create: `rocola/src/app.rs`
- Modify: `rocola/src/main.rs` (add `mod app;`)

**Interfaces:**
- Consumes: `TrackMatch`, `Confidence` from rocola-core.
- Produces:

```rust
pub enum Decision { Accepted(usize), Skipped, Pending }   // index into candidates
pub struct ReviewItem { pub track: TrackMatch, pub decision: Decision }
pub enum Screen {
    Review { items: Vec<ReviewItem>, cursor: usize },     // only Ambiguous tracks land here
    Confirm { playlist_name: String, accepted: usize, skipped: usize, not_found: usize },
    Done,
    Aborted,
}
pub struct App { pub screen: Screen, pub playlist_name: String,
                 pub auto_accepted: Vec<TrackMatch>, pub not_found: Vec<TrackMatch> }
impl App {
    pub fn from_matches(playlist_name: String, matches: Vec<TrackMatch>) -> Self;
    pub fn on_key(&mut self, key: Key);                    // the reducer
    pub fn accepted_catalog_ids(&self) -> Vec<String>;     // what Confirm will write
}
pub enum Key { Up, Down, Digit(u8), Skip, AcceptAllHigh, Confirm, Abort }
```

- Rules encoded (and tested): `from_matches` sends Exact/High to `auto_accepted`, NotFound to `not_found`, Ambiguous to Review (straight to Confirm when none are Ambiguous). In Review: Up/Down move the cursor; Digit(n) accepts candidate n-1 for the cursor row and advances; Skip marks Skipped and advances; when every row is decided, Confirm key moves to Confirm screen; Abort → Aborted from anywhere. On Confirm screen, Confirm → Done (the caller performs the write — the state machine never does I/O). Nothing is ever silently dropped: accepted + skipped + not_found always sums to the input count.

- [ ] **Step 1: Write the failing tests**

Bottom of `rocola/src/app.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rocola_core::{Candidate, Confidence, MatchedBy, SourceTrack, TrackMatch};

    fn tm(title: &str, confidence: Confidence, n_candidates: usize) -> TrackMatch {
        let source = SourceTrack {
            title: title.into(),
            artists: vec!["A".into()],
            album: "L".into(),
            duration_ms: 200_000,
            isrc: None,
        };
        let candidates = (0..n_candidates)
            .map(|i| Candidate {
                catalog_id: format!("cat{i}"),
                title: title.into(),
                artists: vec!["A".into()],
                album: "L".into(),
                duration_ms: 200_000,
                matched_by: MatchedBy::Search,
            })
            .collect();
        TrackMatch { source, candidates, confidence }
    }

    #[test]
    fn triage_splits_by_confidence_and_counts_everything() {
        let app = App::from_matches(
            "Mix".into(),
            vec![
                tm("a", Confidence::Exact, 1),
                tm("b", Confidence::High, 1),
                tm("c", Confidence::Ambiguous, 3),
                tm("d", Confidence::NotFound, 0),
            ],
        );
        assert_eq!(app.auto_accepted.len(), 2);
        assert_eq!(app.not_found.len(), 1);
        let Screen::Review { items, cursor } = &app.screen else {
            panic!("ambiguous tracks must open the review screen")
        };
        assert_eq!(items.len(), 1);
        assert_eq!(*cursor, 0);
    }

    #[test]
    fn no_ambiguous_tracks_skips_review_entirely() {
        let app = App::from_matches("Mix".into(), vec![tm("a", Confidence::Exact, 1)]);
        assert!(matches!(app.screen, Screen::Confirm { accepted: 1, skipped: 0, not_found: 0, .. }));
    }

    #[test]
    fn digit_accepts_that_candidate_and_review_completes_to_confirm() {
        let mut app = App::from_matches(
            "Mix".into(),
            vec![tm("c", Confidence::Ambiguous, 3), tm("e", Confidence::Ambiguous, 2)],
        );
        app.on_key(Key::Digit(2)); // pick candidate index 1 for row 0, advance
        app.on_key(Key::Skip); // skip row 1
        app.on_key(Key::Confirm);
        let Screen::Confirm { accepted, skipped, .. } = &app.screen else {
            panic!("decided review must confirm")
        };
        assert_eq!((*accepted, *skipped), (1, 1));
        assert_eq!(app.accepted_catalog_ids(), vec!["cat1".to_string()]);
    }

    #[test]
    fn confirm_is_refused_while_any_row_is_pending() {
        let mut app = App::from_matches("Mix".into(), vec![tm("c", Confidence::Ambiguous, 2)]);
        app.on_key(Key::Confirm);
        assert!(matches!(app.screen, Screen::Review { .. }), "cannot confirm with pending rows");
    }

    #[test]
    fn digit_out_of_range_is_ignored() {
        let mut app = App::from_matches("Mix".into(), vec![tm("c", Confidence::Ambiguous, 2)]);
        app.on_key(Key::Digit(9));
        let Screen::Review { items, .. } = &app.screen else { panic!() };
        assert!(matches!(items[0].decision, Decision::Pending));
    }

    #[test]
    fn abort_works_from_any_screen_and_confirm_reaches_done() {
        let mut reviewing = App::from_matches("Mix".into(), vec![tm("c", Confidence::Ambiguous, 1)]);
        reviewing.on_key(Key::Abort);
        assert!(matches!(reviewing.screen, Screen::Aborted));

        let mut confirming = App::from_matches("Mix".into(), vec![tm("a", Confidence::Exact, 1)]);
        confirming.on_key(Key::Confirm);
        assert!(matches!(confirming.screen, Screen::Done));
    }
}
```

- [ ] **Step 2: Run to verify failure** — `just test rocola`, expected compile error.

- [ ] **Step 3: Implement**

Top of `rocola/src/app.rs`:

```rust
use rocola_core::{Confidence, TrackMatch};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Digit(u8),
    Skip,
    AcceptAllHigh,
    Confirm,
    Abort,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Accepted(usize),
    Skipped,
    Pending,
}

#[derive(Debug, Clone)]
pub struct ReviewItem {
    pub track: TrackMatch,
    pub decision: Decision,
}

#[derive(Debug, Clone)]
pub enum Screen {
    Review { items: Vec<ReviewItem>, cursor: usize },
    Confirm { playlist_name: String, accepted: usize, skipped: usize, not_found: usize },
    Done,
    Aborted,
}

#[derive(Debug)]
pub struct App {
    pub screen: Screen,
    pub playlist_name: String,
    pub auto_accepted: Vec<TrackMatch>,
    pub not_found: Vec<TrackMatch>,
    /// Review rows, kept after review completes so Confirm/Result can report
    /// every decision and accepted_catalog_ids can read reviewer choices.
    pub decided: Vec<ReviewItem>,
}

impl App {
    pub fn from_matches(playlist_name: String, matches: Vec<TrackMatch>) -> Self {
        let mut auto_accepted = Vec::new();
        let mut not_found = Vec::new();
        let mut review = Vec::new();
        for m in matches {
            match m.confidence {
                Confidence::Exact | Confidence::High => auto_accepted.push(m),
                Confidence::NotFound => not_found.push(m),
                Confidence::Ambiguous => review.push(ReviewItem { track: m, decision: Decision::Pending }),
            }
        }
        let mut app = Self {
            screen: Screen::Review { items: review, cursor: 0 },
            playlist_name,
            auto_accepted,
            not_found,
            decided: Vec::new(),
        };
        app.advance_if_review_done();
        app
    }

    pub fn on_key(&mut self, key: Key) {
        if key == Key::Abort {
            self.screen = Screen::Aborted;
            return;
        }
        match &mut self.screen {
            Screen::Review { items, cursor } => {
                match key {
                    Key::Up => *cursor = cursor.saturating_sub(1),
                    Key::Down => *cursor = (*cursor + 1).min(items.len().saturating_sub(1)),
                    Key::Digit(n) => {
                        let idx = usize::from(n).wrapping_sub(1);
                        if let Some(item) = items.get_mut(*cursor) {
                            if idx < item.track.candidates.len() {
                                item.decision = Decision::Accepted(idx);
                                *cursor = (*cursor + 1).min(items.len().saturating_sub(1));
                            }
                        }
                    }
                    Key::Skip => {
                        if let Some(item) = items.get_mut(*cursor) {
                            item.decision = Decision::Skipped;
                            *cursor = (*cursor + 1).min(items.len().saturating_sub(1));
                        }
                    }
                    Key::AcceptAllHigh | Key::Confirm => {}
                    Key::Abort => unreachable!("handled above"),
                }
                // A Digit or Skip may have decided the last pending row, and
                // Confirm should promote an already-decided review.
                if !matches!(key, Key::Up | Key::Down) {
                    self.advance_if_review_done();
                }
            }
            Screen::Confirm { .. } => {
                if key == Key::Confirm {
                    self.screen = Screen::Done;
                }
            }
            Screen::Done | Screen::Aborted => {}
        }
    }

    /// Everything Confirm will write: best candidate of each auto-accepted
    /// match, then each reviewer-chosen candidate.
    pub fn accepted_catalog_ids(&self) -> Vec<String> {
        let review_rows: &[ReviewItem] = match &self.screen {
            Screen::Review { items, .. } => items,
            _ => &self.decided,
        };
        self.auto_accepted
            .iter()
            .filter_map(|m| m.candidates.first().map(|c| c.catalog_id.clone()))
            .chain(review_rows.iter().filter_map(|item| match item.decision {
                Decision::Accepted(i) => item.track.candidates.get(i).map(|c| c.catalog_id.clone()),
                _ => None,
            }))
            .collect()
    }

    fn advance_if_review_done(&mut self) {
        let Screen::Review { items, .. } = &self.screen else { return };
        if items.iter().any(|i| matches!(i.decision, Decision::Pending)) {
            return;
        }
        let accepted = self.auto_accepted.len()
            + items.iter().filter(|i| matches!(i.decision, Decision::Accepted(_))).count();
        let skipped = items.iter().filter(|i| matches!(i.decision, Decision::Skipped)).count();
        self.decided = items.clone();
        self.screen = Screen::Confirm {
            playlist_name: self.playlist_name.clone(),
            accepted,
            skipped,
            not_found: self.not_found.len(),
        };
    }
}
```

- [ ] **Step 4: Run to verify pass** — `just test rocola && just lint rocola`, expected PASS.

- [ ] **Step 5: Checkpoint — Pedro commits**

Suggested: `feat(cli): review/confirm state machine with nothing-dropped guarantee`

---

## Task 14: rocola binary — wiring, rendering, first-run setup, end-to-end

The last mile: `MusicTarget` impl for the Apple side, ratatui rendering of the Task 13 state, first-run setup prompts, duplicate-run guard, result report. Rendering makes no decisions — every rule already has a test in Task 13.

Deliberate deviation from the spec's six screens: Input is the CLI argument (a URL is pasted more naturally into a shell than a TUI textbox), Setup is plain stdin prompts (copy-paste friendly, happens once), and Matching is a progress line. Review, Confirm and Result — the screens that carry the product's three rules — are the TUI.

**Files:**
- Create: `rocola-apple/src/target.rs` (MusicTarget impl) + modify `rocola-apple/src/lib.rs`, `rocola-apple/Cargo.toml`
- Create: `rocola/src/ui.rs`, `rocola/src/setup.rs`, `rocola/src/run.rs`
- Modify: `rocola/src/main.rs`, `rocola/Cargo.toml` (add rocola-spotify/apple deps already present; add `anyhow = "1"`)
- Modify: `rocola-apple/src/client.rs` (add `list_playlist_names`)

**Interfaces:**
- Consumes: everything produced by Tasks 1–13.
- Produces:
  - `rocola-apple/src/target.rs`: `pub struct AppleTarget { pub client: AppleClient, pub storefront: String }` implementing `rocola_core::MusicTarget` with `type Error = AppleError` — `resolve_by_isrc` and `search` delegate to the client with the stored storefront. Requires `rocola-core` dep (already present).
  - `AppleClient::list_playlist_names(&self) -> Result<Vec<String>, AppleError>` — `GET /v1/me/library/playlists?limit=100`, maps `data[].attributes.name`; used by the duplicate-run guard.
  - `rocola/src/setup.rs`: `pub fn first_run_setup() -> anyhow::Result<Config>` — plain stdin/stdout prompts (not ratatui; setup happens once and must be copy-paste friendly). Prompts in this order, with this copy:
    1. "rocola needs a Spotify app of your own (free, ~2 minutes).\n  1. Open https://developer.spotify.com/dashboard and create an app.\n  2. Add this exact Redirect URI: http://127.0.0.1:8888/callback\n  3. Paste the app's Client ID here: "
    2. "Now your Apple pieces (needs a paid Apple Developer membership):\n  Team ID (developer.apple.com → Membership): "
    3. "Key ID of your MusicKit key: "
    4. "Path to your AuthKey_….p8 file (Apple lets you download it only once — keep it safe): "
    — then saves via `Config::save` and prints where it saved. If `p8_inside_git_worktree()`, print: "Warning: your .p8 key is inside a git repository. Move it somewhere private — anyone who can read that repo can impersonate your Apple developer account."
  - `rocola/src/ui.rs`: `pub fn draw(frame: &mut ratatui::Frame, app: &App)` — match on `app.screen`:
    - `Review`: a `ratatui::widgets::Table` of the cursor row's source track (title/artist/album/duration) above a numbered `List` of candidates, each line formatted `"{n}. {title} — {artists} — {album} ({duration})"`, with the field that differs from the source rendered in `Style::new().bold()` (compare via `rocola_core::normalize::normalize_title`/`normalize_artist` and a 3s duration window). Footer: `"↑/↓ move · 1-9 pick · s skip · enter confirm when done · q abort · {decided}/{total} decided"`.
    - `Confirm`: a `Paragraph`: `"Create Apple Music playlist \"{name}\" with {accepted} songs?\n{skipped} skipped by you · {not_found} not found on Apple Music (all listed after creation)\n\nenter create · q abort"`.
    - `Done`/`Aborted`: single line, `"Done — see the report above."` / `"Nothing was created."`.
  - `rocola/src/run.rs`: `pub async fn run(url: &str) -> anyhow::Result<()>` — the orchestration:
    1. Load config (`Config::load(&Config::default_path())`); `None` → `setup::first_run_setup()`.
    2. Spotify token: refresh if `refresh_token` present, else `run_auth_flow`; persist any new refresh token immediately via `Config::save`.
    3. `parse_playlist_url(url)`, `fetch_playlist` → (name, tracks). Print `"Read \"{name}\" — {n} songs from Spotify."`
    4. Apple: read the .p8 (error names the path), `mint_developer_token`; reuse `music_user_token` from config if present, else `run_user_auth` and save; `storefront()` cached in config the same way. If any Apple call answers 401/403 with a stored user token, clear it, re-run `run_user_auth`, retry once.
    5. `AppleTarget { client, storefront }`, `match_tracks` with a plain `"Matching {i}/{n}…"` stderr progress line.
    6. `App::from_matches`, then the ratatui loop: `crossterm` raw mode + alternate screen, map `KeyCode::Up/Down → Key::Up/Down`, `Char('1'..='9') → Digit`, `Char('s') → Skip`, `Char('A') → AcceptAllHigh`, `Enter → Confirm`, `Char('q')/Esc → Abort`; draw after every event; leave the loop on `Done` or `Aborted`; ALWAYS restore the terminal (a `Drop` guard struct around raw-mode/alt-screen so a panic can't wreck the shell).
    7. On `Aborted`: print `"Nothing was created."` and exit 0.
    8. On `Done`: duplicate-run guard — `list_playlist_names`; if `name` already exists, ask on stdin: `"You already have an Apple Music playlist called \"{name}\". [a]dd these songs to it is not supported yet — create \"{name} (rocola)\" instead? [y/N] "` (v1 creates under the suffixed name on `y`, exits without writing on anything else — adding to an existing playlist needs a library-playlist id lookup that v1 defers).
    9. `create_playlist(name, "Recreated from Spotify by rocola")`, `add_tracks` in one call, then the report (spec: never drop silently):
       `"Created \"{name}\" — {accepted} songs added."` then, if any, `"Skipped by you:"` + one line per skip, and `"Not found on Apple Music:"` + one line per miss formatted `"  {title} — {artists}"`.
  - `rocola/src/main.rs`:

```rust
mod app;
mod config;
mod run;
mod setup;
mod ui;

use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    let Some(url) = std::env::args().nth(1) else {
        eprintln!("Usage: rocola <spotify playlist url>");
        eprintln!("Example: rocola https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M");
        return ExitCode::from(2);
    };
    match run::run(&url).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
```

- [ ] **Step 1: Failing test for AppleTarget delegation** — in `rocola-apple/src/target.rs`, a compile-level test is sufficient and honest: the impl either satisfies the trait or the crate doesn't build. Add instead a unit test for the one piece of logic it owns (none — it is pure delegation), so: no new unit test here; the corpus, pipeline and client tests already cover both sides of the seam. Write the impl:

```rust
use rocola_core::{Candidate, MusicTarget, SourceTrack};

use crate::client::AppleClient;
use crate::AppleError;

/// The Apple side of the rocola-core seam: a client bound to one storefront.
pub struct AppleTarget {
    pub client: AppleClient,
    pub storefront: String,
}

impl MusicTarget for AppleTarget {
    type Error = AppleError;

    async fn resolve_by_isrc(&self, isrcs: &[String]) -> Result<Vec<(String, Candidate)>, AppleError> {
        self.client.resolve_by_isrc(&self.storefront, isrcs).await
    }

    async fn search(&self, track: &SourceTrack) -> Result<Vec<Candidate>, AppleError> {
        self.client.search(&self.storefront, track).await
    }
}
```

- [ ] **Step 2: Add `list_playlist_names` to `rocola-apple/src/client.rs`** with a fixture test first (failing → passing): fixture `rocola-apple/tests/fixtures/library_playlists.json`:

```json
{ "data": [ { "id": "p.abc", "attributes": { "name": "Roadtrip" } } ] }
```

Test in `rocola-apple/tests/api_types.rs`:

```rust
use rocola_apple::api_types::LibraryPlaylists;

#[test]
fn library_playlists_expose_names() {
    let r: LibraryPlaylists = serde_json::from_str(include_str!("fixtures/library_playlists.json")).unwrap();
    assert_eq!(r.names(), vec!["Roadtrip".to_string()]);
}
```

Types in `api_types.rs`:

```rust
#[derive(Debug, Deserialize)]
pub struct LibraryPlaylists {
    pub data: Vec<LibraryPlaylist>,
}

#[derive(Debug, Deserialize)]
pub struct LibraryPlaylist {
    pub id: String,
    pub attributes: LibraryPlaylistAttributes,
}

#[derive(Debug, Deserialize)]
pub struct LibraryPlaylistAttributes {
    pub name: String,
}

impl LibraryPlaylists {
    pub fn names(&self) -> Vec<String> {
        self.data.iter().map(|p| p.attributes.name.clone()).collect()
    }
}
```

Client method:

```rust
    pub async fn list_playlist_names(&self) -> Result<Vec<String>, AppleError> {
        let r: crate::api_types::LibraryPlaylists = self
            .send(|| self.request(reqwest::Method::GET, "/v1/me/library/playlists?limit=100"))
            .await?
            .json()
            .await
            .map_err(|e| AppleError::Http(e.to_string()))?;
        Ok(r.names())
    }
```

- [ ] **Step 3: Implement `ui.rs`, `setup.rs`, `run.rs`, `main.rs`** to the Interfaces block above. The Interfaces block is the specification — copy strings verbatim; they are the product's content design and will be reviewed as such in Task 15.

- [ ] **Step 4: Full check** — `just ci`, expected: fmt clean, clippy clean, all tests PASS across the workspace.

- [ ] **Step 5: End-to-end manual verification (Pedro + agent together)**

1. Delete/rename any existing `~/.config/rocola/config.toml`; run `just run -- <a real 50-track playlist URL>` (adjust justfile run recipe to pass args: `run *args:` / `cargo run -p rocola -- {{args}}`).
2. First-run setup completes; both browser sign-ins land; matching runs; review screen appears for ambiguous tracks; confirm; playlist appears in Music.app.
3. Verify the report lists every skipped and unmatched track (count must equal Spotify count minus added).
4. Run the same URL again — the duplicate guard must offer the suffixed name, and declining must write nothing.
5. `ls -l ~/.config/rocola/config.toml` → `-rw-------`.

- [ ] **Step 6: Checkpoint — Pedro commits**

Suggested: `feat(cli): end-to-end flow — fetch, match, review, create playlist`

---

## Task 15: content-design pass and README

**Files:**
- Create: `README.md` (root)
- Modify: any user-facing string the review changes

**Interfaces:** none — this is the M6 gate before calling it public.

- [ ] **Step 1: Run the content-design skill over every user-facing string** — setup prompts, all error messages (`SpotifyError`, `AppleError`, `ConfigError` display strings), TUI footers, confirm copy, report copy, README. Fix what it flags. (Agent: invoke the `content-design` skill; it reviews against plain-language and error-names-the-fix standards.)

- [ ] **Step 2: Write the README** with this structure (spec §Content design — the membership requirement in the first screenful):

```markdown
# rocola

Recreate a Spotify playlist on your Apple Music account, from the terminal.

## Before you start

Running rocola needs two things:

- **A Spotify account and a free Spotify developer app** (~2 minutes to set up;
  rocola walks you through it on first run).
- **A paid Apple Developer Program membership (~£79–99/year).** Apple only
  issues the credentials that its Music API requires to paid members. There is
  no free way around this — rocola will not ask you to extract tokens from
  Apple's web player, because those break without warning and put your
  Apple ID at risk.

**Contributing is different:** the matching engine — the interesting part —
runs entirely offline against test fixtures. You can build, test and improve
most of rocola with no credentials and no membership. See CONTRIBUTING below.
```

…followed by: Install (cargo install / release binaries), First run (what the setup asks for, with the Apple portal click-path and the one-time-download warning for the .p8), Usage (`rocola <playlist url>`, what the review screen keys do), What rocola can't do (Spotify-made editorial/algorithmic playlists — Spotify blocks new apps from reading them; songs missing from your country's Apple catalog — reported, never silently dropped), Where your credentials live (`~/.config/rocola/config.toml`, mode 0600, what's in it and what never touches disk), and Contributing (fixture corpus in `rocola-core/tests/fixtures/corpus.json`, `just ci`, no credentials needed).

- [ ] **Step 3: Verify** — `just ci` still green; read the README top-to-bottom once as a stranger.

- [ ] **Step 4: Checkpoint — Pedro commits**

Suggested: `docs: README with honest prerequisites; content-design pass over all copy`

---

## Verification (whole plan)

- `just ci` green at every checkpoint — fmt, clippy pedantic+nursery with `-D warnings`, full test suite, all offline.
- `just hurl-spotify` and `just hurl-apple` green with real credentials (manual, never CI).
- Task 14 Step 5 end-to-end run against a real playlist, including the re-run duplicate guard and the 0600 check.
- The nothing-dropped invariant: in the final report, added + skipped + not-found + (local/removed tracks noted at fetch) equals the Spotify track count.

## Execution notes

- Tasks 1–6, 8, 11–13 are fully offline — an agent can drive them end to end, stopping at each checkpoint for Pedro to commit.
- Tasks 0, 7 (step 7), 10 (step 9), 14 (step 5) need Pedro's credentials or hands: the M0 probe, hurl runs, and the live end-to-end.
- Clippy pedantic+nursery is strict: expect to add targeted `#[allow]`s with a one-line reason (e.g. `missing_errors_doc` on internal fns) rather than fighting every lint; never blanket-allow at crate level.

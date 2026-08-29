# rocola — Spotify playlist → Apple Music, as a Rust TUI

**Status:** design, pending review. No implementation has begun.
**Date:** 2026-08-29

## Context

`rocola` takes a Spotify playlist URL and recreates it on the user's Apple Music
account. It is a terminal app in Rust, developed in public so others can use,
fork and contribute to it — which makes onboarding, copy and honesty about
prerequisites part of the deliverable, not an afterthought.

A feasibility spike ran before this design. Its findings shaped every major
decision below, so they are recorded here rather than assumed.

## Spike findings (evidence, not assumption)

**The Apple Music API can do the job, and cleanly.**
`POST /v1/me/library/playlists/{id}/tracks` accepts `type: "songs"` — a
*catalog* ID — so tracks go straight into a playlist with no add-to-library
step. Verified verbatim from Apple's `LibraryPlaylistTracksRequest.Data` schema:
*"The possible values are `library-music-videos`, `library-songs`,
`music-videos`, or `songs`."*

**Matching can be near-exact.** `GET /v1/catalog/{storefront}/songs?filter[isrc]=…`
is an official endpoint. Spotify returns `external_ids.isrc` per track. So the
common case is an exact recording-level match, and fuzzy search is only needed
for a tail.

**Calling the API requires a paid Apple Developer Program membership (~£99/yr).**
Apple staff, on the record in the developer forums: *"you'll need to create a
MusicKit identifier and private key to sign your developer tokens using
Certificates, Identifiers & Profiles… where access to C,I&P requires a paid
Apple Developer Program account."*

**No free macOS path exists.** Every public automation surface was audited:

| Surface | How checked | Result |
|---|---|---|
| AppleScript (Music.app) | read `sdef` | `add` takes **files only**; playlist elements are file/URL/shared track; no catalog type; no ISRC property |
| App Intents (Music.app) | searched bundle | none present |
| Shortcuts / WorkflowKit | grepped action strings | no catalog actions; music actions library-scoped |
| `iTunesLibrary.framework` | grepped symbols | **public but read-only** — no write methods |
| `MusicKit.framework` (Swift) | docs + forums | needs `com.apple.developer.music-kit` entitlement → App ID capability → C,I&P → paid |
| Music.app private ObjC | `strings` on binary | `addEntityWithCatalogID:kind:playlistIdentifier:` exists but is private |

The capability is demonstrably present inside Music.app and deliberately not
exposed. Confidence that no sanctioned free route exists: ~99%.

**Decision:** build the MusicKit API path. The membership requirement is stated
plainly in the README's first screenful, and mitigated by the architecture
below, which keeps the interesting majority of the codebase credential-free.

**Explicitly rejected:** extracting the developer token Apple embeds in its own
web player. It rotates without notice, sits outside Apple's terms, and puts a
user's Apple ID at risk — unacceptable for a tool strangers are invited to run.

---

## Architecture

A small workspace. The split is not ceremony: it makes "the core needs no
credentials" a *structural* property rather than a promise, because
`rocola-core` does not depend on an HTTP client at all.

```
rocola-core/      domain types + matching engine. No I/O, no network deps.
rocola-spotify/   PKCE auth, playlist fetch.
rocola-apple/     developer token, user token, catalog resolve, playlist write.
rocola/           binary: TUI, config, orchestration.
```

### The seam that matters

Apple write access sits behind one trait, so a future AppleScript
("no membership? degraded, library-only mode") backend lands without rework.
Build the seam now; ship one implementation.

```rust
#[async_trait]
pub trait MusicTarget {
    async fn resolve(&self, track: &SourceTrack) -> Result<Vec<Candidate>>;
    async fn create_playlist(&self, name: &str, desc: Option<&str>) -> Result<PlaylistId>;
    async fn add_tracks(&self, id: &PlaylistId, tracks: &[TargetTrackId]) -> Result<()>;
}
```

Resolution is also trait-backed so tests and contributors can run the whole
matching pipeline against JSON fixtures with zero credentials.

### Matching pipeline

1. Fetch Spotify playlist → `SourceTrack { isrc, title, artists, album, duration_ms }`.
2. **Tier 1 — ISRC exact.** `filter[isrc]=` accepts a comma-separated list;
   batch ~25 per request. Normalise first: Spotify ISRCs appear inconsistently
   cased and occasionally hyphenated.
3. **Tier 2 — text search fallback.** `search?types=songs&term=…` for anything
   tier 1 missed.
4. **Score** candidates: normalised title (strip `feat.`, `- Remastered`,
   parentheticals), artist overlap, duration delta (±3s is a strong signal),
   album match.
5. **Classify:** `Exact` / `High` / `Ambiguous` / `NotFound`. Auto-accept the
   first two; the rest go to the review queue.

The scoring function is pure and lives in `rocola-core` — the single most
testable and most contributable part of the project.

### Auth and credentials

Two flows, both browser-assisted, neither requiring a secret in the repo.

**Spotify (Authorization Code + PKCE).** No client secret exists in this flow.
The user registers an app once and pastes a client ID. Redirect URI must be
`http://127.0.0.1:8888/callback` — a **fixed** port, because Spotify requires
exact pre-registration. Note Spotify's Nov 2025 rules: HTTPS required *except*
for loopback literals, and `localhost` is no longer accepted — it must be
`127.0.0.1`.

**Apple.** The user creates a Media ID + key in C,I&P and downloads the `.p8` once.

- *Developer token*: ES256 JWT (`iss`=team ID, `kid`=key ID, `exp` ≤ ~6 months),
  minted **in memory every run**, never persisted.
- *Music User Token*: the TUI serves a local page on `127.0.0.1` loading
  MusicKit JS v3, calls `music.authorize()`, and receives the token by POST.
- Storefront via `GET /v1/me/storefront` — catalog IDs are storefront-specific.

### Storage — `~/.config/rocola/config.toml`, mode 0600

Created 0600 at open time, not `chmod`-ed afterwards.

```toml
[spotify]
client_id = "…"          # not secret
refresh_token = "…"

[apple]
team_id = "…"
key_id = "…"
p8_path = "~/.config/rocola/AuthKey_XXXX.p8"   # path only; file stays put
music_user_token = "…"
storefront = "gb"
```

Short-lived tokens (Spotify access token, Apple developer JWT) live in memory
only. On startup, warn if `p8_path` resolves inside a git working tree.

---

## The TUI

Screens: **Setup** (first run) → **Input** → **Matching** → **Review** →
**Confirm** → **Result**.

**Review is the product.** Everything else is plumbing. Per ambiguous track:
candidates ranked, with the *differing* field highlighted (duration, album,
artist) so the choice is glanceable. `↑/↓` navigate, `1-9` pick, `s` skip,
`A` accept all high-confidence, `?` help.

Three rules the implementation must not violate:

- **Never write without confirmation.** The Confirm screen states exactly what
  will be created and how many tracks are included.
- **Never drop a track silently.** Every unmatched track appears in the result
  with a reason, and the list is exportable.
- **Writes are visible and re-runnable.** A second run on the same URL offers
  "create new" or "add to existing" rather than quietly duplicating.

### Content design

All user-facing copy gets a dedicated content-design pass before release — a
scheduled task, not a vibe.

- Plain English. "Couldn't find this on Apple Music", not "resolution failed".
- Every error names the fix: *"Your Apple sign-in has expired. Press `r` to sign
  in again."*
- README leads with **Before you start** — the ~£99/yr membership, stated in the
  first screenful with the reason, not buried under a feature list. A public repo
  that hides its prerequisite wastes strangers' time.
- README also states plainly that **contributing to the matching engine needs no
  credentials and no membership**, because that is the honest and appealing
  on-ramp.

---

## Verification

- `cargo test -p rocola-core` — matching engine against a golden corpus of ~50
  fixture pairs, deliberately including remasters, live versions, features,
  classical works, non-Latin scripts, and explicit/clean variants. Runs offline.
- Provider tests against recorded HTTP fixtures (`wiremock`). **No network in CI.**
- End-to-end manual: run against a real 50-track playlist, confirm the created
  Apple Music playlist matches, and confirm every skipped track is reported.
- Re-run the same URL and confirm no silent duplication.

## Risks

1. **MusicKit JS on a loopback origin** — it requires a secure context.
   Loopback *is* a "potentially trustworthy origin" per spec, so this should
   work, but it is unverified and the entire Apple auth flow depends on it.
   **Verify this on day one, before anything else is built.**
2. **Port 8888 occupied** — the fixed port is forced by Spotify's exact-match
   registration. Needs a clear error, not a hang.
3. **Storefront gaps** — a recording present in `us` may be absent in `gb`.
   Report as unmatched with that specific reason.
4. **Rate limits** — Apple 429s; Spotify returns `Retry-After`. Backoff required.
5. **Spotify editorial/algorithmic playlists** are blocked for new apps
   (Nov 2024). Detect and explain rather than failing obscurely.
6. **`.p8` is a one-time download** — say so loudly during setup.

## Milestones

| # | Deliverable | Credentials needed |
|---|---|---|
| M0 | Verify MusicKit JS authorises from `127.0.0.1` | Apple membership |
| M1 | `rocola-core`: types + matching engine + fixture corpus | **none** |
| M2 | Spotify PKCE + playlist fetch | Spotify only |
| M3 | Apple developer token + user token + storefront | full |
| M4 | Apple resolve (ISRC + search) + playlist write | full |
| M5 | TUI, all six screens | full |
| M6 | Content-design pass, README, public release | none |

M0 is deliberately first: it is the only remaining unknown that could invalidate
the design, and it is cheap to test.

## References

- [Add Tracks to a Library Playlist](https://developer.apple.com/documentation/applemusicapi/add-tracks-to-a-library-playlist)
- [Get Multiple Catalog Songs by ISRC](https://developer.apple.com/documentation/applemusicapi/get-multiple-catalog-songs-by-isrc)
- [Apple Developer Forums — Apple Music API membership requirement](https://developer.apple.com/forums/thread/661313)
- [Spotify — Authorization Code with PKCE Flow](https://developer.spotify.com/documentation/web-api/tutorials/code-pkce-flow)
- [Spotify — Redirect URIs](https://developer.spotify.com/documentation/web-api/concepts/redirect_uri)

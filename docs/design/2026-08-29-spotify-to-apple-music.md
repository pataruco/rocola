# rocola — Spotify playlist → Apple Music, as a Rust TUI

**Status:** design, revised 2026-08-29 after verifying every external claim
against current Apple/Spotify documentation. No implementation has begun.
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

**Matching can be near-exact — but an ISRC hit is a candidate, not an answer.**
`GET /v1/catalog/{storefront}/songs?filter[isrc]=…` is an official endpoint,
and Spotify returns `external_ids.isrc` per track. Two caveats, straight from
Apple's docs: *"one ISRC value may return more than one song. The maximum fetch
limit is 25"* — the same recording appears on original albums, compilations and
regional releases — and misses are silent: the response simply omits unmatched
ISRCs. So ISRC results still go through the scorer (album and duration break
ties), and unresolved tracks are computed as the diff of the request set
against `attributes.isrc` in the response.

**Calling the API requires a paid Apple Developer Program membership (~£99/yr).**
Apple staff, on the record in the developer forums: *"you'll need to create a
MusicKit identifier and private key to sign your developer tokens using
Certificates, Identifiers & Profiles… where access to C,I&P requires a paid
Apple Developer Program account."* Writing to a library additionally requires
an active **Apple Music subscription** on the account that authorises.

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

Error handling follows the usual split: `thiserror` enums in the library
crates, `anyhow` context at the binary boundary. Long-lived secrets are held
in `secrecy::SecretString` so a stray `Debug` log can't leak them.

### The seam that matters

Apple write access sits behind one trait, so a future AppleScript
("no membership? degraded, library-only mode") backend lands without rework.
Build the seam now; ship one implementation.

```rust
#[async_trait]
pub trait MusicTarget {
    /// Batched on purpose: Apple's ISRC filter takes at most 25 per request,
    /// so the trait takes the whole set and lets the backend own its chunking.
    async fn resolve(&self, tracks: &[SourceTrack]) -> Result<Vec<Resolution>>;
    async fn create_playlist(&self, name: &str, desc: Option<&str>) -> Result<Playlist>;
    async fn add_tracks(&self, id: &PlaylistId, tracks: &[TargetTrackId]) -> Result<()>;
    /// The promised behaviours need reads too: "re-run offers add-to-existing"
    /// needs playlist lookup, and "never drop a track silently" is only
    /// provable by reading the playlist back after the write.
    async fn find_playlists(&self, name: &str) -> Result<Vec<Playlist>>;
    async fn playlist_tracks(&self, id: &PlaylistId) -> Result<Vec<TargetTrackId>>;
}
```

Resolution is also trait-backed so tests and contributors can run the whole
matching pipeline against JSON fixtures with zero credentials.

### Matching pipeline

1. Fetch Spotify playlist (paginated; the documented page maximum is 50) →
   `SourceTrack { isrc, title, artists, album, duration_ms, explicit }`.
2. **Tier 0 — triage.** Local files (`is_local: true`, `id: null`, empty
   `external_ids`) and podcast episodes (`type: "episode"`) can never match;
   route them straight to the report with that reason instead of letting them
   fail downstream as bad searches.
3. **Tier 1 — ISRC.** `filter[isrc]=` accepts a comma-separated list; batch
   25 per request (the documented maximum). Normalise first: Spotify ISRCs
   appear inconsistently cased and occasionally hyphenated. One ISRC may
   return several songs, so these are *candidates*, scored like any other.
4. **Tier 2 — text search fallback.** `search?types=songs&term=…` for anything
   tier 1 missed.
5. **Score** candidates: normalised title (strip `feat.`, `- Remastered`,
   parentheticals), artist overlap, duration delta (±3s is a strong signal),
   album match, explicit/clean agreement (Spotify `explicit` vs Apple
   `contentRating`).
6. **Classify:** `Exact` / `High` / `Ambiguous` / `NotFound`. Auto-accept the
   first two; the rest go to the review queue.

The scoring function is pure and lives in `rocola-core` — the single most
testable and most contributable part of the project.

### Auth and credentials

Two flows, both browser-assisted, neither requiring a secret in the repo.

**Spotify (Authorization Code + PKCE).** No client secret exists in this flow.
The user registers an app once, pastes a client ID, and adds their own account
to the app's user allowlist (development mode — since Feb 2026 capped at 5
users, and the app owner must hold Spotify Premium). Register the redirect URI
as `http://127.0.0.1/callback` **without a port**: Spotify implements
RFC 8252 §7.3 for loopback literals, so the app binds an ephemeral port at
runtime and appends it at authorisation time — no fixed port, no collision to
handle. Per the Nov 2025 rules, HTTPS is required *except* for loopback IP
literals, and `localhost` is not accepted — it must be `127.0.0.1`.

PKCE refresh tokens **rotate**: each refresh returns a new refresh token and
invalidates the old one, so the new token is persisted atomically before the
old one is dropped. Since June 2026 refresh tokens also **expire**, so
"sign in to Spotify again" is a routine flow, not an error path.

**Apple.** The user creates a Media ID + key in C,I&P and downloads the `.p8` once.

- *Developer token*: ES256 JWT (`iss`=team ID, `kid`=key ID), minted
  **in memory every run** with a short `exp` (hours, not the 6-month maximum),
  never persisted. The optional `origin` claim is deliberately omitted so the
  token works from the loopback page.
- *Music User Token*: the TUI serves a local page on `127.0.0.1` loading
  MusicKit JS v3, calls `music.authorize()`, and receives the token by POST.
  Its lifetime is undocumented and effectively arbitrary — field reports range
  from days to months, and a password change revokes it instantly. Any `403`
  from a `/v1/me/…` endpoint therefore means one thing: re-run the browser
  authorisation. First-class flow, not an edge case.
- Storefront via `GET /v1/me/storefront` — catalog IDs are storefront-specific.

### Storage — `~/.config/rocola/config.toml`, mode 0600

Created 0600 at open time, not `chmod`-ed afterwards.

```toml
[spotify]
client_id = "…"          # not secret
refresh_token = "…"      # rotates on every refresh; rewritten atomically

[apple]
team_id = "…"
key_id = "…"
p8_path = "~/.config/rocola/AuthKey_XXXX.p8"   # path only; file stays put
music_user_token = "…"
storefront = "gb"
```

Short-lived tokens (Spotify access token, Apple developer JWT) live in memory
only. On startup, warn if `p8_path` resolves inside a git working tree.

Each run also writes a manifest — `~/.local/state/rocola/runs/<id>.json`:
source playlist snapshot, every match decision, the created playlist ID. The
manifest is what makes the unmatched list exportable and re-run detection
cheap, rather than both being reconstructed from memory.

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
  with a reason, and the list is exportable. Because add-tracks returns a bare
  `204` with no per-track result, the Result screen is built by reading the
  playlist back and diffing — allowing for Apple's documented delay before new
  library resources appear.
- **Writes are visible and re-runnable.** A second run on the same URL offers
  "create new" or "add to existing" rather than quietly duplicating.

### Content design

All user-facing copy gets a dedicated content-design pass before release — a
scheduled task, not a vibe.

- Plain English. "Couldn't find this on Apple Music", not "resolution failed".
- Every error names the fix: *"Your Apple sign-in has expired. Press `r` to sign
  in again."*
- README leads with **Before you start** — all three prerequisites in the first
  screenful, with reasons: the ~£99/yr Apple Developer membership, an active
  Apple Music subscription, and a Spotify account (Premium, per the Feb 2026
  development-mode rules) with a self-registered app. A public repo that hides
  its prerequisites wastes strangers' time.
- A 404 fetching the Spotify playlist is genuinely ambiguous: Spotify returns
  a plain 404 for its own editorial/algorithmic playlists to development-mode
  apps, indistinguishable from "deleted" or "private". The error copy must
  offer all three possibilities.
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
  Apple Music playlist matches via read-back, and confirm every skipped track
  is reported with its reason.
- Re-run the same URL and confirm no silent duplication.

## Risks

1. **Music User Token dies without notice.** Lifetime is undocumented; reports
   range from days to months, and a password change revokes it immediately.
   Surfaces as a 403. Mitigated by making re-auth a first-class flow and by
   measuring real longevity in M0.
2. **MusicKit JS on a loopback origin** — it requires a secure context.
   Loopback is a "potentially trustworthy origin" per spec, community projects
   demonstrably run `music.authorize()` from a plain-http `127.0.0.1` page
   (`file://` does not work; the occasional reported 403 traces to referrer
   policy, so the served page sets a sane one), and Apple's `origin` JWT claim
   is optional. Confirmed first-hand in M0 anyway, because the entire Apple
   auth flow depends on it.
3. **Add-tracks is opaque and occasionally flaky.** No documented batch limit
   (community practice chunks at ≤100); intermittent 400/500 reported on large
   batches; a 204 carries no per-track outcome. Chunk ≤100, retry 5xx with
   backoff, bisect a persistently failing chunk, and trust only the read-back.
4. **Storefront gaps** — a recording present in `us` may be absent in `gb`.
   Report as unmatched with that specific reason.
5. **Rate limits** — Apple 429s; Spotify returns `Retry-After`. Backoff required.
6. **Spotify editorial/algorithmic playlists** are hidden from new apps
   (Nov 2024) behind a plain 404. Can't be detected, only explained — see
   content design. As of Spotify's Feb 2026 development-mode migration, this
   is now the narrower case: apps can only read the contents of playlists the
   signed-in user owns or collaborates on at all (a 403), so editorial
   playlists remain blocked on top of that, and the app owner needs Spotify
   Premium for dev-mode apps.
7. **`.p8` is a one-time download** — say so loudly during setup.

## Milestones

| # | Deliverable | Credentials needed |
|---|---|---|
| M0 | Verify MusicKit JS authorises from `127.0.0.1`; measure how long the captured Music User Token survives reuse outside the browser | Apple membership |
| M1 | `rocola-core`: types + matching engine + fixture corpus | **none** |
| M2 | Spotify PKCE + playlist fetch | Spotify only |
| M3 | Apple developer token + user token + storefront | full |
| M4 | Apple resolve (ISRC + search) + playlist write + read-back | full |
| M5 | TUI, all six screens | full |
| M6 | Content-design pass, README, public release | none |

M0 is deliberately first: community evidence says it passes, but it is the one
assumption everything else depends on, and it is cheap to test.

## References

- [Add Tracks to a Library Playlist](https://developer.apple.com/documentation/applemusicapi/add-tracks-to-a-library-playlist)
- [LibraryPlaylistTracksRequest](https://developer.apple.com/documentation/applemusicapi/libraryplaylisttracksrequest)
- [Get Multiple Catalog Songs by ISRC](https://developer.apple.com/documentation/applemusicapi/get-multiple-catalog-songs-by-isrc)
- [Generating Developer Tokens](https://developer.apple.com/documentation/applemusicapi/generating-developer-tokens)
- [Apple Developer Forums — Apple Music API membership requirement](https://developer.apple.com/forums/thread/661313)
- [Spotify — Authorization Code with PKCE Flow](https://developer.spotify.com/documentation/web-api/tutorials/code-pkce-flow)
- [Spotify — Redirect URIs](https://developer.spotify.com/documentation/web-api/concepts/redirect_uri)
- [Spotify — Quota modes / development mode](https://developer.spotify.com/documentation/web-api/concepts/quota-modes)
- [Spotify blog — Web API changes, Nov 2024](https://developer.spotify.com/blog/2024-11-27-changes-to-the-web-api)
- [Spotify blog — security requirements for redirect URIs, Feb 2025](https://developer.spotify.com/blog/2025-02-12-increasing-the-security-requirements-for-integrating-with-spotify)
- [Spotify blog — refresh token expiration, Jun 2026](https://developer.spotify.com/blog/2026-06-18-refresh-token-expiration)

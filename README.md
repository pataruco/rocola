# rocola

Recreate a Spotify playlist on your Apple Music account, from the terminal.

## Before you start

Running rocola needs three things:

- **A Spotify account with Premium, and a free Spotify developer app**
  (about 2 minutes to set up; rocola walks you through it on first run).
  Spotify only allows developer apps whose owner has Premium.
- **A paid Apple Developer Program membership (~£79–99/year).** Apple only
  issues the credentials that its Music API requires to paid members. There is
  no free way around this — rocola will not ask you to extract tokens from
  Apple's web player, because those break without warning and put your
  Apple ID at risk.
- **An active Apple Music subscription** on the account you want the playlist
  to appear on. That is the account rocola writes to.

**One limit worth knowing before you start:** rocola can only read Spotify
playlists you own or collaborate on. Spotify blocks apps like this one from
reading anything else, including its own editorial playlists. There is a simple
way round it — see [What rocola can't do](#what-rocola-cant-do).

**Contributing is different:** the matching engine — the interesting part —
runs against test fixtures, with no credentials and no calls to Spotify or
Apple. You can build, test and improve most of rocola with no credentials and
no membership. See [Contributing](#contributing).

## Install

rocola is not on crates.io yet and there are no prebuilt binaries. Build it
from source with a recent stable Rust toolchain:

```sh
cargo install --git https://github.com/pataruco/rocola rocola
```

Or clone the repo and run it from there:

```sh
git clone https://github.com/pataruco/rocola
cd rocola
just run <spotify playlist url>
```

## First run

The first time you run rocola it asks for four things and saves them to
`~/.config/rocola/config.toml`. It never asks again.

### 1. Your Spotify Client ID

1. Open <https://developer.spotify.com/dashboard> and create an app.
2. Add this exact Redirect URI: `http://127.0.0.1:8888/callback`
3. Copy the app's Client ID and paste it into rocola.

### 2. Your Apple Team ID, Key ID and .p8 key

All of this lives at <https://developer.apple.com> under **Certificates,
Identifiers & Profiles**.

1. **Identifiers** → the filter dropdown → **Media IDs** → **+** → register a
   **MusicKit** identifier.
2. **Keys** → **+** → name the key, tick **MusicKit**, and pick the identifier
   you just made. Continue, then Register.
3. **Download** the `AuthKey_XXXXXXXXXX.p8` file.
   **Apple lets you download it once.** If you lose it, you have to revoke the
   key and make a new one. Keep it somewhere private and outside any git
   repository — rocola warns you if the path you give it sits inside one.
4. The **Key ID** is the ten characters in the filename, also shown on the key
   page. Your **Team ID** is under **Membership details**.

Paste the Team ID, the Key ID and the path to the `.p8` file into rocola.

### 3. Sign in, twice

rocola opens your browser twice: once for Spotify, once for Apple Music. Both
sign-ins happen on `127.0.0.1` (ports 8888 and 8889) and both time out after
five minutes. Apple's page tells you exactly what rocola is asking for:
permission to create playlists in your library.

## Using rocola

```sh
rocola https://open.spotify.com/playlist/<id>
```

In Spotify, get the link with **Share → Copy link to playlist**. The
`spotify:playlist:<id>` form works too.

rocola then:

1. Reads the playlist from Spotify.
2. Looks up each song on Apple Music — first by ISRC in one batch, then by
   search for anything the ISRC pass missed.
3. Auto-accepts confident matches, and shows you the rest.
4. Creates the playlist and adds the songs.

### The review screen

Anything rocola isn't sure about goes to a review list. The source track sits
at the top; the candidates are numbered below it, with every field that
**disagrees** with the source in bold, so your eye goes straight to the
difference.

| Key      | What it does                      |
| -------- | --------------------------------- |
| ↑ ↓      | Move between tracks               |
| 1–9      | Pick that candidate               |
| s        | Skip this track                   |
| enter    | Confirm, once every track is decided |
| q or esc | Abort — nothing is created       |

Then a confirm screen shows the counts before anything is written. Press
`enter` to create the playlist, or `q` to walk away.

### Nothing is dropped silently

When rocola finishes, it prints every track that did **not** make it into the
playlist, under its own heading: the ones you skipped, the ones it couldn't
find on Apple Music, and the local files, podcast episodes and removed tracks
that can't exist in Apple's catalogue at all. Added + skipped + not found +
local, podcast and removed accounts for every item in the Spotify playlist.

"Added" means Apple accepted it: rocola counts a song as added when Apple takes
the batch it was in. It does not yet read the playlist back to confirm what
landed. If Apple refuses a batch part-way through, rocola says how many songs
went in before it stopped.

### Running it twice

If a playlist with that name already exists in your library, rocola stops and
asks. Say yes and it creates `<name> (rocola)` instead — or `<name> (rocola 2)`,
`(rocola 3)` and so on if those are taken too. It never adds to a playlist that
already exists, and it never quietly makes a duplicate.

## What rocola can't do

- **Read playlists you don't own.** Since Spotify's February 2026 change,
  a development-mode app can only read playlists the signed-in user owns or
  collaborates on. Everything else comes back as "not yours".
  **The workaround:** open the playlist in Spotify, use
  **Add to other playlist** to copy it into a playlist of your own, then run
  rocola on your copy.
- **Read Spotify's own playlists.** Discover Weekly, Release Radar, editorial
  mixes — Spotify blocks these outright, and copying them into your library is
  again the way round it.
- **Find songs that aren't in your country's Apple Music catalogue.** Catalogues
  differ by storefront. Anything missing is reported by name at the end, never
  dropped in silence.
- **Add to an existing Apple Music playlist.** Not yet.
- **Copy local files or podcasts.** Songs you added to a Spotify playlist from
  your own computer, podcast episodes, and tracks Spotify has since removed,
  don't exist in Apple Music's song catalogue. rocola says how many it is leaving out as soon as it reads the
  playlist, and names every one of them at the end.

## Where your credentials live

Everything rocola remembers is in one file:

```
~/.config/rocola/config.toml    (mode 0600 — only you can read it)
```

It holds:

| What                    | What it's for                                    |
| ----------------------- | ------------------------------------------------ |
| Spotify Client ID       | Identifies your developer app                    |
| Spotify refresh token   | So you only sign in to Spotify once              |
| Apple Team ID, Key ID   | Identify your Apple developer account and key    |
| Path to your `.p8` file | rocola reads the key, it never copies or moves it |
| Music User Token        | So you only sign in to Apple Music once          |
| Apple storefront        | Your country's catalogue, e.g. `gb`              |

Short-lived credentials never touch the disk. The Spotify access token and the
Apple developer token (a 12-hour JWT, signed fresh on every run) live in memory
only.

Your `.p8` private key stays exactly where you put it. rocola reads it and
warns you if it is sitting inside a git working tree, because anyone who can
read that repository could sign tokens as your Apple developer account.

To make rocola forget everything, delete the config file.

## Contributing

**You do not need any credentials to contribute.** The matching engine and its
test corpus run against fixtures, with no calls to Spotify or Apple:

```sh
just ci     # format check, clippy (pedantic + nursery, warnings as errors), tests
```

That is the same thing CI runs. It needs no credentials and makes no calls to
Spotify or Apple — only cargo reaching out for crates on a fresh checkout.

### The easiest useful contribution

`rocola-core/tests/fixtures/corpus.json` is a golden corpus of real-world
matching cases — remasters, live versions, covers, non-Latin scripts, classical
works, collaborations, interludes. Each case is one Spotify track, the Apple
Music candidates it might be matched against, and the answer the engine should
give (`Exact`, `High`, `Ambiguous` or `NotFound`).

**If rocola matched one of your songs wrongly, add the case.** It is a JSON
file — no Rust needed. `cargo test -p rocola-core` runs every case and lists
all the failures at once.

### The crates

| Crate            | What it does                                       | Needs network |
| ---------------- | -------------------------------------------------- | ------------- |
| `rocola-core`    | Types, normalisation, scoring, the match pipeline   | no            |
| `rocola-spotify` | Spotify auth (PKCE) and playlist fetch              | yes           |
| `rocola-apple`   | Apple developer token, MusicKit sign-in, catalogue and playlist calls | yes |
| `rocola`         | Config, the TUI, and the flow that joins it all up  | yes           |

The scoring rules live in `rocola-core/src/matching.rs`: title 40, artist
overlap 30, duration 20 (within 3 seconds) or 10 (within 10 seconds), album 10.
85 or more is a confident match, 50 or more goes to review, below that is "not
found". An ISRC hit within 3 seconds of the source is trusted outright.

## For maintainers

Everyday recipes (`just` with no arguments lists them all):

| Recipe                  | What it does                                        |
| ----------------------- | --------------------------------------------------- |
| `just ci`               | Everything CI runs — no credentials needed           |
| `just run <url>`        | Run the TUI from the working tree                   |
| `just hurl-spotify`     | Live read-only Spotify check. Also runs in this repo's CI from repo secrets, so it shows red (harmlessly) on pull requests from forks |
| `just hurl-apple`       | Live Apple Music checks. Local only — they need your own Apple credentials |
| `just secrets-decrypt`  | `tests/hurl/vars.sops.env` → `vars.env` (needs the GPG key) |
| `just secrets-encrypt`  | `vars.env` → `vars.sops.env` after editing credentials |

The hurl checks read their credentials from `tests/hurl/vars.env`, which is
git-ignored; `tests/hurl/vars.env.example` shows the shape. Maintainers keep
the real values in `tests/hurl/vars.sops.env`, encrypted with sops to the key
in `.sops.yaml`.

To mint a fresh Spotify token pair for those checks:

```sh
cargo run -p rocola-spotify --example mint -- <your spotify client id>
```

It opens the browser once and prints the lines to paste into `vars.env`.

`probes/musickit-loopback/` is the standalone page that answered the question
the whole Apple design rests on: **does MusicKit JS authorise from
`http://127.0.0.1`?** Its README has the four steps. Re-run it if a browser
changes its secure-context rules.

## Why "rocola"

It is the word for a jukebox across much of Latin America.

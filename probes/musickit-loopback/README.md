# M0 probe: does MusicKit JS authorise from http://127.0.0.1?

Answers spec Risk 2. Needs an Apple Developer membership.

1. Mint a short-lived developer token (any tool; 1h expiry is fine).
2. `python3 -m http.server 8899 --bind 127.0.0.1` from this directory.
3. Open `http://127.0.0.1:8899/index.html?devtoken=<JWT>` in Safari, Chrome, Firefox.
4. Click the button, sign in.

Record PASS/FAIL per browser in `docs/design/2026-08-29-spotify-to-apple-music.md` §Risks 2.
PASS in at least one mainstream browser = design holds.
FAIL everywhere = STOP; the Apple auth flow needs a redesign (likely mkcert + https) before Tasks 8–11 are valid.

# List available recipes
default:
    @just --list

# Type-check the whole workspace
check:
    cargo check --workspace

# Build all crates, or one: `just build rocola-core`
build crate='':
    cargo build {{ if crate == '' { '--workspace' } else { '-p ' + crate } }}

# Test all crates, or one: `just test rocola-core`
test crate='':
    cargo test {{ if crate == '' { '--workspace' } else { '-p ' + crate } }}

# Lint all crates (warnings as errors), or one: `just lint rocola-core`
lint crate='':
    cargo clippy {{ if crate == '' { '--workspace' } else { '-p ' + crate } }} --all-targets -- -D warnings

# Format all crates
fmt:
    cargo fmt

# Everything CI would run: format check, lint, tests
ci:
    cargo fmt --check
    just lint
    just test

# Run the TUI: `just run <spotify playlist url>`
run *args:
    cargo run -p rocola -- {{args}}

# Live read-only Spotify check. Needs tests/hurl/vars.env — see vars.env.example. CI runs this one from repo secrets, so it shows red on fork pull requests; `just ci` needs no credentials.
hurl-spotify:
    hurl --variables-file tests/hurl/vars.env --test tests/hurl/spotify_playlist.hurl

# Live Spotify refresh-grant check (verified: Spotify does not rotate the refresh token on use).
hurl-spotify-auth:
    hurl --variables-file tests/hurl/vars.env --test tests/hurl/spotify_refresh.hurl

# Live Apple Music checks — local only, never in CI. Needs apple_dev_token/apple_user_token/apple_storefront in tests/hurl/vars.env.
hurl-apple:
    hurl --variables-file tests/hurl/vars.env --test tests/hurl/apple_storefront.hurl tests/hurl/apple_isrc.hurl tests/hurl/apple_search.hurl

# Decrypt tests/hurl/vars.sops.env -> vars.env (needs your GPG key)
secrets-decrypt:
    sops -d tests/hurl/vars.sops.env > tests/hurl/vars.env

# Re-encrypt tests/hurl/vars.env -> vars.sops.env after editing credentials
secrets-encrypt:
    sops -e tests/hurl/vars.env > tests/hurl/vars.sops.env

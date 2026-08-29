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

# Run the TUI
run:
    cargo run -p rocola

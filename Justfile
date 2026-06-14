default:
    @just --list

# Full CI gate: format, lint, test, build
ci: fmt-check clippy test build

# Format the code
fmt:
    cargo fmt

# Check formatting without modifying
fmt-check:
    cargo fmt --check

# Lint with no warnings tolerated
clippy:
    cargo clippy --all-targets -- -D warnings

# Run all tests (including golden tests)
test:
    cargo test

# Release build
build:
    cargo build --release

# Build a statically-linked (musl) binary via Nix, at ./result/bin/flowghetti
build-static:
    nix build .#static
    @echo "static binary: result/bin/flowghetti"

# Regenerate golden test files (review the diff afterwards)
bless:
    BLESS=1 cargo test --test golden

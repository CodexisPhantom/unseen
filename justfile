set windows-shell := ["pwsh.exe", "-NoLogo", "-NoProfile", "-Command"]

# List the recipes
default:
    @just --list

# Run the API
dev:
    cargo run -p api

# Run the Vite dev server
web-dev:
    pnpm --dir web dev

# Check formatting and lints; warnings are errors
check:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    pnpm --dir web check
    pnpm --dir web lint

# Run all tests
test:
    cargo test --workspace
    pnpm --dir web test

# Run the Rust tests with cargo-llvm-cov; fail below the minimums in docs/12-testing.md
coverage:
    cargo llvm-cov --workspace --ignore-filename-regex "(auth_schema|main)\.rs$" --fail-under-lines 80
    cargo llvm-cov --package core --fail-under-lines 90

# Build the frontend into web/build
web-build:
    pnpm --dir web build

# Build the frontend, then the release binary
build: web-build
    cargo build --workspace --release

# Regenerate crates/db/src/auth_schema.rs with the Better Auth CLI
gen-auth:
    better-auth-rs generate --output crates/db/src/auth_schema.rs --force

# Regenerate openapi.json and the TypeScript types in web/src/lib/api
gen:
    cargo run -q -p api -- openapi openapi.json
    pnpm --dir web gen

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

# Run all tests
test:
    cargo test --workspace

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

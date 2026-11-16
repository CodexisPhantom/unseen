set windows-shell := ["pwsh.exe", "-NoLogo", "-NoProfile", "-Command"]

# List the recipes
default:
    @just --list

# Run the API
dev:
    cargo run -p api

# Check formatting and lints; warnings are errors
check:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings

# Run all tests
test:
    cargo test --workspace

# Build the release binary
build:
    cargo build --workspace --release

# Regenerate crates/db/src/auth_schema.rs with the Better Auth CLI
gen-auth:
    better-auth-rs generate --output crates/db/src/auth_schema.rs --force

# Regenerate openapi.json and the TypeScript client (stub until the OpenAPI issue)
gen:
    @echo "gen: nothing to generate yet"

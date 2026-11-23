# syntax=docker/dockerfile:1

# 1. Frontend: static files in /web/build.
FROM node:24-slim AS web
RUN npm install --global pnpm@12
WORKDIR /web
COPY web/package.json web/pnpm-lock.yaml web/pnpm-workspace.yaml ./
RUN pnpm install --frozen-lockfile
COPY web/ ./
RUN pnpm build

# 2. Binary. Ubuntu 26.04 has the OpenSSL 3 and ICU 78 libraries that the
# auth library links (ADR 0006); the runtime stage must use the same release.
FROM ubuntu:26.04 AS chef
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates curl build-essential pkg-config libssl-dev libicu-dev \
    && rm -rf /var/lib/apt/lists/*
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
ENV PATH=/root/.cargo/bin:$PATH
RUN cargo install cargo-chef --version 0.1.78 --locked
WORKDIR /src

# The recipe lists only the dependencies, so the layer below that builds
# them stays cached until Cargo.toml or Cargo.lock change. No cache mount
# on target: layer cache (exported to CI) is what keeps the build.
FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY crates crates
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS build
COPY .cargo .cargo
COPY --from=planner /src/recipe.json recipe.json
RUN cargo chef cook --release --locked -p api --recipe-path recipe.json
COPY Cargo.toml Cargo.lock ./
COPY crates crates
# rust-embed reads the frontend from web/build at compile time.
COPY --from=web /web/build web/build
RUN cargo build --release --locked -p api \
    && cp target/release/unseen /unseen

# 3. Runtime.
FROM ubuntu:26.04
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates curl libssl3t64 libicu78 tzdata \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /unseen /usr/local/bin/unseen
COPY docker/entrypoint.sh /entrypoint.sh
ENV PUID=1000 PGID=1000 UMASK=022 TZ=UTC
VOLUME /config
EXPOSE 7979
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s \
    CMD curl -fsS http://localhost:7979/api/v1/health || exit 1
ENTRYPOINT ["/entrypoint.sh"]

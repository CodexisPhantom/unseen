# Linux build environment for the spike (check 1).
FROM ubuntu:26.04
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl build-essential pkg-config libssl-dev libicu-dev \
    && rm -rf /var/lib/apt/lists/*
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
ENV PATH=/root/.cargo/bin:$PATH RUST_ICU_MAJOR_VERSION_NUMBER=78 CARGO_TARGET_DIR=/target
WORKDIR /spike

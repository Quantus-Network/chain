# syntax=docker/dockerfile:1

############################
# 1️⃣  Builder stage
############################
FROM rust:bookworm AS builder

# Build with the nightly toolchain you already use in CI
ARG RUST_TOOLCHAIN=nightly-2024-12-24

# Optional: non-root build user
ARG USER=quantus
ARG UID=10001
RUN adduser --disabled-password --gecos "" --uid ${UID} ${USER}

WORKDIR /usr/src/quantus

# Install build dependencies for Substrate projects
RUN apt-get update && apt-get install -y \
    protobuf-compiler \
    libclang-dev \
    clang \
    build-essential \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Install the pinned nightly + components and targets
RUN rustup toolchain install ${RUST_TOOLCHAIN} --profile minimal \
 && rustup component add rustfmt clippy rust-src --toolchain ${RUST_TOOLCHAIN} \
 && rustup target add wasm32-unknown-unknown --toolchain ${RUST_TOOLCHAIN} \
 && rustup default ${RUST_TOOLCHAIN}

# Copy the entire workspace (we'll rely on .dockerignore to filter)
COPY . .

# Build only the quantus-node binary
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo +${RUST_TOOLCHAIN} build --release --locked --package quantus-node

############################
# 2️⃣  Runtime stage
############################
FROM debian:bookworm-slim AS runtime

RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*

# ⬇️ copy the statically linked binary we just built
COPY --from=builder /usr/src/quantus/target/release/quantus-node /usr/local/bin/quantus-node

# Expose P2P and public WS/RPC ports
EXPOSE 30333 9944

# Run as an unprivileged user (same UID as builder)
USER 10001:10001

# Start the node on the main chain by default
ENTRYPOINT ["quantus-node"]
CMD ["--chain", "live_resonance"]
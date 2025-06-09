# syntax=docker/dockerfile:1

############################
# Runtime-only stage using pre-built binary
############################
FROM ubuntu:24.04

# Install runtime dependencies
RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    libterm-readline-perl-perl \
 && rm -rf /var/lib/apt/lists/*

# Download the specified pre-built binary from GitHub releases
ARG VERSION_ARG # Expecting format like vX.Y.Z
RUN ARCH="x86_64-unknown-linux-gnu" \
 && echo "Attempting to download version: ${VERSION_ARG} for architecture ${ARCH}" \
 && curl -fsSL "https://github.com/Quantus-Network/chain/releases/download/${VERSION_ARG}/quantus-node-${VERSION_ARG}-${ARCH}.tar.gz" \
    | tar -xzC /usr/local/bin/ \
 && chmod +x /usr/local/bin/quantus-node

# Expose P2P and public WS/RPC ports
EXPOSE 30333 9944

# Run as unprivileged user
RUN useradd --system --uid 10001 quantus
USER 10001:10001

# Start the node
ENTRYPOINT ["quantus-node"]
CMD ["--chain", "live_resonance"]

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

# Download the specified pre-built binary from GitHub releases.
# Token is a BuildKit secret (not an ARG) so it never lands in image layers.
ARG VERSION_ARG # Expecting format like vX.Y.Z
RUN --mount=type=secret,id=github_token,required=true \
    set -eu; \
    DPKG_ARCH="$(dpkg --print-architecture)"; \
    case "$DPKG_ARCH" in \
        amd64)   ARCH="x86_64-unknown-linux-gnu" ;; \
        arm64)   ARCH="aarch64-unknown-linux-gnu" ;; \
        *) echo "Unsupported architecture: $DPKG_ARCH" && exit 1 ;; \
    esac; \
    echo "Downloading version: ${VERSION_ARG} for architecture: ${ARCH}"; \
    TOKEN="$(cat /run/secrets/github_token)"; \
    if [ -z "$TOKEN" ]; then \
        echo "github_token secret is empty" >&2; \
        exit 1; \
    fi; \
    curl -fsSL \
        -H "Authorization: Bearer ${TOKEN}" \
        -H "Accept: application/octet-stream" \
        "https://github.com/Quantus-Network/chain-private/releases/download/${VERSION_ARG}/quantus-node-${VERSION_ARG}-${ARCH}.tar.gz" \
        | tar -xzC /usr/local/bin/; \
    chmod +x /usr/local/bin/quantus-node

# Expose P2P and public WS/RPC ports
EXPOSE 30333 9944

# Run as unprivileged user
RUN useradd --system --uid 10001 quantus
USER 10001:10001

# Start the node
ENTRYPOINT ["quantus-node"]
CMD ["--chain", "planck_live_spec"]
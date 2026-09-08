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

# The workflow downloads the release tarball for each platform into dist/<arch>/;
# the repository is private, so the image build itself cannot fetch it.
ARG TARGETARCH
COPY dist/${TARGETARCH}/ /usr/local/bin/
RUN chmod +x /usr/local/bin/quantus-node

# Expose P2P and public WS/RPC ports
EXPOSE 30333 9944

# Run as unprivileged user
RUN useradd --system --uid 10001 quantus
USER 10001:10001

# Start the node
ENTRYPOINT ["quantus-node"]
CMD ["--chain", "planck_live_spec"]
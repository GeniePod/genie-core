# GeniePod — multi-stage Docker build
#
# Stage 1: Build Rust binaries (~5 min with cache)
# Stage 2: Minimal runtime image (~20 MB total)
#
# Usage:
#   docker build -t geniepod ./software
#   docker run -p 3000:3000 -p 3080:3080 geniepod
#
# For Jetson (aarch64), use: docker buildx build --platform linux/arm64 ...

# ── Stage 1: Build ──────────────────────────────────────────────
FROM rust:1.90-slim-bookworm AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/

# Build release binaries.
RUN cargo build --release \
    && strip target/release/genie-core \
    && strip target/release/genie-governor \
    && strip target/release/genie-health \
    && strip target/release/genie-api

# ── Stage 2: Runtime ────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    alsa-utils \
    && rm -rf /var/lib/apt/lists/*

# Create geniepod user.
RUN useradd -r -s /bin/false -m -d /opt/geniepod geniepod

# Copy binaries.
COPY --from=builder /build/target/release/genie-core /opt/geniepod/bin/
COPY --from=builder /build/target/release/genie-governor /opt/geniepod/bin/
COPY --from=builder /build/target/release/genie-health /opt/geniepod/bin/
COPY --from=builder /build/target/release/genie-api /opt/geniepod/bin/

# Copy config.
COPY deploy/config/ /etc/geniepod/

# Create data directories.
RUN mkdir -p /opt/geniepod/data /opt/geniepod/models /run/geniepod \
    && chown -R geniepod:geniepod /opt/geniepod /run/geniepod

ENV GENIEPOD_CONFIG=/etc/geniepod/geniepod.toml
ENV RUST_LOG=info

USER geniepod
WORKDIR /opt/geniepod

# Default: run genie-core (chat API on :3000).
EXPOSE 3000 3080
CMD ["/opt/geniepod/bin/genie-core"]

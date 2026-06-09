# ---------------------------------------------------------------------------
# Stage 1: Build the `oracles` binary
# ---------------------------------------------------------------------------
ARG RUST_VERSION=1.96
FROM rust:${RUST_VERSION}-slim-bookworm AS builder

WORKDIR /app

# Install native build dependencies.
#
# pkg-config/libssl-dev are needed for native-tls/OpenSSL.
# libsqlite3-dev is needed by rusqlite when not using bundled SQLite.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential \
        pkg-config \
        libssl-dev \
        libsqlite3-dev \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy manifests first for better dependency caching.
#
# This project builds a service/binary, so Cargo.lock should be committed.
# Keep --locked for reproducible Docker builds.
COPY Cargo.toml Cargo.lock ./

# Build dependencies using a temporary minimal crate.
RUN mkdir -p src \
    && printf 'fn main() {}\n' > src/main.rs \
    && printf '' > src/lib.rs \
    && cargo build --release --locked --features "full" \
    && rm -rf src

# Copy actual source and build the real binary.
COPY src/ src/

RUN touch src/main.rs src/lib.rs \
    && cargo build --release --locked --features "full" \
    && install -m 0755 target/release/oracles /usr/local/bin/oracles

# ---------------------------------------------------------------------------
# Stage 2: Runtime image
# ---------------------------------------------------------------------------
FROM debian:bookworm-slim AS runtime

# Runtime dependencies:
# - ca-certificates: HTTPS providers/webhooks/Telegram
# - libssl3: native-tls/OpenSSL runtime
# - libsqlite3-0: SQLite runtime library
# - tini: clean signal forwarding/reaping
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        libssl3 \
        libsqlite3-0 \
        tini \
    && rm -rf /var/lib/apt/lists/*

# Create an unprivileged runtime user.
RUN groupadd --system oracles \
    && useradd \
        --system \
        --gid oracles \
        --home-dir /nonexistent \
        --shell /usr/sbin/nologin \
        oracles

# Runtime directories.
RUN mkdir -p /data /etc/oracles \
    && chown -R oracles:oracles /data /etc/oracles

COPY --from=builder /usr/local/bin/oracles /usr/local/bin/oracles

USER oracles:oracles

VOLUME ["/data"]

ENTRYPOINT ["/usr/bin/tini", "--", "oracles"]
CMD ["--config", "/etc/oracles/Config.toml"]

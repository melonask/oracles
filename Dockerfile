# ---------------------------------------------------------------------------
# Stage 1: Build the `oracles` binary
# ---------------------------------------------------------------------------
ARG RUST_VERSION=1.97
FROM rust:${RUST_VERSION}-slim-bookworm AS builder

WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential \
        pkg-config \
        libssl-dev \
        libsqlite3-dev \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./

RUN mkdir -p src \
    && printf 'fn main() {}\n' > src/main.rs \
    && printf '' > src/lib.rs \
    && cargo build --release --locked --features "full" --bin oracles \
    && rm -rf src

COPY src/ src/

RUN touch src/main.rs src/lib.rs \
    && cargo build --release --locked --features "full" --bin oracles \
    && install -m 0755 target/release/oracles /usr/local/bin/oracles

# ---------------------------------------------------------------------------
# Stage 2: Runtime image
# ---------------------------------------------------------------------------
FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        libssl3 \
        libsqlite3-0 \
        tini \
    && rm -rf /var/lib/apt/lists/*

RUN groupadd --system oracles \
    && useradd \
        --system \
        --gid oracles \
        --home-dir /nonexistent \
        --shell /usr/sbin/nologin \
        oracles

RUN mkdir -p /data /etc/oracles \
    && chown -R oracles:oracles /data /etc/oracles

COPY --from=builder /usr/local/bin/oracles /usr/local/bin/oracles

LABEL org.opencontainers.image.title="Oracles" \
      org.opencontainers.image.description="Stateless cryptocurrency rate oracle service/library" \
      org.opencontainers.image.licenses="MIT" \
      org.opencontainers.image.source="https://github.com/melonask/oracles"

USER oracles:oracles

VOLUME ["/data"]

HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
    CMD ["oracles", "ping"]

ENTRYPOINT ["/usr/bin/tini", "--", "oracles"]
CMD ["--config", "/etc/oracles/Config.toml"]

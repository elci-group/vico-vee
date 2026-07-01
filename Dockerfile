# Multi-stage build for the standalone vico-vee service.
# Uses Debian Slim as the runtime base for glibc compatibility.

FROM rust:1.79-slim-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    libsqlite3-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/vico-vee

# Copy workspace manifests first to cache dependency compilation.
COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release --bin vico-vee

# ─────────────────────────────────────────────────────────────────────────────
# Runtime image
# ─────────────────────────────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libsqlite3-0 \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd -r vico-vee \
    && useradd -r -g vico-vee -d /var/lib/vico-vee -s /sbin/nologin vico-vee

COPY --from=builder /usr/src/vico-vee/target/release/vico-vee /usr/local/bin/vico-vee
COPY migrations /usr/share/vico-vee/migrations

RUN mkdir -p /var/lib/vico-vee /etc/vico-vee \
    && chown -R vico-vee:vico-vee /var/lib/vico-vee /etc/vico-vee

USER vico-vee

ENV VICO_VEE_DATA_DIR=/var/lib/vico-vee
ENV VICO_VEE_CONFIG_DIR=/etc/vico-vee
ENV VICO_VEE_PORT=9987
ENV VICO_VEE_BIND=0.0.0.0

EXPOSE 9987

HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
    CMD vico-vee --help >/dev/null || exit 1

ENTRYPOINT ["/usr/local/bin/vico-vee"]
CMD ["--bind", "0.0.0.0"]

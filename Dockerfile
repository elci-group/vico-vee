# Multi-stage build for the vico-vee standalone service.
#
# Produces a small image containing only the statically linked binary and the
# embedded migration files.

FROM rust:1.79-slim-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    libdbus-1-dev \
    pkg-config \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/vico-vee

# Copy manifest and fetch dependencies first to leverage Docker layer caching.
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src && echo 'fn main() {}' > src/main.rs \
    && cargo build --release --bin vico-vee \
    && rm -rf src

# Copy the full source tree and build the real binary.
COPY . .
RUN touch src/main.rs && cargo build --release --bin vico-vee

# ─────────────────────────────────────────────────────────────────────────────
# Runtime image
# ─────────────────────────────────────────────────────────────────────────────
FROM gcr.io/distroless/cc-debian12:nonroot

# Copy the compiled binary.
COPY --from=builder /usr/src/vico-vee/target/release/vico-vee /usr/local/bin/vico-vee

# Expose the default HTTP port.
EXPOSE 9987

# Data and config volumes.
VOLUME ["/data", "/config"]

ENV VICO_VEE_DATA_DIR=/data
ENV VICO_VEE_CONFIG_DIR=/config
ENV VICO_VEE_BIND=0.0.0.0
ENV VICO_VEE_PORT=9987

ENTRYPOINT ["/usr/local/bin/vico-vee"]
CMD ["--bind", "0.0.0.0"]

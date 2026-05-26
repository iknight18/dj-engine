# syntax=docker/dockerfile:1
FROM rust:1.79-slim AS builder
WORKDIR /build
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY engine/Cargo.toml engine/Cargo.lock /build/engine/
COPY engine/src /build/engine/src
COPY engine/tests /build/engine/tests
COPY schema /build/schema
WORKDIR /build/engine
RUN cargo build --release --bin dj-engine

FROM debian:stable-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
    ffmpeg ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/engine/target/release/dj-engine /usr/local/bin/dj-engine
ENTRYPOINT ["/usr/local/bin/dj-engine"]
CMD ["--help"]

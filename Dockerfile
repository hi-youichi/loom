# syntax=docker/dockerfile:1
FROM rust:bookworm AS builder

WORKDIR /build

COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release -p cli --bin loom \
    && cp /build/target/release/loom /build/loom

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /build/loom /app/loom

ENTRYPOINT ["/app/loom"]

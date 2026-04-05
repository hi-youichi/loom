# syntax=docker/dockerfile:1
FROM rust:bookworm AS builder

WORKDIR /build

COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release -p telegram-bot --bin telegram-bot \
    && cp /build/target/release/telegram-bot /build/telegram-bot-bin

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /build/telegram-bot-bin /app/telegram-bot

ENTRYPOINT ["/app/telegram-bot"]

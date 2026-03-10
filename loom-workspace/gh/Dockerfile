# gh-webhook — build from repo root: docker build -f loom-workspace/gh/Dockerfile .

# -----------------------------------------------------------------------------
# Stage 1: build
# -----------------------------------------------------------------------------
FROM rust:bookworm AS builder

WORKDIR /build

# Copy full workspace (needed: loom, config, stream-event, loom-workspace, gh)
COPY . .

# Build release binary for gh-webhook only
RUN cargo build --release -p gh --bin gh-webhook

# -----------------------------------------------------------------------------
# Stage 2: runtime
# -----------------------------------------------------------------------------
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /build/target/release/gh-webhook /app/gh-webhook

EXPOSE 8080
ENV GH_WEBHOOK_BIND=0.0.0.0
ENV GH_WEBHOOK_PORT=8080

ENTRYPOINT ["/app/gh-webhook"]
CMD ["--bind", "0.0.0.0", "--port", "8080"]

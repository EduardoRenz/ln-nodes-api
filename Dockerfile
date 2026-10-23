FROM rust:1.99-slim-bookworm AS builder

WORKDIR /app

# aws-lc-sys (pulled in by rustls) needs cmake to build on Debian slim.
RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
COPY migrations-sqlite ./migrations-sqlite

RUN cargo build --release --locked

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --system --create-home --uid 10001 app

COPY --from=builder /app/target/release/rust-lightning-service /usr/local/bin/rust-lightning-service

USER app
EXPOSE 3000

HEALTHCHECK --interval=10s --timeout=3s --start-period=15s --retries=5 \
    CMD curl -fsS http://127.0.0.1:3000/health || exit 1

ENTRYPOINT ["rust-lightning-service"]

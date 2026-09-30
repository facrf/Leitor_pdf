FROM rust:1-bookworm AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY tests/rust ./tests/rust
RUN cargo test --release --locked && cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates poppler-utils \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 estante \
    && useradd --uid 10001 --gid 10001 --no-create-home --shell /usr/sbin/nologin estante
WORKDIR /app
COPY --from=builder /build/target/release/estante-livre /usr/local/bin/estante-livre
COPY web ./web
RUN mkdir -p /data /pdf && chown -R estante:estante /data /pdf
ENV APP_BIND=0.0.0.0:20000 \
    LIBRARY_ROOT=/pdf \
    DATABASE_PATH=/data/library.db \
    COVERS_DIR=/data/covers \
    BRANDING_DIR=/data/branding \
    BACKUP_DIR=/data/backups \
    RUST_LOG=estante_livre=info,tower_http=info
EXPOSE 20000
VOLUME ["/data", "/pdf"]
USER estante
CMD ["estante-livre"]

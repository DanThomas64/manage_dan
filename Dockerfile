# syntax=docker/dockerfile:1
# ── Stage 1: Build ────────────────────────────────────────────────────────────
FROM rust:1.87-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config \
        libudev-dev \
        libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

COPY Cargo.toml Cargo.lock ./
COPY app/        app/
COPY db/         db/
COPY log/        log/
COPY notes/      notes/
COPY project/    project/
COPY printer/    printer/
COPY shopping/   shopping/
COPY todo/       todo/
COPY tui/        tui/
COPY vikunja/    vikunja/

RUN cargo build --release -p app

# ── Stage 2: Runtime ───────────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/app /usr/local/bin/manage

# Working directory — app.sqlite is created here at runtime.
WORKDIR /data

# Bake in the default config.  Override specific values with APP_* env vars
# or bind-mount a config/local.toml at /data/config/local.toml.
COPY config/default.toml /data/config/default.toml

EXPOSE 8080

CMD ["/usr/local/bin/manage"]

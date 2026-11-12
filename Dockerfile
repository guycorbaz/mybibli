# Story 18-2 (#506) — every stage is pinned to an exact tag, all on the
# same Alpine minor, so two builds of one commit produce the same image.
# The Rust version matches rust-toolchain.toml and the CI toolchain, the
# Node major the CI's node-version (tests/toolchain_pins.rs checks both).
# Bump them deliberately, together; see CLAUDE.md "Where versions live".

# Stage 1: Build Rust binary
FROM rust:1.99.1-alpine3.24 AS builder
RUN apk add musl-dev pkgconf openssl-dev openssl-libs-static
WORKDIR /app
COPY Cargo.toml Cargo.lock* ./
COPY src/ ./src/
COPY migrations/ ./migrations/
COPY locales/ ./locales/
COPY templates/ ./templates/
COPY .sqlx/ ./.sqlx/
# Story 17-6: default MCP skills, embedded in the binary with include_str!
COPY skills/ ./skills/
ENV SQLX_OFFLINE=true
# #447 — the build's git commit, so the running binary can name itself in the
# log. `.git` is deliberately NOT copied into the build context (it would
# invalidate the layer cache on every commit and bloat the context), so the
# value arrives as a build argument. Absent -> the binary reports "unknown",
# which is the honest answer for a local build.
ARG MYBIBLI_BUILD_SHA
ENV MYBIBLI_BUILD_SHA=$MYBIBLI_BUILD_SHA
RUN cargo build --release --target x86_64-unknown-linux-musl

# Stage 2: Generate CSS
FROM node:24.22.3-alpine3.24 AS css
WORKDIR /app
# #506 — Tailwind at the exact version of the tracked root package.json
# + package-lock.json, never "whatever npm resolves today".
COPY package.json package-lock.json ./
RUN npm ci
COPY static/css/input.css ./static/css/
COPY templates/ ./templates/
# Story 14-1 — the first-party JS modules build class strings too (the
# session and connection toasts): Tailwind must see them, as it does in a
# local build run from the repo root. Sourced explicitly in input.css.
COPY static/js/ ./static/js/
RUN npx @tailwindcss/cli -i static/css/input.css -o static/css/output.css --minify

# Stage 3: Runtime
FROM alpine:3.24.4
RUN apk add --no-cache ca-certificates
# CR #301 (v1.7.0): pre-create the log directory inside the image so a
# fresh deployment doesn't need `mkdir -p` permission at runtime — the
# tracing-appender writer fails-open and silently falls back to
# stdout-only if it can't create the dir, but pre-creating gives us
# correct ownership + visibility from `docker exec` even before the
# named volume is mounted.
RUN mkdir -p /var/log/mybibli && chmod 0755 /var/log/mybibli
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/mybibli /usr/local/bin/
COPY --from=css /app/static/css/output.css /app/static/css/output.css
COPY static/css/browse.css /app/static/css/browse.css
COPY static/js/ /app/static/js/
COPY static/icons/ /app/static/icons/
# v1.5.1 fix #281: DejaVu Sans TTFs are required by the wishlist
# PDF route (CR #266); shipping them was missed in the v1.5.0
# Dockerfile, which crashed `/wishlist/export.pdf` with a generic
# 500 because `genpdf::fonts::from_files("static/fonts", …)` failed
# to open the missing files.
COPY static/fonts/ /app/static/fonts/
COPY locales/ /app/locales/
COPY migrations/ /app/migrations/
# Logo assets — served at /logo/* via the ServeDir mount in
# src/routes/mod.rs. Includes SVG icon, light/dark wordmarks, PNG
# multi-size set, and favicon.ico.
COPY docs/mybibli-logo/ /app/docs/mybibli-logo/
WORKDIR /app
EXPOSE 8080
CMD ["mybibli"]

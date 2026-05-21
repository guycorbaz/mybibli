# Stage 1: Build Rust binary
FROM rust:alpine AS builder
RUN apk add musl-dev pkgconf openssl-dev openssl-libs-static
WORKDIR /app
COPY Cargo.toml Cargo.lock* ./
COPY src/ ./src/
COPY migrations/ ./migrations/
COPY locales/ ./locales/
COPY templates/ ./templates/
COPY .sqlx/ ./.sqlx/
ENV SQLX_OFFLINE=true
RUN cargo build --release --target x86_64-unknown-linux-musl

# Stage 2: Generate CSS
FROM node:alpine AS css
WORKDIR /app
RUN npm install tailwindcss @tailwindcss/cli
COPY static/css/input.css ./static/css/
COPY templates/ ./templates/
RUN npx @tailwindcss/cli -i static/css/input.css -o static/css/output.css --minify

# Stage 3: Runtime
FROM alpine:latest
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

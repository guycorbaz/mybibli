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
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/mybibli /usr/local/bin/
COPY --from=css /app/static/css/output.css /app/static/css/output.css
COPY static/css/browse.css /app/static/css/browse.css
COPY static/js/ /app/static/js/
COPY static/icons/ /app/static/icons/
COPY locales/ /app/locales/
COPY migrations/ /app/migrations/
WORKDIR /app
EXPOSE 8080
CMD ["mybibli"]

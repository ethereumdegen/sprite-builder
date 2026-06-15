# --- API server image (also bundles the built frontend) ---

# 1. Build the frontend
FROM node:20-slim AS frontend
WORKDIR /app/frontend
COPY frontend/package.json frontend/package-lock.json* ./
RUN npm install
COPY frontend/ ./
RUN npm run build

# 2. Build the Rust server
FROM rust:1-slim AS backend
WORKDIR /app/backend
# Compile the sqlx query! macros against the committed offline cache (.sqlx),
# since there's no database at image-build time (ADR 0002).
ENV SQLX_OFFLINE=true
RUN apt-get update && apt-get install -y pkg-config build-essential && rm -rf /var/lib/apt/lists/*
COPY backend/ ./
RUN cargo build --release --bin sprite-builder

# 3. Runtime
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=backend /app/backend/target/release/sprite-builder /usr/local/bin/sprite-builder
COPY --from=backend /app/backend/migrations /app/migrations
COPY --from=frontend /app/frontend/dist /app/static
ENV STATIC_DIR=/app/static
# The server honors $PORT first (Railway injects it), then BIND_ADDR. This is
# the fallback when PORT is unset; on Railway PORT (e.g. 8080) takes precedence.
ENV BIND_ADDR=0.0.0.0:8080
EXPOSE 8080
CMD ["sprite-builder"]

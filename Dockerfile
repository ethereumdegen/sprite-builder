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
RUN apt-get update && apt-get install -y pkg-config && rm -rf /var/lib/apt/lists/*
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
ENV BIND_ADDR=0.0.0.0:8787
EXPOSE 8787
CMD ["sprite-builder"]

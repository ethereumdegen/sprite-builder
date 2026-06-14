# --- Background build worker image ---
FROM rust:1-slim AS build
WORKDIR /app/backend
RUN apt-get update && apt-get install -y pkg-config build-essential && rm -rf /var/lib/apt/lists/*
COPY backend/ ./
RUN cargo build --release --bin sprite-builder-worker

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /app/backend/target/release/sprite-builder-worker /usr/local/bin/sprite-builder-worker
# The worker runs a small liveness server (see worker/main.rs) for platform
# health checks. It honors $PORT first (Railway injects it), then BIND_ADDR;
# this is the fallback when PORT is unset.
ENV BIND_ADDR=0.0.0.0:8080
EXPOSE 8080
CMD ["sprite-builder-worker"]

# --- Background build worker image ---
FROM rust:1-slim AS build
WORKDIR /app/backend
RUN apt-get update && apt-get install -y pkg-config build-essential && rm -rf /var/lib/apt/lists/*
COPY backend/ ./
RUN cargo build --release --bin sprite-builder-worker

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /app/backend/target/release/sprite-builder-worker /usr/local/bin/sprite-builder-worker
CMD ["sprite-builder-worker"]

#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"

if [ ! -f "$ROOT/.env" ]; then
  echo "ERROR: .env not found. Copy backend/.env.example to ./.env and fill it in."
  exit 1
fi

# Export env for the backend + worker.
set -a
# shellcheck disable=SC1091
source "$ROOT/.env"
set +a

WITH_WORKER=true
for arg in "$@"; do
  case "$arg" in
    --no-worker) WITH_WORKER=false ;;
  esac
done

cleanup() {
  echo ""
  echo "Shutting down..."
  kill "$BACKEND_PID" "$FRONTEND_PID" "${WORKER_PID:-}" 2>/dev/null || true
  wait "$BACKEND_PID" "$FRONTEND_PID" "${WORKER_PID:-}" 2>/dev/null || true
}
trap cleanup SIGINT SIGTERM EXIT

echo "Starting API server..."
( cd "$ROOT/backend" && cargo run --bin sprite-builder ) &
BACKEND_PID=$!

if [ "$WITH_WORKER" = true ]; then
  echo "Starting build worker..."
  ( cd "$ROOT/backend" && cargo run --bin sprite-builder-worker ) &
  WORKER_PID=$!
fi

cd "$ROOT/frontend"
if [ ! -d node_modules ]; then
  echo "Installing frontend dependencies..."
  npm install
fi
echo "Starting frontend (Vite)..."
npm run dev &
FRONTEND_PID=$!

echo ""
echo "Frontend: http://localhost:5173   (open this)"
echo "Backend:  ${BIND_ADDR:-0.0.0.0:8787}"
[ "$WITH_WORKER" = true ] && echo "Worker:   running"
echo ""

wait

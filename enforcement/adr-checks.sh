#!/usr/bin/env bash
# ADR enforcement checks for rules that aren't covered by clippy/eslint.
# Run from anywhere; exits non-zero if any rule is violated.
#
#   ./enforcement/adr-checks.sh
#
# See enforcement/README.md for the ADR -> mechanism mapping.

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

fail=0
pass() { printf '  \033[32m✓\033[0m %s\n' "$1"; }
bad()  { printf '  \033[31m✗\033[0m %s\n' "$1"; fail=1; }

# ADR 0013 — structured logging only; never println!/eprintln!/dbg!
echo "ADR 0013 — no stdout/stderr printing in backend"
if grep -rnE '\b(println!|print!|eprintln!|eprint!|dbg!)' backend/src >/dev/null; then
  grep -rnE '\b(println!|print!|eprintln!|eprint!|dbg!)' backend/src
  bad "found print/dbg macros in backend/src"
else
  pass "none found"
fi

# ADR 0002 / 0015 — SQLx only; no general-purpose ORMs or raw drivers
echo "ADR 0002/0015 — no ORM / raw DB drivers"
if grep -nE '^\s*(diesel|sea-orm|sea_orm|rusqlite|tokio-postgres|mysql)\s*=' backend/Cargo.toml >/dev/null; then
  bad "a banned ORM/driver is declared in backend/Cargo.toml"
else
  pass "only sqlx is used"
fi

# ADR 0002 — compile-checked queries require a committed offline cache so the
# Docker/CI build (which has no DB) can compile the query! macros.
echo "ADR 0002 — .sqlx offline cache is committed"
if [ -d backend/.sqlx ] && ls backend/.sqlx/query-*.json >/dev/null 2>&1; then
  pass "$(ls backend/.sqlx/query-*.json | wc -l | tr -d ' ') cached queries"
else
  bad "backend/.sqlx is missing — run: (cd backend && cargo sqlx prepare)"
fi

# ADR 0006 — background queue uses FOR UPDATE SKIP LOCKED, no message broker
echo "ADR 0006 — Postgres queue uses FOR UPDATE SKIP LOCKED"
if grep -q 'FOR UPDATE SKIP LOCKED' backend/src/worker/mod.rs; then
  pass "claim uses SKIP LOCKED"
else
  bad "worker no longer uses FOR UPDATE SKIP LOCKED"
fi

# ADR 0008 — single typed API client; components never call fetch directly
echo "ADR 0008 — no direct fetch() outside src/api.ts"
if grep -rn 'fetch(' frontend/src --include='*.ts' --include='*.tsx' \
     | grep -v 'src/api.ts' >/dev/null; then
  grep -rn 'fetch(' frontend/src --include='*.ts' --include='*.tsx' | grep -v 'src/api.ts'
  bad "fetch() called outside the typed client"
else
  pass "all backend access goes through src/api.ts"
fi

# ADR 0007 — per-domain Zustand stores; no Redux / Context-as-store
echo "ADR 0007 — no Redux / React Context state"
if grep -rnE "from ['\"](redux|react-redux|@reduxjs/toolkit)['\"]" frontend/src >/dev/null \
   || grep -rn 'createContext' frontend/src >/dev/null; then
  bad "found Redux import or createContext in frontend/src"
else
  pass "state lives in zustand stores (frontend/src/stores)"
fi

echo
if [ "$fail" -eq 0 ]; then
  printf '\033[32mAll ADR checks passed.\033[0m\n'
else
  printf '\033[31mADR checks failed.\033[0m\n'
fi
exit "$fail"

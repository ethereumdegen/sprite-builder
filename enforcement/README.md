# Enforcement

Mechanical enforcement of the [Architectural Design Records](https://github.com/ethereumdegen/architectural-design-spec)
this repo follows. Each cross-cutting ADR is backed by a lint or a CI check so
the conventions can't silently regress.

## What lives where

The active configs live next to the tool that reads them; this directory holds
the cross-cutting CI script and this map.

| File | Tool | Enforces |
|------|------|----------|
| `backend/Cargo.toml` `[lints]` | rustc + clippy | ADR 0013 (no `print!`/`dbg!`), ADR 0010 (warn on `unwrap`/`expect`/`panic`), `unsafe` forbidden |
| `backend/clippy.toml` | clippy | disallowed methods (e.g. `std::env::set_var`); arg-count threshold |
| `backend/deny.toml` | cargo-deny | ADR 0002/0015 (ban ORMs & raw DB drivers), licenses, advisories, sources |
| `backend/.sqlx/` | sqlx macros | ADR 0002 (committed offline cache so compile-checked queries build without a DB) |
| `frontend/eslint.config.js` | eslint | ADR 0007 (no Redux/Context state), ADR 0008 (no direct `fetch`) |
| `enforcement/adr-checks.sh` | bash/CI | grep-level backstops for the rules above + ADR 0006 (`FOR UPDATE SKIP LOCKED`) |

## Running locally

```bash
# Rust
(cd backend && cargo clippy --all-targets -- -D warnings)
(cd backend && cargo deny check)         # needs: cargo install cargo-deny

# Frontend
(cd frontend && npx eslint .)

# Cross-cutting greps
./enforcement/adr-checks.sh
```

## Regenerating the SQL offline cache (ADR 0002)

The `query!`/`query_as!` macros are checked against a real Postgres at compile
time. So the Docker build (which has no database) can still compile, a snapshot
of every query lives in `backend/.sqlx/` and the build sets `SQLX_OFFLINE=true`.
Regenerate it whenever you add or change a query:

```bash
cd backend
export DATABASE_URL=postgres://…           # a DB with migrations applied
cargo sqlx prepare -- --bins               # needs: cargo install sqlx-cli
git add .sqlx
```

## Carve-outs

Per the spec, boot code may panic/`expect` (fail-fast) and tests may relax
these rules. Boot-phase `expect` sites carry an explicit
`#[allow(clippy::expect_used)]` with a comment.

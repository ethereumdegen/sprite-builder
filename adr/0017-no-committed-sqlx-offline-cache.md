# ADR-0017: Use runtime SQLx queries; do not commit a `.sqlx` offline cache

- **Status:** Accepted
- **Date:** 2026-06-15
- **Scope:** sprite-builder
- **Deciders:** Andy

## Context

[ADR-0002](0002-sqlx-compile-checked-sql-no-orm.md) mandates SQLx with
*compile-checked* queries (`query!` / `query_as!`). Compile-time validation
requires a live database at build time **or** a committed `.sqlx` offline cache,
because the Docker/CI build has no database.

In this repo that cache was 27 generated JSON files under `backend/.sqlx/`,
regenerated with `cargo sqlx prepare` on every query or schema change, with both
Dockerfiles forced into `SQLX_OFFLINE=true`. The cache is generated codegen
checked into the repo: it pollutes diffs, drifts from the code (a stale cache
fails the *offline* build with `no cached data for this query`), and adds a
manual prepare step that is easy to forget. We judged the maintenance and
repo-pollution cost higher than the value of compile-time SQL checking for a
project at this stage.

## Decision

We will use **runtime SQLx queries** in this repo:

- Use `sqlx::query(...)` / `sqlx::query_as::<_, T>(...)` with `.bind(...)` and
  `#[derive(sqlx::FromRow)]` structs. Do **not** use the `query!` / `query_as!`
  compile-time macros.
- Do **not** commit a `backend/.sqlx/` directory, and do not set `SQLX_OFFLINE`
  in any Dockerfile. The build needs neither a database nor an offline cache.

This **amends [ADR-0002](0002-sqlx-compile-checked-sql-no-orm.md) for
sprite-builder only.** Everything else ADR-0002 requires still holds: SQLx and
never a general-purpose ORM, hand-written SQL visible at the call site, and
numbered forward-only migrations. We drop *only* the compile-checked
sub-requirement and the `.sqlx` cache it needs.

Alternative considered: keep the macros and automate `cargo sqlx prepare` in CI
so the cache maintains itself. Rejected for now — it keeps the generated cache
in the repo, which is exactly the thing we wanted gone.

## Consequences

- No generated `.sqlx` artifacts in the repo; no `cargo sqlx prepare` step; the
  Docker build no longer needs `SQLX_OFFLINE` or a build-time database.
- SQL correctness is **no longer checked at compile time** — a column typo or a
  type mismatch now surfaces at runtime instead of at `cargo build`.
- To recover a safety net, query paths should be covered by integration tests
  that run against a real Postgres in CI. This is follow-on work and not yet in
  place.

## Enforcement

This repo intentionally runs **no ADR enforcement scripts** — the `adr/` folder
is reference documentation for humans and LLMs working in the repo, not a CI
gate. Conformance is by review:

- No `query!` / `query_as!` macros in `backend/src` (grep-able if desired).
- No `backend/.sqlx/` directory and no `SQLX_OFFLINE` in any Dockerfile.

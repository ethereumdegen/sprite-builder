# ADR-0002: Data access uses SQLx (compile-checked SQL), never a general-purpose ORM

- **Status:** Accepted
- **Date:** 2026-06-03
- **Scope:** cross-cutting (starflask-monorepo, noblevida-web)
- **Deciders:** Andy

## Context

ORMs (Diesel, SeaORM, an ActiveRecord-style layer) hide the query behind an abstraction. That
abstraction makes simple CRUD terse but obscures what SQL actually runs, makes N+1 and accidental
full-table scans easy, and fights us the moment a query is non-trivial. Our schema is Postgres and
our developers read SQL fluently.

Evidence of the existing convention:
`noblevida-web/nv-backend/src/db/users.rs:42` uses raw `sqlx::query_as::<_, User>(...)` with inline
SQL; migrations are numbered `.sql` files under `migrations/`.

## Decision

We will use **SQLx** with explicit SQL strings, against **Postgres**, with **numbered `.sql`
migrations** applied via the `migrate` binary (see [ADR-0001](0001-rust-library-plus-thin-binaries.md)).

- SQL is written by hand and is visible at the call site. SQL *is* SQL.
- All DB functions live under a `db/` module, one file per aggregate (`db/users.rs`, `db/jobs.rs`).
- Migrations are forward-only, numbered (`001_*.sql`, `002_*.sql`, …), and never edited once shipped.

We will **not** adopt a general-purpose ORM. We accept writing more SQL by hand in exchange for
predictable, reviewable queries.

## Consequences

- Every query is legible and tunable; performance characteristics are obvious in review.
- Compile-time-checked variants catch column/type drift before runtime.
- More boilerplate for trivial CRUD — accepted as the cost of transparency.
- Schema changes are explicit migration files, giving a clean, ordered history.

## Enforcement

- All SQL lives in `db/` modules; a custom clippy `disallowed-methods` rule flags `sqlx::query*`
  used **outside** `db/` (keeps queries from leaking into controllers/services). This pairs with
  [ADR-0003](0003-tenant-scoping-at-the-query-layer.md), which requires scoping inside those `db/`
  functions.
- CI runs `cargo sqlx prepare --check` (or compiles the `query!` macros) against the migration set,
  so a query that no longer matches the schema fails the build.
- Adding an ORM crate to `Cargo.toml` is rejected in review on sight.

# ADR-0015: SQLx is the standard database client — no raw-driver, hand-rolled models

- **Status:** Accepted
- **Date:** 2026-06-03
- **Scope:** cross-cutting (all Rust backends)
- **Deciders:** Andy
- **Extends:** [ADR-0002](0002-sqlx-compile-checked-sql-no-orm.md)
- **Motivated by:** `ethereumdegen/indiefuture2` (legacy `degen-sql` + raw `tokio-postgres`)

## Context

[ADR-0002](0002-sqlx-compile-checked-sql-no-orm.md) chose SQLx *over an ORM*, but it framed the
rejected alternative as ORMs. The legacy `indiefuture2` backend shows the *other* anti-pattern worth
naming: **raw `tokio-postgres` (via the thin `degen-sql` wrapper) with hand-written row mapping.**

- `Cargo.toml` depends on `tokio-postgres` + `degen-sql`, not SQLx.
- `db/postgres/models/users_model.rs` and `access_tokens_model.rs` hand-roll `fn from_row(row:
  &Row)`, `from_joining_row(row, prefix)`, and `get_query_rows(prefix)` join-aliasing helpers,
  with `row.get::<_, i32>("id")` plucking columns by stringly-typed name.

This is brittle in exactly the ways SQLx removes: column renames break at runtime, not compile time;
`from_row` and the `SELECT` list drift apart silently; and the join-prefix machinery is bespoke
boilerplate reinvented per model. (Note one query in `users_model.rs` even ships a trailing-comma
SQL syntax error in a `SELECT` — precisely the class of bug compile-time checking catches.)

## Decision

Rust services will use **SQLx** as the database client:

- Connect/pool with SQLx (`PgPool`); queries via `sqlx::query`/`query_as!` with **compile-time
  checking** against the schema where practical.
- Map rows with SQLx's `#[derive(FromRow)]`, not hand-written `from_row`/`from_joining_row`
  functions over `tokio_postgres::Row`.
- Keep all of this in `db/` modules ([ADR-0002](0002-sqlx-compile-checked-sql-no-orm.md)) with
  explicit owner/tenant scoping ([ADR-0003](0003-tenant-scoping-at-the-query-layer.md)).
- **Raw `tokio-postgres` and `degen-sql` are legacy.** No new service uses them; `indiefuture2` is
  migrated opportunistically.

## Consequences

- Column/type drift and SQL syntax errors are caught at build time, not in production.
- Row mapping is derived, deleting the bespoke `from_row`/prefix boilerplate.
- A migration cost for the remaining `degen-sql` service — accepted and deferred.

## Enforcement

- CI runs `cargo sqlx prepare --check` (or compiles the `query!` macros) against the migration set;
  a query that no longer matches the schema fails the build.
- Adding `tokio-postgres`/`degen-sql` to a new service's `Cargo.toml`, or hand-writing a `from_row`
  over `tokio_postgres::Row`, is rejected in review.

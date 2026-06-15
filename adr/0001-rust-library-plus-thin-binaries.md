# ADR-0001: Rust services are a library crate with thin `server`/`worker`/`migrate` binaries

- **Status:** Accepted
- **Date:** 2026-06-03
- **Scope:** cross-cutting (starflask-monorepo, noblevida-web, devops-agent)
- **Deciders:** Andy

## Context

Our backends do more than serve HTTP: they run background workers, apply migrations, and
sometimes expose admin/CLI surfaces. If each of these is its own ad-hoc program, business logic
gets duplicated and drifts between the request path and the worker path. We want one place where
domain logic lives and several thin entry points that share it.

We also deploy on Railway, where a single repo conveniently maps to multiple services (a web
service, a worker service, a one-shot release/migrate command) built from one image.

## Decision

We will structure each Rust service as a **library crate (`lib.rs`) that holds all domain logic**,
plus **thin binaries** that wire the library to an entry point:

- `server` — the HTTP/API process (Axum),
- `worker` — the background job runner,
- `migrate` — a one-shot that applies migrations and exits.

Binaries contain *only* setup: parse config, build state, call into the library. No business
logic, no SQL, no domain rules in `main.rs`. Shared types and services are defined once in the
library and imported by every binary.

Alternatives rejected: a single monolithic binary with a `--mode` flag (couples unrelated
lifecycles, complicates scaling each independently) and fully separate crates per process
(duplicates wiring and fragments the dependency graph).

## Consequences

- Domain logic is written and tested once; every entry point gets it for free.
- Each process scales and deploys independently on Railway.
- Slightly more boilerplate up front (workspace + bin targets) — accepted.
- Migrations are a deliberate, observable deploy step rather than a side effect of boot.

## Enforcement

- Cargo `[[bin]]` targets are thin by construction; a `main.rs` over ~150 lines or containing
  `sqlx::query`/domain types is a review red flag and a good target for a custom clippy lint
  (`disallowed-methods` pointing SQL macros at binaries).
- CI builds every binary target, so a binary that reaches into private library internals fails
  the build.

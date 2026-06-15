# Architectural Design Records (ADRs)

Reference documentation for the architectural conventions this repo follows.
These are **markdown for humans and LLMs to read** — there is no enforcement
script and no CI gate. When working in this repo, read the relevant ADR before
changing the pattern it describes.

`0001`–`0016` are the cross-cutting house-style records, mirrored from the
canonical source:
<https://github.com/ethereumdegen/architectural-design-spec> (`cross-cutting/`).
That repo is the source of truth; update it there, then refresh the copies here.
`0017`+ are records specific to sprite-builder.

New records follow [`adr-template.md`](adr-template.md): Status, Date, Scope,
Deciders, then Context / Decision / Consequences / Enforcement.

## Cross-cutting (mirrored)

| ADR | Title |
|----:|-------|
| [0001](0001-rust-library-plus-thin-binaries.md) | Rust services: library crate + thin binaries |
| [0002](0002-sqlx-compile-checked-sql-no-orm.md) | Data access uses SQLx, never a general-purpose ORM — *compile-check requirement amended for this repo by [0017](0017-no-committed-sqlx-offline-cache.md)* |
| [0003](0003-tenant-scoping-at-the-query-layer.md) | Owner/tenant scope is an explicit parameter on every data-access function |
| [0004](0004-authz-via-typed-request-extractors.md) | AuthN/AuthZ via typed request extractors, never inline role checks |
| [0005](0005-config-from-environment-fail-fast.md) | Config loaded from the environment at startup, fail fast |
| [0006](0006-postgres-job-queue-skip-locked.md) | Background work on a Postgres queue with `FOR UPDATE SKIP LOCKED`, no broker |
| [0007](0007-zustand-per-domain-stores.md) | Frontend state in per-domain Zustand stores |
| [0008](0008-single-typed-api-client.md) | Frontend talks to the backend through one typed API client |
| [0009](0009-centralized-json-error-contract.md) | Central error type → `{ "error": message }` (superseded by 0012) |
| [0010](0010-no-panics-on-request-paths.md) | No panics on request paths |
| [0011](0011-credentials-in-headers-not-payload.md) | Credentials in headers, not the payload |
| [0012](0012-errors-map-to-correct-http-status.md) | Errors map to the correct HTTP status |
| [0013](0013-structured-logging-never-log-secrets.md) | Structured logging; never log secrets |
| [0014](0014-axum-standard-http-framework.md) | Axum as the standard HTTP framework |
| [0015](0015-sqlx-standard-db-client.md) | SQLx as the standard DB client |
| [0016](0016-scope-based-authorization-from-roles.md) | Scope-based authorization derived from roles |

## sprite-builder–specific

| ADR | Title |
|----:|-------|
| [0017](0017-no-committed-sqlx-offline-cache.md) | Use runtime SQLx queries; do not commit a `.sqlx` offline cache |

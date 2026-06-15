# ADR-0012: Errors map to their correct HTTP status — never collapse everything to 500

- **Status:** Accepted
- **Date:** 2026-06-03
- **Scope:** cross-cutting (all backends)
- **Deciders:** Andy
- **Supersedes:** [ADR-0009](0009-centralized-json-error-contract.md)
- **Motivated by:** `ethereumdegen/indiefuture2` (the "before")

## Context

[ADR-0009](0009-centralized-json-error-contract.md) established one central error type serialized as
`{ "error": message }`. It did **not** pin down status codes, and the legacy `indiefuture2` backend
shows why that gap matters: `util/backend_server_error.rs` maps **every** variant —
`Unauthorized`, `InputParsingError`, `DatabaseRecordNotFound` included — to
`HttpResponse::InternalServerError()` (500). A bad login returns 500; a missing record returns 500.

Collapsing all errors to 500 is actively harmful: clients can't tell "you did something wrong" (4xx,
don't retry, fix the request) from "we broke" (5xx, retry/page someone); monitoring can't alert on a
real 5xx spike because client errors drown it; and it leaks the impression of server instability.
This ADR restates the 0009 contract and adds the missing status discipline, superseding it.

## Decision

Every error returned to a client maps to the **semantically correct HTTP status**, through one
central error type:

- Backends define **one** error type implementing the framework's `IntoResponse`/`ResponseError`,
  returning `{ "error": "<message>" }` (the 0009 contract, retained).
- Each variant maps to its correct status. Baseline table:
  - `400 Bad Request` — input/validation/parse failures (humanized; no raw framework text).
  - `401 Unauthorized` — missing/invalid credentials.
  - `403 Forbidden` — authenticated but not permitted.
  - `404 Not Found` — record/resource absent.
  - `409 Conflict` — uniqueness/state conflicts.
  - `422` — well-formed but semantically invalid, where useful.
  - `5xx` — **only** genuine server faults; the detail is logged, never returned.
- **No `500` for a client-caused error.** A client fault is always 4xx.
- Handlers/services return `Result<_, AppError>` and `?`-propagate; no hand-built `Response`/status
  in handler bodies.

## Consequences

- Clients and monitoring distinguish client faults from server faults; 5xx alerts mean something.
- Status mapping changes in exactly one place (the error type).
- Each new failure mode must be classified to a status — a deliberate, tiny step. Accepted.

## Enforcement

- The type system: the only way to error is the central type, whose `IntoResponse` owns the mapping.
- **Review/lint:** a variant routed to `InternalServerError`/`5xx` that represents client fault
  (auth, validation, not-found, conflict) is rejected; constructing raw error `Response`/status in a
  controller is flagged.
- 5xx responses are additionally captured by diagnostics middleware for observability.

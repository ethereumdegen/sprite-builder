# ADR-0009: Errors map through one central type to a `{ "error": message }` JSON contract

- **Status:** Superseded by [ADR-0012](0012-errors-map-to-correct-http-status.md)
- **Date:** 2026-06-03
- **Scope:** cross-cutting (noblevida-web, starflask-monorepo)
- **Deciders:** Andy

> **Superseded.** This ADR established the central error type and the `{ "error": message }`
> contract but left HTTP status codes unspecified. [ADR-0012](0012-errors-map-to-correct-http-status.md)
> retains that contract and adds the required status-code mapping (no collapsing client faults to
> 500). Conform to ADR-0012.

## Context

If every handler decides its own error shape and status code, clients face an inconsistent API and
internal framework noise leaks to users (raw Serde messages like `missing field \`start_date\``).
We want one error type that every fallible path returns, one JSON shape on the wire, and
human-readable validation messages.

Existing pattern: `noblevida-web/nv-backend/src/error.rs` has an `IntoResponse` impl that always
emits `{ "error": message }`, plus `humanize_json_error()` that rewrites Serde rejections (e.g.
into `"Required field missing: start_date"`). `starflask-monorepo` maps a `thiserror` `AppError`
enum to HTTP status codes.

## Decision

We will define **one central error type per backend** that implements `IntoResponse` and maps to
HTTP status codes, serializing every error as **`{ "error": "<message>" }`**:

- Handlers and services return `Result<_, AppError>`; `?` propagates. No bespoke error responses
  or hand-built status codes in handler bodies.
- Input-validation/deserialization rejections are **humanized** before they reach the client — no
  raw framework/Serde text on the wire.
- Internal details (DB errors, panics-turned-errors) map to 5xx with a safe message; the detail is
  logged, not returned.

## Consequences

- Clients parse one predictable error shape everywhere.
- Adding an error variant updates status mapping in one place.
- Users see readable validation messages instead of framework jargon.
- A small mapping layer must be maintained as new failure modes appear — accepted.

## Enforcement

- The type system: handlers return `Result<_, AppError>`, so the only way to error *is* the central
  type. `From` impls funnel library errors into it.
- Lint/review: flag construction of raw error `Response`/`StatusCode` in controllers — errors must
  go through `AppError`.
- 5xx responses are additionally captured by diagnostics middleware (see
  [noblevida-web](../noblevida-web/) async diagnostics) for observability.

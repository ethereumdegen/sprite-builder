# ADR-0010: No panics on request paths; fail-fast belongs at boot, not in handlers

- **Status:** Accepted
- **Date:** 2026-06-03
- **Scope:** cross-cutting (all services)
- **Deciders:** Andy
- **Motivated by:** `ethereumdegen/indiefuture2` (the "before" — actix-web backend)

## Context

A panic in a request handler is a denial-of-service waiting to happen: a single malformed input or
a missing environment variable takes down the worker handling that request, returning an opaque
failure. The legacy `indiefuture2` backend panics on attacker- and environment-controlled input
*inside handlers*:

- `controllers/auth_controller.rs:95` — `env::var("BUDZ_ADMIN_PASSWORD").unwrap()` **read inside the
  login handler**.
- `controllers/chat_controller.rs:174` — `env::var("DUTCHIE_API_KEY").unwrap()`; `:191` —
  `Url::parse(&endpoint_url).unwrap()`.
- `util/http_request.rs` — `HeaderName::from_bytes(...).unwrap()`, `HeaderValue::from_str(...).unwrap()`.

Two anti-patterns are tangled here: panicking in handlers, and reading configuration lazily at the
use site. We want fallible request work to *return errors*, and we want configuration resolved
**once, at startup** (see [ADR-0005](0005-config-from-environment-fail-fast.md)).

## Decision

Request-handling code (handlers, services, anything reachable from a route) **will not panic**:

- No `.unwrap()`, `.expect()`, `panic!`, indexing-that-can-panic, or `unreachable!` on the request
  path. Fallible operations return `Result<_, AppError>` and propagate with `?`
  ([ADR-0009](0009-centralized-json-error-contract.md) /
  [ADR-0012](0012-errors-map-to-correct-http-status.md)).
- **Configuration is read only at boot.** Required env vars are loaded once into the typed
  `AppConfig` ([ADR-0005](0005-config-from-environment-fail-fast.md)) and held in memory. At that
  one site, `unwrap`/`expect`/abort is **correct and encouraged** — a missing required var *should*
  crash the process at startup, loudly, before serving traffic.
- Reading `std::env` (or any external config) anywhere other than boot is an anti-pattern: it moves
  a fail-fast condition into the request path, exactly the bug `indiefuture2` shipped.

In short: **panic at boot, never at request time.**

## Consequences

- A bad request returns a clean error; it cannot crash a worker thread.
- Misconfiguration fails at deploy, in the logs, not hours later inside a handler.
- Handlers carry slightly more `?`/error-mapping plumbing — accepted; it is the safety.
- Genuinely impossible states may still use `expect` with a written justification, but not on input.

## Enforcement

- **Lint (hard gate):** `clippy::unwrap_used` and `clippy::expect_used` set to **deny** for the
  server/worker crates; `panic`/`unreachable`/`todo` likewise. CI fails on any occurrence.
- **Config exception is structural:** `unwrap` is permitted only in the config-loading module
  (the one place run at boot), enforced by an allow-annotation scoped to that module.
- **Lint:** `std::env::var` is `disallowed-methods` outside the config module
  ([ADR-0005](0005-config-from-environment-fail-fast.md)), so lazy env reads can't reappear.

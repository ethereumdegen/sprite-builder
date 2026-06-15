# ADR-0013: Use structured logging, never `println!`, and never log secrets

- **Status:** Accepted
- **Date:** 2026-06-03
- **Scope:** cross-cutting (all services)
- **Deciders:** Andy
- **Motivated by:** `ethereumdegen/indiefuture2` (the "before")

## Context

Logs are an operational tool and a liability. `println!` writes unstructured, unleveled lines that
can't be filtered, sampled, or shipped to a log backend, and debug prints left in code become noise.
Worse, careless logging exfiltrates secrets into log stores that have a wider audience and longer
retention than the data deserves.

The legacy `indiefuture2` backend declares `#[macro_use] extern crate log` (`main.rs`) and then
ignores it, `println!`-ing throughout — including secrets:

- `db/postgres/models/access_tokens_model.rs` — prints the freshly minted auth token and its scopes.
- `controllers/auth_controller.rs` — `println!("login_input {:?}", input)` where `LoginInput`
  contains the **password** field.
- `db/postgres/models/users_model.rs` — prints user roles on every call.

## Decision

All logging will go through a **structured logging facade** (`tracing`, or `log` with a structured
subscriber), with levels, and **secrets will never be logged**:

- No `println!`/`eprintln!`/`dbg!` in committed code; use `info!`/`warn!`/`error!`/`debug!` with
  fields.
- **Never log** credentials, tokens, API keys, passwords, or full auth headers — not even at
  `debug`. Sensitive fields are redacted or omitted; log a token's *presence* or a hash, never its
  value.
- Errors are logged where they're handled (often the central error type for 5xx,
  [ADR-0012](0012-errors-map-to-correct-http-status.md)), with context, not re-printed everywhere.

## Consequences

- Logs are filterable, leveled, and shippable to a backend; debug noise doesn't leak to prod.
- Secrets stay out of log stores, shrinking the breach blast radius.
- Slightly more ceremony than a quick `println!` — accepted; that ceremony is the point.

## Enforcement

- **Lint (hard gate):** `clippy::print_stdout`, `print_stderr`, and `dbg_macro` set to **deny**; CI
  fails on any `println!`/`dbg!`.
- **Review/check:** logging a struct that contains a credential field (e.g. `{:?}` on a login/token
  type) is rejected; such types implement a redacting `Debug` or omit the field.

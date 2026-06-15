# ADR-0005: Configuration is loaded from the environment at startup and fails fast

- **Status:** Accepted
- **Date:** 2026-06-03
- **Scope:** cross-cutting (all services)
- **Deciders:** Andy

## Context

Config that is read lazily, scattered across `std::env::var(...)` calls deep in the code, fails at
the worst time: in a request handler, in production, after a deploy looked healthy. We deploy on
Railway, which injects configuration as environment variables, and we want a missing or malformed
setting to stop the process **at boot**, not hours later.

Existing pattern: `starflask-monorepo/sf-backend/src/config.rs` defines a single `AppConfig` struct
with `from_env()`, called once in `main.rs`; `noblevida-web` mirrors this. Optional settings (e.g.
`redis_url`) have defaults so local dev needs no full `.env`.

## Decision

We will follow **12-factor config**: a single typed `AppConfig` struct, populated **once at startup**
from environment variables, with the program **panicking immediately** if a required value is
missing or unparseable.

- One `AppConfig` (or equivalent) per service; nothing else reads `env` directly.
- Required values have no default and abort boot if absent; optional values carry sane defaults
  that make local development work out of the box.
- Config is passed explicitly into the app state — no global singletons, no lazy re-reads.
- Secrets come from the environment, never from committed files.

## Consequences

- Misconfiguration is caught at deploy time, in the logs, before traffic arrives.
- All knobs are discoverable in one struct — the config surface is self-documenting.
- Tests construct config explicitly rather than mutating process env.
- No file-based config format to maintain.

## Enforcement

- Custom lint: `std::env::var` is `disallowed-methods` everywhere except the config module.
- `from_env()` returns a hard error/panic on missing required keys, so a misconfigured deploy
  crash-loops visibly instead of serving broken behavior.
- Review: new settings are added to the `AppConfig` struct, never read ad hoc at the use site.

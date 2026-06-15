# ADR-0008: The frontend talks to the backend through one typed API client

- **Status:** Accepted
- **Date:** 2026-06-03
- **Scope:** cross-cutting (noblevida-web frontend, starflask-monorepo frontend)
- **Deciders:** Andy

## Context

When components call `fetch()` directly, cross-cutting concerns — timeouts, credential inclusion,
error parsing, base URLs — get reimplemented inconsistently, and the request/response shapes drift
from the server. We want exactly one place that knows how to talk to our API, and we want the
return types checked.

Existing pattern: `noblevida-web/nv-frontend/src/api/client.ts` exports a single `api` object whose
methods all route through one generic `request<T>()` that sets a 15s timeout, `credentials:
'include'`, parses errors, and types the response. Built on **Vite + React 19 + strict TS**
(`tsconfig.json` has `strict`, `noUnusedLocals`, `noUnusedParameters`).

## Decision

We will route **all** backend communication through a **single typed API client**:

- One module (`api/client.ts`) exporting an `api` object; every endpoint is a typed method.
- A shared generic `request<T>()` owns timeout, abort, `credentials: 'include'` (cookie auth),
  error extraction, and JSON parsing. Components never call `fetch()` directly.
- Return types are explicit (`request<Course[]>`), so client/server drift surfaces as a type error.
- The app is built with **Vite** under **strict TypeScript**; the dev server proxies `/api` to the
  backend to avoid CORS in development.

## Consequences

- HTTP behavior (timeouts, auth, errors) is consistent and changed in one place.
- Typed methods give autocomplete and catch shape mismatches at compile time.
- Adding an endpoint means adding a typed method — a tiny, uniform tax.
- Cookie-based auth pairs with [ADR-0004](0004-authz-via-typed-request-extractors.md) on the server.

## Enforcement

- Custom ESLint rule: ban bare `fetch(`/`axios` outside `api/`; all calls go through the client.
- Strict TS (`strict: true`, `noUnusedLocals/Parameters`) is on in `tsconfig.json`; CI runs
  `tsc --noEmit`, so an untyped or drifted response fails the build.

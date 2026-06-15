# ADR-0004: AuthN/AuthZ is expressed as typed request extractors, never inline role checks

- **Status:** Accepted
- **Date:** 2026-06-03
- **Scope:** cross-cutting (starflask-monorepo, noblevida-web)
- **Deciders:** Andy

## Context

Authorization checks scattered through handler bodies (`if user.role != "admin" { return 403 }`)
are easy to forget on a new endpoint and easy to get subtly wrong. A handler that simply forgets
the check compiles and runs. We want the *requirement* to authenticate/authorize to be part of the
handler's **signature**, so an unauthorized handler is impossible to write.

Axum's `FromRequestParts` lets us encode this in the type system. Existing pattern:
`noblevida-web` defines `VerifiedUser`, `AdminUser`, `EducatorUser` extractors
(`nv-backend/src/middleware/auth.rs`); `starflask-monorepo` defines `AuthUser`, `AdminUser`, and
`ApiKeyUser`. A handler that needs admin simply takes `_user: AdminUser` and Axum rejects everyone
else *before the handler body runs*.

## Decision

We will express authentication and authorization as **typed request extractors**:

- `VerifiedUser` / `AuthUser` — proves the request is from an authenticated user and yields their
  scope id (feeds [ADR-0003](0003-tenant-scoping-at-the-query-layer.md)).
- `AdminUser`, `EducatorUser`, `ApiKeyUser`, … — prove a specific role/credential. Each re-derives
  the role from the database (or a verified token), not from stale client-supplied claims.
- A handler declares its requirement by **taking the extractor as an argument**. There are no
  inline `if role == …` checks in handler bodies.
- Endpoints that need no auth take none — making "this is intentionally public" explicit.

Alternatives rejected: middleware that checks a route allowlist (drifts from the routes it guards),
and per-handler manual checks (forgettable, untyped).

## Consequences

- Forgetting authz means *omitting a parameter*, which is visible and reviewable.
- Role logic lives in one place (the extractor impl), so changing it changes every protected route.
- Re-fetching the role per request costs a query; accepted for correctness over staleness.
- New roles are new extractor types, keeping the permission model enumerable.

## Enforcement

- The type system does most of the work: a handler cannot read a `user_id`/role without the
  corresponding extractor.
- CI check / custom lint: flag `role ==`/`role !=` string comparisons **outside**
  `middleware/auth.rs` — role decisions belong only in extractors.
- Route-registration review: a new route handler with **no** auth extractor must be justified as
  intentionally public.

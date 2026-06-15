# ADR-0016: Authorization is scope/capability-based, derived from roles at token-mint time

- **Status:** Accepted
- **Date:** 2026-06-03
- **Scope:** cross-cutting (all backends with more than two permission tiers)
- **Deciders:** Andy
- **Complements:** [ADR-0004](0004-authz-via-typed-request-extractors.md)
- **Motivated by:** `ethereumdegen/indiefuture2` (a pattern worth *keeping* — the one good idea)

## Context

[ADR-0004](0004-authz-via-typed-request-extractors.md) gates endpoints by **role** (`AdminUser`,
`EducatorUser`). That is the right tool for coarse "admins only" walls, but it couples each endpoint
to role *names*: the day a third role should also write products, you touch every handler that
checked `role == "vendor"`, and additive/fine-grained permissions get awkward.

The legacy `indiefuture2` backend — otherwise an anti-pattern museum — got this part right. It
separates **roles** (who you are) from **scopes/capabilities** (what a token may do), and resolves
the mapping *once*, when the session is minted:

- `util/auth_scopes.rs` — `default_authed_user_scopes()` (`browse_products`, `read_orders`,
  `read_products`, `write_orders`, …), `vendor_scopes()` (adds `write_products`), and
  `get_scopes_from_user_roles(roles)` which composes them.
- `db/postgres/models/access_tokens_model.rs::create_new_authenticated_user_session` — fetches the
  user's roles, calls `get_scopes_from_user_roles`, and **persists the resulting scope set on the
  token row** (`insert_new_user_session` stores `scopes_string`). The access token carries
  `scopes: Vec<String>`.

So at request time the server already knows the token's capabilities — no role→scope recomputation
per call. We formalize this as the house model for fine-grained authz.

## Decision

Authorization uses a **two-layer model**, with scopes resolved **at token-mint time**:

- **Roles** are coarse identity, assigned to the *user* record (`admin`, `educator`, `vendor`).
- **Scopes (capabilities)** are fine-grained verbs a *token* may perform (`read_orders`,
  `write_products`, …).
- The **role → scope mapping lives in exactly one module** (an `AuthScopes`-style place). It is the
  single source of truth for "who can do what."
- Scopes are **computed once, when a token/session is minted**, from the user's roles, and attached
  to the token:
  - **Opaque session tokens** (the indiefuture2 / `starflask` API-key style): store the scope set
    **server-side** on the token row. The token is an unguessable handle; scopes are read by lookup,
    never trusted from the client — consistent with
    [ADR-0011](0011-credentials-in-headers-not-payload.md) and ADR-0004's "don't trust client claims."
  - **JWTs**: scopes may be embedded as a claim, accepting that the set is a snapshot bounded by the
    token's TTL (see Consequences).
- **Endpoints authorize against scopes, not role names.** A `RequireScope("write_products")`-style
  extractor encodes the required capability in the handler signature (composing with
  [ADR-0004](0004-authz-via-typed-request-extractors.md)); coarse role extractors remain valid for
  blanket admin-only gates.
- New capabilities are **additive**: add the scope, grant it to the roles that should have it, in
  the mapping module. No endpoint edits to reshuffle who holds a capability.

## Consequences

- Endpoints decouple from role churn: they declare the capability they need; granting it to another
  role is a one-line change in the mapping, not a sweep across handlers.
- Per-request authz is a cheap **set-membership check** on scopes already on the token — no
  role→scope recomputation, no extra query, on the hot path.
- **Snapshot staleness (the accepted tradeoff):** scopes reflect the user's roles *as of mint time*.
  A role change does not affect already-issued tokens until they expire. Mitigate with short token
  TTLs (indiefuture2 uses 1 day) and/or explicit revocation/refresh when roles change. If a use case
  needs instant revocation, that is a deliberate exception (re-derive per request) and its own note.
- The mapping module becomes security-critical and must be reviewed as such.

## Enforcement

- **Single source of truth:** the role→scope mapping lives only in the `auth_scopes` module; a
  lint/review rejects scope string literals or capability decisions defined anywhere else.
- **Signature, not ad-hoc checks:** capability requirements are expressed via a `RequireScope`
  extractor; `scopes.contains(...)` written inline in a handler body is rejected (mirrors ADR-0004's
  no-inline-role-checks rule).
- **Server-derived, never client-supplied:** scopes are minted from roles server-side; a scope
  arriving in a request body/param is dropped and flagged
  ([ADR-0011](0011-credentials-in-headers-not-payload.md)).

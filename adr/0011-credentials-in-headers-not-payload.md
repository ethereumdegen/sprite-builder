# ADR-0011: Auth credentials travel in headers/cookies, never in request bodies or query strings

- **Status:** Accepted
- **Date:** 2026-06-03
- **Scope:** cross-cutting (all services)
- **Deciders:** Andy
- **Motivated by:** `ethereumdegen/indiefuture2` (the "before")

## Context

Where a credential lives on the wire determines where it leaks. Tokens placed in **query strings**
end up in server access logs, browser history, the `Referer` header, and proxy/CDN logs. Tokens in
**request bodies** invite the pattern of "trust the caller's claimed identity" instead of verifying
it. The legacy `indiefuture2` backend does both, with a glaring hole:

- `controllers/chat_controller.rs:44` — `auth_token: String` is a field of `GetChatInput`, bound
  from a **GET query string** (`web::Query<GetChatInput>`).
- `controllers/chat_controller.rs:71` — `let session_token = chat_input.auth_token.clone(); //make
  sure this is valid !` — and it is **never validated**. The endpoint is effectively unauthenticated.
- `controllers/command_controller.rs` — `access_token` / `access_domain` arrive **in the JSON body**
  and are forwarded onward.

This is the failure mode [ADR-0004](0004-authz-via-typed-request-extractors.md) and
[ADR-0003](0003-tenant-scoping-at-the-query-layer.md) exist to prevent; this ADR makes the wire-
location rule explicit.

## Decision

Authentication credentials will be transmitted **only** via the `Authorization` header (bearer/API
key) or an `HttpOnly` cookie, and will be **verified by a request extractor before the handler runs**:

- No credential, token, session id, or API key may appear as a field of a request body DTO or as a
  query parameter.
- Identity/scope is produced **only** by an auth extractor
  ([ADR-0004](0004-authz-via-typed-request-extractors.md)); handlers never read a token out of the
  payload, and never "validate later."
- Cookie auth pairs with the typed API client's `credentials: 'include'`
  ([ADR-0008](0008-single-typed-api-client.md)); programmatic callers send a bearer token.

## Consequences

- Credentials stay out of logs, history, and referers.
- "Forgot to validate the token" becomes impossible: the extractor validates, or the handler never
  runs.
- A few legacy clients that passed tokens in bodies must move to headers — accepted, one-time cost.

## Enforcement

- **Lint/check:** flag any request DTO field named `*token*`, `*access_token*`, `*api_key*`,
  `*password*` (outside the login body) and any credential-looking query parameter; CI fails.
- The type system: handlers obtain identity through the extractor type only — there is no token
  field on the request struct to read.
- Code review: a route binding `web::Query`/`Json` to a struct carrying a credential is rejected.

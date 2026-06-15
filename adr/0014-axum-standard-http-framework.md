# ADR-0014: Axum is the standard HTTP framework for Rust services

- **Status:** Accepted
- **Date:** 2026-06-03
- **Scope:** cross-cutting (all Rust backends)
- **Deciders:** Andy
- **Motivated by:** `ethereumdegen/indiefuture2` (legacy actix-web), which this supersedes in style

## Context

[ADR-0004](0004-authz-via-typed-request-extractors.md) and the rest of the house style already
assume Axum extractors, but the framework choice itself was never recorded — and our older services
prove the choice is real, not incidental. The legacy `indiefuture2` backend is built on **actix-web**
(`main.rs`: `HttpServer`/`App`, a custom `WebController::config` trait, `web::Query`/`web::Json`
handlers). New work standardizes elsewhere, and we want one answer so agents stop reaching for the
old patterns.

We use Axum because: it composes on `tokio`/`tower`/`hyper` (the same stack our other infra speaks);
its `FromRequestParts` extractors let us encode auth/scope requirements in handler **signatures**
(the foundation of [ADR-0004](0004-authz-via-typed-request-extractors.md)); and middleware is plain
`tower` layers, reused across services.

## Decision

New Rust HTTP services will be built on **Axum**:

- Routes are Axum `Router`s; cross-cutting concerns are `tower` layers.
- AuthN/AuthZ is expressed as Axum extractors
  ([ADR-0004](0004-authz-via-typed-request-extractors.md)), not framework-specific guards or inline
  checks.
- Errors implement Axum's `IntoResponse` through the central error type
  ([ADR-0012](0012-errors-map-to-correct-http-status.md)).
- **actix-web is legacy.** We do not start new services on it; existing actix-web services
  (`indiefuture2`) are migrated opportunistically, not rewritten on sight.

## Consequences

- One framework, one extractor model, one middleware story across services; patterns transfer.
- Shared `tower` middleware (auth, logging, limits) is reusable.
- A real migration cost for the remaining actix-web service — accepted and deferred, not forced.

## Enforcement

- New service templates/scaffolding use Axum; adding `actix-web` to a new service's `Cargo.toml` is
  rejected in review.
- The extractor-based auth model ([ADR-0004](0004-authz-via-typed-request-extractors.md)) only
  exists in Axum here, so conforming to it implies the framework.

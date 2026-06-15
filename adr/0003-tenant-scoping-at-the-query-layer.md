# ADR-0003: Owner/tenant scoping is an explicit parameter on every data-access function

- **Status:** Accepted
- **Date:** 2026-06-03
- **Scope:** cross-cutting (starflask-monorepo, noblevida-web, and every multi-user repo)
- **Deciders:** Andy

## Context

Mistaking which user or team may access which rows is a company-shattering error. A single query
that forgets its `WHERE user_id = $1` can leak one customer's data to another. This class of bug is
silent: the code runs, returns rows, and looks correct in a demo with one account.

We need scoping to be **impossible to forget**, not merely remembered. Two broad strategies exist:
push scoping into the database (Postgres Row-Level Security) or make it explicit and uniform in
application code. RLS is powerful but opaque — the constraint lives far from the call site, is easy
to misconfigure per-connection, and is hard to review. We chose explicitness.

Existing pattern: every scoped DB function already takes the owner id as a parameter, e.g.
`db::chat_sessions::list_for_user(&state.db, auth.user_id, ...)`
(`starflask-monorepo`), and queries filter `WHERE user_id = $1` with the id supplied by the auth
extractor — never derived inside the query.

## Decision

We will make **owner/tenant scope an explicit, required argument** to every data-access function
that touches a scoped table:

- Each `db/` function that reads or writes a user/team-owned row takes `user_id` (or `team_id`)
  as a parameter and includes the corresponding `WHERE` clause. There is no "fetch by id" that
  skips the scope check.
- The scope value originates **only** from an auth extractor
  (see [ADR-0004](0004-authz-via-typed-request-extractors.md)) — never from request bodies, query
  params, or path params chosen by the client.
- Cross-tenant/admin queries are the rare exception and must be named to advertise it
  (e.g. `*_admin`, `*_unscoped`) and gated behind an `AdminUser` extractor.

We deliberately keep scoping in application code rather than RLS so the constraint is visible at
the call site and reviewable in a normal diff.

## Consequences

- Access control is obvious wherever data is read: the scope is right there in the signature.
- Forgetting the scope requires *deleting a parameter*, which is conspicuous in review and lintable.
- A small amount of repetition (the scope arg threads through call chains) — accepted.
- Genuinely global queries must be explicitly, loudly named, which is the point.

## Enforcement

This is the canonical "the linter will not let you commit it" case from our playbook:

- **Custom lint (clippy `disallowed-methods` / a small rule):** raw `sqlx::query*` is banned
  outside `db/` modules ([ADR-0002](0002-sqlx-compile-checked-sql-no-orm.md)), so *all* DB access
  funnels through functions whose signatures can be audited for a scope parameter.
- **Naming gate:** any `db/` function that performs an unscoped read/write must end in `_admin` or
  `_unscoped`; a CI grep/AST check fails the build if an unscoped query exists in a function
  *without* that suffix.
- **Type system:** the scope id is carried by the auth extractor type, so a handler cannot obtain a
  `user_id` without having authenticated.
- **Code review:** any new `db/` function is reviewed specifically for "where does the scope come
  from, and is it from the extractor?"

# ADR-0018: The healthcheck endpoint comes up fast, independent of the database

- **Status:** Accepted
- **Date:** 2026-06-15
- **Scope:** sprite-builder (API server)
- **Deciders:** Andy

## Context

We deploy the API on Railway, which gates a deploy behind an HTTP healthcheck
(`/api/health`, see `railway.toml`). The deploy only goes live — and the old
replica is only retired — once that endpoint returns success within the retry
window (currently 2 minutes). If the healthcheck never passes, Railway reports
the opaque `1/1 replicas never became healthy` and rolls back, even when the
image built and the binary is fine.

The original `main.rs` boot order made the healthcheck depend on the database
being reachable *before the listener ever opened*:

1. `AppState::from_config` → **eager** `PgPoolOptions::connect()`, with no
   connect/acquire timeout,
2. `run_migrations()`,
3. *only then* `TcpListener::bind` + `axum::serve`.

`/api/health` is a trivial handler that returns `{ "ok": true }` and touches no
state — but because it lived behind steps 1–2, a slow or briefly-unreachable
Postgres (common on Railway, where the internal network and the DB service can
lag the app container at boot) hung step 1 forever. The socket never opened, the
healthcheck timed out, and an otherwise-healthy deploy failed. The failure mode
is maximally confusing: the build log is green, and the only runtime output is an
unrelated sqlx parameter warning.

The fail-fast ADRs ([0005](0005-config-from-environment-fail-fast.md),
[0010](0010-no-panics-on-request-paths.md)) cover *configuration* — a missing env
var *should* abort boot. A transient DB connection is a different thing: a runtime
dependency that may simply arrive a few seconds late, and crashing the process
over it turns a recoverable delay into a failed deploy.

## Decision

**The healthcheck endpoint will be answerable as soon as the process binds, with
no dependency on the database or any other external service.** Concretely:

- The HTTP listener binds and `axum::serve` runs **before** any blocking DB work.
  `/api/health` stays a stateless `{ "ok": true }` — it must never query the DB,
  call sprites, or otherwise depend on a runtime dependency that can be slow or
  down.
- The DB pool is created **lazily** (`PgPoolOptions::connect_lazy`), so building
  `AppState` never dials Postgres and never hangs boot. The pool carries an
  `acquire_timeout` so a request-path query against an unreachable DB returns a
  clean error ([ADR-0012](0012-errors-map-to-correct-http-status.md)) instead of
  hanging.
- Migrations run **off the critical path**, in a spawned task, so a slow or
  late-arriving DB does not delay the healthcheck. A *genuine* migration failure
  still aborts the process (`std::process::exit(1)`) so the deploy crash-loops
  visibly per [ADR-0005](0005-config-from-environment-fail-fast.md), rather than
  silently serving against an unmigrated schema.

Alternatives rejected:

- *Eager connect with a timeout* — still couples liveness to the DB and still
  fails the deploy if the DB is merely a little late; only changes a hang into a
  faster failure.
- *A "deep" healthcheck that pings the DB* — inverts this ADR. Liveness ("is the
  process up and serving?") and readiness ("can it reach its dependencies?") are
  different questions; the platform deploy gate wants liveness. A DB blip should
  not roll back a good deploy or take down a running replica.
- *Raising `healthcheckTimeout`* — masks the coupling, makes every deploy slower,
  and still fails once the delay exceeds the larger window.

## Consequences

- Deploys stop failing on transient DB-at-boot timing; the app comes up and
  starts serving immediately, connecting to Postgres on first use.
- There is a brief window after boot where the listener is up but migrations
  have not finished. DB-backed endpoints may error during it; `/api/health` does
  not. Acceptable: the window is short, and traffic shifts only after health is
  green.
- Boot no longer surfaces a *connection* problem by hanging. A truly misconfigured
  `DATABASE_URL` now shows up as request-path errors / a failed migration
  `exit(1)`, not a mysterious healthcheck timeout — check the runtime logs, not
  the build logs.
- "Liveness, not readiness" is now the contract for `/api/health`; a future
  readiness probe (if we want one) must be a *separate* endpoint.

## Enforcement

- **Boot order is structural:** `main.rs` binds + serves before awaiting any DB
  work, and the pool is `connect_lazy`. Reintroducing an eager `.connect().await`
  ahead of `axum::serve` re-creates the bug.
- **Code-review checklist:** `/api/health` stays stateless — no `State` extractor,
  no DB/sprites calls added to it. Slow startup work (DB, migrations, warmup)
  goes after the listener is serving, or into a spawned task.
- **Observability:** the `api server listening on …` log line is emitted at bind;
  its absence in a failed deploy points straight at pre-listener boot work.

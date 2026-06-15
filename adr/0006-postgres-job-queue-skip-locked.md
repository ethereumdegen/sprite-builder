# ADR-0006: Background work runs on a Postgres queue with `FOR UPDATE SKIP LOCKED`, no broker

- **Status:** Accepted
- **Date:** 2026-06-03
- **Scope:** cross-cutting (starflask-monorepo, noblevida-web)
- **Deciders:** Andy

## Context

We have expensive, asynchronous work: image/video generation and document export
(`starflask-monorepo`), and email delivery, reminders, and storage sweeps (`noblevida-web`). The
conventional answer is a dedicated broker (Kafka, RabbitMQ, SQS). But Postgres is already our
source of truth, already deployed, already backed up, and already transactional. Adding a broker
adds an operational component, a second consistency model, and a place for jobs and data to drift
out of sync.

We also need jobs to be **durable across redeploys** and **safe under multiple worker replicas**.

## Decision

We will run background work as a **Postgres-backed queue**, with workers claiming jobs atomically
using `SELECT … FOR UPDATE SKIP LOCKED`:

- Jobs are rows with a state (`ready` → `claimed`/`in_progress` → `done`/`failed`), payload,
  result, error, and progress.
- A worker claims work with `UPDATE … WHERE id IN (SELECT … FOR UPDATE SKIP LOCKED LIMIT n)` —
  atomic, no double-processing, no external broker.
- Workers are stateless and disposable (see
  [starflask-monorepo/0001](../starflask-monorepo/0001-stateless-backend-redis-shared-state.md));
  they poll on an interval.
- A **stale-job reaper** reverts `claimed` jobs whose worker died back to `ready` and fails jobs
  stuck `in_progress`, so a crashed worker self-heals.
- Idempotency for at-least-once delivery is the handler's responsibility (e.g. claim-before-send
  for email/reminders in `noblevida-web`).

We accept Postgres queue throughput limits in exchange for one fewer moving part. If we ever
outgrow it, *that* is a new ADR.

## Consequences

- One datastore, one backup, one consistency model; jobs and domain data commit together.
- Jobs survive redeploys (they're just rows) and crashes (the reaper recovers them).
- Throughput is bounded by Postgres, not a purpose-built broker — accepted at current scale.
- Workers must be idempotent because delivery is at-least-once.

## Enforcement

- The claim query template (`FOR UPDATE SKIP LOCKED`) lives in a shared `db/jobs.rs` helper; new
  queues reuse it rather than hand-rolling claims.
- The reaper is spawned at startup in `main.rs`; its absence is caught in review of any new worker.
- Tests assert two concurrent workers never claim the same job.

# ADR-0007: Frontend state lives in per-domain Zustand stores, not Redux/Context

- **Status:** Accepted
- **Date:** 2026-06-03
- **Scope:** cross-cutting (starflask-monorepo frontend, noblevida-web frontend)
- **Deciders:** Andy

## Context

Redux brings actions, reducers, middleware, and dispatch ceremony that we do not need; a single
global Context becomes a god-object and re-renders the world. We want client state that is
minimal, co-located with its logic, and partitioned so unrelated domains don't entangle.

Existing pattern: both frontends use **Zustand**, one store per domain —
`stores/auth.ts`, `stores/chat.ts`, `stores/courses.ts`, `stores/settings.ts` in `noblevida-web`;
`starflask-monorepo` has ~13 stores (chat, artifact, media, admin, …). Each store is a single file
with state and mutations co-located, mutated via `set()`.

## Decision

We will manage client state with **Zustand**, organized as **one store per domain**:

- Each store is a single file under `stores/`, owning a slice of state plus the functions that
  mutate it. State and logic live together.
- Stores are partitioned by domain (auth, chat, courses, media, …); a feature reads from the
  store(s) it needs. No single global store.
- No Redux, no action/reducer/dispatch indirection, no Context-as-state-container.

## Consequences

- Minimal boilerplate; a new domain is a new file, not a wiring exercise.
- Re-renders are scoped to the store slice a component subscribes to.
- No time-travel/devtools middleware out of the box — accepted; rarely needed.
- Discipline required to keep cross-store coupling low (stores call APIs, not each other).

## Enforcement

- Convention: all client state lives in `stores/*.ts`; a custom ESLint rule can ban importing
  `redux`/`react-redux` and flag `createContext` used to hold mutable app state.
- Review: a new global Context that holds state (rather than DI/config) is rejected in favor of a
  store.

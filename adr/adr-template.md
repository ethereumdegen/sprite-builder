# ADR-XXXX: <short decision title in the imperative>

- **Status:** Proposed | Accepted | Rejected | Superseded by [ADR-YYYY](...)
- **Date:** YYYY-MM-DD
- **Scope:** <repo name, or "cross-cutting">
- **Deciders:** <who owns this decision>

## Context

What forces are at play? What problem, constraint, or requirement makes a decision necessary?
State the facts — technical, business, and team — that pushed us here. Keep it about *why a
decision is needed*, not the decision itself. Cite real code (`path:line`) where useful.

## Decision

The decision, stated plainly and in the active voice: "We will …". Be specific enough that a
reader can tell conforming code from non-conforming code. Note the alternatives considered and
why they lost — this prevents the decision from being re-opened later.

## Consequences

What becomes easier, and what becomes harder, as a result? Include the costs we accepted, not
just the benefits. Note follow-on work the decision implies.

## Enforcement

How is conformance guaranteed mechanically rather than by memory? e.g.:

- a clippy lint / custom ESLint rule that fails the build,
- the type system making the wrong thing unrepresentable,
- a CI gate, pre-commit hook, or test,
- a code-review checklist item (weakest — prefer the above).

If a decision is important enough to record, it is usually important enough to enforce.

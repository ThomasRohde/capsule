# ADR 0004: Browser applications use named endpoints, not raw SQL

- Status: Accepted for bootstrap
- Date: 2026-08-07

## Context

A generic SQL endpoint would be easy to build but difficult to secure, version, inspect, and govern. The browser still needs a generic way to read and mutate domain data without the host knowing the domain.

## Decision

Store named, parameterised endpoint declarations in `capsule_endpoint`. The host validates input against an explicit schema and executes one statement under read or write rules.

## Consequences

- The host remains domain-independent.
- App capabilities are inspectable and allowlist-based.
- Complex transactions are awkward in v0 and may later require a richer declarative action model.
- Arbitrary ad hoc queries remain available to trusted agents and SQLite tools, not untrusted browser code.

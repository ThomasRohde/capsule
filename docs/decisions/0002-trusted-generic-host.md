# ADR 0002: Use a small trusted generic host

- Status: Accepted for bootstrap
- Date: 2026-08-07

## Context

Executing JavaScript and SQL declarations directly from an unfamiliar database is unsafe. Putting Diagram Studio logic in the launcher would also collapse the separation between format and example.

## Decision

Provide a small generic host responsible only for file lifecycle, verification, asset serving, security headers, parameter validation, named SQL operations, and process lifecycle. Require explicit trust before active execution.

## Consequences

- The host is a security boundary and must remain auditable.
- Example-specific rendering stays in database assets.
- Multiple hosts can implement the same protocol later.
- The bootstrap does not claim safe execution of arbitrary third-party capsules.

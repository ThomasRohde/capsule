# ADR 0007: The named-endpoint contract is trigger-free

- Status: Accepted for the current format
- Date: 2026-08-08

## Context

SQLite triggers can turn one visible endpoint statement into hidden mutations. A generic host could attempt to inspect and authorise every trigger program, but that would enlarge the trusted runtime and make endpoint effects harder for people and agents to review.

## Decision

Capsules must not contain triggers. Verification rejects them. Each endpoint step is one statement whose `:name` placeholders exactly match the declared parameter schema, compiles under the same read/write authoriser used at runtime, and cannot use PRAGMA operations.

Required parameters reject explicit `null` unless `nullable: true` is declared. Numeric inputs must be finite and fit SQLite's supported range.

## Consequences

- One endpoint declaration remains a useful, reviewable approximation of one capability.
- Hidden cascading behavior is limited to declared foreign-key actions such as `ON DELETE CASCADE`.
- Applications needing richer atomic workflows must wait for an explicit declarative transaction contract in a later format version.
- Adding trigger semantics would be a format change, not an example-specific convenience.

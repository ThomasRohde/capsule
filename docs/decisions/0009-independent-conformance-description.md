# ADR 0009: Independent conformance description

## Status

Accepted as the independent conformance contract.

## Context

The generic runtime verifier is executable host code. It is valuable, but a
single implementation cannot be the only evidence that a capsule conforms to
the platform contract. A host and its format description can otherwise drift in
the same direction while remaining internally consistent.

## Decision

Keep a dependency-free, machine-readable platform description at
`format/capsule-v0.2.conformance.json`. The description records the current
identity values, required tables and columns, nullability and primary-key
semantics, required foreign keys, the exact `START_HERE` result shape, and
minimum non-empty platform content.

`tools/capsule_conformance.py` validates a capsule using only Python's standard
library, `sqlite3`, and that description. It is deliberately separate from
`runtime.capsule_host.CapsuleDatabase.verify`; both checks must pass for the
release gate. The independent check proves structural agreement, not publisher
authenticity, SQL safety, or application-level correctness.

The description complements the existing SQLite schema. It does not
introduce migrations, signatures, or a second runtime format. Any incompatible
change requires a new conformance description and format version.

## Consequences

- A format change has a reviewable data contract before host code is changed.
- Adversarial tests can exercise validator/runtime disagreement directly.
- Other hosts can consume the description without importing this Python host.
- The description intentionally does not claim that structural conformance is a
  publisher signature or a complete sandbox.

# ADR 0025: Signed data contracts and restricted migrations

## Status

Accepted on 2026-08-12 by lifecycle milestone M00.

## Context

Generic lifecycle code cannot infer whether a table is user content, seed data,
history, cache or derived state from its name. Likewise, a publisher signature
does not make arbitrary SQL, scripts or named application endpoints safe to run
during an upgrade.

The initial migration draft included `rebuild_dataset` with a
`rebuild_endpoint`. That would execute application-declared SQL through a second
path and conflicts with the no-execution lifecycle boundary.

## Decision

V0.3 signs an exhaustive data contract. Every ordinary domain table used by a
transforming operation belongs to exactly one dataset and declares ordered
primary-key columns plus ignored and immutable columns. Dataset roles are
`seed`, `user-content`, `settings`, `history`, `derived`, or `cache`.
Sensitivity is a separate `normal`/`sensitive` property, not a seventh role.
Dependencies must form a validated acyclic graph.
Declaration hints such as expected row counts or BLOB presence are inspection
metadata only and can never raise host hard ceilings.

"Without data" means constructing from a clean signed template/release or a
complete signed reset policy. The host never implements it by deleting all
non-platform tables.

Migration declarations are part of the signed application compartment. The
first operation profile permits only bounded host-interpreted
`copy_rows`, `copy_dataset`, and `discard_dataset` steps over tables and columns
already validated by the data contracts. Literal and mapped values use an
explicit SQLite typed-value JSON wrapper; raw JSON scalars are not implicitly
coerced. Mapping lists are ordered records rather than JSON object property
names. Non-finite reals and invalid UTF-8 are rejected.

Migration declarations do not carry resource ceilings in the initial profile.
Hard row, byte and deadline limits are host-owned and bound into the lifecycle
plan; publisher metadata can never raise them. A future signed advisory limit
may only lower a host ceiling and requires a versioned contract change.

`rebuild_dataset` and `rebuild_endpoint` are not migration operations. An
upgrade `rebuild` policy means retaining the clean target release's declared
dataset state, with no application endpoint execution. The engine never accepts
SQL, scripts, extensions, loops, attachment, application callbacks or UI-
provided identifiers.

An upgrade starts from a clean verified target release and writes only declared
domain/instance/lineage state in that output. The first implementation requires
the same `app_id`, a valid target signature and the same accepted signing key.
Key delegation is a future versioned extension. Migration graphs are acyclic and
planning requires exactly one supported path.

## Consequences

- Unclassified or structurally inconsistent tables fail transforming
  operations with stable contract errors.
- M02 freezes data-contract shape; M08 implements the interpreter and
  adversarial graph/type/write-boundary tests.
- V0.2-to-v0.3 upgrade remains unavailable until M08 accepts a versioned signed
  legacy-source adapter; planners must not infer legacy dataset semantics.

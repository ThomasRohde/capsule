# ADR 0012: Atomic multi-step named commands

## Status

Accepted for the current format.

## Context

Some application operations must change several semantic rows and append a
durable inverse record atomically. Running several browser requests permits
interleaving and partial commits. Allowing browser-supplied SQL or hidden trigger
side effects would weaken the named-capability boundary.

## Decision

The current format requires `capsule_endpoint_step`.

An endpoint with no step rows remains a single-statement endpoint. A compound
endpoint has two to sixteen contiguous, one-based steps, is a write operation,
uses `changes` result mode, and repeats step one in
`capsule_endpoint.sql_text` for ordinary inspection.

The host validates parameters against all steps, authorises and preflights every
statement, executes the sequence in one immediate transaction, enforces optional
row-count preconditions, rolls back on any failure, and appends one change-log
row before commit. Browser code invokes only the stable endpoint name and never
supplies SQL.

## Consequences

- Multi-row semantic commands have an inspectable atomic boundary.
- Exact row-count guards support optimistic concurrency and history checks.
- Triggers and raw browser SQL remain unnecessary and prohibited.
- Applications still own semantic operation payloads and inverse commands.

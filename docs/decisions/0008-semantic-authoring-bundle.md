# ADR 0008: Round-trip authoring uses a semantic bundle

- Status: Accepted for current tooling
- Date: 2026-08-08

## Context

The SQLite file is the canonical runtime and distribution artefact, but binary database pages are a poor review surface. A round-trip tool must preserve arbitrary domain tables without teaching generic tooling about Diagram Studio.

## Decision

`capsule unpack` emits a deterministic `org.sqlite-capsule.authoring-bundle/0.2` directory containing:

- SQLite pragmas and ordered schema objects;
- one canonical JSONL file per table;
- typed wrappers for BLOB values;
- content-addressed files for capsule assets;
- source identity and digest metadata.

`capsule pack` reconstructs the database into a new temporary file, checks foreign keys, runs full capsule verification, vacuums deterministically, and only then publishes the requested output. `capsule diff` compares pragmas, schema, and table rows semantically with bounded key output.

## Consequences

- Runtime edits can return to a reviewable, product-independent representation.
- Repacking preserves database semantics but is not required to reproduce the source file's historical page layout or digest.
- Repeated packs from one bundle are byte-identical within the supported environment.
- Migration and export remain separate future contracts.

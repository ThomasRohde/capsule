# Execution plan — M04: Duplicate, compact duplicate, fork and template creation

## Outcome

Users can create verified new capsule files through one coherent
`Create copy…` workflow: exact duplicate, compact duplicate, fork with data,
create from template and policy-controlled selective fork.

## Scope

- Implement plan/execute APIs and CLI/Tauri flows for all copy modes.
- Preserve exact identity for duplicates; generate new IDs for forks/templates.
- Apply signed dataset policies and dependency ordering.
- Record lineage for non-identical logical outputs.
- Support v0.2 duplicate/compact duplicate only.

## Explicit non-scope

- No comparison or reconciliation.
- No application upgrade.
- No inferred blanking of unknown tables.
- No overwrite or in-place mutation.

## Programme rules carried into this milestone

- Read the repository root `AGENTS.md` and `CONTRIBUTING.md` before editing.
- Inspect the live code. Paths below are likely integration points, not permission to
  assume their contents.
- Preserve the distinction between generic format/host code and Diagram Studio.
- Inputs are pinned and read-only. Transformations produce create-new outputs.
- The raw Wry renderer receives no lifecycle command, path, handle or report.
- No UI field may become an arbitrary SQL, file-system or process-execution surface.
- Keep v0.2 behaviour stable unless an accepted ADR says otherwise.
- Update this milestone's `RESULT.md` and `PROGRAM_STATUS.json` with exact evidence.
- Do not mark the milestone complete while any required acceptance item is false.

## Likely integration points

- `native/crates/capsule-workspace` copy modules
- `native/crates/capsule-cli`
- trusted Tauri shell Overview/Create copy wizard
- top-level generic Python tooling only if cross-implementation copy is accepted;
  Rust/native remains normative for lifecycle security
- Diagram Studio template and selective-fork fixtures

## Implementation sequence

1. Implement internal `preview_copy` as a deterministic, non-authoritative
   dry-run returning identity effects, dataset actions, estimated row counts,
   sensitivity prompts, dependencies, expected application digest and output
   constraints. Host-only `prepare_copy` retains execution authority and is a
   separate API.
2. Implement exact duplicate by copying the exact verified private snapshot
   byte-for-byte. Preserve capsule and revision identity and require identical
   snapshot/output SHA-256.
3. Implement compact duplicate with `VACUUM INTO` or an equivalent controlled
   create-new operation. Preserve logical identity while documenting that file
   bytes/hash differ.
4. Implement fork-with-data:
   - copy application compartment without alteration;
   - generate new capsule/revision IDs;
   - carry/reset/omit/prompt datasets by contract;
   - update mutable profile timestamps;
   - add fork lineage with source digest.
5. Implement create-from-template from a clean verified release:
   - require a signed template-state proof that matches every dataset's actual
     canonical state; a general application signature is insufficient;
   - new capsule/revision IDs;
   - publisher seed/target datasets retained;
   - user content empty or explicitly seeded;
   - derived/cache datasets rebuilt or omitted;
   - no source working-data leakage.
6. Implement selective fork only when all affected dataset policies and
   dependencies, including actual cross-dataset foreign keys, permit it.
   Sensitive datasets default to omitted. Scrub omitted bytes from freelist,
   mutable platform tables and SQLite sequence state before publication.
7. Execute from an immutable plan, rebind source, create a new path, verify output,
   compare expected app digest and publish atomically.
8. Add Tauri wizard with clear operation names, descriptions, dataset decisions,
   identity preview, destination picker and result card.
9. Record a plan digest and lineage event for fork/template/selective outputs.
   Exact duplicates do not invent a new logical lineage event.
10. Add crash/race and data-leakage tests.

## Required tests

- Input byte hash and SQLite contents unchanged for every mode.
- Exact duplicate is logically identical and consistently readable.
- Compact duplicate is logically equivalent, often smaller, and contains no
  recoverable free-page fixture data where the platform test can establish this.
- Fork has new IDs and unchanged application digest.
- Template output contains required seed data and no user-content sentinel.
- Selective fork respects dependencies and sensitive defaults.
- v0.2 fork/template rejected with a precise compatibility error; duplicate works.
- Destination exists/stale plan/source replacement/crash-stage tests.
- Tauri UI and raw renderer negative tests.

## Evidence to retain

- Copy-mode truth table.
- Dataset decision logs for fixtures.
- Input/output digests and verification reports.
- Crash/race matrix.
- UI screenshots and accessibility evidence.

## Acceptance gate

- No mode writes the input.
- No mode overwrites an existing destination.
- Application digest is preserved for v0.3 fork/template outputs.
- `without data` is implemented through declared policy, never table-name guesses.
- Sensitive data cannot leak into a clean/template output by default.

## Failure and rollback posture

- Keep the working tree reviewable and do not combine unrelated cleanup.
- A failed transform must remove or quarantine only its private temporary output;
  it must never "repair" an input in place.
- A partially implemented public command/UI action remains feature-gated or absent.
- If a draft contract proves wrong, update it and record an ADR before broadening
  production code around it.
- When a repository-wide gate has a known pre-existing failure, prove it from the
  M00 baseline, avoid worsening it, and create explicit follow-up evidence rather
  than silently ignoring it.

## Handoff requirement

Complete `RESULT.md`, update `PROGRAM_STATUS.json`, and identify the exact first
task for the next milestone. The result must be sufficient for a fresh Codex
session with no hidden context.

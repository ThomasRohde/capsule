# Execution plan — M08: Restricted declarative data migrations

## Outcome

Application upgrade supports a unique signed path across data-schema
versions using a bounded, non-Turing-complete migration interpreter. Outputs are
still created from the clean target application release and verified before
publication.

## Scope

- Validate signed migration graph and unique path selection.
- Implement the restricted declarative migration profile.
- Add source/target assertions, typed column mapping, allowlisted value maps,
  dataset copy/omit/rebuild operations and bounded execution.
- Add Diagram Studio v1→v2 migration fixtures and upgrade UI details.

## Explicit non-scope

- No arbitrary SQL, JavaScript, WASM, shell commands, extensions or plugin code.
- No loops, recursion or network/filesystem access from migration definitions.
- No publisher key rotation.

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

- `native/crates/capsule-workspace/src/migration*` and upgrade integration
- v0.3 migration contract/conformance
- CLI planning/execution and Tauri migration-path review
- Diagram Studio source/target schema fixtures
- plugin authoring/validation support after canonical implementation

## Implementation sequence

1. Validate the signed migration graph:
   - same application/data schema ID;
   - positive versions;
   - no self-edge, duplicate edge or cycle;
   - exactly one path from source to target;
   - every definition hash belongs to the verified target release.
2. Finalise operation profile `org.sqlite-capsule.migration-ops/1`. Keep the
   interpreter allowlist small. Initial operations:
   - source/target structural assertions;
   - `copy_rows` with explicit source/target tables and typed column mappings;
   - literal defaults;
   - finite value-map transforms with fail/keep/null handling;
   - whole declared dataset copy where schemas are provably identical;
   - omit/discard and signed rebuild for declared derived/cache datasets.
3. Reject unknown operations/fields. Enforce table/column identifiers against
   inspected schemas and dataset contracts; callers never supply SQL.
4. Execute each migration edge into the clean final target output or controlled
   intermediate host-owned databases, with transactions, cancellation, row/byte
   limits and no untrusted attachments to a writable input connection.
5. Enforce source/target read/write compartments:
   - old working source read-only;
   - clean target release read-only;
   - only host-owned output writable;
   - application signed tables immutable after the initial target copy.
6. Run edge preconditions/postconditions and full final verification.
7. Add plan evidence showing path, per-dataset actions, estimated limits and
   irreversible warnings.
8. Add Diagram Studio v1 and v2 fixtures with:
   - stable IDs retained;
   - new style identifier mapped from legacy kind;
   - target presets retained;
   - derived cache rebuilt;
   - invalid/unmapped legacy value negative fixture.
9. Extend the upgrade wizard to show migration steps in human terms, not raw
   definitions, and require confirmation for irreversible migration.
10. Add fuzz/property tests for malformed definitions, graph ambiguity, overflow,
    type confusion, row bombs and cancellation.

## Required tests

- Valid v1→v2 upgrade and exact expected output comparison.
- Invalid/unmapped value fails without published output.
- Unknown operation/field rejected.
- Cycle, duplicate edge, no path and ambiguous path rejected.
- Limits/cancellation/overflow/type mismatch.
- Attempts to name platform/application tables rejected.
- Inputs unchanged, target app digest retained, output verified.
- Cross-fixture deterministic plan/result reports.

## Evidence to retain

- Final migration operation specification.
- Graph validation and interpreter threat review.
- Diagram Studio migration before/after semantic report.
- Fuzz/property test summary and resource-limit results.
- Final application signature and lineage proof.

## Acceptance gate

- Migration definitions contain no arbitrary executable language.
- Only host-owned output domain/instance compartments are writable.
- A unique, verified path is required.
- Any failed assertion or limit prevents publication.
- Final application digest equals the clean target release.

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

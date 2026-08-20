# Execution plan — M02: Data contract, lineage and workspace service foundation

## Outcome

A product-independent Rust workspace service can validate signed dataset
contracts and mutable lineage, produce deterministic dry-run lifecycle plans,
rebind pinned inputs, create private outputs and publish only verified create-new
files.

## Scope

- Implement the signed dataset lifecycle contract and lineage model.
- Add the `capsule-workspace` crate and a narrow CLI/API surface.
- Implement stable lifecycle error codes and serialisable plan/report types.
- Implement deterministic canonical JSON and plan digests.
- Implement private temporary output and no-replace publication primitives.
- Add Diagram Studio v0.3 data-contract source fixtures, but no feature UI yet.

## Explicit non-scope

- No full copy/fork operation.
- No comparison, reconciliation or migration interpreter.
- No Cabinet UI beyond possible developer diagnostics.

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

- New `native/crates/capsule-workspace/`
- `native/Cargo.toml` workspace members/dependencies
- `native/crates/capsule-cli` for inspect/profile/plan diagnostics
- v0.3 format/conformance and Python authoring helpers
- `examples/diagram-studio/` reviewable data-contract source
- generic tests under native and top-level test locations

## Implementation sequence

1. Create `capsule-workspace` with modules for errors, limits, identity, profile,
   datasets, lineage, canonical plans and output publication.
2. Define stable public error codes, including:
   `unsupported_format`, `invalid_contract`, `undeclared_table`,
   `missing_primary_key`, `incompatible_application`, `incompatible_schema`,
   `stale_plan`, `destination_exists`, `limit_exceeded`,
   `signature_changed`, `verification_failed`, `publisher_mismatch`,
   `migration_path_missing`, and `migration_path_ambiguous`.
3. Parse dataset declarations only from the verified signed compartment. Validate:
   - every declared table exists and is declared once;
   - primary-key columns exist and match supported SQLite PK semantics;
   - ignored/immutable columns exist and do not include the full PK;
   - dataset dependencies reference existing datasets and are acyclic;
   - every non-platform application table is classified, unless the accepted ADR
     permits a fail-closed extension mechanism;
   - policy combinations are internally valid.
4. Parse lineage from the mutable compartment. Validate sequence, result IDs,
   parent limits, hash formats and current-revision consistency. Do not treat
   lineage assertions as publisher-authenticated facts.
5. Implement immutable operation-plan structures conforming to
   `contracts/lifecycle-plan-v1.schema.json`. Canonicalise and digest them.
6. Implement plan rebinding against file identity, byte length, file digest,
   capsule/revision IDs, application digest and data schema.
7. Implement create-new output staging:
   - private owner-only temporary path;
   - destination must not exist;
   - transaction/rollback semantics;
   - integrity, foreign-key, signature and application checks;
   - fsync and atomic publish where available;
   - clear failure semantics where atomicity is unavailable.
8. Add CLI JSON output for overview, data contract, lineage and plan validation.
9. Populate Diagram Studio reviewable source with dataset declarations matching
   actual tables. Add at least one sensitive and one derived fixture for tests.
10. Add property/adversarial tests for cycles, duplicates, malformed PKs, huge
    declarations, stale source and destination races.

## Required tests

- `cargo test -p capsule-workspace --all-targets`.
- Workspace fmt/check/test/clippy.
- JSON contract fixture validation.
- Deterministic plan digest across repeated runs.
- Source replacement and destination race tests.
- Negative tests proving no input connection is writable.
- Cross-platform unit tests for publication abstractions; Windows integration
  tests where available.

## Evidence to retain

- Crate API documentation and dependency-boundary review.
- Error-code catalogue.
- Data-contract validation matrix.
- Plan canonicalisation fixtures.
- Publication crash-stage test results.
- Diagram Studio contract coverage report.

## Acceptance gate

- The crate contains no Diagram Studio identifiers.
- Every lifecycle write path targets only a private new output.
- A stale plan or existing destination fails closed.
- Dataset semantics come exclusively from a verified signed contract.
- Lineage is treated as useful provenance, not publisher authentication.

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

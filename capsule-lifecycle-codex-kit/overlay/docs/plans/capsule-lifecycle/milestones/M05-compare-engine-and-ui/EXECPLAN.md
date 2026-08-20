# Execution plan — M05: Bounded compare engine and trusted shell comparison

## Outcome

Two capsules can be compared without executing either. The engine reports
identity/lineage, application, schema and policy-controlled domain-data
differences through bounded, paginated JSON and a trusted Tauri interface.

## Scope

- Compatibility classification and four-layer comparison.
- Deterministic row/field comparison for declared datasets with stable PKs.
- Summary-first, paginated disclosure and sensitive-data controls.
- CLI JSON report and trusted shell pair-selection/detail UI.
- Optional ancestor recognition, but no changes are applied yet.

## Explicit non-scope

- No reconciliation writes.
- No heuristic comparison of undeclared application tables.
- No execution of capsule assets/endpoints/prompts/runbook commands.

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

- `native/crates/capsule-workspace/src/compare*`
- compare report schema and error catalogue
- `native/crates/capsule-cli`
- trusted Tauri compare commands/session state/UI
- generic fixtures under `compatibility/` and Diagram Studio variants

## Implementation sequence

1. Implement compatibility classification:
   - same release/same schema;
   - same app/same schema;
   - same app/migration available;
   - same app/incompatible schema;
   - different application;
   - invalid input.
2. Compare identity/lineage from bounded metadata. Treat lineage as claims and
   verify common-ancestor file/digest where supplied.
3. Compare application compartments by digest first. Expand into assets,
   endpoints, permissions, contracts and signatures only on explicit detail.
4. Compare schema from canonical `sqlite_schema`, PRAGMA metadata and accepted
   structural rules. Do not execute application SQL.
5. Compare domain data strictly through the verified dataset contract:
   - deterministic PK ordering with explicit SQLite value ordering;
   - ignored columns excluded;
   - BLOBs shown as size/hash by default;
   - field values bounded/truncated;
   - cancellation, row, byte and deadline limits;
   - summary counts remain available when detail truncates.
6. Define row and value digests with an explicit typed canonical representation
   that distinguishes NULL, integer, real, text and blob.
7. Expose summary and paginated detail through opaque host session IDs. UI cannot
   name arbitrary tables; it requests validated dataset/table tokens returned by
   the host.
8. Sensitive datasets show counts only until the trusted user explicitly reveals
   a bounded page. Never persist revealed values to the Cabinet cache or logs.
9. Add pair picker, compatibility banner, four sections, dataset summaries and
   accessible table/list detail.
10. Add adversarial fixtures: huge rows, malformed UTF-8 representation boundaries,
    NaN/float edge cases as SQLite permits, composite keys, blobs, no PK,
    collations, schema drift and cancellation.

## Required tests

- Determinism: same pair produces same report digest under fixed limits.
- Symmetry properties for summary counts where applicable.
- No execution side effects: canary endpoint/asset/command remains untouched.
- Memory/time/row limits and cancellation.
- Sensitive-value redaction and no-log checks.
- Different-app and incompatible-schema behaviour.
- CLI schema validation of reports.
- Tauri pagination/accessibility/raw-window isolation.

## Evidence to retain

- Typed value canonicalisation specification.
- Compatibility and adversarial fixture matrix.
- Performance measurements on small, medium and bounded-large fixtures.
- Sensitive-data review.
- UI screenshots and report-schema examples.

## Acceptance gate

- Comparison is read-only and execution-free.
- Undeclared or unstable tables fail closed or remain summary-only per accepted ADR.
- Reports stay bounded and deterministic.
- Sensitive values require explicit trusted-shell disclosure.
- No compare command is available to the raw renderer.

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

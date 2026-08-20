# Execution plan — M06: Apply selected changes to a new target-derived copy

## Outcome

Users can review and apply selected compatible data changes from a source
capsule to a new copy derived from the target. Inputs remain untouched, conflicts
are explicit, and the output retains the target application release.

## Scope

- Reconciliation plans bound to a comparison report and exact inputs.
- Row/field operations for datasets whose contract allows manual reconciliation.
- Optional three-way conflict classification when a verified ancestor is supplied.
- Target-derived create-new output, lineage and validation.
- Trusted shell review/resolution workflow and CLI JSON support.

## Explicit non-scope

- No in-place merge.
- No automatic merge across incompatible schemas.
- No arbitrary conflict-resolution code.
- SQLite Session Extension capture may be deferred to a later ADR; this milestone
  works from verified snapshots and optional ancestor.

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

- `native/crates/capsule-workspace/src/reconcile*`
- reconcile plan/report schemas
- CLI and trusted Tauri compare/reconcile session commands
- Diagram Studio branch/conflict fixtures

## Implementation sequence

1. Build a reconciliation plan only from a non-truncated compatible comparison
   report or from re-queried exact rows bound to its digest.
2. Default the output to a copy of the target. Preserve target capsule ID,
   generate a new revision ID and retain the target application digest.
3. Support allowlisted operations:
   - insert row from source;
   - delete target row;
   - replace target row from source;
   - set selected fields from source.
   Bind each operation to target row/value precondition digests.
4. Enforce dataset/table policy, immutable columns, FK dependency ordering and
   sensitive-data confirmation.
5. With a verified common ancestor, classify insert/insert, update/update,
   delete/update and immutable-field conflicts. Without an ancestor, label
   ambiguous changes as user decisions rather than pretending they are
   automatically mergeable.
6. Require all conflicts to be resolved before execution. Store no raw sensitive
   values in the plan when a digest plus source lookup is sufficient.
7. At execute time rebind both inputs and report/plan digests, create a target copy,
   apply operations transactionally, update revision/profile/lineage, run all
   checks and publish only after verification.
8. Add two-parent lineage:
   - target-derived-from;
   - changes-applied-from;
   plus ancestor details when used.
9. Add UI wording `Apply selected changes to a new copy`, output identity preview,
   conflict list, resolution controls, validation checklist and result evidence.
10. Add negative tests for stale rows, changed input, unresolved conflicts,
    forbidden datasets, immutable fields, constraint failures and destination race.

## Required tests

- Source and target input hashes unchanged.
- Output application digest equals target digest.
- Selected rows/fields only are changed.
- Row preconditions detect changes after review.
- FK/unique/check failures roll back and leave no published output.
- Three-way fixture conflict classification.
- Reopen/verify output and compare it with the intended expected fixture.
- Lineage parents and plan digest correctness.
- Raw renderer cannot prepare or execute reconciliation.

## Evidence to retain

- Reconciliation operation matrix and conflict taxonomy.
- Input/output/expected comparison reports.
- Transaction rollback and stale-plan results.
- Lineage record examples.
- Trusted-shell screenshots and accessibility results.

## Acceptance gate

- There is no in-place merge code path.
- All inputs remain byte-identical.
- The target application compartment is unchanged.
- Unresolved conflicts and failed validation block publication.
- The published output corresponds exactly to the reviewed plan.

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

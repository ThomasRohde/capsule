# Execution plan — M07: Application release upgrade with unchanged data schema

## Outcome

A working v0.3 capsule can receive a newer, clean, publisher-verified
application release without losing compatible user-owned data when the old and
new releases use the same data schema version.

## Scope

- Upgrade planning and execution for same `app_id`, same publisher key and same
  data schema ID/version.
- Start from clean target release; carry/migrate no schema yet.
- Carry instance identity/profile, generate new revision, apply dataset policies.
- Capability-delta review, lineage, output verification and trusted shell wizard.

## Explicit non-scope

- No data-schema migration; M08 handles that.
- No publisher key rotation/delegation in the first implementation.
- No cross-application import.
- No mutation of the working capsule or target release.

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

- `native/crates/capsule-workspace/src/upgrade*`
- upgrade plan schema
- signed-release verification and trust/capability evaluation
- CLI and trusted Tauri Versions/Upgrade wizard
- clean Diagram Studio release fixtures with same data schema

## Implementation sequence

1. Inspect and pin the working capsule and clean target release without running
   either application.
2. Require:
   - v0.3;
   - same `app_id`;
   - target `app_version` newer under the accepted version policy;
   - valid target signature;
   - same accepted publisher key;
   - same data schema ID and version;
   - host supports target format/runtime profile.
3. Calculate signed application and requested-capability deltas. Show added,
   removed and changed capabilities. An increase requires normal trust review
   before the upgraded app can execute.
4. Build the output by copying the clean target release, not by patching the old
   working database.
5. Preserve source `capsule_id`, title/description/tags/document kind and
   user-owned instance icon where valid. Generate a new `revision_id`.
6. Apply target-release dataset policy:
   - `copy`: carry compatible source user data;
   - `target`: keep clean target rows;
   - `rebuild`: invoke only a signed, allowlisted rebuild mechanism after data copy;
   - `omit`: leave absent/empty as defined;
   - `migrate`: reject in M07 because schema migration is not enabled;
   - `forbid`: block.
7. Validate that the final application digest exactly equals the clean target
   release digest and signature remains valid.
8. Record `application-upgrade` lineage with both upgraded-from working revision
   and application-release file digest.
9. Publish to a new path and leave both inputs untouched.
10. Add the trusted shell wizard under Versions/Upgrade application.

## Required tests

- Happy path retains all user-content sentinel rows and profile fields.
- Target presets/assets/endpoints replace old release content.
- Final application digest equals target clean release.
- New revision ID, same capsule ID.
- Increased capability produces review-required state.
- Same version/downgrade/different app/different key/schema mismatch/invalid
  signature all fail with precise codes.
- Inputs remain byte-identical and output reopens/verifies.
- Crash and destination-race matrix.

## Evidence to retain

- Same-schema upgrade truth table.
- Application/capability delta reports.
- Dataset carry/reset/rebuild evidence.
- Final digest/signature and lineage evidence.
- UI wizard screenshots and raw-window isolation result.

## Acceptance gate

- The output begins from and retains the clean target application release.
- User data/profile survive according to signed policy.
- Application digest matches target exactly.
- Schema mismatch cannot enter this code path.
- The original and target release are unchanged.

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

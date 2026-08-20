# Execution plan — M00: Reconcile live repository and freeze architecture

## Outcome

A reviewed, repository-specific implementation baseline and a set of accepted
architecture decisions. Draft contracts are reconciled with the live verifier,
signature canonicalisation, native command boundaries, plugin snapshot and build
pipeline before production code is changed.

## Scope

- Capture the exact Git commit, dirty state, toolchain versions and relevant file tree.
- Trace the v0.2 format through Python authoring/conformance/signing and Rust
  inspection/crypto/runtime.
- Trace the trusted Tauri shell and raw Wry command/capability boundary.
- Reconcile all draft v0.3 contracts in `../../contracts/` with actual repository
  conventions.
- Decide the application/instance signature split, v0.3 compatibility policy,
  `capsule-workspace` crate boundary, canonical JSON/digest rules, metadata limits,
  lifecycle error taxonomy and migration DSL posture.
- Produce accepted ADRs and a path-impact map.

## Explicit non-scope

- No new user-facing feature.
- No format v0.3 implementation.
- No automatic migration of existing capsules.
- No dependency upgrades unless required solely to run the baseline.

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

- `AGENTS.md`, `CONTRIBUTING.md`, `README.md`
- `format/`, `tools/capsule_author.py`, `tools/capsule_conformance.py`,
  `tools/capsule_signatures.py`
- `native/Cargo.toml`, `native/crates/capsule-core`,
  `native/crates/capsule-crypto`, `native/crates/capsule-lifecycle`,
  `native/crates/capsule-runtime`, `native/desktop/src-tauri`,
  `native/desktop/ui`
- `plugins/capsule-creator/`
- repository test and release workflows

## Implementation sequence

1. Run `docs/plans/capsule-lifecycle/tools/codex_lifecycle/capture_baseline.py` and store the JSON under
   `evidence/M00/`.
2. Run the current fast Python and Rust gates without modifying generated artefacts.
   Record pre-existing failures separately.
3. Produce a call graph/data-flow note for:
   - format inspection;
   - signed-application canonicalisation;
   - native first-open/trust transition;
   - raw renderer protocol;
   - pack/build/sign pipelines.
4. Diff each draft SQL/JSON contract against current conventions. Resolve:
   - table names and exact structural-profile policy;
   - which tables belong to the signed application stream;
   - v0.2/v0.3 coexistence and dispatch;
   - instance/profile ownership;
   - UUID/time/hash/canonical JSON representation;
   - icon decoding limits and safe media types;
   - lifecycle plan and output-publish semantics;
   - data contract and migration representation.
5. Create ADRs under `docs/decisions/` or the repository's established ADR location.
   At minimum cover:
   - application release versus capsule instance identity;
   - format v0.3 rather than silently altering v0.2;
   - signed application v0.3 canonical stream;
   - new `capsule-workspace` crate;
   - plan/execute create-new protocol;
   - restricted declarative migrations;
   - host-owned Cabinet cache.
6. Replace draft-contract assumptions only when justified. Record every material
   deviation in the milestone result.
7. Produce a concrete implementation path map and dependency graph for M01-M09.
8. Ask an independent architecture/security critic to review the ADR set. Resolve
   high-severity findings.

## Required tests

- Existing Python suite: `python -m unittest discover -s tests -v`.
- Current generated-artifact checks documented in `CONTRIBUTING.md`.
- From `native/`: format/check/test/clippy commands documented in `native/README.md`.
- `python docs/plans/capsule-lifecycle/tools/codex_lifecycle/validate_lifecycle_specs.py`
  after any contract edits.
- No generated artefact should change in M00.

## Evidence to retain

- Baseline JSON with commit, status, toolchain and path inventory.
- Baseline test transcript or machine-readable summary.
- Accepted ADRs and critic report.
- Contract reconciliation table: proposal, live state, decision, affected milestone.
- Updated `PROGRAM_STATUS.json` and completed `RESULT.md`.

## Acceptance gate

- The exact live baseline is recorded.
- All architectural choices required by M01 and M02 are resolved or explicitly
  marked as blocking.
- The signature boundary proves that mutable instance/profile/domain changes can
  occur without publisher re-signing.
- The raw renderer boundary is documented with negative test points.
- No production implementation has started prematurely.

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

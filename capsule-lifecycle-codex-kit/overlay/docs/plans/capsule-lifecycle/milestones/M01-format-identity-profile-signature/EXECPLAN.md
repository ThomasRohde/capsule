# Execution plan — M01: Format v0.3 identity, profile and signature compartment

## Outcome

Python and Rust implementations can inspect and verify a new v0.3 capsule in
which the publisher-signed application release is independent of mutable capsule
identity, profile, lineage and domain data. Existing v0.2 verification remains
unchanged.

## Scope

- Finalise and implement generic format v0.3 and signed-app v0.3 schemas.
- Add application display metadata and bounded instance profile metadata.
- Add `capsule_id` and `revision_id` with precise semantics.
- Create a distinct signed-application canonicalisation context/profile.
- Add Python and Rust v0.3 dispatch, inspection, conformance and signature support.
- Add cross-implementation signature fixtures and mutation tests.
- Define a safe v0.2 fallback profile for Overview without rewriting v0.2.

## Explicit non-scope

- No copy/fork execution yet.
- No Cabinet UI.
- No data comparison or migration execution.
- No automatic v0.2-to-v0.3 conversion.

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

- New versioned files in `format/`; never replace v0.2 contracts in place.
- `tools/capsule_author.py`, `tools/capsule_conformance.py`,
  `tools/capsule_signatures.py`, `tools/build_signed_app_fixtures.py`
- `native/crates/capsule-core`, `native/crates/capsule-crypto`,
  `native/crates/capsule-runtime`, `native/crates/capsule-cli`
- `compatibility/` signature and conformance vectors
- plugin format/runtime snapshots, updated only after canonical sources pass

## Implementation sequence

1. Materialise accepted M00 decisions as versioned `format/*v0.3*` files and an
   independent conformance record.
2. Implement a version dispatcher in Python and Rust. Do not scatter
   `if version == ...` throughout unrelated code; expose version-specific profiles
   behind stable interfaces.
3. Split inspected identity into explicit structures:
   - `ApplicationReleaseIdentity`;
   - `CapsuleInstanceIdentity`;
   - `DataSchemaIdentity`;
   - `CapsuleOverview`.
4. Parse metadata with strict row counts, UTF-8/length bounds, JSON shape checks,
   timestamp rules and safe icon references. Never decode an icon during generic
   SQL inspection.
5. Define the signed-app v0.3 canonical stream and context. Include all
   application-controlled schema/metadata/assets/endpoints/checks/contracts and
   exclude:
   - instance profile and instance icon;
   - lineage;
   - grants and host state;
   - domain rows and change history.
6. Implement identical canonicalisation in Python and Rust. Generate positive and
   negative cross-language vectors.
7. Add mutation matrix tests:
   - changing an application asset fails verification;
   - changing an endpoint/data contract/migration fails verification;
   - changing title/tags/instance icon/domain rows retains application signature;
   - changing `app_id`, version, data schema identity or permissions fails.
8. Add v0.3 authoring/build support without changing v0.2 output defaults unless
   explicitly accepted in M00.
9. Add a minimal generic v0.3 fixture and verify it with both implementations.
10. Sync the standalone creator plugin only after repository source tests pass.

## Required tests

- Unit tests for every bounded metadata field and exact-row invariant.
- Python↔Rust canonical stream byte-for-byte fixture comparison.
- Positive/negative signature vectors.
- v0.2 golden fixture verification before and after the change.
- v0.3 mutation matrix.
- Full Python and Rust workspace gates.
- `python tools/capsule.py inspect|verify` for both v0.2 and v0.3 fixtures.

## Evidence to retain

- Final format and signed-app contracts.
- Canonical stream specification and vector digests.
- Compatibility matrix showing v0.2 and v0.3 behaviour.
- Mutation matrix results.
- Updated docs and plugin snapshot consistency evidence.

## Acceptance gate

- A mutable instance-profile or domain-row change does not alter the v0.3
  application digest.
- Any signed application change does alter the digest and invalidates the old
  signature.
- Python and Rust agree exactly.
- v0.2 fixtures remain accepted/rejected exactly as before.
- No lifecycle transform exists yet that can mutate an input.

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

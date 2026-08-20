# Execution plan — M09: Plugin sync, hardening, qualification and release

## Outcome

The lifecycle feature set is documented, independently testable, represented
in Diagram Studio and the creator plugin, and passes repository, native UI,
security, generated-artefact, packaging and release qualification gates.

## Scope

- Complete Diagram Studio v0.3 profiles, icons, data contracts, branch fixtures and
  upgrade/migration releases.
- Sync standalone creator plugin from canonical implementation.
- Add CI, compatibility vectors, docs, ADRs, security review and release notes.
- Run adversarial, accessibility, performance, crash and installer qualification.
- Rebuild required generated artefacts and installer outputs.

## Explicit non-scope

- Cloud synchronisation, collaboration, marketplace or remote release discovery.
- macOS/Linux support unless separately accepted.
- Publisher key rotation unless an earlier ADR expanded scope.

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

- `examples/diagram-studio/`, `capsules/`, `exports/`, `compatibility/`
- `plugins/capsule-creator/`
- root and native docs/ADRs/security policy
- test suites and `.github/workflows/`
- native SBOM/licence inventories and installer build path

## Implementation sequence

1. Convert Diagram Studio fully to the accepted v0.3 source format:
   application profile/icon, instance profile, data contract, lineage and at least
   two signed releases suitable for same-schema and schema-migration upgrade tests.
2. Add deterministic generated fixtures:
   - clean application release;
   - working instance;
   - fork with/without sensitive history;
   - two branches and common ancestor;
   - expected reconciled output;
   - old/new application releases and expected upgraded outputs.
3. Update generic CLI, documentation map, authoring guide, native host contract,
   architecture/security docs and user help.
4. Sync the standalone `capsule-creator` plugin from canonical repository files.
   Test it from a copied directory with no repository checkout. It must scaffold
   v0.3 metadata/contracts and validate migration declarations.
5. Add/strengthen CI:
   - Python unit/conformance/generated checks;
   - Rust fmt/check/test/clippy;
   - signature and migration compatibility vectors;
   - browser/export tests;
   - native Tauri trusted/raw UI tests;
   - SBOM/licence/RustSec gates;
   - installer/release workflow qualification.
6. Run the security gauntlet from `prompts/SECURITY_GAUNTLET.md` with independent
   reviewers. Resolve critical/high issues and explicitly disposition lower ones.
7. Run performance tests against bounded large fixtures. Define supported limits
   rather than claiming unbounded scalability.
8. Run crash-injection tests at all durable lifecycle stages and verify no input
   corruption, no half-published output and recoverable host state.
9. Run accessibility review including keyboard, scaling, reduced motion and a
   documented manual screen-reader pass or explicit remaining gap.
10. Build/check generated examples and exports. Follow root `AGENTS.md` installer
    requirements: rebuild from matching source and perform native acceptance before
    replacing/distributing installer artefacts.
11. Produce release notes, compatibility matrix, limitations and operational
    recovery guidance.

## Required tests

- Every command required by root `AGENTS.md`, `CONTRIBUTING.md`, native README and
  release workflow.
- Clean-checkout regeneration check.
- Standalone plugin smoke/black-box tests.
- Full native UI trusted/raw/window suites.
- Security gauntlet, crash matrix, accessibility and performance suite.
- ZIP/installer/signed fixture verification from a clean temporary directory.

## Evidence to retain

- Consolidated release qualification report.
- Full command transcript/test summary with environment.
- Threat-model review and findings disposition.
- Accessibility and performance reports.
- Generated artefact digests, plugin sync manifest, SBOM/licence evidence.
- Final compatibility matrix and known limitations.

## Acceptance gate

- All programme invariants and milestone acceptance gates pass.
- Diagram Studio demonstrates every lifecycle workflow.
- Canonical repository and standalone plugin are synchronised and independently
  verified.
- Raw renderer remains unable to invoke lifecycle features.
- Generated artefacts and installer were rebuilt from the tested source.
- Remaining limitations are explicit and do not contradict security claims.

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

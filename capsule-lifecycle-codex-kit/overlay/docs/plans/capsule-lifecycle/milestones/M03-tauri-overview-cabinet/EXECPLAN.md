# Execution plan — M03: Capsule Overview and Cabinet trusted shell UX

## Outcome

The native client opens on a safe Capsule Cabinet/Overview experience that
shows bounded application and instance metadata before execution, while retaining
the existing trust, capability, recovery, signing and update controls under clear
secondary navigation.

## Scope

- Add host-owned Overview and Cabinet models and Tauri commands.
- Render safe application/instance metadata and icon/fallback.
- Add host-local recent-file cache that is rebuildable and non-canonical.
- Reorganise existing shell navigation without weakening the state machine.
- Add accessibility, visual and raw-renderer-isolation tests.

## Explicit non-scope

- Copy/fork execution is not enabled until M04.
- Compare and upgrade buttons may be disabled with precise reasons.
- No capsule-supplied HTML/CSS/Markdown runs in the trusted shell.

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

- `native/desktop/src-tauri`
- `native/desktop/ui/index.html`, `app.js`, styles and host-owned assets
- native capability/permission declarations
- `tests/native/` preparation, shell, raw-window and visual fixtures
- optional host-local Cabinet store in a new explicit module

## Implementation sequence

1. Add a `CapsuleOverviewViewModel` assembled in Rust from bounded inspected data.
   JavaScript receives no database path beyond what current trusted UI policy
   permits and no raw SQL/table access.
2. Add safe icon loading:
   - verify declared hash;
   - PNG/WebP only;
   - compressed and decoded limits;
   - decode outside the DOM;
   - deterministic fallback;
   - no remote content and no SVG in the first release.
3. Add Overview as the first page after bounded inspection. Separate visually:
   - signed application and verified publisher;
   - mutable/self-described capsule profile;
   - file and trust state.
4. Add Cabinet as the no-file page with `Open capsule…` and
   `Create from template…` (the latter disabled until M04 if necessary).
5. Implement recent-file caching outside capsules. Store only bounded metadata,
   last-opened time, trust badge/cache version and path identity needed to reopen.
   Reinspect on every open; never trust cached metadata for execution.
6. Move existing controls under:
   - Security: trust, capabilities, publisher tools, local trust;
   - Recovery: backups, conflicts and restore;
   - Settings: host updates.
   Keep all previous actions and tests working.
7. Add precise v0.2 Overview fallback: title/summary/application identity with
   explicit `legacy v0.2` badge; do not synthesize mutable v0.3 identity.
8. Add disabled lifecycle action cards with compatibility explanations until the
   respective milestone is implemented.
9. Add keyboard, focus, reduced-motion, screen scaling and semantic-heading tests.
10. Prove the raw renderer cannot invoke any Overview/Cabinet/lifecycle command.

## Required tests

- Native prepared-fixture checks.
- Tauri trusted-shell UI suite and raw-window negative suite.
- Icon corpus: valid, hash mismatch, wrong media type, oversized compressed data,
  excessive dimensions, malformed/decompression-bomb candidates.
- Cabinet cache corruption/missing-file tests.
- v0.2, v0.3 signed, v0.3 unsigned and invalid-signature screenshots/visual tests.
- Existing trust/recovery/signing/update interaction regression tests.

## Evidence to retain

- Before/after navigation map and screenshots.
- Accessibility test report plus any remaining manual screen-reader gap.
- Raw renderer command inventory showing no lifecycle access.
- Safe icon adversarial matrix.
- Cabinet cache schema and ownership note.

## Acceptance gate

- Overview appears without releasing or executing application assets.
- Publisher identity and mutable profile cannot be confused.
- Existing security and recovery controls remain reachable and tested.
- Cached Cabinet data is never used as authority.
- Raw renderer isolation tests pass.

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

---
name: capsule-lifecycle-program
description: Implement the repository's Capsule lifecycle programme milestone by milestone, including v0.3 identity and metadata, Tauri Capsule Overview, copy/fork, compare, apply-to-copy, and signed application upgrade with data migration.
---

# Capsule lifecycle programme

Use this skill when asked to implement, continue, review or diagnose the Capsule
lifecycle programme.

## Discovery

1. Read the repository root `AGENTS.md` and `CONTRIBUTING.md`.
2. Read `CODEX_LIFECYCLE_START.md`.
3. Validate the programme files:
   `python docs/plans/capsule-lifecycle/tools/codex_lifecycle/validate_lifecycle_specs.py`.
4. Read `docs/plans/capsule-lifecycle/PROGRAM_STATUS.json`.
5. Select the lowest-numbered milestone not marked `complete`.
6. Read that milestone's `EXECPLAN.md`, `ACCEPTANCE.md` and existing
   `RESULT.md`, if present.

## Work protocol

- Inspect the actual code and tests named by the plan. Paths are hypotheses
  until confirmed in the current checkout.
- Keep the generic format/host independent of Diagram Studio.
- Prefer adding the product-independent `capsule-workspace` boundary described
  by the target architecture rather than broadening raw renderer authority.
- Lifecycle inputs are pinned and read-only. Writes occur only in a create-new
  temporary output that becomes visible after verification.
- Do not infer dataset semantics from table names. Require a signed data
  contract.
- Do not treat display metadata or icons as publisher identity.
- Do not call a cross-publisher import an upgrade.
- Never make a v0.2 signed capsule appear v0.3 by rewriting its signed
  compartment.
- Use stable, bounded JSON reports for Tauri and CLI integration.
- Add failure-path tests before or alongside the success path.
- Run the exact relevant gates listed by the milestone.
- Update status, evidence and handoff before leaving the milestone.

## Review loop

When subagents are available:

1. Delegate bounded implementation components to builders only when their file
   ownership does not overlap.
2. Ask a fresh critic to compare the actual diff and test evidence with
   `ACCEPTANCE.md`.
3. Ask a security critic to test raw-renderer isolation, input immutability,
   TOCTOU resistance, output publication and signature invariants.
4. Resolve findings and rerun gates.
5. The main agent remains accountable for integration and final evidence.

## Completion

A milestone is complete only when every required acceptance item is evidenced,
all required tests pass, generated artefacts are current, and
`PROGRAM_STATUS.json` is updated atomically.

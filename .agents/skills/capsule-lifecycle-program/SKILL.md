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
7. Do not preload unrelated milestone plans, ADRs or historical evidence. Open
   additional material only when the active plan or a concrete finding points
   to it.

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
- Run focused gates while the tree is changing. After source freeze, run the
  exact milestone gates once; rerun only gates affected by later production
  changes.
- Update status, evidence and handoff before leaving the milestone.

## Efficient milestone loop

1. Use one Codex task and one final commit for the milestone. Do not begin the
   next milestone in the same task.
2. The main agent owns integration and the exclusive Cargo/build lease. Do not
   overlap Cargo, native builds, installer builds or end-to-end test runs.
3. Use no implementation subagent by default. If parallel work has a concrete
   elapsed-time benefit, use at most one implementation subagent at a time with
   a bounded brief and disjoint file ownership, then stop it after integration.
4. Freeze the source before independent review. Ask one fresh reviewer for a
   consolidated `ACCEPTANCE.md` and security audit. Add a second specialist only
   for a concrete unresolved risk or at the user's request.
5. Resolve the consolidated findings, rerun affected gates, and permit at most
   one remediation review. The main agent remains accountable for the final
   gate rather than starting an open-ended critic loop.
6. Run full repository qualification, generated-artifact rebuilds and any
   required NSIS export once after the final freeze.
7. Report repeated passing evidence compactly by command, count and artifact
   path. Retain detailed logs only for failures or unique runtime evidence.
8. Defer sound findings outside the active milestone to the next milestone's
   backlog instead of expanding scope.

## Completion

A milestone is complete only when every required acceptance item is evidenced,
all required tests pass, generated artefacts are current, and
`PROGRAM_STATUS.json` is updated atomically.

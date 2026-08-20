# Codex start: Capsule lifecycle programme

Treat `docs/plans/capsule-lifecycle/` as the implementation programme for this
repository.

## Objective

Evolve the native Capsule host into a trusted Capsule Cabinet that can safely
show, duplicate, fork, compare, reconcile and upgrade capsules while preserving
the repository's existing trust boundaries and application/example separation.

## Required reading

Before editing code, read:

1. `AGENTS.md`
2. `CONTRIBUTING.md`
3. `docs/architecture.md`
4. `docs/format-contract.md`
5. `docs/native-host-contract.md`
6. `docs/security.md`
7. `docs/plans/capsule-lifecycle/README.md`
8. `docs/plans/capsule-lifecycle/program/00-CHARTER.md`
9. `docs/plans/capsule-lifecycle/program/03-TARGET-ARCHITECTURE.md`
10. `docs/plans/capsule-lifecycle/program/04-SECURITY-AND-TRUST.md`
11. `docs/plans/capsule-lifecycle/PROGRAM_STATUS.json`
12. the next incomplete milestone's `EXECPLAN.md`

## Execution rules

- Start with M00. Draft contracts are not a substitute for inspecting the live
  code.
- Work in the current response/session; do not merely propose code.
- Complete one milestone gate before starting the next.
- Update `PROGRAM_STATUS.json` and the milestone `RESULT.md`.
- Use builder, independent critic and security-critic subagents when available.
  Keep changes integrated through the main agent.
- Never weaken an existing fail-closed rule to make a test pass.
- Never mutate source capsule fixtures during lifecycle operations.
- Never expose lifecycle or filesystem commands to the raw Wry application
  renderer.
- Any output capsule is published only after structural, signature, declared
  check, integrity, foreign-key and operation-specific validation.
- Maintain v0.2 compatibility as specified. Do not silently rewrite v0.2 signed
  capsules.
- Review and synchronise the standalone `capsule-creator` plugin for every
  material format, runtime, security or authoring change.
- Follow the repository's installer rebuild rule when native host changes
  require it.

## Start

Run:

```text
python docs/plans/capsule-lifecycle/tools/codex_lifecycle/validate_lifecycle_specs.py
python docs/plans/capsule-lifecycle/tools/codex_lifecycle/capture_baseline.py
```

Then execute M00 from
`docs/plans/capsule-lifecycle/milestones/M00-baseline-and-decisions/EXECPLAN.md`.
Continue until its acceptance gate passes. Thereafter proceed to the next
pending milestone, unless a documented blocking invariant makes continuation
unsafe.

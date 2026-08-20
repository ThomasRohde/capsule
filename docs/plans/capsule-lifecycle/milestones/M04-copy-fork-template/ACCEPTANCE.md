# Acceptance gate — M04: Duplicate, compact duplicate, fork and template creation

This file is the release gate for the milestone. Each checked item must cite
evidence in `RESULT.md`; a checked box without evidence is invalid.

## Product and architecture

- [x] No mode writes the input.
- [x] No mode overwrites an existing destination.
- [x] Application digest is preserved for v0.3 fork/template outputs.
- [x] `without data` is implemented through declared policy, never table-name guesses.
- [x] Sensitive data cannot leak into a clean/template output by default.

## Cross-cutting invariants

- [x] Every lifecycle input used by a test or implementation is opened/pinned
      read-only and remains unchanged.
- [x] Every transforming operation writes to a create-new output and refuses an
      existing destination.
- [x] No Diagram Studio table, endpoint, shape or rendering concept appears in
      generic format/runtime/workspace code.
- [x] No new lifecycle command, event or capability is registered for the raw Wry
      application renderer.
- [x] v0.2 acceptance/rejection and runtime behaviour are unchanged unless an
      accepted ADR explicitly authorises a versioned compatibility change.
- [x] Error paths fail closed with stable, actionable error codes and do not expose
      sensitive row values by default.
- [x] Relevant documentation, contracts, tests and generated artefacts are updated
      together.

## Verification

- [x] Focused unit and integration tests pass.
- [x] Required repository-wide gates listed in `EXECPLAN.md` pass.
- [x] An independent critic/security review was performed and findings were
      resolved or explicitly accepted with rationale.
- [x] `RESULT.md` records exact commands, environment, pass/fail counts and any
      remaining limitations.
- [x] `PROGRAM_STATUS.json` is valid and accurately reflects the state.
- [x] The next milestone has a precise, executable handoff.

## Stop conditions

Do not complete this milestone when:

- any required check above is false;
- a test is skipped because it exposes a design flaw;
- generated source/artefacts are out of sync;
- a source capsule was mutated;
- security relies only on UI hiding rather than native command/capability denial;
- the implementation requires application-specific knowledge in generic code.

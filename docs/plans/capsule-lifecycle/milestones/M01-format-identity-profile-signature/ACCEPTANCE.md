# Acceptance gate — M01: Format v0.3 identity, profile and signature compartment

This file is the release gate for the milestone. Each checked item must cite
evidence in `RESULT.md`; a checked box without evidence is invalid.

## Product and architecture

- [x] A mutable instance-profile or domain-row change does not alter the v0.3
      application digest.
- [x] Any signed application change does alter the digest and invalidates the old
      signature.
- [x] Python and Rust agree exactly.
- [x] v0.2 fixtures remain accepted/rejected exactly as before.
- [x] No lifecycle transform exists yet that can mutate an input.

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

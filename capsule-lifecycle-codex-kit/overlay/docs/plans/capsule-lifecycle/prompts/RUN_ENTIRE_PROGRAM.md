# Run the complete Capsule lifecycle programme

Implement the complete programme under `docs/plans/capsule-lifecycle/`, from the
first incomplete milestone through M09.

Read the repository's `AGENTS.md` and `CONTRIBUTING.md` first. Then read the
programme charter, architecture, security model, contracts and status.

## Operating contract

1. Start or resume exactly one milestone at a time.
2. Use its `EXECPLAN.md`, `PROMPT.md` and `ACCEPTANCE.md` as the working contract.
3. Keep `RESULT.md` and `PROGRAM_STATUS.json` current throughout the work.
4. Make milestone boundaries reviewable. Prefer one coherent commit per completed
   milestone, plus small prerequisite/fix commits when necessary.
5. Do not begin a dependent milestone until the current acceptance gate passes.
6. Use parallel specialist subagents only for independent bounded work:
   architecture review, Rust implementation review, security attack review,
   Tauri/accessibility review and test-fixture review. The lead agent owns
   integration and reruns all gates.
7. If the live repository invalidates a proposal, do not force the proposal.
   Record an ADR, update contracts/plans, and preserve the programme invariants.
8. Never mutate checked-in capsule inputs. Use temporary or create-new files.
9. Never expose lifecycle functions to the raw Wry renderer.
10. Do not fetch or add dependencies unless the repository policy permits it and
    the change is justified, pinned, licensed and reflected in SBOM/reporting.
11. Do not claim completion on the basis of unit tests alone. Run the milestone
    gates and the cross-cutting tests relevant to touched trust boundaries.
12. When external tooling or a platform-specific gate is unavailable, complete all
    possible work, leave the milestone open, and record the exact missing command
    and required environment. Do not fabricate evidence.

## Completion output

At the end of each milestone, report:

- delivered outcome;
- architectural decisions/deviations;
- exact test commands and results;
- security/critic findings;
- generated artefacts;
- remaining limitations;
- next milestone and first action.

Continue until M09 is genuinely complete or a hard environment constraint blocks
a required gate. In the latter case, leave a deterministic handoff rather than
weakening the gate.

# Codex prompt — M08: Restricted declarative data migrations

Implement milestone **M08** from the Capsule lifecycle programme.

## Read first

1. repository root `AGENTS.md`;
2. `CONTRIBUTING.md`;
3. `CODEX_LIFECYCLE_START.md`;
4. `docs/plans/capsule-lifecycle/program/00-CHARTER.md`;
5. target architecture, security model and relevant programme specifications;
6. `docs/plans/capsule-lifecycle/PROGRAM_STATUS.json`;
7. this milestone's `EXECPLAN.md` and `ACCEPTANCE.md`;
8. the previous milestone result (M07) when applicable.

## Required outcome

Application upgrade supports a unique signed path across data-schema
versions using a bounded, non-Turing-complete migration interpreter. Outputs are
still created from the clean target application release and verified before
publication.

## Working method

- Inspect the live repository before editing and adapt stale path assumptions.
- Mark this milestone `in_progress` with the status tool.
- Create a concrete implementation checklist in `RESULT.md`.
- Implement the smallest coherent vertical slices; keep each slice tested.
- Use specialist subagents when available for independent architecture, security,
  Rust, UI/accessibility and test review. Do not delegate final integration.
- Run focused tests after each slice, then all milestone gates.
- Fix findings. Do not lower a limit, skip a negative test or weaken a trust
  boundary merely to obtain green tests.
- Keep all source capsules immutable and work on disposable/new outputs.
- Record exact commands, results, changed paths, decisions and residual risks.
- Run an independent critic against `ACCEPTANCE.md`.
- Mark the milestone complete only when every required item is evidenced.
- Do not start the next milestone in the same change set unless the programme
  prompt explicitly asks for continuous execution and this milestone is clean.

## Hard prohibitions

- No lifecycle commands in the raw Wry renderer.
- No arbitrary SQL/process/filesystem surface from trusted-shell JavaScript.
- No in-place capsule transformations or destination overwrite.
- No Diagram Studio concepts in generic crates or format code.
- No silent alteration of v0.2 semantics.
- No claim of success without test evidence.

At completion, leave `RESULT.md` as a deterministic handoff: what changed, what
was tested, which acceptance clauses passed, and the exact first action for the
next milestone.

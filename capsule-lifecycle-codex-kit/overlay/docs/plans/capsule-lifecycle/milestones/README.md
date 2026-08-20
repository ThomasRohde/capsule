# Milestone bundles

Each milestone directory contains four durable files:

- `EXECPLAN.md` — outcome, scope, implementation sequence, tests and evidence;
- `PROMPT.md` — a self-contained Codex instruction;
- `ACCEPTANCE.md` — the release gate;
- `RESULT.md` — the mutable implementation/evidence handoff.

Work in numeric order. A milestone is complete only when its acceptance items are
evidenced and `PROGRAM_STATUS.json` is updated. The next milestone must be
resumable by a fresh session using repository files alone.

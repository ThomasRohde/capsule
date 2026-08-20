# Run the next Capsule lifecycle milestone

Read `PROGRAM_STATUS.json`.

- If a milestone is `in_progress`, resume it.
- Otherwise select the first `pending` milestone whose dependencies are complete.
- If no such milestone exists, validate programme consistency and report why.

Read that milestone's `EXECPLAN.md`, `PROMPT.md`, `ACCEPTANCE.md` and the previous
milestone's `RESULT.md`. Mark it in progress and implement only that milestone.

Do not start the following milestone. Complete the result/evidence/status handoff
and stop after reporting the acceptance decision.

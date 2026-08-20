# Resume the Capsule lifecycle programme

Reconstruct state from files, not conversation memory.

1. Read root `AGENTS.md` and `CONTRIBUTING.md`.
2. Run `python docs/plans/capsule-lifecycle/tools/codex_lifecycle/program_status.py show`.
3. Read the current milestone's `RESULT.md`, `EXECPLAN.md`, `ACCEPTANCE.md`, recent
   Git history and working-tree diff.
4. Verify that recorded evidence paths exist and that the last claimed focused
   tests still pass where inexpensive.
5. Identify unfinished acceptance clauses and the smallest coherent next slice.
6. Continue without redoing completed work or starting a dependent milestone.
7. Update result and status before ending the session.

Treat an inconsistent status/result/test state as a programme defect. Repair the
records or implementation before continuing.

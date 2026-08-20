Open the Capsule repository and implement the Capsule lifecycle programme
installed under `docs/plans/capsule-lifecycle/`.

Read, in order:

1. the repository root `AGENTS.md`;
2. `CONTRIBUTING.md`;
3. `CODEX_LIFECYCLE_START.md`;
4. the programme `README.md`, charter, target architecture and security model;
5. `PROGRAM_STATUS.json`;
6. the first milestone whose state is `pending` or `in_progress`.

Start with Milestone 0 even if the draft contracts look implementation-ready.
Capture the actual repository baseline, reconcile the proposal with current
code, and record design deviations before changing production code.

Work milestone by milestone. For each milestone:

- create or update its result file from the supplied template;
- maintain `PROGRAM_STATUS.json`;
- run the milestone-specific tests and the repository gates it affects;
- use a separate critic/security review when subagents are available;
- fix findings rather than merely documenting avoidable failures;
- leave input capsule fixtures untouched and use create-new paths;
- do not release lifecycle commands to the raw Wry renderer;
- do not continue past a failed acceptance gate.

Continue through the programme without asking me to restate the goal. When a
session must end, leave a precise, tested handoff in the programme status and
milestone result so another Codex session can resume deterministically.

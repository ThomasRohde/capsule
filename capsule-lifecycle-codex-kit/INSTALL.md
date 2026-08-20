# Installation

## 1. Prepare the repository

Use a clean checkout or a dedicated worktree. Do not install this planning
overlay into a tree containing unrelated uncommitted changes.

Record the current state:

```text
git status --short
git rev-parse HEAD
```

## 2. Preview the overlay

From the extracted kit:

```text
python scripts/install.py --repo C:\path\to\capsule --check
```

On macOS or Linux:

```text
python scripts/install.py --repo /path/to/capsule --check
```

The installer will propose these new top-level paths inside the repository:

```text
CODEX_LIFECYCLE_START.md
.agents/skills/capsule-lifecycle-program/
docs/plans/capsule-lifecycle/
```

It does not edit the existing root `AGENTS.md`. The installed skill explicitly
requires Codex to read and obey that file before working.

## 3. Install

```text
python scripts/install.py --repo <path-to-capsule>
```

If an identical file already exists it is left unchanged. If a different file
exists, installation stops and reports the conflict. Resolve the conflict
manually; do not use a broad force option.

## 4. Validate

From the repository root:

```text
python docs/plans/capsule-lifecycle/tools/codex_lifecycle/validate_lifecycle_specs.py
python docs/plans/capsule-lifecycle/tools/codex_lifecycle/capture_baseline.py
```

The second command writes:

```text
docs/plans/capsule-lifecycle/evidence/M00/baseline-<UTC timestamp>.json
```

## 5. Start Codex

Open the repository as the Codex project. Paste the contents of
`CODEX_LIFECYCLE_START.md`, or set it as a Goal in the Codex app.

For one milestone per session, use:

```text
docs/plans/capsule-lifecycle/prompts/RUN_NEXT_MILESTONE.md
```

For a context-reset handoff, use:

```text
docs/plans/capsule-lifecycle/prompts/RESUME_PROGRAM.md
```

## Removal

The overlay is deliberately isolated. Before implementation changes begin, it
can be removed by deleting only the three installed paths listed above. Once
Codex has implemented milestones, use Git history rather than deleting paths
blindly.

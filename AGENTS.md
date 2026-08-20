# Repository instructions

## First principle

This repository explores SQLite databases as self-describing application artefacts. Keep the generic concept, architecture, and format independent from the Diagram Studio example.

## Capsule discovery

When asked to run, inspect, modify, or explain a `.capsule.sqlite` file:

1. Treat the database as untrusted until it has been inspected and verified.
2. Start with `python tools/capsule.py instructions <capsule>` or query the `START_HERE` view using Python's standard-library `sqlite3` module.
3. Run `python tools/capsule.py verify <capsule>` before executing embedded application assets.
4. For this repository-owned example, execution is permitted with `--trust-capsule`. Do not apply that trust to unrelated external databases.
5. Follow the runbook embedded in the database when it differs from prose in the repository; it is versioned with the artefact.

## Source boundaries

- Product-independent vision: `docs/vision.md`.
- Product-independent runtime architecture and format: `docs/architecture.md`, `docs/format-contract.md`, `format/`, and `runtime/`.
- Example-specific requirements and code: `docs/example-diagram-studio.md` and `examples/diagram-studio/`.
- Generated distribution artefacts: `capsules/`. Do not hand-edit generated databases for durable source changes.

## Development rules

- Use only the Python standard library in the bootstrap runtime unless a later architecture decision explicitly permits a dependency.
- Keep the browser application free of runtime network dependencies.
- Do not expose a general SQL endpoint to browser code.
- Browser writes must use named, parameterised endpoints declared in the capsule.
- Bind the server to loopback only.
- Preserve the default-deny Content Security Policy.
- Treat internal hashes as integrity checks, not proof of publisher authenticity.
- Keep `plugins/capsule-creator/` synchronized with material changes to the
  capsule framework. Changes to the format contract, runtime or host behavior,
  security model, authoring workflow, or shared UI guidance must include a
  review of the plugin's skill instructions, references, scripts, templates,
  examples, and tests. Update every affected plugin surface and verify that the
  plugin still works from a standalone copy without repository access.
- Rebuild the example after changing embedded assets or data:
  `python tools/build_example.py`.
- Rebuild and export the current Windows NSIS installer only when native host
  or packaging changes can affect the installed binary. Use
  `python native/tools/build_installers.py`; the stable artifact must be
  `capsules/sqlite-capsule-host-setup.exe`. Capsule content, creator-plugin,
  documentation, and generated-capsule-only changes do not require an installer
  rebuild.
- Run all tests before considering a change complete:
  `python -m unittest discover -s tests -v`.
- Verify the generated capsule:
  `python tools/capsule.py verify capsules/diagram-studio.capsule.sqlite`.

## Agent execution efficiency

For lifecycle milestones and other long-running agent work:

- Keep one lifecycle milestone per Codex task and per commit. Finish, qualify,
  document and commit that milestone before starting the next one.
- The main agent owns integration and an exclusive Cargo/build lease. Never run
  overlapping Cargo, native build, installer or end-to-end test processes in a
  shared checkout.
- Use focused tests and package-scoped checks while implementation is changing.
  Freeze the source before review, run the full required qualification once,
  and rerun only affected gates after any subsequent production change.
- Use no implementation subagent by default. If delegation materially reduces
  elapsed time, use at most one implementation subagent at a time, give it a
  short bounded brief with disjoint file ownership, and stop it when its slice
  is integrated.
- Do not review a moving tree continuously. After source freeze, request one
  consolidated independent acceptance and security review. Add a second
  specialist only for a concrete unresolved risk, and allow at most one
  remediation review before the main agent decides the gate.
- Keep progress updates and retained evidence concise. Preserve full logs for
  failures or unique runtime evidence; summarize repeated passing gates by
  command, count and artifact path.
- Record valid out-of-scope findings in the next milestone's backlog instead of
  widening the active milestone.
- Rebuild generated capsules and the NSIS installer only at the existing
  definition-of-done boundary, after the relevant source has settled.

## Definition of done

A change is complete only when the relevant documentation is consistent, every
affected generated capsule rebuilds deterministically enough for review, tests
pass, the affected capsules verify, and the one-prompt Codex launch path still
works. Material framework changes must also leave the standalone creator plugin
current and verified. Native host or packaging changes additionally require the
current NSIS installer to be rebuilt, exported to
`capsules/sqlite-capsule-host-setup.exe`, and verified.

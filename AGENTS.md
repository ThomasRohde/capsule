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
- Rebuild the example after changing embedded assets or data:
  `python tools/build_example.py`.
- Run all tests before considering a change complete:
  `python -m unittest discover -s tests -v`.
- Verify the generated capsule:
  `python tools/capsule.py verify capsules/diagram-studio.capsule.sqlite`.

## Definition of done

A change is complete only when the relevant documentation is consistent, the example rebuilds deterministically enough for review, tests pass, the generated capsule verifies, and the one-prompt Codex launch path still works.

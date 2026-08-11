---
name: create-capsule
description: Create, extend, build, verify, and smoke-test product-independent SQLite Capsule applications from reviewable source. Use when Codex is asked to make a new capsule, turn an application or data brief into a `.capsule.sqlite` file, scaffold a capsule project, or author capsule assets, domain tables, named endpoints, checks, prompts, documents, and embedded runbooks. The workflow uses the repository's Python loopback host and does not require the native Tauri client.
---

# Create Capsule

Create a self-describing offline application whose canonical runtime artifact is SQLite. Keep the generic host and format independent from the application being authored.

## Workflow

1. Read the repository `AGENTS.md` and [authoring-contract.md](references/authoring-contract.md).
2. Turn the request into an application-specific source plan: identity, domain tables, seed data, browser assets, named reads/writes, checks, embedded docs, and agent prompts. Resolve important product choices from local context or ask only when a wrong assumption would materially change the application.
3. Scaffold a new source directory. Never replace an existing path implicitly:

   ```bash
   python plugins/capsule-creator/skills/create-capsule/scripts/capsule_project.py init <project-dir> --title "<title>" --app-id <reverse-domain-id>
   ```

4. Replace the starter domain with the requested application. Edit only reviewable files under the new project. Keep application-specific SQL and UI out of `format/`, `runtime/`, and the generic plugin implementation.
5. Build to a resolved destination. Use `--replace` only when that exact generated target is intentionally being regenerated:

   ```bash
   python plugins/capsule-creator/skills/create-capsule/scripts/capsule_project.py build <project-dir> <output.capsule.sqlite>
   ```

6. Inspect the embedded `START_HERE` projection and run independent repository checks:

   ```bash
   python tools/capsule.py instructions <output.capsule.sqlite>
   python tools/capsule.py verify <output.capsule.sqlite>
   python tools/capsule.py conformance <output.capsule.sqlite>
   python plugins/capsule-creator/skills/create-capsule/scripts/capsule_project.py check <project-dir> <output.capsule.sqlite>
   ```

7. Treat the new database as untrusted until the built artifact and executable assets have been reviewed. After that deliberate decision, run only the Python loopback host:

   ```bash
   python tools/capsule.py start <output.capsule.sqlite> --trust-capsule
   python tools/capsule.py status <output.capsule.sqlite>
   ```

   Exercise the real browser flow, including one declared write when the application is editable, reload it, and confirm persistence directly in SQLite. Stop the matching host with `python tools/capsule.py stop <output.capsule.sqlite>`.
8. Run the repository's relevant tests and generated-artifact gates. Report the capsule path, identity, SHA-256, verification result, runtime evidence, and any unresolved acceptance gap.

## Guardrails

- Use Python's standard library and the repository's generic `tools/`, `format/`, and `runtime/` sources.
- Do not invoke or import the Tauri desktop client, Rust crates, native IPC, Cargo, WebView2, or installer artifacts as part of creating, building, verifying, or running a capsule.
- Keep browser assets offline. Do not add remote scripts, fonts, styles, images, analytics, or API calls.
- Expose only enabled named parameterized endpoints. Never expose a general SQL or filesystem bridge to browser code.
- Preserve loopback-only binding, explicit trust, default-deny CSP, resource ceilings, atomic writes, and refusal to overwrite unspecified targets.
- Treat internal hashes as integrity evidence, not publisher authentication.
- Do not hand-edit a generated `.capsule.sqlite` file for durable source changes.

## Existing capsules

For an existing or externally supplied capsule, inspect `START_HERE` and verify it before unpacking. Use `python tools/capsule.py unpack`, edit the deterministic bundle, and pack to a new output. Do not use this starter-project script to silently reinterpret another capsule's source model.

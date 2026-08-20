---
name: create-capsule
description: Create, extend, build, verify, run, and black-box test exceptional product-independent SQLite Capsule applications from reviewable source. Use when Codex is asked to make a new capsule, turn an application or data brief into a `.capsule.sqlite`, scaffold a capsule project, design named database interfaces, create a self-contained offline browser application, or inspect a Capsule with the bundled reference Inspector. The plugin includes its own format, Python runtime, conformance spec, SQLite WASM engine, Fluent UI reference, and example; it needs no repository checkout or Tauri client.
---

# Create Capsule

Build a self-describing offline application whose canonical runtime artifact is
SQLite. Resolve `<skill>` as the directory containing this file. All commands,
references, format assets, and runtime assets are relative to it.

## Load only the references the work needs

- Always read [authoring-contract.md](references/authoring-contract.md) and
  [quality-playbook.md](references/quality-playbook.md).
- Read [format-and-runtime.md](references/format-and-runtime.md) when changing
  domain SQL, endpoints, permissions, checks, build behavior, or trust/runtime
  behavior.
- Read [fluent-ui.md](references/fluent-ui.md) for a Windows 11/Fluent UI or when
  no visual direction is supplied.
- Read [inspector-black-box.md](references/inspector-black-box.md) when inspecting
  another database, adding import/file behavior, or preparing black-box evidence.

The normative machine resources are under `assets/`; do not substitute guessed
schemas or download a runtime.

The no-flag authoring path remains Capsule format v0.2 for compatibility. Choose
v0.3 explicitly only when the capsule needs separate application/instance
identity and lifecycle dataset contracts:

```text
python <skill>/scripts/capsule_project.py init <project> --title "<title>" --app-id <reverse-domain-id> --format-version 0.3
```

Add `--template` only for an intentionally clean v0.3 seed project. The
builder derives its template-state document from the actual seed database; it
never trusts handwritten row counts or hashes. That proof becomes authoritative
only after the resulting application compartment is signed.

When choosing v0.3 dataset policies, review the native lifecycle truth table in
`references/authoring-contract.md`: `copy` cannot be downgraded, sensitive
`prompt` inclusion needs explicit confirmation, `forbid` blocks every semantic
mode, and fork/selective `reset` is deliberately unavailable without a separate
clean source. The standalone plugin authors and verifies these declarations but
does not execute lifecycle copies or require the Tauri host.
Treat `compare_policy` as a signed disclosure ceiling too: prefer `summary` or
`row` unless bounded field values are genuinely required, and remember that
sensitive values still need an explicit trusted-shell reveal.
The separate Application expansion is value-free and fixed by the trusted
host; author metadata cannot select application tables or expose their values.
Treat `reconcile_policy` as signed transformation authority: `ignore` excludes,
`forbid` blocks, `manual` allows explicit two-way choices, and `three-way`
allows automatic clean-change classification only with a separately verified
exact ancestor. Three-way requires row/field comparison. Keep timestamps and
non-semantic noise ignored, and mark identity/ownership columns immutable;
immutable conflicts can only keep the target. The plugin never applies a
reconciliation or accepts lineage as ancestor proof.
For a same-schema application-upgrade target, author an intentionally clean
signed v0.3 `--template` release with the same application, exact schema and
dataset/table/key/dependency structure, a strictly newer SemVer version, and an
Ed25519 key accepted for the working release. Review every signed
`upgrade_policy`: `copy` carries working state, `target`/`rebuild` retain
authenticated target state, `omit` stays empty, and `migrate`/`forbid` make M07
fail closed. The plugin never executes an upgrade or treats publisher names or
mutable lineage as key
authority.

## Workflow

1. Turn the request into a compact product/source plan: user job, domain model,
   lifecycle states, seed data, named reads/writes, invariants, browser pages,
   signature interaction, embedded knowledge, and acceptance evidence.
2. Scaffold without replacing an existing path:

   ```text
   python <skill>/scripts/capsule_project.py init <project> --title "<title>" --app-id <reverse-domain-id>
   ```

3. Replace the starter with application-specific reviewable source. Keep generic
   builder/runtime code free of product logic. For a sophisticated example,
   inspect `assets/examples/capsule-inspector/`; copy patterns, not its domain.
   Give every table an explicit primary key, do not use triggers or virtual
   tables, and list suffix-classified pure-content files in
   `capsule-project.json` under `non_executable_assets`. For v0.3, also keep
   `source/data-contract.json` exhaustive: classify every ordinary domain table
   in exactly one dataset and declare its real ordered primary key.
4. Build to a resolved output. Use `--replace` only when intentionally
   regenerating that exact file:

   ```text
   python <skill>/scripts/capsule_project.py build <project> <output.capsule.sqlite>
   ```

5. Inspect and verify with the bundled, repository-independent tools:

   ```text
   python <skill>/scripts/capsule_project.py host instructions <output.capsule.sqlite>
   python <skill>/scripts/capsule_project.py host verify <output.capsule.sqlite>
   python <skill>/scripts/capsule_project.py conformance <output.capsule.sqlite>
   python <skill>/scripts/capsule_project.py check <project> <output.capsule.sqlite>
   ```

6. Review executable assets and make an explicit trust decision. Then launch
   only the bundled Python loopback host:

   ```text
   python <skill>/scripts/capsule_project.py host start <output.capsule.sqlite> --trust-capsule
   python <skill>/scripts/capsule_project.py host status <output.capsule.sqlite>
   ```

7. Exercise the real browser. Cover initial/loading/error states, the core read
   path, responsive layout, and console/network evidence. For editable apps,
   perform a named write, reload, and confirm both the domain row and
   `capsule_change_log` directly in SQLite. Stop the matching host with `host
   stop`.
8. Use the Inspector as a consumer-side compatibility test: open the new capsule
   and confirm its four X-ray stages, identity, hashes, interfaces, guidance,
   and domain schema without executing the target.
9. Report output path, identity, SHA-256, verification/conformance/freshness,
   runtime evidence, persistence evidence when applicable, and every untested
   acceptance row.

## Guardrails

- Use the bundled Python standard-library runtime. Do not require the source
  repository, Tauri, Rust, Cargo, WebView2 IPC, installers, or native globals.
- Keep every browser asset embedded and offline: no CDN, remote font, analytics,
  image host, API, or runtime package download.
- Expose only enabled named parameterised endpoints. Never add raw SQL,
  filesystem, shell, environment, or general native bridges to browser code.
- Preserve loopback-only binding, explicit trust, default-deny CSP, bounded
  resources, atomic writes, exact asset hashes, and refusal to overwrite
  unspecified targets.
- Keep schema portable: explicit table primary keys only, with no triggers or
  virtual tables.
- Do not add lifecycle migration endpoints. V0.3 migrations are signed,
  host-interpreted `copy_rows`, `copy_dataset`, or `discard_dataset`
  declarations; the application endpoint engine is outside that boundary.
- Treat internal hashes as integrity evidence, never publisher authentication.
- Render database/untrusted text with safe DOM text APIs.
- For v0.3 application or instance artwork, use only hash-valid static PNG/WebP
  within the 512 KiB, 1024-by-1024, and 4 MiB decoded host ceilings; never use
  SVG, remote/data URLs, animation, or Cabinet state as authority.
- Do not hand-edit generated capsules for durable changes.

## Existing capsules

Treat an external `.capsule.sqlite` as untrusted. Prefer the bundled Inspector
for inert black-box review. Use `host instructions` and `host verify` before
extracting or executing anything. If source reconstruction is required, unpack
to a new reviewable directory and publish a new output; never silently reinterpret
or overwrite the supplied artifact.

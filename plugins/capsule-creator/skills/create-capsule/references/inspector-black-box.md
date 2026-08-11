# Capsule Inspector black-box reference

The plugin carries a complete reference project and built artifact:

- source: `assets/examples/capsule-inspector/`
- capsule: `assets/examples/capsule-inspector.capsule.sqlite`

Use it as a quality example for a sophisticated read-only Capsule, a Fluent UI
example, and a black-box compatibility test for newly created capsules.

## Boundary

The Inspector loads a user-selected file into the pinned official
`@sqlite.org/sqlite-wasm` `3.53.0-build1` engine in browser memory. It has a
64 MiB input ceiling, opens the virtual file read-only, enables query-only and
trusted-schema-off modes, and runs fixed catalogue queries authored by the
Inspector.

File and asset hashes use Web Crypto when `crypto.subtle` is available. Native
capsule origins can expose only the non-subtle portion of Web Crypto, so the
Inspector includes a deterministic portable SHA-256 implementation as its
offline fallback. This compatibility path does not weaken or skip integrity
comparison.

The pinned SQLite engine is WebAssembly. A host which serves this example must
retain a default-deny policy while allowing `script-src 'self'
'wasm-unsafe-eval'`; plain `script-src 'self'` blocks WebAssembly compilation.
Do not substitute JavaScript `unsafe-eval`. The plugin's bundled Python host and
the current SQLite Capsule Windows host both implement the narrow exception.

It reads:

- file size and SHA-256;
- SQLite header, page geometry, quick check, application/user versions;
- schema object names and foreign-key issues;
- Capsule manifest identity and required platform object presence;
- asset bytes and SHA-256 comparisons;
- endpoint metadata and parameter declarations;
- runbooks, docs, prompts, commands, and checks as text;
- application-owned versus platform-owned schema objects.

It never mounts or executes the target entry asset, JavaScript, commands,
prompts, runbooks, or declared check SQL. It never exposes a raw SQL editor,
writes the selected file, or sends target bytes over the network. Database text
is rendered with `textContent`.

## Four-stage X-ray model

1. **File** — bounded bytes, SQLite header, SHA-256.
2. **SQLite** — readable pages, quick check, foreign keys, page geometry.
3. **Contract** — Capsule identity, current format, required tables, entry
   asset, offline declaration, asset hashes.
4. **Application** — named interfaces, embedded guidance, and domain schema.

A green rail means the inspected signals are coherent. The verdict explicitly
states that black-box inspection does not authenticate a publisher and does not
execute the target's declared checks. Full verification is still required
before a trust decision.

## Run it with plugin-only tools

```text
python <skill>/scripts/capsule_project.py host verify <skill>/assets/examples/capsule-inspector.capsule.sqlite
python <skill>/scripts/capsule_project.py conformance <skill>/assets/examples/capsule-inspector.capsule.sqlite
python <skill>/scripts/capsule_project.py check <skill>/assets/examples/capsule-inspector <skill>/assets/examples/capsule-inspector.capsule.sqlite
python <skill>/scripts/capsule_project.py host start <skill>/assets/examples/capsule-inspector.capsule.sqlite --trust-capsule
```

In the browser, choose the capsule under test. Confirm that the title remains
“Capsule Inspector,” the target is identified by manifest fields, asset hashes
are reported, and no target-specific UI or dialog appears. Exercise Assets,
Interfaces, Guidance, and Schema pages. Then stop the matching host.

## Adversarial test shape

For stronger release evidence, build a disposable target whose entry HTML and
JavaScript would visibly set a sentinel or open a dialog if executed, and whose
docs contain HTML-like text. Inspect it and assert:

- no dialog appears and the sentinel is absent;
- the Inspector title/navigation remain mounted;
- hostile text appears only as text;
- the target file hash is unchanged before/after;
- network requests stay on the Inspector's loopback origin;
- malformed, non-SQLite, empty, and oversized inputs produce bounded errors.

Do not weaken the Inspector to make a malformed target look valid. A generic
SQLite database should remain inspectable but receive a clearly non-Capsule
verdict.

## Vendored engine

The plugin bundles the module, WASM binary, Apache-2.0 license, and third-party
notice under `assets/runtime/browser/vendor/sqlite-wasm/`. The example source
contains its own byte-identical copy so its built capsule has no runtime
download. Do not update the version or hashes casually; treat an engine update
as a compatibility and browser-matrix change.

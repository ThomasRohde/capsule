# Authoring, runtime, and distribution

SQLite Capsule intentionally has more than one useful representation, but only one runtime source of truth.

| Representation | Purpose | Authority |
|---|---|---|
| Application authoring source | Review HTML, CSS, JavaScript, docs, endpoint declarations, and seed data in a normal repository | Authoritative for the next clean application release |
| `.capsule.sqlite` | Run, inspect, edit, and distribute the complete application | Authoritative for the state of that concrete runtime artefact |
| Unpacked authoring bundle | Review or reconcile arbitrary schema, rows, and assets without application-specific tooling | Semantic round-trip representation of one capsule snapshot |
| Self-contained HTML export | Open a verified capsule through the pinned browser-only SQLite WASM host | Derivative revision with immutable source provenance and its own current database payload |
| HTML, SVG, PNG, PDF, or other exports | Reach recipients or capture a view | Derivative output with source provenance; never silently canonical |

## Clean application releases

Diagram Studio's clean release is assembled from reviewable source:

```bash
python tools/build_example.py
python -m unittest discover -s tests -v
python tools/capsule.py verify capsules/diagram-studio.capsule.sqlite
python tools/build_example.py --check
```

The final command rebuilds independently and compares the exact digest. A release capsule with ad hoc runtime edits or a stale embedded host fails that gate.

## Create a new application with the Codex plugin

The `plugins/capsule-creator` plugin supplies a `create-capsule` skill and a
deterministic standard-library project tool. The plugin is self-contained: its
format, host, browser client, conformance description, references, and pinned
SQLite WASM travel with it. It is for new application-specific source trees;
it does not add product logic to the generic host, require this repository, or
depend on the native Tauri client.

The no-flag path continues to author format v0.2. Choose v0.3 explicitly when
the application needs separate immutable application and mutable instance
metadata plus an exhaustive dataset lifecycle contract. V0.3 projects include
`source/data-contract.json`; every ordinary domain table must be classified in
exactly one dataset.

```bash
python plugins/capsule-creator/skills/create-capsule/scripts/capsule_project.py init my-app --title "My App" --app-id org.example.my-app
python plugins/capsule-creator/skills/create-capsule/scripts/capsule_project.py init my-v03-app --title "My v0.3 App" --app-id org.example.my-v03-app --format-version 0.3
python plugins/capsule-creator/skills/create-capsule/scripts/capsule_project.py build my-app my-app.capsule.sqlite
python plugins/capsule-creator/skills/create-capsule/scripts/capsule_project.py host instructions my-app.capsule.sqlite
python plugins/capsule-creator/skills/create-capsule/scripts/capsule_project.py host verify my-app.capsule.sqlite
python plugins/capsule-creator/skills/create-capsule/scripts/capsule_project.py conformance my-app.capsule.sqlite
python plugins/capsule-creator/skills/create-capsule/scripts/capsule_project.py check my-app my-app.capsule.sqlite
```

For an intentionally clean v0.3 seed release, add `--template`. The builder
derives an exhaustive template-state proof from the generated dataset rows;
authors classify each dataset as `seed` or `empty` but never provide proof
hashes. The proof becomes usable only after signing and host-side reproduction
against the exact verified snapshot. Ordinary signed releases are not inferred
to be templates.

For reconciliation, author each dataset's signed policy deliberately:

- `ignore` excludes the dataset from reconciliation;
- `forbid` blocks every reconciliation transform;
- `manual` permits only explicit two-way row/field selections;
- `three-way` permits clean-change classification only with a separately
  verified exact ancestor and therefore requires `compare_policy` `row` or
  `field`.

Every reconciled table needs an ordered explicit primary key. Put timestamps or
other intentionally non-semantic noise in `ignored_columns_json`; these columns
cannot be selected or compared. Put identity and ownership fields in
`immutable_columns_json`; three-way divergence there can only keep the target.
Dependencies must reflect every cross-dataset reference. The creator plugin
authors and verifies this signed authority but never reconciles files itself;
the native host always creates a new target-derived copy.

The generated source separates stable identity, domain SQL, seed rows, offline
browser assets, named endpoints, application checks, prompts, documents, and
runbooks. The builder embeds its bundled generic Python host and browser client,
publishes atomically after full verification, and refuses implicit replacement.
All `.html`, `.js`, `.mjs`, `.py`, and `.wasm` app files are executable by
default. Pure-content files with one of those suffixes can be removed from the
signed executable surface by listing their `app/...` paths in
`capsule-project.json` under `non_executable_assets`; the entry asset cannot be
excluded.
After reviewing the result and making an explicit trust decision, exercise it
through `capsule_project.py host start ... --trust-capsule` and confirm the real
loopback application and write persistence before distribution.

The plugin also carries a reviewable Capsule Inspector project and built
artifact under `assets/examples/`. It uses a Windows 11 Fluent browser UI and a
pinned in-memory SQLite engine to inspect another database without mounting or
executing the target's browser code. Use it as a consumer-side compatibility
gate for newly authored capsules.

## Working runtime artefacts

Named write endpoints deliberately mutate the same capsule file. That working file is authoritative for its own user state. Restarting the host must reproduce the edit.

Do not rebuild over a working file merely to recover application source changes. First unpack and compare it:

```bash
python tools/capsule.py unpack working.capsule.sqlite working-bundle
python tools/capsule.py diff clean.capsule.sqlite working.capsule.sqlite
```

The semantic diff identifies changed keys without dumping unbounded row content.

## Generic unpack and pack

`unpack` is read-only with respect to the capsule and refuses an existing output path. It writes deterministic metadata, schema objects, JSONL table rows, typed BLOB values, and content-addressed assets. It also refuses tables without an explicit primary key, because implicit `rowid` identity cannot be round-tripped without silently renumbering rows.

`pack` treats the bundle as untrusted input. It rejects triggers and virtual
tables before reconstruction, validates other schema-object declarations,
prevents bundle path escape, verifies referenced file sizes and hashes,
reconstructs into a temporary database, checks foreign keys, runs the complete
capsule verifier, and publishes only on success. It refuses an existing output
unless `--replace` is explicit.

Repeated packs from one bundle are deterministic in the supported environment. A semantic round trip is required to compare equal; the output is not required to reproduce the historical page layout of an unrelated source database.

Format-hardening tools are independent of the authoring bundle: `conformance`
checks the platform contract from a machine-readable description,
and `permissions` reports requested capabilities plus host-managed grants. These tools
are inspection-only with respect to the capsule.

## Publisher signing

Publisher signing is a separate release operation, never an implicit effect of
`pack` or a domain write. The native CLI verifies structure, creates a new
destination with SQLite's backup API, adds or checks the signed-app extension,
signs it, atomically publishes the new file, then reopens and verifies it. It
refuses an existing or in-place output by default.

```text
capsule-native sign source.capsule.sqlite signed.capsule.sqlite \
  --publisher-id org.example --publisher-name "Example Publisher" \
  --key protected-seed-file --signed-at 2026-08-08T12:34:56Z
capsule-native verify-signature signed.capsule.sqlite
python tools/capsule.py signatures signed.capsule.sqlite \
  --native-verifier path/to/capsule-native
```

Raw key files are a development adapter, not the production key-management
recommendation. Release publishers should use a reviewed hardware/KMS or
protected CI signer. The only repository seed is public, deterministic, clearly
labelled test material under `compatibility/signed-app-v0.2/`; it confers no
identity or trust. The v0.3 interoperability vector lives separately under
`compatibility/signed-app-v0.3/` and uses a distinct signing profile/context.

The trusted desktop shell also exposes **Publisher signing** for a local,
use-once key workflow. Its native pickers accept the existing 32-byte seed and
64-hex-digit seed formats plus interoperable Ed25519 PKCS#8 PEM or DER. The key
file path and private bytes remain in Rust; the bundled WebView receives only
the file name, format, public key, and key fingerprint. The shell verifies the
source without executing embedded assets, selects a new destination, prepares
the exact same-directory temporary copy and application digest for review, and
requires a second explicit action to sign. It then consumes the in-memory key,
reopens and verifies the result, and atomically publishes only to a previously
absent destination. Clearing the session drops the key and removes any prepared
temporary copy. Encrypted PKCS#8, persistent key storage, and remote/KMS signing
are deliberately not part of this use-once adapter.

The CLI and desktop both call `sqlite-capsule-signing`; neither reimplements the
signed-extension SQL or copy/publish sequence. The Python environment-variable
wrapper remains the non-interactive CI path.

Signing dispatches only after the complete v0.2 or v0.3 format tuple is known.
Preparation holds one verified read-only connection across exhaustive
conformance and declared checks, binds an exact private snapshot to the reviewed
source digest, and re-verifies that snapshot before adding extension rows.

### Automated release signing

`tools/sign_release.py` wraps the native signer with source verification,
fail-closed output handling, native signature verification, and an independent
signature inventory. Command-line arguments take precedence over the matching
environment variables:

| Environment variable | Meaning |
| --- | --- |
| `SQLITE_CAPSULE_PUBLISHER_ID` | Required stable publisher identifier |
| `SQLITE_CAPSULE_PUBLISHER_NAME` | Required publisher display name |
| `SQLITE_CAPSULE_SIGNING_KEY_FILE` | Preferred path to a mounted 32-byte, 64-hex-digit, or Ed25519 PKCS#8 PEM/DER seed file |
| `SQLITE_CAPSULE_SIGNING_KEY_HEX` | CI-only alternative containing exactly 64 hexadecimal digits |
| `SQLITE_CAPSULE_SIGN_SOURCE` | Source capsule; defaults to the canonical Diagram Studio capsule |
| `SQLITE_CAPSULE_SIGN_OUTPUT` | New output path; defaults under ignored `output/` |
| `SQLITE_CAPSULE_NATIVE_CLI` | Optional explicit `capsule-native` executable |
| `SQLITE_CAPSULE_SIGNED_AT` | Optional reproducible exact UTC second; current UTC is used when absent |

Set exactly one key source. A file-backed or CI-mounted secret is preferred
because ordinary environment variables can be exposed by process inspection,
crash reporting, or careless logging. When the hex fallback is necessary, the
wrapper writes it to an exclusive private temporary file, zeroes its decoded
buffer, removes the file on exit, and ensures the secret variable is not passed to
child processes. It never prints either key representation.

PowerShell example:

```powershell
$env:SQLITE_CAPSULE_PUBLISHER_ID = 'com.example'
$env:SQLITE_CAPSULE_PUBLISHER_NAME = 'Example Publisher'
$env:SQLITE_CAPSULE_SIGNING_KEY_FILE = 'C:\protected\publisher.seed.hex'
$env:SQLITE_CAPSULE_SIGN_OUTPUT = 'output\diagram-studio.signed.sqlitecapsule'
python tools/sign_release.py
```

For reproducible CI metadata, also set `SQLITE_CAPSULE_SIGNED_AT` to the release
timestamp in exact `YYYY-MM-DDTHH:MM:SSZ` form. The output path must not already
exist; the automation intentionally preserves the native signer's no-replacement
and no-in-place-signing rules. A successful JSON report states
`signature_valid: true` but keeps publisher trust false because signing and a
host-local user trust decision are separate operations.

## Self-contained HTML derivatives

The HTML export contract is independent of the SQLite capsule format. Exporting
first verifies the source, resolves the bounded static entry-asset subset, reports
dynamic network-looking tokens without following them, and emits no remote URL.

```bash
python tools/capsule.py export-html source.capsule.sqlite output.html --profile view
python tools/capsule.py inspect-html output.html
python tools/capsule.py verify-html output.html
```

Pass `--replace` only after resolving the exact target. Initial exports are
deterministic and support `--check`. Browser-saved editable revisions are
structurally canonical and independently verifiable, but browser gzip/HTML
serialization is not claimed to be byte-identical across engines.

`view` and `interactive` are worker-enforced read-only profiles. `editable`
stores writes in an in-memory database until an explicit user-picked Save HTML
or fallback download creates a complete next revision. The original capsule and
initial HTML are never silently mutated.

## Version boundaries

The repository supports the current capsule format only. Self-contained HTML
uses the separate `org.sqlite-capsule.html-export/0.2` derivative contract and
does not change the database format version. Hosts must not invent implicit
migrations or treat HTML as a new canonical database format.

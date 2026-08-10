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

## Working runtime artefacts

Named write endpoints deliberately mutate the same capsule file. That working file is authoritative for its own user state. Restarting the host must reproduce the edit.

Do not rebuild over a working file merely to recover application source changes. First unpack and compare it:

```bash
python tools/capsule.py unpack working.capsule.sqlite working-bundle
python tools/capsule.py diff clean.capsule.sqlite working.capsule.sqlite
```

The semantic diff identifies changed keys without dumping unbounded row content.

## Generic unpack and pack

`unpack` is read-only with respect to the capsule and refuses an existing output path. It writes deterministic metadata, schema objects, JSONL table rows, typed BLOB values, and content-addressed assets.

`pack` treats the bundle as untrusted input. It validates schema-object declarations, prevents bundle path escape, verifies referenced file sizes and hashes, reconstructs into a temporary database, checks foreign keys, runs the complete capsule verifier, and publishes only on success. It refuses an existing output unless `--replace` is explicit.

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
identity or trust.

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

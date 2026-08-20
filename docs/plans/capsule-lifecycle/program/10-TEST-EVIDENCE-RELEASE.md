# Test, evidence and release strategy

## Test philosophy

Lifecycle code handles untrusted files and creates durable outputs. Happy-path
unit tests are insufficient. Each milestone must add:

- contract/conformance tests;
- deterministic fixtures;
- failure-path and adversarial tests;
- cross-implementation tests where the format changes;
- native trusted-shell tests;
- negative raw-renderer capability tests;
- process-termination tests at durable boundaries;
- evidence describing what was actually run.

## Required fixture matrix

### Format

- minimal valid v0.2 unsigned;
- valid v0.2 signed;
- minimal valid v0.3 unsigned;
- valid v0.3 signed;
- v0.3 with rich metadata and safe icon;
- malformed/oversized profile and icon;
- unknown platform table;
- bad signature and changed mutable instance metadata;
- changed signed application metadata.

### Data contracts

- all dataset roles;
- missing table classification;
- duplicate table classification;
- mismatched primary key;
- sensitive and dependent datasets;
- derived/cache datasets;
- immutable/ignored columns.

### Lineage

- created;
- exact duplicate;
- fork;
- reconcile with two parents;
- application upgrade;
- malformed parent references and sequence.

### Compare/reconcile

- identical;
- add/remove/update;
- type differences (`INTEGER`, `REAL`, `TEXT`, `BLOB`, `NULL`);
- composite primary keys;
- large bounded data;
- source replacement;
- two-way conflict;
- three-way clean change and conflict;
- constraint failure;
- process crash before/after transaction and publication.

### Upgrade/migration

- same-schema upgrade;
- version 2→3→4 unique path;
- missing and ambiguous path;
- publisher mismatch;
- permission increase;
- source schema mismatch;
- invalid transform;
- row/byte/time limit;
- target app digest preservation;
- v0.2 legacy source to v0.3 target.

## Gate hierarchy

### Fast local gate

```text
python docs/plans/capsule-lifecycle/tools/codex_lifecycle/validate_lifecycle_specs.py
python tools/build_example.py --check
python tools/build_exports.py --check
python -m unittest discover -s tests -v
python tools/capsule.py verify capsules/diagram-studio.capsule.sqlite
```

### Rust gate

From `native/`:

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
python tools/generate_sbom.py --check
python tools/generate_license_report.py --check
```

### Browser/native gate

From repository root after `npm ci`:

```text
npm run test:browser
npm run test:browser:html
npm run test:native:prepare
npm run test:native:prepare:check
npm run test:native
npm run test:native:window
npm run test:native:raw
```

Use the exact current scripts discovered in M00 if names have changed.

### Generated artefact and plugin gate

- rebuild Diagram Studio capsule;
- regenerate exports;
- verify deterministic checks;
- update conformance vectors;
- copy material framework changes into `plugins/capsule-creator`;
- run the plugin from a standalone temporary copy with no repository access;
- black-box inspect a target capsule without execution.

### Installer gate

Native host or packaging changes that affect the installed binary require the
repository-prescribed installer rebuild:

```text
python native/tools/build_installers.py
```

The stable ignored output must be verified at the path required by the root
`AGENTS.md`.

## Milestone evidence

Each milestone writes:

```text
milestones/<id>/RESULT.md
evidence/<id>-tests.json
evidence/<id>-security-review.md
```

`RESULT.md` contains:

- baseline commit;
- files changed;
- decisions made;
- tests and exact results;
- generated artefacts;
- known gaps;
- next-milestone handoff.

Test evidence JSON uses the supplied template and includes command, working
directory, exit code, timestamp, duration when available and artefact hashes.

## Release acceptance

M09 is not complete until a clean checkout reproduces:

- all generated artefacts;
- Python/Rust/browser/native gates;
- plugin standalone operation;
- installer build required by repository policy;
- a manual UX walkthrough for Overview, Fork, Compare, Reconcile and Upgrade;
- a threat-model review with no unresolved critical/high findings;
- documented remaining platform gaps without overstating support.

# Fixture, adversarial and benchmark strategy

## 1. Fixture classes

### Golden compatibility fixtures

Small deterministic checked artefacts proving stable format/signature behaviour.
They include source inputs and expected digest/result metadata.

### Generated example fixtures

Diagram Studio capsules built from reviewable source:

- clean application release;
- working instance;
- blank/template instance;
- fork with all permitted data;
- fork with sensitive history omitted;
- common ancestor plus left/right branches;
- expected reconcile output;
- old/new releases with unchanged schema;
- old/new releases with v1→v2 schema migration.

Builders require a `--check` mode and fixed timestamps/IDs in test fixtures.

### Adversarial fixtures

Minimal capsules isolating one fault:

- malformed singleton/table/schema/JSON/hash/signature;
- unknown/future format;
- oversized text/blob/icon/schema;
- duplicate/undeclared dataset table;
- bad PK/collation/dependency cycle;
- sensitive dataset;
- stale/replaced source;
- migration graph/operation/type errors;
- canary application asset/endpoint/command that records any execution attempt.

### Scale fixtures

Generated at test time from deterministic seeds:

- 10, 1,000 and 100,000 rows;
- composite keys;
- bounded 1 KiB/1 MiB blobs;
- long text;
- many datasets/tables;
- deep but permitted lineage;
- comparison with sparse and dense changes.

Do not commit unnecessarily large binaries.

## 2. Fixture metadata

Every fixture has reviewable metadata:

```json
{
  "profile": "org.sqlite-capsule.fixture/1",
  "id": "diagram-branches-small",
  "purpose": "Three-way reconciliation conflict classification",
  "builder": "tools/build_lifecycle_fixtures.py",
  "inputs": ["..."],
  "expected_sha256": "...",
  "expected": {
    "inspect": "valid",
    "application_digest": "...",
    "compare_summary": "..."
  }
}
```

## 3. Immutability discipline

Test inputs live under a read-only/golden directory. Tests copy only when runtime
behaviour requires a writable working instance. Every lifecycle test captures
input SHA-256 before and after.

The checked `capsules/diagram-studio.capsule.sqlite` remains a generated release
artefact; write experiments use `.tmp/` copies.

## 4. Cross-implementation matrix

| Capability | Python | Rust CLI/core | Tauri shell | Creator plugin |
| --- | --- | --- | --- | --- |
| v0.2 inspect/verify | required | required | required | snapshot verify |
| v0.3 inspect/verify | required | required | required | required |
| signed-app digest vectors | required | required | consumes | required |
| data-contract validation | author/build | required | consumes | required |
| copy/compare/upgrade | optional/reference | normative | normative UX | scaffold/test |
| migration declaration | author/validate | normative interpreter | review/execute | scaffold/validate |

If Python lifecycle transforms are added, they must share contract fixtures but
must not become an unreviewed alternate security model.

## 5. Performance measures

Measure separately:

- input hashing throughput;
- Overview inspection latency;
- icon decode latency/memory;
- duplicate and compact duplicate throughput;
- compare summary and detail throughput;
- reconciliation rows/second;
- same-schema and migrated upgrade;
- peak resident memory;
- cancellation responsiveness;
- temporary/output disk amplification.

Record hardware, filesystem and antivirus context. Publish supported limits and
observations, not universal performance claims.

## 6. Baseline service levels

Initial engineering targets, subject to M00/M05 measurement:

- Overview for a normal small capsule: perceptually immediate and cancellable;
- compare summary should stream progress and remain within host memory ceilings;
- cancellation observed within a bounded interval;
- UI remains responsive while hashing/compare/migration occurs off the UI thread;
- no operation accepts a capsule declaration that raises host hard limits.

Targets are not security invariants. Exceeding a performance target may be a
product defect; exceeding a hard resource limit must fail closed.

## 7. Crash injection

Expose test-only stage hooks in the workspace/publication layer rather than
sprinkling process exits throughout production code. Stages:

```text
after_input_bind
after_plan
after_temp_create
after_base_copy
after_dataset
after_lineage
after_verify
after_fsync
before_publish
after_publish
after_reopen
```

Tests terminate or inject an error at each stage and inspect inputs, destination,
temp/recovery state and next-start behaviour.

## 8. Fuzzing/property tests

Prioritise pure parsers/canonicalisers:

- format/conformance metadata;
- data contracts;
- canonical JSON/typed values;
- compare tokens and reports;
- reconciliation plans;
- migration graphs and operation definitions;
- icon metadata/decoder wrapper;
- path and filename validation.

Seed corpora include all golden and adversarial fixtures. Fuzzing supplements,
not replaces, semantic integration tests.

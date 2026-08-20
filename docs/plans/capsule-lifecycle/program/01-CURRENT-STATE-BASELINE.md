# Current-state baseline

Prepared from public `main` as observed on 12 August 2026 at commit:

```text
f67da560fb4baaa13144cea220c9329df87ad534
```

Codex must regenerate a local baseline before implementation because the
repository may have advanced.

## M00 live reconciliation

M00 observed `e73cf948fba233ef84d4680930b61549012020a7` on 12 August
2026, three commits beyond the package baseline. Those commits update Diagram
Studio editing/runtime tests and deterministically regenerate the example and
HTML exports; they do not change the lifecycle programme's application/instance
or workspace boundaries. Exact dirty state, toolchains and hashes are retained
under `evidence/M00/`.

Live inspection also found two pre-existing contract gaps that M01 must address
before adding v0.3: native metadata inspection admits 512 MiB while the current
cross-host contract is 64 MiB, and native first-open/signing label a shallow
metadata/integrity/FK pass as structure verification before exhaustive runtime
conformance. [ADR 0028](../../../decisions/0028-verification-phases-and-size-policy.md)
freezes the fail-closed correction.

## Existing strengths to preserve

- Capsule format v0.2 with a discoverable `START_HERE` runbook.
- Python standard-library loopback host with named parameterised endpoints.
- Independent Rust verification and runtime implementation.
- Trusted Tauri shell separated from a raw Wry application renderer.
- Host-local trust, grants, revocation, backup, restore and source pinning.
- Signed application extension v0.2.
- Generic pack, unpack, diff, verification and HTML export tools.
- Diagram Studio as a visual, interactive, product-specific reference.
- Standalone `capsule-creator` Codex/Agent Plugin.
- Deterministic generated artefacts and extensive Python, browser and native
  acceptance gates.

## Current structural pressure

The v0.2 singleton `capsule_manifest` contains both:

- application release fields such as `app_id`, `app_version`, `entry_asset`,
  runtime protocol and permissions; and
- capsule instance fields such as `capsule_id`, title, summary and timestamps.

The signed-app v0.2 canonical compartment includes the whole
`capsule_manifest`. Therefore changing instance identity or descriptive
metadata changes the publisher-signed application digest. This prevents a
signature-preserving fork and makes `updated_at` ambiguous.

The live v0.2 canonical stream signs every non-internal schema record, including
domain and mutable-table schema, while excluding mutable/domain *rows*. V0.3
retains that schema/row distinction under a new profile and contexts; see
[ADR 0022](../../../decisions/0022-signed-application-v0.3-compartment.md).

## Current native shell pressure

The shell begins on Trust review and exposes security, data protection,
publisher signing, host update and application-window pages. This is correct for
the current proof, but lifecycle management needs a host-owned Overview as the
first product surface while retaining the existing trust gate underneath it.

## Current code boundaries relevant to this programme

```text
native/crates/capsule-core          bounded metadata inspection
native/crates/capsule-crypto        signed application canonicalisation
native/crates/capsule-launch        launch and capability evaluation
native/crates/capsule-policy        protected local trust and audit
native/crates/capsule-runtime       named endpoint execution
native/crates/capsule-lifecycle     source identity, writer lease, backup/restore
native/desktop/src-tauri            trusted shell command boundary
native/desktop/ui                   host-owned user interface
runtime/ and tools/                 Python implementation and authoring tools
plugins/capsule-creator/            standalone creator plugin snapshot
```

## Baseline capture command

Run:

```text
python docs/plans/capsule-lifecycle/tools/codex_lifecycle/capture_baseline.py
```

M00 must compare the generated evidence with this document and update the plan
when current code has materially changed.

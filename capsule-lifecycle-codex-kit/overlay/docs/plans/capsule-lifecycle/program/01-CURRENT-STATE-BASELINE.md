# Current-state baseline

Prepared from public `main` as observed on 12 August 2026 at commit:

```text
f67da560fb4baaa13144cea220c9329df87ad534
```

Codex must regenerate a local baseline before implementation because the
repository may have advanced.

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

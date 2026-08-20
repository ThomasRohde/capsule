# M00 implementation path and dependency map

## Gate order

```text
M00 decisions
  -> M01 versioned v0.3 identity/profile/signature
  -> M02 data contract/lineage/workspace/publication foundation
  -> M03 trusted Overview/Cabinet
  -> M04 copy/fork/template
  -> M05 bounded compare
  -> M06 reconcile to copy
  -> M07 same-schema application upgrade
  -> M08 declarative migration upgrade
  -> M09 ecosystem hardening/qualification/release
```

No downstream milestone may supply missing evidence for an earlier gate.

## Path impact by milestone

| Milestone | First integrated slice | Principal generic paths | Example/plugin/generated impact | Gate hazard |
| --- | --- | --- | --- | --- |
| M01 | Factor one exhaustive read-only native verifier, correct native 64 MiB policy, and lock v0.2 behavior before adding dispatch | `format/`, `runtime/capsule_host.py`, `tools/capsule_{author,conformance,signatures}.py`, `native/crates/capsule-{core,crypto,launch,runtime,signing,cli}`, `compatibility/` | Minimal generic v0.3 fixture; then every affected `plugins/capsule-creator/` snapshot/test surface | Do not change v0.2 contexts/vectors; mutable rows excluded but every schema record signed |
| M02 | Scaffold `native/crates/capsule-workspace` with dependency-boundary tests, validated plan/error types and a stable input-snapshot primitive | `native/Cargo.toml`, new workspace crate, `capsule-cli`, lifecycle pin/publication helpers, v0.3 conformance | Move reconciled Diagram Studio data contract into reviewable example source; plugin authoring validators | Inputs read-only; reject WAL/journal sidecars; bind and reproduce a raw private snapshot digest; use snapshot-only reads; recheck source before publish; pin destination parent identity; no endpoint execution; plan digest cross-language deterministic |
| M03 | Add Rust `CapsuleOverviewViewModel` and exact trusted-shell command before UI reshaping | `native/desktop/src-tauri`, capability/permission files, `native/desktop/ui`, `tests/native/` | v0.2/v0.3 visual fixtures; no app-renderer changes | Metadata spoofing/icon bombs; Cabinet separate from trust; raw label denied |
| M04 | Implement duplicate plan/execute through shared no-replace publisher | `capsule-workspace/copy*`, `capsule-cli`, trusted copy wizard | Deterministic template/fork/selective fixtures | Never use authoring `--replace`; hash inputs before/after; sensitive datasets default omit |
| M05 | Split compare summary/detail contracts, then implement streaming PK comparison | `capsule-workspace/compare*`, report schemas, CLI/Tauri compare sessions | branch/ancestor/adversarial scale fixtures | No rowid/caller SQL; sensitive values explicit; hard bounds and cancellation |
| M06 | Bind a reconcile plan to one exact report and target copy | `capsule-workspace/reconcile*`, plan schema, CLI/Tauri resolution flow | expected branch/conflict/reconciled fixtures | No two-way auto-merge claim; row preconditions; target application digest unchanged |
| M07 | Plan same-app/same-key/same-schema upgrade from a clean target release | `capsule-workspace/upgrade*`, launch/policy capability delta, CLI/Tauri | two signed Diagram Studio releases with same schema | Never patch old assets into working file; target digest/signature must survive exactly |
| M08 | Validate unique migration graph and implement one typed operation at a time | `capsule-workspace/migration*`, migration schema/conformance, upgrade integration | v1/v2 source/target/expected fixtures; plugin declaration support | No rebuild endpoint, SQL, script, attach or application callback; only output writable |
| M09 | Reconcile canonical source, plugin snapshot and deterministic distributions, then qualify native host | examples, capsules, exports, plugin, docs, tests, workflows, SBOM/licence, installer tools | full demonstrator and standalone plugin copied test | Native changes require current NSIS rebuild; MSI remains opt-in; platform/clean-machine gaps stated honestly |

## M01 executable handoff

1. Read M01 `EXECPLAN.md` and `ACCEPTANCE.md` plus ADRs 0021, 0022 and 0028.
2. Add regression tests that distinguish `metadata_inspected` from exhaustive
   conformance and prove direct native signing cannot prepare a capsule that
   fails runtime conformance.
3. Tighten `capsule-core::MAX_CAPSULE_BYTES` to 64 MiB under the accepted
   compatibility correction and record old/new rejection evidence.
4. Factor the exhaustive v0.2 read-only verifier without changing v0.2 fixture
   acceptance or signed-app bytes.
5. Only then materialise versioned v0.3 SQL/conformance and dispatch.

Do not start M02 crate work until Python/Rust v0.3 streams, mutation vectors,
v0.2 regression matrix, plugin snapshot and M01 generated checks all pass.

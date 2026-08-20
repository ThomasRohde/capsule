# Result — M00: Reconcile live repository and freeze architecture

**State:** complete  
**Started:** 2026-08-12T14:41:17Z  
**Completed:** 2026-08-12T15:30:59Z  
**Repository commit at start:** `e73cf948fba233ef84d4680930b61549012020a7`  
**Repository commit at completion:** `e73cf948fba233ef84d4680930b61549012020a7`

## Outcome

The live v0.2 implementation, trust boundary, signing/build paths and standalone
creator plugin were inspected before design changes. Eight accepted ADRs now
freeze the M01/M02 architecture, reconciled draft SQL/JSON contracts match the
repository's live conventions, and the implementation path through M09 is
explicit. No production format/runtime/native/plugin/generated code was changed.

The gate passed on Windows 11 x86-64: 144 Python tests and 125 Rust tests passed;
format/check/clippy, strict JSON Schema, generated capsule/export, conformance,
verification, SBOM and license checks all passed. Both independent and security
critics reported no remaining M00 design blocker.

## Scope delivered

- Captured the exact baseline, including a three-commit difference from the kit's
  expected commit, dirty/untracked programme state and toolchain inventory.
- Traced Python and Rust verification, signed-app v0.2 canonicalisation,
  first-open/trust, raw Wry protocol, authoring, generation and signing.
- Accepted ADRs 0021-0028 for v0.3 identity/signature, workspace ownership,
  stable snapshot and no-replace publication, restricted migrations, Cabinet,
  compare identity and exhaustive verification/64 MiB policy.
- Reconciled application, instance, lineage, data, migration, plan, error and
  conformance drafts; classified all ten live Diagram Studio domain tables once.
- Produced the M01-M09 path/dependency map and an executable M01 first slice.

## Changed paths

- `docs/decisions/0021-0028*.md` — accepted architecture decisions.
- `docs/plans/capsule-lifecycle/contracts/` — reconciled draft SQL/JSON contracts.
- `docs/plans/capsule-lifecycle/examples/` — schema-valid examples and live
  Diagram Studio data-contract coverage.
- `docs/plans/capsule-lifecycle/program/` — live baseline, architecture, security,
  format, copy/compare/upgrade, compatibility and decision-register corrections.
- `docs/plans/capsule-lifecycle/evidence/M00/` — baseline, transcripts, architecture
  trace, implementation map, contract checker and critic report.
- `docs/plans/capsule-lifecycle/milestones/M00-baseline-and-decisions/` and
  `PROGRAM_STATUS.json` — acceptance/result/status records.

Unchanged by M00: `format/`, `runtime/`, `native/`,
`plugins/capsule-creator/`, `capsules/` and `exports/`.

## Decisions and deviations

| Decision/deviation | Rationale | ADR or source |
| --- | --- | --- |
| Distinct v0.3 tuple and mutable instance compartment; v0.2 remains separate | Mutable profile/domain rows must not require publisher re-signing | ADR 0021 |
| New signed-app v2 contexts; all non-internal schema plus exhaustive signed row allowlist | Preserve v0.2 bytes while authenticating application schema and excluding mutable/domain rows | ADR 0022 |
| New product-independent `capsule-workspace` | Keep lifecycle transforms out of runtime/raw-renderer and Diagram Studio | ADR 0023 |
| Plan/execute binds exact private snapshot digest, expiry and stable destination parent/leaf | Close same-size, ABA, WAL/sidecar and parent-substitution races; publish create-new only | ADR 0024 |
| Typed declarative migration allowlist; no endpoints/SQL/scripts/rebuild callbacks | A publisher signature does not make code safe | ADR 0025 |
| Protected rebuildable Cabinet and bounded PNG/WebP metadata | Separate convenience state from trust/canonical identity | ADR 0026 |
| Declared PK typed comparison; explicit two-way and base-required three-way | Avoid rowid and false automatic-merge claims | ADR 0027 |
| Metadata/conformance/check/signature/policy phases; 64 MiB native correction | Live `verify_structure` is shallow and native 512 MiB contradicted every other host surface | ADR 0028 |
| V0.2-to-v0.3 upgrade unavailable until a signed legacy-adapter contract exists | Current drafts cannot represent it; no inference or silent rewrite is safe | ADRs 0021/0025; programme 09/11 |
| Migration resource ceilings are host-owned plan values | No canonical signed declaration storage exists; publisher input may never raise host caps | ADR 0025 |

## Acceptance evidence

| Acceptance clause | Status | Evidence |
| --- | --- | --- |
| Exact live baseline recorded | pass | `evidence/M00/baseline-20260812T143205Z.json`; reconciliation “Baseline movement” |
| M01/M02 architecture choices resolved or blocking | pass | ADRs 0021-0028; decision register; independent re-audit in `critic-report.md` |
| Mutable instance/profile/domain rows do not require re-signing | pass (contract) | ADR 0022 signs all schema but excludes mutable/domain rows; mutation proof is the M01 gate |
| Raw renderer boundary and negative points documented | pass | Reconciliation “Raw renderer protocol and negative boundary”; no production command/capability change |
| No premature production implementation | pass | Only docs/draft contracts/evidence/status changed; generated checks remained current |
| Lifecycle inputs pinned/read-only and unchanged | pass (no M00 transform) | No lifecycle transform ran; source capsule SHA-256 remained `fa6168437d74e372b22485efdbf3db51721ce7f267364c2d2331c1784050f157` |
| Transforming operations create-new/refuse existing destinations | pass (contract) | ADR 0024 and lifecycle-plan `/1`; implementation is gated to M02/M04+ |
| Generic code contains no Diagram Studio semantics | pass | No generic production code changed; Diagram Studio appears only in examples/evidence |
| Raw Wry receives no lifecycle command/event/capability | pass | Live trace plus critic report; `native/` unchanged |
| V0.2 acceptance/runtime unchanged | pass | Full Python/Rust baseline and final suites pass; production and v0.2 vectors unchanged |
| Error paths fail closed with stable redacted codes | pass (contract) | `lifecycle-error-codes-v1.json`, ADR 0024/0027 and unified programme lists |
| Docs/contracts/tests/generated artefacts synchronized | pass | 37-record strict schema validation; six examples; generated capsule/export checks |
| Focused unit/integration tests pass | pass | Full 144 Python and 125 Rust tests; plugin standalone-copy test included |
| Repository-wide gates pass | pass | All commands in the table below returned exit 0 |
| Independent/security review resolved | pass | `evidence/M00/critic-report.md`; both final verdicts pass |
| Exact commands/environment/counts/limits recorded | pass | This result, baseline JSON and bounded evidence transcripts |
| Programme status valid/accurate | pass | Final integrated validator plus atomic completion transition |
| Next milestone has executable handoff | pass | `evidence/M00/implementation-path-map.md` and Handoff below |

## Tests and validation

Environment: Windows 11 build 26200 x86-64; CPython 3.13.7 for repository
commands, CPython 3.12.4 with `jsonschema` for strict Draft 2020-12 validation;
Rust/Cargo 1.97.1 MSVC; Git 2.45.2. A process-local Git `safe.directory` setting
was used because the checkout owner differs; no global Git configuration changed.

| Command | Result | Evidence path |
| --- | --- | --- |
| `python .../capture_baseline.py` | pass after process-local safe-directory rerun | `evidence/M00/baseline-20260812T143205Z.json` |
| `python .../validate_lifecycle_specs.py` | pass, 37 records | `evidence/M00/20260812T152821Z-final-integrated-specs/` |
| `C:\Python312\python.exe .../validate_lifecycle_specs.py --require-jsonschema` | pass, six examples and 37 records | `evidence/M00/20260812T152821Z-final-integrated-jsonschema/` |
| `pwsh -NoProfile -File evidence/M00/check-contract-examples.ps1` | pass, six examples and 10/10 live domain tables | `evidence/M00/20260812T152119Z-gate-contract-examples/` |
| `python -m unittest discover -s tests -v` | pass, 144 tests in 100.207 s | `evidence/M00/20260812T152119Z-gate-python-suite/` |
| `python tools/capsule.py conformance capsules/diagram-studio.capsule.sqlite` | pass | `evidence/M00/20260812T152402Z-gate-capsule-conformance/` |
| `python tools/capsule.py verify capsules/diagram-studio.capsule.sqlite` | pass | `evidence/M00/20260812T152403Z-gate-capsule-verify/` |
| `python tools/build_example.py --check` | pass, current | `evidence/M00/20260812T152404Z-gate-generated-capsule-check/` |
| `python tools/build_exports.py --check` | pass, current | `evidence/M00/20260812T152404Z-gate-generated-exports-check/` |
| `cargo fmt --all -- --check` from `native/` | pass | `evidence/M00/20260812T152405Z-gate-rust-fmt/` |
| `cargo check --workspace --all-targets` from `native/` | pass | `evidence/M00/20260812T152404Z-gate-rust-check/` |
| `cargo test --workspace --all-targets` from `native/` | pass, 125 tests, 0 failed/ignored | `evidence/M00/20260812T152402Z-gate-rust-tests/` |
| `cargo clippy --workspace --all-targets -- -D warnings` from `native/` | pass | `evidence/M00/20260812T152403Z-gate-rust-clippy/` |
| `python tools/generate_sbom.py --check` from `native/` | pass | `evidence/M00/20260812T152404Z-gate-sbom-check/` |
| `python tools/generate_license_report.py --check` from `native/` | pass | `evidence/M00/20260812T152405Z-gate-license-check/` |

The Python suite includes 12 creator-plugin tests, including a standalone copied
plugin build/verify without repository access. M01 must synchronize every
affected plugin surface after canonical v0.3 source/vectors pass.

## Security and critic review

- Reviewers: builder `m00_builder`, independent critic
  `m00_independent_critic`, security critic `m00_security_critic`.
- Resolved high finding: plan/execute now binds/reproduces an exact private
  snapshot digest, rejects source sidecar state, detects ABA and binds a held
  stable destination parent plus leaf.
- Resolved contract findings: expiry, exact view count, error catalogue,
  host-owned limits and explicit legacy-adapter deferral.
- Final verdict: no unresolved M00 architecture/security blocker.
- Accepted pre-existing high residual: native first-open/direct-signing uses the
  shallow `verify_structure`; ADR 0028 hard-fences exhaustive verifier factoring
  and the 64 MiB correction as M01's first slice before v0.3 dispatch.
- Later residuals: compare contract remains non-normative until M05; lifecycle
  race and raw-renderer negative tests are gates in their owning milestones.

## Generated artefacts

| Artefact | Rebuild/check command | Digest/result |
| --- | --- | --- |
| `capsules/diagram-studio.capsule.sqlite` | `python tools/build_example.py --check` | unchanged/current; 856,064 bytes; SHA-256 `fa6168437d74e372b22485efdbf3db51721ce7f267364c2d2331c1784050f157` |
| `exports/` | `python tools/build_exports.py --check` | unchanged/current |
| Native SBOM/licenses | check-only commands above | unchanged/current |
| Windows installer | not rebuilt | M00 changed no native host or packaging source; repository rule does not require rebuild |

## Remaining limitations

- No production v0.3 or lifecycle behavior exists yet; M00 deliberately freezes
  contracts and architecture only.
- The pre-existing shallow native verification/direct-signing gap and 512 MiB
  native admission are not fixed by M00; M01 must close both before v0.3 work.
- V0.2-to-v0.3 upgrade is unavailable until M08 accepts a signed legacy-adapter
  contract; this is a deliberate fail-closed limitation.
- The checkout already contained the untracked lifecycle programme/kit at
  baseline; M00 preserved that user state.

## Handoff

**Next milestone:** M01 — Format v0.3 identity, profile and signature compartment  
**First action:** Add regression fixtures/tests that separate metadata inspection
from exhaustive non-executing conformance, prove direct native signing rejects a
runtime-invalid capsule, and lock current v0.2 accept/reject/signature vectors.
Then change native admission from 512 MiB to 64 MiB with old/new rejection
evidence. Only after those pass may v0.3 dispatch and canonical streams be added.  
**Relevant files:** ADRs 0021, 0022, 0028;
`native/crates/capsule-{core,launch,runtime,signing,crypto}`; `format/`;
`runtime/capsule_host.py`; `tools/capsule_{author,conformance,signatures}.py`;
`compatibility/`; `plugins/capsule-creator/`.  
**Known hazards:** do not alter v0.2 contexts/vectors; schema records are signed
even when their rows are mutable; do not call the shallow helper exhaustive;
update every affected standalone plugin surface; installer rebuild is required
once native host changes are integrated.  
**Do not repeat:** baseline/live-flow inspection and M00 contract decisions are
captured in `evidence/M00/`; begin from the M01 first slice in the implementation
path map.

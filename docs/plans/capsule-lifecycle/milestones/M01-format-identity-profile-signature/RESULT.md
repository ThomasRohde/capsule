# Result — M01: Format v0.3 identity, profile and signature compartment

**State:** complete  
**Started:** 2026-08-12T15:31:32Z  
**Completed:** 2026-08-12T17:31:24Z  
**Repository commit at start:** `e73cf948fba233ef84d4680930b61549012020a7`  
**Repository commit at completion:** `e73cf948fba233ef84d4680930b61549012020a7`

## Outcome

Python and Rust now inspect, exhaustively verify, canonicalise and authenticate a
generic v0.3 Capsule whose signed application release is distinct from mutable
instance profile, lineage and domain rows. The v0.2 profile, canonical contexts
and golden vector bytes remain unchanged. The standalone creator plugin authors
v0.3 only when explicitly requested and still defaults to v0.2.

The gate passed on Windows 11 x86-64: 164 Python tests and 142 Rust tests passed,
all workspace/freshness/vector/contract gates passed, and the native NSIS-only
installer was rebuilt and exported. Both independent and security critics report
no remaining M01 blocker.

## Scope delivered

- Added versioned v0.3 format, conformance and signed-app contracts with separate
  application, instance, data-schema, lineage, dataset and migration records.
- Added strict Python/Rust overview dispatch and bounded metadata parsing; v0.2
  receives an explicit legacy projection without invented v0.3 identity.
- Added v0.3 canonicalisation/profile/context and deterministic cross-language
  Ed25519 vectors while preserving v0.2 bytes and 512 MiB stream framing.
- Factored one exhaustive native verification gate shared by launch, runtime and
  direct signing, including declarations, checks, assets and signed compartment.
- Closed source races with a sidecar-rejecting, 64 MiB-bounded private snapshot,
  one-connection evidence, exact signing copy and runtime handoff rebind.
- Added atomic v0.3 revision/change-log updates for named writes and fail-closed
  session poisoning for ambiguous rollback/COMMIT outcomes.
- Added explicit v0.3 author/build/inspect/verify support, synchronized all
  standalone creator-plugin surfaces and rebuilt generated artefacts.

## Changed paths

- `format/*v0.3*`, `compatibility/signed-app-v0.3/` — normative format,
  conformance, signed application schema and deterministic vectors.
- `runtime/capsule_host.py`, `tools/capsule*.py`, `tools/check_signed_app_vectors.py`
  — version dispatch, overview, conformance, authoring and canonical signatures.
- `native/crates/capsule-{core,crypto,launch,runtime,signing,cli}/` — typed
  identities, exhaustive verification, canonical crypto, exact snapshots,
  runtime atomicity and administrative CLI support.
- `plugins/capsule-creator/` — explicit v0.3 authoring, bundled sources,
  references, standalone tests and Inspector example; default remains v0.2.
- `tests/`, `docs/`, `README.md`, `compatibility/README.md` — boundary,
  mutation, integration and contract documentation.
- Generated capsule, HTML exports, native SBOM/licenses and NSIS installer.

## Decisions and deviations

| Decision/deviation | Rationale | ADR or source |
| --- | --- | --- |
| New v0.3 format and signed-app v2 contexts; v0.2 constants untouched | Mutable instance/domain state must not invalidate an application release | ADRs 0021, 0022 |
| File admission is 64 MiB; canonical-stream framing remains v0.2-compatible at 512 MiB | Admission and stream framing are distinct policies | ADR 0028 |
| Every trust/signing/runtime path uses exhaustive verification on one exact snapshot/transaction | Prevent mixed evidence, WAL state, ABA and direct-signing races | ADRs 0024, 0028 |
| Rollback/COMMIT ambiguity poisons the session | An uncertain transaction must never remain usable | Security review |
| V0.2-to-v0.3 conversion remains unavailable | No accepted signed legacy adapter exists; silent rewrite is forbidden | ADRs 0021, 0025 |
| Lifecycle `/1` external error envelopes start in M02 | M01 exposes no lifecycle plan/execute API; the stable catalogue is already frozen | ADR 0024; security review |

## Acceptance evidence

| Acceptance clause | Status | Evidence |
| --- | --- | --- |
| Instance/profile/domain mutations preserve v0.3 application signature | pass | Cross-language signed vector mutation matrix; Rust crypto stored-envelope tests |
| Signed application changes invalidate the old signature | pass | Included-table/schema/app-version mutation vectors and Rust crypto tests |
| Python and Rust agree exactly | pass | `gate-final-vectors`; deterministic digest `72fc34df29a902010766c9b2af647fa6f12dfa499572bd0a064d77a91351e57f` |
| V0.2 behavior and vectors remain compatible | pass | Full suites; explicit Python/native v0.2 inspect/verify; original golden vector stays 1,062,207 bytes with digest `2e8878…` |
| No lifecycle transform mutates an input | pass | No lifecycle transform exists; signing snapshots source and create-new refuses existing output |
| Inputs are pinned/read-only; outputs create-new | pass | Launch/runtime/signing race tests and `security-review.md` |
| Generic/application and raw-Wry boundaries preserved | pass | Generic crate review; no lifecycle renderer command/event/capability added |
| Stable redacted failure posture | pass | `/1` error catalogue unchanged; declared-check values redacted; typed fail-closed native errors; M02 envelope handoff |
| Docs/contracts/tests/generated artefacts synchronized | pass | Strict 54-record schema validation, full suites, plugin isolated matrix, build/export/SBOM/license checks |
| Independent/security review resolved | pass | `evidence/M01/critic-report.md`; `evidence/M01/security-review.md` |
| Programme status and next handoff accurate | pass | Final spec/status validation and Handoff below |

## Tests and validation

Environment: Microsoft Windows NT 10.0.26200.0 x86-64; CPython 3.13.7 for
repository commands; CPython 3.12.4 plus `jsonschema` for strict Draft 2020-12
validation; Rust/Cargo 1.97.1 MSVC.

| Command | Result | Evidence path |
| --- | --- | --- |
| `python -m unittest discover -s tests -v` | pass, 164 tests in 88.110 s | `evidence/M01/20260812T171904Z-gate-final-python-rerun/` |
| `cargo test --workspace --all-targets` from `native/` | pass, 142 tests, 0 failed/ignored | `evidence/M01/20260812T172105Z-gate-final-rust-workspace-rerun/` |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass | `evidence/M01/20260812T172344Z-gate-final-rust-clippy-rerun/` |
| `cargo check --workspace --all-targets` | pass | `evidence/M01/20260812T172405Z-gate-final-rust-check/` |
| `cargo fmt --all -- --check` | pass | `evidence/M01/20260812T172410Z-gate-final-rust-fmt/` |
| `python tools/check_signed_app_vectors.py --all` | pass, v0.2/v0.3 | `evidence/M01/20260812T172310Z-gate-final-vectors/` |
| `C:\Python312\python.exe .../validate_lifecycle_specs.py --require-jsonschema` | pass, six examples and 54 records | `evidence/M01/20260812T172310Z-gate-final-specs/` |
| Python `capsule.py inspect` and `verify`, v0.2/v0.3 | four passes | `evidence/M01/20260812T172506Z-cli-*/` |
| Native `capsule-native inspect` and `verify`, v0.2/v0.3 | four passes | `evidence/M01/20260812T172535Z-native-*/` |
| `python tools/build_example.py --check` | pass | `evidence/M01/20260812T172839Z-gate-final-example/` |
| `python tools/build_exports.py --check` | pass after required rebuild | `evidence/M01/20260812T172343Z-gate-final-exports-rerun/` |
| `python tools/capsule.py conformance ...` | pass | `evidence/M01/20260812T172839Z-gate-final-conformance-v02/` |
| SBOM and license `--check` commands | pass after required regeneration | `evidence/M01/20260812T172436Z-gate-final-sbom-rerun/`; `...172437Z-gate-final-license-rerun/` |
| `git diff --check` | pass | `evidence/M01/20260812T172839Z-gate-final-git-diff-check/` |
| `python native/tools/build_installers.py --bundles nsis` | pass, NSIS only | `evidence/M01/20260812T172611Z-gate-nsis-installer-rerun/` |

The initially recorded Python, export and SBOM failures are retained as honest
superseded evidence: they exposed stale generated artefacts during integration;
the artefacts were rebuilt and the named final reruns pass.

## Security and critic review

- Reviewers: builder `m00_builder`, independent critic
  `m00_independent_critic`, security critic `m00_security_critic`.
- Resolved high findings: later endpoint steps, complete tuple/cardinality,
  signed vectors, exact snapshot/signing evidence, 64 MiB capture, runtime
  verification handoff and CLI snapshot coherence.
- Resolved medium findings: declared-check redaction, rollback poisoning,
  multibyte metadata bounds, hostile JSON shapes and plugin documentation.
- Final verdicts: independent and security gates pass; focused security run
  passed 41/41 Rust tests.
- M02-owned residuals: held destination-parent publication primitive and the
  first external lifecycle `/1` error-code envelope.

## Generated artefacts

| Artefact | Rebuild command | Digest/result |
| --- | --- | --- |
| `capsules/diagram-studio.capsule.sqlite` | `python tools/build_example.py` | 884,736 bytes; SHA-256 `cb7869032d6e017429fff3ea738018791d5df91b3f7013f64285f299edb2be21`; 18 checks pass |
| Creator-plugin Inspector capsule | plugin build/freshness tests | 1,785,856 bytes; SHA-256 `fafa0b162ccf04e9a85788bfcee5fa878d37fe4b18df568f8bb09e0bcc1b8dd8` |
| Three HTML exports | `python tools/build_exports.py` | SHA-256 `a720e7e2…`, `364af052…`, `d8bbc54f…`; check passes |
| Native SBOM/licenses | generator commands | SHA-256 `dac0a114…` and `fac31463…`; checks pass |
| `capsules/sqlite-capsule-host-setup.exe` | `python native/tools/build_installers.py --bundles nsis` | development-unsigned NSIS; 6,434,142 bytes; SHA-256 `7d75211ee25de0abd9d7074bd4c038b2ea784167746c028cbd709bb63709bd62` |

## Remaining limitations

- No lifecycle copy/fork/compare/reconcile/upgrade transform is exposed yet.
- V0.2-to-v0.3 conversion remains fail-closed until M08 defines and signs a
  legacy adapter.
- The rebuilt local installer is explicitly development-unsigned; production
  signing remains a release-pipeline responsibility.
- The destination-parent held-handle and external lifecycle error-envelope
  implementations are deliberately the first M02 foundation work.

## Handoff

**Next milestone:** M02 — Data contract, lineage and workspace service foundation  
**First action:** Create `native/crates/capsule-workspace` with no CLI/Tauri/Wry
registration. Its first vertical slice must accept only a
`VerifiedReadOnlyCapsule` connection, parse the signed v0.3 dataset contract,
classify every ordinary table once, validate PK columns/dependencies/policies,
and map every failure to the frozen lifecycle `/1` error catalogue. Add hostile
duplicate, undeclared-table, missing-PK and dependency-cycle tests before adding
plans or output publication.  
**Relevant files:** M02 `EXECPLAN.md`; ADRs 0023-0025; lifecycle error/plan
contracts; `capsule-launch::VerifiedReadOnlyCapsule`; v0.3 dataset tables and
conformance; creator-plugin data-contract source.  
**Known hazards:** do not accept an unverified path/connection; lineage is mutable
provenance, never publisher authentication; no raw Wry command; no output helper
until the held-parent create-new publication primitive and race tests exist.  
**Do not repeat:** M01 complete-tuple dispatch, canonical crypto, metadata bounds
and snapshot verification are shared prerequisites; reuse them instead of
reopening live paths or inventing a second error vocabulary.

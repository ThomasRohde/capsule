# Result — M05: Bounded compare engine and trusted shell comparison

**State:** complete  
**Started:** 2026-08-13T05:17:14Z  
**Completed:** 2026-08-13T07:30:30Z  
**Repository commit at start:** `e73cf948fba233ef84d4680930b61549012020a7`  
**Repository commit at completion:** `e73cf948fba233ef84d4680930b61549012020a7` plus the reviewable M00–M05 working-tree programme changes

## Outcome

The trusted Cabinet can compare two signed-v0.3 Capsules without executing or
mutating either. A retained, bounded workspace engine reports compatibility,
identity, mutable lineage claims, application, schema and signed-contract data
differences. The trusted shell reveals only opaque, paginated detail and requires
an explicit action before disclosing bounded sensitive fields; the raw Wry
renderer receives no comparison authority.

## Scope delivered

- Added `compare_sources` over two retained `VerifiedWorkspaceSource` snapshots
  with five verified-source compatibility states. Invalid capsules fail at
  admission and never receive a misleading report.
- Froze collision-free typed compare-key and compare-row byte contracts with an
  independent Python/Rust vector. NULL, integer, real, text and BLOB storage
  classes, signed zero and integer/real ordering remain distinct.
- Implemented a true streaming ordered merge that retains one row per side,
  enforces shared row/value/stream/deadline limits and returns deterministic
  counts/digests without executing application SQL.
- Kept different applications generic and domain-count-free. Contract drift is
  summary-only; undeclared/unstable data never gains detail authority.
- Added bounded lineage evidence. Direct parent claims require the exact other
  retained source digest and current child IDs; shared third hashes remain
  explicitly mutable-untrusted claims.
- Added one-use opaque detail cursors bound to pair, table, stable limits and
  disclosure state. `row` policy returns row evidence only, `field` returns
  bounded fields, `summary`/`ignore` deny detail, and sensitive detail requires
  a fresh trusted-shell reveal.
- Added explicit application expansion into thirteen fixed value-free digest
  families. Large valid assets bind verified SHA-256/length metadata rather than
  materialising content.
- Added stable CLI `/1` JSON. Repeating the same pair produces byte-identical
  output, a fixed `deadline_ms: 30000` and the same report digest.
- Added a trusted Tauri compare controller with opaque candidate/session/table/
  page tokens, exact-session cancellation, a single absolute 30-second lifetime
  and native idle reaping. Wall expiry is anchored conservatively before the
  monotonic authority deadline.
- Added accessible trusted-shell compatibility, four-layer summary, dataset
  cards, bounded page-two continuation, sensitive reveal and application
  expansion. Capsule-controlled controls and bidi text are visibly escaped and
  isolated.
- Added strict schemas/examples for report, page, application detail and Tauri
  session; synchronized ADR 0027, architecture/security/native docs, standalone
  creator-plugin guidance and generated capability schemas.
- Proved real Windows trusted-shell pagination/reveal/application behavior and
  raw-renderer denial in both locked and authorized states.

## Changed paths

- Compare engines: `native/crates/capsule-workspace/src/{compare,compare_detail,compare_application}.rs`
- CLI: `native/crates/capsule-cli/`, `native/crates/capsule-workspace/src/bin/`, workspace CLI tests
- Trusted shell: `native/desktop/src-tauri/src/{compare_flow,lib}.rs`,
  `native/desktop/ui/`, capability/permission schemas
- Contracts/vectors: `compatibility/compare-row-v1/`,
  `docs/plans/capsule-lifecycle/contracts/compare-*.schema.json`,
  `tauri-compare-v1.schema.json`, corresponding examples and checker
- Documentation and authoring: ADR 0027, architecture/security/native docs,
  standalone `plugins/capsule-creator/` references/tests
- Native E2E/static tests: `tests/native/{cabinet-overview,raw-child}.e2e.mjs`,
  `tests/test_native_ui.py`, `tests/test_compare_row_vectors.py`
- Generated outputs: Diagram capsule/exports checks, native SBOM/license inventory,
  and `capsules/sqlite-capsule-host-setup.exe`

## Decisions and deviations

| Decision/deviation | Rationale | ADR or source |
| --- | --- | --- |
| Invalid input is an admission error, not a sixth report state. | A bounded report cannot safely describe a source that failed exhaustive admission. | ADR 0027; M05 `EXECPLAN.md` |
| Different applications expose only generic identity/application/schema metadata. | Domain labels and row counts are application data and must not leak across incompatible app identities. | ADR 0027; security review |
| Detail pagination uses consumed in-memory authority, never a serialized cursor from JavaScript. | Prevents table/order/row-position authority from crossing the trusted-shell boundary. | target architecture; SI-6/SI-7 |
| Report deadlines are stable at 30 seconds; operation deadlines are a separate decreasing budget. | Deterministic reports must not hash elapsed wall time, while every request still shares one absolute bounded lifetime. | compare `/1` contracts; critic review |
| Mutable lineage can corroborate an exact other-file digest but never publisher trust. | A matching retained file proves the named bytes exist, not that the mutable lineage claim was signed. | ADR 0027 |

## Acceptance evidence

| Acceptance clause | Status | Evidence |
| --- | --- | --- |
| Read-only, execution-free comparison | pass | Workspace Compare tests, canary/static checks and unchanged-source assertions; settled Rust gate |
| Undeclared/unstable data fail closed or summary-only | pass | Contract-drift, missing-PK/collation, different-app and policy hostile tests |
| Bounded deterministic reports | pass | Independent vector, repeated CLI byte equality, streaming/limit tests and `/1` schemas |
| Sensitive values require trusted disclosure | pass | Core reveal tests and real sensitive page-two E2E; `ui/compare-sensitive-revealed-page-2.png` |
| No raw renderer authority | pass | `20260813T072635Z-gate-final-native-raw-compare-boundary-settled` |
| All inputs pinned and unchanged | pass | Final rebind/live mutation tests and trusted E2E source hashes |
| Stable redacted failures | pass | In-flight cancel/deadline, stale cursor/session and value-free error snapshots |
| Generic/example boundary and v0.2 stability | pass | Static/plugin review and full repository suites |
| Documentation/contracts/generated artefacts synchronized | pass | Strict jsonschema, vectors, capsule/export/SBOM/license/NSIS gates |
| Independent review | pass | `evidence/M05/critic-report.md`; `evidence/M05/security-review.md` |

## Tests and validation

| Command | Environment | Result | Evidence path |
| --- | --- | --- | --- |
| `python -m unittest discover -s tests -v` | Windows, Python | 178 passed | `evidence/M05/20260813T071416Z-gate-final-python-suite/evidence.json` |
| `cargo test --workspace --all-targets` | Windows, pinned Rust | 314 passed across 20 suites | `evidence/M05/20260813T072154Z-gate-final-rust-workspace-settled/evidence.json` |
| `cargo fmt --all -- --check` | Windows | pass | `evidence/M05/20260813T072425Z-gate-final-fmt-settled/evidence.json` |
| `cargo check --workspace --all-targets` | Windows | pass | `evidence/M05/20260813T072416Z-gate-final-check-workspace-settled/evidence.json` |
| `cargo clippy --workspace --all-targets -- -D warnings` | Windows | pass | `evidence/M05/20260813T072408Z-gate-final-clippy-workspace-settled/evidence.json` |
| lifecycle specification validator | Python 3.12 + jsonschema | 193 records; 16 examples | `evidence/M05/20260813T073221Z-gate-final-status-jsonschema/evidence.json` |
| compare row + lifecycle/signed/template/compact vectors | independent Python | all pass | `20260813T072713Z-gate-final-compare-vectors`; `20260813T072807Z-gate-final-*-vectors` |
| trusted-shell Windows E2E | rebuilt WebView2 host | pass; page 2, sensitive continuity, 13 families, unchanged sources | `evidence/M05/20260813T072556Z-gate-final-native-compare-shell-settled/evidence.json` |
| raw-Wry denial E2E | locked + authorized raw app | pass; all six Compare methods denied | `evidence/M05/20260813T072635Z-gate-final-native-raw-compare-boundary-settled/evidence.json` |
| capsule/export checks and independent verify | Windows | pass; 18 capsule checks | `20260813T072713Z-gate-final-example-check`; `gate-final-exports-check`; `gate-final-capsule-verify` |
| SBOM, license and RustSec checks | Windows | pass; no vulnerabilities, 17 exact reviewed warnings | `20260813T072713Z-gate-final-sbom`; `gate-final-license`; `20260813T072730Z-gate-final-rustsec` |
| `python native/tools/build_installers.py --bundles nsis` | Windows, debug NSIS only | pass | `evidence/M05/20260813T072434Z-gate-final-nsis-settled/evidence.json` |

Bounded performance evidence includes the 61-row/page-two fixture, a valid 2 MiB
asset, row/value/stream overflow tests and the 100,000-row hard ceiling. The
real trusted-shell matrix completed in 27.7 seconds; the compare engine remained
inside its single 30-second session budget.

## Security and critic review

- Independent critic: final PASS after every correctness/contract finding and
  the absolute authority/display/reaper expiry delta were fixed.
- Security critic: final PASS after exact-session cancellation, different-app
  disclosure, raw boundary, idle expiry and trusted pagination were rechecked.
- No finding is waived and no HIGH/MEDIUM/LOW implementation residual remains.
- A focused raw boundary scenario is the retained M05 gate; the broader legacy
  raw runtime suite had one unrelated local restore-picker infrastructure
  failure after its Compare assertions, so M05 does not misreport that unrelated
  path as green.

## Generated artefacts

| Artefact | Rebuild command | Digest/result |
| --- | --- | --- |
| `capsules/diagram-studio.capsule.sqlite` | `python tools/build_example.py --check` | 905,216 bytes; SHA-256 `10a6db3300991970b5e36c1460898534cf5e88ef70a7f0ac15432916150c6b67`; 18 checks |
| three HTML exports | `python tools/build_exports.py --check` | view `2bf355c9…42b6b`; interactive `74e96ab0…293a`; editable `2859073e…10006` |
| `native/sbom.cdx.json` / `native/THIRD_PARTY_LICENSES.md` | native generators | current; checks pass |
| `capsules/sqlite-capsule-host-setup.exe` | `python native/tools/build_installers.py --bundles nsis` | NSIS only; 7,444,237 bytes; SHA-256 `08efdbd71f4c84104d26ad4f3f26651fcc4bdde6574b73e0f9e5dfc9a32600ef` |

## Remaining limitations

- M05 is comparison-only. It never applies a row or field change; reconciliation
  is deliberately M06.
- A lineage parent match is evidence about retained bytes, not authenticated
  ancestry or publisher trust.
- Sensitive values are intentionally ephemeral trusted-shell data and are not
  persisted to Cabinet, logs or support bundles.

## Handoff

**Next milestone:** M06 — Apply selected changes to a new target-derived copy  
**First action:** add a product-independent, non-serializable reconciliation
review over a non-truncated `CompareSummary`. Freeze an allowlisted operation
matrix (`insert-source-row`, `delete-target-row`, `replace-from-source`,
`set-fields-from-source`) and precondition digest grammar before any executor or
Tauri command. The output must start from the retained target snapshot, preserve
its application digest/capsule ID, mint a new revision, and remain unpublished
until exact post-operation comparison and validation pass.  
**Relevant files:** `native/crates/capsule-workspace/src/{compare,compare_detail,reconcile*}.rs`,
ADR 0027, `reconcile-plan-v1.schema.json`, CLI session patterns and trusted
Compare controller.  
**Known hazards:** never serialize raw sensitive values or canonical cursors into
the plan; bind both exact sources/report digest/row preconditions; enforce
immutable columns, signed policy, dependency/FK order and explicit unresolved
conflicts; never mutate either input or expose reconcile commands to raw Wry.  
**Do not repeat:** reuse retained Compare sources, typed row/value digests,
opaque session authority, target-derived create-new publication and M04
operation-specific postpublish verification.

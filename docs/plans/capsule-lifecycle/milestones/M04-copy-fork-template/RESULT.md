# Result — M04: Duplicate, compact duplicate, fork and template creation

**State:** complete  
**Started:** 2026-08-12T22:48:30Z  
**Completed:** 2026-08-13T05:15:24Z  
**Repository commit at start:** `e73cf948fba233ef84d4680930b61549012020a7`  
**Repository commit at completion:** `e73cf948fba233ef84d4680930b61549012020a7` plus the reviewable M00–M04 working-tree programme changes

## Outcome

The trusted Cabinet now offers one coherent, review-first `Create copy` flow for
exact duplicate, compact duplicate, fork with data, authenticated template
creation and policy-controlled selective fork. Inputs remain retained,
read-only verified snapshots. All destinations are host-owned create-new
capabilities and become visible only after exhaustive operation-specific and
postpublication verification.

## Scope delivered

- Added a bounded, deterministic `org.sqlite-capsule.copy-preview/1` dry-run
  with identity effects, signed dataset actions, dependencies, sensitivity
  prompts, exact/truncated row estimates, application-digest expectations and
  create-new constraints. Authenticated template preview reproduces the signed
  template-state proof under separate total/per-dataset/deadline/byte limits.
- Added a duplicate-only `VerifiedCopySource` supporting signed or unsigned
  v0.2/v0.3 after exhaustive verification and complete signature-inventory
  evaluation.
- Implemented byte-exact duplicate with identical source/output SHA-256 and no
  invented lineage.
- Implemented compact duplicate through owner-private in-place `VACUUM`, a
  frozen typed logical-state digest (including accessible implicit rowid), zero
  freelist, deleted-sentinel absence and full pre/postpublish verification.
- Implemented signed-v0.3 semantic fork, authenticated template creation and
  selective fork. Actions are rederived from the signed contract at every
  authority transition; template `forbid` and one-source fork `reset` fail
  closed; dependencies and actual cross-dataset foreign keys remain closed.
- Generated new capsule/revision IDs and lineage for semantic outputs while
  preserving the signed application digest and complete signature inventory.
  Grants/change-log/sequence state are rebuilt under fixed host policy.
- Scrubbed omitted sensitive rows and mutable instance metadata before VACUUM;
  hostile raw-byte sentinels prove absence from published template/selective
  outputs and byte-identical source preservation.
- Added canonical exact/compact/semantic lifecycle-plan vectors, compact logical
  state and template-state vectors, schemas and independent Python checkers.
- Added the in-process CLI plan/execute flows and trusted Tauri wizard with
  opaque selection/destination/plan/operation identifiers, a bounded 30-second
  prepared authority, main-window-only progress and no filesystem paths in the
  renderer view.
- Kept every lifecycle command/event absent from the raw Wry application
  renderer and proved denial in locked and authorized states.

## Changed paths

- Workspace engines: `native/crates/capsule-workspace/src/{copy,copy_source,exact_copy,compact_state,compact_copy,semantic_copy,template_state}.rs`
- Held publication safety: `native/crates/capsule-lifecycle/src/lib.rs`
- CLI and trusted shell: `native/crates/capsule-cli/`,
  `native/desktop/src-tauri/src/{copy_flow,lib}.rs`, `native/desktop/ui/`
- Contracts and vectors: `docs/plans/capsule-lifecycle/contracts/`,
  `docs/plans/capsule-lifecycle/examples/`, `compatibility/*copy*`,
  `compatibility/{template-state-v1,semantic-copy-plan-v1}`
- Standalone authoring/plugin and Diagram review fixtures:
  `plugins/capsule-creator/`, `examples/diagram-studio/source/`
- Generated outputs: `capsules/diagram-studio.capsule.sqlite`, `exports/`,
  `capsules/sqlite-capsule-host-setup.exe`, native SBOM/license inventory

## Decisions and deviations

| Decision/deviation | Rationale | ADR or source |
| --- | --- | --- |
| A general application signature is insufficient for template cleanliness. | Signed application bytes exclude mutable domain rows; a signed exhaustive template-state proof binds every dataset seed/empty state. | ADR 0029; `docs/format-contract.md` |
| Fork/selective `reset` is unavailable with one working source. | Reset requires a separately authenticated clean source; treating reset as no-op would launder working data. | ADR 0029 |
| Compact logical state includes an accessible implicit rowid. | SQLite `VACUUM` may renumber rowids in PK-less rowid tables, which can change endpoint/view semantics. | compact logical-state/1 contract |
| Windows postpublication failure may leave an owner-private marker rather than rename the exact leaf. | This is the accepted M02 held-parent publication fallback; failure is never reported as success. | ADR 0024 |
| Prepared Tauri copy authority is 30 seconds; review/destination handles are five minutes. | A retained verified source has one 30-second operation budget. UI expiry must not overpromise authority. | Tauri copy `/1` contract |

## Acceptance evidence

| Acceptance clause | Status | Evidence |
| --- | --- | --- |
| No mode writes input; all inputs pinned read-only | pass | 277 Rust tests including exact/compact/semantic race, crash and source-hash assertions; `20260813T050716Z-gate-final-rust-settled` |
| No destination overwrite | pass | Held-parent create-new/no-replace tests, destination ABA/racer and sidecar-family tests; final Rust gate |
| v0.3 application digest preserved | pass | Semantic output assertions and canonical plan vectors; final Rust/vector gates |
| Policy-only omit/reset behavior | pass | Signed contract action rederivation, dependency/FK closure, template proof and hostile policy tests |
| Sensitive data cannot leak by default | pass | Selective/template raw sentinel, mutable instance metadata, freelist and sequence hostile tests |
| Generic/example boundary | pass | Static review and plugin/Diagram fixture gate; no Diagram identifiers in generic engines |
| Raw renderer has no lifecycle authority | pass | `20260813T051225Z-gate-final-native-raw-settled` |
| v0.2 compatibility | pass | Exact/compact signed+unsigned v0.2 matrix; fork/template return precise `unsupported_operation` |
| Stable, redacted failures | pass | Workspace error catalogue parity and Tauri hostile token/expiry/session tests |
| Documentation/contracts/generated artefacts synchronized | pass | Specs 137, vectors, plugin standalone, capsule/export/SBOM/license and NSIS gates |
| Independent review | pass | `evidence/M04/critic-report.md` and `evidence/M04/security-review.md` |

## Tests and validation

| Command | Environment | Result | Evidence path |
| --- | --- | --- | --- |
| `python -m unittest discover -s tests -v` | Windows, Python 3.13 | 176 passed | `evidence/M04/20260813T050716Z-gate-final-python-settled/evidence.json` |
| `cargo test --offline --workspace --all-targets` | Windows, pinned Rust | 277 passed across 20 suites | `evidence/M04/20260813T050716Z-gate-final-rust-settled/evidence.json` |
| `cargo fmt --all -- --check` | Windows | pass | `evidence/M04/20260813T051046Z-gate-final-fmt/evidence.json` |
| `cargo check --offline --workspace --all-targets` | Windows | pass | `evidence/M04/20260813T051045Z-gate-final-check/evidence.json` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Windows | pass | `evidence/M04/20260813T051046Z-gate-final-clippy/evidence.json` |
| lifecycle specification validator | Python + jsonschema | 137 checks, 12 examples | `evidence/M04/20260813T051046Z-gate-final-specs/evidence.json` |
| lifecycle/signed/template/compact independent vectors | Python | all pass | `evidence/M04/20260813T051046Z-gate-final-plan-vectors/evidence.json`; `20260813T051046Z-gate-final-signed-vectors`; `20260813T051011Z-gate-final-template-vectors`; `20260813T051045Z-gate-final-compact-vectors` |
| standalone plugin + Diagram contract tests | Windows | 21 passed | `evidence/M04/20260813T051101Z-gate-final-plugin-diagram/evidence.json` |
| capsule build/export checks and independent verify | Windows | pass | `evidence/M04/20260813T051101Z-gate-final-example`; `gate-final-exports`; `gate-final-capsule-verify` |
| SBOM, license and RustSec checks | Windows | pass, 17 reviewed RustSec warnings and no vulnerabilities | `evidence/M04/20260813T051101Z-gate-final-sbom`; `gate-final-license`; `gate-final-rustsec` |
| trusted-shell Windows E2E | WebView2/Save dialog | pass; exact output SHA equals source and verifies | `evidence/M04/20260813T051341Z-gate-final-native-copy-shell-settled/evidence.json` |
| raw Wry denial E2E | locked + authorized raw app | pass | `evidence/M04/20260813T051225Z-gate-final-native-raw-settled/evidence.json` |
| `python native/tools/build_installers.py` | Windows, NSIS-only debug bundle | pass | `evidence/M04/20260813T051416Z-gate-final-nsis-settled/evidence.json` |

## Security and critic review

- Independent critic: final PASS after authenticated-template preview and
  per-dataset scanner-bound fixes.
- Security critic: final PASS after prepared-authority expiry and path-display
  corrections.
- Findings were fixed in the authoritative scanner/controller and covered by
  hostile tests; no finding is waived.
- Accepted low-level behavior: a Windows postpublish failure can preserve the
  suspect final leaf plus a protected failure marker, but never returns success.

## Generated artefacts

| Artefact | Rebuild command | Digest/result |
| --- | --- | --- |
| `capsules/diagram-studio.capsule.sqlite` | `python tools/build_example.py` | 901,120 bytes; SHA-256 `d45eb5157ebe045e7fda20900450318472b5b925d72adeb61d49f5b81b971672`; 18 checks pass |
| three HTML exports | `python tools/build_exports.py` | view `00c89887...`; interactive `8bc65777...`; editable `7d5ee4b9...`; all independently verified |
| `native/sbom.cdx.json` / `native/THIRD_PARTY_LICENSES.md` | native generators | current; checks pass |
| `capsules/sqlite-capsule-host-setup.exe` | `python native/tools/build_installers.py` | NSIS only; 7,257,507 bytes; SHA-256 `71e323dafe6d1d4598d0248960826d082877a9aa3500a2467a6803136109acde` |

## Remaining limitations

- M04 intentionally does not compare or reconcile capsules; that work starts in
  M05/M06.
- Semantic fork `reset` stays unavailable until a second authenticated clean
  source can be bound; template creation already uses the required proof.
- Publisher trust is host-local and remains distinct from valid signature state.

## Handoff

**Next milestone:** M05 — Bounded compare engine and trusted shell comparison  
**First action:** add a read-only `compare` module in `capsule-workspace` that opens two retained verified sources, classifies the six compatibility outcomes, and emits only a bounded `/1` identity/application/schema/dataset summary. Freeze the typed value/row digest grammar and adversarial vector before adding detail pagination or Tauri commands.  
**Relevant files:** `native/crates/capsule-workspace/src/`, `docs/plans/capsule-lifecycle/contracts/compare-report-v1.schema.json`, ADR 0027, `native/crates/capsule-cli`, trusted Tauri copy/session patterns.  
**Known hazards:** comparison must execute nothing, must not reopen paths or infer undeclared tables, must keep sensitive datasets counts-only until trusted reveal, and must never expose table/path/value authority to raw Wry.  
**Do not repeat:** reuse retained verified sources, workspace controls, stable error envelopes, opaque Tauri sessions and held selection binding; do not build another path-based inspection surface.

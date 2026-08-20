# Result — M06: Apply selected changes to a new target-derived copy

**State:** complete  
**Started:** 2026-08-13T07:30:30Z  
**Completed:** 2026-08-20T06:47:39Z  
**Repository commit at start:** `e73cf948fba233ef84d4680930b61549012020a7`  
**Repository commit at completion:** this milestone commit on `main`

## Outcome

Users can review compatible changes and apply an exact allowlisted subset to a
new capsule derived from the retained target. Two-way review supports row and
field selection; three-way review uses an independently verified ancestor to
classify and resolve explicit conflicts. Neither input nor the ancestor is ever
modified, the target application/signature compartment is preserved, and no
output becomes visible until the reviewed plan has executed and the reopened
capsule has passed exhaustive verification.

## Scope delivered

- Added a non-serializable reconciliation review over retained verified source
  and target snapshots. The core independently recomputes and exactly matches
  the non-truncated Compare report; callers cannot authenticate a forged report
  by recomputing its public digest.
- Froze strict value-free `reconcile-plan/1` payload and `LifecyclePlan`
  contracts, canonical vectors and independent Python verification. Tagged
  absent/present row states keep SQL NULL distinct from absence.
- Implemented four operations: insert source row, delete target row, replace the
  target row from source and set selected target fields from source. Exact typed
  keys, row/value digests and private write-set preconditions are rebound before
  writes; signed zero, storage class, UTF-8 and BLOB identity remain distinct.
- Added exact signed-policy enforcement. `forbid` blocks reconciliation globally;
  manual and three-way policies require the exact reviewed operation mode;
  immutable/generated/ignored columns retain their documented write semantics;
  sensitive confirmation is bound to the exact changed dataset set.
- Implemented optional three-way classification with a separately supplied,
  pinned and compatible ancestor. Insert/insert, update/update, delete/update and
  immutable-field conflicts use opaque host-owned resolution authority; every
  conflict must be resolved and immutable choices remain fail-closed.
- Implemented the current FK reconciliation profile over the complete signed
  contract graph: acyclic `NO ACTION`/`RESTRICT` relationships, parent-first
  inserts/updates, child-first deletes, deferred enforcement and final
  `foreign_key_check`. Cascades, set actions and cycles return
  `unsupported_operation` rather than inducing unreviewed writes.
- Implemented target-derived private execution and publication typestates. The
  executor copies the exact target, applies domain operations transactionally,
  finalizes revision/lineage privately, preserves natural `sqlite_sequence`
  high-water state, vacuums removed bytes, exhaustively validates, then uses the
  held-parent create-new/no-replace lifecycle publisher and postpublish reopen.
- Preserved target capsule ID, application digest, complete signature inventory,
  schema, instance/platform state and prior lineage. A fresh UUIDv4 revision and
  event append exact `target-derived-from` and `changes-applied-from` parents;
  optional ancestor evidence is bounded and never treated as signed authority.
- Added separate time domains: classification/resolve/native execution work is
  capped at 30 seconds, while host-owned human review authority is capped at five
  minutes and cannot be refreshed. Expiry maps precisely to `limit_exceeded` or
  `session_expired`.
- Added stable value-free CLI JSON for two-way and three-way candidates, review
  and execution. CLI input is non-authoritative and reconstructs the same pinned
  core typestate in one process.
- Added trusted Tauri reconcile sessions with opaque selection, ancestor,
  conflict, resolution, destination, prepare and operation tokens. Token vectors
  are prebounded and indexed; cancellation, expiry, replay and cross-session use
  fail closed. No path, SQLite identifier, digest, raw value or serialized plan
  crosses JavaScript as authority.
- Added accessible trusted-shell review, conflict resolution, progress/result
  handling and idempotent terminal finalization. All ten reconciliation commands
  remain absent from raw Wry authority.
- Synchronized ADR 0027, architecture/security/format/native/authoring docs,
  CLI/Tauri schemas/examples, the standalone Capsule Creator skill and its
  authoring/runtime guidance.
- Added declarative Diagram Studio reconciliation fixtures and executable signed
  v0.3 generic fixtures. Real Windows two-way and pure-three-way flows independently
  build expected outputs, compare actual-to-expected with M05 and retain exact
  signatures, application/data contracts, lineage and unchanged-input evidence.

## Changed paths

- Core and CLI: `native/crates/capsule-workspace/src/{reconcile,lib}.rs`,
  `native/crates/capsule-workspace/src/bin/capsule-workspace.rs` and CLI tests.
- Trusted host/UI: `native/desktop/src-tauri/src/{reconcile_flow,compare_flow,lib}.rs`,
  Tauri permissions/generated capability schemas and `native/desktop/ui/`.
- Contracts/vectors: `compatibility/reconcile-plan-v1/` and lifecycle, payload,
  CLI/Tauri reconcile schemas/examples/checkers.
- Documentation/plugin: ADR 0027, format/architecture/security/native/authoring
  docs and `plugins/capsule-creator/` guidance, assets and regression tests.
- Fixtures/tests: Diagram data/reconcile fixtures, Rust hostile/crash tests,
  Python contract/plugin/static UI tests and native trusted/raw E2E.
- Generated/release outputs: Diagram capsule and three exports, SBOM/license
  inventory and `capsules/sqlite-capsule-host-setup.exe`.

## Decisions and deviations

| Decision/deviation | Rationale | ADR or source |
| --- | --- | --- |
| Output is always an exact target-derived create-new capsule. | Preserves the reviewed target application/capsule identity and eliminates in-place/overwrite ambiguity. | ADR 0027; M06 `EXECPLAN.md` |
| An ancestor is explicit retained input, never inferred from mutable lineage. | Lineage can describe a claim but cannot grant read or merge authority. | ADR 0027; security review |
| `reconcile=forbid` blocks the entire transform. | The signed wording says the transform is forbidden, including differences hidden by summary policy. | format contract; critic review |
| Sensitive confirmation binds exact changed dataset IDs, including keep-target conflicts. | Disclosure of one dataset must never authorize another or disappear merely because resolution emits zero writes. | ADR 0027; security review |
| Current FK support is acyclic and restrictive only. | Deterministic ordering and final FK proof are safe without unreviewed cascade/set effects; unsupported graphs fail closed. | format contract; M06 review |
| Human review authority and native work budgets are separate. | Users may resolve conflicts for up to five minutes, while each native scan/resolve/execute remains capped at 30 seconds. | Tauri contract; deadline review |
| CLI JSON is evidence/input, not authority; Tauri uses opaque capabilities. | Neither serialized plans nor renderer-controlled indices/paths may bypass retained typestate. | architecture/security docs |
| Diagram Studio remains a declarative fixture for M06. | The shipped example capsule is intentionally verified legacy v0.2 with no signed data schema and is therefore outside v0.3 reconciliation admission. Executable E2E uses the repository's generic signed v0.3 fixture. | M06 `EXECPLAN.md`; fixture tests |

## Acceptance evidence

| Acceptance clause | Status | Evidence |
| --- | --- | --- |
| No in-place merge; create-new/no-replace only | pass | Executor typestate, destination-race/ABA/crash tests and trusted E2E |
| Inputs remain byte-identical | pass | Core hostile/crash matrix and `evidence/M06/ui/native-m06-reconcile-evidence.json` |
| Target application/signature compartment unchanged | pass | Exact output validator and trusted E2E full signature/application/data-contract equality |
| Unresolved conflicts/failed validation block publication | pass | Three-way resolution matrix, stale/forbid/FK/UNIQUE/CHECK/cancel tests |
| Output equals reviewed plan | pass | Exact canonical payload/LifecyclePlan approval, private dry-run state, reopened validation and expected-output M05 comparison |
| Policy, sensitivity and immutable fields fail closed | pass | Global-forbid, exact sensitive-set/keep-target and immutable/generated/ignored hostile tests |
| Trusted-only command boundary | pass | Focused raw boundary E2E denies all ten commands locked and authorized |
| Generic/example boundary and v0.2 stability | pass | Static/plugin/full suites; Diagram-specific vocabulary remains fixtures/docs only |
| Documentation/contracts/generated artefacts synchronized | pass | lifecycle validator, schemas/vectors, plugin 19/19, capsule/export/SBOM/license gates |
| Independent review | pass | `evidence/M06/critic-report.md`; `evidence/M06/security-review.md` |

## Tests and validation

| Command | Environment | Result | Evidence path |
| --- | --- | --- | --- |
| `python -m unittest discover -s tests -v` | Windows, Python 3.13 | 202 passed; 1 optional `jsonschema` skip | `evidence/M06/final-gates.json` |
| PowerShell Draft 2020-12 `Test-Json` schema validation | Windows PowerShell | pass for lifecycle/payload/CLI/Tauri examples | `evidence/M06/final-gates.json` |
| `cargo test --offline --workspace --all-targets -- --test-threads=1` | Windows, Rust 1.97.1 | 373 passed; 0 failed | `evidence/M06/final-gates.json` |
| `cargo fmt --all -- --check` | Windows | pass | `evidence/M06/final-gates.json` |
| `cargo check --offline --workspace --all-targets` | Windows | pass | `evidence/M06/final-gates.json` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Windows | pass | `evidence/M06/final-gates.json` |
| `npm run test:browser` | Windows Chromium | 4 passed | `evidence/M06/final-gates.json` |
| `npm run test:browser:html` | Windows Chromium | 59 passed; 13 expected project/platform skips | `evidence/M06/final-gates.json` |
| lifecycle specification validator | Python | 224 records/checks after completion | `evidence/M06/20260820T064808Z-final-completed-lifecycle-specs/evidence.json` |
| reconcile CLI/Tauri/contracts/vectors/Diagram tests | Python + PowerShell schemas | 25 passed; semantic schemas pass | `evidence/M06/20260820T063633Z-final-reconcile-contracts/evidence.json` |
| trusted-shell reconcile E2E | rebuilt Windows WebView2 host | two-way and pure-three-way pass; expected compare and exact lineage/signatures | `evidence/M06/ui/native-m06-reconcile-evidence.json` |
| raw-Wry boundary-only E2E | locked + authorized raw app | pass; ten commands denied and Tauri globals absent | `evidence/M06/final-gates.json` |
| terminal poll/event race test | Node | pass; one terminal finalizer/acknowledgement | `evidence/M06/20260820T063711Z-final-js-terminal-race/evidence.json` |
| standalone creator plugin | copied outside repository | 19 passed | `evidence/M06/final-gates.json` |
| capsule/export/verify checks | Windows | pass; 18 capsule checks | `20260820T063652Z-final-example-check`; `final-exports-check`; `final-capsule-verify` |
| SBOM/license/RustSec checks | Windows, fresh RustSec database | pass; no vulnerabilities, 17 exact warnings | `20260820T063700Z-final-sbom-check`; `final-license-check`; `20260820T063907Z-final-rustsec` |
| `python native/tools/build_installers.py --bundles nsis` | Windows x86-64, debug unsigned | pass; NSIS only | `evidence/M06/20260820T063754Z-final-nsis-installer/release-evidence.json` |

The one Python skip is only the optional `jsonschema` package path; equivalent
Draft 2020-12 semantic validation passed through PowerShell `Test-Json`. Browser
HTML skips are explicit project/platform-conditional cases, not suppressed M06
failures. An initial browser launch hit sandbox `EPERM`; the same tests passed in
the required elevated filesystem context with a fresh ignored state directory.

## Security and critic review

- Independent critic: static PASS after global-forbid, exact sensitive scope,
  FK ordering, schema parity, deadline separation and resolution-vector bounds
  were fixed; no substantive source/contract finding remained.
- Security reviewers: executor PASS and final static no-blocker review after
  retained authority, sequence, UUID, crash/rollback and raw boundary fixes.
- The user requested no further duplicate subagent review cycles during final
  packaging. Root completed the final settled repository, browser, E2E,
  generated-artifact, supply-chain and installer gates directly.
- No finding is waived and no UI-hiding-only security dependency remains.

## Generated artefacts

| Artefact | Rebuild command | Digest/result |
| --- | --- | --- |
| `capsules/diagram-studio.capsule.sqlite` | `python tools/build_example.py` | 913,408 bytes; SHA-256 `76c883e39d6d9617919fe0f3c1f1974e63b97c9377fb39d4cb3e1f6031890e2e`; 18 checks |
| three HTML exports | `python tools/build_exports.py` | view `2b3e1c91…ae884`; interactive `62308a32…d3f`; editable `d7540f13…b83` |
| `native/sbom.cdx.json` / `native/THIRD_PARTY_LICENSES.md` | native generators | current; checks pass |
| `capsules/sqlite-capsule-host-setup.exe` | `python native/tools/build_installers.py --bundles nsis` | NSIS only; 7,780,957 bytes; SHA-256 `2fb2bda1dd3329228e02fe00ed43dad44ce6df13ed93752abd4d210774af8db8`; development-unsigned |

## Remaining limitations

- Reconciliation requires compatible signed v0.3 data contracts. The shipped
  Diagram Studio capsule remains supported legacy v0.2 and is not silently
  admitted to M06; its reconciliation fixture is declarative only.
- The current reconcile FK profile intentionally rejects cascades, set actions,
  self/cyclic graphs and other induced-effect cases with `unsupported_operation`.
- Human reconciliation review expires after at most five minutes; every native
  classify/resolve/execute operation remains capped at 30 seconds.
- The retained installer is a development-unsigned NSIS build. No MSI was built,
  changed, exported or claimed.
- The required focused raw reconciliation boundary passed. The broader legacy
  raw suite later encountered an unrelated save-picker restore failure after its
  boundary assertions, so this result does not misreport that unrelated path.

## Handoff

**Next milestone:** M07 — Application release upgrade with unchanged data schema  
**First action:** freeze an upgrade review contract over a retained working
capsule and a clean signed template/release capsule with the same signed data
schema ID/version. Bind exact input hashes, publisher/signature evidence,
application digests, expected preserved dataset states and target-derived output
identity before adding any executor.  
**Relevant files:** M07 `EXECPLAN.md`, ADRs 0025/0027/0029,
`native/crates/capsule-workspace/src/`, lifecycle publication typestates,
trusted compare/reconcile controller patterns and `upgrade-plan-v1.schema.json`.  
**Known hazards:** publisher trust is host policy separate from signature
validity; the clean template is an explicit retained authority; application
replacement must preserve all user datasets exactly; v0.2 remains unsupported
unless a future accepted compatibility decision says otherwise; raw Wry stays
command-free.  
**Do not repeat:** reuse retained snapshots, application-family digests, exact
dataset-state proofs, opaque trusted-shell capabilities, separate review/work
deadlines and create-new quarantine publication from M04–M06.

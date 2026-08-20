# Result — M07: Application release upgrade with unchanged data schema

**State:** complete

**Started:** 2026-08-20T07:05:16Z

**Completed:** 2026-08-20T10:15:17Z

**Repository commit at start:** `e5b1170b410f310dafd5726cf7697dea58223081` plus execution-protocol commit `4fbe04a` (cherry-picked as `28765f1`)

**Repository commit at completion:** this milestone commit on `codex/m07-same-schema-upgrade`

## Outcome

A verified v0.3 working capsule can now be upgraded to a newer signed release
with the exact same signed data-schema ID/version. The output begins from the
clean target release, preserves the working capsule's user-owned data/profile
and capsule identity according to signed dataset policy, retains the target's
complete application/signature compartment, records both inputs in lineage and
is published create-new only after exhaustive private validation and reopen.

## Scope delivered

- Added a non-serializable upgrade review and Prepared/Staging/Validated/
  Published typestate pipeline over retained, verified, read-only working and
  clean-release inputs.
- Enforced v0.3, same application, strict newer SemVer, exact signed data-schema
  ID/version, supported runtime/format profile, complete signature validity and
  one exact common accepted publisher key. SemVer is bounded to 128 bytes,
  rejects malformed empty identifiers and compares valid arbitrary-length
  numeric identifiers without machine-integer limits.
- Derived the output from the exact target release. Signed `copy`, `target`,
  `rebuild` and `omit` policies have explicit state proofs; `migrate` and
  `forbid` fail closed in M07.
- Preserved working capsule ID, instance profile fields, eligible instance
  assets and exact source dataset states; minted a new revision; cleared grants
  and old lineage; retained the target application assets, manifest, endpoints,
  contracts and complete signature inventory.
- Bound application digest, target clean-state proof, exhaustive source/target/
  expected dataset states, capability delta and lifecycle plan into the review.
  Added/changed capabilities require explicit review; removed capabilities are
  reported without granting new authority.
- Published through held-parent, create-new/no-replace lifecycle primitives with
  private staging, late input rebinding, `VACUUM`, exhaustive validation and
  post-publication reopen. Cancellation, destination races and six crash stages
  cannot mutate either input or report false success.
- Added value-free `plan-upgrade`/`upgrade` CLI JSON with exact confirmation,
  and trusted Tauri selection/destination/review/operation capabilities that
  retain exact paths and publisher authority outside JavaScript.
- Added the Versions “Upgrade application” wizard with release screening,
  explicit full admission at Prepare, capability/dataset review, exact publisher
  confirmation, bounded progress/cancellation and terminal result handling.
- Kept all seven upgrade commands absent from raw Wry authority and verified
  denial in both locked and otherwise authorized raw-renderer states.
- Synchronized ADR 0030, architecture/format/security/native/authoring docs,
  lifecycle schemas/error catalogue/example, Tauri permissions/generated
  schemas and the standalone Capsule Creator skill/references/tests.
- Added deterministic generic signed upgrade fixtures and real Windows E2E that
  proves target application replacement, exact user state, lineage, zero grants,
  source/target immutability, path secrecy and 200% layout.

## Changed paths

- Core and CLI: `native/crates/capsule-workspace/src/upgrade.rs`, workspace
  exports/errors/plans and `src/bin/capsule-workspace.rs`.
- Trusted host/UI: `native/desktop/src-tauri/src/upgrade_flow.rs`, command
  registration/permissions/generated capability schemas and
  `native/desktop/ui/`.
- Contracts/docs/plugin: ADR 0030, lifecycle upgrade schema/example/error
  catalogue, architecture/format/security/native/authoring docs and the
  standalone Capsule Creator guidance/tests.
- Tests/evidence: Rust hostile/crash tests, Python static/plugin tests, trusted
  and raw native E2E, deterministic fixture builder and `evidence/M07/`.
- Generated/release outputs: Diagram capsule, three HTML exports and the stable
  NSIS installer export.

## Decisions and deviations

| Decision/deviation | Rationale | ADR or source |
| --- | --- | --- |
| Output always begins from the exact clean target release. | Prevents a patch-in-place implementation from retaining stale application state and makes the target application/signature compartment the authority. | ADR 0030; M07 `EXECPLAN.md` |
| One exact host-selected common valid publisher key is retained through review. | Complete signature inventories may contain multiple keys; continuity must bind one accepted key without trusting renderer choice or assuming a singleton. | ADR 0030; consolidated review |
| Signed target template state is proven before user data is applied. | A release containing undeclared user state is not a clean release and cannot become an upgrade base. | ADR 0030; format/security contracts |
| `rebuild` keeps the declared clean target state and `omit` requires its defined empty/absent state. | M07 never runs application code or silently discards non-empty state. | ADR 0030; upgrade plan schema |
| `migrate` and `forbid` both reject. | Schema-changing migration belongs to M08; forbidden policy cannot be weakened into another transform. | M07 explicit non-scope; ADR 0030 |
| Full compatibility admission occurs at Prepare, not release selection. | Selection screens only signature/template safety; the UI must not claim same-app/schema/newer admission before core review construction. | Remediation review |
| Diagram Studio remains only a generated regression fixture. | Generic upgrade code contains no Diagram-specific tables, endpoints or rendering concepts; executable upgrade E2E uses a generic signed v0.3 fixture. | Repository `AGENTS.md`; M07 `EXECPLAN.md` |

## Acceptance evidence

| Acceptance clause | Status | Evidence |
| --- | --- | --- |
| Output begins from and retains the clean target application release | pass | Core target-derived validator and `evidence/M07/ui/native-m07-upgrade-evidence.json` prove target manifest/assets/endpoints/signature inventory |
| User data/profile survive according to signed policy | pass | Upgrade truth-table tests plus native evidence show exact `copy` rows, instance profile and instance asset; `target`/`rebuild`/`omit` cases are covered |
| Application digest matches target exactly | pass | Core reopen validation and native evidence record target digest `9a200816…eaeebd7b` on output event/signature/application state |
| Schema mismatch cannot enter this code path | pass | Same signed schema ID/version is checked before review authority; mismatch, different app/key, invalid signature and non-newer versions have stable hostile tests |
| Original and target release remain unchanged | pass | Pinned input rebinding, crash/race matrix and native SHA-256 evidence for both inputs |
| Read-only/pinned inputs and create-new/no-replace output | pass | Core typestate, late-change/alias/destination-race tests and native publication evidence |
| Generic boundary and v0.2 behavior remain stable | pass | Full Python/Rust/browser suites; generic module contains no Diagram vocabulary; no v0.2 admission change |
| Raw Wry receives no lifecycle authority | pass | Full `npm run test:native:raw` denies all seven upgrade commands in locked and authorized states |
| Stable fail-closed errors without sensitive values | pass | `version_not_newer` catalogue/schema synchronization, hostile matrix and value-free CLI/Tauri contracts |
| Documentation, contracts, plugin and generated artefacts synchronized | pass | Lifecycle validator 228 checks/records after final evidence/status; plugin/static suites; example/export/capsule verification gates |
| Focused and repository-wide verification | pass | `evidence/M07/final-gates.json`; 10/10 core upgrade, 4/4 Tauri authority and all final qualification gates pass |
| Independent review findings resolved | pass | `evidence/M07/consolidated-review.md`; no P0/P1, all initial findings and one remediation P2 resolved |
| Status and executable handoff recorded | pass | Completed `PROGRAM_STATUS.json`, this result and M08 handoff below |

## Tests and validation

| Command | Environment | Result | Evidence path |
| --- | --- | --- | --- |
| Focused upgrade/Tauri/UI/plugin/package checks | Windows; Rust/Python/Node | core 10/10, Tauri authority 4/4, UI/plugin 26/26; package checks pass | `evidence/M07/final-gates.json` |
| `python -m unittest discover -s tests -v` | Windows, Python | settled 202 passed; 1 optional `jsonschema` skip | `evidence/M07/final-gates.json` |
| lifecycle validator with `--require-jsonschema` | Windows, Python 3.12 | 228 checks/records after final evidence/status; 20 examples schema-valid | `evidence/M07/final-gates.json` |
| `cargo test --offline --workspace --all-targets -- --test-threads=1` | Windows, Rust | settled 388 passed; 0 failed | `evidence/M07/final-gates.json` |
| `cargo fmt`, workspace `check`, workspace `clippy -D warnings` | Windows, Rust | pass | `evidence/M07/final-gates.json` |
| `npm run test:browser` / `test:browser:html` | Windows Chromium | 4 passed; 59 passed and 13 expected conditional skips | `evidence/M07/final-gates.json` |
| trusted shell baseline and upgrade native E2E | Windows WebView2 | 6 baseline checks plus exact upgrade flow pass | `evidence/M07/ui/native-m07-upgrade-evidence.json` |
| full raw-Wry E2E and terminal finalization | Windows WebView2/Node | pass | `evidence/M07/final-gates.json` |
| example/export freshness and capsule verification | Windows | pass; 18 capsule checks | `evidence/M07/final-gates.json` |
| SBOM/license/RustSec | Windows; local advisory database | pass; no vulnerabilities and 17 exact reviewed warnings | `evidence/M07/final-gates.json` |
| `python native/tools/build_installers.py --bundles nsis` | Windows x86-64, debug unsigned | pass; one NSIS build/export only | `evidence/M07/20260820T101304Z-final-nsis-installer/release-evidence.json` |

The first full Python run correctly exposed stale generated capsule/export
outputs; they were rebuilt and only the affected freshness test was rerun. The
first full Rust run exposed lifecycle error-catalogue ordering drift; that
production catalogue was corrected and only the affected test was rerun. The
settled counts above include those directly affected reruns. The optional Python
skip is independently covered by the required Python 3.12 `jsonschema` run.
Browser HTML skips are explicit existing project/platform cases, not suppressed
M07 failures.

## Security and critic review

- One consolidated independent read-only review ran only after the first frozen
  candidate passed focused checks; it ran no Cargo/build process.
- Initial findings covered strict SemVer edge cases, multiple valid signatures,
  selection-bound cancellation and missing hostile/crash coverage. All were
  fixed before qualification.
- The single allowed remediation review found no P0/P1 and one P2 UI wording
  mismatch. Release selection now claims screening only; full compatibility
  admission remains at Prepare. Directly affected UI/native checks passed.
- Opaque tokens and retained exact snapshots remain native authority. Raw Wry
  has no upgrade command registration and the complete raw suite passed. No
  finding or UI-only authorization exception is accepted.

## Generated artefacts

| Artefact | Rebuild command | Digest/result |
| --- | --- | --- |
| `capsules/diagram-studio.capsule.sqlite` | `python tools/build_example.py` | 921,600 bytes; SHA-256 `567b4a6fb1c22d16d47baee46ac2ac60542f044ee65c70942fe1df3b76f10586`; 18 checks |
| three HTML exports | `python tools/build_exports.py` | view `826a709d…47f3`; interactive `1e771333…ede`; editable `84aa9b2e…e8e6`; freshness passes |
| `native/sbom.cdx.json` / `native/THIRD_PARTY_LICENSES.md` | native generators | current; checks pass |
| `capsules/sqlite-capsule-host-setup.exe` | `python native/tools/build_installers.py --bundles nsis` | NSIS only; 7,946,886 bytes; SHA-256 `823c455159abe2fa6f4eedfc54c8782ea0a2ea6301f9e96e61b9f921f12c2896`; development-unsigned |

## Remaining limitations

- M07 requires the exact same signed data-schema ID/version. Declarative schema
  migration is intentionally rejected until M08.
- Publisher key rotation/delegation and cross-application import remain outside
  M07; at least one exact valid accepted publisher key must be common to both
  inputs.
- Upgrade requires signed v0.3 contracts. Existing v0.2 runtime behavior remains
  supported but is not silently admitted to this transform.
- The retained installer is a development-unsigned NSIS build. No MSI was built,
  changed, exported or claimed.

## Handoff

**Next milestone:** M08 — Restricted declarative data migrations

**First action:** freeze a signed, value-free migration-plan contract for one
explicit forward data-schema transition. Bind exact source/target schema IDs and
versions, retained input/application hashes, declared operations, affected
dataset state proofs, limits and expected output before implementing any
migration executor.

**Relevant files:** M08 `EXECPLAN.md` and `ACCEPTANCE.md`, ADR 0030,
`upgrade.rs`, lifecycle plan/publication typestates and upgrade plan/error
contracts.

**Known hazards:** never run application code or arbitrary SQL as migration;
publisher continuity remains host policy; migration must preserve create-new,
read-only input, raw-Wry denial, exact write-set and post-reopen proof.

**Do not repeat:** reuse retained verified snapshots, clean target-derived
staging, exact dataset-state commitments, opaque trusted-shell authority,
capability-delta review, crash/race handling and two-parent upgrade lineage from
M07.

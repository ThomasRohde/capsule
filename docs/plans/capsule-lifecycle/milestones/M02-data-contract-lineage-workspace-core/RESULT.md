# Result — M02: Data contract, lineage and workspace service foundation

**State:** complete  
**Started:** 2026-08-12T17:31:24Z  
**Completed:** 2026-08-12T20:10:53Z  
**Repository commit at start:** `e73cf948fba233ef84d4680930b61549012020a7`  
**Repository commit at completion:** `e73cf948fba233ef84d4680930b61549012020a7`

## Outcome

The product-independent native workspace now accepts only pinned, read-only,
exhaustively verified and cryptographically signed v0.3 snapshots; validates
bounded signed data contracts and redacted mutable lineage; emits deterministic
review plans; promotes only fully rebound plans to host authority; and can
publish an exact duplicate only through owner-private, create-new, no-replace,
pre/post-verified output typestate.

No capsule-workspace command is registered with Tauri or the raw Wry renderer.
The diagnostic native CLI exposes versioned, bounded overview, data-contract,
lineage and plan-validation JSON only. Both independent and security critics
report no remaining M02 implementation or security blocker.

## Scope delivered

- Added `sqlite-capsule-workspace` with caller-lowerable hard-capped limits,
  cancellation/deadline control, all 32 stable lifecycle error envelopes and
  exact verified-source authority.
- Loaded signed v0.3 dataset contracts with max-plus-one enumeration, exhaustive
  table classification, exact-case bounded UTF-8 identifiers, ordered SQLite
  PK semantics, binary/ascending collation, policy invariants and dependency
  cycle validation.
- Loaded mutable lineage as explicitly unauthenticated provenance with bounded
  sequence/parents/JSON and no raw `details_json` in public reports.
- Implemented strict canonical lifecycle-plan parsing/digests, deterministic
  duplicate-plan generation, complete identity/digest/schema/publisher/expiry/
  destination rebinding, and opaque prepared-plan authority. Edited plans with
  recomputed digests gain no additional authority.
- Implemented held-parent component walks, owner-only staging, exact held-file
  verification, no-replace publication, final leaf/input rebinding, and
  quarantine/private-marker failure handling. M02 execution supports only exact
  duplicate; all later operations fail closed.
- Added versioned CLI response profiles and standalone signed-v0.3 projection
  tests for overview, data contract and redacted lineage.
- Synchronized the v0.3 creator plugin, Diagram Studio data-contract fixtures,
  schemas, architecture/security/native docs, generated example/exports, SBOM,
  licenses and the NSIS-only native installer.

## Changed paths

- `native/crates/capsule-workspace/` — verified sources, errors, contracts,
  lineage, plan/parser/planner, publication typestate and CLI.
- `native/crates/capsule-{lifecycle,launch,cli,runtime}/` — held-handle private
  publication, exact snapshot/control, diagnostic projections and race tests.
- `docs/plans/capsule-lifecycle/contracts/`, `examples/`, `compatibility/` —
  aligned data-contract, lineage, lifecycle-plan and vector contracts.
- `examples/diagram-studio/source/data-contract.json`, `tests/` — reviewable
  sensitive/derived fixtures and deterministic projection coverage.
- `plugins/capsule-creator/` — standalone v0.3 validation, byte/collection/PK/
  policy bounds, documentation and signed native-projection integration.
- `docs/{architecture,native-host-contract,security}.md`, `native/README.md` —
  trusted-workspace ownership, CLI profiles, publication and crash posture.
- Generated Diagram Studio capsule/HTML exports, native SBOM/licenses and NSIS.

## Decisions and deviations

| Decision/deviation | Rationale | ADR or source |
| --- | --- | --- |
| M02 executes only exact duplicate | The milestone needs a proven publication foundation; fork/template/reconcile/upgrade semantics remain owned by later milestones | ADR 0024; M02/M04 scope |
| Serialized plans are review data, never authority | A recomputed digest cannot mint source or destination capabilities | ADR 0024 |
| Exact duplicate consumes zero logical row-inspection/write budget | It copies the already verified private snapshot byte-for-byte; positive caller-lowered row ceilings are therefore satisfied | M02 EXECPLAN limits |
| Existing lineage is structurally validated but not required to equal the current revision | Named ordinary writes advance revision without creating lifecycle lineage | ADR 0021; M02 EXECPLAN |
| Windows directory durability uses file sync plus held-parent identity rebind | Windows has no portable directory-fsync equivalent for this handle; false post-rename failures are forbidden | Security review |
| Non-Linux Unix publication is unavailable rather than using copy/replace fallback | Only proven native no-replace primitives may publish | ADR 0024 |

## Acceptance evidence

| Acceptance clause | Status | Evidence |
| --- | --- | --- |
| Generic crate contains no Diagram Studio identifiers | pass | Static scan; only generic “shape” terminology in test names, no application tables/endpoints/rendering concepts |
| Every lifecycle write targets private create-new output | pass | Workspace typestate and lifecycle 15-test Windows publication matrix |
| Stale plan/existing destination fail closed | pass | Recomputed-plan matrix, exact `stale_plan` parent/source tests, no-replace race tests |
| Dataset semantics come exclusively from verified signed contract | pass | Same-snapshot signature gate plus exhaustive contract/PK/cycle/bounds tests |
| Lineage is provenance, not authentication | pass | Redacted `mutable-untrusted` projection/schema and hostile-lineage tests |
| Inputs remain read-only and unchanged | pass | Read-only connection test; before/capture/transform/final-window races; crash matrix hashes source before/after |
| Outputs are create-new and refuse existing destinations | pass | Held-parent no-replace race and exact duplicate publication tests |
| Generic/application boundaries preserved | pass | No workspace dependency in desktop/Wry; sole non-test unsafe publisher call is workspace typestate |
| V0.2 behaviour remains compatible | pass | Full Python/Rust suites and unchanged v0.2 signed vector digest/size |
| Stable redacted errors | pass | Exact 32-code catalogue parity; expiry/parent/source/output exact-code tests; no raw paths/SQL/rows in envelopes |
| Docs/contracts/plugin/generated artefacts synchronized | pass | Strict 80-record schema validation, standalone plugin suite, example/export/SBOM/license freshness |
| Critic and security findings resolved | pass | Final security PASS and independent audit after adversarial/crash/CLI/error fixes |

## Tests and validation

Environment: Windows x86-64; CPython 3.13.7 for repository commands; CPython
3.12.4 with `jsonschema` for strict Draft 2020-12 validation; Rust/Cargo 1.97.1
MSVC.

| Command | Result | Evidence path |
| --- | --- | --- |
| `python -m unittest discover -s tests -v` | pass, 168 tests in 91.298 s | `evidence/M02/20260812T200041Z-gate-final-python-rerun/` |
| `cargo test --workspace --all-targets` | pass, 191 tests, 0 failed/ignored | `evidence/M02/20260812T195846Z-gate-final-rust-workspace/` |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass | `evidence/M02/20260812T200005Z-gate-final-rust-clippy/` |
| `cargo check --workspace --all-targets` | pass | `evidence/M02/20260812T200031Z-gate-final-rust-check/` |
| `cargo fmt --all -- --check` | pass | `evidence/M02/20260812T200035Z-gate-final-rust-fmt/` |
| Strict lifecycle spec validator | pass, six examples and 80 records | `evidence/M02/20260812T200238Z-gate-final-specs/` |
| Lifecycle-plan canonical vectors | pass, exact Python/Rust bytes/digests | `evidence/M02/20260812T200238Z-gate-final-plan-vectors/` |
| Signed application vectors `--all` | pass, v0.2/v0.3 | `evidence/M02/20260812T200238Z-gate-final-signed-vectors/` |
| Example/export freshness and capsule verify | pass; 18 declared checks | `evidence/M02/20260812T200239Z-gate-final-example/`; `...gate-final-exports/`; `evidence/M02/20260812T200253Z-gate-final-capsule-verify/` |
| SBOM/license `--check` | pass | `evidence/M02/20260812T200254Z-gate-final-sbom/`; `evidence/M02/20260812T200256Z-gate-final-license/` |
| `git diff --check` | pass | `evidence/M02/20260812T200256Z-gate-final-git-diff-check/` |
| NSIS-only installer rebuild/export | pass, development-unsigned | `evidence/M02/20260812T200303Z-gate-nsis-installer/` |

Two superseded failures are retained honestly: the first Rust workspace run
exposed an overly specific fail-closed runtime error assertion, and the first
Python suite exposed a stale generated Diagram Studio capsule. Both were fixed
and the named final reruns pass.

## Security and critic review

- Reviewers: builder `m00_builder`, independent critic
  `m00_independent_critic`, security critic `m00_security_critic`.
- Resolved findings include snapshot/signature budgets, nullable/ordered PKs,
  exact plan bindings, destination reparse/ACL/FileId handling, unforgeable
  publication typestate, post-rename quarantine, stable error semantics and
  versioned CLI roots.
- Adversarial evidence includes real Windows junction and hostile inherited ACL
  tests; held-parent/final-leaf substitution; create-new races; same-object
  source writes before capture, during transform and immediately before success;
  change-capture-restore ABA; and child termination at private-create,
  snapshot-copy, sealed-verification and post-rename-reopen boundaries.
- Final security verdict: PASS; no remaining HIGH or MEDIUM finding. Accepted
  low residual: the Windows DACL test verifies a protected single ACE while
  source inspection shows that ACE is built from the current process `TokenUser`.

## Generated artefacts

| Artefact | Rebuild command | Digest/result |
| --- | --- | --- |
| `capsules/diagram-studio.capsule.sqlite` | `python tools/build_example.py` | 888,832 bytes; SHA-256 `9526a68ed021b34b8fadfef27c07b4d2707de1a7abab0587942c65a7b2a0abf7`; 18 checks pass |
| Creator-plugin Inspector capsule | plugin freshness/build tests | 1,785,856 bytes; SHA-256 `fafa0b162ccf04e9a85788bfcee5fa878d37fe4b18df568f8bb09e0bcc1b8dd8` |
| Three HTML exports | `python tools/build_exports.py` | SHA-256 `4ceb3d8d…`, `7d555668…`, `a9480d44…`; check passes |
| Native SBOM/licenses | generator commands | SHA-256 `2df848ff…` and `fac31463…`; checks pass |
| `capsules/sqlite-capsule-host-setup.exe` | `python native/tools/build_installers.py --bundles nsis` | 6,444,772 bytes; SHA-256 `997292cf93e22eb2275ca8bf77247f318fced9efa4dac0d7d0f61a972f73edbb`; development-unsigned |

## Remaining limitations

- Only exact duplicate is executable in the workspace foundation. Compact
  duplicate, fork and template creation remain fail-closed until M04.
- No Cabinet/Overview Tauri command or renderer surface exists yet; M03 owns the
  host-only view model, recent cache, icons and trusted-shell UX.
- V0.2-to-v0.3 conversion remains unavailable pending the M08 signed legacy
  adapter/migration design.
- The local NSIS artifact is intentionally development-unsigned; production
  signing remains a release-pipeline responsibility.

## Handoff

**Next milestone:** M03 — Capsule Overview and Cabinet trusted shell UX  
**First action:** Add a host-owned `CapsuleOverviewViewModel` in the trusted
Tauri crate, populated only from bounded verified metadata. Keep it absent from
the raw Wry command inventory and prove the overview can render before any
capsule application asset is released.  
**Relevant files:** M03 `EXECPLAN.md`; `native/desktop/src-tauri`; trusted shell
`desktop/ui`; `capsule-core` overview identities; `capsule-workspace` verified
v0.3 reports; current raw-window/Tauri capability tests.  
**Known hazards:** publisher verification, publisher trust and mutable instance
profile must remain visually distinct; v0.2 gets an explicit legacy fallback;
cached Cabinet metadata is never execution authority; PNG/WebP icons require
hash/compressed/decoded bounds outside the DOM; no capsule HTML/Markdown/CSS may
enter the trusted shell.  
**Do not repeat:** reuse the exact verified snapshot/overview projection and the
frozen lifecycle error catalogue. Do not reopen a live path for mixed evidence,
deserialize plan JSON as authority, or expose workspace/filesystem commands to
the raw renderer.

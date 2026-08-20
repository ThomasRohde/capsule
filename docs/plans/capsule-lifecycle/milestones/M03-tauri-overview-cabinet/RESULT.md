# Result — M03: Capsule Overview and Cabinet trusted shell UX

**State:** complete  
**Started:** 2026-08-12T20:11:12Z  
**Completed:** 2026-08-12T22:44:45Z  
**Repository commit at start:** `e73cf948fba233ef84d4680930b61549012020a7`  
**Repository commit at completion:** `e73cf948fba233ef84d4680930b61549012020a7`

## Outcome

The native host now starts as a trusted Capsule Cabinet and renders a bounded,
host-owned Overview before releasing any application asset. Application
metadata, cryptographic signature state, host-local publisher trust, mutable
instance identity and file state are deliberately separate. The Overview is
derived from one retained, pinned, verified read-only snapshot; remembered
authorization stops at `remembered-ready` until an explicit trusted-shell Open.

The Cabinet stores only bounded, owner-private, non-authoritative display hints
behind opaque recent IDs and freshly reinspects every selected path. Safe icons
are hash/type/magic/dimension checked and re-encoded as static metadata-free PNG
data. No Overview, Cabinet, filesystem or lifecycle command was added to the raw
Wry renderer. Independent and security critics report no remaining M03 blocker.

## Scope delivered

- Added `CapsuleOverviewViewModel` profile
  `org.sqlite-capsule.tauri-overview/1` with explicit v0.2 legacy fallback and
  bounded v0.3 application, instance, schema, signature, trust and action state.
- Retained the exact verified launch snapshot for Overview, signature evidence
  and safe-image projection instead of reopening a live path.
- Added the owner-protected `cabinet-v1` recent cache, opaque recent handles,
  corruption/size/crash handling, passive listing and Windows reparse/junction
  rejection.
- Added bounded PNG/WebP inspection and static PNG re-encoding with invalid-
  media fallback; raw capsule asset identifiers never become renderer lookup
  authority.
- Split inspection from activation. Remembered grants, picker/drop/recent opens
  and explicit rollback recovery show Overview with the raw renderer locked.
- Bound capability and recovery actions to the current opaque selection and map
  stale UI actions to stable `stale_plan` failures.
- Reorganised the trusted shell into Cabinet, Overview, Lineage, Compare,
  Versions, Security, Recovery and Settings while retaining prior trust,
  signing, update, backup, restore and support actions.
- Added semantic-heading, keyboard/focus, reduced-motion/forced-color, real 200%
  WebView2 scaling, visual identity-state and raw-renderer negative coverage.
- Synchronized framework/native/security/authoring documentation and the
  standalone creator plugin; rebuilt generated example/exports, SBOM/licenses
  and the current NSIS-only installer.

## Changed paths

- `native/desktop/src-tauri/src/{overview,cabinet,safe_image}.rs` and `lib.rs` —
  bounded models, exact-snapshot projection, cache, icon and selection state.
- `native/crates/capsule-launch/` — retained verified inspection snapshot and
  bounded projection callback.
- `native/desktop/ui/{index.html,app.js,styles.css}` — Cabinet/Overview shell,
  eight-destination navigation and explicit-open state machine.
- `native/desktop/src-tauri/{permissions,capabilities,gen}/` — main-only command
  inventory; no raw Wry IPC surface.
- `tests/native/`, `tests/test_native_ui.py` — Overview matrix, scaling,
  accessibility, raw isolation, recovery, crash and production-picker evidence.
- `plugins/capsule-creator/`, repository docs and generated artefacts — reviewed
  and synchronized for the material host/security/UI change.

## Decisions and deviations

| Decision/deviation | Rationale | ADR or source |
| --- | --- | --- |
| Structurally inspected metadata is not called publisher-verified | Exhaustive structure/check validation does not authenticate a signature or establish host trust | ADR 0026; security review |
| Remembered authorization requires a new explicit Open after restart | Overview must be observable before any raw asset release; stored policy remains visible without silently activating code | M03 EXECPLAN; ADR 0016/0026 |
| Recovery-required inspection receives an opaque selection before recovery | The trusted action must be bound to the reviewed dirty source without exposing its path or accepting a stale click | Security review |
| Cabinet listings do not stat cached target paths | Passive listing must not probe stale UNC/network locations; host resolves a recent ID and performs fresh inspection only on explicit open | ADR 0026 |
| Safe images are re-encoded data URLs in the trusted shell | Only bounded host-owned PNG bytes enter the DOM; no general asset URL/token is usable by the raw origin | ADR 0026 |
| Copy/fork/template cards remain disabled | M04 owns their planner, executor and publication semantics | M03 explicit non-scope |

## Acceptance evidence

| Acceptance clause | Status | Evidence |
| --- | --- | --- |
| Overview appears without executing/releasing application assets | pass | Six-state Overview E2E; `assets_released=false`, raw `/__host/locked`, remembered-ready explicit-open test |
| Publisher identity and mutable profile cannot be confused | pass | Signed/unsigned/invalid visual matrix and separate application/signature/trust/instance view-model groups |
| Security and recovery controls remain reachable/tested | pass | Eight-destination navigation assertions; trusted-shell and raw recovery/crash suites |
| Cabinet cache is never authority | pass | Opaque recent JSON, passive load, fresh reinspect, corrupt/future/oversize/missing and junction tests |
| Raw renderer isolation passes | pass | Final raw E2E plus static main-only capability/no-IPC inventory |
| Inputs remain read-only and unchanged | pass | Overview matrix hashes every v0.2/v0.3 source before/after; explicit recovery is a separate user action |
| Outputs are create-new/no-replace | pass | Existing restore/signing/publication regressions pass; M03 introduces no transform |
| Generic/application boundary preserved | pass | Host models contain no Diagram Studio data concepts; app-specific fixture stays under example/tests |
| V0.2 compatibility preserved | pass | Legacy Overview fallback plus full Python/Rust suites and v0.2 signed vectors |
| Stable redacted failures | pass | Workspace error envelopes; stale selection tests; raw reports omit entry asset, permissions and capsule asset IDs |
| Docs/contracts/plugin/generated artefacts synchronized | pass | Full Python/plugin suite, strict contracts, example/export/SBOM/license freshness and NSIS rebuild |
| Critic and security findings resolved | pass | `evidence/M03/critic-report.md`; `evidence/M03/security-review.md` |

## Tests and validation

Environment: Windows 11 x86-64; CPython 3.13.7 for repository commands;
CPython 3.12.4 plus `jsonschema` for strict Draft 2020-12 validation;
Rust/Cargo 1.97.1 MSVC; native Tauri/WebView2 desktop process.

| Command | Result | Evidence path |
| --- | --- | --- |
| `python -m unittest discover -s tests -v` | pass, 169 tests in 163.051 s | `evidence/M03/20260812T223941Z-gate-final-python-post-review/` |
| `cargo test --offline --workspace --all-targets` | pass, 210 tests, 0 failed/ignored | `evidence/M03/20260812T223941Z-gate-final-rust-post-review/` |
| `cargo fmt --all -- --check` | pass | `evidence/M03/20260812T224250Z-gate-final-rust-fmt-post-review/` |
| `cargo check --offline --workspace --all-targets` | pass | `evidence/M03/20260812T224249Z-gate-final-rust-check-post-review/` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | pass | `evidence/M03/20260812T224250Z-gate-final-rust-clippy-post-review/` |
| Strict lifecycle spec validator | pass, six examples and 110 checks/records | `evidence/M03/20260812T224318Z-gate-final-lifecycle-specs/` |
| Signed application vectors `--all` | pass, v0.2/v0.3 | `evidence/M03/20260812T224318Z-gate-final-signed-vectors/` |
| Lifecycle-plan vectors | pass, exact canonical bytes/digests | `evidence/M03/20260812T224319Z-gate-final-plan-vectors/` |
| Native Cabinet/Overview matrix | pass, six states; actual 200% viewport bounds | `evidence/M03/20260812T224735Z-gate-native-overview-visual-viewport-final/` |
| Native raw renderer/recovery/crash/picker | pass | `evidence/M03/20260812T223833Z-gate-native-raw-final/` |
| Trusted-shell WebDriver suite | pass, six specs | `evidence/M03/20260812T224524Z-gate-native-trusted-shell-final/` |
| Example freshness and capsule verify | pass; 18 declared checks | `evidence/M03/20260812T224320Z-gate-final-example-check/`; `evidence/M03/20260812T224345Z-gate-final-capsule-verify/` |
| SBOM/license freshness | pass | `evidence/M03/20260812T224319Z-gate-final-sbom/`; `evidence/M03/20260812T224318Z-gate-final-licenses/` |
| RustSec exact exception gate | pass; no vulnerabilities, 17 exact reviewed warnings | `evidence/M03/20260812T224345Z-gate-final-rustsec/` |
| NSIS-only rebuild/export | pass, development-unsigned | `evidence/M03/20260812T224330Z-gate-final-nsis-post-review/` |

Two superseded native evidence failures are retained honestly. The first raw
evidence attempt hit a transient WebView2 resource lock. Early Overview scaling
attempts exposed an unsupported DPR emulation and then an overly broad block-box
heading assertion; the final gate measures the actual 200% `VisualViewport` and
visible text/action bounds. All named final reruns pass.

## Security and critic review

- Reviewers: builder `m00_builder`, independent critic
  `m00_independent_critic`, security critic `m00_security_critic`.
- Resolved findings include same-snapshot evidence, unsigned/invalid-signature
  display states, remembered-grant auto-activation, icon decode bounds, opaque
  recents, stale selection races, explicit recovery binding and Windows cache
  junction traversal.
- Final independent verdict: PASS; no remaining substantive M03 blocker.
- Final security verdict: PASS; no remaining HIGH or MEDIUM M03 finding.
- Accepted low residual: Cabinet reparse checks are path-based rather than a
  held-handle component walk. Cache data remains owner-private and
  non-authoritative, and the real pre-existing junction case fails closed
  without touching its target.

## Generated artefacts

| Artefact | Rebuild command | Digest/result |
| --- | --- | --- |
| `capsules/diagram-studio.capsule.sqlite` | `python tools/build_example.py` | 888,832 bytes; SHA-256 `addd01712879db46af594f5936043700ad938b0b9a4a26fc0dd2014c730a3be8`; 18 checks |
| Creator-plugin Inspector capsule | standalone plugin build/freshness tests | 1,785,856 bytes; SHA-256 `fafa0b162ccf04e9a85788bfcee5fa878d37fe4b18df568f8bb09e0bcc1b8dd8` |
| Three HTML exports | `python tools/build_exports.py` | SHA-256 `72c859b7…`, `55dd9807…`, `a2680527…`; freshness passes |
| Native SBOM/licenses | generator `--check` commands | SHA-256 `a459d76c…` and `1fe2d407…`; freshness passes |
| `capsules/sqlite-capsule-host-setup.exe` | `python native/tools/build_installers.py --bundles nsis` | 6,822,950 bytes; SHA-256 `273dbe88f7afe6bc33f0f27d734d3b32a591b8ad38e3c0196bd3f91399913687`; development-unsigned |

## Remaining limitations

- M03 exposes no copy/fork/template executor. The action cards remain disabled
  until M04 proves plans, dataset decisions and create-new publication.
- Cabinet cache reparse checks do not yet use the held-parent component-walk
  capability used for authoritative lifecycle publication; this is acceptable
  only while the cache remains non-authoritative.
- Formal screen-reader certification and non-Windows native host support remain
  outside this milestone. Automated semantics, focus, forced-colour,
  reduced-motion and 200% scaling gates are retained.
- The local NSIS artifact is intentionally development-unsigned; production
  signing remains a release-pipeline responsibility.

## Handoff

**Next milestone:** M04 — Duplicate, compact duplicate, fork and template creation  
**First action:** Add a dry-run `prepare_copy` planner in
`native/crates/capsule-workspace` that accepts only verified/pinned sources and
returns operation identity effects, signed-dataset decisions/dependencies,
sensitivity prompts, bounded row estimates, expected application digest and
create-new output constraints. Keep execution limited to the already-proven
exact duplicate until the plan truth table and hostile policy tests pass.  
**Relevant files:** M04 `EXECPLAN.md`; program `07-COPY-FORK-LINEAGE.md` and
`15-TAURI-COMMAND-CONTRACT.md`; `capsule-workspace/{planner,plan,publication}`;
`capsule-lifecycle`; trusted Cabinet action cards; creator-plugin v0.3 fixtures.  
**Known hazards:** v0.2 supports only duplicate/compact duplicate; fork/template
must preserve the signed application compartment and app digest while creating
new instance/revision identity; “without data” must follow the signed data
contract, never names; sensitive datasets default to omit; every output is
create-new and post-verified.  
**Do not repeat:** do not deserialize a plan as authority, reopen live input
paths for mixed evidence, mutate fixtures, infer dataset semantics, enable a UI
operation before native planner/executor proof, or register any copy command on
the raw Wry renderer.

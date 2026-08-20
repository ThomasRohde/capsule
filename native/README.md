# SQLite Capsule native host

This tree is an independent Rust implementation of the generic SQLite Capsule
contract. It complements the Python loopback host and browser-only SQLite WASM
host. No crate in this tree contains Diagram Studio tables, endpoint names, or
rendering logic.

## Current implementation

The native host currently provides:

- bounded read-only inspection of untrusted v0.2 capsules;
- signed application verification and protected host-local trust, grant,
  denial, and revocation decisions;
- a generic verified SQLite runtime exposing only declared named endpoints;
- a trusted Tauri shell and a separate raw Wry renderer with no Tauri bootstrap
  or registered native IPC;
- a non-mutating Capsule Cabinet/Overview that keeps application release,
  signature/publisher trust, mutable instance profile, and file state separate;
- a bounded host-owned PNG/WebP derivative pipeline and a passive,
  owner-protected, rebuildable recent-capsule cache;
- a session-bound, sequenced custom-protocol bridge;
- pinned source identity, one-writer coordination, external-conflict detection,
  owner-private/no-replace publication, verified pre-write backups, bounded
  checkpoints, and new-path restore;
- a product-independent lifecycle workspace that validates signed v0.3 data
  contracts, reports redacted mutable lineage, prepares digest-bound plans and
  publishes only exhaustively verified create-new outputs;
- one coherent create-new copy engine: byte-exact and logical compact duplicate
  for v0.2/v0.3, plus signed-v0.3 fork, authenticated template creation and
  policy-controlled selective fork with dependency/FK closure, lineage,
  sensitive-state scrubbing and post-publication verification;
- a deterministic, read-only signed-v0.3 compare engine with compatibility,
  identity/lineage, application, schema and signed-policy dataset summaries,
  plus opaque bounded detail cursors and explicit sensitive disclosure;
- two-way explicit and verified-ancestor three-way reconciliation through a
  value-free review, closed conflict resolutions, transactional target-copy
  transformation, exact validation and create-new publication;
- host-owned open, restore, support-export, trust, and capability controls;
- host-owned use-once publisher signing to a verified new capsule copy;
- offline host-release, Authenticode, and Sigstore policy verification; and
- durable update staging and rollback state without exposing installer control
  to capsule content.

The application window remains hidden until the trusted shell explicitly opens
the reviewed capsule. This remains true for an exact remembered authorization:
Overview appears first with assets unreleased and the bridge inactive.
Rejection, replacement, conflict, or trust reset closes the runtime session and
returns executable assets to the locked state.

The complete requirements are in
[`../docs/native-host-contract.md`](../docs/native-host-contract.md). Dependency
policy is in [`DEPENDENCIES.md`](DEPENDENCIES.md).

The administrative `capsule-native template-state <capsule>` command verifies
the reserved signed template-state document against every declared dataset on
the same pinned private snapshot. It returns the versioned
`org.sqlite-capsule.workspace-template-state-response/1` projection and never
turns an unsigned document or application signature alone into template
authority. The canonical proof schema and independent vectors live under
`docs/plans/capsule-lifecycle/contracts/template-state-v1.schema.json` and
`compatibility/template-state-v1/`.

The administrative `capsule-workspace` CLI uses the same non-serializable
review, retained source and held destination typestates as the trusted shell.
It exposes `copy-exact`, `copy-compact`, `copy-fork`, `copy-template` and
`copy-selective`; success is the bounded
`org.sqlite-capsule.workspace-copy-result/1` projection and failures use the
frozen lifecycle error envelope. Lifecycle plans and compact/dataset-state
digests have independent Python/Rust vectors under `compatibility/`.

The same CLI also exposes bounded reconciliation without turning serialized
JSON into authority:

```text
capsule-workspace reconcile-candidates <source> <target> <dataset-index> <table-index> [sensitive-confirmed]
capsule-workspace plan-reconcile <source> <target> <new-output> <normal|sensitive-confirmed> <selection>...
capsule-workspace reconcile <source> <target> <new-output> <normal|sensitive-confirmed> <selection>...
capsule-workspace three-way-reconcile-candidates <ancestor> <source> <target> <normal|sensitive-confirmed>
capsule-workspace plan-reconcile-three-way <ancestor> <source> <target> <new-output> <normal|sensitive-confirmed> <conflict-id>:<keep-target|take-source>...
capsule-workspace reconcile-three-way <ancestor> <source> <target> <new-output> <normal|sensitive-confirmed> <conflict-id>:<keep-target|take-source>...
```

Each selection is value-free and has the closed form
`dataset:table:key-digest:source-row-digest-or--:target-row-digest-or--:action:fields-or--`.
Actions are `insert`, `delete`, `replace` or `fields`; field positions are a
strictly increasing comma-separated list. Candidate and plan responses are
versioned review projections containing digests, never row values. The
planning command exits after dropping its retained capability, so its JSON
cannot later be executed. `reconcile` freshly opens and pins both inputs,
recomputes comparison, mints the plan IDs/time and create-new reservation,
then approves its own exact canonical plan/payload bytes and consumes the
non-serializable review through the same prepare/stage/validate/publish
typestate. It refuses an existing destination and emits only the output leaf,
bounded identities/digests and verification flags on success.

Three-way classification independently pins and verifies the ancestor, source
and target, rejects an ancestor with a different signed application, schema or
data contract, and reports value-free clean-change counts plus the closed
insert/insert, update/update, delete/update and immutable-field conflict set.
Every reported conflict must be resolved exactly once. `take-source` is
available only when the classifier supplied that closed choice; immutable-field
conflicts permit `keep-target` only. The plan command drops the retained
authority after emitting review JSON. Only `reconcile-three-way` resolves and
executes within one process, preserving all three input byte streams and the
target application compartment while publishing a new target-derived copy.

The current reconcile executor supports only acyclic foreign-key graphs whose
update and delete actions are `NO ACTION` or `RESTRICT`. Restrictive acyclic
relationships are supported both within a dataset and across datasets (with a
matching signed dependency for cross-dataset edges). Writes run parent-first,
deletes child-first, and the private transaction must pass a final foreign-key
check before publication. Cascades, `SET NULL`, `SET DEFAULT`, self-references
and cycles fail with `unsupported_operation`.

Both `capsule-native compare <left> <right>` and `capsule-workspace compare
<left> <right>` emit the direct versioned `org.sqlite-capsule.compare-report/1`
summary. They retain both sources read-only under one deadline, emit no row
values, and use the same canonical compare frames frozen under
`compatibility/compare-row-v1/`. Paginated values are available only through
opaque trusted-shell sessions. A separate explicit application expansion emits
the fixed `org.sqlite-capsule.compare-application/1` projection: thirteen
host-owned families containing counts and digests only, bound to the original
comparison report and source file digests.

## Workspace boundaries

| Path | Responsibility |
| --- | --- |
| `crates/capsule-core` | Untrusted metadata inspection and child request grammar |
| `crates/capsule-crypto` | Signed application canonicalisation and verification |
| `crates/capsule-signing` | Use-once key import plus safe prepare/sign/verify/publish workflow |
| `crates/capsule-launch` | Launch decisions and capability evaluation |
| `crates/capsule-policy` | Protected local trust, grants, audit, and revocation |
| `crates/capsule-runtime` | Capsule verification and named endpoint execution |
| `crates/capsule-lifecycle` | Source identity, writer lease, held-parent private staging and no-replace publication |
| `crates/capsule-workspace` | Signed data contracts, redacted lineage, deterministic plans, bounded comparison, reconciliation and verified lifecycle publication |
| `crates/capsule-distribution` | Offline host-release and revocation policy |
| `crates/capsule-platform` | Platform signer identity and package verification |
| `crates/capsule-sigstore` | Bounded offline Sigstore verification |
| `crates/capsule-update` | Verified staging and startup-health/rollback state |
| `crates/capsule-installer` | Narrow host-only Windows installer handoff |
| `desktop/src-tauri` | Trusted Cabinet/Overview plus opaque create-copy, compare and reconcile controllers, safe-image projection, and raw application-window lifecycle |
| `desktop/ui` | Cabinet, Overview, five-mode create-copy review, bounded comparison/reconciliation, lifecycle entry points, security, recovery, settings, signing, update, and support UI |

## Build and run

The exact Rust toolchain and dependency graph are pinned. From `native/`:

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
python tools/generate_sbom.py --check
python tools/generate_license_report.py --check
cargo run -p sqlite-capsule-desktop -- ../capsules/diagram-studio.capsule.sqlite
```

Use a disposable copy for write tests. The checked example is a generated
release artefact and a clean rebuild will replace it.

## Windows WebView2 acceptance

From the repository root:

```text
npm ci
npm run test:native:prepare
npm run test:native:prepare:check
npm run test:native
npm run test:native:window
npm run test:native:raw
npm run test:native:overview
```

These gates exercise the trusted Cabinet/Overview and identity-state matrix,
keyboard order, remembered-release Overview-before-assets behavior, decision
reversal, raw renderer isolation, named reads and writes, persistence, backup,
restore, conflict closure, source pinning, safe image handling, support export,
and selected abrupt process-termination recovery paths against isolated state
under `.tmp/`.

## Security advisory and legal inventories

Run `python tools/check_rustsec.py` with the pinned `cargo-audit` version. The
gate rejects vulnerabilities and warnings that do not match an exact,
unexpired entry in `rustsec-exceptions.json`.

Regenerate and review the checked dependency artefacts after dependency changes:

```text
python tools/generate_license_report.py
python tools/generate_sbom.py
```

`THIRD_PARTY_LICENSES.md` and `sbom.cdx.json` are required packaging inputs,
not substitutes for release-specific legal review or signed provenance.

## Package the Windows host

Pull requests and main-branch pushes run source and test gates without building
installers. `.github/workflows/release.yml` builds and qualifies the Windows
x86-64 NSIS bundle only for a manual dry run or a `vMAJOR.MINOR.PATCH` tag.
Manual runs retain workflow artifacts; tag runs also publish GitHub Release
assets after the repository and binary versions match.

For local iteration, use the repository wrapper from the repository root. It
pins the Tauri CLI, removes only prior generated installer candidates, builds
NSIS with Cargo's debug profile, and exports its stable ignored path:

```text
python native/tools/build_installers.py
python native/tools/build_installers.py --bundle-only
```

The second command packages the existing debug executable without invoking a
Rust compilation. Windows debug and release executables both use the GUI
subsystem, so neither installer opens a companion console window. Bundle-only
mode fails closed if that executable does not exist, but does not establish
source freshness; use it only after a successful matching build.
Use `python native/tools/build_installers.py --release` only for a fully
optimized full-LTO build. The GitHub release workflow passes `--release`
explicitly; `--release --bundle-only` can repackage an already-built release
executable.

The exporter writes `../capsules/sqlite-capsule-host-setup.exe`. MSI packaging
is opt-in with `python native/tools/build_installers.py --bundles msi` and is
not part of the default release workflow. Generated installers are deliberately
not committed. An exact matching global `cargo-tauri` is reused;
otherwise the first run installs the pinned CLI into ignored `native/.tools`
and therefore requires Cargo registry access.

Before creating a tag, verify every package version with:

```text
python native/tools/check_release_version.py --tag v0.3.0
```

## Current limits

- Native builds and automated UI acceptance currently target Windows x86-64.
  macOS and Linux hosts are not supported here.
- Local installers are unsigned development artefacts. The repository does not
  provide production signing identities, release roots, or update endpoints.
- Installer execution and rollback commands remain unavailable to both
  WebViews. Staging verified bytes does not install them.
- Clean-machine signed install/update/rollback evidence, Explorer double-click
  and cross-window drag/drop acceptance, formal accessibility certification,
  and human screen-reader review are not claimed.
- Existing `.capsule.sqlite` files open through the command line, picker,
  drag/drop, or **Open with**. The registered desktop suffix is
  `.sqlitecapsule`; neither suffix is treated as trustworthy.

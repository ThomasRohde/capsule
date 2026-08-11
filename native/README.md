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
- a session-bound, sequenced custom-protocol bridge;
- pinned source identity, one-writer coordination, external-conflict detection,
  verified pre-write backups, bounded checkpoints, and new-path restore;
- host-owned open, restore, support-export, trust, and capability controls;
- offline host-release, Authenticode, and Sigstore policy verification; and
- durable update staging and rollback state without exposing installer control
  to capsule content.

The application window remains hidden until the trusted shell authorises the
current capsule. Rejection, replacement, conflict, or trust reset closes the
runtime session and returns executable assets to the locked state.

The complete requirements are in
[`../docs/native-host-contract.md`](../docs/native-host-contract.md). Dependency
policy is in [`DEPENDENCIES.md`](DEPENDENCIES.md).

## Workspace boundaries

| Path | Responsibility |
| --- | --- |
| `crates/capsule-core` | Untrusted metadata inspection and child request grammar |
| `crates/capsule-crypto` | Signed application canonicalisation and verification |
| `crates/capsule-launch` | Launch decisions and capability evaluation |
| `crates/capsule-policy` | Protected local trust, grants, audit, and revocation |
| `crates/capsule-runtime` | Capsule verification and named endpoint execution |
| `crates/capsule-lifecycle` | Source identity, writer lease, backup, conflict, and restore |
| `crates/capsule-distribution` | Offline host-release and revocation policy |
| `crates/capsule-platform` | Platform signer identity and package verification |
| `crates/capsule-sigstore` | Bounded offline Sigstore verification |
| `crates/capsule-update` | Verified staging and startup-health/rollback state |
| `crates/capsule-installer` | Narrow host-only Windows installer handoff |
| `desktop/src-tauri` | Trusted shell and raw application-window lifecycle |
| `desktop/ui` | Host identity, trust, recovery, update, and support UI |

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
```

These gates exercise the trusted first-open UI, keyboard order, decision
reversal, raw renderer isolation, named reads and writes, persistence, backup,
restore, conflict closure, source pinning, support export, and selected abrupt
process-termination recovery paths against isolated state under `.tmp/`.

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

For a local release build, use the repository wrapper from the repository root.
It pins the Tauri CLI, removes only prior generated installer candidates,
builds NSIS, and exports its stable ignored path:

```text
python native/tools/build_installers.py
```

The exporter writes `../capsules/sqlite-capsule-host-setup.exe`. MSI packaging
is opt-in with `python native/tools/build_installers.py --bundles msi` and is
not part of the default release workflow. Generated installers are deliberately
not committed. An exact matching global `cargo-tauri` is reused;
otherwise the first run installs the pinned CLI into ignored `native/.tools`
and therefore requires Cargo registry access.

Before creating a tag, verify every package version with:

```text
python native/tools/check_release_version.py --tag v0.2.0
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

# Native dependency policy

The native host is an independent implementation of the generic capsule
contract. Its bootstrap and release dependency choices do not change the Python
standard-library rule in the repository root.

## Pinned direct inputs

| Input | Exact version | License | Purpose |
| --- | --- | --- | --- |
| Rust toolchain | 1.97.1 | Apache-2.0 OR MIT | compiler, Cargo, rustfmt, Clippy |
| Tauri | 2.11.5 | Apache-2.0 OR MIT | trusted shell and desktop lifecycle; pinned `unstable` API exposes a native window without a bundled WebView |
| tauri-build | 2.6.3 | Apache-2.0 OR MIT | compile-time desktop configuration |
| tauri-plugin-dialog | 2.7.2 | Apache-2.0 OR MIT | host-owned open and new-path restore pickers; no capsule API |
| tauri-plugin-single-instance | 2.4.3 | Apache-2.0 OR MIT | secondary launch forwarding and existing-window focus |
| tauri-plugin-updater | 2.10.1 | Apache-2.0 OR MIT | Rust-only signed update metadata and installer transport; no guest plugin permission |
| Wry | 0.55.1 | Apache-2.0 OR MIT | raw, non-Tauri application WebView |
| rusqlite | 0.40.1 | MIT | native SQLite API |
| libsqlite3-sys | 0.38.1 | MIT | bundled SQLite build used by rusqlite |
| serde | 1.0.229 | MIT OR Apache-2.0 | typed host responses |
| serde_json | 1.0.151 | MIT OR Apache-2.0 | bounded manifest decoding |
| serde_json_canonicalizer | 0.3.2 | MIT | RFC 8785 JSON canonicalisation for signed fields |
| ed25519-dalek | 3.0.0 | BSD-3-Clause | Ed25519 signing and verification |
| pkcs8 | 0.11.0 | Apache-2.0 OR MIT | standards-based Ed25519 private-key import for the host-owned use-once signer |
| sha2 | 0.11.0 | MIT OR Apache-2.0 | application digests and public-key IDs |
| image | 0.25.5 | MIT OR Apache-2.0 | bounded PNG/WebP Overview decode and metadata-free static PNG re-encoding |
| tempfile | 3.27.0 | MIT OR Apache-2.0 | private same-directory signing copies and no-clobber publication |
| base64 | 0.22.1 | MIT OR Apache-2.0 | bounded decoding of Tauri-compatible Minisign key and signature documents |
| minisign-verify | 0.2.5 | MIT | verify bounded downloaded host packages under the pinned updater key |
| reqwest | 0.13.4 | MIT OR Apache-2.0 | host-only bounded same-origin package and Sigstore-evidence download |
| sigstore-verify | 0.11.0 | Apache-2.0 | offline artifact, Fulcio, SCT, Rekor, identity, and issuer verification under an embedded production trust root |
| getrandom | 0.4.3 | MIT OR Apache-2.0 | per-launch 256-bit native protocol sessions from the OS RNG |
| libc | 0.2.189 | MIT OR Apache-2.0 | POSIX crash-releasing writer lease via non-blocking `flock` |
| thiserror | 2.0.20 | MIT OR Apache-2.0 | explicit core errors |
| windows-sys | 0.61.2 | MIT OR Apache-2.0 | current-token owner lookup, protected trust-store DACLs, offline Authenticode/certificate identity verification, MSI/PE package-version metadata, system-directory lookup, and installer handoff on Windows |

The SQLite amalgamation bundled by `libsqlite3-sys` is public-domain SQLite
upstream code. `Cargo.lock` is authoritative for transitive versions. The
desktop host does not enable dynamic SQLite extension loading.

## Update procedure

1. Read the upstream release and security notes for the proposed version.
2. Change one exact version in `Cargo.toml` or `rust-toolchain.toml` at a time.
3. Regenerate `Cargo.lock`, inspect every transitive change, and update this
   inventory when a direct input or bundled SQLite version changes.
4. Run `cargo fmt --all -- --check`, `cargo check --workspace --all-targets
   --all-features`, `cargo test --workspace --all-features`, and `cargo clippy
   --workspace --all-targets --all-features -- -D warnings`.
5. Run `python tools/check_rustsec.py` against the checked-in `Cargo.lock`. The
   gate pins `cargo-audit`, fetches the current advisory database, rejects every
   vulnerability, requires an exact package/version/kind match for every
   warning in `rustsec-exceptions.json`, fails on stale or expired exceptions,
   and finally reruns `cargo audit --deny warnings` while ignoring only those
   exact reviewed IDs. Record the audit-tool version and advisory database
   revision in release evidence. Any unreviewed finding is a release blocker.
6. Repeat native shell, child-isolation, capsule parity, installer, and update
   tests on Windows, macOS, and Linux before accepting the upgrade.

Crate source and license metadata are taken from the locked registry packages.
`python tools/generate_license_report.py` regenerates the deterministic
[`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md) inventory and
`python tools/generate_license_report.py --check` fails if it no longer matches
the complete `cargo metadata --locked --all-features` graph or if a registry
package omits both a license expression and a declared license file. Run and
review that check before every signed distribution.

The 2026-08-09 report contains 576 third-party package records and 35 distinct
license expressions/files, with no missing-license fallback. Its reciprocal
entries are MPL-2.0 (`cssparser`, `cssparser-macros`, `dtoa-short`,
`option-ext`, and `selectors`); preserve the corresponding source and notices
when distributing modified versions. `r-efi` offers MIT or Apache-2.0 as
alternatives to LGPL-2.1-or-later. The inventory is an engineering
completeness gate, not a substitute for release-specific legal review or the
license texts/notices shipped with installers.

`python tools/generate_sbom.py` produces deterministic CycloneDX 1.5
`sbom.cdx.json` for all 589 first- and third-party package records plus the 590
workspace/package dependency nodes. Registry components carry their locked
crate archive SHA-256 when Cargo provides it. `--check` is a packaging gate;
platform release provenance and artifact attestations remain separate signed
release evidence.

## Current RustSec audit record

`cargo-audit` 0.22.2 scanned the 589-package `Cargo.lock` against advisory
database revision `1237bbe09d2701e14e6593a630fbaf28928df712`. No vulnerability
entry was reported. Raw `cargo audit --deny warnings` still reports 17 warnings:

- ten GTK3 binding packages and `proc-macro-error` are unmaintained; the GTK3
  and `glib` packages are absent from `cargo tree --target
  x86_64-pc-windows-msvc`, but remain Linux release concerns;
- `glib` 0.18.5 has the unsound `VariantStrIter` advisory
  `RUSTSEC-2024-0429`; and
- five `unic-*` 0.9 packages are unmaintained and are reachable on Windows
  through `urlpattern -> tauri-utils`.

`rustsec-exceptions.json` now records each exact advisory, package, version,
warning class, target reachability, API reachability, compensating control,
owner, removal condition, and a 2026-09-30 review deadline. The Linux GTK/glib
and procedural-macro branch is absent from the Windows and macOS target trees.
Repository and locked registry-source search found no caller of the unsound
`VariantStrIter` methods outside `glib` itself. The `unic-*` branch remains
reachable on all targets while current `tauri-utils` requires `urlpattern` 0.3,
but those advisories report maintenance status rather than a vulnerable API.

`python tools/check_rustsec.py` passes only when the live report matches all 17
records exactly and their declared Windows/macOS/Linux reachability matches
fresh locked `cargo tree` resolution, then reruns the warnings-denied audit while
ignoring only those IDs. A new vulnerability or warning,
package/version/kind/target drift, a disappeared finding, malformed record, or
deadline expiry fails closed. This is a reviewed, time-bounded release
exception, not a claim that the upstream findings are fixed.

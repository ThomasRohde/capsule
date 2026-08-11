# Contributing

SQLite Capsule keeps the reusable format and hosts independent from the Diagram
Studio example. Changes should preserve that boundary and leave reviewable
source, generated artefacts, documentation, and tests consistent.

## Source boundaries

- Product-independent intent lives in `docs/vision.md`.
- Generic architecture and format work belongs in `docs/architecture.md`,
  `docs/format-contract.md`, `format/`, and `runtime/`.
- Native host contracts and implementation belong in
  `docs/native-host-contract.md` and `native/`.
- Diagram Studio requirements and code belong in
  `docs/example-diagram-studio.md` and `examples/diagram-studio/`.
- `capsules/` and `exports/` are generated distribution artefacts. Do not
  hand-edit them for durable source changes.

## Safety and compatibility rules

- Inspect and verify a capsule before executing embedded assets.
- Keep the bootstrap Python runtime on the standard library.
- Keep browser applications free of runtime network dependencies.
- Expose named, parameterised endpoints only; never add a general SQL endpoint.
- Bind the loopback host to loopback and preserve the default-deny CSP.
- Treat hashes as integrity checks, not publisher authentication.
- Refuse implicit replacement when packing, exporting, or restoring.

## Core development gate

Python 3.11 or later is recommended.

```bash
python tools/build_example.py
python -m unittest discover -s tests -v
python tools/capsule.py verify capsules/diagram-studio.capsule.sqlite
python tools/build_example.py --check
python tools/build_exports.py --check
```

Rebuild the example after changing embedded assets, data, runbooks, format
documentation, or the compatible host. Review generated capsule and export
changes alongside their source changes.

## Browser tests

Install the exact dependencies from `package-lock.json`, then run the relevant
suite:

```bash
npm ci
npm run test:browser
npm run test:browser:html
```

Visual snapshots are acceptance evidence, not automatic truth. Review every
changed image and update it only for an intentional rendering change.

## Native host

The toolchain is pinned in `native/rust-toolchain.toml` and
`native/Cargo.lock`. From `native/` run:

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
python tools/generate_sbom.py --check
python tools/generate_license_report.py --check
```

The RustSec gate also requires the pinned `cargo-audit` version and advisory
database access. Every accepted warning must match an exact, unexpired record
in `native/rustsec-exceptions.json`.

Windows WebView2 and packaging commands are documented in
[`native/README.md`](native/README.md).

## GitHub workflows and releases

`.github/workflows/ci.yml` is the clean-checkout pull-request gate. It verifies
current generated distributions, the Python suite, Rust formatting, checking,
tests and linting, plus the SBOM and license inventories. It deliberately does
not build installers or run installer lifecycle acceptance.

`.github/workflows/release.yml` owns the expensive Windows qualification path.
A manual dispatch builds and retains unsigned installer artifacts without
publishing a release. Pushing a canonical `vMAJOR.MINOR.PATCH` tag additionally
requires every repository package version to match, then publishes the
qualified MSI, NSIS setup, checksums, evidence, SBOM, and license inventory as
GitHub Release assets. Verify a proposed tag locally before pushing it:

```text
python native/tools/check_release_version.py --tag v0.3.0
```

## Pull requests

Describe the contract or behavior changed, the trust boundary affected, and the
verification performed. Keep unrelated working-tree changes out of the patch.
If a platform or browser acceptance gap remains, state it directly instead of
inferring support from another engine or operating system.

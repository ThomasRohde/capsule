# Lifecycle release qualification

Run only after M09 implementation is otherwise complete.

1. Verify a clean working tree or document intended generated changes.
2. Rebuild deterministic capsule examples and HTML exports.
3. Run all Python tests and generated checks.
4. Run Rust fmt, check, test, clippy, RustSec, SBOM and licence checks.
5. Run signature/conformance/migration compatibility vectors.
6. Run browser and HTML-export matrices.
7. Run native preparation checks and trusted/raw/window UI suites on Windows.
8. Run lifecycle security gauntlet, crash matrix, performance limits and
   accessibility review.
9. Copy the creator plugin outside the repository and run its complete scaffold,
   build, verify and black-box smoke path.
10. Build installers from the exact tested source through the repository-owned
    wrapper. Do not use bundle-only as proof of source freshness.
11. Verify installer/output hashes and reopen all distributable fixtures from a
    clean temporary directory.
12. Produce one release qualification report with command, environment, result,
    duration, evidence path and explicit gaps.

Do not replace or distribute installer artefacts when matching-source build and
native acceptance evidence are absent.

# Package validation report

**Package:** `org.sqlite-capsule.lifecycle-codex-kit/1`  
**Prepared:** 12 August 2026  
**Repository baseline:** `f67da560fb4baaa13144cea220c9329df87ad534`

## Scope

This report covers the implementation kit itself. It does not claim that the
lifecycle features have already been implemented in the Capsule repository.
Those product changes and their repository-wide gates are the work assigned to
Codex by milestones M00–M09.

## Completed package checks

- All JSON files parse.
- All seven Python files compile without writing bytecode into the kit.
- The draft v0.3 base and signed-application SQL schemas compile together in an
  in-memory SQLite database and pass `PRAGMA quick_check`.
- Ten ordered milestone bundles are present; each contains an execution plan,
  Codex prompt, acceptance gate and result handoff.
- Programme status IDs, ordering, dependencies and result paths validate.
- Six Diagram Studio examples validate against their Draft 2020-12 JSON
  schemas when `jsonschema` is available.
- The overlay installer was tested in dry-run, first-install and idempotent
  re-install modes.
- The installer creates 106 programme files and refuses a conflicting existing
  file rather than overwriting it.
- Installer path escape, symlink and destination-conflict protections are
  present in the package installer.
- Python bytecode and `__pycache__` directories are excluded from installation
  and distribution.
- The installed programme validates independently from a disposable repository
  fixture.

## Packaging gates

The final distribution step additionally verifies:

1. every package file against `PACKAGE_MANIFEST.json`;
2. ZIP structural integrity;
3. validation from a clean extracted copy;
4. installer dry-run from that extracted copy;
5. absence of symlinks, bytecode and cache directories.

The ZIP SHA-256 is reported outside the archive because including it inside the
archive would be circular.

## Deliberate limitations

- Draft v0.3 contracts are implementation inputs, not already accepted Capsule
  standards. M00 must reconcile them with the live repository before code
  changes.
- Native Windows/Tauri, Rust workspace, browser and installer product gates
  cannot be run against this overlay alone. The milestone plans specify where
  and when Codex must run them in the actual repository.
- No production Capsule source file is included or modified by this package.

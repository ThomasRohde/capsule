# Capsule lifecycle implementation kit

This package is a non-invasive Codex programme overlay for the repository
`https://github.com/ThomasRohde/capsule`.

It specifies and sequences the implementation of:

1. a clean split between a publisher-signed application release and a
   user-owned capsule instance;
2. richer application and capsule metadata, including safe icons and cover
   information;
3. a Capsule Overview/Cabinet as the first trusted Tauri shell surface;
4. duplicate, compact duplicate, fork-with-data, template-based creation and
   selective fork;
5. bounded, execution-free comparison of two compatible capsules;
6. applying selected changes to a new copy of the target, never merging in
   place;
7. application release upgrade ("rebase") while retaining and migrating user
   data;
8. compatibility, security, plugin, documentation, test and release hardening.

The kit contains product requirements, target architecture, draft format and
JSON contracts, UX specifications, security invariants, ten executable
milestone plans, per-milestone prompts, review prompts, validation scripts and
handoff templates.

## Intended use

Extract this ZIP outside or beside a clean checkout of the Capsule repository,
then install the overlay:

```text
python scripts/install.py --repo <path-to-capsule>
```

The installer creates only new planning and skill paths. It refuses to overwrite
a different existing file. Run it first with `--check` to preview:

```text
python scripts/install.py --repo <path-to-capsule> --check
```

Open the Capsule repository in Codex and use the text in
`CODEX_LIFECYCLE_START.md` as a Goal or normal prompt.

Validate the extracted kit before installation when desired:

```text
python scripts/validate_package.py --require-manifest
```

`PACKAGE_INFO.json` identifies the baseline and entry points.
`PACKAGE_MANIFEST.json` contains the SHA-256 and size of every distributed file.
`VALIDATION_REPORT.md` records the checks performed on the kit itself.

## Operating model

The programme is intentionally milestone-gated. Codex must:

- inspect the live checkout rather than trust stale path assumptions;
- preserve the existing generic/application-example separation;
- keep the raw Wry renderer outside all lifecycle commands;
- operate on pinned, read-only inputs and create-new outputs;
- update the programme status and milestone result after each gate;
- use independent review and security-critic subagents when available;
- stop a milestone on a failed invariant rather than weakening the invariant;
- continue to the next milestone only after the current acceptance gate passes.

The package is detailed, but draft schema files are design inputs rather than a
licence to overwrite the repository format blindly. Milestone 0 reconciles the
proposal against the actual current tree and records any justified deviation as
an ADR before implementation begins.

## Baseline

The package was prepared against `main` as observed on 12 August 2026, latest
commit `f67da560fb4baaa13144cea220c9329df87ad534`. The repository is moving
quickly. The first Codex action is therefore to capture the actual checkout
commit and compare it with this baseline.

## Programme completion

The work is complete only when:

- v0.2 capsules remain safely inspectable and runnable under their current
  compatibility policy;
- v0.3 application signatures remain valid when instance metadata, lineage or
  domain data change;
- lifecycle operations never mutate an input file;
- all output files are verified before publication;
- the trusted Tauri shell exposes lifecycle operations, while the raw
  application renderer exposes none;
- Diagram Studio supplies a complete v0.3 reference application, data contract,
  metadata, icon, lineage and upgrade fixture;
- Python, Rust, browser, native, generated artefact and standalone
  `capsule-creator` plugin gates pass;
- installer rebuild requirements in the repository's `AGENTS.md` are met.

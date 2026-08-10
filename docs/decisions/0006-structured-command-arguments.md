# ADR 0006: Machine-executable commands use structured argument vectors

- Status: Accepted for the current format
- Date: 2026-08-08

## Context

Shell command strings are useful to humans but are not a portable machine contract. Quoting rules differ across PowerShell, `cmd.exe`, and POSIX shells, and interpolating capsule paths into Python source creates an injection and correctness risk.

## Decision

Add nullable `argv_json` to `capsule_command` and expose it through `START_HERE`. When present, it is the preferred machine form: a non-empty JSON array of argument strings with the same constrained placeholders as command templates. Agents substitute whole argument values and launch the process without a shell. `command_template` remains the human-readable display and fallback for genuinely composite development commands.

The database-only extraction command receives capsule and cache paths through `sys.argv`, verifies the embedded asset hash, and creates its target exclusively.

## Consequences

- Fresh agents do not need to reinterpret shell quoting to launch a capsule.
- Paths remain data rather than executable source text.
- Composite shell workflows may retain only a display template until they are split into atomic commands.
- The exact `START_HERE` projection and `capsule_command` columns are verification invariants.

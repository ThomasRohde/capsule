# Compatibility fixture — <fixture ID>

## Purpose

State the exact format/signature/data/lifecycle behaviour this fixture proves.

## Generation

- Source files:
- Deterministic builder:
- Required tool versions:
- Build command:
- Expected SHA-256:

## Identity

- Format/profile:
- Application ID/version/digest:
- Publisher key ID:
- Capsule/revision IDs:
- Data schema ID/version:

## Expected behaviour

| Implementation/operation | Expected result/code |
| --- | --- |
| Python inspect | … |
| Python verify | … |
| Rust inspect | … |
| Rust verify | … |
| Native Overview | … |
| Copy/fork/compare/upgrade | … |

## Mutation variants

List one isolated mutation per negative fixture and why it must fail or remain
valid.

## Regeneration policy

State whether the binary is checked in, generated in CI, or both. Generated
fixtures require a `--check` mode and must not contain machine-local timestamps,
paths or random identifiers unless fixed by fixture input.

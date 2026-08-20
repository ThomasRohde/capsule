# Capsule lifecycle programme

This directory is the durable implementation contract for extending SQLite
Capsule with first-class document/application lifecycle management.

## Outcome

The trusted native shell becomes the place where a user can understand and
manage capsules before any embedded application runs:

```text
inspect → understand → duplicate/fork → compare → reconcile → upgrade → open
```

The embedded application remains a separate, capability-constrained renderer.

## Milestones

| ID | Outcome |
| --- | --- |
| M00 | Reconcile the programme against the live tree and freeze decisions |
| M01 | Introduce v0.3 application/instance separation and rich metadata |
| M02 | Add signed data contracts, lineage and the generic workspace core |
| M03 | Make Capsule Overview/Cabinet the first trusted Tauri experience |
| M04 | Implement duplicate, compact duplicate, fork and template creation |
| M05 | Implement bounded, execution-free comparison |
| M06 | Apply selected changes to a new target-derived copy |
| M07 | Upgrade a capsule to a newer application release with unchanged schema |
| M08 | Add the restricted declarative data migration engine |
| M09 | Synchronise ecosystem surfaces, harden, qualify and release |

## Programme files

- `program/` contains cross-milestone requirements and architecture.
- `contracts/` contains concrete draft SQL and JSON contracts.
- `milestones/` contains executable plans and acceptance gates.
- `prompts/` supports whole-program, next-milestone, resume and review sessions.
- `templates/` contains evidence and decision templates.
- `evidence/` is populated by Codex while implementing.
- `PROGRAM_STATUS.json` is the machine-readable handoff record.

## Priority of authority

1. Safety constraints and repository root instructions.
2. Accepted ADRs produced by M00 or later milestones.
3. Product requirements and acceptance outcomes in this programme.
4. Target architecture and concrete contract drafts.
5. File-path suggestions and implementation sketches.

A code-path mismatch is not permission to abandon the requirement. Adapt the
implementation and record the difference.

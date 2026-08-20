# M00 builder and critic report

Review performed 2026-08-12 against commit
`e73cf948fba233ef84d4680930b61549012020a7` after live-code inspection and
again after the integrated contract/ADR changes.

## Reviewers

- builder subagent `m00_builder`: traced Python/Rust verification, signed-app
  canonicalisation, first-open, raw-renderer and authoring/signing flows;
- independent critic subagent `m00_independent_critic`: audited every M00 gate,
  programme/contract consistency and evidence posture;
- security critic subagent `m00_security_critic`: attacked input stability,
  publication, migration, verification and renderer boundaries.

All reviewers were read-only. The main agent integrated every change.

## Material findings and disposition

| Severity | Finding | Disposition |
| --- | --- | --- |
| high | Native `verify_structure` checks metadata/integrity/FKs only; direct native signing and first-open wording could overstate exhaustive conformance | Accepted as a pre-existing fenced residual. ADR 0028 requires exhaustive non-executing verification before trust/signing/lifecycle use. This is M01's first implementation slice, before v0.3 dispatch. |
| high | Pre-execute file hashing did not prevent same-object, same-size mutation or change-capture-restore ABA during a transform | Resolved in ADR 0024 and the plan schema: plan and execute capture the pinned main database as raw bytes into private create-new storage, bind/reproduce exact `snapshot_sha256`, verify/read only the snapshot, and recheck the source before publication. |
| high | Read-only SQLite open could create sidecars or ignore complete WAL logical state | Resolved: lifecycle planning rejects WAL/SHM/rollback-journal state with a stable safe error and does not open SQLite against the source. A future complete WAL snapshot needs a separately reviewed profile. |
| medium | Destination parent substitution/reparse/alias races were not frozen | Resolved: destination token binds stable parent filesystem object identity plus validated leaf; temp/publish use a held parent handle; junction, symlink, reparse, ADS and alias cases fail closed. Directory mtime is deliberately not identity. |
| medium | Draft plan claimed expiry but its schema omitted it | Resolved: exact UTC `expires_at` is required and covered by `plan_digest`. |
| medium | `START_HERE` conformance claimed 13 columns while draft SQL selected 12 | Corrected to 12. |
| medium | Programme sections advertised divergent error aliases | Resolved: one `/1` catalogue is authoritative; programme lists now use it and bounded safe details. |
| medium | V0.2-to-v0.3 legacy upgrade was described but not representable | Resolved fail-closed: explicitly unavailable until M08 accepts a separate signed legacy-adapter ADR/schema; planners must not infer v0.2 datasets. |
| medium | Migration declarations exposed limits without a canonical signed storage source | Resolved: declaration limits removed. Resource ceilings are host-owned and plan-bound; publisher data cannot raise them. |
| medium | Draft application/instance/lineage/migration/example contracts diverged from SQL and the live Diagram Studio schema | Resolved: names, FKs, projections, typed values and operation allowlist reconciled; all ten live domain tables are classified exactly once. |
| medium | Native 512 MiB admission contradicted the 64 MiB Python/browser/plugin policy | ADR 0028 accepts 64 MiB as the cross-host v0.2/v0.3 policy correction; implementation and old/new rejection evidence are the first M01 slice. |
| medium | Raw-renderer absence was inferred rather than enumerated for future commands | Negative points are frozen. Every future lifecycle command/method must gain raw-label/protocol rejection coverage in its owning milestone. |

## Final verdict

Both critics reported no unresolved M00 architecture or security blocker after
the fixes. Strict Draft 2020-12 validation passed six examples and 37 lifecycle
records; the Diagram Studio contract covers all ten live domain tables exactly
once.

Accepted residuals are implementation work, not claims of delivered behavior:

- the pre-existing native shallow-verification/direct-signing gap is the hard
  first task of M01;
- compare-report v1 remains non-normative until M05 splits summary/detail and
  proves redaction/pagination;
- lifecycle source-race, destination-race and raw-renderer negative tests are
  required gates in the milestones that add those production surfaces.

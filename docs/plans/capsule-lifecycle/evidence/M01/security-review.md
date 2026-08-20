# M01 security critic report

Review performed 2026-08-12 by security critic subagent
`m00_security_critic`, including a final re-audit of the integrated fixes.

## Material findings and disposition

| Severity | Finding | Disposition |
| --- | --- | --- |
| high | Main-file hashes around ordinary SQLite opens permitted mixed evidence, WAL state and same-object ABA | Fixed; launch rejects WAL/SHM/journal state, captures a bounded private standalone snapshot, holds one read transaction, and rebinds exact bytes. |
| high | Snapshot capture could outgrow the 64 MiB admission checked earlier | Fixed; capture binds the inspected length, opened-handle metadata, an exact bounded copy, extra-byte rejection, snapshot length and final source hash/length. |
| high | Runtime could accept a writer commit released immediately after verification COMMIT | Fixed; `data_version` and change-log position are captured inside the verification transaction; a deterministic blocked-writer handoff test proves first asset release rejects the change. |
| high | CLI digest/signature reports reopened the live path | Fixed; native verify/digest/verify-signature derive identity, profile, digest and signature evidence from one `verify_read_only` snapshot connection. |
| high | v0.3 later endpoint steps and minimum host profile were not checked by Rust | Fixed with exhaustive step compilation and complete-tuple validation. |
| medium | Declared-check failures exposed expected/actual row values | Fixed; public detail is constant and tests assert private values are absent. |
| medium | Rollback/COMMIT ambiguity did not poison the runtime session | Fixed; rollback failure and any COMMIT failure return `SessionPoisoned`, which requires session closure; deferred-FK atomicity is tested. |

## Final verdict

M01 security gate passes with no unresolved high or medium finding. The final
focused security run passed 41/41 Rust tests across launch, runtime, CLI and
signing.

M02 owns two deliberately deferred surfaces: the held destination-parent
publication primitive and the first external lifecycle `/1` error envelope.
No lifecycle command is exposed to raw Wry, and direct signing is improved
without broadening that renderer boundary.


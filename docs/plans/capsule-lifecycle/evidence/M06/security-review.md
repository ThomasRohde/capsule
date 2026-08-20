# M06 security review

**Verdict:** PASS  
**Reviewed:** 2026-08-20  
**Scope:** input immutability, row/value authority, policy and sensitivity,
publication, cancellation/deadline behavior, trusted Tauri and raw isolation.

The M06 security reviews found no unresolved release blocker in the settled
implementation.

- Source, target and optional ancestor are opened as retained verified read-only
  snapshots, rebound by exact identity/hash before publication and proven
  byte-identical by hostile and Windows end-to-end tests.
- Renderer-supplied JSON is never execution authority. Core review and prepared
  typestates are non-serializable; trusted Tauri uses random opaque capabilities;
  CLI recomputes and revalidates the complete report and plan in-process.
- Sensitive confirmation is the exact set of changed signed datasets and remains
  bound even when a conflict resolves to keep-target with zero emitted writes.
- SQL identifiers come only from admitted signed contracts, values are typed
  parameters, operations are transactionally checked, removed bytes are vacuumed,
  and exact output state/application/signature/lineage is reverified.
- Destination authority is held-parent-relative and create-new/no-replace. Late
  input changes or cancellation cannot report success; post-rename failures are
  quarantined or marked using the lifecycle primitive.
- Classification and resolution each have one bounded work deadline; human
  review authority cannot exceed five minutes or refresh itself.
- Raw Wry exposes no Tauri globals and rejects all ten reconciliation commands in
  both locked and authorized states.

The final independent security pass was stopped from running an additional
duplicate gate cycle at the user's direction after static review found no
blocker. All previously raised HIGH/MEDIUM findings had already been fixed and
independently rerun; root subsequently completed the settled 373-test Rust gate,
trusted/raw Windows evidence, supply-chain checks and NSIS qualification. No
security exception or UI-only authorization dependency is accepted.

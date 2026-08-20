# M05 security review

**Verdict:** PASS  
**Reviewed:** 2026-08-13  
**Scope:** source immutability, bounded disclosure, opaque continuation,
trusted-shell authority, deadline/cancellation and raw renderer isolation.

The settled-tree security audit found no unresolved M05 security blocker.

- Both sources are independently pinned, exhaustively verified and retained
  read-only. Final rebinds and hostile tests prove source hashes remain
  unchanged.
- Summary, row/field pages and application expansion are bounded, deterministic
  and value-free by default. Sensitive pages require an explicit session-bound
  reveal; BLOB content is never rendered.
- Dataset/table/page selection uses random host-owned capabilities. JavaScript
  cannot submit paths, SQLite identifiers, SQL, numeric contract positions or
  canonical cursors.
- Active cancellation and expiry are bound to the exact session ID. One
  absolute deadline begins before source verification, supplies every remaining
  operation budget, and drops the exact idle session without resetting.
- Trusted-shell evidence consumes a real second page, preserves sensitive reveal
  continuity, expands exactly thirteen application families and confirms source
  immutability.
- Raw-Wry evidence denies all six compare methods in locked and authorized
  states and confirms Tauri globals/internals are absent.

Independent settled-tree reruns passed desktop Compare 5/5, workspace Compare
30/30 plus CLI 1/1, and strict lifecycle validation. No HIGH, MEDIUM or accepted
security residual remains.

# M03 security review

Date: 2026-08-12  
Reviewer: `m00_security_critic`  
Verdict: **PASS**

No HIGH or MEDIUM M03 security blocker remains. The final review confirmed:

- Overview uses the retained verified snapshot and performs no input recovery
  or mutation;
- signature validity, host-local publisher trust and mutable instance identity
  remain distinct;
- PNG/WebP images are bounded, hash-checked, decoded outside the DOM and
  re-encoded as host-owned static PNG data;
- Cabinet recents are owner-private, bounded, non-authoritative and freshly
  inspected on open;
- Windows cache roots reject reparse components and a real junction test leaves
  the redirect target untouched;
- capability and recovery actions require the current opaque selection and
  reject stale handles with the stable `stale_plan` error;
- remembered releases remain locked until explicit trusted-shell Open;
- Tauri permissions remain scoped to `main`, and the raw Wry renderer has no
  lifecycle IPC handler.

Accepted low residual: Cabinet component checks are path-based rather than a
held-handle walk. The cache remains owner-private and non-authoritative, and
pre-existing Windows junctions fail closed, so this does not grant lifecycle
or execution authority.

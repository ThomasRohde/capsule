# M07 consolidated independent review

**Verdict:** PASS after remediation

**Reviewed:** 2026-08-20

**Scope:** same-schema upgrade core and publication typestate, SemVer and signed
policy admission, CLI/Tauri authority, trusted UI/raw isolation, contracts,
tests and generated artefacts.

The one permitted read-only consolidated review found no P0/P1 issue. Its
initial findings were remediated before qualification:

- The SemVer implementation now rejects empty pre-release/build fields and
  compares arbitrarily large valid numeric identifiers without a `u64` limit,
  while preserving the 128-byte admission bound.
- A release with multiple signatures retains one exact host-selected common,
  valid publisher key through review and confirmation instead of assuming a
  singleton signature.
- Active operations are bound to the retained selection and cancel when that
  selection is invalidated; later input changes fail closed.
- Positive `rebuild`/empty-`omit`, non-empty-`omit`, cancellation, late-input,
  destination-race and all six crash-stage cases have direct coverage.

The single allowed remediation review verified those corrections and found one
P2 copy mismatch: the intermediate release-selection screen claimed full
compatibility admission before `prepare_upgrade_review` ran. The UI now says
only “Release screened” and explicitly defers same-app, newer-version and
same-schema admission to Prepare. The directly affected native UI and trusted
upgrade E2E passed after that correction.

No finding is waived. Security does not depend on renderer hiding: opaque
selection/destination/review/operation authority remains host-owned, and the
full raw-Wry suite denies all seven upgrade commands in locked and authorized
states. The review ran no Cargo/build process and therefore did not contend for
the root-owned build lease.

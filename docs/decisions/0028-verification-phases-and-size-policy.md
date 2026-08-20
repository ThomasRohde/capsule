# ADR 0028: Native verification phases and capsule size policy

## Status

Accepted on 2026-08-12 by lifecycle milestone M00. The implementation gap is a
required first slice of M01 before v0.3 signing or lifecycle planning is added.

## Context

Live Rust first-open and signing call `capsule-launch::verify_structure`, which
currently performs bounded manifest inspection, SQLite integrity and foreign-key
checks only. Exhaustive platform shape, assets, endpoint compilation and
declared checks run later in `capsule-runtime::VerifiedCapsule::open`. Calling
the earlier result `structure_verified` can let a malformed capsule reach a
trust prompt or direct signing preparation before it is rejected at activation.

The live Python/bootstrap/browser/plugin contract caps capsule files at 64 MiB,
while `capsule-core` currently admits 512 MiB during native inspection.

## Decision

Use explicit verification phases:

1. `metadata_inspected` — bounded read-only header/manifest projection only;
2. `conformance_verified` — integrity, foreign keys, exact versioned structural
   profile, forbidden-object policy, asset paths/hashes/sizes, endpoint and
   permission declarations, and signature-compartment structure;
3. `declared_checks_passed` — capsule checks run only through the existing
   read-only authoriser, progress deadline and result bounds;
4. `signature_evaluated` — cryptographic validity and digest match, still
   separate from publisher trust;
5. `policy_decided`; and
6. `runnable` — the only state that releases application assets or a protocol
   session.

First-open review may render clearly labelled metadata before phase 2, but it
must not report the capsule as structurally verified or offer a persistent
release decision until phases 2-4 have completed as applicable. Native signing
preparation and every lifecycle input/output gate use the same exhaustive
non-executing verifier, including declared checks. Application assets,
endpoints, commands and prompts are not executed by these phases.

The product-independent maximum capsule file size is 64 MiB for v0.2 and v0.3.
M01 will tighten the native inspection limit from 512 MiB to the already
documented 64 MiB. This accepted compatibility correction is fail-closed and
aligns native behavior with the current format, Python, HTML and standalone
creator-plugin contracts; it does not reinterpret a valid signed compartment.

Unknown platform tables and additional platform columns are rejected by the
exhaustive v0.3 conformance profile. M01 must record the current v0.2 verifier
behavior separately and must not silently impose v0.3 table semantics on v0.2.

## Consequences

- A trust prompt, signature preview or lifecycle plan cannot lend credibility to
  a capsule that has only passed shallow metadata inspection.
- `capsule-runtime` conformance logic should be factored into a reusable
  read-only verifier rather than duplicated with weaker launch checks.
- Tests must isolate each phase and prove that failure releases no assets,
  protocol session, signature output or lifecycle plan.


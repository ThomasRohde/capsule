# M01 independent critic report

Review performed 2026-08-12 after the v0.3 implementation and repeated after
all integrated fixes.

## Reviewer

Independent critic subagent `m00_independent_critic` inspected contracts, Python
and Rust implementations, fixtures, plugin snapshots and test coverage. The
critic made no edits; the main agent integrated the resolutions.

## Material findings and disposition

| Severity | Finding | Disposition |
| --- | --- | --- |
| high | Rust skipped later v0.3 compound endpoint steps | Fixed; steps load for v0.2/v0.3 and a hostile later-step test fails closed. |
| high | The first v0.3 vector proved only digests and was not a conformant signed fixture | Replaced with a conformant fixture containing a deterministic Ed25519 envelope; Python/Rust verify the old signature across included/excluded mutations. |
| high | Python/Rust could dispatch signatures or overview from `user_version` alone | Fixed; application ID and the complete format/runtime/minimum-host tuple are checked before profile selection. |
| high | Python overview accepted extra application/instance rows and omitted bounded metadata families | Fixed; exact cardinality, UTF-8 byte bounds, UUIDs, timestamps, references, tags and instance-asset metadata have hostile tests. |
| high | Signing/launch evidence came from several path reopens | Fixed; exhaustive conformance, checks and signatures share one read-only private snapshot connection; signing copies only that verified snapshot. |
| medium | Canonical-stream limit was accidentally conflated with 64 MiB file admission | Fixed; v0.2's 512 MiB canonical-stream limit remains unchanged while file admission is 64 MiB. |
| medium | Python inspection could throw `TypeError` for non-string tags | Fixed; element shape is validated before uniqueness and direct/CLI tests prove a structured failure without traceback. |
| medium | Plugin/runtime/docs did not fully describe or carry the v0.3 authoring/signature contract | Fixed; standalone plugin supports explicit v0.3 while defaulting to v0.2, carries byte-identical sources, documents compartments and passes its isolated matrix. |

## Final verdict

No substantive M01 implementation blocker remains. The critic independently
observed passing direct hostile-inspection tests, standalone plugin tests,
v0.2/v0.3 signed vectors, focused Rust tests and generated capsule freshness.
Final repository-wide and installer evidence is recorded in `RESULT.md`.


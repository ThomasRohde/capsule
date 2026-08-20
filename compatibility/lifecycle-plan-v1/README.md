# Lifecycle plan v1 canonical JSON vectors

These vectors freeze `org.sqlite-capsule.lifecycle-plan/1` canonical bytes and
SHA-256 behavior across the Rust workspace implementation and the independent
Python standard-library checker.

The profile is intentionally not JCS: object keys use Unicode scalar ordering,
floating-point values and duplicate keys are forbidden, strings are not Unicode
normalised, and `plan_digest` is omitted when its digest is calculated.

`vector-plan.json` is also a semantically valid `duplicate` plan under ADR
0029: it preserves capsule/revision identity and contains exactly one
`copy-exact-snapshot` application decision. Operation-specific hostile vectors
remain necessary; canonical validity or a recomputed digest never grants
execution authority.

Run:

```text
python tools/check_lifecycle_plan_vectors.py
cargo test -p sqlite-capsule-workspace plan --all-targets
```

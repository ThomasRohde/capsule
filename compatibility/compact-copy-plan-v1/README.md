# Compact-copy plan v1 vector

This vector freezes one semantically valid `compact-duplicate` lifecycle plan
using action `copy-compact-logical-state`. Canonical JSON validity grants no
authority: execution additionally requires the original host-held destination
reservation, retained verified compact source, current time/cancellation, and
exact byte-for-byte approved review plan.

Run:

```text
python tools/check_lifecycle_plan_vectors.py --vector-dir compatibility/compact-copy-plan-v1
cargo test -p sqlite-capsule-workspace compact_plan_vector --all-targets
```

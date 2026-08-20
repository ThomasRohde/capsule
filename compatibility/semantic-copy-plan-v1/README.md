# Semantic copy lifecycle-plan v1 vector

This directory freezes one canonical, semantically valid signed-v0.3 `fork`
plan. It binds the host-owned semantic operation decision, the frozen
`dataset-state/1` source digest profile, signed policy, mutable-platform reset
profile, fresh output identities and create-new publication contract.

Run the independent standard-library checker and the Rust parser test:

```text
python tools/check_lifecycle_plan_vectors.py --vector-dir compatibility/semantic-copy-plan-v1
cargo test -p sqlite-capsule-workspace semantic_plan_vector --all-targets
```

Canonical validity never grants execution authority. The executor additionally
requires the non-serializable held review, retained verified source and held
destination reservation.

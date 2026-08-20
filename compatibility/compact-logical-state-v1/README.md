# Compact logical-state v1 vectors

These vectors freeze `org.sqlite-capsule.compact-logical-state/1`
independently of the Rust lifecycle executor. The standard-library Python
checker builds one physical SQLite fixture, hashes its complete logical state,
runs `VACUUM`, and requires the same digest while the exact file hash changes.
It also proves isolated domain-row, schema, `sqlite_sequence`, signature-row and
implicit-rowid mutations change the digest.

Run:

```text
python tools/check_compact_state_vectors.py
cargo test -p sqlite-capsule-workspace compact_state --all-targets
```

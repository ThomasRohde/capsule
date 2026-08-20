# Template-state v1 vectors

These vectors freeze `org.sqlite-capsule.dataset-state/1` framing independently
of the Rust workspace implementation. They use the existing conformant v0.3
fixture plus `type-spectrum.sql`. They include every actual ordinary-table
column; stored and virtual generated columns; an empty-table header; multiple
tables; composite WITHOUT ROWID key ordering; NULL, signed INTEGER, REAL signed
zero, TEXT and BLOB values; and distinct precomposed/combining UTF-8 forms.
They preserve raw SQLite storage classes and perform no Unicode normalization.

Run:

```text
python tools/check_template_state_vectors.py
cargo test -p sqlite-capsule-workspace template_state --all-targets
```

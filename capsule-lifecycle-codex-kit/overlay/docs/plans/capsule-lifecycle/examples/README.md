# Diagram Studio lifecycle examples

These reviewable JSON examples turn the generic contracts into one visual
application scenario without placing Diagram Studio semantics in generic code.

They cover:

- signed application display metadata;
- mutable instance title/description/tags;
- dataset roles and fork/compare/reconcile/upgrade policies;
- lineage across template creation and application upgrade;
- same-schema upgrade planning;
- a restricted v1→v2 data migration.

Hashes in illustrative plans/lineage are synthetic. M01-M09 must generate
deterministic SQLite fixtures and real digests from repository builders.

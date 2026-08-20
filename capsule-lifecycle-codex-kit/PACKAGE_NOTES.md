# Package notes

## Normative versus draft material

The product requirements, security invariants, lifecycle semantics and
acceptance outcomes are normative unless the live repository makes one
impossible or unsafe. Draft SQL and JSON files are proposed concrete contracts.
Milestone 0 may refine them, but any semantic change requires a recorded
decision and corresponding updates to all plans and tests.

## Deliberate non-goals

This programme does not add:

- cloud sync, accounts or collaboration;
- a remote capsule marketplace or automatic application download;
- general filesystem, network or SQL access for capsule applications;
- in-place merge, in-place upgrade or implicit source replacement;
- execution of untrusted capsule assets during metadata, copy, compare or
  upgrade planning;
- arbitrary migration code supplied by a capsule;
- cross-publisher application takeover disguised as an upgrade.

## Terminology

- **Host update** means updating the installed native Tauri host.
- **Application upgrade** means moving a working capsule to a newer signed
  embedded application release while preserving its user-owned instance and
  data.
- **Duplicate** preserves logical capsule and revision identity.
- **Fork** creates a new logical capsule and revision.
- **Reconcile** applies selected data changes to a new target-derived copy.
- **Rebase** is the product-facing shorthand for application upgrade, not Git
  rebase.

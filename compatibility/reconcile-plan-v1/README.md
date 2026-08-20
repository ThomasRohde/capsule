# Reconciliation payload v1 vectors

These vectors freeze the two-layer reconciliation authority used by M06:

- `vector-payload.json` is canonical
  `org.sqlite-capsule.reconcile-payload/1`. It is value-free, contains only
  reviewed effects/evidence, and grants no filesystem authority.
- `vector-plan.json` is the authoritative
  `org.sqlite-capsule.lifecycle-plan/1` envelope. It binds the exact payload
  with one `bind-reconcile-payload` decision and owns all pinned inputs and the
  create-new destination.

The payload digest uses the two-alias omission rule below; the lifecycle plan
digest omits `plan_digest`. Both use the lifecycle canonical JSON profile. Conflict IDs are
SHA-256 over the canonical `org.sqlite-capsule.reconcile-conflict-id/1`
evidence object reproduced by the independent checker.

The exact lineage event is part of the payload. To represent the same payload
digest in durable lineage details without a hash fixed-point, digest material
omits both the top-level `payload_digest` and the equal
`lineage.details.payload_digest` alias. The checker then populates and verifies
both aliases against the resulting SHA-256.

Normative host ceilings are 16 MiB source and canonical payload bytes, nesting
depth 32, 10,000 operations, 10,000 resolved conflicts, 256 datasets, 256
fields per field operation and 4,096 signature records per input inventory.
The byte ceiling is checked before JSON decoding; all other ceilings are checked
before planning or executing writes. These payload limits are separate from the
1 MiB `lifecycle-plan/1` ceiling because the plan carries only one payload
digest, never an embedded payload.

Run:

```text
python tools/check_reconcile_plan_vectors.py
python -m unittest tests.test_reconcile_plan_vectors -v
```

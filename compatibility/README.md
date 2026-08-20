# Interoperability test vectors

This directory contains reviewable, deterministic vectors used to compare
independent implementations of optional capsule extensions.

`signed-app-v0.2/` freezes the compatibility canonical stream byte-for-byte;
`signed-app-v0.3/` freezes the separate v2 context, generic conformant fixture,
and mutable-versus-signed mutation contract. Python and Rust must agree on both
profiles. The included v0.2 private seed is shared public test-only material and
must never be treated as a publisher identity.

`template-state-v1/` freezes the independent
`org.sqlite-capsule.dataset-state/1` streaming digests used by authenticated
clean-template proofs. Python and Rust reproduce the same row counts, canonical
stream byte lengths and SHA-256 values from the generic v0.3 fixture.

`compare-row-v1/` freezes the exact `org.sqlite-capsule.compare-key/1` and
`org.sqlite-capsule.compare-row/1` bytes and SHA-256 digests. It covers every
SQLite storage class, signed integer bounds, composite and mixed-storage keys,
positive/negative zero, composed/decomposed Unicode, empty BLOBs and rejection
of non-finite REAL or invalid UTF-8 input. `tools/check_compare_row_vectors.py`
is the independent Python implementation; the Rust comparator must consume the
same checked-in vector rather than generating its expected values itself.

`reconcile-plan-v1/` freezes the value-free
`org.sqlite-capsule.reconcile-payload/1`, its exact lineage/conflict evidence,
and the authoritative `org.sqlite-capsule.lifecycle-plan/1` envelope that binds
the payload digest, pinned source/target/ancestor inputs and create-new output.
`tools/check_reconcile_plan_vectors.py` independently enforces canonical bytes,
closed effects, count/depth/byte ceilings and all cross-layer bindings.

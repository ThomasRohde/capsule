# Programme helper tools

All tools use the Python standard library unless stated otherwise.

- `capture_baseline.py` records checkout/toolchain/path state without running
  tests or builders.
- `program_status.py` atomically validates and advances milestone status.
- `validate_lifecycle_specs.py` parses contracts, compiles draft SQL, checks
  milestone structure and optionally validates JSON examples.
- `run_evidence.py` runs one command without a shell and retains bounded output.

Run these from the repository root using the full installed path:

```text
python docs/plans/capsule-lifecycle/tools/codex_lifecycle/validate_lifecycle_specs.py
```

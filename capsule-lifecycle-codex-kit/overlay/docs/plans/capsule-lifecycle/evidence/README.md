# Programme evidence

Codex populates this directory during implementation.

Recommended layout:

```text
evidence/
  M00/
    baseline-<timestamp>.json
    baseline-tests/
    architecture-review.md
  M01/
    canonical-vectors/
    signature-mutation-matrix.json
    ...
```

Evidence may contain local paths, environment details or security-sensitive
diagnostics. Follow repository policy before committing it. Never store private
keys, secrets, raw sensitive comparison values or unredacted environment dumps.

Use `tools/codex_lifecycle/run_evidence.py` to retain bounded command output when
appropriate. Milestone `RESULT.md` remains the index to evidence.

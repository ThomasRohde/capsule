---
name: capsule-runner
description: Inspect, verify, run, stop, and develop SQLite Capsule application files in this repository. Use whenever the user mentions a .capsule.sqlite file, asks to run the database, or asks how the embedded application protocol works.
---

# SQLite Capsule runner

Operate capsules through their embedded metadata rather than assuming launch commands from memory.

## Run an existing capsule

1. Read the repository `AGENTS.md`.
2. Identify the target `.capsule.sqlite` path. Prefer the explicit path in the request; otherwise use `capsules/diagram-studio.capsule.sqlite` for this scaffold.
3. Print the embedded agent runbook:

   ```bash
   python tools/capsule.py instructions <capsule>
   ```

   When the repository host is absent, query `SELECT * FROM START_HERE` with Python's standard-library `sqlite3` module.

4. Inspect and verify before execution:

   ```bash
   python tools/capsule.py inspect <capsule>
   python tools/capsule.py verify <capsule>
   ```

5. Execute only after a deliberate trust decision. The checked-in generated example is trusted by repository policy:

   ```bash
   python tools/capsule.py start <capsule> --trust-capsule
   ```

6. Confirm the returned URL through `status` or `/__capsule/health`. Report the healthy URL, capsule identity, and any warnings.
7. Stop only the matching recorded host:

   ```bash
   python tools/capsule.py stop <capsule>
   ```

## Develop the example

Keep generic platform code and example code separate. Edit reviewable sources, then run:

```bash
python tools/build_example.py
python -m unittest discover -s tests -v
python tools/capsule.py verify capsules/diagram-studio.capsule.sqlite
```

For browser behaviour, test the real served application and persistence after restart. Do not add raw SQL access from browser code, remote dependencies, non-loopback binding, or Diagram Studio knowledge to the generic host.

## Standalone fallback

When only a capsule file survives, follow the exact `extract.embedded`, `verify.embedded`, and `start.embedded` command records stored in the database. Treat the extracted host as untrusted until inspected; embedding provides compatibility, not authenticity.

# Agent operation

A coding agent should be able to receive only this intent:

> Open this SQLite capsule, read `START_HERE`, verify it, and run it.

The database contains the version-specific procedure.

## Safe discovery with only Python

Python includes a SQLite client in its standard library. To print the complete launch runbook without executing capsule assets:

```bash
python -c "import pathlib,sqlite3,sys,urllib.parse; p=pathlib.Path(sys.argv[1]).resolve().as_posix(); c=sqlite3.connect('file:'+urllib.parse.quote(p,safe='/:')+'?mode=ro',uri=True); q=c.execute('SELECT * FROM START_HERE ORDER BY sequence'); [print(dict(zip([d[0] for d in q.description],r))) for r in q]" "PATH_TO_CAPSULE"
```

Use `argv_json` from that view and `capsule_command` when present. Substitute placeholders as whole process arguments and execute without a shell. `command_template` is the human-readable fallback. Available placeholders include:

- `{python}` with the current Python executable, normally `python` or `py -3`;
- `{capsule}` with the absolute or correctly quoted capsule path;
- `{repo}` with the source repository root when present;
- `{cache}` with a disposable directory, normally `.capsule-cache`.
- `{bundle}`, `{output_capsule}`, and `{other_capsule}` for repository authoring commands.

## Trust sequence

1. Inspect identity, requested permissions, assets, endpoints, prompts, documents, and commands read-only.
2. Verify full SQLite integrity, foreign keys, required platform structure, hashes, endpoint compilation, trigger absence, and capsule checks.
3. Make an explicit trust decision. This repository authorises its checked-in generated example; an arbitrary downloaded database is not automatically trusted.
4. Start the loopback-only host and confirm the health endpoint.
5. Report the actual healthy local URL rather than merely reporting that a process was spawned.
6. Stop the matching host through its recorded shutdown command when finished.

## Standalone survival path

`bootstrap/capsule_host.py` is stored as an asset in the database. When the repository host is unavailable, follow the explicit `extract.embedded`, `verify.embedded`, `start.embedded`, `status.embedded`, and `stop.embedded` rows. Extraction uses paths passed through `sys.argv`, verifies the asset hash, and refuses existing targets. Executing the extracted verifier or host is still local execution of code supplied by the capsule; internal integrity is not publisher authentication.

The detached start command must return promptly. Status accepts only a non-redirecting loopback health response with this capsule's identity and protocol. Normal output never includes the shutdown secret.

This capsule uses format `0.2` and runtime protocol `capsule-http/0.2`. Its write endpoints may contain bounded ordered `capsule_endpoint_step` rows that execute atomically. The embedded host supports this current profile only and rejects any other format identity.

For persistence evidence, mutate only a verified temporary copy, record its history cursor and model state, stop the matching identity-checked host, restart with the same capsule and state directory, and confirm both state and Undo/Redo behavior. Do not hand-edit the generated database or claim success from process creation alone.

When the repository exporter is present, a self-contained HTML file is a
derivative of this capsule, not a replacement. Verify the source before export,
use `inspect-html` / `verify-html` on the result, and retain its source/current/
parent provenance. Read-only profile names do not grant trust; editable saves
create a new HTML revision and must never silently mutate this source database.

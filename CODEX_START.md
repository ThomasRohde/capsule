# Start with Codex

Open this folder in Codex and send exactly this:

> Open `capsules/diagram-studio.capsule.sqlite`. Read its embedded `START_HERE` runbook, verify the capsule, and run it. Treat the database as the runtime source of truth and do not fetch dependencies. Report the healthy local URL.

That prompt states intent, not setup procedure. Codex should discover the current procedure, trust notes, structured argument vectors, and success conditions from the database itself.

For a standalone test, give Codex only the `.capsule.sqlite` file and this prompt:

> Inspect this SQLite application read-only, query `START_HERE`, extract its compatible host only after reviewing the embedded trust instructions, verify it, run it offline, and report the healthy local URL.

The standalone path needs Python with its built-in `sqlite3` module. No repository-specific package is required.

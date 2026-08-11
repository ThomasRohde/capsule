# Capsule authoring contract

Use this reference while designing or editing a project created by `capsule_project.py`.

## Project source

`capsule-project.json` owns stable application identity, version, summary, entry asset, UTC timestamps, and declared capabilities. Builds must not invent changing metadata.

`domain.sql` owns application-specific tables, indexes, and views. Do not use triggers, virtual tables, attached databases, extension loading, or `capsule_`-prefixed objects.

`source/data/seed.json` maps domain table names to arrays of complete row objects. Arrays and objects are stored as canonical compact JSON text.

`source/app/` contains offline browser assets. `app/index.html` is the entry asset. Reference embedded assets with root-relative `/app/...` URLs because the host serves the entry document at `/`. Use the embedded `app/capsule-client.js` API:

- `SQLiteCapsuleClient.manifest()`
- `SQLiteCapsuleClient.read(name, parameters)`
- `SQLiteCapsuleClient.write(name, parameters)`

The browser receives no SQLite handle and no arbitrary SQL method.

`source/endpoints.json` declares stable named reads and writes. Every SQL parameter must have one matching rule with type `string`, `number`, `integer`, `boolean`, or `json`. Writes are transactional and logged by the host. Use `steps` only for atomic compound writes.

`source/checks.json` contains bounded read-only application invariants. Error checks must pass before publication.

`source/runbooks.json`, `source/prompts.json`, and `source/docs.json` make the result discoverable to agents and humans. The builder adds generic inspect, verify, start, status, stop, and standalone-host commands.

## Required boundaries

- SQLite is the canonical runtime artifact; HTML exports are derivatives.
- The application stays offline and runs through the loopback Python host.
- The build embeds the current generic Python host and browser client from this repository.
- The plugin's authoring path has no native desktop dependency.
- `database.read` and `database.write` must be explicitly declared when matching endpoints exist; `network.value` remains `none`.
- Entry HTML, each asset SHA-256, foreign keys, endpoint declarations, and application checks must verify before the output is atomically published.
- Build into a new path by default. Permit replacement only after resolving the exact generated target.

## Completion evidence

For each created capsule, retain:

1. successful `capsule_project.py check` byte-freshness output;
2. successful `tools/capsule.py verify` and `conformance` output;
3. the embedded `START_HERE` instructions;
4. a real loopback-browser smoke test;
5. for editable apps, a named write followed by reload and direct SQLite persistence evidence.

# Source map

Prepared against public repository `ThomasRohde/capsule`, main branch observed
12 August 2026.

## Repository sources

- `README.md` — product scope, current v0.2 features, Diagram Studio and native
  host commands.
- `AGENTS.md` — source boundaries, security rules, plugin synchronisation and
  definition of done.
- `CONTRIBUTING.md` — development gates and release workflow.
- `docs/architecture.md` — trusted host, raw renderer, signed application,
  lifecycle and backup architecture.
- `docs/format-contract.md` — v0.2 contract and compatibility rules.
- `docs/native-host-contract.md` — trusted shell/raw Wry separation, state
  machine, lifecycle and update boundaries.
- `format/capsule-v0.2.sql` — current mixed manifest.
- `format/capsule-signed-app-v0.2.sql` and schema — publisher/signature
  extension.
- `native/crates/capsule-core/src/lib.rs` — bounded v0.2 metadata inspection.
- `native/crates/capsule-crypto/src/lib.rs` — signed table allowlist and
  canonical digest.
- `native/crates/capsule-lifecycle/src/lib.rs` — current file lifecycle scope.
- `native/desktop/ui/index.html` and `app.js` — current Trust review-first shell.
- `native/Cargo.toml` — current crate workspace and pinned dependencies.

## External primary references

- SQLite application file format:
  `https://sqlite.org/appfileformat.html`
- SQLite online backup API:
  `https://sqlite.org/backup.html`
- SQLite `VACUUM INTO`:
  `https://sqlite.org/lang_vacuum.html#vacuuminto`
- SQLite Session Extension:
  `https://sqlite.org/sessionintro.html`
- Codex AGENTS.md:
  `https://developers.openai.com/codex/agent-configuration/agents-md`
- Codex skills:
  `https://developers.openai.com/codex/build-skills`
- Codex subagents:
  `https://developers.openai.com/codex/agent-configuration/subagents`

## Source discipline

Codex must prefer the live checkout over this source map. When current
repository behaviour differs, record the actual path/contract and update the
programme before implementation.

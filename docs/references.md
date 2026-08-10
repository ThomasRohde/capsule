# References

Primary inspirations and implementation references, checked in August 2026.

## SQLite

- SQLite as an application file format: <https://sqlite.org/appfileformat.html>
- Official SQLite WASM documentation: <https://sqlite.org/wasm/doc/trunk/index.md>
- SQLite WASM persistence and OPFS choices: <https://sqlite.org/wasm/doc/trunk/persistence.md>
- Pinned ES module wrapper source: <https://github.com/sqlite/sqlite-wasm>
- WebAssembly in-memory instantiation: <https://developer.mozilla.org/docs/WebAssembly/Reference/JavaScript_interface/instantiate_static>
- Compression Streams API: <https://developer.mozilla.org/docs/Web/API/Compression_Streams_API>
- File System Access API: <https://developer.mozilla.org/docs/Web/API/File_System_API>
- Sandboxed iframe behavior: <https://developer.mozilla.org/docs/Web/HTML/Reference/Elements/iframe#sandbox>
- SQLite command-line shell: <https://sqlite.org/cli.html>
- SQLite online backup API: <https://sqlite.org/backup.html>

SQLite's own documentation explicitly presents the database as a stable, cross-platform application file format and warns implementers to harden applications that open untrusted database files.

## Cryptography and native host

- RFC 8032, EdDSA and Ed25519: <https://www.rfc-editor.org/rfc/rfc8032>
- RFC 8785, JSON Canonicalization Scheme: <https://www.rfc-editor.org/rfc/rfc8785>
- Tauri architecture: <https://v2.tauri.app/concept/architecture/>
- Wry 0.55.1 API documentation: <https://docs.rs/wry/0.55.1/wry/>
- Windows file-type registration: <https://learn.microsoft.com/windows/win32/shell/fa-file-types>
- Tauri Windows installer hooks and WiX fragments: <https://v2.tauri.app/distribute/windows-installer/>
- Windows Installer `OriginalDatabase` property: <https://learn.microsoft.com/windows/win32/msi/originaldatabase>
- Windows `ShellExecuteExW`: <https://learn.microsoft.com/windows/win32/api/shellapi/nf-shellapi-shellexecuteexw>
- RustSec advisory database and cargo-audit: <https://rustsec.org/>
- `glib::VariantStrIter` advisory RUSTSEC-2024-0429: <https://rustsec.org/advisories/RUSTSEC-2024-0429.html>
- Current `tauri-utils` crate metadata: <https://crates.io/crates/tauri-utils>
- Current `urlpattern` crate metadata: <https://crates.io/crates/urlpattern>

The signed-app profile uses the RFC algorithms as byte-level interoperability
contracts. Tauri supplies trusted desktop lifecycle only; untrusted capsule
content is hosted in a raw Wry child with no Tauri bootstrap or registered IPC
handler.

## Bento

- Bento repository: <https://github.com/nyblnet/bento>
- Bento architecture: <https://github.com/nyblnet/bento/blob/main/docs/architecture.md>
- Bento platform invariants: <https://github.com/nyblnet/bento/blob/main/docs/PLATFORM.md>

Relevant principles are one-file sovereignty, offline operation, embedded viewer/editor, self-save, inspectable document data, and agent-friendly manipulation. SQLite Capsule adopts those principles but uses SQLite as canonical state and a generic host as an explicit trust boundary.

## Codex

- Project guidance with `AGENTS.md`: <https://developers.openai.com/codex/agent-configuration/agents-md>
- Codex skills: <https://developers.openai.com/codex/build-skills>
- Agent-friendly CLI guidance: <https://developers.openai.com/codex/use-cases/agent-friendly-clis>

The repository uses root `AGENTS.md` and a repo-scoped `.agents/skills/capsule-runner/SKILL.md`. Current Codex documentation identifies those as native project guidance and skill mechanisms.

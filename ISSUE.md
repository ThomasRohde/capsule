# Capsule Platform Review

Status: **complete** (2026-08-11)
Reviewer: Claude Code (Fable 5)

Scope: format contract, Python loopback host, browser runtime, HTML export,
CLI/authoring tools, creator plugin, native Windows host (sampled), tests.
Method: manual code review against the repository's own stated rules
(AGENTS.md, docs/security.md, format contract) plus general correctness and
security analysis. Findings are listed per component with severity:
**[HIGH]** likely bug / security boundary issue, **[MED]** correctness or
robustness concern, **[LOW]** polish / consistency / documentation.

## Findings

### 1. Format contract (`format/`, `docs/format-contract.md`)

Overall: coherent and unusually well-specified. Schema, conformance JSON, and
prose agree on tables, keys, and types. Issues found:

- **F1 [LOW] Asset-path control characters are documented as rejected but no
  layer rejects them.** `docs/format-contract.md` ("Assets") says absolute
  paths, backslashes, traversal, *control characters*, empty segments, and
  encoded traversal are rejected. The schema CHECKs
  (`format/capsule-v0.2.sql:36-42`) and the host validator
  (`runtime/capsule_host.py:317` `safe_asset_path`) cover the other cases, but
  nothing rejects a path containing e.g. `\n` or `\x00`. No header-injection
  path exists today (responses never echo the path into headers), so impact is
  doc/impl drift rather than a live vulnerability. Fix: add a control-character
  check to `safe_asset_path` (and mirror it in the authoring validator +
  conformance description), or soften the doc claim.
- **F2 [LOW] `capsule_change_log.endpoint_name` has no FK to
  `capsule_endpoint`.** Probably intentional (log must survive endpoint
  removal), but the format doc doesn't say so; one sentence would prevent a
  conformance implementer from "fixing" it.
- **F3 [LOW] Cosmetic:** `capsule_grant` column alignment drift in
  `format/capsule-v0.2.sql:23-25` (`decision`/`reason`/`granted_at` indented
  one space differently than the rest of the file).

### 2. Python loopback host (`runtime/capsule_host.py`)

Overall: a genuinely careful implementation. Loopback-only bind, Host/Origin
validation, session + shutdown secrets compared with `compare_digest`,
default-deny CSP, authorizer + `query_only` double-guard on reads, protected
`capsule_`/`sqlite_` prefixes, bounded results, JSON-depth limits, atomic state
files with symlink refusal. Issues found:

- **F4 [MED] Windows port hijack: `allow_reuse_address = True` sets
  `SO_REUSEADDR` on Windows (`runtime/capsule_host.py:1485`).** Verified on the
  local Python 3.13: `TCPServer.server_bind` applies `SO_REUSEADDR`
  unconditionally when the flag is set. On Windows, `SO_REUSEADDR` allows
  another local process to bind the same port *while the host is listening*
  (classic Windows port-stealing; POSIX semantics do not apply). On a
  multi-user machine another user's process can race/steal the listener and
  impersonate the capsule UI origin. The client-side capsule-identity health
  check mitigates lifecycle tooling but not a browser already pointed at the
  URL. Fix: on Windows set `allow_reuse_address = False` and prefer
  `SO_EXCLUSIVEADDRUSE`.
- **F5 [MED] Write endpoints mis-handle `RETURNING` statements.** Two related
  defects:
  1. Compound steps (`runtime/capsule_host.py:1242-1252`) read
     `SELECT changes()` *before* `cursor.fetchall()`. For a step with a
     `RETURNING` clause the statement is only partially stepped at that point,
     so `changed_rows` undercounts, `required_changes` enforcement judges the
     wrong number, and the change log records a wrong total.
  2. Single write endpoints with `result_mode='changes'`
     (`runtime/capsule_host.py:1203-1205` + `encode_cursor_result:1422-1423`)
     never fetch the cursor, so a `RETURNING` statement is left mid-execution
     when `SELECT changes()` runs and when `commit()` is called.
  `verify()` compiles endpoints with `EXPLAIN` and accepts `RETURNING`, so
  such capsules pass verification and then behave wrongly at runtime. Fix:
  drain the cursor immediately after each write `execute()` (before reading
  `changes()`), or reject `RETURNING` in write endpoints at verify time.
- **F6 [LOW] Doc overstates request validation: no fetch-metadata checks
  exist.** `docs/format-contract.md` ("Runtime protocol") says the loopback
  host "validates origin and fetch metadata". Grep confirms no
  `Sec-Fetch-*` handling anywhere in the repo. Host/Origin checks are present
  and adequate; either add `Sec-Fetch-Site` validation or drop the claim.
  Same doc says "unguessable route" — the Python host uses fixed
  `/__capsule/*` routes plus a session token header (equivalent protection,
  different mechanism); wording should match the implementation.
- **F7 [LOW] `looks_like_single_statement` / `statement_kind` false-positives.**
  A legitimate endpoint whose SQL contains `;` inside a string literal is
  rejected as multi-statement (`runtime/capsule_host.py:414-422`), and a
  statement starting with a `/* block comment */` is rejected because
  `statement_kind` only skips `--` line comments (`:405-411`). Both fail
  closed, so security is unaffected, but the authoring error is confusing.
- **F8 [LOW] Unhandled exceptions on `/__capsule/health` and
  `/__capsule/manifest`.** In `do_GET` these two branches call
  `capsule.manifest()` outside any try/except (`runtime/capsule_host.py:1584-1611`);
  a `CapsuleError` there produces a raw traceback + dropped connection instead
  of the JSON error the other branches return. Low likelihood (capsule is
  verified at startup) but inconsistent.
- **F9 [LOW] `read_state` hard-codes `127.0.0.1` while `start_detached` accepts
  `localhost`.** `start_detached(host="localhost", ...)` would write state that
  `read_state` (`:1788-1791`) then rejects, breaking `stop`/`status` for that
  host. Unreachable via the CLI today (main() pins `127.0.0.1`), but a latent
  trap for library callers.
- **F10 [LOW] `health()` reads the HTTP response unbounded**
  (`runtime/capsule_host.py:1875-1876`). A hostile process squatting the
  recorded port could feed gigabytes to the CLI. Local-attacker scenario, low
  priority; a `read(65536)` cap would close it.

### 3. Browser runtime (`runtime/browser/loader.js`, `worker-host.js`, `capsule-client.js`)

Overall: strong. Sandboxed srcdoc iframe without `allow-same-origin`, nonce +
source-window + strictly-increasing-id postMessage bridge, exact-shape metadata
validation, digest checks on every embedded block, bounded gzip decompression,
double-export TOCTOU guard before save. Issues found:

- **F11 [LOW] Browser verification is materially shallower than the Python
  verifier.** `worker-host.js verifyCapsule` skips media-type validation,
  cache-policy checks, case-insensitive path collisions, PK/type/NOT NULL
  shape checks, START_HERE column checks, and the permission↔endpoint
  consistency rule that Python enforces (`runtime/capsule_host.py:851-873`).
  Because endpoints cannot write `capsule_*` tables, drift through the
  sanctioned path is impossible, so this is an accepted-risk asymmetry — but
  it is not documented anywhere, and `docs/format-contract.md` implies the
  hosts enforce the same contract. Document the browser host's reduced
  verification profile, or align it.
- **F12 [LOW] Browser `executeEndpoint` does not re-check
  single-statement/statement-kind at call time.** Python re-validates on every
  call (`runtime/capsule_host.py:1140-1149`); the worker relies on init-time
  verification (`worker-host.js:537-560`) plus the authorizer. Since
  sqlite-wasm `db.exec` happily executes multi-statement SQL, the
  single-statement invariant rests entirely on the init verification pass and
  the protected-table rule. Holds today; one `looksLikeSingleStatement` call in
  `executeEndpoint` would make the invariant local.
- **F13 [LOW] Asymmetric raw-text guards in `materialiseApplication`.**
  Inline scripts are rejected if they contain `</script`
  (`loader.js:315`), but stylesheets are inlined with no `</style` guard
  (`loader.js:305-308`). A CSS asset containing `</style><script>…` changes the
  serialized srcdoc structure. No privilege escalation (the capsule already
  runs its own scripts in the same sandbox), but the symmetric check is one
  line and keeps the serialized DOM honest.
- **F14 [LOW] Dead code: `appUrl` in `loader.js` (declared `:16`, revoked
  `:560`) is never assigned.**
- **F15 [LOW] Cross-host parameter-coercion divergence.** For string inputs,
  Python `int(value)` accepts `"1_000"`, leading/trailing whitespace, and
  `"+5"` (`runtime/capsule_host.py:1342`), while the worker requires
  `/^[+-]?\d+$/` (`worker-host.js:239`). A GET read that works on the loopback
  host can fail in an HTML export. Tighten Python to the same pattern.

### 4. HTML export (`tools/capsule_html.py`)

Overall: thorough and defensive. Escaped JSON metadata, base64-only payloads,
`</script` guards on the two raw text blocks, decompression size limits,
deterministic gzip (mtime=0) enabling byte-exact `--check`, atomic output
writes, full re-verification of the embedded capsule in `verify-html`
including provenance cross-checks. Issues found:

- **F16 [LOW] TOCTOU between `read_bytes` and verification in
  `export_html`.** The embedded bytes are read at `tools/capsule_html.py:356`
  but verification runs against the live file afterwards (`:359-363`); a
  concurrent writer could get unverified bytes embedded. Local authoring tool,
  so impact is small; verifying the extracted copy (as `verify_html` already
  does) would close it.
- **F17 [LOW] `created_at` in export metadata is the manifest's
  `updated_at`** (`tools/capsule_html.py:233`), not the export time. This is
  what makes `--check` deterministic, and revisions saved in the browser use
  real time — but the field name suggests export time. Worth one sentence in
  `docs/html-export-contract.md`.

### 5. CLI + authoring tools (`tools/`)

Overall: `capsule.py` dispatch is clean; `capsule_author.py` unpack/pack/diff
is deterministic, staging-based, integrity-checked, and never replaces outputs
implicitly; `capsule_conformance.py` is genuinely independent of the runtime
verifier; `capsule_signatures.py` correctly treats inventory as
non-authentication and delegates crypto to the native verifier. The full test
suite passes (124 tests, `python -m unittest discover -s tests`). Issues found:

- **F18 [LOW] Conformance checker opens untrusted databases without the
  hardening the host applies.** `_readonly_connection`
  (`tools/capsule_conformance.py:27-32`) sets neither
  `PRAGMA trusted_schema = OFF` nor `query_only = ON`, yet the tool goes on to
  prepare `SELECT COUNT(*)` / `SELECT * FROM <view> LIMIT 0` against an
  arbitrary file. The runtime host sets both (`runtime/capsule_host.py:307`).
  The checker is explicitly for *unverified* files, so it should match.
- **F19 [LOW] `unpack`/`pack` silently renumbers implicit rowids.**
  `_read_table_rows` (`tools/capsule_author.py:123-147`) captures only declared
  columns and sorts PK-less tables by JSON; on pack, rows get fresh rowids. Any
  app table relying on implicit rowid identity round-trips incorrectly with no
  warning. Either document "app tables must declare explicit primary keys" as
  an authoring rule, or fail unpack on PK-less tables.
- **F20 [LOW] Virtual-table policy is undefined.** Pack permits
  `CREATE VIRTUAL TABLE` (`tools/capsule_author.py:302`), the verifier rejects
  triggers but says nothing about vtabs, and the format doc never mentions
  them. A capsule carrying an fts5 table verifies and runs today. If that is
  intended, one sentence in the format contract; if not, verify should reject
  what pack accepts. Pack also accepts `CREATE TRIGGER` only for the final
  verification step to reject it — an earlier error would be clearer.

### 6. Creator plugin (`plugins/capsule-creator`)

Verified during this review:

- All five embedded snapshots are byte-identical to their repo originals
  (`capsule_host.py`, `capsule-v0.2.sql`, `capsule-v0.2.conformance.json`,
  `capsule_conformance.py`, `capsule-client.js`) — the AGENTS.md sync rule
  holds today.
- Black-box test passed: copied the plugin directory to an out-of-repo
  location, ran `init` → `build` → (implicit full verify) → `conformance`;
  all succeeded with no repository access.
- `capsule_project.py` is careful: strict identifier quoting for all dynamic
  SQL, traversal-guarded doc paths, exclusive-create extraction command,
  deterministic builds with a `check` freshness command.

Issues found:

- **F21 [LOW] `_domain_sql` forbidden-pattern regexes are bypassable with
  block comments.** `CREATE/**/TRIGGER` and `CREATE/**/VIRTUAL/**/TABLE` are
  valid SQLite but don't match the `\s+`-based patterns
  (`plugins/.../scripts/capsule_project.py:673-686`). A smuggled trigger still
  dies at final verification, but a smuggled *virtual table* survives because
  neither the verifier nor the conformance checker rejects vtabs (see F20).
  The threat model is weak (an author attacking their own build), but the
  robust fix is cheap: after `executescript`, inspect `sqlite_schema` for
  trigger/vtab rows instead of (or in addition to) regexing source text.
- **F22 [LOW] The `executable` flag is assigned purely by file suffix**
  (`plugins/.../scripts/capsule_project.py:797-799`): every `.html`/`.js`/
  `.mjs`/`.py`/`.wasm` under `source/app/` is marked executable, including
  pure-content files. Since the signed-app extension signs executable
  declarations, over-marking inflates the signed executable surface. Consider
  letting projects declare non-executable overrides.

### 7. Native Windows host (sampled: `capsule-runtime/endpoint.rs`, `capsule-core/protocol.rs`, `capsule-crypto`, desktop shell excerpts)

Scope note: the native workspace is ~22k lines; this pass fully read the
endpoint boundary and child protocol, read the signing/verification core, and
spot-checked the desktop shell's asset serving and session plumbing. It did
not line-review policy, lifecycle, update, installer, or distribution crates.

Overall: the sampled code is the strongest layer in the repo. The endpoint
layer re-validates the full declaration on *every* call (statement count,
kind, parameter reconciliation) — stronger than the browser host — and its
authorizer additionally denies `Savepoint`, `Unknown`, vtable actions, and the
`load_extension()` function by name. The child protocol is an exact-grammar,
deny-unknown-fields, strictly-sequenced, replay-proof design with a bounded
request budget per session. Ed25519 signing uses a domain-separated,
length-prefixed message and key-ids bound to the exact public key. Issues:

- **F23 [MED] Same `RETURNING` family as F5, narrower window.** In
  `execute_write` (`native/crates/capsule-runtime/src/endpoint.rs:394-427`),
  `run_query` fully drains rows for `rows`/`changes` modes, but for
  `row`/`scalar` result modes it steps only the first row and drops the
  statement. A write statement with a multi-row `RETURNING` clause is left
  partially executed: remaining rows are never inserted/updated, yet the
  transaction commits, and `connection.changes()` undercounts. The Python
  host has the same gap for `row`/`scalar` writes (F5 covers its
  `changes`-mode variant). Consistent fix across hosts: drain every write
  cursor to completion, or reject `RETURNING` at verification time.
- **F24 [LOW] Session-token comparison is not constant-time.**
  `ProtocolSession::accept` compares with `!=`
  (`native/crates/capsule-core/src/protocol.rs:118`), while the Python host
  uses `compare_digest`. The renderer already holds the token, so exposure is
  theoretical; still, `subtle::ConstantTimeEq` is a one-line hardening.
- **F25 [LOW] `verify_envelope` uses `Verifier::verify`, not
  `verify_strict`** (`native/crates/capsule-crypto/src/lib.rs:177`).
  ed25519-dalek's strict variant additionally rejects small-order/mixed-order
  components. The key-id binding to exact key bytes makes practical impact
  negligible, but for a trust-decision signature path `verify_strict` is the
  conservative default.

### 8. Tests + CI

- `python -m unittest discover -s tests`: **124 tests, all pass** on this
  machine (Windows 11, Python 3.13.7).
- Coverage spread is good: host, authoring, conformance, browser logic, HTML
  export, plugin (including standalone-copy behaviour), signatures, native
  installer/release/rustsec evidence, release versioning, visual baselines.
- Standalone plugin black-box run (init → build → conformance from an
  out-of-repo copy) succeeded during this review — the "works without a
  repository checkout" claim holds.
- CI (`.github/workflows/ci.yml`) checks capsule + exports freshness, Python
  suite, Rust workspace, SBOM, and license inventory from a clean checkout;
  the release workflow adds native UI/installer qualification. No findings.

### 9. Documentation consistency

The documentation is unusually candid (limits and non-goals are stated
plainly, e.g. "not a production sandbox", "internal hashes are not publisher
authenticity"). The drift found is small and already captured above: F1
(control characters claimed rejected but not checked), F6 ("origin and fetch
metadata" / "unguessable route" wording vs. actual mechanism), F11 (browser
verification depth vs. "hosts require the exact same identity" phrasing),
F17 (`created_at` naming), F20 (virtual-table policy unstated). One addition:

- **F26 [LOW] `docs/security.md` "Origin and CSRF controls" is listed under
  "Additional production controls" as future work, while
  `docs/format-contract.md` presents origin/fetch-metadata validation as
  present-tense behaviour.** The two documents should agree on which
  controls exist today (Host/Origin checks + session token do exist;
  Sec-Fetch and random route prefix do not).

## Summary

**Verdict: this is a high-quality, security-literate codebase.** The
architecture consistently enforces its stated invariants at multiple layers
(verify-then-trust, named endpoints only, protected platform tables,
loopback-only, default-deny CSP, bounded everything), the three hosts agree on
the format identity, the plugin snapshot is in sync, and all 124 tests pass.
Nothing found rises to HIGH severity; there is no remotely exploitable issue
in the reviewed code under the project's stated threat model.

The three findings worth fixing first:

1. **F4** — Windows `SO_REUSEADDR` port hijack exposure in the loopback host
   (one-line fix; real cross-user risk on shared machines).
2. **F5 + F23** — `RETURNING` statements in write endpoints mis-count changes
   and can partially execute across Python and native hosts (silent
   data-integrity hazard; either drain cursors or reject `RETURNING` at
   verify time in all hosts).
3. **F1/F20** — close the small contract gaps (control-character paths,
   virtual-table policy) so the format documentation matches what verifiers
   actually enforce.

Everything else is polish: cross-host coercion parity (F15), symmetric
raw-text guards (F13), doc wording (F6, F11, F17, F26), and defensive
hardening in tools (F18, F21).

Findings count: 26 total — 3 MED (F4, F5, F23), 23 LOW.

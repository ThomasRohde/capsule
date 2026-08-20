# Architecture

## 1. System context

A SQLite capsule is a passive application artefact. It becomes an interactive application when opened by a compatible **capsule host**.

```text
┌───────────────────────────────┐
│ Coding agent / human operator │
└───────────────┬───────────────┘
                │ inspect, verify, launch
┌───────────────▼───────────────┐
│ Trusted generic capsule host  │
│ - file lifecycle              │
│ - integrity checks            │
│ - constrained HTTP bridge     │
│ - asset delivery and CSP      │
└───────────────┬───────────────┘
                │ SQLite transactions
┌───────────────▼───────────────┐
│ .capsule.sqlite               │
│ - manifest                    │
│ - runbooks and docs           │
│ - app assets                  │
│ - named endpoints             │
│ - domain schema and data      │
│ - views/scenes/checks         │
└───────────────────────────────┘
```

The bootstrap host is a small Python standard-library process bound to loopback. It serves application assets directly from the database and exposes only named, parameterised endpoints declared by the capsule.

The architecture is not committed to Python. Future hosts may use native SQLite, SQLite WASM, a desktop shell, or a browser-packaged runtime as long as they honour the same format and security contract.

## 2. Separation of concerns

### Vision layer

Defines the product thesis and durable principles. It must not depend on Diagram Studio or the current Python host.

### Format layer

Defines the minimum tables, fields, invariants, discovery protocol, endpoint semantics, validation, and versioning rules a capsule host can rely on.

### Host layer

Provides trusted mechanics:

- opening the file;
- enforcing trust and permissions;
- verifying internal asset hashes and structural checks;
- serving embedded assets;
- applying a restrictive Content Security Policy;
- validating endpoint parameters;
- executing one named operation in a transaction;
- returning structured JSON;
- exposing requested capabilities and host-managed grant decisions;
- managing loopback process lifecycle.

It does not render diagrams or know domain table names.

### Capsule application layer

Supplies:

- HTML, CSS, JavaScript, icons, templates, and optional bootstrap tools;
- domain schema and data;
- named endpoint declarations;
- views, saved scenes, presentation paths, and application settings;
- application-specific validation and documentation.

### Example layer

Diagram Studio defines diagram nodes, edges, scenes, its UI, and sample content. None of these concepts belong in the generic host.

### Trusted lifecycle workspace

Native lifecycle work is isolated in the product-independent
`sqlite-capsule-workspace` service. It accepts only pinned, read-only v0.3
capsules that pass exhaustive conformance, declared checks, integrity, foreign
keys, signed data-contract validation, and same-snapshot signature validation.
Mutable lineage is reported as useful but unauthenticated provenance.

Planning produces deterministic canonical `lifecycle-plan/1` review JSON; that
JSON is never execution authority. Execution reparses it into a non-serializable
prepared plan, reopens and rebinds every exact source, retains a one-use held
destination-parent capability, writes only owner-private create-new staging,
and publishes with a platform no-replace primitive. M02 introduced exact
duplication; M04 adds separate exact-snapshot and compact-logical typestates.
M04 also adds signed-v0.3 semantic fork, authenticated template creation and
selective fork. Each uses a non-serializable held review and repeats its
operation-specific dataset, mutable-platform, identity, signature, lineage,
scrubbing and post-publication proofs.

M04 separates duplicate-only admission from semantic workspace admission.
`VerifiedCopySource` can retain an exhaustively verified signed or unsigned
v0.2/v0.3 snapshot for byte-exact or compact duplication, while semantic
fork/template work continues to require the signed v0.3 data contract. A clean
template additionally requires a signed `template-state/1` document whose
dataset-state digests are reproduced against the exact retained snapshot;
application signatures do not authenticate ordinary seed rows on their own.
Preview reports remain review data and grant no filesystem or execution
authority.

The trusted `main` shell receives only opaque selection, destination,
dataset-choice, plan and operation authority identifiers plus bounded host-owned
display labels for the selected leaf and a generic local-folder description.
Filesystem paths remain in Rust with the verified sources, canonical plan and
held destination authority. Exact and compact support v0.2
or v0.3 with no invalid signature envelope. Semantic modes require a complete
valid signed-v0.3 inventory. Signed `copy` cannot be weakened, `forbid` rejects
the operation, sensitive inclusion is explicit, and the one-source initial
profile rejects fork/selective `reset` until a separate clean source can supply
that authority.

Compact duplication copies only the retained verified private snapshot into
owner-private staging, runs in-place `VACUUM` there, and never points SQLite at
the final destination. Before and after publication it reproduces the complete
`compact-logical-state/1` digest, exhaustive capsule verification and signature
state; it also requires unchanged page size, DELETE journal mode, no sidecars
and zero freelist pages. Implicit rowid participates whenever SQL can observe
it, so a VACUUM that renumbers a live row fails closed instead of being called
logically equivalent.

### Trusted Capsule Cabinet

The Windows shell is also a host-owned Capsule Cabinet. Selecting a path mints
an opaque process-local selection, captures one private verified snapshot, and
projects a bounded `org.sqlite-capsule.tauri-overview/1` model from that same
snapshot. Application-release metadata, cryptographic signature state,
host-local publisher trust, mutable self-described instance metadata, and file
state remain separate fields and separate visual groups.

Inspection is non-mutating and non-activating. It neither performs rollback-
journal recovery nor releases the raw renderer, even when an exact prior
authorization is remembered. Recovery and application activation are explicit
trusted-shell actions that rebind and reinspect the selected source. The raw
application window remains hidden at the locked host probe until activation.

Recent cards live in a separate owner-protected, bounded, versioned and
rebuildable `cabinet-v1` store. They contain opaque recent identifiers and
last-observed display hints, never trust decisions, grants, capsule bytes, or
execution authority. Opening a recent resolves its path only inside Rust and
performs fresh pinned inspection.

M05 comparison retains two separately pinned, exhaustively verified signed
v0.3 snapshots under one deadline and never activates either application. The
workspace first emits a deterministic bounded summary across identity, mutable
lineage claims, signed application, structural schema and signed-contract
datasets. Row and field pages exist only for `row`/`field` policy, use consumed
in-memory cursors and are selected in the trusted shell through random opaque
dataset/table/page tokens; JavaScript never supplies paths, SQLite identifiers,
ordering expressions or SQL. Sensitive datasets remain counts-only until an
explicit session-bound trusted-shell reveal. BLOB bytes are never rendered.
The application layer expands only after another explicit trusted-shell action
and contains thirteen fixed, value-free digest families bound to the original
report and both retained file digests; callers cannot select tables or columns.
Lineage parent evidence means only that a mutable claim names the exact digest
of the other retained source; shared third-parent hashes stay labelled mutable,
untrusted claims and never inherit publisher trust.

M06 reconciliation consumes those retained, non-truncated Compare authorities
and always derives a new output from the target. Two-way review applies only
explicitly selected insert, delete, replace-row or set-fields actions. Optional
three-way review requires a separately selected, pinned and fully verified
ancestor with the exact same signed application, data schema and data contract;
lineage claims alone never supply ancestor authority. The classifier distinguishes
insert/insert, update/update, delete/update and immutable-field conflicts.
Every conflict must receive one closed keep-target or allowed take-source
resolution before the ordinary reconciliation typestate can be prepared.

The canonical `reconcile-payload/1` is value-free review evidence bound by one
`lifecycle-plan/1` decision. It carries typed row/value precondition digests,
complete signature inventories, expected dataset states, exactly two lineage
parents and optional bounded ancestor evidence, but no row keys, row values,
SQL, path or destination authority. Execution retains the exact verified
inputs and held destination parent in memory, re-computes Compare, rechecks
every precondition, transforms an owner-private copy of the target in one
transaction, preserves the target capsule/application/signature compartments,
mints only a new revision and publishes create-new after exhaustive validation.
The trusted shell projects only opaque orientation, selection, ancestor,
conflict, resolution, destination, confirmation and operation tokens.

## 3. Capsule anatomy

A capsule uses ordinary SQLite tables divided conceptually into two namespaces.

### Platform tables

Platform tables use the `capsule_` prefix and describe how the artefact is opened and governed:

```text
capsule_manifest
capsule_asset
capsule_runbook
capsule_command
capsule_doc
capsule_endpoint
capsule_endpoint_step  (v0.2)
capsule_check
capsule_prompt
capsule_change_log
```

The `START_HERE` view exposes the agent runbook with an obvious name for discovery through `.tables` or `sqlite_master`, and expands referenced command templates so the launch path can be recovered in one query.

### Domain tables

Domain tables are owned by the application. Diagram Studio currently uses:

```text
diagram_document
diagram_layer
diagram_node
diagram_edge
diagram_group
diagram_group_member
diagram_scene
diagram_scene_override
diagram_history
diagram_operation
```

A future recipe capsule, game, fieldbook, or notebook would define a different domain schema while retaining the platform tables.

## 4. Launch sequence

### Agent-assisted launch

1. The agent locates the `.capsule.sqlite` file.
2. It lists tables or invokes the repository CLI.
3. It reads `START_HERE`, `capsule_manifest`, and relevant `capsule_command` rows.
4. It performs read-only structural, integrity, declared-check, and signature
   verification over one exact opened database connection; source bytes are
   hashed before and after so evidence from different files cannot be mixed.
5. It decides whether the publisher/file is trusted. Repository-owned test capsules may be explicitly trusted; unrelated capsules require human approval.
6. It starts a compatible host using the command embedded for that capsule version.
7. The host binds to `127.0.0.1`, verifies the capsule again, and reports a local URL.
8. The agent checks the health endpoint and reports the URL.

### Browser application launch

1. The native host first presents its bounded trusted Overview without reading
   or releasing executable application assets.
2. After an explicit trusted-shell action, the host reads `entry_asset` from
   the already reviewed release and rebinds the selected source.
3. It serves that asset at `/` and other assets by relative path.
4. It applies a network-denying Content Security Policy.
5. The app requests `/__capsule/manifest` and named read endpoints.
6. User edits invoke named write endpoints.
7. The host validates types and allowed parameters, binds values, executes the
   named endpoint's one statement or bounded v0.2 step sequence in one
   transaction, and reports affected rows.
8. The same database file now contains the new application state.

## 5. Named endpoint bridge

The bootstrap deliberately does not expose raw SQL to browser JavaScript.

Each `capsule_endpoint` row declares:

- a stable endpoint name;
- read or write operation;
- one SQL statement (also the first step when v0.2 steps are present);
- an explicit parameter schema;
- a result mode;
- a human-readable purpose.

Example:

```sql
UPDATE diagram_node
SET x = :x,
    y = :y,
    updated_at = CURRENT_TIMESTAMP
WHERE id = :id;
```

The parameter contract might be:

```json
{
  "id": {"type": "string", "required": true},
  "x": {"type": "number", "required": true},
  "y": {"type": "number", "required": true}
}
```

The host rejects missing parameters, unknown parameters, type mismatches, multi-statement SQL, writes through read endpoints, and mutations of protected platform tables.

V0.2 adds `capsule_endpoint_step` for product-independent atomic commands. A
compound write has two to sixteen ordered single-statement steps, one exact
parameter contract, optional row-count preconditions, one authorisation context,
one transaction, and one change-log record. Rollback is all-or-nothing. The
browser still names a capability and cannot supply SQL; semantic inverses remain
application data rather than generic host logic. See [ADR
0012](decisions/0012-atomic-named-commands.md).

V0.3 keeps the named-endpoint boundary but separates application-release and
capsule-instance identity. After endpoint SQL succeeds, the host generates a
new UUIDv4 revision, updates the instance timestamp, and appends its change-log
entry inside the same transaction. Rollback preserves both the domain state and
the previous revision identity.

This bridge is intentionally narrow. It is sufficient for a convincing bootstrap
and creates a stable place to evolve richer capabilities later.

## 6. App assets

`capsule_asset` stores paths, media types, bytes, hashes, and an executable flag. Assets are addressed as a virtual file tree, for example:

```text
app/index.html
app/styles.css
app/app.js
bootstrap/capsule_host.py
```

The host serves only assets in this table. It never maps browser paths to arbitrary local files.

The embedded `bootstrap/capsule_host.py` enables an agent to launch a capsule even when it receives only the database. The agent uses Python's built-in SQLite module to extract that asset, then runs it against the original file. The bootstrap source is therefore versioned with the application, while a separately installed newer host may still be preferred.

## 7. Documentation and runbooks as data

`capsule_doc` contains durable, version-specific explanation of the application and its data model.

`capsule_runbook` contains ordered instructions for audiences such as `agent`, `human`, and `runtime`.

`capsule_command` contains command templates, platform applicability, risk classification, and success conditions. Commands may use placeholders such as `{capsule}` and `{repo}`. Agents should present substituted commands before performing anything beyond the capsule's declared trust scope.

The current format stores nullable structured argument vectors. They are the preferred machine form because placeholders remain whole process arguments instead of being interpolated into shell source. Display templates remain useful for humans and composite development gates.

This design makes setup instructions part of the artefact rather than an external README that can drift.

## 8. Validation model

Validation has three explicit phases: non-executing structural/conformance
verification, bounded read-only declared checks, and trust/policy evaluation.
The native launch and signing paths hold one read-only connection across the
first two phases and bind the result to before/after source hashes. No phase
silently substitutes for another.

### Host structural checks

The host verifies:

- full SQLite integrity and foreign keys;
- expected platform tables, runtime columns, keys, `user_version`, and the exact discovery-view shape;
- exactly one manifest;
- known format version;
- a valid entry asset;
- asset path safety and SHA-256 matches;
- valid endpoint metadata, exact SQL placeholders, typed parameter schemas, trigger absence, and successful compilation under the runtime authoriser;
- no disallowed endpoint operations.

### Capsule-declared checks

`capsule_check` rows contain read-only queries and expected results. They validate application-level invariants such as orphan-free edges or a required default diagram.

### Test-suite checks

Repository tests rebuild the capsule, inspect it, call read and write endpoints, exercise the HTTP bridge, and verify that mutations persist.

## 9. Trust model

The capsule contains executable browser assets and SQL declarations. Internal hashes detect corruption but cannot establish who authored the file because an attacker can change both content and hash.

The bootstrap therefore requires explicit `--trust-capsule` before serving
executable assets. The optional signed-app extension now authenticates an exact
immutable application compartment while allowing ordinary domain writes, but a
valid signature alone releases nothing. The native topology now evaluates the
shared signed/structural launch evidence through a protected host-local SQLite
trust store and an exact capability intersection. The store is outside the
capsule, is never attached to capsule SQL, and scopes persistent grants to the
capsule ID, application ID, and signed application digest. The trusted desktop
shell owns the first-open prompt and its allow-once/exact-release/deny/cancel
commands; the raw child renderer has no Tauri initialization or trust command.
That raw renderer is mounted in a separate host-owned native window which stays
hidden until authorisation and opens maximized for application use; the trusted
shell remains the only Tauri WebView.
The shell's separate publisher-signing page is also host-owned. It can import a
bounded use-once Ed25519 key, verify a source, prepare an exact application
digest, and sign only a new copy through narrow Rust commands. Key paths and
private bytes never cross into JavaScript, signing never targets the active raw
renderer, and the result remains untrusted until the ordinary host-local trust
policy evaluates it.
The same trusted shell can forget only the exact current file/release decision
after literal confirmation. That transaction removes current-digest grants,
retains publisher and revocation state plus other capsules and audit history,
records its own audit event, closes any active runtime, and re-evaluates to a
prompt without granting authority.
The native runtime now consumes the resulting `executable_allowed` gate: it
re-inspects the capsule, verifies conformance and declared checks, opens SQLite
with the granted read/write mode, and only then serves verified assets and the
host-neutral named-endpoint API through a session-bound custom protocol. The
Python bootstrap retains its explicit local-trust switch and can inventory or
delegate signature verification without silently changing that policy. See
[ADR 0016](decisions/0016-host-local-trust-and-capability-policy.md) and
[ADR 0017](decisions/0017-native-runtime-and-custom-protocol.md).

Native host distribution policy is a separate offline core. Strict signed
release manifests first produce a reviewed newer exact-target candidate without
implying consent. A separate authorization step consumes that candidate only
after consent, quiescence, and verified backup; strict signed revocation
bundles atomically populate the
protected last-known-good store and continue to block known entries when stale.
The policy core has no downloader/installer API and is unreachable from the raw
capsule renderer. A separate host-only staging core accepts only already
verified artifact and Sigstore bytes, preserves the exact signed release
envelope and prior installer under an owner-protected directory, and records
prepared, installer-started, awaiting-health, healthy, rollback-required,
rollback-started, awaiting-rollback-health, rolled-back, or rollback-failed as
immutable durable transitions.
Crash-left markers are quarantined; a version mismatch cannot record startup
health, and rollback exposes only the exact inventoried prior installer. It
still does not fetch bytes or execute installers. A separate host-only
`capsule-installer` coordinator consumes only the freshly rebound prepared
value. On Windows it reverifies and retains native no-replacement guards for
both the candidate and preserved prior installer, requires both to match the
same signed Authenticode identity, requires signed package ProductVersion to
match the signed release or preserved rollback version, records installer-started before an audited
`ShellExecuteExW` handoff, and records awaiting-health or rollback-required
from that immediate launch result. MSI packages run through system
`msiexec.exe`; NSIS packages run directly. The guards remain alive across the
operating-system handoff. Once the bundled trusted UI
has loaded, its top-level-only Tauri command reconciles one in-flight stage:
exact-version awaiting-health becomes healthy, while interrupted installer
start or a version mismatch becomes rollback-required. A separate exact-stage
rollback boundary repeats capsule backup/quiescence, native signature and exact-
byte checks, records rollback-started before the same audited platform handoff,
then awaits startup under the preserved version. Exact prior-version startup
records rolled-back; a rejected installer or wrong-version startup records a
terminal rollback failure. The raw child has no route to either command. Before installer preparation, the stored envelope is
reverified under the compiled release root and rebound to every staged field;
older stages without that evidence fail closed. The pinned Tauri updater 2.10.1 backend is initialized
only in the trusted Rust host. Production-like builds must compile a complete
credential-free HTTPS endpoint, Minisign updater key, Ed25519 release-policy
root, and current release sequence; absent inputs visibly disable transport,
and partial or unsafe inputs fail before UI release. The bundled shell may
explicitly request a concurrency-bounded metadata check. It treats the Tauri
announcement as untrusted until an embedded strict `sqlite_capsule` wrapper
contains a valid signed release manifest matching the newer sequence/version,
exact native target and artifact URL, compiled origin, and expected platform-
signing class. Release profile 0.2 additionally signs the exact native signer
identity, whether trusted timestamp evidence is mandatory, the Sigstore
certificate identity, and its OIDC issuer. A same-origin Sigstore-evidence URL
is also required. A separate
explicit user action may download bounded bytes into host memory with at most
five same-origin redirects, a 30-second timeout, Tauri-compatible Minisign
verification, and exact release/Sigstore digest checks. On Windows the exact
package is then materialised through a create-new owner-protected temporary
file, checked offline with `WinVerifyTrust`, bound to the signed leaf-certificate
SHA-256 and required countersignature, and deleted before the bytes return to
host memory. A held read handle denies write/delete sharing and the adapter
requires the same length/hash before and after native verification. The bounded
Sigstore bundle then passes an offline embedded-root verification of the
artifact signature, Fulcio chain, SCT, Rekor evidence, integrated time, and
exact certificate identity/OIDC issuer. Only a non-constructible accepted-
download type can bind both reports to the same selected bytes. Wrong pins,
missing signatures/timestamps, unavailable cached revocation evidence, Sigstore
failure, unsafe suffixes, and cleanup failures all block the download.
macOS/Linux platform adapters remain required before cross-platform acceptance;
the trusted shell now exposes a separate install-intent action that establishes
rollback readiness before backup/quiescence, mints the installable state, and
persists the exact verified bytes without executing them. Rollback readiness
prefers one prior healthy stage whose historical signed envelope exactly matches
the running version, compiled release sequence/root, native target, and update
origin; the current package is then copied into the new stage. Envelope expiry
is not reused as download authorization, but signature and exact release identity
remain mandatory. When no healthy stage exists, the Windows installers provide
the bootstrap source: NSIS copies its exact `$EXEPATH`, while the MSI WiX fragment
copies the Windows Installer `OriginalDatabase`, into one fixed private cache
name. The stager locks that file, repeats native signature, timestamp, signer,
and exact-byte verification, and requires signed package version metadata to
equal the running host before capsule preflight. The Windows install and rollback
execution commands are registered only as host orchestration boundaries and deliberately omitted
from the bundled WebView capability until live signed-package and clean-machine
acceptance are complete. The WiX bundle explicitly permits the verified downgrade
needed by rollback; local compilation and fake-launch tests are not live acceptance.
No `updater:*` guest permission is granted, and the raw child still has no Tauri
IPC channel. A read-only GitHub
Actions matrix is configured for Windows
x86-64, Linux x86-64, and macOS ARM64/x86-64. It runs compatibility and native
gates, creates only visibly unsigned development bundles, and emits a
deterministic artifact/SBOM/license evidence document. The collector refuses
symlinked artifacts, wrong-platform bundles, or a signed-release claim; actual
remote-run and signed-release evidence remain separate gates. See [ADR
0019](decisions/0019-signed-host-updates-and-revocations.md).

Detached lifecycle state is private to the local user, never exposes its shutdown token in normal output, and is accepted only when its loopback URL, capsule path, identity, and runtime protocol match. The default temporary state directory is scoped by numeric user ID on POSIX and by a versioned, path-safe fingerprint of the actual process-token principal on Windows. The Windows identity comes from `GetUserNameExW`, not inherited `USERNAME` or `USERDOMAIN` variables, so a sandbox token cannot poison the normal user's state directory even when it inherits the user's environment. `SQLITE_CAPSULE_STATE_DIR` remains an explicit override. Lifecycle HTTP requests do not follow redirects.

Embedded agent instructions are also untrusted input. An agent may inspect them read-only, but should not execute commands from an unfamiliar capsule without approval. This is both a software-supply-chain issue and a prompt-injection issue.

See [`security.md`](security.md).

## 10. Persistence and concurrency

The bootstrap uses one local host process and ordinary SQLite transactions. It enables foreign keys, a busy timeout, and trusted-schema restrictions. Multiple browser tabs may contend for one connection; the current host serialises database access with a process-local lock.

The native host additionally retains a canonical source-file identity for the
session. Windows denies delete sharing so replacement is blocked; POSIX keeps
the inode open and rechecks the path. A crash-releasing named mutex or `flock`
allows one native host writer, with a second host falling back to read-only.
The runtime captures SQLite `data_version` and the latest change-log position,
and closes its child session when an external commit or replacement makes the
snapshot stale.

Before the first native named write, SQLite's online backup API creates an
owner-protected backup outside the source directory. The host verifies its
identity, signed application digest, complete conformance, declared checks,
byte length, and SHA-256 before allowing the write, then stores a protected
inventory record. Restore uses the same checks and writes only to a new path.
Dirty runtimes checkpoint before accepting the eleventh named write and on clean
close; retention is bounded by count, bytes, and age while preserving one
verified copy. The
host-update preflight is stricter than an ordinary dirty checkpoint: every
writable session must have a verified current-state recovery point, including a
newly opened session with no writes. A dirty session creates a fresh backup; a
clean session may reuse its current verified backup. Only after that succeeds
does the trusted shell release the runtime, writer lease, protocol token, and
raw child-renderer session. Read-only sessions have no mutable capsule state to
back up. The internal install-preparation and Windows execution commands
additionally require an exact prepared stage and distinct literal confirmations,
and remain absent from the trusted WebView capability until installer
orchestration is release-ready. The
trusted shell owns open/restore selection, secondary-instance forwarding, and
explicit reopen/read-only/restore conflict choices. When a rollback journal
prevents read-only inspection, a header-only capsule probe, pinned identity, and
the host writer lease permit SQLite itself to recover it; the host then repeats
integrity, signature, policy, and capsule verification before releasing assets.
In-progress markers make crash-left backup and restore artefacts visible and
non-recoverable. Test-only child-process fault points cover all durable backup,
checkpoint, close, update-preflight, and restore boundaries. A separately
guarded debug-host acceptance path exercises real Windows process termination
during open, pre-write backup, bounded checkpoint, close, update preflight, and
restore without compiling the controls into release executables. Five additional
host-update stages cover artifact sync through startup-health pending. Signed-
installer registration and platform acceptance remain open. See
[ADR 0018](decisions/0018-native-file-lifecycle-and-recovery.md) and
[ADR 0019](decisions/0019-signed-host-updates-and-revocations.md).

Host-owned support export collects launch, lifecycle, update, and redacted trust
evidence on a fixed 8 MiB worker and serialises/syncs the selected create-new JSON
on a second fixed-stack worker. Profile `org.sqlite-capsule.support-bundle/0.2`
marks capsule-controlled text as untrusted data, makes the host-owned severity
boundary explicit, declares protected byte categories absent, and recursively
removes known selected-file and host-state paths before writing. Capsule bytes,
trust-store bytes, selected-file contents, shutdown tokens, and private keys are
not inputs to the bundle.

Both hosts bound capsule size, asset size, request bodies, encoded endpoint
results, and application concurrency. The browser shell admits at most eight
concurrent named-endpoint requests and its worker serialises accepted operations;
the Python host bounds concurrent HTTP requests. These limits are generic policy,
not Diagram Studio assumptions; failures are explicit and fail closed.

Additional topologies may add:

- desktop/native hosts can use WAL where appropriate;
- browser-only hosts may add OPFS recovery as an optional cache, never as the portable file;
- concurrency and collaboration should be treated as separate optional protocols, not hidden assumptions of the file format;
- every saved file must remain a complete standalone artefact.

## 11. Self-contained HTML export

The database-primary architecture now supports a Bento-like distribution mode without abandoning SQLite as canonical source. The normative derivative contract is [`html-export-contract.md`](html-export-contract.md), with the accepted host choice in [ADR 0013](decisions/0013-browser-host-and-html-export.md).

The exporter creates one HTML file containing:

```text
verified bootloader and sandbox shell
compressed pinned SQLite WASM host runtime
compressed SQLite database bytes and component digests
manifest and title metadata
third-party notices and provenance
```

On open, the host validates the envelope, starts a dedicated SQLite WASM worker,
imports and verifies the database, and only then materialises the capsule entry
asset into a sandboxed `srcdoc` child. The child receives the same host-neutral
manifest/permissions/named-endpoint client as the loopback topology. It never
receives SQLite, SQL, raw database bytes, or the private serialisation channel.

The embedded, digest-checked upstream SQLite ES module is converted by an exact,
shape-checked rewrite into a classic blob worker at boot. This preserves the
pinned reviewed bytes while avoiding module-worker CORS loading on opaque
`file://` origins. A changed upstream module shape fails before worker launch.

The required working database is in memory, so `file://` and static hosting do
not depend on OPFS, service workers, cross-origin isolation, or headers. An
explicit editable save re-verifies and serialises the database, then uses a
user-picked file handle when supported or downloads a complete next HTML
revision. The source capsule remains unchanged.

Export profiles distinguish:

- **view** — verified database reads and a presentation/reader surface;
- **interactive** — pan, zoom, inspect, scene navigation, and allowed downloads, no durable writes;
- **editable HTML** — full local editor and database writeback;
- **source capsule** — original `.capsule.sqlite`.

## 12. Format evolution

The manifest identifies both a capsule format version and an app version. Host compatibility must be explicit.

Rules:

- additive platform changes are preferred;
- existing column semantics must not silently change;
- a future format may store migrations as data and apply them transactionally through a compatible host;
- old capsules remain openable in read-only mode when a host cannot safely migrate them;
- exports record their source capsule identity and version;
- the generic schema should remain small enough to implement in multiple hosts.

## 13. Authoring lifecycle

The repository uses a pack/build process for reviewability:

```text
reviewable source files -> build_example.py -> capsule.sqlite
```

At runtime:

```text
capsule.sqlite -> edits through named endpoints -> same capsule.sqlite
```

Generic `unpack`, `pack`, and semantic `diff` tools now round-trip runtime state through a deterministic authoring bundle without knowing application table names. Application-code changes are still made in the example source tree and rebuilt; runtime content edits can be unpacked and reconciled deliberately.

The checked-in release capsule is a clean deterministic build. `build_example.py --check` independently rebuilds it and compares the byte digest, preventing a stale embedded host or ad hoc runtime edit from becoming an accidental release.

The versioned independent conformance descriptions at
`format/capsule-v0.2.conformance.json` and
`format/capsule-v0.3.conformance.json` are intentionally separate from the
Python host verifier so a second implementation can check platform structure
without importing host code. Both runtime and conformance reports are required
evidence; neither is a
publisher signature.

## 14. Deployment topologies

### Bootstrap topology

```text
Python host + local SQLite file + browser
```

### Desktop topology

```text
small signed desktop host + associated .capsule.sqlite files
```

### Browser-only topology

```text
single HTML + SQLite WASM + embedded DB + optional OPFS working copy
```

### Agent topology

```text
coding agent + ordinary SQLite/Python tools + embedded runbook + host
```

All topologies should converge on the same core database contract rather than forking the document model.

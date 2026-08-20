# Native host contract

Status: current product-independent launch/runtime contract. Signature,
host-local trust, recovery, and the implemented update controls are normative.

## Trust surfaces

The desktop host has three deliberately separate surfaces:

1. Rust core: file identity, verification, trust and grant policy, named
   endpoints, backup and recovery.
2. Trusted shell: bundled Tauri UI for identity, publisher, grants, recovery,
   update consent, and close controls.
3. Application renderer: raw Wry WebView attached directly to a dedicated
   host-owned native application window, with no Tauri initialization and no
   Wry IPC handler.

The verifier dispatches the complete format tuple for v0.2 or v0.3. Before any
application asset, signing preview, or runtime session is released, the same
read-only connection must pass exhaustive machine conformance, integrity and
foreign-key checks, asset and endpoint validation, bounded declared checks, and
profile-specific signature evidence. Before/after source hashes detect path
replacement or mutation; signing additionally binds and re-verifies the exact
private snapshot before adding signature rows.

Launch/signing inputs must be standalone SQLite main databases: adjacent WAL,
SHM, or rollback-journal state and a WAL-mode database header are rejected
without opening the source through SQLite. The verifier raw-copies the pinned
main bytes into private create-new storage, compares before/snapshot/after
SHA-256, and runs every logical phase only on that held private snapshot.
Signing stages bytes from that verified snapshot rather than reopening the live
source path. A writable runtime starts a rollback-mode read transaction before
logical verification and rebinds the main-file digest while that transaction
prevents a concurrent rollback-journal commit.

The application window is created hidden and remains hidden while trust or
capability decisions are unresolved. Once execution is authorised, the host
shows it maximized and the raw renderer fills its client area. Rejection,
replacement, conflict, or trust reset hides it and returns focus to the trusted
shell. Capsule content cannot draw outside its own native window or control
either host window. It never receives a filesystem path, SQLite connection, SQL
text, database bytes, trust-store handle, backup handle, updater handle, Tauri
command, or generic native message callback.

The bundled top-level UI may receive host-owned reports through Tauri's exact
event listen/unlisten permissions. It has no generic event emit permission. The
raw child is not a Tauri capability target, so it cannot listen for or emit
native events. Secondary-instance inspection runs outside the synchronous
platform message callback; only the completed bounded report is published back
to the trusted shell on the main thread.

M04 adds an opaque create-copy controller only to the trusted `main` Tauri
WebView. The renderer supplies a current Overview selection ID, closed copy
mode, opaque dataset-choice tokens and a one-use confirmation token; Rust keeps
the source path, verified snapshot, destination path/held parent, plan and
publication capability private. Switching selections invalidates pending
authority. Progress is targeted only to `main`, and the raw Wry application
renderer still has no IPC or lifecycle event access. Narrow diagnostic CLIs emit
bounded overview, signed data-contract, redacted lineage, and deterministic
plan-review JSON. Their root response profiles are respectively
`org.sqlite-capsule.workspace-overview-response/1`,
`org.sqlite-capsule.workspace-data-contract-response/1`, and
`org.sqlite-capsule.workspace-lineage-response/1`; a serialized plan cannot
hold a filesystem capability or authorize publication.

Exact and compact duplicates support verified v0.2/v0.3 signed or unsigned
sources. Fork, authenticated template creation and selective fork require a
complete valid signed-v0.3 signature inventory and signed data contract.
Semantic actions are re-derived throughout execution; template-state is
reproduced from the same retained snapshot, `forbid` fails closed, sensitive
state defaults out, dependencies and restrictive cross-dataset FKs are
enforced, and the private output is compacted before no-replace publication.

M05 compare commands are registered only for the trusted `main` WebView. The
shell submits the current opaque Overview selection and a host-picker-minted
candidate; Rust retains both verified sources, numeric contract positions and
continuation cursors. Responses contain bounded summary projections and random
dataset/table/page tokens, never source paths, raw SQL, arbitrary identifiers or
unbounded values. `ignore`/`summary` policies never yield detail pages,
`row` never yields field values, sensitive datasets require an explicit reveal,
and BLOB content is always length/hash only. Application expansion is a
separate explicit command over thirteen fixed host-owned families; it returns
only bounded counts and digests bound to the reviewed pair, never compartment
values. Closing or switching the selected Capsule releases the retained pair
and tokens. The raw Wry renderer has no compare handler, event or capability.

M06 reconcile commands are likewise registered only for `main`. The shell can
orient the retained pair explicitly, select only row actions previously
disclosed by Compare, and optionally use a host-picker-minted ancestor token.
Rust holds the source, target, optional ancestor, comparison rows, primary keys,
row values, canonical plan/payload and destination path. Three-way response
objects expose bounded identity/count projections plus random conflict and
resolution tokens; JavaScript never supplies a conflict digest or resolution
vocabulary directly. `prepare_reconcile` accepts either closed two-way
selection tokens or one ancestor token plus an exhaustive set of resolution
tokens, never both.

The reconciliation core reopens no renderer-supplied path. It consumes pinned
verified inputs, re-computes the exact source/target comparison, checks signed
manual/three-way policy and immutable fields, and turns the reviewed selection
into a one-use in-memory capability. Execution copies the target to private
staging, applies only the bound operations transactionally, preserves target
capsule/application/signature identity, creates a new revision and two-parent
lineage, then verifies and publishes create-new. Progress is sequenced and
targeted only to `main`; cancellation is allowed only before publication. The
raw Wry renderer has no reconcile handler, progress listener or capability.

M07 upgrade commands are registered only for `main`. A release picker retains
the target path and returns an opaque candidate token; a destination picker
returns a separate opaque create-new token. Rust reopens no renderer-supplied
path. Preparation returns a bounded, path-free candidate/review projection and
a one-use nonce bound to the current Overview selection, exact working/target
digests, accepted publisher key, capability delta and destination. Capability
additions or changes require their own closed confirmation. Switching or
closing the selection invalidates pending authority.

The core executes the retained same-schema review in the background through
prepared, staging, validated and published typestates. It starts from the clean
target release, preserves target application/signatures and source mutable
identity/data according to the target-signed policy, emits sequenced bounded
progress only to `main`, and allows cancellation only before publication.
Terminal results expose identities and digests, never paths or data values. The
raw Wry renderer has no upgrade handler, progress listener, token or capability.

Create-new publication walks the destination parent component-by-component
without following reparse points or symbolic links, retains its stable parent
handle, creates owner-only staging on the same filesystem, and binds the held
staged object by identity, length, and SHA-256. The workspace verifies the exact
held output before and after the no-replace publish, rebinds all inputs, and
returns success only after the final leaf still names that exact object.
Test-only child-process crash points cover private creation, exact snapshot
copy, sealed verification, and the post-rename reopen boundary. Each stage
proves the input digest unchanged and refuses incomplete final-name success.

The trusted shell exposes the M03 Cabinet/Overview surface only to the exact
`main` Tauri WebView. Picker and recent-card actions mint or resolve opaque
host-owned selection identifiers; no command accepts a capsule path supplied by
JavaScript. The bounded `org.sqlite-capsule.tauri-overview/1` response separates
structural profile, cryptographic signature evidence, local publisher
trust/revocation, mutable self-described instance metadata, and last-observed
file state. It omits entry assets, permission declarations, endpoint SQL,
capsule asset identifiers, and filesystem capabilities.

Overview inspection uses one retained private snapshot and is both non-mutating
and non-activating. A rollback journal produces an actionable recovery state;
only a later explicit recovery action may use the legacy writable recovery path,
after which the host freshly inspects the result. Remembered authorization is a
`remembered-ready` state, not automatic execution: the bridge remains inactive,
the raw application window stays hidden at `/__host/locked`, and assets remain
unreleased until the user explicitly opens the application from the trusted
shell.

The Cabinet cache is a distinct owner-protected `cabinet-v1` store with strict
size/count/schema bounds and create-new replacement. Trusted UI receives only
opaque recent IDs plus bounded last-observed labels. Rust resolves a recent ID
and performs fresh pinned inspection before any action; cache deletion or
corruption cannot change trust, grants, capsule bytes, or recovery state.

PNG/WebP Overview artwork is selected from the retained verified snapshot,
hash-checked, bounded to 512 KiB compressed, preflighted to 1024 by 1024 pixels
and 4 MiB decoded RGBA, decoded off the UI thread, and re-encoded as a static
metadata-free host PNG. Animation, malformed/truncated data, media mismatch,
dimension mismatch, overflow, and unsupported formats use the deterministic
host fallback. Only that derivative is embedded in the selection-bound trusted
Overview; it is never served to the raw origin.

The trusted shell may forget an active exact decision only after the literal
confirmation `FORGET-CURRENT-DECISION`. The host-local transaction may delete
the current exact-file exception, exact signed-release row, and grants for the
same capsule/application/application digest. It cannot delete publisher trust,
revocations, backups, another capsule or digest, or audit history. The action
records a new audit event, grants no authority, deactivates the runtime, locks
the raw child, and re-renders the current capsule as a promptable decision.

## Launch state machine

```text
received -> metadata inspected -> structurally verified -> overview
         -> publisher evaluated -> capabilities decided -> explicit open -> runnable
         -> application window released
```

Any state may move to `rejected`. Only `runnable` may materialise executable
assets or create a child-protocol session. Metadata UI receives a bounded copy
of identity fields, never asset bytes or endpoint SQL. Unknown or unsupported
format versions remain inspectable only when that can be done safely.

After verification, a complete stored `always` decision for the exact signed
release moves to `remembered-ready`. The trusted Overview remains first and the
host does not repeat the capability prompt, but it activates the bridge and
shows the application window only after the explicit trusted-shell open action.
A changed capsule/application identity, digest, signing key, permission request,
missing grant, deny, revocation, or invalid signature stays locked and cannot
take this path.

Every selected or dropped replacement path first deactivates the prior runtime.
A drop containing anything other than exactly one path becomes stored
`drop-rejected` host state, clears the prior inspection, and navigates the raw
child back to its locked probe before the trusted shell displays the rejection.
A cancelled picker makes no replacement attempt and preserves the current
session.

## Child request grammar

The native custom-protocol RPC accepts UTF-8 JSON bodies of at most 65,536
bytes. The Rust parser in `capsule_core::protocol` is the executable definition.
Each request has exactly these fields:

```json
{
  "version": 1,
  "session": "43 base64url characters without padding",
  "sequence": 1,
  "id": "caller request id",
  "method": "manifest",
  "params": {}
}
```

Rules:

- `session` is a fresh 256-bit secret created only after the launch reaches
  `runnable`; a token from an older child or file is stale;
- `sequence` starts at 1 and increases by exactly one, including after an
  accepted request whose operation later fails;
- `id` is 1–64 ASCII letters, digits, `.`, `_`, `:`, or `-`, and cannot repeat
  in one session;
- a session accepts at most 4,096 requests before a new launch is required;
- duplicate JSON fields, unknown fields, trailing data, unknown methods,
  unknown parameter fields, and invalid names fail closed;
- failures use stable error codes and do not echo secrets, paths, SQL, or
  database contents.

The only method shapes are:

| Method | Exact `params` | Result class |
| --- | --- | --- |
| `manifest` | `{}` | sanitized manifest identity and effective capabilities |
| `permissions` | `{}` | effective requested/granted capability map |
| `read` | `{"endpoint": string, "arguments": object}` | bounded rows/value declared by that named read |
| `write` | `{"endpoint": string, "arguments": object}` | bounded named-write result and change identity |

Endpoint names are 1–128 characters from the same ASCII name alphabet. The
application supplies values only. It cannot supply SQL, statement counts,
transaction options, file names, or capability decisions.

## Response grammar

Every response to a structurally accepted request repeats `version`, `sequence`,
and `id`, but never the session secret. A rejection that occurs before those
fields can be authenticated returns only `ok` and a stable error object. An
accepted request contains exactly one of:

```json
{"version":1,"sequence":1,"id":"x","ok":true,"result":{}}
```

```json
{"version":1,"sequence":1,"id":"x","ok":false,"error":{"code":"denied","message":"Operation denied by host policy."}}
```

Response byte and row limits are method-specific and always bounded below the
renderer memory limit. The host serialises responses; application content never
receives an SQLite cursor or streaming file primitive. The current runtime
bounds named results to 1,000 rows and 2 MiB, individual verified assets to 16
MiB, compound endpoints to 16 steps, and endpoint execution to a three-second
progress deadline. Verified schemas remain trigger-free, while SQLite's internal
foreign-key action machinery is bounded to 32 cascade levels so declared
`ON DELETE` and `ON UPDATE` actions remain usable.

## Renderer defaults

Until a capability-specific revision says otherwise, the raw child is
incognito and denies clipboard access, developer tools, navigation, popups,
external protocols, downloads, network requests, file URLs, workers, frames,
objects, forms, media, camera, microphone, and persistent service workers. The
entry document and its reviewed same-release assets receive a generated
default-deny CSP. Browser or operating-system permission prompts never replace
host grants.

The script policy admits same-origin scripts plus `wasm-unsafe-eval`. This is
the narrow WebAssembly compilation exception required by reviewed capsule-local
engines such as SQLite WASM; it does not admit JavaScript `unsafe-eval`, remote
scripts, workers, or additional origins.

## Fail-closed requirements

- No protocol endpoint exists before `runnable`.
- A verification, publisher, revocation, grant, session, schema, or backup
  uncertainty denies the affected operation.
- A parser error has no partial effect and consumes no sequence number.
- A structurally accepted request consumes its sequence number before endpoint
  execution, preventing replay after a timeout.
- A child crash, WebView crash, protocol timeout, lock loss, source-file
  identity change, or trust-store migration error closes the writable session.
- Changing capsule identity, signed application digest, publisher key, or
  requested capabilities creates a new decision and a new session.

## File lifecycle

A writable native runtime requires all of the following before activation:

- a retained canonical regular-file identity;
- a fixed/local filesystem classification where the platform can establish it;
- the one-host-writer operating-system lease;
- unchanged post-SQLite-open launch evidence; and
- a host-owned backup directory outside the source directory.

On Windows, only a volume positively classified as fixed/local is eligible for
writes. Removable, remote, and unknown drive types reopen with an explicitly
reported read-only effective permission set; a classification failure never
silently enables writes.

Before each operation the host checks retained identity, SQLite `data_version`,
and change-log position. Before the first write it creates and verifies a
SQLite-consistent backup plus protected inventory. It repeats that checkpoint
after at most ten committed named writes and on clean close, and bounds
retention by count, bytes, and age without deleting the final verified copy. A
second host writer opens read-only with `database.write` effectively denied.
Restore accepts only an inventoried, hash-matching, conformant backup and a
host-picked path that does not exist. Secondary launches forward one explicit
path to the existing trusted window. A conflict offers reinspection, an
explicit read-only session when fresh policy still authorises the release, or a
verified new-path restore. Replacing the source is never an implicit restore
operation.

## Host updates and revocations

Release and revocation documents are verified by a host-only offline core under
compiled roots. Candidate review verifies the exact newer target, signature,
time, version, and allowlisted HTTPS origin without implying consent. Only a
later authorization step can convert that reviewed candidate into an installable
update, after explicit user consent, no live capsule session, and a completed
verified capsule backup. The
application renderer has no update, revocation, network-refresh, staging, or
rollback method.

Verified artifact and Sigstore bytes may be placed only in the protected update
stager. It retains the exact signed release envelope, exact hashes, and, when
available, an exact prior installer.
State advances through create-new immutable records: prepared, installer
started, awaiting health, then healthy or rollback required. Partial stages keep
an in-progress marker and are never inferred recoverable. After the bundled
trusted UI loads and the trust store opens, the top-level shell reconciles the
one in-flight stage. Awaiting health succeeds only when the running host version
equals the signed candidate; interrupted installer start or version mismatch
requires rollback. The staging core does not fetch or execute either package.
The separate installer coordinator accepts only the opaque prepared value and,
on Windows, freshly verifies and locks both the candidate and preserved prior
installer under the same signed Authenticode identity. It records installer
started before invoking system `msiexec.exe` for MSI or the exact NSIS path for
EXE through `ShellExecuteExW`; immediate launch failure records rollback
required, while success records awaiting health. The host exits only after the
handoff succeeds. It does not yet execute the preserved rollback installer.

Install preparation is a fail-closed host boundary. It first reverifies the
persisted envelope against the compiled release root, current release
sequence/version, exact target, time, and update host, then requires explicit
confirmation of the one exact prepared stage and rejects damaged, ambiguous, or
already-transitioned inventory. For a writable capsule session it establishes a
verified current-state SQLite backup even if the session is newly opened, or a
fresh backup when dirty. The bridge releases the runtime, writer lease, protocol
token, and child renderer only after that succeeds; failure leaves them active.
A read-only session has no mutable capsule state to preserve. The internal
preparation, Windows execution, and Windows rollback commands are registered host
boundaries but are not granted to the bundled WebView until live signed-package
and clean-machine acceptance complete the consent flow. The
execution boundary additionally requires the literal `RUN VERIFIED HOST
INSTALLER` and refuses to quiesce a capsule when no preserved prior installer
exists. Rollback independently requires `RUN VERIFIED HOST ROLLBACK`, the exact
single rollback-required stage, and a running candidate version equal to that
stage. The raw renderer never receives any of these commands.
The current durable profile is `org.sqlite-capsule.host-update-stage/0.4`;
profile 0.2 stages are rejected because they did not retain the signed envelope,
and profile 0.3 stages are rejected because they did not bind the preserved
installer's version or durable rollback execution states.
Before a new stage can quiesce a capsule, the host also requires one healthy
prior stage whose candidate package is the exact running version, compiled
release sequence, target, root, and update origin. Its historical envelope is
reverified without treating expiry as permission to download again, and its
exact package becomes the new stage's rollback installer. If no healthy stage
exists, staging discovers exactly one fixed-name package retained beside the
installed host by its MSI or NSIS bootstrap installer. It holds a native
no-replacement guard, repeats the signed signer/timestamp policy, and requires
the MSI `ProductVersion` or PE fixed product version to equal the running host
before copying that package. Missing, ambiguous, replaced, or mismatched
bootstrap retention refuses staging before preflight.

Rollback preparation exposes only that exact preserved path. The platform
coordinator repeats native signer/timestamp, exact size/hash, and signed MSI/PE
product-version checks while holding its no-replacement guard. It durably records
rollback-started before `ShellExecuteExW`; rejection records rollback-failed,
while accepted handoff records awaiting-rollback-health and exits the newer host.
Startup reconciliation records rolled-back only under the exact preserved
version and otherwise records rollback-failed. These terminal states do not
change capsule bytes or host-local trust decisions.

The host initializes a pinned Rust-only Tauri updater backend. A build either
contains all four compile-time inputs—credential-free HTTPS endpoint, Minisign
updater key, 32-byte Ed25519 release-policy root, and positive current release
sequence—or none. Partial/unsafe configuration fails before UI release and an
unconfigured build reports transport disabled. An explicit trusted-shell check
is bounded to one operation and 30 seconds. Its Tauri announcement must contain
a strict `sqlite_capsule` object with `signed_release` and
`sigstore_bundle_url`. The signed manifest must match the exact newer stable
version, sequence, native target, artifact URL, compiled origin, and required
platform-signing class. Release profile 0.2 also binds the exact platform signer
identity, a mandatory timestamp flag for Authenticode/Developer ID, the
Sigstore certificate identity, and its credential-free HTTPS OIDC issuer; the
Sigstore URL must share the compiled origin. Unknown wrapper fields, missing
signatures, downgrades, expiry, mismatches, credentials, fragments, foreign
origins, and more than five redirects fail closed.

Only the bundled top-level UI receives the specific `download_host_update`
command. Its checkbox plus button is explicit download consent, not install
consent. The host caps the signed package at 512 MiB and Sigstore evidence at 16
MiB, streams without exceeding those bounds, requires the exact signed package
length, verifies the Tauri-compatible Minisign signature, then checks the
release and Sigstore digests. On Windows it next writes the exact package to a
random create-new file under an owner-protected host directory, syncs it, runs
offline Authenticode policy with no UI, requires the signed leaf-certificate
SHA-256 identity and countersignature, closes trust-provider state, and removes
the file. The adapter holds a read handle that denies write/delete sharing and
requires identical package length/hash before and after native trust evaluation.
The bounded Sigstore bundle is independently verified offline under the embedded
production trust root: artifact signature, Fulcio chain, SCT, Rekor checkpoint,
inclusion proof/promise, integrated time, exact certificate identity, and exact
OIDC issuer must all pass without warnings. An unsafe package suffix, invalid
chain, wrong signer, missing timestamp, unavailable cached revocation evidence,
Sigstore failure, evidence referring to different artifact bytes, or cleanup
failure rejects the download. Successful bytes remain only in host memory in a
non-constructible accepted-download state and are reported platform- and
Sigstore-verified but not yet staged, install-authorized, or installed. A
separate trusted-shell checkbox/button records install intent. Its bounded
`stage_host_update` command rejects a different candidate or dirty/active stage
inventory, establishes a verified backup for writable capsule state, quiesces
the active capsule session, mints the opaque installable state, and persists the
exact package and Sigstore bytes under create-new staging. Success is explicitly
staged but not installed. macOS/Linux platform adapters remain open. The
registered installer-preparation/execution commands and every generic Tauri
updater permission remain unavailable to both WebViews.

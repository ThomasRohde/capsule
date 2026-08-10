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

The trusted shell may forget an active exact decision only after the literal
confirmation `FORGET-CURRENT-DECISION`. The host-local transaction may delete
the current exact-file exception, exact signed-release row, and grants for the
same capsule/application/application digest. It cannot delete publisher trust,
revocations, backups, another capsule or digest, or audit history. The action
records a new audit event, grants no authority, deactivates the runtime, locks
the raw child, and re-renders the current capsule as a promptable decision.

## Launch state machine

```text
received -> metadata inspected -> structurally verified
         -> publisher evaluated -> capabilities decided -> runnable
         -> application window released
```

Any state may move to `rejected`. Only `runnable` may materialise executable
assets or create a child-protocol session. Metadata UI receives a bounded copy
of identity fields, never asset bytes or endpoint SQL. Unknown or unsupported
format versions remain inspectable only when that can be done safely.

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
progress deadline.

## Renderer defaults

Until a capability-specific revision says otherwise, the raw child is
incognito and denies clipboard access, developer tools, navigation, popups,
external protocols, downloads, network requests, file URLs, workers, frames,
objects, forms, media, camera, microphone, and persistent service workers. The
entry document and its reviewed same-release assets receive a generated
default-deny CSP. Browser or operating-system permission prompts never replace
host grants.

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

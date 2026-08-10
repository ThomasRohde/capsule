# ADR 0019: Signed host updates and revocations

## Status

Accepted on 2026-08-08 for the offline-verifiable release-policy core,
protected last-known-good revocation state, and durable host-update staging.
Host-owned bounded refresh and Windows Authenticode acceptance are implemented.
The Windows installer and rollback handoff coordinators are implemented but
remain ungranted and have not executed a release installer. Production signing,
macOS/Linux acceptance and launch adapters, live rollback acceptance,
production Sigstore bundle generation, and clean-machine platform evidence
remain open delivery obligations.

## Context

The native host is part of the capsule security boundary. Replacing it changes
signature verification, trust policy, filesystem access, backups, and the
renderer sandbox, so transport security or an ordinary version check alone is
not enough. Revocation data must remain useful offline, must never roll back to
an older sequence, and must override local trust for a known bad key or exact
application digest.

Capsule applications must not gain a network/update/revocation API. The policy
therefore needs to verify already-fetched bytes without containing a downloader
or installer.

## Decision

### Signed release manifest

`org.sqlite-capsule.host-release/0.2` is a strictly decoded RFC 8785 JSON
payload signed with Ed25519 under the context `SQLite Capsule host release
manifest v2`. It contains a positive monotonic sequence, stable
`major.minor.patch` version, exact UTC issue/expiry seconds, and at most 32
target artifacts. Every artifact binds the exact target, lowercase HTTPS URL,
byte length, SHA-256, Sigstore-bundle SHA-256, and required platform-signing
class (`authenticode`, `developer-id-notarized`, or `linux-detached`). It also
binds one class-specific canonical signer identity, the platform timestamp
requirement, the expected Sigstore certificate identity, and the credential-
free HTTPS OIDC issuer. Authenticode uses a lowercase SHA-256 fingerprint of
the complete leaf certificate; Developer ID uses the ten-character team ID;
Linux detached signatures use the uppercase OpenPGP primary-key fingerprint.
Authenticode and Developer ID releases may not make timestamp evidence
optional.

The verifier accepts only the compiled root key ID and signature. Candidate
review requires a strictly newer sequence and version, one exact target, and an
allowlisted HTTPS host, but does not imply download or install consent. The
completed artifact must match both signed length and hash; the Sigstore bundle
has an independent signed digest. Redirects are restricted to the already-
signed host even when other hosts are generally allowlisted. Manifest
validation proves only signed policy. A non-constructible
`VerifiedDownloadedUpdate` is produced only when the exact artifact bytes also
match a native platform-verification report and an offline Sigstore report for
the signed certificate identity, OIDC issuer, Fulcio chain, SCT, Rekor proof,
and integrated time. A later authorization step consumes that state only after
explicit install consent, quiesced capsule sessions, and a completed verified
backup to produce `VerifiedInstallableUpdate`. The legacy one-call policy API
remains for compatibility but is not accepted by durable staging.

### Signed revocation bundle

`org.sqlite-capsule.revocations/0.2` is a separately contextualised, strict RFC
8785/Ed25519 payload with a positive monotonic sequence, exact UTC issue and
next-update seconds, bounded unique key-ID and application-digest revocations,
reasons, and at most 16 emergency-root actions. A root delegation or revocation
is effective only because the already-trusted revocation root signed the bundle;
putting another signature or key in a capsule grants nothing.

Future-issued bundles outside five minutes of clock skew fail closed. A bundle
past `next_update` remains the last-known-good set and is reported stale: known
entries still block offline, while unrelated decisions expose stale status.
Sequence rollback, duplicate entries, invalid keys/digests, bad signatures, and
malformed/unknown fields fail closed.

### Protected installation

Trust-store schema v2 atomically migrates v1 and stores one active bundle plus
remote key/release entries and explicit delegated/revoked roots. Installing a
verified bundle deactivates the prior record and replaces active entries in one
SQLite transaction with a redacted audit event. Public root bytes are available
only to trusted host code selecting a verifier; redacted export omits them.

Remote key or exact application-digest revocation takes precedence over local
publisher, release, grant, and exact-file trust. Reset remains separately
confirmed and removes remote state only after the existing verified trust-store
backup workflow.

### Durable staging and startup health

`capsule-update` is a separate host-only filesystem core. Its durable
`org.sqlite-capsule.host-update-stage/0.4` record preserves the exact signed
release envelope as well as all selected platform and Sigstore identities. It
accepts only a
`VerifiedInstallableUpdate` plus artifact and Sigstore bytes that still match
the signed release manifest. The type can be minted only after exact-byte
platform/Sigstore acceptance and the separate install authorization preflight.
It creates an owner-protected stage with create-new files, retains an
exact hashed copy and version of the prior installer when available, and records
state as immutable transitions: prepared, installer started, awaiting startup
health, healthy, rollback required, rollback started, awaiting rollback health,
rolled back, or rollback failed. It does not download or execute an installer.

Before installer preparation can quiesce a capsule session, the stager verifies
the persisted release envelope again under the compiled Ed25519 release root,
current release sequence/version, exact native target, current time, and
compiled update host. It then requires every persisted selected field and file
hash to match the freshly verified candidate. Profiles 0.2 and 0.3 are rejected
rather than migrated because they lack durable authorization or rollback-version
evidence.
The Windows platform verifier can also return a guard that retains its
no-write/no-delete-sharing file handle through a later path-based installer
handoff; dropping the guard ends that replacement lock.

`capsule-installer` is the separate host-only execution coordinator. It accepts
only the opaque prepared value, then freshly verifies and retains platform
guards for both the candidate and preserved prior installer. Both must match
their exact stored size/hash and the same signed Authenticode identity and
timestamp policy; this deliberately rejects signer rotation until rollback
policy can bind distinct old and new signer identities. It records installer
started before invoking the operating system. On Windows an STA thread uses
`ShellExecuteExW` with a process handle and no-UI/no-async flags, routing MSI to
system `msiexec.exe` with `/promptrestart AUTOLAUNCHAPP=True` and NSIS to the
exact staged executable with `/UPDATE`. An immediate OS rejection records
rollback required; a successful handoff records awaiting health. The file
guards stay alive through this handoff. Candidate MSI/PE ProductVersion metadata
must also equal the signed release version.

An in-progress marker makes partial staging non-recoverable. Once prepared, each
durable transition can be recovered without interpreting absence as success.
The new host may mark healthy only when its running version exactly matches the
staged version. A failed installer, failed startup, version mismatch, or explicit
operator choice can require rollback; only the inventoried preserved installer
path is then exposed to the platform layer. A separate opaque rollback value is
available only for the exact rollback-required stage and only while the running
host equals the failed candidate. The coordinator repeats immutable native and
signed ProductVersion verification, records rollback-started before the platform
handoff, then records awaiting-rollback-health, rolled-back, or rollback-failed.
Test-only process exits cover every staging, update-health, and rollback-health
transition boundary.

The next update discovers rollback material only from a prior stage that reached
healthy startup. A historical-release verifier rechecks its envelope under the
compiled root and requires the exact running version, compiled release sequence,
native target, and update origin. It validates timestamp ordering but does not
reuse manifest expiry as download authorization; the purpose is provenance for
already-installed bytes. The exact healthy candidate package is copied into the
new stage before capsule preflight. If no such stage exists, the first upgrade
uses exactly one fixed-name installer retained by the clean-install package.
The NSIS hook copies its own `$EXEPATH`; the MSI WiX fragment copies the Windows
Installer `OriginalDatabase`. Before the package is copied into the new stage,
the host locks it against replacement, repeats the signed native signer and
timestamp policy, and requires MSI or PE signed product-version metadata to
equal the running host. Missing, ambiguous, replaced, or mismatched bootstrap
retention fails before quiescence.

The desktop reconciles that state through a top-level Tauri command invoked only
after the bundled trusted UI has loaded and the protected trust store is open.
This makes the UI/core startup boundary part of health evidence. The command is
not registered on the raw child WebView, and it performs no check, download, or
installation by itself.

Before a platform adapter may start an installer, a separate internal command
requires the exact prepared stage identifier and two explicit boundaries: the
existing preparation confirmation `INSTALL HOST UPDATE` and the execution
confirmation `RUN VERIFIED HOST INSTALLER`. It rejects incomplete, invalid,
ambiguous, already-started, or different candidate inventory and refuses a
missing preserved prior installer before capsule preflight. A writable capsule session
then creates a verified current-state SQLite backup even when it has made no
writes, or refreshes the backup when dirty. Only a successful backup lets the
bridge drop the runtime, writer lease, protocol token, and child session. A
failure leaves the session active. The execution command launches on Windows
and exits the old host only after the OS accepts the handoff. Rollback requires
the independent literal `RUN VERIFIED HOST ROLLBACK`, performs the same preflight,
launches only the exact preserved installer, and exits the failed newer host only
after accepted handoff. All three internal commands are deliberately omitted from the bundled WebView capability until
live signed-package and clean-machine acceptance are present. The
bundled WebView instead receives one narrower `stage_host_update` command. Its
separate install-intent checkbox/button consumes only the accepted in-memory
download, establishes the same recovery/quiescence preflight, authorizes the
opaque installable state, and writes the exact package and Sigstore bytes to the
durable stager. It does not start an installer; the generic Tauri updater
permission is never granted.

The native shell pins `tauri-plugin-updater` 2.10.1 as a Rust-only transport
backend. `SQLITE_CAPSULE_UPDATER_ENDPOINT`,
`SQLITE_CAPSULE_UPDATER_PUBLIC_KEY`,
`SQLITE_CAPSULE_RELEASE_PUBLIC_KEY_HEX`, and
`SQLITE_CAPSULE_HOST_RELEASE_SEQUENCE` are compile-time inputs and must be
present together. The endpoint must be credential-free HTTPS without a
fragment, the release root must be exactly 32 lowercase-hex bytes, and the
current sequence must be positive; bad or partial configuration stops startup
before the UI is released. Development builds with no inputs are visibly
transport-disabled.

The trusted shell may request one update operation at a time. A 30-second check
uses at most five same-origin redirects and treats Tauri metadata as untrusted
until a strict `sqlite_capsule` wrapper supplies the separately Ed25519-signed
release manifest and a same-origin Sigstore URL. The signed sequence, stable
version, exact native target and artifact URL, origin, and platform-signing
class must match the announcement. A second checkbox-and-button action is
explicit consent to download, not to install. It streams a package no larger
than the signed 512 MiB ceiling and a Sigstore bundle no larger than 16 MiB,
verifies the Tauri-compatible Minisign signature, and matches the signed length
and both signed digests. On Windows the exact package must have a `.msi` or
`.exe` suffix and is synchronously materialised in an owner-protected create-new
file. `WinVerifyTrust` runs with no UI, cached-only URL retrieval, whole-chain
revocation policy, and state that is always closed. The adapter extracts the
verified leaf, compares its SHA-256 certificate fingerprint with the signed
identity, requires a valid countersignature, records a redacted subject, and
holds a no-write/no-delete-sharing file handle while hashing the package before
and after trust evaluation. The Sigstore verifier then uses its pinned embedded
production trust root without network access and requires the artifact
signature, Fulcio chain, SCT, Rekor inclusion/checkpoint/promise evidence,
positive integrated time, and exact signed certificate identity/OIDC issuer.
Only one acceptance object binding both reports to the same package bytes may
remain in trusted host memory. A third, separately confirmed action may pass
that object through backup/session preflight and durable staging without
executing it. Wrong or missing signatures, pins, timestamp
evidence, offline revocation state, Sigstore proof, suffixes, or cleanup fail
closed. Downloaded bytes are not staged or install-authorized until that third
action succeeds, and staged bytes remain uninstalled.
The specific check/download/stage commands are granted only to the bundled shell; no
`updater:*` command is granted to WebView JavaScript and the raw child has no
Tauri IPC at all.

## Consequences

- The current core can test signature, expiry, downgrade, target, consent,
  quiescence, backup, partial bytes, redirect, monotonicity, clock skew,
  stale-offline, exact revocation, and root-delegation policy without network
  access or production credentials.
- The desktop still needs a persisted refresh schedule, macOS/Linux platform
  verification and installer adapters, real installed-version discovery, and
  explicit execution of the staged rollback. The durable staging and startup-
  health record never modifies capsule bytes or trust decisions.
- Real Authenticode, Developer ID/notarization, Linux package signatures,
  production roots, HTTPS endpoints, and clean-machine evidence are mandatory
  before a signed public release can be claimed.

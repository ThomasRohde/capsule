# ADR 0018: Native file lifecycle and recovery

## Status

Accepted on 2026-08-08 for the native lifecycle foundation. Windows automated
identity, one-writer, conflict, backup, restore, host-owned picker, secondary-
instance, open-path, and hot rollback-journal recovery tests are implemented.
Child-process fault injection covers durable pre-write backup, bounded
checkpoint, restore, clean-close, and update-preflight stages. Guarded debug-host
acceptance also covers real Windows termination during open and each of those
lifecycle operations. Clean-installer association evidence, signed update flows,
and live macOS/Linux behavior remain open obligations; macOS/Linux work is
postponed until the repository is public and suitable runners are available.

## Context

A capsule is both the portable application artifact and its user data. Native
conveniences such as double-click, drag/drop, and writable sessions must not
turn path aliases, external replacement, concurrent hosts, crashes, or restore
shortcuts into silent data loss. A byte copy of a live SQLite main file can omit
committed WAL data or copy an inconsistent transaction state.

The earlier runtime re-inspected a path before opening SQLite, but did not hold
an operating-system file identity, coordinate host writers, detect later
external commits, or create recovery material before mutation.

## Decision

### Pinned identity and one writer

Add a product-independent `capsule-lifecycle` crate. It rejects non-files and
direct symbolic links, canonicalises the source, opens and retains a file
handle, records the volume/device, file/inode, and byte length, and verifies
that the path still names the held object.

On Windows the retained handle allows read/write sharing required by SQLite but
deliberately omits delete sharing, so rename/replacement is blocked while the
session is alive. It classifies fixed, removable, and remote drives and denies
writable sessions on removable or remote storage. POSIX retains the inode and
rechecks the canonical path before and after operations; removable/network
classification still needs platform-specific evidence.

A Windows named mutex or POSIX non-blocking `flock` grants one host writer per
canonical source identity and releases automatically after process exit. If the
lease is busy, the desktop opens the capsule read-only and downgrades the
effective `database.write` result for that session instead of silently
contending. The trusted status surface explains the read-only reason.

The runtime captures SQLite `data_version` and the latest change-log position
after verification. It checks the pinned path, data version, and change
position before reads/assets/writes and the pinned path again after a write.
An external SQLite commit or source replacement closes the child session before
another host write. Opening SQLite is followed by a second full launch
inspection so journal recovery or an in-place change cannot inherit stale trust
evidence unnoticed.

### Verified backups and restore

Every writable native runtime requires a host-owned backup directory outside
the source directory. Before the first named write, the host:

1. applies owner-only directory/file protection;
2. creates a new destination exclusively;
3. uses SQLite's online backup API from the live runtime connection;
4. reopens and independently inspects the backup;
5. checks capsule identity, signed application digest, machine conformance,
   asset hashes, endpoints, and every declared check;
6. records backup ID, timestamps, byte length, SHA-256, source file identity,
   source file SHA-256, capsule/application identity, signed digest, and
   change-log position in a protected JSON inventory record; and
7. only then permits the named write.

Failed write preconditions do not remove the verified pre-write backup. A dirty
runtime creates a new verified checkpoint after at most ten named writes and on
clean close. Retention keeps at most ten backups, 2 GiB, and 90 days while never
automatically deleting the last verified copy.

Restore validates the managed backup ID, inventory, byte length, SHA-256,
launch identity, signed digest, and complete conformance again. It uses the
SQLite backup API to a caller-selected path that must not exist, verifies the
restored database, and returns its digest. It never replaces the original or an
existing output. The trusted host exposes this only through its native save
picker, then treats the restored file as a fresh capsule session.

Backup creation retains an owner-protected in-progress marker until both the
SQLite copy and protected manifest are complete. Inventory scanning classifies
missing pairs or marker-left records as interrupted and hash mismatches as
invalid; the trusted UI reports both and offers neither as recoverable. Restore
likewise keeps an adjacent in-progress marker until the new-path database passes
all checks. A crash-left marked output is refused by ordinary launch so a
partially restored file cannot masquerade as complete.

### Rollback-journal recovery

Normal launch remains read-only. If an adjacent rollback journal prevents that
inspection, the host reads only the fixed SQLite header and capsule application
ID, pins the source identity, and takes the normal host writer lease. It opens
SQLite read/write without create or extensions and issues only fixed internal
schema-version and bounded integrity queries, allowing SQLite itself to decide
and perform rollback. The host never removes the journal. While identity is
still pinned, it repeats complete launch inspection, signatures, policy, and
capsule checks. The trusted report identifies the journal digest, hot-journal
candidate state, before/after source digests, and whether SQLite retained the
sidecar; executable assets remain locked until the fresh decision is accepted.

### File-open routing

The bundle declares only `.sqlitecapsule` as an automatic application
association, with `application/vnd.sqlite-capsule`; it does not claim `.sqlite`.
Initial command-line and trusted-window drag/drop paths share the same
content-based inspection/trust path and reject non-SQLite inputs. Existing
`.capsule.sqlite` files remain supported through explicit Open with, command
line, picker, and drop. A suffix never bypasses verification.

## Consequences

- A second host cannot silently become another writer. An unrelated SQLite
  process is detected by `data_version` before the host writes again.
- On Windows, a path replacement is blocked for the session; on POSIX it is
  detected, but stronger filesystem classification and live evidence remain.
- Backup failure is a write failure, not a warning. Backup manifests contain no
  private key and no original path.
- Periodic checkpoints are created before an eleventh named write, and clean-
  close checkpoints are implemented. A real child-process
  abrupt exit proves rollback of a spilled uncommitted mutation, removal of the
  hot journal by SQLite, unchanged change-log position, direct integrity, fresh
  inspection, and verified runtime reopen. Separate process-exit triggers are
  compiled only under `cfg(test)` and terminate child processes after the pre-
  write backup copy, checkpoint/close manifest sync, and restore copy. Parent
  tests prove direct SQLite integrity/change position and marker-based
  quarantine. Guarded debug-host acceptance exercises the actual Windows open,
  pre-write, bounded-checkpoint, close, update-preflight, and restore paths; the
  fault controls are absent from release binaries. Signed-update acceptance
  remains separate release work.
- Single-instance forwarding/focus is implemented. Actual signed-installer
  registration remains delivery work; configuration is not clean-install evidence.

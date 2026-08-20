# Capsule lifecycle security gauntlet

Run this as an independent attack review after the implementation exists and
again during M09. Assume every capsule, filename, metadata value, database page,
schema object, icon, lineage claim, migration declaration and comparison partner
is attacker-controlled until verified.

## 1. Trust-boundary inventory

Prove, from code and generated Tauri capabilities:

- which commands the trusted shell can invoke;
- which commands the raw Wry renderer can invoke;
- which windows receive events;
- which code reads paths and opens SQLite connections;
- which code can write or publish files;
- which stores hold trust, recents, backups and operation state.

Fail the review if lifecycle access is prevented only by hiding UI controls.

## 2. Input immutability and source races

Attempt:

- direct write through every source connection;
- WAL/journal/sidecar creation beside source;
- symlink, junction, reparse point and hard-link aliases;
- source replacement after plan, after destination selection and during execute;
- metadata-only changes that preserve length;
- same path with different file identity;
- read-only media and network-share behaviour.

Required result: no input mutation; stale/replaced source rejected.

## 3. Destination and publication attacks

Attempt:

- pre-create destination between review and execute;
- symlink destination or parent substitution;
- path traversal and alternate data stream names where relevant;
- destination equal/alias to an input;
- insufficient disk space/quotas;
- crash/kill at every stage: temp create, data write, validation, fsync, rename,
  lineage write and post-publish reopen.

Required result: no overwrite, no corrupted input, no half-accepted published
output, recoverable private temp state only.

## 4. Format, profile and icon parsing

Fuzz:

- missing/duplicate singleton rows;
- invalid UTF-8 boundaries and extreme Unicode;
- oversized title/description/tags;
- malformed JSON and deep nesting;
- path-like asset IDs;
- hash mismatch;
- malformed PNG/WebP, oversized dimensions, pixel bombs and truncated images;
- future/unknown tables and versions;
- spoofed title/icon/publisher combinations.

Required result: bounded rejection or safe fallback; signed publisher identity
remains visually distinct from self-described profile.

## 5. Signature compartment

Mutate each table/field individually.

Required result:

- every application-controlled mutation changes the application digest;
- instance/profile/icon/lineage/domain-row changes do not;
- fork/reconcile output digest equals the expected release;
- upgrade output digest equals the clean target release;
- v0.2 semantics are unchanged.

## 6. Data contract and comparison

Attempt:

- duplicate/undeclared tables;
- absent/composite/malformed primary keys;
- cycles and impossible dataset dependencies;
- sensitive datasets labelled normal by mutable metadata;
- huge row/blob/text values;
- collation and type edge cases;
- cancellation and deadline races;
- arbitrary table/token requests from JavaScript;
- canary endpoints/assets/commands that detect execution.

Required result: verified signed contract only; bounded deterministic reports;
no execution; sensitive values masked until trusted-shell disclosure.

## 7. Reconciliation

Attempt:

- replay stale report/plan;
- alter source or target row after review;
- mutate immutable/PK columns;
- violate FK/unique/check constraints;
- inject table/column names into a plan;
- unresolved or forged conflict resolution;
- apply to original target;
- preserve malicious source application tables.

Required result: target-derived create-new output, target application digest
unchanged, transactional rollback, no publication on any failed precondition.

## 8. Upgrade and migration

Attempt:

- different app ID or publisher key;
- downgrade/same-version ambiguity;
- capability escalation hidden in metadata;
- invalid target signature;
- missing/cyclic/ambiguous migration path;
- unknown migration operation/field;
- platform/application table target;
- type confusion, overflow, row bomb and unmapped values;
- migration write to input or signed application compartment;
- application release with malicious checks/endpoints.

Required result: clean target release as base; restricted host interpreter only;
same publisher; explicit capability delta; exact target application digest;
failed migration leaves no published output.

## 9. Privacy and audit

Inspect logs, crash reports, status files, recents and evidence for:

- raw sensitive values;
- full paths where not necessary;
- keys/signatures/secrets;
- comparison pages cached after close;
- stale operation sessions;
- clipboard or support-export leakage.

Required result: minimal redacted metadata and explicit user action for disclosure.

## 10. Evidence

For every attempted attack, record:

- fixture and setup;
- exact command/action;
- expected result;
- actual result;
- relevant logs without sensitive data;
- code/test reference;
- severity and resolution.

Critical/high unresolved findings block release. A test that cannot be run remains
an explicit gap; it is not a pass.

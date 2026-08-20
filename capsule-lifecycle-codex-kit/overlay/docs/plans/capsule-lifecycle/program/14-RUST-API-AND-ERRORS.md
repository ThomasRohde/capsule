# Rust API and stable error design

This is a proposed implementation shape for M02 onward. M00 must reconcile names
with the live workspace, but the ownership boundaries and fail-closed semantics are
requirements.

## 1. Crate dependency direction

```text
capsule-core ───────────────┐
capsule-crypto ─────────────┤
capsule-lifecycle ──────────┼──> capsule-workspace ──> capsule-cli
capsule-runtime (verify only)┘                          desktop/src-tauri
```

`capsule-workspace` may depend on generic inspection, crypto verification and
source-pinning/publication primitives. Those crates must not depend back on
`capsule-workspace`. The raw renderer/runtime endpoint bridge must not receive a
workspace handle.

## 2. Core types

Illustrative Rust shape:

```rust
pub struct CapsuleFileRef {
    pub path_hint: PathBuf,
    pub file_sha256: Sha256Digest,
    pub size_bytes: u64,
    pub filesystem_identity: FilesystemIdentity,
    pub overview: CapsuleOverview,
}

pub struct ApplicationReleaseIdentity {
    pub app_id: String,
    pub app_version: String,
    pub application_digest: Sha256Digest,
    pub publisher_key_id: Option<KeyId>,
    pub data_schema: DataSchemaIdentity,
}

pub struct CapsuleInstanceIdentity {
    pub capsule_id: Uuid,
    pub revision_id: Uuid,
}

pub struct DataSchemaIdentity {
    pub id: String,
    pub version: u64,
}

pub struct LifecyclePlan {
    pub plan_id: Uuid,
    pub operation: OperationKind,
    pub inputs: Vec<CapsuleFileRef>,
    pub output: CreateNewDestination,
    pub decisions: Vec<PolicyDecision>,
    pub limits: OperationLimits,
    pub digest: Sha256Digest,
}
```

Use validated newtypes for identifiers, hashes, safe relative asset paths,
dataset/table/column tokens, version values and timestamps. Do not pass arbitrary
`String` values through trusted boundaries when a stronger type is possible.

## 3. Plan/execute interfaces

```rust
pub trait LifecyclePlanner {
    fn prepare_copy(&self, request: PrepareCopyRequest)
        -> Result<Prepared<CopyPlan>, WorkspaceError>;

    fn prepare_compare(&self, request: PrepareCompareRequest)
        -> Result<CompareSession, WorkspaceError>;

    fn prepare_reconcile(&self, request: PrepareReconcileRequest)
        -> Result<Prepared<ReconcilePlan>, WorkspaceError>;

    fn prepare_upgrade(&self, request: PrepareUpgradeRequest)
        -> Result<Prepared<UpgradePlan>, WorkspaceError>;
}

pub trait LifecycleExecutor {
    fn execute_copy(
        &self,
        plan: &CopyPlan,
        cancellation: &CancellationToken,
    ) -> Result<PublishedCapsule, WorkspaceError>;

    fn execute_reconcile(
        &self,
        plan: &ReconcilePlan,
        cancellation: &CancellationToken,
    ) -> Result<PublishedCapsule, WorkspaceError>;

    fn execute_upgrade(
        &self,
        plan: &UpgradePlan,
        cancellation: &CancellationToken,
    ) -> Result<PublishedCapsule, WorkspaceError>;
}
```

The executor accepts a complete immutable plan produced by the host. It rebinds
every input and destination precondition. It does not repair or silently recompute
a stale plan.

## 4. Source and destination handles

The trusted shell should not pass arbitrary writable paths into execute commands.

Suggested sequence:

1. Tauri command opens a host-owned picker.
2. Rust validates/pins selected inputs and returns opaque session IDs.
3. Rust opens the save picker and reserves a create-new destination token.
4. Planning binds the input and destination tokens.
5. Execution consumes the one-use plan/destination token.

Opaque handles expire on file replacement, cancellation, process restart, timeout
or successful publication.

## 5. Workspace error

```rust
pub struct WorkspaceError {
    pub code: WorkspaceErrorCode,
    pub safe_message: String,
    pub operation_id: Option<Uuid>,
    pub source: Option<Box<dyn Error + Send + Sync>>,
    pub retry: RetryGuidance,
}
```

The stable code catalogue is in
`contracts/lifecycle-error-codes-v1.json`. Internal causes can be logged under
host policy, but Tauri/CLI JSON must expose only safe bounded details.

Do not place:

- raw sensitive row values;
- secret keys or signature bytes;
- arbitrary SQL;
- full source content;
- environment variables;
- unredacted support logs

in user-facing error structures.

## 6. Operation phases and audit states

```text
created
  → inputs_bound
  → planned
  → destination_reserved
  → output_staged
  → transformed
  → verified
  → published
  → reopened_verified
```

Terminal failure states:

```text
cancelled
stale
rejected
failed_before_publish
published_but_postcheck_failed
```

`published_but_postcheck_failed` must be rare and explicit. The output remains
quarantined/untrusted until it can be reopened and verified; it is never reported
as success.

Persist only the minimal operation state required for recovery. Source row values
and comparison details stay in memory/temporary encrypted OS storage according to
host policy, not in general logs.

## 7. Limits

Every public planning/execution request binds explicit limits:

- maximum input/output bytes;
- maximum schema objects;
- maximum datasets/tables/columns;
- maximum rows inspected/written;
- maximum value and report bytes;
- deadline;
- cancellation token;
- maximum lineage parents/events considered;
- maximum migration path/step count.

Use lower UI defaults and configurable host hard ceilings. Capsule declarations
cannot raise host ceilings.

## 8. CLI JSON

Human output may change, but `--json` responses require a versioned `profile`,
stable code fields and deterministic ordering. CLI commands must not bypass the
same planner/executor used by Tauri.

Example error:

```json
{
  "profile": "org.sqlite-capsule.workspace-error/1",
  "code": "stale_plan",
  "safe_message": "An input changed after review. Prepare a new plan.",
  "operation_id": "9aa22fea-8ac1-40d9-bc75-f8ea12c1d34b",
  "retry": "prepare-new-plan"
}
```

## 9. Testing requirements

- compile-time/module tests proving no renderer crate dependency;
- API tests with read-only file handles and synthetic race injection;
- serialisation compatibility tests for plans/reports/errors;
- cancellation at every phase;
- error redaction snapshots;
- no-panic fuzzing for untrusted schema/contract/report parsing;
- platform-specific path identity/publication tests.

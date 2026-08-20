//! Reviewed reconciliation authority over two retained compare inputs.
//!
//! Callers select signed-contract positions and reviewed digests only; table
//! names, columns, SQL, and row values are resolved privately from retained
//! verified snapshots. The non-serializable review retains the exact private
//! values and destination authority consumed by the create-new executor.

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    fmt,
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::atomic::Ordering,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use rusqlite::{
    Connection, OpenFlags, params, params_from_iter,
    types::{Value, ValueRef},
};
use serde::Serialize;
use serde_json::{Value as JsonValue, json};
use sha2::{Digest, Sha256};
use sqlite_capsule_lifecycle::{
    DestinationReservation, PrivateOutput, PublishedOutput, SealedPrivateOutput, SourceIdentity,
};

use crate::{
    CancellationToken, ComparePolicy, CompareSummary, Dataset, DatasetTable, InputRole,
    LifecyclePlan, Operation, ReconcilePolicy, Sensitivity, VerifiedWorkspaceSource,
    WorkspaceControl, WorkspaceError, WorkspaceErrorCode, compare::CompareValue,
};

pub const RECONCILE_REVIEW_PROFILE: &str = "org.sqlite-capsule.reconcile-review/1";

const REVIEW_DIGEST_PROFILE: &str = "org.sqlite-capsule.reconcile-review-digest/1";
const WRITE_SET_PROFILE: &str = "org.sqlite-capsule.reconcile-write-set/1";
const SIGNATURE_INVENTORY_PROFILE: &str = "org.sqlite-capsule.reconcile-signature-inventory/1";
const RECONCILE_PAYLOAD_PROFILE: &str = "org.sqlite-capsule.reconcile-payload/1";
const RECONCILE_LINEAGE_DETAILS_PROFILE: &str = "org.sqlite-capsule.reconcile-lineage-details/1";
const RECONCILE_ANCESTOR_EVIDENCE_PROFILE: &str =
    "org.sqlite-capsule.reconcile-ancestor-evidence/1";
const RECONCILE_CONFLICT_ID_PROFILE: &str = "org.sqlite-capsule.reconcile-conflict-id/1";
const HARD_DEADLINE: Duration = Duration::from_secs(30);
const HARD_REVIEW_LIFETIME: Duration = Duration::from_secs(5 * 60);
const HARD_OPERATIONS: usize = 10_000;
const HARD_ROWS_SCANNED: u64 = 100_000;
const HARD_VALUE_BYTES: u64 = 1024 * 1024;
const HARD_STREAM_BYTES: u64 = 256 * 1024 * 1024;
const HARD_RETAINED_BYTES: u64 = 64 * 1024 * 1024;
const HARD_COLUMNS: usize = 256;
const HARD_FOREIGN_KEYS: usize = 256;

#[derive(Clone, Debug)]
pub struct ReconcileReviewLimits {
    pub deadline: Duration,
    /// Maximum lifetime of a retained three-way classification authority.
    /// Each classify/resolve operation still has its own shorter `deadline`.
    pub review_lifetime: Duration,
    pub max_operations: usize,
    pub max_rows_scanned: u64,
    pub max_value_bytes: u64,
    pub max_stream_bytes: u64,
    pub max_retained_bytes: u64,
}

impl Default for ReconcileReviewLimits {
    fn default() -> Self {
        Self {
            deadline: HARD_DEADLINE,
            review_lifetime: HARD_REVIEW_LIFETIME,
            max_operations: 10_000,
            max_rows_scanned: HARD_ROWS_SCANNED,
            max_value_bytes: HARD_VALUE_BYTES,
            max_stream_bytes: 64 * 1024 * 1024,
            max_retained_bytes: 16 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReconcileReviewLimitsApplied {
    pub deadline_ms: u64,
    pub review_lifetime_ms: u64,
    pub max_operations: usize,
    pub max_rows_scanned: u64,
    pub max_value_bytes: u64,
    pub max_stream_bytes: u64,
    pub max_retained_bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReconcileAction {
    InsertFromSource,
    DeleteFromTarget,
    ReplaceRowFromSource,
    SetFields,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReconcileOperationBasis {
    UserSelected,
    ThreeWayClean,
    ConflictResolution,
}

impl ReconcileOperationBasis {
    const fn label(self) -> &'static str {
        match self {
            Self::UserSelected => "user-selected",
            Self::ThreeWayClean => "three-way-clean",
            Self::ConflictResolution => "conflict-resolution",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThreeWayConflictKind {
    InsertInsert,
    UpdateUpdate,
    DeleteUpdate,
    ImmutableField,
}

impl ThreeWayConflictKind {
    const fn label(self) -> &'static str {
        match self {
            Self::InsertInsert => "insert-insert",
            Self::UpdateUpdate => "update-update",
            Self::DeleteUpdate => "delete-update",
            Self::ImmutableField => "immutable-field",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThreeWayDeletedSide {
    Source,
    Target,
}

impl ThreeWayDeletedSide {
    const fn label(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Target => "target",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThreeWayResolutionChoice {
    KeepTarget,
    TakeSource,
}

impl ThreeWayResolutionChoice {
    const fn label(self) -> &'static str {
        match self {
            Self::KeepTarget => "keep-target",
            Self::TakeSource => "take-source",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ThreeWayConflictResolution {
    pub conflict_id: String,
    pub choice: ThreeWayResolutionChoice,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ThreeWayConflictReview {
    pub id: String,
    pub dataset_id: String,
    pub table: String,
    pub key_digest: String,
    pub kind: ThreeWayConflictKind,
    pub deleted_side: Option<ThreeWayDeletedSide>,
    pub source_row_digest: Option<String>,
    pub target_row_digest: Option<String>,
    pub ancestor_row_digest: Option<String>,
    pub allowed_choices: Vec<ThreeWayResolutionChoice>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ResolvedThreeWayConflictReview {
    pub conflict: ThreeWayConflictReview,
    pub resolution: ThreeWayResolutionChoice,
}

impl ReconcileAction {
    const fn label(self) -> &'static str {
        match self {
            Self::InsertFromSource => "insert-from-source",
            Self::DeleteFromTarget => "delete-from-target",
            Self::ReplaceRowFromSource => "replace-row-from-source",
            Self::SetFields => "set-fields",
        }
    }
}

const fn reconcile_action_phase(action: ReconcileAction) -> u8 {
    match action {
        ReconcileAction::InsertFromSource => 0,
        ReconcileAction::ReplaceRowFromSource => 1,
        ReconcileAction::SetFields => 2,
        ReconcileAction::DeleteFromTarget => 3,
    }
}

/// A trusted-shell selection bound to a row shown by compare detail.
///
/// Numeric positions are resolved only against the signed data contract. The
/// caller cannot supply a table name, column name, SQL fragment, or row value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReconcileSelection {
    pub dataset_index: usize,
    pub table_index: usize,
    pub key_digest: String,
    pub source_row_digest: Option<String>,
    pub target_row_digest: Option<String>,
    pub action: ReconcileAction,
    pub field_indices: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReconcileOutputRequest {
    pub output_path: PathBuf,
    pub plan_id: String,
    pub created_at: String,
    pub expires_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ReconcileDatasetStateReview {
    pub dataset_id: String,
    pub source_row_count: u64,
    pub source_state_sha256: String,
    pub target_row_count: u64,
    pub target_state_sha256: String,
    pub output_row_count: u64,
    pub output_state_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SequenceStateReview {
    name: String,
    sequence: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReconcileReference {
    pub file_sha256: String,
    pub capsule_id: String,
    pub revision_id: String,
    pub application_digest: String,
    pub signature_count: u32,
    pub signature_inventory_digest: String,
    pub data_schema_id: String,
    pub data_schema_version: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ReconcileFieldReview {
    pub column: String,
    pub source_value_digest: String,
    pub target_value_digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReconcileOperationReview {
    pub sequence: u64,
    pub dataset_id: String,
    pub table: String,
    pub key_digest: String,
    pub action: ReconcileAction,
    pub basis: ReconcileOperationBasis,
    pub source_row_digest: Option<String>,
    pub precondition_target_row_digest: Option<String>,
    pub ancestor_row_digest: Option<String>,
    pub conflict_id: Option<String>,
    pub fields: Vec<ReconcileFieldReview>,
    pub sensitive_confirmed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReconcileLineageRelation {
    TargetDerivedFrom,
    ChangesAppliedFrom,
}

impl ReconcileLineageRelation {
    const fn label(self) -> &'static str {
        match self {
            Self::TargetDerivedFrom => "target-derived-from",
            Self::ChangesAppliedFrom => "changes-applied-from",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReconcileLineageParentReview {
    pub ordinal: u8,
    pub relation: ReconcileLineageRelation,
    pub file_sha256: String,
    pub capsule_id: String,
    pub revision_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReconcileOutputReview {
    pub capsule_id: String,
    pub revision_id: String,
    pub application_digest: String,
    pub signature_count: u32,
    pub signature_inventory_digest: String,
    pub preserves_target_capsule_id: bool,
    pub preserves_target_application_digest: bool,
    pub preserves_target_signature_inventory: bool,
    pub must_not_exist: bool,
    pub lineage_event_id: String,
    pub lineage_parents: Vec<ReconcileLineageParentReview>,
}

/// Host-held, non-serializable review/plan capability. Debug output is opaque
/// so private row values cannot enter logs.
pub struct ReconcileReview {
    profile: &'static str,
    plan: LifecyclePlan,
    payload: Vec<u8>,
    payload_digest: String,
    expected_summary: CompareSummary,
    compare_report_digest: String,
    source_ref: ReconcileReference,
    target_ref: ReconcileReference,
    ancestor_ref: Option<ReconcileReference>,
    output: ReconcileOutputReview,
    destination: DestinationReservation,
    operations: Vec<PlannedOperation>,
    operation_reviews: Vec<ReconcileOperationReview>,
    dataset_states: Vec<ReconcileDatasetStateReview>,
    sequence_state: Vec<SequenceStateReview>,
    resolved_conflicts: Vec<ResolvedThreeWayConflictReview>,
    limits: ReconcileReviewLimitsApplied,
    review_digest: String,
    source_handle: VerifiedWorkspaceSource,
    target_handle: VerifiedWorkspaceSource,
    ancestor_handle: Option<VerifiedWorkspaceSource>,
    expires_at: Instant,
    cancellation: CancellationToken,
    confirmed_sensitive_dataset_indices: BTreeSet<usize>,
}

impl fmt::Debug for ReconcileReview {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReconcileReview")
            .field("profile", &self.profile)
            .field("operation_count", &self.operation_reviews.len())
            .field("output_revision_id", &self.output.revision_id)
            .field("payload_digest", &self.payload_digest)
            .field("review_digest", &self.review_digest)
            .finish()
    }
}

impl ReconcileReview {
    pub const fn profile(&self) -> &'static str {
        self.profile
    }

    pub fn compare_report_digest(&self) -> &str {
        &self.compare_report_digest
    }

    pub fn plan(&self) -> &LifecyclePlan {
        &self.plan
    }

    /// Canonical, value-free reconciliation payload bytes. These bytes are
    /// review data only and must be approved byte-for-byte at preparation.
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    pub fn payload_digest(&self) -> &str {
        &self.payload_digest
    }

    pub fn dataset_states(&self) -> &[ReconcileDatasetStateReview] {
        &self.dataset_states
    }

    pub fn source(&self) -> &ReconcileReference {
        &self.source_ref
    }

    pub fn target(&self) -> &ReconcileReference {
        &self.target_ref
    }

    pub fn ancestor(&self) -> Option<&ReconcileReference> {
        self.ancestor_ref.as_ref()
    }

    pub fn resolved_conflicts(&self) -> &[ResolvedThreeWayConflictReview] {
        &self.resolved_conflicts
    }

    pub fn operations(&self) -> &[ReconcileOperationReview] {
        &self.operation_reviews
    }

    pub fn output(&self) -> &ReconcileOutputReview {
        &self.output
    }

    pub fn limits(&self) -> &ReconcileReviewLimitsApplied {
        &self.limits
    }

    pub fn review_digest(&self) -> &str {
        &self.review_digest
    }

    /// Conservative remaining lifetime of this exact host-held authority.
    pub fn remaining_lifetime(&self) -> Result<Duration, WorkspaceError> {
        let (expires_at, expiry_code) = bounded_plan_deadline(
            self.expires_at,
            &self.plan,
            SystemTime::now(),
            Instant::now(),
        )?;
        remaining_effective(expires_at, expiry_code, &self.cancellation)
    }

    pub fn operation_count(&self) -> usize {
        debug_assert_eq!(self.operations.len(), self.operation_reviews.len());
        self.operation_reviews.len()
    }

    /// Consumes the review capability, rebinds every retained authority under
    /// its original absolute deadline/cancellation pair, and yields the only
    /// typestate accepted by the create-new executor.
    pub fn prepare(
        self,
        approved_plan: LifecyclePlan,
        approved_payload: &[u8],
    ) -> Result<PreparedReconcileReview, WorkspaceError> {
        self.prepare_at(approved_plan, approved_payload, SystemTime::now())
    }

    fn prepare_at(
        self,
        approved_plan: LifecyclePlan,
        approved_payload: &[u8],
        now: SystemTime,
    ) -> Result<PreparedReconcileReview, WorkspaceError> {
        self.prepare_at_with_clock(approved_plan, approved_payload, now, Instant::now())
    }

    fn prepare_at_with_clock(
        self,
        approved_plan: LifecyclePlan,
        approved_payload: &[u8],
        now: SystemTime,
        monotonic_now: Instant,
    ) -> Result<PreparedReconcileReview, WorkspaceError> {
        if self.plan.canonical_bytes()? != approved_plan.canonical_bytes()?
            || self.payload.as_slice() != approved_payload
        {
            return Err(WorkspaceError::new(WorkspaceErrorCode::StalePlan));
        }
        validate_reconcile_plan_shape(&approved_plan, &self.payload_digest)?;
        crate::prepared_plan::validate_time_window(&approved_plan, now)?;
        let (expires_at, expiry_code) =
            bounded_plan_deadline(self.expires_at, &approved_plan, now, monotonic_now)?;
        check_effective(expires_at, expiry_code, &self.cancellation)?;
        bind_reconcile_plan(
            &approved_plan,
            &self.source_handle,
            &self.target_handle,
            self.ancestor_handle.as_ref(),
            &self.destination,
            &self.output,
        )?;
        let remaining = remaining_effective(expires_at, expiry_code, &self.cancellation)?;
        let control = WorkspaceControl::new(remaining, &self.cancellation);
        control.check()?;
        let recomputed_summary = crate::compare_sources(
            &self.source_handle,
            &self.target_handle,
            &crate::CompareLimits {
                deadline: Duration::from_millis(self.expected_summary.limits.deadline_ms),
                operation_deadline: Some(control.remaining()?),
                max_rows_per_table: self.expected_summary.limits.max_rows_per_table,
                max_total_rows: self.expected_summary.limits.max_total_rows,
                max_value_bytes: self.expected_summary.limits.max_value_bytes,
                max_stream_bytes: self.expected_summary.limits.max_stream_bytes,
            },
            &self.cancellation,
        )?;
        if recomputed_summary != self.expected_summary {
            return Err(WorkspaceError::new(WorkspaceErrorCode::StalePlan));
        }
        let rebind = crate::WorkspaceLimits {
            deadline: control.remaining()?,
            ..crate::WorkspaceLimits::default()
        };
        self.source_handle
            .assert_current_with_control(&rebind, &self.cancellation)?;
        let rebind = crate::WorkspaceLimits {
            deadline: control.remaining()?,
            ..crate::WorkspaceLimits::default()
        };
        self.target_handle
            .assert_current_with_control(&rebind, &self.cancellation)?;
        if let Some(ancestor) = &self.ancestor_handle {
            let rebind = crate::WorkspaceLimits {
                deadline: control.remaining()?,
                ..crate::WorkspaceLimits::default()
            };
            ancestor.assert_current_with_control(&rebind, &self.cancellation)?;
        }
        self.destination
            .assert_reserved_current()
            .map_err(crate::prepared_plan::map_prepared_destination_error)?;
        let digest = review_digest(
            &self.compare_report_digest,
            &self.source_ref,
            &self.target_ref,
            self.ancestor_ref.as_ref(),
            &self.operations,
            &self.operation_reviews,
            &self.resolved_conflicts,
            &self.output,
            &self.destination,
            &self.sequence_state,
            &self.limits,
            &self.confirmed_sensitive_dataset_indices,
        )?;
        if digest != self.review_digest {
            return Err(WorkspaceError::new(WorkspaceErrorCode::StalePlan));
        }
        control.check()?;
        Ok(PreparedReconcileReview {
            profile: self.profile,
            plan: approved_plan,
            payload: self.payload,
            payload_digest: self.payload_digest,
            compare_report_digest: self.compare_report_digest,
            source_ref: self.source_ref,
            target_ref: self.target_ref,
            output: self.output,
            destination: self.destination,
            operations: self.operations,
            operation_reviews: self.operation_reviews,
            dataset_states: self.dataset_states,
            sequence_state: self.sequence_state,
            limits: self.limits,
            review_digest: self.review_digest,
            source_handle: self.source_handle,
            target_handle: self.target_handle,
            ancestor_handle: self.ancestor_handle,
            expires_at,
            expiry_code,
            cancellation: self.cancellation,
        })
    }
}

/// One-use, non-serializable capability accepted by the executor typestate.
#[allow(dead_code)]
pub struct PreparedReconcileReview {
    profile: &'static str,
    plan: LifecyclePlan,
    payload: Vec<u8>,
    payload_digest: String,
    compare_report_digest: String,
    source_ref: ReconcileReference,
    target_ref: ReconcileReference,
    output: ReconcileOutputReview,
    destination: DestinationReservation,
    operations: Vec<PlannedOperation>,
    operation_reviews: Vec<ReconcileOperationReview>,
    dataset_states: Vec<ReconcileDatasetStateReview>,
    sequence_state: Vec<SequenceStateReview>,
    limits: ReconcileReviewLimitsApplied,
    review_digest: String,
    source_handle: VerifiedWorkspaceSource,
    target_handle: VerifiedWorkspaceSource,
    ancestor_handle: Option<VerifiedWorkspaceSource>,
    expires_at: Instant,
    expiry_code: WorkspaceErrorCode,
    cancellation: CancellationToken,
}

impl fmt::Debug for PreparedReconcileReview {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedReconcileReview")
            .field("profile", &self.profile)
            .field("operation_count", &self.operation_reviews.len())
            .field("review_digest", &self.review_digest)
            .finish()
    }
}

impl PreparedReconcileReview {
    pub fn review_digest(&self) -> &str {
        &self.review_digest
    }

    pub fn operation_count(&self) -> usize {
        self.operation_reviews.len()
    }

    pub fn stage(self) -> Result<ReconcileStaging, WorkspaceError> {
        check_effective(self.expires_at, self.expiry_code, &self.cancellation)?;
        assert_reconcile_authorities_current(
            &self.source_handle,
            &self.target_handle,
            self.ancestor_handle.as_ref(),
            self.expires_at,
            &self.cancellation,
        )?;
        let private = self
            .destination
            .stage()
            .map_err(crate::prepared_plan::map_prepared_destination_error)?;
        maybe_crash("private-created");
        Ok(ReconcileStaging {
            plan: self.plan,
            payload: self.payload,
            payload_digest: self.payload_digest,
            compare_report_digest: self.compare_report_digest,
            source_ref: self.source_ref,
            target_ref: self.target_ref,
            output: self.output,
            operations: self.operations,
            operation_reviews: self.operation_reviews,
            dataset_states: self.dataset_states,
            sequence_state: self.sequence_state,
            source_handle: self.source_handle,
            target_handle: self.target_handle,
            ancestor_handle: self.ancestor_handle,
            private,
            expires_at: self.expires_at,
            expiry_code: self.expiry_code,
            cancellation: self.cancellation,
        })
    }
}

pub struct ReconcileStaging {
    plan: LifecyclePlan,
    payload: Vec<u8>,
    payload_digest: String,
    compare_report_digest: String,
    source_ref: ReconcileReference,
    target_ref: ReconcileReference,
    output: ReconcileOutputReview,
    operations: Vec<PlannedOperation>,
    operation_reviews: Vec<ReconcileOperationReview>,
    dataset_states: Vec<ReconcileDatasetStateReview>,
    sequence_state: Vec<SequenceStateReview>,
    source_handle: VerifiedWorkspaceSource,
    target_handle: VerifiedWorkspaceSource,
    ancestor_handle: Option<VerifiedWorkspaceSource>,
    private: PrivateOutput,
    expires_at: Instant,
    expiry_code: WorkspaceErrorCode,
    cancellation: CancellationToken,
}

pub struct ValidatedReconcile {
    plan: LifecyclePlan,
    payload: Vec<u8>,
    payload_digest: String,
    compare_report_digest: String,
    output: ReconcileOutputReview,
    operation_reviews: Vec<ReconcileOperationReview>,
    dataset_states: Vec<ReconcileDatasetStateReview>,
    sequence_state: Vec<SequenceStateReview>,
    source_handle: VerifiedWorkspaceSource,
    target_handle: VerifiedWorkspaceSource,
    ancestor_handle: Option<VerifiedWorkspaceSource>,
    sealed: SealedPrivateOutput,
    expires_at: Instant,
    expiry_code: WorkspaceErrorCode,
    cancellation: CancellationToken,
}

pub struct PublishedReconcile {
    inner: PublishedOutput,
}

impl ReconcileStaging {
    /// Copies the exact retained target snapshot, applies only the reviewed
    /// host-generated statements, appends the reviewed lineage event, vacuums
    /// the private artifact and proves all output postconditions.
    pub fn transform_and_validate(mut self) -> Result<ValidatedReconcile, WorkspaceError> {
        check_effective(self.expires_at, self.expiry_code, &self.cancellation)?;
        assert_reconcile_authorities_current(
            &self.source_handle,
            &self.target_handle,
            self.ancestor_handle.as_ref(),
            self.expires_at,
            &self.cancellation,
        )?;
        let verification = sqlite_capsule_launch::VerificationControl::new(
            remaining_effective(self.expires_at, self.expiry_code, &self.cancellation)?,
            self.cancellation.shared_flag(),
        )
        .with_max_bytes(self.plan.limits().max_output_bytes());
        let copied = self
            .target_handle
            .verified
            .copy_snapshot_to_file_with_control(
                self.private.file_mut(),
                &verification,
                self.plan.limits().max_output_bytes(),
            )
            .map_err(map_launch_output_error)?;
        if copied != self.target_handle.source_identity().bytes {
            return Err(verification_failed());
        }
        self.private
            .file_mut()
            .sync_all()
            .map_err(|_| output_failed())?;
        maybe_crash("target-snapshot-copied");
        apply_operations_transaction(
            self.private.private_path_hint(),
            &self.target_handle,
            &self.operations,
            HARD_VALUE_BYTES,
            self.expires_at,
            &self.cancellation,
        )?;
        finalize_reconcile_metadata(
            self.private.private_path_hint(),
            &self.plan,
            &self.payload,
            &self.output,
            &self.source_ref,
            &self.target_ref,
            self.expires_at,
            &self.cancellation,
        )?;
        maybe_crash("transformed");
        vacuum_private(
            self.private.private_path_hint(),
            self.expires_at,
            &self.cancellation,
        )?;
        maybe_crash("vacuumed");
        let staged_path = self.private.private_path_hint().to_path_buf();
        let sealed = self
            .private
            .seal_with_limit(self.plan.limits().max_output_bytes())
            .map_err(crate::prepared_plan::map_destination_error)?;
        sealed
            .assert_staged_current()
            .map_err(crate::prepared_plan::map_destination_error)?;
        let output_handle = open_output_bound(
            &staged_path,
            sealed.identity().bytes,
            *sealed.sha256(),
            self.expires_at,
            &self.cancellation,
        )?;
        validate_reconcile_output(
            &output_handle,
            &self.source_handle,
            &self.target_handle,
            &self.plan,
            &self.payload,
            &self.payload_digest,
            &self.compare_report_digest,
            &self.output,
            &self.operation_reviews,
            &self.dataset_states,
            &self.sequence_state,
            self.expires_at,
            &self.cancellation,
        )?;
        drop(output_handle);
        sealed
            .assert_staged_current()
            .map_err(crate::prepared_plan::map_destination_error)?;
        assert_reconcile_authorities_current(
            &self.source_handle,
            &self.target_handle,
            self.ancestor_handle.as_ref(),
            self.expires_at,
            &self.cancellation,
        )?;
        maybe_crash("sealed-and-verified");
        Ok(ValidatedReconcile {
            plan: self.plan,
            payload: self.payload,
            payload_digest: self.payload_digest,
            compare_report_digest: self.compare_report_digest,
            output: self.output,
            operation_reviews: self.operation_reviews,
            dataset_states: self.dataset_states,
            sequence_state: self.sequence_state,
            source_handle: self.source_handle,
            target_handle: self.target_handle,
            ancestor_handle: self.ancestor_handle,
            sealed,
            expires_at: self.expires_at,
            expiry_code: self.expiry_code,
            cancellation: self.cancellation,
        })
    }
}

impl ValidatedReconcile {
    pub fn publish(self) -> Result<PublishedReconcile, WorkspaceError> {
        self.publish_with_hook(|| {})
    }

    fn publish_with_hook<F>(
        self,
        after_final_output_check: F,
    ) -> Result<PublishedReconcile, WorkspaceError>
    where
        F: FnOnce(),
    {
        check_effective(self.expires_at, self.expiry_code, &self.cancellation)?;
        assert_reconcile_authorities_current(
            &self.source_handle,
            &self.target_handle,
            self.ancestor_handle.as_ref(),
            self.expires_at,
            &self.cancellation,
        )?;
        self.sealed
            .assert_staged_current()
            .map_err(crate::prepared_plan::map_destination_error)?;
        let prepublish = open_output_bound(
            self.sealed.private_path_hint(),
            self.sealed.identity().bytes,
            *self.sealed.sha256(),
            self.expires_at,
            &self.cancellation,
        )?;
        validate_reconcile_output(
            &prepublish,
            &self.source_handle,
            &self.target_handle,
            &self.plan,
            &self.payload,
            &self.payload_digest,
            &self.compare_report_digest,
            &self.output,
            &self.operation_reviews,
            &self.dataset_states,
            &self.sequence_state,
            self.expires_at,
            &self.cancellation,
        )?;
        drop(prepublish);
        self.sealed
            .assert_staged_current()
            .map_err(crate::prepared_plan::map_destination_error)?;
        assert_reconcile_authorities_current(
            &self.source_handle,
            &self.target_handle,
            self.ancestor_handle.as_ref(),
            self.expires_at,
            &self.cancellation,
        )?;
        let plan = &self.plan;
        let payload = self.payload.clone();
        let payload_digest = self.payload_digest.clone();
        let compare_report_digest = self.compare_report_digest.clone();
        let output_review = self.output.clone();
        let operation_reviews = self.operation_reviews.clone();
        let dataset_states = self.dataset_states.clone();
        let sequence_state = self.sequence_state.clone();
        let source = &self.source_handle;
        let target = &self.target_handle;
        let ancestor = self.ancestor_handle.as_ref();
        let deadline = self.expires_at;
        let cancellation = self.cancellation.clone();
        let max_output_bytes = plan.limits().max_output_bytes();
        // SAFETY: the exact sealed target-derived bytes were exhaustively
        // validated above. The callback snapshots the held reopened file,
        // repeats every postcondition and performs the final two-input rebind
        // while lifecycle quarantine is still available.
        let published = unsafe {
            self.sealed
                .publish_no_replace_unchecked(|reopened, reopened_identity| {
                    let snapshot =
                        snapshot_held_file(reopened, max_output_bytes, deadline, &cancellation)?;
                    let output_handle = open_output(snapshot.path(), deadline, &cancellation)
                        .map_err(|_| {
                            sqlite_capsule_lifecycle::LifecycleError::PostPublishVerification
                        })?;
                    if output_handle.source_identity().bytes != reopened_identity.bytes
                        || validate_reconcile_output(
                            &output_handle,
                            source,
                            target,
                            plan,
                            &payload,
                            &payload_digest,
                            &compare_report_digest,
                            &output_review,
                            &operation_reviews,
                            &dataset_states,
                            &sequence_state,
                            deadline,
                            &cancellation,
                        )
                        .is_err()
                    {
                        return Err(
                            sqlite_capsule_lifecycle::LifecycleError::PostPublishVerification,
                        );
                    }
                    maybe_crash("postrename-reopened");
                    after_final_output_check();
                    assert_reconcile_authorities_current(
                        source,
                        target,
                        ancestor,
                        deadline,
                        &cancellation,
                    )
                    .map_err(|_| {
                        sqlite_capsule_lifecycle::LifecycleError::PostPublishVerification
                    })?;
                    Ok(())
                })
        }
        .map_err(crate::prepared_plan::map_destination_error)?;
        Ok(PublishedReconcile { inner: published })
    }
}

impl PublishedReconcile {
    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    pub fn identity(&self) -> &SourceIdentity {
        &self.inner.identity
    }
}

struct PlannedOperation {
    action: ReconcileAction,
    basis: ReconcileOperationBasis,
    dataset_index: usize,
    table_index: usize,
    key: Vec<(String, CompareValue)>,
    source_values: Option<Vec<(String, CompareValue)>>,
    target_values: Option<Vec<(String, CompareValue)>>,
    write_set_digest: String,
    target_row_digest: Option<String>,
    ancestor_row_digest: Option<String>,
    conflict_id: Option<String>,
}

#[derive(Clone)]
struct RowSnapshot {
    key: Vec<(String, CompareValue)>,
    key_digest: String,
    row_digest: String,
    compared_values: Vec<(String, CompareValue)>,
    writable_values: Vec<(String, CompareValue)>,
}

struct TableSnapshots {
    columns: Vec<String>,
    writable_columns: Vec<String>,
    source: BTreeMap<String, RowSnapshot>,
    target: BTreeMap<String, RowSnapshot>,
}

struct ThreeWayTableSnapshots {
    columns: Vec<String>,
    writable_columns: Vec<String>,
    source: BTreeMap<String, RowSnapshot>,
    target: BTreeMap<String, RowSnapshot>,
    ancestor: BTreeMap<String, RowSnapshot>,
}

struct ThreeWayClassifiedChange {
    selection: ReconcileSelection,
    ancestor_row_digest: Option<String>,
}

struct ThreeWayClassifiedConflict {
    review: ThreeWayConflictReview,
    take_source: Option<ThreeWayClassifiedChange>,
}

#[derive(Clone)]
struct ReconcileTableOrder {
    rank_by_table: BTreeMap<(usize, usize), usize>,
    table_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ReconcileOperationOrderKey {
    delete_phase: u8,
    dependency_rank: usize,
    action_phase: u8,
    dataset_index: usize,
    table_index: usize,
    canonical_key: Vec<u8>,
}

/// Non-serializable three-way classification authority. It owns all three
/// verified inputs and exposes only digest-based clean-change/conflict review.
pub struct ThreeWayReconcileReview {
    expected_summary: CompareSummary,
    ancestor_ref: ReconcileReference,
    source_ref: ReconcileReference,
    target_ref: ReconcileReference,
    clean_changes: Vec<ThreeWayClassifiedChange>,
    conflicts: Vec<ThreeWayClassifiedConflict>,
    snapshots: BTreeMap<(usize, usize), ThreeWayTableSnapshots>,
    ancestor_handle: VerifiedWorkspaceSource,
    source_handle: VerifiedWorkspaceSource,
    target_handle: VerifiedWorkspaceSource,
    limits: ReconcileReviewLimitsApplied,
    expires_at: Instant,
    cancellation: CancellationToken,
    confirmed_sensitive_dataset_indices: BTreeSet<usize>,
    table_order: ReconcileTableOrder,
}

impl fmt::Debug for ThreeWayReconcileReview {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ThreeWayReconcileReview")
            .field("clean_change_count", &self.clean_changes.len())
            .field("conflict_count", &self.conflicts.len())
            .finish()
    }
}

impl ThreeWayReconcileReview {
    pub fn ancestor(&self) -> &ReconcileReference {
        &self.ancestor_ref
    }

    pub fn clean_change_count(&self) -> usize {
        self.clean_changes.len()
    }

    /// Conservative remaining lifetime of this exact three-input authority.
    /// The shell must not start a resolution workflow that can outlive the
    /// retained classification snapshot.
    pub fn remaining_lifetime(&self) -> Result<Duration, WorkspaceError> {
        remaining_effective(
            self.expires_at,
            WorkspaceErrorCode::SessionExpired,
            &self.cancellation,
        )
    }

    pub fn conflicts(&self) -> impl ExactSizeIterator<Item = &ThreeWayConflictReview> {
        self.conflicts.iter().map(|conflict| &conflict.review)
    }

    pub fn resolve(
        self,
        resolutions: &[ThreeWayConflictResolution],
        output: &ReconcileOutputRequest,
        work_budget: Duration,
    ) -> Result<ReconcileReview, WorkspaceError> {
        resolve_three_way_review(self, resolutions, output, work_budget)
    }
}

#[allow(clippy::too_many_arguments)]
pub fn prepare_reconcile_review(
    source: VerifiedWorkspaceSource,
    target: VerifiedWorkspaceSource,
    expected_summary: &CompareSummary,
    selections: &[ReconcileSelection],
    confirmed_sensitive_dataset_indices: &BTreeSet<usize>,
    output: &ReconcileOutputRequest,
    requested: &ReconcileReviewLimits,
    cancellation: &CancellationToken,
) -> Result<ReconcileReview, WorkspaceError> {
    let limits = effective_limits(requested)?;
    validate_global_reconcile_policy(&source, &target)?;
    let expires_at = Instant::now()
        .checked_add(Duration::from_millis(limits.deadline_ms))
        .ok_or_else(limit_exceeded)?;
    let control = WorkspaceControl::new(remaining(expires_at)?, cancellation);
    let recomputed_summary = crate::compare_sources(
        &source,
        &target,
        &crate::CompareLimits {
            deadline: Duration::from_millis(expected_summary.limits.deadline_ms),
            operation_deadline: Some(control.remaining()?),
            max_rows_per_table: expected_summary.limits.max_rows_per_table,
            max_total_rows: expected_summary.limits.max_total_rows,
            max_value_bytes: expected_summary.limits.max_value_bytes,
            max_stream_bytes: expected_summary.limits.max_stream_bytes,
        },
        cancellation,
    )?;
    if &recomputed_summary != expected_summary {
        return Err(WorkspaceError::new(WorkspaceErrorCode::StalePlan));
    }
    control.install(source.verified.connection())?;
    if let Err(error) = control.install(target.verified.connection()) {
        clear_progress(source.verified.connection());
        return Err(error);
    }
    let result = prepare_inner(
        source,
        target,
        expected_summary,
        selections,
        confirmed_sensitive_dataset_indices,
        output,
        &limits,
        &control,
        expires_at,
        cancellation,
    );
    match result {
        Ok(review) => Ok(review),
        Err(error) => {
            control.check()?;
            Err(error)
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn classify_three_way_reconcile(
    ancestor: VerifiedWorkspaceSource,
    source: VerifiedWorkspaceSource,
    target: VerifiedWorkspaceSource,
    expected_summary: &CompareSummary,
    confirmed_sensitive_dataset_indices: &BTreeSet<usize>,
    requested: &ReconcileReviewLimits,
    cancellation: &CancellationToken,
) -> Result<ThreeWayReconcileReview, WorkspaceError> {
    let limits = effective_limits(requested)?;
    validate_global_reconcile_policy(&source, &target)?;
    let started = Instant::now();
    let work_expires_at = started
        .checked_add(Duration::from_millis(limits.deadline_ms))
        .ok_or_else(limit_exceeded)?;
    let authority_expires_at = started
        .checked_add(Duration::from_millis(limits.review_lifetime_ms))
        .ok_or_else(limit_exceeded)?;
    let control = WorkspaceControl::new(remaining(work_expires_at)?, cancellation);
    let recomputed_summary = crate::compare_sources(
        &source,
        &target,
        &crate::CompareLimits {
            deadline: Duration::from_millis(expected_summary.limits.deadline_ms),
            operation_deadline: Some(control.remaining()?),
            max_rows_per_table: expected_summary.limits.max_rows_per_table,
            max_total_rows: expected_summary.limits.max_total_rows,
            max_value_bytes: expected_summary.limits.max_value_bytes,
            max_stream_bytes: expected_summary.limits.max_stream_bytes,
        },
        cancellation,
    )?;
    if &recomputed_summary != expected_summary {
        return Err(WorkspaceError::new(WorkspaceErrorCode::StalePlan));
    }
    validate_summary_binding(&source, &target, expected_summary)?;
    validate_ancestor_compatibility(&ancestor, &source, &target)?;
    control.install(ancestor.verified.connection())?;
    if let Err(error) = control.install(source.verified.connection()) {
        clear_progress(ancestor.verified.connection());
        return Err(error);
    }
    if let Err(error) = control.install(target.verified.connection()) {
        clear_progress(ancestor.verified.connection());
        clear_progress(source.verified.connection());
        return Err(error);
    }
    let result = classify_three_way_inner(
        ancestor,
        source,
        target,
        expected_summary,
        confirmed_sensitive_dataset_indices,
        limits,
        &control,
        work_expires_at,
        authority_expires_at,
        cancellation,
    );
    match result {
        Ok(review) => Ok(review),
        Err(error) => {
            control.check()?;
            Err(error)
        }
    }
}

fn validate_ancestor_compatibility(
    ancestor: &VerifiedWorkspaceSource,
    source: &VerifiedWorkspaceSource,
    target: &VerifiedWorkspaceSource,
) -> Result<(), WorkspaceError> {
    let ancestor_schema = ancestor
        .identity()
        .overview
        .data_schema
        .as_ref()
        .ok_or_else(invalid_contract)?;
    let source_schema = source
        .identity()
        .overview
        .data_schema
        .as_ref()
        .ok_or_else(invalid_contract)?;
    let target_schema = target
        .identity()
        .overview
        .data_schema
        .as_ref()
        .ok_or_else(invalid_contract)?;
    if !ancestor.has_complete_valid_signature_inventory()
        || !source.has_complete_valid_signature_inventory()
        || !target.has_complete_valid_signature_inventory()
    {
        return Err(WorkspaceError::new(WorkspaceErrorCode::InvalidSignature));
    }
    if ancestor.identity().app_id != source.identity().app_id
        || ancestor.identity().app_id != target.identity().app_id
        || ancestor.application_digest() != source.application_digest()
        || ancestor.application_digest() != target.application_digest()
        || ancestor_schema.data_schema_id != source_schema.data_schema_id
        || ancestor_schema.data_schema_id != target_schema.data_schema_id
        || ancestor_schema.data_schema_version != source_schema.data_schema_version
        || ancestor_schema.data_schema_version != target_schema.data_schema_version
        || ancestor.data_contract() != source.data_contract()
        || ancestor.data_contract() != target.data_contract()
    {
        return Err(WorkspaceError::new(WorkspaceErrorCode::IncompatibleSchema));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn classify_three_way_inner(
    ancestor: VerifiedWorkspaceSource,
    source: VerifiedWorkspaceSource,
    target: VerifiedWorkspaceSource,
    expected_summary: &CompareSummary,
    confirmed_sensitive_dataset_indices: &BTreeSet<usize>,
    limits: ReconcileReviewLimitsApplied,
    control: &WorkspaceControl,
    work_expires_at: Instant,
    authority_expires_at: Instant,
    cancellation: &CancellationToken,
) -> Result<ThreeWayReconcileReview, WorkspaceError> {
    let ancestor_ref = reconcile_reference(&ancestor, control)?;
    let source_ref = reconcile_reference(&source, control)?;
    let target_ref = reconcile_reference(&target, control)?;
    let mut total_rows = 0_u64;
    let mut stream_bytes = 0_u64;
    let mut retained_bytes = 0_u64;
    let mut snapshots = BTreeMap::new();
    let mut clean_changes = Vec::new();
    let mut conflicts = Vec::new();
    let table_order = reconcile_table_order(&source, &target, control)?;
    for (dataset_index, dataset) in source.data_contract().datasets.iter().enumerate() {
        control.check()?;
        let _summary = expected_summary
            .datasets
            .iter()
            .find(|candidate| candidate.dataset_id == dataset.id)
            .ok_or_else(invalid_contract)?;
        if dataset.reconcile != ReconcilePolicy::ThreeWay {
            continue;
        }
        if !matches!(dataset.compare, ComparePolicy::Row | ComparePolicy::Field) {
            return Err(WorkspaceError::new(
                WorkspaceErrorCode::UnsupportedOperation,
            ));
        }
        for (table_index, table) in dataset.tables.iter().enumerate() {
            control.check()?;
            let columns =
                crate::compare::compared_columns(source.verified.connection(), table, control)?;
            if columns
                != crate::compare::compared_columns(target.verified.connection(), table, control)?
                || columns
                    != crate::compare::compared_columns(
                        ancestor.verified.connection(),
                        table,
                        control,
                    )?
            {
                return Err(WorkspaceError::new(WorkspaceErrorCode::IncompatibleSchema));
            }
            let writable = writable_columns(source.verified.connection(), table, control)?;
            if writable != writable_columns(target.verified.connection(), table, control)?
                || writable != writable_columns(ancestor.verified.connection(), table, control)?
            {
                return Err(WorkspaceError::new(WorkspaceErrorCode::IncompatibleSchema));
            }
            let source_rows = load_all_rows(
                source.verified.connection(),
                table,
                &columns,
                &writable,
                &limits,
                &mut total_rows,
                &mut stream_bytes,
                &mut retained_bytes,
                control,
            )?;
            let target_rows = load_all_rows(
                target.verified.connection(),
                table,
                &columns,
                &writable,
                &limits,
                &mut total_rows,
                &mut stream_bytes,
                &mut retained_bytes,
                control,
            )?;
            let ancestor_rows = load_all_rows(
                ancestor.verified.connection(),
                table,
                &columns,
                &writable,
                &limits,
                &mut total_rows,
                &mut stream_bytes,
                &mut retained_bytes,
                control,
            )?;
            let table_snapshots = ThreeWayTableSnapshots {
                columns,
                writable_columns: writable,
                source: source_rows,
                target: target_rows,
                ancestor: ancestor_rows,
            };
            let mut keys = BTreeSet::new();
            keys.extend(table_snapshots.source.keys().cloned());
            keys.extend(table_snapshots.target.keys().cloned());
            keys.extend(table_snapshots.ancestor.keys().cloned());
            for key_digest in keys {
                control.check()?;
                classify_three_way_row(
                    dataset_index,
                    table_index,
                    dataset,
                    table,
                    &key_digest,
                    &table_snapshots,
                    &ancestor_ref,
                    &source_ref,
                    &target_ref,
                    limits.max_value_bytes,
                    &mut clean_changes,
                    &mut conflicts,
                )?;
                if clean_changes.len().saturating_add(conflicts.len()) > limits.max_operations {
                    return Err(limit_exceeded());
                }
            }
            snapshots.insert((dataset_index, table_index), table_snapshots);
        }
    }
    if clean_changes.is_empty() && conflicts.is_empty() {
        return Err(WorkspaceError::new(
            WorkspaceErrorCode::UnsupportedOperation,
        ));
    }
    let required_sensitive = clean_changes
        .iter()
        .map(|change| change.selection.dataset_index)
        .chain(conflicts.iter().filter_map(|conflict| {
            source
                .data_contract()
                .datasets
                .iter()
                .position(|dataset| dataset.id == conflict.review.dataset_id)
        }))
        .filter(|index| {
            source.data_contract().datasets[*index].sensitivity == Sensitivity::Sensitive
        })
        .collect::<BTreeSet<_>>();
    validate_sensitive_confirmations(&required_sensitive, confirmed_sensitive_dataset_indices)?;
    clear_progress(ancestor.verified.connection());
    clear_progress(source.verified.connection());
    clear_progress(target.verified.connection());
    assert_reconcile_authorities_current(
        &source,
        &target,
        Some(&ancestor),
        work_expires_at,
        cancellation,
    )?;
    control.check()?;
    Ok(ThreeWayReconcileReview {
        expected_summary: expected_summary.clone(),
        ancestor_ref,
        source_ref,
        target_ref,
        clean_changes,
        conflicts,
        snapshots,
        ancestor_handle: ancestor,
        source_handle: source,
        target_handle: target,
        limits,
        expires_at: authority_expires_at,
        cancellation: cancellation.clone(),
        confirmed_sensitive_dataset_indices: confirmed_sensitive_dataset_indices.clone(),
        table_order,
    })
}

#[allow(clippy::too_many_arguments)]
fn classify_three_way_row(
    dataset_index: usize,
    table_index: usize,
    dataset: &Dataset,
    table: &DatasetTable,
    key_digest: &str,
    snapshots: &ThreeWayTableSnapshots,
    ancestor_ref: &ReconcileReference,
    source_ref: &ReconcileReference,
    target_ref: &ReconcileReference,
    max_value_bytes: u64,
    clean: &mut Vec<ThreeWayClassifiedChange>,
    conflicts: &mut Vec<ThreeWayClassifiedConflict>,
) -> Result<(), WorkspaceError> {
    let source = snapshots.source.get(key_digest);
    let target = snapshots.target.get(key_digest);
    let ancestor = snapshots.ancestor.get(key_digest);
    let same = |left: Option<&RowSnapshot>, right: Option<&RowSnapshot>| {
        left.map(|row| row.row_digest.as_str()) == right.map(|row| row.row_digest.as_str())
    };
    if same(source, target) || (same(source, ancestor) && target.is_some()) {
        return Ok(());
    }
    let ancestor_digest = ancestor.map(|row| row.row_digest.clone());
    match (ancestor, source, target) {
        (None, Some(source), None) => clean.push(ThreeWayClassifiedChange {
            selection: selection_for_rows(
                dataset_index,
                table_index,
                key_digest,
                Some(source),
                None,
                ReconcileAction::InsertFromSource,
                Vec::new(),
            ),
            ancestor_row_digest: None,
        }),
        (None, Some(source), Some(target)) => {
            let immutable =
                immutable_values_differ(table, snapshots, source, target, max_value_bytes)?;
            let kind = if immutable {
                ThreeWayConflictKind::ImmutableField
            } else {
                ThreeWayConflictKind::InsertInsert
            };
            let take_source = (!immutable)
                .then(|| {
                    differing_mutable_indices(table, snapshots, source, target, max_value_bytes)
                })
                .transpose()?
                .filter(|indices| !indices.is_empty())
                .map(|indices| ThreeWayClassifiedChange {
                    selection: selection_for_rows(
                        dataset_index,
                        table_index,
                        key_digest,
                        Some(source),
                        Some(target),
                        ReconcileAction::SetFields,
                        indices,
                    ),
                    ancestor_row_digest: None,
                });
            conflicts.push(make_three_way_conflict(
                dataset,
                table,
                key_digest,
                kind,
                None,
                Some(source),
                Some(target),
                ancestor,
                take_source,
                ancestor_ref,
                source_ref,
                target_ref,
            )?);
        }
        (Some(ancestor), None, Some(target)) => {
            if same(Some(target), Some(ancestor)) {
                clean.push(ThreeWayClassifiedChange {
                    selection: selection_for_rows(
                        dataset_index,
                        table_index,
                        key_digest,
                        None,
                        Some(target),
                        ReconcileAction::DeleteFromTarget,
                        Vec::new(),
                    ),
                    ancestor_row_digest: ancestor_digest,
                });
            } else {
                let take_source = Some(ThreeWayClassifiedChange {
                    selection: selection_for_rows(
                        dataset_index,
                        table_index,
                        key_digest,
                        None,
                        Some(target),
                        ReconcileAction::DeleteFromTarget,
                        Vec::new(),
                    ),
                    ancestor_row_digest: ancestor_digest.clone(),
                });
                conflicts.push(make_three_way_conflict(
                    dataset,
                    table,
                    key_digest,
                    ThreeWayConflictKind::DeleteUpdate,
                    Some(ThreeWayDeletedSide::Source),
                    source,
                    Some(target),
                    Some(ancestor),
                    take_source,
                    ancestor_ref,
                    source_ref,
                    target_ref,
                )?);
            }
        }
        (Some(ancestor), Some(source), None) => {
            if !same(Some(source), Some(ancestor)) {
                let immutable =
                    immutable_values_differ(table, snapshots, source, ancestor, max_value_bytes)?;
                let kind = if immutable {
                    ThreeWayConflictKind::ImmutableField
                } else {
                    ThreeWayConflictKind::DeleteUpdate
                };
                let take_source = (!immutable).then(|| ThreeWayClassifiedChange {
                    selection: selection_for_rows(
                        dataset_index,
                        table_index,
                        key_digest,
                        Some(source),
                        None,
                        ReconcileAction::InsertFromSource,
                        Vec::new(),
                    ),
                    ancestor_row_digest: ancestor_digest.clone(),
                });
                conflicts.push(make_three_way_conflict(
                    dataset,
                    table,
                    key_digest,
                    kind,
                    (kind == ThreeWayConflictKind::DeleteUpdate)
                        .then_some(ThreeWayDeletedSide::Target),
                    Some(source),
                    target,
                    Some(ancestor),
                    take_source,
                    ancestor_ref,
                    source_ref,
                    target_ref,
                )?);
            }
        }
        (Some(ancestor), Some(source), Some(target)) => {
            classify_present_three_way_row(
                dataset_index,
                table_index,
                dataset,
                table,
                key_digest,
                snapshots,
                ancestor,
                source,
                target,
                ancestor_ref,
                source_ref,
                target_ref,
                max_value_bytes,
                clean,
                conflicts,
            )?;
        }
        (None, None, _) | (Some(_), None, None) => {}
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn classify_present_three_way_row(
    dataset_index: usize,
    table_index: usize,
    dataset: &Dataset,
    table: &DatasetTable,
    key_digest: &str,
    snapshots: &ThreeWayTableSnapshots,
    ancestor: &RowSnapshot,
    source: &RowSnapshot,
    target: &RowSnapshot,
    ancestor_ref: &ReconcileReference,
    source_ref: &ReconcileReference,
    target_ref: &ReconcileReference,
    max_value_bytes: u64,
    clean: &mut Vec<ThreeWayClassifiedChange>,
    conflicts: &mut Vec<ThreeWayClassifiedConflict>,
) -> Result<(), WorkspaceError> {
    let ancestor_digest = Some(ancestor.row_digest.clone());
    if target.row_digest == ancestor.row_digest {
        let immutable = immutable_values_differ(table, snapshots, source, target, max_value_bytes)?;
        if immutable {
            conflicts.push(make_three_way_conflict(
                dataset,
                table,
                key_digest,
                ThreeWayConflictKind::ImmutableField,
                None,
                Some(source),
                Some(target),
                Some(ancestor),
                None,
                ancestor_ref,
                source_ref,
                target_ref,
            )?);
            return Ok(());
        }
        let action = if dataset.compare == ComparePolicy::Field {
            ReconcileAction::SetFields
        } else {
            ReconcileAction::ReplaceRowFromSource
        };
        let indices = if action == ReconcileAction::SetFields {
            differing_mutable_indices(table, snapshots, source, target, max_value_bytes)?
        } else {
            Vec::new()
        };
        clean.push(ThreeWayClassifiedChange {
            selection: selection_for_rows(
                dataset_index,
                table_index,
                key_digest,
                Some(source),
                Some(target),
                action,
                indices,
            ),
            ancestor_row_digest: ancestor_digest,
        });
        return Ok(());
    }
    if dataset.compare == ComparePolicy::Row {
        conflicts.push(make_update_conflict(
            dataset_index,
            table_index,
            dataset,
            table,
            key_digest,
            snapshots,
            ancestor,
            source,
            target,
            ancestor_ref,
            source_ref,
            target_ref,
            max_value_bytes,
        )?);
        return Ok(());
    }
    let mut take_indices = Vec::new();
    let mut has_conflict = false;
    let mut immutable_conflict = false;
    for (index, column) in snapshots.columns.iter().enumerate() {
        let ancestor_value = &ancestor.compared_values[index].1;
        let source_value = &source.compared_values[index].1;
        let target_value = &target.compared_values[index].1;
        if values_equal(source_value, target_value, max_value_bytes)?
            || values_equal(source_value, ancestor_value, max_value_bytes)?
        {
            continue;
        }
        if values_equal(target_value, ancestor_value, max_value_bytes)? {
            if table.immutable_columns.contains(column) || table.primary_key.contains(column) {
                immutable_conflict = true;
            } else if snapshots.writable_columns.contains(column) {
                take_indices.push(index);
            }
        } else {
            has_conflict = true;
            if table.immutable_columns.contains(column) || table.primary_key.contains(column) {
                immutable_conflict = true;
            }
        }
    }
    if has_conflict || immutable_conflict {
        let kind = if immutable_conflict {
            ThreeWayConflictKind::ImmutableField
        } else {
            ThreeWayConflictKind::UpdateUpdate
        };
        let take_source = (kind != ThreeWayConflictKind::ImmutableField)
            .then(|| differing_mutable_indices(table, snapshots, source, target, max_value_bytes))
            .transpose()?
            .filter(|indices| !indices.is_empty())
            .map(|indices| ThreeWayClassifiedChange {
                selection: selection_for_rows(
                    dataset_index,
                    table_index,
                    key_digest,
                    Some(source),
                    Some(target),
                    ReconcileAction::SetFields,
                    indices,
                ),
                ancestor_row_digest: ancestor_digest,
            });
        conflicts.push(make_three_way_conflict(
            dataset,
            table,
            key_digest,
            kind,
            None,
            Some(source),
            Some(target),
            Some(ancestor),
            take_source,
            ancestor_ref,
            source_ref,
            target_ref,
        )?);
    } else if !take_indices.is_empty() {
        clean.push(ThreeWayClassifiedChange {
            selection: selection_for_rows(
                dataset_index,
                table_index,
                key_digest,
                Some(source),
                Some(target),
                ReconcileAction::SetFields,
                take_indices,
            ),
            ancestor_row_digest: ancestor_digest,
        });
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn make_update_conflict(
    dataset_index: usize,
    table_index: usize,
    dataset: &Dataset,
    table: &DatasetTable,
    key_digest: &str,
    snapshots: &ThreeWayTableSnapshots,
    ancestor: &RowSnapshot,
    source: &RowSnapshot,
    target: &RowSnapshot,
    ancestor_ref: &ReconcileReference,
    source_ref: &ReconcileReference,
    target_ref: &ReconcileReference,
    max_value_bytes: u64,
) -> Result<ThreeWayClassifiedConflict, WorkspaceError> {
    let immutable = immutable_values_differ(table, snapshots, source, target, max_value_bytes)?;
    let kind = if immutable {
        ThreeWayConflictKind::ImmutableField
    } else {
        ThreeWayConflictKind::UpdateUpdate
    };
    let take_source = (!immutable).then(|| ThreeWayClassifiedChange {
        selection: selection_for_rows(
            dataset_index,
            table_index,
            key_digest,
            Some(source),
            Some(target),
            ReconcileAction::ReplaceRowFromSource,
            Vec::new(),
        ),
        ancestor_row_digest: Some(ancestor.row_digest.clone()),
    });
    make_three_way_conflict(
        dataset,
        table,
        key_digest,
        kind,
        None,
        Some(source),
        Some(target),
        Some(ancestor),
        take_source,
        ancestor_ref,
        source_ref,
        target_ref,
    )
}

fn selection_for_rows(
    dataset_index: usize,
    table_index: usize,
    key_digest: &str,
    source: Option<&RowSnapshot>,
    target: Option<&RowSnapshot>,
    action: ReconcileAction,
    field_indices: Vec<usize>,
) -> ReconcileSelection {
    ReconcileSelection {
        dataset_index,
        table_index,
        key_digest: key_digest.to_owned(),
        source_row_digest: source.map(|row| row.row_digest.clone()),
        target_row_digest: target.map(|row| row.row_digest.clone()),
        action,
        field_indices,
    }
}

fn immutable_values_differ(
    table: &DatasetTable,
    snapshots: &ThreeWayTableSnapshots,
    left: &RowSnapshot,
    right: &RowSnapshot,
    max_value_bytes: u64,
) -> Result<bool, WorkspaceError> {
    for immutable in &table.immutable_columns {
        let index = snapshots
            .columns
            .iter()
            .position(|column| column == immutable)
            .ok_or_else(invalid_contract)?;
        if !values_equal(
            &left.compared_values[index].1,
            &right.compared_values[index].1,
            max_value_bytes,
        )? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn differing_mutable_indices(
    table: &DatasetTable,
    snapshots: &ThreeWayTableSnapshots,
    source: &RowSnapshot,
    target: &RowSnapshot,
    max_value_bytes: u64,
) -> Result<Vec<usize>, WorkspaceError> {
    let mut result = Vec::new();
    for (index, column) in snapshots.columns.iter().enumerate() {
        if !table.primary_key.contains(column)
            && !table.immutable_columns.contains(column)
            && snapshots.writable_columns.contains(column)
            && !values_equal(
                &source.compared_values[index].1,
                &target.compared_values[index].1,
                max_value_bytes,
            )?
        {
            result.push(index);
        }
    }
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
fn make_three_way_conflict(
    dataset: &Dataset,
    table: &DatasetTable,
    key_digest: &str,
    kind: ThreeWayConflictKind,
    deleted_side: Option<ThreeWayDeletedSide>,
    source: Option<&RowSnapshot>,
    target: Option<&RowSnapshot>,
    ancestor: Option<&RowSnapshot>,
    take_source: Option<ThreeWayClassifiedChange>,
    ancestor_ref: &ReconcileReference,
    source_ref: &ReconcileReference,
    target_ref: &ReconcileReference,
) -> Result<ThreeWayClassifiedConflict, WorkspaceError> {
    let id = three_way_conflict_id(
        dataset,
        table,
        key_digest,
        kind,
        deleted_side,
        source.map(|row| row.row_digest.as_str()),
        target.map(|row| row.row_digest.as_str()),
        ancestor.map(|row| row.row_digest.as_str()),
        ancestor_ref,
        source_ref,
        target_ref,
    )?;
    let allowed_choices = if kind == ThreeWayConflictKind::ImmutableField {
        vec![ThreeWayResolutionChoice::KeepTarget]
    } else {
        vec![
            ThreeWayResolutionChoice::KeepTarget,
            ThreeWayResolutionChoice::TakeSource,
        ]
    };
    Ok(ThreeWayClassifiedConflict {
        review: ThreeWayConflictReview {
            id,
            dataset_id: dataset.id.clone(),
            table: table.name.clone(),
            key_digest: key_digest.to_owned(),
            kind,
            deleted_side,
            source_row_digest: source.map(|row| row.row_digest.clone()),
            target_row_digest: target.map(|row| row.row_digest.clone()),
            ancestor_row_digest: ancestor.map(|row| row.row_digest.clone()),
            allowed_choices,
        },
        take_source,
    })
}

#[allow(clippy::too_many_arguments)]
fn three_way_conflict_id(
    dataset: &Dataset,
    table: &DatasetTable,
    key_digest: &str,
    kind: ThreeWayConflictKind,
    deleted_side: Option<ThreeWayDeletedSide>,
    source_state: Option<&str>,
    target_state: Option<&str>,
    ancestor_state: Option<&str>,
    ancestor_ref: &ReconcileReference,
    source_ref: &ReconcileReference,
    target_ref: &ReconcileReference,
) -> Result<String, WorkspaceError> {
    let mut frame = Vec::new();
    frame_text(&mut frame, RECONCILE_CONFLICT_ID_PROFILE)?;
    frame_text(&mut frame, &dataset.id)?;
    frame_text(&mut frame, &table.name)?;
    frame_text(&mut frame, key_digest)?;
    frame_text(&mut frame, kind.label())?;
    frame_optional_text(&mut frame, deleted_side.map(ThreeWayDeletedSide::label))?;
    frame_optional_text(&mut frame, source_state)?;
    frame_optional_text(&mut frame, target_state)?;
    frame_optional_text(&mut frame, ancestor_state)?;
    frame_text(&mut frame, &ancestor_ref.file_sha256)?;
    frame_text(&mut frame, &source_ref.file_sha256)?;
    frame_text(&mut frame, &target_ref.file_sha256)?;
    Ok(lower_hex(&Sha256::digest(frame)))
}

fn resolve_three_way_review(
    review: ThreeWayReconcileReview,
    resolutions: &[ThreeWayConflictResolution],
    output_request: &ReconcileOutputRequest,
    requested_work_budget: Duration,
) -> Result<ReconcileReview, WorkspaceError> {
    let cancellation = review.cancellation.clone();
    let work_budget = requested_work_budget.min(Duration::from_millis(review.limits.deadline_ms));
    if work_budget.is_zero() {
        return Err(limit_exceeded());
    }
    let (work_expires_at, expiry_code) =
        bounded_authority_work_deadline(review.expires_at, work_budget, Instant::now())?;
    check_effective(work_expires_at, expiry_code, &cancellation)?;
    let result = resolve_three_way_inner(review, resolutions, output_request, work_expires_at);
    match result {
        Ok(review) => Ok(review),
        Err(error) => {
            check_effective(work_expires_at, expiry_code, &cancellation)?;
            Err(error)
        }
    }
}

fn resolve_three_way_inner(
    review: ThreeWayReconcileReview,
    resolutions: &[ThreeWayConflictResolution],
    output_request: &ReconcileOutputRequest,
    work_expires_at: Instant,
) -> Result<ReconcileReview, WorkspaceError> {
    if resolutions.len() > review.limits.max_operations {
        return Err(limit_exceeded());
    }
    if resolutions.len() != review.conflicts.len() {
        return Err(WorkspaceError::new(WorkspaceErrorCode::ConflictsUnresolved));
    }
    let mut choices = BTreeMap::new();
    for resolution in resolutions {
        validate_digest(&resolution.conflict_id)?;
        if choices
            .insert(resolution.conflict_id.clone(), resolution.choice)
            .is_some()
        {
            return Err(invalid_contract());
        }
    }
    let recomputed_summary = crate::compare_sources(
        &review.source_handle,
        &review.target_handle,
        &crate::CompareLimits {
            deadline: Duration::from_millis(review.expected_summary.limits.deadline_ms),
            operation_deadline: Some(remaining(work_expires_at)?),
            max_rows_per_table: review.expected_summary.limits.max_rows_per_table,
            max_total_rows: review.expected_summary.limits.max_total_rows,
            max_value_bytes: review.expected_summary.limits.max_value_bytes,
            max_stream_bytes: review.expected_summary.limits.max_stream_bytes,
        },
        &review.cancellation,
    )?;
    if recomputed_summary != review.expected_summary {
        return Err(WorkspaceError::new(WorkspaceErrorCode::StalePlan));
    }
    assert_reconcile_authorities_current(
        &review.source_handle,
        &review.target_handle,
        Some(&review.ancestor_handle),
        work_expires_at,
        &review.cancellation,
    )?;
    let mut authorized = review
        .clean_changes
        .iter()
        .map(|change| {
            (
                change,
                ReconcileOperationBasis::ThreeWayClean,
                None::<String>,
            )
        })
        .collect::<Vec<_>>();
    let mut resolved_conflicts = Vec::with_capacity(review.conflicts.len());
    for conflict in &review.conflicts {
        let choice = choices
            .remove(&conflict.review.id)
            .ok_or_else(|| WorkspaceError::new(WorkspaceErrorCode::ConflictsUnresolved))?;
        if !conflict.review.allowed_choices.contains(&choice) {
            return Err(WorkspaceError::new(WorkspaceErrorCode::ImmutableColumn));
        }
        if choice == ThreeWayResolutionChoice::TakeSource {
            let change = conflict
                .take_source
                .as_ref()
                .ok_or_else(|| WorkspaceError::new(WorkspaceErrorCode::ImmutableColumn))?;
            authorized.push((
                change,
                ReconcileOperationBasis::ConflictResolution,
                Some(conflict.review.id.clone()),
            ));
        }
        resolved_conflicts.push(ResolvedThreeWayConflictReview {
            conflict: conflict.review.clone(),
            resolution: choice,
        });
    }
    if !choices.is_empty() {
        return Err(invalid_contract());
    }
    let mut operation_pairs = Vec::with_capacity(authorized.len());
    for (change, basis, conflict_id) in authorized {
        let table_snapshots = review
            .snapshots
            .get(&(change.selection.dataset_index, change.selection.table_index))
            .ok_or_else(invalid_contract)?;
        let pair = plan_three_way_change(
            &review.source_handle,
            &review.target_handle,
            change,
            basis,
            conflict_id,
            table_snapshots,
            &review.limits,
        )?;
        let key = canonical_planned_operation_key(&review.source_handle, &pair.0, &review.limits)?;
        let key = operation_order_key(&review.table_order, &pair.0, key)?;
        operation_pairs.push((key, pair));
    }
    operation_pairs.sort_by(|left, right| left.0.cmp(&right.0));
    let mut planned = Vec::with_capacity(operation_pairs.len());
    let mut operation_reviews = Vec::with_capacity(operation_pairs.len());
    for (index, (_, (operation, mut operation_review))) in operation_pairs.into_iter().enumerate() {
        operation_review.sequence = u64::try_from(index + 1).map_err(|_| limit_exceeded())?;
        planned.push(operation);
        operation_reviews.push(operation_review);
    }
    let (output_revision_id, lineage_event_id) =
        generate_reconcile_ids(&review.source_handle, &review.target_handle, output_request)?;
    let destination = reserve_destination(
        &review.source_handle,
        &review.target_handle,
        Some(&review.ancestor_handle),
        output_request,
        &output_revision_id,
        &lineage_event_id,
    )?;
    let output = output_review(
        &review.source_ref,
        &review.target_ref,
        &output_revision_id,
        &lineage_event_id,
    )?;
    let (dataset_states, sequence_state) = dry_run_dataset_states(
        &review.source_handle,
        &review.target_handle,
        Some(&review.ancestor_handle),
        &destination,
        &planned,
        &review.limits,
        work_expires_at,
        &review.cancellation,
    )?;
    let (payload, payload_digest) = build_reconcile_payload(
        &review.expected_summary.report_digest,
        &review.source_ref,
        &review.target_ref,
        Some(&review.ancestor_ref),
        &operation_reviews,
        &planned,
        &resolved_conflicts,
        &dataset_states,
        &output,
        output_request,
        &review.confirmed_sensitive_dataset_indices,
        &review.source_handle,
    )?;
    let plan = build_reconcile_lifecycle_plan(
        &review.source_handle,
        &review.target_handle,
        Some(&review.ancestor_handle),
        &destination,
        &output,
        output_request,
        &payload_digest,
        &review.limits,
    )?;
    let review_digest = review_digest(
        &review.expected_summary.report_digest,
        &review.source_ref,
        &review.target_ref,
        Some(&review.ancestor_ref),
        &planned,
        &operation_reviews,
        &resolved_conflicts,
        &output,
        &destination,
        &sequence_state,
        &review.limits,
        &review.confirmed_sensitive_dataset_indices,
    )?;
    assert_reconcile_authorities_current(
        &review.source_handle,
        &review.target_handle,
        Some(&review.ancestor_handle),
        work_expires_at,
        &review.cancellation,
    )?;
    destination
        .assert_reserved_current()
        .map_err(crate::prepared_plan::map_prepared_destination_error)?;
    Ok(ReconcileReview {
        profile: RECONCILE_REVIEW_PROFILE,
        plan,
        payload,
        payload_digest,
        expected_summary: review.expected_summary,
        compare_report_digest: recomputed_summary.report_digest,
        source_ref: review.source_ref,
        target_ref: review.target_ref,
        ancestor_ref: Some(review.ancestor_ref),
        output,
        destination,
        operations: planned,
        operation_reviews,
        dataset_states,
        sequence_state,
        resolved_conflicts,
        limits: review.limits,
        review_digest,
        source_handle: review.source_handle,
        target_handle: review.target_handle,
        ancestor_handle: Some(review.ancestor_handle),
        expires_at: work_expires_at,
        cancellation: review.cancellation,
        confirmed_sensitive_dataset_indices: review.confirmed_sensitive_dataset_indices,
    })
}

fn canonical_planned_operation_key(
    source: &VerifiedWorkspaceSource,
    operation: &PlannedOperation,
    limits: &ReconcileReviewLimitsApplied,
) -> Result<Vec<u8>, WorkspaceError> {
    let table = contract_table(
        source
            .data_contract()
            .datasets
            .get(operation.dataset_index)
            .ok_or_else(invalid_contract)?,
        operation.table_index,
    )?;
    crate::compare::canonical_compare_key(&table.name, &operation.key, limits.max_value_bytes)
}

fn plan_three_way_change(
    source: &VerifiedWorkspaceSource,
    target: &VerifiedWorkspaceSource,
    change: &ThreeWayClassifiedChange,
    basis: ReconcileOperationBasis,
    conflict_id: Option<String>,
    snapshots: &ThreeWayTableSnapshots,
    limits: &ReconcileReviewLimitsApplied,
) -> Result<(PlannedOperation, ReconcileOperationReview), WorkspaceError> {
    let selection = &change.selection;
    validate_selection_shapes(std::slice::from_ref(selection))?;
    let dataset = contract_dataset(source, target, selection.dataset_index)?;
    let table = contract_table(dataset, selection.table_index)?;
    let source_row = snapshots.source.get(&selection.key_digest);
    let target_row = snapshots.target.get(&selection.key_digest);
    validate_row_binding(selection, source_row, target_row)?;
    let two_way_snapshots = TableSnapshots {
        columns: snapshots.columns.clone(),
        writable_columns: snapshots.writable_columns.clone(),
        source: snapshots.source.clone(),
        target: snapshots.target.clone(),
    };
    let (selected_columns, fields) = selected_fields(
        selection,
        dataset,
        table,
        &two_way_snapshots,
        source_row,
        target_row,
        limits.max_value_bytes,
    )?;
    let key = source_row
        .or(target_row)
        .ok_or_else(row_precondition_failed)?
        .key
        .clone();
    let source_values = match selection.action {
        ReconcileAction::InsertFromSource => Some(
            source_row
                .ok_or_else(row_precondition_failed)?
                .writable_values
                .clone(),
        ),
        ReconcileAction::ReplaceRowFromSource | ReconcileAction::SetFields => {
            Some(selected_write_values(
                &source_row
                    .ok_or_else(row_precondition_failed)?
                    .writable_values,
                &selected_columns,
            )?)
        }
        ReconcileAction::DeleteFromTarget => None,
    };
    let target_values = match selection.action {
        ReconcileAction::DeleteFromTarget => Some(
            target_row
                .ok_or_else(row_precondition_failed)?
                .writable_values
                .clone(),
        ),
        ReconcileAction::ReplaceRowFromSource | ReconcileAction::SetFields => {
            Some(selected_write_values(
                &target_row
                    .ok_or_else(row_precondition_failed)?
                    .writable_values,
                &selected_columns,
            )?)
        }
        ReconcileAction::InsertFromSource => None,
    };
    let write_set_digest = write_set_digest(
        table,
        &key,
        selection.action,
        match selection.action {
            ReconcileAction::DeleteFromTarget => target_values.as_deref().unwrap_or(&[]),
            _ => source_values.as_deref().unwrap_or(&[]),
        },
        limits.max_value_bytes,
    )?;
    Ok((
        PlannedOperation {
            action: selection.action,
            basis,
            dataset_index: selection.dataset_index,
            table_index: selection.table_index,
            key,
            source_values,
            target_values,
            write_set_digest,
            target_row_digest: selection.target_row_digest.clone(),
            ancestor_row_digest: change.ancestor_row_digest.clone(),
            conflict_id: conflict_id.clone(),
        },
        ReconcileOperationReview {
            sequence: 0,
            dataset_id: dataset.id.clone(),
            table: table.name.clone(),
            key_digest: selection.key_digest.clone(),
            action: selection.action,
            basis,
            source_row_digest: selection.source_row_digest.clone(),
            precondition_target_row_digest: selection.target_row_digest.clone(),
            ancestor_row_digest: change.ancestor_row_digest.clone(),
            conflict_id,
            fields,
            sensitive_confirmed: dataset.sensitivity == Sensitivity::Sensitive,
        },
    ))
}

#[allow(clippy::too_many_arguments)]
fn prepare_inner(
    source: VerifiedWorkspaceSource,
    target: VerifiedWorkspaceSource,
    expected_summary: &CompareSummary,
    selections: &[ReconcileSelection],
    confirmed_sensitive_dataset_indices: &BTreeSet<usize>,
    output_request: &ReconcileOutputRequest,
    limits: &ReconcileReviewLimitsApplied,
    control: &WorkspaceControl,
    expires_at: Instant,
    cancellation: &CancellationToken,
) -> Result<ReconcileReview, WorkspaceError> {
    validate_summary_binding(&source, &target, expected_summary)?;
    if selections.is_empty() || selections.len() > limits.max_operations {
        return Err(limit_exceeded());
    }
    validate_selection_shapes(selections)?;
    let required_sensitive = selections
        .iter()
        .filter_map(|selection| {
            source
                .data_contract()
                .datasets
                .get(selection.dataset_index)
                .filter(|dataset| dataset.sensitivity == Sensitivity::Sensitive)
                .map(|_| selection.dataset_index)
        })
        .collect::<BTreeSet<_>>();
    validate_sensitive_confirmations(&required_sensitive, confirmed_sensitive_dataset_indices)?;
    let table_order = reconcile_table_order(&source, &target, control)?;
    let mut table_keys: BTreeMap<(usize, usize), BTreeSet<String>> = BTreeMap::new();
    for selection in selections {
        control.check()?;
        table_keys
            .entry((selection.dataset_index, selection.table_index))
            .or_default()
            .insert(selection.key_digest.clone());
    }

    let mut total_rows = 0_u64;
    let mut stream_bytes = 0_u64;
    let mut retained_bytes = 0_u64;
    let mut snapshots = BTreeMap::new();
    for ((dataset_index, table_index), wanted_keys) in &table_keys {
        control.check()?;
        let dataset = contract_dataset(&source, &target, *dataset_index)?;
        let table = contract_table(dataset, *table_index)?;
        validate_dataset_policy(dataset, confirmed_sensitive_dataset_indices, *dataset_index)?;
        let columns =
            crate::compare::compared_columns(source.verified.connection(), table, control)?;
        if columns
            != crate::compare::compared_columns(target.verified.connection(), table, control)?
        {
            return Err(WorkspaceError::new(WorkspaceErrorCode::IncompatibleSchema));
        }
        let writable = writable_columns(source.verified.connection(), table, control)?;
        if writable != writable_columns(target.verified.connection(), table, control)? {
            return Err(WorkspaceError::new(WorkspaceErrorCode::IncompatibleSchema));
        }
        let source_rows = load_rows(
            source.verified.connection(),
            table,
            &columns,
            &writable,
            wanted_keys,
            limits,
            &mut total_rows,
            &mut stream_bytes,
            &mut retained_bytes,
            control,
        )?;
        let target_rows = load_rows(
            target.verified.connection(),
            table,
            &columns,
            &writable,
            wanted_keys,
            limits,
            &mut total_rows,
            &mut stream_bytes,
            &mut retained_bytes,
            control,
        )?;
        snapshots.insert(
            (*dataset_index, *table_index),
            TableSnapshots {
                columns,
                writable_columns: writable,
                source: source_rows,
                target: target_rows,
            },
        );
    }

    let mut planned = Vec::with_capacity(selections.len());
    let mut reviews = Vec::with_capacity(selections.len());
    let mut unique_rows = BTreeSet::new();
    for (index, selection) in selections.iter().enumerate() {
        control.check()?;
        if !unique_rows.insert((
            selection.dataset_index,
            selection.table_index,
            selection.key_digest.clone(),
        )) {
            return Err(invalid_contract());
        }
        let dataset = contract_dataset(&source, &target, selection.dataset_index)?;
        let table = contract_table(dataset, selection.table_index)?;
        let table_rows = snapshots
            .get(&(selection.dataset_index, selection.table_index))
            .ok_or_else(invalid_contract)?;
        let source_row = table_rows.source.get(&selection.key_digest);
        let target_row = table_rows.target.get(&selection.key_digest);
        validate_row_binding(selection, source_row, target_row)?;
        let (selected_columns, fields) = selected_fields(
            selection,
            dataset,
            table,
            table_rows,
            source_row,
            target_row,
            limits.max_value_bytes,
        )?;
        let key = source_row
            .or(target_row)
            .ok_or_else(row_precondition_failed)?
            .key
            .clone();
        let source_values = match selection.action {
            ReconcileAction::InsertFromSource => Some(
                source_row
                    .ok_or_else(row_precondition_failed)?
                    .writable_values
                    .clone(),
            ),
            ReconcileAction::ReplaceRowFromSource | ReconcileAction::SetFields => {
                Some(selected_write_values(
                    &source_row
                        .ok_or_else(row_precondition_failed)?
                        .writable_values,
                    &selected_columns,
                )?)
            }
            ReconcileAction::DeleteFromTarget => None,
        };
        let target_values = match selection.action {
            ReconcileAction::DeleteFromTarget => Some(
                target_row
                    .ok_or_else(row_precondition_failed)?
                    .writable_values
                    .clone(),
            ),
            ReconcileAction::ReplaceRowFromSource | ReconcileAction::SetFields => {
                Some(selected_write_values(
                    &target_row
                        .ok_or_else(row_precondition_failed)?
                        .writable_values,
                    &selected_columns,
                )?)
            }
            ReconcileAction::InsertFromSource => None,
        };
        let write_set_digest = write_set_digest(
            table,
            &key,
            selection.action,
            match selection.action {
                ReconcileAction::DeleteFromTarget => target_values.as_deref().unwrap_or(&[]),
                ReconcileAction::InsertFromSource
                | ReconcileAction::ReplaceRowFromSource
                | ReconcileAction::SetFields => source_values.as_deref().unwrap_or(&[]),
            },
            limits.max_value_bytes,
        )?;
        planned.push(PlannedOperation {
            action: selection.action,
            basis: ReconcileOperationBasis::UserSelected,
            dataset_index: selection.dataset_index,
            table_index: selection.table_index,
            key,
            source_values,
            target_values,
            write_set_digest,
            target_row_digest: selection.target_row_digest.clone(),
            ancestor_row_digest: None,
            conflict_id: None,
        });
        reviews.push(ReconcileOperationReview {
            sequence: u64::try_from(index + 1).map_err(|_| limit_exceeded())?,
            dataset_id: dataset.id.clone(),
            table: table.name.clone(),
            key_digest: selection.key_digest.clone(),
            action: selection.action,
            basis: ReconcileOperationBasis::UserSelected,
            source_row_digest: selection.source_row_digest.clone(),
            precondition_target_row_digest: selection.target_row_digest.clone(),
            ancestor_row_digest: None,
            conflict_id: None,
            fields,
            sensitive_confirmed: dataset.sensitivity == Sensitivity::Sensitive,
        });
    }
    let mut ordered = planned
        .into_iter()
        .zip(reviews)
        .map(|(planned, review)| {
            let table = contract_table(
                contract_dataset(&source, &target, planned.dataset_index)?,
                planned.table_index,
            )?;
            let key = crate::compare::canonical_compare_key(
                &table.name,
                &planned.key,
                limits.max_value_bytes,
            )?;
            let order = operation_order_key(&table_order, &planned, key)?;
            Ok((order, planned, review))
        })
        .collect::<Result<Vec<_>, WorkspaceError>>()?;
    ordered.sort_by(|left, right| left.0.cmp(&right.0));
    let mut planned = Vec::with_capacity(ordered.len());
    let mut reviews = Vec::with_capacity(ordered.len());
    for (index, (_, operation, mut review)) in ordered.into_iter().enumerate() {
        review.sequence = u64::try_from(index + 1).map_err(|_| limit_exceeded())?;
        planned.push(operation);
        reviews.push(review);
    }
    let (output_revision_id, lineage_event_id) =
        generate_reconcile_ids(&source, &target, output_request)?;
    let destination = reserve_destination(
        &source,
        &target,
        None,
        output_request,
        &output_revision_id,
        &lineage_event_id,
    )?;
    let source_ref = reconcile_reference(&source, control)?;
    let target_ref = reconcile_reference(&target, control)?;
    let output = output_review(
        &source_ref,
        &target_ref,
        &output_revision_id,
        &lineage_event_id,
    )?;
    let (dataset_states, sequence_state) = dry_run_dataset_states(
        &source,
        &target,
        None,
        &destination,
        &planned,
        limits,
        expires_at,
        cancellation,
    )?;
    let (payload, payload_digest) = build_reconcile_payload(
        &expected_summary.report_digest,
        &source_ref,
        &target_ref,
        None,
        &reviews,
        &planned,
        &[],
        &dataset_states,
        &output,
        output_request,
        confirmed_sensitive_dataset_indices,
        &source,
    )?;
    let plan = build_reconcile_lifecycle_plan(
        &source,
        &target,
        None,
        &destination,
        &output,
        output_request,
        &payload_digest,
        limits,
    )?;
    let review_digest = review_digest(
        &expected_summary.report_digest,
        &source_ref,
        &target_ref,
        None,
        &planned,
        &reviews,
        &[],
        &output,
        &destination,
        &sequence_state,
        limits,
        confirmed_sensitive_dataset_indices,
    )?;
    clear_progress(source.verified.connection());
    clear_progress(target.verified.connection());
    let rebind = crate::WorkspaceLimits {
        deadline: control.remaining()?,
        ..crate::WorkspaceLimits::default()
    };
    source.assert_current_with_control(&rebind, cancellation)?;
    let rebind = crate::WorkspaceLimits {
        deadline: control.remaining()?,
        ..crate::WorkspaceLimits::default()
    };
    target.assert_current_with_control(&rebind, cancellation)?;
    destination
        .assert_reserved_current()
        .map_err(crate::prepared_plan::map_prepared_destination_error)?;
    control.check()?;
    Ok(ReconcileReview {
        profile: RECONCILE_REVIEW_PROFILE,
        plan,
        payload,
        payload_digest,
        expected_summary: expected_summary.clone(),
        compare_report_digest: expected_summary.report_digest.clone(),
        source_ref,
        target_ref,
        ancestor_ref: None,
        output,
        destination,
        operations: planned,
        operation_reviews: reviews,
        dataset_states,
        sequence_state,
        resolved_conflicts: Vec::new(),
        limits: limits.clone(),
        review_digest,
        source_handle: source,
        target_handle: target,
        ancestor_handle: None,
        expires_at,
        cancellation: cancellation.clone(),
        confirmed_sensitive_dataset_indices: confirmed_sensitive_dataset_indices.clone(),
    })
}

fn validate_summary_binding(
    source: &VerifiedWorkspaceSource,
    target: &VerifiedWorkspaceSource,
    summary: &CompareSummary,
) -> Result<(), WorkspaceError> {
    if !source.has_complete_valid_signature_inventory()
        || !target.has_complete_valid_signature_inventory()
    {
        return Err(WorkspaceError::new(WorkspaceErrorCode::InvalidSignature));
    }
    let mut value = serde_json::to_value(summary).map_err(|_| invalid_contract())?;
    value
        .as_object_mut()
        .ok_or_else(invalid_contract)?
        .remove("report_digest");
    let digest = lower_hex(&Sha256::digest(crate::plan::canonical_json(&value)?));
    let source_identity = source.identity();
    let target_identity = target.identity();
    let source_revision = source_identity
        .overview
        .instance
        .revision_id
        .as_deref()
        .ok_or_else(invalid_contract)?;
    let target_revision = target_identity
        .overview
        .instance
        .revision_id
        .as_deref()
        .ok_or_else(invalid_contract)?;
    if summary.profile != crate::COMPARE_SUMMARY_PROFILE
        || summary.report_digest != digest
        || summary.truncated
        || summary
            .datasets
            .iter()
            .any(|dataset| dataset.truncated || dataset.tables.iter().any(|table| table.truncated))
        || !summary.compatibility.can_reconcile
        || summary.left.file_sha256 != source.source_sha256()
        || summary.right.file_sha256 != target.source_sha256()
        || summary.left.capsule_id != source_identity.capsule_id
        || summary.right.capsule_id != target_identity.capsule_id
        || summary.left.revision_id != source_revision
        || summary.right.revision_id != target_revision
        || summary.left.application_digest != lower_hex(source.application_digest())
        || summary.right.application_digest != lower_hex(target.application_digest())
        || source.data_contract() != target.data_contract()
    {
        return Err(WorkspaceError::new(WorkspaceErrorCode::StalePlan));
    }
    Ok(())
}

fn validate_selection_shapes(selections: &[ReconcileSelection]) -> Result<(), WorkspaceError> {
    for selection in selections {
        validate_digest(&selection.key_digest)?;
        if let Some(digest) = &selection.source_row_digest {
            validate_digest(digest)?;
        }
        if let Some(digest) = &selection.target_row_digest {
            validate_digest(digest)?;
        }
        let valid = match selection.action {
            ReconcileAction::InsertFromSource => {
                selection.source_row_digest.is_some()
                    && selection.target_row_digest.is_none()
                    && selection.field_indices.is_empty()
            }
            ReconcileAction::DeleteFromTarget => {
                selection.source_row_digest.is_none()
                    && selection.target_row_digest.is_some()
                    && selection.field_indices.is_empty()
            }
            ReconcileAction::ReplaceRowFromSource => {
                selection.source_row_digest.is_some()
                    && selection.target_row_digest.is_some()
                    && selection.field_indices.is_empty()
            }
            ReconcileAction::SetFields => {
                selection.source_row_digest.is_some()
                    && selection.target_row_digest.is_some()
                    && !selection.field_indices.is_empty()
                    && selection.field_indices.len() <= HARD_COLUMNS
            }
        };
        if !valid {
            return Err(invalid_contract());
        }
    }
    Ok(())
}

fn contract_dataset<'a>(
    source: &'a VerifiedWorkspaceSource,
    target: &VerifiedWorkspaceSource,
    dataset_index: usize,
) -> Result<&'a Dataset, WorkspaceError> {
    let source_dataset = source
        .data_contract()
        .datasets
        .get(dataset_index)
        .ok_or_else(invalid_contract)?;
    if target.data_contract().datasets.get(dataset_index) != Some(source_dataset) {
        return Err(WorkspaceError::new(WorkspaceErrorCode::IncompatibleSchema));
    }
    Ok(source_dataset)
}

fn contract_table(dataset: &Dataset, table_index: usize) -> Result<&DatasetTable, WorkspaceError> {
    dataset.tables.get(table_index).ok_or_else(invalid_contract)
}

fn validate_dataset_policy(
    dataset: &Dataset,
    confirmed_sensitive_dataset_indices: &BTreeSet<usize>,
    dataset_index: usize,
) -> Result<(), WorkspaceError> {
    if dataset.reconcile != ReconcilePolicy::Manual
        || !matches!(dataset.compare, ComparePolicy::Row | ComparePolicy::Field)
    {
        return Err(WorkspaceError::new(
            WorkspaceErrorCode::UnsupportedOperation,
        ));
    }
    if dataset.sensitivity == Sensitivity::Sensitive
        && !confirmed_sensitive_dataset_indices.contains(&dataset_index)
    {
        return Err(WorkspaceError::new(
            WorkspaceErrorCode::SensitiveConfirmationRequired,
        ));
    }
    Ok(())
}

fn validate_global_reconcile_policy(
    source: &VerifiedWorkspaceSource,
    target: &VerifiedWorkspaceSource,
) -> Result<(), WorkspaceError> {
    if source.data_contract() != target.data_contract() {
        return Err(WorkspaceError::new(WorkspaceErrorCode::IncompatibleSchema));
    }
    if source
        .data_contract()
        .datasets
        .iter()
        .any(|dataset| dataset.reconcile == ReconcilePolicy::Forbid)
    {
        return Err(WorkspaceError::new(
            WorkspaceErrorCode::UnsupportedOperation,
        ));
    }
    Ok(())
}

fn validate_sensitive_confirmations(
    required: &BTreeSet<usize>,
    confirmed: &BTreeSet<usize>,
) -> Result<(), WorkspaceError> {
    if required == confirmed {
        Ok(())
    } else if required.is_subset(confirmed) {
        Err(invalid_contract())
    } else {
        Err(WorkspaceError::new(
            WorkspaceErrorCode::SensitiveConfirmationRequired,
        ))
    }
}

fn validate_row_binding(
    selection: &ReconcileSelection,
    source: Option<&RowSnapshot>,
    target: Option<&RowSnapshot>,
) -> Result<(), WorkspaceError> {
    let source_digest = source.map(|row| row.row_digest.as_str());
    let target_digest = target.map(|row| row.row_digest.as_str());
    if source_digest != selection.source_row_digest.as_deref()
        || target_digest != selection.target_row_digest.as_deref()
        || source
            .or(target)
            .is_some_and(|row| row.key_digest != selection.key_digest)
    {
        return Err(row_precondition_failed());
    }
    match selection.action {
        ReconcileAction::InsertFromSource if source.is_some() && target.is_none() => Ok(()),
        ReconcileAction::DeleteFromTarget if source.is_none() && target.is_some() => Ok(()),
        ReconcileAction::ReplaceRowFromSource | ReconcileAction::SetFields
            if source.is_some() && target.is_some() && source_digest != target_digest =>
        {
            Ok(())
        }
        _ => Err(row_precondition_failed()),
    }
}

#[allow(clippy::too_many_arguments)]
fn selected_fields(
    selection: &ReconcileSelection,
    dataset: &Dataset,
    table: &DatasetTable,
    snapshots: &TableSnapshots,
    source: Option<&RowSnapshot>,
    target: Option<&RowSnapshot>,
    max_value_bytes: u64,
) -> Result<(Vec<String>, Vec<ReconcileFieldReview>), WorkspaceError> {
    let source = source.map(|row| &row.compared_values);
    let target = target.map(|row| &row.compared_values);
    let indices: Vec<usize> = match selection.action {
        ReconcileAction::SetFields => {
            if dataset.compare != ComparePolicy::Field {
                return Err(WorkspaceError::new(
                    WorkspaceErrorCode::UnsupportedOperation,
                ));
            }
            let unique: BTreeSet<_> = selection.field_indices.iter().copied().collect();
            if unique.len() != selection.field_indices.len() {
                return Err(invalid_contract());
            }
            if selection
                .field_indices
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
            {
                return Err(invalid_contract());
            }
            selection.field_indices.clone()
        }
        ReconcileAction::ReplaceRowFromSource => snapshots
            .columns
            .iter()
            .enumerate()
            .filter_map(|(index, column)| {
                (!table.primary_key.contains(column)
                    && !table.immutable_columns.contains(column)
                    && snapshots.writable_columns.contains(column))
                .then_some(index)
            })
            .collect(),
        ReconcileAction::InsertFromSource | ReconcileAction::DeleteFromTarget => Vec::new(),
    };
    let mut columns = Vec::with_capacity(indices.len());
    let mut fields = Vec::with_capacity(indices.len());
    for index in indices {
        let column = snapshots.columns.get(index).ok_or_else(invalid_contract)?;
        if table.primary_key.contains(column)
            || table.immutable_columns.contains(column)
            || !snapshots.writable_columns.contains(column)
        {
            return Err(WorkspaceError::new(WorkspaceErrorCode::ImmutableColumn));
        }
        let source_value = source
            .and_then(|values| values.get(index))
            .ok_or_else(row_precondition_failed)?;
        let target_value = target
            .and_then(|values| values.get(index))
            .ok_or_else(row_precondition_failed)?;
        if source_value.0 != *column || target_value.0 != *column {
            return Err(invalid_contract());
        }
        if values_equal(&source_value.1, &target_value.1, max_value_bytes)? {
            if selection.action == ReconcileAction::ReplaceRowFromSource {
                continue;
            }
            return Err(invalid_contract());
        }
        fields.push(ReconcileFieldReview {
            column: column.clone(),
            source_value_digest: value_digest(&source_value.1, max_value_bytes)?,
            target_value_digest: value_digest(&target_value.1, max_value_bytes)?,
        });
        columns.push(column.clone());
    }
    if matches!(
        selection.action,
        ReconcileAction::SetFields | ReconcileAction::ReplaceRowFromSource
    ) && fields.is_empty()
    {
        return Err(invalid_contract());
    }
    if selection.action == ReconcileAction::ReplaceRowFromSource {
        let source = source.ok_or_else(row_precondition_failed)?;
        let target = target.ok_or_else(row_precondition_failed)?;
        for immutable in &table.immutable_columns {
            if let Some(index) = snapshots
                .columns
                .iter()
                .position(|column| column == immutable)
                && !values_equal(&source[index].1, &target[index].1, max_value_bytes)?
            {
                return Err(WorkspaceError::new(WorkspaceErrorCode::ImmutableColumn));
            }
        }
    }
    Ok((columns, fields))
}

#[allow(clippy::too_many_arguments)]
fn load_rows(
    connection: &Connection,
    table: &DatasetTable,
    compared_columns: &[String],
    writable_columns: &[String],
    wanted_keys: &BTreeSet<String>,
    limits: &ReconcileReviewLimitsApplied,
    total_rows: &mut u64,
    stream_bytes: &mut u64,
    retained_bytes: &mut u64,
    control: &WorkspaceControl,
) -> Result<BTreeMap<String, RowSnapshot>, WorkspaceError> {
    load_rows_matching(
        connection,
        table,
        compared_columns,
        writable_columns,
        Some(wanted_keys),
        limits,
        total_rows,
        stream_bytes,
        retained_bytes,
        control,
    )
}

#[allow(clippy::too_many_arguments)]
fn load_all_rows(
    connection: &Connection,
    table: &DatasetTable,
    compared_columns: &[String],
    writable_columns: &[String],
    limits: &ReconcileReviewLimitsApplied,
    total_rows: &mut u64,
    stream_bytes: &mut u64,
    retained_bytes: &mut u64,
    control: &WorkspaceControl,
) -> Result<BTreeMap<String, RowSnapshot>, WorkspaceError> {
    load_rows_matching(
        connection,
        table,
        compared_columns,
        writable_columns,
        None,
        limits,
        total_rows,
        stream_bytes,
        retained_bytes,
        control,
    )
}

#[allow(clippy::too_many_arguments)]
fn load_rows_matching(
    connection: &Connection,
    table: &DatasetTable,
    compared_columns: &[String],
    writable_columns: &[String],
    wanted_keys: Option<&BTreeSet<String>>,
    limits: &ReconcileReviewLimitsApplied,
    total_rows: &mut u64,
    stream_bytes: &mut u64,
    retained_bytes: &mut u64,
    control: &WorkspaceControl,
) -> Result<BTreeMap<String, RowSnapshot>, WorkspaceError> {
    let mut all_columns = compared_columns.to_vec();
    for column in writable_columns {
        if !all_columns.contains(column) {
            all_columns.push(column.clone());
        }
    }
    if all_columns.len() > HARD_COLUMNS {
        return Err(limit_exceeded());
    }
    let projection = all_columns
        .iter()
        .map(|column| crate::compare::quote_identifier(column))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT {projection} FROM {} LIMIT ?1",
        crate::compare::quote_identifier(&table.name)
    );
    let mut statement = connection.prepare(&sql).map_err(|_| invalid_contract())?;
    let max_plus_one = limits
        .max_rows_scanned
        .checked_add(1)
        .ok_or_else(limit_exceeded)?;
    let mut rows = statement
        .query([i64::try_from(max_plus_one).map_err(|_| limit_exceeded())?])
        .map_err(|_| invalid_contract())?;
    let compared_indexes = compared_columns
        .iter()
        .map(|column| {
            all_columns
                .iter()
                .position(|candidate| candidate == column)
                .ok_or_else(invalid_contract)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let key_indexes = table
        .primary_key
        .iter()
        .map(|key| {
            compared_columns
                .iter()
                .position(|column| column == key)
                .ok_or_else(invalid_contract)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut found = BTreeMap::new();
    while let Some(row) = rows.next().map_err(|_| invalid_contract())? {
        control.check()?;
        if *total_rows == limits.max_rows_scanned {
            return Err(limit_exceeded());
        }
        *total_rows += 1;
        let values = (0..all_columns.len())
            .map(|index| {
                owned_value(
                    row.get_ref(index).map_err(|_| invalid_contract())?,
                    limits.max_value_bytes,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let compared_values = compared_columns
            .iter()
            .enumerate()
            .map(|(index, column)| (column.clone(), values[compared_indexes[index]].clone()))
            .collect::<Vec<_>>();
        let key = key_indexes
            .iter()
            .map(|index| compared_values[*index].clone())
            .collect::<Vec<_>>();
        let key_frame =
            crate::compare::canonical_compare_key(&table.name, &key, limits.max_value_bytes)?;
        let key_digest = lower_hex(&Sha256::digest(&key_frame));
        let row_frame = crate::compare::canonical_compare_row(
            &table.name,
            &key,
            &compared_values,
            limits.max_value_bytes,
        )?;
        let mut scanned_bytes = u64::try_from(key_frame.len())
            .map_err(|_| limit_exceeded())?
            .checked_add(u64::try_from(row_frame.len()).map_err(|_| limit_exceeded())?)
            .ok_or_else(limit_exceeded)?;
        for (index, column) in all_columns.iter().enumerate() {
            if !compared_columns.contains(column) {
                let value =
                    crate::compare::canonical_value_bytes(&values[index], limits.max_value_bytes)?;
                let value_bytes = u64::try_from(value.len()).map_err(|_| limit_exceeded())?;
                scanned_bytes = scanned_bytes
                    .checked_add(u64::try_from(column.len()).map_err(|_| limit_exceeded())?)
                    .and_then(|total| total.checked_add(value_bytes))
                    .ok_or_else(limit_exceeded)?;
            }
        }
        charge_bytes(stream_bytes, scanned_bytes, limits.max_stream_bytes)?;
        if wanted_keys.is_some_and(|wanted| !wanted.contains(&key_digest)) {
            continue;
        }
        let writable_values = all_columns
            .iter()
            .enumerate()
            .filter(|(_, column)| writable_columns.contains(*column))
            .map(|(index, column)| (column.clone(), values[index].clone()))
            .collect::<Vec<_>>();
        let mut retained_charge = scanned_bytes;
        for (column, value) in &writable_values {
            let value = crate::compare::canonical_value_bytes(value, limits.max_value_bytes)?;
            let value_bytes = u64::try_from(value.len()).map_err(|_| limit_exceeded())?;
            retained_charge = retained_charge
                .checked_add(u64::try_from(column.len()).map_err(|_| limit_exceeded())?)
                .and_then(|total| total.checked_add(value_bytes))
                .ok_or_else(limit_exceeded)?;
        }
        charge_bytes(retained_bytes, retained_charge, limits.max_retained_bytes)?;
        if found
            .insert(
                key_digest.clone(),
                RowSnapshot {
                    key,
                    key_digest,
                    row_digest: lower_hex(&Sha256::digest(row_frame)),
                    compared_values,
                    writable_values,
                },
            )
            .is_some()
        {
            return Err(invalid_contract());
        }
    }
    Ok(found)
}

fn writable_columns(
    connection: &Connection,
    table: &DatasetTable,
    control: &WorkspaceControl,
) -> Result<Vec<String>, WorkspaceError> {
    let mut statement = connection
        .prepare("SELECT name FROM pragma_table_xinfo(?1) WHERE hidden=0 ORDER BY cid LIMIT 257")
        .map_err(|_| invalid_contract())?;
    let mut rows = statement
        .query([&table.name])
        .map_err(|_| invalid_contract())?;
    let mut columns = Vec::new();
    while let Some(row) = rows.next().map_err(|_| invalid_contract())? {
        control.check()?;
        if columns.len() == HARD_COLUMNS {
            return Err(limit_exceeded());
        }
        let column: String = row.get(0).map_err(|_| invalid_contract())?;
        if columns.contains(&column) {
            return Err(invalid_contract());
        }
        columns.push(column);
    }
    if columns.is_empty() {
        return Err(invalid_contract());
    }
    Ok(columns)
}

#[derive(PartialEq, Eq)]
struct ReconcileForeignKeyGraph {
    parents: BTreeMap<(usize, usize), BTreeSet<(usize, usize)>>,
}

fn reconcile_table_order(
    source: &VerifiedWorkspaceSource,
    target: &VerifiedWorkspaceSource,
    control: &WorkspaceControl,
) -> Result<ReconcileTableOrder, WorkspaceError> {
    let source_graph = reconcile_foreign_key_graph(source, control)?;
    let target_graph = reconcile_foreign_key_graph(target, control)?;
    if source_graph != target_graph {
        return Err(WorkspaceError::new(WorkspaceErrorCode::IncompatibleSchema));
    }
    let mut rank_by_table = BTreeMap::new();
    let mut visiting = BTreeSet::new();
    for table in source_graph.parents.keys() {
        reconcile_table_rank(
            *table,
            &source_graph.parents,
            &mut visiting,
            &mut rank_by_table,
            control,
        )?;
    }
    Ok(ReconcileTableOrder {
        table_count: source_graph.parents.len(),
        rank_by_table,
    })
}

fn reconcile_foreign_key_graph(
    source: &VerifiedWorkspaceSource,
    control: &WorkspaceControl,
) -> Result<ReconcileForeignKeyGraph, WorkspaceError> {
    let contract = source.data_contract();
    let mut by_name = BTreeMap::new();
    let mut parents = BTreeMap::new();
    for (dataset_index, dataset) in contract.datasets.iter().enumerate() {
        for (table_index, table) in dataset.tables.iter().enumerate() {
            if by_name
                .insert(table.name.clone(), (dataset_index, table_index))
                .is_some()
            {
                return Err(invalid_contract());
            }
            parents.insert((dataset_index, table_index), BTreeSet::new());
        }
    }
    for (child_name, child) in &by_name {
        control.check()?;
        let mut statement = source
            .verified
            .connection()
            .prepare(
                "SELECT \"table\", on_update, on_delete \
                 FROM pragma_foreign_key_list(?1) ORDER BY id, seq LIMIT ?2",
            )
            .map_err(|_| invalid_contract())?;
        let mut rows = statement
            .query(params![
                child_name,
                i64::try_from(HARD_FOREIGN_KEYS + 1).map_err(|_| limit_exceeded())?
            ])
            .map_err(|_| invalid_contract())?;
        let mut count = 0_usize;
        while let Some(row) = rows.next().map_err(|_| invalid_contract())? {
            control.check()?;
            if count == HARD_FOREIGN_KEYS {
                return Err(limit_exceeded());
            }
            count += 1;
            let parent_name: String = row.get(0).map_err(|_| invalid_contract())?;
            let on_update: String = row.get(1).map_err(|_| invalid_contract())?;
            let on_delete: String = row.get(2).map_err(|_| invalid_contract())?;
            if !matches!(on_update.as_str(), "NO ACTION" | "RESTRICT")
                || !matches!(on_delete.as_str(), "NO ACTION" | "RESTRICT")
            {
                return Err(WorkspaceError::new(
                    WorkspaceErrorCode::UnsupportedOperation,
                ));
            }
            let parent = *by_name.get(&parent_name).ok_or_else(invalid_contract)?;
            if child.0 != parent.0
                && !contract.datasets[child.0]
                    .dependencies
                    .iter()
                    .any(|dependency| dependency.dataset_id == contract.datasets[parent.0].id)
            {
                return Err(invalid_contract());
            }
            parents
                .get_mut(child)
                .ok_or_else(invalid_contract)?
                .insert(parent);
        }
    }
    Ok(ReconcileForeignKeyGraph { parents })
}

fn reconcile_table_rank(
    table: (usize, usize),
    parents: &BTreeMap<(usize, usize), BTreeSet<(usize, usize)>>,
    visiting: &mut BTreeSet<(usize, usize)>,
    ranks: &mut BTreeMap<(usize, usize), usize>,
    control: &WorkspaceControl,
) -> Result<usize, WorkspaceError> {
    control.check()?;
    if let Some(rank) = ranks.get(&table) {
        return Ok(*rank);
    }
    if !visiting.insert(table) {
        return Err(WorkspaceError::new(
            WorkspaceErrorCode::UnsupportedOperation,
        ));
    }
    let mut rank = 0_usize;
    for parent in parents.get(&table).ok_or_else(invalid_contract)? {
        rank = rank.max(
            reconcile_table_rank(*parent, parents, visiting, ranks, control)?
                .checked_add(1)
                .ok_or_else(limit_exceeded)?,
        );
    }
    visiting.remove(&table);
    ranks.insert(table, rank);
    Ok(rank)
}

fn operation_order_key(
    order: &ReconcileTableOrder,
    operation: &PlannedOperation,
    canonical_key: Vec<u8>,
) -> Result<ReconcileOperationOrderKey, WorkspaceError> {
    let rank = *order
        .rank_by_table
        .get(&(operation.dataset_index, operation.table_index))
        .ok_or_else(invalid_contract)?;
    let delete = operation.action == ReconcileAction::DeleteFromTarget;
    let dependency_rank = if delete {
        order
            .table_count
            .checked_sub(rank.checked_add(1).ok_or_else(limit_exceeded)?)
            .ok_or_else(invalid_contract)?
    } else {
        rank
    };
    Ok(ReconcileOperationOrderKey {
        delete_phase: u8::from(delete),
        dependency_rank,
        action_phase: reconcile_action_phase(operation.action),
        dataset_index: operation.dataset_index,
        table_index: operation.table_index,
        canonical_key,
    })
}

fn reconcile_reference(
    source: &VerifiedWorkspaceSource,
    control: &WorkspaceControl,
) -> Result<ReconcileReference, WorkspaceError> {
    let identity = source.identity();
    let schema = identity
        .overview
        .data_schema
        .as_ref()
        .ok_or_else(invalid_contract)?;
    let (signature_count, signature_inventory_digest) =
        signature_inventory_digest(source, control)?;
    Ok(ReconcileReference {
        file_sha256: source.source_sha256(),
        capsule_id: identity.capsule_id.clone(),
        revision_id: identity
            .overview
            .instance
            .revision_id
            .clone()
            .ok_or_else(invalid_contract)?,
        application_digest: lower_hex(source.application_digest()),
        signature_count,
        signature_inventory_digest,
        data_schema_id: schema.data_schema_id.clone(),
        data_schema_version: schema.data_schema_version,
    })
}

fn signature_inventory_digest(
    source: &VerifiedWorkspaceSource,
    control: &WorkspaceControl,
) -> Result<(u32, String), WorkspaceError> {
    if !source.has_complete_valid_signature_inventory()
        || source.signature_reports().len() > sqlite_capsule_crypto::MAX_SIGNATURES
    {
        return Err(WorkspaceError::new(WorkspaceErrorCode::InvalidSignature));
    }
    const COLUMNS: [&str; 6] = [
        "key_id",
        "algorithm",
        "public_key",
        "application_digest",
        "signature",
        "signed_at",
    ];
    let mut statement = source
        .verified
        .connection()
        .prepare(
            "SELECT key_id, algorithm, public_key, application_digest, signature, signed_at \
             FROM capsule_signature ORDER BY key_id LIMIT 33",
        )
        .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InvalidSignature))?;
    let mut rows = statement
        .query([])
        .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InvalidSignature))?;
    let mut hashes = Vec::new();
    while let Some(row) = rows
        .next()
        .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InvalidSignature))?
    {
        control.check()?;
        if hashes.len() == sqlite_capsule_crypto::MAX_SIGNATURES {
            return Err(WorkspaceError::new(WorkspaceErrorCode::InvalidSignature));
        }
        let values = COLUMNS
            .iter()
            .enumerate()
            .map(|(index, column)| {
                Ok((
                    (*column).to_owned(),
                    owned_value(
                        row.get_ref(index).map_err(|_| {
                            WorkspaceError::new(WorkspaceErrorCode::InvalidSignature)
                        })?,
                        HARD_VALUE_BYTES,
                    )?,
                ))
            })
            .collect::<Result<Vec<_>, WorkspaceError>>()?;
        let key = values[..1].to_vec();
        let frame = crate::compare::canonical_compare_row(
            "capsule_signature",
            &key,
            &values,
            HARD_VALUE_BYTES,
        )?;
        hashes.push(Sha256::digest(frame));
    }
    if hashes.len() != source.signature_reports().len() {
        return Err(WorkspaceError::new(WorkspaceErrorCode::InvalidSignature));
    }
    hashes.sort();
    let mut frame = Vec::new();
    frame_text(&mut frame, SIGNATURE_INVENTORY_PROFILE)?;
    frame_u64(&mut frame, hashes.len())?;
    for hash in hashes {
        frame.extend_from_slice(&hash);
    }
    Ok((
        u32::try_from(source.signature_reports().len()).map_err(|_| limit_exceeded())?,
        lower_hex(&Sha256::digest(frame)),
    ))
}

fn reserve_destination(
    source: &VerifiedWorkspaceSource,
    target: &VerifiedWorkspaceSource,
    ancestor: Option<&VerifiedWorkspaceSource>,
    request: &ReconcileOutputRequest,
    output_revision_id: &str,
    lineage_event_id: &str,
) -> Result<DestinationReservation, WorkspaceError> {
    validate_uuid(output_revision_id)?;
    validate_uuid(lineage_event_id)?;
    let target_revision = target
        .identity()
        .overview
        .instance
        .revision_id
        .as_deref()
        .ok_or_else(invalid_contract)?;
    let source_revision = source
        .identity()
        .overview
        .instance
        .revision_id
        .as_deref()
        .ok_or_else(invalid_contract)?;
    if output_revision_id == target_revision
        || output_revision_id == source_revision
        || lineage_event_id == output_revision_id
    {
        return Err(invalid_contract());
    }
    let output = absolute_output(&request.output_path)?;
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(invalid_contract)?;
    let leaf = output
        .file_name()
        .and_then(OsStr::to_str)
        .filter(|leaf| !leaf.is_empty() && leaf.len() <= 255)
        .ok_or_else(invalid_contract)?;
    let mut inputs = vec![
        source.source_identity().clone(),
        target.source_identity().clone(),
    ];
    if let Some(ancestor) = ancestor {
        inputs.push(ancestor.source_identity().clone());
    }
    DestinationReservation::reserve(parent, OsStr::new(leaf), &inputs)
        .map_err(crate::prepared_plan::map_destination_error)
}

fn output_review(
    source: &ReconcileReference,
    target: &ReconcileReference,
    output_revision_id: &str,
    lineage_event_id: &str,
) -> Result<ReconcileOutputReview, WorkspaceError> {
    Ok(ReconcileOutputReview {
        capsule_id: target.capsule_id.clone(),
        revision_id: output_revision_id.to_owned(),
        application_digest: target.application_digest.clone(),
        signature_count: target.signature_count,
        signature_inventory_digest: target.signature_inventory_digest.clone(),
        preserves_target_capsule_id: true,
        preserves_target_application_digest: true,
        preserves_target_signature_inventory: true,
        must_not_exist: true,
        lineage_event_id: lineage_event_id.to_owned(),
        lineage_parents: vec![
            ReconcileLineageParentReview {
                ordinal: 1,
                relation: ReconcileLineageRelation::TargetDerivedFrom,
                file_sha256: target.file_sha256.clone(),
                capsule_id: target.capsule_id.clone(),
                revision_id: target.revision_id.clone(),
            },
            ReconcileLineageParentReview {
                ordinal: 2,
                relation: ReconcileLineageRelation::ChangesAppliedFrom,
                file_sha256: source.file_sha256.clone(),
                capsule_id: source.capsule_id.clone(),
                revision_id: source.revision_id.clone(),
            },
        ],
    })
}

fn generate_reconcile_ids(
    source: &VerifiedWorkspaceSource,
    target: &VerifiedWorkspaceSource,
    request: &ReconcileOutputRequest,
) -> Result<(String, String), WorkspaceError> {
    let source_revision = source
        .identity()
        .overview
        .instance
        .revision_id
        .as_deref()
        .ok_or_else(invalid_contract)?;
    let target_revision = target
        .identity()
        .overview
        .instance
        .revision_id
        .as_deref()
        .ok_or_else(invalid_contract)?;
    validate_uuid(&request.plan_id)?;
    let forbidden = BTreeSet::from([
        source_revision,
        target_revision,
        source.identity().capsule_id.as_str(),
        target.identity().capsule_id.as_str(),
    ]);
    let revision = mint_distinct_uuid_v4(&forbidden)?;
    let mut event_forbidden = forbidden;
    event_forbidden.insert(&revision);
    let event = mint_distinct_uuid_v4(&event_forbidden)?;
    Ok((revision, event))
}

fn mint_distinct_uuid_v4(forbidden: &BTreeSet<&str>) -> Result<String, WorkspaceError> {
    for _ in 0..8 {
        let candidate = mint_uuid_v4()?;
        if !forbidden.contains(candidate.as_str()) {
            return Ok(candidate);
        }
    }
    Err(WorkspaceError::new(WorkspaceErrorCode::InternalError))
}

fn mint_uuid_v4() -> Result<String, WorkspaceError> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes)
        .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InternalError))?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    ))
}

#[allow(clippy::too_many_arguments)]
fn dry_run_dataset_states(
    source: &VerifiedWorkspaceSource,
    target: &VerifiedWorkspaceSource,
    ancestor: Option<&VerifiedWorkspaceSource>,
    destination: &DestinationReservation,
    operations: &[PlannedOperation],
    limits: &ReconcileReviewLimitsApplied,
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<(Vec<ReconcileDatasetStateReview>, Vec<SequenceStateReview>), WorkspaceError> {
    check(deadline, cancellation)?;
    let parent = destination
        .path_hint()
        .parent()
        .ok_or_else(invalid_contract)?
        .to_path_buf();
    let scratch_leaf = format!(
        ".sqlite-capsule-reconcile-dry-run-{}.discard",
        lower_hex(&Sha256::digest(
            destination.path_hint().as_os_str().as_encoded_bytes()
        ))
    );
    let mut inputs = vec![
        source.source_identity().clone(),
        target.source_identity().clone(),
    ];
    if let Some(ancestor) = ancestor {
        inputs.push(ancestor.source_identity().clone());
    }
    let scratch = DestinationReservation::reserve(&parent, OsStr::new(&scratch_leaf), &inputs)
        .map_err(crate::prepared_plan::map_destination_error)?;
    let mut private = scratch
        .stage()
        .map_err(crate::prepared_plan::map_destination_error)?;
    let verification = sqlite_capsule_launch::VerificationControl::new(
        remaining(deadline)?,
        cancellation.shared_flag(),
    )
    .with_max_bytes(sqlite_capsule_core::MAX_CAPSULE_BYTES);
    let copied = target
        .verified
        .copy_snapshot_to_file_with_control(
            private.file_mut(),
            &verification,
            sqlite_capsule_core::MAX_CAPSULE_BYTES,
        )
        .map_err(map_launch_output_error)?;
    if copied != target.source_identity().bytes {
        return Err(verification_failed());
    }
    private.file_mut().sync_all().map_err(|_| output_failed())?;
    apply_operations_transaction(
        private.private_path_hint(),
        target,
        operations,
        limits.max_value_bytes,
        deadline,
        cancellation,
    )?;
    let output = VerifiedWorkspaceSource::open_with_control(
        private.private_path_hint(),
        &crate::WorkspaceLimits {
            deadline: remaining(deadline)?,
            max_capsule_bytes: sqlite_capsule_core::MAX_CAPSULE_BYTES,
            ..crate::WorkspaceLimits::default()
        },
        cancellation,
    )?;
    let mut datasets = source
        .data_contract()
        .datasets
        .iter()
        .map(|dataset| dataset.id.clone())
        .collect::<Vec<_>>();
    datasets.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    let mut source_rows = limits.max_rows_scanned;
    let mut target_rows = limits.max_rows_scanned;
    let mut output_rows = limits.max_rows_scanned;
    let mut source_bytes = limits.max_stream_bytes;
    let mut target_bytes = limits.max_stream_bytes;
    let mut output_bytes = limits.max_stream_bytes;
    let mut result = Vec::with_capacity(datasets.len());
    for dataset_id in datasets {
        check(deadline, cancellation)?;
        let source_dataset = source
            .data_contract()
            .datasets
            .iter()
            .find(|dataset| dataset.id == dataset_id)
            .ok_or_else(invalid_contract)?;
        let target_dataset = target
            .data_contract()
            .datasets
            .iter()
            .find(|dataset| dataset.id == dataset_id)
            .ok_or_else(invalid_contract)?;
        let output_dataset = output
            .data_contract()
            .datasets
            .iter()
            .find(|dataset| dataset.id == dataset_id)
            .ok_or_else(invalid_contract)?;
        let (source_row_count, source_state_sha256) =
            crate::template_state::dataset_state_with_budget(
                source,
                source_dataset,
                &mut source_rows,
                &mut source_bytes,
                deadline,
                cancellation,
            )?;
        let (target_row_count, target_state_sha256) =
            crate::template_state::dataset_state_with_budget(
                target,
                target_dataset,
                &mut target_rows,
                &mut target_bytes,
                deadline,
                cancellation,
            )?;
        let (output_row_count, output_state_sha256) =
            crate::template_state::dataset_state_with_budget(
                &output,
                output_dataset,
                &mut output_rows,
                &mut output_bytes,
                deadline,
                cancellation,
            )?;
        result.push(ReconcileDatasetStateReview {
            dataset_id,
            source_row_count,
            source_state_sha256,
            target_row_count,
            target_state_sha256,
            output_row_count,
            output_state_sha256,
        });
    }
    let sequence_state = capture_sequence_state(output.verified.connection())?;
    Ok((result, sequence_state))
}

#[allow(clippy::too_many_arguments)]
fn build_reconcile_payload(
    compare_report_digest: &str,
    source: &ReconcileReference,
    target: &ReconcileReference,
    ancestor: Option<&ReconcileReference>,
    reviews: &[ReconcileOperationReview],
    planned: &[PlannedOperation],
    resolved_conflicts: &[ResolvedThreeWayConflictReview],
    dataset_states: &[ReconcileDatasetStateReview],
    output: &ReconcileOutputReview,
    request: &ReconcileOutputRequest,
    confirmed_sensitive_dataset_indices: &BTreeSet<usize>,
    source_handle: &VerifiedWorkspaceSource,
) -> Result<(Vec<u8>, String), WorkspaceError> {
    if reviews.len() != planned.len() {
        return Err(invalid_contract());
    }
    let operations = reviews
        .iter()
        .zip(planned)
        .map(|(review, planned)| {
            let source_state = row_state_json(review.source_row_digest.as_deref());
            let target_state = row_state_json(review.precondition_target_row_digest.as_deref());
            let action = match review.action {
                ReconcileAction::InsertFromSource => json!({
                    "kind": "insert-source-row",
                    "source_write_set_digest": planned.write_set_digest,
                }),
                ReconcileAction::DeleteFromTarget => json!({"kind": "delete-target-row"}),
                ReconcileAction::ReplaceRowFromSource => json!({
                    "kind": "replace-target-row-from-source",
                    "source_write_set_digest": planned.write_set_digest,
                }),
                ReconcileAction::SetFields => json!({
                    "kind": "set-target-fields-from-source",
                    "fields": review.fields,
                }),
            };
            json!({
                "sequence": review.sequence,
                "dataset_id": review.dataset_id,
                "table": review.table,
                "key_digest": review.key_digest,
                "basis": review.basis.label(),
                "source_state": source_state,
                "target_state": target_state,
                "action": action,
            })
        })
        .map(|mut operation| {
            let index = usize::try_from(operation["sequence"].as_u64().unwrap_or_default())
                .unwrap_or_default()
                .saturating_sub(1);
            let review = reviews.get(index).ok_or_else(invalid_contract)?;
            if let Some(ancestor_state) = review.ancestor_row_digest.as_deref() {
                operation["ancestor_state"] = row_state_json(Some(ancestor_state));
            } else if ancestor.is_some() {
                operation["ancestor_state"] = row_state_json(None);
            }
            if let Some(conflict_id) = &review.conflict_id {
                operation["conflict_id"] = JsonValue::String(conflict_id.clone());
            }
            Ok(operation)
        })
        .collect::<Result<Vec<_>, WorkspaceError>>()?;
    let states = dataset_states
        .iter()
        .map(|state| {
            json!({
                "dataset_id": state.dataset_id,
                "source": dataset_state_json(state.source_row_count, &state.source_state_sha256),
                "target": dataset_state_json(state.target_row_count, &state.target_state_sha256),
                "output": dataset_state_json(state.output_row_count, &state.output_state_sha256),
            })
        })
        .collect::<Vec<_>>();
    let required_sensitive = confirmed_sensitive_dataset_indices
        .iter()
        .map(|dataset_index| {
            let dataset = source_handle
                .data_contract()
                .datasets
                .get(*dataset_index)
                .ok_or_else(invalid_contract)?;
            if dataset.sensitivity != Sensitivity::Sensitive {
                return Err(invalid_contract());
            }
            Ok(dataset.id.clone())
        })
        .collect::<Result<BTreeSet<_>, WorkspaceError>>()?
        .into_iter()
        .collect::<Vec<_>>();
    let mut details = json!({
        "profile": RECONCILE_LINEAGE_DETAILS_PROFILE,
        "compare_report_digest": compare_report_digest,
        "payload_digest": "",
        "operation_count": reviews.len(),
        "resolved_conflict_count": resolved_conflicts.len(),
    });
    let mut signature_inventories = json!({
        "source": {"count": source.signature_count, "sha256": source.signature_inventory_digest},
        "target": {"count": target.signature_count, "sha256": target.signature_inventory_digest},
    });
    if let Some(ancestor) = ancestor {
        signature_inventories["ancestor"] = json!({
            "count": ancestor.signature_count,
            "sha256": ancestor.signature_inventory_digest,
        });
        details["ancestor_evidence"] = json!({
            "profile": RECONCILE_ANCESTOR_EVIDENCE_PROFILE,
            "file_sha256": ancestor.file_sha256,
            "capsule_id": ancestor.capsule_id,
            "revision_id": ancestor.revision_id,
            "evidence_digest": ancestor_evidence_digest(ancestor)?,
        });
    }
    let resolved_conflicts_json = resolved_conflicts
        .iter()
        .map(|resolved| {
            let conflict = &resolved.conflict;
            let mut value = json!({
                "id": conflict.id,
                "dataset_id": conflict.dataset_id,
                "table": conflict.table,
                "key_digest": conflict.key_digest,
                "kind": conflict.kind.label(),
                "source_state": row_state_json(conflict.source_row_digest.as_deref()),
                "target_state": row_state_json(conflict.target_row_digest.as_deref()),
                "ancestor_state": row_state_json(conflict.ancestor_row_digest.as_deref()),
                "allowed_choices": conflict.allowed_choices.iter().map(|choice| choice.label()).collect::<Vec<_>>(),
                "resolution": resolved.resolution.label(),
            });
            if let Some(deleted_side) = conflict.deleted_side {
                value["deleted_side"] = JsonValue::String(deleted_side.label().to_owned());
            }
            value
        })
        .collect::<Vec<_>>();
    let mut payload = json!({
        "profile": RECONCILE_PAYLOAD_PROFILE,
        "compare_report_digest": compare_report_digest,
        "source_side": "source",
        "target_side": "target",
        "mode": if ancestor.is_some() { "three-way" } else { "two-way-explicit" },
        "signature_inventories": signature_inventories,
        "lineage": {
            "event_id": output.lineage_event_id,
            "occurred_at": request.created_at,
            "operation": "reconcile-to-copy",
            "result": {"capsule_id": output.capsule_id, "revision_id": output.revision_id},
            "parents": output.lineage_parents.iter().map(|parent| json!({
                "ordinal": parent.ordinal,
                "relation": parent.relation.label(),
                "file_sha256": parent.file_sha256,
                "capsule_id": parent.capsule_id,
                "revision_id": parent.revision_id,
            })).collect::<Vec<_>>(),
            "details": details,
        },
        "operations": operations,
        "resolved_conflicts": resolved_conflicts_json,
        "expected_dataset_states": states,
        "sensitive_confirmation": {
            "required_dataset_ids": required_sensitive,
            "confirmed_dataset_ids": required_sensitive,
        },
        "payload_digest": "",
    });
    // Prove every required sensitive dataset was genuinely signed sensitive.
    if payload["sensitive_confirmation"]["required_dataset_ids"]
        .as_array()
        .ok_or_else(invalid_contract)?
        .iter()
        .any(|id| {
            let id = id.as_str().unwrap_or_default();
            !source_handle
                .data_contract()
                .datasets
                .iter()
                .any(|dataset| dataset.id == id && dataset.sensitivity == Sensitivity::Sensitive)
        })
    {
        return Err(invalid_contract());
    }
    let digest = reconcile_payload_digest(&payload)?;
    payload["payload_digest"] = JsonValue::String(digest.clone());
    payload["lineage"]["details"]["payload_digest"] = JsonValue::String(digest.clone());
    let bytes = crate::plan::canonical_json(&payload)?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err(limit_exceeded());
    }
    Ok((bytes, digest))
}

fn row_state_json(digest: Option<&str>) -> JsonValue {
    match digest {
        Some(digest) => json!({"state": "present", "row_digest": digest}),
        None => json!({"state": "absent"}),
    }
}

fn dataset_state_json(row_count: u64, digest: &str) -> JsonValue {
    json!({
        "profile": crate::template_state::DATASET_STATE_PROFILE,
        "row_count": row_count,
        "sha256": digest,
    })
}

fn ancestor_evidence_digest(ancestor: &ReconcileReference) -> Result<String, WorkspaceError> {
    let mut frame = Vec::new();
    frame_text(&mut frame, RECONCILE_ANCESTOR_EVIDENCE_PROFILE)?;
    frame_reference(&mut frame, ancestor)?;
    Ok(lower_hex(&Sha256::digest(frame)))
}

fn reconcile_payload_digest(payload: &JsonValue) -> Result<String, WorkspaceError> {
    let mut material = payload.clone();
    material
        .as_object_mut()
        .ok_or_else(invalid_contract)?
        .remove("payload_digest");
    material["lineage"]["details"]
        .as_object_mut()
        .ok_or_else(invalid_contract)?
        .remove("payload_digest");
    Ok(lower_hex(&Sha256::digest(crate::plan::canonical_json(
        &material,
    )?)))
}

#[allow(clippy::too_many_arguments)]
fn build_reconcile_lifecycle_plan(
    source: &VerifiedWorkspaceSource,
    target: &VerifiedWorkspaceSource,
    ancestor: Option<&VerifiedWorkspaceSource>,
    destination: &DestinationReservation,
    output: &ReconcileOutputReview,
    request: &ReconcileOutputRequest,
    payload_digest: &str,
    limits: &ReconcileReviewLimitsApplied,
) -> Result<LifecyclePlan, WorkspaceError> {
    validate_uuid(&request.plan_id)?;
    let target_identity = target.identity();
    let target_schema = target_identity
        .overview
        .data_schema
        .as_ref()
        .ok_or_else(invalid_contract)?;
    let mut max_input = source
        .source_identity()
        .bytes
        .checked_add(target.source_identity().bytes)
        .ok_or_else(limit_exceeded)?;
    if let Some(ancestor) = ancestor {
        max_input = max_input
            .checked_add(ancestor.source_identity().bytes)
            .ok_or_else(limit_exceeded)?;
    }
    let mut inputs = vec![
        plan_input_json("source", source)?,
        plan_input_json("target", target)?,
    ];
    if let Some(ancestor) = ancestor {
        inputs.push(plan_input_json("ancestor", ancestor)?);
    }
    let max_output = sqlite_capsule_core::MAX_CAPSULE_BYTES;
    let mut value = json!({
        "profile": crate::PLAN_PROFILE,
        "plan_id": request.plan_id,
        "operation": "reconcile-to-copy",
        "created_at": request.created_at,
        "expires_at": request.expires_at,
        "inputs": inputs,
        "output": {
            "path": utf8_path(&destination.path_hint())?,
            "leaf_name": destination.leaf().to_str().ok_or_else(invalid_contract)?,
            "parent_filesystem_identity": {
                "platform": std::env::consts::OS,
                "volume_or_device": destination.identity().device.to_string(),
                "file_id_or_inode": destination.identity().stable_file_id,
            },
            "must_not_exist": true,
            "publish_mode": "create-new-no-replace",
        },
        "decisions": [{
            "scope": "application",
            "subject": target_identity.app_id,
            "action": "bind-reconcile-payload",
            "reason": "Apply only the exact reviewed value-free reconciliation payload to a new target-derived copy.",
            "parameters": {"reconcile_payload_digest": payload_digest},
        }],
        "limits": {
            "max_input_bytes": max_input,
            "max_output_bytes": max_output,
            "max_rows_inspected": limits.max_rows_scanned,
            "max_rows_written": limits.max_operations,
            "deadline_ms": limits.deadline_ms,
        },
        "expected": {
            "capsule_id": output.capsule_id,
            "revision_id": output.revision_id,
            "app_id": target_identity.app_id,
            "application_digest": output.application_digest,
            "data_schema_id": target_schema.data_schema_id,
            "data_schema_version": target_schema.data_schema_version,
        },
        "plan_digest": "",
    });
    let digest = crate::plan::canonical_digest_value(&value)?;
    value["plan_digest"] = JsonValue::String(digest);
    let bytes = crate::plan::canonical_json(&value)?;
    let plan = LifecyclePlan::parse(&bytes)?;
    validate_reconcile_plan_shape(&plan, payload_digest)?;
    Ok(plan)
}

fn plan_input_json(
    role: &str,
    source: &VerifiedWorkspaceSource,
) -> Result<JsonValue, WorkspaceError> {
    let identity = source.identity();
    let instance = &identity.overview.instance;
    let schema = identity
        .overview
        .data_schema
        .as_ref()
        .ok_or_else(invalid_contract)?;
    let live = source.source_identity();
    let digest = source.source_sha256();
    Ok(json!({
        "role": role,
        "path_hint": utf8_path(&identity.canonical_path)?,
        "file_sha256": digest,
        "snapshot_sha256": digest,
        "size_bytes": live.bytes,
        "filesystem_identity": {
            "platform": std::env::consts::OS,
            "volume_or_device": live.device.to_string(),
            "file_id_or_inode": live.stable_file_id,
            "modified_ns": live.modified_ns,
        },
        "capsule": {
            "format_version": identity.format_version,
            "capsule_id": identity.capsule_id,
            "revision_id": instance.revision_id,
            "app_id": identity.app_id,
            "app_version": identity.app_version,
            "application_digest": lower_hex(source.application_digest()),
            "data_schema_id": schema.data_schema_id,
            "data_schema_version": schema.data_schema_version,
            "publisher_key_id": JsonValue::Null,
        },
    }))
}

fn validate_reconcile_plan_shape(
    plan: &LifecyclePlan,
    payload_digest: &str,
) -> Result<(), WorkspaceError> {
    if plan.operation() != Operation::ReconcileToCopy
        || !matches!(plan.inputs().len(), 2 | 3)
        || plan.inputs()[0].role() != InputRole::Source
        || plan.inputs()[1].role() != InputRole::Target
        || (plan.inputs().len() == 3 && plan.inputs()[2].role() != InputRole::Ancestor)
        || plan.decisions().len() != 1
        || plan.decisions()[0].action() != "bind-reconcile-payload"
    {
        return Err(invalid_contract());
    }
    let value = serde_json::to_value(plan).map_err(|_| invalid_contract())?;
    if value["decisions"][0]["parameters"]["reconcile_payload_digest"].as_str()
        != Some(payload_digest)
    {
        return Err(invalid_contract());
    }
    Ok(())
}

fn bind_reconcile_plan(
    plan: &LifecyclePlan,
    source: &VerifiedWorkspaceSource,
    target: &VerifiedWorkspaceSource,
    ancestor: Option<&VerifiedWorkspaceSource>,
    destination: &DestinationReservation,
    output: &ReconcileOutputReview,
) -> Result<(), WorkspaceError> {
    validate_plan_input(&plan.inputs()[0], source)?;
    validate_plan_input(&plan.inputs()[1], target)?;
    match (ancestor, plan.inputs().get(2)) {
        (Some(ancestor), Some(input)) => validate_plan_input(input, ancestor)?,
        (None, None) => {}
        _ => return Err(WorkspaceError::new(WorkspaceErrorCode::StalePlan)),
    }
    let parent = plan.output().parent_identity();
    if plan.output().path() != utf8_path(&destination.path_hint())?
        || plan.output().leaf_name() != destination.leaf().to_str().unwrap_or_default()
        || parent.platform() != std::env::consts::OS
        || parent.volume_or_device() != destination.identity().device.to_string()
        || parent.file_id_or_inode() != destination.identity().stable_file_id
        || plan.expected().capsule_id() != Some(output.capsule_id.as_str())
        || plan.expected().revision_id() != Some(output.revision_id.as_str())
        || plan.expected().application_digest() != Some(output.application_digest.as_str())
    {
        return Err(WorkspaceError::new(WorkspaceErrorCode::StalePlan));
    }
    let mut total_input = source
        .source_identity()
        .bytes
        .checked_add(target.source_identity().bytes)
        .ok_or_else(limit_exceeded)?;
    if let Some(ancestor) = ancestor {
        total_input = total_input
            .checked_add(ancestor.source_identity().bytes)
            .ok_or_else(limit_exceeded)?;
    }
    if plan.limits().max_input_bytes() != total_input {
        return Err(WorkspaceError::new(WorkspaceErrorCode::StalePlan));
    }
    Ok(())
}

fn validate_plan_input(
    input: &crate::PlanInput,
    source: &VerifiedWorkspaceSource,
) -> Result<(), WorkspaceError> {
    let identity = source.identity();
    let instance = &identity.overview.instance;
    let schema = identity
        .overview
        .data_schema
        .as_ref()
        .ok_or_else(invalid_contract)?;
    let capsule = input.capsule();
    let live = source.source_identity();
    let digest = source.source_sha256();
    if input.path_hint() != utf8_path(&identity.canonical_path)?
        || input.file_sha256() != digest
        || input.snapshot_sha256() != digest
        || input.size_bytes() != live.bytes
        || input.filesystem_identity().platform() != std::env::consts::OS
        || input.filesystem_identity().volume_or_device() != live.device.to_string()
        || input.filesystem_identity().file_id_or_inode() != live.stable_file_id
        || input.filesystem_identity().modified_ns() != live.modified_ns
        || capsule.format_version() != "0.3"
        || capsule.publisher_key_id().is_some()
        || capsule.capsule_id() != Some(identity.capsule_id.as_str())
        || capsule.revision_id() != instance.revision_id.as_deref()
        || capsule.app_id() != identity.app_id
        || capsule.app_version() != identity.app_version
        || capsule.application_digest() != Some(lower_hex(source.application_digest()).as_str())
        || capsule.data_schema_id() != Some(schema.data_schema_id.as_str())
        || capsule.data_schema_version() != u64::try_from(schema.data_schema_version).ok()
    {
        return Err(WorkspaceError::new(WorkspaceErrorCode::StalePlan));
    }
    Ok(())
}

fn utf8_path(path: &Path) -> Result<String, WorkspaceError> {
    path.to_str()
        .filter(|value| !value.is_empty() && value.len() <= 4_096)
        .map(str::to_owned)
        .ok_or_else(invalid_contract)
}

fn apply_operations_transaction(
    path: &Path,
    target: &VerifiedWorkspaceSource,
    operations: &[PlannedOperation],
    max_value_bytes: u64,
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<(), WorkspaceError> {
    reject_sidecars(path)?;
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| output_failed())?;
    install_progress(&connection, deadline, cancellation)?;
    connection
        .execute_batch(
            "PRAGMA trusted_schema=OFF; PRAGMA foreign_keys=ON; PRAGMA journal_mode=DELETE;",
        )
        .map_err(|_| query_error(deadline, cancellation))?;
    let result = (|| -> Result<(), WorkspaceError> {
        connection
            .execute_batch("BEGIN IMMEDIATE; PRAGMA defer_foreign_keys=ON;")
            .map_err(|_| query_error(deadline, cancellation))?;
        for operation in operations {
            check(deadline, cancellation)?;
            let dataset = target
                .data_contract()
                .datasets
                .get(operation.dataset_index)
                .ok_or_else(invalid_contract)?;
            let table = dataset
                .tables
                .get(operation.table_index)
                .ok_or_else(invalid_contract)?;
            let compared =
                compared_columns_for_connection(&connection, table, deadline, cancellation)?;
            let mut projected = compared.clone();
            if let Some(expected) = &operation.target_values {
                for (column, _) in expected {
                    if !projected.contains(column) {
                        projected.push(column.clone());
                    }
                }
            }
            let current = load_exact_current_row(
                &connection,
                table,
                &compared,
                &projected,
                &operation.key,
                max_value_bytes,
                deadline,
                cancellation,
            )?;
            match operation.action {
                ReconcileAction::InsertFromSource if current.is_some() => {
                    return Err(row_precondition_failed());
                }
                ReconcileAction::InsertFromSource => {}
                _ => {
                    let current = current.as_ref().ok_or_else(row_precondition_failed)?;
                    if Some(current.row_digest.as_str()) != operation.target_row_digest.as_deref() {
                        return Err(row_precondition_failed());
                    }
                    if let Some(expected) = &operation.target_values {
                        for (column, value) in expected {
                            let current_value = current
                                .values
                                .iter()
                                .find(|(candidate, _)| candidate == column)
                                .ok_or_else(row_precondition_failed)?;
                            if !values_equal(&current_value.1, value, max_value_bytes)? {
                                return Err(row_precondition_failed());
                            }
                        }
                    }
                }
            }
            let changes = match operation.action {
                ReconcileAction::InsertFromSource => execute_insert(
                    &connection,
                    table,
                    operation
                        .source_values
                        .as_deref()
                        .ok_or_else(invalid_contract)?,
                    deadline,
                    cancellation,
                )?,
                ReconcileAction::DeleteFromTarget => {
                    execute_delete(&connection, table, &operation.key, deadline, cancellation)?
                }
                ReconcileAction::ReplaceRowFromSource | ReconcileAction::SetFields => {
                    execute_update(
                        &connection,
                        table,
                        operation
                            .source_values
                            .as_deref()
                            .ok_or_else(invalid_contract)?,
                        &operation.key,
                        deadline,
                        cancellation,
                    )?
                }
            };
            if changes != 1 {
                return Err(row_precondition_failed());
            }
        }
        let foreign_key_errors: i64 = connection
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })
            .map_err(|_| query_error(deadline, cancellation))?;
        if foreign_key_errors != 0 {
            return Err(verification_failed());
        }
        connection
            .execute_batch("COMMIT")
            .map_err(|_| query_error(deadline, cancellation))?;
        Ok(())
    })();
    if result.is_err() && !connection.is_autocommit() {
        let _ = connection.execute_batch("ROLLBACK");
    }
    let _ = connection.progress_handler(0, None::<fn() -> bool>);
    drop(connection);
    result?;
    reject_sidecars(path)
}

#[allow(clippy::too_many_arguments)]
fn finalize_reconcile_metadata(
    path: &Path,
    plan: &LifecyclePlan,
    payload_bytes: &[u8],
    output: &ReconcileOutputReview,
    source: &ReconcileReference,
    target: &ReconcileReference,
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<(), WorkspaceError> {
    reject_sidecars(path)?;
    let payload: JsonValue =
        serde_json::from_slice(payload_bytes).map_err(|_| invalid_contract())?;
    let details = payload
        .get("lineage")
        .and_then(|lineage| lineage.get("details"))
        .ok_or_else(invalid_contract)?;
    let details_json = crate::plan::canonical_json(details)?;
    let details_json = String::from_utf8(details_json).map_err(|_| invalid_contract())?;
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| output_failed())?;
    install_progress(&connection, deadline, cancellation)?;
    connection
        .execute_batch(
            "PRAGMA trusted_schema=OFF; PRAGMA foreign_keys=ON; PRAGMA journal_mode=DELETE;",
        )
        .map_err(|_| query_error(deadline, cancellation))?;
    let result = (|| -> Result<(), WorkspaceError> {
        connection
            .execute_batch("BEGIN IMMEDIATE; PRAGMA defer_foreign_keys=ON;")
            .map_err(|_| query_error(deadline, cancellation))?;
        let changed = connection
            .execute(
                "UPDATE capsule_instance SET revision_id=?1, content_updated_at=?2 WHERE id=1 AND capsule_id=?3 AND revision_id=?4",
                params![output.revision_id, plan.created_at(), target.capsule_id, target.revision_id],
            )
            .map_err(|_| query_error(deadline, cancellation))?;
        if changed != 1 {
            return Err(row_precondition_failed());
        }
        let next_sequence: i64 = connection
            .query_row(
                "SELECT coalesce(max(sequence),0)+1 FROM capsule_lineage_event",
                [],
                |row| row.get(0),
            )
            .map_err(|_| query_error(deadline, cancellation))?;
        connection
            .execute(
                "INSERT INTO capsule_lineage_event (event_id,sequence,operation,result_capsule_id,result_revision_id,occurred_at,application_digest,data_schema_id,data_schema_version,plan_digest,details_json) VALUES (?1,?2,'reconcile',?3,?4,?5,?6,?7,?8,?9,?10)",
                params![
                    output.lineage_event_id,
                    next_sequence,
                    output.capsule_id,
                    output.revision_id,
                    plan.created_at(),
                    output.application_digest,
                    target.data_schema_id,
                    target.data_schema_version,
                    plan.plan_digest(),
                    details_json,
                ],
            )
            .map_err(|_| query_error(deadline, cancellation))?;
        for (ordinal, relation, reference) in [
            (1_i64, "target-derived-from", target),
            (2_i64, "changes-applied-from", source),
        ] {
            connection
                .execute(
                    "INSERT INTO capsule_lineage_parent (event_id,ordinal,relation,parent_capsule_id,parent_revision_id,parent_file_sha256) VALUES (?1,?2,?3,?4,?5,?6)",
                    params![
                        output.lineage_event_id,
                        ordinal,
                        relation,
                        reference.capsule_id,
                        reference.revision_id,
                        reference.file_sha256,
                    ],
                )
                .map_err(|_| query_error(deadline, cancellation))?;
        }
        validate_sqlite_sequence_shape(&connection)?;
        let fk_errors: i64 = connection
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })
            .map_err(|_| query_error(deadline, cancellation))?;
        if fk_errors != 0 {
            return Err(verification_failed());
        }
        connection
            .execute_batch("COMMIT")
            .map_err(|_| query_error(deadline, cancellation))?;
        Ok(())
    })();
    if result.is_err() && !connection.is_autocommit() {
        let _ = connection.execute_batch("ROLLBACK");
    }
    let _ = connection.progress_handler(0, None::<fn() -> bool>);
    drop(connection);
    result?;
    reject_sidecars(path)
}

fn validate_sqlite_sequence_shape(connection: &Connection) -> Result<(), WorkspaceError> {
    let exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name='sqlite_sequence')",
            [],
            |row| row.get(0),
        )
        .map_err(|_| verification_failed())?;
    if !exists {
        return Ok(());
    }
    let names = sequence_managed_tables(connection)?;
    let current = capture_sequence_state(connection)?;
    if current.iter().all(|state| names.contains(&state.name)) {
        Ok(())
    } else {
        Err(verification_failed())
    }
}

fn capture_sequence_state(
    connection: &Connection,
) -> Result<Vec<SequenceStateReview>, WorkspaceError> {
    let exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name='sqlite_sequence')",
            [],
            |row| row.get(0),
        )
        .map_err(|_| verification_failed())?;
    if !exists {
        return Ok(Vec::new());
    }
    let mut current = BTreeMap::new();
    let mut current_statement = connection
        .prepare("SELECT name,seq FROM sqlite_sequence ORDER BY name COLLATE BINARY")
        .map_err(|_| verification_failed())?;
    let current_rows = current_statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|_| verification_failed())?;
    for row in current_rows {
        let (name, sequence) = row.map_err(|_| verification_failed())?;
        if sequence < 0 || current.insert(name, sequence).is_some() {
            return Err(verification_failed());
        }
    }
    drop(current_statement);
    Ok(current
        .into_iter()
        .map(|(name, sequence)| SequenceStateReview { name, sequence })
        .collect())
}

fn sequence_managed_tables(connection: &Connection) -> Result<BTreeSet<String>, WorkspaceError> {
    let mut statement = connection
        .prepare(
            r"SELECT name,sql FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite\_%' ESCAPE '\' ORDER BY name COLLATE BINARY",
        )
        .map_err(|_| verification_failed())?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|_| verification_failed())?;
    let mut tables = BTreeSet::new();
    for row in rows {
        let (name, sql) = row.map_err(|_| verification_failed())?;
        if contains_sql_keyword(&sql, b"AUTOINCREMENT") && !tables.insert(name) {
            return Err(verification_failed());
        }
    }
    Ok(tables)
}

fn contains_sql_keyword(sql: &str, keyword: &[u8]) -> bool {
    let bytes = sql.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\'' | b'"' | b'`' => {
                let quote = bytes[index];
                index += 1;
                while index < bytes.len() {
                    if bytes[index] == quote {
                        if bytes.get(index + 1) == Some(&quote) {
                            index += 2;
                        } else {
                            index += 1;
                            break;
                        }
                    } else {
                        index += 1;
                    }
                }
            }
            b'[' => {
                index += 1;
                while index < bytes.len() && bytes[index] != b']' {
                    index += 1;
                }
                index = index.saturating_add(1);
            }
            b'-' if bytes.get(index + 1) == Some(&b'-') => {
                index += 2;
                while index < bytes.len() && !matches!(bytes[index], b'\n' | b'\r') {
                    index += 1;
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index += 2;
                while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/')
                {
                    index += 1;
                }
                index = (index + 2).min(bytes.len());
            }
            byte if byte.is_ascii_alphabetic() || byte == b'_' => {
                let start = index;
                index += 1;
                while index < bytes.len()
                    && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
                {
                    index += 1;
                }
                if bytes[start..index].eq_ignore_ascii_case(keyword) {
                    return true;
                }
            }
            _ => index += 1,
        }
    }
    false
}

fn vacuum_private(
    path: &Path,
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<(), WorkspaceError> {
    reject_sidecars(path)?;
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| output_failed())?;
    install_progress(&connection, deadline, cancellation)?;
    let mode: String = connection
        .query_row("PRAGMA journal_mode=DELETE", [], |row| row.get(0))
        .map_err(|_| query_error(deadline, cancellation))?;
    if !mode.eq_ignore_ascii_case("delete") {
        return Err(verification_failed());
    }
    connection
        .execute_batch("VACUUM")
        .map_err(|_| query_error(deadline, cancellation))?;
    let freelist: i64 = connection
        .query_row("PRAGMA freelist_count", [], |row| row.get(0))
        .map_err(|_| query_error(deadline, cancellation))?;
    if freelist != 0 {
        return Err(verification_failed());
    }
    let _ = connection.progress_handler(0, None::<fn() -> bool>);
    drop(connection);
    check(deadline, cancellation)?;
    reject_sidecars(path)
}

#[allow(clippy::too_many_arguments)]
fn validate_reconcile_output(
    output: &VerifiedWorkspaceSource,
    source: &VerifiedWorkspaceSource,
    target: &VerifiedWorkspaceSource,
    plan: &LifecyclePlan,
    payload_bytes: &[u8],
    payload_digest: &str,
    compare_report_digest: &str,
    expected_output: &ReconcileOutputReview,
    operation_reviews: &[ReconcileOperationReview],
    dataset_states: &[ReconcileDatasetStateReview],
    sequence_state: &[SequenceStateReview],
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<(), WorkspaceError> {
    check(deadline, cancellation)?;
    validate_reconcile_plan_shape(plan, payload_digest)?;
    let payload: JsonValue =
        serde_json::from_slice(payload_bytes).map_err(|_| invalid_contract())?;
    if crate::plan::canonical_json(&payload)? != payload_bytes
        || payload.get("payload_digest").and_then(JsonValue::as_str) != Some(payload_digest)
        || reconcile_payload_digest(&payload)? != payload_digest
        || payload
            .get("compare_report_digest")
            .and_then(JsonValue::as_str)
            != Some(compare_report_digest)
        || payload
            .get("operations")
            .and_then(JsonValue::as_array)
            .map(Vec::len)
            != Some(operation_reviews.len())
    {
        return Err(verification_failed());
    }
    if !output.has_complete_valid_signature_inventory()
        || output.application_digest() != target.application_digest()
        || output.data_contract() != target.data_contract()
        || schema_catalog_digest(output.verified.connection(), deadline, cancellation)?
            != schema_catalog_digest(target.verified.connection(), deadline, cancellation)?
    {
        return Err(verification_failed());
    }
    let source_identity = source.identity();
    let target_identity = target.identity();
    let output_identity = output.identity();
    if output_identity.user_version != 3
        || output_identity.format_version != "0.3"
        || output_identity.capsule_id != target_identity.capsule_id
        || output_identity.overview.instance.revision_id.as_deref()
            != Some(expected_output.revision_id.as_str())
        || output_identity.app_id != target_identity.app_id
        || output_identity.app_version != target_identity.app_version
        || output_identity.overview.application != target_identity.overview.application
        || output_identity.overview.data_schema != target_identity.overview.data_schema
        || output_identity.overview.instance.title != target_identity.overview.instance.title
        || output_identity.overview.instance.description
            != target_identity.overview.instance.description
        || output_identity.overview.instance.document_kind
            != target_identity.overview.instance.document_kind
        || output_identity.overview.instance.tags != target_identity.overview.instance.tags
        || output_identity.overview.instance.icon_asset_id
            != target_identity.overview.instance.icon_asset_id
        || output_identity.overview.instance.cover_asset_id
            != target_identity.overview.instance.cover_asset_id
        || output_identity.overview.instance.created_at
            != target_identity.overview.instance.created_at
        || output_identity.overview.instance.content_updated_at != plan.created_at()
    {
        return Err(verification_failed());
    }
    let control = WorkspaceControl::new(remaining(deadline)?, cancellation);
    let output_reference = reconcile_reference(output, &control)?;
    if output_reference.signature_count != expected_output.signature_count
        || output_reference.signature_inventory_digest != expected_output.signature_inventory_digest
        || output_reference.application_digest != expected_output.application_digest
        || output_reference.capsule_id != expected_output.capsule_id
        || output_reference.revision_id != expected_output.revision_id
    {
        return Err(verification_failed());
    }
    for table in [
        "capsule_instance_asset",
        "capsule_grant",
        "capsule_change_log",
    ] {
        if platform_table_digest(target.verified.connection(), table, deadline, cancellation)?
            != platform_table_digest(output.verified.connection(), table, deadline, cancellation)?
        {
            return Err(verification_failed());
        }
    }
    if output.lineage().events.len() != target.lineage().events.len() + 1
        || output.lineage().events[..target.lineage().events.len()] != target.lineage().events[..]
    {
        return Err(verification_failed());
    }
    let event = output
        .lineage()
        .events
        .last()
        .ok_or_else(verification_failed)?;
    let details = payload
        .get("lineage")
        .and_then(|lineage| lineage.get("details"))
        .ok_or_else(invalid_contract)?;
    let details_sha256 = lower_hex(&Sha256::digest(crate::plan::canonical_json(details)?));
    if event.event_id != expected_output.lineage_event_id
        || event.operation != crate::LineageOperation::Reconcile
        || event.result_capsule_id != expected_output.capsule_id
        || event.result_revision_id != expected_output.revision_id
        || event.occurred_at != plan.created_at()
        || event.application_digest != expected_output.application_digest
        || event.data_schema_id != output_reference.data_schema_id
        || i64::try_from(event.data_schema_version).ok()
            != Some(output_reference.data_schema_version)
        || event.plan_digest != plan.plan_digest()
        || event.details_sha256 != details_sha256
        || event.parents.len() != 2
        || event.parents[0].relation != crate::ParentRelation::TargetDerivedFrom
        || event.parents[0].capsule_id.as_deref() != Some(target_identity.capsule_id.as_str())
        || event.parents[0].revision_id.as_deref()
            != target_identity.overview.instance.revision_id.as_deref()
        || event.parents[0].file_sha256 != target.source_sha256()
        || event.parents[1].relation != crate::ParentRelation::ChangesAppliedFrom
        || event.parents[1].capsule_id.as_deref() != Some(source_identity.capsule_id.as_str())
        || event.parents[1].revision_id.as_deref()
            != source_identity.overview.instance.revision_id.as_deref()
        || event.parents[1].file_sha256 != source.source_sha256()
    {
        return Err(verification_failed());
    }
    validate_dataset_states(
        source,
        target,
        output,
        dataset_states,
        deadline,
        cancellation,
    )?;
    if capture_sequence_state(output.verified.connection())? != sequence_state {
        return Err(verification_failed());
    }
    let freelist: i64 = output
        .verified
        .connection()
        .query_row("PRAGMA freelist_count", [], |row| row.get(0))
        .map_err(|_| verification_failed())?;
    if freelist != 0 {
        return Err(verification_failed());
    }
    reject_sidecars(&output.identity().canonical_path)?;
    assert_reconcile_inputs_current(source, target, deadline, cancellation)
}

fn schema_catalog_digest(
    connection: &Connection,
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<[u8; 32], WorkspaceError> {
    let mut statement = connection
        .prepare(
            "SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type COLLATE BINARY,name COLLATE BINARY,tbl_name COLLATE BINARY",
        )
        .map_err(|_| verification_failed())?;
    let mut rows = statement.query([]).map_err(|_| verification_failed())?;
    let mut count = 0_u64;
    let mut hasher = Sha256::new();
    frame_text_for_hasher(&mut hasher, "org.sqlite-capsule.sqlite-schema/1")?;
    while let Some(row) = rows.next().map_err(|_| verification_failed())? {
        check(deadline, cancellation)?;
        count = count.checked_add(1).ok_or_else(limit_exceeded)?;
        if count > 8_192 {
            return Err(limit_exceeded());
        }
        for index in 0..4 {
            let value = owned_value(
                row.get_ref(index).map_err(|_| verification_failed())?,
                HARD_VALUE_BYTES,
            )?;
            let bytes = crate::compare::canonical_value_bytes(&value, HARD_VALUE_BYTES)?;
            hasher.update(
                u64::try_from(bytes.len())
                    .map_err(|_| limit_exceeded())?
                    .to_be_bytes(),
            );
            hasher.update(bytes);
        }
    }
    hasher.update(count.to_be_bytes());
    Ok(hasher.finalize().into())
}

fn validate_dataset_states(
    source: &VerifiedWorkspaceSource,
    target: &VerifiedWorkspaceSource,
    output: &VerifiedWorkspaceSource,
    expected: &[ReconcileDatasetStateReview],
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<(), WorkspaceError> {
    if expected.len() != source.data_contract().datasets.len() {
        return Err(verification_failed());
    }
    let mut source_rows = HARD_ROWS_SCANNED;
    let mut target_rows = HARD_ROWS_SCANNED;
    let mut output_rows = HARD_ROWS_SCANNED;
    let mut source_bytes = HARD_STREAM_BYTES;
    let mut target_bytes = HARD_STREAM_BYTES;
    let mut output_bytes = HARD_STREAM_BYTES;
    for state in expected {
        let source_dataset = source
            .data_contract()
            .datasets
            .iter()
            .find(|d| d.id == state.dataset_id)
            .ok_or_else(verification_failed)?;
        let target_dataset = target
            .data_contract()
            .datasets
            .iter()
            .find(|d| d.id == state.dataset_id)
            .ok_or_else(verification_failed)?;
        let output_dataset = output
            .data_contract()
            .datasets
            .iter()
            .find(|d| d.id == state.dataset_id)
            .ok_or_else(verification_failed)?;
        let source_state = crate::template_state::dataset_state_with_budget(
            source,
            source_dataset,
            &mut source_rows,
            &mut source_bytes,
            deadline,
            cancellation,
        )?;
        let target_state = crate::template_state::dataset_state_with_budget(
            target,
            target_dataset,
            &mut target_rows,
            &mut target_bytes,
            deadline,
            cancellation,
        )?;
        let output_state = crate::template_state::dataset_state_with_budget(
            output,
            output_dataset,
            &mut output_rows,
            &mut output_bytes,
            deadline,
            cancellation,
        )?;
        if source_state != (state.source_row_count, state.source_state_sha256.clone())
            || target_state != (state.target_row_count, state.target_state_sha256.clone())
            || output_state != (state.output_row_count, state.output_state_sha256.clone())
        {
            return Err(verification_failed());
        }
    }
    Ok(())
}

fn platform_table_digest(
    connection: &Connection,
    table: &str,
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<[u8; 32], WorkspaceError> {
    check(deadline, cancellation)?;
    let mut columns_statement = connection
        .prepare("SELECT name FROM pragma_table_xinfo(?1) WHERE hidden=0 ORDER BY cid LIMIT 257")
        .map_err(|_| verification_failed())?;
    let columns = columns_statement
        .query_map([table], |row| row.get::<_, String>(0))
        .map_err(|_| verification_failed())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| verification_failed())?;
    if columns.is_empty() || columns.len() > HARD_COLUMNS {
        return Err(verification_failed());
    }
    let projection = columns
        .iter()
        .map(|column| crate::compare::quote_identifier(column))
        .collect::<Vec<_>>()
        .join(",");
    let ordering = columns
        .iter()
        .map(|column| crate::compare::quote_identifier(column))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT {projection} FROM {} ORDER BY {ordering} LIMIT 100001",
        crate::compare::quote_identifier(table)
    );
    let mut statement = connection
        .prepare(&sql)
        .map_err(|_| verification_failed())?;
    let mut rows = statement.query([]).map_err(|_| verification_failed())?;
    let mut count = 0_u64;
    let mut hasher = Sha256::new();
    frame_text_for_hasher(&mut hasher, table)?;
    while let Some(row) = rows.next().map_err(|_| verification_failed())? {
        check(deadline, cancellation)?;
        count = count.checked_add(1).ok_or_else(limit_exceeded)?;
        if count > HARD_ROWS_SCANNED {
            return Err(limit_exceeded());
        }
        for index in 0..columns.len() {
            let value = owned_value(
                row.get_ref(index).map_err(|_| verification_failed())?,
                HARD_VALUE_BYTES,
            )?;
            let bytes = crate::compare::canonical_value_bytes(&value, HARD_VALUE_BYTES)?;
            hasher.update(
                u64::try_from(bytes.len())
                    .map_err(|_| limit_exceeded())?
                    .to_be_bytes(),
            );
            hasher.update(bytes);
        }
    }
    hasher.update(count.to_be_bytes());
    Ok(hasher.finalize().into())
}

fn frame_text_for_hasher(hasher: &mut Sha256, value: &str) -> Result<(), WorkspaceError> {
    hasher.update(
        u64::try_from(value.len())
            .map_err(|_| limit_exceeded())?
            .to_be_bytes(),
    );
    hasher.update(value.as_bytes());
    Ok(())
}

fn open_output_bound(
    path: &Path,
    expected_size: u64,
    expected_sha256: [u8; 32],
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<VerifiedWorkspaceSource, WorkspaceError> {
    VerifiedWorkspaceSource::open_with_control_expected_binding(
        path,
        &crate::WorkspaceLimits {
            deadline: remaining(deadline)?,
            ..crate::WorkspaceLimits::default()
        },
        cancellation,
        Some(expected_size),
        Some(expected_sha256),
    )
}

fn open_output(
    path: &Path,
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<VerifiedWorkspaceSource, WorkspaceError> {
    VerifiedWorkspaceSource::open_with_control(
        path,
        &crate::WorkspaceLimits {
            deadline: remaining(deadline)?,
            ..crate::WorkspaceLimits::default()
        },
        cancellation,
    )
}

fn snapshot_held_file(
    source: &File,
    max_bytes: u64,
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<tempfile::NamedTempFile, sqlite_capsule_lifecycle::LifecycleError> {
    let mut source = source.try_clone()?;
    source.seek(SeekFrom::Start(0))?;
    let mut output = tempfile::NamedTempFile::new()?;
    sqlite_capsule_lifecycle::protect_private_file(output.path())?;
    let mut copied = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        if cancellation.is_cancelled() || Instant::now() >= deadline {
            return Err(sqlite_capsule_lifecycle::LifecycleError::PostPublishVerification);
        }
        let read = source.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        copied =
            copied
                .checked_add(u64::try_from(read).map_err(|_| {
                    sqlite_capsule_lifecycle::LifecycleError::PostPublishVerification
                })?)
                .ok_or(sqlite_capsule_lifecycle::LifecycleError::PostPublishVerification)?;
        if copied > max_bytes {
            return Err(sqlite_capsule_lifecycle::LifecycleError::PostPublishVerification);
        }
        output.write_all(&buffer[..read])?;
    }
    output.flush()?;
    output.as_file().sync_all()?;
    Ok(output)
}

#[derive(Debug)]
struct CurrentRow {
    row_digest: String,
    values: Vec<(String, CompareValue)>,
}

fn compared_columns_for_connection(
    connection: &Connection,
    table: &DatasetTable,
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<Vec<String>, WorkspaceError> {
    let control = WorkspaceControl::new(remaining(deadline)?, cancellation);
    control.install(connection)?;
    let columns = crate::compare::compared_columns(connection, table, &control)?;
    let _ = connection.progress_handler(0, None::<fn() -> bool>);
    Ok(columns)
}

#[allow(clippy::too_many_arguments)]
fn load_exact_current_row(
    connection: &Connection,
    table: &DatasetTable,
    compared_columns: &[String],
    projected_columns: &[String],
    key: &[(String, CompareValue)],
    max_value_bytes: u64,
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<Option<CurrentRow>, WorkspaceError> {
    check(deadline, cancellation)?;
    let projection = projected_columns
        .iter()
        .map(|column| crate::compare::quote_identifier(column))
        .collect::<Vec<_>>()
        .join(",");
    let predicate = key
        .iter()
        .enumerate()
        .map(|(index, (column, _))| {
            format!(
                "{}=?{}",
                crate::compare::quote_identifier(column),
                index + 1
            )
        })
        .collect::<Vec<_>>()
        .join(" AND ");
    let sql = format!(
        "SELECT {projection} FROM {} WHERE {predicate} LIMIT 2",
        crate::compare::quote_identifier(&table.name)
    );
    let values = key
        .iter()
        .map(|(_, value)| sqlite_value(value))
        .collect::<Vec<_>>();
    let mut statement = connection.prepare(&sql).map_err(|_| invalid_contract())?;
    let mut rows = statement
        .query(params_from_iter(values.iter()))
        .map_err(|_| query_error(deadline, cancellation))?;
    let Some(row) = rows
        .next()
        .map_err(|_| query_error(deadline, cancellation))?
    else {
        return Ok(None);
    };
    let values = projected_columns
        .iter()
        .enumerate()
        .map(|(index, column)| {
            Ok((
                column.clone(),
                owned_value(
                    row.get_ref(index).map_err(|_| invalid_contract())?,
                    max_value_bytes,
                )?,
            ))
        })
        .collect::<Result<Vec<_>, WorkspaceError>>()?;
    if rows
        .next()
        .map_err(|_| query_error(deadline, cancellation))?
        .is_some()
    {
        return Err(invalid_contract());
    }
    let actual_key = table
        .primary_key
        .iter()
        .map(|column| {
            values
                .iter()
                .find(|(candidate, _)| candidate == column)
                .cloned()
                .ok_or_else(invalid_contract)
        })
        .collect::<Result<Vec<_>, WorkspaceError>>()?;
    if actual_key.len() != key.len() {
        return Err(row_precondition_failed());
    }
    for (actual, expected) in actual_key.iter().zip(key) {
        if actual.0 != expected.0 || !values_equal(&actual.1, &expected.1, max_value_bytes)? {
            return Err(row_precondition_failed());
        }
    }
    let compared = compared_columns
        .iter()
        .map(|column| {
            values
                .iter()
                .find(|(candidate, _)| candidate == column)
                .cloned()
                .ok_or_else(invalid_contract)
        })
        .collect::<Result<Vec<_>, WorkspaceError>>()?;
    let frame =
        crate::compare::canonical_compare_row(&table.name, key, &compared, max_value_bytes)?;
    Ok(Some(CurrentRow {
        row_digest: lower_hex(&Sha256::digest(frame)),
        values,
    }))
}

fn execute_insert(
    connection: &Connection,
    table: &DatasetTable,
    values: &[(String, CompareValue)],
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<usize, WorkspaceError> {
    if values.is_empty() {
        return Err(invalid_contract());
    }
    let columns = values
        .iter()
        .map(|(column, _)| crate::compare::quote_identifier(column))
        .collect::<Vec<_>>()
        .join(",");
    let placeholders = (1..=values.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "INSERT INTO {} ({columns}) VALUES ({placeholders})",
        crate::compare::quote_identifier(&table.name)
    );
    let values = values
        .iter()
        .map(|(_, value)| sqlite_value(value))
        .collect::<Vec<_>>();
    connection
        .execute(&sql, params_from_iter(values.iter()))
        .map_err(|_| query_error(deadline, cancellation))
}

fn execute_update(
    connection: &Connection,
    table: &DatasetTable,
    values: &[(String, CompareValue)],
    key: &[(String, CompareValue)],
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<usize, WorkspaceError> {
    if values.is_empty() || key.is_empty() {
        return Err(invalid_contract());
    }
    let assignments = values
        .iter()
        .enumerate()
        .map(|(index, (column, _))| {
            format!(
                "{}=?{}",
                crate::compare::quote_identifier(column),
                index + 1
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let predicate = key
        .iter()
        .enumerate()
        .map(|(index, (column, _))| {
            format!(
                "{}=?{}",
                crate::compare::quote_identifier(column),
                values.len() + index + 1
            )
        })
        .collect::<Vec<_>>()
        .join(" AND ");
    let sql = format!(
        "UPDATE {} SET {assignments} WHERE {predicate}",
        crate::compare::quote_identifier(&table.name)
    );
    let bound = values
        .iter()
        .map(|(_, value)| sqlite_value(value))
        .chain(key.iter().map(|(_, value)| sqlite_value(value)))
        .collect::<Vec<_>>();
    connection
        .execute(&sql, params_from_iter(bound.iter()))
        .map_err(|_| query_error(deadline, cancellation))
}

fn execute_delete(
    connection: &Connection,
    table: &DatasetTable,
    key: &[(String, CompareValue)],
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<usize, WorkspaceError> {
    if key.is_empty() {
        return Err(invalid_contract());
    }
    let predicate = key
        .iter()
        .enumerate()
        .map(|(index, (column, _))| {
            format!(
                "{}=?{}",
                crate::compare::quote_identifier(column),
                index + 1
            )
        })
        .collect::<Vec<_>>()
        .join(" AND ");
    let sql = format!(
        "DELETE FROM {} WHERE {predicate}",
        crate::compare::quote_identifier(&table.name)
    );
    let values = key
        .iter()
        .map(|(_, value)| sqlite_value(value))
        .collect::<Vec<_>>();
    connection
        .execute(&sql, params_from_iter(values.iter()))
        .map_err(|_| query_error(deadline, cancellation))
}

fn sqlite_value(value: &CompareValue) -> Value {
    match value {
        CompareValue::Null => Value::Null,
        CompareValue::Integer(value) => Value::Integer(*value),
        CompareValue::Real(value) => Value::Real(*value),
        CompareValue::Text(value) => {
            Value::Text(String::from_utf8(value.clone()).expect("validated UTF-8"))
        }
        CompareValue::Blob(value) => Value::Blob(value.clone()),
    }
}

fn assert_reconcile_inputs_current(
    source: &VerifiedWorkspaceSource,
    target: &VerifiedWorkspaceSource,
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<(), WorkspaceError> {
    check(deadline, cancellation)?;
    let limits = crate::WorkspaceLimits {
        deadline: remaining(deadline)?,
        ..crate::WorkspaceLimits::default()
    };
    source.assert_current_with_control(&limits, cancellation)?;
    let limits = crate::WorkspaceLimits {
        deadline: remaining(deadline)?,
        ..crate::WorkspaceLimits::default()
    };
    target.assert_current_with_control(&limits, cancellation)
}

fn assert_reconcile_authorities_current(
    source: &VerifiedWorkspaceSource,
    target: &VerifiedWorkspaceSource,
    ancestor: Option<&VerifiedWorkspaceSource>,
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<(), WorkspaceError> {
    assert_reconcile_inputs_current(source, target, deadline, cancellation)?;
    if let Some(ancestor) = ancestor {
        let limits = crate::WorkspaceLimits {
            deadline: remaining(deadline)?,
            ..crate::WorkspaceLimits::default()
        };
        ancestor.assert_current_with_control(&limits, cancellation)?;
    }
    Ok(())
}

fn install_progress(
    connection: &Connection,
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<(), WorkspaceError> {
    let cancelled = cancellation.shared_flag();
    connection
        .progress_handler(
            1_000,
            Some(move || cancelled.load(Ordering::Relaxed) || Instant::now() >= deadline),
        )
        .map_err(|_| output_failed())
}

fn reject_sidecars(path: &Path) -> Result<(), WorkspaceError> {
    let file_name = path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(invalid_contract)?;
    for suffix in ["-journal", "-wal", "-shm"] {
        if path.with_file_name(format!("{file_name}{suffix}")).exists() {
            return Err(verification_failed());
        }
    }
    Ok(())
}

fn map_launch_output_error(error: sqlite_capsule_launch::LaunchError) -> WorkspaceError {
    match error {
        sqlite_capsule_launch::LaunchError::Cancelled => cancelled(),
        sqlite_capsule_launch::LaunchError::LimitExceeded => limit_exceeded(),
        sqlite_capsule_launch::LaunchError::SourceRace => {
            WorkspaceError::new(WorkspaceErrorCode::StalePlan)
        }
        _ => output_failed(),
    }
}

fn query_error(deadline: Instant, cancellation: &CancellationToken) -> WorkspaceError {
    check(deadline, cancellation)
        .err()
        .unwrap_or_else(verification_failed)
}

fn check(deadline: Instant, cancellation: &CancellationToken) -> Result<(), WorkspaceError> {
    if cancellation.is_cancelled() {
        Err(cancelled())
    } else if Instant::now() >= deadline {
        Err(limit_exceeded())
    } else {
        Ok(())
    }
}

#[cfg(test)]
fn maybe_crash(stage: &str) {
    if std::env::var_os("SQLITE_CAPSULE_RECONCILE_CRASH_STAGE").is_some_and(|value| value == stage)
    {
        std::process::exit(97);
    }
}

#[cfg(not(test))]
const fn maybe_crash(_stage: &str) {}

const fn cancelled() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::Cancelled)
}

const fn verification_failed() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::VerificationFailed)
}

const fn output_failed() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::OutputPublishFailed)
}

#[allow(clippy::too_many_arguments)]
fn review_digest(
    report_digest: &str,
    source: &ReconcileReference,
    target: &ReconcileReference,
    ancestor: Option<&ReconcileReference>,
    planned: &[PlannedOperation],
    operations: &[ReconcileOperationReview],
    resolved_conflicts: &[ResolvedThreeWayConflictReview],
    output: &ReconcileOutputReview,
    destination: &DestinationReservation,
    sequence_state: &[SequenceStateReview],
    limits: &ReconcileReviewLimitsApplied,
    confirmed_sensitive_dataset_indices: &BTreeSet<usize>,
) -> Result<String, WorkspaceError> {
    if planned.len() != operations.len() {
        return Err(invalid_contract());
    }
    let mut frame = Vec::new();
    frame_text(&mut frame, REVIEW_DIGEST_PROFILE)?;
    frame_text(&mut frame, report_digest)?;
    frame_reference(&mut frame, source)?;
    frame_reference(&mut frame, target)?;
    match ancestor {
        Some(ancestor) => {
            frame.push(1);
            frame_reference(&mut frame, ancestor)?;
        }
        None => frame.push(0),
    }
    frame_text(&mut frame, &output.capsule_id)?;
    frame_text(&mut frame, &output.revision_id)?;
    frame_text(&mut frame, &output.application_digest)?;
    frame.extend_from_slice(&output.signature_count.to_be_bytes());
    frame_text(&mut frame, &output.signature_inventory_digest)?;
    frame.push(u8::from(output.preserves_target_signature_inventory));
    frame_text(&mut frame, &output.lineage_event_id)?;
    frame_u64(&mut frame, output.lineage_parents.len())?;
    for parent in &output.lineage_parents {
        frame.push(parent.ordinal);
        frame_text(&mut frame, parent.relation.label())?;
        frame_text(&mut frame, &parent.file_sha256)?;
        frame_text(&mut frame, &parent.capsule_id)?;
        frame_text(&mut frame, &parent.revision_id)?;
    }
    let destination_path = destination.path_hint();
    let destination_path = destination_path.to_str().ok_or_else(invalid_contract)?;
    frame_text(&mut frame, destination_path)?;
    frame.extend_from_slice(&destination.identity().device.to_be_bytes());
    frame.extend_from_slice(&destination.identity().file.to_be_bytes());
    frame_text(&mut frame, &destination.identity().stable_file_id)?;
    frame_u64(&mut frame, operations.len())?;
    for operation in operations {
        frame.extend_from_slice(&operation.sequence.to_be_bytes());
        frame_text(&mut frame, &operation.dataset_id)?;
        frame_text(&mut frame, &operation.table)?;
        frame_text(&mut frame, &operation.key_digest)?;
        frame_text(&mut frame, operation.action.label())?;
        frame_text(&mut frame, operation.basis.label())?;
        frame_optional_text(&mut frame, operation.source_row_digest.as_deref())?;
        frame_optional_text(
            &mut frame,
            operation.precondition_target_row_digest.as_deref(),
        )?;
        frame_optional_text(&mut frame, operation.ancestor_row_digest.as_deref())?;
        frame_optional_text(&mut frame, operation.conflict_id.as_deref())?;
        frame_u64(&mut frame, operation.fields.len())?;
        for field in &operation.fields {
            frame_text(&mut frame, &field.column)?;
            frame_text(&mut frame, &field.source_value_digest)?;
            frame_text(&mut frame, &field.target_value_digest)?;
        }
    }
    frame_u64(&mut frame, planned.len())?;
    for operation in planned {
        frame_text(&mut frame, operation.action.label())?;
        frame_text(&mut frame, operation.basis.label())?;
        frame_u64(&mut frame, operation.dataset_index)?;
        frame_u64(&mut frame, operation.table_index)?;
        frame_text(&mut frame, &operation.write_set_digest)?;
        frame_optional_text(&mut frame, operation.ancestor_row_digest.as_deref())?;
        frame_optional_text(&mut frame, operation.conflict_id.as_deref())?;
    }
    frame_u64(&mut frame, resolved_conflicts.len())?;
    for resolved in resolved_conflicts {
        frame_text(&mut frame, &resolved.conflict.id)?;
        frame_text(&mut frame, resolved.resolution.label())?;
    }
    frame_u64(&mut frame, sequence_state.len())?;
    for state in sequence_state {
        frame_text(&mut frame, &state.name)?;
        frame.extend_from_slice(&state.sequence.to_be_bytes());
    }
    frame_u64(&mut frame, confirmed_sensitive_dataset_indices.len())?;
    for index in confirmed_sensitive_dataset_indices {
        frame.extend_from_slice(
            &u64::try_from(*index)
                .map_err(|_| limit_exceeded())?
                .to_be_bytes(),
        );
    }
    frame.extend_from_slice(&limits.deadline_ms.to_be_bytes());
    frame.extend_from_slice(&limits.review_lifetime_ms.to_be_bytes());
    frame_u64(&mut frame, limits.max_operations)?;
    frame.extend_from_slice(&limits.max_rows_scanned.to_be_bytes());
    frame.extend_from_slice(&limits.max_value_bytes.to_be_bytes());
    frame.extend_from_slice(&limits.max_stream_bytes.to_be_bytes());
    frame.extend_from_slice(&limits.max_retained_bytes.to_be_bytes());
    Ok(lower_hex(&Sha256::digest(frame)))
}

fn frame_reference(
    frame: &mut Vec<u8>,
    reference: &ReconcileReference,
) -> Result<(), WorkspaceError> {
    frame_text(frame, &reference.file_sha256)?;
    frame_text(frame, &reference.capsule_id)?;
    frame_text(frame, &reference.revision_id)?;
    frame_text(frame, &reference.application_digest)?;
    frame.extend_from_slice(&reference.signature_count.to_be_bytes());
    frame_text(frame, &reference.signature_inventory_digest)?;
    frame_text(frame, &reference.data_schema_id)?;
    frame.extend_from_slice(&reference.data_schema_version.to_be_bytes());
    Ok(())
}

fn frame_text(frame: &mut Vec<u8>, value: &str) -> Result<(), WorkspaceError> {
    frame.extend_from_slice(
        &u64::try_from(value.len())
            .map_err(|_| limit_exceeded())?
            .to_be_bytes(),
    );
    frame.extend_from_slice(value.as_bytes());
    Ok(())
}

fn frame_optional_text(frame: &mut Vec<u8>, value: Option<&str>) -> Result<(), WorkspaceError> {
    match value {
        None => frame.push(0),
        Some(value) => {
            frame.push(1);
            frame_text(frame, value)?;
        }
    }
    Ok(())
}

fn frame_u64(frame: &mut Vec<u8>, value: usize) -> Result<(), WorkspaceError> {
    frame.extend_from_slice(
        &u64::try_from(value)
            .map_err(|_| limit_exceeded())?
            .to_be_bytes(),
    );
    Ok(())
}

fn value_digest(value: &CompareValue, max: u64) -> Result<String, WorkspaceError> {
    Ok(lower_hex(&Sha256::digest(
        crate::compare::canonical_value_bytes(value, max)?,
    )))
}

fn values_equal(
    left: &CompareValue,
    right: &CompareValue,
    max: u64,
) -> Result<bool, WorkspaceError> {
    Ok(crate::compare::canonical_value_bytes(left, max)?
        == crate::compare::canonical_value_bytes(right, max)?)
}

fn selected_write_values(
    values: &[(String, CompareValue)],
    selected_columns: &[String],
) -> Result<Vec<(String, CompareValue)>, WorkspaceError> {
    selected_columns
        .iter()
        .map(|column| {
            values
                .iter()
                .find(|(candidate, _)| candidate == column)
                .cloned()
                .ok_or_else(invalid_contract)
        })
        .collect()
}

fn write_set_digest(
    table: &DatasetTable,
    key: &[(String, CompareValue)],
    action: ReconcileAction,
    values: &[(String, CompareValue)],
    max_value_bytes: u64,
) -> Result<String, WorkspaceError> {
    let mut frame = Vec::new();
    frame_text(&mut frame, WRITE_SET_PROFILE)?;
    frame_text(&mut frame, &table.name)?;
    frame_text(&mut frame, action.label())?;
    frame_u64(&mut frame, key.len())?;
    for (column, value) in key {
        frame_text(&mut frame, column)?;
        let value = crate::compare::canonical_value_bytes(value, max_value_bytes)?;
        frame_u64(&mut frame, value.len())?;
        frame.extend_from_slice(&value);
    }
    frame_u64(&mut frame, values.len())?;
    for (column, value) in values {
        frame_text(&mut frame, column)?;
        let value = crate::compare::canonical_value_bytes(value, max_value_bytes)?;
        frame_u64(&mut frame, value.len())?;
        frame.extend_from_slice(&value);
    }
    Ok(lower_hex(&Sha256::digest(frame)))
}

fn charge_bytes(total: &mut u64, bytes: u64, maximum: u64) -> Result<(), WorkspaceError> {
    *total = total.checked_add(bytes).ok_or_else(limit_exceeded)?;
    if *total > maximum {
        return Err(limit_exceeded());
    }
    Ok(())
}

fn owned_value(value: ValueRef<'_>, maximum: u64) -> Result<CompareValue, WorkspaceError> {
    let value = match value {
        ValueRef::Null => CompareValue::Null,
        ValueRef::Integer(value) => CompareValue::Integer(value),
        ValueRef::Real(value) if value.is_finite() => CompareValue::Real(value),
        ValueRef::Real(_) => return Err(invalid_contract()),
        ValueRef::Text(value) => {
            if u64::try_from(value.len()).map_err(|_| limit_exceeded())? > maximum {
                return Err(limit_exceeded());
            }
            std::str::from_utf8(value).map_err(|_| invalid_contract())?;
            CompareValue::Text(value.to_vec())
        }
        ValueRef::Blob(value) => {
            if u64::try_from(value.len()).map_err(|_| limit_exceeded())? > maximum {
                return Err(limit_exceeded());
            }
            CompareValue::Blob(value.to_vec())
        }
    };
    Ok(value)
}

fn effective_limits(
    requested: &ReconcileReviewLimits,
) -> Result<ReconcileReviewLimitsApplied, WorkspaceError> {
    let applied = ReconcileReviewLimitsApplied {
        deadline_ms: u64::try_from(requested.deadline.min(HARD_DEADLINE).as_millis())
            .map_err(|_| limit_exceeded())?,
        review_lifetime_ms: u64::try_from(
            requested
                .review_lifetime
                .min(HARD_REVIEW_LIFETIME)
                .as_millis(),
        )
        .map_err(|_| limit_exceeded())?,
        max_operations: requested.max_operations.min(HARD_OPERATIONS),
        max_rows_scanned: requested.max_rows_scanned.min(HARD_ROWS_SCANNED),
        max_value_bytes: requested.max_value_bytes.min(HARD_VALUE_BYTES),
        max_stream_bytes: requested.max_stream_bytes.min(HARD_STREAM_BYTES),
        max_retained_bytes: requested.max_retained_bytes.min(HARD_RETAINED_BYTES),
    };
    if applied.deadline_ms == 0
        || applied.review_lifetime_ms == 0
        || applied.max_operations == 0
        || applied.max_rows_scanned == 0
        || applied.max_value_bytes == 0
        || applied.max_stream_bytes == 0
        || applied.max_retained_bytes == 0
    {
        return Err(limit_exceeded());
    }
    Ok(applied)
}

fn remaining(deadline: Instant) -> Result<Duration, WorkspaceError> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(limit_exceeded());
    }
    Ok(remaining)
}

fn remaining_effective(
    deadline: Instant,
    expiry_code: WorkspaceErrorCode,
    cancellation: &CancellationToken,
) -> Result<Duration, WorkspaceError> {
    if cancellation.is_cancelled() {
        return Err(cancelled());
    }
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(WorkspaceError::new(expiry_code));
    }
    Ok(remaining)
}

fn bounded_authority_work_deadline(
    authority_expires_at: Instant,
    work_budget: Duration,
    now: Instant,
) -> Result<(Instant, WorkspaceErrorCode), WorkspaceError> {
    if now >= authority_expires_at {
        return Err(WorkspaceError::new(WorkspaceErrorCode::SessionExpired));
    }
    let work_expires_at = now.checked_add(work_budget).ok_or_else(limit_exceeded)?;
    if authority_expires_at <= work_expires_at {
        Ok((authority_expires_at, WorkspaceErrorCode::SessionExpired))
    } else {
        Ok((work_expires_at, WorkspaceErrorCode::LimitExceeded))
    }
}

fn bounded_plan_deadline(
    operation_deadline: Instant,
    plan: &LifecyclePlan,
    wall_now: SystemTime,
    monotonic_now: Instant,
) -> Result<(Instant, WorkspaceErrorCode), WorkspaceError> {
    crate::prepared_plan::validate_time_window(plan, wall_now)?;
    let expires_at = UNIX_EPOCH
        .checked_add(Duration::from_secs(
            crate::prepared_plan::parse_utc_seconds(plan.expires_at())?,
        ))
        .ok_or_else(invalid_contract)?;
    let plan_remaining = expires_at
        .duration_since(wall_now)
        .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::SessionExpired))?;
    if plan_remaining.is_zero() {
        return Err(WorkspaceError::new(WorkspaceErrorCode::SessionExpired));
    }
    let plan_deadline = monotonic_now
        .checked_add(plan_remaining)
        .ok_or_else(limit_exceeded)?;
    if plan_deadline <= operation_deadline {
        Ok((plan_deadline, WorkspaceErrorCode::SessionExpired))
    } else {
        Ok((operation_deadline, WorkspaceErrorCode::LimitExceeded))
    }
}

fn check_effective(
    deadline: Instant,
    expiry_code: WorkspaceErrorCode,
    cancellation: &CancellationToken,
) -> Result<(), WorkspaceError> {
    if cancellation.is_cancelled() {
        Err(cancelled())
    } else if Instant::now() >= deadline {
        Err(WorkspaceError::new(expiry_code))
    } else {
        Ok(())
    }
}

fn validate_uuid(value: &str) -> Result<(), WorkspaceError> {
    let bytes = value.as_bytes();
    if bytes.len() != 36
        || [8, 13, 18, 23].iter().any(|index| bytes[*index] != b'-')
        || bytes[14] != b'4'
        || !matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
        || bytes.iter().enumerate().any(|(index, byte)| {
            ![8, 13, 18, 23].contains(&index)
                && !(byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        })
    {
        return Err(invalid_contract());
    }
    Ok(())
}

fn absolute_output(path: &Path) -> Result<PathBuf, WorkspaceError> {
    if !path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
    {
        return Err(invalid_contract());
    }
    Ok(path.to_path_buf())
}

fn validate_digest(value: &str) -> Result<(), WorkspaceError> {
    if value.len() != 64
        || value
            .as_bytes()
            .iter()
            .any(|byte| !matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(invalid_contract());
    }
    Ok(())
}

fn clear_progress(connection: &Connection) {
    let _ = connection.progress_handler(0, None::<fn() -> bool>);
}

fn lower_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

const fn invalid_contract() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::InvalidContract)
}

const fn limit_exceeded() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::LimitExceeded)
}

const fn row_precondition_failed() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::RowPreconditionFailed)
}

#[cfg(test)]
mod tests {
    use std::{fs, process::Command};

    use ed25519_dalek::SigningKey;
    use rusqlite::Connection;
    use sqlite_capsule_crypto::{PROFILE_V03, application_digest, sign_digest_for_profile};

    use super::*;

    const DEVELOPMENT_SEED: &str =
        include_str!("../../../../compatibility/signed-app-v0.2/development-seed.hex");

    #[test]
    fn all_allowlisted_operations_are_bound_in_canonical_order_and_read_only() {
        let (_source_directory, source_path) = crate::tests::signed_fixture("reconcile-source");
        let (_target_directory, target_path) = crate::tests::signed_fixture("reconcile-target");
        make_content_reconcilable(&source_path, "manual", false);
        make_content_reconcilable(&target_path, "manual", false);
        make_table_ignored(&source_path, "payload");
        make_table_ignored(&target_path, "payload");
        let source_connection = Connection::open(&source_path).unwrap();
        source_connection
            .execute_batch(
                "INSERT INTO vector_domain VALUES ('insert','source insert',1.0,X'01');
                 INSERT INTO vector_domain VALUES ('replace','source replace',2.0,X'02');
                 INSERT INTO vector_domain VALUES ('fields','source fields',3.0,X'03');",
            )
            .unwrap();
        drop(source_connection);
        let target_connection = Connection::open(&target_path).unwrap();
        target_connection
            .execute_batch(
                "INSERT INTO vector_domain VALUES ('delete','target delete',4.0,X'04');
                 INSERT INTO vector_domain VALUES ('replace','target replace',2.0,X'02');
                 INSERT INTO vector_domain VALUES ('fields','target fields',3.0,X'03');",
            )
            .unwrap();
        drop(target_connection);
        let source_before = fs::read(&source_path).unwrap();
        let target_before = fs::read(&target_path).unwrap();
        let source = VerifiedWorkspaceSource::open(&source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&target_path).unwrap();
        let summary = compare(&source, &target);
        let page = detail(&source, &target, 0, 0, false);
        let inserted = page
            .rows
            .iter()
            .find(|row| row.kind == crate::CompareDetailRowKind::Removed)
            .unwrap();
        let deleted = page
            .rows
            .iter()
            .find(|row| row.kind == crate::CompareDetailRowKind::Added)
            .unwrap();
        let changed = page
            .rows
            .iter()
            .filter(|row| row.kind == crate::CompareDetailRowKind::Changed)
            .collect::<Vec<_>>();
        assert!(changed.len() >= 2);
        let selections = vec![
            ReconcileSelection {
                dataset_index: 0,
                table_index: 0,
                key_digest: inserted.key_digest.clone(),
                source_row_digest: inserted.left_digest.clone(),
                target_row_digest: None,
                action: ReconcileAction::InsertFromSource,
                field_indices: Vec::new(),
            },
            ReconcileSelection {
                dataset_index: 0,
                table_index: 0,
                key_digest: deleted.key_digest.clone(),
                source_row_digest: None,
                target_row_digest: deleted.right_digest.clone(),
                action: ReconcileAction::DeleteFromTarget,
                field_indices: Vec::new(),
            },
            ReconcileSelection {
                dataset_index: 0,
                table_index: 0,
                key_digest: changed[0].key_digest.clone(),
                source_row_digest: changed[0].left_digest.clone(),
                target_row_digest: changed[0].right_digest.clone(),
                action: ReconcileAction::ReplaceRowFromSource,
                field_indices: Vec::new(),
            },
            ReconcileSelection {
                dataset_index: 0,
                table_index: 0,
                key_digest: changed[1].key_digest.clone(),
                source_row_digest: changed[1].left_digest.clone(),
                target_row_digest: changed[1].right_digest.clone(),
                action: ReconcileAction::SetFields,
                field_indices: vec![1],
            },
        ];
        let output = output(&target);
        let first = prepare_reconcile_review(
            reopen(&source),
            reopen(&target),
            &summary,
            &selections,
            &BTreeSet::new(),
            &output,
            &ReconcileReviewLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        let second = prepare_reconcile_review(
            reopen(&source),
            reopen(&target),
            &summary,
            &selections,
            &BTreeSet::new(),
            &output,
            &ReconcileReviewLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        assert_eq!(first.operations(), second.operations());
        assert_ne!(first.review_digest(), second.review_digest());
        assert_ne!(first.output().revision_id, second.output().revision_id);
        assert_ne!(
            first.output().lineage_event_id,
            second.output().lineage_event_id
        );
        assert_eq!(first.operation_count(), 4);
        assert_eq!(first.source().signature_count, 1);
        assert_eq!(first.target().signature_count, 1);
        assert_eq!(first.source().signature_inventory_digest.len(), 64);
        assert_eq!(
            first.target().application_digest,
            summary.right.application_digest
        );
        let field_review = first
            .operations()
            .iter()
            .find(|operation| operation.action == ReconcileAction::SetFields)
            .unwrap();
        assert_eq!(field_review.fields.len(), 1);
        assert_eq!(field_review.fields[0].column, "note");
        let insert_private = first
            .operations
            .iter()
            .find(|operation| operation.action == ReconcileAction::InsertFromSource)
            .unwrap();
        let delete_private = first
            .operations
            .iter()
            .find(|operation| operation.action == ReconcileAction::DeleteFromTarget)
            .unwrap();
        let replace_private = first
            .operations
            .iter()
            .find(|operation| operation.action == ReconcileAction::ReplaceRowFromSource)
            .unwrap();
        assert!(
            insert_private
                .source_values
                .as_ref()
                .unwrap()
                .iter()
                .any(|(column, _)| column == "payload")
        );
        assert_eq!(insert_private.source_values.as_ref().unwrap().len(), 4);
        assert_eq!(delete_private.target_values.as_ref().unwrap().len(), 4);
        assert_eq!(
            replace_private
                .source_values
                .as_ref()
                .unwrap()
                .iter()
                .map(|(column, _)| column.as_str())
                .collect::<Vec<_>>(),
            vec!["note"]
        );
        assert_eq!(first.output().capsule_id, target.identity().capsule_id);
        assert!(first.output().preserves_target_signature_inventory);
        assert_eq!(
            first.output().signature_inventory_digest,
            first.target().signature_inventory_digest
        );
        assert_eq!(first.output().lineage_parents.len(), 2);
        assert_eq!(
            first.output().lineage_parents[0].relation,
            ReconcileLineageRelation::TargetDerivedFrom
        );
        assert_eq!(
            first.output().lineage_parents[1].relation,
            ReconcileLineageRelation::ChangesAppliedFrom
        );
        let debug = format!("{first:?}");
        assert!(!debug.contains("source insert"));
        assert!(!debug.contains("target fields"));
        let mut wrong_payload = first.payload().to_vec();
        wrong_payload.push(b' ');
        let same_plan = first.plan().clone();
        assert_eq!(
            first
                .prepare_at(same_plan, &wrong_payload, approved_time())
                .unwrap_err()
                .kind(),
            WorkspaceErrorCode::StalePlan
        );
        let first = prepare_reconcile_review(
            reopen(&source),
            reopen(&target),
            &summary,
            &selections,
            &BTreeSet::new(),
            &output,
            &ReconcileReviewLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        let approved_plan = first.plan().clone();
        let approved_payload = first.payload().to_vec();
        let prepared = first
            .prepare_at(approved_plan, &approved_payload, approved_time())
            .unwrap();
        assert_eq!(prepared.operation_count(), 4);
        let second_plan = second.plan().clone();
        let second_payload = second.payload().to_vec();
        second
            .prepare_at(second_plan, &second_payload, approved_time())
            .unwrap()
            .stage()
            .unwrap()
            .transform_and_validate()
            .unwrap();
        let cancellation = CancellationToken::new();
        let cancelled_review = prepare_reconcile_review(
            reopen(&source),
            reopen(&target),
            &summary,
            &selections,
            &BTreeSet::new(),
            &output,
            &ReconcileReviewLimits::default(),
            &cancellation,
        )
        .unwrap();
        cancellation.cancel();
        let approved_plan = cancelled_review.plan().clone();
        let approved_payload = cancelled_review.payload().to_vec();
        assert_eq!(
            cancelled_review
                .prepare_at(approved_plan, &approved_payload, approved_time())
                .unwrap_err()
                .kind(),
            WorkspaceErrorCode::Cancelled
        );
        assert_eq!(fs::read(&source_path).unwrap(), source_before);
        assert_eq!(fs::read(&target_path).unwrap(), target_before);
    }

    #[test]
    fn shuffled_selections_produce_identical_canonical_operation_order() {
        let (_source_directory, source_path) =
            crate::tests::signed_fixture("reconcile-order-source");
        let (_target_directory, target_path) =
            crate::tests::signed_fixture("reconcile-order-target");
        make_content_reconcilable(&source_path, "manual", false);
        make_content_reconcilable(&target_path, "manual", false);
        Connection::open(&source_path).unwrap().execute_batch(
            "INSERT INTO vector_domain VALUES ('z','source z',1.0,X'01'); INSERT INTO vector_domain VALUES ('a','source a',2.0,X'02');"
        ).unwrap();
        let source = VerifiedWorkspaceSource::open(&source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&target_path).unwrap();
        let summary = compare(&source, &target);
        let page = detail(&source, &target, 0, 0, false);
        let mut selections = page
            .rows
            .iter()
            .filter(|row| row.kind == crate::CompareDetailRowKind::Removed)
            .map(|row| ReconcileSelection {
                dataset_index: 0,
                table_index: 0,
                key_digest: row.key_digest.clone(),
                source_row_digest: row.left_digest.clone(),
                target_row_digest: None,
                action: ReconcileAction::InsertFromSource,
                field_indices: Vec::new(),
            })
            .collect::<Vec<_>>();
        assert_eq!(selections.len(), 2);
        let forward = prepare(&source, &target, &summary, &selections, false).unwrap();
        selections.reverse();
        let reverse = prepare(&source, &target, &summary, &selections, false).unwrap();
        assert_eq!(forward.operations(), reverse.operations());
        let forward_payload: JsonValue = serde_json::from_slice(forward.payload()).unwrap();
        let reverse_payload: JsonValue = serde_json::from_slice(reverse.payload()).unwrap();
        assert_eq!(forward_payload["operations"], reverse_payload["operations"]);
        assert_ne!(forward.output().revision_id, reverse.output().revision_id);
        assert_ne!(
            forward.output().lineage_event_id,
            reverse.output().lineage_event_id
        );
    }

    #[test]
    fn serialized_plan_expiry_never_outlives_the_review_authority() {
        let (_source_directory, source_path) =
            crate::tests::signed_fixture("reconcile-expiry-source");
        let (_target_directory, target_path) =
            crate::tests::signed_fixture("reconcile-expiry-target");
        make_content_reconcilable(&source_path, "manual", false);
        make_content_reconcilable(&target_path, "manual", false);
        Connection::open(&source_path)
            .unwrap()
            .execute("UPDATE vector_domain SET note='expiring source'", [])
            .unwrap();
        let source = VerifiedWorkspaceSource::open(&source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&target_path).unwrap();
        let summary = compare(&source, &target);
        let row = detail(&source, &target, 0, 0, false)
            .rows
            .into_iter()
            .find(|row| row.kind == crate::CompareDetailRowKind::Changed)
            .unwrap();
        let selection = ReconcileSelection {
            dataset_index: 0,
            table_index: 0,
            key_digest: row.key_digest,
            source_row_digest: row.left_digest,
            target_row_digest: row.right_digest,
            action: ReconcileAction::SetFields,
            field_indices: vec![1],
        };

        let mut expired_request = output(&target);
        expired_request.expires_at = "2026-08-13T08:01:00Z".to_owned();
        let review = prepare_reconcile_review(
            reopen(&source),
            reopen(&target),
            &summary,
            std::slice::from_ref(&selection),
            &BTreeSet::new(),
            &expired_request,
            &ReconcileReviewLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        let plan = review.plan().clone();
        let payload = review.payload().to_vec();
        assert_eq!(
            review
                .prepare_at(plan, &payload, approved_time())
                .unwrap_err()
                .kind(),
            WorkspaceErrorCode::SessionExpired
        );
        assert!(!expired_request.output_path.exists());

        let mut short_request = output(&target);
        short_request.expires_at = "2026-08-13T08:01:01Z".to_owned();
        let review = prepare_reconcile_review(
            reopen(&source),
            reopen(&target),
            &summary,
            std::slice::from_ref(&selection),
            &BTreeSet::new(),
            &short_request,
            &ReconcileReviewLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        let plan = review.plan().clone();
        let payload = review.payload().to_vec();
        let mut prepared = review
            .prepare_at_with_clock(plan, &payload, approved_time(), Instant::now())
            .unwrap();
        assert_eq!(prepared.expiry_code, WorkspaceErrorCode::SessionExpired);
        // Deterministically inject the handoff crossing the already-derived
        // serialized-plan deadline. No destination staging may occur.
        prepared.expires_at = Instant::now();
        let error = match prepared.stage() {
            Ok(_) => panic!("expired serialized plan must not stage"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::SessionExpired);
        assert!(!short_request.output_path.exists());

        let long_request = output(&target);
        let review = prepare_reconcile_review(
            reopen(&source),
            reopen(&target),
            &summary,
            &[selection],
            &BTreeSet::new(),
            &long_request,
            &ReconcileReviewLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        let plan = review.plan().clone();
        let payload = review.payload().to_vec();
        let mut prepared = review
            .prepare_at_with_clock(plan, &payload, approved_time(), Instant::now())
            .unwrap();
        assert_eq!(prepared.expiry_code, WorkspaceErrorCode::LimitExceeded);
        prepared.expires_at = Instant::now();
        let error = match prepared.stage() {
            Ok(_) => panic!("expired operation budget must not stage"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::LimitExceeded);
        assert!(!long_request.output_path.exists());
    }

    #[test]
    fn each_review_mints_fresh_distinct_uuid_v4_authority() {
        let (directory, source_path) = crate::tests::signed_fixture("reconcile-fresh-id-source");
        let (_target_directory, target_path) =
            crate::tests::signed_fixture("reconcile-fresh-id-target");
        make_content_reconcilable(&source_path, "manual", false);
        make_content_reconcilable(&target_path, "manual", false);
        Connection::open(&source_path)
            .unwrap()
            .execute_batch(
                "INSERT INTO vector_domain VALUES ('first','source first',1.0,X'01');
                 INSERT INTO vector_domain VALUES ('second','source second',2.0,X'02');",
            )
            .unwrap();
        let source = VerifiedWorkspaceSource::open(&source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&target_path).unwrap();
        let summary = compare(&source, &target);
        let rows = detail(&source, &target, 0, 0, false)
            .rows
            .into_iter()
            .filter(|row| row.kind == crate::CompareDetailRowKind::Removed)
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 2);
        let selection = |row: &crate::CompareRowDetail| ReconcileSelection {
            dataset_index: 0,
            table_index: 0,
            key_digest: row.key_digest.clone(),
            source_row_digest: row.left_digest.clone(),
            target_row_digest: None,
            action: ReconcileAction::InsertFromSource,
            field_indices: Vec::new(),
        };
        let mut first_request = output(&target);
        first_request.output_path = directory.path().join("fresh-id-first.sqlitecapsule");
        let mut second_request = output(&target);
        second_request.output_path = directory.path().join("fresh-id-second.sqlitecapsule");
        let first = prepare_reconcile_review(
            reopen(&source),
            reopen(&target),
            &summary,
            &[selection(&rows[0])],
            &BTreeSet::new(),
            &first_request,
            &ReconcileReviewLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        let second = prepare_reconcile_review(
            reopen(&source),
            reopen(&target),
            &summary,
            &[selection(&rows[1])],
            &BTreeSet::new(),
            &second_request,
            &ReconcileReviewLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        let source_revision = source
            .identity()
            .overview
            .instance
            .revision_id
            .as_deref()
            .unwrap();
        let target_revision = target
            .identity()
            .overview
            .instance
            .revision_id
            .as_deref()
            .unwrap();
        let minted = [
            first.output().revision_id.as_str(),
            first.output().lineage_event_id.as_str(),
            second.output().revision_id.as_str(),
            second.output().lineage_event_id.as_str(),
        ];
        assert_eq!(minted.iter().copied().collect::<BTreeSet<_>>().len(), 4);
        for value in minted {
            validate_uuid(value).unwrap();
            assert_ne!(value, source_revision);
            assert_ne!(value, target_revision);
        }
    }

    #[test]
    fn executable_two_way_review_publishes_target_derived_copy() {
        let (_source_directory, source_path) =
            crate::tests::signed_fixture("reconcile-execute-source");
        let (_target_directory, target_path) =
            crate::tests::signed_fixture("reconcile-execute-target");
        make_content_reconcilable(&source_path, "manual", false);
        make_content_reconcilable(&target_path, "manual", false);
        Connection::open(&source_path)
            .unwrap()
            .execute(
                "UPDATE vector_domain SET note='executed source' WHERE id='domain'",
                [],
            )
            .unwrap();
        let source_before = fs::read(&source_path).unwrap();
        let target_before = fs::read(&target_path).unwrap();
        let source = VerifiedWorkspaceSource::open(&source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&target_path).unwrap();
        let summary = compare(&source, &target);
        let row = detail(&source, &target, 0, 0, false)
            .rows
            .into_iter()
            .find(|row| row.kind == crate::CompareDetailRowKind::Changed)
            .unwrap();
        let selection = ReconcileSelection {
            dataset_index: 0,
            table_index: 0,
            key_digest: row.key_digest,
            source_row_digest: row.left_digest,
            target_row_digest: row.right_digest,
            action: ReconcileAction::SetFields,
            field_indices: vec![1],
        };
        let review = prepare(&source, &target, &summary, &[selection], false).unwrap();
        let plan = review.plan().clone();
        let payload = review.payload().to_vec();
        let published = review
            .prepare_at(plan, &payload, approved_time())
            .unwrap()
            .stage()
            .unwrap()
            .transform_and_validate()
            .unwrap()
            .publish()
            .unwrap();
        let output = VerifiedWorkspaceSource::open(published.path()).unwrap();
        let note: String = output
            .verified
            .connection()
            .query_row(
                "SELECT note FROM vector_domain WHERE id='domain'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(note, "executed source");
        assert_eq!(output.identity().capsule_id, target.identity().capsule_id);
        assert_eq!(output.application_digest(), target.application_digest());
        assert_eq!(
            output.lineage().events.len(),
            target.lineage().events.len() + 1
        );
        assert_eq!(fs::read(&source_path).unwrap(), source_before);
        assert_eq!(fs::read(&target_path).unwrap(), target_before);
    }

    #[test]
    fn three_way_clean_field_merge_preserves_independent_target_change() {
        let (_ancestor_directory, ancestor_path) =
            crate::tests::signed_fixture("reconcile-three-clean-ancestor");
        let (_source_directory, source_path) =
            crate::tests::signed_fixture("reconcile-three-clean-source");
        let (_target_directory, target_path) =
            crate::tests::signed_fixture("reconcile-three-clean-target");
        for path in [&ancestor_path, &source_path, &target_path] {
            make_three_way_reconcilable(path, false);
        }
        Connection::open(&source_path)
            .unwrap()
            .execute(
                "UPDATE vector_domain SET note='source-clean' WHERE id='domain'",
                [],
            )
            .unwrap();
        Connection::open(&target_path)
            .unwrap()
            .execute(
                "UPDATE vector_domain SET measurement=7.0 WHERE id='domain'",
                [],
            )
            .unwrap();
        let ancestor_before = fs::read(&ancestor_path).unwrap();
        let source_before = fs::read(&source_path).unwrap();
        let target_before = fs::read(&target_path).unwrap();
        let ancestor = VerifiedWorkspaceSource::open(&ancestor_path).unwrap();
        let source = VerifiedWorkspaceSource::open(&source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&target_path).unwrap();
        let summary = compare(&source, &target);
        let classified = classify_three_way_reconcile(
            ancestor,
            reopen(&source),
            reopen(&target),
            &summary,
            &BTreeSet::new(),
            &ReconcileReviewLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        assert_eq!(classified.clean_change_count(), 1);
        assert_eq!(classified.conflicts().len(), 0);
        let review = classified
            .resolve(&[], &output(&target), HARD_DEADLINE)
            .unwrap();
        assert_eq!(review.plan().inputs().len(), 3);
        assert_eq!(review.plan().inputs()[2].role(), InputRole::Ancestor);
        assert!(review.ancestor().is_some());
        let payload: JsonValue = serde_json::from_slice(review.payload()).unwrap();
        assert_eq!(payload["mode"], "three-way");
        assert!(payload["lineage"]["details"]["ancestor_evidence"].is_object());
        assert_eq!(payload["lineage"]["parents"].as_array().unwrap().len(), 2);
        let published = execute(review);
        let connection = Connection::open(published.path()).unwrap();
        let (note, measurement): (String, f64) = connection
            .query_row(
                "SELECT note,measurement FROM vector_domain WHERE id='domain'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(note, "source-clean");
        assert_eq!(measurement, 7.0);
        assert_eq!(fs::read(&ancestor_path).unwrap(), ancestor_before);
        assert_eq!(fs::read(&source_path).unwrap(), source_before);
        assert_eq!(fs::read(&target_path).unwrap(), target_before);
    }

    #[test]
    fn three_way_clean_row_existence_changes_apply_exactly() {
        let (_ancestor_directory, ancestor_path) =
            crate::tests::signed_fixture("reconcile-three-rows-ancestor");
        let (_source_directory, source_path) =
            crate::tests::signed_fixture("reconcile-three-rows-source");
        let (_target_directory, target_path) =
            crate::tests::signed_fixture("reconcile-three-rows-target");
        for path in [&ancestor_path, &source_path, &target_path] {
            Connection::open(path)
                .unwrap()
                .execute(
                    "INSERT INTO vector_domain VALUES ('delete-clean','base',1.0,X'01')",
                    [],
                )
                .unwrap();
            make_three_way_reconcilable(path, false);
        }
        Connection::open(&source_path)
            .unwrap()
            .execute_batch(
                "DELETE FROM vector_domain WHERE id='delete-clean';
                 INSERT INTO vector_domain VALUES ('insert-clean','source',2.0,X'02');",
            )
            .unwrap();
        let source = VerifiedWorkspaceSource::open(&source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&target_path).unwrap();
        let summary = compare(&source, &target);
        let classified = classify_three_way_reconcile(
            VerifiedWorkspaceSource::open(&ancestor_path).unwrap(),
            reopen(&source),
            reopen(&target),
            &summary,
            &BTreeSet::new(),
            &ReconcileReviewLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        assert_eq!(classified.clean_change_count(), 2);
        assert_eq!(classified.conflicts().len(), 0);
        let published = execute(
            classified
                .resolve(&[], &output(&target), HARD_DEADLINE)
                .unwrap(),
        );
        let connection = Connection::open(published.path()).unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT count(*) FROM vector_domain WHERE id='delete-clean'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT note FROM vector_domain WHERE id='insert-clean'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "source"
        );
    }

    #[test]
    fn three_way_conflict_matrix_resolves_only_keep_target_or_take_source() {
        let (_ancestor_directory, ancestor_path) =
            crate::tests::signed_fixture("reconcile-three-conflict-ancestor");
        let (_source_directory, source_path) =
            crate::tests::signed_fixture("reconcile-three-conflict-source");
        let (_target_directory, target_path) =
            crate::tests::signed_fixture("reconcile-three-conflict-target");
        for path in [&ancestor_path, &source_path, &target_path] {
            let connection = Connection::open(path).unwrap();
            connection
                .execute_batch(
                    "INSERT INTO vector_domain VALUES ('uc','same',0.0,X'01');
                     INSERT INTO vector_domain VALUES ('ds','same',0.0,X'02');
                     INSERT INTO vector_domain VALUES ('dt','same',0.0,X'03');
                     INSERT INTO vector_domain VALUES ('im','base immutable',0.0,X'04');",
                )
                .unwrap();
            drop(connection);
            make_three_way_reconcilable(path, true);
        }
        Connection::open(&source_path)
            .unwrap()
            .execute_batch(
                "UPDATE vector_domain SET measurement=1.0 WHERE id='uc';
                 DELETE FROM vector_domain WHERE id='ds';
                 UPDATE vector_domain SET measurement=3.0 WHERE id='dt';
                 UPDATE vector_domain SET note='source immutable' WHERE id='im';
                 INSERT INTO vector_domain VALUES ('ii','same',4.0,X'05');",
            )
            .unwrap();
        Connection::open(&target_path)
            .unwrap()
            .execute_batch(
                "UPDATE vector_domain SET measurement=2.0 WHERE id='uc';
                 UPDATE vector_domain SET measurement=2.0 WHERE id='ds';
                 DELETE FROM vector_domain WHERE id='dt';
                 INSERT INTO vector_domain VALUES ('ii','same',5.0,X'05');",
            )
            .unwrap();
        let source = VerifiedWorkspaceSource::open(&source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&target_path).unwrap();
        let summary = compare(&source, &target);
        let classify = || {
            classify_three_way_reconcile(
                VerifiedWorkspaceSource::open(&ancestor_path).unwrap(),
                reopen(&source),
                reopen(&target),
                &summary,
                &BTreeSet::new(),
                &ReconcileReviewLimits::default(),
                &CancellationToken::new(),
            )
            .unwrap()
        };
        let classified = classify();
        assert_eq!(classified.clean_change_count(), 0);
        let conflicts = classified.conflicts().cloned().collect::<Vec<_>>();
        assert_eq!(conflicts.len(), 5);
        assert_eq!(
            conflicts
                .iter()
                .filter(|conflict| conflict.kind == ThreeWayConflictKind::DeleteUpdate)
                .count(),
            2
        );
        assert!(conflicts.iter().any(|conflict| {
            conflict.kind == ThreeWayConflictKind::DeleteUpdate
                && conflict.deleted_side == Some(ThreeWayDeletedSide::Source)
        }));
        assert!(conflicts.iter().any(|conflict| {
            conflict.kind == ThreeWayConflictKind::DeleteUpdate
                && conflict.deleted_side == Some(ThreeWayDeletedSide::Target)
        }));
        assert!(
            conflicts
                .iter()
                .any(|conflict| conflict.kind == ThreeWayConflictKind::InsertInsert)
        );
        let immutable = conflicts
            .iter()
            .find(|conflict| conflict.kind == ThreeWayConflictKind::ImmutableField)
            .unwrap();
        assert_eq!(
            immutable.allowed_choices,
            vec![ThreeWayResolutionChoice::KeepTarget]
        );
        assert_eq!(
            classify()
                .resolve(
                    &conflicts
                        .iter()
                        .map(|conflict| ThreeWayConflictResolution {
                            conflict_id: conflict.id.clone(),
                            choice: ThreeWayResolutionChoice::TakeSource,
                        })
                        .collect::<Vec<_>>(),
                    &output(&target),
                    HARD_DEADLINE,
                )
                .unwrap_err()
                .kind(),
            WorkspaceErrorCode::ImmutableColumn
        );
        let resolutions = conflicts
            .iter()
            .map(|conflict| ThreeWayConflictResolution {
                conflict_id: conflict.id.clone(),
                choice: if conflict.kind == ThreeWayConflictKind::ImmutableField
                    || (conflict.kind == ThreeWayConflictKind::DeleteUpdate
                        && conflict.deleted_side == Some(ThreeWayDeletedSide::Source))
                {
                    ThreeWayResolutionChoice::KeepTarget
                } else {
                    ThreeWayResolutionChoice::TakeSource
                },
            })
            .collect::<Vec<_>>();
        let review = classified
            .resolve(&resolutions, &output(&target), HARD_DEADLINE)
            .unwrap();
        assert_eq!(review.resolved_conflicts().len(), 5);
        assert_eq!(review.operation_count(), 3);
        let payload: JsonValue = serde_json::from_slice(review.payload()).unwrap();
        assert_eq!(payload["resolved_conflicts"].as_array().unwrap().len(), 5);
        assert!(
            payload["operations"]
                .as_array()
                .unwrap()
                .iter()
                .all(|operation| {
                    operation["basis"] == "conflict-resolution"
                        && operation["conflict_id"].is_string()
                })
        );
        let published = execute(review);
        let connection = Connection::open(published.path()).unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT measurement FROM vector_domain WHERE id='uc'",
                    [],
                    |row| row.get::<_, f64>(0),
                )
                .unwrap(),
            1.0
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT measurement FROM vector_domain WHERE id='ds'",
                    [],
                    |row| row.get::<_, f64>(0),
                )
                .unwrap(),
            2.0
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT measurement FROM vector_domain WHERE id='dt'",
                    [],
                    |row| row.get::<_, f64>(0),
                )
                .unwrap(),
            3.0
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT measurement FROM vector_domain WHERE id='ii'",
                    [],
                    |row| row.get::<_, f64>(0),
                )
                .unwrap(),
            4.0
        );
        assert_eq!(
            connection
                .query_row("SELECT note FROM vector_domain WHERE id='im'", [], |row| {
                    row.get::<_, String>(0)
                },)
                .unwrap(),
            "base immutable"
        );
    }

    #[test]
    fn three_way_unresolved_forged_stale_and_incompatible_ancestor_fail_closed() {
        let (_ancestor_directory, ancestor_path) =
            crate::tests::signed_fixture("reconcile-three-hostile-ancestor");
        let (_source_directory, source_path) =
            crate::tests::signed_fixture("reconcile-three-hostile-source");
        let (_target_directory, target_path) =
            crate::tests::signed_fixture("reconcile-three-hostile-target");
        for path in [&ancestor_path, &source_path, &target_path] {
            make_three_way_reconcilable(path, false);
        }
        Connection::open(&source_path)
            .unwrap()
            .execute("UPDATE vector_domain SET note='source conflict'", [])
            .unwrap();
        Connection::open(&target_path)
            .unwrap()
            .execute("UPDATE vector_domain SET note='target conflict'", [])
            .unwrap();
        let source = VerifiedWorkspaceSource::open(&source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&target_path).unwrap();
        let summary = compare(&source, &target);
        let classify = || {
            classify_three_way_reconcile(
                VerifiedWorkspaceSource::open(&ancestor_path).unwrap(),
                reopen(&source),
                reopen(&target),
                &summary,
                &BTreeSet::new(),
                &ReconcileReviewLimits::default(),
                &CancellationToken::new(),
            )
            .unwrap()
        };
        assert_eq!(
            assert_reconcile_authorities_current(
                &source,
                &target,
                Some(&VerifiedWorkspaceSource::open(&ancestor_path).unwrap()),
                Instant::now(),
                &CancellationToken::new(),
            )
            .unwrap_err()
            .kind(),
            WorkspaceErrorCode::LimitExceeded
        );
        assert_eq!(
            classify()
                .resolve(&[], &output(&target), Duration::ZERO)
                .unwrap_err()
                .kind(),
            WorkspaceErrorCode::LimitExceeded
        );
        let mut expired = classify();
        let conflict = expired.conflicts().next().unwrap().clone();
        let remaining_review = expired.remaining_lifetime().unwrap();
        assert!(remaining_review > HARD_DEADLINE);
        assert!(remaining_review <= HARD_REVIEW_LIFETIME);
        expired.expires_at = Instant::now();
        assert_eq!(
            expired.remaining_lifetime().unwrap_err().kind(),
            WorkspaceErrorCode::SessionExpired
        );

        let now = Instant::now();
        let authority = now + Duration::from_secs(90);
        let (work_deadline, expiry_code) =
            bounded_authority_work_deadline(authority, HARD_DEADLINE, now).unwrap();
        assert_eq!(work_deadline, now + HARD_DEADLINE);
        assert_eq!(expiry_code, WorkspaceErrorCode::LimitExceeded);
        let authority = now + Duration::from_secs(5);
        let (work_deadline, expiry_code) =
            bounded_authority_work_deadline(authority, HARD_DEADLINE, now).unwrap();
        assert_eq!(work_deadline, authority);
        assert_eq!(expiry_code, WorkspaceErrorCode::SessionExpired);
        assert_eq!(
            bounded_authority_work_deadline(now, HARD_DEADLINE, now)
                .unwrap_err()
                .kind(),
            WorkspaceErrorCode::SessionExpired
        );
        assert_eq!(
            expired
                .resolve(
                    &[ThreeWayConflictResolution {
                        conflict_id: conflict.id,
                        choice: ThreeWayResolutionChoice::KeepTarget,
                    }],
                    &output(&target),
                    HARD_DEADLINE,
                )
                .unwrap_err()
                .kind(),
            WorkspaceErrorCode::SessionExpired
        );
        assert_eq!(
            classify()
                .resolve(&[], &output(&target), HARD_DEADLINE)
                .unwrap_err()
                .kind(),
            WorkspaceErrorCode::ConflictsUnresolved
        );
        assert_eq!(
            classify()
                .resolve(
                    &[ThreeWayConflictResolution {
                        conflict_id: "0".repeat(64),
                        choice: ThreeWayResolutionChoice::KeepTarget,
                    }],
                    &output(&target),
                    HARD_DEADLINE,
                )
                .unwrap_err()
                .kind(),
            WorkspaceErrorCode::ConflictsUnresolved
        );
        let stale = classify();
        Connection::open(&ancestor_path)
            .unwrap()
            .execute("UPDATE capsule_instance SET title='stale ancestor'", [])
            .unwrap();
        let conflict = stale.conflicts().next().unwrap().clone();
        assert_eq!(
            stale
                .resolve(
                    &[ThreeWayConflictResolution {
                        conflict_id: conflict.id,
                        choice: ThreeWayResolutionChoice::KeepTarget,
                    }],
                    &output(&target),
                    HARD_DEADLINE,
                )
                .unwrap_err()
                .kind(),
            WorkspaceErrorCode::StalePlan
        );

        let (_bad_directory, bad_path) =
            crate::tests::signed_fixture("reconcile-three-incompatible-ancestor");
        make_content_reconcilable(&bad_path, "manual", false);
        assert_eq!(
            classify_three_way_reconcile(
                VerifiedWorkspaceSource::open(&bad_path).unwrap(),
                reopen(&source),
                reopen(&target),
                &summary,
                &BTreeSet::new(),
                &ReconcileReviewLimits::default(),
                &CancellationToken::new(),
            )
            .unwrap_err()
            .kind(),
            WorkspaceErrorCode::IncompatibleSchema
        );
    }

    #[test]
    fn three_way_prepared_authority_rebinds_ancestor_before_staging() {
        let (_ancestor_directory, ancestor_path) =
            crate::tests::signed_fixture("reconcile-three-rebind-ancestor");
        let (_source_directory, source_path) =
            crate::tests::signed_fixture("reconcile-three-rebind-source");
        let (_target_directory, target_path) =
            crate::tests::signed_fixture("reconcile-three-rebind-target");
        for path in [&ancestor_path, &source_path, &target_path] {
            make_three_way_reconcilable(path, false);
        }
        Connection::open(&source_path)
            .unwrap()
            .execute("UPDATE vector_domain SET note='clean source'", [])
            .unwrap();
        let source = VerifiedWorkspaceSource::open(&source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&target_path).unwrap();
        let summary = compare(&source, &target);
        let review = classify_three_way_reconcile(
            VerifiedWorkspaceSource::open(&ancestor_path).unwrap(),
            reopen(&source),
            reopen(&target),
            &summary,
            &BTreeSet::new(),
            &ReconcileReviewLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap()
        .resolve(&[], &output(&target), HARD_DEADLINE)
        .unwrap();
        let plan = review.plan().clone();
        let payload = review.payload().to_vec();
        let prepared = review.prepare_at(plan, &payload, approved_time()).unwrap();
        Connection::open(&ancestor_path)
            .unwrap()
            .execute(
                "UPDATE capsule_instance SET title='late ancestor mutation'",
                [],
            )
            .unwrap();
        let error = match prepared.stage() {
            Ok(_) => panic!("stale ancestor must block staging"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::StalePlan);
        assert!(!output(&target).output_path.exists());
    }

    #[test]
    fn three_way_callback_late_ancestor_mutation_is_quarantined() {
        let (_ancestor_directory, ancestor_path) =
            crate::tests::signed_fixture("reconcile-three-callback-ancestor");
        let (_source_directory, source_path) =
            crate::tests::signed_fixture("reconcile-three-callback-source");
        let (_target_directory, target_path) =
            crate::tests::signed_fixture("reconcile-three-callback-target");
        for path in [&ancestor_path, &source_path, &target_path] {
            make_three_way_reconcilable(path, false);
        }
        Connection::open(&source_path)
            .unwrap()
            .execute("UPDATE vector_domain SET note='clean source'", [])
            .unwrap();
        let source = VerifiedWorkspaceSource::open(&source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&target_path).unwrap();
        let summary = compare(&source, &target);
        let request = output(&target);
        let review = classify_three_way_reconcile(
            VerifiedWorkspaceSource::open(&ancestor_path).unwrap(),
            reopen(&source),
            reopen(&target),
            &summary,
            &BTreeSet::new(),
            &ReconcileReviewLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap()
        .resolve(&[], &request, HARD_DEADLINE)
        .unwrap();
        let plan = review.plan().clone();
        let payload = review.payload().to_vec();
        let validated = review
            .prepare_at(plan, &payload, approved_time())
            .unwrap()
            .stage()
            .unwrap()
            .transform_and_validate()
            .unwrap();
        let result = validated.publish_with_hook(|| {
            Connection::open(&ancestor_path)
                .unwrap()
                .execute(
                    "UPDATE capsule_instance SET title='late raced ancestor'",
                    [],
                )
                .unwrap();
        });
        let error = match result {
            Ok(_) => panic!("late ancestor mutation must not report success"),
            Err(error) => error,
        };
        assert_eq!(
            error.kind(),
            WorkspaceErrorCode::PostpublishVerificationFailed
        );
        assert_quarantine_evidence(&request.output_path);
    }

    #[test]
    fn three_way_requires_complete_valid_ancestor_signature_inventory() {
        let (_ancestor_directory, ancestor_path) =
            crate::tests::signed_fixture("reconcile-three-signature-ancestor");
        let (_source_directory, source_path) =
            crate::tests::signed_fixture("reconcile-three-signature-source");
        let (_target_directory, target_path) =
            crate::tests::signed_fixture("reconcile-three-signature-target");
        for path in [&ancestor_path, &source_path, &target_path] {
            make_three_way_reconcilable(path, false);
        }
        Connection::open(&source_path)
            .unwrap()
            .execute("UPDATE vector_domain SET note='source change'", [])
            .unwrap();
        let source = VerifiedWorkspaceSource::open(&source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&target_path).unwrap();
        let summary = compare(&source, &target);
        let connection = Connection::open(&ancestor_path).unwrap();
        connection
            .execute(
                "INSERT INTO capsule_signature VALUES \
                 ('ed25519:sha256:0000000000000000000000000000000000000000000000000000000000000000',\
                  'ed25519',?1,?2,?3,'2026-08-09T00:00:00Z')",
                params![vec![0_u8; 32], vec![0_u8; 32], vec![0_u8; 64]],
            )
            .unwrap();
        drop(connection);
        let ancestor = VerifiedWorkspaceSource::open(&ancestor_path).unwrap();
        assert!(!ancestor.has_complete_valid_signature_inventory());
        assert_eq!(
            classify_three_way_reconcile(
                ancestor,
                reopen(&source),
                reopen(&target),
                &summary,
                &BTreeSet::new(),
                &ReconcileReviewLimits::default(),
                &CancellationToken::new(),
            )
            .unwrap_err()
            .kind(),
            WorkspaceErrorCode::InvalidSignature
        );
    }

    #[test]
    fn three_way_policy_forbidden_change_never_mints_review_authority() {
        let (_ancestor_directory, ancestor_path) =
            crate::tests::signed_fixture("reconcile-three-forbid-ancestor");
        let (_source_directory, source_path) =
            crate::tests::signed_fixture("reconcile-three-forbid-source");
        let (_target_directory, target_path) =
            crate::tests::signed_fixture("reconcile-three-forbid-target");
        for path in [&ancestor_path, &source_path, &target_path] {
            let connection = Connection::open(path).unwrap();
            connection
                .execute_batch(
                    "UPDATE capsule_dataset SET reconcile_policy='forbid' WHERE id='content';
                     UPDATE capsule_dataset SET reconcile_policy='three-way' WHERE id='settings';",
                )
                .unwrap();
            resign(&connection);
        }
        Connection::open(&source_path)
            .unwrap()
            .execute(
                "UPDATE vector_domain SET note='forbidden source change'",
                [],
            )
            .unwrap();
        let source = VerifiedWorkspaceSource::open(&source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&target_path).unwrap();
        let summary = compare(&source, &target);
        assert!(summary.compatibility.can_reconcile);
        assert_eq!(
            classify_three_way_reconcile(
                VerifiedWorkspaceSource::open(&ancestor_path).unwrap(),
                reopen(&source),
                reopen(&target),
                &summary,
                &BTreeSet::new(),
                &ReconcileReviewLimits::default(),
                &CancellationToken::new(),
            )
            .unwrap_err()
            .kind(),
            WorkspaceErrorCode::UnsupportedOperation
        );
    }

    #[test]
    fn unrelated_update_preserves_target_sequence_high_water_mark() {
        let (_source_directory, source_path) =
            crate::tests::signed_fixture("reconcile-sequence-source");
        let (_target_directory, target_path) =
            crate::tests::signed_fixture("reconcile-sequence-target");
        make_content_reconcilable(&source_path, "manual", false);
        make_content_reconcilable(&target_path, "manual", false);
        Connection::open(&source_path)
            .unwrap()
            .execute(
                "UPDATE vector_domain SET note='sequence source' WHERE id='domain'",
                [],
            )
            .unwrap();
        let target_connection = Connection::open(&target_path).unwrap();
        target_connection
            .execute_batch(
                "WITH RECURSIVE n(value) AS (SELECT 1 UNION ALL SELECT value+1 FROM n WHERE value<10)
                 INSERT INTO capsule_change_log(endpoint_name,parameters_json,changed_rows,occurred_at)
                 SELECT 'sequence-test','{}',0,'2026-08-13T08:00:00Z' FROM n;
                 DELETE FROM capsule_change_log;",
            )
            .unwrap();
        let target_high_water = sequence(&target_connection, "capsule_change_log");
        assert!(target_high_water > 10);
        drop(target_connection);
        let source_before = fs::read(&source_path).unwrap();
        let target_before = fs::read(&target_path).unwrap();
        let source = VerifiedWorkspaceSource::open(&source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&target_path).unwrap();
        let summary = compare(&source, &target);
        let row = detail(&source, &target, 0, 0, false)
            .rows
            .into_iter()
            .find(|row| row.kind == crate::CompareDetailRowKind::Changed)
            .unwrap();
        let selection = ReconcileSelection {
            dataset_index: 0,
            table_index: 0,
            key_digest: row.key_digest,
            source_row_digest: row.left_digest,
            target_row_digest: row.right_digest,
            action: ReconcileAction::SetFields,
            field_indices: vec![1],
        };
        let review = prepare(&source, &target, &summary, &[selection], false).unwrap();
        assert!(review.sequence_state.iter().any(|state| {
            state.name == "capsule_change_log" && state.sequence == target_high_water
        }));
        let published = execute(review);
        let output_connection = Connection::open(published.path()).unwrap();
        assert_eq!(
            sequence(&output_connection, "capsule_change_log"),
            target_high_water
        );
        assert_eq!(fs::read(&source_path).unwrap(), source_before);
        assert_eq!(fs::read(&target_path).unwrap(), target_before);
    }

    #[test]
    fn deleting_selected_autoincrement_max_row_does_not_lower_sequence() {
        let (_source_directory, source_path) =
            crate::tests::signed_fixture("reconcile-sequence-delete-source");
        let (_target_directory, target_path) =
            crate::tests::signed_fixture("reconcile-sequence-delete-target");
        for path in [&source_path, &target_path] {
            add_autoincrement_dataset_table(path);
            make_content_reconcilable(path, "manual", false);
        }
        let target_connection = Connection::open(&target_path).unwrap();
        target_connection
            .execute(
                "INSERT INTO auto_domain(id,note) VALUES (10,'delete me')",
                [],
            )
            .unwrap();
        assert_eq!(sequence(&target_connection, "auto_domain"), 10);
        drop(target_connection);
        let source = VerifiedWorkspaceSource::open(&source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&target_path).unwrap();
        let summary = compare(&source, &target);
        let row = detail(&source, &target, 0, 1, false)
            .rows
            .into_iter()
            .find(|row| row.kind == crate::CompareDetailRowKind::Added)
            .unwrap();
        let selection = ReconcileSelection {
            dataset_index: 0,
            table_index: 1,
            key_digest: row.key_digest,
            source_row_digest: None,
            target_row_digest: row.right_digest,
            action: ReconcileAction::DeleteFromTarget,
            field_indices: Vec::new(),
        };
        let published = execute(prepare(&source, &target, &summary, &[selection], false).unwrap());
        let output_connection = Connection::open(published.path()).unwrap();
        assert_eq!(
            output_connection
                .query_row("SELECT count(*) FROM auto_domain", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(sequence(&output_connection, "auto_domain"), 10);
    }

    #[test]
    fn selected_autoincrement_insert_advances_sequence_naturally() {
        let (_source_directory, source_path) =
            crate::tests::signed_fixture("reconcile-sequence-insert-source");
        let (_target_directory, target_path) =
            crate::tests::signed_fixture("reconcile-sequence-insert-target");
        for path in [&source_path, &target_path] {
            add_autoincrement_dataset_table(path);
            make_content_reconcilable(path, "manual", false);
        }
        let source_connection = Connection::open(&source_path).unwrap();
        source_connection
            .execute(
                "INSERT INTO auto_domain(id,note) VALUES (11,'insert me')",
                [],
            )
            .unwrap();
        drop(source_connection);
        let target_connection = Connection::open(&target_path).unwrap();
        target_connection
            .execute(
                "INSERT INTO auto_domain(id,note) VALUES (10,'prior row')",
                [],
            )
            .unwrap();
        target_connection
            .execute("DELETE FROM auto_domain", [])
            .unwrap();
        assert_eq!(sequence(&target_connection, "auto_domain"), 10);
        drop(target_connection);
        let source = VerifiedWorkspaceSource::open(&source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&target_path).unwrap();
        let summary = compare(&source, &target);
        let row = detail(&source, &target, 0, 1, false)
            .rows
            .into_iter()
            .find(|row| row.kind == crate::CompareDetailRowKind::Removed)
            .unwrap();
        let selection = ReconcileSelection {
            dataset_index: 0,
            table_index: 1,
            key_digest: row.key_digest,
            source_row_digest: row.left_digest,
            target_row_digest: None,
            action: ReconcileAction::InsertFromSource,
            field_indices: Vec::new(),
        };
        let published = execute(prepare(&source, &target, &summary, &[selection], false).unwrap());
        let output_connection = Connection::open(published.path()).unwrap();
        assert_eq!(
            output_connection
                .query_row("SELECT note FROM auto_domain WHERE id=11", [], |row| {
                    row.get::<_, String>(0)
                })
                .unwrap(),
            "insert me"
        );
        assert_eq!(sequence(&output_connection, "auto_domain"), 11);
    }

    #[test]
    fn unique_constraint_failure_rolls_back_and_publishes_nothing() {
        let (_source_directory, source_path) =
            crate::tests::signed_fixture("reconcile-unique-source");
        let (_target_directory, target_path) =
            crate::tests::signed_fixture("reconcile-unique-target");
        for path in [&source_path, &target_path] {
            add_constrained_dataset_table(path);
            make_content_reconcilable(path, "manual", false);
        }
        Connection::open(&source_path)
            .unwrap()
            .execute(
                "INSERT INTO constrained_domain VALUES (2,'duplicate',1,2)",
                [],
            )
            .unwrap();
        Connection::open(&target_path)
            .unwrap()
            .execute(
                "INSERT INTO constrained_domain VALUES (1,'duplicate',1,2)",
                [],
            )
            .unwrap();
        let source_before = fs::read(&source_path).unwrap();
        let target_before = fs::read(&target_path).unwrap();
        let source = VerifiedWorkspaceSource::open(&source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&target_path).unwrap();
        let summary = compare(&source, &target);
        let row = detail(&source, &target, 0, 1, false)
            .rows
            .into_iter()
            .find(|row| row.kind == crate::CompareDetailRowKind::Removed)
            .unwrap();
        let selection = ReconcileSelection {
            dataset_index: 0,
            table_index: 1,
            key_digest: row.key_digest,
            source_row_digest: row.left_digest,
            target_row_digest: None,
            action: ReconcileAction::InsertFromSource,
            field_indices: Vec::new(),
        };
        let request = output(&target);
        assert_eq!(
            prepare_reconcile_review(
                reopen(&source),
                reopen(&target),
                &summary,
                &[selection],
                &BTreeSet::new(),
                &request,
                &ReconcileReviewLimits::default(),
                &CancellationToken::new(),
            )
            .unwrap_err()
            .kind(),
            WorkspaceErrorCode::VerificationFailed
        );
        assert!(!request.output_path.exists());
        assert_eq!(fs::read(&source_path).unwrap(), source_before);
        assert_eq!(fs::read(&target_path).unwrap(), target_before);
    }

    #[test]
    fn check_constraint_failure_rolls_back_and_publishes_nothing() {
        let (_source_directory, source_path) =
            crate::tests::signed_fixture("reconcile-check-source");
        let (_target_directory, target_path) =
            crate::tests::signed_fixture("reconcile-check-target");
        for path in [&source_path, &target_path] {
            add_constrained_dataset_table(path);
            make_content_reconcilable(path, "manual", false);
        }
        Connection::open(&source_path)
            .unwrap()
            .execute("INSERT INTO constrained_domain VALUES (1,'same',4,5)", [])
            .unwrap();
        Connection::open(&target_path)
            .unwrap()
            .execute("INSERT INTO constrained_domain VALUES (1,'same',1,2)", [])
            .unwrap();
        let source_before = fs::read(&source_path).unwrap();
        let target_before = fs::read(&target_path).unwrap();
        let source = VerifiedWorkspaceSource::open(&source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&target_path).unwrap();
        let summary = compare(&source, &target);
        let row = detail(&source, &target, 0, 1, false)
            .rows
            .into_iter()
            .find(|row| row.kind == crate::CompareDetailRowKind::Changed)
            .unwrap();
        let selection = ReconcileSelection {
            dataset_index: 0,
            table_index: 1,
            key_digest: row.key_digest,
            source_row_digest: row.left_digest,
            target_row_digest: row.right_digest,
            action: ReconcileAction::SetFields,
            field_indices: vec![2],
        };
        let request = output(&target);
        assert_eq!(
            prepare_reconcile_review(
                reopen(&source),
                reopen(&target),
                &summary,
                &[selection],
                &BTreeSet::new(),
                &request,
                &ReconcileReviewLimits::default(),
                &CancellationToken::new(),
            )
            .unwrap_err()
            .kind(),
            WorkspaceErrorCode::VerificationFailed
        );
        assert!(!request.output_path.exists());
        assert_eq!(fs::read(&source_path).unwrap(), source_before);
        assert_eq!(fs::read(&target_path).unwrap(), target_before);
    }

    #[test]
    fn late_cancellation_is_quarantined_and_never_reported_as_success() {
        let (_source_directory, source_path) =
            crate::tests::signed_fixture("reconcile-late-source");
        let (_target_directory, target_path) =
            crate::tests::signed_fixture("reconcile-late-target");
        make_content_reconcilable(&source_path, "manual", false);
        make_content_reconcilable(&target_path, "manual", false);
        Connection::open(&source_path)
            .unwrap()
            .execute(
                "UPDATE vector_domain SET note='late source' WHERE id='domain'",
                [],
            )
            .unwrap();
        let source = VerifiedWorkspaceSource::open(&source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&target_path).unwrap();
        let summary = compare(&source, &target);
        let row = detail(&source, &target, 0, 0, false)
            .rows
            .into_iter()
            .find(|row| row.kind == crate::CompareDetailRowKind::Changed)
            .unwrap();
        let selection = ReconcileSelection {
            dataset_index: 0,
            table_index: 0,
            key_digest: row.key_digest,
            source_row_digest: row.left_digest,
            target_row_digest: row.right_digest,
            action: ReconcileAction::SetFields,
            field_indices: vec![1],
        };
        let cancellation = CancellationToken::new();
        let request = output(&target);
        let review = prepare_reconcile_review(
            reopen(&source),
            reopen(&target),
            &summary,
            &[selection],
            &BTreeSet::new(),
            &request,
            &ReconcileReviewLimits::default(),
            &cancellation,
        )
        .unwrap();
        let plan = review.plan().clone();
        let payload = review.payload().to_vec();
        let validated = review
            .prepare_at(plan, &payload, approved_time())
            .unwrap()
            .stage()
            .unwrap()
            .transform_and_validate()
            .unwrap();
        let result = validated.publish_with_hook(|| cancellation.cancel());
        let error = match result {
            Ok(_) => panic!("late cancellation must not report success"),
            Err(error) => error,
        };
        assert_eq!(
            error.kind(),
            WorkspaceErrorCode::PostpublishVerificationFailed
        );
        assert_quarantine_evidence(&request.output_path);
    }

    #[test]
    fn input_mutation_before_publish_blocks_publication() {
        let (_source_directory, source_path) =
            crate::tests::signed_fixture("reconcile-prepublish-source");
        let (_target_directory, target_path) =
            crate::tests::signed_fixture("reconcile-prepublish-target");
        make_content_reconcilable(&source_path, "manual", false);
        make_content_reconcilable(&target_path, "manual", false);
        Connection::open(&source_path)
            .unwrap()
            .execute(
                "UPDATE vector_domain SET note='prepublish source' WHERE id='domain'",
                [],
            )
            .unwrap();
        let source = VerifiedWorkspaceSource::open(&source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&target_path).unwrap();
        let summary = compare(&source, &target);
        let row = detail(&source, &target, 0, 0, false)
            .rows
            .into_iter()
            .find(|row| row.kind == crate::CompareDetailRowKind::Changed)
            .unwrap();
        let selection = ReconcileSelection {
            dataset_index: 0,
            table_index: 0,
            key_digest: row.key_digest,
            source_row_digest: row.left_digest,
            target_row_digest: row.right_digest,
            action: ReconcileAction::SetFields,
            field_indices: vec![1],
        };
        let request = output(&target);
        let review = prepare(&source, &target, &summary, &[selection], false).unwrap();
        let plan = review.plan().clone();
        let payload = review.payload().to_vec();
        let validated = review
            .prepare_at(plan, &payload, approved_time())
            .unwrap()
            .stage()
            .unwrap()
            .transform_and_validate()
            .unwrap();
        Connection::open(&target_path)
            .unwrap()
            .execute("UPDATE capsule_instance SET title='raced target'", [])
            .unwrap();
        let error = match validated.publish() {
            Ok(_) => panic!("mutated target must block publication"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::StalePlan);
        assert!(!request.output_path.exists());
    }

    #[test]
    fn callback_late_input_mutation_is_quarantined() {
        let (_source_directory, source_path) =
            crate::tests::signed_fixture("reconcile-callback-source");
        let (_target_directory, target_path) =
            crate::tests::signed_fixture("reconcile-callback-target");
        make_content_reconcilable(&source_path, "manual", false);
        make_content_reconcilable(&target_path, "manual", false);
        Connection::open(&source_path)
            .unwrap()
            .execute(
                "UPDATE vector_domain SET note='callback source' WHERE id='domain'",
                [],
            )
            .unwrap();
        let source = VerifiedWorkspaceSource::open(&source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&target_path).unwrap();
        let summary = compare(&source, &target);
        let row = detail(&source, &target, 0, 0, false)
            .rows
            .into_iter()
            .find(|row| row.kind == crate::CompareDetailRowKind::Changed)
            .unwrap();
        let selection = ReconcileSelection {
            dataset_index: 0,
            table_index: 0,
            key_digest: row.key_digest,
            source_row_digest: row.left_digest,
            target_row_digest: row.right_digest,
            action: ReconcileAction::SetFields,
            field_indices: vec![1],
        };
        let request = output(&target);
        let review = prepare(&source, &target, &summary, &[selection], false).unwrap();
        let plan = review.plan().clone();
        let payload = review.payload().to_vec();
        let validated = review
            .prepare_at(plan, &payload, approved_time())
            .unwrap()
            .stage()
            .unwrap()
            .transform_and_validate()
            .unwrap();
        let result = validated.publish_with_hook(|| {
            Connection::open(&source_path)
                .unwrap()
                .execute("UPDATE capsule_instance SET title='late raced source'", [])
                .unwrap();
        });
        let error = match result {
            Ok(_) => panic!("late input mutation must not report success"),
            Err(error) => error,
        };
        assert_eq!(
            error.kind(),
            WorkspaceErrorCode::PostpublishVerificationFailed
        );
        assert_quarantine_evidence(&request.output_path);
    }

    #[test]
    fn destination_race_after_approval_is_never_overwritten() {
        let (_source_directory, source_path) =
            crate::tests::signed_fixture("reconcile-race-source");
        let (_target_directory, target_path) =
            crate::tests::signed_fixture("reconcile-race-target");
        make_content_reconcilable(&source_path, "manual", false);
        make_content_reconcilable(&target_path, "manual", false);
        Connection::open(&source_path)
            .unwrap()
            .execute(
                "UPDATE vector_domain SET note='race source' WHERE id='domain'",
                [],
            )
            .unwrap();
        let source = VerifiedWorkspaceSource::open(&source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&target_path).unwrap();
        let summary = compare(&source, &target);
        let row = detail(&source, &target, 0, 0, false)
            .rows
            .into_iter()
            .find(|row| row.kind == crate::CompareDetailRowKind::Changed)
            .unwrap();
        let selection = ReconcileSelection {
            dataset_index: 0,
            table_index: 0,
            key_digest: row.key_digest,
            source_row_digest: row.left_digest,
            target_row_digest: row.right_digest,
            action: ReconcileAction::SetFields,
            field_indices: vec![1],
        };
        let request = output(&target);
        let review = prepare(&source, &target, &summary, &[selection], false).unwrap();
        let plan = review.plan().clone();
        let payload = review.payload().to_vec();
        let prepared = review.prepare_at(plan, &payload, approved_time()).unwrap();
        fs::write(&request.output_path, b"attacker-owned destination").unwrap();
        let error = match prepared.stage() {
            Ok(_) => panic!("destination race must fail"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::DestinationExists);
        assert_eq!(
            fs::read(&request.output_path).unwrap(),
            b"attacker-owned destination"
        );
    }

    #[test]
    fn reconcile_crash_worker() {
        let Some(_) = std::env::var_os("SQLITE_CAPSULE_RECONCILE_CRASH_STAGE") else {
            return;
        };
        let source_path =
            PathBuf::from(std::env::var_os("SQLITE_CAPSULE_RECONCILE_CRASH_SOURCE").unwrap());
        let target_path =
            PathBuf::from(std::env::var_os("SQLITE_CAPSULE_RECONCILE_CRASH_TARGET").unwrap());
        let output_path =
            PathBuf::from(std::env::var_os("SQLITE_CAPSULE_RECONCILE_CRASH_OUTPUT").unwrap());
        let source = VerifiedWorkspaceSource::open(&source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&target_path).unwrap();
        let summary = compare(&source, &target);
        let row = detail(&source, &target, 0, 0, false)
            .rows
            .into_iter()
            .find(|row| row.kind == crate::CompareDetailRowKind::Changed)
            .unwrap();
        let selection = ReconcileSelection {
            dataset_index: 0,
            table_index: 0,
            key_digest: row.key_digest,
            source_row_digest: row.left_digest,
            target_row_digest: row.right_digest,
            action: ReconcileAction::SetFields,
            field_indices: vec![1],
        };
        let mut request = output(&target);
        request.output_path = output_path;
        let review = prepare_reconcile_review(
            reopen(&source),
            reopen(&target),
            &summary,
            &[selection],
            &BTreeSet::new(),
            &request,
            &ReconcileReviewLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        let plan = review.plan().clone();
        let payload = review.payload().to_vec();
        review
            .prepare_at(plan, &payload, approved_time())
            .unwrap()
            .stage()
            .unwrap()
            .transform_and_validate()
            .unwrap()
            .publish()
            .unwrap();
        panic!("configured reconcile crash stage did not terminate");
    }

    #[test]
    fn abrupt_reconcile_stages_preserve_inputs_and_never_leave_invalid_final_output() {
        let (directory, source_path) = crate::tests::signed_fixture("reconcile-crash-source");
        let (_target_directory, target_path) =
            crate::tests::signed_fixture("reconcile-crash-target");
        make_content_reconcilable(&source_path, "manual", false);
        make_content_reconcilable(&target_path, "manual", false);
        Connection::open(&source_path)
            .unwrap()
            .execute(
                "UPDATE vector_domain SET note='crash source' WHERE id='domain'",
                [],
            )
            .unwrap();
        let source_before = fs::read(&source_path).unwrap();
        let target_before = fs::read(&target_path).unwrap();
        let executable = std::env::current_exe().unwrap();
        for stage in [
            "private-created",
            "target-snapshot-copied",
            "transformed",
            "vacuumed",
            "sealed-and-verified",
            "postrename-reopened",
        ] {
            let output_path = directory
                .path()
                .join(format!("reconcile-crash-{stage}.sqlitecapsule"));
            let status = Command::new(&executable)
                .arg("reconcile::tests::reconcile_crash_worker")
                .arg("--exact")
                .arg("--nocapture")
                .env("SQLITE_CAPSULE_RECONCILE_CRASH_STAGE", stage)
                .env("SQLITE_CAPSULE_RECONCILE_CRASH_SOURCE", &source_path)
                .env("SQLITE_CAPSULE_RECONCILE_CRASH_TARGET", &target_path)
                .env("SQLITE_CAPSULE_RECONCILE_CRASH_OUTPUT", &output_path)
                .status()
                .unwrap();
            assert_eq!(status.code(), Some(97), "crash stage {stage}");
            assert_eq!(fs::read(&source_path).unwrap(), source_before);
            assert_eq!(fs::read(&target_path).unwrap(), target_before);
            if stage == "postrename-reopened" {
                assert!(
                    output_path.exists(),
                    "postrename stage must reach final leaf"
                );
                VerifiedWorkspaceSource::open(&output_path).unwrap();
            } else {
                assert!(
                    !output_path.exists(),
                    "prepublish crash stage {stage} exposed final leaf"
                );
            }
        }
    }

    #[test]
    fn stale_summary_row_preconditions_and_duplicate_operations_fail_closed() {
        let (_source_directory, source_path) =
            crate::tests::signed_fixture("reconcile-stale-source");
        let (_target_directory, target_path) =
            crate::tests::signed_fixture("reconcile-stale-target");
        make_content_reconcilable(&source_path, "manual", false);
        make_content_reconcilable(&target_path, "manual", false);
        let connection = Connection::open(&source_path).unwrap();
        connection
            .execute("UPDATE vector_domain SET note='source changed'", [])
            .unwrap();
        drop(connection);
        let source = VerifiedWorkspaceSource::open(&source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&target_path).unwrap();
        let summary = compare(&source, &target);
        let page = detail(&source, &target, 0, 0, false);
        let row = page
            .rows
            .iter()
            .find(|row| row.kind == crate::CompareDetailRowKind::Changed)
            .unwrap();
        let selection = ReconcileSelection {
            dataset_index: 0,
            table_index: 0,
            key_digest: row.key_digest.clone(),
            source_row_digest: row.left_digest.clone(),
            target_row_digest: row.right_digest.clone(),
            action: ReconcileAction::SetFields,
            field_indices: vec![1],
        };
        let mut edited = summary.clone();
        edited.report_digest = "0".repeat(64);
        assert_eq!(
            prepare(
                &source,
                &target,
                &edited,
                std::slice::from_ref(&selection),
                false
            )
            .unwrap_err()
            .kind(),
            WorkspaceErrorCode::StalePlan
        );
        let mut laundered = summary.clone();
        laundered.identity.change_count += 1;
        laundered.report_digest = recompute_report_digest(&laundered);
        assert_eq!(
            prepare(
                &source,
                &target,
                &laundered,
                std::slice::from_ref(&selection),
                false
            )
            .unwrap_err()
            .kind(),
            WorkspaceErrorCode::StalePlan
        );
        let mut wrong = selection.clone();
        wrong.target_row_digest = Some("1".repeat(64));
        assert_eq!(
            prepare(&source, &target, &summary, &[wrong], false)
                .unwrap_err()
                .kind(),
            WorkspaceErrorCode::RowPreconditionFailed
        );
        assert_eq!(
            prepare(
                &source,
                &target,
                &summary,
                &[selection.clone(), selection],
                false
            )
            .unwrap_err()
            .kind(),
            WorkspaceErrorCode::InvalidContract
        );

        let mut truncated = summary.clone();
        truncated.truncated = true;
        truncated.report_digest = recompute_report_digest(&truncated);
        assert_eq!(
            prepare(&source, &target, &truncated, &[], false)
                .unwrap_err()
                .kind(),
            WorkspaceErrorCode::StalePlan
        );
    }

    #[test]
    fn signed_policy_immutable_sensitive_and_fk_boundaries_are_fail_closed() {
        let (_source_directory, source_path) =
            crate::tests::signed_fixture("reconcile-policy-source");
        let (_target_directory, target_path) =
            crate::tests::signed_fixture("reconcile-policy-target");
        for path in [&source_path, &target_path] {
            make_content_reconcilable(path, "manual", true);
        }
        let source_connection = Connection::open(&source_path).unwrap();
        source_connection
            .execute("UPDATE vector_domain SET note='private source'", [])
            .unwrap();
        drop(source_connection);
        let source = VerifiedWorkspaceSource::open(&source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&target_path).unwrap();
        let summary = compare(&source, &target);
        let page = detail(&source, &target, 0, 0, true);
        let row = page
            .rows
            .iter()
            .find(|row| row.kind == crate::CompareDetailRowKind::Changed)
            .unwrap();
        let selection = ReconcileSelection {
            dataset_index: 0,
            table_index: 0,
            key_digest: row.key_digest.clone(),
            source_row_digest: row.left_digest.clone(),
            target_row_digest: row.right_digest.clone(),
            action: ReconcileAction::SetFields,
            field_indices: vec![1],
        };
        assert_eq!(
            prepare(
                &source,
                &target,
                &summary,
                std::slice::from_ref(&selection),
                false
            )
            .unwrap_err()
            .kind(),
            WorkspaceErrorCode::SensitiveConfirmationRequired
        );
        assert!(
            prepare(
                &source,
                &target,
                &summary,
                std::slice::from_ref(&selection),
                true
            )
            .is_ok()
        );
        let mut immutable = selection.clone();
        immutable.field_indices = vec![0];
        assert_eq!(
            prepare(&source, &target, &summary, &[immutable], true)
                .unwrap_err()
                .kind(),
            WorkspaceErrorCode::ImmutableColumn
        );

        let (_ignored_source_directory, ignored_source_path) =
            crate::tests::signed_fixture("reconcile-ignore-source");
        let (_ignored_target_directory, ignored_target_path) =
            crate::tests::signed_fixture("reconcile-ignore-target");
        for path in [&ignored_source_path, &ignored_target_path] {
            make_content_reconcilable(path, "ignore", false);
        }
        let connection = Connection::open(&ignored_source_path).unwrap();
        connection
            .execute("UPDATE vector_domain SET note='different'", [])
            .unwrap();
        drop(connection);
        let ignored_source = VerifiedWorkspaceSource::open(&ignored_source_path).unwrap();
        let ignored_target = VerifiedWorkspaceSource::open(&ignored_target_path).unwrap();
        let ignored_summary = compare(&ignored_source, &ignored_target);
        let ignored_page = detail(&ignored_source, &ignored_target, 0, 0, false);
        let ignored_row = ignored_page
            .rows
            .iter()
            .find(|row| row.kind == crate::CompareDetailRowKind::Changed)
            .unwrap();
        let ignored_selection = ReconcileSelection {
            dataset_index: 0,
            table_index: 0,
            key_digest: ignored_row.key_digest.clone(),
            source_row_digest: ignored_row.left_digest.clone(),
            target_row_digest: ignored_row.right_digest.clone(),
            action: ReconcileAction::ReplaceRowFromSource,
            field_indices: Vec::new(),
        };
        assert_eq!(
            prepare(
                &ignored_source,
                &ignored_target,
                &ignored_summary,
                &[ignored_selection],
                false
            )
            .unwrap_err()
            .kind(),
            WorkspaceErrorCode::UnsupportedOperation
        );

        let (_fk_source_directory, fk_source_path) =
            crate::tests::signed_fixture("reconcile-fk-source");
        let (_fk_target_directory, fk_target_path) =
            crate::tests::signed_fixture("reconcile-fk-target");
        for path in [&fk_source_path, &fk_target_path] {
            let connection = Connection::open(path).unwrap();
            connection.execute_batch("CREATE TABLE vector_child (id TEXT PRIMARY KEY NOT NULL, parent_id TEXT REFERENCES vector_domain(id)); INSERT INTO vector_child VALUES ('child','domain'); INSERT INTO capsule_dataset_table VALUES ('content','vector_child',1,'[\"id\"]','[]','[\"id\"]'); UPDATE capsule_dataset SET reconcile_policy='manual' WHERE id='content';").unwrap();
            resign(&connection);
        }
        let connection = Connection::open(&fk_source_path).unwrap();
        connection
            .execute("UPDATE vector_domain SET note='fk source'", [])
            .unwrap();
        drop(connection);
        let fk_source = VerifiedWorkspaceSource::open(&fk_source_path).unwrap();
        let fk_target = VerifiedWorkspaceSource::open(&fk_target_path).unwrap();
        let fk_summary = compare(&fk_source, &fk_target);
        let fk_page = detail(&fk_source, &fk_target, 0, 0, false);
        let fk_row = fk_page
            .rows
            .iter()
            .find(|row| row.kind == crate::CompareDetailRowKind::Changed)
            .unwrap();
        let fk_selection = ReconcileSelection {
            dataset_index: 0,
            table_index: 0,
            key_digest: fk_row.key_digest.clone(),
            source_row_digest: fk_row.left_digest.clone(),
            target_row_digest: fk_row.right_digest.clone(),
            action: ReconcileAction::SetFields,
            field_indices: vec![1],
        };
        assert!(prepare(&fk_source, &fk_target, &fk_summary, &[fk_selection], false).is_ok());
    }

    #[test]
    fn any_signed_forbid_dataset_blocks_two_way_reconcile_before_destination_reservation() {
        let (_source_directory, source_path) =
            crate::tests::signed_fixture("reconcile-global-forbid-source");
        let (_target_directory, target_path) =
            crate::tests::signed_fixture("reconcile-global-forbid-target");
        for path in [&source_path, &target_path] {
            let connection = Connection::open(path).unwrap();
            connection
                .execute_batch(
                    "UPDATE capsule_dataset SET reconcile_policy='manual' WHERE id='content';
                     UPDATE capsule_dataset SET reconcile_policy='forbid', compare_policy='ignore'
                     WHERE id='settings';",
                )
                .unwrap();
            resign(&connection);
        }
        Connection::open(&source_path)
            .unwrap()
            .execute("UPDATE vector_domain SET note='selected source change'", [])
            .unwrap();
        let source = VerifiedWorkspaceSource::open(&source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&target_path).unwrap();
        let summary = compare(&source, &target);
        let row = detail(&source, &target, 0, 0, false)
            .rows
            .into_iter()
            .find(|row| row.kind == crate::CompareDetailRowKind::Changed)
            .unwrap();
        let selection = ReconcileSelection {
            dataset_index: 0,
            table_index: 0,
            key_digest: row.key_digest,
            source_row_digest: row.left_digest,
            target_row_digest: row.right_digest,
            action: ReconcileAction::SetFields,
            field_indices: vec![1],
        };
        let request = output(&target);
        assert_eq!(
            prepare_reconcile_review(
                reopen(&source),
                reopen(&target),
                &summary,
                &[selection],
                &BTreeSet::new(),
                &request,
                &ReconcileReviewLimits::default(),
                &CancellationToken::new(),
            )
            .unwrap_err()
            .kind(),
            WorkspaceErrorCode::UnsupportedOperation
        );
        assert!(!request.output_path.exists());
    }

    #[test]
    fn three_way_sensitive_confirmation_is_exact_per_changed_dataset() {
        let (_ancestor_directory, ancestor_path) =
            crate::tests::signed_fixture("reconcile-sensitive-set-ancestor");
        let (_source_directory, source_path) =
            crate::tests::signed_fixture("reconcile-sensitive-set-source");
        let (_target_directory, target_path) =
            crate::tests::signed_fixture("reconcile-sensitive-set-target");
        for path in [&ancestor_path, &source_path, &target_path] {
            let connection = Connection::open(path).unwrap();
            connection
                .execute(
                    "UPDATE capsule_dataset SET reconcile_policy='three-way', sensitivity='sensitive'",
                    [],
                )
                .unwrap();
            resign(&connection);
        }
        Connection::open(&source_path)
            .unwrap()
            .execute_batch(
                "UPDATE vector_domain SET note='sensitive content';
                 UPDATE vector_settings SET value='sensitive setting';",
            )
            .unwrap();
        let source = VerifiedWorkspaceSource::open(&source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&target_path).unwrap();
        let summary = compare(&source, &target);
        assert_eq!(
            classify_three_way_reconcile(
                VerifiedWorkspaceSource::open(&ancestor_path).unwrap(),
                reopen(&source),
                reopen(&target),
                &summary,
                &BTreeSet::from([0]),
                &ReconcileReviewLimits::default(),
                &CancellationToken::new(),
            )
            .unwrap_err()
            .kind(),
            WorkspaceErrorCode::SensitiveConfirmationRequired
        );
        assert_eq!(
            classify_three_way_reconcile(
                VerifiedWorkspaceSource::open(&ancestor_path).unwrap(),
                reopen(&source),
                reopen(&target),
                &summary,
                &BTreeSet::from([0, 1, 2]),
                &ReconcileReviewLimits::default(),
                &CancellationToken::new(),
            )
            .unwrap_err()
            .kind(),
            WorkspaceErrorCode::InvalidContract
        );
        let review = classify_three_way_reconcile(
            VerifiedWorkspaceSource::open(&ancestor_path).unwrap(),
            reopen(&source),
            reopen(&target),
            &summary,
            &BTreeSet::from([0, 1]),
            &ReconcileReviewLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        assert_eq!(review.clean_change_count(), 2);
        assert_eq!(review.conflicts().len(), 0);
        let review = review
            .resolve(&[], &output(&target), HARD_DEADLINE)
            .unwrap();
        let payload: JsonValue = serde_json::from_slice(review.payload()).unwrap();
        assert_eq!(
            payload["sensitive_confirmation"]["confirmed_dataset_ids"],
            json!(["content", "settings"])
        );

        let (_conflict_ancestor_directory, conflict_ancestor_path) =
            crate::tests::signed_fixture("reconcile-sensitive-conflict-ancestor");
        let (_conflict_source_directory, conflict_source_path) =
            crate::tests::signed_fixture("reconcile-sensitive-conflict-source");
        let (_conflict_target_directory, conflict_target_path) =
            crate::tests::signed_fixture("reconcile-sensitive-conflict-target");
        for path in [
            &conflict_ancestor_path,
            &conflict_source_path,
            &conflict_target_path,
        ] {
            let connection = Connection::open(path).unwrap();
            connection
                .execute_batch(
                    "UPDATE capsule_dataset SET reconcile_policy='three-way';
                     UPDATE capsule_dataset SET sensitivity='sensitive' WHERE id='content';",
                )
                .unwrap();
            resign(&connection);
        }
        Connection::open(&conflict_source_path)
            .unwrap()
            .execute("UPDATE vector_domain SET note='sensitive source'", [])
            .unwrap();
        Connection::open(&conflict_target_path)
            .unwrap()
            .execute("UPDATE vector_domain SET note='sensitive target'", [])
            .unwrap();
        let source = VerifiedWorkspaceSource::open(&conflict_source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&conflict_target_path).unwrap();
        let summary = compare(&source, &target);
        let classified = classify_three_way_reconcile(
            VerifiedWorkspaceSource::open(&conflict_ancestor_path).unwrap(),
            source,
            target,
            &summary,
            &BTreeSet::from([0]),
            &ReconcileReviewLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        let conflict_id = classified.conflicts().next().unwrap().id.clone();
        let review = classified
            .resolve(
                &[ThreeWayConflictResolution {
                    conflict_id,
                    choice: ThreeWayResolutionChoice::KeepTarget,
                }],
                &output(&VerifiedWorkspaceSource::open(&conflict_target_path).unwrap()),
                HARD_DEADLINE,
            )
            .unwrap();
        assert_eq!(review.operation_count(), 0);
        let payload: JsonValue = serde_json::from_slice(review.payload()).unwrap();
        assert_eq!(
            payload["sensitive_confirmation"]["confirmed_dataset_ids"],
            json!(["content"])
        );
    }

    #[test]
    fn restrictive_foreign_keys_use_signed_dependency_order_and_missing_companions_rollback() {
        fn install_cross_dataset_fk(path: &Path, seed: bool, policy: &str) {
            let connection = Connection::open(path).unwrap();
            connection
                .execute_batch(&format!(
                    "CREATE TABLE vector_pref_ref (
                         id TEXT PRIMARY KEY NOT NULL,
                         setting_key TEXT NOT NULL REFERENCES vector_settings(key)
                             ON UPDATE RESTRICT ON DELETE RESTRICT,
                         note TEXT NOT NULL DEFAULT 'target'
                     );
                     INSERT INTO capsule_dataset_table VALUES
                         ('content','vector_pref_ref',1,'[\"id\"]','[]','[\"id\"]');
                     UPDATE capsule_dataset SET reconcile_policy='{policy}';"
                ))
                .unwrap();
            if seed {
                connection
                    .execute_batch(
                        "INSERT INTO vector_settings VALUES ('new-setting','source');
                         INSERT INTO vector_pref_ref (id,setting_key)
                         VALUES ('ref','new-setting');",
                    )
                    .unwrap();
            }
            resign(&connection);
        }

        let (_source_directory, source_path) =
            crate::tests::signed_fixture("reconcile-fk-order-source");
        let (_target_directory, target_path) =
            crate::tests::signed_fixture("reconcile-fk-order-target");
        install_cross_dataset_fk(&source_path, false, "manual");
        install_cross_dataset_fk(&target_path, false, "manual");
        Connection::open(&source_path)
            .unwrap()
            .execute_batch(
                "INSERT INTO vector_settings VALUES ('new-setting','source');
                 INSERT INTO vector_pref_ref (id,setting_key)
                 VALUES ('ref','new-setting');",
            )
            .unwrap();
        let source = VerifiedWorkspaceSource::open(&source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&target_path).unwrap();
        let summary = compare(&source, &target);
        let child = detail(&source, &target, 0, 1, false)
            .rows
            .into_iter()
            .find(|row| row.left_digest.is_some() && row.right_digest.is_none())
            .unwrap();
        let parent = detail(&source, &target, 1, 0, false)
            .rows
            .into_iter()
            .find(|row| row.left_digest.is_some() && row.right_digest.is_none())
            .unwrap();
        let child_selection = ReconcileSelection {
            dataset_index: 0,
            table_index: 1,
            key_digest: child.key_digest,
            source_row_digest: child.left_digest,
            target_row_digest: None,
            action: ReconcileAction::InsertFromSource,
            field_indices: Vec::new(),
        };
        let parent_selection = ReconcileSelection {
            dataset_index: 1,
            table_index: 0,
            key_digest: parent.key_digest,
            source_row_digest: parent.left_digest,
            target_row_digest: None,
            action: ReconcileAction::InsertFromSource,
            field_indices: Vec::new(),
        };

        let mut missing_request = output(&target);
        missing_request.output_path = missing_request
            .output_path
            .with_file_name("reconcile-fk-missing-parent.sqlitecapsule");
        assert_eq!(
            prepare_reconcile_review(
                reopen(&source),
                reopen(&target),
                &summary,
                std::slice::from_ref(&child_selection),
                &BTreeSet::new(),
                &missing_request,
                &ReconcileReviewLimits::default(),
                &CancellationToken::new(),
            )
            .unwrap_err()
            .kind(),
            WorkspaceErrorCode::VerificationFailed
        );
        assert!(!missing_request.output_path.exists());

        let review = prepare(
            &source,
            &target,
            &summary,
            &[child_selection, parent_selection],
            false,
        )
        .unwrap();
        assert_eq!(
            review
                .operations()
                .iter()
                .map(|operation| operation.table.as_str())
                .collect::<Vec<_>>(),
            vec!["vector_settings", "vector_pref_ref"]
        );
        let published = execute(review);
        let connection = Connection::open(published.path()).unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT setting_key FROM vector_pref_ref WHERE id='ref'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "new-setting"
        );

        let (_delete_source_directory, delete_source_path) =
            crate::tests::signed_fixture("reconcile-fk-delete-source");
        let (_delete_target_directory, delete_target_path) =
            crate::tests::signed_fixture("reconcile-fk-delete-target");
        install_cross_dataset_fk(&delete_source_path, true, "manual");
        install_cross_dataset_fk(&delete_target_path, true, "manual");
        Connection::open(&delete_source_path)
            .unwrap()
            .execute_batch(
                "DELETE FROM vector_pref_ref WHERE id='ref';
                 DELETE FROM vector_settings WHERE key='new-setting';",
            )
            .unwrap();
        let delete_source = VerifiedWorkspaceSource::open(&delete_source_path).unwrap();
        let delete_target = VerifiedWorkspaceSource::open(&delete_target_path).unwrap();
        let delete_summary = compare(&delete_source, &delete_target);
        let child = detail(&delete_source, &delete_target, 0, 1, false)
            .rows
            .into_iter()
            .find(|row| row.left_digest.is_none() && row.right_digest.is_some())
            .unwrap();
        let parent = detail(&delete_source, &delete_target, 1, 0, false)
            .rows
            .into_iter()
            .find(|row| row.left_digest.is_none() && row.right_digest.is_some())
            .unwrap();
        let review = prepare(
            &delete_source,
            &delete_target,
            &delete_summary,
            &[
                ReconcileSelection {
                    dataset_index: 1,
                    table_index: 0,
                    key_digest: parent.key_digest,
                    source_row_digest: None,
                    target_row_digest: parent.right_digest,
                    action: ReconcileAction::DeleteFromTarget,
                    field_indices: Vec::new(),
                },
                ReconcileSelection {
                    dataset_index: 0,
                    table_index: 1,
                    key_digest: child.key_digest,
                    source_row_digest: None,
                    target_row_digest: child.right_digest,
                    action: ReconcileAction::DeleteFromTarget,
                    field_indices: Vec::new(),
                },
            ],
            false,
        )
        .unwrap();
        assert_eq!(
            review
                .operations()
                .iter()
                .map(|operation| operation.table.as_str())
                .collect::<Vec<_>>(),
            vec!["vector_pref_ref", "vector_settings"]
        );
        execute(review);

        let (_reparent_source_directory, reparent_source_path) =
            crate::tests::signed_fixture("reconcile-fk-reparent-source");
        let (_reparent_target_directory, reparent_target_path) =
            crate::tests::signed_fixture("reconcile-fk-reparent-target");
        install_cross_dataset_fk(&reparent_source_path, false, "manual");
        install_cross_dataset_fk(&reparent_target_path, false, "manual");
        for path in [&reparent_source_path, &reparent_target_path] {
            Connection::open(path)
                .unwrap()
                .execute(
                    "INSERT INTO vector_pref_ref (id,setting_key,note)
                     VALUES ('ref','theme','existing')",
                    [],
                )
                .unwrap();
        }
        Connection::open(&reparent_source_path)
            .unwrap()
            .execute_batch(
                "INSERT INTO vector_settings VALUES ('new-parent','source');
                 UPDATE vector_pref_ref SET setting_key='new-parent' WHERE id='ref';",
            )
            .unwrap();
        let source = VerifiedWorkspaceSource::open(&reparent_source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&reparent_target_path).unwrap();
        let summary = compare(&source, &target);
        let child = detail(&source, &target, 0, 1, false)
            .rows
            .into_iter()
            .find(|row| row.kind == crate::CompareDetailRowKind::Changed)
            .unwrap();
        let parent = detail(&source, &target, 1, 0, false)
            .rows
            .into_iter()
            .find(|row| row.left_digest.is_some() && row.right_digest.is_none())
            .unwrap();
        let review = prepare(
            &source,
            &target,
            &summary,
            &[
                ReconcileSelection {
                    dataset_index: 0,
                    table_index: 1,
                    key_digest: child.key_digest,
                    source_row_digest: child.left_digest,
                    target_row_digest: child.right_digest,
                    action: ReconcileAction::ReplaceRowFromSource,
                    field_indices: Vec::new(),
                },
                ReconcileSelection {
                    dataset_index: 1,
                    table_index: 0,
                    key_digest: parent.key_digest,
                    source_row_digest: parent.left_digest,
                    target_row_digest: None,
                    action: ReconcileAction::InsertFromSource,
                    field_indices: Vec::new(),
                },
            ],
            false,
        )
        .unwrap();
        assert_eq!(
            review
                .operations()
                .iter()
                .map(|operation| operation.table.as_str())
                .collect::<Vec<_>>(),
            vec!["vector_settings", "vector_pref_ref"]
        );
        let published = execute(review);
        assert_eq!(
            Connection::open(published.path())
                .unwrap()
                .query_row(
                    "SELECT setting_key FROM vector_pref_ref WHERE id='ref'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "new-parent"
        );

        let (_update_source_directory, update_source_path) =
            crate::tests::signed_fixture("reconcile-fk-update-source");
        let (_update_target_directory, update_target_path) =
            crate::tests::signed_fixture("reconcile-fk-update-target");
        install_cross_dataset_fk(&update_source_path, true, "manual");
        install_cross_dataset_fk(&update_target_path, true, "manual");
        Connection::open(&update_source_path)
            .unwrap()
            .execute_batch(
                "UPDATE vector_settings SET value='updated-parent' WHERE key='new-setting';
                 UPDATE vector_pref_ref SET note='updated-child' WHERE id='ref';",
            )
            .unwrap();
        let update_source = VerifiedWorkspaceSource::open(&update_source_path).unwrap();
        let update_target = VerifiedWorkspaceSource::open(&update_target_path).unwrap();
        let update_summary = compare(&update_source, &update_target);
        let child = detail(&update_source, &update_target, 0, 1, false)
            .rows
            .into_iter()
            .find(|row| row.kind == crate::CompareDetailRowKind::Changed)
            .unwrap();
        let parent = detail(&update_source, &update_target, 1, 0, false)
            .rows
            .into_iter()
            .find(|row| row.kind == crate::CompareDetailRowKind::Changed)
            .unwrap();
        let review = prepare(
            &update_source,
            &update_target,
            &update_summary,
            &[
                ReconcileSelection {
                    dataset_index: 0,
                    table_index: 1,
                    key_digest: child.key_digest,
                    source_row_digest: child.left_digest,
                    target_row_digest: child.right_digest,
                    action: ReconcileAction::ReplaceRowFromSource,
                    field_indices: Vec::new(),
                },
                ReconcileSelection {
                    dataset_index: 1,
                    table_index: 0,
                    key_digest: parent.key_digest,
                    source_row_digest: parent.left_digest,
                    target_row_digest: parent.right_digest,
                    action: ReconcileAction::ReplaceRowFromSource,
                    field_indices: Vec::new(),
                },
            ],
            false,
        )
        .unwrap();
        assert_eq!(
            review
                .operations()
                .iter()
                .map(|operation| operation.table.as_str())
                .collect::<Vec<_>>(),
            vec!["vector_settings", "vector_pref_ref"]
        );
        execute(review);

        let (_ancestor_directory, ancestor_path) =
            crate::tests::signed_fixture("reconcile-fk-three-way-ancestor");
        let (_three_source_directory, three_source_path) =
            crate::tests::signed_fixture("reconcile-fk-three-way-source");
        let (_three_target_directory, three_target_path) =
            crate::tests::signed_fixture("reconcile-fk-three-way-target");
        for path in [&ancestor_path, &three_source_path, &three_target_path] {
            install_cross_dataset_fk(path, false, "three-way");
        }
        Connection::open(&three_source_path)
            .unwrap()
            .execute_batch(
                "INSERT INTO vector_settings VALUES ('new-setting','source');
                 INSERT INTO vector_pref_ref VALUES ('ref','new-setting','source');",
            )
            .unwrap();
        let three_source = VerifiedWorkspaceSource::open(&three_source_path).unwrap();
        let three_target = VerifiedWorkspaceSource::open(&three_target_path).unwrap();
        let three_summary = compare(&three_source, &three_target);
        let review = classify_three_way_reconcile(
            VerifiedWorkspaceSource::open(&ancestor_path).unwrap(),
            three_source,
            three_target,
            &three_summary,
            &BTreeSet::new(),
            &ReconcileReviewLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap()
        .resolve(
            &[],
            &ReconcileOutputRequest {
                output_path: three_target_path
                    .with_file_name("reconcile-fk-three-way-output.sqlitecapsule"),
                ..output(&VerifiedWorkspaceSource::open(&three_target_path).unwrap())
            },
            HARD_DEADLINE,
        )
        .unwrap();
        let ordered_tables = review
            .operations()
            .iter()
            .map(|operation| operation.table.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ordered_tables, vec!["vector_settings", "vector_pref_ref"]);
        let payload: JsonValue = serde_json::from_slice(review.payload()).unwrap();
        assert_eq!(
            payload["operations"]
                .as_array()
                .unwrap()
                .iter()
                .map(|operation| operation["table"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ordered_tables
        );
        execute(review);
    }

    #[test]
    fn same_dataset_restrictive_foreign_keys_reorder_parent_first_insert_and_child_first_delete() {
        fn install_same_dataset_fk(path: &Path, seed: bool) {
            let connection = Connection::open(path).unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE vector_child (
                         id TEXT PRIMARY KEY NOT NULL,
                         parent_id TEXT NOT NULL REFERENCES vector_domain(id)
                             ON UPDATE RESTRICT ON DELETE RESTRICT
                     );
                     INSERT INTO capsule_dataset_table VALUES
                         ('content','vector_child',1,'[\"id\"]','[]','[\"id\"]');
                     UPDATE capsule_dataset SET reconcile_policy='manual';",
                )
                .unwrap();
            if seed {
                connection
                    .execute_batch(
                        "INSERT INTO vector_domain VALUES
                             ('new-parent','parent',1.0,X'01');
                         INSERT INTO vector_child VALUES ('new-child','new-parent');",
                    )
                    .unwrap();
            }
            resign(&connection);
        }

        let (_insert_source_directory, insert_source_path) =
            crate::tests::signed_fixture("reconcile-same-fk-insert-source");
        let (_insert_target_directory, insert_target_path) =
            crate::tests::signed_fixture("reconcile-same-fk-insert-target");
        install_same_dataset_fk(&insert_source_path, false);
        install_same_dataset_fk(&insert_target_path, false);
        Connection::open(&insert_source_path)
            .unwrap()
            .execute_batch(
                "INSERT INTO vector_domain VALUES ('new-parent','parent',1.0,X'01');
                 INSERT INTO vector_child VALUES ('new-child','new-parent');",
            )
            .unwrap();
        let source = VerifiedWorkspaceSource::open(&insert_source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&insert_target_path).unwrap();
        let summary = compare(&source, &target);
        let parent = detail(&source, &target, 0, 0, false)
            .rows
            .into_iter()
            .find(|row| row.left_digest.is_some() && row.right_digest.is_none())
            .unwrap();
        let child = detail(&source, &target, 0, 1, false)
            .rows
            .into_iter()
            .find(|row| row.left_digest.is_some() && row.right_digest.is_none())
            .unwrap();
        let review = prepare(
            &source,
            &target,
            &summary,
            &[
                ReconcileSelection {
                    dataset_index: 0,
                    table_index: 1,
                    key_digest: child.key_digest,
                    source_row_digest: child.left_digest,
                    target_row_digest: None,
                    action: ReconcileAction::InsertFromSource,
                    field_indices: Vec::new(),
                },
                ReconcileSelection {
                    dataset_index: 0,
                    table_index: 0,
                    key_digest: parent.key_digest,
                    source_row_digest: parent.left_digest,
                    target_row_digest: None,
                    action: ReconcileAction::InsertFromSource,
                    field_indices: Vec::new(),
                },
            ],
            false,
        )
        .unwrap();
        assert_eq!(
            review
                .operations()
                .iter()
                .map(|operation| operation.table.as_str())
                .collect::<Vec<_>>(),
            vec!["vector_domain", "vector_child"]
        );
        let published = execute(review);
        assert_eq!(
            Connection::open(published.path())
                .unwrap()
                .query_row(
                    "SELECT parent_id FROM vector_child WHERE id='new-child'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "new-parent"
        );

        let (_delete_source_directory, delete_source_path) =
            crate::tests::signed_fixture("reconcile-same-fk-delete-source");
        let (_delete_target_directory, delete_target_path) =
            crate::tests::signed_fixture("reconcile-same-fk-delete-target");
        install_same_dataset_fk(&delete_source_path, true);
        install_same_dataset_fk(&delete_target_path, true);
        Connection::open(&delete_source_path)
            .unwrap()
            .execute_batch(
                "DELETE FROM vector_child WHERE id='new-child';
                 DELETE FROM vector_domain WHERE id='new-parent';",
            )
            .unwrap();
        let source = VerifiedWorkspaceSource::open(&delete_source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&delete_target_path).unwrap();
        let summary = compare(&source, &target);
        let parent = detail(&source, &target, 0, 0, false)
            .rows
            .into_iter()
            .find(|row| row.left_digest.is_none() && row.right_digest.is_some())
            .unwrap();
        let child = detail(&source, &target, 0, 1, false)
            .rows
            .into_iter()
            .find(|row| row.left_digest.is_none() && row.right_digest.is_some())
            .unwrap();
        let review = prepare(
            &source,
            &target,
            &summary,
            &[
                ReconcileSelection {
                    dataset_index: 0,
                    table_index: 0,
                    key_digest: parent.key_digest,
                    source_row_digest: None,
                    target_row_digest: parent.right_digest,
                    action: ReconcileAction::DeleteFromTarget,
                    field_indices: Vec::new(),
                },
                ReconcileSelection {
                    dataset_index: 0,
                    table_index: 1,
                    key_digest: child.key_digest,
                    source_row_digest: None,
                    target_row_digest: child.right_digest,
                    action: ReconcileAction::DeleteFromTarget,
                    field_indices: Vec::new(),
                },
            ],
            false,
        )
        .unwrap();
        assert_eq!(
            review
                .operations()
                .iter()
                .map(|operation| operation.table.as_str())
                .collect::<Vec<_>>(),
            vec!["vector_child", "vector_domain"]
        );
        let published = execute(review);
        assert_eq!(
            Connection::open(published.path())
                .unwrap()
                .query_row(
                    "SELECT count(*) FROM vector_domain WHERE id='new-parent'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn nonrestrictive_and_cyclic_foreign_key_graphs_never_gain_reconcile_authority() {
        for (name, suffix) in [
            ("delete-cascade", "ON DELETE CASCADE"),
            ("update-cascade", "ON UPDATE CASCADE"),
            ("delete-set-null", "ON DELETE SET NULL"),
            ("update-set-default", "ON UPDATE SET DEFAULT"),
            ("self-cycle", "ON DELETE RESTRICT"),
        ] {
            let (_source_directory, source_path) =
                crate::tests::signed_fixture(&format!("reconcile-fk-{name}-source"));
            let (_target_directory, target_path) =
                crate::tests::signed_fixture(&format!("reconcile-fk-{name}-target"));
            for path in [&source_path, &target_path] {
                let connection = Connection::open(path).unwrap();
                connection
                    .execute_batch(&format!(
                        "CREATE TABLE vector_child (
                             id TEXT PRIMARY KEY NOT NULL,
                             parent_id TEXT REFERENCES vector_domain(id) {suffix}
                         );
                         INSERT INTO vector_child VALUES ('child','domain');
                         INSERT INTO capsule_dataset_table VALUES
                             ('content','vector_child',1,'[\"id\"]','[]','[\"id\"]');
                         UPDATE capsule_dataset SET reconcile_policy='manual';"
                    ))
                    .unwrap();
                if name == "self-cycle" {
                    connection
                        .execute_batch(
                            "DROP TABLE vector_child;
                             CREATE TABLE vector_child (
                                 id TEXT PRIMARY KEY NOT NULL,
                                 parent_id TEXT REFERENCES vector_child(id) ON DELETE RESTRICT
                             );
                             INSERT INTO vector_child VALUES ('child',NULL);",
                        )
                        .unwrap();
                }
                resign(&connection);
            }
            Connection::open(&source_path)
                .unwrap()
                .execute("UPDATE vector_domain SET note='fk change'", [])
                .unwrap();
            let source = VerifiedWorkspaceSource::open(&source_path).unwrap();
            let target = VerifiedWorkspaceSource::open(&target_path).unwrap();
            let summary = compare(&source, &target);
            let row = detail(&source, &target, 0, 0, false)
                .rows
                .into_iter()
                .find(|row| row.kind == crate::CompareDetailRowKind::Changed)
                .unwrap();
            assert_eq!(
                prepare(
                    &source,
                    &target,
                    &summary,
                    &[ReconcileSelection {
                        dataset_index: 0,
                        table_index: 0,
                        key_digest: row.key_digest,
                        source_row_digest: row.left_digest,
                        target_row_digest: row.right_digest,
                        action: ReconcileAction::SetFields,
                        field_indices: vec![1],
                    }],
                    false,
                )
                .unwrap_err()
                .kind(),
                WorkspaceErrorCode::UnsupportedOperation
            );
        }
    }

    #[test]
    fn cancellation_limits_and_live_source_mutation_fail_without_value_leaks() {
        let (_source_directory, source_path) =
            crate::tests::signed_fixture("reconcile-control-source");
        let (_target_directory, target_path) =
            crate::tests::signed_fixture("reconcile-control-target");
        make_content_reconcilable(&source_path, "manual", false);
        make_content_reconcilable(&target_path, "manual", false);
        let connection = Connection::open(&source_path).unwrap();
        connection
            .execute("UPDATE vector_domain SET note='secret source value'", [])
            .unwrap();
        drop(connection);
        let source = VerifiedWorkspaceSource::open(&source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&target_path).unwrap();
        let summary = compare(&source, &target);
        let page = detail(&source, &target, 0, 0, false);
        let row = page
            .rows
            .iter()
            .find(|row| row.kind == crate::CompareDetailRowKind::Changed)
            .unwrap();
        let selection = ReconcileSelection {
            dataset_index: 0,
            table_index: 0,
            key_digest: row.key_digest.clone(),
            source_row_digest: row.left_digest.clone(),
            target_row_digest: row.right_digest.clone(),
            action: ReconcileAction::SetFields,
            field_indices: vec![1],
        };
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        assert_eq!(
            prepare_reconcile_review(
                reopen(&source),
                reopen(&target),
                &summary,
                std::slice::from_ref(&selection),
                &BTreeSet::new(),
                &output(&target),
                &ReconcileReviewLimits::default(),
                &cancellation
            )
            .unwrap_err()
            .kind(),
            WorkspaceErrorCode::Cancelled
        );
        for limits in [
            ReconcileReviewLimits {
                max_operations: 0,
                ..ReconcileReviewLimits::default()
            },
            ReconcileReviewLimits {
                max_rows_scanned: 1,
                ..ReconcileReviewLimits::default()
            },
            ReconcileReviewLimits {
                max_value_bytes: 1,
                ..ReconcileReviewLimits::default()
            },
            ReconcileReviewLimits {
                max_stream_bytes: 1,
                ..ReconcileReviewLimits::default()
            },
            ReconcileReviewLimits {
                max_retained_bytes: 1,
                ..ReconcileReviewLimits::default()
            },
        ] {
            let error = prepare_reconcile_review(
                reopen(&source),
                reopen(&target),
                &summary,
                std::slice::from_ref(&selection),
                &BTreeSet::new(),
                &output(&target),
                &limits,
                &CancellationToken::new(),
            )
            .unwrap_err();
            assert_eq!(error.kind(), WorkspaceErrorCode::LimitExceeded);
            assert!(!format!("{error:?}").contains("secret source value"));
        }
        let too_many = vec![selection.clone(); HARD_OPERATIONS + 1];
        assert_eq!(
            prepare_reconcile_review(
                reopen(&source),
                reopen(&target),
                &summary,
                &too_many,
                &BTreeSet::new(),
                &output(&target),
                &ReconcileReviewLimits {
                    max_operations: usize::MAX,
                    ..ReconcileReviewLimits::default()
                },
                &CancellationToken::new(),
            )
            .unwrap_err()
            .kind(),
            WorkspaceErrorCode::LimitExceeded
        );
        let held = prepare(
            &source,
            &target,
            &summary,
            std::slice::from_ref(&selection),
            false,
        )
        .unwrap();
        let connection = Connection::open(&target_path).unwrap();
        connection
            .execute("UPDATE capsule_instance SET title='secret raced value'", [])
            .unwrap();
        drop(connection);
        let error = prepare(&source, &target, &summary, &[selection], false).unwrap_err();
        assert_eq!(error.kind(), WorkspaceErrorCode::StalePlan);
        assert!(!format!("{error:?}").contains("secret raced value"));
        let approved_plan = held.plan().clone();
        let approved_payload = held.payload().to_vec();
        assert_eq!(
            held.prepare_at(approved_plan, &approved_payload, approved_time())
                .unwrap_err()
                .kind(),
            WorkspaceErrorCode::StalePlan
        );
    }

    #[test]
    fn typed_value_equality_is_canonical_and_distinguishes_absence() {
        let maximum = HARD_VALUE_BYTES;
        assert!(
            !values_equal(&CompareValue::Real(-0.0), &CompareValue::Real(0.0), maximum).unwrap()
        );
        assert!(
            !values_equal(&CompareValue::Integer(1), &CompareValue::Real(1.0), maximum).unwrap()
        );
        assert!(
            !values_equal(
                &CompareValue::Blob(b"same".to_vec()),
                &CompareValue::Text(b"same".to_vec()),
                maximum
            )
            .unwrap()
        );
        assert!(
            !values_equal(
                &CompareValue::Text("é".as_bytes().to_vec()),
                &CompareValue::Text("e\u{301}".as_bytes().to_vec()),
                maximum
            )
            .unwrap()
        );
        assert!(values_equal(&CompareValue::Null, &CompareValue::Null, maximum).unwrap());

        let null_digest = value_digest(&CompareValue::Null, maximum).unwrap();
        assert_ne!(null_digest, lower_hex(&Sha256::digest([])));
        let mut absent = Vec::new();
        frame_optional_text(&mut absent, None).unwrap();
        let mut present_null = Vec::new();
        frame_optional_text(&mut present_null, Some(&null_digest)).unwrap();
        assert_ne!(absent, present_null);

        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch("CREATE TABLE flexible (id PRIMARY KEY NOT NULL, note TEXT); INSERT INTO flexible VALUES (1,'integer key');")
            .unwrap();
        let table = DatasetTable {
            name: "flexible".to_owned(),
            sequence: 0,
            primary_key: vec!["id".to_owned()],
            ignored_columns: Vec::new(),
            immutable_columns: Vec::new(),
        };
        let cancellation = CancellationToken::new();
        assert_eq!(
            load_exact_current_row(
                &connection,
                &table,
                &["id".to_owned(), "note".to_owned()],
                &["id".to_owned(), "note".to_owned()],
                &[("id".to_owned(), CompareValue::Real(1.0))],
                maximum,
                Instant::now() + Duration::from_secs(5),
                &cancellation,
            )
            .unwrap_err()
            .kind(),
            WorkspaceErrorCode::RowPreconditionFailed
        );
    }

    #[test]
    fn sqlite_sequence_authority_ignores_fake_tokens_and_rejects_unknown_or_duplicate_rows() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(
            "CREATE TABLE real_sequence (id INTEGER PRIMARY KEY AUTOINCREMENT, note TEXT); \
             CREATE TABLE never_used_sequence (id INTEGER PRIMARY KEY AUTOINCREMENT, note TEXT); \
             CREATE TABLE fake_literal (id INTEGER PRIMARY KEY, note TEXT DEFAULT 'AUTOINCREMENT'); \
             CREATE TABLE fake_comment (id INTEGER PRIMARY KEY /* AUTOINCREMENT */, note TEXT); \
             INSERT INTO real_sequence(note) VALUES ('one');",
        ).unwrap();
        assert_eq!(
            sequence_managed_tables(&connection).unwrap(),
            BTreeSet::from(["never_used_sequence".to_owned(), "real_sequence".to_owned(),])
        );
        validate_sqlite_sequence_shape(&connection).unwrap();
        assert_eq!(
            capture_sequence_state(&connection).unwrap(),
            vec![SequenceStateReview {
                name: "real_sequence".to_owned(),
                sequence: 1,
            }]
        );
        connection
            .execute(
                "INSERT INTO sqlite_sequence(name,seq) VALUES ('unknown',1)",
                [],
            )
            .unwrap();
        assert_eq!(
            validate_sqlite_sequence_shape(&connection)
                .unwrap_err()
                .kind(),
            WorkspaceErrorCode::VerificationFailed
        );
        connection
            .execute("DELETE FROM sqlite_sequence WHERE name='unknown'", [])
            .unwrap();
        connection
            .execute(
                "INSERT INTO sqlite_sequence(name,seq) VALUES ('real_sequence',1)",
                [],
            )
            .unwrap();
        assert_eq!(
            validate_sqlite_sequence_shape(&connection)
                .unwrap_err()
                .kind(),
            WorkspaceErrorCode::VerificationFailed
        );
    }

    #[test]
    fn mixed_signature_inventory_never_gains_reconcile_authority() {
        let (_source_directory, source_path) =
            crate::tests::signed_fixture("reconcile-mixed-signature-source");
        let (_target_directory, target_path) =
            crate::tests::signed_fixture("reconcile-mixed-signature-target");
        make_content_reconcilable(&source_path, "manual", false);
        make_content_reconcilable(&target_path, "manual", false);
        let valid_source = VerifiedWorkspaceSource::open(&source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&target_path).unwrap();
        let summary = compare(&valid_source, &target);
        drop(valid_source);

        let connection = Connection::open(&source_path).unwrap();
        connection
            .execute(
                "INSERT INTO capsule_signature VALUES \
                 ('ed25519:sha256:0000000000000000000000000000000000000000000000000000000000000000',\
                  'ed25519',?1,?2,?3,'2026-08-09T00:00:00Z')",
                params![vec![0_u8; 32], vec![0_u8; 32], vec![0_u8; 64]],
            )
            .unwrap();
        drop(connection);
        let mixed_source = VerifiedWorkspaceSource::open(&source_path).unwrap();
        assert!(!mixed_source.has_complete_valid_signature_inventory());
        let error = prepare_reconcile_review(
            mixed_source,
            reopen(&target),
            &summary,
            &[],
            &BTreeSet::new(),
            &output(&target),
            &ReconcileReviewLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap_err();
        assert_eq!(error.kind(), WorkspaceErrorCode::InvalidSignature);
    }

    fn compare(
        source: &VerifiedWorkspaceSource,
        target: &VerifiedWorkspaceSource,
    ) -> CompareSummary {
        crate::compare_sources(
            source,
            target,
            &crate::CompareLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap()
    }

    fn detail(
        source: &VerifiedWorkspaceSource,
        target: &VerifiedWorkspaceSource,
        dataset: usize,
        table: usize,
        reveal_sensitive: bool,
    ) -> crate::CompareDetailPage {
        crate::comparison_detail_page(
            source,
            target,
            dataset,
            table,
            None,
            reveal_sensitive,
            &crate::CompareDetailLimits {
                page_size: 100,
                ..crate::CompareDetailLimits::default()
            },
            &CancellationToken::new(),
        )
        .unwrap()
    }

    fn prepare(
        source: &VerifiedWorkspaceSource,
        target: &VerifiedWorkspaceSource,
        summary: &CompareSummary,
        selections: &[ReconcileSelection],
        sensitive: bool,
    ) -> Result<ReconcileReview, WorkspaceError> {
        let confirmed = if sensitive {
            selections
                .iter()
                .map(|selection| selection.dataset_index)
                .collect()
        } else {
            BTreeSet::new()
        };
        prepare_reconcile_review(
            reopen(source),
            reopen(target),
            summary,
            selections,
            &confirmed,
            &output(target),
            &ReconcileReviewLimits::default(),
            &CancellationToken::new(),
        )
    }

    fn execute(review: ReconcileReview) -> PublishedReconcile {
        let plan = review.plan().clone();
        let payload = review.payload().to_vec();
        review
            .prepare_at(plan, &payload, approved_time())
            .unwrap()
            .stage()
            .unwrap()
            .transform_and_validate()
            .unwrap()
            .publish()
            .unwrap()
    }

    fn sequence(connection: &Connection, table: &str) -> i64 {
        connection
            .query_row(
                "SELECT seq FROM sqlite_sequence WHERE name=?1",
                [table],
                |row| row.get(0),
            )
            .unwrap()
    }

    fn assert_quarantine_evidence(output_path: &Path) {
        if output_path.exists() {
            VerifiedWorkspaceSource::open(output_path).unwrap();
        }
        let parent = output_path.parent().unwrap();
        assert!(fs::read_dir(parent).unwrap().any(|entry| {
            let name = entry.unwrap().file_name();
            let name = name.to_string_lossy();
            name.contains("failed") || name.contains("quarantine") || name.ends_with(".marker")
        }));
    }

    fn reopen(source: &VerifiedWorkspaceSource) -> VerifiedWorkspaceSource {
        VerifiedWorkspaceSource::open(&source.identity().canonical_path).unwrap()
    }

    fn recompute_report_digest(summary: &CompareSummary) -> String {
        let mut value = serde_json::to_value(summary).unwrap();
        value.as_object_mut().unwrap().remove("report_digest");
        lower_hex(&Sha256::digest(
            crate::plan::canonical_json(&value).unwrap(),
        ))
    }

    fn output(target: &VerifiedWorkspaceSource) -> ReconcileOutputRequest {
        ReconcileOutputRequest {
            output_path: target
                .identity()
                .canonical_path
                .with_file_name("reconcile-reviewed-output.sqlitecapsule"),
            plan_id: "33333333-3333-4333-8333-333333333333".to_owned(),
            created_at: "2026-08-13T08:00:00Z".to_owned(),
            expires_at: "2026-08-13T08:59:00Z".to_owned(),
        }
    }

    fn approved_time() -> SystemTime {
        SystemTime::UNIX_EPOCH
            + Duration::from_secs(
                crate::prepared_plan::parse_utc_seconds("2026-08-13T08:01:00Z").unwrap(),
            )
    }

    fn resign(connection: &Connection) {
        connection
            .execute("DELETE FROM capsule_signature", [])
            .unwrap();
        let digest = application_digest(connection).unwrap();
        let seed_text = DEVELOPMENT_SEED.trim();
        let mut seed = [0_u8; 32];
        for (index, byte) in seed.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&seed_text[index * 2..index * 2 + 2], 16).unwrap();
        }
        let key = SigningKey::from_bytes(&seed);
        seed.fill(0);
        let envelope =
            sign_digest_for_profile(&key, digest, "2026-08-08T12:34:56Z", PROFILE_V03).unwrap();
        connection
            .execute(
                "INSERT INTO capsule_signature VALUES (?1,'ed25519',?2,?3,?4,?5)",
                rusqlite::params![
                    envelope.key_id,
                    envelope.public_key.as_slice(),
                    envelope.application_digest.as_slice(),
                    envelope.signature.as_slice(),
                    envelope.signed_at
                ],
            )
            .unwrap();
    }

    fn make_content_reconcilable(path: &Path, policy: &str, sensitive: bool) {
        let connection = Connection::open(path).unwrap();
        connection
            .execute(
                "UPDATE capsule_dataset SET reconcile_policy=?1, sensitivity=?2 \
                 WHERE id='content'",
                params![policy, if sensitive { "sensitive" } else { "normal" }],
            )
            .unwrap();
        resign(&connection);
    }

    fn make_three_way_reconcilable(path: &Path, immutable_note: bool) {
        let connection = Connection::open(path).unwrap();
        connection
            .execute(
                "UPDATE capsule_dataset SET reconcile_policy='three-way' WHERE id='content'",
                [],
            )
            .unwrap();
        if immutable_note {
            connection
                .execute(
                    "UPDATE capsule_dataset_table SET immutable_columns_json='[\"id\",\"note\"]' \
                     WHERE table_name='vector_domain'",
                    [],
                )
                .unwrap();
        }
        resign(&connection);
    }

    fn make_table_ignored(path: &Path, column: &str) {
        let connection = Connection::open(path).unwrap();
        connection
            .execute(
                "UPDATE capsule_dataset_table SET ignored_columns_json=?1 \
                 WHERE table_name='vector_domain'",
                [format!("[\"{column}\"]")],
            )
            .unwrap();
        resign(&connection);
    }

    fn add_autoincrement_dataset_table(path: &Path) {
        let connection = Connection::open(path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE auto_domain (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     note TEXT NOT NULL
                 );
                 INSERT INTO capsule_dataset_table VALUES (
                     'content','auto_domain',1,'[\"id\"]','[]','[\"id\"]'
                 );",
            )
            .unwrap();
    }

    fn add_constrained_dataset_table(path: &Path) {
        let connection = Connection::open(path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE constrained_domain (
                     id INTEGER PRIMARY KEY,
                     unique_value TEXT NOT NULL UNIQUE,
                     low INTEGER NOT NULL,
                     high INTEGER NOT NULL,
                     CHECK (low < high)
                 );
                 INSERT INTO capsule_dataset_table VALUES (
                     'content','constrained_domain',1,'[\"id\"]','[]','[\"id\"]'
                 );",
            )
            .unwrap();
    }
}

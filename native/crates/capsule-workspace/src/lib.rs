//! Product-independent trusted workspace boundary for Capsule lifecycle work.
//!
//! A workspace source is pinned read-only, exhaustively verified through an
//! exact private snapshot, rebound to the pinned file, restricted to v0.3 and
//! backed by at least one cryptographically valid matching signature. Trust in
//! a publisher remains a separate host policy decision.

mod compact_copy;
mod compact_state;
mod compare;
mod compare_application;
mod compare_detail;
mod copy;
mod copy_source;
mod dataset;
mod error;
mod exact_copy;
mod lineage;
mod plan;
mod planner;
mod prepared_plan;
mod publication;
mod reconcile;
mod semantic_copy;
mod template_state;
mod upgrade;

use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub use compact_copy::{
    COMPACT_COPY_PREVIEW_PROFILE, CompactCopyPlanRequest, CompactCopyPreview, CompactCopyReview,
    CompactCopyStaging, PreparedCompactCopy, PublishedCompactCopy, ValidatedCompactCopy,
    generate_compact_copy_plan, parse_compact_copy_plan,
};
pub use compact_state::{
    COMPACT_LOGICAL_STATE_PROFILE, CompactLogicalState, VerifiedCompactSource,
};
pub use compare::{
    COMPARE_KEY_PROFILE, COMPARE_ROW_PROFILE, COMPARE_SUMMARY_PROFILE, CompareCompatibility,
    CompareCounts, CompareDataState, CompareDatasetSummary, CompareInputRef, CompareLimits,
    CompareLimitsApplied, ComparePublisherEvidence, CompareSection, CompareSectionState,
    CompareSignatureEvidence, CompareSignatureStatus, CompareSummary, CompareTableSummary,
    CompatibilityState, compare_sources,
};
pub use compare_application::{
    COMPARE_APPLICATION_PROFILE, CompareApplicationDetail, CompareApplicationFamily,
    CompareApplicationFamilyState, CompareApplicationFamilySummary, CompareApplicationLimits,
    CompareApplicationLimitsApplied, compare_application_detail,
};
pub use compare_detail::{
    COMPARE_PAGE_PROFILE, CompareCursor, CompareDetailFieldKind, CompareDetailLimits,
    CompareDetailPage, CompareDetailRowKind, CompareFieldDetail, CompareRowDetail,
    CompareStorageClass, CompareValueProjection, comparison_detail_page,
};
pub use copy::{
    COPY_PREVIEW_PROFILE, CopyAvailability, CopyBlocker, CopyBlockingReason, CopyDatasetAction,
    CopyDatasetChoice, CopyDatasetDecision, CopyFormatAvailability, CopyIdentityDisposition,
    CopyIdentityEffects, CopyInstanceProfileDisposition, CopyMode, CopyModeTruth,
    CopyMutableStateDisposition, CopyOutputConstraints, CopyPreviewLimits, CopyPreviewReport,
    CopyPreviewRequest, CopyPrompt, CopyPromptKind, CopyRowEstimate, DatasetChoiceDisposition,
    DigestExpectation, ExecutionAvailability, copy_mode_truth_table, preview_copy,
};
pub use copy_source::{
    COPY_SOURCE_PROFILE, CopySourceIdentity, CopySourceSignatureState, VerifiedCopySource,
};
pub use dataset::{
    ComparePolicy, DATA_CONTRACT_PROFILE, DataContract, Dataset, DatasetDependency, DatasetRole,
    DatasetTable, ForkPolicy, ReconcilePolicy, Sensitivity, UpgradePolicy,
};
pub use error::{ERROR_PROFILE, WorkspaceError, WorkspaceErrorCode};
pub use exact_copy::{
    EXACT_COPY_PREVIEW_PROFILE, ExactCopyPlanRequest, ExactCopyPreview, ExactCopyReview,
    ExactCopyStaging, PreparedExactCopy, PublishedExactCopy, ValidatedExactCopy,
    generate_exact_copy_plan, parse_exact_copy_plan,
};
pub use lineage::{
    LINEAGE_PROFILE, LineageEvent, LineageOperation, LineageParent, LineageReport, ParentRelation,
    ProvenanceStatus,
};
pub use plan::{
    DecisionScope, ExpectedOutput, InputRole, LifecyclePlan, MAX_PLAN_BYTES, Operation,
    PLAN_PROFILE, ParentFilesystemIdentity, PlanCapsuleIdentity, PlanDecision, PlanInput,
    PlanLimits, PlanOutput, SourceFilesystemIdentity,
};
pub use planner::{DuplicatePlanLimits, DuplicatePlanRequest, generate_duplicate_plan};
pub use prepared_plan::{PreparedPlan, PreparedPlanInput};
pub use publication::{PublishedCapsule, ValidatedWorkspaceOutput, WorkspaceStagingOutput};
pub use reconcile::{
    PreparedReconcileReview, PublishedReconcile, RECONCILE_REVIEW_PROFILE, ReconcileAction,
    ReconcileDatasetStateReview, ReconcileFieldReview, ReconcileLineageParentReview,
    ReconcileLineageRelation, ReconcileOperationBasis, ReconcileOperationReview,
    ReconcileOutputRequest, ReconcileOutputReview, ReconcileReference, ReconcileReview,
    ReconcileReviewLimits, ReconcileReviewLimitsApplied, ReconcileSelection, ReconcileStaging,
    ResolvedThreeWayConflictReview, ThreeWayConflictKind, ThreeWayConflictResolution,
    ThreeWayConflictReview, ThreeWayDeletedSide, ThreeWayReconcileReview, ThreeWayResolutionChoice,
    ValidatedReconcile, classify_three_way_reconcile, prepare_reconcile_review,
};
pub use semantic_copy::{
    PreparedSemanticCopy, PublishedSemanticCopy, SEMANTIC_COPY_PREVIEW_PROFILE,
    SemanticChoiceDisposition, SemanticCopyMode, SemanticCopyPlanRequest, SemanticCopyPreview,
    SemanticCopyReview, SemanticCopyStaging, SemanticDatasetAction, SemanticDatasetChoice,
    SemanticDatasetDecision, ValidatedSemanticCopy, generate_semantic_copy_plan,
    open_semantic_copy_source, parse_semantic_copy_plan,
};
use sqlite_capsule_core::CapsuleIdentity;
use sqlite_capsule_crypto::{SignatureVerification, application_digest, verify_signatures};
use sqlite_capsule_launch::{
    LaunchError, VerificationControl, VerifiedReadOnlyCapsule, verify_read_only_with_control,
};
use sqlite_capsule_lifecycle::{LifecycleError, PinnedSource, SourceIdentity};
pub use template_state::{
    DATASET_STATE_PROFILE, TEMPLATE_PLATFORM_RESET_PROFILE, TEMPLATE_STATE_DOC_SLUG,
    TEMPLATE_STATE_PROFILE, TemplateDatasetDisposition, TemplateDatasetProof, TemplateStateLimits,
    TemplateStateProof, verify_template_state,
};
pub use upgrade::{
    APPLICATION_UPGRADE_REVIEW_PROFILE, CapabilityDelta, PreparedUpgrade, PublishedUpgrade,
    UpgradeApproval, UpgradeDatasetAction, UpgradeDatasetDecision, UpgradeInputRef,
    UpgradePlanRequest, UpgradePublisherContinuity, UpgradeReview, UpgradeReviewReport,
    UpgradeStaging, ValidatedUpgrade, parse_upgrade_plan, prepare_upgrade_review,
};

const HARD_MAX_DEADLINE: Duration = Duration::from_secs(30);
const HARD_MAX_DATASETS: usize = 256;
const HARD_MAX_TABLES_TOTAL: usize = 4_096;
const HARD_MAX_TABLES_PER_DATASET: usize = 256;
const HARD_MAX_DEPENDENCIES_PER_DATASET: usize = 64;
const HARD_MAX_COLUMNS_PER_TABLE: usize = 256;
const HARD_MAX_PRIMARY_KEY_COLUMNS: usize = 16;
const HARD_MAX_IGNORED_COLUMNS: usize = 64;
const HARD_MAX_IMMUTABLE_COLUMNS: usize = 64;
const HARD_MAX_JSON_BYTES: usize = 16_384;
const HARD_MAX_JSON_DEPTH: usize = 4;
const HARD_MAX_LINEAGE_EVENTS: usize = 10_000;
const HARD_MAX_LINEAGE_PARENTS_PER_EVENT: usize = 8;
const HARD_MAX_LINEAGE_DETAILS_BYTES: usize = 16_384;
const HARD_MAX_LINEAGE_DETAILS_DEPTH: usize = 8;
const HARD_MAX_LINEAGE_DETAIL_PROPERTIES: usize = 64;

/// Caller operation budgets. Effective values are always clamped to the host
/// profile hard ceilings; capsule declarations cannot raise either set.
#[derive(Clone, Debug)]
pub struct WorkspaceLimits {
    pub deadline: Duration,
    pub max_capsule_bytes: u64,
    pub max_datasets: usize,
    pub max_tables_total: usize,
    pub max_tables_per_dataset: usize,
    pub max_dependencies_per_dataset: usize,
    pub max_columns_per_table: usize,
    pub max_primary_key_columns: usize,
    pub max_ignored_columns: usize,
    pub max_immutable_columns: usize,
    pub max_json_bytes: usize,
    pub max_json_depth: usize,
    pub max_lineage_events: usize,
    pub max_lineage_parents_per_event: usize,
    pub max_lineage_details_bytes: usize,
    pub max_lineage_details_depth: usize,
    pub max_lineage_detail_properties: usize,
}

impl Default for WorkspaceLimits {
    fn default() -> Self {
        Self {
            deadline: HARD_MAX_DEADLINE,
            max_capsule_bytes: sqlite_capsule_core::MAX_CAPSULE_BYTES,
            max_datasets: HARD_MAX_DATASETS,
            max_tables_total: HARD_MAX_TABLES_TOTAL,
            max_tables_per_dataset: HARD_MAX_TABLES_PER_DATASET,
            max_dependencies_per_dataset: HARD_MAX_DEPENDENCIES_PER_DATASET,
            max_columns_per_table: HARD_MAX_COLUMNS_PER_TABLE,
            max_primary_key_columns: HARD_MAX_PRIMARY_KEY_COLUMNS,
            max_ignored_columns: HARD_MAX_IGNORED_COLUMNS,
            max_immutable_columns: HARD_MAX_IMMUTABLE_COLUMNS,
            max_json_bytes: HARD_MAX_JSON_BYTES,
            max_json_depth: HARD_MAX_JSON_DEPTH,
            max_lineage_events: HARD_MAX_LINEAGE_EVENTS,
            max_lineage_parents_per_event: HARD_MAX_LINEAGE_PARENTS_PER_EVENT,
            max_lineage_details_bytes: HARD_MAX_LINEAGE_DETAILS_BYTES,
            max_lineage_details_depth: HARD_MAX_LINEAGE_DETAILS_DEPTH,
            max_lineage_detail_properties: HARD_MAX_LINEAGE_DETAIL_PROPERTIES,
        }
    }
}

#[derive(Clone, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    pub(crate) fn shared_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancelled)
    }
}

pub(crate) struct EffectiveLimits {
    max_capsule_bytes: u64,
    max_datasets: usize,
    max_tables_total: usize,
    max_tables_per_dataset: usize,
    max_dependencies_per_dataset: usize,
    max_columns_per_table: usize,
    max_primary_key_columns: usize,
    max_ignored_columns: usize,
    max_immutable_columns: usize,
    max_json_bytes: usize,
    max_json_depth: usize,
    max_lineage_events: usize,
    max_lineage_parents_per_event: usize,
    max_lineage_details_bytes: usize,
    max_lineage_details_depth: usize,
    max_lineage_detail_properties: usize,
}

impl EffectiveLimits {
    fn from_caller(limits: &WorkspaceLimits) -> Result<(Self, Duration), WorkspaceError> {
        let deadline = limits.deadline.min(HARD_MAX_DEADLINE);
        let effective = Self {
            max_capsule_bytes: limits
                .max_capsule_bytes
                .min(sqlite_capsule_core::MAX_CAPSULE_BYTES),
            max_datasets: limits.max_datasets.min(HARD_MAX_DATASETS),
            max_tables_total: limits.max_tables_total.min(HARD_MAX_TABLES_TOTAL),
            max_tables_per_dataset: limits
                .max_tables_per_dataset
                .min(HARD_MAX_TABLES_PER_DATASET),
            max_dependencies_per_dataset: limits
                .max_dependencies_per_dataset
                .min(HARD_MAX_DEPENDENCIES_PER_DATASET),
            max_columns_per_table: limits.max_columns_per_table.min(HARD_MAX_COLUMNS_PER_TABLE),
            max_primary_key_columns: limits
                .max_primary_key_columns
                .min(HARD_MAX_PRIMARY_KEY_COLUMNS),
            max_ignored_columns: limits.max_ignored_columns.min(HARD_MAX_IGNORED_COLUMNS),
            max_immutable_columns: limits.max_immutable_columns.min(HARD_MAX_IMMUTABLE_COLUMNS),
            max_json_bytes: limits.max_json_bytes.min(HARD_MAX_JSON_BYTES),
            max_json_depth: limits.max_json_depth.min(HARD_MAX_JSON_DEPTH),
            max_lineage_events: limits.max_lineage_events.min(HARD_MAX_LINEAGE_EVENTS),
            max_lineage_parents_per_event: limits
                .max_lineage_parents_per_event
                .min(HARD_MAX_LINEAGE_PARENTS_PER_EVENT),
            max_lineage_details_bytes: limits
                .max_lineage_details_bytes
                .min(HARD_MAX_LINEAGE_DETAILS_BYTES),
            max_lineage_details_depth: limits
                .max_lineage_details_depth
                .min(HARD_MAX_LINEAGE_DETAILS_DEPTH),
            max_lineage_detail_properties: limits
                .max_lineage_detail_properties
                .min(HARD_MAX_LINEAGE_DETAIL_PROPERTIES),
        };
        if deadline.is_zero()
            || effective.max_capsule_bytes == 0
            || effective.max_datasets == 0
            || effective.max_tables_total == 0
            || effective.max_tables_per_dataset == 0
            || effective.max_columns_per_table == 0
            || effective.max_primary_key_columns == 0
            || effective.max_json_bytes == 0
            || effective.max_json_depth == 0
            || effective.max_lineage_details_bytes == 0
            || effective.max_lineage_details_depth == 0
            || effective.max_lineage_detail_properties == 0
        {
            return Err(WorkspaceError::new(WorkspaceErrorCode::LimitExceeded));
        }
        Ok((effective, deadline))
    }
}

pub(crate) struct WorkspaceControl {
    deadline: Instant,
    cancellation: CancellationToken,
}

impl WorkspaceControl {
    fn new(budget: Duration, cancellation: &CancellationToken) -> Self {
        Self {
            deadline: Instant::now() + budget,
            cancellation: cancellation.clone(),
        }
    }

    fn remaining(&self) -> Result<Duration, WorkspaceError> {
        self.check()?;
        Ok(self.deadline.saturating_duration_since(Instant::now()))
    }

    pub(crate) fn check(&self) -> Result<(), WorkspaceError> {
        if self.cancellation.is_cancelled() {
            return Err(WorkspaceError::new(WorkspaceErrorCode::Cancelled));
        }
        if Instant::now() >= self.deadline {
            return Err(WorkspaceError::new(WorkspaceErrorCode::LimitExceeded));
        }
        Ok(())
    }

    pub(crate) fn install(&self, connection: &rusqlite::Connection) -> Result<(), WorkspaceError> {
        self.check()?;
        let deadline = self.deadline;
        let cancelled = Arc::clone(&self.cancellation.cancelled);
        connection
            .progress_handler(
                1_000,
                Some(move || cancelled.load(Ordering::Relaxed) || Instant::now() >= deadline),
            )
            .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InvalidContract))
    }
}

pub struct VerifiedWorkspaceSource {
    pinned: PinnedSource,
    pub(crate) verified: VerifiedReadOnlyCapsule,
    application_digest: [u8; 32],
    signature_reports: Vec<SignatureVerification>,
    data_contract: DataContract,
    lineage: LineageReport,
}

impl VerifiedWorkspaceSource {
    pub fn open(path: &Path) -> Result<Self, WorkspaceError> {
        Self::open_with_control(path, &WorkspaceLimits::default(), &CancellationToken::new())
    }

    pub fn open_with_control(
        path: &Path,
        limits: &WorkspaceLimits,
        cancellation: &CancellationToken,
    ) -> Result<Self, WorkspaceError> {
        Self::open_with_control_expected_binding(path, limits, cancellation, None, None)
    }

    pub(crate) fn open_with_control_expected_binding(
        path: &Path,
        limits: &WorkspaceLimits,
        cancellation: &CancellationToken,
        expected_size: Option<u64>,
        expected_sha256: Option<[u8; 32]>,
    ) -> Result<Self, WorkspaceError> {
        let (limits, deadline) = EffectiveLimits::from_caller(limits)?;
        let control = WorkspaceControl::new(deadline, cancellation);
        control.check()?;
        let pinned = PinnedSource::open(path, false).map_err(map_pin_open_error)?;
        if expected_size.is_some_and(|expected| pinned.identity().bytes != expected) {
            return Err(stale_plan());
        }
        let verification_control =
            VerificationControl::new(control.remaining()?, Arc::clone(&cancellation.cancelled))
                .with_max_bytes(limits.max_capsule_bytes);
        if let Some(expected) = expected_sha256 {
            sqlite_capsule_launch::assert_source_binding_with_control(
                pinned.canonical_path(),
                &expected,
                &verification_control,
            )
            .map_err(|error| map_launch_error(error, &control))?;
        }
        let verified =
            verify_read_only_with_control(pinned.canonical_path(), &verification_control)
                .map_err(|error| map_launch_error(error, &control))?;
        pinned.assert_current().map_err(|_| stale_plan())?;
        verified
            .assert_source_current_with_control(&verification_control)
            .map_err(|error| map_launch_error(error, &control))?;

        let identity = &verified.identity;
        if identity.application_id != sqlite_capsule_core::SQLITE_CAPSULE_APPLICATION_ID
            || identity.user_version != 3
            || identity.format_id != "org.sqlite-capsule"
            || identity.format_version != "0.3"
            || identity.runtime_protocol != "capsule-http/0.2"
        {
            return Err(WorkspaceError::new(WorkspaceErrorCode::UnsupportedFormat));
        }

        let _verification_guard = verified
            .start_control(&verification_control)
            .map_err(|error| map_launch_error(error, &control))?;
        control.install(verified.connection())?;
        let application_digest = application_digest(verified.connection())
            .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InvalidSignature))?;
        let reports = verify_signatures(verified.connection());
        let _ = verified
            .connection()
            .progress_handler(0, None::<fn() -> bool>);
        control.check()?;
        let reports =
            reports.map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InvalidSignature))?;
        if !reports
            .iter()
            .any(|report| report.cryptographically_valid && report.digest_matches)
        {
            return Err(WorkspaceError::new(WorkspaceErrorCode::InvalidSignature));
        }

        let schema = identity
            .overview
            .data_schema
            .as_ref()
            .ok_or_else(|| WorkspaceError::new(WorkspaceErrorCode::InvalidContract))?;
        let data_contract = dataset::load(
            verified.connection(),
            &identity.app_id,
            &schema.data_schema_id,
            schema.data_schema_version,
            &limits,
            &control,
        )?;
        let lineage = lineage::load(
            verified.connection(),
            &identity.capsule_id,
            &limits,
            &control,
        )?;

        // Bind signature evidence to the same verified snapshot while also
        // ensuring the originally pinned live source has not changed.
        pinned.assert_current().map_err(|_| stale_plan())?;
        verified
            .assert_source_current_with_control(&verification_control)
            .map_err(|error| map_launch_error(error, &control))?;
        Ok(Self {
            pinned,
            verified,
            application_digest,
            signature_reports: reports,
            data_contract,
            lineage,
        })
    }

    pub fn identity(&self) -> &CapsuleIdentity {
        &self.verified.identity
    }

    pub fn source_identity(&self) -> &SourceIdentity {
        self.pinned.identity()
    }

    pub fn source_sha256(&self) -> String {
        self.verified
            .source_sha256
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    pub fn assert_source_binding(
        &self,
        expected_identity: &SourceIdentity,
        expected_sha256: &str,
    ) -> Result<(), WorkspaceError> {
        self.assert_current()?;
        if self.source_identity() == expected_identity && self.source_sha256() == expected_sha256 {
            Ok(())
        } else {
            Err(stale_plan())
        }
    }

    pub fn has_complete_valid_signature_inventory(&self) -> bool {
        !self.signature_reports.is_empty()
            && self
                .signature_reports
                .iter()
                .all(|report| report.cryptographically_valid && report.digest_matches)
    }

    /// Bounded cryptographic key identifiers from the retained verified
    /// snapshot. These are evidence for a separate host-policy trust decision;
    /// they do not confer trust by themselves.
    pub fn valid_signature_key_ids(&self) -> Vec<String> {
        self.signature_reports
            .iter()
            .filter(|report| report.cryptographically_valid && report.digest_matches)
            .map(|report| report.key_id.clone())
            .collect()
    }

    pub fn data_contract(&self) -> &DataContract {
        &self.data_contract
    }

    pub fn lineage(&self) -> &LineageReport {
        &self.lineage
    }

    pub(crate) fn application_digest(&self) -> &[u8; 32] {
        &self.application_digest
    }

    pub(crate) fn signature_reports(&self) -> &[SignatureVerification] {
        &self.signature_reports
    }

    pub fn assert_current(&self) -> Result<(), WorkspaceError> {
        self.pinned.assert_current().map_err(|_| stale_plan())?;
        self.verified
            .assert_source_current()
            .map_err(map_launch_error_without_control)
    }

    pub fn assert_current_with_control(
        &self,
        limits: &WorkspaceLimits,
        cancellation: &CancellationToken,
    ) -> Result<(), WorkspaceError> {
        let (_, deadline) = EffectiveLimits::from_caller(limits)?;
        let control = WorkspaceControl::new(deadline, cancellation);
        self.pinned.assert_current().map_err(|_| stale_plan())?;
        let verification_control =
            VerificationControl::new(control.remaining()?, Arc::clone(&cancellation.cancelled));
        self.verified
            .assert_source_current_with_control(&verification_control)
            .map_err(|error| map_launch_error(error, &control))
    }
}

fn map_pin_open_error(error: LifecycleError) -> WorkspaceError {
    match error {
        LifecycleError::ChangedDuringOpen | LifecycleError::Replaced => stale_plan(),
        LifecycleError::NotRegularFile | LifecycleError::SymbolicLink => {
            WorkspaceError::new(WorkspaceErrorCode::InvalidCapsule)
        }
        _ => WorkspaceError::new(WorkspaceErrorCode::InternalError),
    }
}

fn map_launch_error(error: LaunchError, control: &WorkspaceControl) -> WorkspaceError {
    if let Err(control_error) = control.check() {
        return control_error;
    }
    map_launch_error_without_control(error)
}

fn map_launch_error_without_control(error: LaunchError) -> WorkspaceError {
    match error {
        LaunchError::SourceRace => stale_plan(),
        LaunchError::SourceSidecar => {
            WorkspaceError::new(WorkspaceErrorCode::SourceJournalStateUnsupported)
        }
        LaunchError::Cancelled => WorkspaceError::new(WorkspaceErrorCode::Cancelled),
        LaunchError::LimitExceeded => WorkspaceError::new(WorkspaceErrorCode::LimitExceeded),
        LaunchError::Inspect(sqlite_capsule_core::InspectError::UnsupportedFormat { .. }) => {
            WorkspaceError::new(WorkspaceErrorCode::UnsupportedFormat)
        }
        _ => WorkspaceError::new(WorkspaceErrorCode::InvalidCapsule),
    }
}

const fn stale_plan() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::StalePlan)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use ed25519_dalek::SigningKey;
    use rusqlite::Connection;
    use sha2::{Digest, Sha256};
    use sqlite_capsule_crypto::{PROFILE_V03, application_digest, sign_digest_for_profile};
    use tempfile::TempDir;

    use super::*;

    const V03_SCHEMA: &str = include_str!("../../../../format/capsule-v0.3.sql");
    const SIGNED_SCHEMA: &str = include_str!("../../../../format/capsule-signed-app-v0.3.sql");
    const SIGNED_FIXTURE: &str =
        include_str!("../../../../compatibility/signed-app-v0.3/fixture-v0.3.sql");
    const TEMPLATE_TYPE_SPECTRUM: &str =
        include_str!("../../../../compatibility/template-state-v1/type-spectrum.sql");
    const DEVELOPMENT_SEED: &str =
        include_str!("../../../../compatibility/signed-app-v0.2/development-seed.hex");

    pub(crate) fn signed_fixture(name: &str) -> (TempDir, std::path::PathBuf) {
        fixture_from_sources(name, V03_SCHEMA, &stable_fixture_source())
    }

    fn stable_fixture_source() -> String {
        SIGNED_FIXTURE.to_owned()
    }

    fn fixture_from_sources(
        name: &str,
        format_schema: &str,
        signed_fixture: &str,
    ) -> (TempDir, std::path::PathBuf) {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join(format!("{name}.sqlitecapsule"));
        let connection = Connection::open(&path).expect("create fixture");
        connection
            .execute_batch(format_schema)
            .expect("v0.3 schema");
        connection
            .execute_batch(SIGNED_SCHEMA)
            .expect("signed schema");
        connection
            .execute_batch(signed_fixture)
            .expect("signed fixture");
        resign(&connection);
        drop(connection);
        (directory, path)
    }

    fn mutate_and_resign(name: &str, sql: &str) -> (TempDir, std::path::PathBuf) {
        let (directory, path) = signed_fixture(name);
        let connection = Connection::open(&path).expect("open fixture for mutation");
        connection.execute_batch(sql).expect("mutate fixture");
        resign(&connection);
        drop(connection);
        (directory, path)
    }

    fn mutate_mutable(name: &str, sql: &str) -> (TempDir, std::path::PathBuf) {
        let (directory, path) = signed_fixture(name);
        let connection = Connection::open(&path).expect("open mutable fixture");
        connection.execute_batch(sql).expect("mutate excluded rows");
        drop(connection);
        (directory, path)
    }

    fn domain_primary_key_fixture(
        name: &str,
        table_definition: &str,
        primary_key_json: &str,
        immutable_columns_json: &str,
        integer_id: bool,
    ) -> (TempDir, std::path::PathBuf) {
        let mut fixture = stable_fixture_source();
        let start = fixture
            .find("CREATE TABLE vector_domain (")
            .expect("domain table start");
        let relative_end = fixture[start..]
            .find(");\n\nCREATE TABLE vector_settings")
            .expect("domain table end");
        fixture.replace_range(start..start + relative_end + 2, table_definition);
        if integer_id {
            fixture = fixture.replacen(
                "INSERT INTO vector_domain VALUES ('domain',",
                "INSERT INTO vector_domain VALUES (1,",
                1,
            );
        }
        let (directory, path) = fixture_from_sources(name, V03_SCHEMA, &fixture);
        let connection = Connection::open(&path).expect("open primary-key fixture");
        connection
            .execute(
                "UPDATE capsule_dataset_table \
                 SET primary_key_json = ?1, immutable_columns_json = ?2 \
                 WHERE table_name = 'vector_domain'",
                [primary_key_json, immutable_columns_json],
            )
            .expect("declare fixture primary key");
        resign(&connection);
        drop(connection);
        (directory, path)
    }

    fn resign(connection: &Connection) {
        connection
            .execute("DELETE FROM capsule_signature", [])
            .expect("remove old signature");
        let digest = application_digest(connection).expect("application digest");
        let seed_text = DEVELOPMENT_SEED.trim();
        assert_eq!(seed_text.len(), 64);
        let mut seed = [0_u8; 32];
        for (index, byte) in seed.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&seed_text[index * 2..index * 2 + 2], 16)
                .expect("development seed hex");
        }
        let key = SigningKey::from_bytes(&seed);
        seed.fill(0);
        let envelope = sign_digest_for_profile(&key, digest, "2026-08-08T12:34:56Z", PROFILE_V03)
            .expect("sign mutated fixture");
        connection
            .execute(
                "INSERT INTO capsule_signature \
                 (key_id, algorithm, public_key, application_digest, signature, signed_at) \
                 VALUES (?1, 'ed25519', ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    envelope.key_id,
                    envelope.public_key.as_slice(),
                    envelope.application_digest.as_slice(),
                    envelope.signature.as_slice(),
                    envelope.signed_at,
                ],
            )
            .expect("store replacement signature");
    }

    fn template_proof_value(source: &VerifiedWorkspaceSource) -> serde_json::Value {
        let datasets = source
            .data_contract()
            .datasets
            .iter()
            .map(|dataset| {
                let (stored_row_count, state_sha256) =
                    crate::template_state::dataset_state_for_test(source, dataset)
                        .expect("dataset state");
                serde_json::json!({
                    "dataset_id": dataset.id,
                    "disposition": if stored_row_count == 0 { "empty" } else { "seed" },
                    "stored_row_count": stored_row_count,
                    "state_sha256": state_sha256
                })
            })
            .collect::<Vec<_>>();
        let identity = source.identity();
        let schema = identity.overview.data_schema.as_ref().expect("schema");
        serde_json::json!({
            "profile": TEMPLATE_STATE_PROFILE,
            "app_id": identity.app_id,
            "app_version": identity.app_version,
            "data_schema_id": schema.data_schema_id,
            "data_schema_version": schema.data_schema_version,
            "dataset_state_profile": DATASET_STATE_PROFILE,
            "mutable_platform_state_profile": TEMPLATE_PLATFORM_RESET_PROFILE,
            "datasets": datasets
        })
    }

    fn install_template_proof(path: &std::path::Path, proof: &[u8]) {
        let proof = std::str::from_utf8(proof).expect("template proof UTF-8");
        let connection = Connection::open(path).expect("open template proof fixture");
        connection
            .execute(
                "INSERT OR REPLACE INTO capsule_doc (slug, title, media_type, content, sequence) \
                 VALUES (?1, 'SQLite Capsule authenticated template state', \
                 'application/vnd.sqlite-capsule.template-state+json', ?2, 0)",
                rusqlite::params![TEMPLATE_STATE_DOC_SLUG, proof],
            )
            .expect("install template proof");
        resign(&connection);
    }

    fn canonical_template_proof(path: &std::path::Path) -> Vec<u8> {
        let source = VerifiedWorkspaceSource::open(path).expect("source before template proof");
        crate::plan::canonical_json(&template_proof_value(&source)).expect("canonical proof")
    }

    fn open_error(path: &Path) -> WorkspaceErrorCode {
        match VerifiedWorkspaceSource::open(path) {
            Ok(_) => panic!("hostile fixture must be rejected"),
            Err(error) => error.kind(),
        }
    }

    #[test]
    fn opens_only_a_pinned_verified_signed_v03_snapshot() {
        let (_directory, path) = signed_fixture("valid");
        let source = VerifiedWorkspaceSource::open(&path).expect("workspace source");
        assert_eq!(source.identity().user_version, 3);
        assert_eq!(source.identity().format_version, "0.3");
        assert_eq!(source.data_contract().datasets.len(), 2);
        assert_eq!(source.data_contract().datasets[0].id, "content");
        assert_eq!(source.lineage().events.len(), 1);
        assert_eq!(
            source.lineage().provenance_status,
            ProvenanceStatus::MutableUntrusted
        );
        source.assert_current().expect("source still bound");
    }

    #[test]
    fn duplicate_plan_is_deterministic_review_data_and_prepares_without_writes() {
        let (directory, path) = signed_fixture("duplicate-plan");
        let output = directory.path().join("Duplicate.capsule.sqlite");
        let source = VerifiedWorkspaceSource::open(&path).expect("verified source");
        let request = || DuplicatePlanRequest {
            output_path: &output,
            plan_id: "a8bc8b65-a691-4c76-909f-e481e93b830e",
            created_at: "2026-08-12T12:00:00Z",
            expires_at: "2026-08-12T12:05:00Z",
            limits: DuplicatePlanLimits {
                max_input_bytes: u64::MAX,
                max_output_bytes: u64::MAX,
                max_rows_inspected: u64::MAX,
                max_rows_written: u64::MAX,
                deadline: Duration::from_secs(3_600),
            },
        };
        let first = generate_duplicate_plan(&source, &request()).expect("first plan");
        let second = generate_duplicate_plan(&source, &request()).expect("same plan again");
        let first_bytes = first.canonical_bytes().expect("canonical first plan");
        let second_bytes = second.canonical_bytes().expect("canonical second plan");
        assert_eq!(first_bytes, second_bytes);
        assert!(!output.exists(), "planning must not create the output");

        let value: serde_json::Value = serde_json::from_slice(&first_bytes).expect("review JSON");
        assert_eq!(value["operation"], "duplicate");
        assert_eq!(value["inputs"][0]["capsule"]["format_version"], "0.3");
        assert!(value["inputs"][0]["capsule"]["publisher_key_id"].is_string());
        assert_eq!(
            value["inputs"][0]["file_sha256"],
            value["inputs"][0]["snapshot_sha256"]
        );
        assert_eq!(value["limits"]["max_input_bytes"], 64 * 1024 * 1024);
        assert_eq!(value["limits"]["max_output_bytes"], 64 * 1024 * 1024);
        assert_eq!(value["limits"]["max_rows_inspected"], 100_000);
        assert_eq!(value["limits"]["max_rows_written"], 100_000);
        assert_eq!(value["limits"]["deadline_ms"], 30_000);

        let parsed = LifecyclePlan::parse(&first_bytes).expect("parse generated review plan");
        let now = std::time::UNIX_EPOCH
            + Duration::from_secs(
                crate::prepared_plan::parse_utc_seconds("2026-08-12T12:00:01Z")
                    .expect("operation time"),
            );
        let prepared = PreparedPlan::prepare_at(
            parsed,
            now,
            &WorkspaceLimits::default(),
            &CancellationToken::new(),
        )
        .expect("generated plan immediately prepares");
        assert_eq!(prepared.plan().operation(), Operation::Duplicate);
        assert!(!output.exists(), "preparation also must not create output");
        drop(prepared);
        assert!(!output.exists());
    }

    #[test]
    fn recomputed_duplicate_plan_edits_never_gain_execution_authority() {
        use crate::plan::canonical_digest_value;

        let (directory, path) = signed_fixture("duplicate-plan-hostile-edits");
        let source = VerifiedWorkspaceSource::open(&path).expect("verified source");
        let output = directory.path().join("hostile-edits.capsule.sqlite");
        let plan = generate_duplicate_plan(
            &source,
            &DuplicatePlanRequest {
                output_path: &output,
                plan_id: "b41ddc60-a06c-4c40-9b2c-31d4a45c41dd",
                created_at: "2026-08-12T12:00:00Z",
                expires_at: "2026-08-12T12:05:00Z",
                limits: DuplicatePlanLimits::default(),
            },
        )
        .expect("review plan");
        let base: serde_json::Value =
            serde_json::from_slice(&plan.canonical_bytes().expect("canonical plan"))
                .expect("review JSON");
        let now = std::time::UNIX_EPOCH
            + Duration::from_secs(
                crate::prepared_plan::parse_utc_seconds("2026-08-12T12:00:01Z")
                    .expect("operation time"),
            );

        for (name, expected_code) in [
            ("operation", WorkspaceErrorCode::UnsupportedOperation),
            ("target-role", WorkspaceErrorCode::InvalidContract),
            ("extra-input", WorkspaceErrorCode::InvalidContract),
            ("decision-action", WorkspaceErrorCode::InvalidContract),
            ("decision-subject", WorkspaceErrorCode::InvalidContract),
            ("decision-parameters", WorkspaceErrorCode::InvalidContract),
            ("expected-capsule", WorkspaceErrorCode::InvalidContract),
            ("expected-revision", WorkspaceErrorCode::InvalidContract),
            ("expected-schema", WorkspaceErrorCode::InvalidContract),
            ("row-cap", WorkspaceErrorCode::InvalidContract),
            ("declared-size", WorkspaceErrorCode::StalePlan),
        ] {
            let mut value = base.clone();
            match name {
                "operation" => value["operation"] = serde_json::json!("fork"),
                "target-role" => value["inputs"][0]["role"] = serde_json::json!("target"),
                "extra-input" => {
                    let duplicate = value["inputs"][0].clone();
                    value["inputs"].as_array_mut().unwrap().push(duplicate);
                }
                "decision-action" => value["decisions"][0]["action"] = serde_json::json!("replace"),
                "decision-subject" => {
                    value["decisions"][0]["subject"] = serde_json::json!("org.example.other")
                }
                "decision-parameters" => {
                    value["decisions"][0]["parameters"] = serde_json::json!({"unreviewed": 1})
                }
                "expected-capsule" => {
                    value["expected"]["capsule_id"] =
                        serde_json::json!("e324112d-e65f-497d-8ad2-13dbef74ea93")
                }
                "expected-revision" => {
                    value["expected"]["revision_id"] =
                        serde_json::json!("4e39c3dc-d63b-4452-ae32-c9f937fc776f")
                }
                "expected-schema" => {
                    value["expected"]["data_schema_id"] = serde_json::json!("other-schema")
                }
                "row-cap" => value["limits"]["max_rows_inspected"] = serde_json::json!(100_001),
                "declared-size" => {
                    let size = value["inputs"][0]["size_bytes"].as_u64().unwrap();
                    value["inputs"][0]["size_bytes"] = serde_json::json!(size + 1);
                }
                _ => unreachable!(),
            }
            let digest = canonical_digest_value(&value).expect("recomputed digest");
            value["plan_digest"] = serde_json::Value::String(digest);
            let bytes = serde_json::to_vec(&value).expect("edited plan bytes");
            let parsed = LifecyclePlan::parse(&bytes).expect("edited plan parses");
            let error = match PreparedPlan::prepare_at(
                parsed,
                now,
                &WorkspaceLimits::default(),
                &CancellationToken::new(),
            ) {
                Ok(_) => panic!("edited plan must not acquire authority"),
                Err(error) => error,
            };
            assert_eq!(error.kind(), expected_code, "hostile edit {name}");
            assert!(!output.exists(), "hostile edit {name} created output");
        }

        let mut lowered = base;
        lowered["limits"]["max_rows_inspected"] = serde_json::json!(1);
        lowered["limits"]["max_rows_written"] = serde_json::json!(1);
        let digest = canonical_digest_value(&lowered).expect("lowered plan digest");
        lowered["plan_digest"] = serde_json::Value::String(digest);
        let lowered =
            LifecyclePlan::parse(&serde_json::to_vec(&lowered).expect("lowered plan bytes"))
                .expect("lowered plan parses");
        PreparedPlan::prepare_at(
            lowered,
            now,
            &WorkspaceLimits::default(),
            &CancellationToken::new(),
        )
        .expect("positive lowered zero-row duplicate budgets prepare");
        assert!(!output.exists());
    }

    #[test]
    fn duplicate_planner_refuses_existing_output_without_changing_it() {
        let (directory, path) = signed_fixture("duplicate-plan-existing");
        let output = directory.path().join("existing.capsule.sqlite");
        fs::write(&output, b"pre-existing destination").expect("existing destination");
        let source = VerifiedWorkspaceSource::open(&path).expect("verified source");
        let error = generate_duplicate_plan(
            &source,
            &DuplicatePlanRequest {
                output_path: &output,
                plan_id: "89300936-61f4-4ebd-aa46-0724435a9ed0",
                created_at: "2026-08-12T12:00:00Z",
                expires_at: "2026-08-12T12:05:00Z",
                limits: DuplicatePlanLimits::default(),
            },
        )
        .expect_err("existing destination must be rejected");
        assert_eq!(error.kind(), WorkspaceErrorCode::DestinationExists);
        assert_eq!(
            fs::read(&output).expect("destination remains readable"),
            b"pre-existing destination"
        );
    }

    #[test]
    fn caller_limits_and_cancellation_apply_to_the_complete_open() {
        let (_directory, path) = signed_fixture("controlled");
        let token = CancellationToken::new();
        token.cancel();
        let error = match VerifiedWorkspaceSource::open_with_control(
            &path,
            &WorkspaceLimits::default(),
            &token,
        ) {
            Ok(_) => panic!("cancelled open must fail"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::Cancelled);

        let limits = WorkspaceLimits {
            max_datasets: 1,
            ..WorkspaceLimits::default()
        };
        let error = match VerifiedWorkspaceSource::open_with_control(
            &path,
            &limits,
            &CancellationToken::new(),
        ) {
            Ok(_) => panic!("caller dataset limit must fail"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::LimitExceeded);

        let limits = WorkspaceLimits {
            max_lineage_events: 0,
            ..WorkspaceLimits::default()
        };
        let error = match VerifiedWorkspaceSource::open_with_control(
            &path,
            &limits,
            &CancellationToken::new(),
        ) {
            Ok(_) => panic!("caller lineage limit must fail"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::LimitExceeded);

        let limits = WorkspaceLimits {
            deadline: Duration::ZERO,
            ..WorkspaceLimits::default()
        };
        let error = match VerifiedWorkspaceSource::open_with_control(
            &path,
            &limits,
            &CancellationToken::new(),
        ) {
            Ok(_) => panic!("zero deadline must fail before inspection"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::LimitExceeded);
    }

    #[test]
    fn source_journal_state_has_its_own_stable_code() {
        let (_directory, path) = signed_fixture("journal-sidecar");
        let mut sidecar_name = path.as_os_str().to_os_string();
        sidecar_name.push("-wal");
        fs::write(
            std::path::PathBuf::from(sidecar_name),
            b"hostile journal state",
        )
        .expect("create adjacent WAL");
        assert_eq!(
            open_error(&path),
            WorkspaceErrorCode::SourceJournalStateUnsupported
        );
    }

    #[test]
    fn mutable_lineage_details_are_redacted_and_do_not_authenticate_publishers() {
        let raw_details = r#"{"private":"secret-row-value","nested":{"count":1}}"#;
        let (_directory, path) = signed_fixture("lineage-redaction");
        let connection = Connection::open(&path).expect("open mutable lineage fixture");
        connection
            .execute(
                "UPDATE capsule_lineage_event SET details_json = ?1",
                [raw_details],
            )
            .expect("mutate unsigned details");
        connection
            .execute(
                "UPDATE capsule_instance SET revision_id = \
                 '44444444-4444-4444-8444-444444444444' WHERE id = 1",
                [],
            )
            .expect("advance instance without lifecycle event");
        drop(connection);

        let source = VerifiedWorkspaceSource::open(&path)
            .expect("mutable lineage remains valid and signature still matches");
        assert_ne!(
            source.identity().overview.instance.revision_id.as_deref(),
            Some(source.lineage().events[0].result_revision_id.as_str())
        );
        let expected = Sha256::digest(raw_details.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(source.lineage().events[0].details_sha256, expected);
        let report = serde_json::to_string(source.lineage()).expect("serialize lineage report");
        assert!(report.contains("mutable-untrusted"));
        assert!(report.contains("details_sha256"));
        assert!(!report.contains("secret-row-value"));
        assert!(!report.contains("details_json"));
        assert!(!report.contains("publisher_authenticated"));
    }

    #[test]
    fn malformed_mutable_lineage_maps_to_invalid_capsule() {
        let cases = [
            (
                "lineage-sequence-gap",
                "UPDATE capsule_lineage_event SET sequence = 2",
            ),
            (
                "lineage-event-uuid",
                "UPDATE capsule_lineage_event SET event_id = \
                 '33333333-3333-6333-8333-333333333333'",
            ),
            (
                "lineage-event-uuid-length",
                "UPDATE capsule_lineage_event SET event_id = 'not-a-uuid'",
            ),
            (
                "lineage-result-uuid",
                "UPDATE capsule_lineage_event SET result_revision_id = \
                 '22222222-2222-6222-8222-222222222222'",
            ),
            (
                "lineage-calendar-time",
                "UPDATE capsule_lineage_event SET occurred_at = '2023-02-29T00:00:00Z'",
            ),
            (
                "lineage-calendar-year-zero",
                "UPDATE capsule_lineage_event SET occurred_at = '0000-01-01T00:00:00Z'",
            ),
            (
                "lineage-operation",
                "PRAGMA ignore_check_constraints=ON; \
                 UPDATE capsule_lineage_event SET operation = 'execute-script'; \
                 PRAGMA ignore_check_constraints=OFF",
            ),
            (
                "lineage-hash",
                "PRAGMA ignore_check_constraints=ON; \
                 UPDATE capsule_lineage_event SET plan_digest = \
                 'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA'; \
                 PRAGMA ignore_check_constraints=OFF",
            ),
            (
                "lineage-schema-version",
                "PRAGMA ignore_check_constraints=ON; \
                 UPDATE capsule_lineage_event SET data_schema_version = 0; \
                 PRAGMA ignore_check_constraints=OFF",
            ),
            (
                "lineage-details-shape",
                "UPDATE capsule_lineage_event SET details_json = '[]'",
            ),
        ];
        for (name, mutation) in cases {
            let (_directory, path) = mutate_mutable(name, mutation);
            assert_eq!(
                open_error(&path),
                WorkspaceErrorCode::InvalidCapsule,
                "mutable lineage case {name}"
            );
        }
    }

    #[test]
    fn lineage_parent_ordinals_relations_and_caps_fail_closed() {
        let parent = "('33333333-3333-4333-8333-333333333333', 2, 'created-from', \
                      NULL, NULL, \
                      'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc')";
        let (_directory, path) = mutate_mutable(
            "parent-ordinal-gap",
            &format!("INSERT INTO capsule_lineage_parent VALUES {parent}"),
        );
        assert_eq!(open_error(&path), WorkspaceErrorCode::InvalidCapsule);

        let (_directory, path) = mutate_mutable(
            "parent-relation",
            "PRAGMA ignore_check_constraints=ON; \
             INSERT INTO capsule_lineage_parent VALUES \
             ('33333333-3333-4333-8333-333333333333', 1, 'publisher-says-so', \
              NULL, NULL, \
              'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc'); \
             PRAGMA ignore_check_constraints=OFF",
        );
        assert_eq!(open_error(&path), WorkspaceErrorCode::InvalidCapsule);

        let (_directory, path) = mutate_mutable(
            "parent-invalid-optional-id",
            "INSERT INTO capsule_lineage_parent VALUES \
             ('33333333-3333-4333-8333-333333333333', 1, 'created-from', \
              'not-a-uuid', NULL, \
              'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc')",
        );
        assert_eq!(open_error(&path), WorkspaceErrorCode::InvalidCapsule);

        let (_directory, path) = signed_fixture("too-many-parents");
        let connection = Connection::open(&path).expect("open parent limit fixture");
        connection
            .execute_batch("PRAGMA ignore_check_constraints=ON")
            .expect("allow hostile ordinal");
        for ordinal in 1..=9 {
            connection
                .execute(
                    "INSERT INTO capsule_lineage_parent VALUES \
                     ('33333333-3333-4333-8333-333333333333', ?1, 'created-from', \
                      NULL, NULL, \
                      'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc')",
                    [ordinal],
                )
                .expect("insert hostile parent");
        }
        connection
            .execute_batch("PRAGMA ignore_check_constraints=OFF")
            .expect("restore constraints");
        drop(connection);
        assert_eq!(open_error(&path), WorkspaceErrorCode::LimitExceeded);
    }

    #[test]
    fn lineage_details_depth_properties_and_bytes_are_bounded() {
        let (_directory, path) = mutate_mutable(
            "lineage-details-depth",
            "UPDATE capsule_lineage_event SET details_json = \
             '{\"a\":{\"b\":{\"c\":{\"d\":{\"e\":{\"f\":{\"g\":{\"h\":{\"i\":1}}}}}}}}}'",
        );
        assert_eq!(open_error(&path), WorkspaceErrorCode::LimitExceeded);

        let (_directory, path) = signed_fixture("lineage-properties");
        let details = serde_json::Value::Object(
            (0..65)
                .map(|index| (format!("key-{index}"), serde_json::Value::Null))
                .collect(),
        );
        let connection = Connection::open(&path).expect("open property fixture");
        connection
            .execute(
                "UPDATE capsule_lineage_event SET details_json = ?1",
                [serde_json::to_string(&details).expect("details JSON")],
            )
            .expect("store oversized details object");
        drop(connection);
        assert_eq!(open_error(&path), WorkspaceErrorCode::LimitExceeded);

        let (_directory, path) = signed_fixture("lineage-byte-budget");
        let limits = WorkspaceLimits {
            max_lineage_details_bytes: 1,
            ..WorkspaceLimits::default()
        };
        let error = match VerifiedWorkspaceSource::open_with_control(
            &path,
            &limits,
            &CancellationToken::new(),
        ) {
            Ok(_) => panic!("lineage details byte budget must fail"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::LimitExceeded);
    }

    #[test]
    fn duplicate_classification_and_undeclared_tables_fail_closed() {
        let schema_without_unique = V03_SCHEMA.replacen(
            "table_name              TEXT NOT NULL UNIQUE,",
            "table_name              TEXT NOT NULL,",
            1,
        );
        assert_ne!(schema_without_unique, V03_SCHEMA);
        let (_directory, path) = fixture_from_sources(
            "duplicate-classification",
            &schema_without_unique,
            &stable_fixture_source(),
        );
        let connection = Connection::open(&path).expect("open duplicate fixture");
        connection
            .execute(
                "INSERT INTO capsule_dataset_table VALUES \
                 ('settings', 'vector_domain', 1, '[\"id\"]', '[]', '[\"id\"]')",
                [],
            )
            .expect("duplicate table classification");
        resign(&connection);
        drop(connection);
        assert_eq!(open_error(&path), WorkspaceErrorCode::InvalidContract);

        let (_directory, path) = mutate_and_resign(
            "undeclared-table",
            "CREATE TABLE extra_domain (id INTEGER PRIMARY KEY, value TEXT NOT NULL)",
        );
        assert_eq!(open_error(&path), WorkspaceErrorCode::UndeclaredTable);
    }

    #[test]
    fn wrong_primary_key_ignored_key_and_dependency_cycle_fail_closed() {
        let (_directory, path) = mutate_and_resign(
            "wrong-pk",
            "UPDATE capsule_dataset_table SET primary_key_json = '[\"note\"]' \
             WHERE table_name = 'vector_domain'",
        );
        assert_eq!(open_error(&path), WorkspaceErrorCode::MissingPrimaryKey);

        let (_directory, path) = mutate_and_resign(
            "ignored-pk",
            "UPDATE capsule_dataset_table SET ignored_columns_json = '[\"id\"]' \
             WHERE table_name = 'vector_domain'",
        );
        assert_eq!(open_error(&path), WorkspaceErrorCode::InvalidContract);

        let (_directory, path) = mutate_and_resign(
            "dependency-cycle",
            "INSERT INTO capsule_dataset_dependency VALUES \
             ('settings', 'content', 'Cycle back to content.')",
        );
        assert_eq!(open_error(&path), WorkspaceErrorCode::InvalidContract);
    }

    #[test]
    fn signed_dependencies_must_cover_restrictive_cross_dataset_foreign_keys() {
        fn cross_dataset_fixture(action: &str) -> String {
            stable_fixture_source()
                .replacen(
                    "    payload BLOB NOT NULL\n);",
                    &format!(
                        "    payload BLOB NOT NULL,\n    settings_key TEXT REFERENCES vector_settings(key) ON DELETE {action}\n);"
                    ),
                    1,
                )
                .replacen(
                    "INSERT INTO vector_domain VALUES ('domain', 'mutable', -0.0, X'102030');",
                    "INSERT INTO vector_domain VALUES ('domain', 'mutable', -0.0, X'102030', NULL);",
                    1,
                )
        }

        let (directory, path) = fixture_from_sources(
            "cross-dataset-fk-covered",
            V03_SCHEMA,
            &cross_dataset_fixture("RESTRICT"),
        );
        VerifiedWorkspaceSource::open(&path)
            .expect("declared restrictive child-to-parent dependency is accepted");

        let connection = Connection::open(&path).expect("open FK fixture");
        connection
            .execute("DELETE FROM capsule_dataset_dependency", [])
            .expect("remove signed dependency declaration");
        resign(&connection);
        drop(connection);
        assert_eq!(open_error(&path), WorkspaceErrorCode::InvalidContract);
        drop(directory);

        let (_directory, path) = fixture_from_sources(
            "cross-dataset-fk-cascade",
            V03_SCHEMA,
            &cross_dataset_fixture("CASCADE"),
        );
        assert_eq!(open_error(&path), WorkspaceErrorCode::InvalidContract);
    }

    #[test]
    fn authenticated_template_state_matches_then_domain_edits_fail_closed() {
        let (_directory, path) = signed_fixture("template-state-proof");
        let proof = canonical_template_proof(&path);
        install_template_proof(&path, &proof);

        let source = VerifiedWorkspaceSource::open(&path).expect("verified proof source");
        let verified = verify_template_state(
            &source,
            &TemplateStateLimits::default(),
            &CancellationToken::new(),
        )
        .expect("template proof matches actual state");
        assert_eq!(verified.datasets.len(), 2);
        drop(source);

        let connection = Connection::open(&path).expect("mutate domain after proof");
        connection
            .execute(
                "UPDATE vector_domain SET note = 'working-user-state' WHERE id = 'domain'",
                [],
            )
            .expect("mutate unsigned domain state");
        drop(connection);
        let source = VerifiedWorkspaceSource::open(&path)
            .expect("application signature remains valid after domain edit");
        assert_eq!(
            verify_template_state(
                &source,
                &TemplateStateLimits::default(),
                &CancellationToken::new(),
            )
            .expect_err("stale proof cannot authenticate a working capsule")
            .kind(),
            WorkspaceErrorCode::InvalidContract
        );
    }

    #[test]
    fn template_state_proof_mutations_fail_closed_with_stable_codes() {
        for (name, mutate) in [
            (
                "wrong-profile",
                Box::new(|proof: &mut serde_json::Value| {
                    proof["profile"] = serde_json::json!("org.example.wrong/1");
                }) as Box<dyn Fn(&mut serde_json::Value)>,
            ),
            (
                "wrong-count",
                Box::new(|proof: &mut serde_json::Value| {
                    proof["datasets"][0]["stored_row_count"] = serde_json::json!(99);
                }),
            ),
            (
                "wrong-digest",
                Box::new(|proof: &mut serde_json::Value| {
                    proof["datasets"][0]["state_sha256"] = serde_json::json!("0".repeat(64));
                }),
            ),
            (
                "wrong-disposition",
                Box::new(|proof: &mut serde_json::Value| {
                    proof["datasets"][0]["disposition"] = serde_json::json!("empty");
                }),
            ),
            (
                "missing-dataset",
                Box::new(|proof: &mut serde_json::Value| {
                    proof["datasets"].as_array_mut().expect("datasets").pop();
                }),
            ),
            (
                "extra-dataset",
                Box::new(|proof: &mut serde_json::Value| {
                    let extra = proof["datasets"][0].clone();
                    proof["datasets"]
                        .as_array_mut()
                        .expect("datasets")
                        .push(extra);
                }),
            ),
            (
                "reordered-datasets",
                Box::new(|proof: &mut serde_json::Value| {
                    proof["datasets"]
                        .as_array_mut()
                        .expect("datasets")
                        .reverse();
                }),
            ),
            (
                "unknown-property",
                Box::new(|proof: &mut serde_json::Value| {
                    proof["unexpected"] = serde_json::json!(true);
                }),
            ),
        ] {
            let (_directory, path) = signed_fixture(&format!("template-{name}"));
            let source = VerifiedWorkspaceSource::open(&path).expect("source before proof");
            let mut proof = template_proof_value(&source);
            drop(source);
            mutate(&mut proof);
            let proof = crate::plan::canonical_json(&proof).expect("mutated canonical proof");
            install_template_proof(&path, &proof);
            let source = VerifiedWorkspaceSource::open(&path).expect("signed mutated proof");
            assert_eq!(
                verify_template_state(
                    &source,
                    &TemplateStateLimits::default(),
                    &CancellationToken::new(),
                )
                .expect_err("mutated proof must not authenticate template state")
                .kind(),
                WorkspaceErrorCode::InvalidContract,
                "mutation {name}"
            );
        }
    }

    #[test]
    fn template_state_rejects_noncanonical_duplicate_oversize_and_wrong_doc_metadata() {
        let cases = [
            ("noncanonical", b"{ \"profile\": \"x\" }".to_vec()),
            (
                "duplicate-key",
                b"{\"profile\":\"x\",\"profile\":\"y\"}".to_vec(),
            ),
            ("oversize", vec![b' '; 256 * 1024 + 1]),
        ];
        for (name, proof) in cases {
            let (_directory, path) = signed_fixture(&format!("template-{name}"));
            install_template_proof(&path, &proof);
            let source = VerifiedWorkspaceSource::open(&path).expect("signed hostile proof");
            assert_eq!(
                verify_template_state(
                    &source,
                    &TemplateStateLimits::default(),
                    &CancellationToken::new(),
                )
                .expect_err("hostile proof must fail")
                .kind(),
                WorkspaceErrorCode::InvalidContract,
                "case {name}"
            );
        }

        let (_directory, path) = signed_fixture("template-wrong-doc-metadata");
        let proof = canonical_template_proof(&path);
        install_template_proof(&path, &proof);
        let connection = Connection::open(&path).expect("mutate proof metadata");
        connection
            .execute(
                "UPDATE capsule_doc SET media_type='application/json' WHERE slug=?1",
                [TEMPLATE_STATE_DOC_SLUG],
            )
            .expect("wrong proof media type");
        resign(&connection);
        drop(connection);
        let source = VerifiedWorkspaceSource::open(&path).expect("signed wrong metadata");
        assert_eq!(
            verify_template_state(
                &source,
                &TemplateStateLimits::default(),
                &CancellationToken::new(),
            )
            .expect_err("wrong signed doc metadata must fail")
            .kind(),
            WorkspaceErrorCode::InvalidContract
        );
    }

    #[test]
    fn template_state_limits_cancellation_and_live_source_race_fail_closed() {
        let (_directory, path) = signed_fixture("template-limits");
        let proof = canonical_template_proof(&path);
        install_template_proof(&path, &proof);
        let source = VerifiedWorkspaceSource::open(&path).expect("verified template source");

        let limits = TemplateStateLimits {
            max_rows: 1,
            ..TemplateStateLimits::default()
        };
        assert_eq!(
            verify_template_state(&source, &limits, &CancellationToken::new())
                .expect_err("row ceiling must apply to the full proof")
                .kind(),
            WorkspaceErrorCode::LimitExceeded
        );
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        assert_eq!(
            verify_template_state(&source, &TemplateStateLimits::default(), &cancellation)
                .expect_err("cancelled proof verification must fail")
                .kind(),
            WorkspaceErrorCode::Cancelled
        );

        let before = fs::metadata(&path).expect("source metadata").len();
        let connection = Connection::open(&path).expect("same-object live mutation");
        connection
            .execute(
                "UPDATE vector_domain SET note='user-state' WHERE id='domain'",
                [],
            )
            .expect("mutate live source");
        drop(connection);
        assert_eq!(
            fs::metadata(&path).expect("source after mutation").len(),
            before
        );
        assert_eq!(
            verify_template_state(
                &source,
                &TemplateStateLimits::default(),
                &CancellationToken::new(),
            )
            .expect_err("live source race must invalidate the retained snapshot")
            .kind(),
            WorkspaceErrorCode::StalePlan
        );
    }

    #[test]
    fn rust_matches_the_independent_template_state_vectors() {
        let vectors: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../compatibility/template-state-v1/vectors.json"
        ))
        .expect("template-state vectors JSON");
        let fixture = format!("{SIGNED_FIXTURE}\n{TEMPLATE_TYPE_SPECTRUM}");
        let (_directory, path) =
            fixture_from_sources("template-state-vectors", V03_SCHEMA, &fixture);
        let source = VerifiedWorkspaceSource::open(&path).expect("verified vector source");

        for dataset in &source.data_contract().datasets {
            let expected = &vectors["datasets"][&dataset.id];
            assert!(expected.is_object(), "missing vector for {}", dataset.id);
            let (rows, stream_bytes, state_sha256) =
                crate::template_state::dataset_state_vector_for_test(&source, dataset)
                    .expect("Rust dataset-state vector");
            assert_eq!(rows, expected["stored_row_count"].as_u64().unwrap());
            assert_eq!(
                stream_bytes,
                expected["canonical_stream_bytes"].as_u64().unwrap()
            );
            assert_eq!(state_sha256, expected["state_sha256"].as_str().unwrap());
        }
    }

    #[test]
    fn dataset_enumeration_limits_accept_exact_boundary_and_reject_plus_one() {
        let (_directory, path) = signed_fixture("enumeration-boundaries");
        let exact = WorkspaceLimits {
            max_datasets: 2,
            max_tables_total: 2,
            max_tables_per_dataset: 1,
            max_dependencies_per_dataset: 1,
            max_columns_per_table: 4,
            max_primary_key_columns: 1,
            max_ignored_columns: 0,
            max_immutable_columns: 1,
            ..WorkspaceLimits::default()
        };
        VerifiedWorkspaceSource::open_with_control(&path, &exact, &CancellationToken::new())
            .expect("every dataset enumeration accepts its exact boundary");

        for (name, limits) in [
            (
                "datasets",
                WorkspaceLimits {
                    max_datasets: 1,
                    ..exact.clone()
                },
            ),
            (
                "tables-total",
                WorkspaceLimits {
                    max_tables_total: 1,
                    ..exact.clone()
                },
            ),
            (
                "dependencies",
                WorkspaceLimits {
                    max_dependencies_per_dataset: 0,
                    ..exact.clone()
                },
            ),
            (
                "columns",
                WorkspaceLimits {
                    max_columns_per_table: 3,
                    ..exact.clone()
                },
            ),
        ] {
            let error = match VerifiedWorkspaceSource::open_with_control(
                &path,
                &limits,
                &CancellationToken::new(),
            ) {
                Ok(_) => panic!("one row beyond the {name} cap must fail"),
                Err(error) => error,
            };
            assert_eq!(error.kind(), WorkspaceErrorCode::LimitExceeded, "{name}");
        }
    }

    #[test]
    fn sqlite_table_and_column_names_are_bounded_utf8_but_dataset_ids_remain_restricted() {
        let (_directory, path) = mutate_and_resign(
            "unicode-sqlite-identifiers",
            "CREATE TABLE \"Résumé Table\" (\
                 \"Primary Key\" INTEGER PRIMARY KEY, \
                 \"Value 😀\" TEXT NOT NULL\
             ); \
             INSERT INTO capsule_dataset_table VALUES (\
                 'content', 'Résumé Table', 1, '[\"Primary Key\"]', '[]', \
                 '[\"Primary Key\"]'\
             )",
        );
        let limits = WorkspaceLimits {
            max_tables_total: 3,
            max_tables_per_dataset: 2,
            ..WorkspaceLimits::default()
        };
        let source =
            VerifiedWorkspaceSource::open_with_control(&path, &limits, &CancellationToken::new())
                .expect("bounded UTF-8 SQLite names are valid");
        assert!(
            source
                .data_contract()
                .datasets
                .iter()
                .flat_map(|dataset| &dataset.tables)
                .any(|table| table.name == "Résumé Table")
        );
        let plus_one = WorkspaceLimits {
            max_tables_per_dataset: 1,
            ..limits
        };
        let error = match VerifiedWorkspaceSource::open_with_control(
            &path,
            &plus_one,
            &CancellationToken::new(),
        ) {
            Ok(_) => panic!("per-dataset max plus one must fail"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::LimitExceeded);

        let (_directory, path) = mutate_and_resign(
            "restricted-dataset-id",
            "PRAGMA foreign_keys=OFF; \
             UPDATE capsule_dataset SET id = 'content space' WHERE id = 'content'; \
             UPDATE capsule_dataset_table SET dataset_id = 'content space' \
                 WHERE dataset_id = 'content'; \
             UPDATE capsule_dataset_dependency SET dataset_id = 'content space' \
                 WHERE dataset_id = 'content'; \
             PRAGMA foreign_keys=ON",
        );
        assert_eq!(open_error(&path), WorkspaceErrorCode::InvalidContract);
    }

    #[test]
    fn accepted_primary_key_shapes_and_exact_declared_order_are_enforced() {
        let integer = "CREATE TABLE vector_domain (\n\
    id INTEGER PRIMARY KEY,\n\
    note TEXT NOT NULL,\n\
    measurement REAL NOT NULL,\n\
    payload BLOB NOT NULL\n\
);";
        let (_directory, path) = domain_primary_key_fixture(
            "integer-primary-key",
            integer,
            "[\"id\"]",
            "[\"id\"]",
            true,
        );
        VerifiedWorkspaceSource::open(&path).expect("INTEGER PRIMARY KEY is stable identity");

        let composite_rowid = "CREATE TABLE vector_domain (\n\
    id TEXT NOT NULL,\n\
    note TEXT NOT NULL,\n\
    measurement REAL NOT NULL,\n\
    payload BLOB NOT NULL,\n\
    PRIMARY KEY (id, note)\n\
);";
        let (_directory, path) = domain_primary_key_fixture(
            "not-null-composite-rowid",
            composite_rowid,
            "[\"id\",\"note\"]",
            "[\"id\",\"note\"]",
            false,
        );
        VerifiedWorkspaceSource::open(&path)
            .expect("NOT NULL composite rowid key is stable identity");

        let without_rowid = "CREATE TABLE vector_domain (\n\
    id TEXT,\n\
    note TEXT,\n\
    measurement REAL NOT NULL,\n\
    payload BLOB NOT NULL,\n\
    PRIMARY KEY (id, note)\n\
) WITHOUT ROWID;";
        let (_directory, path) = domain_primary_key_fixture(
            "composite-without-rowid",
            without_rowid,
            "[\"id\",\"note\"]",
            "[\"id\",\"note\"]",
            false,
        );
        VerifiedWorkspaceSource::open(&path)
            .expect("WITHOUT ROWID composite key supplies stable non-null identity");

        let (_directory, path) = domain_primary_key_fixture(
            "wrong-composite-order",
            composite_rowid,
            "[\"note\",\"id\"]",
            "[\"id\",\"note\"]",
            false,
        );
        assert_eq!(open_error(&path), WorkspaceErrorCode::MissingPrimaryKey);

        let (_directory, path) = mutate_and_resign(
            "table-case-mismatch",
            "UPDATE capsule_dataset_table SET table_name = 'Vector_Domain' \
             WHERE table_name = 'vector_domain'",
        );
        assert_eq!(open_error(&path), WorkspaceErrorCode::InvalidContract);

        let (_directory, path) = mutate_and_resign(
            "column-case-mismatch",
            "UPDATE capsule_dataset_table SET primary_key_json = '[\"ID\"]' \
             WHERE table_name = 'vector_domain'",
        );
        assert_eq!(open_error(&path), WorkspaceErrorCode::MissingPrimaryKey);
    }

    #[test]
    fn policy_and_json_shape_fail_closed_without_role_inference() {
        let (_directory, path) = mutate_and_resign(
            "required-omit",
            "UPDATE capsule_dataset SET fork_policy = 'omit' WHERE id = 'content'",
        );
        assert_eq!(open_error(&path), WorkspaceErrorCode::InvalidContract);

        let (_directory, path) = mutate_and_resign(
            "three-way-summary",
            "UPDATE capsule_dataset SET compare_policy = 'summary' WHERE id = 'content'",
        );
        assert_eq!(open_error(&path), WorkspaceErrorCode::InvalidContract);

        let (_directory, path) = mutate_and_resign(
            "nested-key-json",
            "UPDATE capsule_dataset_table SET primary_key_json = '[[[[[\"id\"]]]]]' \
             WHERE table_name = 'vector_domain'",
        );
        assert_eq!(open_error(&path), WorkspaceErrorCode::LimitExceeded);
    }

    #[test]
    fn composite_nullable_and_nonbinary_or_descending_keys_are_rejected() {
        // SQLite rowid tables permit repeated NULL values for a non-INTEGER
        // single-column PRIMARY KEY, so that declaration is not stable identity.
        let nullable_fixture =
            SIGNED_FIXTURE.replacen("id TEXT PRIMARY KEY NOT NULL,", "id TEXT PRIMARY KEY,", 1);
        let (_directory, path) =
            fixture_from_sources("nullable-single", V03_SCHEMA, &nullable_fixture);
        assert_eq!(open_error(&path), WorkspaceErrorCode::MissingPrimaryKey);

        let composite_fixture = stable_fixture_source().replacen(
            "id TEXT PRIMARY KEY NOT NULL,\n    note TEXT NOT NULL,\n    measurement REAL NOT NULL,\n    payload BLOB NOT NULL",
            "id TEXT,\n    note TEXT NOT NULL,\n    measurement REAL NOT NULL,\n    payload BLOB NOT NULL,\n    PRIMARY KEY (id, note)",
            1,
        );
        assert_ne!(composite_fixture, SIGNED_FIXTURE);
        let (_directory, path) =
            fixture_from_sources("nullable-composite", V03_SCHEMA, &composite_fixture);
        let connection = Connection::open(&path).expect("open composite fixture");
        connection
            .execute(
                "UPDATE capsule_dataset_table SET primary_key_json = '[\"id\",\"note\"]' \
                 WHERE table_name = 'vector_domain'",
                [],
            )
            .expect("declare composite key");
        resign(&connection);
        drop(connection);
        assert_eq!(open_error(&path), WorkspaceErrorCode::MissingPrimaryKey);

        for (name, replacement, code) in [
            (
                "nonbinary-key",
                "id TEXT COLLATE NOCASE NOT NULL PRIMARY KEY,",
                WorkspaceErrorCode::UnsupportedCollation,
            ),
            (
                "descending-key",
                "id TEXT NOT NULL PRIMARY KEY DESC,",
                WorkspaceErrorCode::MissingPrimaryKey,
            ),
        ] {
            let fixture =
                stable_fixture_source().replacen("id TEXT PRIMARY KEY NOT NULL,", replacement, 1);
            let (_directory, path) = fixture_from_sources(name, V03_SCHEMA, &fixture);
            let connection = Connection::open(&path).expect("open key fixture");
            resign(&connection);
            drop(connection);
            assert_eq!(open_error(&path), code, "hostile key case {name}");
        }
    }

    #[test]
    fn verified_snapshot_connection_is_read_only() {
        let (_directory, path) = signed_fixture("read-only");
        let source = VerifiedWorkspaceSource::open(&path).expect("workspace source");
        assert!(
            source
                .verified
                .connection()
                .execute("DELETE FROM vector_domain", [])
                .is_err()
        );
    }

    #[test]
    fn rejects_a_structural_capsule_without_a_matching_signature() {
        let (_directory, path) = signed_fixture("signature-mismatch");
        let connection = Connection::open(&path).expect("mutate signed fixture");
        connection
            .execute(
                "UPDATE capsule_application SET description = 'changed after signing' WHERE id = 1",
                [],
            )
            .expect("invalidate signature digest");
        drop(connection);

        let error = match VerifiedWorkspaceSource::open(&path) {
            Ok(_) => panic!("signature must fail"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::InvalidSignature);
    }

    #[test]
    fn source_replacement_after_open_is_stale() {
        let (directory, path) = signed_fixture("stale-source");
        let source = VerifiedWorkspaceSource::open(&path).expect("workspace source");
        let replacement = directory.path().join("replacement.sqlitecapsule");
        fs::copy(&path, &replacement).expect("copy replacement");
        if fs::rename(&replacement, &path).is_ok() {
            assert_eq!(
                source
                    .assert_current()
                    .expect_err("replacement must fail")
                    .kind(),
                WorkspaceErrorCode::StalePlan
            );
        } else {
            source
                .assert_current()
                .expect("held pin blocked replacement and remains current");
        }
    }

    #[test]
    fn same_object_same_size_source_mutation_is_always_stale() {
        use std::io::{Read, Seek, SeekFrom, Write};
        let (_directory, path) = signed_fixture("same-object-stale");
        let source = VerifiedWorkspaceSource::open(&path).expect("workspace source");
        let before = fs::read(&path).expect("source bytes");
        let mut file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .expect("open same source object");
        let offset = before.len() - 1;
        file.seek(SeekFrom::Start(offset as u64)).unwrap();
        let mut original = [0_u8; 1];
        file.read_exact(&mut original).unwrap();
        file.seek(SeekFrom::Start(offset as u64)).unwrap();
        file.write_all(&[original[0] ^ 1]).unwrap();
        file.sync_all().unwrap();
        assert_eq!(
            source
                .assert_current()
                .expect_err("same-object mutation must fail")
                .kind(),
            WorkspaceErrorCode::StalePlan
        );
        file.seek(SeekFrom::Start(offset as u64)).unwrap();
        file.write_all(&original).unwrap();
        file.sync_all().unwrap();
        assert_eq!(fs::read(&path).unwrap(), before);
    }
}

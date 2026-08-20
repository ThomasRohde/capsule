//! Trusted-shell reconciliation authority.
//!
//! This module deliberately has no Tauri command functions. The command layer
//! supplies host-owned compare handoffs and picker results; renderer requests
//! contain only random capabilities minted here. Paths, signed-contract
//! positions, row and field digests, canonical plans, payload bytes and SQL
//! never cross the WebView boundary.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlite_capsule_workspace::{
    CancellationToken, CompareDetailFieldKind, CompareDetailRowKind, CompareLimits, ComparePolicy,
    CompareRowDetail, CompareSummary, DatasetTable, LifecyclePlan, ReconcileAction,
    ReconcileOutputRequest, ReconcilePolicy, ReconcileReview, ReconcileReviewLimits,
    ReconcileSelection, Sensitivity, ThreeWayConflictKind, ThreeWayConflictResolution,
    ThreeWayDeletedSide, ThreeWayReconcileReview, ThreeWayResolutionChoice,
    VerifiedWorkspaceSource, WorkspaceError, WorkspaceErrorCode, WorkspaceLimits,
    classify_three_way_reconcile, compare_sources, prepare_reconcile_review,
};

pub(crate) const RECONCILE_OPTIONS_PROFILE: &str = "org.sqlite-capsule.tauri-reconcile-options/1";
pub(crate) const RECONCILE_SESSION_PROFILE: &str = "org.sqlite-capsule.tauri-reconcile-session/1";
pub(crate) const RECONCILE_REVIEW_PROFILE: &str = "org.sqlite-capsule.tauri-reconcile-review/1";
pub(crate) const RECONCILE_THREE_WAY_PROFILE: &str =
    "org.sqlite-capsule.tauri-reconcile-three-way/1";
pub(crate) const RECONCILE_STATUS_PROFILE: &str = "org.sqlite-capsule.tauri-reconcile-status/1";
pub(crate) const RECONCILE_PROGRESS_EVENT: &str = "capsule-reconcile-progress-v1";
pub(crate) const HUMAN_REVIEW_LIFETIME: Duration = Duration::from_secs(5 * 60);
pub(crate) const EXECUTION_LIFETIME: Duration = Duration::from_secs(30);
const MAX_SELECTION_AUTHORITIES: usize = 10_000;
const MAX_DISPLAY_CHARS: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReconcileOrientation {
    LeftToRight,
    RightToLeft,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReconcileOptionsRequest {
    pub session_token: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StartReconcileRequest {
    pub orientation_token: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReconcileSessionRequest {
    pub review_token: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PrepareReconcileRequest {
    pub review_token: String,
    pub destination_token: String,
    #[serde(default)]
    pub selection_tokens: Vec<String>,
    #[serde(default)]
    pub ancestor_token: Option<String>,
    #[serde(default)]
    pub resolution_tokens: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChooseReconcileAncestorRequest {
    pub review_token: String,
    pub destination_token: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExecuteReconcileRequest {
    pub confirmation_nonce: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReconcileOperationRequest {
    pub operation_token: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ReconcileOrientationView {
    pub orientation_token: String,
    pub source_label: String,
    pub target_label: String,
    pub result_label: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ReconcileOptionsView {
    pub profile: &'static str,
    pub orientations: [ReconcileOrientationView; 2],
    pub disclosed_change_count: usize,
    pub sensitive_change_count: usize,
    pub blockers: Vec<&'static str>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ReconcileSelectionView {
    pub selection_token: String,
    pub dataset_label: String,
    pub table_label: String,
    pub action: &'static str,
    pub field_count: usize,
    pub sensitivity: Sensitivity,
    pub sensitive_reveal_confirmed: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ReconcileSessionView {
    pub profile: &'static str,
    pub review_token: String,
    pub source_label: String,
    pub target_label: String,
    pub output_capsule_id: String,
    pub output_application_digest: String,
    pub output_signature_count: u32,
    pub selections: Vec<ReconcileSelectionView>,
    pub expires_at: String,
    pub checks: [&'static str; 6],
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ReconcileDestinationView {
    pub destination_token: String,
    pub parent_display: &'static str,
    pub leaf_display: String,
    pub expires_at: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ReconcileResolutionChoiceView {
    pub resolution_token: String,
    pub choice: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ReconcileConflictView {
    pub conflict_token: String,
    pub dataset_label: String,
    pub table_label: String,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_side: Option<&'static str>,
    pub choices: Vec<ReconcileResolutionChoiceView>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ReconcileThreeWayView {
    pub profile: &'static str,
    pub ancestor_token: String,
    pub ancestor: ReconcileReferenceView,
    pub clean_change_count: usize,
    pub conflicts: Vec<ReconcileConflictView>,
    pub expires_at: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ReconcileReferenceView {
    pub capsule_id: String,
    pub revision_id: String,
    pub application_digest: String,
    pub signature_count: u32,
    pub data_schema_id: String,
    pub data_schema_version: i64,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ReconcileOperationReviewView {
    pub sequence: u64,
    pub dataset_label: String,
    pub table_label: String,
    pub action: &'static str,
    pub field_count: usize,
    pub sensitive_confirmed: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct PreparedReconcileView {
    pub profile: &'static str,
    pub confirmation_nonce: String,
    pub review_digest: String,
    pub payload_digest: String,
    pub compare_report_digest: String,
    pub source: ReconcileReferenceView,
    pub target: ReconcileReferenceView,
    pub output: ReconcileReferenceView,
    pub operation_count: usize,
    pub operations: Vec<ReconcileOperationReviewView>,
    pub lineage_parent_count: usize,
    pub destination: ReconcileDestinationView,
    pub checks: [&'static str; 9],
    pub expires_at: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ReconcileOperationPhase {
    Queued,
    Reverify,
    Stage,
    Transform,
    Validate,
    Publish,
    Postpublish,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ReconcileProgressEvent {
    pub profile: &'static str,
    pub operation_token: String,
    pub sequence: u64,
    pub phase: ReconcileOperationPhase,
    pub cancellable: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ReconcileOperationStatus {
    pub profile: &'static str,
    pub operation_token: String,
    pub phase: ReconcileOperationPhase,
    pub cancellable: bool,
    pub output_leaf: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<WorkspaceError>,
}

/// Private contract binding returned by `CompareController` for a page that
/// has already been produced from retained verified sources.
pub(crate) struct ReconcilePageBinding {
    pub selection_id: String,
    pub session_id: String,
    pub report_digest: String,
    pub dataset_index: usize,
    pub table_index: usize,
    pub dataset_label: String,
    pub table_label: String,
    pub compare_policy: ComparePolicy,
    pub reconcile_policy: ReconcilePolicy,
    pub sensitivity: Sensitivity,
    pub table: DatasetTable,
}

/// One-use private compare handoff. Debug intentionally omits all sources.
pub(crate) struct CompareReconcileHandoff {
    pub selection_id: String,
    pub session_id: String,
    pub report: CompareSummary,
    pub left: VerifiedWorkspaceSource,
    pub right: VerifiedWorkspaceSource,
    pub compare_deadline: Instant,
}

impl std::fmt::Debug for CompareReconcileHandoff {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CompareReconcileHandoff")
            .field("session_id", &"<opaque>")
            .finish()
    }
}

pub(crate) struct OrientedReconcileHandoff {
    selection_id: String,
    session_id: String,
    origin_report_digest: String,
    report: CompareSummary,
    source: VerifiedWorkspaceSource,
    target: VerifiedWorkspaceSource,
}

impl std::fmt::Debug for OrientedReconcileHandoff {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("OrientedReconcileHandoff(<opaque>)")
    }
}

#[derive(Clone)]
struct SelectionPair {
    left_to_right: ReconcileSelection,
    right_to_left: ReconcileSelection,
    dataset_label: String,
    table_label: String,
    sensitivity: Sensitivity,
    sensitive_reveal_confirmed: bool,
}

struct EvidenceSession {
    selection_id: String,
    session_id: String,
    report_digest: String,
    left_label: String,
    right_label: String,
    compatible: bool,
    orientations: BTreeMap<String, ReconcileOrientation>,
    rows: BTreeMap<String, SelectionPair>,
    three_way_rows: BTreeSet<String>,
    three_way_sensitive_rows: BTreeSet<String>,
    three_way_sensitive_datasets: BTreeSet<usize>,
}

struct SelectionAuthority {
    selection: ReconcileSelection,
    sensitivity: Sensitivity,
    sensitive_reveal_confirmed: bool,
}

struct HumanSession {
    selection_id: String,
    review_token: String,
    report: CompareSummary,
    source: VerifiedWorkspaceSource,
    target: VerifiedWorkspaceSource,
    selections: BTreeMap<[u8; 32], SelectionAuthority>,
    three_way_reviewed: bool,
    three_way_sensitive_datasets: BTreeSet<usize>,
    destination: Option<DestinationAuthority>,
    deadline: Instant,
    expires_at: String,
}

struct DestinationAuthority {
    token: String,
    path: PathBuf,
    leaf_display: String,
    expires_at: String,
    deadline: Instant,
}

struct PreparedAuthority {
    selection_id: String,
    nonce_sha256: [u8; 32],
    review: ReconcileReview,
    approved_plan: LifecyclePlan,
    approved_payload: Vec<u8>,
    deadline: Instant,
    output_leaf: String,
    cancellation: CancellationToken,
}

struct ResolutionAuthority {
    conflict_id: String,
    choice: ThreeWayResolutionChoice,
}

struct ThreeWayAuthority {
    selection_id: String,
    review_token: String,
    ancestor_token: String,
    review: ThreeWayReconcileReview,
    resolutions: BTreeMap<[u8; 32], ResolutionAuthority>,
    destination_view: ReconcileDestinationView,
    output_path: PathBuf,
    deadline: Instant,
    output_leaf: String,
    compare_report_digest: String,
    cancellation: CancellationToken,
}

struct ActiveOperation {
    selection_id: String,
    status: ReconcileOperationStatus,
    cancellation: CancellationToken,
}

#[derive(Default)]
pub(crate) struct ReconcileController {
    evidence: Option<EvidenceSession>,
    human: Option<HumanSession>,
    three_way: Option<ThreeWayAuthority>,
    prepared: Option<PreparedAuthority>,
    active: Option<ActiveOperation>,
}

#[derive(Clone, Default)]
pub(crate) struct ReconcileState(pub(crate) Arc<Mutex<ReconcileController>>);

pub(crate) enum ReconcilePrepareJob {
    TwoWay {
        source: Box<VerifiedWorkspaceSource>,
        target: Box<VerifiedWorkspaceSource>,
        report: Box<CompareSummary>,
        selections: Vec<ReconcileSelection>,
        sensitive_datasets: BTreeSet<usize>,
        output: ReconcileOutputRequest,
        cancellation: CancellationToken,
        selection_id: String,
        output_leaf: String,
        destination_view: ReconcileDestinationView,
        operation_deadline: Instant,
        operation_expires_at: String,
    },
    ThreeWay {
        review: Box<ThreeWayReconcileReview>,
        resolutions: Vec<ThreeWayConflictResolution>,
        output: ReconcileOutputRequest,
        cancellation: CancellationToken,
        selection_id: String,
        output_leaf: String,
        destination_view: ReconcileDestinationView,
        operation_deadline: Instant,
        operation_expires_at: String,
        compare_report_digest: String,
    },
}

pub(crate) struct ReconcileThreeWayJob {
    ancestor_path: PathBuf,
    source: VerifiedWorkspaceSource,
    target: VerifiedWorkspaceSource,
    report: CompareSummary,
    sensitive_datasets: BTreeSet<usize>,
    selection_id: String,
    review_token: String,
    ancestor_token: String,
    destination_view: ReconcileDestinationView,
    output_path: PathBuf,
    classification_deadline: Instant,
    authority_deadline: Instant,
    authority_expires_at: String,
    output_leaf: String,
    cancellation: CancellationToken,
}

pub(crate) struct ReconcileThreeWayShell {
    selection_id: String,
    review_token: String,
    ancestor_token: String,
    destination_view: ReconcileDestinationView,
    output_path: PathBuf,
    classification_deadline: Instant,
    authority_deadline: Instant,
    authority_expires_at: String,
    output_leaf: String,
    compare_report_digest: String,
    cancellation: CancellationToken,
}

pub(crate) struct ReconcilePreparedShell {
    selection_id: String,
    output_leaf: String,
    destination_view: ReconcileDestinationView,
    compare_report_digest: String,
    operation_deadline: Instant,
    operation_expires_at: String,
    cancellation: CancellationToken,
}

pub(crate) struct StartedReconcile {
    review: ReconcileReview,
    approved_plan: LifecyclePlan,
    approved_payload: Vec<u8>,
}

#[derive(Clone, Debug)]
pub(crate) struct ReconcileHandoffBinding {
    pub session_id: String,
    pub selection_id: String,
    pub report_digest: String,
    pub orientation: ReconcileOrientation,
    orientation_token: String,
}

impl ReconcileController {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn begin_compare_evidence(
        &mut self,
        selection_id: &str,
        session_id: &str,
        report_digest: &str,
        left_label: String,
        right_label: String,
        compatible: bool,
    ) -> Result<(), WorkspaceError> {
        validate_token(selection_id)?;
        validate_token(session_id)?;
        validate_digest(report_digest)?;
        self.human = None;
        self.three_way = None;
        self.prepared = None;
        self.evidence = Some(EvidenceSession {
            selection_id: selection_id.to_owned(),
            session_id: session_id.to_owned(),
            report_digest: report_digest.to_owned(),
            left_label: bounded_display(&left_label),
            right_label: bounded_display(&right_label),
            compatible,
            orientations: BTreeMap::new(),
            rows: BTreeMap::new(),
            three_way_rows: BTreeSet::new(),
            three_way_sensitive_rows: BTreeSet::new(),
            three_way_sensitive_datasets: BTreeSet::new(),
        });
        Ok(())
    }

    pub(crate) fn expire_compare_evidence(&mut self, session_id: &str) {
        if self
            .evidence
            .as_ref()
            .is_some_and(|evidence| evidence.session_id == session_id)
        {
            self.evidence = None;
        }
    }

    /// Retains only action authority derived from a page already returned by
    /// the compare core. Repeated pages replace no token and add no authority.
    pub(crate) fn record_compare_page(
        &mut self,
        binding: &ReconcilePageBinding,
        revealed: bool,
        rows: &[CompareRowDetail],
    ) -> Result<(), WorkspaceError> {
        let evidence = self.evidence.as_mut().ok_or_else(stale_plan)?;
        if evidence.selection_id != binding.selection_id
            || evidence.session_id != binding.session_id
            || evidence.report_digest != binding.report_digest
            || !matches!(
                binding.reconcile_policy,
                ReconcilePolicy::Manual | ReconcilePolicy::ThreeWay
            )
            || !matches!(
                binding.compare_policy,
                ComparePolicy::Row | ComparePolicy::Field
            )
            || binding.sensitivity == Sensitivity::Sensitive && !revealed
        {
            return Err(unsupported_operation());
        }
        if evidence
            .rows
            .len()
            .saturating_add(evidence.three_way_rows.len())
            .saturating_add(rows.len())
            > MAX_SELECTION_AUTHORITIES
        {
            return Err(limit_exceeded());
        }
        if binding.reconcile_policy == ReconcilePolicy::ThreeWay {
            let mut saw_changed_row = false;
            for row in rows {
                if row.kind != CompareDetailRowKind::Unchanged {
                    saw_changed_row = true;
                    let binding_key = row_binding_key(binding, row);
                    evidence.three_way_rows.insert(binding_key.clone());
                    if binding.sensitivity == Sensitivity::Sensitive {
                        evidence.three_way_sensitive_rows.insert(binding_key);
                    }
                }
            }
            if binding.sensitivity == Sensitivity::Sensitive && revealed && saw_changed_row {
                evidence
                    .three_way_sensitive_datasets
                    .insert(binding.dataset_index);
            }
            return Ok(());
        }
        for row in rows {
            if row.kind == CompareDetailRowKind::Unchanged {
                continue;
            }
            let Some(left_to_right) = selection_for_orientation(binding, row, true)? else {
                continue;
            };
            let Some(right_to_left) = selection_for_orientation(binding, row, false)? else {
                continue;
            };
            let row_binding = row_binding_key(binding, row);
            if evidence.rows.values().any(|existing| {
                row_binding_key_from_selection(
                    existing.left_to_right.dataset_index,
                    existing.left_to_right.table_index,
                    &existing.left_to_right.key_digest,
                ) == row_binding
            }) {
                continue;
            }
            evidence.rows.insert(
                random_token()?,
                SelectionPair {
                    left_to_right,
                    right_to_left,
                    dataset_label: bounded_display(&binding.dataset_label),
                    table_label: bounded_display(&binding.table_label),
                    sensitivity: binding.sensitivity,
                    sensitive_reveal_confirmed: binding.sensitivity != Sensitivity::Sensitive
                        || revealed,
                },
            );
        }
        Ok(())
    }

    pub(crate) fn options(
        &mut self,
        request: &ReconcileOptionsRequest,
        current_selection: &str,
    ) -> Result<ReconcileOptionsView, WorkspaceError> {
        validate_token(&request.session_token)?;
        validate_token(current_selection)?;
        let evidence = self.evidence.as_mut().ok_or_else(stale_plan)?;
        if evidence.selection_id != current_selection
            || !constant_time_equal(
                evidence.session_id.as_bytes(),
                request.session_token.as_bytes(),
            )
        {
            return Err(stale_plan());
        }
        if evidence.orientations.is_empty() {
            evidence
                .orientations
                .insert(random_token()?, ReconcileOrientation::LeftToRight);
            evidence
                .orientations
                .insert(random_token()?, ReconcileOrientation::RightToLeft);
        }
        let token_for = |orientation| {
            evidence
                .orientations
                .iter()
                .find_map(|(token, value)| (*value == orientation).then(|| token.clone()))
                .ok_or_else(internal_error)
        };
        let blockers = if !evidence.compatible {
            vec!["comparison-not-reconcilable"]
        } else if evidence.rows.is_empty() && evidence.three_way_rows.is_empty() {
            vec!["review-row-detail-first"]
        } else {
            Vec::new()
        };
        Ok(ReconcileOptionsView {
            profile: RECONCILE_OPTIONS_PROFILE,
            orientations: [
                ReconcileOrientationView {
                    orientation_token: token_for(ReconcileOrientation::LeftToRight)?,
                    source_label: evidence.left_label.clone(),
                    target_label: evidence.right_label.clone(),
                    result_label: "New copy of target",
                },
                ReconcileOrientationView {
                    orientation_token: token_for(ReconcileOrientation::RightToLeft)?,
                    source_label: evidence.right_label.clone(),
                    target_label: evidence.left_label.clone(),
                    result_label: "New copy of target",
                },
            ],
            disclosed_change_count: evidence.rows.len() + evidence.three_way_rows.len(),
            sensitive_change_count: evidence
                .rows
                .values()
                .filter(|row| row.sensitivity == Sensitivity::Sensitive)
                .count()
                + evidence.three_way_sensitive_rows.len(),
            blockers,
        })
    }

    pub(crate) fn authorize_handoff(
        &self,
        request: &StartReconcileRequest,
        current_selection: &str,
    ) -> Result<ReconcileHandoffBinding, WorkspaceError> {
        validate_token(&request.orientation_token)?;
        validate_token(current_selection)?;
        let evidence = self.evidence.as_ref().ok_or_else(stale_plan)?;
        if evidence.selection_id != current_selection || !evidence.compatible {
            return Err(stale_plan());
        }
        let orientation = evidence
            .orientations
            .iter()
            .find_map(|(token, orientation)| {
                constant_time_equal(token.as_bytes(), request.orientation_token.as_bytes())
                    .then_some(*orientation)
            })
            .ok_or_else(stale_plan)?;
        Ok(ReconcileHandoffBinding {
            session_id: evidence.session_id.clone(),
            selection_id: evidence.selection_id.clone(),
            report_digest: evidence.report_digest.clone(),
            orientation,
            orientation_token: request.orientation_token.clone(),
        })
    }

    pub(crate) fn retain_human_session(
        &mut self,
        binding: ReconcileHandoffBinding,
        handoff: OrientedReconcileHandoff,
        review_token: String,
        expires_at: String,
    ) -> Result<ReconcileSessionView, WorkspaceError> {
        validate_token(&review_token)?;
        let evidence = self.evidence.take().ok_or_else(stale_plan)?;
        if evidence.selection_id != binding.selection_id
            || evidence.session_id != binding.session_id
            || evidence.report_digest != binding.report_digest
            || !oriented_origin_matches(
                &binding,
                &handoff.selection_id,
                &handoff.session_id,
                &handoff.origin_report_digest,
            )
            || !evidence.orientations.iter().any(|(token, orientation)| {
                *orientation == binding.orientation
                    && constant_time_equal(token.as_bytes(), binding.orientation_token.as_bytes())
            })
        {
            return Err(stale_plan());
        }
        let mut selections = BTreeMap::new();
        let mut views = Vec::with_capacity(evidence.rows.len());
        for pair in evidence.rows.into_values() {
            let selection = match binding.orientation {
                ReconcileOrientation::LeftToRight => pair.left_to_right,
                ReconcileOrientation::RightToLeft => pair.right_to_left,
            };
            let selection_token = random_token()?;
            let action = action_label(selection.action);
            let field_count = selection.field_indices.len();
            views.push(ReconcileSelectionView {
                selection_token: selection_token.clone(),
                dataset_label: pair.dataset_label,
                table_label: pair.table_label,
                action,
                field_count,
                sensitivity: pair.sensitivity,
                sensitive_reveal_confirmed: pair.sensitive_reveal_confirmed,
            });
            selections.insert(
                token_digest(&selection_token),
                SelectionAuthority {
                    selection,
                    sensitivity: pair.sensitivity,
                    sensitive_reveal_confirmed: pair.sensitive_reveal_confirmed,
                },
            );
        }
        views.sort_by(|left, right| {
            (
                &left.dataset_label,
                &left.table_label,
                left.action,
                &left.selection_token,
            )
                .cmp(&(
                    &right.dataset_label,
                    &right.table_label,
                    right.action,
                    &right.selection_token,
                ))
        });
        let (source_label, target_label) = match binding.orientation {
            ReconcileOrientation::LeftToRight => (evidence.left_label, evidence.right_label),
            ReconcileOrientation::RightToLeft => (evidence.right_label, evidence.left_label),
        };
        let output_capsule_id = handoff.report.right.capsule_id.clone();
        let output_application_digest = handoff.report.right.application_digest.clone();
        let output_signature_count = handoff.report.right.publisher.signature_count;
        self.prepared = None;
        self.three_way = None;
        self.human = Some(HumanSession {
            selection_id: binding.selection_id,
            review_token: review_token.clone(),
            report: handoff.report,
            source: handoff.source,
            target: handoff.target,
            selections,
            three_way_reviewed: !evidence.three_way_rows.is_empty(),
            three_way_sensitive_datasets: evidence.three_way_sensitive_datasets,
            destination: None,
            deadline: Instant::now()
                .checked_add(HUMAN_REVIEW_LIFETIME)
                .ok_or_else(limit_exceeded)?,
            expires_at: expires_at.clone(),
        });
        Ok(ReconcileSessionView {
            profile: RECONCILE_SESSION_PROFILE,
            review_token,
            source_label,
            target_label,
            output_capsule_id,
            output_application_digest,
            output_signature_count,
            selections: views,
            expires_at,
            checks: [
                "source-and-target-rebound",
                "new-copy-only",
                "target-identity-preserved",
                "signed-application-preserved",
                "all-datasets-validated",
                "two-parent-lineage",
            ],
        })
    }

    pub(crate) fn retain_destination(
        &mut self,
        request: &ReconcileSessionRequest,
        current_selection: &str,
        path: PathBuf,
        destination_token: String,
    ) -> Result<ReconcileDestinationView, WorkspaceError> {
        validate_token(&request.review_token)?;
        validate_token(current_selection)?;
        validate_token(&destination_token)?;
        let human = self.human.as_mut().ok_or_else(stale_plan)?;
        ensure_human_current(human)?;
        if human.selection_id != current_selection
            || !constant_time_equal(
                human.review_token.as_bytes(),
                request.review_token.as_bytes(),
            )
            || !path.is_absolute()
            || path.exists()
            || !path.parent().is_some_and(|parent| parent.is_dir())
        {
            return Err(stale_plan());
        }
        let leaf = path
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .ok_or_else(invalid_contract)?;
        let view = ReconcileDestinationView {
            destination_token: destination_token.clone(),
            parent_display: "Selected local folder",
            leaf_display: bounded_display(leaf),
            expires_at: human.expires_at.clone(),
        };
        human.destination = Some(DestinationAuthority {
            token: destination_token,
            path,
            leaf_display: view.leaf_display.clone(),
            expires_at: human.expires_at.clone(),
            deadline: human.deadline,
        });
        Ok(view)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn take_three_way_job(
        &mut self,
        request: &ChooseReconcileAncestorRequest,
        current_selection: &str,
        ancestor_path: PathBuf,
        ancestor_token: String,
        classification_deadline: Instant,
    ) -> Result<ReconcileThreeWayJob, WorkspaceError> {
        validate_token(&request.review_token)?;
        validate_token(&request.destination_token)?;
        validate_token(current_selection)?;
        validate_token(&ancestor_token)?;
        if !ancestor_path.is_absolute() || !ancestor_path.is_file() {
            return Err(invalid_contract());
        }
        let human = self.human.take().ok_or_else(stale_plan)?;
        ensure_human_current(&human)?;
        let destination = human.destination.ok_or_else(stale_plan)?;
        let cancellation = CancellationToken::new();
        ensure_operation_current(classification_deadline, &cancellation)?;
        if human.selection_id != current_selection
            || !constant_time_equal(
                human.review_token.as_bytes(),
                request.review_token.as_bytes(),
            )
            || !constant_time_equal(
                destination.token.as_bytes(),
                request.destination_token.as_bytes(),
            )
        {
            return Err(stale_plan());
        }
        if !human.three_way_reviewed {
            return Err(unsupported_operation());
        }
        let sensitive_datasets = human.three_way_sensitive_datasets;
        let destination_view = ReconcileDestinationView {
            destination_token: destination.token,
            parent_display: "Selected local folder",
            leaf_display: destination.leaf_display.clone(),
            expires_at: destination.expires_at.clone(),
        };
        self.prepared = None;
        self.three_way = None;
        Ok(ReconcileThreeWayJob {
            ancestor_path,
            source: human.source,
            target: human.target,
            report: human.report,
            sensitive_datasets,
            selection_id: human.selection_id,
            review_token: human.review_token,
            ancestor_token,
            destination_view,
            output_path: destination.path,
            classification_deadline,
            authority_deadline: destination.deadline,
            authority_expires_at: destination.expires_at,
            output_leaf: destination.leaf_display,
            cancellation,
        })
    }

    pub(crate) fn retain_three_way(
        &mut self,
        shell: ReconcileThreeWayShell,
        review: ThreeWayReconcileReview,
    ) -> Result<ReconcileThreeWayView, WorkspaceError> {
        let now = Instant::now();
        ensure_operation_current(shell.classification_deadline, &shell.cancellation)?;
        let deadline = authority_deadline_from_remaining_at(review.remaining_lifetime()?, now)?
            .min(shell.authority_deadline);
        if now >= deadline {
            return Err(session_expired());
        }
        let ancestor = reference_view(review.ancestor());
        let mut resolutions = BTreeMap::new();
        let mut conflicts = Vec::with_capacity(review.conflicts().len());
        for conflict in review.conflicts() {
            ensure_operation_current(shell.classification_deadline, &shell.cancellation)?;
            let conflict_token = random_token()?;
            let mut choices = Vec::with_capacity(conflict.allowed_choices.len());
            for choice in &conflict.allowed_choices {
                ensure_operation_current(shell.classification_deadline, &shell.cancellation)?;
                let resolution_token = random_token()?;
                resolutions.insert(
                    token_digest(&resolution_token),
                    ResolutionAuthority {
                        conflict_id: conflict.id.clone(),
                        choice: *choice,
                    },
                );
                choices.push(ReconcileResolutionChoiceView {
                    resolution_token,
                    choice: resolution_choice_label(*choice),
                });
            }
            conflicts.push(ReconcileConflictView {
                conflict_token,
                dataset_label: bounded_display(&conflict.dataset_id),
                table_label: bounded_display(&conflict.table),
                kind: conflict_kind_label(conflict.kind),
                deleted_side: conflict.deleted_side.map(deleted_side_label),
                choices,
            });
        }
        let view = ReconcileThreeWayView {
            profile: RECONCILE_THREE_WAY_PROFILE,
            ancestor_token: shell.ancestor_token.clone(),
            ancestor,
            clean_change_count: review.clean_change_count(),
            conflicts,
            expires_at: shell.authority_expires_at,
        };
        self.three_way = Some(ThreeWayAuthority {
            selection_id: shell.selection_id,
            review_token: shell.review_token,
            ancestor_token: shell.ancestor_token,
            review,
            resolutions,
            destination_view: shell.destination_view,
            output_path: shell.output_path,
            deadline,
            output_leaf: shell.output_leaf,
            compare_report_digest: shell.compare_report_digest,
            cancellation: shell.cancellation,
        });
        Ok(view)
    }

    pub(crate) fn take_prepare_job(
        &mut self,
        request: &PrepareReconcileRequest,
        current_selection: &str,
        plan_id: String,
        created_at: String,
        expires_at: String,
        operation_deadline: Instant,
    ) -> Result<ReconcilePrepareJob, WorkspaceError> {
        validate_token(&request.review_token)?;
        validate_token(&request.destination_token)?;
        validate_token(current_selection)?;
        if let Some(ancestor_token) = &request.ancestor_token {
            validate_token(ancestor_token)?;
            if !request.selection_tokens.is_empty() {
                return Err(invalid_contract());
            }
            if request.resolution_tokens.len() > MAX_SELECTION_AUTHORITIES {
                return Err(invalid_contract());
            }
            let three_way = self.three_way.take().ok_or_else(stale_plan)?;
            let now = Instant::now();
            if now >= three_way.deadline {
                return Err(session_expired());
            }
            ensure_operation_current(operation_deadline, &three_way.cancellation)?;
            if three_way.selection_id != current_selection
                || !constant_time_equal(
                    three_way.review_token.as_bytes(),
                    request.review_token.as_bytes(),
                )
                || !constant_time_equal(
                    three_way.ancestor_token.as_bytes(),
                    ancestor_token.as_bytes(),
                )
                || !constant_time_equal(
                    three_way.destination_view.destination_token.as_bytes(),
                    request.destination_token.as_bytes(),
                )
            {
                return Err(stale_plan());
            }
            let expected_conflicts = three_way.review.conflicts().len();
            validate_resolution_count(request.resolution_tokens.len(), expected_conflicts)?;
            let mut unique_tokens = BTreeSet::new();
            let mut unique_conflicts = BTreeSet::new();
            let mut resolutions = Vec::with_capacity(request.resolution_tokens.len());
            for token in &request.resolution_tokens {
                ensure_operation_current(operation_deadline, &three_way.cancellation)?;
                validate_token(token)?;
                let digest = token_digest(token);
                if !unique_tokens.insert(digest) {
                    return Err(invalid_contract());
                }
                let authority = three_way.resolutions.get(&digest).ok_or_else(stale_plan)?;
                if !unique_conflicts.insert(authority.conflict_id.clone()) {
                    return Err(invalid_contract());
                }
                resolutions.push(ThreeWayConflictResolution {
                    conflict_id: authority.conflict_id.clone(),
                    choice: authority.choice,
                });
            }
            validate_resolution_count(unique_conflicts.len(), expected_conflicts)?;
            let operation_deadline =
                clamp_operation_deadline(three_way.deadline, operation_deadline, Instant::now())?;
            let output = ReconcileOutputRequest {
                output_path: three_way.output_path,
                plan_id,
                created_at,
                expires_at: expires_at.clone(),
            };
            let destination_view = ReconcileDestinationView {
                expires_at: expires_at.clone(),
                ..three_way.destination_view
            };
            return Ok(ReconcilePrepareJob::ThreeWay {
                review: Box::new(three_way.review),
                resolutions,
                output,
                cancellation: three_way.cancellation,
                selection_id: three_way.selection_id,
                output_leaf: three_way.output_leaf,
                destination_view,
                operation_deadline,
                operation_expires_at: expires_at,
                compare_report_digest: three_way.compare_report_digest,
            });
        }
        if !request.resolution_tokens.is_empty() {
            return Err(invalid_contract());
        }
        if request.selection_tokens.is_empty()
            || request.selection_tokens.len() > MAX_SELECTION_AUTHORITIES
        {
            return Err(invalid_contract());
        }
        let human = self.human.take().ok_or_else(stale_plan)?;
        ensure_human_current(&human)?;
        let destination = human.destination.ok_or_else(stale_plan)?;
        let cancellation = CancellationToken::new();
        ensure_operation_current(operation_deadline, &cancellation)?;
        if human.selection_id != current_selection
            || !constant_time_equal(
                human.review_token.as_bytes(),
                request.review_token.as_bytes(),
            )
            || !constant_time_equal(
                destination.token.as_bytes(),
                request.destination_token.as_bytes(),
            )
        {
            return Err(stale_plan());
        }
        let output = ReconcileOutputRequest {
            output_path: destination.path.clone(),
            plan_id,
            created_at,
            expires_at: expires_at.clone(),
        };
        let mut unique = BTreeSet::new();
        let mut selections = Vec::with_capacity(request.selection_tokens.len());
        let mut sensitive_datasets = BTreeSet::new();
        for token in &request.selection_tokens {
            ensure_operation_current(operation_deadline, &cancellation)?;
            validate_token(token)?;
            let digest = token_digest(token);
            if !unique.insert(digest) {
                return Err(invalid_contract());
            }
            let authority = human.selections.get(&digest).ok_or_else(stale_plan)?;
            if authority.sensitivity == Sensitivity::Sensitive {
                if !authority.sensitive_reveal_confirmed {
                    return Err(WorkspaceError::new(
                        WorkspaceErrorCode::SensitiveConfirmationRequired,
                    ));
                }
                sensitive_datasets.insert(authority.selection.dataset_index);
            }
            selections.push(authority.selection.clone());
        }
        Ok(ReconcilePrepareJob::TwoWay {
            source: Box::new(human.source),
            target: Box::new(human.target),
            report: Box::new(human.report),
            selections,
            sensitive_datasets,
            output,
            cancellation,
            selection_id: human.selection_id,
            output_leaf: destination.leaf_display.clone(),
            destination_view: ReconcileDestinationView {
                destination_token: destination.token,
                parent_display: "Selected local folder",
                leaf_display: destination.leaf_display,
                expires_at: destination.expires_at,
            },
            operation_deadline,
            operation_expires_at: expires_at,
        })
    }

    pub(crate) fn retain_prepared(
        &mut self,
        shell: ReconcilePreparedShell,
        review: ReconcileReview,
        confirmation_nonce: String,
    ) -> Result<PreparedReconcileView, WorkspaceError> {
        validate_token(&confirmation_nonce)?;
        let remaining_lifetime = review.remaining_lifetime()?;
        let deadline = deadline_from_remaining_at(remaining_lifetime, Instant::now())?
            .min(shell.operation_deadline);
        if Instant::now() >= deadline {
            return Err(session_expired());
        }
        if review.compare_report_digest() != shell.compare_report_digest {
            return Err(stale_plan());
        }
        let approved_plan = review.plan().clone();
        let approved_payload = review.payload().to_vec();
        let source = reference_view(review.source());
        let target = reference_view(review.target());
        let output_review = review.output();
        let output = ReconcileReferenceView {
            capsule_id: output_review.capsule_id.clone(),
            revision_id: output_review.revision_id.clone(),
            application_digest: output_review.application_digest.clone(),
            signature_count: output_review.signature_count,
            data_schema_id: target.data_schema_id.clone(),
            data_schema_version: target.data_schema_version,
        };
        let operations = review
            .operations()
            .iter()
            .map(|operation| ReconcileOperationReviewView {
                sequence: operation.sequence,
                dataset_label: bounded_display(&operation.dataset_id),
                table_label: bounded_display(&operation.table),
                action: action_label(operation.action),
                field_count: operation.fields.len(),
                sensitive_confirmed: operation.sensitive_confirmed,
            })
            .collect();
        let view = PreparedReconcileView {
            profile: RECONCILE_REVIEW_PROFILE,
            confirmation_nonce: confirmation_nonce.clone(),
            review_digest: review.review_digest().to_owned(),
            payload_digest: review.payload_digest().to_owned(),
            compare_report_digest: review.compare_report_digest().to_owned(),
            source,
            target,
            output,
            operation_count: review.operation_count(),
            operations,
            lineage_parent_count: output_review.lineage_parents.len(),
            destination: shell.destination_view,
            checks: [
                "exact-plan-and-payload-bound",
                "both-inputs-reverified",
                "target-derived-private-stage",
                "transactional-named-write-set",
                "foreign-keys-checked",
                "all-dataset-states-matched",
                "signed-application-and-signatures-preserved",
                "no-replace-publish",
                "postpublish-reopen-and-final-rebind",
            ],
            expires_at: shell.operation_expires_at.clone(),
        };
        self.prepared = Some(PreparedAuthority {
            selection_id: shell.selection_id,
            nonce_sha256: Sha256::digest(confirmation_nonce.as_bytes()).into(),
            review,
            approved_plan,
            approved_payload,
            deadline,
            output_leaf: shell.output_leaf,
            cancellation: shell.cancellation,
        });
        Ok(view)
    }

    pub(crate) fn start(
        &mut self,
        request: &ExecuteReconcileRequest,
        current_selection: &str,
        operation_token: String,
    ) -> Result<StartedReconcile, WorkspaceError> {
        validate_token(&request.confirmation_nonce)?;
        validate_token(current_selection)?;
        validate_token(&operation_token)?;
        if self.active.is_some() {
            return Err(unsupported_operation());
        }
        let prepared = self.prepared.take().ok_or_else(stale_plan)?;
        let nonce: [u8; 32] = Sha256::digest(request.confirmation_nonce.as_bytes()).into();
        if Instant::now() >= prepared.deadline
            || prepared.selection_id != current_selection
            || !constant_time_equal(&prepared.nonce_sha256, &nonce)
        {
            return Err(stale_plan());
        }
        let cancellation = prepared.cancellation;
        self.active = Some(ActiveOperation {
            selection_id: prepared.selection_id,
            status: ReconcileOperationStatus {
                profile: RECONCILE_STATUS_PROFILE,
                operation_token: operation_token.clone(),
                phase: ReconcileOperationPhase::Queued,
                cancellable: true,
                output_leaf: prepared.output_leaf,
                output_bytes: None,
                error: None,
            },
            cancellation: cancellation.clone(),
        });
        Ok(StartedReconcile {
            review: prepared.review,
            approved_plan: prepared.approved_plan,
            approved_payload: prepared.approved_payload,
        })
    }

    pub(crate) fn update_phase(
        &mut self,
        operation_token: &str,
        phase: ReconcileOperationPhase,
        cancellable: bool,
    ) -> Result<ReconcileOperationStatus, WorkspaceError> {
        validate_token(operation_token)?;
        let active = self.active.as_mut().ok_or_else(stale_plan)?;
        if !constant_time_equal(
            active.status.operation_token.as_bytes(),
            operation_token.as_bytes(),
        ) {
            return Err(stale_plan());
        }
        active.status.phase = phase;
        active.status.cancellable = cancellable;
        Ok(active.status.clone())
    }

    pub(crate) fn finish(
        &mut self,
        operation_token: &str,
        result: Result<u64, WorkspaceError>,
    ) -> Result<ReconcileOperationStatus, WorkspaceError> {
        let active = self.active.as_mut().ok_or_else(stale_plan)?;
        if !constant_time_equal(
            active.status.operation_token.as_bytes(),
            operation_token.as_bytes(),
        ) {
            return Err(stale_plan());
        }
        match result {
            Ok(bytes) => {
                active.status.phase = ReconcileOperationPhase::Succeeded;
                active.status.output_bytes = Some(bytes);
                active.status.error = None;
            }
            Err(error) => {
                active.status.phase = if error.kind() == WorkspaceErrorCode::Cancelled {
                    ReconcileOperationPhase::Cancelled
                } else {
                    ReconcileOperationPhase::Failed
                };
                active.status.error = Some(error);
            }
        }
        active.status.cancellable = false;
        Ok(active.status.clone())
    }

    pub(crate) fn status(
        &self,
        request: &ReconcileOperationRequest,
        current_selection: &str,
    ) -> Result<ReconcileOperationStatus, WorkspaceError> {
        validate_token(&request.operation_token)?;
        validate_token(current_selection)?;
        let active = self.active.as_ref().ok_or_else(stale_plan)?;
        if active.selection_id != current_selection
            || !constant_time_equal(
                active.status.operation_token.as_bytes(),
                request.operation_token.as_bytes(),
            )
        {
            return Err(stale_plan());
        }
        Ok(active.status.clone())
    }

    pub(crate) fn cancel(
        &self,
        request: &ReconcileOperationRequest,
        current_selection: &str,
    ) -> Result<(), WorkspaceError> {
        validate_token(current_selection)?;
        let active = self.active.as_ref().ok_or_else(stale_plan)?;
        if active.selection_id != current_selection
            || !constant_time_equal(
                active.status.operation_token.as_bytes(),
                request.operation_token.as_bytes(),
            )
            || !active.status.cancellable
        {
            return Err(stale_plan());
        }
        active.cancellation.cancel();
        Ok(())
    }

    pub(crate) fn acknowledge(
        &mut self,
        request: &ReconcileOperationRequest,
        current_selection: &str,
    ) -> Result<(), WorkspaceError> {
        validate_token(&request.operation_token)?;
        validate_token(current_selection)?;
        let terminal = self.active.as_ref().is_some_and(|active| {
            active.selection_id == current_selection
                && constant_time_equal(
                    active.status.operation_token.as_bytes(),
                    request.operation_token.as_bytes(),
                )
                && matches!(
                    active.status.phase,
                    ReconcileOperationPhase::Succeeded
                        | ReconcileOperationPhase::Failed
                        | ReconcileOperationPhase::Cancelled
                )
        });
        if !terminal {
            return Err(stale_plan());
        }
        self.active = None;
        Ok(())
    }

    pub(crate) fn invalidate_selection(&mut self, current_selection: Option<&str>) {
        if self
            .evidence
            .as_ref()
            .is_some_and(|item| Some(item.selection_id.as_str()) != current_selection)
        {
            self.evidence = None;
        }
        if self
            .human
            .as_ref()
            .is_some_and(|item| Some(item.selection_id.as_str()) != current_selection)
        {
            self.human = None;
        }
        if self
            .prepared
            .as_ref()
            .is_some_and(|item| Some(item.selection_id.as_str()) != current_selection)
        {
            self.prepared = None;
        }
        if self
            .three_way
            .as_ref()
            .is_some_and(|item| Some(item.selection_id.as_str()) != current_selection)
        {
            self.three_way = None;
        }
        if let Some(active) = &self.active
            && Some(active.selection_id.as_str()) != current_selection
            && active.status.cancellable
        {
            active.cancellation.cancel();
        }
    }

    pub(crate) fn prepare_for_close(&mut self) -> bool {
        self.evidence = None;
        self.human = None;
        self.three_way = None;
        self.prepared = None;
        match self.active.as_ref() {
            None => true,
            Some(active)
                if matches!(
                    active.status.phase,
                    ReconcileOperationPhase::Succeeded
                        | ReconcileOperationPhase::Failed
                        | ReconcileOperationPhase::Cancelled
                ) =>
            {
                self.active = None;
                true
            }
            Some(active) => {
                if active.status.cancellable {
                    active.cancellation.cancel();
                }
                false
            }
        }
    }
}

impl ReconcilePrepareJob {
    pub(crate) fn prepare(
        self,
    ) -> Result<(ReconcilePreparedShell, ReconcileReview), WorkspaceError> {
        match self {
            Self::TwoWay {
                source,
                target,
                report,
                selections,
                sensitive_datasets,
                output,
                cancellation,
                selection_id,
                output_leaf,
                destination_view,
                operation_deadline,
                operation_expires_at,
            } => {
                let source = *source;
                let target = *target;
                let report = *report;
                let remaining = remaining_operation_budget(operation_deadline, &cancellation)?;
                let review = prepare_reconcile_review(
                    source,
                    target,
                    &report,
                    &selections,
                    &sensitive_datasets,
                    &output,
                    &ReconcileReviewLimits {
                        deadline: remaining.min(EXECUTION_LIFETIME),
                        ..ReconcileReviewLimits::default()
                    },
                    &cancellation,
                )?;
                let shell = ReconcilePreparedShell {
                    selection_id,
                    output_leaf,
                    destination_view,
                    compare_report_digest: report.report_digest,
                    operation_deadline,
                    operation_expires_at,
                    cancellation,
                };
                Ok((shell, review))
            }
            Self::ThreeWay {
                review,
                resolutions,
                output,
                cancellation,
                selection_id,
                output_leaf,
                destination_view,
                operation_deadline,
                operation_expires_at,
                compare_report_digest,
            } => {
                let remaining = remaining_operation_budget(operation_deadline, &cancellation)?;
                let review =
                    (*review).resolve(&resolutions, &output, remaining.min(EXECUTION_LIFETIME))?;
                let shell = ReconcilePreparedShell {
                    selection_id,
                    output_leaf,
                    destination_view,
                    compare_report_digest,
                    operation_deadline,
                    operation_expires_at,
                    cancellation,
                };
                Ok((shell, review))
            }
        }
    }
}

impl ReconcileThreeWayJob {
    pub(crate) fn classify(
        self,
    ) -> Result<(ReconcileThreeWayShell, ThreeWayReconcileReview), WorkspaceError> {
        let Self {
            ancestor_path,
            source,
            target,
            report,
            sensitive_datasets,
            selection_id,
            review_token,
            ancestor_token,
            destination_view,
            output_path,
            classification_deadline,
            authority_deadline,
            authority_expires_at,
            output_leaf,
            cancellation,
        } = self;
        let open_budget = remaining_operation_budget(classification_deadline, &cancellation)?;
        let ancestor = VerifiedWorkspaceSource::open_with_control(
            &ancestor_path,
            &WorkspaceLimits {
                deadline: open_budget.min(EXECUTION_LIFETIME),
                ..WorkspaceLimits::default()
            },
            &cancellation,
        )?;
        let now = Instant::now();
        let remaining = remaining_operation_budget(classification_deadline, &cancellation)?;
        let review_lifetime = authority_deadline
            .checked_duration_since(now)
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(session_expired)?
            .min(HUMAN_REVIEW_LIFETIME);
        let review = classify_three_way_reconcile(
            ancestor,
            source,
            target,
            &report,
            &sensitive_datasets,
            &ReconcileReviewLimits {
                deadline: remaining.min(EXECUTION_LIFETIME),
                review_lifetime,
                ..ReconcileReviewLimits::default()
            },
            &cancellation,
        )?;
        let shell = ReconcileThreeWayShell {
            selection_id,
            review_token,
            ancestor_token,
            destination_view,
            output_path,
            classification_deadline,
            authority_deadline,
            authority_expires_at,
            output_leaf,
            compare_report_digest: report.report_digest,
            cancellation,
        };
        Ok((shell, review))
    }
}

/// Runs orientation work under the original compare deadline. Reversing the
/// pair recomputes the exact report; an old report is never relabelled.
pub(crate) fn orient_handoff(
    handoff: CompareReconcileHandoff,
    binding: &ReconcileHandoffBinding,
    cancellation: &CancellationToken,
) -> Result<OrientedReconcileHandoff, WorkspaceError> {
    if handoff.selection_id != binding.selection_id
        || handoff.session_id != binding.session_id
        || handoff.report.report_digest != binding.report_digest
    {
        return Err(stale_plan());
    }
    let remaining = handoff
        .compare_deadline
        .checked_duration_since(Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(session_expired)?;
    match binding.orientation {
        ReconcileOrientation::LeftToRight => Ok(OrientedReconcileHandoff {
            selection_id: handoff.selection_id,
            session_id: handoff.session_id,
            origin_report_digest: handoff.report.report_digest.clone(),
            report: handoff.report,
            source: handoff.left,
            target: handoff.right,
        }),
        ReconcileOrientation::RightToLeft => {
            let report = compare_sources(
                &handoff.right,
                &handoff.left,
                &CompareLimits {
                    deadline: Duration::from_millis(handoff.report.limits.deadline_ms),
                    operation_deadline: Some(remaining.min(EXECUTION_LIFETIME)),
                    max_rows_per_table: handoff.report.limits.max_rows_per_table,
                    max_total_rows: handoff.report.limits.max_total_rows,
                    max_value_bytes: handoff.report.limits.max_value_bytes,
                    max_stream_bytes: handoff.report.limits.max_stream_bytes,
                },
                cancellation,
            )?;
            Ok(OrientedReconcileHandoff {
                selection_id: handoff.selection_id,
                session_id: handoff.session_id,
                origin_report_digest: handoff.report.report_digest,
                report,
                source: handoff.right,
                target: handoff.left,
            })
        }
    }
}

impl StartedReconcile {
    pub(crate) fn execute<F>(self, mut progress: F) -> Result<u64, WorkspaceError>
    where
        F: FnMut(ReconcileOperationPhase, bool),
    {
        progress(ReconcileOperationPhase::Reverify, true);
        let prepared = self
            .review
            .prepare(self.approved_plan, &self.approved_payload)?;
        progress(ReconcileOperationPhase::Stage, true);
        let staged = prepared.stage()?;
        progress(ReconcileOperationPhase::Transform, true);
        let validated = staged.transform_and_validate()?;
        progress(ReconcileOperationPhase::Validate, true);
        progress(ReconcileOperationPhase::Publish, false);
        let published = validated.publish()?;
        progress(ReconcileOperationPhase::Postpublish, false);
        Ok(published.identity().bytes)
    }
}

fn selection_for_orientation(
    binding: &ReconcilePageBinding,
    row: &CompareRowDetail,
    source_is_left: bool,
) -> Result<Option<ReconcileSelection>, WorkspaceError> {
    let (source_digest, target_digest, kind) = if source_is_left {
        (row.left_digest.clone(), row.right_digest.clone(), row.kind)
    } else {
        let reversed = match row.kind {
            CompareDetailRowKind::Added => CompareDetailRowKind::Removed,
            CompareDetailRowKind::Removed => CompareDetailRowKind::Added,
            other => other,
        };
        (row.right_digest.clone(), row.left_digest.clone(), reversed)
    };
    let (action, field_indices) = match kind {
        CompareDetailRowKind::Removed => (ReconcileAction::InsertFromSource, Vec::new()),
        CompareDetailRowKind::Added => (ReconcileAction::DeleteFromTarget, Vec::new()),
        CompareDetailRowKind::Changed if binding.compare_policy == ComparePolicy::Row => {
            (ReconcileAction::ReplaceRowFromSource, Vec::new())
        }
        CompareDetailRowKind::Changed if binding.compare_policy == ComparePolicy::Field => {
            let indices = row
                .fields
                .iter()
                .enumerate()
                .filter_map(|(index, field)| {
                    (!matches!(field.kind, CompareDetailFieldKind::Same)
                        && !binding.table.primary_key.contains(&field.column)
                        && !binding.table.immutable_columns.contains(&field.column))
                    .then_some(index)
                })
                .collect::<Vec<_>>();
            if indices.is_empty() {
                return Ok(None);
            }
            (ReconcileAction::SetFields, indices)
        }
        CompareDetailRowKind::Unchanged => return Ok(None),
        CompareDetailRowKind::Changed => return Err(unsupported_operation()),
    };
    Ok(Some(ReconcileSelection {
        dataset_index: binding.dataset_index,
        table_index: binding.table_index,
        key_digest: row.key_digest.clone(),
        source_row_digest: source_digest,
        target_row_digest: target_digest,
        action,
        field_indices,
    }))
}

fn row_binding_key(binding: &ReconcilePageBinding, row: &CompareRowDetail) -> String {
    row_binding_key_from_selection(binding.dataset_index, binding.table_index, &row.key_digest)
}

fn row_binding_key_from_selection(dataset: usize, table: usize, digest: &str) -> String {
    format!("{dataset}:{table}:{digest}")
}

fn reference_view(
    reference: &sqlite_capsule_workspace::ReconcileReference,
) -> ReconcileReferenceView {
    ReconcileReferenceView {
        capsule_id: reference.capsule_id.clone(),
        revision_id: reference.revision_id.clone(),
        application_digest: reference.application_digest.clone(),
        signature_count: reference.signature_count,
        data_schema_id: reference.data_schema_id.clone(),
        data_schema_version: reference.data_schema_version,
    }
}

fn oriented_origin_matches(
    binding: &ReconcileHandoffBinding,
    selection_id: &str,
    session_id: &str,
    origin_report_digest: &str,
) -> bool {
    binding.selection_id == selection_id
        && binding.session_id == session_id
        && binding.report_digest == origin_report_digest
}

const fn action_label(action: ReconcileAction) -> &'static str {
    match action {
        ReconcileAction::InsertFromSource => "insert-from-source",
        ReconcileAction::DeleteFromTarget => "delete-from-target",
        ReconcileAction::ReplaceRowFromSource => "replace-row-from-source",
        ReconcileAction::SetFields => "set-fields",
    }
}

const fn conflict_kind_label(kind: ThreeWayConflictKind) -> &'static str {
    match kind {
        ThreeWayConflictKind::InsertInsert => "insert-insert",
        ThreeWayConflictKind::UpdateUpdate => "update-update",
        ThreeWayConflictKind::DeleteUpdate => "delete-update",
        ThreeWayConflictKind::ImmutableField => "immutable-field",
    }
}

const fn deleted_side_label(side: ThreeWayDeletedSide) -> &'static str {
    match side {
        ThreeWayDeletedSide::Source => "source",
        ThreeWayDeletedSide::Target => "target",
    }
}

const fn resolution_choice_label(choice: ThreeWayResolutionChoice) -> &'static str {
    match choice {
        ThreeWayResolutionChoice::KeepTarget => "keep-target",
        ThreeWayResolutionChoice::TakeSource => "take-source",
    }
}

fn ensure_human_current(session: &HumanSession) -> Result<(), WorkspaceError> {
    if Instant::now() >= session.deadline {
        Err(session_expired())
    } else {
        Ok(())
    }
}

fn deadline_from_remaining_at(
    remaining: Duration,
    now: Instant,
) -> Result<Instant, WorkspaceError> {
    if remaining.is_zero() || remaining > EXECUTION_LIFETIME {
        return Err(session_expired());
    }
    now.checked_add(remaining).ok_or_else(limit_exceeded)
}

fn authority_deadline_from_remaining_at(
    remaining: Duration,
    now: Instant,
) -> Result<Instant, WorkspaceError> {
    if remaining.is_zero() || remaining > HUMAN_REVIEW_LIFETIME {
        return Err(session_expired());
    }
    now.checked_add(remaining).ok_or_else(limit_exceeded)
}

fn clamp_operation_deadline(
    authority_deadline: Instant,
    requested_deadline: Instant,
    now: Instant,
) -> Result<Instant, WorkspaceError> {
    if now >= authority_deadline {
        return Err(session_expired());
    }
    if now >= requested_deadline {
        return Err(limit_exceeded());
    }
    Ok(authority_deadline.min(requested_deadline))
}

fn remaining_operation_budget(
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<Duration, WorkspaceError> {
    if cancellation.is_cancelled() {
        return Err(WorkspaceError::new(WorkspaceErrorCode::Cancelled));
    }
    deadline
        .checked_duration_since(Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(limit_exceeded)
}

fn ensure_operation_current(
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<(), WorkspaceError> {
    remaining_operation_budget(deadline, cancellation).map(|_| ())
}

fn token_digest(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

fn validate_resolution_count(actual: usize, expected: usize) -> Result<(), WorkspaceError> {
    if actual > MAX_SELECTION_AUTHORITIES {
        Err(invalid_contract())
    } else if actual != expected {
        Err(WorkspaceError::new(WorkspaceErrorCode::ConflictsUnresolved))
    } else {
        Ok(())
    }
}

fn validate_token(value: &str) -> Result<(), WorkspaceError> {
    if value.len() == 43
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        Ok(())
    } else {
        Err(invalid_contract())
    }
}

fn validate_digest(value: &str) -> Result<(), WorkspaceError> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        Ok(())
    } else {
        Err(invalid_contract())
    }
}

fn random_token() -> Result<String, WorkspaceError> {
    let mut random = [0_u8; 32];
    getrandom::fill(&mut random).map_err(|_| internal_error())?;
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut output = String::with_capacity(43);
    for chunk in random.chunks(3) {
        let mut block = u32::from(chunk[0]) << 16;
        if let Some(value) = chunk.get(1) {
            block |= u32::from(*value) << 8;
        }
        if let Some(value) = chunk.get(2) {
            block |= u32::from(*value);
        }
        output.push(ALPHABET[((block >> 18) & 63) as usize] as char);
        output.push(ALPHABET[((block >> 12) & 63) as usize] as char);
        if chunk.len() >= 2 {
            output.push(ALPHABET[((block >> 6) & 63) as usize] as char);
        }
        if chunk.len() == 3 {
            output.push(ALPHABET[(block & 63) as usize] as char);
        }
    }
    validate_token(&output)?;
    Ok(output)
}

fn bounded_display(value: &str) -> String {
    value.chars().take(MAX_DISPLAY_CHARS).collect()
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    for index in 0..left.len().max(right.len()) {
        difference |= usize::from(
            left.get(index).copied().unwrap_or(0) ^ right.get(index).copied().unwrap_or(0),
        );
    }
    difference == 0
}

const fn invalid_contract() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::InvalidContract)
}

const fn stale_plan() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::StalePlan)
}

const fn session_expired() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::SessionExpired)
}

const fn unsupported_operation() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::UnsupportedOperation)
}

const fn limit_exceeded() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::LimitExceeded)
}

const fn internal_error() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::InternalError)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn token(character: char) -> String {
        std::iter::repeat_n(character, 43).collect()
    }

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "sqlite-capsule-reconcile-shell-{label}-{}-{}",
                std::process::id(),
                random_token().expect("random test suffix")
            ));
            std::fs::create_dir(&path).expect("create test directory");
            Self(path)
        }

        fn fixture(&self, leaf: &str, capsule_id: &str, revision_id: &str) -> PathBuf {
            let path = self.0.join(leaf);
            let connection = Connection::open(&path).expect("create fixture");
            connection
                .execute_batch(include_str!("../../../../format/capsule-v0.3.sql"))
                .expect("install v0.3 schema");
            connection
                .execute_batch(include_str!(
                    "../../../../format/capsule-signed-app-v0.3.sql"
                ))
                .expect("install signed-app schema");
            connection
                .execute_batch(include_str!(
                    "../../../../compatibility/signed-app-v0.3/fixture-v0.3.sql"
                ))
                .expect("install signed fixture");
            connection
                .execute(
                    "UPDATE capsule_instance SET capsule_id=?1, revision_id=?2",
                    [capsule_id, revision_id],
                )
                .expect("set distinct mutable identity");
            drop(connection);
            path
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn renderer_requests_are_token_only_and_closed() {
        let request = serde_json::from_value::<PrepareReconcileRequest>(serde_json::json!({
            "review_token": token('A'),
            "destination_token": token('B'),
            "selection_tokens": [token('C')]
        }))
        .expect("closed token request");
        assert_eq!(request.selection_tokens.len(), 1);
        for forbidden in [
            "path", "plan", "payload", "digest", "index", "table", "sql", "value",
        ] {
            assert!(
                serde_json::from_value::<PrepareReconcileRequest>(serde_json::json!({
                    "review_token": token('A'),
                    "destination_token": token('B'),
                    "selection_tokens": [token('C')],
                    (forbidden): "attacker-controlled"
                }))
                .is_err(),
                "accepted forbidden field {forbidden}"
            );
        }
        let three_way = serde_json::from_value::<PrepareReconcileRequest>(serde_json::json!({
            "review_token": token('A'),
            "destination_token": token('B'),
            "selection_tokens": [],
            "ancestor_token": token('D'),
            "resolution_tokens": [token('E')]
        }))
        .expect("closed three-way token request");
        assert_eq!(three_way.ancestor_token, Some(token('D')));
        assert_eq!(three_way.resolution_tokens, vec![token('E')]);
        assert!(
            serde_json::from_value::<ChooseReconcileAncestorRequest>(serde_json::json!({
                "review_token": token('A'),
                "destination_token": token('B'),
                "path": "C:/attacker-controlled.sqlitecapsule"
            }))
            .is_err()
        );
    }

    #[test]
    fn orientation_capabilities_are_one_session_and_selection_bound() {
        let mut controller = ReconcileController::default();
        controller
            .begin_compare_evidence(
                &token('S'),
                &token('C'),
                &"a".repeat(64),
                "selected".to_owned(),
                "comparison".to_owned(),
                true,
            )
            .unwrap();
        let options = controller
            .options(
                &ReconcileOptionsRequest {
                    session_token: token('C'),
                },
                &token('S'),
            )
            .unwrap();
        assert_ne!(
            options.orientations[0].orientation_token,
            options.orientations[1].orientation_token
        );
        let request = StartReconcileRequest {
            orientation_token: options.orientations[0].orientation_token.clone(),
        };
        assert_eq!(
            controller
                .authorize_handoff(&request, &token('X'))
                .unwrap_err()
                .kind(),
            WorkspaceErrorCode::StalePlan
        );
        let binding = controller.authorize_handoff(&request, &token('S')).unwrap();
        assert_eq!(binding.session_id, token('C'));
        controller
            .begin_compare_evidence(
                &token('S'),
                &token('D'),
                &"b".repeat(64),
                "selected".to_owned(),
                "other".to_owned(),
                true,
            )
            .unwrap();
        assert_eq!(
            controller
                .authorize_handoff(&request, &token('S'))
                .unwrap_err()
                .kind(),
            WorkspaceErrorCode::StalePlan
        );
    }

    #[test]
    fn both_orientations_bind_the_original_report_not_the_execution_digest() {
        for orientation in [
            ReconcileOrientation::LeftToRight,
            ReconcileOrientation::RightToLeft,
        ] {
            let binding = ReconcileHandoffBinding {
                session_id: token('C'),
                selection_id: token('S'),
                report_digest: "a".repeat(64),
                orientation,
                orientation_token: token('O'),
            };
            assert!(oriented_origin_matches(
                &binding,
                &token('S'),
                &token('C'),
                &"a".repeat(64),
            ));
            assert!(!oriented_origin_matches(
                &binding,
                &token('S'),
                &token('C'),
                &"b".repeat(64),
            ));
        }
    }

    #[test]
    fn review_and_execution_deadlines_are_separate_and_bounded() {
        assert_eq!(HUMAN_REVIEW_LIFETIME, Duration::from_secs(300));
        assert_eq!(EXECUTION_LIFETIME, Duration::from_secs(30));
        assert!(EXECUTION_LIFETIME < HUMAN_REVIEW_LIFETIME);
        let now = Instant::now();
        let delayed_remaining = Duration::from_secs(17);
        assert_eq!(
            deadline_from_remaining_at(delayed_remaining, now)
                .unwrap()
                .saturating_duration_since(now),
            delayed_remaining,
            "planning delay must not remint the 30-second authority"
        );
        assert_eq!(
            deadline_from_remaining_at(Duration::ZERO, now)
                .unwrap_err()
                .kind(),
            WorkspaceErrorCode::SessionExpired
        );
        assert_eq!(
            deadline_from_remaining_at(EXECUTION_LIFETIME + Duration::from_millis(1), now)
                .unwrap_err()
                .kind(),
            WorkspaceErrorCode::SessionExpired
        );
    }

    #[test]
    fn three_way_human_authority_survives_classification_budget_but_expires_exactly() {
        let classified_at = Instant::now();
        let authority_deadline =
            authority_deadline_from_remaining_at(HUMAN_REVIEW_LIFETIME, classified_at).unwrap();
        let delayed_review = classified_at + EXECUTION_LIFETIME + Duration::from_secs(1);
        let requested_work = delayed_review + EXECUTION_LIFETIME;
        assert_eq!(
            clamp_operation_deadline(authority_deadline, requested_work, delayed_review).unwrap(),
            requested_work,
            "a conflict review delayed beyond classification still receives fresh bounded work"
        );
        assert_eq!(
            clamp_operation_deadline(
                authority_deadline,
                authority_deadline + EXECUTION_LIFETIME,
                authority_deadline,
            )
            .unwrap_err()
            .kind(),
            WorkspaceErrorCode::SessionExpired,
            "the exact human-review expiry must fail closed"
        );
        let work_deadline = delayed_review + Duration::from_secs(1);
        assert_eq!(
            clamp_operation_deadline(authority_deadline, work_deadline, work_deadline,)
                .unwrap_err()
                .kind(),
            WorkspaceErrorCode::LimitExceeded,
            "the exact prepare work expiry must remain distinct from authority expiry"
        );
        let cancelled = CancellationToken::new();
        cancelled.cancel();
        assert_eq!(
            remaining_operation_budget(work_deadline, &cancelled)
                .unwrap_err()
                .kind(),
            WorkspaceErrorCode::Cancelled
        );
    }

    #[test]
    fn raw_values_and_private_authority_are_absent_from_views() {
        let view = ReconcileSelectionView {
            selection_token: token('R'),
            dataset_label: "dataset".to_owned(),
            table_label: "table".to_owned(),
            action: "set-fields",
            field_count: 2,
            sensitivity: Sensitivity::Normal,
            sensitive_reveal_confirmed: true,
        };
        let json = serde_json::to_string(&view).unwrap();
        for forbidden in [
            "key_digest",
            "row_digest",
            "field_indices",
            "sql",
            "path",
            "payload",
        ] {
            assert!(!json.contains(forbidden));
        }
        let conflict = ReconcileConflictView {
            conflict_token: token('C'),
            dataset_label: "content".to_owned(),
            table_label: "records".to_owned(),
            kind: "update-update",
            deleted_side: None,
            choices: vec![ReconcileResolutionChoiceView {
                resolution_token: token('R'),
                choice: "keep-target",
            }],
        };
        let json = serde_json::to_string(&conflict).unwrap();
        for forbidden in [
            "conflict_id",
            "key_digest",
            "source_row_digest",
            "target_row_digest",
            "ancestor_row_digest",
            "value",
        ] {
            assert!(!json.contains(forbidden));
        }
        let operation = ReconcileOperationReviewView {
            sequence: 10_000,
            dataset_label: "content".to_owned(),
            table_label: "records".to_owned(),
            action: "set-fields",
            field_count: 1,
            sensitive_confirmed: false,
        };
        let json = serde_json::to_value(operation).unwrap();
        assert_eq!(json["sequence"], 10_000);
    }

    #[test]
    fn three_way_sensitive_evidence_is_exact_dataset_bound() {
        let directory = TestDirectory::new("three-way-sensitive-evidence");
        let path = directory.fixture(
            "sensitive.sqlitecapsule",
            "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
            "aaaaaaaa-aaaa-4aaa-9aaa-aaaaaaaaaaaa",
        );
        let source = VerifiedWorkspaceSource::open(&path).unwrap();
        let selection_id = token('S');
        let session_id = token('C');
        let report_digest = "a".repeat(64);
        let row = CompareRowDetail {
            kind: CompareDetailRowKind::Changed,
            key_digest: "b".repeat(64),
            left_digest: Some("c".repeat(64)),
            right_digest: Some("d".repeat(64)),
            fields: Vec::new(),
        };
        let mut controller = ReconcileController::default();
        controller
            .begin_compare_evidence(
                &selection_id,
                &session_id,
                &report_digest,
                "source".to_owned(),
                "target".to_owned(),
                true,
            )
            .unwrap();
        let revealed = ReconcilePageBinding {
            selection_id: selection_id.clone(),
            session_id: session_id.clone(),
            report_digest: report_digest.clone(),
            dataset_index: 0,
            table_index: 0,
            dataset_label: "first-sensitive".to_owned(),
            table_label: "first-table".to_owned(),
            compare_policy: ComparePolicy::Field,
            reconcile_policy: ReconcilePolicy::ThreeWay,
            sensitivity: Sensitivity::Sensitive,
            table: source.data_contract().datasets[0].tables[0].clone(),
        };
        controller
            .record_compare_page(&revealed, true, std::slice::from_ref(&row))
            .unwrap();
        let withheld = ReconcilePageBinding {
            dataset_index: 1,
            dataset_label: "second-sensitive".to_owned(),
            table_label: "second-table".to_owned(),
            table: source.data_contract().datasets[1].tables[0].clone(),
            ..revealed
        };
        assert_eq!(
            controller
                .record_compare_page(&withheld, false, std::slice::from_ref(&row))
                .unwrap_err()
                .kind(),
            WorkspaceErrorCode::UnsupportedOperation
        );
        let evidence = controller.evidence.as_ref().unwrap();
        assert_eq!(evidence.three_way_sensitive_datasets, BTreeSet::from([0]));
        assert_eq!(evidence.three_way_sensitive_rows.len(), 1);
    }

    #[test]
    fn unchanged_sensitive_three_way_page_grants_no_dataset_confirmation() {
        let directory = TestDirectory::new("three-way-unchanged-sensitive-evidence");
        let path = directory.fixture(
            "evidence.sqlitecapsule",
            "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
            "aaaaaaaa-aaaa-4aaa-9aaa-aaaaaaaaaaaa",
        );
        let source = VerifiedWorkspaceSource::open(&path).unwrap();
        let selection_id = token('S');
        let session_id = token('C');
        let report_digest = "a".repeat(64);
        let changed = CompareRowDetail {
            kind: CompareDetailRowKind::Changed,
            key_digest: "b".repeat(64),
            left_digest: Some("c".repeat(64)),
            right_digest: Some("d".repeat(64)),
            fields: Vec::new(),
        };
        let unchanged = CompareRowDetail {
            kind: CompareDetailRowKind::Unchanged,
            key_digest: "e".repeat(64),
            left_digest: Some("f".repeat(64)),
            right_digest: Some("f".repeat(64)),
            fields: Vec::new(),
        };
        let mut controller = ReconcileController::default();
        controller
            .begin_compare_evidence(
                &selection_id,
                &session_id,
                &report_digest,
                "source".to_owned(),
                "target".to_owned(),
                true,
            )
            .unwrap();
        let sensitive = ReconcilePageBinding {
            selection_id: selection_id.clone(),
            session_id: session_id.clone(),
            report_digest: report_digest.clone(),
            dataset_index: 0,
            table_index: 0,
            dataset_label: "sensitive".to_owned(),
            table_label: "sensitive-table".to_owned(),
            compare_policy: ComparePolicy::Field,
            reconcile_policy: ReconcilePolicy::ThreeWay,
            sensitivity: Sensitivity::Sensitive,
            table: source.data_contract().datasets[0].tables[0].clone(),
        };
        controller
            .record_compare_page(&sensitive, true, &[unchanged])
            .unwrap();
        let normal = ReconcilePageBinding {
            selection_id,
            session_id,
            report_digest,
            dataset_index: 1,
            table_index: 0,
            dataset_label: "normal".to_owned(),
            table_label: "normal-table".to_owned(),
            compare_policy: ComparePolicy::Field,
            reconcile_policy: ReconcilePolicy::ThreeWay,
            sensitivity: Sensitivity::Normal,
            table: source.data_contract().datasets[1].tables[0].clone(),
        };
        controller
            .record_compare_page(&normal, false, std::slice::from_ref(&changed))
            .unwrap();
        let evidence = controller.evidence.as_ref().unwrap();
        assert!(evidence.three_way_sensitive_datasets.is_empty());
        assert!(evidence.three_way_sensitive_rows.is_empty());
        assert_eq!(evidence.three_way_rows.len(), 1);

        controller
            .record_compare_page(&sensitive, true, std::slice::from_ref(&changed))
            .unwrap();
        let evidence = controller.evidence.as_ref().unwrap();
        assert_eq!(evidence.three_way_sensitive_datasets, BTreeSet::from([0]));
        assert_eq!(evidence.three_way_sensitive_rows.len(), 1);
    }

    #[test]
    fn right_to_left_session_targets_left_identity_and_prepares_delete() {
        let directory = TestDirectory::new("right-to-left");
        let left_capsule = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
        let right_capsule = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
        let left_path = directory.fixture(
            "left.sqlitecapsule",
            left_capsule,
            "aaaaaaaa-aaaa-4aaa-9aaa-aaaaaaaaaaaa",
        );
        let right_path = directory.fixture(
            "right.sqlitecapsule",
            right_capsule,
            "bbbbbbbb-bbbb-4bbb-9bbb-bbbbbbbbbbbb",
        );
        Connection::open(&left_path)
            .unwrap()
            .execute(
                "INSERT INTO vector_settings VALUES ('left-only','delete in reverse')",
                [],
            )
            .unwrap();
        let left = VerifiedWorkspaceSource::open(&left_path).unwrap();
        let right = VerifiedWorkspaceSource::open(&right_path).unwrap();
        let report = compare_sources(
            &left,
            &right,
            &CompareLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        assert!(report.compatibility.can_reconcile);
        let page = sqlite_capsule_workspace::comparison_detail_page(
            &left,
            &right,
            1,
            0,
            None,
            false,
            &sqlite_capsule_workspace::CompareDetailLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        let row = page
            .rows
            .iter()
            .find(|row| row.kind == CompareDetailRowKind::Removed)
            .expect("left-only row");
        let selection_id = token('S');
        let session_id = token('C');
        let table = left.data_contract().datasets[1].tables[0].clone();
        let page_binding = ReconcilePageBinding {
            selection_id: selection_id.clone(),
            session_id: session_id.clone(),
            report_digest: report.report_digest.clone(),
            dataset_index: 1,
            table_index: 0,
            dataset_label: "settings".to_owned(),
            table_label: "vector_settings".to_owned(),
            compare_policy: ComparePolicy::Row,
            reconcile_policy: ReconcilePolicy::Manual,
            sensitivity: Sensitivity::Normal,
            table,
        };
        let mut controller = ReconcileController::default();
        controller
            .begin_compare_evidence(
                &selection_id,
                &session_id,
                &report.report_digest,
                "left".to_owned(),
                "right".to_owned(),
                true,
            )
            .unwrap();
        controller
            .record_compare_page(&page_binding, false, std::slice::from_ref(row))
            .unwrap();
        let options = controller
            .options(
                &ReconcileOptionsRequest {
                    session_token: session_id.clone(),
                },
                &selection_id,
            )
            .unwrap();
        let binding = controller
            .authorize_handoff(
                &StartReconcileRequest {
                    orientation_token: options.orientations[1].orientation_token.clone(),
                },
                &selection_id,
            )
            .unwrap();
        let handoff = CompareReconcileHandoff {
            selection_id: selection_id.clone(),
            session_id,
            report,
            left,
            right,
            compare_deadline: Instant::now() + EXECUTION_LIFETIME,
        };
        let oriented = orient_handoff(handoff, &binding, &CancellationToken::new()).unwrap();
        assert_eq!(oriented.report.right.capsule_id, left_capsule);
        let review_token = token('R');
        let session = controller
            .retain_human_session(
                binding,
                oriented,
                review_token.clone(),
                "2030-01-01T00:00:00Z".to_owned(),
            )
            .unwrap();
        assert_eq!(session.output_capsule_id, left_capsule);
        assert_eq!(session.selections.len(), 1);
        assert_eq!(session.selections[0].action, "delete-from-target");
        let destination = directory.0.join("reverse-output.sqlitecapsule");
        let destination_token = token('D');
        controller
            .retain_destination(
                &ReconcileSessionRequest {
                    review_token: review_token.clone(),
                },
                &selection_id,
                destination,
                destination_token.clone(),
            )
            .unwrap();
        let created_at = crate::current_utc_seconds().unwrap();
        let expires_at =
            crate::utc_seconds_at(std::time::SystemTime::now() + EXECUTION_LIFETIME).unwrap();
        let job = controller
            .take_prepare_job(
                &PrepareReconcileRequest {
                    review_token,
                    destination_token,
                    selection_tokens: vec![session.selections[0].selection_token.clone()],
                    ancestor_token: None,
                    resolution_tokens: Vec::new(),
                },
                &selection_id,
                "cccccccc-cccc-4ccc-8ccc-cccccccccccc".to_owned(),
                created_at,
                expires_at,
                Instant::now() + EXECUTION_LIFETIME,
            )
            .unwrap();
        let (_, review) = job.prepare().unwrap();
        assert_eq!(review.target().capsule_id, left_capsule);
        assert_eq!(
            review.operations()[0].action,
            ReconcileAction::DeleteFromTarget
        );
    }

    #[test]
    fn token_validation_rejects_short_and_non_url_safe_values() {
        assert!(validate_token(&token('A')).is_ok());
        assert_eq!(
            validate_token("short").unwrap_err().kind(),
            WorkspaceErrorCode::InvalidContract
        );
        assert_eq!(
            validate_token(&format!("{}!", "A".repeat(42)))
                .unwrap_err()
                .kind(),
            WorkspaceErrorCode::InvalidContract
        );
    }

    #[test]
    fn resolution_count_is_bounded_before_token_allocation_or_lookup() {
        assert!(validate_resolution_count(10_000, 10_000).is_ok());
        assert_eq!(
            validate_resolution_count(10_001, 10_001)
                .unwrap_err()
                .kind(),
            WorkspaceErrorCode::InvalidContract
        );
        assert_eq!(
            validate_resolution_count(1, 2).unwrap_err().kind(),
            WorkspaceErrorCode::ConflictsUnresolved
        );
    }

    #[test]
    fn one_cancellation_token_is_shared_from_planning_into_active_execution() {
        // `take_prepare_job` creates this token; the review clones it and the
        // prepared shell moves the same shared token into ActiveOperation.
        let planning_token = CancellationToken::new();
        let review_token = planning_token.clone();
        let prepared_token = planning_token.clone();
        let active_token = prepared_token;
        active_token.cancel();
        assert!(planning_token.is_cancelled());
        assert!(review_token.is_cancelled());
    }

    #[test]
    fn cancellation_is_open_prepublish_and_closed_for_publication_tail() {
        let selection = token('S');
        let operation = token('O');
        let cancellation = CancellationToken::new();
        let mut controller = ReconcileController {
            active: Some(ActiveOperation {
                selection_id: selection.clone(),
                status: ReconcileOperationStatus {
                    profile: RECONCILE_STATUS_PROFILE,
                    operation_token: operation.clone(),
                    phase: ReconcileOperationPhase::Transform,
                    cancellable: true,
                    output_leaf: "new-copy.sqlitecapsule".to_owned(),
                    output_bytes: None,
                    error: None,
                },
                cancellation: cancellation.clone(),
            }),
            ..ReconcileController::default()
        };
        let request = ReconcileOperationRequest {
            operation_token: operation.clone(),
        };
        controller.cancel(&request, &selection).unwrap();
        assert!(cancellation.is_cancelled());

        let tail_cancellation = CancellationToken::new();
        controller.active = Some(ActiveOperation {
            selection_id: selection.clone(),
            status: ReconcileOperationStatus {
                profile: RECONCILE_STATUS_PROFILE,
                operation_token: operation,
                phase: ReconcileOperationPhase::Publish,
                cancellable: false,
                output_leaf: "new-copy.sqlitecapsule".to_owned(),
                output_bytes: None,
                error: None,
            },
            cancellation: tail_cancellation.clone(),
        });
        assert_eq!(
            controller.cancel(&request, &selection).unwrap_err().kind(),
            WorkspaceErrorCode::StalePlan
        );
        assert!(!tail_cancellation.is_cancelled());
        assert!(!controller.prepare_for_close());
        assert!(!tail_cancellation.is_cancelled());
    }
}

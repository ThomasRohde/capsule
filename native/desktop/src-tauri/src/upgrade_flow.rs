//! Trusted-shell same-schema application-upgrade controller.
//!
//! Renderer requests contain only opaque capabilities, booleans and one-use
//! confirmation material. Paths, publisher key selection, verified inputs,
//! canonical plans and upgrade authority remain host-owned Rust state.

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlite_capsule_lifecycle::SourceIdentity;
use sqlite_capsule_workspace::{
    CancellationToken, LifecyclePlan, UpgradeApproval, UpgradePlanRequest, UpgradeReview,
    UpgradeReviewReport, VerifiedWorkspaceSource, WorkspaceError, WorkspaceErrorCode,
    WorkspaceLimits, parse_upgrade_plan, prepare_upgrade_review,
};

pub(crate) const UPGRADE_CANDIDATE_PROFILE: &str = "org.sqlite-capsule.tauri-upgrade-candidate/1";
pub(crate) const UPGRADE_REVIEW_PROFILE: &str = "org.sqlite-capsule.tauri-upgrade-review/1";
pub(crate) const UPGRADE_STATUS_PROFILE: &str = "org.sqlite-capsule.tauri-upgrade-status/1";
pub(crate) const UPGRADE_PROGRESS_EVENT: &str = "capsule-upgrade-progress-v1";
const REVIEW_LIFETIME: Duration = Duration::from_secs(5 * 60);
pub(crate) const EXECUTION_LIFETIME: Duration = Duration::from_secs(30);
const MAX_DISPLAY_CHARS: usize = 512;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChooseUpgradeReleaseRequest {
    pub selection_id: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChooseUpgradeDestinationRequest {
    pub candidate_token: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PrepareUpgradeRequest {
    pub selection_id: String,
    pub candidate_token: String,
    pub destination_token: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExecuteUpgradeRequest {
    pub plan_id: String,
    pub confirmation_nonce: String,
    pub publisher_key_confirmed: bool,
    pub capability_changes_confirmed: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UpgradeOperationRequest {
    pub operation_token: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct UpgradeCandidateView {
    pub profile: &'static str,
    pub candidate_token: String,
    pub source_version: String,
    pub target_version: String,
    pub app_id: String,
    pub data_schema_id: String,
    pub data_schema_version: i64,
    pub publisher_key_id: String,
    pub release_file_display: String,
    pub expires_at: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct UpgradeDestinationView {
    pub destination_token: String,
    pub parent_display: &'static str,
    pub leaf_display: String,
    pub expires_at: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct PreparedUpgradeView {
    pub profile: &'static str,
    pub plan_id: String,
    pub plan_digest: String,
    pub confirmation_nonce: String,
    pub output: UpgradeDestinationView,
    pub review: UpgradeReviewReport,
    pub checks: [&'static str; 8],
    pub expires_at: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum UpgradeOperationPhase {
    Queued,
    Reverify,
    Stage,
    Rebase,
    Validate,
    Publish,
    Postpublish,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct UpgradeProgressEvent {
    pub profile: &'static str,
    pub operation_token: String,
    pub sequence: u64,
    pub phase: UpgradeOperationPhase,
    pub cancellable: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct UpgradeOperationStatus {
    pub profile: &'static str,
    pub operation_token: String,
    pub phase: UpgradeOperationPhase,
    pub cancellable: bool,
    pub output_leaf: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<WorkspaceError>,
}

struct CandidateAuthority {
    selection_id: String,
    target_path: PathBuf,
    candidate_token: String,
    publisher_key_id: String,
    deadline: Instant,
}

struct DestinationAuthority {
    candidate_token: String,
    destination_token: String,
    path: PathBuf,
    leaf_display: String,
    expires_at: String,
    deadline: Instant,
}

struct PreparedAuthority {
    selection_id: String,
    plan_id: String,
    nonce_sha256: [u8; 32],
    deadline: Instant,
    review: UpgradeReview,
    approved: LifecyclePlan,
    source: VerifiedWorkspaceSource,
    target: VerifiedWorkspaceSource,
    publisher_key_id: String,
    capability_review_required: bool,
    output_leaf: String,
}

struct ActiveOperation {
    selection_id: String,
    status: UpgradeOperationStatus,
    cancellation: CancellationToken,
}

#[derive(Default)]
pub(crate) struct UpgradeController {
    candidate: Option<CandidateAuthority>,
    destination: Option<DestinationAuthority>,
    prepared: Option<PreparedAuthority>,
    active: Option<ActiveOperation>,
}

#[derive(Clone, Default)]
pub(crate) struct UpgradeState(pub(crate) Arc<Mutex<UpgradeController>>);

pub(crate) struct StartedUpgrade {
    review: UpgradeReview,
    approved: LifecyclePlan,
    approval: UpgradeApproval,
    source: VerifiedWorkspaceSource,
    target: VerifiedWorkspaceSource,
    cancellation: CancellationToken,
}

impl UpgradeController {
    pub(crate) fn invalidate_selection(&mut self, current_selection: Option<&str>) {
        if self
            .candidate
            .as_ref()
            .is_some_and(|item| Some(item.selection_id.as_str()) != current_selection)
        {
            self.candidate = None;
            self.destination = None;
            self.prepared = None;
        }
        if self
            .prepared
            .as_ref()
            .is_some_and(|item| Some(item.selection_id.as_str()) != current_selection)
        {
            self.prepared = None;
        }
        if let Some(active) = self.active.as_ref()
            && Some(active.selection_id.as_str()) != current_selection
            && active.status.cancellable
        {
            active.cancellation.cancel();
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn retain_candidate(
        &mut self,
        selection_id: &str,
        target_path: PathBuf,
        candidate_token: String,
        publisher_key_id: String,
        source_version: String,
        target_version: String,
        app_id: String,
        data_schema_id: String,
        data_schema_version: i64,
        expires_at: String,
    ) -> Result<UpgradeCandidateView, WorkspaceError> {
        validate_token(selection_id)?;
        validate_token(&candidate_token)?;
        if !target_path.is_absolute()
            || publisher_key_id.is_empty()
            || publisher_key_id.len() > 1_024
        {
            return Err(invalid_contract());
        }
        let release_file_display = target_path
            .file_name()
            .and_then(|value| value.to_str())
            .map(bounded_display)
            .ok_or_else(invalid_contract)?;
        self.destination = None;
        self.prepared = None;
        self.candidate = Some(CandidateAuthority {
            selection_id: selection_id.to_owned(),
            target_path,
            candidate_token: candidate_token.clone(),
            publisher_key_id: publisher_key_id.clone(),
            deadline: Instant::now()
                .checked_add(REVIEW_LIFETIME)
                .ok_or_else(limit_exceeded)?,
        });
        Ok(UpgradeCandidateView {
            profile: UPGRADE_CANDIDATE_PROFILE,
            candidate_token,
            source_version,
            target_version,
            app_id,
            data_schema_id,
            data_schema_version,
            publisher_key_id,
            release_file_display,
            expires_at,
        })
    }

    pub(crate) fn candidate_target_path(
        &self,
        candidate_token: &str,
    ) -> Result<PathBuf, WorkspaceError> {
        validate_token(candidate_token)?;
        let candidate = self.candidate.as_ref().ok_or_else(stale_plan)?;
        if Instant::now() >= candidate.deadline {
            return Err(session_expired());
        }
        if !constant_time_equal(
            candidate.candidate_token.as_bytes(),
            candidate_token.as_bytes(),
        ) {
            return Err(stale_plan());
        }
        Ok(candidate.target_path.clone())
    }

    pub(crate) fn retain_destination(
        &mut self,
        candidate_token: &str,
        destination_token: String,
        path: PathBuf,
        expires_at: String,
    ) -> Result<UpgradeDestinationView, WorkspaceError> {
        validate_token(candidate_token)?;
        validate_token(&destination_token)?;
        let candidate = self.candidate.as_ref().ok_or_else(stale_plan)?;
        if Instant::now() >= candidate.deadline
            || !constant_time_equal(
                candidate.candidate_token.as_bytes(),
                candidate_token.as_bytes(),
            )
            || !path.is_absolute()
            || path.exists()
            || !path.parent().is_some_and(Path::is_dir)
        {
            return Err(stale_plan());
        }
        let leaf_display = path
            .file_name()
            .and_then(|value| value.to_str())
            .map(bounded_display)
            .ok_or_else(invalid_contract)?;
        self.prepared = None;
        self.destination = Some(DestinationAuthority {
            candidate_token: candidate_token.to_owned(),
            destination_token: destination_token.clone(),
            path,
            leaf_display: leaf_display.clone(),
            expires_at: expires_at.clone(),
            deadline: Instant::now()
                .checked_add(REVIEW_LIFETIME)
                .ok_or_else(limit_exceeded)?,
        });
        Ok(UpgradeDestinationView {
            destination_token,
            parent_display: "Selected local folder",
            leaf_display,
            expires_at,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare(
        &mut self,
        request: &PrepareUpgradeRequest,
        source_path: &Path,
        source_identity: &SourceIdentity,
        source_sha256: &str,
        plan_id: &str,
        created_at: &str,
        expires_at: &str,
        confirmation_nonce: &str,
    ) -> Result<PreparedUpgradeView, WorkspaceError> {
        validate_token(&request.candidate_token)?;
        validate_token(&request.destination_token)?;
        validate_token(&request.selection_id)?;
        validate_token(confirmation_nonce)?;
        let candidate = self.candidate.take().ok_or_else(stale_plan)?;
        let destination = self.destination.take().ok_or_else(stale_plan)?;
        if Instant::now() >= candidate.deadline
            || Instant::now() >= destination.deadline
            || !constant_time_equal(
                candidate.candidate_token.as_bytes(),
                request.candidate_token.as_bytes(),
            )
            || !constant_time_equal(
                destination.destination_token.as_bytes(),
                request.destination_token.as_bytes(),
            )
            || destination.candidate_token != candidate.candidate_token
            || request.selection_id != candidate.selection_id
        {
            return Err(stale_plan());
        }
        let limits = WorkspaceLimits::default();
        let cancellation = CancellationToken::new();
        let source =
            VerifiedWorkspaceSource::open_with_control(source_path, &limits, &cancellation)?;
        source.assert_source_binding(source_identity, source_sha256)?;
        let target = VerifiedWorkspaceSource::open_with_control(
            &candidate.target_path,
            &limits,
            &cancellation,
        )?;
        let review = prepare_upgrade_review(
            &source,
            &target,
            &UpgradePlanRequest {
                output_path: &destination.path,
                plan_id,
                created_at,
                expires_at,
                accepted_publisher_key_id: &candidate.publisher_key_id,
                max_output_bytes: limits.max_capsule_bytes,
                max_rows: 100_000,
                max_stream_bytes: 512 * 1024 * 1024,
                deadline: EXECUTION_LIFETIME,
            },
            &cancellation,
        )?;
        let approved = parse_upgrade_plan(&review.plan().canonical_bytes()?)?;
        let report = review.report().clone();
        let plan_digest = approved.plan_digest().to_owned();
        let plan_id = approved.plan_id().to_owned();
        let nonce_sha256 = Sha256::digest(confirmation_nonce.as_bytes()).into();
        let output = UpgradeDestinationView {
            destination_token: destination.destination_token,
            parent_display: "Selected local folder",
            leaf_display: destination.leaf_display.clone(),
            expires_at: destination.expires_at,
        };
        self.prepared = Some(PreparedAuthority {
            selection_id: candidate.selection_id,
            plan_id: plan_id.clone(),
            nonce_sha256,
            deadline: Instant::now()
                .checked_add(EXECUTION_LIFETIME)
                .ok_or_else(limit_exceeded)?,
            review,
            approved,
            source,
            target,
            publisher_key_id: candidate.publisher_key_id,
            capability_review_required: report.capability_delta.requires_review,
            output_leaf: destination.leaf_display,
        });
        Ok(PreparedUpgradeView {
            profile: UPGRADE_REVIEW_PROFILE,
            plan_id,
            plan_digest,
            confirmation_nonce: confirmation_nonce.to_owned(),
            output,
            review: report,
            checks: [
                "both retained inputs are rebound read-only",
                "target clean template state is signed and reproduced",
                "publisher key continuity is exact",
                "data schema ID/version and physical schema are unchanged",
                "every target upgrade policy has one closed action",
                "target application digest and signatures are preserved",
                "new lineage binds both input file digests",
                "create-new output reopens and verifies before success",
            ],
            expires_at: expires_at.to_owned(),
        })
    }

    pub(crate) fn start(
        &mut self,
        request: &ExecuteUpgradeRequest,
        operation_token: String,
    ) -> Result<StartedUpgrade, WorkspaceError> {
        validate_token(&request.confirmation_nonce)?;
        validate_token(&operation_token)?;
        if self.active.is_some() {
            return Err(unsupported_operation());
        }
        let prepared = self.prepared.take().ok_or_else(stale_plan)?;
        let nonce: [u8; 32] = Sha256::digest(request.confirmation_nonce.as_bytes()).into();
        if Instant::now() >= prepared.deadline
            || prepared.plan_id != request.plan_id
            || !constant_time_equal(&prepared.nonce_sha256, &nonce)
            || !request.publisher_key_confirmed
            || (prepared.capability_review_required && !request.capability_changes_confirmed)
        {
            return Err(if Instant::now() >= prepared.deadline {
                session_expired()
            } else {
                stale_plan()
            });
        }
        let cancellation = CancellationToken::new();
        let status = UpgradeOperationStatus {
            profile: UPGRADE_STATUS_PROFILE,
            operation_token: operation_token.clone(),
            phase: UpgradeOperationPhase::Queued,
            cancellable: true,
            output_leaf: prepared.output_leaf,
            output_bytes: None,
            error: None,
        };
        self.active = Some(ActiveOperation {
            selection_id: prepared.selection_id,
            status,
            cancellation: cancellation.clone(),
        });
        Ok(StartedUpgrade {
            review: prepared.review,
            approved: prepared.approved,
            approval: UpgradeApproval {
                accepted_publisher_key_id: prepared.publisher_key_id,
                capability_changes_accepted: request.capability_changes_confirmed,
            },
            source: prepared.source,
            target: prepared.target,
            cancellation,
        })
    }

    pub(crate) fn update_phase(
        &mut self,
        operation_token: &str,
        phase: UpgradeOperationPhase,
        cancellable: bool,
    ) -> Result<UpgradeOperationStatus, WorkspaceError> {
        let active = self.active.as_mut().ok_or_else(stale_plan)?;
        if active.status.operation_token != operation_token {
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
    ) -> Result<UpgradeOperationStatus, WorkspaceError> {
        let active = self.active.as_mut().ok_or_else(stale_plan)?;
        if active.status.operation_token != operation_token {
            return Err(stale_plan());
        }
        match result {
            Ok(bytes) => {
                active.status.phase = UpgradeOperationPhase::Succeeded;
                active.status.output_bytes = Some(bytes);
                active.status.error = None;
            }
            Err(error) => {
                active.status.phase = if error.kind() == WorkspaceErrorCode::Cancelled {
                    UpgradeOperationPhase::Cancelled
                } else {
                    UpgradeOperationPhase::Failed
                };
                active.status.error = Some(error);
            }
        }
        active.status.cancellable = false;
        Ok(active.status.clone())
    }

    pub(crate) fn status(
        &self,
        operation_token: &str,
    ) -> Result<UpgradeOperationStatus, WorkspaceError> {
        let active = self.active.as_ref().ok_or_else(stale_plan)?;
        if active.status.operation_token != operation_token {
            return Err(stale_plan());
        }
        Ok(active.status.clone())
    }

    pub(crate) fn cancel(&self, operation_token: &str) -> Result<(), WorkspaceError> {
        let active = self.active.as_ref().ok_or_else(stale_plan)?;
        if active.status.operation_token != operation_token || !active.status.cancellable {
            return Err(stale_plan());
        }
        active.cancellation.cancel();
        Ok(())
    }

    pub(crate) fn acknowledge(&mut self, operation_token: &str) -> Result<(), WorkspaceError> {
        let terminal = self.active.as_ref().is_some_and(|active| {
            active.status.operation_token == operation_token
                && matches!(
                    active.status.phase,
                    UpgradeOperationPhase::Succeeded
                        | UpgradeOperationPhase::Failed
                        | UpgradeOperationPhase::Cancelled
                )
        });
        if !terminal {
            return Err(stale_plan());
        }
        self.active = None;
        Ok(())
    }

    pub(crate) fn prepare_for_close(&mut self) -> bool {
        self.candidate = None;
        self.destination = None;
        self.prepared = None;
        match self.active.as_ref() {
            None => true,
            Some(active)
                if matches!(
                    active.status.phase,
                    UpgradeOperationPhase::Succeeded
                        | UpgradeOperationPhase::Failed
                        | UpgradeOperationPhase::Cancelled
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

impl StartedUpgrade {
    pub(crate) fn execute<F>(self, mut progress: F) -> Result<u64, WorkspaceError>
    where
        F: FnMut(UpgradeOperationPhase, bool),
    {
        let limits = WorkspaceLimits::default();
        progress(UpgradeOperationPhase::Reverify, true);
        let prepared = self.review.prepare(
            self.approved,
            &self.approval,
            self.source,
            self.target,
            &limits,
            &self.cancellation,
        )?;
        progress(UpgradeOperationPhase::Stage, true);
        let staged = prepared.stage()?;
        progress(UpgradeOperationPhase::Rebase, true);
        let validated = staged.transform_and_validate()?;
        progress(UpgradeOperationPhase::Validate, true);
        progress(UpgradeOperationPhase::Publish, false);
        let published = validated.publish()?;
        progress(UpgradeOperationPhase::Postpublish, false);
        Ok(published.identity().bytes)
    }
}

pub(crate) fn accepted_common_publisher_key(
    source: &VerifiedWorkspaceSource,
    target: &VerifiedWorkspaceSource,
) -> Result<String, WorkspaceError> {
    let target_keys = target
        .valid_signature_key_ids()
        .into_iter()
        .collect::<BTreeSet<_>>();
    select_host_accepted_common_key(source.valid_signature_key_ids(), &target_keys)
}

fn select_host_accepted_common_key(
    source_keys_in_host_order: Vec<String>,
    target_keys: &BTreeSet<String>,
) -> Result<String, WorkspaceError> {
    // The trusted host selects one already-valid source signature and retains
    // that exact key as authority. The renderer can confirm the displayed key,
    // but cannot substitute a key ID. Multiple valid common signatures are not
    // ambiguous because only this retained key reaches the core approval.
    source_keys_in_host_order
        .into_iter()
        .find(|key_id| target_keys.contains(key_id))
        .ok_or_else(|| WorkspaceError::new(WorkspaceErrorCode::PublisherMismatch))
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

fn bounded_display(value: &str) -> String {
    value.chars().take(MAX_DISPLAY_CHARS).collect()
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    let length = left.len().max(right.len());
    for index in 0..length {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn active(phase: UpgradeOperationPhase, cancellable: bool) -> ActiveOperation {
        ActiveOperation {
            selection_id: "selection-a".to_owned(),
            status: UpgradeOperationStatus {
                profile: UPGRADE_STATUS_PROFILE,
                operation_token: "operation".to_owned(),
                phase,
                cancellable,
                output_leaf: "upgraded.sqlitecapsule".to_owned(),
                output_bytes: None,
                error: None,
            },
            cancellation: CancellationToken::new(),
        }
    }

    #[test]
    fn raw_tokens_and_terminal_close_state_are_closed() {
        assert!(validate_token(&"A".repeat(43)).is_ok());
        assert!(validate_token("path-or-short-token").is_err());
        for phase in [
            UpgradeOperationPhase::Succeeded,
            UpgradeOperationPhase::Failed,
            UpgradeOperationPhase::Cancelled,
        ] {
            let mut controller = UpgradeController {
                active: Some(active(phase, false)),
                ..UpgradeController::default()
            };
            assert!(controller.prepare_for_close());
            assert!(controller.active.is_none());
        }
    }

    #[test]
    fn running_upgrade_is_cancelled_but_remains_observable() {
        let active = active(UpgradeOperationPhase::Rebase, true);
        let cancellation = active.cancellation.clone();
        let mut controller = UpgradeController {
            active: Some(active),
            ..UpgradeController::default()
        };
        assert!(!controller.prepare_for_close());
        assert!(cancellation.is_cancelled());
        assert_eq!(
            controller.status("operation").unwrap().phase,
            UpgradeOperationPhase::Rebase
        );
    }

    #[test]
    fn selection_change_cancels_bound_operation_but_keeps_status_observable() {
        let active = active(UpgradeOperationPhase::Rebase, true);
        let cancellation = active.cancellation.clone();
        let mut controller = UpgradeController {
            active: Some(active),
            ..UpgradeController::default()
        };
        controller.invalidate_selection(Some("selection-b"));
        assert!(cancellation.is_cancelled());
        assert_eq!(
            controller.status("operation").unwrap().phase,
            UpgradeOperationPhase::Rebase
        );
    }

    #[test]
    fn host_selects_one_retained_key_when_several_signatures_are_common() {
        let target_keys = ["key-a".to_owned(), "key-b".to_owned()]
            .into_iter()
            .collect();
        let selected = select_host_accepted_common_key(
            vec!["key-b".to_owned(), "key-a".to_owned()],
            &target_keys,
        )
        .unwrap();
        assert_eq!(selected, "key-b");
    }
}

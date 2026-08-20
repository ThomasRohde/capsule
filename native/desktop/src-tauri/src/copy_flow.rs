//! Trusted-shell lifecycle copy controller.
//!
//! Renderer data is limited to opaque IDs, closed enums and bounded review
//! projections. Filesystem paths, retained verified sources, destination
//! reservations, canonical plans and confirmation authority remain in Rust.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlite_capsule_lifecycle::SourceIdentity;
use sqlite_capsule_workspace::{
    CancellationToken, CompactCopyPlanRequest, CompactCopyPreview, CompactCopyReview,
    CopySourceIdentity, ExactCopyPlanRequest, ExactCopyPreview, ExactCopyReview, ForkPolicy,
    LifecyclePlan, SemanticChoiceDisposition, SemanticCopyMode, SemanticCopyPlanRequest,
    SemanticCopyPreview, SemanticCopyReview, SemanticDatasetChoice, Sensitivity,
    VerifiedCompactSource, VerifiedCopySource, VerifiedWorkspaceSource, WorkspaceError,
    WorkspaceErrorCode, WorkspaceLimits, generate_compact_copy_plan, generate_exact_copy_plan,
    generate_semantic_copy_plan, open_semantic_copy_source, parse_compact_copy_plan,
    parse_exact_copy_plan, parse_semantic_copy_plan,
};

pub(crate) const COPY_REVIEW_PROFILE: &str = "org.sqlite-capsule.tauri-copy-review/1";
pub(crate) const COPY_STATUS_PROFILE: &str = "org.sqlite-capsule.tauri-copy-status/1";
pub(crate) const COPY_PROGRESS_EVENT: &str = "capsule-copy-progress-v1";
pub(crate) const COPY_PROFILE_PREVIEW_PROFILE: &str =
    "org.sqlite-capsule.tauri-copy-profile-preview/1";
const REVIEW_AUTHORITY_LIFETIME: Duration = Duration::from_secs(5 * 60);
pub(crate) const PREPARED_AUTHORITY_LIFETIME: Duration = Duration::from_secs(30);
const MAX_DISPLAY_CHARS: usize = 512;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ShellCopyMode {
    ExactDuplicate,
    CompactDuplicate,
    ForkWithData,
    CreateFromTemplate,
    SelectiveFork,
}

impl ShellCopyMode {
    pub(crate) const fn suggested_leaf(self) -> &'static str {
        match self {
            Self::ExactDuplicate => "capsule-copy.sqlitecapsule",
            Self::CompactDuplicate => "capsule-compact.sqlitecapsule",
            Self::ForkWithData => "capsule-fork.sqlitecapsule",
            Self::CreateFromTemplate => "new-capsule.sqlitecapsule",
            Self::SelectiveFork => "capsule-selective-fork.sqlitecapsule",
        }
    }

    pub(crate) fn semantic(self) -> Option<SemanticCopyMode> {
        match self {
            Self::ForkWithData => Some(SemanticCopyMode::Fork),
            Self::CreateFromTemplate => Some(SemanticCopyMode::CreateFromTemplate),
            Self::SelectiveFork => Some(SemanticCopyMode::SelectiveFork),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PreviewCopyProfileRequest {
    pub selection_id: String,
    pub mode: ShellCopyMode,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChooseCopyDestinationRequest {
    pub selection_id: String,
    pub mode: ShellCopyMode,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CancelCopyDestinationRequest {
    pub destination_id: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PrepareCopyRequest {
    pub selection_id: String,
    pub destination_id: String,
    pub mode: ShellCopyMode,
    #[serde(default)]
    pub choices: Vec<ShellDatasetChoiceRequest>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ShellDatasetChoiceRequest {
    pub choice_id: String,
    pub disposition: ShellChoiceDisposition,
    pub sensitive_confirmed: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ShellChoiceDisposition {
    Include,
    Omit,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExecuteCopyRequest {
    pub plan_id: String,
    pub confirmation_nonce: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OperationRequest {
    pub operation_id: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct CopyDestinationView {
    pub destination_id: String,
    pub mode: ShellCopyMode,
    pub parent_display: String,
    pub leaf_display: String,
    pub expires_at: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
pub(crate) enum CopyPreviewView {
    Exact(ExactCopyPreview),
    Compact(CompactCopyPreview),
    Semantic(SemanticCopyPreview),
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct CopyProfilePreviewView {
    pub profile: &'static str,
    pub mode: ShellCopyMode,
    pub source_format_version: String,
    pub source_sha256: String,
    pub datasets: Vec<ShellDatasetChoiceView>,
    pub blockers: Vec<&'static str>,
    pub expires_at: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ShellDatasetChoiceView {
    pub choice_id: Option<String>,
    pub dataset_id: String,
    pub sensitivity: Sensitivity,
    pub signed_fork_policy: ForkPolicy,
    pub fixed_action: Option<&'static str>,
    pub default_disposition: Option<ShellChoiceDisposition>,
    pub allow_include: bool,
    pub allow_omit: bool,
    pub sensitive_confirmation_required: bool,
    pub dependencies: Vec<String>,
    pub auto_selected_by_dependency: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct PreparedCopyView {
    pub profile: &'static str,
    pub plan_id: String,
    pub plan_digest: String,
    pub mode: ShellCopyMode,
    pub output: CopyDestinationView,
    pub preview: CopyPreviewView,
    pub checks: [&'static str; 7],
    pub expires_at: String,
    pub confirmation_nonce: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum CopyOperationPhase {
    Queued,
    Reverify,
    Stage,
    CopyOrCompact,
    Validate,
    Publish,
    Postpublish,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct CopyProgressEvent {
    pub profile: &'static str,
    pub operation_id: String,
    pub sequence: u64,
    pub mode: ShellCopyMode,
    pub phase: CopyOperationPhase,
    pub cancellable: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct CopyOperationStatus {
    pub profile: &'static str,
    pub operation_id: String,
    pub mode: ShellCopyMode,
    pub phase: CopyOperationPhase,
    pub cancellable: bool,
    pub output_leaf: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<WorkspaceError>,
}

struct DestinationAuthority {
    destination_id: String,
    selection_id: String,
    mode: ShellCopyMode,
    path: PathBuf,
    parent_display: String,
    leaf_display: String,
    expires_at: String,
    deadline: Instant,
}

enum PreparedAuthority {
    Exact {
        review: ExactCopyReview,
        approved: LifecyclePlan,
        source: VerifiedCopySource,
    },
    Compact {
        review: CompactCopyReview,
        approved: LifecyclePlan,
        source: VerifiedCompactSource,
    },
    Semantic {
        review: SemanticCopyReview,
        approved: LifecyclePlan,
        source: VerifiedWorkspaceSource,
    },
}

struct ChoiceAuthority {
    selection_id: String,
    mode: ShellCopyMode,
    source_sha256: String,
    choices: BTreeMap<String, String>,
    blocked: bool,
    deadline: Instant,
}

struct PreparedSession {
    selection_id: String,
    mode: ShellCopyMode,
    plan_id: String,
    nonce_sha256: [u8; 32],
    deadline: Instant,
    authority: PreparedAuthority,
    output_leaf: String,
}

struct ActiveOperation {
    status: CopyOperationStatus,
    cancellation: CancellationToken,
}

#[derive(Default)]
pub(crate) struct CopyController {
    choices: Option<ChoiceAuthority>,
    destination: Option<DestinationAuthority>,
    prepared: Option<PreparedSession>,
    active: Option<ActiveOperation>,
}

#[derive(Clone, Default)]
pub(crate) struct CopyState(pub(crate) Arc<Mutex<CopyController>>);

pub(crate) struct StartedCopy {
    pub mode: ShellCopyMode,
    pub cancellation: CancellationToken,
    authority: PreparedAuthority,
}

impl CopyController {
    pub(crate) fn invalidate_selection(&mut self, current_selection: Option<&str>) {
        if self
            .choices
            .as_ref()
            .is_some_and(|item| Some(item.selection_id.as_str()) != current_selection)
        {
            self.choices = None;
        }
        if self
            .destination
            .as_ref()
            .is_some_and(|item| Some(item.selection_id.as_str()) != current_selection)
        {
            self.destination = None;
        }
        if self
            .prepared
            .as_ref()
            .is_some_and(|item| Some(item.selection_id.as_str()) != current_selection)
        {
            self.prepared = None;
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn retain_profile_preview(
        &mut self,
        selection_id: &str,
        mode: ShellCopyMode,
        source_format_version: String,
        source_sha256: &str,
        datasets: Vec<ShellDatasetChoiceView>,
        blockers: Vec<&'static str>,
        expires_at: String,
    ) -> Result<CopyProfilePreviewView, WorkspaceError> {
        validate_token(selection_id)?;
        if source_sha256.len() != 64 || datasets.len() > 256 || blockers.len() > 16 {
            return Err(invalid_contract());
        }
        let mut choices = BTreeMap::new();
        for dataset in &datasets {
            if let Some(token) = &dataset.choice_id {
                validate_token(token)?;
                if choices
                    .insert(token.clone(), dataset.dataset_id.clone())
                    .is_some()
                {
                    return Err(invalid_contract());
                }
            }
        }
        self.prepared = None;
        self.choices = Some(ChoiceAuthority {
            selection_id: selection_id.to_owned(),
            mode,
            source_sha256: source_sha256.to_owned(),
            choices,
            blocked: !blockers.is_empty(),
            deadline: Instant::now()
                .checked_add(REVIEW_AUTHORITY_LIFETIME)
                .ok_or_else(limit_exceeded)?,
        });
        Ok(CopyProfilePreviewView {
            profile: COPY_PROFILE_PREVIEW_PROFILE,
            mode,
            source_format_version,
            source_sha256: source_sha256.to_owned(),
            datasets,
            blockers,
            expires_at,
        })
    }

    pub(crate) fn retain_destination(
        &mut self,
        selection_id: &str,
        mode: ShellCopyMode,
        path: PathBuf,
        destination_id: String,
        expires_at: String,
    ) -> Result<CopyDestinationView, WorkspaceError> {
        validate_token(selection_id)?;
        validate_token(&destination_id)?;
        if !path.is_absolute() || path.exists() {
            return Err(invalid_contract());
        }
        if !path.parent().is_some_and(|value| value.is_dir()) {
            return Err(invalid_contract());
        }
        let leaf = path
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .ok_or_else(invalid_contract)?;
        let parent_display = "Selected local folder".to_owned();
        let leaf_display = bounded_display(leaf);
        let view = CopyDestinationView {
            destination_id: destination_id.clone(),
            mode,
            parent_display: parent_display.clone(),
            leaf_display: leaf_display.clone(),
            expires_at: expires_at.clone(),
        };
        self.prepared = None;
        self.destination = Some(DestinationAuthority {
            destination_id,
            selection_id: selection_id.to_owned(),
            mode,
            path,
            parent_display,
            leaf_display,
            expires_at,
            deadline: Instant::now()
                .checked_add(REVIEW_AUTHORITY_LIFETIME)
                .ok_or_else(limit_exceeded)?,
        });
        Ok(view)
    }

    pub(crate) fn cancel_destination(
        &mut self,
        destination_id: &str,
    ) -> Result<(), WorkspaceError> {
        validate_token(destination_id)?;
        if self
            .destination
            .as_ref()
            .is_some_and(|item| Instant::now() >= item.deadline)
        {
            self.destination = None;
            self.prepared = None;
            return Err(session_expired());
        }
        let matches = self.destination.as_ref().is_some_and(|item| {
            constant_time_equal(item.destination_id.as_bytes(), destination_id.as_bytes())
        });
        if !matches {
            return Err(stale_plan());
        }
        self.destination = None;
        self.prepared = None;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare(
        &mut self,
        request: &PrepareCopyRequest,
        source_path: &Path,
        expected_source_identity: &SourceIdentity,
        expected_source_sha256: &str,
        plan_id: &str,
        created_at: &str,
        expires_at: &str,
        confirmation_nonce: &str,
    ) -> Result<PreparedCopyView, WorkspaceError> {
        validate_token(&request.selection_id)?;
        validate_token(&request.destination_id)?;
        validate_token(confirmation_nonce)?;
        let destination = self.destination.take().ok_or_else(stale_plan)?;
        if Instant::now() >= destination.deadline {
            return Err(session_expired());
        }
        if destination.selection_id != request.selection_id
            || destination.mode != request.mode
            || !constant_time_equal(
                destination.destination_id.as_bytes(),
                request.destination_id.as_bytes(),
            )
        {
            return Err(stale_plan());
        }
        let limits = WorkspaceLimits::default();
        let cancellation = CancellationToken::new();
        let output_view = CopyDestinationView {
            destination_id: destination.destination_id,
            mode: destination.mode,
            parent_display: destination.parent_display,
            leaf_display: destination.leaf_display.clone(),
            expires_at: destination.expires_at,
        };
        let (authority, preview, digest) = match request.mode {
            ShellCopyMode::ExactDuplicate => {
                let source =
                    VerifiedCopySource::open_with_control(source_path, &limits, &cancellation)?;
                source.assert_source_binding(expected_source_identity)?;
                require_selected_source(source.identity(), expected_source_sha256)?;
                let review = generate_exact_copy_plan(
                    &source,
                    &ExactCopyPlanRequest {
                        output_path: &destination.path,
                        plan_id,
                        created_at,
                        expires_at,
                        deadline: PREPARED_AUTHORITY_LIFETIME.min(limits.deadline),
                        max_output_bytes: limits.max_capsule_bytes,
                    },
                )?;
                let preview = review.preview(&source)?;
                let approved = parse_exact_copy_plan(&review.plan().canonical_bytes()?)?;
                let digest = approved.plan_digest().to_owned();
                (
                    PreparedAuthority::Exact {
                        review,
                        approved,
                        source,
                    },
                    CopyPreviewView::Exact(preview),
                    digest,
                )
            }
            ShellCopyMode::CompactDuplicate => {
                let source =
                    VerifiedCompactSource::open_with_control(source_path, &limits, &cancellation)?;
                source.assert_source_binding(expected_source_identity)?;
                require_selected_source(source.identity(), expected_source_sha256)?;
                let review = generate_compact_copy_plan(
                    &source,
                    &CompactCopyPlanRequest {
                        output_path: &destination.path,
                        plan_id,
                        created_at,
                        expires_at,
                        deadline: PREPARED_AUTHORITY_LIFETIME.min(limits.deadline),
                        max_output_bytes: limits.max_capsule_bytes,
                    },
                )?;
                let preview = review.preview(&source)?;
                let approved = parse_compact_copy_plan(&review.plan().canonical_bytes()?)?;
                let digest = approved.plan_digest().to_owned();
                (
                    PreparedAuthority::Compact {
                        review,
                        approved,
                        source,
                    },
                    CopyPreviewView::Compact(preview),
                    digest,
                )
            }
            mode => {
                let semantic_mode = mode.semantic().ok_or_else(unsupported_operation)?;
                let choice_authority = self.choices.take().ok_or_else(stale_plan)?;
                if Instant::now() >= choice_authority.deadline {
                    return Err(session_expired());
                }
                if choice_authority.selection_id != request.selection_id
                    || choice_authority.mode != request.mode
                    || choice_authority.source_sha256 != expected_source_sha256
                    || choice_authority.blocked
                {
                    return Err(stale_plan());
                }
                if request.choices.len() > 256 {
                    return Err(invalid_contract());
                }
                let mut used = BTreeSet::new();
                let mut choices = Vec::with_capacity(request.choices.len());
                for submitted in &request.choices {
                    validate_token(&submitted.choice_id)?;
                    if !used.insert(submitted.choice_id.as_str()) {
                        return Err(invalid_contract());
                    }
                    let dataset_id = choice_authority
                        .choices
                        .get(&submitted.choice_id)
                        .ok_or_else(invalid_contract)?;
                    choices.push(SemanticDatasetChoice {
                        dataset_id: dataset_id.clone(),
                        disposition: match submitted.disposition {
                            ShellChoiceDisposition::Include => SemanticChoiceDisposition::Copy,
                            ShellChoiceDisposition::Omit => SemanticChoiceDisposition::Omit,
                        },
                        sensitive_confirmed: submitted.sensitive_confirmed,
                    });
                }
                let source = open_semantic_copy_source(source_path, &limits, &cancellation)?;
                source.assert_source_binding(expected_source_identity, expected_source_sha256)?;
                let review = generate_semantic_copy_plan(
                    &source,
                    &SemanticCopyPlanRequest {
                        output_path: &destination.path,
                        plan_id,
                        created_at,
                        expires_at,
                        mode: semantic_mode,
                        choices: &choices,
                        deadline: PREPARED_AUTHORITY_LIFETIME.min(limits.deadline),
                        max_output_bytes: limits.max_capsule_bytes,
                        max_rows: 100_000,
                        max_stream_bytes: 512 * 1024 * 1024,
                    },
                    &cancellation,
                )?;
                let preview = review.preview();
                let approved = parse_semantic_copy_plan(&review.plan().canonical_bytes()?)?;
                let digest = approved.plan_digest().to_owned();
                (
                    PreparedAuthority::Semantic {
                        review,
                        approved,
                        source,
                    },
                    CopyPreviewView::Semantic(preview),
                    digest,
                )
            }
        };
        self.prepared = Some(PreparedSession {
            selection_id: request.selection_id.clone(),
            mode: request.mode,
            plan_id: plan_id.to_owned(),
            nonce_sha256: Sha256::digest(confirmation_nonce.as_bytes()).into(),
            deadline: Instant::now()
                .checked_add(PREPARED_AUTHORITY_LIFETIME)
                .ok_or_else(limit_exceeded)?,
            authority,
            output_leaf: output_view.leaf_display.clone(),
        });
        Ok(PreparedCopyView {
            profile: COPY_REVIEW_PROFILE,
            plan_id: plan_id.to_owned(),
            plan_digest: digest,
            mode: request.mode,
            output: output_view,
            preview,
            checks: [
                "source-rebound",
                "destination-create-new",
                "capsule-structure",
                "signature-inventory",
                "declared-checks",
                "operation-postconditions",
                "postpublish-reopen",
            ],
            expires_at: expires_at.to_owned(),
            confirmation_nonce: confirmation_nonce.to_owned(),
        })
    }

    pub(crate) fn start(
        &mut self,
        request: &ExecuteCopyRequest,
        operation_id: String,
    ) -> Result<StartedCopy, WorkspaceError> {
        validate_token(&request.confirmation_nonce)?;
        if self.active.is_some() {
            return Err(unsupported_operation());
        }
        let prepared = self.prepared.take().ok_or_else(stale_plan)?;
        let nonce: [u8; 32] = Sha256::digest(request.confirmation_nonce.as_bytes()).into();
        ensure_prepared_authority_current(prepared.deadline)?;
        if prepared.plan_id != request.plan_id
            || !constant_time_equal(&prepared.nonce_sha256, &nonce)
        {
            return Err(stale_plan());
        }
        let cancellation = CancellationToken::new();
        let status = CopyOperationStatus {
            profile: COPY_STATUS_PROFILE,
            operation_id: operation_id.clone(),
            mode: prepared.mode,
            phase: CopyOperationPhase::Queued,
            cancellable: true,
            output_leaf: prepared.output_leaf.clone(),
            output_bytes: None,
            error: None,
        };
        self.active = Some(ActiveOperation {
            status,
            cancellation: cancellation.clone(),
        });
        Ok(StartedCopy {
            mode: prepared.mode,
            cancellation,
            authority: prepared.authority,
        })
    }

    pub(crate) fn update_phase(
        &mut self,
        operation_id: &str,
        phase: CopyOperationPhase,
        cancellable: bool,
    ) -> Result<CopyOperationStatus, WorkspaceError> {
        let active = self.active.as_mut().ok_or_else(stale_plan)?;
        if active.status.operation_id != operation_id {
            return Err(stale_plan());
        }
        active.status.phase = phase;
        active.status.cancellable = cancellable;
        Ok(active.status.clone())
    }

    pub(crate) fn finish(
        &mut self,
        operation_id: &str,
        result: Result<u64, WorkspaceError>,
    ) -> Result<CopyOperationStatus, WorkspaceError> {
        let active = self.active.as_mut().ok_or_else(stale_plan)?;
        if active.status.operation_id != operation_id {
            return Err(stale_plan());
        }
        match result {
            Ok(bytes) => {
                active.status.phase = CopyOperationPhase::Succeeded;
                active.status.cancellable = false;
                active.status.output_bytes = Some(bytes);
                active.status.error = None;
            }
            Err(error) => {
                active.status.phase = if error.kind() == WorkspaceErrorCode::Cancelled {
                    CopyOperationPhase::Cancelled
                } else {
                    CopyOperationPhase::Failed
                };
                active.status.cancellable = false;
                active.status.error = Some(error);
            }
        }
        Ok(active.status.clone())
    }

    pub(crate) fn status(&self, operation_id: &str) -> Result<CopyOperationStatus, WorkspaceError> {
        let active = self.active.as_ref().ok_or_else(stale_plan)?;
        if active.status.operation_id != operation_id {
            return Err(stale_plan());
        }
        Ok(active.status.clone())
    }

    pub(crate) fn cancel(&self, operation_id: &str) -> Result<(), WorkspaceError> {
        let active = self.active.as_ref().ok_or_else(stale_plan)?;
        if active.status.operation_id != operation_id || !active.status.cancellable {
            return Err(stale_plan());
        }
        active.cancellation.cancel();
        Ok(())
    }

    pub(crate) fn acknowledge(&mut self, operation_id: &str) -> Result<(), WorkspaceError> {
        let terminal = self.active.as_ref().is_some_and(|active| {
            active.status.operation_id == operation_id
                && matches!(
                    active.status.phase,
                    CopyOperationPhase::Succeeded
                        | CopyOperationPhase::Failed
                        | CopyOperationPhase::Cancelled
                )
        });
        if !terminal {
            return Err(stale_plan());
        }
        self.active = None;
        Ok(())
    }

    /// Returns true when the process can close immediately. Pending review
    /// authority is dropped. A cancellable operation is asked to stop and a
    /// non-cancellable publication tail is allowed to finish; both keep the
    /// trusted shell alive until a later close request.
    pub(crate) fn prepare_for_close(&mut self) -> bool {
        self.destination = None;
        self.choices = None;
        self.prepared = None;
        match self.active.as_ref() {
            None => true,
            Some(active)
                if matches!(
                    active.status.phase,
                    CopyOperationPhase::Succeeded
                        | CopyOperationPhase::Failed
                        | CopyOperationPhase::Cancelled
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

impl StartedCopy {
    pub(crate) fn execute<F>(self, mut progress: F) -> Result<u64, WorkspaceError>
    where
        F: FnMut(CopyOperationPhase, bool),
    {
        let limits = WorkspaceLimits::default();
        progress(CopyOperationPhase::Reverify, true);
        match self.authority {
            PreparedAuthority::Exact {
                review,
                approved,
                source,
            } => {
                let prepared = review.prepare(approved, source, &limits, &self.cancellation)?;
                progress(CopyOperationPhase::Stage, true);
                let staged = prepared.stage()?;
                progress(CopyOperationPhase::CopyOrCompact, true);
                let validated = staged.copy_and_validate()?;
                progress(CopyOperationPhase::Validate, true);
                progress(CopyOperationPhase::Publish, false);
                let published = validated.publish()?;
                progress(CopyOperationPhase::Postpublish, false);
                Ok(published.identity().bytes)
            }
            PreparedAuthority::Compact {
                review,
                approved,
                source,
            } => {
                let prepared = review.prepare(approved, source, &limits, &self.cancellation)?;
                progress(CopyOperationPhase::Stage, true);
                let staged = prepared.stage()?;
                progress(CopyOperationPhase::CopyOrCompact, true);
                let validated = staged.compact_and_validate()?;
                progress(CopyOperationPhase::Validate, true);
                progress(CopyOperationPhase::Publish, false);
                let published = validated.publish()?;
                progress(CopyOperationPhase::Postpublish, false);
                Ok(published.identity().bytes)
            }
            PreparedAuthority::Semantic {
                review,
                approved,
                source,
            } => {
                let prepared = review.prepare(approved, source, &limits, &self.cancellation)?;
                progress(CopyOperationPhase::Stage, true);
                let staged = prepared.stage()?;
                progress(CopyOperationPhase::CopyOrCompact, true);
                let validated = staged.transform_and_validate()?;
                progress(CopyOperationPhase::Validate, true);
                progress(CopyOperationPhase::Publish, false);
                let published = validated.publish()?;
                progress(CopyOperationPhase::Postpublish, false);
                Ok(published.identity().bytes)
            }
        }
    }
}

fn require_selected_source(
    actual: &CopySourceIdentity,
    expected_sha256: &str,
) -> Result<(), WorkspaceError> {
    if expected_sha256.len() == 64 && actual.file_sha256 == expected_sha256 && actual.size_bytes > 0
    {
        Ok(())
    } else {
        Err(stale_plan())
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

fn ensure_prepared_authority_current(deadline: Instant) -> Result<(), WorkspaceError> {
    if Instant::now() >= deadline {
        Err(session_expired())
    } else {
        Ok(())
    }
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

    fn active(phase: CopyOperationPhase, cancellable: bool) -> ActiveOperation {
        ActiveOperation {
            status: CopyOperationStatus {
                profile: COPY_STATUS_PROFILE,
                operation_id: "operation-1".to_owned(),
                mode: ShellCopyMode::ExactDuplicate,
                phase,
                cancellable,
                output_leaf: "copy.sqlitecapsule".to_owned(),
                output_bytes: None,
                error: None,
            },
            cancellation: CancellationToken::new(),
        }
    }

    #[test]
    fn tokens_and_constant_time_comparison_are_closed() {
        let token = "A".repeat(43);
        assert!(validate_token(&token).is_ok());
        assert!(validate_token("short").is_err());
        assert!(validate_token(&format!("{}!", "A".repeat(42))).is_err());
        assert!(constant_time_equal(token.as_bytes(), token.as_bytes()));
        assert!(!constant_time_equal(
            token.as_bytes(),
            "B".repeat(43).as_bytes()
        ));
        assert!(!constant_time_equal(token.as_bytes(), b"short"));
    }

    #[test]
    fn terminal_copy_state_never_traps_process_close() {
        for phase in [
            CopyOperationPhase::Succeeded,
            CopyOperationPhase::Failed,
            CopyOperationPhase::Cancelled,
        ] {
            let mut controller = CopyController {
                active: Some(active(phase, false)),
                ..CopyController::default()
            };
            assert!(controller.prepare_for_close());
            assert!(controller.active.is_none());
        }
    }

    #[test]
    fn running_copy_is_cancelled_without_orphaning_its_operation_id() {
        let active = active(CopyOperationPhase::CopyOrCompact, true);
        let cancellation = active.cancellation.clone();
        let mut controller = CopyController {
            active: Some(active),
            ..CopyController::default()
        };
        assert!(!controller.prepare_for_close());
        assert!(cancellation.is_cancelled());
        assert_eq!(
            controller
                .status("operation-1")
                .expect("operation remains observable")
                .phase,
            CopyOperationPhase::CopyOrCompact
        );
    }

    #[test]
    fn expired_destination_authority_has_the_exact_session_code() {
        let token = "A".repeat(43);
        let mut controller = CopyController {
            destination: Some(DestinationAuthority {
                destination_id: token.clone(),
                selection_id: "B".repeat(43),
                mode: ShellCopyMode::ExactDuplicate,
                path: PathBuf::from("unused"),
                parent_display: "parent".to_owned(),
                leaf_display: "copy.sqlitecapsule".to_owned(),
                expires_at: "2026-08-13T12:05:00Z".to_owned(),
                deadline: Instant::now(),
            }),
            ..CopyController::default()
        };
        let error = controller
            .cancel_destination(&token)
            .expect_err("expired destination must not be reusable");
        assert_eq!(error.kind(), WorkspaceErrorCode::SessionExpired);
        assert!(controller.destination.is_none());
    }

    #[test]
    fn prepared_authority_expiry_matches_the_retained_source_budget() {
        assert_eq!(
            PREPARED_AUTHORITY_LIFETIME,
            WorkspaceLimits::default().deadline
        );
        assert_eq!(
            ensure_prepared_authority_current(Instant::now())
                .expect_err("the exact expiry boundary is closed")
                .kind(),
            WorkspaceErrorCode::SessionExpired
        );
        ensure_prepared_authority_current(
            Instant::now()
                .checked_add(PREPARED_AUTHORITY_LIFETIME)
                .expect("bounded prepared deadline"),
        )
        .expect("a newly prepared authority remains usable");
    }

    #[test]
    fn retained_preview_preserves_v02_format_and_native_blockers() {
        let mut controller = CopyController::default();
        let selection = "S".repeat(43);
        let preview = controller
            .retain_profile_preview(
                &selection,
                ShellCopyMode::ExactDuplicate,
                "0.2".to_owned(),
                &"a".repeat(64),
                Vec::new(),
                vec!["unsupported-operation"],
                "2026-08-13T12:05:00Z".to_owned(),
            )
            .expect("bounded preview");
        assert_eq!(preview.source_format_version, "0.2");
        assert_eq!(preview.blockers, vec!["unsupported-operation"]);
        assert!(controller.choices.as_ref().is_some_and(|item| item.blocked));
    }
}

//! Review-only planning for duplicate, compact, fork and template operations.
//!
//! This module never creates a destination, mutates a source or returns
//! execution authority. It projects a bounded report from the already verified
//! private source snapshot. Exact-snapshot and compact-logical duplication use
//! separate source typestates and host-held destination authority; this report
//! never becomes either executor's authority.

use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

use rusqlite::params;
use serde::Serialize;

use crate::{
    CancellationToken, DatasetRole, ForkPolicy, Sensitivity, VerifiedWorkspaceSource,
    WorkspaceControl, WorkspaceError, WorkspaceErrorCode, WorkspaceLimits,
    template_state::{TemplateStateProof, verify_template_state_with_control},
};

pub const COPY_PREVIEW_PROFILE: &str = "org.sqlite-capsule.copy-preview/1";

const HARD_MAX_REPORT_BYTES: usize = 512 * 1_024;
const HARD_MAX_ROWS_SCANNED_PER_DATASET: u64 = 100_000;
const HARD_MAX_TEMPLATE_STREAM_BYTES: u64 = 512 * 1024 * 1024;
const HARD_MAX_DEADLINE: Duration = Duration::from_secs(30);
const MAX_DATASET_CHOICES: usize = 256;

macro_rules! string_enum {
    ($name:ident { $($variant:ident),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
        #[serde(rename_all = "kebab-case")]
        pub enum $name { $($variant),+ }
    };
}

string_enum!(CopyMode {
    ExactDuplicate,
    CompactDuplicate,
    ForkWithData,
    CreateFromTemplate,
    SelectiveFork,
});

string_enum!(CopyFormatAvailability {
    Supported,
    Unsupported,
    SpecifiedButPlannerUnavailable,
    RequiresVerifiedTemplate,
    RequiresPolicyReview,
});

string_enum!(CopyIdentityDisposition {
    Preserve,
    GenerateNew
});
string_enum!(CopyInstanceProfileDisposition {
    Preserve,
    ExplicitForkPolicy,
    VerifiedTemplatePolicy
});
string_enum!(CopyMutableStateDisposition { Preserve, Clear });
string_enum!(CopyAvailability {
    Ready,
    NeedsReview,
    Blocked
});
string_enum!(ExecutionAvailability {
    ExistingExactSnapshotExecutor,
    ExistingCompactLogicalExecutor,
    ExistingSemanticExecutor,
    Blocked
});
string_enum!(DatasetChoiceDisposition { Include, Omit });
string_enum!(CopyDatasetAction {
    Copy,
    Reset,
    Omit,
    Prompt,
    Forbid
});
string_enum!(CopyPromptKind {
    DatasetChoice,
    SensitiveConfirmation
});

string_enum!(CopyBlocker {
    VerifiedTemplateRequired,
    ResetSemanticsUnavailable,
    PolicyForbidsOperation,
    DatasetChoiceRequired,
    SensitiveConfirmationRequired,
    SelectiveOmissionNotPermitted,
    RequiredDatasetOmitted,
    DependencyNotPermitted,
    SensitiveCopyConfirmationRequired,
    PreviewNotExecutionAuthority,
    CompactSecurityDesignUnavailable,
    ForkExecutorUnavailable,
    SelectiveExecutorUnavailable,
    CrossDatasetForeignKeysUnverified,
    CleanSeedStateUnprovable,
});

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct CopyModeTruth {
    pub mode: CopyMode,
    pub v0_2: CopyFormatAvailability,
    pub v0_3: CopyFormatAvailability,
    pub unsigned_v0_3: CopyFormatAvailability,
    pub capsule_identity: CopyIdentityDisposition,
    pub revision_identity: CopyIdentityDisposition,
    pub requires_signed_data_contract: bool,
    pub supports_dataset_choices: bool,
}

const COPY_MODE_TRUTH_TABLE: [CopyModeTruth; 5] = [
    CopyModeTruth {
        mode: CopyMode::ExactDuplicate,
        v0_2: CopyFormatAvailability::Supported,
        v0_3: CopyFormatAvailability::Supported,
        unsigned_v0_3: CopyFormatAvailability::Supported,
        capsule_identity: CopyIdentityDisposition::Preserve,
        revision_identity: CopyIdentityDisposition::Preserve,
        requires_signed_data_contract: false,
        supports_dataset_choices: false,
    },
    CopyModeTruth {
        mode: CopyMode::CompactDuplicate,
        v0_2: CopyFormatAvailability::Supported,
        v0_3: CopyFormatAvailability::Supported,
        unsigned_v0_3: CopyFormatAvailability::Supported,
        capsule_identity: CopyIdentityDisposition::Preserve,
        revision_identity: CopyIdentityDisposition::Preserve,
        requires_signed_data_contract: false,
        supports_dataset_choices: false,
    },
    CopyModeTruth {
        mode: CopyMode::ForkWithData,
        v0_2: CopyFormatAvailability::Unsupported,
        v0_3: CopyFormatAvailability::Supported,
        unsigned_v0_3: CopyFormatAvailability::Unsupported,
        capsule_identity: CopyIdentityDisposition::GenerateNew,
        revision_identity: CopyIdentityDisposition::GenerateNew,
        requires_signed_data_contract: true,
        supports_dataset_choices: true,
    },
    CopyModeTruth {
        mode: CopyMode::CreateFromTemplate,
        v0_2: CopyFormatAvailability::Unsupported,
        v0_3: CopyFormatAvailability::RequiresVerifiedTemplate,
        unsigned_v0_3: CopyFormatAvailability::Unsupported,
        capsule_identity: CopyIdentityDisposition::GenerateNew,
        revision_identity: CopyIdentityDisposition::GenerateNew,
        requires_signed_data_contract: true,
        supports_dataset_choices: false,
    },
    CopyModeTruth {
        mode: CopyMode::SelectiveFork,
        v0_2: CopyFormatAvailability::Unsupported,
        v0_3: CopyFormatAvailability::RequiresPolicyReview,
        unsigned_v0_3: CopyFormatAvailability::Unsupported,
        capsule_identity: CopyIdentityDisposition::GenerateNew,
        revision_identity: CopyIdentityDisposition::GenerateNew,
        requires_signed_data_contract: true,
        supports_dataset_choices: true,
    },
];

pub const fn copy_mode_truth_table() -> &'static [CopyModeTruth] {
    &COPY_MODE_TRUTH_TABLE
}

#[derive(Clone, Debug)]
pub struct CopyPreviewLimits {
    pub deadline: Duration,
    pub max_rows_scanned_per_dataset: u64,
    pub max_rows_scanned_total: u64,
    pub max_report_bytes: usize,
}

impl Default for CopyPreviewLimits {
    fn default() -> Self {
        Self {
            deadline: HARD_MAX_DEADLINE,
            max_rows_scanned_per_dataset: HARD_MAX_ROWS_SCANNED_PER_DATASET,
            max_rows_scanned_total: HARD_MAX_ROWS_SCANNED_PER_DATASET,
            max_report_bytes: HARD_MAX_REPORT_BYTES,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyDatasetChoice {
    pub dataset_id: String,
    pub disposition: DatasetChoiceDisposition,
    pub sensitive_confirmed: bool,
}

#[derive(Clone, Debug)]
pub struct CopyPreviewRequest {
    pub mode: CopyMode,
    pub dataset_choices: Vec<CopyDatasetChoice>,
    pub limits: CopyPreviewLimits,
}

impl CopyPreviewRequest {
    pub fn new(mode: CopyMode) -> Self {
        Self {
            mode,
            dataset_choices: Vec::new(),
            limits: CopyPreviewLimits::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CopyIdentityEffects {
    pub capsule_id: CopyIdentityDisposition,
    pub revision_id: CopyIdentityDisposition,
    pub instance_profile: CopyInstanceProfileDisposition,
    pub grants: CopyMutableStateDisposition,
    pub change_log: CopyMutableStateDisposition,
    pub lineage_operation: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CopyRowEstimate {
    pub rows: u64,
    pub exact: bool,
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CopyDependency {
    pub dataset_id: String,
    pub reason: String,
    pub auto_selected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CopyDatasetDecision {
    pub dataset_id: String,
    pub role: DatasetRole,
    pub sensitivity: Sensitivity,
    pub required: bool,
    pub signed_fork_policy: ForkPolicy,
    pub action: Option<CopyDatasetAction>,
    pub row_estimate: CopyRowEstimate,
    pub dependencies: Vec<CopyDependency>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CopyPrompt {
    pub dataset_id: String,
    pub kind: CopyPromptKind,
    pub unconfirmed_action: CopyDatasetAction,
}

string_enum!(DigestExpectation { Source });

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CopyOutputConstraints {
    pub format_version: &'static str,
    pub signed_application_profile: &'static str,
    pub source_is_read_only: bool,
    pub create_new: bool,
    pub overwrite_allowed: bool,
    pub application_digest_must_match_source: bool,
    pub application_digest_from: DigestExpectation,
    pub publish_only_after_full_verification: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CopyBlockingReason {
    pub code: CopyBlocker,
    pub safe_reason: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CopyPreviewReport {
    pub profile: &'static str,
    pub mode: CopyMode,
    pub availability: CopyAvailability,
    pub identity: CopyIdentityEffects,
    pub datasets: Vec<CopyDatasetDecision>,
    pub prompts: Vec<CopyPrompt>,
    pub blockers: Vec<CopyBlocker>,
    pub expected_application_digest: Option<String>,
    pub output: CopyOutputConstraints,
    pub execution_availability: ExecutionAvailability,
    pub execution_blockers: Vec<CopyBlockingReason>,
    pub execution_must_rederive_decisions: bool,
    /// This dry-run report is review data and never an execution capability.
    pub execution_authority_issued: bool,
}

/// Builds a bounded, review-only copy report from a verified signed v0.3 source.
///
/// Row estimates are read from the source's exact private verification snapshot.
/// No live source path is reopened and no destination is selected or created.
pub fn preview_copy(
    source: &VerifiedWorkspaceSource,
    request: &CopyPreviewRequest,
    cancellation: &CancellationToken,
) -> Result<CopyPreviewReport, WorkspaceError> {
    validate_request(source, request)?;

    let deadline = request.limits.deadline.min(HARD_MAX_DEADLINE);
    let rows_per_dataset = request
        .limits
        .max_rows_scanned_per_dataset
        .min(HARD_MAX_ROWS_SCANNED_PER_DATASET);
    let rows_total = request
        .limits
        .max_rows_scanned_total
        .min(HARD_MAX_ROWS_SCANNED_PER_DATASET);
    let max_report_bytes = request.limits.max_report_bytes.min(HARD_MAX_REPORT_BYTES);
    if deadline.is_zero() || rows_per_dataset == 0 || rows_total == 0 || max_report_bytes == 0 {
        return Err(limit_exceeded());
    }

    let control = WorkspaceControl::new(deadline, cancellation);
    let template_proof = if request.mode == CopyMode::CreateFromTemplate {
        Some(verify_template_state_with_control(
            source,
            rows_total,
            rows_per_dataset,
            HARD_MAX_TEMPLATE_STREAM_BYTES,
            &control,
            cancellation,
        )?)
    } else {
        None
    };
    assert_source_current(source, &control, cancellation)?;
    control.install(source.verified.connection())?;
    let result = prepare_copy_inner(
        source,
        request,
        rows_per_dataset,
        rows_total,
        template_proof.as_ref(),
        &control,
    );
    let _ = source
        .verified
        .connection()
        .progress_handler(0, None::<fn() -> bool>);
    let report = result?;
    control.check()?;
    assert_source_current(source, &control, cancellation)?;
    let report_bytes = serde_json::to_vec(&report).map_err(|_| invalid_contract())?;
    if report_bytes.len() > max_report_bytes {
        return Err(limit_exceeded());
    }
    Ok(report)
}

fn prepare_copy_inner(
    source: &VerifiedWorkspaceSource,
    request: &CopyPreviewRequest,
    rows_per_dataset: u64,
    rows_total: u64,
    template_proof: Option<&TemplateStateProof>,
    control: &WorkspaceControl,
) -> Result<CopyPreviewReport, WorkspaceError> {
    let choices: BTreeMap<&str, &CopyDatasetChoice> = request
        .dataset_choices
        .iter()
        .map(|choice| (choice.dataset_id.as_str(), choice))
        .collect();
    let auto_selected = if request.mode == CopyMode::SelectiveFork {
        dependency_closure(source, &choices, control)?
    } else {
        BTreeSet::new()
    };

    let mut datasets = Vec::with_capacity(source.data_contract().datasets.len());
    let mut prompts = Vec::new();
    let mut blockers = Vec::new();
    let mut rows_remaining = rows_total;
    let semantic_mode = matches!(
        request.mode,
        CopyMode::ForkWithData | CopyMode::SelectiveFork | CopyMode::CreateFromTemplate
    );
    for dataset in source
        .data_contract()
        .datasets
        .iter()
        .filter(|_| semantic_mode)
    {
        control.check()?;
        let estimate = if let Some(proof) = template_proof {
            let dataset_proof = proof
                .datasets
                .iter()
                .find(|item| item.dataset_id == dataset.id)
                .ok_or_else(invalid_contract)?;
            CopyRowEstimate {
                rows: dataset_proof.stored_row_count,
                exact: true,
                truncated: false,
            }
        } else {
            estimate_dataset_rows(
                source,
                dataset,
                rows_per_dataset,
                &mut rows_remaining,
                control,
            )?
        };
        let choice = choices.get(dataset.id.as_str()).copied();
        let selected_by_dependency = auto_selected.contains(dataset.id.as_str());
        let action = decide_dataset(
            request.mode,
            dataset,
            choice,
            selected_by_dependency,
            &mut prompts,
            &mut blockers,
        );
        let dependencies = dataset
            .dependencies
            .iter()
            .map(|dependency| CopyDependency {
                dataset_id: dependency.dataset_id.clone(),
                reason: dependency.reason.clone(),
                auto_selected: auto_selected.contains(dependency.dataset_id.as_str()),
            })
            .collect();
        datasets.push(CopyDatasetDecision {
            dataset_id: dataset.id.clone(),
            role: dataset.role,
            sensitivity: dataset.sensitivity,
            required: dataset.required,
            signed_fork_policy: dataset.fork,
            action,
            row_estimate: estimate,
            dependencies,
        });
    }
    validate_dependency_actions(&datasets, &mut blockers);
    deduplicate(&mut blockers);
    deduplicate_prompts(&mut prompts);
    let availability = if blockers.iter().any(|blocker| {
        !matches!(
            blocker,
            CopyBlocker::DatasetChoiceRequired | CopyBlocker::SensitiveConfirmationRequired
        )
    }) {
        CopyAvailability::Blocked
    } else if blockers.is_empty() {
        CopyAvailability::Ready
    } else {
        CopyAvailability::NeedsReview
    };
    let truth = copy_mode_truth_table()
        .iter()
        .find(|entry| entry.mode == request.mode)
        .expect("copy mode truth table is exhaustive");
    let execution_blockers = execution_blockers(request.mode, &blockers);
    let execution_availability = match request.mode {
        CopyMode::ExactDuplicate => ExecutionAvailability::ExistingExactSnapshotExecutor,
        CopyMode::CompactDuplicate => ExecutionAvailability::ExistingCompactLogicalExecutor,
        CopyMode::ForkWithData | CopyMode::SelectiveFork => {
            ExecutionAvailability::ExistingSemanticExecutor
        }
        CopyMode::CreateFromTemplate => {
            if blockers.contains(&CopyBlocker::PolicyForbidsOperation) {
                ExecutionAvailability::Blocked
            } else {
                ExecutionAvailability::ExistingSemanticExecutor
            }
        }
    };
    Ok(CopyPreviewReport {
        profile: COPY_PREVIEW_PROFILE,
        mode: request.mode,
        availability,
        identity: CopyIdentityEffects {
            capsule_id: truth.capsule_identity,
            revision_id: truth.revision_identity,
            instance_profile: match request.mode {
                CopyMode::ExactDuplicate | CopyMode::CompactDuplicate => {
                    CopyInstanceProfileDisposition::Preserve
                }
                CopyMode::ForkWithData | CopyMode::SelectiveFork => {
                    CopyInstanceProfileDisposition::ExplicitForkPolicy
                }
                CopyMode::CreateFromTemplate => {
                    CopyInstanceProfileDisposition::VerifiedTemplatePolicy
                }
            },
            grants: match request.mode {
                CopyMode::ExactDuplicate | CopyMode::CompactDuplicate => {
                    CopyMutableStateDisposition::Preserve
                }
                _ => CopyMutableStateDisposition::Clear,
            },
            change_log: match request.mode {
                CopyMode::ExactDuplicate | CopyMode::CompactDuplicate => {
                    CopyMutableStateDisposition::Preserve
                }
                _ => CopyMutableStateDisposition::Clear,
            },
            lineage_operation: match request.mode {
                CopyMode::ExactDuplicate | CopyMode::CompactDuplicate => None,
                CopyMode::ForkWithData | CopyMode::SelectiveFork => Some("fork"),
                CopyMode::CreateFromTemplate => Some("created-from-template"),
            },
        },
        datasets,
        prompts,
        blockers,
        expected_application_digest: Some(lower_hex(source.application_digest())),
        output: CopyOutputConstraints {
            format_version: "0.3",
            signed_application_profile: "org.sqlite-capsule.signed-app/0.3",
            source_is_read_only: true,
            create_new: true,
            overwrite_allowed: false,
            application_digest_must_match_source: true,
            application_digest_from: DigestExpectation::Source,
            publish_only_after_full_verification: true,
        },
        execution_availability,
        execution_blockers,
        execution_must_rederive_decisions: true,
        execution_authority_issued: false,
    })
}

fn validate_dependency_actions(datasets: &[CopyDatasetDecision], blockers: &mut Vec<CopyBlocker>) {
    let actions: BTreeMap<&str, Option<CopyDatasetAction>> = datasets
        .iter()
        .map(|dataset| (dataset.dataset_id.as_str(), dataset.action))
        .collect();
    for dataset in datasets
        .iter()
        .filter(|dataset| dataset.action == Some(CopyDatasetAction::Copy))
    {
        for dependency in &dataset.dependencies {
            if actions.get(dependency.dataset_id.as_str()) != Some(&Some(CopyDatasetAction::Copy)) {
                blockers.push(CopyBlocker::DependencyNotPermitted);
            }
        }
    }
}

fn execution_blockers(mode: CopyMode, planning: &[CopyBlocker]) -> Vec<CopyBlockingReason> {
    let mut codes = vec![CopyBlocker::PreviewNotExecutionAuthority];
    match mode {
        CopyMode::ExactDuplicate => {}
        CopyMode::CompactDuplicate => {}
        CopyMode::CreateFromTemplate => {}
        CopyMode::ForkWithData | CopyMode::SelectiveFork => {}
    }
    codes.extend(planning.iter().copied());
    deduplicate(&mut codes);
    codes
        .into_iter()
        .map(|code| CopyBlockingReason {
            code,
            safe_reason: blocker_reason(code),
        })
        .collect()
}

const fn blocker_reason(code: CopyBlocker) -> &'static str {
    match code {
        CopyBlocker::VerifiedTemplateRequired => {
            "Template creation requires a separately verified clean application release."
        }
        CopyBlocker::ResetSemanticsUnavailable => {
            "The signed policy requests reset semantics that this preview cannot execute safely."
        }
        CopyBlocker::PolicyForbidsOperation => "A signed dataset policy forbids this operation.",
        CopyBlocker::DatasetChoiceRequired => "A signed prompt policy requires a dataset choice.",
        CopyBlocker::SensitiveConfirmationRequired => {
            "Sensitive data remains omitted until explicit confirmation."
        }
        CopyBlocker::SelectiveOmissionNotPermitted => {
            "The signed policy does not permit omitting an unselected dataset."
        }
        CopyBlocker::RequiredDatasetOmitted => "A required dataset cannot be omitted.",
        CopyBlocker::DependencyNotPermitted => {
            "A dependency cannot be copied under its signed fork policy."
        }
        CopyBlocker::SensitiveCopyConfirmationRequired => {
            "A signed copy action remains blocked until sensitive-data inclusion is explicitly confirmed."
        }
        CopyBlocker::PreviewNotExecutionAuthority => {
            "This report is advisory and never authorizes execution."
        }
        CopyBlocker::CompactSecurityDesignUnavailable => {
            "Compact copy remains unavailable on this host profile."
        }
        CopyBlocker::ForkExecutorUnavailable => {
            "Fork execution remains disabled until identity, lineage and publication checks are implemented."
        }
        CopyBlocker::SelectiveExecutorUnavailable => {
            "Selective-fork execution remains disabled while policy transforms are preview-only."
        }
        CopyBlocker::CrossDatasetForeignKeysUnverified => {
            "Cross-dataset foreign-key coverage has not been proven for this source profile."
        }
        CopyBlocker::CleanSeedStateUnprovable => {
            "The application digest excludes domain rows and cannot prove that this source is a clean seed release."
        }
    }
}

fn decide_dataset(
    mode: CopyMode,
    dataset: &crate::Dataset,
    choice: Option<&CopyDatasetChoice>,
    selected_by_dependency: bool,
    prompts: &mut Vec<CopyPrompt>,
    blockers: &mut Vec<CopyBlocker>,
) -> Option<CopyDatasetAction> {
    match mode {
        // Exact and compact duplicate preserve the whole snapshot/logical
        // database. They intentionally do not interpret dataset policies.
        CopyMode::ExactDuplicate | CopyMode::CompactDuplicate => None,
        CopyMode::CreateFromTemplate => match dataset.fork {
            ForkPolicy::Forbid => {
                blockers.push(CopyBlocker::PolicyForbidsOperation);
                Some(CopyDatasetAction::Forbid)
            }
            _ => Some(CopyDatasetAction::Reset),
        },
        CopyMode::ForkWithData => Some(decide_fork_dataset(dataset, choice, prompts, blockers)),
        CopyMode::SelectiveFork => {
            decide_selective_dataset(dataset, choice, selected_by_dependency, prompts, blockers)
        }
    }
}

fn decide_fork_dataset(
    dataset: &crate::Dataset,
    choice: Option<&CopyDatasetChoice>,
    prompts: &mut Vec<CopyPrompt>,
    blockers: &mut Vec<CopyBlocker>,
) -> CopyDatasetAction {
    match dataset.fork {
        ForkPolicy::Copy => decide_copy(dataset, choice, prompts, blockers),
        ForkPolicy::Prompt => decide_prompt(dataset, choice, prompts, blockers),
        ForkPolicy::Omit => CopyDatasetAction::Omit,
        ForkPolicy::Reset => {
            blockers.push(CopyBlocker::ResetSemanticsUnavailable);
            CopyDatasetAction::Reset
        }
        ForkPolicy::Forbid => {
            blockers.push(CopyBlocker::PolicyForbidsOperation);
            CopyDatasetAction::Forbid
        }
    }
}

fn decide_selective_dataset(
    dataset: &crate::Dataset,
    choice: Option<&CopyDatasetChoice>,
    selected_by_dependency: bool,
    prompts: &mut Vec<CopyPrompt>,
    blockers: &mut Vec<CopyBlocker>,
) -> Option<CopyDatasetAction> {
    let include = choice
        .is_some_and(|choice| choice.disposition == DatasetChoiceDisposition::Include)
        || selected_by_dependency;
    if include {
        if !matches!(dataset.fork, ForkPolicy::Copy | ForkPolicy::Prompt) {
            blockers.push(CopyBlocker::DependencyNotPermitted);
            return None;
        }
        if dataset.sensitivity == Sensitivity::Sensitive
            && !choice.is_some_and(|choice| choice.sensitive_confirmed)
        {
            prompts.push(prompt(
                dataset,
                CopyPromptKind::SensitiveConfirmation,
                CopyDatasetAction::Omit,
            ));
            blockers.push(CopyBlocker::SensitiveConfirmationRequired);
            return Some(CopyDatasetAction::Prompt);
        }
        return Some(CopyDatasetAction::Copy);
    }
    if dataset.required {
        blockers.push(CopyBlocker::RequiredDatasetOmitted);
        return None;
    }
    match dataset.fork {
        ForkPolicy::Omit => Some(CopyDatasetAction::Omit),
        ForkPolicy::Reset => {
            blockers.push(CopyBlocker::ResetSemanticsUnavailable);
            Some(CopyDatasetAction::Reset)
        }
        _ => {
            blockers.push(CopyBlocker::SelectiveOmissionNotPermitted);
            None
        }
    }
}

fn decide_copy(
    dataset: &crate::Dataset,
    choice: Option<&CopyDatasetChoice>,
    prompts: &mut Vec<CopyPrompt>,
    blockers: &mut Vec<CopyBlocker>,
) -> CopyDatasetAction {
    if choice.is_some_and(|choice| choice.disposition == DatasetChoiceDisposition::Omit) {
        blockers.push(CopyBlocker::PolicyForbidsOperation);
        return CopyDatasetAction::Forbid;
    }
    if dataset.sensitivity != Sensitivity::Sensitive {
        return CopyDatasetAction::Copy;
    }
    if choice.is_some_and(|choice| {
        choice.disposition == DatasetChoiceDisposition::Include && choice.sensitive_confirmed
    }) {
        return CopyDatasetAction::Copy;
    }
    prompts.push(prompt(
        dataset,
        CopyPromptKind::SensitiveConfirmation,
        CopyDatasetAction::Forbid,
    ));
    blockers.push(CopyBlocker::SensitiveCopyConfirmationRequired);
    CopyDatasetAction::Copy
}

fn decide_prompt(
    dataset: &crate::Dataset,
    choice: Option<&CopyDatasetChoice>,
    prompts: &mut Vec<CopyPrompt>,
    blockers: &mut Vec<CopyBlocker>,
) -> CopyDatasetAction {
    let Some(choice) = choice else {
        prompts.push(prompt(
            dataset,
            CopyPromptKind::DatasetChoice,
            CopyDatasetAction::Omit,
        ));
        blockers.push(CopyBlocker::DatasetChoiceRequired);
        return CopyDatasetAction::Prompt;
    };
    if choice.disposition == DatasetChoiceDisposition::Omit {
        if dataset.required {
            blockers.push(CopyBlocker::RequiredDatasetOmitted);
            return CopyDatasetAction::Forbid;
        }
        return CopyDatasetAction::Omit;
    }
    if dataset.sensitivity == Sensitivity::Sensitive && !choice.sensitive_confirmed {
        prompts.push(prompt(
            dataset,
            CopyPromptKind::SensitiveConfirmation,
            CopyDatasetAction::Omit,
        ));
        blockers.push(CopyBlocker::SensitiveConfirmationRequired);
        return CopyDatasetAction::Prompt;
    }
    CopyDatasetAction::Copy
}

fn prompt(
    dataset: &crate::Dataset,
    kind: CopyPromptKind,
    unconfirmed_action: CopyDatasetAction,
) -> CopyPrompt {
    CopyPrompt {
        dataset_id: dataset.id.clone(),
        kind,
        unconfirmed_action,
    }
}

fn dependency_closure(
    source: &VerifiedWorkspaceSource,
    choices: &BTreeMap<&str, &CopyDatasetChoice>,
    control: &WorkspaceControl,
) -> Result<BTreeSet<String>, WorkspaceError> {
    let datasets: BTreeMap<&str, &crate::Dataset> = source
        .data_contract()
        .datasets
        .iter()
        .map(|dataset| (dataset.id.as_str(), dataset))
        .collect();
    let mut closure: BTreeSet<String> = choices
        .values()
        .filter(|choice| choice.disposition == DatasetChoiceDisposition::Include)
        .map(|choice| choice.dataset_id.clone())
        .collect();
    let mut pending: Vec<String> = closure.iter().cloned().collect();
    while let Some(dataset_id) = pending.pop() {
        control.check()?;
        let dataset = datasets
            .get(dataset_id.as_str())
            .ok_or_else(invalid_contract)?;
        for dependency in &dataset.dependencies {
            if closure.insert(dependency.dataset_id.clone()) {
                pending.push(dependency.dataset_id.clone());
            }
        }
    }
    Ok(closure)
}

fn validate_request(
    source: &VerifiedWorkspaceSource,
    request: &CopyPreviewRequest,
) -> Result<(), WorkspaceError> {
    if request.dataset_choices.len() > MAX_DATASET_CHOICES
        || matches!(
            request.mode,
            CopyMode::ExactDuplicate | CopyMode::CompactDuplicate | CopyMode::CreateFromTemplate
        ) && !request.dataset_choices.is_empty()
    {
        return Err(invalid_contract());
    }
    let declared: BTreeSet<&str> = source
        .data_contract()
        .datasets
        .iter()
        .map(|dataset| dataset.id.as_str())
        .collect();
    let mut unique = BTreeSet::new();
    for choice in &request.dataset_choices {
        if !declared.contains(choice.dataset_id.as_str()) || !unique.insert(&choice.dataset_id) {
            return Err(invalid_contract());
        }
        if request.mode == CopyMode::ForkWithData {
            let dataset = source
                .data_contract()
                .datasets
                .iter()
                .find(|dataset| dataset.id == choice.dataset_id)
                .ok_or_else(invalid_contract)?;
            let choice_permitted = dataset.fork == ForkPolicy::Prompt
                || (dataset.fork == ForkPolicy::Copy
                    && dataset.sensitivity == Sensitivity::Sensitive
                    && choice.disposition == DatasetChoiceDisposition::Include);
            if !choice_permitted {
                return Err(invalid_contract());
            }
        }
    }
    Ok(())
}

fn assert_source_current(
    source: &VerifiedWorkspaceSource,
    control: &WorkspaceControl,
    cancellation: &CancellationToken,
) -> Result<(), WorkspaceError> {
    let limits = WorkspaceLimits {
        deadline: control.remaining()?,
        ..WorkspaceLimits::default()
    };
    source.assert_current_with_control(&limits, cancellation)
}

fn estimate_dataset_rows(
    source: &VerifiedWorkspaceSource,
    dataset: &crate::Dataset,
    maximum: u64,
    total_remaining: &mut u64,
    control: &WorkspaceControl,
) -> Result<CopyRowEstimate, WorkspaceError> {
    if *total_remaining == 0 {
        return Ok(CopyRowEstimate {
            rows: 0,
            exact: false,
            truncated: true,
        });
    }
    let maximum = maximum.min(*total_remaining);
    let mut rows = 0_u64;
    for table in &dataset.tables {
        control.check()?;
        let remaining = maximum.saturating_sub(rows);
        let probe = remaining.min(*total_remaining);
        if probe == 0 {
            return Ok(CopyRowEstimate {
                rows,
                exact: false,
                truncated: true,
            });
        }
        let probe = i64::try_from(probe).map_err(|_| limit_exceeded())?;
        let sql = format!(
            "SELECT 1 FROM \"{}\" LIMIT ?1",
            table.name.replace('"', "\"\"")
        );
        let mut statement = source
            .verified
            .connection()
            .prepare(&sql)
            .map_err(|_| invalid_contract())?;
        let mut query = statement
            .query(params![probe])
            .map_err(|_| invalid_contract())?;
        let mut table_rows = 0_u64;
        while query.next().map_err(|_| invalid_contract())?.is_some() {
            control.check()?;
            table_rows = table_rows.checked_add(1).ok_or_else(limit_exceeded)?;
        }
        *total_remaining = total_remaining.saturating_sub(table_rows);
        if table_rows == probe as u64 {
            return Ok(CopyRowEstimate {
                rows: maximum,
                exact: false,
                truncated: true,
            });
        }
        rows = rows.checked_add(table_rows).ok_or_else(limit_exceeded)?;
    }
    Ok(CopyRowEstimate {
        rows,
        exact: true,
        truncated: false,
    })
}

fn deduplicate(values: &mut Vec<CopyBlocker>) {
    let mut seen = BTreeSet::new();
    values.retain(|value| seen.insert(*value as u8));
}

fn deduplicate_prompts(values: &mut Vec<CopyPrompt>) {
    let mut seen = BTreeSet::new();
    values.retain(|value| seen.insert((value.dataset_id.clone(), value.kind as u8)));
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

#[cfg(test)]
mod tests {
    use std::fs;

    use ed25519_dalek::SigningKey;
    use rusqlite::Connection;
    use sqlite_capsule_crypto::{PROFILE_V03, application_digest, sign_digest_for_profile};

    use super::*;

    const DEVELOPMENT_SEED: &str =
        include_str!("../../../../compatibility/signed-app-v0.2/development-seed.hex");

    fn source(
        name: &str,
    ) -> (
        tempfile::TempDir,
        std::path::PathBuf,
        VerifiedWorkspaceSource,
    ) {
        let (directory, path) = crate::tests::signed_fixture(name);
        let verified = VerifiedWorkspaceSource::open(&path).expect("verified workspace source");
        (directory, path, verified)
    }

    fn source_with_policy(
        name: &str,
        sql: &str,
    ) -> (
        tempfile::TempDir,
        std::path::PathBuf,
        VerifiedWorkspaceSource,
    ) {
        let (directory, path) = crate::tests::signed_fixture(name);
        let connection = Connection::open(&path).expect("open fixture");
        connection.execute_batch(sql).expect("change signed policy");
        resign(&connection);
        drop(connection);
        let verified = VerifiedWorkspaceSource::open(&path).expect("verified workspace source");
        (directory, path, verified)
    }

    fn resign(connection: &Connection) {
        connection
            .execute("DELETE FROM capsule_signature", [])
            .expect("remove signature");
        let digest = application_digest(connection).expect("application digest");
        let seed_text = DEVELOPMENT_SEED.trim();
        let mut seed = [0_u8; 32];
        for (index, byte) in seed.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&seed_text[index * 2..index * 2 + 2], 16)
                .expect("development seed hex");
        }
        let key = SigningKey::from_bytes(&seed);
        seed.fill(0);
        let envelope = sign_digest_for_profile(&key, digest, "2026-08-08T12:34:56Z", PROFILE_V03)
            .expect("sign fixture");
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
            .expect("store signature");
    }

    fn install_template_proof(path: &std::path::Path) {
        let source = VerifiedWorkspaceSource::open(path).expect("template source");
        let identity = source.identity();
        let schema = identity.overview.data_schema.as_ref().expect("data schema");
        let datasets = source
            .data_contract()
            .datasets
            .iter()
            .map(|dataset| {
                let (stored_row_count, state_sha256) =
                    crate::template_state::dataset_state_for_test(&source, dataset)
                        .expect("dataset state");
                serde_json::json!({
                    "dataset_id": dataset.id,
                    "disposition": if stored_row_count == 0 { "empty" } else { "seed" },
                    "stored_row_count": stored_row_count,
                    "state_sha256": state_sha256
                })
            })
            .collect::<Vec<_>>();
        let proof = serde_json::json!({
            "profile": crate::template_state::TEMPLATE_STATE_PROFILE,
            "app_id": identity.app_id,
            "app_version": identity.app_version,
            "data_schema_id": schema.data_schema_id,
            "data_schema_version": schema.data_schema_version,
            "dataset_state_profile": crate::template_state::DATASET_STATE_PROFILE,
            "mutable_platform_state_profile": crate::template_state::TEMPLATE_PLATFORM_RESET_PROFILE,
            "datasets": datasets
        });
        let proof = crate::plan::canonical_json(&proof).expect("canonical template proof");
        drop(source);
        let connection = Connection::open(path).expect("open template fixture");
        connection
            .execute(
                "INSERT OR REPLACE INTO capsule_doc (slug, title, media_type, content, sequence) \
                 VALUES ('org.sqlite-capsule.template-state', \
                 'SQLite Capsule authenticated template state', \
                 'application/vnd.sqlite-capsule.template-state+json', ?1, 0)",
                [std::str::from_utf8(&proof).expect("proof UTF-8")],
            )
            .expect("install template proof");
        resign(&connection);
    }

    fn choice(
        dataset_id: &str,
        disposition: DatasetChoiceDisposition,
        sensitive_confirmed: bool,
    ) -> CopyDatasetChoice {
        CopyDatasetChoice {
            dataset_id: dataset_id.to_owned(),
            disposition,
            sensitive_confirmed,
        }
    }

    #[test]
    fn truth_table_keeps_v02_to_duplicate_modes_and_new_identity_to_v03_forks() {
        let table = copy_mode_truth_table();
        assert_eq!(table.len(), 5);
        assert_eq!(table[0].v0_2, CopyFormatAvailability::Supported);
        assert_eq!(table[1].v0_2, CopyFormatAvailability::Supported);
        for entry in &table[2..] {
            assert_eq!(entry.v0_2, CopyFormatAvailability::Unsupported);
            assert_eq!(entry.capsule_identity, CopyIdentityDisposition::GenerateNew);
            assert_eq!(
                entry.revision_identity,
                CopyIdentityDisposition::GenerateNew
            );
        }
        assert_eq!(
            table[3].v0_3,
            CopyFormatAvailability::RequiresVerifiedTemplate
        );
    }

    #[test]
    fn exact_duplicate_report_is_bounded_review_data_and_does_not_mutate_source() {
        let (_directory, path, source) = source("copy-report-exact");
        let before = fs::read(&path).expect("source bytes");
        let report = preview_copy(
            &source,
            &CopyPreviewRequest::new(CopyMode::ExactDuplicate),
            &CancellationToken::new(),
        )
        .expect("copy report");
        assert_eq!(report.profile, COPY_PREVIEW_PROFILE);
        assert_eq!(report.availability, CopyAvailability::Ready);
        assert_eq!(
            report.identity.capsule_id,
            CopyIdentityDisposition::Preserve
        );
        assert_eq!(
            report.identity.revision_id,
            CopyIdentityDisposition::Preserve
        );
        assert_eq!(report.identity.lineage_operation, None);
        assert_eq!(
            report.identity.instance_profile,
            CopyInstanceProfileDisposition::Preserve
        );
        assert_eq!(
            report.identity.grants,
            CopyMutableStateDisposition::Preserve
        );
        assert!(report.datasets.is_empty());
        assert_eq!(
            report
                .expected_application_digest
                .as_deref()
                .expect("source digest")
                .len(),
            64
        );
        assert!(report.output.source_is_read_only);
        assert!(report.output.create_new);
        assert!(!report.output.overwrite_allowed);
        assert!(report.output.publish_only_after_full_verification);
        assert_eq!(report.output.format_version, "0.3");
        assert_eq!(
            report.output.signed_application_profile,
            "org.sqlite-capsule.signed-app/0.3"
        );
        assert_eq!(
            report.execution_availability,
            ExecutionAvailability::ExistingExactSnapshotExecutor
        );
        assert!(report.execution_must_rederive_decisions);
        assert_eq!(report.execution_blockers.len(), 1);
        assert_eq!(
            report.execution_blockers[0].code,
            CopyBlocker::PreviewNotExecutionAuthority
        );
        assert!(!report.execution_authority_issued);
        assert_eq!(fs::read(&path).expect("source after planning"), before);
    }

    #[test]
    fn fork_report_projects_dependencies_and_new_identity_without_paths() {
        let (_directory, _path, source) = source("copy-report-fork");
        let report = preview_copy(
            &source,
            &CopyPreviewRequest::new(CopyMode::ForkWithData),
            &CancellationToken::new(),
        )
        .expect("fork report");
        assert_eq!(report.availability, CopyAvailability::Ready);
        assert_eq!(
            report.identity.capsule_id,
            CopyIdentityDisposition::GenerateNew
        );
        assert_eq!(
            report.identity.revision_id,
            CopyIdentityDisposition::GenerateNew
        );
        assert_eq!(report.identity.lineage_operation, Some("fork"));
        assert_eq!(
            report.identity.instance_profile,
            CopyInstanceProfileDisposition::ExplicitForkPolicy
        );
        assert_eq!(report.identity.grants, CopyMutableStateDisposition::Clear);
        assert_eq!(
            report.identity.change_log,
            CopyMutableStateDisposition::Clear
        );
        let content = report
            .datasets
            .iter()
            .find(|dataset| dataset.dataset_id == "content")
            .expect("content decision");
        assert_eq!(content.action, Some(CopyDatasetAction::Copy));
        assert_eq!(content.dependencies.len(), 1);
        assert_eq!(content.dependencies[0].dataset_id, "settings");
        let json = serde_json::to_string(&report).expect("report JSON");
        assert!(!json.contains("sqlitecapsule"));
        assert!(!json.contains("path"));
    }

    #[test]
    fn signed_sensitive_copy_stays_copy_but_blocks_until_explicit_confirmation() {
        let (_directory, _path, source) = source_with_policy(
            "copy-report-sensitive",
            "UPDATE capsule_dataset SET sensitivity = 'sensitive' WHERE id = 'content';",
        );
        let report = preview_copy(
            &source,
            &CopyPreviewRequest::new(CopyMode::ForkWithData),
            &CancellationToken::new(),
        )
        .expect("sensitive report");
        assert_eq!(report.availability, CopyAvailability::Blocked);
        assert_eq!(report.prompts.len(), 1);
        assert_eq!(
            report.prompts[0].kind,
            CopyPromptKind::SensitiveConfirmation
        );
        assert_eq!(
            report.prompts[0].unconfirmed_action,
            CopyDatasetAction::Forbid
        );

        let mut request = CopyPreviewRequest::new(CopyMode::ForkWithData);
        request
            .dataset_choices
            .push(choice("content", DatasetChoiceDisposition::Include, true));
        let confirmed =
            preview_copy(&source, &request, &CancellationToken::new()).expect("confirmed report");
        assert_eq!(confirmed.availability, CopyAvailability::Ready);
        assert!(confirmed.prompts.is_empty());
        assert_eq!(confirmed.datasets[0].action, Some(CopyDatasetAction::Copy));
    }

    #[test]
    fn selective_fork_adds_dependencies_and_rejects_policy_invalid_omissions() {
        let (_directory, _path, source) = source("copy-report-selective");
        let mut request = CopyPreviewRequest::new(CopyMode::SelectiveFork);
        request
            .dataset_choices
            .push(choice("content", DatasetChoiceDisposition::Include, false));
        let report = preview_copy(&source, &request, &CancellationToken::new())
            .expect("dependency-closed report");
        assert_eq!(report.availability, CopyAvailability::Ready);
        assert!(
            report
                .datasets
                .iter()
                .all(|dataset| dataset.action == Some(CopyDatasetAction::Copy))
        );
        assert!(report.datasets[0].dependencies[0].auto_selected);

        let mut invalid = CopyPreviewRequest::new(CopyMode::SelectiveFork);
        invalid
            .dataset_choices
            .push(choice("settings", DatasetChoiceDisposition::Include, false));
        let blocked = preview_copy(&source, &invalid, &CancellationToken::new())
            .expect("bounded blocked report");
        assert_eq!(blocked.availability, CopyAvailability::Blocked);
        assert!(
            blocked
                .blockers
                .contains(&CopyBlocker::RequiredDatasetOmitted)
        );
    }

    #[test]
    fn full_fork_never_copies_a_dependent_while_omitting_its_prerequisite() {
        let (_directory, _path, source) = source_with_policy(
            "copy-report-hostile-dependency",
            "UPDATE capsule_dataset SET fork_policy = 'prompt', required = 0 \
             WHERE id = 'settings';",
        );
        let mut request = CopyPreviewRequest::new(CopyMode::ForkWithData);
        request
            .dataset_choices
            .push(choice("settings", DatasetChoiceDisposition::Omit, false));
        let report = preview_copy(&source, &request, &CancellationToken::new())
            .expect("bounded hostile dependency report");
        assert_eq!(report.availability, CopyAvailability::Blocked);
        assert!(
            report
                .blockers
                .contains(&CopyBlocker::DependencyNotPermitted)
        );
        assert_eq!(
            report
                .datasets
                .iter()
                .find(|dataset| dataset.dataset_id == "content")
                .expect("dependent")
                .action,
            Some(CopyDatasetAction::Copy)
        );
        assert_eq!(
            report
                .datasets
                .iter()
                .find(|dataset| dataset.dataset_id == "settings")
                .expect("prerequisite")
                .action,
            Some(CopyDatasetAction::Omit)
        );
    }

    #[test]
    fn authenticated_template_preview_is_ready_and_stale_or_missing_proofs_fail_closed() {
        let (directory, path) = crate::tests::signed_fixture("copy-report-template");
        let missing = VerifiedWorkspaceSource::open(&path).expect("source without proof");
        assert_eq!(
            preview_copy(
                &missing,
                &CopyPreviewRequest::new(CopyMode::CreateFromTemplate),
                &CancellationToken::new(),
            )
            .expect_err("missing proof"),
            invalid_contract()
        );
        drop(missing);

        let connection = Connection::open(&path).expect("extend template dataset");
        connection
            .execute(
                "INSERT INTO vector_domain (id, note, measurement, payload) \
                 VALUES ('second', 'seed', 0.0, X'42')",
                [],
            )
            .expect("second authenticated template row");
        drop(connection);
        install_template_proof(&path);
        let source = VerifiedWorkspaceSource::open(&path).expect("authenticated template");
        let template = preview_copy(
            &source,
            &CopyPreviewRequest::new(CopyMode::CreateFromTemplate),
            &CancellationToken::new(),
        )
        .expect("ready template report");
        assert_eq!(template.availability, CopyAvailability::Ready);
        assert_eq!(template.datasets.len(), 2);
        assert!(template.datasets.iter().all(|dataset| {
            dataset.action == Some(CopyDatasetAction::Reset) && dataset.row_estimate.exact
        }));
        assert_eq!(
            template.expected_application_digest,
            Some(lower_hex(source.application_digest()))
        );
        assert_eq!(
            template.output.application_digest_from,
            DigestExpectation::Source
        );
        assert!(template.output.application_digest_must_match_source);
        assert_eq!(
            template.execution_availability,
            ExecutionAvailability::ExistingSemanticExecutor
        );

        let mut lowered = CopyPreviewRequest::new(CopyMode::CreateFromTemplate);
        lowered.limits.max_rows_scanned_per_dataset = 1;
        assert_eq!(
            preview_copy(&source, &lowered, &CancellationToken::new())
                .expect_err("caller-lowered template dataset limit"),
            limit_exceeded()
        );
        drop(source);

        let connection = Connection::open(&path).expect("mutate template source");
        connection
            .execute(
                "UPDATE vector_domain SET note = 'stale-proof' WHERE id = 'domain'",
                [],
            )
            .expect("change domain row");
        drop(connection);
        let stale = VerifiedWorkspaceSource::open(&path).expect("stale proof source");
        assert_eq!(
            preview_copy(
                &stale,
                &CopyPreviewRequest::new(CopyMode::CreateFromTemplate),
                &CancellationToken::new(),
            )
            .expect_err("stale proof"),
            invalid_contract()
        );
        drop(directory);
    }

    #[test]
    fn reset_semantics_for_one_source_forks_remain_unavailable() {
        let (_directory, _path, source) = source_with_policy(
            "copy-report-reset",
            "UPDATE capsule_dataset SET fork_policy = 'reset' WHERE id = 'content';",
        );
        let reset = preview_copy(
            &source,
            &CopyPreviewRequest::new(CopyMode::ForkWithData),
            &CancellationToken::new(),
        )
        .expect("blocked reset report");
        assert_eq!(reset.availability, CopyAvailability::Blocked);
        assert!(
            reset
                .blockers
                .contains(&CopyBlocker::ResetSemanticsUnavailable)
        );
    }

    #[test]
    fn choices_limits_and_cancellation_fail_closed() {
        let (_directory, _path, source) = source("copy-report-limits");
        let mut unknown = CopyPreviewRequest::new(CopyMode::ForkWithData);
        unknown.dataset_choices.push(choice(
            "not-declared",
            DatasetChoiceDisposition::Include,
            false,
        ));
        assert_eq!(
            preview_copy(&source, &unknown, &CancellationToken::new())
                .expect_err("unknown dataset")
                .kind(),
            WorkspaceErrorCode::InvalidContract
        );

        let mut tiny = CopyPreviewRequest::new(CopyMode::ExactDuplicate);
        tiny.limits.max_report_bytes = 1;
        assert_eq!(
            preview_copy(&source, &tiny, &CancellationToken::new())
                .expect_err("report bound")
                .kind(),
            WorkspaceErrorCode::LimitExceeded
        );

        let cancellation = CancellationToken::new();
        cancellation.cancel();
        assert_eq!(
            preview_copy(
                &source,
                &CopyPreviewRequest::new(CopyMode::ExactDuplicate),
                &cancellation,
            )
            .expect_err("cancelled")
            .kind(),
            WorkspaceErrorCode::Cancelled
        );
    }

    #[test]
    fn row_estimates_share_one_total_budget_and_expose_no_values() {
        let (_directory, _path, source) = source("copy-report-row-budget");
        let mut request = CopyPreviewRequest::new(CopyMode::ForkWithData);
        request.limits.max_rows_scanned_total = 1;
        request.limits.max_rows_scanned_per_dataset = 10;
        let report =
            preview_copy(&source, &request, &CancellationToken::new()).expect("budgeted report");
        assert_eq!(
            report
                .datasets
                .iter()
                .map(|dataset| dataset.row_estimate.rows)
                .sum::<u64>(),
            1
        );
        assert!(
            report
                .datasets
                .iter()
                .any(|dataset| dataset.row_estimate.truncated)
        );
        let json = serde_json::to_string(&report).expect("report JSON");
        assert!(!json.contains("mutable"));
        assert!(!json.contains("light"));
        assert!(!json.contains("102030"));
    }
}

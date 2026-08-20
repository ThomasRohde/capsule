use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::Value;
use sqlite_capsule_workspace::{
    CancellationToken, CompactCopyPlanRequest, CompareDataState, CompareDetailLimits,
    CompareDetailRowKind, CompareLimits, CompareSummary, DuplicatePlanLimits, DuplicatePlanRequest,
    ExactCopyPlanRequest, LifecyclePlan, ReconcileAction, ReconcileOutputRequest, ReconcilePolicy,
    ReconcileReview, ReconcileReviewLimits, ReconcileSelection, SemanticChoiceDisposition,
    SemanticCopyMode, SemanticCopyPlanRequest, SemanticDatasetChoice, Sensitivity,
    ThreeWayConflictKind, ThreeWayConflictResolution, ThreeWayConflictReview, ThreeWayDeletedSide,
    ThreeWayResolutionChoice, UpgradeApproval, UpgradePlanRequest, UpgradeReviewReport,
    VerifiedCompactSource, VerifiedCopySource, VerifiedWorkspaceSource, WorkspaceError,
    WorkspaceErrorCode, WorkspaceLimits, classify_three_way_reconcile, compare_sources,
    comparison_detail_page, generate_compact_copy_plan, generate_duplicate_plan,
    generate_exact_copy_plan, generate_semantic_copy_plan, open_semantic_copy_source,
    parse_compact_copy_plan, parse_exact_copy_plan, parse_semantic_copy_plan, parse_upgrade_plan,
    prepare_reconcile_review, prepare_upgrade_review,
};

const RESULT_PROFILE: &str = "org.sqlite-capsule.workspace-copy-result/1";
const RECONCILE_CANDIDATES_PROFILE: &str = "org.sqlite-capsule.reconcile-cli-candidates/1";
const THREE_WAY_CANDIDATES_PROFILE: &str =
    "org.sqlite-capsule.reconcile-cli-three-way-candidates/1";
const RECONCILE_REVIEW_PROFILE: &str = "org.sqlite-capsule.reconcile-cli-review/1";
const RECONCILE_RESULT_PROFILE: &str = "org.sqlite-capsule.reconcile-cli-result/1";
const UPGRADE_REVIEW_PROFILE: &str = "org.sqlite-capsule.upgrade-cli-review/1";
const UPGRADE_RESULT_PROFILE: &str = "org.sqlite-capsule.upgrade-cli-result/1";
const RECONCILE_DEADLINE: Duration = Duration::from_secs(30);
const RECONCILE_EXPIRY: Duration = Duration::from_secs(5 * 60);
const MAX_CLI_SELECTIONS: usize = 10_000;

#[derive(Serialize)]
struct CopyResult<'a> {
    profile: &'static str,
    mode: &'a str,
    output_leaf: &'a str,
    output_bytes: u64,
}

#[derive(Serialize)]
struct ReconcileCandidateField<'a> {
    field_index: usize,
    column: &'a str,
    kind: sqlite_capsule_workspace::CompareDetailFieldKind,
    source_value_digest: Option<&'a str>,
    target_value_digest: Option<&'a str>,
}

#[derive(Serialize)]
struct ReconcileCandidateRow<'a> {
    kind: CompareDetailRowKind,
    key_digest: &'a str,
    source_row_digest: Option<&'a str>,
    target_row_digest: Option<&'a str>,
    fields: Vec<ReconcileCandidateField<'a>>,
}

#[derive(Serialize)]
struct ReconcileCandidates<'a> {
    profile: &'static str,
    compare_report_digest: &'a str,
    dataset_index: usize,
    table_index: usize,
    dataset_label: &'a str,
    table_label: &'a str,
    rows: Vec<ReconcileCandidateRow<'a>>,
    truncated: bool,
    note: &'static str,
}

#[derive(Serialize)]
struct ThreeWayCandidates<'a> {
    profile: &'static str,
    compare_report_digest: &'a str,
    clean_change_count: usize,
    conflicts: Vec<ThreeWayCandidateConflict<'a>>,
    executable_authority: bool,
    note: &'static str,
}

#[derive(Serialize)]
struct ThreeWayCandidateConflict<'a> {
    id: &'a str,
    dataset_id: &'a str,
    table: &'a str,
    key_digest: &'a str,
    kind: ThreeWayConflictKind,
    deleted_side: Option<ThreeWayDeletedSide>,
    source_row_digest: Option<&'a str>,
    target_row_digest: Option<&'a str>,
    ancestor_row_digest: Option<&'a str>,
    allowed_choices: &'a [ThreeWayResolutionChoice],
}

impl<'a> From<&'a ThreeWayConflictReview> for ThreeWayCandidateConflict<'a> {
    fn from(conflict: &'a ThreeWayConflictReview) -> Self {
        Self {
            id: &conflict.id,
            dataset_id: &conflict.dataset_id,
            table: &conflict.table,
            key_digest: &conflict.key_digest,
            kind: conflict.kind,
            deleted_side: conflict.deleted_side,
            source_row_digest: conflict.source_row_digest.as_deref(),
            target_row_digest: conflict.target_row_digest.as_deref(),
            ancestor_row_digest: conflict.ancestor_row_digest.as_deref(),
            allowed_choices: &conflict.allowed_choices,
        }
    }
}

#[derive(Serialize)]
struct ReconcileCliReview {
    profile: &'static str,
    compare_report_digest: String,
    review_digest: String,
    operation_count: usize,
    plan: Value,
    payload: Value,
    executable_authority: bool,
    note: &'static str,
}

#[derive(Serialize)]
struct ReconcileCliResult {
    profile: &'static str,
    operation_count: usize,
    plan_digest: String,
    payload_digest: String,
    compare_report_digest: String,
    output_leaf: String,
    output_bytes: u64,
    capsule_id: String,
    revision_id: String,
    application_digest: String,
    preserves_target_application_digest: bool,
    verified_reopened: bool,
}

#[derive(Serialize)]
struct UpgradeCliReview {
    profile: &'static str,
    review: UpgradeReviewReport,
    plan: Value,
    executable_authority: bool,
    note: &'static str,
}

#[derive(Serialize)]
struct UpgradeCliResult {
    profile: &'static str,
    plan_digest: String,
    review_digest: String,
    output_leaf: String,
    output_bytes: u64,
    capsule_id: String,
    revision_id: String,
    app_id: String,
    app_version: String,
    application_digest: String,
    accepted_publisher_key_id: String,
    verified_reopened: bool,
    inputs_unchanged: bool,
}

fn main() -> ExitCode {
    match run(std::env::args_os().skip(1).collect()) {
        Ok(bytes) => match std::io::stdout().write_all(&bytes) {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => emit_error(WorkspaceError::new(WorkspaceErrorCode::InternalError)),
        },
        Err(error) => emit_error(error),
    }
}

fn run(arguments: Vec<OsString>) -> Result<Vec<u8>, WorkspaceError> {
    let command = arguments
        .first()
        .and_then(|value| value.to_str())
        .ok_or_else(invalid_contract)?;
    match command {
        "plan-duplicate" => plan_duplicate(&arguments),
        "compare" => compare(&arguments),
        "reconcile-candidates" => reconcile_candidates(&arguments),
        "three-way-reconcile-candidates" => three_way_reconcile_candidates(&arguments),
        "plan-reconcile" => reconcile_command(&arguments, false),
        "reconcile" => reconcile_command(&arguments, true),
        "plan-reconcile-three-way" => three_way_reconcile_command(&arguments, false),
        "reconcile-three-way" => three_way_reconcile_command(&arguments, true),
        "plan-upgrade" => upgrade_command(&arguments, false),
        "upgrade" => upgrade_command(&arguments, true),
        "copy-exact" => execute_copy(&arguments, CopyCommand::Exact),
        "copy-compact" => execute_copy(&arguments, CopyCommand::Compact),
        "copy-fork" => execute_copy(&arguments, CopyCommand::Semantic(SemanticCopyMode::Fork)),
        "copy-template" => execute_copy(
            &arguments,
            CopyCommand::Semantic(SemanticCopyMode::CreateFromTemplate),
        ),
        "copy-selective" => execute_copy(
            &arguments,
            CopyCommand::Semantic(SemanticCopyMode::SelectiveFork),
        ),
        _ => Err(invalid_contract()),
    }
}

fn upgrade_command(arguments: &[OsString], execute: bool) -> Result<Vec<u8>, WorkspaceError> {
    let valid_length = if execute {
        matches!(arguments.len(), 6 | 7)
    } else {
        arguments.len() == 5
    };
    if !valid_length {
        return Err(invalid_contract());
    }
    let source_path = PathBuf::from(&arguments[1]);
    let target_path = PathBuf::from(&arguments[2]);
    let output_path = PathBuf::from(&arguments[3]);
    let accepted_key_id = utf8_argument(&arguments[4])?.to_owned();
    if accepted_key_id.is_empty() || accepted_key_id.len() > 1_024 {
        return Err(invalid_contract());
    }
    let capability_changes_accepted = if execute {
        let expected_confirmation = format!("confirm-publisher-key={accepted_key_id}");
        if utf8_argument(&arguments[5])? != expected_confirmation {
            return Err(invalid_contract());
        }
        match arguments.get(6).map(utf8_argument).transpose()? {
            None => false,
            Some("confirm-capability-changes") => true,
            Some(_) => return Err(invalid_contract()),
        }
    } else {
        false
    };
    let limits = WorkspaceLimits::default();
    let cancellation = CancellationToken::new();
    let source = VerifiedWorkspaceSource::open_with_control(&source_path, &limits, &cancellation)?;
    let target = VerifiedWorkspaceSource::open_with_control(&target_path, &limits, &cancellation)?;
    let now = SystemTime::now();
    let plan_id = mint_uuid_v4()?;
    let created_at = utc_seconds(now)?;
    let expires_at = utc_seconds(
        now.checked_add(RECONCILE_EXPIRY)
            .ok_or_else(invalid_contract)?,
    )?;
    let review = prepare_upgrade_review(
        &source,
        &target,
        &UpgradePlanRequest {
            output_path: &output_path,
            plan_id: &plan_id,
            created_at: &created_at,
            expires_at: &expires_at,
            accepted_publisher_key_id: &accepted_key_id,
            max_output_bytes: limits.max_capsule_bytes,
            max_rows: 100_000,
            max_stream_bytes: 512 * 1024 * 1024,
            deadline: RECONCILE_DEADLINE,
        },
        &cancellation,
    )?;
    if !execute {
        let plan = serde_json::from_slice(&review.plan().canonical_bytes()?)
            .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InternalError))?;
        return serde_json::to_vec(&UpgradeCliReview {
            profile: UPGRADE_REVIEW_PROFILE,
            review: review.report().clone(),
            plan,
            executable_authority: false,
            note: "The report and canonical plan are review evidence only. Execution requires retained in-process inputs plus explicit publisher-key and capability-change confirmation.",
        })
        .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InternalError));
    }
    let approved_plan = parse_upgrade_plan(&review.plan().canonical_bytes()?)?;
    let plan_digest = approved_plan.plan_digest().to_owned();
    let published = review
        .prepare(
            approved_plan,
            &UpgradeApproval {
                accepted_publisher_key_id: accepted_key_id.clone(),
                capability_changes_accepted,
            },
            source,
            target,
            &limits,
            &cancellation,
        )?
        .stage()?
        .transform_and_validate()?
        .publish()?;
    let report = published.report();
    let output_leaf = published
        .path()
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(invalid_contract)?
        .to_owned();
    serde_json::to_vec(&UpgradeCliResult {
        profile: UPGRADE_RESULT_PROFILE,
        plan_digest,
        review_digest: report.review_digest.clone(),
        output_leaf,
        output_bytes: published.identity().bytes,
        capsule_id: report.output.capsule_id.clone(),
        revision_id: report.output.revision_id.clone(),
        app_id: report.output.app_id.clone(),
        app_version: report.output.app_version.clone(),
        application_digest: report.output.application_digest.clone(),
        accepted_publisher_key_id: report.publisher_continuity.accepted_key_id.clone(),
        verified_reopened: true,
        inputs_unchanged: true,
    })
    .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InternalError))
}

fn three_way_reconcile_candidates(arguments: &[OsString]) -> Result<Vec<u8>, WorkspaceError> {
    if arguments.len() != 5 {
        return Err(invalid_contract());
    }
    let sensitive_confirmation = parse_sensitive_confirmation(&arguments[4])?;
    let started = Instant::now();
    let cancellation = CancellationToken::new();
    let ancestor_path = PathBuf::from(&arguments[1]);
    let source_path = PathBuf::from(&arguments[2]);
    let target_path = PathBuf::from(&arguments[3]);
    let ancestor = VerifiedWorkspaceSource::open_with_control(
        &ancestor_path,
        &remaining_workspace_limits(started)?,
        &cancellation,
    )?;
    let source = VerifiedWorkspaceSource::open_with_control(
        &source_path,
        &remaining_workspace_limits(started)?,
        &cancellation,
    )?;
    let target = VerifiedWorkspaceSource::open_with_control(
        &target_path,
        &remaining_workspace_limits(started)?,
        &cancellation,
    )?;
    let summary = compare_sources(
        &source,
        &target,
        &CompareLimits {
            operation_deadline: Some(remaining_reconcile(started)?),
            ..CompareLimits::default()
        },
        &cancellation,
    )?;
    let confirmed_sensitive_dataset_indices =
        three_way_sensitive_confirmations(sensitive_confirmation, &source, &summary);
    let review = classify_three_way_reconcile(
        ancestor,
        source,
        target,
        &summary,
        &confirmed_sensitive_dataset_indices,
        &ReconcileReviewLimits {
            deadline: remaining_reconcile(started)?,
            ..ReconcileReviewLimits::default()
        },
        &cancellation,
    )?;
    serde_json::to_vec(&ThreeWayCandidates {
        profile: THREE_WAY_CANDIDATES_PROFILE,
        compare_report_digest: &summary.report_digest,
        clean_change_count: review.clean_change_count(),
        conflicts: review.conflicts().map(ThreeWayCandidateConflict::from).collect(),
        executable_authority: false,
        note: "Conflict and row digests are review data only. Resolution requires the retained in-process authority.",
    })
    .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InternalError))
}

fn reconcile_candidates(arguments: &[OsString]) -> Result<Vec<u8>, WorkspaceError> {
    if !matches!(arguments.len(), 5 | 6) {
        return Err(invalid_contract());
    }
    let source_path = PathBuf::from(&arguments[1]);
    let target_path = PathBuf::from(&arguments[2]);
    let dataset_index = parse_index(&arguments[3])?;
    let table_index = parse_index(&arguments[4])?;
    let reveal_sensitive = arguments
        .get(5)
        .map(|value| utf8_argument(value))
        .transpose()?
        .is_some_and(|value| value == "sensitive-confirmed");
    if arguments.len() == 6 && !reveal_sensitive {
        return Err(invalid_contract());
    }
    let started = Instant::now();
    let cancellation = CancellationToken::new();
    let source = VerifiedWorkspaceSource::open_with_control(
        &source_path,
        &remaining_workspace_limits(started)?,
        &cancellation,
    )?;
    let target = VerifiedWorkspaceSource::open_with_control(
        &target_path,
        &remaining_workspace_limits(started)?,
        &cancellation,
    )?;
    let summary = compare_sources(
        &source,
        &target,
        &CompareLimits {
            operation_deadline: Some(remaining_reconcile(started)?),
            ..CompareLimits::default()
        },
        &cancellation,
    )?;
    let page = comparison_detail_page(
        &source,
        &target,
        dataset_index,
        table_index,
        None,
        reveal_sensitive,
        &CompareDetailLimits {
            deadline: remaining_reconcile(started)?,
            page_size: 100,
            ..CompareDetailLimits::default()
        },
        &cancellation,
    )?;
    if page.next_cursor.is_some() {
        return Err(WorkspaceError::new(WorkspaceErrorCode::LimitExceeded));
    }
    let rows = page
        .rows
        .iter()
        .map(|row| ReconcileCandidateRow {
            kind: row.kind,
            key_digest: &row.key_digest,
            source_row_digest: row.left_digest.as_deref(),
            target_row_digest: row.right_digest.as_deref(),
            fields: row
                .fields
                .iter()
                .enumerate()
                .map(|(field_index, field)| ReconcileCandidateField {
                    field_index,
                    column: &field.column,
                    kind: field.kind,
                    source_value_digest: field.left.as_ref().map(|value| value.sha256.as_str()),
                    target_value_digest: field.right.as_ref().map(|value| value.sha256.as_str()),
                })
                .collect(),
        })
        .collect();
    serde_json::to_vec(&ReconcileCandidates {
        profile: RECONCILE_CANDIDATES_PROFILE,
        compare_report_digest: &summary.report_digest,
        dataset_index,
        table_index,
        dataset_label: &page.dataset_label,
        table_label: &page.table_label,
        rows,
        truncated: false,
        note: "Value displays are omitted. Candidate digests are review data, not execution authority.",
    })
    .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InternalError))
}

fn reconcile_command(arguments: &[OsString], execute: bool) -> Result<Vec<u8>, WorkspaceError> {
    if arguments.len() < 5 || arguments.len() > 5 + MAX_CLI_SELECTIONS {
        return Err(invalid_contract());
    }
    let source_path = PathBuf::from(&arguments[1]);
    let target_path = PathBuf::from(&arguments[2]);
    let output_path = PathBuf::from(&arguments[3]);
    let (sensitive_confirmation, selection_start) = match utf8_argument(&arguments[4])? {
        "normal" => (false, 5),
        "sensitive-confirmed" => (true, 5),
        _ => return Err(invalid_contract()),
    };
    let selections = parse_reconcile_selections(&arguments[selection_start..])?;
    let review = build_reconcile_review(
        &source_path,
        &target_path,
        &output_path,
        &selections,
        sensitive_confirmation,
    )?;
    if !execute {
        return serialize_reconcile_review(&review);
    }
    approve_and_execute(review)
}

fn three_way_reconcile_command(
    arguments: &[OsString],
    execute: bool,
) -> Result<Vec<u8>, WorkspaceError> {
    if arguments.len() < 6 || arguments.len() > 6 + MAX_CLI_SELECTIONS {
        return Err(invalid_contract());
    }
    let ancestor_path = PathBuf::from(&arguments[1]);
    let source_path = PathBuf::from(&arguments[2]);
    let target_path = PathBuf::from(&arguments[3]);
    let output_path = PathBuf::from(&arguments[4]);
    let sensitive_confirmation = parse_sensitive_confirmation(&arguments[5])?;
    let resolutions = parse_three_way_resolutions(&arguments[6..])?;
    let review = build_three_way_reconcile_review(
        &ancestor_path,
        &source_path,
        &target_path,
        &output_path,
        sensitive_confirmation,
        &resolutions,
    )?;
    if !execute {
        return serialize_reconcile_review(&review);
    }
    approve_and_execute(review)
}

fn build_three_way_reconcile_review(
    ancestor_path: &std::path::Path,
    source_path: &std::path::Path,
    target_path: &std::path::Path,
    output_path: &std::path::Path,
    sensitive_confirmation: bool,
    resolutions: &[ThreeWayConflictResolution],
) -> Result<ReconcileReview, WorkspaceError> {
    let started = Instant::now();
    let cancellation = CancellationToken::new();
    let ancestor = VerifiedWorkspaceSource::open_with_control(
        ancestor_path,
        &remaining_workspace_limits(started)?,
        &cancellation,
    )?;
    let source = VerifiedWorkspaceSource::open_with_control(
        source_path,
        &remaining_workspace_limits(started)?,
        &cancellation,
    )?;
    let target = VerifiedWorkspaceSource::open_with_control(
        target_path,
        &remaining_workspace_limits(started)?,
        &cancellation,
    )?;
    let summary = compare_sources(
        &source,
        &target,
        &CompareLimits {
            operation_deadline: Some(remaining_reconcile(started)?),
            ..CompareLimits::default()
        },
        &cancellation,
    )?;
    let confirmed_sensitive_dataset_indices =
        three_way_sensitive_confirmations(sensitive_confirmation, &source, &summary);
    let classified = classify_three_way_reconcile(
        ancestor,
        source,
        target,
        &summary,
        &confirmed_sensitive_dataset_indices,
        &ReconcileReviewLimits {
            deadline: remaining_reconcile(started)?,
            ..ReconcileReviewLimits::default()
        },
        &cancellation,
    )?;
    classified.resolve(
        resolutions,
        &reconcile_output_request(output_path)?,
        remaining_reconcile(started)?,
    )
}

fn build_reconcile_review(
    source_path: &std::path::Path,
    target_path: &std::path::Path,
    output_path: &std::path::Path,
    selections: &[ReconcileSelection],
    sensitive_confirmation: bool,
) -> Result<ReconcileReview, WorkspaceError> {
    let started = Instant::now();
    let cancellation = CancellationToken::new();
    let source = VerifiedWorkspaceSource::open_with_control(
        source_path,
        &remaining_workspace_limits(started)?,
        &cancellation,
    )?;
    let target = VerifiedWorkspaceSource::open_with_control(
        target_path,
        &remaining_workspace_limits(started)?,
        &cancellation,
    )?;
    let summary = compare_sources(
        &source,
        &target,
        &CompareLimits {
            operation_deadline: Some(remaining_reconcile(started)?),
            ..CompareLimits::default()
        },
        &cancellation,
    )?;
    let confirmed_sensitive_dataset_indices = if sensitive_confirmation {
        selections
            .iter()
            .filter_map(|selection| {
                source
                    .data_contract()
                    .datasets
                    .get(selection.dataset_index)
                    .filter(|dataset| dataset.sensitivity == Sensitivity::Sensitive)
                    .map(|_| selection.dataset_index)
            })
            .collect()
    } else {
        BTreeSet::new()
    };
    let request = reconcile_output_request(output_path)?;
    prepare_reconcile_review(
        source,
        target,
        &summary,
        selections,
        &confirmed_sensitive_dataset_indices,
        &request,
        &ReconcileReviewLimits {
            deadline: remaining_reconcile(started)?,
            ..ReconcileReviewLimits::default()
        },
        &cancellation,
    )
}

fn three_way_sensitive_confirmations(
    confirmed: bool,
    source: &VerifiedWorkspaceSource,
    summary: &CompareSummary,
) -> BTreeSet<usize> {
    if !confirmed {
        return BTreeSet::new();
    }
    source
        .data_contract()
        .datasets
        .iter()
        .zip(&summary.datasets)
        .enumerate()
        .filter_map(|(dataset_index, (dataset, compared))| {
            (dataset.sensitivity == Sensitivity::Sensitive
                && dataset.reconcile == ReconcilePolicy::ThreeWay
                && compared.state != CompareDataState::Same)
                .then_some(dataset_index)
        })
        .collect()
}

fn reconcile_output_request(
    output_path: &std::path::Path,
) -> Result<ReconcileOutputRequest, WorkspaceError> {
    let now = SystemTime::now();
    Ok(ReconcileOutputRequest {
        output_path: output_path.to_path_buf(),
        plan_id: mint_uuid_v4()?,
        created_at: utc_seconds(now)?,
        expires_at: utc_seconds(
            now.checked_add(RECONCILE_EXPIRY)
                .ok_or_else(invalid_contract)?,
        )?,
    })
}

fn serialize_reconcile_review(review: &ReconcileReview) -> Result<Vec<u8>, WorkspaceError> {
    let plan = serde_json::from_slice(&review.plan().canonical_bytes()?)
        .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InternalError))?;
    let payload = serde_json::from_slice(review.payload())
        .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InternalError))?;
    serde_json::to_vec(&ReconcileCliReview {
        profile: RECONCILE_REVIEW_PROFILE,
        compare_report_digest: review.compare_report_digest().to_owned(),
        review_digest: review.review_digest().to_owned(),
        operation_count: review.operation_count(),
        plan,
        payload,
        executable_authority: false,
        note: "This canonical plan and payload are review data only. The retained in-process capability is required for execution.",
    })
    .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InternalError))
}

fn approve_and_execute(review: ReconcileReview) -> Result<Vec<u8>, WorkspaceError> {
    let plan_bytes = review.plan().canonical_bytes()?;
    let approved_plan = LifecyclePlan::parse(&plan_bytes)?;
    let approved_payload = review.payload().to_vec();
    execute_approved_reconcile(review, approved_plan, &approved_payload)
}

fn execute_approved_reconcile(
    review: ReconcileReview,
    approved_plan: LifecyclePlan,
    approved_payload: &[u8],
) -> Result<Vec<u8>, WorkspaceError> {
    let operation_count = review.operation_count();
    let plan_digest = approved_plan.plan_digest().to_owned();
    let payload_digest = review.payload_digest().to_owned();
    let compare_report_digest = review.compare_report_digest().to_owned();
    let output = review.output().clone();
    let published = review
        .prepare(approved_plan, approved_payload)?
        .stage()?
        .transform_and_validate()?
        .publish()?;
    let output_leaf = published
        .path()
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(invalid_contract)?
        .to_owned();
    serde_json::to_vec(&ReconcileCliResult {
        profile: RECONCILE_RESULT_PROFILE,
        operation_count,
        plan_digest,
        payload_digest,
        compare_report_digest,
        output_leaf,
        output_bytes: published.identity().bytes,
        capsule_id: output.capsule_id,
        revision_id: output.revision_id,
        application_digest: output.application_digest,
        preserves_target_application_digest: output.preserves_target_application_digest,
        verified_reopened: true,
    })
    .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InternalError))
}

fn compare(arguments: &[OsString]) -> Result<Vec<u8>, WorkspaceError> {
    if arguments.len() != 3 {
        return Err(invalid_contract());
    }
    let started = Instant::now();
    let operation_deadline = Duration::from_secs(30);
    let cancellation = CancellationToken::new();
    let limits_with_remaining = || {
        operation_deadline
            .checked_sub(started.elapsed())
            .filter(|remaining| !remaining.is_zero())
            .map(|deadline| WorkspaceLimits {
                deadline,
                ..WorkspaceLimits::default()
            })
            .ok_or_else(|| WorkspaceError::new(WorkspaceErrorCode::LimitExceeded))
    };
    let left_path = PathBuf::from(&arguments[1]);
    let right_path = PathBuf::from(&arguments[2]);
    let left = VerifiedWorkspaceSource::open_with_control(
        &left_path,
        &limits_with_remaining()?,
        &cancellation,
    )?;
    let right = VerifiedWorkspaceSource::open_with_control(
        &right_path,
        &limits_with_remaining()?,
        &cancellation,
    )?;
    let remaining = operation_deadline
        .checked_sub(started.elapsed())
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| WorkspaceError::new(WorkspaceErrorCode::LimitExceeded))?;
    let summary = compare_sources(
        &left,
        &right,
        &CompareLimits {
            operation_deadline: Some(remaining),
            ..CompareLimits::default()
        },
        &cancellation,
    )?;
    serde_json::to_vec(&summary).map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InternalError))
}

fn plan_duplicate(arguments: &[OsString]) -> Result<Vec<u8>, WorkspaceError> {
    let CommonArguments {
        source,
        output,
        plan_id,
        created_at,
        expires_at,
    } = common_arguments(arguments, false)?;
    let source = VerifiedWorkspaceSource::open(&source)?;
    let plan = generate_duplicate_plan(
        &source,
        &DuplicatePlanRequest {
            output_path: &output,
            plan_id: &plan_id,
            created_at: &created_at,
            expires_at: &expires_at,
            limits: DuplicatePlanLimits::default(),
        },
    )?;
    plan.canonical_bytes()
}

enum CopyCommand {
    Exact,
    Compact,
    Semantic(SemanticCopyMode),
}

fn execute_copy(arguments: &[OsString], command: CopyCommand) -> Result<Vec<u8>, WorkspaceError> {
    let semantic = matches!(command, CopyCommand::Semantic(_));
    let CommonArguments {
        source,
        output,
        plan_id,
        created_at,
        expires_at,
    } = common_arguments(arguments, semantic)?;
    let limits = WorkspaceLimits::default();
    let cancellation = CancellationToken::new();
    let (mode, output_bytes) = match command {
        CopyCommand::Exact => {
            let source = VerifiedCopySource::open_with_control(&source, &limits, &cancellation)?;
            let review = generate_exact_copy_plan(
                &source,
                &ExactCopyPlanRequest {
                    output_path: &output,
                    plan_id: &plan_id,
                    created_at: &created_at,
                    expires_at: &expires_at,
                    deadline: limits.deadline,
                    max_output_bytes: limits.max_capsule_bytes,
                },
            )?;
            let approved = parse_exact_copy_plan(&review.plan().canonical_bytes()?)?;
            let published = review
                .prepare(approved, source, &limits, &cancellation)?
                .stage()?
                .copy_and_validate()?
                .publish()?;
            ("exact-duplicate", published.identity().bytes)
        }
        CopyCommand::Compact => {
            let source = VerifiedCompactSource::open_with_control(&source, &limits, &cancellation)?;
            let review = generate_compact_copy_plan(
                &source,
                &CompactCopyPlanRequest {
                    output_path: &output,
                    plan_id: &plan_id,
                    created_at: &created_at,
                    expires_at: &expires_at,
                    deadline: limits.deadline,
                    max_output_bytes: limits.max_capsule_bytes,
                },
            )?;
            let approved = parse_compact_copy_plan(&review.plan().canonical_bytes()?)?;
            let published = review
                .prepare(approved, source, &limits, &cancellation)?
                .stage()?
                .compact_and_validate()?
                .publish()?;
            ("compact-duplicate", published.identity().bytes)
        }
        CopyCommand::Semantic(mode) => {
            let choices = semantic_choices(&arguments[6..])?;
            let source = open_semantic_copy_source(&source, &limits, &cancellation)?;
            let review = generate_semantic_copy_plan(
                &source,
                &SemanticCopyPlanRequest {
                    output_path: &output,
                    plan_id: &plan_id,
                    created_at: &created_at,
                    expires_at: &expires_at,
                    mode,
                    choices: &choices,
                    deadline: limits.deadline,
                    max_output_bytes: limits.max_capsule_bytes,
                    max_rows: 100_000,
                    max_stream_bytes: 512 * 1024 * 1024,
                },
                &cancellation,
            )?;
            let approved = parse_semantic_copy_plan(&review.plan().canonical_bytes()?)?;
            let published = review
                .prepare(approved, source, &limits, &cancellation)?
                .stage()?
                .transform_and_validate()?
                .publish()?;
            let name = match mode {
                SemanticCopyMode::Fork => "fork-with-data",
                SemanticCopyMode::CreateFromTemplate => "create-from-template",
                SemanticCopyMode::SelectiveFork => "selective-fork",
            };
            (name, published.identity().bytes)
        }
    };
    let leaf = output
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(invalid_contract)?;
    serde_json::to_vec(&CopyResult {
        profile: RESULT_PROFILE,
        mode,
        output_leaf: leaf,
        output_bytes,
    })
    .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InternalError))
}

struct CommonArguments {
    source: PathBuf,
    output: PathBuf,
    plan_id: String,
    created_at: String,
    expires_at: String,
}

fn common_arguments(
    arguments: &[OsString],
    allow_trailing: bool,
) -> Result<CommonArguments, WorkspaceError> {
    if arguments.len() < 6 || (!allow_trailing && arguments.len() != 6) {
        return Err(invalid_contract());
    }
    Ok(CommonArguments {
        source: PathBuf::from(&arguments[1]),
        output: PathBuf::from(&arguments[2]),
        plan_id: utf8_argument(&arguments[3])?.to_owned(),
        created_at: utf8_argument(&arguments[4])?.to_owned(),
        expires_at: utf8_argument(&arguments[5])?.to_owned(),
    })
}

fn semantic_choices(arguments: &[OsString]) -> Result<Vec<SemanticDatasetChoice>, WorkspaceError> {
    if arguments.len() > 256 {
        return Err(invalid_contract());
    }
    arguments
        .iter()
        .map(|argument| {
            let value = utf8_argument(argument)?;
            let (dataset_id, action) = value.split_once('=').ok_or_else(invalid_contract)?;
            if dataset_id.is_empty() {
                return Err(invalid_contract());
            }
            let (disposition, sensitive_confirmed) = match action {
                "include" => (SemanticChoiceDisposition::Copy, false),
                "include-confirmed" => (SemanticChoiceDisposition::Copy, true),
                "omit" => (SemanticChoiceDisposition::Omit, false),
                _ => return Err(invalid_contract()),
            };
            Ok(SemanticDatasetChoice {
                dataset_id: dataset_id.to_owned(),
                disposition,
                sensitive_confirmed,
            })
        })
        .collect()
}

fn parse_reconcile_selections(
    arguments: &[OsString],
) -> Result<Vec<ReconcileSelection>, WorkspaceError> {
    if arguments.is_empty() || arguments.len() > MAX_CLI_SELECTIONS {
        return Err(invalid_contract());
    }
    arguments
        .iter()
        .map(|argument| parse_reconcile_selection(utf8_argument(argument)?))
        .collect()
}

fn parse_sensitive_confirmation(value: &OsString) -> Result<bool, WorkspaceError> {
    match utf8_argument(value)? {
        "normal" => Ok(false),
        "sensitive-confirmed" => Ok(true),
        _ => Err(invalid_contract()),
    }
}

fn parse_three_way_resolutions(
    arguments: &[OsString],
) -> Result<Vec<ThreeWayConflictResolution>, WorkspaceError> {
    if arguments.len() > MAX_CLI_SELECTIONS {
        return Err(invalid_contract());
    }
    arguments
        .iter()
        .map(|argument| {
            let value = utf8_argument(argument)?;
            if value.len() != 76 {
                return Err(invalid_contract());
            }
            let (conflict_id, choice) = value.split_once(':').ok_or_else(invalid_contract)?;
            let choice = match choice {
                "keep-target" => ThreeWayResolutionChoice::KeepTarget,
                "take-source" => ThreeWayResolutionChoice::TakeSource,
                _ => return Err(invalid_contract()),
            };
            Ok(ThreeWayConflictResolution {
                conflict_id: parse_digest(conflict_id)?,
                choice,
            })
        })
        .collect()
}

/// Parses a value-free selection:
/// dataset:table:key-sha:source-row-sha-or--:target-row-sha-or--:action:fields-or--
fn parse_reconcile_selection(value: &str) -> Result<ReconcileSelection, WorkspaceError> {
    if value.len() > 1024 {
        return Err(invalid_contract());
    }
    let parts = value.split(':').collect::<Vec<_>>();
    if parts.len() != 7 {
        return Err(invalid_contract());
    }
    let action = match parts[5] {
        "insert" => ReconcileAction::InsertFromSource,
        "delete" => ReconcileAction::DeleteFromTarget,
        "replace" => ReconcileAction::ReplaceRowFromSource,
        "fields" => ReconcileAction::SetFields,
        _ => return Err(invalid_contract()),
    };
    let selection = ReconcileSelection {
        dataset_index: parse_usize(parts[0])?,
        table_index: parse_usize(parts[1])?,
        key_digest: parse_digest(parts[2])?,
        source_row_digest: parse_optional_digest(parts[3])?,
        target_row_digest: parse_optional_digest(parts[4])?,
        action,
        field_indices: parse_field_indices(parts[6])?,
    };
    let shape_is_valid = match action {
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
        }
    };
    if !shape_is_valid {
        return Err(invalid_contract());
    }
    Ok(selection)
}

fn parse_index(value: &OsString) -> Result<usize, WorkspaceError> {
    parse_usize(utf8_argument(value)?)
}

fn parse_usize(value: &str) -> Result<usize, WorkspaceError> {
    if value.is_empty()
        || value.len() > 20
        || !value.bytes().all(|byte| byte.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return Err(invalid_contract());
    }
    value.parse().map_err(|_| invalid_contract())
}

fn parse_digest(value: &str) -> Result<String, WorkspaceError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(invalid_contract());
    }
    Ok(value.to_owned())
}

fn parse_optional_digest(value: &str) -> Result<Option<String>, WorkspaceError> {
    if value == "-" {
        Ok(None)
    } else {
        parse_digest(value).map(Some)
    }
}

fn parse_field_indices(value: &str) -> Result<Vec<usize>, WorkspaceError> {
    if value == "-" {
        return Ok(Vec::new());
    }
    let fields = value
        .split(',')
        .map(parse_usize)
        .collect::<Result<Vec<_>, _>>()?;
    if fields.is_empty() || fields.len() > 256 || fields.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(invalid_contract());
    }
    Ok(fields)
}

fn remaining_reconcile(started: Instant) -> Result<Duration, WorkspaceError> {
    RECONCILE_DEADLINE
        .checked_sub(started.elapsed())
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| WorkspaceError::new(WorkspaceErrorCode::LimitExceeded))
}

fn remaining_workspace_limits(started: Instant) -> Result<WorkspaceLimits, WorkspaceError> {
    Ok(WorkspaceLimits {
        deadline: remaining_reconcile(started)?,
        ..WorkspaceLimits::default()
    })
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

fn utc_seconds(time: SystemTime) -> Result<String, WorkspaceError> {
    let seconds = time
        .duration_since(UNIX_EPOCH)
        .map_err(|_| invalid_contract())?
        .as_secs();
    let days = i64::try_from(seconds / 86_400).map_err(|_| invalid_contract())?;
    let seconds_in_day = seconds % 86_400;
    let (year, month, day) = civil_from_days(days)?;
    let hour = seconds_in_day / 3_600;
    let minute = (seconds_in_day % 3_600) / 60;
    let second = seconds_in_day % 60;
    Ok(format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z"
    ))
}

fn civil_from_days(days_since_epoch: i64) -> Result<(i64, i64, i64), WorkspaceError> {
    let z = days_since_epoch
        .checked_add(719_468)
        .ok_or_else(invalid_contract)?;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    if !(1..=9999).contains(&year) {
        return Err(invalid_contract());
    }
    Ok((year, month, day))
}

fn utf8_argument(value: &OsString) -> Result<&str, WorkspaceError> {
    value.to_str().ok_or_else(invalid_contract)
}

const fn invalid_contract() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::InvalidContract)
}

fn emit_error(error: WorkspaceError) -> ExitCode {
    let _ = serde_json::to_writer(std::io::stderr(), &error);
    ExitCode::from(2)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicUsize, Ordering},
    };

    use ed25519_dalek::SigningKey;
    use rusqlite::{Connection, params};
    use sqlite_capsule_crypto::{PROFILE_V03, application_digest, sign_digest_for_profile};

    use super::*;

    const DEVELOPMENT_SEED: &str =
        include_str!("../../../../../compatibility/signed-app-v0.2/development-seed.hex");
    static TEMP_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(name: &str) -> Self {
            let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "sqlite-capsule-workspace-cli-{name}-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn selection_parser_accepts_only_value_free_closed_shapes() {
        let digest = "aa".repeat(32);
        let insert = format!("0:1:{digest}:{digest}:-:insert:-");
        let parsed = parse_reconcile_selection(&insert).unwrap();
        assert_eq!(parsed.action, ReconcileAction::InsertFromSource);
        assert!(parsed.target_row_digest.is_none());

        let fields = format!("0:1:{digest}:{digest}:{digest}:fields:1,3");
        assert_eq!(
            parse_reconcile_selection(&fields).unwrap().field_indices,
            vec![1, 3]
        );
        for hostile in [
            format!("0:1:{digest}:{digest}:-:insert:0"),
            format!("0:1:{digest}:-:-:insert:-"),
            format!("0:1:{digest}:{digest}:{digest}:fields:3,1"),
            format!("0:1:{digest}:{digest}:{digest}:sql:-"),
            format!("0:1:{digest}:{digest}:{digest}:fields:1,1"),
        ] {
            assert!(parse_reconcile_selection(&hostile).is_err(), "{hostile}");
        }
        assert!(parse_reconcile_selection(&"x".repeat(1025)).is_err());

        let resolution = format!("{digest}:take-source");
        assert_eq!(
            parse_three_way_resolutions(&[OsString::from(&resolution)])
                .unwrap()
                .as_slice(),
            &[ThreeWayConflictResolution {
                conflict_id: digest.clone(),
                choice: ThreeWayResolutionChoice::TakeSource,
            }]
        );
        for hostile in [
            format!("{digest}:automatic"),
            format!("{}:take-source", "A".repeat(64)),
            format!("{}:keep-target:extra", digest),
        ] {
            assert!(parse_three_way_resolutions(&[OsString::from(hostile)]).is_err());
        }
    }

    #[test]
    fn host_time_and_ids_have_frozen_lifecycle_shapes() {
        assert_eq!(utc_seconds(UNIX_EPOCH).unwrap(), "1970-01-01T00:00:00Z");
        assert_eq!(
            utc_seconds(UNIX_EPOCH + Duration::from_secs(951_782_400)).unwrap(),
            "2000-02-29T00:00:00Z"
        );
        let id = mint_uuid_v4().unwrap();
        assert_eq!(id.len(), 36);
        assert_eq!(&id[14..15], "4");
        assert!(matches!(&id[19..20], "8" | "9" | "a" | "b"));
    }

    #[test]
    fn upgrade_cli_requires_exact_closed_confirmations_before_path_access() {
        let base = [
            OsString::from("upgrade"),
            OsString::from("missing-source.sqlitecapsule"),
            OsString::from("missing-target.sqlitecapsule"),
            OsString::from("new-output.sqlitecapsule"),
            OsString::from("ed25519:key-id"),
        ];
        assert_eq!(
            upgrade_command(&base, true).unwrap_err().kind(),
            WorkspaceErrorCode::InvalidContract
        );
        for confirmation in [
            "confirm-publisher-key=wrong",
            "confirm-publisher-key=ed25519:key-id:extra",
        ] {
            let mut arguments = base.to_vec();
            arguments.push(OsString::from(confirmation));
            assert_eq!(
                upgrade_command(&arguments, true).unwrap_err().kind(),
                WorkspaceErrorCode::InvalidContract
            );
        }
        let mut arguments = base.to_vec();
        arguments.push(OsString::from("confirm-publisher-key=ed25519:key-id"));
        arguments.push(OsString::from("confirm-everything"));
        assert_eq!(
            upgrade_command(&arguments, true).unwrap_err().kind(),
            WorkspaceErrorCode::InvalidContract
        );
    }

    #[test]
    fn cli_executes_all_four_actions_through_one_use_typestate() {
        let directory = TestDirectory::new("four-actions");
        let (source_path, target_path) = reconcile_pair(&directory);
        let output_path = directory.path().join("reconciled.sqlitecapsule");
        let source_before = fs::read(&source_path).unwrap();
        let target_before = fs::read(&target_path).unwrap();
        let selections = selections_for_pair(&source_path, &target_path);
        let review =
            build_reconcile_review(&source_path, &target_path, &output_path, &selections, false)
                .unwrap();
        let target_application_digest = review.target().application_digest.clone();
        let result: Value = serde_json::from_slice(&approve_and_execute(review).unwrap()).unwrap();
        assert_eq!(result["profile"], RECONCILE_RESULT_PROFILE);
        assert_eq!(result["operation_count"], 4);
        assert_eq!(result["verified_reopened"], true);
        assert_eq!(result["preserves_target_application_digest"], true);
        assert_eq!(result["application_digest"], target_application_digest);
        assert_eq!(fs::read(&source_path).unwrap(), source_before);
        assert_eq!(fs::read(&target_path).unwrap(), target_before);
        let reopened = VerifiedWorkspaceSource::open(&output_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&target_path).unwrap();
        let comparison = compare_sources(
            &target,
            &reopened,
            &CompareLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        assert_eq!(
            comparison.left.application_digest,
            target_application_digest
        );
        assert_eq!(
            comparison.right.application_digest,
            target_application_digest
        );

        let connection = Connection::open(&output_path).unwrap();
        let rows = connection
            .prepare("SELECT id,note FROM vector_domain ORDER BY id")
            .unwrap()
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(rows.contains(&("insert".to_owned(), "source insert".to_owned())));
        assert!(!rows.iter().any(|(id, _)| id == "delete"));
        assert!(rows.contains(&("replace".to_owned(), "source replace".to_owned())));
        assert!(rows.contains(&("fields".to_owned(), "source fields".to_owned())));
    }

    #[test]
    fn candidates_are_bounded_value_free_review_data() {
        let directory = TestDirectory::new("candidates");
        let (source_path, target_path) = reconcile_pair(&directory);
        let bytes = reconcile_candidates(&[
            OsString::from("reconcile-candidates"),
            source_path.clone().into_os_string(),
            target_path.clone().into_os_string(),
            OsString::from("0"),
            OsString::from("0"),
        ])
        .unwrap();
        assert!(bytes.len() < 4 * 1024 * 1024);
        let response: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(response["profile"], RECONCILE_CANDIDATES_PROFILE);
        assert_eq!(response["truncated"], false);
        let text = String::from_utf8(bytes).unwrap();
        assert!(!text.contains("source insert"));
        assert!(!text.contains("target delete"));
        assert!(!text.contains(source_path.to_string_lossy().as_ref()));
        assert!(!text.contains(target_path.to_string_lossy().as_ref()));
    }

    #[test]
    fn review_json_is_non_authoritative_and_edited_approval_fails() {
        let directory = TestDirectory::new("review-only");
        let (source_path, target_path) = reconcile_pair(&directory);
        let output_path = directory.path().join("reviewed.sqlitecapsule");
        let selections = selections_for_pair(&source_path, &target_path);
        let review =
            build_reconcile_review(&source_path, &target_path, &output_path, &selections, false)
                .unwrap();
        let serialized: Value =
            serde_json::from_slice(&serialize_reconcile_review(&review).unwrap()).unwrap();
        assert_eq!(serialized["executable_authority"], false);
        assert!(serialized["payload"].get("operations").is_some());
        assert!(serialized["payload"].to_string().len() < 16 * 1024 * 1024);

        let plan = LifecyclePlan::parse(&review.plan().canonical_bytes().unwrap()).unwrap();
        let mut edited_payload = review.payload().to_vec();
        edited_payload.push(b' ');
        let error = review.prepare(plan, &edited_payload).unwrap_err();
        assert_eq!(error.kind(), WorkspaceErrorCode::StalePlan);
        assert!(!output_path.exists());
    }

    #[test]
    fn stale_input_and_existing_destination_fail_closed() {
        let directory = TestDirectory::new("stale-existing");
        let (source_path, target_path) = reconcile_pair(&directory);
        let selections = selections_for_pair(&source_path, &target_path);
        let stale_output = directory.path().join("stale.sqlitecapsule");
        let review = build_reconcile_review(
            &source_path,
            &target_path,
            &stale_output,
            &selections,
            false,
        )
        .unwrap();
        Connection::open(&source_path)
            .unwrap()
            .execute(
                "UPDATE vector_domain SET note='changed after review' WHERE id='insert'",
                [],
            )
            .unwrap();
        let plan = LifecyclePlan::parse(&review.plan().canonical_bytes().unwrap()).unwrap();
        let payload = review.payload().to_vec();
        let error = review.prepare(plan, &payload).unwrap_err();
        assert_eq!(error.kind(), WorkspaceErrorCode::StalePlan);
        assert!(!stale_output.exists());

        let existing_directory = TestDirectory::new("existing-destination");
        let (source_path, target_path) = reconcile_pair(&existing_directory);
        let selections = selections_for_pair(&source_path, &target_path);
        let existing = existing_directory.path().join("existing.sqlitecapsule");
        fs::write(&existing, b"preserve me").unwrap();
        let error =
            build_reconcile_review(&source_path, &target_path, &existing, &selections, false)
                .unwrap_err();
        assert!(
            matches!(
                error.kind(),
                WorkspaceErrorCode::DestinationExists | WorkspaceErrorCode::InvalidContract
            ),
            "unexpected error: {:?}",
            error.kind()
        );
        assert_eq!(fs::read(&existing).unwrap(), b"preserve me");
    }

    #[test]
    fn three_way_cli_classifies_resolves_and_executes_with_value_free_json() {
        let directory = TestDirectory::new("three-way");
        let (ancestor_path, source_path, target_path) = three_way_reconcile_triplet(&directory);
        let output_path = directory.path().join("three-way-output.sqlitecapsule");
        let ancestor_before = fs::read(&ancestor_path).unwrap();
        let source_before = fs::read(&source_path).unwrap();
        let target_before = fs::read(&target_path).unwrap();

        let candidate_bytes = three_way_reconcile_candidates(&[
            OsString::from("three-way-reconcile-candidates"),
            ancestor_path.clone().into_os_string(),
            source_path.clone().into_os_string(),
            target_path.clone().into_os_string(),
            OsString::from("normal"),
        ])
        .unwrap();
        let candidates: Value = serde_json::from_slice(&candidate_bytes).unwrap();
        assert_eq!(candidates["profile"], THREE_WAY_CANDIDATES_PROFILE);
        assert_eq!(candidates["clean_change_count"], 1);
        assert_eq!(candidates["conflicts"].as_array().unwrap().len(), 1);
        assert_eq!(candidates["conflicts"][0]["kind"], "update-update");
        assert_eq!(candidates["executable_authority"], false);
        let candidate_text = String::from_utf8(candidate_bytes).unwrap();
        assert!(!candidate_text.contains("source conflict"));
        assert!(!candidate_text.contains("target conflict"));
        assert!(!candidate_text.contains(ancestor_path.to_string_lossy().as_ref()));

        let conflict_id = candidates["conflicts"][0]["id"].as_str().unwrap();
        let resolutions =
            parse_three_way_resolutions(&[OsString::from(format!("{conflict_id}:take-source"))])
                .unwrap();
        let review = build_three_way_reconcile_review(
            &ancestor_path,
            &source_path,
            &target_path,
            &output_path,
            false,
            &resolutions,
        )
        .unwrap();
        let review_json: Value =
            serde_json::from_slice(&serialize_reconcile_review(&review).unwrap()).unwrap();
        assert_eq!(review_json["payload"]["mode"], "three-way");
        assert_eq!(
            review_json["payload"]["resolved_conflicts"][0]["resolution"],
            "take-source"
        );
        assert!(
            review_json["payload"]["signature_inventories"]
                .get("ancestor")
                .is_some()
        );
        let target_digest = review.target().application_digest.clone();
        let result: Value = serde_json::from_slice(&approve_and_execute(review).unwrap()).unwrap();
        assert_eq!(result["profile"], RECONCILE_RESULT_PROFILE);
        assert_eq!(result["application_digest"], target_digest);
        assert_eq!(result["preserves_target_application_digest"], true);

        let connection = Connection::open(&output_path).unwrap();
        let note: String = connection
            .query_row(
                "SELECT note FROM vector_domain WHERE id='shared'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(note, "source conflict");
        assert_eq!(
            connection
                .query_row(
                    "SELECT count(*) FROM vector_domain WHERE id='source-clean'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
        assert_eq!(fs::read(&ancestor_path).unwrap(), ancestor_before);
        assert_eq!(fs::read(&source_path).unwrap(), source_before);
        assert_eq!(fs::read(&target_path).unwrap(), target_before);
    }

    #[test]
    fn three_way_cli_confirmation_set_includes_only_changed_sensitive_datasets() {
        let directory = TestDirectory::new("three-way-sensitive-scope");
        let ancestor_path = signed_fixture(&directory, "sensitive-ancestor.sqlitecapsule");
        let connection = Connection::open(&ancestor_path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE vector_archive (
                     id TEXT PRIMARY KEY NOT NULL,
                     note TEXT NOT NULL
                 );
                 INSERT INTO vector_archive VALUES ('archive','base');
                 INSERT INTO capsule_dataset VALUES
                     ('archive','history','Archive.','copy','row','three-way','copy','normal',0);
                 INSERT INTO capsule_dataset_table VALUES
                     ('archive','vector_archive',0,'[\"id\"]','[]','[\"id\"]');
                 UPDATE capsule_dataset SET reconcile_policy='three-way';
                 UPDATE capsule_dataset SET sensitivity='sensitive'
                 WHERE id IN ('content','settings');
                 UPDATE capsule_dataset_table SET ignored_columns_json='[\"payload\"]'
                 WHERE table_name='vector_domain';",
            )
            .unwrap();
        resign(&connection);
        drop(connection);
        let source_path = directory.path().join("sensitive-source.sqlitecapsule");
        let target_path = directory.path().join("sensitive-target.sqlitecapsule");
        fs::copy(&ancestor_path, &source_path).unwrap();
        fs::copy(&ancestor_path, &target_path).unwrap();
        Connection::open(&source_path)
            .unwrap()
            .execute_batch(
                "UPDATE vector_domain SET note='changed sensitive';
                 UPDATE vector_archive SET note='changed normal';",
            )
            .unwrap();

        let source = VerifiedWorkspaceSource::open(&source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&target_path).unwrap();
        let summary = compare_sources(
            &source,
            &target,
            &CompareLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        let confirmed = three_way_sensitive_confirmations(true, &source, &summary);
        let confirmed_ids = confirmed
            .iter()
            .map(|index| source.data_contract().datasets[*index].id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(confirmed_ids, vec!["content"]);
        assert!(
            !confirmed
                .iter()
                .any(|index| { source.data_contract().datasets[*index].id == "settings" })
        );
        assert!(
            !confirmed
                .iter()
                .any(|index| { source.data_contract().datasets[*index].id == "archive" })
        );

        let output_path = directory.path().join("sensitive-output.sqlitecapsule");
        let review = build_three_way_reconcile_review(
            &ancestor_path,
            &source_path,
            &target_path,
            &output_path,
            true,
            &[],
        )
        .unwrap();
        let payload: Value = serde_json::from_slice(review.payload()).unwrap();
        assert_eq!(
            payload["sensitive_confirmation"]["confirmed_dataset_ids"],
            serde_json::json!(["content"])
        );
        assert_eq!(review.operation_count(), 2);
    }

    fn reconcile_pair(directory: &TestDirectory) -> (PathBuf, PathBuf) {
        let source = signed_fixture(directory, "source.sqlitecapsule");
        let target = signed_fixture(directory, "target.sqlitecapsule");
        make_reconcilable(&source);
        make_reconcilable(&target);
        Connection::open(&source)
            .unwrap()
            .execute_batch(
                "INSERT INTO vector_domain VALUES ('insert','source insert',1.0,X'01');
                 INSERT INTO vector_domain VALUES ('replace','source replace',2.0,X'02');
                 INSERT INTO vector_domain VALUES ('fields','source fields',3.0,X'03');",
            )
            .unwrap();
        Connection::open(&target)
            .unwrap()
            .execute_batch(
                "INSERT INTO vector_domain VALUES ('delete','target delete',4.0,X'04');
                 INSERT INTO vector_domain VALUES ('replace','target replace',2.0,X'02');
                 INSERT INTO vector_domain VALUES ('fields','target fields',3.0,X'03');",
            )
            .unwrap();
        (source, target)
    }

    fn three_way_reconcile_triplet(directory: &TestDirectory) -> (PathBuf, PathBuf, PathBuf) {
        let ancestor = signed_fixture(directory, "ancestor.sqlitecapsule");
        make_three_way_reconcilable(&ancestor);
        Connection::open(&ancestor)
            .unwrap()
            .execute(
                "INSERT INTO vector_domain VALUES ('shared','base',1.0,X'01')",
                [],
            )
            .unwrap();
        let source = directory.path().join("three-way-source.sqlitecapsule");
        let target = directory.path().join("three-way-target.sqlitecapsule");
        fs::copy(&ancestor, &source).unwrap();
        fs::copy(&ancestor, &target).unwrap();
        Connection::open(&source)
            .unwrap()
            .execute_batch(
                "UPDATE vector_domain SET note='source conflict' WHERE id='shared';
                 INSERT INTO vector_domain VALUES ('source-clean','source only',2.0,X'02');",
            )
            .unwrap();
        Connection::open(&target)
            .unwrap()
            .execute(
                "UPDATE vector_domain SET note='target conflict' WHERE id='shared'",
                [],
            )
            .unwrap();
        (ancestor, source, target)
    }

    fn selections_for_pair(source_path: &Path, target_path: &Path) -> Vec<ReconcileSelection> {
        let source = VerifiedWorkspaceSource::open(source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(target_path).unwrap();
        let page = comparison_detail_page(
            &source,
            &target,
            0,
            0,
            None,
            false,
            &CompareDetailLimits {
                page_size: 100,
                ..CompareDetailLimits::default()
            },
            &CancellationToken::new(),
        )
        .unwrap();
        assert!(page.next_cursor.is_none());
        let inserted = page
            .rows
            .iter()
            .find(|row| row.kind == CompareDetailRowKind::Removed)
            .unwrap();
        let deleted = page
            .rows
            .iter()
            .find(|row| row.kind == CompareDetailRowKind::Added)
            .unwrap();
        let changed = page
            .rows
            .iter()
            .filter(|row| row.kind == CompareDetailRowKind::Changed)
            .collect::<Vec<_>>();
        assert_eq!(changed.len(), 2);
        let changed_field_index = changed[1]
            .fields
            .iter()
            .position(|field| {
                field.kind == sqlite_capsule_workspace::CompareDetailFieldKind::Changed
            })
            .unwrap();
        vec![
            selection(inserted, ReconcileAction::InsertFromSource, Vec::new()),
            selection(deleted, ReconcileAction::DeleteFromTarget, Vec::new()),
            selection(
                changed[0],
                ReconcileAction::ReplaceRowFromSource,
                Vec::new(),
            ),
            selection(
                changed[1],
                ReconcileAction::SetFields,
                vec![changed_field_index],
            ),
        ]
    }

    fn selection(
        row: &sqlite_capsule_workspace::CompareRowDetail,
        action: ReconcileAction,
        field_indices: Vec<usize>,
    ) -> ReconcileSelection {
        ReconcileSelection {
            dataset_index: 0,
            table_index: 0,
            key_digest: row.key_digest.clone(),
            source_row_digest: row.left_digest.clone(),
            target_row_digest: row.right_digest.clone(),
            action,
            field_indices,
        }
    }

    fn signed_fixture(directory: &TestDirectory, leaf: &str) -> PathBuf {
        let path = directory.path().join(leaf);
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(include_str!("../../../../../format/capsule-v0.3.sql"))
            .unwrap();
        connection
            .execute_batch(include_str!(
                "../../../../../format/capsule-signed-app-v0.3.sql"
            ))
            .unwrap();
        connection
            .execute_batch(include_str!(
                "../../../../../compatibility/signed-app-v0.3/fixture-v0.3.sql"
            ))
            .unwrap();
        drop(connection);
        path
    }

    fn make_reconcilable(path: &Path) {
        let connection = Connection::open(path).unwrap();
        connection
            .execute(
                "UPDATE capsule_dataset SET reconcile_policy='manual' WHERE id='content'",
                [],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE capsule_dataset_table SET ignored_columns_json='[\"payload\"]' WHERE table_name='vector_domain'",
                [],
            )
            .unwrap();
        resign(&connection);
    }

    fn make_three_way_reconcilable(path: &Path) {
        let connection = Connection::open(path).unwrap();
        connection
            .execute(
                "UPDATE capsule_dataset SET reconcile_policy='three-way' WHERE id='content'",
                [],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE capsule_dataset_table SET ignored_columns_json='[\"payload\"]' WHERE table_name='vector_domain'",
                [],
            )
            .unwrap();
        resign(&connection);
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
                params![
                    envelope.key_id,
                    envelope.public_key.as_slice(),
                    envelope.application_digest.as_slice(),
                    envelope.signature.as_slice(),
                    envelope.signed_at
                ],
            )
            .unwrap();
    }
}

//! Executable semantic fork, template and selective-fork profiles.
//!
//! Serialized plans are review data only. Execution authority is retained in
//! the non-serializable typestates below, which hold the verified v0.3 source,
//! the exact canonical plan and a one-use held-parent destination reservation.
//! Every dataset action is derived again from the signed contract and the
//! closed host choices at preparation, private transformation and publication.

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::atomic::Ordering,
    time::{Duration, Instant, SystemTime},
};

use rusqlite::{Connection, OpenFlags, params, types::ValueRef};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlite_capsule_lifecycle::{
    DestinationReservation, PrivateOutput, PublishedOutput, SealedPrivateOutput, SourceIdentity,
};

use crate::{
    CancellationToken, DecisionScope, ForkPolicy, InputRole, LifecyclePlan, Operation, Sensitivity,
    TemplateStateLimits, TemplateStateProof, VerifiedCopySource, VerifiedWorkspaceSource,
    WorkspaceError, WorkspaceErrorCode, WorkspaceLimits,
    plan::canonical_digest_value,
    prepared_plan::{map_destination_error, map_prepared_destination_error, validate_time_window},
    verify_template_state,
};

const SEMANTIC_ACTION: &str = "semantic-copy-v1";
const MUTABLE_PLATFORM_PROFILE: &str = "org.sqlite-capsule.semantic-mutable-reset/1";
const FORK_INSTANCE_PROFILE: &str =
    "org.sqlite-capsule.semantic-fork-instance-preserve-text-clear-assets/1";
const TEMPLATE_INSTANCE_PROFILE: &str =
    "org.sqlite-capsule.semantic-template-instance-signed-app-defaults/1";
const SELECTIVE_INSTANCE_PROFILE: &str =
    "org.sqlite-capsule.semantic-selective-instance-signed-app-defaults/1";
const HARD_DEADLINE: Duration = Duration::from_secs(30);
const HARD_MAX_ROWS: u64 = 100_000;
const HARD_MAX_STREAM_BYTES: u64 = 512 * 1024 * 1024;
const HARD_MAX_DATASETS: usize = 256;

pub const SEMANTIC_COPY_PREVIEW_PROFILE: &str = "org.sqlite-capsule.semantic-copy-preview/1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SemanticCopyMode {
    Fork,
    CreateFromTemplate,
    SelectiveFork,
}

impl SemanticCopyMode {
    fn operation_name(self) -> &'static str {
        match self {
            Self::Fork => "fork",
            Self::CreateFromTemplate => "create-from-template",
            Self::SelectiveFork => "selective-fork",
        }
    }

    fn lineage_operation(self) -> &'static str {
        match self {
            Self::CreateFromTemplate => "created-from-template",
            Self::Fork | Self::SelectiveFork => "fork",
        }
    }

    fn parent_relation(self) -> &'static str {
        match self {
            Self::CreateFromTemplate => "created-from",
            Self::Fork | Self::SelectiveFork => "forked-from",
        }
    }

    fn instance_profile(self) -> &'static str {
        match self {
            Self::CreateFromTemplate => TEMPLATE_INSTANCE_PROFILE,
            Self::SelectiveFork => SELECTIVE_INSTANCE_PROFILE,
            Self::Fork => FORK_INSTANCE_PROFILE,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SemanticChoiceDisposition {
    Copy,
    Omit,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SemanticDatasetChoice {
    pub dataset_id: String,
    pub disposition: SemanticChoiceDisposition,
    pub sensitive_confirmed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SemanticDatasetAction {
    Copy,
    Omit,
    Reset,
}

impl SemanticDatasetAction {
    fn name(self) -> &'static str {
        match self {
            Self::Copy => "copy",
            Self::Omit => "omit",
            Self::Reset => "reset",
        }
    }

    fn is_present(self) -> bool {
        !matches!(self, Self::Omit)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SemanticDatasetDecision {
    pub dataset_id: String,
    pub action: SemanticDatasetAction,
    pub sensitivity: Sensitivity,
    pub sensitive_confirmed: bool,
    pub source_row_count: u64,
    pub source_state_profile: &'static str,
    pub source_state_sha256: String,
}

#[derive(Clone, Debug)]
pub struct SemanticCopyPlanRequest<'a> {
    pub output_path: &'a Path,
    pub plan_id: &'a str,
    pub created_at: &'a str,
    pub expires_at: &'a str,
    pub mode: SemanticCopyMode,
    pub choices: &'a [SemanticDatasetChoice],
    pub deadline: Duration,
    pub max_output_bytes: u64,
    pub max_rows: u64,
    pub max_stream_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SemanticCopyPreview {
    pub profile: &'static str,
    pub mode: SemanticCopyMode,
    pub source_format_version: &'static str,
    pub signature_count: u8,
    pub publisher_trust: &'static str,
    pub output_capsule_id: String,
    pub output_revision_id: String,
    pub capsule_identity: &'static str,
    pub revision_identity: &'static str,
    pub application_identity: &'static str,
    pub application_digest: String,
    pub datasets: Vec<SemanticDatasetDecision>,
    pub grants: &'static str,
    pub change_log: &'static str,
    pub prior_lineage: &'static str,
    pub lineage_events: u8,
    pub instance_text: &'static str,
    pub instance_assets: &'static str,
    pub destination: &'static str,
    pub overwrite_allowed: bool,
}

/// Host-held review authority. This deliberately does not implement
/// serialization and cannot be constructed from parsed renderer data.
pub struct SemanticCopyReview {
    plan: LifecyclePlan,
    destination: DestinationReservation,
    mode: SemanticCopyMode,
    choices: Vec<SemanticDatasetChoice>,
    decisions: Vec<SemanticDatasetDecision>,
    template_proof: Option<TemplateStateProof>,
    signature_inventory_sha256: String,
    output_capsule_id: String,
    output_revision_id: String,
    event_id: String,
}

pub struct PreparedSemanticCopy {
    plan: LifecyclePlan,
    source: VerifiedWorkspaceSource,
    destination: DestinationReservation,
    mode: SemanticCopyMode,
    choices: Vec<SemanticDatasetChoice>,
    decisions: Vec<SemanticDatasetDecision>,
    template_proof: Option<TemplateStateProof>,
    signature_inventory_sha256: String,
    output_capsule_id: String,
    output_revision_id: String,
    event_id: String,
    deadline: Instant,
    cancellation: CancellationToken,
    max_rows: u64,
    max_stream_bytes: u64,
}

pub struct SemanticCopyStaging {
    plan: LifecyclePlan,
    source: VerifiedWorkspaceSource,
    private: PrivateOutput,
    mode: SemanticCopyMode,
    choices: Vec<SemanticDatasetChoice>,
    decisions: Vec<SemanticDatasetDecision>,
    template_proof: Option<TemplateStateProof>,
    signature_inventory_sha256: String,
    output_capsule_id: String,
    output_revision_id: String,
    event_id: String,
    deadline: Instant,
    cancellation: CancellationToken,
    max_rows: u64,
    max_stream_bytes: u64,
}

pub struct ValidatedSemanticCopy {
    plan: LifecyclePlan,
    source: VerifiedWorkspaceSource,
    sealed: SealedPrivateOutput,
    mode: SemanticCopyMode,
    choices: Vec<SemanticDatasetChoice>,
    decisions: Vec<SemanticDatasetDecision>,
    template_proof: Option<TemplateStateProof>,
    signature_inventory_sha256: String,
    output_capsule_id: String,
    output_revision_id: String,
    event_id: String,
    deadline: Instant,
    cancellation: CancellationToken,
    max_rows: u64,
    max_stream_bytes: u64,
}

pub struct PublishedSemanticCopy {
    inner: PublishedOutput,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DatasetState {
    dataset_id: String,
    row_count: u64,
    digest_sha256: String,
}

/// Opens a semantic source with the operation-specific v0.2 result required by
/// ADR 0029. The initial duplicate verifier prevents a malformed v0.2 capsule
/// from being misreported as merely unsupported.
pub fn open_semantic_copy_source(
    path: &Path,
    limits: &WorkspaceLimits,
    cancellation: &CancellationToken,
) -> Result<VerifiedWorkspaceSource, WorkspaceError> {
    let started = Instant::now();
    let first = VerifiedCopySource::open_with_control(path, limits, cancellation)?;
    if first.identity().format_version == "0.2" {
        return Err(unsupported_operation());
    }
    drop(first);
    let remaining = limits.deadline.saturating_sub(started.elapsed());
    if remaining.is_zero() {
        return Err(limit_exceeded());
    }
    VerifiedWorkspaceSource::open_with_control(
        path,
        &WorkspaceLimits {
            deadline: remaining,
            ..limits.clone()
        },
        cancellation,
    )
}

pub fn generate_semantic_copy_plan(
    source: &VerifiedWorkspaceSource,
    request: &SemanticCopyPlanRequest<'_>,
    cancellation: &CancellationToken,
) -> Result<SemanticCopyReview, WorkspaceError> {
    require_complete_signature_inventory(source)?;
    if source.identity().user_version != 3 || source.identity().format_version != "0.3" {
        return Err(unsupported_operation());
    }
    let deadline_budget = request.deadline.min(HARD_DEADLINE);
    let max_rows = request.max_rows.min(HARD_MAX_ROWS);
    let max_stream_bytes = request.max_stream_bytes.min(HARD_MAX_STREAM_BYTES);
    let max_output_bytes = request
        .max_output_bytes
        .min(sqlite_capsule_core::MAX_CAPSULE_BYTES);
    if deadline_budget.is_zero()
        || max_rows == 0
        || max_stream_bytes == 0
        || max_output_bytes < source.source_identity().bytes
    {
        return Err(limit_exceeded());
    }
    let deadline = Instant::now()
        .checked_add(deadline_budget)
        .ok_or_else(limit_exceeded)?;
    assert_source_current(source, deadline, cancellation)?;

    // Fork/selective reset requires a separately retained clean template
    // source. This initial typestate owns only one source, so it must not
    // reinterpret a working source's proof as reset authority.
    if request.mode != SemanticCopyMode::CreateFromTemplate
        && source
            .data_contract()
            .datasets
            .iter()
            .any(|dataset| dataset.fork == ForkPolicy::Reset)
    {
        return Err(unsupported_operation());
    }
    if request.mode == SemanticCopyMode::CreateFromTemplate
        && source
            .data_contract()
            .datasets
            .iter()
            .any(|dataset| dataset.fork == ForkPolicy::Forbid)
    {
        return Err(unsupported_operation());
    }
    let needs_template = request.mode == SemanticCopyMode::CreateFromTemplate;
    let template_proof = if needs_template {
        Some(verify_template_state(
            source,
            &TemplateStateLimits {
                deadline: remaining(deadline)?,
                max_rows,
                max_stream_bytes,
            },
            cancellation,
        )?)
    } else {
        None
    };
    let actions = derive_actions(
        source,
        request.mode,
        request.choices,
        template_proof.as_ref(),
    )?;
    let states =
        capture_dataset_states(source, deadline, cancellation, max_rows, max_stream_bytes)?;
    let decisions = join_decisions(source, &actions, &states)?;

    let source_revision_id = source
        .identity()
        .overview
        .instance
        .revision_id
        .as_deref()
        .ok_or_else(invalid_contract)?;
    let (output_capsule_id, output_revision_id, event_id) = generate_distinct_ids(
        source.verified.connection(),
        &source.identity().capsule_id,
        source_revision_id,
    )?;
    let output = absolute_output(request.output_path)?;
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(invalid_contract)?;
    let leaf = output
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(invalid_contract)?;
    let destination = DestinationReservation::reserve(
        parent,
        OsStr::new(leaf),
        std::slice::from_ref(source.source_identity()),
    )
    .map_err(map_destination_error)?;
    let signature_inventory_sha256 = signature_inventory_sha256(source)?;
    let proof_sha256 = template_proof.as_ref().map(proof_sha256).transpose()?;
    let identity = source.identity();
    let instance = &identity.overview.instance;
    let schema = identity
        .overview
        .data_schema
        .as_ref()
        .ok_or_else(invalid_contract)?;
    let source_sha256 = lower_hex(&source.verified.source_sha256);
    let app_digest = lower_hex(source.application_digest());
    let deadline_ms = u64::try_from(deadline_budget.as_millis()).map_err(|_| limit_exceeded())?;
    let mut plan_decisions = vec![json!({
        "scope": "application",
        "subject": identity.app_id,
        "action": SEMANTIC_ACTION,
        "reason": "Host-owned semantic copy with signed dataset-policy enforcement.",
        "parameters": {
            "event_id": event_id,
            "instance_profile": request.mode.instance_profile(),
            "mode": request.mode.operation_name(),
            "mutable_platform_profile": MUTABLE_PLATFORM_PROFILE,
            "occurred_at": request.created_at,
            "signature_count": source.signature_reports().len(),
            "signature_inventory_sha256": signature_inventory_sha256,
            "template_state_sha256": proof_sha256
        }
    })];
    for decision in &decisions {
        let dataset = source
            .data_contract()
            .datasets
            .iter()
            .find(|dataset| dataset.id == decision.dataset_id)
            .ok_or_else(invalid_contract)?;
        plan_decisions.push(json!({
            "scope": "dataset",
            "subject": decision.dataset_id,
            "action": decision.action.name(),
            "reason": "Derived from the signed dataset contract and closed host choices.",
            "parameters": {
                "fork_policy": fork_policy_name(dataset.fork),
                "sensitive_confirmed": decision.sensitive_confirmed,
                "sensitivity": sensitivity_name(decision.sensitivity),
                "source_row_count": decision.source_row_count,
                "source_state_profile": decision.source_state_profile,
                "source_state_sha256": decision.source_state_sha256
            }
        }));
    }
    let live = source.source_identity();
    let mut value = json!({
        "profile": crate::PLAN_PROFILE,
        "plan_id": request.plan_id,
        "operation": request.mode.operation_name(),
        "created_at": request.created_at,
        "expires_at": request.expires_at,
        "inputs": [{
            "role": "source",
            "path_hint": utf8_path(&identity.canonical_path)?,
            "file_sha256": source_sha256,
            "snapshot_sha256": source_sha256,
            "size_bytes": live.bytes,
            "filesystem_identity": {
                "platform": std::env::consts::OS,
                "volume_or_device": live.device.to_string(),
                "file_id_or_inode": live.stable_file_id,
                "modified_ns": live.modified_ns
            },
            "capsule": {
                "format_version": "0.3",
                "capsule_id": identity.capsule_id,
                "revision_id": instance.revision_id,
                "app_id": identity.app_id,
                "app_version": identity.app_version,
                "application_digest": app_digest,
                "data_schema_id": schema.data_schema_id,
                "data_schema_version": schema.data_schema_version,
                "publisher_key_id": null
            }
        }],
        "output": {
            "path": utf8_path(&destination.path_hint())?,
            "leaf_name": leaf,
            "parent_filesystem_identity": {
                "platform": std::env::consts::OS,
                "volume_or_device": destination.identity().device.to_string(),
                "file_id_or_inode": destination.identity().stable_file_id
            },
            "must_not_exist": true,
            "publish_mode": "create-new-no-replace"
        },
        "decisions": plan_decisions,
        "limits": {
            "max_input_bytes": live.bytes,
            "max_output_bytes": max_output_bytes,
            "max_rows_inspected": max_rows,
            "max_rows_written": max_rows,
            "deadline_ms": deadline_ms
        },
        "expected": {
            "capsule_id": output_capsule_id,
            "revision_id": output_revision_id,
            "app_id": identity.app_id,
            "application_digest": app_digest,
            "data_schema_id": schema.data_schema_id,
            "data_schema_version": schema.data_schema_version
        },
        "plan_digest": ""
    });
    let digest = canonical_digest_value(&value)?;
    value["plan_digest"] = serde_json::Value::String(digest);
    let bytes = serde_json::to_vec(&value).map_err(|_| invalid_contract())?;
    let plan = parse_semantic_copy_plan(&bytes)?;
    bind_source(&plan, source, &signature_inventory_sha256)?;
    bind_parent(&plan, &destination)?;
    assert_source_current(source, deadline, cancellation)?;
    destination
        .assert_reserved_current()
        .map_err(map_prepared_destination_error)?;
    Ok(SemanticCopyReview {
        plan,
        destination,
        mode: request.mode,
        choices: request.choices.to_vec(),
        decisions,
        template_proof,
        signature_inventory_sha256,
        output_capsule_id,
        output_revision_id,
        event_id,
    })
}

pub fn parse_semantic_copy_plan(bytes: &[u8]) -> Result<LifecyclePlan, WorkspaceError> {
    let plan = LifecyclePlan::parse(bytes)?;
    validate_shape(&plan)?;
    Ok(plan)
}

impl SemanticCopyReview {
    pub fn plan(&self) -> &LifecyclePlan {
        &self.plan
    }

    pub fn preview(&self) -> SemanticCopyPreview {
        SemanticCopyPreview {
            profile: SEMANTIC_COPY_PREVIEW_PROFILE,
            mode: self.mode,
            source_format_version: "0.3",
            signature_count: u8::try_from(
                self.plan
                    .decisions()
                    .first()
                    .and_then(|_| {
                        serde_json::to_value(&self.plan).ok()?.get("decisions")?[0]
                            ["parameters"]["signature_count"]
                            .as_u64()
                    })
                    .unwrap_or(0),
            )
            .unwrap_or(0),
            publisher_trust: "separate-host-policy",
            output_capsule_id: self.output_capsule_id.clone(),
            output_revision_id: self.output_revision_id.clone(),
            capsule_identity: "new-uuid-v4",
            revision_identity: "new-uuid-v4",
            application_identity: "preserved",
            application_digest: self
                .plan
                .expected()
                .application_digest()
                .unwrap_or_default()
                .to_owned(),
            datasets: self.decisions.clone(),
            grants: "cleared",
            change_log: "cleared",
            prior_lineage: "cleared",
            lineage_events: 1,
            instance_text: match self.mode {
                SemanticCopyMode::CreateFromTemplate | SemanticCopyMode::SelectiveFork => {
                    "signed-app-defaults"
                }
                SemanticCopyMode::Fork => "preserved-nonauthoritative",
            },
            instance_assets: "cleared",
            destination: "create-new-no-replace",
            overwrite_allowed: false,
        }
    }

    pub fn prepare(
        self,
        approved_plan: LifecyclePlan,
        source: VerifiedWorkspaceSource,
        limits: &WorkspaceLimits,
        cancellation: &CancellationToken,
    ) -> Result<PreparedSemanticCopy, WorkspaceError> {
        PreparedSemanticCopy::prepare_at(
            self,
            approved_plan,
            source,
            SystemTime::now(),
            limits,
            cancellation,
        )
    }
}

impl PreparedSemanticCopy {
    fn prepare_at(
        review: SemanticCopyReview,
        plan: LifecyclePlan,
        source: VerifiedWorkspaceSource,
        now: SystemTime,
        limits: &WorkspaceLimits,
        cancellation: &CancellationToken,
    ) -> Result<Self, WorkspaceError> {
        if review.plan.canonical_bytes()? != plan.canonical_bytes()? {
            return Err(stale_plan());
        }
        validate_time_window(&plan, now)?;
        validate_shape(&plan)?;
        require_complete_signature_inventory(&source)?;
        let budget = Duration::from_millis(plan.limits().deadline_ms())
            .min(limits.deadline)
            .min(HARD_DEADLINE);
        if budget.is_zero() || cancellation.is_cancelled() {
            return Err(if cancellation.is_cancelled() {
                cancelled()
            } else {
                limit_exceeded()
            });
        }
        let deadline = Instant::now()
            .checked_add(budget)
            .ok_or_else(limit_exceeded)?;
        bind_source(&plan, &source, &review.signature_inventory_sha256)?;
        bind_parent(&plan, &review.destination)?;
        assert_source_current(&source, deadline, cancellation)?;
        review
            .destination
            .assert_reserved_current()
            .map_err(map_prepared_destination_error)?;

        let max_rows = plan_limit_u64(&plan, "max_rows_inspected")?.min(HARD_MAX_ROWS);
        let max_stream_bytes = limits
            .max_capsule_bytes
            .saturating_mul(8)
            .clamp(1, HARD_MAX_STREAM_BYTES);
        let proof = reproduce_template_proof(
            &source,
            review.template_proof.as_ref(),
            review.mode,
            max_rows,
            max_stream_bytes,
            deadline,
            cancellation,
        )?;
        let actions = derive_actions(&source, review.mode, &review.choices, proof.as_ref())?;
        let states =
            capture_dataset_states(&source, deadline, cancellation, max_rows, max_stream_bytes)?;
        let decisions = join_decisions(&source, &actions, &states)?;
        if decisions != review.decisions {
            return Err(stale_plan());
        }
        validate_decisions_bound(&plan, &source, &decisions)?;
        Ok(Self {
            plan,
            source,
            destination: review.destination,
            mode: review.mode,
            choices: review.choices,
            decisions,
            template_proof: proof,
            signature_inventory_sha256: review.signature_inventory_sha256,
            output_capsule_id: review.output_capsule_id,
            output_revision_id: review.output_revision_id,
            event_id: review.event_id,
            deadline,
            cancellation: cancellation.clone(),
            max_rows,
            max_stream_bytes,
        })
    }

    pub fn stage(self) -> Result<SemanticCopyStaging, WorkspaceError> {
        check(self.deadline, &self.cancellation)?;
        validate_time_window(&self.plan, SystemTime::now())?;
        assert_source_current(&self.source, self.deadline, &self.cancellation)?;
        self.destination
            .assert_reserved_current()
            .map_err(map_prepared_destination_error)?;
        let private = self
            .destination
            .stage()
            .map_err(map_prepared_destination_error)?;
        maybe_crash("private-created");
        Ok(SemanticCopyStaging {
            plan: self.plan,
            source: self.source,
            private,
            mode: self.mode,
            choices: self.choices,
            decisions: self.decisions,
            template_proof: self.template_proof,
            signature_inventory_sha256: self.signature_inventory_sha256,
            output_capsule_id: self.output_capsule_id,
            output_revision_id: self.output_revision_id,
            event_id: self.event_id,
            deadline: self.deadline,
            cancellation: self.cancellation,
            max_rows: self.max_rows,
            max_stream_bytes: self.max_stream_bytes,
        })
    }
}

impl SemanticCopyStaging {
    /// Copies only the retained verified private snapshot, transforms only
    /// domain rows and mutable platform compartments, compacts the private
    /// payload and performs exhaustive operation-specific validation.
    pub fn transform_and_validate(mut self) -> Result<ValidatedSemanticCopy, WorkspaceError> {
        check(self.deadline, &self.cancellation)?;
        validate_time_window(&self.plan, SystemTime::now())?;
        require_complete_signature_inventory(&self.source)?;
        assert_source_current(&self.source, self.deadline, &self.cancellation)?;

        let proof = reproduce_template_proof(
            &self.source,
            self.template_proof.as_ref(),
            self.mode,
            self.max_rows,
            self.max_stream_bytes,
            self.deadline,
            &self.cancellation,
        )?;
        let actions = derive_actions(&self.source, self.mode, &self.choices, proof.as_ref())?;
        let states = capture_dataset_states(
            &self.source,
            self.deadline,
            &self.cancellation,
            self.max_rows,
            self.max_stream_bytes,
        )?;
        let decisions = join_decisions(&self.source, &actions, &states)?;
        if decisions != self.decisions {
            return Err(stale_plan());
        }
        validate_decisions_bound(&self.plan, &self.source, &decisions)?;

        let control = verification_control(self.deadline, &self.cancellation)?;
        let copied = self
            .source
            .verified
            .copy_snapshot_to_file_with_control(
                self.private.file_mut(),
                &control,
                self.plan.limits().max_output_bytes(),
            )
            .map_err(map_launch_output_error)?;
        if copied != self.source.source_identity().bytes {
            return Err(verification_failed());
        }
        self.private
            .file_mut()
            .sync_all()
            .map_err(|_| output_failed())?;
        maybe_crash("snapshot-copied");

        transform_private(
            self.private.private_path_hint(),
            &self.source,
            self.mode,
            &decisions,
            &self.output_capsule_id,
            &self.output_revision_id,
            &self.event_id,
            &self.plan,
            self.deadline,
            &self.cancellation,
        )?;
        maybe_crash("transformed");
        vacuum_private(
            self.private.private_path_hint(),
            self.deadline,
            &self.cancellation,
        )?;
        maybe_crash("vacuumed");
        let staged_path = self.private.private_path_hint().to_path_buf();
        let sealed = self
            .private
            .seal_with_limit(self.plan.limits().max_output_bytes())
            .map_err(map_destination_error)?;
        sealed
            .assert_staged_current()
            .map_err(map_destination_error)?;
        let output = open_output_bound(
            &staged_path,
            sealed.identity().bytes,
            *sealed.sha256(),
            self.deadline,
            &self.cancellation,
        )?;
        require_same_object(sealed.identity(), output.source_identity())?;
        validate_semantic_output(
            &output,
            &self.source,
            self.mode,
            &self.choices,
            &decisions,
            proof.as_ref(),
            &self.signature_inventory_sha256,
            &self.output_capsule_id,
            &self.output_revision_id,
            &self.event_id,
            &self.plan,
            self.deadline,
            &self.cancellation,
            self.max_rows,
            self.max_stream_bytes,
        )?;
        drop(output);
        sealed
            .assert_staged_current()
            .map_err(map_destination_error)?;
        assert_source_current(&self.source, self.deadline, &self.cancellation)?;
        maybe_crash("sealed-and-verified");
        Ok(ValidatedSemanticCopy {
            plan: self.plan,
            source: self.source,
            sealed,
            mode: self.mode,
            choices: self.choices,
            decisions,
            template_proof: proof,
            signature_inventory_sha256: self.signature_inventory_sha256,
            output_capsule_id: self.output_capsule_id,
            output_revision_id: self.output_revision_id,
            event_id: self.event_id,
            deadline: self.deadline,
            cancellation: self.cancellation,
            max_rows: self.max_rows,
            max_stream_bytes: self.max_stream_bytes,
        })
    }
}

impl ValidatedSemanticCopy {
    pub fn publish(self) -> Result<PublishedSemanticCopy, WorkspaceError> {
        self.publish_with_hook(|| {})
    }

    fn publish_with_hook<F>(
        self,
        after_final_output_check: F,
    ) -> Result<PublishedSemanticCopy, WorkspaceError>
    where
        F: FnOnce(),
    {
        check(self.deadline, &self.cancellation)?;
        validate_time_window(&self.plan, SystemTime::now())?;
        assert_source_current(&self.source, self.deadline, &self.cancellation)?;
        self.sealed
            .assert_staged_current()
            .map_err(map_destination_error)?;
        let prepublish = open_output_bound(
            self.sealed.private_path_hint(),
            self.sealed.identity().bytes,
            *self.sealed.sha256(),
            self.deadline,
            &self.cancellation,
        )?;
        require_same_object(self.sealed.identity(), prepublish.source_identity())?;
        validate_semantic_output(
            &prepublish,
            &self.source,
            self.mode,
            &self.choices,
            &self.decisions,
            self.template_proof.as_ref(),
            &self.signature_inventory_sha256,
            &self.output_capsule_id,
            &self.output_revision_id,
            &self.event_id,
            &self.plan,
            self.deadline,
            &self.cancellation,
            self.max_rows,
            self.max_stream_bytes,
        )?;
        drop(prepublish);
        self.sealed
            .assert_staged_current()
            .map_err(map_destination_error)?;
        assert_source_current(&self.source, self.deadline, &self.cancellation)?;

        let mode = self.mode;
        let choices = self.choices.clone();
        let decisions = self.decisions.clone();
        let template_proof = self.template_proof.clone();
        let signature_inventory_sha256 = self.signature_inventory_sha256.clone();
        let output_capsule_id = self.output_capsule_id.clone();
        let output_revision_id = self.output_revision_id.clone();
        let event_id = self.event_id.clone();
        let deadline = self.deadline;
        let cancellation = self.cancellation.clone();
        let max_rows = self.max_rows;
        let max_stream_bytes = self.max_stream_bytes;
        let max_output_bytes = self.plan.limits().max_output_bytes();
        let plan = &self.plan;
        let source = &self.source;
        // SAFETY: the held staged file has been exhaustively reopened as a
        // signed v0.3 workspace source and all semantic postconditions were
        // proven. The callback repeats those proofs on a snapshot of the held
        // final file and performs the final source rebind while quarantine is
        // still available.
        let published = unsafe {
            self.sealed
                .publish_no_replace_unchecked(|reopened, reopened_identity| {
                    let snapshot =
                        snapshot_held_file(reopened, max_output_bytes, deadline, &cancellation)?;
                    let output =
                        open_output(snapshot.path(), deadline, &cancellation).map_err(|_| {
                            sqlite_capsule_lifecycle::LifecycleError::PostPublishVerification
                        })?;
                    if output.source_identity().bytes != reopened_identity.bytes
                        || validate_semantic_output(
                            &output,
                            source,
                            mode,
                            &choices,
                            &decisions,
                            template_proof.as_ref(),
                            &signature_inventory_sha256,
                            &output_capsule_id,
                            &output_revision_id,
                            &event_id,
                            plan,
                            deadline,
                            &cancellation,
                            max_rows,
                            max_stream_bytes,
                        )
                        .is_err()
                    {
                        return Err(
                            sqlite_capsule_lifecycle::LifecycleError::PostPublishVerification,
                        );
                    }
                    maybe_crash("postrename-reopened");
                    after_final_output_check();
                    assert_source_current(source, deadline, &cancellation).map_err(|_| {
                        sqlite_capsule_lifecycle::LifecycleError::PostPublishVerification
                    })?;
                    Ok(())
                })
        }
        .map_err(map_destination_error)?;
        Ok(PublishedSemanticCopy { inner: published })
    }
}

impl PublishedSemanticCopy {
    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    pub fn identity(&self) -> &SourceIdentity {
        &self.inner.identity
    }
}

fn derive_actions(
    source: &VerifiedWorkspaceSource,
    mode: SemanticCopyMode,
    choices: &[SemanticDatasetChoice],
    template_proof: Option<&TemplateStateProof>,
) -> Result<BTreeMap<String, (SemanticDatasetAction, bool)>, WorkspaceError> {
    if choices.len() > HARD_MAX_DATASETS {
        return Err(limit_exceeded());
    }
    let mut choice_map = BTreeMap::new();
    for choice in choices {
        if choice.dataset_id.is_empty()
            || choice.dataset_id.len() > 512
            || choice_map
                .insert(choice.dataset_id.clone(), choice)
                .is_some()
        {
            return Err(invalid_contract());
        }
    }
    if mode == SemanticCopyMode::CreateFromTemplate && !choice_map.is_empty() {
        return Err(invalid_contract());
    }
    if mode == SemanticCopyMode::CreateFromTemplate && template_proof.is_none() {
        return Err(unsupported_operation());
    }

    let mut result = BTreeMap::new();
    for dataset in &source.data_contract().datasets {
        let choice = choice_map.remove(&dataset.id);
        let (action, confirmed) = if mode == SemanticCopyMode::CreateFromTemplate {
            if dataset.fork == ForkPolicy::Forbid {
                return Err(unsupported_operation());
            }
            // Other fork policies do not designate clean rows. Template mode
            // is governed instead by the exhaustive authenticated template
            // proof reproduced from this retained clean source.
            (SemanticDatasetAction::Reset, false)
        } else {
            match (mode, dataset.fork, choice) {
                (_, ForkPolicy::Forbid, _) => return Err(unsupported_operation()),
                (_, ForkPolicy::Reset, _) => return Err(unsupported_operation()),
                (SemanticCopyMode::Fork, ForkPolicy::Copy, choice) => {
                    copy_policy_action(dataset.sensitivity, choice)?
                }
                (SemanticCopyMode::Fork, ForkPolicy::Prompt, Some(choice)) => {
                    prompt_action(dataset.sensitivity, choice)?
                }
                (SemanticCopyMode::Fork, ForkPolicy::Prompt, None) => {
                    return Err(invalid_contract());
                }
                (SemanticCopyMode::Fork, ForkPolicy::Omit, None) => {
                    (SemanticDatasetAction::Omit, false)
                }
                (SemanticCopyMode::Fork, ForkPolicy::Omit, Some(_)) => {
                    return Err(invalid_contract());
                }
                (SemanticCopyMode::SelectiveFork, ForkPolicy::Copy, choice) => {
                    copy_policy_action(dataset.sensitivity, choice)?
                }
                (SemanticCopyMode::SelectiveFork, ForkPolicy::Prompt, Some(choice)) => {
                    prompt_action(dataset.sensitivity, choice)?
                }
                (SemanticCopyMode::SelectiveFork, ForkPolicy::Prompt, None) => {
                    (SemanticDatasetAction::Omit, false)
                }
                (SemanticCopyMode::SelectiveFork, ForkPolicy::Omit, None) => {
                    (SemanticDatasetAction::Omit, false)
                }
                (SemanticCopyMode::SelectiveFork, ForkPolicy::Omit, Some(choice))
                    if choice.disposition == SemanticChoiceDisposition::Omit
                        && !choice.sensitive_confirmed =>
                {
                    (SemanticDatasetAction::Omit, false)
                }
                (SemanticCopyMode::SelectiveFork, ForkPolicy::Omit, Some(_)) => {
                    return Err(invalid_contract());
                }
                (SemanticCopyMode::CreateFromTemplate, _, _) => unreachable!(),
            }
        };
        if dataset.required && action == SemanticDatasetAction::Omit {
            return Err(invalid_contract());
        }
        result.insert(dataset.id.clone(), (action, confirmed));
    }
    if !choice_map.is_empty() {
        return Err(invalid_contract());
    }

    // Dataset loading has already proven that declared dependencies cover all
    // actual cross-dataset foreign keys with restrictive actions. Enforce the
    // action closure for every semantic mode; no present dependent may retain
    // an omitted prerequisite.
    for dataset in &source.data_contract().datasets {
        let action = result.get(&dataset.id).ok_or_else(invalid_contract)?.0;
        if !action.is_present() {
            continue;
        }
        for dependency in &dataset.dependencies {
            if !result
                .get(&dependency.dataset_id)
                .ok_or_else(invalid_contract)?
                .0
                .is_present()
            {
                return Err(invalid_contract());
            }
        }
    }
    Ok(result)
}

fn copy_policy_action(
    sensitivity: Sensitivity,
    choice: Option<&SemanticDatasetChoice>,
) -> Result<(SemanticDatasetAction, bool), WorkspaceError> {
    if choice.is_some_and(|choice| choice.disposition == SemanticChoiceDisposition::Omit) {
        // A host choice cannot weaken signed `copy` policy.
        return Err(invalid_contract());
    }
    if sensitivity == Sensitivity::Sensitive {
        let Some(choice) = choice else {
            return Err(sensitive_confirmation_required());
        };
        if choice.disposition != SemanticChoiceDisposition::Copy || !choice.sensitive_confirmed {
            return Err(sensitive_confirmation_required());
        }
        Ok((SemanticDatasetAction::Copy, true))
    } else {
        if choice.is_some_and(|choice| choice.sensitive_confirmed) {
            return Err(invalid_contract());
        }
        Ok((SemanticDatasetAction::Copy, false))
    }
}

fn prompt_action(
    sensitivity: Sensitivity,
    choice: &SemanticDatasetChoice,
) -> Result<(SemanticDatasetAction, bool), WorkspaceError> {
    match choice.disposition {
        SemanticChoiceDisposition::Omit if choice.sensitive_confirmed => Err(invalid_contract()),
        SemanticChoiceDisposition::Omit => Ok((SemanticDatasetAction::Omit, false)),
        SemanticChoiceDisposition::Copy
            if sensitivity == Sensitivity::Sensitive && !choice.sensitive_confirmed =>
        {
            Err(sensitive_confirmation_required())
        }
        SemanticChoiceDisposition::Copy
            if sensitivity == Sensitivity::Normal && choice.sensitive_confirmed =>
        {
            Err(invalid_contract())
        }
        SemanticChoiceDisposition::Copy => Ok((
            SemanticDatasetAction::Copy,
            sensitivity == Sensitivity::Sensitive,
        )),
    }
}

fn reproduce_template_proof(
    source: &VerifiedWorkspaceSource,
    expected: Option<&TemplateStateProof>,
    mode: SemanticCopyMode,
    max_rows: u64,
    max_stream_bytes: u64,
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<Option<TemplateStateProof>, WorkspaceError> {
    let required = mode == SemanticCopyMode::CreateFromTemplate;
    if !required {
        if expected.is_some() {
            return Err(stale_plan());
        }
        return Ok(None);
    }
    let proof = verify_template_state(
        source,
        &TemplateStateLimits {
            deadline: remaining(deadline)?,
            max_rows,
            max_stream_bytes,
        },
        cancellation,
    )?;
    if expected.is_some_and(|expected| expected != &proof) {
        return Err(stale_plan());
    }
    Ok(Some(proof))
}

fn join_decisions(
    source: &VerifiedWorkspaceSource,
    actions: &BTreeMap<String, (SemanticDatasetAction, bool)>,
    states: &[DatasetState],
) -> Result<Vec<SemanticDatasetDecision>, WorkspaceError> {
    let states: BTreeMap<_, _> = states
        .iter()
        .map(|state| (state.dataset_id.as_str(), state))
        .collect();
    source
        .data_contract()
        .datasets
        .iter()
        .map(|dataset| {
            let (action, confirmed) = actions
                .get(&dataset.id)
                .copied()
                .ok_or_else(invalid_contract)?;
            let state = states
                .get(dataset.id.as_str())
                .ok_or_else(invalid_contract)?;
            Ok(SemanticDatasetDecision {
                dataset_id: dataset.id.clone(),
                action,
                sensitivity: dataset.sensitivity,
                sensitive_confirmed: confirmed,
                source_row_count: state.row_count,
                source_state_profile: crate::template_state::DATASET_STATE_PROFILE,
                source_state_sha256: state.digest_sha256.clone(),
            })
        })
        .collect()
}

fn capture_dataset_states(
    source: &VerifiedWorkspaceSource,
    deadline: Instant,
    cancellation: &CancellationToken,
    max_rows: u64,
    max_stream_bytes: u64,
) -> Result<Vec<DatasetState>, WorkspaceError> {
    let mut rows_remaining = max_rows;
    let mut bytes_remaining = max_stream_bytes;
    source
        .data_contract()
        .datasets
        .iter()
        .map(|dataset| {
            let (row_count, digest_sha256) = crate::template_state::dataset_state_with_budget(
                source,
                dataset,
                &mut rows_remaining,
                &mut bytes_remaining,
                deadline,
                cancellation,
            )?;
            Ok(DatasetState {
                dataset_id: dataset.id.clone(),
                row_count,
                digest_sha256,
            })
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn transform_private(
    path: &Path,
    source: &VerifiedWorkspaceSource,
    mode: SemanticCopyMode,
    decisions: &[SemanticDatasetDecision],
    output_capsule_id: &str,
    output_revision_id: &str,
    event_id: &str,
    plan: &LifecyclePlan,
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
            "PRAGMA trusted_schema=OFF; \
             PRAGMA foreign_keys=ON; \
             PRAGMA journal_mode=DELETE;",
        )
        .map_err(|_| query_error(deadline, cancellation))?;
    check(deadline, cancellation)?;
    let result = (|| -> Result<(), WorkspaceError> {
        connection
            .execute_batch("BEGIN IMMEDIATE; PRAGMA defer_foreign_keys=ON;")
            .map_err(|_| query_error(deadline, cancellation))?;

        // Defer restrictive cross-dataset foreign keys and remove every
        // omitted dataset table. Schema objects are never dropped or edited.
        for dataset in source.data_contract().datasets.iter().rev() {
            let decision = decisions
                .iter()
                .find(|decision| decision.dataset_id == dataset.id)
                .ok_or_else(invalid_contract)?;
            if decision.action != SemanticDatasetAction::Omit {
                continue;
            }
            for table in dataset.tables.iter().rev() {
                check(deadline, cancellation)?;
                connection
                    .execute(
                        &format!("DELETE FROM {}", quote_identifier(&table.name)),
                        [],
                    )
                    .map_err(|_| query_error(deadline, cancellation))?;
            }
        }

        let (title, description, document_kind, tags_json) = match mode {
            SemanticCopyMode::CreateFromTemplate | SemanticCopyMode::SelectiveFork => (
                source.identity().overview.application.name.clone(),
                String::new(),
                "capsule".to_owned(),
                "[]".to_owned(),
            ),
            SemanticCopyMode::Fork => {
                let instance = &source.identity().overview.instance;
                // The values are mutable and explicitly remain
                // nonauthoritative; JSON is taken from the verified projection
                // rather than copied as unchecked source text.
                let tags = serde_json::to_string(&instance.tags)
                    .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InternalError))?;
                (
                    instance.title.clone(),
                    instance.description.clone(),
                    instance.document_kind.clone(),
                    tags,
                )
            }
        };
        let instance_changes = connection
            .execute(
                "UPDATE capsule_instance SET \
                 capsule_id = ?1, revision_id = ?2, title = ?3, \
                 description = ?4, document_kind = ?5, tags_json = ?6, \
                 icon_asset_id = NULL, cover_asset_id = NULL, \
                 created_at = ?7, content_updated_at = ?7 \
                 WHERE id = 1",
                params![
                    output_capsule_id,
                    output_revision_id,
                    title,
                    description,
                    document_kind,
                    tags_json,
                    plan.created_at()
                ],
            )
            .map_err(|_| query_error(deadline, cancellation))?;
        if instance_changes != 1 {
            return Err(verification_failed());
        }
        connection
            .execute_batch(
                "DELETE FROM capsule_instance_asset; \
                 DELETE FROM capsule_grant; \
                 DELETE FROM capsule_change_log; \
                 DELETE FROM capsule_lineage_parent; \
                 DELETE FROM capsule_lineage_event;",
            )
            .map_err(|_| query_error(deadline, cancellation))?;

        let source_identity = source.identity();
        let source_revision = source_identity
            .overview
            .instance
            .revision_id
            .as_deref()
            .ok_or_else(invalid_contract)?;
        let schema = source_identity
            .overview
            .data_schema
            .as_ref()
            .ok_or_else(invalid_contract)?;
        connection
            .execute(
                "INSERT INTO capsule_lineage_event \
                 (event_id, sequence, operation, result_capsule_id, \
                  result_revision_id, occurred_at, application_digest, \
                  data_schema_id, data_schema_version, plan_digest, details_json) \
                 VALUES (?1, 1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, '{}')",
                params![
                    event_id,
                    mode.lineage_operation(),
                    output_capsule_id,
                    output_revision_id,
                    plan.created_at(),
                    lower_hex(source.application_digest()),
                    schema.data_schema_id,
                    schema.data_schema_version,
                    plan.plan_digest(),
                ],
            )
            .map_err(|_| query_error(deadline, cancellation))?;
        connection
            .execute(
                "INSERT INTO capsule_lineage_parent \
                 (event_id, ordinal, relation, parent_capsule_id, \
                  parent_revision_id, parent_file_sha256) \
                 VALUES (?1, 1, ?2, ?3, ?4, ?5)",
                params![
                    event_id,
                    mode.parent_relation(),
                    source_identity.capsule_id,
                    source_revision,
                    lower_hex(&source.verified.source_sha256),
                ],
            )
            .map_err(|_| query_error(deadline, cancellation))?;

        rebuild_sqlite_sequence(&connection, deadline, cancellation)?;
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
    check(deadline, cancellation)?;
    reject_sidecars(path)
}

fn rebuild_sqlite_sequence(
    connection: &Connection,
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<(), WorkspaceError> {
    let exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema \
             WHERE type = 'table' AND name = 'sqlite_sequence')",
            [],
            |row| row.get(0),
        )
        .map_err(|_| query_error(deadline, cancellation))?;
    if !exists {
        return Ok(());
    }
    // `sqlite_sequence` is mutable platform state and has no uniqueness
    // constraint. Never use its existing names as table authority: derive the
    // complete allowlist from the verified schema and replace all rows.
    let names = sequence_managed_tables(connection).map_err(|_| verification_failed())?;
    let mut rebuilt = Vec::with_capacity(names.len());
    for name in names {
        check(deadline, cancellation)?;
        let maximum: Option<i64> = connection
            .query_row(
                &format!("SELECT max(rowid) FROM {}", quote_identifier(&name)),
                [],
                |row| row.get(0),
            )
            .map_err(|_| query_error(deadline, cancellation))?;
        rebuilt.push((name, maximum.unwrap_or(0)));
    }
    connection
        .execute("DELETE FROM sqlite_sequence", [])
        .map_err(|_| query_error(deadline, cancellation))?;
    for (name, maximum) in rebuilt {
        connection
            .execute(
                "INSERT INTO sqlite_sequence(name, seq) VALUES (?1, ?2)",
                params![name, maximum],
            )
            .map_err(|_| query_error(deadline, cancellation))?;
    }
    Ok(())
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
    let _ = connection.progress_handler(0, None::<fn() -> bool>);
    drop(connection);
    check(deadline, cancellation)?;
    reject_sidecars(path)
}

#[allow(clippy::too_many_arguments)]
fn validate_semantic_output(
    output: &VerifiedWorkspaceSource,
    source: &VerifiedWorkspaceSource,
    mode: SemanticCopyMode,
    choices: &[SemanticDatasetChoice],
    decisions: &[SemanticDatasetDecision],
    template_proof: Option<&TemplateStateProof>,
    signature_inventory_sha256_expected: &str,
    output_capsule_id: &str,
    output_revision_id: &str,
    event_id: &str,
    plan: &LifecyclePlan,
    deadline: Instant,
    cancellation: &CancellationToken,
    max_rows: u64,
    max_stream_bytes: u64,
) -> Result<(), WorkspaceError> {
    check(deadline, cancellation)?;
    require_complete_signature_inventory(output)?;
    let source_identity = source.identity();
    let output_identity = output.identity();
    if output_identity.user_version != 3
        || output_identity.format_version != "0.3"
        || output_identity.capsule_id != output_capsule_id
        || output_identity.overview.instance.revision_id.as_deref() != Some(output_revision_id)
        || output_identity.app_id != source_identity.app_id
        || output_identity.app_version != source_identity.app_version
        || output_identity.overview.application != source_identity.overview.application
        || output_identity.overview.data_schema != source_identity.overview.data_schema
        || output.application_digest() != source.application_digest()
        || output.data_contract() != source.data_contract()
        || signature_inventory_sha256(output)? != signature_inventory_sha256_expected
    {
        return Err(verification_failed());
    }
    let proof = reproduce_template_proof(
        output,
        template_proof,
        mode,
        max_rows,
        max_stream_bytes,
        deadline,
        cancellation,
    )?;
    let actions = derive_actions(output, mode, choices, proof.as_ref())?;
    let states =
        capture_dataset_states(output, deadline, cancellation, max_rows, max_stream_bytes)?;
    let output_states: BTreeMap<_, _> = states
        .iter()
        .map(|state| (state.dataset_id.as_str(), state))
        .collect();
    for expected in decisions {
        let (action, confirmed) = actions
            .get(&expected.dataset_id)
            .copied()
            .ok_or_else(verification_failed)?;
        if action != expected.action || confirmed != expected.sensitive_confirmed {
            return Err(verification_failed());
        }
        let actual = output_states
            .get(expected.dataset_id.as_str())
            .ok_or_else(verification_failed)?;
        match expected.action {
            SemanticDatasetAction::Copy | SemanticDatasetAction::Reset
                if actual.row_count == expected.source_row_count
                    && actual.digest_sha256 == expected.source_state_sha256 => {}
            SemanticDatasetAction::Omit if actual.row_count == 0 => {}
            _ => return Err(verification_failed()),
        }
    }
    validate_mutable_platform(
        output,
        source,
        mode,
        output_capsule_id,
        output_revision_id,
        event_id,
        plan,
    )?;
    require_compacted(output.verified.connection())?;
    validate_sequences(output.verified.connection())?;
    reject_sidecars(&output.identity().canonical_path)?;
    check(deadline, cancellation)
}

#[allow(clippy::too_many_arguments)]
fn validate_mutable_platform(
    output: &VerifiedWorkspaceSource,
    source: &VerifiedWorkspaceSource,
    mode: SemanticCopyMode,
    output_capsule_id: &str,
    output_revision_id: &str,
    event_id: &str,
    plan: &LifecyclePlan,
) -> Result<(), WorkspaceError> {
    let connection = output.verified.connection();
    for table in [
        "capsule_grant",
        "capsule_change_log",
        "capsule_instance_asset",
    ] {
        let count: i64 = connection
            .query_row(
                &format!("SELECT count(*) FROM {}", quote_identifier(table)),
                [],
                |row| row.get(0),
            )
            .map_err(|_| verification_failed())?;
        if count != 0 {
            return Err(verification_failed());
        }
    }
    let instance = &output.identity().overview.instance;
    let source_instance = &source.identity().overview.instance;
    let instance_text_valid = match mode {
        SemanticCopyMode::CreateFromTemplate | SemanticCopyMode::SelectiveFork => {
            instance.title == source.identity().overview.application.name
                && instance.description.is_empty()
                && instance.document_kind == "capsule"
                && instance.tags.is_empty()
        }
        SemanticCopyMode::Fork => {
            instance.title == source_instance.title
                && instance.description == source_instance.description
                && instance.document_kind == source_instance.document_kind
                && instance.tags == source_instance.tags
        }
    };
    if !instance_text_valid
        || instance.capsule_id != output_capsule_id
        || instance.revision_id.as_deref() != Some(output_revision_id)
        || instance.icon_asset_id.is_some()
        || instance.cover_asset_id.is_some()
        || instance.created_at != plan.created_at()
        || instance.content_updated_at != plan.created_at()
    {
        return Err(verification_failed());
    }

    let event_count: i64 = connection
        .query_row("SELECT count(*) FROM capsule_lineage_event", [], |row| {
            row.get(0)
        })
        .map_err(|_| verification_failed())?;
    let parent_count: i64 = connection
        .query_row("SELECT count(*) FROM capsule_lineage_parent", [], |row| {
            row.get(0)
        })
        .map_err(|_| verification_failed())?;
    if event_count != 1 || parent_count != 1 {
        return Err(verification_failed());
    }
    let schema = source
        .identity()
        .overview
        .data_schema
        .as_ref()
        .ok_or_else(verification_failed)?;
    let event: (
        String,
        i64,
        String,
        String,
        String,
        String,
        String,
        String,
        i64,
        String,
        String,
    ) = connection
        .query_row(
            "SELECT event_id, sequence, operation, result_capsule_id, \
                 result_revision_id, occurred_at, application_digest, \
                 data_schema_id, data_schema_version, plan_digest, details_json \
                 FROM capsule_lineage_event",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                    row.get(10)?,
                ))
            },
        )
        .map_err(|_| verification_failed())?;
    if event.0 != event_id
        || event.1 != 1
        || event.2 != mode.lineage_operation()
        || event.3 != output_capsule_id
        || event.4 != output_revision_id
        || event.5 != plan.created_at()
        || event.6 != lower_hex(source.application_digest())
        || event.7 != schema.data_schema_id
        || event.8 != schema.data_schema_version
        || event.9 != plan.plan_digest()
        || event.10 != "{}"
    {
        return Err(verification_failed());
    }
    let parent: (String, i64, String, Option<String>, Option<String>, String) = connection
        .query_row(
            "SELECT event_id, ordinal, relation, parent_capsule_id, \
             parent_revision_id, parent_file_sha256 FROM capsule_lineage_parent",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .map_err(|_| verification_failed())?;
    if parent.0 != event_id
        || parent.1 != 1
        || parent.2 != mode.parent_relation()
        || parent.3.as_deref() != Some(source.identity().capsule_id.as_str())
        || parent.4.as_deref() != source.identity().overview.instance.revision_id.as_deref()
        || parent.5 != lower_hex(&source.verified.source_sha256)
    {
        return Err(verification_failed());
    }
    Ok(())
}

fn validate_sequences(connection: &Connection) -> Result<(), WorkspaceError> {
    let exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema \
             WHERE type = 'table' AND name = 'sqlite_sequence')",
            [],
            |row| row.get(0),
        )
        .map_err(|_| verification_failed())?;
    if !exists {
        return if sequence_managed_tables(connection)?.is_empty() {
            Ok(())
        } else {
            Err(verification_failed())
        };
    }
    let expected = sequence_managed_tables(connection)?;
    let entries = {
        let mut statement = connection
            .prepare("SELECT name, seq FROM sqlite_sequence ORDER BY name COLLATE BINARY")
            .map_err(|_| verification_failed())?;
        statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|_| verification_failed())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| verification_failed())?
    };
    if entries.len() != expected.len() {
        return Err(verification_failed());
    }
    let mut actual = BTreeMap::new();
    for (name, sequence) in entries {
        if !expected.contains(&name) || actual.insert(name.clone(), sequence).is_some() {
            return Err(verification_failed());
        }
        let maximum: Option<i64> = connection
            .query_row(
                &format!("SELECT max(rowid) FROM {}", quote_identifier(&name)),
                [],
                |row| row.get(0),
            )
            .map_err(|_| verification_failed())?;
        if sequence != maximum.unwrap_or(0) || sequence < 0 {
            return Err(verification_failed());
        }
    }
    if actual.keys().eq(expected.iter()) {
        Ok(())
    } else {
        Err(verification_failed())
    }
}

fn sequence_managed_tables(connection: &Connection) -> Result<BTreeSet<String>, WorkspaceError> {
    let mut statement = connection
        .prepare(
            "SELECT name, sql FROM sqlite_schema \
             WHERE type = 'table' AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' \
             ORDER BY name COLLATE BINARY",
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

/// Finds an ASCII SQL keyword while ignoring quoted strings/identifiers and
/// comments. SQLite exposes no pragma for the AUTOINCREMENT bit, so this small
/// tokenizer derives the sequence-managed set from authenticated CREATE TABLE
/// SQL without accepting a string literal or comment as authority.
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

fn require_compacted(connection: &Connection) -> Result<(), WorkspaceError> {
    let freelist: i64 = connection
        .pragma_query_value(None, "freelist_count", |row| row.get(0))
        .map_err(|_| verification_failed())?;
    let journal: String = connection
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .map_err(|_| verification_failed())?;
    if freelist == 0 && journal.eq_ignore_ascii_case("delete") {
        Ok(())
    } else {
        Err(verification_failed())
    }
}

fn validate_shape(plan: &LifecyclePlan) -> Result<(), WorkspaceError> {
    if !matches!(
        plan.operation(),
        Operation::Fork | Operation::CreateFromTemplate | Operation::SelectiveFork
    ) || plan.inputs().len() != 1
        || plan.inputs()[0].role() != InputRole::Source
        || plan.inputs()[0].capsule().format_version() != "0.3"
        || plan.inputs()[0].capsule().publisher_key_id().is_some()
        || plan.decisions().is_empty()
        || plan.decisions().len() > HARD_MAX_DATASETS + 1
        || plan.decisions()[0].scope() != DecisionScope::Application
        || plan.decisions()[0].action() != SEMANTIC_ACTION
        || plan.decisions()[0].subject() != plan.expected().app_id()
        || plan_limit_u64(plan, "max_rows_inspected")? > HARD_MAX_ROWS
        || plan_limit_u64(plan, "max_rows_written")? > HARD_MAX_ROWS
    {
        return Err(invalid_contract());
    }
    let mode = mode_from_operation(plan.operation());
    let value = serde_json::to_value(plan).map_err(|_| invalid_contract())?;
    let decisions = value["decisions"].as_array().ok_or_else(invalid_contract)?;
    let application = decisions[0]["parameters"]
        .as_object()
        .ok_or_else(invalid_contract)?;
    if application.len() != 8
        || application.get("mode").and_then(|value| value.as_str()) != Some(mode.operation_name())
        || application
            .get("event_id")
            .and_then(|value| value.as_str())
            .is_none_or(|value| !valid_uuid_v4(value))
        || application
            .get("occurred_at")
            .and_then(|value| value.as_str())
            != Some(plan.created_at())
        || application
            .get("mutable_platform_profile")
            .and_then(|value| value.as_str())
            != Some(MUTABLE_PLATFORM_PROFILE)
        || application
            .get("instance_profile")
            .and_then(|value| value.as_str())
            != Some(mode.instance_profile())
        || application
            .get("signature_count")
            .and_then(|value| value.as_u64())
            .is_none_or(|count| count == 0 || count > sqlite_capsule_crypto::MAX_SIGNATURES as u64)
        || application
            .get("signature_inventory_sha256")
            .and_then(|value| value.as_str())
            .is_none_or(|value| !valid_sha256(value))
        || !matches!(
            application.get("template_state_sha256"),
            Some(serde_json::Value::Null | serde_json::Value::String(_))
        )
        || application
            .get("template_state_sha256")
            .and_then(|value| value.as_str())
            .is_some_and(|value| !valid_sha256(value))
        || plan
            .expected()
            .capsule_id()
            .is_none_or(|value| !valid_uuid_v4(value))
        || plan
            .expected()
            .revision_id()
            .is_none_or(|value| !valid_uuid_v4(value))
    {
        return Err(invalid_contract());
    }
    let mut subjects = BTreeSet::new();
    for (typed, value) in plan.decisions()[1..].iter().zip(&decisions[1..]) {
        let parameters = value["parameters"]
            .as_object()
            .ok_or_else(invalid_contract)?;
        if typed.scope() != DecisionScope::Dataset
            || !matches!(typed.action(), "copy" | "omit" | "reset")
            || !subjects.insert(typed.subject())
            || parameters.len() != 6
            || !matches!(
                parameters
                    .get("fork_policy")
                    .and_then(|value| value.as_str()),
                Some("copy" | "reset" | "omit" | "prompt" | "forbid")
            )
            || !matches!(
                parameters
                    .get("sensitivity")
                    .and_then(|value| value.as_str()),
                Some("normal" | "sensitive")
            )
            || parameters
                .get("sensitive_confirmed")
                .and_then(|value| value.as_bool())
                .is_none()
            || parameters
                .get("source_row_count")
                .and_then(|value| value.as_u64())
                .is_none()
            || parameters
                .get("source_state_profile")
                .and_then(|value| value.as_str())
                != Some(crate::template_state::DATASET_STATE_PROFILE)
            || parameters
                .get("source_state_sha256")
                .and_then(|value| value.as_str())
                .is_none_or(|value| !valid_sha256(value))
        {
            return Err(invalid_contract());
        }
    }
    Ok(())
}

fn bind_source(
    plan: &LifecyclePlan,
    source: &VerifiedWorkspaceSource,
    signature_inventory_sha256_expected: &str,
) -> Result<(), WorkspaceError> {
    let input = &plan.inputs()[0];
    let capsule = input.capsule();
    let identity = source.identity();
    let instance = &identity.overview.instance;
    let schema = identity
        .overview
        .data_schema
        .as_ref()
        .ok_or_else(invalid_contract)?;
    let live = source.source_identity();
    let source_sha256 = lower_hex(&source.verified.source_sha256);
    let app_digest = lower_hex(source.application_digest());
    let expected = plan.expected();
    let value = serde_json::to_value(plan).map_err(|_| invalid_contract())?;
    let application = &value["decisions"][0]["parameters"];
    if input.path_hint() != utf8_path(&identity.canonical_path)?
        || input.file_sha256() != source_sha256
        || input.snapshot_sha256() != source_sha256
        || input.size_bytes() != live.bytes
        || input.filesystem_identity().platform() != std::env::consts::OS
        || input.filesystem_identity().volume_or_device() != live.device.to_string()
        || input.filesystem_identity().file_id_or_inode() != live.stable_file_id
        || input.filesystem_identity().modified_ns() != live.modified_ns
        || capsule.format_version() != "0.3"
        || capsule.capsule_id() != Some(identity.capsule_id.as_str())
        || capsule.revision_id() != instance.revision_id.as_deref()
        || capsule.app_id() != identity.app_id
        || capsule.app_version() != identity.app_version
        || capsule.application_digest() != Some(app_digest.as_str())
        || capsule.data_schema_id() != Some(schema.data_schema_id.as_str())
        || capsule.data_schema_version() != u64::try_from(schema.data_schema_version).ok()
        || expected.app_id() != identity.app_id
        || expected.application_digest() != Some(app_digest.as_str())
        || expected.data_schema_id() != Some(schema.data_schema_id.as_str())
        || expected.data_schema_version() != u64::try_from(schema.data_schema_version).ok()
        || application["signature_inventory_sha256"].as_str()
            != Some(signature_inventory_sha256_expected)
        || application["signature_count"].as_u64() != Some(source.signature_reports().len() as u64)
    {
        return Err(stale_plan());
    }
    Ok(())
}

fn bind_parent(
    plan: &LifecyclePlan,
    destination: &DestinationReservation,
) -> Result<(), WorkspaceError> {
    let expected = plan.output().parent_identity();
    let actual = destination.identity();
    if plan.output().path() != utf8_path(&destination.path_hint())?
        || plan.output().leaf_name() != destination.leaf().to_str().unwrap_or_default()
        || expected.platform() != std::env::consts::OS
        || expected.volume_or_device() != actual.device.to_string()
        || expected.file_id_or_inode() != actual.stable_file_id
    {
        return Err(stale_plan());
    }
    Ok(())
}

fn validate_decisions_bound(
    plan: &LifecyclePlan,
    source: &VerifiedWorkspaceSource,
    decisions: &[SemanticDatasetDecision],
) -> Result<(), WorkspaceError> {
    if plan.decisions().len() != decisions.len() + 1
        || decisions.len() != source.data_contract().datasets.len()
    {
        return Err(stale_plan());
    }
    let value = serde_json::to_value(plan).map_err(|_| invalid_contract())?;
    for ((plan_decision, parameters), (expected, dataset)) in plan.decisions()[1..]
        .iter()
        .zip(
            value["decisions"].as_array().ok_or_else(invalid_contract)?[1..]
                .iter()
                .map(|decision| {
                    decision["parameters"]
                        .as_object()
                        .ok_or_else(invalid_contract)
                }),
        )
        .zip(decisions.iter().zip(&source.data_contract().datasets))
    {
        let parameters = parameters?;
        if plan_decision.subject() != expected.dataset_id
            || plan_decision.action() != expected.action.name()
            || expected.dataset_id != dataset.id
            || parameters["fork_policy"].as_str() != Some(fork_policy_name(dataset.fork))
            || parameters["sensitivity"].as_str() != Some(sensitivity_name(dataset.sensitivity))
            || parameters["sensitive_confirmed"].as_bool() != Some(expected.sensitive_confirmed)
            || parameters["source_row_count"].as_u64() != Some(expected.source_row_count)
            || parameters["source_state_profile"].as_str() != Some(expected.source_state_profile)
            || parameters["source_state_sha256"].as_str()
                != Some(expected.source_state_sha256.as_str())
        {
            return Err(stale_plan());
        }
    }
    Ok(())
}

fn require_complete_signature_inventory(
    source: &VerifiedWorkspaceSource,
) -> Result<(), WorkspaceError> {
    if source.signature_reports().is_empty()
        || source
            .signature_reports()
            .iter()
            .any(|report| !report.cryptographically_valid || !report.digest_matches)
    {
        Err(WorkspaceError::new(WorkspaceErrorCode::InvalidSignature))
    } else {
        Ok(())
    }
}

fn signature_inventory_sha256(source: &VerifiedWorkspaceSource) -> Result<String, WorkspaceError> {
    let connection = source.verified.connection();
    let mut statement = connection
        .prepare(
            "SELECT key_id, algorithm, public_key, application_digest, signature, signed_at \
             FROM capsule_signature ORDER BY key_id COLLATE BINARY",
        )
        .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InvalidSignature))?;
    let mut rows = statement
        .query([])
        .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InvalidSignature))?;
    let mut count = 0_usize;
    let mut hasher = Sha256::new();
    hasher.update(b"SQLite Capsule exact signature inventory v1\0");
    while let Some(row) = rows
        .next()
        .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InvalidSignature))?
    {
        count = count.checked_add(1).ok_or_else(limit_exceeded)?;
        if count > sqlite_capsule_crypto::MAX_SIGNATURES {
            return Err(WorkspaceError::new(WorkspaceErrorCode::InvalidSignature));
        }
        for index in 0..6 {
            let value = row
                .get_ref(index)
                .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InvalidSignature))?;
            let bytes = match value {
                ValueRef::Text(bytes) | ValueRef::Blob(bytes) => bytes,
                _ => return Err(WorkspaceError::new(WorkspaceErrorCode::InvalidSignature)),
            };
            hasher.update((bytes.len() as u64).to_be_bytes());
            hasher.update(bytes);
        }
    }
    if count != source.signature_reports().len() {
        return Err(WorkspaceError::new(WorkspaceErrorCode::InvalidSignature));
    }
    Ok(lower_hex(&hasher.finalize()))
}

fn proof_sha256(proof: &TemplateStateProof) -> Result<String, WorkspaceError> {
    let value = serde_json::to_value(proof).map_err(|_| invalid_contract())?;
    let bytes = crate::plan::canonical_json(&value).map_err(|_| invalid_contract())?;
    Ok(lower_hex(&Sha256::digest(bytes)))
}

fn generate_distinct_ids(
    connection: &Connection,
    source_capsule_id: &str,
    source_revision_id: &str,
) -> Result<(String, String, String), WorkspaceError> {
    for _ in 0..8 {
        let capsule = generate_uuid_v4(connection)?;
        let revision = generate_uuid_v4(connection)?;
        let event = generate_uuid_v4(connection)?;
        if capsule != source_capsule_id
            && capsule != revision
            && capsule != event
            && revision != source_revision_id
            && revision != event
        {
            return Ok((capsule, revision, event));
        }
    }
    Err(WorkspaceError::new(WorkspaceErrorCode::InternalError))
}

fn generate_uuid_v4(connection: &Connection) -> Result<String, WorkspaceError> {
    let mut bytes: Vec<u8> = connection
        .query_row("SELECT randomblob(16)", [], |row| row.get(0))
        .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InternalError))?;
    if bytes.len() != 16 {
        return Err(WorkspaceError::new(WorkspaceErrorCode::InternalError));
    }
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

fn open_output_bound(
    path: &Path,
    expected_size: u64,
    expected_sha256: [u8; 32],
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<VerifiedWorkspaceSource, WorkspaceError> {
    VerifiedWorkspaceSource::open_with_control_expected_binding(
        path,
        &WorkspaceLimits {
            deadline: remaining(deadline)?,
            ..WorkspaceLimits::default()
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
        &WorkspaceLimits {
            deadline: remaining(deadline)?,
            ..WorkspaceLimits::default()
        },
        cancellation,
    )
}

fn assert_source_current(
    source: &VerifiedWorkspaceSource,
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<(), WorkspaceError> {
    source.assert_current_with_control(
        &WorkspaceLimits {
            deadline: remaining(deadline)?,
            ..WorkspaceLimits::default()
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
        copied = copied
            .checked_add(read as u64)
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

fn require_same_object(
    expected: &SourceIdentity,
    actual: &SourceIdentity,
) -> Result<(), WorkspaceError> {
    let same = expected.device == actual.device
        && if expected.stable_file_id.is_empty() || actual.stable_file_id.is_empty() {
            expected.file == actual.file
        } else {
            expected.stable_file_id == actual.stable_file_id
        };
    if same && expected.bytes == actual.bytes {
        Ok(())
    } else {
        Err(verification_failed())
    }
}

fn install_progress(
    connection: &Connection,
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<(), WorkspaceError> {
    let cancelled_flag = cancellation.shared_flag();
    connection
        .progress_handler(
            1_000,
            Some(move || cancelled_flag.load(Ordering::Relaxed) || Instant::now() >= deadline),
        )
        .map_err(|_| output_failed())
}

fn verification_control(
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<sqlite_capsule_launch::VerificationControl, WorkspaceError> {
    Ok(sqlite_capsule_launch::VerificationControl::new(
        remaining(deadline)?,
        cancellation.shared_flag(),
    ))
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

fn plan_limit_u64(plan: &LifecyclePlan, name: &str) -> Result<u64, WorkspaceError> {
    serde_json::to_value(plan)
        .map_err(|_| invalid_contract())?
        .get("limits")
        .and_then(|limits| limits.get(name))
        .and_then(|value| value.as_u64())
        .ok_or_else(invalid_contract)
}

fn mode_from_operation(operation: Operation) -> SemanticCopyMode {
    match operation {
        Operation::Fork => SemanticCopyMode::Fork,
        Operation::CreateFromTemplate => SemanticCopyMode::CreateFromTemplate,
        Operation::SelectiveFork => SemanticCopyMode::SelectiveFork,
        _ => unreachable!("validated semantic operation"),
    }
}

fn fork_policy_name(policy: ForkPolicy) -> &'static str {
    match policy {
        ForkPolicy::Copy => "copy",
        ForkPolicy::Reset => "reset",
        ForkPolicy::Omit => "omit",
        ForkPolicy::Prompt => "prompt",
        ForkPolicy::Forbid => "forbid",
    }
}

fn sensitivity_name(sensitivity: Sensitivity) -> &'static str {
    match sensitivity {
        Sensitivity::Normal => "normal",
        Sensitivity::Sensitive => "sensitive",
    }
}

fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn valid_uuid_v4(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && [8, 13, 18, 23]
            .into_iter()
            .all(|index| bytes[index] == b'-')
        && bytes[14] == b'4'
        && matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
        && bytes.iter().enumerate().all(|(index, byte)| {
            [8, 13, 18, 23].contains(&index)
                || byte.is_ascii_digit()
                || (b'a'..=b'f').contains(byte)
        })
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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

fn utf8_path(path: &Path) -> Result<String, WorkspaceError> {
    path.to_str()
        .filter(|value| !value.is_empty() && value.len() <= 4_096)
        .map(str::to_owned)
        .ok_or_else(invalid_contract)
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn map_launch_output_error(error: sqlite_capsule_launch::LaunchError) -> WorkspaceError {
    match error {
        sqlite_capsule_launch::LaunchError::Cancelled => cancelled(),
        sqlite_capsule_launch::LaunchError::LimitExceeded => limit_exceeded(),
        sqlite_capsule_launch::LaunchError::SourceRace => stale_plan(),
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

fn remaining(deadline: Instant) -> Result<Duration, WorkspaceError> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        Err(limit_exceeded())
    } else {
        Ok(remaining)
    }
}

#[cfg(test)]
fn maybe_crash(stage: &str) {
    if std::env::var_os("SQLITE_CAPSULE_SEMANTIC_CRASH_STAGE").is_some_and(|value| value == stage) {
        std::process::exit(97);
    }
}

#[cfg(not(test))]
const fn maybe_crash(_stage: &str) {}

const fn unsupported_operation() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::UnsupportedOperation)
}

const fn sensitive_confirmation_required() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::SensitiveConfirmationRequired)
}

const fn invalid_contract() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::InvalidContract)
}

const fn stale_plan() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::StalePlan)
}

const fn cancelled() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::Cancelled)
}

const fn limit_exceeded() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::LimitExceeded)
}

const fn verification_failed() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::VerificationFailed)
}

const fn output_failed() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::OutputPublishFailed)
}

#[cfg(test)]
mod tests {
    use std::{fs, process::Command, time::UNIX_EPOCH};

    use super::*;
    use ed25519_dalek::SigningKey;
    use rusqlite::Connection;
    use sqlite_capsule_crypto::{PROFILE_V03, application_digest, sign_digest_for_profile};

    const CREATED: &str = "2026-08-12T00:00:00Z";
    // Public stage/publish transitions use the real production clock. This
    // fixture must stay valid regardless of the day the operation tests run;
    // expiry boundaries are covered by dedicated injected-clock tests.
    const EXPIRES: &str = "9999-12-31T23:59:59Z";

    fn operation_time() -> SystemTime {
        UNIX_EPOCH
            + Duration::from_secs(
                crate::prepared_plan::parse_utc_seconds(CREATED).expect("created") + 60,
            )
    }

    fn request<'a>(
        output: &'a Path,
        mode: SemanticCopyMode,
        choices: &'a [SemanticDatasetChoice],
    ) -> SemanticCopyPlanRequest<'a> {
        SemanticCopyPlanRequest {
            output_path: output,
            plan_id: "95a669c4-1832-4311-9d97-4d681ab8c2ac",
            created_at: CREATED,
            expires_at: EXPIRES,
            mode,
            choices,
            deadline: Duration::from_secs(30),
            max_output_bytes: sqlite_capsule_core::MAX_CAPSULE_BYTES,
            max_rows: HARD_MAX_ROWS,
            max_stream_bytes: HARD_MAX_STREAM_BYTES,
        }
    }

    fn prepare_at(
        source: VerifiedWorkspaceSource,
        output: &Path,
        mode: SemanticCopyMode,
        choices: &[SemanticDatasetChoice],
        cancellation: &CancellationToken,
    ) -> PreparedSemanticCopy {
        let review =
            generate_semantic_copy_plan(&source, &request(output, mode, choices), cancellation)
                .expect("semantic review");
        let plan = parse_semantic_copy_plan(
            &review
                .plan()
                .canonical_bytes()
                .expect("canonical review plan"),
        )
        .expect("approved semantic plan");
        PreparedSemanticCopy::prepare_at(
            review,
            plan,
            source,
            operation_time(),
            &WorkspaceLimits::default(),
            cancellation,
        )
        .expect("prepared semantic copy")
    }

    fn resign(connection: &Connection) {
        const DEVELOPMENT_SEED: &str =
            include_str!("../../../../compatibility/signed-app-v0.2/development-seed.hex");
        connection
            .execute("DELETE FROM capsule_signature", [])
            .expect("clear signature");
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
                params![
                    envelope.key_id,
                    envelope.public_key.as_slice(),
                    envelope.application_digest.as_slice(),
                    envelope.signature.as_slice(),
                    envelope.signed_at,
                ],
            )
            .expect("store signature");
    }

    fn install_template_proof(path: &Path) {
        let source = VerifiedWorkspaceSource::open(path).expect("source before proof");
        let identity = source.identity();
        let schema = identity.overview.data_schema.as_ref().expect("data schema");
        let datasets = source
            .data_contract()
            .datasets
            .iter()
            .map(|dataset| {
                let (count, digest) =
                    crate::template_state::dataset_state_for_test(&source, dataset)
                        .expect("dataset state");
                json!({
                    "dataset_id": dataset.id,
                    "disposition": if count == 0 { "empty" } else { "seed" },
                    "stored_row_count": count,
                    "state_sha256": digest
                })
            })
            .collect::<Vec<_>>();
        let proof = json!({
            "profile": crate::TEMPLATE_STATE_PROFILE,
            "app_id": identity.app_id,
            "app_version": identity.app_version,
            "data_schema_id": schema.data_schema_id,
            "data_schema_version": schema.data_schema_version,
            "dataset_state_profile": crate::DATASET_STATE_PROFILE,
            "mutable_platform_state_profile": crate::TEMPLATE_PLATFORM_RESET_PROFILE,
            "datasets": datasets
        });
        let proof = String::from_utf8(
            crate::plan::canonical_json(&proof).expect("canonical template proof"),
        )
        .expect("proof UTF-8");
        drop(source);
        let connection = Connection::open(path).expect("open source to install proof");
        connection
            .execute(
                "INSERT OR REPLACE INTO capsule_doc \
                 (slug, title, media_type, content, sequence) VALUES \
                 (?1, 'SQLite Capsule authenticated template state', \
                  'application/vnd.sqlite-capsule.template-state+json', ?2, 0)",
                params![crate::TEMPLATE_STATE_DOC_SLUG, proof],
            )
            .expect("install proof");
        resign(&connection);
    }

    #[test]
    fn fork_publishes_new_ids_preserves_signed_application_and_source() {
        let (directory, source_path) = crate::tests::signed_fixture("semantic-fork-success");
        let output = directory.path().join("forked.sqlitecapsule");
        let source_before = fs::read(&source_path).expect("source bytes");
        let source = VerifiedWorkspaceSource::open(&source_path).expect("workspace source");
        let source_capsule = source.identity().capsule_id.clone();
        let source_revision = source
            .identity()
            .overview
            .instance
            .revision_id
            .clone()
            .expect("source revision");
        let source_digest = *source.application_digest();
        let cancellation = CancellationToken::new();
        let published = prepare_at(source, &output, SemanticCopyMode::Fork, &[], &cancellation)
            .stage()
            .expect("stage")
            .transform_and_validate()
            .expect("transform and validate")
            .publish()
            .expect("publish");
        assert_eq!(published.path(), output);
        let reopened = VerifiedWorkspaceSource::open(&output).expect("published workspace source");
        assert_ne!(reopened.identity().capsule_id, source_capsule);
        assert_ne!(
            reopened.identity().overview.instance.revision_id.as_deref(),
            Some(source_revision.as_str())
        );
        assert_eq!(*reopened.application_digest(), source_digest);
        assert_eq!(fs::read(&source_path).expect("source after"), source_before);
        assert_eq!(
            reopened
                .verified
                .connection()
                .query_row("SELECT count(*) FROM capsule_grant", [], |row| row
                    .get::<_, i64>(0))
                .expect("grant count"),
            0
        );
    }

    #[test]
    fn template_uses_only_signed_app_defaults_and_scrubs_mutable_sentinels() {
        const SENTINEL: &str = "TEMPLATE-MUTABLE-SECRET-a9b73f6e";
        let (directory, source_path) = crate::tests::signed_fixture("semantic-template");
        let connection = Connection::open(&source_path).expect("open mutable fixture");
        let sentinel_sha = lower_hex(&Sha256::digest(SENTINEL.as_bytes()));
        connection
            .execute(
                "INSERT INTO capsule_instance_asset \
                 (id, media_type, content, sha256, width, height, description) \
                 VALUES ('secret', 'image/png', CAST(?1 AS BLOB), ?2, 1, 1, ?1)",
                params![SENTINEL, sentinel_sha],
            )
            .expect("secret instance asset");
        connection
            .execute(
                "UPDATE capsule_instance SET title=?1, description=?1, \
                 document_kind=?1, tags_json=json_array(?1), \
                 icon_asset_id='secret', cover_asset_id='secret' WHERE id=1",
                [SENTINEL],
            )
            .expect("secret instance metadata");
        drop(connection);
        install_template_proof(&source_path);
        let source_before = fs::read(&source_path).expect("source bytes");
        let output = directory.path().join("from-template.sqlitecapsule");
        let source = VerifiedWorkspaceSource::open(&source_path).expect("template source");
        verify_template_state(
            &source,
            &TemplateStateLimits::default(),
            &CancellationToken::new(),
        )
        .expect("installed template proof verifies");
        let signed_name = source.identity().overview.application.name.clone();
        let cancellation = CancellationToken::new();
        prepare_at(
            source,
            &output,
            SemanticCopyMode::CreateFromTemplate,
            &[],
            &cancellation,
        )
        .stage()
        .expect("stage")
        .transform_and_validate()
        .expect("template validation")
        .publish()
        .expect("publish template");
        let reopened = VerifiedWorkspaceSource::open(&output).expect("template output");
        let instance = &reopened.identity().overview.instance;
        assert_eq!(instance.title, signed_name);
        assert!(instance.description.is_empty());
        assert_eq!(instance.document_kind, "capsule");
        assert!(instance.tags.is_empty());
        assert!(instance.icon_asset_id.is_none());
        assert!(instance.cover_asset_id.is_none());
        assert!(
            !fs::read(&output)
                .expect("output bytes")
                .windows(SENTINEL.len())
                .any(|window| window == SENTINEL.as_bytes()),
            "VACUUMed template output must not retain mutable sentinel bytes"
        );
        assert_eq!(fs::read(&source_path).expect("source after"), source_before);
    }

    #[test]
    fn selective_sensitive_prompt_defaults_omit_and_dependency_closure_blocks_gap() {
        const SENTINEL: &str = "SELECTIVE-SENSITIVE-SECRET-81d2";
        let (directory, source_path) = crate::tests::signed_fixture("semantic-selective");
        let connection = Connection::open(&source_path).expect("selective fixture");
        connection
            .execute(
                "UPDATE capsule_dataset SET fork_policy='prompt', \
                 sensitivity='sensitive', required=0 WHERE id='content'",
                [],
            )
            .expect("sensitive prompt policy");
        resign(&connection);
        connection
            .execute(
                "UPDATE vector_domain SET note=?1, payload=?1 WHERE id='domain'",
                [SENTINEL],
            )
            .expect("sensitive sentinel");
        let sentinel_sha = lower_hex(&Sha256::digest(SENTINEL.as_bytes()));
        connection
            .execute(
                "INSERT INTO capsule_instance_asset \
                 (id, media_type, content, sha256, width, height, description) \
                 VALUES ('selective-secret', 'image/png', CAST(?1 AS BLOB), ?2, 1, 1, ?1)",
                params![SENTINEL, sentinel_sha],
            )
            .expect("selective secret instance asset");
        connection
            .execute(
                "UPDATE capsule_instance SET title=?1, description=?1, \
                 document_kind=?1, tags_json=json_array(?1), \
                 icon_asset_id='selective-secret', cover_asset_id='selective-secret' \
                 WHERE id=1",
                [SENTINEL],
            )
            .expect("selective secret instance metadata");
        drop(connection);
        let source_before = fs::read(&source_path).expect("selective source bytes");
        let output = directory.path().join("selective.sqlitecapsule");
        let source = VerifiedWorkspaceSource::open(&source_path).expect("selective source");
        let signed_name = source.identity().overview.application.name.clone();
        let cancellation = CancellationToken::new();
        let review = generate_semantic_copy_plan(
            &source,
            &request(&output, SemanticCopyMode::SelectiveFork, &[]),
            &cancellation,
        )
        .expect("selective review");
        assert_eq!(review.preview().instance_text, "signed-app-defaults");
        let reviewed_value = serde_json::to_value(review.plan()).expect("reviewed plan value");
        assert_eq!(
            reviewed_value["decisions"][0]["parameters"]["instance_profile"],
            SELECTIVE_INSTANCE_PROFILE
        );
        let approved = parse_semantic_copy_plan(
            &review
                .plan()
                .canonical_bytes()
                .expect("selective plan bytes"),
        )
        .expect("approved selective plan");
        PreparedSemanticCopy::prepare_at(
            review,
            approved,
            source,
            operation_time(),
            &WorkspaceLimits::default(),
            &cancellation,
        )
        .expect("prepared selective copy")
        .stage()
        .expect("stage")
        .transform_and_validate()
        .expect("selective validation")
        .publish()
        .expect("selective publish");
        let output_connection = Connection::open(&output).expect("open selective output");
        assert_eq!(
            output_connection
                .query_row("SELECT count(*) FROM vector_domain", [], |row| row
                    .get::<_, i64>(0))
                .expect("domain count"),
            0
        );
        assert_eq!(
            output_connection
                .query_row("SELECT count(*) FROM vector_settings", [], |row| row
                    .get::<_, i64>(0))
                .expect("settings count"),
            1
        );
        assert_eq!(
            output_connection
                .pragma_query_value(None, "freelist_count", |row| row.get::<_, i64>(0))
                .expect("freelist"),
            0
        );
        drop(output_connection);
        let reopened = VerifiedWorkspaceSource::open(&output).expect("selective output");
        let instance = &reopened.identity().overview.instance;
        assert_eq!(instance.title, signed_name);
        assert!(instance.description.is_empty());
        assert_eq!(instance.document_kind, "capsule");
        assert!(instance.tags.is_empty());
        assert!(instance.icon_asset_id.is_none());
        assert!(instance.cover_asset_id.is_none());
        assert!(
            !fs::read(&output)
                .expect("selective bytes")
                .windows(SENTINEL.len())
                .any(|window| window == SENTINEL.as_bytes())
        );
        assert_eq!(
            fs::read(&source_path).expect("selective source after"),
            source_before
        );

        let (directory, path) = crate::tests::signed_fixture("semantic-dependency-gap");
        let connection = Connection::open(&path).expect("dependency fixture");
        connection
            .execute(
                "UPDATE capsule_dataset SET fork_policy='prompt', required=0 \
                 WHERE id='settings'",
                [],
            )
            .expect("omittable dependency");
        resign(&connection);
        drop(connection);
        let source = VerifiedWorkspaceSource::open(&path).expect("dependency source");
        let blocked_output = directory.path().join("blocked.sqlitecapsule");
        let error = match generate_semantic_copy_plan(
            &source,
            &request(&blocked_output, SemanticCopyMode::SelectiveFork, &[]),
            &CancellationToken::new(),
        ) {
            Ok(_) => panic!("present dependent with omitted prerequisite must block"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::InvalidContract);
    }

    #[test]
    fn edited_recomputed_plan_never_remints_authority_and_basic_races_fail_closed() {
        let (directory, source_path) = crate::tests::signed_fixture("semantic-edited-plan");
        let output = directory.path().join("edited.sqlitecapsule");
        let source = VerifiedWorkspaceSource::open(&source_path).expect("source");
        let review = generate_semantic_copy_plan(
            &source,
            &request(&output, SemanticCopyMode::Fork, &[]),
            &CancellationToken::new(),
        )
        .expect("review");
        let mut edited = serde_json::to_value(review.plan()).expect("plan value");
        edited["decisions"][1]["parameters"]["source_row_count"] = json!(999);
        edited["plan_digest"] = json!(canonical_digest_value(&edited).expect("edited digest"));
        let edited = LifecyclePlan::parse(
            &serde_json::to_vec(&edited).expect("serialize edited recomputed plan"),
        )
        .expect("syntactically valid recomputed plan");
        let error = match PreparedSemanticCopy::prepare_at(
            review,
            edited,
            source,
            operation_time(),
            &WorkspaceLimits::default(),
            &CancellationToken::new(),
        ) {
            Ok(_) => panic!("edited review must not gain destination authority"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::StalePlan);
        assert!(!output.exists());

        let existing = directory.path().join("existing.sqlitecapsule");
        fs::write(&existing, b"occupied").expect("existing destination");
        let source = VerifiedWorkspaceSource::open(&source_path).expect("source again");
        let error = match generate_semantic_copy_plan(
            &source,
            &request(&existing, SemanticCopyMode::Fork, &[]),
            &CancellationToken::new(),
        ) {
            Ok(_) => panic!("existing destination must block"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::DestinationExists);

        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let cancelled_output = directory.path().join("cancelled.sqlitecapsule");
        let source = VerifiedWorkspaceSource::open(&source_path).expect("source for cancel");
        let error = match generate_semantic_copy_plan(
            &source,
            &request(&cancelled_output, SemanticCopyMode::Fork, &[]),
            &cancellation,
        ) {
            Ok(_) => panic!("cancelled generation must block"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::Cancelled);
    }

    #[test]
    fn source_race_and_late_cancellation_never_report_publication_success() {
        let (directory, source_path) = crate::tests::signed_fixture("semantic-source-race");
        let output = directory.path().join("raced.sqlitecapsule");
        let source = VerifiedWorkspaceSource::open(&source_path).expect("source");
        let review = generate_semantic_copy_plan(
            &source,
            &request(&output, SemanticCopyMode::Fork, &[]),
            &CancellationToken::new(),
        )
        .expect("review");
        let plan = parse_semantic_copy_plan(&review.plan().canonical_bytes().expect("plan bytes"))
            .expect("approved plan");
        fs::OpenOptions::new()
            .append(true)
            .open(&source_path)
            .expect("open raced source")
            .write_all(b"race")
            .expect("mutate source");
        let error = match PreparedSemanticCopy::prepare_at(
            review,
            plan,
            source,
            operation_time(),
            &WorkspaceLimits::default(),
            &CancellationToken::new(),
        ) {
            Ok(_) => panic!("source race must block"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::StalePlan);
        assert!(!output.exists());

        let (directory, source_path) = crate::tests::signed_fixture("semantic-late-cancel");
        let output = directory.path().join("late-cancel.sqlitecapsule");
        let cancellation = CancellationToken::new();
        let cancel_in_callback = cancellation.clone();
        let source = VerifiedWorkspaceSource::open(&source_path).expect("late source");
        let validated = prepare_at(source, &output, SemanticCopyMode::Fork, &[], &cancellation)
            .stage()
            .expect("stage")
            .transform_and_validate()
            .expect("validated");
        let error = match validated.publish_with_hook(move || cancel_in_callback.cancel()) {
            Ok(_) => panic!("late cancellation must not report success"),
            Err(error) => error,
        };
        assert_eq!(
            error.kind(),
            WorkspaceErrorCode::PostpublishVerificationFailed
        );
    }

    #[test]
    fn v02_is_unsupported_operation_and_mixed_signature_inventory_is_rejected() {
        let (_directory, v02_path) =
            crate::copy_source::tests::fixture("semantic-v02-unsupported", 2, true);
        let error = match open_semantic_copy_source(
            &v02_path,
            &WorkspaceLimits::default(),
            &CancellationToken::new(),
        ) {
            Ok(_) => panic!("v0.2 semantic source must be unsupported"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::UnsupportedOperation);

        let (directory, path) = crate::tests::signed_fixture("semantic-mixed-signature");
        let connection = Connection::open(&path).expect("mixed signature fixture");
        connection
            .execute(
                "INSERT INTO capsule_signature \
                 (key_id, algorithm, public_key, application_digest, signature, signed_at) \
                 VALUES ('ed25519:sha256:0000000000000000000000000000000000000000000000000000000000000000', \
                         'ed25519', zeroblob(32), zeroblob(32), zeroblob(64), \
                         '2026-08-08T12:34:56Z')",
                [],
            )
            .expect("invalid second signature");
        drop(connection);
        let source = VerifiedWorkspaceSource::open(&path)
            .expect("base workspace admits at least one valid envelope");
        let output = directory.path().join("must-not-plan.sqlitecapsule");
        let error = match generate_semantic_copy_plan(
            &source,
            &request(&output, SemanticCopyMode::Fork, &[]),
            &CancellationToken::new(),
        ) {
            Ok(_) => panic!("mixed signature inventory must fail closed"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::InvalidSignature);
    }

    #[test]
    fn reset_policy_fork_and_selective_require_a_separate_clean_source() {
        for mode in [SemanticCopyMode::Fork, SemanticCopyMode::SelectiveFork] {
            let (directory, path) =
                crate::tests::signed_fixture(&format!("semantic-reset-{}", mode.operation_name()));
            let connection = Connection::open(&path).expect("reset-policy fixture");
            connection
                .execute(
                    "UPDATE capsule_dataset SET fork_policy='reset' WHERE id='content'",
                    [],
                )
                .expect("signed reset policy");
            resign(&connection);
            drop(connection);
            let source_before = fs::read(&path).expect("reset source bytes");
            let source = VerifiedWorkspaceSource::open(&path).expect("reset source");
            let output = directory
                .path()
                .join(format!("reset-{}.sqlitecapsule", mode.operation_name()));
            let error = match generate_semantic_copy_plan(
                &source,
                &request(&output, mode, &[]),
                &CancellationToken::new(),
            ) {
                Ok(_) => panic!("one-source reset must remain unavailable"),
                Err(error) => error,
            };
            assert_eq!(error.kind(), WorkspaceErrorCode::UnsupportedOperation);
            assert!(!output.exists());
            assert_eq!(fs::read(&path).expect("reset source after"), source_before);
        }
    }

    #[test]
    fn template_forbid_policy_is_unsupported_before_destination_or_mutation() {
        let (directory, path) = crate::tests::signed_fixture("semantic-template-forbid");
        let connection = Connection::open(&path).expect("forbid-policy fixture");
        connection
            .execute(
                "UPDATE capsule_dataset SET fork_policy='forbid' WHERE id='content'",
                [],
            )
            .expect("signed forbid policy");
        resign(&connection);
        drop(connection);
        let source_before = fs::read(&path).expect("forbid source bytes");
        let source = VerifiedWorkspaceSource::open(&path).expect("forbid source");
        let output = directory.path().join("forbidden-template.sqlitecapsule");
        let error = match generate_semantic_copy_plan(
            &source,
            &request(&output, SemanticCopyMode::CreateFromTemplate, &[]),
            &CancellationToken::new(),
        ) {
            Ok(_) => panic!("template must honor signed forbid policy"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::UnsupportedOperation);
        assert!(!output.exists());
        assert_eq!(fs::read(&path).expect("forbid source after"), source_before);
    }

    #[test]
    fn semantic_plan_vector_is_canonical_and_binds_dataset_state_profile() {
        let bytes =
            include_bytes!("../../../../compatibility/semantic-copy-plan-v1/vector-plan.json");
        let plan = parse_semantic_copy_plan(bytes).expect("frozen semantic lifecycle plan");
        assert_eq!(plan.operation(), Operation::Fork);
        assert_eq!(plan.decisions()[0].action(), SEMANTIC_ACTION);
        let value = serde_json::to_value(&plan).expect("semantic plan projection");
        assert_eq!(
            value["decisions"][1]["parameters"]["source_state_profile"],
            crate::template_state::DATASET_STATE_PROFILE
        );
        assert_eq!(plan.canonical_bytes().expect("canonical plan"), bytes);
    }

    #[test]
    fn semantic_copy_rebuilds_sequence_from_exact_schema_allowlist() {
        assert!(contains_sql_keyword(
            "CREATE TABLE x(id INTEGER PRIMARY KEY AUTOINCREMENT)",
            b"AUTOINCREMENT"
        ));
        assert!(!contains_sql_keyword(
            "CREATE TABLE x(note TEXT DEFAULT 'AUTOINCREMENT', \
             [AUTOINCREMENT] TEXT /* AUTOINCREMENT */)",
            b"AUTOINCREMENT"
        ));

        let (directory, path) = crate::tests::signed_fixture("semantic-hostile-sequence");
        let connection = Connection::open(&path).expect("hostile sequence fixture");
        connection
            .execute(
                "INSERT INTO sqlite_sequence(name, seq) VALUES \
                 ('capsule_change_log', 999999), \
                 ('untrusted_shadow_table', 424242)",
                [],
            )
            .expect("duplicate and unknown sequence rows");
        drop(connection);
        let source_before = fs::read(&path).expect("sequence source bytes");
        let source = VerifiedWorkspaceSource::open(&path).expect("hostile sequence source");
        let output = directory.path().join("sequence-rebuilt.sqlitecapsule");
        let cancellation = CancellationToken::new();
        prepare_at(source, &output, SemanticCopyMode::Fork, &[], &cancellation)
            .stage()
            .expect("sequence stage")
            .transform_and_validate()
            .expect("sequence validation")
            .publish()
            .expect("sequence publication");

        let connection = Connection::open(&output).expect("sequence output");
        assert_eq!(
            sequence_managed_tables(&connection).expect("schema allowlist"),
            BTreeSet::from(["capsule_change_log".to_owned()])
        );
        let entries = {
            let mut statement = connection
                .prepare("SELECT name, seq FROM sqlite_sequence ORDER BY name COLLATE BINARY")
                .expect("sequence query");
            statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })
                .expect("sequence rows")
                .collect::<Result<Vec<_>, _>>()
                .expect("sequence values")
        };
        assert_eq!(entries, vec![("capsule_change_log".to_owned(), 0)]);
        drop(connection);
        assert_eq!(
            fs::read(&path).expect("sequence source after"),
            source_before
        );
    }

    #[test]
    fn semantic_crash_worker() {
        let Some(_) = std::env::var_os("SQLITE_CAPSULE_SEMANTIC_CRASH_STAGE") else {
            return;
        };
        let source_path = PathBuf::from(
            std::env::var_os("SQLITE_CAPSULE_SEMANTIC_CRASH_SOURCE").expect("crash source"),
        );
        let output_path = PathBuf::from(
            std::env::var_os("SQLITE_CAPSULE_SEMANTIC_CRASH_OUTPUT").expect("crash output"),
        );
        let cancellation = CancellationToken::new();
        let source = VerifiedWorkspaceSource::open(&source_path).expect("crash source");
        prepare_at(
            source,
            &output_path,
            SemanticCopyMode::Fork,
            &[],
            &cancellation,
        )
        .stage()
        .expect("crash stage")
        .transform_and_validate()
        .expect("crash validation")
        .publish()
        .expect("crash publication");
        panic!("configured semantic crash stage did not terminate");
    }

    #[test]
    fn abrupt_semantic_stages_preserve_source_and_never_leave_invalid_final_output() {
        let (directory, source_path) = crate::tests::signed_fixture("semantic-crash-matrix");
        let source_before = fs::read(&source_path).expect("source bytes");
        let executable = std::env::current_exe().expect("test executable");
        for stage in [
            "private-created",
            "snapshot-copied",
            "transformed",
            "vacuumed",
            "sealed-and-verified",
            "postrename-reopened",
        ] {
            let output = directory
                .path()
                .join(format!("semantic-crash-{stage}.sqlitecapsule"));
            let status = Command::new(&executable)
                .arg("semantic_copy::tests::semantic_crash_worker")
                .arg("--exact")
                .arg("--nocapture")
                .env("SQLITE_CAPSULE_SEMANTIC_CRASH_STAGE", stage)
                .env("SQLITE_CAPSULE_SEMANTIC_CRASH_SOURCE", &source_path)
                .env("SQLITE_CAPSULE_SEMANTIC_CRASH_OUTPUT", &output)
                .status()
                .expect("run crash worker");
            assert_eq!(status.code(), Some(97), "crash stage {stage}");
            assert_eq!(
                fs::read(&source_path).expect("source after crash"),
                source_before
            );
            if output.exists() {
                VerifiedWorkspaceSource::open(&output)
                    .expect("any final-name crash output must remain valid");
            }
        }
    }
}

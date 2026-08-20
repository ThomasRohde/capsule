//! Same-schema application release upgrade over two retained verified inputs.
//!
//! The working capsule and clean application release remain pinned read-only.
//! Execution begins from the exact release snapshot, carries only state allowed
//! by the release's signed upgrade policies, and publishes through a held
//! create-new destination after exhaustive operation-specific verification.

use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

use rusqlite::{Connection, OpenFlags, OptionalExtension, params, types::Value as SqlValue};
use serde::Serialize;
use serde_json::{Value as JsonValue, json};
use sha2::{Digest, Sha256};
use sqlite_capsule_lifecycle::{
    DestinationReservation, PrivateOutput, PublishedOutput, SealedPrivateOutput, SourceIdentity,
};

use crate::{
    CancellationToken, DataContract, Dataset, InputRole, LifecyclePlan, Operation,
    TemplateStateLimits, TemplateStateProof, UpgradePolicy, VerifiedWorkspaceSource,
    WorkspaceError, WorkspaceErrorCode, WorkspaceLimits,
    plan::canonical_digest_value,
    prepared_plan::{map_destination_error, map_prepared_destination_error, validate_time_window},
    verify_template_state,
};

const UPGRADE_ACTION: &str = "application-upgrade-same-schema-v1";
const INSTANCE_PROFILE: &str = "org.sqlite-capsule.upgrade-instance-preserve-profile-assets/1";
const MUTABLE_PLATFORM_PROFILE: &str = "org.sqlite-capsule.upgrade-mutable-reset/1";
const HARD_DEADLINE: Duration = Duration::from_secs(30);
const HARD_MAX_ROWS: u64 = 100_000;
const HARD_MAX_STREAM_BYTES: u64 = 512 * 1024 * 1024;
const HARD_MAX_CAPABILITIES: usize = 256;
const HARD_MAX_DATASETS: usize = 256;

pub const APPLICATION_UPGRADE_REVIEW_PROFILE: &str = "org.sqlite-capsule.upgrade-plan/1";

#[derive(Clone, Debug)]
pub struct UpgradePlanRequest<'a> {
    pub output_path: &'a Path,
    pub plan_id: &'a str,
    pub created_at: &'a str,
    pub expires_at: &'a str,
    /// Host-policy trust decision. The key must be present and valid in both
    /// retained signature inventories; display metadata never supplies it.
    pub accepted_publisher_key_id: &'a str,
    pub max_output_bytes: u64,
    pub max_rows: u64,
    pub max_stream_bytes: u64,
    pub deadline: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UpgradeApproval {
    pub accepted_publisher_key_id: String,
    pub capability_changes_accepted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UpgradeInputRef {
    pub file_sha256: String,
    pub capsule_id: String,
    pub revision_id: String,
    pub app_id: String,
    pub app_version: String,
    pub application_digest: String,
    pub publisher_key_id: String,
    pub signature_inventory_sha256: String,
    pub signature_count: u64,
    pub data_schema_id: String,
    pub data_schema_version: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UpgradePublisherContinuity {
    pub state: &'static str,
    pub accepted_key_id: String,
    pub source_inventory_sha256: String,
    pub target_inventory_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CapabilityDelta {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub changed: Vec<String>,
    pub source_permissions_sha256: String,
    pub target_permissions_sha256: String,
    pub requires_review: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpgradeDatasetAction {
    Copy,
    TakeTarget,
    Rebuild,
    Omit,
}

impl UpgradeDatasetAction {
    const fn name(self) -> &'static str {
        match self {
            Self::Copy => "copy",
            Self::TakeTarget => "take-target",
            Self::Rebuild => "rebuild",
            Self::Omit => "omit",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UpgradeDatasetStateEvidence {
    pub profile: &'static str,
    pub row_count: u64,
    pub state_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UpgradeDatasetDecision {
    pub dataset_id: String,
    pub policy: &'static str,
    pub action: UpgradeDatasetAction,
    pub source: UpgradeDatasetStateEvidence,
    pub target: UpgradeDatasetStateEvidence,
    pub expected: UpgradeDatasetStateEvidence,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UpgradeOutputIdentity {
    pub capsule_id: String,
    pub revision_id: String,
    pub app_id: String,
    pub app_version: String,
    pub application_digest: String,
    pub data_schema_id: String,
    pub data_schema_version: u64,
    pub publish_mode: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UpgradeLineageReview {
    pub event_id: String,
    pub operation: &'static str,
    pub occurred_at: String,
    pub working_relation: &'static str,
    pub release_relation: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UpgradeReviewLimits {
    pub max_input_bytes: u64,
    pub max_output_bytes: u64,
    pub max_rows_written: u64,
    pub deadline_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UpgradeReviewReport {
    pub profile: &'static str,
    pub source: UpgradeInputRef,
    pub target_release: UpgradeInputRef,
    pub output: UpgradeOutputIdentity,
    pub publisher_continuity: UpgradePublisherContinuity,
    pub capability_delta: CapabilityDelta,
    pub target_template_state_sha256: String,
    pub dataset_actions: Vec<UpgradeDatasetDecision>,
    pub lineage: UpgradeLineageReview,
    pub limits: UpgradeReviewLimits,
    pub review_digest: String,
}

/// Non-serializable review authority. Public JSON is evidence only; callers
/// must retain this value and present the exact host-generated lifecycle plan.
pub struct UpgradeReview {
    plan: LifecyclePlan,
    destination: DestinationReservation,
    report: UpgradeReviewReport,
    target_template_proof: TemplateStateProof,
    source_instance_assets: Vec<InstanceAssetRow>,
}

pub struct PreparedUpgrade {
    plan: LifecyclePlan,
    destination: DestinationReservation,
    report: UpgradeReviewReport,
    source: VerifiedWorkspaceSource,
    target: VerifiedWorkspaceSource,
    target_template_proof: TemplateStateProof,
    source_instance_assets: Vec<InstanceAssetRow>,
    deadline: Instant,
    cancellation: CancellationToken,
    max_rows: u64,
    max_stream_bytes: u64,
}

pub struct UpgradeStaging {
    plan: LifecyclePlan,
    report: UpgradeReviewReport,
    source: VerifiedWorkspaceSource,
    target: VerifiedWorkspaceSource,
    target_template_proof: TemplateStateProof,
    source_instance_assets: Vec<InstanceAssetRow>,
    private: PrivateOutput,
    deadline: Instant,
    cancellation: CancellationToken,
    max_rows: u64,
    max_stream_bytes: u64,
}

pub struct ValidatedUpgrade {
    plan: LifecyclePlan,
    report: UpgradeReviewReport,
    source: VerifiedWorkspaceSource,
    target: VerifiedWorkspaceSource,
    target_template_proof: TemplateStateProof,
    source_instance_assets: Vec<InstanceAssetRow>,
    sealed: SealedPrivateOutput,
    deadline: Instant,
    cancellation: CancellationToken,
    max_rows: u64,
    max_stream_bytes: u64,
}

pub struct PublishedUpgrade {
    inner: PublishedOutput,
    report: UpgradeReviewReport,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct InstanceAssetRow {
    id: String,
    media_type: String,
    content: Vec<u8>,
    sha256: String,
    width: i64,
    height: i64,
    description: String,
}

pub fn prepare_upgrade_review(
    source: &VerifiedWorkspaceSource,
    target: &VerifiedWorkspaceSource,
    request: &UpgradePlanRequest<'_>,
    cancellation: &CancellationToken,
) -> Result<UpgradeReview, WorkspaceError> {
    require_complete_signature_inventory(source)?;
    require_complete_signature_inventory(target)?;
    require_upgrade_identity(source, target, request.accepted_publisher_key_id)?;
    require_compatible_schema_contract(source, target)?;

    let deadline_budget = request.deadline.min(HARD_DEADLINE);
    let max_rows = request.max_rows.min(HARD_MAX_ROWS);
    let max_stream_bytes = request.max_stream_bytes.min(HARD_MAX_STREAM_BYTES);
    let max_output_bytes = request
        .max_output_bytes
        .min(sqlite_capsule_core::MAX_CAPSULE_BYTES);
    if deadline_budget.is_zero()
        || max_rows == 0
        || max_stream_bytes == 0
        || max_output_bytes < target.source_identity().bytes
    {
        return Err(limit_exceeded());
    }
    let deadline = Instant::now()
        .checked_add(deadline_budget)
        .ok_or_else(limit_exceeded)?;
    assert_inputs_current(source, target, deadline, cancellation)?;

    let target_template_proof = verify_template_state(
        target,
        &TemplateStateLimits {
            deadline: remaining(deadline)?,
            max_rows,
            max_stream_bytes,
        },
        cancellation,
    )?;
    let target_template_state_sha256 = digest_serializable(&target_template_proof)?;
    let source_states =
        capture_dataset_states(source, deadline, cancellation, max_rows, max_stream_bytes)?;
    let target_states =
        capture_dataset_states(target, deadline, cancellation, max_rows, max_stream_bytes)?;
    let dataset_actions = derive_dataset_decisions(target, &source_states, &target_states)?;
    let capability_delta = capability_delta(source, target)?;
    let source_signature_inventory_sha256 = signature_inventory_sha256(source)?;
    let target_signature_inventory_sha256 = signature_inventory_sha256(target)?;
    let source_instance_assets = referenced_instance_assets(source)?;

    let source_revision_id = source
        .identity()
        .overview
        .instance
        .revision_id
        .as_deref()
        .ok_or_else(invalid_contract)?;
    let (output_revision_id, event_id) = generate_distinct_ids(
        target.verified.connection(),
        source_revision_id,
        target.identity().overview.instance.revision_id.as_deref(),
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
        &[
            source.source_identity().clone(),
            target.source_identity().clone(),
        ],
    )
    .map_err(map_destination_error)?;

    let source_ref = input_ref(
        source,
        request.accepted_publisher_key_id,
        &source_signature_inventory_sha256,
    )?;
    let target_ref = input_ref(
        target,
        request.accepted_publisher_key_id,
        &target_signature_inventory_sha256,
    )?;
    let schema = target
        .identity()
        .overview
        .data_schema
        .as_ref()
        .ok_or_else(invalid_contract)?;
    let deadline_ms = u64::try_from(deadline_budget.as_millis()).map_err(|_| limit_exceeded())?;
    let mut report = UpgradeReviewReport {
        profile: APPLICATION_UPGRADE_REVIEW_PROFILE,
        source: source_ref,
        target_release: target_ref,
        output: UpgradeOutputIdentity {
            capsule_id: source.identity().capsule_id.clone(),
            revision_id: output_revision_id.clone(),
            app_id: target.identity().app_id.clone(),
            app_version: target.identity().app_version.clone(),
            application_digest: lower_hex(target.application_digest()),
            data_schema_id: schema.data_schema_id.clone(),
            data_schema_version: u64::try_from(schema.data_schema_version)
                .map_err(|_| invalid_contract())?,
            publish_mode: "create-new-no-replace",
        },
        publisher_continuity: UpgradePublisherContinuity {
            state: "same-accepted-key",
            accepted_key_id: request.accepted_publisher_key_id.to_owned(),
            source_inventory_sha256: source_signature_inventory_sha256,
            target_inventory_sha256: target_signature_inventory_sha256,
        },
        capability_delta,
        target_template_state_sha256,
        dataset_actions,
        lineage: UpgradeLineageReview {
            event_id: event_id.clone(),
            operation: "application-upgrade",
            occurred_at: request.created_at.to_owned(),
            working_relation: "upgraded-from",
            release_relation: "application-release",
        },
        limits: UpgradeReviewLimits {
            max_input_bytes: source
                .source_identity()
                .bytes
                .max(target.source_identity().bytes),
            max_output_bytes,
            max_rows_written: max_rows,
            deadline_ms,
        },
        review_digest: String::new(),
    };
    report.review_digest = review_digest(&report)?;
    let plan = build_plan(
        source,
        target,
        &destination,
        leaf,
        request,
        &report,
        &event_id,
    )?;
    bind_plan(&plan, source, target, &destination, &report)?;
    assert_inputs_current(source, target, deadline, cancellation)?;
    destination
        .assert_reserved_current()
        .map_err(map_prepared_destination_error)?;
    Ok(UpgradeReview {
        plan,
        destination,
        report,
        target_template_proof,
        source_instance_assets,
    })
}

pub fn parse_upgrade_plan(bytes: &[u8]) -> Result<LifecyclePlan, WorkspaceError> {
    let plan = LifecyclePlan::parse(bytes)?;
    validate_plan_shape(&plan)?;
    Ok(plan)
}

impl UpgradeReview {
    pub fn plan(&self) -> &LifecyclePlan {
        &self.plan
    }

    pub fn report(&self) -> &UpgradeReviewReport {
        &self.report
    }

    pub fn prepare(
        self,
        approved_plan: LifecyclePlan,
        approval: &UpgradeApproval,
        source: VerifiedWorkspaceSource,
        target: VerifiedWorkspaceSource,
        limits: &WorkspaceLimits,
        cancellation: &CancellationToken,
    ) -> Result<PreparedUpgrade, WorkspaceError> {
        PreparedUpgrade::prepare_at(
            self,
            approved_plan,
            approval,
            source,
            target,
            SystemTime::now(),
            limits,
            cancellation,
        )
    }
}

impl PreparedUpgrade {
    #[allow(clippy::too_many_arguments)]
    fn prepare_at(
        review: UpgradeReview,
        plan: LifecyclePlan,
        approval: &UpgradeApproval,
        source: VerifiedWorkspaceSource,
        target: VerifiedWorkspaceSource,
        now: SystemTime,
        limits: &WorkspaceLimits,
        cancellation: &CancellationToken,
    ) -> Result<Self, WorkspaceError> {
        if review.plan.canonical_bytes()? != plan.canonical_bytes()? {
            return Err(stale_plan());
        }
        if approval.accepted_publisher_key_id != review.report.publisher_continuity.accepted_key_id
        {
            return Err(publisher_mismatch());
        }
        if review.report.capability_delta.requires_review && !approval.capability_changes_accepted {
            return Err(WorkspaceError::new(
                WorkspaceErrorCode::CapabilityReviewRequired,
            ));
        }
        validate_time_window(&plan, now)?;
        validate_plan_shape(&plan)?;
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
        require_complete_signature_inventory(&source)?;
        require_complete_signature_inventory(&target)?;
        require_upgrade_identity(
            &source,
            &target,
            &review.report.publisher_continuity.accepted_key_id,
        )?;
        require_compatible_schema_contract(&source, &target)?;
        bind_plan(&plan, &source, &target, &review.destination, &review.report)?;
        assert_inputs_current(&source, &target, deadline, cancellation)?;
        review
            .destination
            .assert_reserved_current()
            .map_err(map_prepared_destination_error)?;

        let max_rows = plan.limits().max_rows_written().min(HARD_MAX_ROWS);
        let max_stream_bytes = limits
            .max_capsule_bytes
            .saturating_mul(8)
            .clamp(1, HARD_MAX_STREAM_BYTES);
        let proof = verify_template_state(
            &target,
            &TemplateStateLimits {
                deadline: remaining(deadline)?,
                max_rows,
                max_stream_bytes,
            },
            cancellation,
        )?;
        if proof != review.target_template_proof
            || digest_serializable(&proof)? != review.report.target_template_state_sha256
        {
            return Err(stale_plan());
        }
        let decisions = reproduce_decisions(
            &source,
            &target,
            deadline,
            cancellation,
            max_rows,
            max_stream_bytes,
        )?;
        if decisions != review.report.dataset_actions
            || capability_delta(&source, &target)? != review.report.capability_delta
            || signature_inventory_sha256(&source)?
                != review.report.source.signature_inventory_sha256
            || signature_inventory_sha256(&target)?
                != review.report.target_release.signature_inventory_sha256
            || referenced_instance_assets(&source)? != review.source_instance_assets
        {
            return Err(stale_plan());
        }
        Ok(Self {
            plan,
            destination: review.destination,
            report: review.report,
            source,
            target,
            target_template_proof: proof,
            source_instance_assets: review.source_instance_assets,
            deadline,
            cancellation: cancellation.clone(),
            max_rows,
            max_stream_bytes,
        })
    }

    pub fn stage(self) -> Result<UpgradeStaging, WorkspaceError> {
        check(self.deadline, &self.cancellation)?;
        validate_time_window(&self.plan, SystemTime::now())?;
        assert_inputs_current(
            &self.source,
            &self.target,
            self.deadline,
            &self.cancellation,
        )?;
        self.destination
            .assert_reserved_current()
            .map_err(map_prepared_destination_error)?;
        let private = self
            .destination
            .stage()
            .map_err(map_prepared_destination_error)?;
        maybe_crash("private-created");
        Ok(UpgradeStaging {
            plan: self.plan,
            report: self.report,
            source: self.source,
            target: self.target,
            target_template_proof: self.target_template_proof,
            source_instance_assets: self.source_instance_assets,
            private,
            deadline: self.deadline,
            cancellation: self.cancellation,
            max_rows: self.max_rows,
            max_stream_bytes: self.max_stream_bytes,
        })
    }
}

impl UpgradeStaging {
    /// Copies the retained clean release snapshot, then writes only declared
    /// domain state and mutable instance/lineage compartments.
    pub fn transform_and_validate(mut self) -> Result<ValidatedUpgrade, WorkspaceError> {
        check(self.deadline, &self.cancellation)?;
        validate_time_window(&self.plan, SystemTime::now())?;
        assert_inputs_current(
            &self.source,
            &self.target,
            self.deadline,
            &self.cancellation,
        )?;
        reproduce_review(
            &self.source,
            &self.target,
            &self.report,
            &self.target_template_proof,
            &self.source_instance_assets,
            self.deadline,
            &self.cancellation,
            self.max_rows,
            self.max_stream_bytes,
        )?;

        let control = verification_control(self.deadline, &self.cancellation)?;
        let copied = self
            .target
            .verified
            .copy_snapshot_to_file_with_control(
                self.private.file_mut(),
                &control,
                self.plan.limits().max_output_bytes(),
            )
            .map_err(map_launch_output_error)?;
        if copied != self.target.source_identity().bytes {
            return Err(verification_failed());
        }
        self.private
            .file_mut()
            .sync_all()
            .map_err(|_| output_failed())?;
        maybe_crash("release-snapshot-copied");

        transform_private(
            self.private.private_path_hint(),
            &self.source,
            &self.target,
            &self.report,
            &self.source_instance_assets,
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
        validate_upgrade_output(
            &output,
            &self.source,
            &self.target,
            &self.report,
            &self.source_instance_assets,
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
        assert_inputs_current(
            &self.source,
            &self.target,
            self.deadline,
            &self.cancellation,
        )?;
        maybe_crash("sealed-and-verified");
        Ok(ValidatedUpgrade {
            plan: self.plan,
            report: self.report,
            source: self.source,
            target: self.target,
            target_template_proof: self.target_template_proof,
            source_instance_assets: self.source_instance_assets,
            sealed,
            deadline: self.deadline,
            cancellation: self.cancellation,
            max_rows: self.max_rows,
            max_stream_bytes: self.max_stream_bytes,
        })
    }
}

impl ValidatedUpgrade {
    pub fn publish(self) -> Result<PublishedUpgrade, WorkspaceError> {
        self.publish_with_hook(|| {})
    }

    fn publish_with_hook<F>(
        self,
        after_final_output_check: F,
    ) -> Result<PublishedUpgrade, WorkspaceError>
    where
        F: FnOnce(),
    {
        check(self.deadline, &self.cancellation)?;
        validate_time_window(&self.plan, SystemTime::now())?;
        reproduce_review(
            &self.source,
            &self.target,
            &self.report,
            &self.target_template_proof,
            &self.source_instance_assets,
            self.deadline,
            &self.cancellation,
            self.max_rows,
            self.max_stream_bytes,
        )?;
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
        validate_upgrade_output(
            &prepublish,
            &self.source,
            &self.target,
            &self.report,
            &self.source_instance_assets,
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
        assert_inputs_current(
            &self.source,
            &self.target,
            self.deadline,
            &self.cancellation,
        )?;

        let deadline = self.deadline;
        let cancellation = self.cancellation.clone();
        let max_rows = self.max_rows;
        let max_stream_bytes = self.max_stream_bytes;
        let max_output_bytes = self.plan.limits().max_output_bytes();
        let plan = &self.plan;
        let report = &self.report;
        let source = &self.source;
        let target = &self.target;
        let assets = &self.source_instance_assets;
        // SAFETY: the exact held output has passed exhaustive upgrade proofs.
        // The callback snapshots and repeats them before the lifecycle layer's
        // final name/identity/digest rebind and possible quarantine return.
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
                        || validate_upgrade_output(
                            &output,
                            source,
                            target,
                            report,
                            assets,
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
                    assert_inputs_current(source, target, deadline, &cancellation).map_err(
                        |_| sqlite_capsule_lifecycle::LifecycleError::PostPublishVerification,
                    )?;
                    Ok(())
                })
        }
        .map_err(map_destination_error)?;
        Ok(PublishedUpgrade {
            inner: published,
            report: self.report,
        })
    }
}

impl PublishedUpgrade {
    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    pub fn identity(&self) -> &SourceIdentity {
        &self.inner.identity
    }

    pub fn report(&self) -> &UpgradeReviewReport {
        &self.report
    }
}

fn build_plan(
    source: &VerifiedWorkspaceSource,
    target: &VerifiedWorkspaceSource,
    destination: &DestinationReservation,
    leaf: &str,
    request: &UpgradePlanRequest<'_>,
    report: &UpgradeReviewReport,
    event_id: &str,
) -> Result<LifecyclePlan, WorkspaceError> {
    let mut decisions = vec![json!({
        "scope": "application",
        "subject": target.identity().app_id,
        "action": UPGRADE_ACTION,
        "reason": "Host-owned same-schema upgrade from an authenticated clean release.",
        "parameters": {
            "accepted_publisher_key_id": report.publisher_continuity.accepted_key_id,
            "capability_review_required": report.capability_delta.requires_review,
            "event_id": event_id,
            "instance_profile": INSTANCE_PROFILE,
            "mutable_platform_profile": MUTABLE_PLATFORM_PROFILE,
            "occurred_at": request.created_at,
            "review_digest": report.review_digest,
            "source_signature_inventory_sha256": report.source.signature_inventory_sha256,
            "target_signature_inventory_sha256": report.target_release.signature_inventory_sha256,
            "target_template_state_sha256": report.target_template_state_sha256
        }
    })];
    for decision in &report.dataset_actions {
        decisions.push(json!({
            "scope": "dataset",
            "subject": decision.dataset_id,
            "action": decision.action.name(),
            "reason": "Derived from the clean target release's signed upgrade policy.",
            "parameters": {
                "expected_row_count": decision.expected.row_count,
                "expected_state_profile": decision.expected.profile,
                "expected_state_sha256": decision.expected.state_sha256,
                "policy": decision.policy,
                "source_row_count": decision.source.row_count,
                "source_state_sha256": decision.source.state_sha256,
                "target_row_count": decision.target.row_count,
                "target_state_sha256": decision.target.state_sha256
            }
        }));
    }
    let source_input = plan_input(source, InputRole::Source, &report.source)?;
    let target_input = plan_input(
        target,
        InputRole::ApplicationRelease,
        &report.target_release,
    )?;
    let mut value = json!({
        "profile": crate::PLAN_PROFILE,
        "plan_id": request.plan_id,
        "operation": "application-upgrade",
        "created_at": request.created_at,
        "expires_at": request.expires_at,
        "inputs": [source_input, target_input],
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
        "decisions": decisions,
        "limits": {
            "max_input_bytes": report.limits.max_input_bytes,
            "max_output_bytes": report.limits.max_output_bytes,
            "max_rows_inspected": report.limits.max_rows_written,
            "max_rows_written": report.limits.max_rows_written,
            "deadline_ms": report.limits.deadline_ms
        },
        "expected": {
            "capsule_id": report.output.capsule_id,
            "revision_id": report.output.revision_id,
            "app_id": report.output.app_id,
            "application_digest": report.output.application_digest,
            "data_schema_id": report.output.data_schema_id,
            "data_schema_version": report.output.data_schema_version
        },
        "plan_digest": ""
    });
    let digest = canonical_digest_value(&value)?;
    value["plan_digest"] = JsonValue::String(digest);
    parse_upgrade_plan(&serde_json::to_vec(&value).map_err(|_| invalid_contract())?)
}

fn plan_input(
    source: &VerifiedWorkspaceSource,
    role: InputRole,
    reference: &UpgradeInputRef,
) -> Result<JsonValue, WorkspaceError> {
    let live = source.source_identity();
    Ok(json!({
        "role": match role {
            InputRole::Source => "source",
            InputRole::ApplicationRelease => "application-release",
            _ => return Err(invalid_contract()),
        },
        "path_hint": utf8_path(&source.identity().canonical_path)?,
        "file_sha256": reference.file_sha256,
        "snapshot_sha256": reference.file_sha256,
        "size_bytes": live.bytes,
        "filesystem_identity": {
            "platform": std::env::consts::OS,
            "volume_or_device": live.device.to_string(),
            "file_id_or_inode": live.stable_file_id,
            "modified_ns": live.modified_ns
        },
        "capsule": {
            "format_version": "0.3",
            "capsule_id": reference.capsule_id,
            "revision_id": reference.revision_id,
            "app_id": reference.app_id,
            "app_version": reference.app_version,
            "application_digest": reference.application_digest,
            "data_schema_id": reference.data_schema_id,
            "data_schema_version": reference.data_schema_version,
            "publisher_key_id": reference.publisher_key_id
        }
    }))
}

fn input_ref(
    source: &VerifiedWorkspaceSource,
    accepted_key_id: &str,
    inventory_sha256: &str,
) -> Result<UpgradeInputRef, WorkspaceError> {
    let identity = source.identity();
    let schema = identity
        .overview
        .data_schema
        .as_ref()
        .ok_or_else(invalid_contract)?;
    Ok(UpgradeInputRef {
        file_sha256: source.source_sha256(),
        capsule_id: identity.capsule_id.clone(),
        revision_id: identity
            .overview
            .instance
            .revision_id
            .clone()
            .ok_or_else(invalid_contract)?,
        app_id: identity.app_id.clone(),
        app_version: identity.app_version.clone(),
        application_digest: lower_hex(source.application_digest()),
        publisher_key_id: accepted_key_id.to_owned(),
        signature_inventory_sha256: inventory_sha256.to_owned(),
        signature_count: source.signature_reports().len() as u64,
        data_schema_id: schema.data_schema_id.clone(),
        data_schema_version: u64::try_from(schema.data_schema_version)
            .map_err(|_| invalid_contract())?,
    })
}

fn review_digest(report: &UpgradeReviewReport) -> Result<String, WorkspaceError> {
    let mut value = serde_json::to_value(report).map_err(|_| invalid_contract())?;
    value
        .as_object_mut()
        .ok_or_else(invalid_contract)?
        .remove("review_digest");
    Ok(lower_hex(&Sha256::digest(crate::plan::canonical_json(
        &value,
    )?)))
}

fn digest_serializable(value: &impl Serialize) -> Result<String, WorkspaceError> {
    let value = serde_json::to_value(value).map_err(|_| invalid_contract())?;
    Ok(lower_hex(&Sha256::digest(crate::plan::canonical_json(
        &value,
    )?)))
}

fn validate_plan_shape(plan: &LifecyclePlan) -> Result<(), WorkspaceError> {
    if plan.operation() != Operation::ApplicationUpgrade
        || plan.inputs().len() != 2
        || plan.inputs()[0].role() != InputRole::Source
        || plan.inputs()[1].role() != InputRole::ApplicationRelease
        || plan.decisions().is_empty()
        || plan.decisions()[0].scope() != crate::DecisionScope::Application
        || plan.decisions()[0].action() != UPGRADE_ACTION
        || plan.decisions()[1..]
            .iter()
            .any(|decision| decision.scope() != crate::DecisionScope::Dataset)
    {
        return Err(invalid_contract());
    }
    Ok(())
}

fn bind_plan(
    plan: &LifecyclePlan,
    source: &VerifiedWorkspaceSource,
    target: &VerifiedWorkspaceSource,
    destination: &DestinationReservation,
    report: &UpgradeReviewReport,
) -> Result<(), WorkspaceError> {
    validate_plan_shape(plan)?;
    if review_digest(report)? != report.review_digest
        || plan.decisions().len() != report.dataset_actions.len() + 1
        || report.dataset_actions.len() != target.data_contract().datasets.len()
    {
        return Err(stale_plan());
    }
    bind_input(&plan.inputs()[0], source, &report.source)?;
    bind_input(&plan.inputs()[1], target, &report.target_release)?;
    let parent = plan.output().parent_identity();
    if plan.output().path() != utf8_path(&destination.path_hint())?
        || plan.output().leaf_name() != destination.leaf().to_str().unwrap_or_default()
        || parent.platform() != std::env::consts::OS
        || parent.volume_or_device() != destination.identity().device.to_string()
        || parent.file_id_or_inode() != destination.identity().stable_file_id
        || plan.expected().capsule_id() != Some(report.output.capsule_id.as_str())
        || plan.expected().revision_id() != Some(report.output.revision_id.as_str())
        || plan.expected().app_id() != report.output.app_id
        || plan.expected().application_digest() != Some(report.output.application_digest.as_str())
        || plan.expected().data_schema_id() != Some(report.output.data_schema_id.as_str())
        || plan.expected().data_schema_version() != Some(report.output.data_schema_version)
    {
        return Err(stale_plan());
    }
    let value = serde_json::to_value(plan).map_err(|_| invalid_contract())?;
    let app = &value["decisions"][0]["parameters"];
    if app["accepted_publisher_key_id"].as_str()
        != Some(report.publisher_continuity.accepted_key_id.as_str())
        || app["capability_review_required"].as_bool()
            != Some(report.capability_delta.requires_review)
        || app["event_id"].as_str() != Some(report.lineage.event_id.as_str())
        || app["instance_profile"].as_str() != Some(INSTANCE_PROFILE)
        || app["mutable_platform_profile"].as_str() != Some(MUTABLE_PLATFORM_PROFILE)
        || app["occurred_at"].as_str() != Some(report.lineage.occurred_at.as_str())
        || app["review_digest"].as_str() != Some(report.review_digest.as_str())
        || app["target_template_state_sha256"].as_str()
            != Some(report.target_template_state_sha256.as_str())
    {
        return Err(stale_plan());
    }
    for ((plan_decision, parameters), expected) in plan.decisions()[1..]
        .iter()
        .zip(value["decisions"].as_array().ok_or_else(invalid_contract)?[1..].iter())
        .zip(&report.dataset_actions)
    {
        let parameters = &parameters["parameters"];
        if plan_decision.subject() != expected.dataset_id
            || plan_decision.action() != expected.action.name()
            || parameters["policy"].as_str() != Some(expected.policy)
            || parameters["source_state_sha256"].as_str()
                != Some(expected.source.state_sha256.as_str())
            || parameters["target_state_sha256"].as_str()
                != Some(expected.target.state_sha256.as_str())
            || parameters["expected_state_sha256"].as_str()
                != Some(expected.expected.state_sha256.as_str())
            || parameters["expected_row_count"].as_u64() != Some(expected.expected.row_count)
        {
            return Err(stale_plan());
        }
    }
    Ok(())
}

fn bind_input(
    input: &crate::PlanInput,
    source: &VerifiedWorkspaceSource,
    reference: &UpgradeInputRef,
) -> Result<(), WorkspaceError> {
    let live = source.source_identity();
    let capsule = input.capsule();
    if input.path_hint() != utf8_path(&source.identity().canonical_path)?
        || input.file_sha256() != reference.file_sha256
        || input.snapshot_sha256() != reference.file_sha256
        || input.size_bytes() != live.bytes
        || input.filesystem_identity().platform() != std::env::consts::OS
        || input.filesystem_identity().volume_or_device() != live.device.to_string()
        || input.filesystem_identity().file_id_or_inode() != live.stable_file_id
        || input.filesystem_identity().modified_ns() != live.modified_ns
        || capsule.format_version() != "0.3"
        || capsule.capsule_id() != Some(reference.capsule_id.as_str())
        || capsule.revision_id() != Some(reference.revision_id.as_str())
        || capsule.app_id() != reference.app_id
        || capsule.app_version() != reference.app_version
        || capsule.application_digest() != Some(reference.application_digest.as_str())
        || capsule.data_schema_id() != Some(reference.data_schema_id.as_str())
        || capsule.data_schema_version() != Some(reference.data_schema_version)
        || capsule.publisher_key_id() != Some(reference.publisher_key_id.as_str())
    {
        return Err(stale_plan());
    }
    Ok(())
}

fn require_complete_signature_inventory(
    source: &VerifiedWorkspaceSource,
) -> Result<(), WorkspaceError> {
    if source.has_complete_valid_signature_inventory() {
        Ok(())
    } else {
        Err(WorkspaceError::new(WorkspaceErrorCode::InvalidSignature))
    }
}

fn require_upgrade_identity(
    source: &VerifiedWorkspaceSource,
    target: &VerifiedWorkspaceSource,
    accepted_key_id: &str,
) -> Result<(), WorkspaceError> {
    if source.identity().user_version != 3
        || target.identity().user_version != 3
        || source.identity().format_version != "0.3"
        || target.identity().format_version != "0.3"
    {
        return Err(WorkspaceError::new(WorkspaceErrorCode::UnsupportedFormat));
    }
    if source.identity().app_id != target.identity().app_id {
        return Err(WorkspaceError::new(
            WorkspaceErrorCode::IncompatibleApplication,
        ));
    }
    if !version_is_strictly_newer(
        &source.identity().app_version,
        &target.identity().app_version,
    )? {
        return Err(WorkspaceError::new(WorkspaceErrorCode::VersionNotNewer));
    }
    if accepted_key_id.is_empty()
        || accepted_key_id.len() > 1_024
        || !source.signature_reports().iter().any(|report| {
            report.key_id == accepted_key_id
                && report.cryptographically_valid
                && report.digest_matches
        })
        || !target.signature_reports().iter().any(|report| {
            report.key_id == accepted_key_id
                && report.cryptographically_valid
                && report.digest_matches
        })
    {
        return Err(publisher_mismatch());
    }
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
    if source_schema.data_schema_id != target_schema.data_schema_id
        || source_schema.data_schema_version != target_schema.data_schema_version
    {
        return Err(WorkspaceError::new(WorkspaceErrorCode::IncompatibleSchema));
    }
    Ok(())
}

fn require_compatible_schema_contract(
    source: &VerifiedWorkspaceSource,
    target: &VerifiedWorkspaceSource,
) -> Result<(), WorkspaceError> {
    let left = contract_projection(source.data_contract());
    let right = contract_projection(target.data_contract());
    if left != right {
        return Err(WorkspaceError::new(WorkspaceErrorCode::IncompatibleSchema));
    }
    for dataset in &source.data_contract().datasets {
        for table in &dataset.tables {
            let source_sql = schema_sql(source.verified.connection(), "table", &table.name)?;
            let target_sql = schema_sql(target.verified.connection(), "table", &table.name)?;
            if source_sql != target_sql {
                return Err(WorkspaceError::new(WorkspaceErrorCode::IncompatibleSchema));
            }
            let source_objects =
                dependent_schema_objects(source.verified.connection(), &table.name)?;
            let target_objects =
                dependent_schema_objects(target.verified.connection(), &table.name)?;
            if source_objects != target_objects {
                return Err(WorkspaceError::new(WorkspaceErrorCode::IncompatibleSchema));
            }
        }
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
struct DatasetProjection<'a> {
    id: &'a str,
    role: crate::DatasetRole,
    sensitivity: crate::Sensitivity,
    required: bool,
    tables: &'a [crate::DatasetTable],
    dependencies: Vec<&'a str>,
}

fn contract_projection(contract: &DataContract) -> Vec<DatasetProjection<'_>> {
    contract
        .datasets
        .iter()
        .map(|dataset| DatasetProjection {
            id: &dataset.id,
            role: dataset.role,
            sensitivity: dataset.sensitivity,
            required: dataset.required,
            tables: &dataset.tables,
            dependencies: dataset
                .dependencies
                .iter()
                .map(|dependency| dependency.dataset_id.as_str())
                .collect(),
        })
        .collect()
}

fn schema_sql(
    connection: &Connection,
    object_type: &str,
    name: &str,
) -> Result<String, WorkspaceError> {
    connection
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE type = ?1 AND name = ?2 COLLATE BINARY",
            params![object_type, name],
            |row| row.get(0),
        )
        .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::IncompatibleSchema))
}

fn dependent_schema_objects(
    connection: &Connection,
    table: &str,
) -> Result<Vec<(String, String, Option<String>)>, WorkspaceError> {
    let mut statement = connection
        .prepare(
            "SELECT type, name, sql FROM sqlite_schema \
             WHERE tbl_name = ?1 COLLATE BINARY AND type IN ('index','trigger') \
             ORDER BY type COLLATE BINARY, name COLLATE BINARY",
        )
        .map_err(|_| invalid_contract())?;
    statement
        .query_map([table], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .map_err(|_| invalid_contract())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| invalid_contract())
}

fn capability_delta(
    source: &VerifiedWorkspaceSource,
    target: &VerifiedWorkspaceSource,
) -> Result<CapabilityDelta, WorkspaceError> {
    let source_object = source
        .identity()
        .permissions
        .as_object()
        .ok_or_else(invalid_contract)?;
    let target_object = target
        .identity()
        .permissions
        .as_object()
        .ok_or_else(invalid_contract)?;
    if source_object.len() > HARD_MAX_CAPABILITIES || target_object.len() > HARD_MAX_CAPABILITIES {
        return Err(limit_exceeded());
    }
    let source_keys: BTreeSet<_> = source_object.keys().collect();
    let target_keys: BTreeSet<_> = target_object.keys().collect();
    if source_keys
        .iter()
        .chain(target_keys.iter())
        .any(|key| key.is_empty() || key.len() > 256)
    {
        return Err(invalid_contract());
    }
    let added = target_keys
        .difference(&source_keys)
        .map(|value| (*value).clone())
        .collect::<Vec<_>>();
    let removed = source_keys
        .difference(&target_keys)
        .map(|value| (*value).clone())
        .collect::<Vec<_>>();
    let mut changed = Vec::new();
    for key in source_keys.intersection(&target_keys) {
        let source_value = source_object.get(*key).ok_or_else(invalid_contract)?;
        let target_value = target_object.get(*key).ok_or_else(invalid_contract)?;
        if crate::plan::canonical_json(source_value)? != crate::plan::canonical_json(target_value)?
        {
            changed.push((*key).clone());
        }
    }
    let source_permissions_sha256 = lower_hex(&Sha256::digest(crate::plan::canonical_json(
        &source.identity().permissions,
    )?));
    let target_permissions_sha256 = lower_hex(&Sha256::digest(crate::plan::canonical_json(
        &target.identity().permissions,
    )?));
    Ok(CapabilityDelta {
        requires_review: !added.is_empty() || !changed.is_empty(),
        added,
        removed,
        changed,
        source_permissions_sha256,
        target_permissions_sha256,
    })
}

fn capture_dataset_states(
    source: &VerifiedWorkspaceSource,
    deadline: Instant,
    cancellation: &CancellationToken,
    max_rows: u64,
    max_stream_bytes: u64,
) -> Result<BTreeMap<String, UpgradeDatasetStateEvidence>, WorkspaceError> {
    let mut rows_remaining = max_rows;
    let mut bytes_remaining = max_stream_bytes;
    source
        .data_contract()
        .datasets
        .iter()
        .map(|dataset| {
            let (row_count, state_sha256) = crate::template_state::dataset_state_with_budget(
                source,
                dataset,
                &mut rows_remaining,
                &mut bytes_remaining,
                deadline,
                cancellation,
            )?;
            Ok((
                dataset.id.clone(),
                UpgradeDatasetStateEvidence {
                    profile: crate::DATASET_STATE_PROFILE,
                    row_count,
                    state_sha256,
                },
            ))
        })
        .collect()
}

fn derive_dataset_decisions(
    target: &VerifiedWorkspaceSource,
    source_states: &BTreeMap<String, UpgradeDatasetStateEvidence>,
    target_states: &BTreeMap<String, UpgradeDatasetStateEvidence>,
) -> Result<Vec<UpgradeDatasetDecision>, WorkspaceError> {
    if target.data_contract().datasets.len() > HARD_MAX_DATASETS {
        return Err(limit_exceeded());
    }
    target
        .data_contract()
        .datasets
        .iter()
        .map(|dataset| {
            let source = source_states
                .get(&dataset.id)
                .ok_or_else(invalid_contract)?;
            let target_state = target_states
                .get(&dataset.id)
                .ok_or_else(invalid_contract)?;
            let (policy, action, expected) = match dataset.upgrade {
                UpgradePolicy::Copy => ("copy", UpgradeDatasetAction::Copy, source),
                UpgradePolicy::Target => ("target", UpgradeDatasetAction::TakeTarget, target_state),
                UpgradePolicy::Rebuild => ("rebuild", UpgradeDatasetAction::Rebuild, target_state),
                UpgradePolicy::Omit if target_state.row_count == 0 => {
                    ("omit", UpgradeDatasetAction::Omit, target_state)
                }
                UpgradePolicy::Omit => return Err(invalid_contract()),
                UpgradePolicy::Migrate | UpgradePolicy::Forbid => {
                    return Err(WorkspaceError::new(
                        WorkspaceErrorCode::UnsupportedOperation,
                    ));
                }
            };
            Ok(UpgradeDatasetDecision {
                dataset_id: dataset.id.clone(),
                policy,
                action,
                source: source.clone(),
                target: target_state.clone(),
                expected: expected.clone(),
            })
        })
        .collect()
}

fn reproduce_decisions(
    source: &VerifiedWorkspaceSource,
    target: &VerifiedWorkspaceSource,
    deadline: Instant,
    cancellation: &CancellationToken,
    max_rows: u64,
    max_stream_bytes: u64,
) -> Result<Vec<UpgradeDatasetDecision>, WorkspaceError> {
    require_compatible_schema_contract(source, target)?;
    let source_states =
        capture_dataset_states(source, deadline, cancellation, max_rows, max_stream_bytes)?;
    let target_states =
        capture_dataset_states(target, deadline, cancellation, max_rows, max_stream_bytes)?;
    derive_dataset_decisions(target, &source_states, &target_states)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ReleaseVersion {
    core: [NumericIdentifier; 3],
    prerelease: Vec<PrereleaseIdentifier>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct NumericIdentifier(String);

impl Ord for NumericIdentifier {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0
            .len()
            .cmp(&other.0.len())
            .then_with(|| self.0.cmp(&other.0))
    }
}

impl PartialOrd for NumericIdentifier {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PrereleaseIdentifier {
    Numeric(NumericIdentifier),
    Text(String),
}

impl Ord for PrereleaseIdentifier {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Numeric(left), Self::Numeric(right)) => left.cmp(right),
            (Self::Numeric(_), Self::Text(_)) => Ordering::Less,
            (Self::Text(_), Self::Numeric(_)) => Ordering::Greater,
            (Self::Text(left), Self::Text(right)) => left.cmp(right),
        }
    }
}

impl PartialOrd for PrereleaseIdentifier {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ReleaseVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.core.cmp(&other.core) {
            Ordering::Equal => match (self.prerelease.is_empty(), other.prerelease.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => self.prerelease.cmp(&other.prerelease),
            },
            ordering => ordering,
        }
    }
}

impl PartialOrd for ReleaseVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn version_is_strictly_newer(source: &str, target: &str) -> Result<bool, WorkspaceError> {
    Ok(parse_release_version(target)? > parse_release_version(source)?)
}

fn parse_release_version(value: &str) -> Result<ReleaseVersion, WorkspaceError> {
    if value.is_empty() || value.len() > 128 {
        return Err(WorkspaceError::new(
            WorkspaceErrorCode::UnsupportedOperation,
        ));
    }
    let without_build = if let Some((without_build, build)) = value.split_once('+') {
        if !valid_identifier_list(build, false) || without_build.contains('+') {
            return Err(WorkspaceError::new(
                WorkspaceErrorCode::UnsupportedOperation,
            ));
        }
        without_build
    } else {
        value
    };
    let (core, prerelease) = if let Some((core, prerelease)) = without_build.split_once('-') {
        if prerelease.is_empty() {
            return Err(WorkspaceError::new(
                WorkspaceErrorCode::UnsupportedOperation,
            ));
        }
        (core, Some(prerelease))
    } else {
        (without_build, None)
    };
    let core_parts = core.split('.').collect::<Vec<_>>();
    if core_parts.len() != 3 {
        return Err(WorkspaceError::new(
            WorkspaceErrorCode::UnsupportedOperation,
        ));
    }
    let mut parsed_core: [NumericIdentifier; 3] =
        std::array::from_fn(|_| NumericIdentifier(String::new()));
    for (index, part) in core_parts.into_iter().enumerate() {
        if part.is_empty()
            || !part.bytes().all(|byte| byte.is_ascii_digit())
            || (part.len() > 1 && part.starts_with('0'))
        {
            return Err(WorkspaceError::new(
                WorkspaceErrorCode::UnsupportedOperation,
            ));
        }
        parsed_core[index] = NumericIdentifier(part.to_owned());
    }
    let prerelease = if let Some(prerelease) = prerelease {
        if !valid_identifier_list(prerelease, true) {
            return Err(WorkspaceError::new(
                WorkspaceErrorCode::UnsupportedOperation,
            ));
        }
        prerelease
            .split('.')
            .map(|part| {
                if part.bytes().all(|byte| byte.is_ascii_digit()) {
                    Ok(PrereleaseIdentifier::Numeric(NumericIdentifier(
                        part.to_owned(),
                    )))
                } else {
                    Ok(PrereleaseIdentifier::Text(part.to_owned()))
                }
            })
            .collect::<Result<Vec<_>, WorkspaceError>>()?
    } else {
        Vec::new()
    };
    Ok(ReleaseVersion {
        core: parsed_core,
        prerelease,
    })
}

fn valid_identifier_list(value: &str, forbid_numeric_leading_zero: bool) -> bool {
    !value.is_empty()
        && value.split('.').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                && (!forbid_numeric_leading_zero
                    || !part.bytes().all(|byte| byte.is_ascii_digit())
                    || part.len() == 1
                    || !part.starts_with('0'))
        })
}

fn signature_inventory_sha256(source: &VerifiedWorkspaceSource) -> Result<String, WorkspaceError> {
    let mut statement = source
        .verified
        .connection()
        .prepare(
            "SELECT key_id, algorithm, public_key, application_digest, signature, signed_at \
             FROM capsule_signature ORDER BY key_id COLLATE BINARY",
        )
        .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InvalidSignature))?;
    let mut rows = statement
        .query([])
        .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InvalidSignature))?;
    let mut count = 0_u64;
    let mut hasher = Sha256::new();
    hasher.update(b"org.sqlite-capsule.signature-inventory/1\0");
    while let Some(row) = rows
        .next()
        .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InvalidSignature))?
    {
        count = count.checked_add(1).ok_or_else(limit_exceeded)?;
        for field in [
            row.get::<_, String>(0)
                .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InvalidSignature))?
                .into_bytes(),
            row.get::<_, String>(1)
                .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InvalidSignature))?
                .into_bytes(),
            row.get::<_, Vec<u8>>(2)
                .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InvalidSignature))?,
            row.get::<_, Vec<u8>>(3)
                .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InvalidSignature))?,
            row.get::<_, Vec<u8>>(4)
                .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InvalidSignature))?,
            row.get::<_, String>(5)
                .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InvalidSignature))?
                .into_bytes(),
        ] {
            hasher.update((field.len() as u64).to_be_bytes());
            hasher.update(field);
        }
    }
    if count == 0 || count != source.signature_reports().len() as u64 {
        return Err(WorkspaceError::new(WorkspaceErrorCode::InvalidSignature));
    }
    Ok(lower_hex(&hasher.finalize()))
}

fn referenced_instance_assets(
    source: &VerifiedWorkspaceSource,
) -> Result<Vec<InstanceAssetRow>, WorkspaceError> {
    let instance = &source.identity().overview.instance;
    let mut ids = BTreeSet::new();
    if let Some(id) = &instance.icon_asset_id {
        ids.insert(id.clone());
    }
    if let Some(id) = &instance.cover_asset_id {
        ids.insert(id.clone());
    }
    let mut result = Vec::new();
    for id in ids {
        let row = source
            .verified
            .connection()
            .query_row(
                "SELECT id, media_type, content, sha256, width, height, description \
                 FROM capsule_instance_asset WHERE id = ?1",
                [&id],
                |row| {
                    Ok(InstanceAssetRow {
                        id: row.get(0)?,
                        media_type: row.get(1)?,
                        content: row.get(2)?,
                        sha256: row.get(3)?,
                        width: row.get(4)?,
                        height: row.get(5)?,
                        description: row.get(6)?,
                    })
                },
            )
            .map_err(|_| invalid_contract())?;
        result.push(row);
    }
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
fn reproduce_review(
    source: &VerifiedWorkspaceSource,
    target: &VerifiedWorkspaceSource,
    report: &UpgradeReviewReport,
    expected_proof: &TemplateStateProof,
    expected_assets: &[InstanceAssetRow],
    deadline: Instant,
    cancellation: &CancellationToken,
    max_rows: u64,
    max_stream_bytes: u64,
) -> Result<(), WorkspaceError> {
    require_complete_signature_inventory(source)?;
    require_complete_signature_inventory(target)?;
    require_upgrade_identity(source, target, &report.publisher_continuity.accepted_key_id)?;
    require_compatible_schema_contract(source, target)?;
    assert_inputs_current(source, target, deadline, cancellation)?;
    let proof = verify_template_state(
        target,
        &TemplateStateLimits {
            deadline: remaining(deadline)?,
            max_rows,
            max_stream_bytes,
        },
        cancellation,
    )?;
    if &proof != expected_proof
        || digest_serializable(&proof)? != report.target_template_state_sha256
        || reproduce_decisions(
            source,
            target,
            deadline,
            cancellation,
            max_rows,
            max_stream_bytes,
        )? != report.dataset_actions
        || capability_delta(source, target)? != report.capability_delta
        || signature_inventory_sha256(source)? != report.source.signature_inventory_sha256
        || signature_inventory_sha256(target)? != report.target_release.signature_inventory_sha256
        || referenced_instance_assets(source)? != expected_assets
        || review_digest(report)? != report.review_digest
    {
        return Err(stale_plan());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn transform_private(
    path: &Path,
    source: &VerifiedWorkspaceSource,
    target: &VerifiedWorkspaceSource,
    report: &UpgradeReviewReport,
    source_assets: &[InstanceAssetRow],
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
    let mut rows_remaining = plan.limits().max_rows_written().min(HARD_MAX_ROWS);
    let result = (|| -> Result<(), WorkspaceError> {
        connection
            .execute_batch("BEGIN IMMEDIATE; PRAGMA defer_foreign_keys=ON;")
            .map_err(|_| query_error(deadline, cancellation))?;
        let ordered = ordered_datasets(target.data_contract())?;

        for dataset in ordered.iter().rev() {
            let decision = decision_for(report, &dataset.id)?;
            if decision.action != UpgradeDatasetAction::Copy {
                continue;
            }
            for table in dataset.tables.iter().rev() {
                check(deadline, cancellation)?;
                let changed = connection
                    .execute(
                        &format!("DELETE FROM {}", quote_identifier(&table.name)),
                        [],
                    )
                    .map_err(|_| query_error(deadline, cancellation))?;
                spend_rows(&mut rows_remaining, changed as u64)?;
            }
        }
        for dataset in ordered {
            let decision = decision_for(report, &dataset.id)?;
            if decision.action != UpgradeDatasetAction::Copy {
                continue;
            }
            for table in &dataset.tables {
                copy_table_rows(
                    source.verified.connection(),
                    &connection,
                    table,
                    &mut rows_remaining,
                    deadline,
                    cancellation,
                )?;
            }
        }

        for sql in [
            "DELETE FROM capsule_grant",
            "DELETE FROM capsule_change_log",
            "DELETE FROM capsule_lineage_parent",
            "DELETE FROM capsule_lineage_event",
            "DELETE FROM capsule_instance_asset",
        ] {
            let changed = connection
                .execute(sql, [])
                .map_err(|_| query_error(deadline, cancellation))?;
            spend_rows(&mut rows_remaining, changed as u64)?;
        }
        for asset in source_assets {
            connection
                .execute(
                    "INSERT INTO capsule_instance_asset \
                     (id, media_type, content, sha256, width, height, description) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        asset.id,
                        asset.media_type,
                        asset.content,
                        asset.sha256,
                        asset.width,
                        asset.height,
                        asset.description,
                    ],
                )
                .map_err(|_| query_error(deadline, cancellation))?;
            spend_rows(&mut rows_remaining, 1)?;
        }
        let source_instance = &source.identity().overview.instance;
        let changed = connection
            .execute(
                "UPDATE capsule_instance SET \
                 capsule_id = ?1, revision_id = ?2, title = ?3, description = ?4, \
                 document_kind = ?5, tags_json = ?6, icon_asset_id = ?7, \
                 cover_asset_id = ?8, created_at = ?9, content_updated_at = ?10 \
                 WHERE id = 1",
                params![
                    report.output.capsule_id,
                    report.output.revision_id,
                    source_instance.title,
                    source_instance.description,
                    source_instance.document_kind,
                    serde_json::to_string(&source_instance.tags).map_err(|_| invalid_contract())?,
                    source_instance.icon_asset_id,
                    source_instance.cover_asset_id,
                    source_instance.created_at,
                    source_instance.content_updated_at,
                ],
            )
            .map_err(|_| query_error(deadline, cancellation))?;
        if changed != 1 {
            return Err(verification_failed());
        }
        spend_rows(&mut rows_remaining, 1)?;

        let details = crate::plan::canonical_json(&json!({
            "capability_delta": {
                "added": report.capability_delta.added,
                "changed": report.capability_delta.changed,
                "removed": report.capability_delta.removed,
                "requires_review": report.capability_delta.requires_review
            },
            "dataset_count": report.dataset_actions.len(),
            "publisher_key_id": report.publisher_continuity.accepted_key_id,
            "review_digest": report.review_digest,
            "target_template_state_sha256": report.target_template_state_sha256
        }))?;
        let details = String::from_utf8(details).map_err(|_| invalid_contract())?;
        connection
            .execute(
                "INSERT INTO capsule_lineage_event \
                 (event_id, sequence, operation, result_capsule_id, result_revision_id, \
                  occurred_at, application_digest, data_schema_id, data_schema_version, \
                  plan_digest, details_json) \
                 VALUES (?1, 1, 'application-upgrade', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    report.lineage.event_id,
                    report.output.capsule_id,
                    report.output.revision_id,
                    report.lineage.occurred_at,
                    report.output.application_digest,
                    report.output.data_schema_id,
                    i64::try_from(report.output.data_schema_version)
                        .map_err(|_| invalid_contract())?,
                    plan.plan_digest(),
                    details,
                ],
            )
            .map_err(|_| query_error(deadline, cancellation))?;
        spend_rows(&mut rows_remaining, 1)?;
        connection
            .execute(
                "INSERT INTO capsule_lineage_parent \
                 (event_id, ordinal, relation, parent_capsule_id, parent_revision_id, \
                  parent_file_sha256) VALUES (?1, 1, 'upgraded-from', ?2, ?3, ?4)",
                params![
                    report.lineage.event_id,
                    report.source.capsule_id,
                    report.source.revision_id,
                    report.source.file_sha256,
                ],
            )
            .map_err(|_| query_error(deadline, cancellation))?;
        connection
            .execute(
                "INSERT INTO capsule_lineage_parent \
                 (event_id, ordinal, relation, parent_capsule_id, parent_revision_id, \
                  parent_file_sha256) VALUES (?1, 2, 'application-release', ?2, ?3, ?4)",
                params![
                    report.lineage.event_id,
                    report.target_release.capsule_id,
                    report.target_release.revision_id,
                    report.target_release.file_sha256,
                ],
            )
            .map_err(|_| query_error(deadline, cancellation))?;
        spend_rows(&mut rows_remaining, 2)?;

        let foreign_key_failure: Option<i64> = connection
            .query_row(
                "SELECT 1 FROM pragma_foreign_key_check LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| query_error(deadline, cancellation))?;
        if foreign_key_failure.is_some() {
            return Err(verification_failed());
        }
        connection
            .execute_batch("COMMIT")
            .map_err(|_| query_error(deadline, cancellation))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = connection.execute_batch("ROLLBACK");
    }
    let _ = connection.progress_handler(0, None::<fn() -> bool>);
    result
}

fn decision_for<'a>(
    report: &'a UpgradeReviewReport,
    dataset_id: &str,
) -> Result<&'a UpgradeDatasetDecision, WorkspaceError> {
    report
        .dataset_actions
        .iter()
        .find(|decision| decision.dataset_id == dataset_id)
        .ok_or_else(invalid_contract)
}

fn ordered_datasets(contract: &DataContract) -> Result<Vec<&Dataset>, WorkspaceError> {
    let by_id = contract
        .datasets
        .iter()
        .map(|dataset| (dataset.id.as_str(), dataset))
        .collect::<BTreeMap<_, _>>();
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    let mut ordered = Vec::with_capacity(contract.datasets.len());
    fn visit<'a>(
        id: &'a str,
        by_id: &BTreeMap<&'a str, &'a Dataset>,
        visiting: &mut BTreeSet<&'a str>,
        visited: &mut BTreeSet<&'a str>,
        ordered: &mut Vec<&'a Dataset>,
    ) -> Result<(), WorkspaceError> {
        if visited.contains(id) {
            return Ok(());
        }
        if !visiting.insert(id) {
            return Err(invalid_contract());
        }
        let dataset = by_id.get(id).copied().ok_or_else(invalid_contract)?;
        for dependency in &dataset.dependencies {
            visit(&dependency.dataset_id, by_id, visiting, visited, ordered)?;
        }
        visiting.remove(id);
        visited.insert(id);
        ordered.push(dataset);
        Ok(())
    }
    for dataset in &contract.datasets {
        visit(
            &dataset.id,
            &by_id,
            &mut visiting,
            &mut visited,
            &mut ordered,
        )?;
    }
    Ok(ordered)
}

fn copy_table_rows(
    source: &Connection,
    output: &Connection,
    table: &crate::DatasetTable,
    rows_remaining: &mut u64,
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<(), WorkspaceError> {
    let columns = stored_columns(source, &table.name)?;
    if columns.is_empty() {
        return Err(invalid_contract());
    }
    let column_list = columns
        .iter()
        .map(|column| quote_identifier(column))
        .collect::<Vec<_>>()
        .join(",");
    let order = table
        .primary_key
        .iter()
        .map(|column| quote_identifier(column))
        .collect::<Vec<_>>()
        .join(",");
    let select_sql = format!(
        "SELECT {column_list} FROM {} ORDER BY {order}",
        quote_identifier(&table.name)
    );
    let placeholders = (1..=columns.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let insert_sql = format!(
        "INSERT INTO {} ({column_list}) VALUES ({placeholders})",
        quote_identifier(&table.name)
    );
    let mut select = source
        .prepare(&select_sql)
        .map_err(|_| invalid_contract())?;
    let mut rows = select.query([]).map_err(|_| invalid_contract())?;
    let mut insert = output
        .prepare_cached(&insert_sql)
        .map_err(|_| verification_failed())?;
    while let Some(row) = rows.next().map_err(|_| invalid_contract())? {
        check(deadline, cancellation)?;
        let values = (0..columns.len())
            .map(|index| {
                row.get::<_, SqlValue>(index)
                    .map_err(|_| invalid_contract())
            })
            .collect::<Result<Vec<_>, _>>()?;
        insert
            .execute(rusqlite::params_from_iter(values.iter()))
            .map_err(|_| query_error(deadline, cancellation))?;
        spend_rows(rows_remaining, 1)?;
    }
    Ok(())
}

fn stored_columns(connection: &Connection, table: &str) -> Result<Vec<String>, WorkspaceError> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_xinfo({})", quote_identifier(table)))
        .map_err(|_| invalid_contract())?;
    let mut rows = statement.query([]).map_err(|_| invalid_contract())?;
    let mut columns = Vec::new();
    while let Some(row) = rows.next().map_err(|_| invalid_contract())? {
        let hidden: i64 = row.get(6).map_err(|_| invalid_contract())?;
        if hidden == 0 {
            columns.push(row.get(1).map_err(|_| invalid_contract())?);
        }
    }
    Ok(columns)
}

fn spend_rows(remaining: &mut u64, count: u64) -> Result<(), WorkspaceError> {
    *remaining = remaining.checked_sub(count).ok_or_else(limit_exceeded)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn validate_upgrade_output(
    output: &VerifiedWorkspaceSource,
    source: &VerifiedWorkspaceSource,
    target: &VerifiedWorkspaceSource,
    report: &UpgradeReviewReport,
    expected_assets: &[InstanceAssetRow],
    plan: &LifecyclePlan,
    deadline: Instant,
    cancellation: &CancellationToken,
    max_rows: u64,
    max_stream_bytes: u64,
) -> Result<(), WorkspaceError> {
    check(deadline, cancellation)?;
    let identity = output.identity();
    let target_identity = target.identity();
    let source_instance = &source.identity().overview.instance;
    let output_instance = &identity.overview.instance;
    if identity.user_version != 3
        || identity.format_version != "0.3"
        || identity.runtime_protocol != target_identity.runtime_protocol
        || identity.app_id != target_identity.app_id
        || identity.app_version != target_identity.app_version
        || identity.entry_asset != target_identity.entry_asset
        || identity.permissions != target_identity.permissions
        || identity.overview.application != target_identity.overview.application
        || identity.overview.data_schema != target_identity.overview.data_schema
        || identity.capsule_id != report.output.capsule_id
        || output_instance.revision_id.as_deref() != Some(report.output.revision_id.as_str())
        || output_instance.title != source_instance.title
        || output_instance.description != source_instance.description
        || output_instance.document_kind != source_instance.document_kind
        || output_instance.tags != source_instance.tags
        || output_instance.icon_asset_id != source_instance.icon_asset_id
        || output_instance.cover_asset_id != source_instance.cover_asset_id
        || output_instance.created_at != source_instance.created_at
        || output_instance.content_updated_at != source_instance.content_updated_at
        || output.application_digest() != target.application_digest()
        || output.data_contract() != target.data_contract()
        || !output.has_complete_valid_signature_inventory()
        || signature_inventory_sha256(output)? != report.target_release.signature_inventory_sha256
        || referenced_instance_assets(output)? != expected_assets
    {
        return Err(verification_failed());
    }
    let all_instance_assets = all_instance_assets(output.verified.connection())?;
    if all_instance_assets != expected_assets {
        return Err(verification_failed());
    }
    for table in ["capsule_grant", "capsule_change_log"] {
        let count: i64 = output
            .verified
            .connection()
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .map_err(|_| verification_failed())?;
        if count != 0 {
            return Err(verification_failed());
        }
    }
    let states =
        capture_dataset_states(output, deadline, cancellation, max_rows, max_stream_bytes)?;
    for decision in &report.dataset_actions {
        if states.get(&decision.dataset_id) != Some(&decision.expected) {
            return Err(verification_failed());
        }
    }
    validate_upgrade_lineage(output.verified.connection(), report, plan)?;
    assert_inputs_current(source, target, deadline, cancellation)?;
    Ok(())
}

fn all_instance_assets(connection: &Connection) -> Result<Vec<InstanceAssetRow>, WorkspaceError> {
    let mut statement = connection
        .prepare(
            "SELECT id, media_type, content, sha256, width, height, description \
             FROM capsule_instance_asset ORDER BY id COLLATE BINARY",
        )
        .map_err(|_| verification_failed())?;
    statement
        .query_map([], |row| {
            Ok(InstanceAssetRow {
                id: row.get(0)?,
                media_type: row.get(1)?,
                content: row.get(2)?,
                sha256: row.get(3)?,
                width: row.get(4)?,
                height: row.get(5)?,
                description: row.get(6)?,
            })
        })
        .map_err(|_| verification_failed())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| verification_failed())
}

fn validate_upgrade_lineage(
    connection: &Connection,
    report: &UpgradeReviewReport,
    plan: &LifecyclePlan,
) -> Result<(), WorkspaceError> {
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
            "SELECT event_id, sequence, operation, result_capsule_id, result_revision_id, \
             occurred_at, application_digest, data_schema_id, data_schema_version, \
             plan_digest, details_json FROM capsule_lineage_event",
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
    let details = String::from_utf8(crate::plan::canonical_json(&json!({
        "capability_delta": {
            "added": report.capability_delta.added,
            "changed": report.capability_delta.changed,
            "removed": report.capability_delta.removed,
            "requires_review": report.capability_delta.requires_review
        },
        "dataset_count": report.dataset_actions.len(),
        "publisher_key_id": report.publisher_continuity.accepted_key_id,
        "review_digest": report.review_digest,
        "target_template_state_sha256": report.target_template_state_sha256
    }))?)
    .map_err(|_| verification_failed())?;
    if event.0 != report.lineage.event_id
        || event.1 != 1
        || event.2 != "application-upgrade"
        || event.3 != report.output.capsule_id
        || event.4 != report.output.revision_id
        || event.5 != report.lineage.occurred_at
        || event.6 != report.output.application_digest
        || event.7 != report.output.data_schema_id
        || event.8
            != i64::try_from(report.output.data_schema_version).map_err(|_| invalid_contract())?
        || event.9 != plan.plan_digest()
        || event.10 != details
    {
        return Err(verification_failed());
    }
    let mut statement = connection
        .prepare(
            "SELECT ordinal, relation, parent_capsule_id, parent_revision_id, parent_file_sha256 \
             FROM capsule_lineage_parent WHERE event_id = ?1 ORDER BY ordinal",
        )
        .map_err(|_| verification_failed())?;
    let parents = statement
        .query_map([&report.lineage.event_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(|_| verification_failed())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| verification_failed())?;
    let expected = vec![
        (
            1,
            "upgraded-from".to_owned(),
            Some(report.source.capsule_id.clone()),
            Some(report.source.revision_id.clone()),
            report.source.file_sha256.clone(),
        ),
        (
            2,
            "application-release".to_owned(),
            Some(report.target_release.capsule_id.clone()),
            Some(report.target_release.revision_id.clone()),
            report.target_release.file_sha256.clone(),
        ),
    ];
    if parents != expected {
        return Err(verification_failed());
    }
    Ok(())
}

fn reject_sidecars(path: &Path) -> Result<(), WorkspaceError> {
    for suffix in ["-journal", "-wal", "-shm"] {
        let mut sidecar = path.as_os_str().to_os_string();
        sidecar.push(suffix);
        if Path::new(&sidecar).exists() {
            return Err(output_failed());
        }
    }
    Ok(())
}

fn vacuum_private(
    path: &Path,
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<(), WorkspaceError> {
    check(deadline, cancellation)?;
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| output_failed())?;
    install_progress(&connection, deadline, cancellation)?;
    connection
        .execute_batch(
            "PRAGMA trusted_schema=OFF; \
             PRAGMA journal_mode=DELETE; \
             VACUUM;",
        )
        .map_err(|_| query_error(deadline, cancellation))?;
    let freelist: i64 = connection
        .pragma_query_value(None, "freelist_count", |row| row.get(0))
        .map_err(|_| verification_failed())?;
    let _ = connection.progress_handler(0, None::<fn() -> bool>);
    if freelist != 0 {
        return Err(verification_failed());
    }
    drop(connection);
    reject_sidecars(path)
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
            Some(move || {
                cancelled.load(std::sync::atomic::Ordering::Relaxed) || Instant::now() >= deadline
            }),
        )
        .map_err(|_| invalid_contract())
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

fn assert_inputs_current(
    source: &VerifiedWorkspaceSource,
    target: &VerifiedWorkspaceSource,
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<(), WorkspaceError> {
    source.assert_current_with_control(
        &WorkspaceLimits {
            deadline: remaining(deadline)?,
            ..WorkspaceLimits::default()
        },
        cancellation,
    )?;
    target.assert_current_with_control(
        &WorkspaceLimits {
            deadline: remaining(deadline)?,
            ..WorkspaceLimits::default()
        },
        cancellation,
    )
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

fn snapshot_held_file(
    source: &File,
    max_bytes: u64,
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<tempfile::NamedTempFile, sqlite_capsule_lifecycle::LifecycleError> {
    let mut reader = source.try_clone()?;
    reader.seek(SeekFrom::Start(0))?;
    let mut output = tempfile::NamedTempFile::new()?;
    let mut copied = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        if cancellation.is_cancelled() || Instant::now() >= deadline {
            return Err(sqlite_capsule_lifecycle::LifecycleError::PostPublishVerification);
        }
        let read = reader.read(&mut buffer)?;
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

fn generate_distinct_ids(
    connection: &Connection,
    source_revision_id: &str,
    target_revision_id: Option<&str>,
) -> Result<(String, String), WorkspaceError> {
    for _ in 0..8 {
        let revision = generate_uuid_v4(connection)?;
        let event = generate_uuid_v4(connection)?;
        if revision != source_revision_id
            && target_revision_id != Some(revision.as_str())
            && revision != event
        {
            return Ok((revision, event));
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

fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
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
    if std::env::var_os("SQLITE_CAPSULE_UPGRADE_CRASH_STAGE").is_some_and(|value| value == stage) {
        std::process::exit(97);
    }
}

#[cfg(not(test))]
const fn maybe_crash(_stage: &str) {}

const fn invalid_contract() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::InvalidContract)
}

const fn publisher_mismatch() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::PublisherMismatch)
}

const fn stale_plan() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::StalePlan)
}

const fn limit_exceeded() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::LimitExceeded)
}

const fn cancelled() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::Cancelled)
}

const fn output_failed() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::OutputPublishFailed)
}

const fn verification_failed() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::VerificationFailed)
}

#[cfg(test)]
mod tests {
    use std::{fs, process::Command};

    use ed25519_dalek::SigningKey;
    use rusqlite::Connection;
    use sqlite_capsule_crypto::{PROFILE_V03, application_digest, sign_digest_for_profile};

    use super::*;

    const CREATED: &str = "2026-08-20T07:05:16Z";
    const EXPIRES: &str = "9999-12-31T23:59:59Z";
    const DEVELOPMENT_SEED: &str =
        include_str!("../../../../compatibility/signed-app-v0.2/development-seed.hex");

    fn resign(connection: &Connection, seed_hex: &str) {
        connection
            .execute("DELETE FROM capsule_signature", [])
            .expect("clear signature");
        let digest = application_digest(connection).expect("application digest");
        let seed_hex = seed_hex.trim();
        let mut seed = [0_u8; 32];
        for (index, byte) in seed.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&seed_hex[index * 2..index * 2 + 2], 16).expect("seed hex");
        }
        let key = SigningKey::from_bytes(&seed);
        seed.fill(0);
        let envelope = sign_digest_for_profile(&key, digest, CREATED, PROFILE_V03).expect("sign");
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

    fn prepare_working(path: &Path) {
        let connection = Connection::open(path).expect("open working fixture");
        let icon = b"working-instance-icon";
        connection
            .execute(
                "UPDATE vector_domain SET note = 'working-user-sentinel' WHERE id = 'domain'",
                [],
            )
            .expect("working content");
        connection
            .execute(
                "UPDATE vector_settings SET value = 'working-setting' WHERE key = 'theme'",
                [],
            )
            .expect("working setting");
        connection
            .execute(
                "UPDATE capsule_instance SET title = 'Preserved working title', \
                 description = 'Preserved working description', tags_json = '[\"kept\",\"m07\"]', \
                 content_updated_at = '2026-08-20T06:00:00Z' WHERE id = 1",
                [],
            )
            .expect("working profile");
        connection
            .execute(
                "UPDATE capsule_instance_asset SET content = ?1, sha256 = ?2, \
                 description = 'Preserved working icon' WHERE id = 'instance-icon'",
                params![icon, lower_hex(&Sha256::digest(icon))],
            )
            .expect("working icon");
    }

    fn prepare_target(path: &Path, version: &str, permissions: &str, policies: &str) {
        let connection = Connection::open(path).expect("open target fixture");
        let asset = b"<html><body>target release sentinel</body></html>";
        connection
            .execute(
                "UPDATE capsule_manifest SET app_version = ?1, permissions_json = ?2, \
                 released_at = '2026-08-20T06:30:00Z' WHERE id = 1",
                params![version, permissions],
            )
            .expect("target manifest");
        connection
            .execute(
                "UPDATE capsule_asset SET content = ?1, sha256 = ?2, \
                 description = 'Target release asset' WHERE path = 'app/index.html'",
                params![asset, lower_hex(&Sha256::digest(asset))],
            )
            .expect("target asset");
        connection
            .execute(
                "UPDATE capsule_endpoint SET description = 'Target release endpoint' \
                 WHERE name = 'vector.write'",
                [],
            )
            .expect("target endpoint");
        connection
            .execute(
                "UPDATE vector_domain SET note = 'target-clean-content' WHERE id = 'domain'",
                [],
            )
            .expect("target clean content");
        connection
            .execute(
                "UPDATE vector_settings SET value = 'target-release-setting' WHERE key = 'theme'",
                [],
            )
            .expect("target setting");
        connection.execute_batch(policies).expect("target policies");
        resign(&connection, DEVELOPMENT_SEED);
        drop(connection);
        install_template_proof(path);
    }

    fn install_template_proof(path: &Path) {
        let target = VerifiedWorkspaceSource::open(path).expect("target before proof");
        let identity = target.identity();
        let schema = identity.overview.data_schema.as_ref().expect("schema");
        let datasets = target
            .data_contract()
            .datasets
            .iter()
            .map(|dataset| {
                let (row_count, digest) =
                    crate::template_state::dataset_state_for_test(&target, dataset)
                        .expect("target dataset state");
                json!({
                    "dataset_id": dataset.id,
                    "disposition": if row_count == 0 { "empty" } else { "seed" },
                    "stored_row_count": row_count,
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
        let proof = String::from_utf8(crate::plan::canonical_json(&proof).expect("proof JSON"))
            .expect("proof UTF-8");
        drop(target);
        let connection = Connection::open(path).expect("open target for proof");
        connection
            .execute(
                "INSERT OR REPLACE INTO capsule_doc \
                 (slug, title, media_type, content, sequence) VALUES \
                 (?1, 'SQLite Capsule authenticated template state', \
                  'application/vnd.sqlite-capsule.template-state+json', ?2, 0)",
                params![crate::TEMPLATE_STATE_DOC_SLUG, proof],
            )
            .expect("store target proof");
        resign(&connection, DEVELOPMENT_SEED);
    }

    struct UpgradeCase {
        _source_dir: tempfile::TempDir,
        _target_dir: tempfile::TempDir,
        source_path: PathBuf,
        target_path: PathBuf,
        output_path: PathBuf,
        accepted_key: String,
    }

    fn upgrade_case(name: &str) -> UpgradeCase {
        let (source_dir, source_path) = crate::tests::signed_fixture(&format!("{name}-working"));
        let (target_dir, target_path) = crate::tests::signed_fixture(&format!("{name}-release"));
        prepare_working(&source_path);
        prepare_target(
            &target_path,
            "3.1.0",
            r#"{"database.read":{"required":true},"database.write":{"required":true},"export":{"required":true},"network":{"value":"none"},"z":0,"é":"café","":"bmp","😀":"astral"}"#,
            "UPDATE capsule_dataset SET upgrade_policy = 'copy' WHERE id = 'content'; \
             UPDATE capsule_dataset SET upgrade_policy = 'target' WHERE id = 'settings';",
        );
        let source = VerifiedWorkspaceSource::open(&source_path).expect("working source");
        let accepted_key = source.signature_reports()[0].key_id.clone();
        drop(source);
        let output_path = source_dir
            .path()
            .join(format!("{name}-upgraded.sqlitecapsule"));
        UpgradeCase {
            _source_dir: source_dir,
            _target_dir: target_dir,
            source_path,
            target_path,
            output_path,
            accepted_key,
        }
    }

    fn request<'a>(case: &'a UpgradeCase) -> UpgradePlanRequest<'a> {
        UpgradePlanRequest {
            output_path: &case.output_path,
            plan_id: "c5f44498-1a23-4c15-a384-e8a4782d0984",
            created_at: CREATED,
            expires_at: EXPIRES,
            accepted_publisher_key_id: &case.accepted_key,
            max_output_bytes: sqlite_capsule_core::MAX_CAPSULE_BYTES,
            max_rows: HARD_MAX_ROWS,
            max_stream_bytes: HARD_MAX_STREAM_BYTES,
            deadline: HARD_DEADLINE,
        }
    }

    fn review(case: &UpgradeCase) -> UpgradeReview {
        let source = VerifiedWorkspaceSource::open(&case.source_path).expect("source review");
        let target = VerifiedWorkspaceSource::open(&case.target_path).expect("target review");
        prepare_upgrade_review(&source, &target, &request(case), &CancellationToken::new())
            .expect("prepare upgrade review")
    }

    fn execute(case: &UpgradeCase) -> PublishedUpgrade {
        let review = review(case);
        let plan = parse_upgrade_plan(&review.plan().canonical_bytes().expect("plan bytes"))
            .expect("parse approved plan");
        let source = VerifiedWorkspaceSource::open(&case.source_path).expect("source execute");
        let target = VerifiedWorkspaceSource::open(&case.target_path).expect("target execute");
        review
            .prepare(
                plan,
                &UpgradeApproval {
                    accepted_publisher_key_id: case.accepted_key.clone(),
                    capability_changes_accepted: true,
                },
                source,
                target,
                &WorkspaceLimits::default(),
                &CancellationToken::new(),
            )
            .expect("prepare retained upgrade")
            .stage()
            .expect("stage")
            .transform_and_validate()
            .expect("transform")
            .publish()
            .expect("publish")
    }

    #[test]
    fn same_schema_upgrade_starts_from_release_and_preserves_signed_policy_state() {
        let case = upgrade_case("happy");
        let source_before = fs::read(&case.source_path).expect("source before");
        let target_before = fs::read(&case.target_path).expect("target before");
        let target = VerifiedWorkspaceSource::open(&case.target_path).expect("target identity");
        let target_digest = *target.application_digest();
        drop(target);

        let review = review(&case);
        assert!(review.report().capability_delta.requires_review);
        assert_eq!(review.report().capability_delta.added, ["export"]);
        assert_eq!(
            review.report().dataset_actions[0].action,
            UpgradeDatasetAction::Copy
        );
        assert_eq!(
            review.report().dataset_actions[1].action,
            UpgradeDatasetAction::TakeTarget
        );
        assert_eq!(
            review.report().output.capsule_id,
            "11111111-1111-4111-8111-111111111111"
        );
        drop(review);

        let published = execute(&case);
        assert_eq!(published.path(), case.output_path);
        let output = VerifiedWorkspaceSource::open(published.path()).expect("reopen output");
        assert_eq!(*output.application_digest(), target_digest);
        assert_eq!(output.identity().app_version, "3.1.0");
        assert_eq!(
            output.identity().overview.instance.title,
            "Preserved working title"
        );
        assert_ne!(
            output.identity().overview.instance.revision_id.as_deref(),
            Some("22222222-2222-4222-8222-222222222222")
        );
        let content: String = output
            .verified
            .connection()
            .query_row(
                "SELECT note FROM vector_domain WHERE id = 'domain'",
                [],
                |row| row.get(0),
            )
            .expect("copied content");
        let setting: String = output
            .verified
            .connection()
            .query_row(
                "SELECT value FROM vector_settings WHERE key = 'theme'",
                [],
                |row| row.get(0),
            )
            .expect("target setting");
        let endpoint: String = output
            .verified
            .connection()
            .query_row(
                "SELECT description FROM capsule_endpoint WHERE name = 'vector.write'",
                [],
                |row| row.get(0),
            )
            .expect("target endpoint");
        assert_eq!(content, "working-user-sentinel");
        assert_eq!(setting, "target-release-setting");
        assert_eq!(endpoint, "Target release endpoint");
        assert_eq!(fs::read(&case.source_path).unwrap(), source_before);
        assert_eq!(fs::read(&case.target_path).unwrap(), target_before);
    }

    #[test]
    fn capability_increase_requires_explicit_approval() {
        let case = upgrade_case("capability-review");
        let review = review(&case);
        let plan = parse_upgrade_plan(&review.plan().canonical_bytes().unwrap()).unwrap();
        let source = VerifiedWorkspaceSource::open(&case.source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&case.target_path).unwrap();
        let error = match review.prepare(
            plan,
            &UpgradeApproval {
                accepted_publisher_key_id: case.accepted_key.clone(),
                capability_changes_accepted: false,
            },
            source,
            target,
            &WorkspaceLimits::default(),
            &CancellationToken::new(),
        ) {
            Ok(_) => panic!("capability increase must require approval"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::CapabilityReviewRequired);
        assert!(!case.output_path.exists());
    }

    #[test]
    fn rebuild_and_empty_omit_are_closed_positive_actions() {
        let case = upgrade_case("rebuild-omit");
        let source_connection = Connection::open(&case.source_path).unwrap();
        source_connection
            .execute(
                "UPDATE capsule_dataset SET required = 0 WHERE id = 'settings'",
                [],
            )
            .unwrap();
        resign(&source_connection, DEVELOPMENT_SEED);
        drop(source_connection);
        let connection = Connection::open(&case.target_path).unwrap();
        connection
            .execute_batch(
                "UPDATE capsule_dataset SET upgrade_policy = 'rebuild' WHERE id = 'content'; \
                 UPDATE capsule_dataset SET upgrade_policy = 'omit', required = 0 \
                    WHERE id = 'settings'; \
                 DELETE FROM vector_settings; \
                 DELETE FROM capsule_doc WHERE slug = 'org.sqlite-capsule.template-state';",
            )
            .unwrap();
        resign(&connection, DEVELOPMENT_SEED);
        drop(connection);
        install_template_proof(&case.target_path);

        let upgrade_review = review(&case);
        assert_eq!(
            upgrade_review.report().dataset_actions[0].action,
            UpgradeDatasetAction::Rebuild
        );
        assert_eq!(
            upgrade_review.report().dataset_actions[1].action,
            UpgradeDatasetAction::Omit
        );
        drop(upgrade_review);

        let published = execute(&case);
        let output = VerifiedWorkspaceSource::open(published.path()).unwrap();
        let rebuilt: String = output
            .verified
            .connection()
            .query_row(
                "SELECT note FROM vector_domain WHERE id = 'domain'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let omitted: i64 = output
            .verified
            .connection()
            .query_row("SELECT count(*) FROM vector_settings", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rebuilt, "target-clean-content");
        assert_eq!(omitted, 0);
    }

    #[test]
    fn cancellation_after_private_stage_fails_closed() {
        let case = upgrade_case("cancelled-stage");
        let source_before = fs::read(&case.source_path).unwrap();
        let target_before = fs::read(&case.target_path).unwrap();
        let upgrade_review = review(&case);
        let plan = parse_upgrade_plan(&upgrade_review.plan().canonical_bytes().unwrap()).unwrap();
        let source = VerifiedWorkspaceSource::open(&case.source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&case.target_path).unwrap();
        let cancellation = CancellationToken::new();
        let staged = upgrade_review
            .prepare(
                plan,
                &UpgradeApproval {
                    accepted_publisher_key_id: case.accepted_key.clone(),
                    capability_changes_accepted: true,
                },
                source,
                target,
                &WorkspaceLimits::default(),
                &cancellation,
            )
            .unwrap()
            .stage()
            .unwrap();
        cancellation.cancel();
        let error = match staged.transform_and_validate() {
            Ok(_) => panic!("cancelled staging must not validate"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::Cancelled);
        assert!(!case.output_path.exists());
        assert_eq!(fs::read(&case.source_path).unwrap(), source_before);
        assert_eq!(fs::read(&case.target_path).unwrap(), target_before);
    }

    #[test]
    fn later_source_change_invalidates_the_retained_review() {
        let case = upgrade_case("later-source-race");
        let target_before = fs::read(&case.target_path).unwrap();
        let upgrade_review = review(&case);
        let plan = parse_upgrade_plan(&upgrade_review.plan().canonical_bytes().unwrap()).unwrap();
        let connection = Connection::open(&case.source_path).unwrap();
        connection
            .execute(
                "UPDATE vector_domain SET note = 'changed-after-review' WHERE id = 'domain'",
                [],
            )
            .unwrap();
        drop(connection);
        let source = VerifiedWorkspaceSource::open(&case.source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&case.target_path).unwrap();
        let error = match upgrade_review.prepare(
            plan,
            &UpgradeApproval {
                accepted_publisher_key_id: case.accepted_key.clone(),
                capability_changes_accepted: true,
            },
            source,
            target,
            &WorkspaceLimits::default(),
            &CancellationToken::new(),
        ) {
            Ok(_) => panic!("changed source must invalidate review"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::StalePlan);
        assert!(!case.output_path.exists());
        assert_eq!(fs::read(&case.target_path).unwrap(), target_before);
    }

    #[test]
    fn semver_precedence_is_strict_and_build_metadata_does_not_upgrade() {
        assert!(version_is_strictly_newer("1.0.0-alpha.1", "1.0.0").unwrap());
        assert!(version_is_strictly_newer("1.0.0", "1.0.1-alpha").unwrap());
        assert!(!version_is_strictly_newer("1.0.0+one", "1.0.0+two").unwrap());
        assert!(
            version_is_strictly_newer("18446744073709551616.0.0", "18446744073709551617.0.0")
                .unwrap()
        );
        assert!(
            version_is_strictly_newer("1.0.0-18446744073709551616", "1.0.0-18446744073709551617")
                .unwrap()
        );
        assert!(parse_release_version("01.0.0").is_err());
        assert!(parse_release_version("1.0").is_err());
        assert!(parse_release_version("1.0.0-").is_err());
        assert!(parse_release_version("1.0.0+").is_err());
        assert!(parse_release_version("1.0.0-+build").is_err());
        assert!(parse_release_version(&format!("1.0.0+{}", "a".repeat(123))).is_err());
    }

    #[test]
    fn admission_rejects_same_version_schema_app_key_policy_and_dirty_release() {
        for (name, mutation, code) in [
            (
                "same-version",
                "UPDATE capsule_manifest SET app_version = '3.0.0' WHERE id = 1",
                WorkspaceErrorCode::VersionNotNewer,
            ),
            (
                "downgrade",
                "UPDATE capsule_manifest SET app_version = '2.9.9' WHERE id = 1",
                WorkspaceErrorCode::VersionNotNewer,
            ),
            (
                "different-app",
                "UPDATE capsule_manifest SET app_id = 'org.example.other' WHERE id = 1",
                WorkspaceErrorCode::IncompatibleApplication,
            ),
            (
                "different-schema",
                "UPDATE capsule_manifest SET data_schema_id = 'org.example.other-data' WHERE id = 1",
                WorkspaceErrorCode::IncompatibleSchema,
            ),
            (
                "migrate-policy",
                "UPDATE capsule_dataset SET upgrade_policy = 'migrate' WHERE id = 'content'",
                WorkspaceErrorCode::UnsupportedOperation,
            ),
            (
                "forbid-policy",
                "UPDATE capsule_dataset SET upgrade_policy = 'forbid' WHERE id = 'content'",
                WorkspaceErrorCode::UnsupportedOperation,
            ),
        ] {
            let case = upgrade_case(name);
            let connection = Connection::open(&case.target_path).unwrap();
            connection.execute_batch(mutation).unwrap();
            resign(&connection, DEVELOPMENT_SEED);
            drop(connection);
            if !matches!(code, WorkspaceErrorCode::UnsupportedOperation) {
                install_template_proof(&case.target_path);
            }
            let source = VerifiedWorkspaceSource::open(&case.source_path).unwrap();
            let target = VerifiedWorkspaceSource::open(&case.target_path).unwrap();
            let error = match prepare_upgrade_review(
                &source,
                &target,
                &request(&case),
                &CancellationToken::new(),
            ) {
                Ok(_) => panic!("{name} must fail"),
                Err(error) => error,
            };
            assert_eq!(error.kind(), code, "case {name}");
            assert!(!case.output_path.exists());
        }

        let case = upgrade_case("nonempty-omit-policy");
        let source_connection = Connection::open(&case.source_path).unwrap();
        source_connection
            .execute(
                "UPDATE capsule_dataset SET required = 0 WHERE id = 'content'",
                [],
            )
            .unwrap();
        resign(&source_connection, DEVELOPMENT_SEED);
        drop(source_connection);
        let target_connection = Connection::open(&case.target_path).unwrap();
        target_connection
            .execute_batch(
                "UPDATE capsule_dataset SET upgrade_policy = 'omit', required = 0 \
                    WHERE id = 'content'; \
                 DELETE FROM capsule_doc WHERE slug = 'org.sqlite-capsule.template-state';",
            )
            .unwrap();
        resign(&target_connection, DEVELOPMENT_SEED);
        drop(target_connection);
        install_template_proof(&case.target_path);
        let source = VerifiedWorkspaceSource::open(&case.source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&case.target_path).unwrap();
        let error = match prepare_upgrade_review(
            &source,
            &target,
            &request(&case),
            &CancellationToken::new(),
        ) {
            Ok(_) => panic!("non-empty omit policy must fail"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::InvalidContract);
        assert!(!case.output_path.exists());

        let case = upgrade_case("wrong-key");
        let connection = Connection::open(&case.target_path).unwrap();
        resign(
            &connection,
            "7777777777777777777777777777777777777777777777777777777777777777",
        );
        let source = VerifiedWorkspaceSource::open(&case.source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&case.target_path).unwrap();
        let error = match prepare_upgrade_review(
            &source,
            &target,
            &request(&case),
            &CancellationToken::new(),
        ) {
            Ok(_) => panic!("different signing key must fail"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::PublisherMismatch);

        let case = upgrade_case("dirty-release");
        let connection = Connection::open(&case.target_path).unwrap();
        connection
            .execute("UPDATE vector_domain SET note = 'dirty-after-proof'", [])
            .unwrap();
        drop(connection);
        let source = VerifiedWorkspaceSource::open(&case.source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&case.target_path).unwrap();
        let error = match prepare_upgrade_review(
            &source,
            &target,
            &request(&case),
            &CancellationToken::new(),
        ) {
            Ok(_) => panic!("dirty release must fail"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::InvalidContract);

        let case = upgrade_case("invalid-signature");
        let connection = Connection::open(&case.target_path).unwrap();
        connection
            .execute(
                "UPDATE capsule_application SET description = 'changed after signing' WHERE id = 1",
                [],
            )
            .unwrap();
        drop(connection);
        let error = match VerifiedWorkspaceSource::open(&case.target_path) {
            Ok(_) => panic!("invalid signature must fail during verified admission"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::InvalidSignature);
    }

    #[test]
    fn existing_destination_and_plan_tamper_fail_before_publication() {
        let case = upgrade_case("destination-race");
        let first_review = review(&case);
        let plan_bytes = first_review.plan().canonical_bytes().unwrap();
        fs::write(&case.output_path, b"destination race").unwrap();
        let source = VerifiedWorkspaceSource::open(&case.source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&case.target_path).unwrap();
        let error = match first_review.prepare(
            parse_upgrade_plan(&plan_bytes).unwrap(),
            &UpgradeApproval {
                accepted_publisher_key_id: case.accepted_key.clone(),
                capability_changes_accepted: true,
            },
            source,
            target,
            &WorkspaceLimits::default(),
            &CancellationToken::new(),
        ) {
            Ok(prepared) => match prepared.stage() {
                Ok(_) => panic!("destination must remain create-new"),
                Err(error) => error,
            },
            Err(error) => error,
        };
        assert!(matches!(
            error.kind(),
            WorkspaceErrorCode::DestinationExists | WorkspaceErrorCode::StalePlan
        ));
        assert_eq!(fs::read(&case.output_path).unwrap(), b"destination race");

        let case = upgrade_case("plan-tamper");
        let review = review(&case);
        let mut value: JsonValue =
            serde_json::from_slice(&review.plan().canonical_bytes().unwrap()).unwrap();
        value["decisions"][1]["parameters"]["expected_state_sha256"] =
            JsonValue::String("0".repeat(64));
        let digest = canonical_digest_value(&value).unwrap();
        value["plan_digest"] = JsonValue::String(digest);
        let tampered = parse_upgrade_plan(&serde_json::to_vec(&value).unwrap()).unwrap();
        let source = VerifiedWorkspaceSource::open(&case.source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&case.target_path).unwrap();
        let error = match review.prepare(
            tampered,
            &UpgradeApproval {
                accepted_publisher_key_id: case.accepted_key.clone(),
                capability_changes_accepted: true,
            },
            source,
            target,
            &WorkspaceLimits::default(),
            &CancellationToken::new(),
        ) {
            Ok(_) => panic!("recomputed public plan is not retained authority"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::StalePlan);
    }

    #[test]
    fn upgrade_crash_worker() {
        let Some(stage) = std::env::var_os("SQLITE_CAPSULE_UPGRADE_CRASH_STAGE") else {
            return;
        };
        let source_path = PathBuf::from(std::env::var_os("SQLITE_CAPSULE_UPGRADE_SOURCE").unwrap());
        let target_path = PathBuf::from(std::env::var_os("SQLITE_CAPSULE_UPGRADE_TARGET").unwrap());
        let output_path = PathBuf::from(std::env::var_os("SQLITE_CAPSULE_UPGRADE_OUTPUT").unwrap());
        let source = VerifiedWorkspaceSource::open(&source_path).unwrap();
        let accepted_key = source.signature_reports()[0].key_id.clone();
        let target = VerifiedWorkspaceSource::open(&target_path).unwrap();
        let request = UpgradePlanRequest {
            output_path: &output_path,
            plan_id: "c5f44498-1a23-4c15-a384-e8a4782d0984",
            created_at: CREATED,
            expires_at: EXPIRES,
            accepted_publisher_key_id: &accepted_key,
            max_output_bytes: sqlite_capsule_core::MAX_CAPSULE_BYTES,
            max_rows: HARD_MAX_ROWS,
            max_stream_bytes: HARD_MAX_STREAM_BYTES,
            deadline: HARD_DEADLINE,
        };
        let review =
            prepare_upgrade_review(&source, &target, &request, &CancellationToken::new()).unwrap();
        let plan = parse_upgrade_plan(&review.plan().canonical_bytes().unwrap()).unwrap();
        let source = VerifiedWorkspaceSource::open(&source_path).unwrap();
        let target = VerifiedWorkspaceSource::open(&target_path).unwrap();
        let prepared = review
            .prepare(
                plan,
                &UpgradeApproval {
                    accepted_publisher_key_id: accepted_key,
                    capability_changes_accepted: true,
                },
                source,
                target,
                &WorkspaceLimits::default(),
                &CancellationToken::new(),
            )
            .unwrap();
        let validated = prepared.stage().unwrap().transform_and_validate().unwrap();
        if stage == "postrename-reopened" {
            let _ = validated.publish_with_hook(|| std::process::exit(97));
        }
        panic!("unexpected crash stage");
    }

    #[test]
    fn crash_stage_matrix_never_publishes_a_partial_output() {
        for stage in [
            "private-created",
            "release-snapshot-copied",
            "transformed",
            "vacuumed",
            "sealed-and-verified",
            "postrename-reopened",
        ] {
            let case = upgrade_case(&format!("crash-{stage}"));
            let source_before = fs::read(&case.source_path).unwrap();
            let target_before = fs::read(&case.target_path).unwrap();
            let status = Command::new(std::env::current_exe().unwrap())
                .arg("upgrade::tests::upgrade_crash_worker")
                .arg("--exact")
                .env("SQLITE_CAPSULE_UPGRADE_CRASH_STAGE", stage)
                .env("SQLITE_CAPSULE_UPGRADE_SOURCE", &case.source_path)
                .env("SQLITE_CAPSULE_UPGRADE_TARGET", &case.target_path)
                .env("SQLITE_CAPSULE_UPGRADE_OUTPUT", &case.output_path)
                .status()
                .unwrap();
            assert_eq!(status.code(), Some(97), "stage {stage}");
            if stage == "postrename-reopened" {
                VerifiedWorkspaceSource::open(&case.output_path)
                    .expect("complete verified postrename residue");
            } else {
                assert!(!case.output_path.exists(), "stage {stage}");
            }
            assert_eq!(fs::read(&case.source_path).unwrap(), source_before);
            assert_eq!(fs::read(&case.target_path).unwrap(), target_before);
        }
    }
}

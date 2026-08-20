//! Host-bound lifecycle plan authority.
//!
//! Parsed JSON is review data, never execution authority. `PreparedPlan` is
//! non-serializable and retains every verified input plus a one-use destination
//! reservation. Execution consumes it and must rebind inputs again immediately
//! before any transform and publication.

use std::{
    ffi::OsStr,
    path::Path,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use sqlite_capsule_lifecycle::{DestinationReservation, DirectoryIdentity, SourceIdentity};

use crate::{
    CancellationToken, InputRole, LifecyclePlan, ParentFilesystemIdentity, PlanCapsuleIdentity,
    PlanInput, SourceFilesystemIdentity, VerifiedWorkspaceSource, WorkspaceError,
    WorkspaceErrorCode, WorkspaceLimits,
};

pub struct PreparedPlan {
    plan: LifecyclePlan,
    inputs: Vec<PreparedPlanInput>,
    destination: DestinationReservation,
    deadline: Instant,
}

pub struct PreparedPlanInput {
    role: InputRole,
    source: VerifiedWorkspaceSource,
}

impl PreparedPlan {
    pub fn prepare(
        plan: LifecyclePlan,
        limits: &WorkspaceLimits,
        cancellation: &CancellationToken,
    ) -> Result<Self, WorkspaceError> {
        Self::prepare_at(plan, SystemTime::now(), limits, cancellation)
    }

    pub(crate) fn prepare_at(
        plan: LifecyclePlan,
        now: SystemTime,
        limits: &WorkspaceLimits,
        cancellation: &CancellationToken,
    ) -> Result<Self, WorkspaceError> {
        validate_time_window(&plan, now)?;
        validate_exact_v03_bindings(&plan)?;
        let operation_budget = Duration::from_millis(plan.limits().deadline_ms())
            .min(limits.deadline)
            .min(Duration::from_secs(30));
        if operation_budget.is_zero() {
            return Err(WorkspaceError::new(WorkspaceErrorCode::InvalidContract));
        }
        let deadline = Instant::now()
            .checked_add(operation_budget)
            .ok_or_else(|| WorkspaceError::new(WorkspaceErrorCode::LimitExceeded))?;
        let mut inputs = Vec::with_capacity(plan.inputs().len());
        let mut total_input_bytes = 0_u64;
        for input in plan.inputs() {
            total_input_bytes = total_input_bytes
                .checked_add(input.size_bytes())
                .ok_or_else(|| WorkspaceError::new(WorkspaceErrorCode::LimitExceeded))?;
            if total_input_bytes > plan.limits().max_input_bytes() {
                return Err(WorkspaceError::new(WorkspaceErrorCode::LimitExceeded));
            }
            let mut remaining_limits = limits.clone();
            remaining_limits.deadline = deadline.saturating_duration_since(Instant::now());
            remaining_limits.max_capsule_bytes = input.size_bytes().min(
                plan.limits()
                    .max_input_bytes()
                    .saturating_sub(total_input_bytes.saturating_sub(input.size_bytes())),
            );
            let source = VerifiedWorkspaceSource::open_with_control_expected_binding(
                Path::new(input.path_hint()),
                &remaining_limits,
                cancellation,
                Some(input.size_bytes()),
                Some(parse_sha256(input.file_sha256())?),
            )?;
            bind_input(input, &source)?;
            inputs.push(PreparedPlanInput {
                role: input.role(),
                source,
            });
        }
        bind_expected_release(&plan, &inputs)?;
        let destination_path = Path::new(plan.output().path());
        let parent = destination_path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let parent = if parent.is_absolute() {
            parent.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InternalError))?
                .join(parent)
        };
        if parent.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        }) {
            return Err(WorkspaceError::new(WorkspaceErrorCode::InvalidContract));
        }
        let identities: Vec<SourceIdentity> = inputs
            .iter()
            .map(|input| input.source.source_identity().clone())
            .collect();
        let destination = DestinationReservation::reserve(
            &parent,
            OsStr::new(plan.output().leaf_name()),
            &identities,
        )
        .map_err(map_destination_error)?;
        bind_parent(plan.output().parent_identity(), destination.identity())?;
        Ok(Self {
            plan,
            inputs,
            destination,
            deadline,
        })
    }

    pub fn plan(&self) -> &LifecyclePlan {
        &self.plan
    }

    pub fn inputs(&self) -> &[PreparedPlanInput] {
        &self.inputs
    }

    pub fn assert_current(
        &self,
        limits: &WorkspaceLimits,
        cancellation: &CancellationToken,
    ) -> Result<(), WorkspaceError> {
        self.assert_current_at(SystemTime::now(), limits, cancellation)
    }

    pub(crate) fn assert_current_at(
        &self,
        now: SystemTime,
        limits: &WorkspaceLimits,
        cancellation: &CancellationToken,
    ) -> Result<(), WorkspaceError> {
        validate_time_window(&self.plan, now)?;
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(WorkspaceError::new(WorkspaceErrorCode::LimitExceeded));
        }
        let mut limits = limits.clone();
        limits.deadline = limits.deadline.min(remaining);
        for (declared, prepared) in self.plan.inputs().iter().zip(&self.inputs) {
            prepared
                .source
                .assert_current_with_control(&limits, cancellation)?;
            bind_input(declared, &prepared.source)?;
        }
        self.destination
            .assert_reserved_current()
            .map_err(map_prepared_destination_error)
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        LifecyclePlan,
        Vec<PreparedPlanInput>,
        DestinationReservation,
        Instant,
    ) {
        (self.plan, self.inputs, self.destination, self.deadline)
    }
}

fn validate_exact_v03_bindings(plan: &LifecyclePlan) -> Result<(), WorkspaceError> {
    if plan.operation() != crate::Operation::Duplicate {
        return Err(WorkspaceError::new(
            WorkspaceErrorCode::UnsupportedOperation,
        ));
    }
    if plan.inputs().len() != 1
        || plan.inputs()[0].role() != InputRole::Source
        || plan.decisions().len() != 1
        || plan.decisions()[0].scope() != crate::DecisionScope::Application
        || plan.decisions()[0].action() != "copy-exact-snapshot"
        || !plan.decisions()[0].parameters_are_empty()
        // Exact snapshot duplication inspects/writes zero logical domain rows,
        // so any positive caller-lowered budget is satisfied. Values above the
        // host profile cap are never accepted as an authority increase.
        || !plan.limits().row_budgets_within_duplicate_profile()
        || plan.inputs().iter().any(|input| {
            let capsule = input.capsule();
            capsule.format_version() != "0.3"
                || capsule.capsule_id().is_none()
                || capsule.revision_id().is_none()
                || capsule.application_digest().is_none()
                || capsule.data_schema_id().is_none()
                || capsule.data_schema_version().is_none()
                || capsule.publisher_key_id().is_none()
        })
        || plan.expected().capsule_id().is_none()
        || plan.expected().revision_id().is_none()
        || plan.expected().application_digest().is_none()
        || plan.expected().data_schema_id().is_none()
        || plan.expected().data_schema_version().is_none()
    {
        return Err(WorkspaceError::new(WorkspaceErrorCode::InvalidContract));
    }
    Ok(())
}

impl PreparedPlanInput {
    pub fn role(&self) -> InputRole {
        self.role
    }

    pub fn source(&self) -> &VerifiedWorkspaceSource {
        &self.source
    }
}

fn bind_input(plan: &PlanInput, source: &VerifiedWorkspaceSource) -> Result<(), WorkspaceError> {
    let live = source.source_identity();
    let identity = source.identity();
    let overview = &identity.overview;
    let data_schema = overview.data_schema.as_ref();
    let contract = plan.capsule();
    let application_digest_hex = lower_hex(source.application_digest());
    let signatures = source.signature_reports();
    let source_sha256 = lower_hex(&source.verified.source_sha256);
    if plan.size_bytes() != live.bytes
        || plan.file_sha256() != source_sha256
        || plan.snapshot_sha256() != source_sha256
        || !bind_source_identity(plan.filesystem_identity(), live)
        || !bind_capsule_identity(contract, source, &application_digest_hex)
        || !signatures.iter().any(|signature| {
            signature.cryptographically_valid
                && signature.digest_matches
                && contract
                    .publisher_key_id()
                    .is_none_or(|key_id| key_id == signature.key_id)
        })
        || data_schema.is_none()
    {
        return Err(WorkspaceError::new(WorkspaceErrorCode::StalePlan));
    }
    Ok(())
}

pub(crate) fn rebind_prepared_inputs(
    plan: &LifecyclePlan,
    inputs: &[PreparedPlanInput],
    now: SystemTime,
    limits: &WorkspaceLimits,
    cancellation: &CancellationToken,
) -> Result<(), WorkspaceError> {
    validate_time_window(plan, now)?;
    if inputs.len() != plan.inputs().len() {
        return Err(WorkspaceError::new(WorkspaceErrorCode::InvalidContract));
    }
    for (declared, prepared) in plan.inputs().iter().zip(inputs) {
        prepared
            .source
            .assert_current_with_control(limits, cancellation)?;
        bind_input(declared, &prepared.source)?;
    }
    Ok(())
}

fn bind_capsule_identity(
    plan: &PlanCapsuleIdentity,
    source: &VerifiedWorkspaceSource,
    application_digest_hex: &str,
) -> bool {
    let identity = source.identity();
    let overview = &identity.overview;
    let schema = overview.data_schema.as_ref();
    plan.app_id() == identity.app_id
        && plan.format_version() == identity.format_version
        && plan.app_version() == identity.app_version
        && plan
            .capsule_id()
            .is_none_or(|value| value == identity.capsule_id)
        && plan
            .revision_id()
            .is_none_or(|value| overview.instance.revision_id.as_deref() == Some(value))
        && plan
            .application_digest()
            .is_none_or(|value| value == application_digest_hex)
        && plan
            .data_schema_id()
            .is_none_or(|value| schema.is_some_and(|schema| schema.data_schema_id == value))
        && plan.data_schema_version().is_none_or(|value| {
            i64::try_from(value).ok().is_some_and(|value| {
                schema.is_some_and(|schema| schema.data_schema_version == value)
            })
        })
}

fn bind_source_identity(plan: &SourceFilesystemIdentity, live: &SourceIdentity) -> bool {
    plan.platform() == std::env::consts::OS
        && plan.volume_or_device() == live.device.to_string()
        && plan.file_id_or_inode() == live.stable_file_id
        && plan.modified_ns() == live.modified_ns
}

fn bind_expected_release(
    plan: &LifecyclePlan,
    inputs: &[PreparedPlanInput],
) -> Result<(), WorkspaceError> {
    let source = inputs
        .iter()
        .find(|input| input.role == InputRole::Source)
        .ok_or_else(|| WorkspaceError::new(WorkspaceErrorCode::InvalidContract))?;
    let expected = plan.expected();
    let identity = source.source.identity();
    let schema = identity.overview.data_schema.as_ref();
    let source_digest = lower_hex(source.source.application_digest());
    if expected.app_id() != identity.app_id
        || plan.decisions()[0].subject() != identity.app_id
        || expected.capsule_id() != Some(identity.capsule_id.as_str())
        || expected.revision_id() != identity.overview.instance.revision_id.as_deref()
        || expected
            .application_digest()
            .is_some_and(|value| value != source_digest)
        || expected
            .data_schema_id()
            .is_some_and(|value| !schema.is_some_and(|schema| schema.data_schema_id == value))
        || expected.data_schema_version().is_some_and(|value| {
            i64::try_from(value).ok().is_none_or(|value| {
                !schema.is_some_and(|schema| schema.data_schema_version == value)
            })
        })
    {
        return Err(WorkspaceError::new(WorkspaceErrorCode::InvalidContract));
    }
    let expected_publisher = plan
        .inputs()
        .iter()
        .find(|input| input.role() == InputRole::Source)
        .and_then(|input| input.capsule().publisher_key_id());
    if expected_publisher.is_none() {
        return Err(WorkspaceError::new(WorkspaceErrorCode::InvalidContract));
    }
    Ok(())
}

pub(crate) fn validate_output_expected(
    plan: &LifecyclePlan,
    output: &VerifiedWorkspaceSource,
) -> Result<(), WorkspaceError> {
    let expected = plan.expected();
    let identity = output.identity();
    let schema = identity.overview.data_schema.as_ref();
    let digest = lower_hex(output.application_digest());
    let expected_publisher = plan
        .inputs()
        .iter()
        .find(|input| input.role() == InputRole::Source)
        .and_then(|input| input.capsule().publisher_key_id());
    let output_signatures = output.signature_reports();
    if expected.app_id() != identity.app_id
        || expected
            .capsule_id()
            .is_some_and(|value| value != identity.capsule_id)
        || expected
            .revision_id()
            .is_some_and(|value| identity.overview.instance.revision_id.as_deref() != Some(value))
        || expected
            .data_schema_id()
            .is_some_and(|value| !schema.is_some_and(|schema| schema.data_schema_id == value))
        || expected.data_schema_version().is_some_and(|value| {
            i64::try_from(value).ok().is_none_or(|value| {
                !schema.is_some_and(|schema| schema.data_schema_version == value)
            })
        })
    {
        return Err(WorkspaceError::new(WorkspaceErrorCode::VerificationFailed));
    }
    if expected
        .application_digest()
        .is_some_and(|value| value != digest)
    {
        return Err(WorkspaceError::new(WorkspaceErrorCode::SignatureChanged));
    }
    if !expected_publisher.is_some_and(|key_id| {
        output_signatures.iter().any(|signature| {
            signature.key_id == key_id
                && signature.cryptographically_valid
                && signature.digest_matches
        })
    }) {
        return Err(WorkspaceError::new(WorkspaceErrorCode::PublisherMismatch));
    }
    Ok(())
}

pub(crate) fn validate_operation_specific(
    plan: &LifecyclePlan,
    inputs: &[PreparedPlanInput],
    output: &VerifiedWorkspaceSource,
) -> Result<(), WorkspaceError> {
    match plan.operation() {
        crate::Operation::Duplicate => {
            let source = inputs
                .iter()
                .find(|input| input.role() == InputRole::Source)
                .ok_or_else(|| WorkspaceError::new(WorkspaceErrorCode::InvalidContract))?;
            let source_identity = source.source().identity();
            let output_identity = output.identity();
            if source.source().verified.source_sha256 != output.verified.source_sha256
                || source_identity.app_id != output_identity.app_id
                || source_identity.app_version != output_identity.app_version
                || source_identity.capsule_id != output_identity.capsule_id
                || source_identity.overview.instance.revision_id
                    != output_identity.overview.instance.revision_id
                || source_identity.overview.data_schema != output_identity.overview.data_schema
            {
                return Err(WorkspaceError::new(WorkspaceErrorCode::VerificationFailed));
            }
            Ok(())
        }
        _ => Err(WorkspaceError::new(
            WorkspaceErrorCode::UnsupportedOperation,
        )),
    }
}

fn bind_parent(
    plan: &ParentFilesystemIdentity,
    live: &DirectoryIdentity,
) -> Result<(), WorkspaceError> {
    if plan.platform() != std::env::consts::OS
        || plan.volume_or_device() != live.device.to_string()
        || plan.file_id_or_inode() != live.stable_file_id
    {
        return Err(WorkspaceError::new(WorkspaceErrorCode::StalePlan));
    }
    Ok(())
}

pub(crate) fn map_prepared_destination_error(
    error: sqlite_capsule_lifecycle::LifecycleError,
) -> WorkspaceError {
    use sqlite_capsule_lifecycle::LifecycleError;
    match error {
        LifecycleError::UnsafeDestinationParent | LifecycleError::Replaced => {
            WorkspaceError::new(WorkspaceErrorCode::StalePlan)
        }
        other => map_destination_error(other),
    }
}

pub(crate) fn validate_time_window(
    plan: &LifecyclePlan,
    now: SystemTime,
) -> Result<(), WorkspaceError> {
    let created = parse_utc_seconds(plan.created_at())?;
    let expires = parse_utc_seconds(plan.expires_at())?;
    let now = now
        .duration_since(UNIX_EPOCH)
        .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InvalidContract))?
        .as_secs();
    if now < created {
        return Err(WorkspaceError::new(WorkspaceErrorCode::StalePlan));
    }
    if now >= expires {
        return Err(WorkspaceError::new(WorkspaceErrorCode::SessionExpired));
    }
    Ok(())
}

pub(crate) fn parse_utc_seconds(value: &str) -> Result<u64, WorkspaceError> {
    let year = value[0..4].parse::<i64>().map_err(|_| invalid_contract())?;
    let month = value[5..7].parse::<i64>().map_err(|_| invalid_contract())?;
    let day = value[8..10]
        .parse::<i64>()
        .map_err(|_| invalid_contract())?;
    let hour = value[11..13]
        .parse::<i64>()
        .map_err(|_| invalid_contract())?;
    let minute = value[14..16]
        .parse::<i64>()
        .map_err(|_| invalid_contract())?;
    let second = value[17..19]
        .parse::<i64>()
        .map_err(|_| invalid_contract())?;
    let adjusted_year = year - i64::from(month <= 2);
    let era = adjusted_year.div_euclid(400);
    let year_of_era = adjusted_year - era * 400;
    let adjusted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * adjusted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    u64::try_from(days * 86_400 + hour * 3_600 + minute * 60 + second)
        .map_err(|_| invalid_contract())
}

fn lower_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn parse_sha256(value: &str) -> Result<[u8; 32], WorkspaceError> {
    if value.len() != 64 {
        return Err(invalid_contract());
    }
    let mut output = [0_u8; 32];
    for (index, chunk) in value.as_bytes().chunks_exact(2).enumerate() {
        let high = hex_nibble(chunk[0]).ok_or_else(invalid_contract)?;
        let low = hex_nibble(chunk[1]).ok_or_else(invalid_contract)?;
        output[index] = (high << 4) | low;
    }
    Ok(output)
}

const fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

pub(crate) fn map_destination_error(
    error: sqlite_capsule_lifecycle::LifecycleError,
) -> WorkspaceError {
    use sqlite_capsule_lifecycle::LifecycleError;
    match error {
        LifecycleError::DestinationExists => {
            WorkspaceError::new(WorkspaceErrorCode::DestinationExists)
        }
        LifecycleError::UnsafeDestinationLeaf | LifecycleError::UnsafeDestinationParent => {
            WorkspaceError::new(WorkspaceErrorCode::InvalidContract)
        }
        LifecycleError::DestinationAliasesInput => {
            WorkspaceError::new(WorkspaceErrorCode::DestinationAliasesInput)
        }
        LifecycleError::PostPublishVerification => {
            WorkspaceError::new(WorkspaceErrorCode::PostpublishVerificationFailed)
        }
        LifecycleError::PrivateOutputIncomplete => {
            WorkspaceError::new(WorkspaceErrorCode::VerificationFailed)
        }
        _ => WorkspaceError::new(WorkspaceErrorCode::OutputPublishFailed),
    }
}

const fn invalid_contract() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::InvalidContract)
}

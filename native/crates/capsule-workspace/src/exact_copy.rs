//! Executable exact-snapshot duplicate profile.
//!
//! Serialized lifecycle plans are immutable review data. Execution authority
//! exists only in the non-serializable typestates in this module, which retain
//! a [`crate::VerifiedCopySource`] and a one-use held-parent destination
//! reservation. No semantic, compact, renderer or CLI surface is provided.

use std::{
    ffi::OsStr,
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

use serde::Serialize;
use serde_json::json;
use sqlite_capsule_lifecycle::{
    DestinationReservation, PrivateOutput, PublishedOutput, SealedPrivateOutput, SourceIdentity,
};

use crate::{
    CancellationToken, CopySourceIdentity, CopySourceSignatureState, DecisionScope, InputRole,
    LifecyclePlan, Operation, VerifiedCopySource, WorkspaceError, WorkspaceErrorCode,
    WorkspaceLimits,
    plan::canonical_digest_value,
    prepared_plan::{map_destination_error, map_prepared_destination_error, validate_time_window},
};

const EXACT_ACTION: &str = "copy-exact-snapshot";
const HARD_DEADLINE: Duration = Duration::from_secs(30);
pub const EXACT_COPY_PREVIEW_PROFILE: &str = "org.sqlite-capsule.exact-copy-preview/1";

#[derive(Clone, Debug)]
pub struct ExactCopyPlanRequest<'a> {
    pub output_path: &'a Path,
    pub plan_id: &'a str,
    pub created_at: &'a str,
    pub expires_at: &'a str,
    pub deadline: Duration,
    pub max_output_bytes: u64,
}

/// Host-held review state that binds immutable plan text to the original
/// destination reservation. It is intentionally neither serializable nor
/// constructible outside this module.
pub struct ExactCopyReview {
    plan: LifecyclePlan,
    destination: DestinationReservation,
}

/// Path-free bounded projection of the actual v0.2/v0.3 exact-copy source.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ExactCopyPreview {
    pub profile: &'static str,
    pub format_version: String,
    pub signature_state: CopySourceSignatureState,
    pub signature_count: u8,
    pub source_size_bytes: u64,
    pub source_sha256: String,
    pub capsule_identity: &'static str,
    pub revision_identity: &'static str,
    pub logical_state: &'static str,
    pub destination: &'static str,
    pub overwrite_allowed: bool,
    pub expected_output_sha256: String,
}

pub struct PreparedExactCopy {
    plan: LifecyclePlan,
    source: VerifiedCopySource,
    destination: DestinationReservation,
    deadline: Instant,
    cancellation: CancellationToken,
}

pub struct ExactCopyStaging {
    plan: LifecyclePlan,
    source: VerifiedCopySource,
    private: PrivateOutput,
    deadline: Instant,
    cancellation: CancellationToken,
}

pub struct ValidatedExactCopy {
    plan: LifecyclePlan,
    source: VerifiedCopySource,
    sealed: SealedPrivateOutput,
    expected: CopySourceIdentity,
    deadline: Instant,
    cancellation: CancellationToken,
}

pub struct PublishedExactCopy {
    inner: PublishedOutput,
}

/// Generates deterministic exact-copy review data and its non-serializable,
/// one-use destination capability. The capability must remain held until the
/// reviewed plan is either rejected or consumed by preparation.
pub fn generate_exact_copy_plan(
    source: &VerifiedCopySource,
    request: &ExactCopyPlanRequest<'_>,
) -> Result<ExactCopyReview, WorkspaceError> {
    source.assert_current()?;
    let source_identity = source.source_identity();
    let identity = source.identity();
    let output = absolute_output(request.output_path)?;
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(invalid_contract)?;
    let leaf = output
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(invalid_contract)?;
    let reservation = DestinationReservation::reserve(
        parent,
        OsStr::new(leaf),
        std::slice::from_ref(source_identity),
    )
    .map_err(map_destination_error)?;
    let output_path = utf8_path(&reservation.path_hint())?;
    let source_path = utf8_path(source.canonical_path())?;
    let source_sha256 = &identity.file_sha256;
    let deadline_ms = u64::try_from(request.deadline.min(HARD_DEADLINE).as_millis())
        .map_err(|_| limit_exceeded())?;
    let max_output_bytes = request
        .max_output_bytes
        .min(sqlite_capsule_core::MAX_CAPSULE_BYTES);
    if deadline_ms == 0 || max_output_bytes < identity.size_bytes {
        return Err(limit_exceeded());
    }
    let signature_state = signature_state_name(identity.signature_state);
    let mut value = json!({
        "profile": crate::PLAN_PROFILE,
        "plan_id": request.plan_id,
        "operation": "duplicate",
        "created_at": request.created_at,
        "expires_at": request.expires_at,
        "inputs": [{
            "role": "source",
            "path_hint": source_path,
            "file_sha256": source_sha256,
            "snapshot_sha256": source_sha256,
            "size_bytes": identity.size_bytes,
            "filesystem_identity": {
                "platform": std::env::consts::OS,
                "volume_or_device": source_identity.device.to_string(),
                "file_id_or_inode": source_identity.stable_file_id,
                "modified_ns": source_identity.modified_ns
            },
            "capsule": {
                "format_version": identity.format_version,
                "capsule_id": identity.capsule_id,
                "revision_id": identity.revision_id,
                "app_id": identity.app_id,
                "app_version": identity.app_version,
                "application_digest": identity.application_digest,
                "data_schema_id": identity.data_schema_id,
                "data_schema_version": identity.data_schema_version,
                "publisher_key_id": null
            }
        }],
        "output": {
            "path": output_path,
            "leaf_name": leaf,
            "parent_filesystem_identity": {
                "platform": std::env::consts::OS,
                "volume_or_device": reservation.identity().device.to_string(),
                "file_id_or_inode": reservation.identity().stable_file_id
            },
            "must_not_exist": true,
            "publish_mode": "create-new-no-replace"
        },
        "decisions": [{
            "scope": "application",
            "subject": identity.app_id,
            "action": EXACT_ACTION,
            "reason": "Exact duplicate preserves the verified private snapshot byte-for-byte.",
            "parameters": {
                "source_profile": crate::COPY_SOURCE_PROFILE,
                "signature_state": signature_state,
                "signature_count": identity.signature_count
            }
        }],
        "limits": {
            "max_input_bytes": identity.size_bytes,
            "max_output_bytes": max_output_bytes,
            "max_rows_inspected": 1,
            "max_rows_written": 1,
            "deadline_ms": deadline_ms
        },
        "expected": {
            "capsule_id": identity.capsule_id,
            "revision_id": identity.revision_id,
            "app_id": identity.app_id,
            "application_digest": identity.application_digest,
            "data_schema_id": identity.data_schema_id,
            "data_schema_version": identity.data_schema_version
        },
        "plan_digest": ""
    });
    let digest = canonical_digest_value(&value)?;
    value["plan_digest"] = serde_json::Value::String(digest);
    let bytes = serde_json::to_vec(&value).map_err(|_| invalid_contract())?;
    let plan = parse_exact_copy_plan(&bytes)?;
    reservation
        .assert_reserved_current()
        .map_err(map_prepared_destination_error)?;
    source.assert_current()?;
    Ok(ExactCopyReview {
        plan,
        destination: reservation,
    })
}

/// Parses untrusted review JSON and enforces the exact-copy operation shape.
/// Parsing never creates a destination reservation or execution capability.
pub fn parse_exact_copy_plan(bytes: &[u8]) -> Result<LifecyclePlan, WorkspaceError> {
    let plan = LifecyclePlan::parse(bytes)?;
    validate_exact_shape(&plan)?;
    Ok(plan)
}

impl PreparedExactCopy {
    fn prepare_at(
        review: ExactCopyReview,
        plan: LifecyclePlan,
        source: VerifiedCopySource,
        now: SystemTime,
        limits: &WorkspaceLimits,
        cancellation: &CancellationToken,
    ) -> Result<Self, WorkspaceError> {
        if review.plan.canonical_bytes()? != plan.canonical_bytes()? {
            return Err(WorkspaceError::new(WorkspaceErrorCode::StalePlan));
        }
        validate_time_window(&plan, now)?;
        validate_exact_shape(&plan)?;
        let budget = Duration::from_millis(plan.limits().deadline_ms())
            .min(limits.deadline)
            .min(HARD_DEADLINE);
        if budget.is_zero() || cancellation.is_cancelled() {
            return Err(if cancellation.is_cancelled() {
                WorkspaceError::new(WorkspaceErrorCode::Cancelled)
            } else {
                limit_exceeded()
            });
        }
        let deadline = Instant::now()
            .checked_add(budget)
            .ok_or_else(limit_exceeded)?;
        bind_source(&plan, &source)?;
        let control = verification_control(deadline, cancellation)?;
        source.assert_current_with_control(&control)?;

        let destination = review.destination;
        bind_parent(&plan, &destination)?;
        destination
            .assert_reserved_current()
            .map_err(map_prepared_destination_error)?;
        Ok(Self {
            plan,
            source,
            destination,
            deadline,
            cancellation: cancellation.clone(),
        })
    }

    pub fn stage(self) -> Result<ExactCopyStaging, WorkspaceError> {
        validate_time_window(&self.plan, SystemTime::now())?;
        let control = verification_control(self.deadline, &self.cancellation)?;
        self.source.assert_current_with_control(&control)?;
        self.destination
            .assert_reserved_current()
            .map_err(map_prepared_destination_error)?;
        let private = self
            .destination
            .stage()
            .map_err(map_prepared_destination_error)?;
        Ok(ExactCopyStaging {
            plan: self.plan,
            source: self.source,
            private,
            deadline: self.deadline,
            cancellation: self.cancellation,
        })
    }
}

impl ExactCopyReview {
    /// Immutable plan data for UI review or canonical serialization.
    pub fn plan(&self) -> &LifecyclePlan {
        &self.plan
    }

    pub fn preview(&self, source: &VerifiedCopySource) -> Result<ExactCopyPreview, WorkspaceError> {
        self.destination
            .assert_reserved_current()
            .map_err(map_prepared_destination_error)?;
        bind_parent(&self.plan, &self.destination)?;
        bind_source(&self.plan, source)?;
        source.assert_current()?;
        let identity = source.identity();
        Ok(ExactCopyPreview {
            profile: EXACT_COPY_PREVIEW_PROFILE,
            format_version: identity.format_version.clone(),
            signature_state: identity.signature_state,
            signature_count: identity.signature_count,
            source_size_bytes: identity.size_bytes,
            source_sha256: identity.file_sha256.clone(),
            capsule_identity: "preserved",
            revision_identity: "preserved",
            logical_state: "preserved-exactly",
            destination: "create-new-no-replace",
            overwrite_allowed: false,
            expected_output_sha256: identity.file_sha256.clone(),
        })
    }

    /// Consumes the original held destination capability after the host has
    /// parsed and approved the exact review bytes.
    pub fn prepare(
        self,
        approved_plan: LifecyclePlan,
        source: VerifiedCopySource,
        limits: &WorkspaceLimits,
        cancellation: &CancellationToken,
    ) -> Result<PreparedExactCopy, WorkspaceError> {
        PreparedExactCopy::prepare_at(
            self,
            approved_plan,
            source,
            SystemTime::now(),
            limits,
            cancellation,
        )
    }
}

impl ExactCopyStaging {
    pub fn copy_and_validate(mut self) -> Result<ValidatedExactCopy, WorkspaceError> {
        validate_time_window(&self.plan, SystemTime::now())?;
        let control = verification_control(self.deadline, &self.cancellation)?;
        let expected = self.source.identity().clone();
        let copied = self.source.copy_exact_snapshot_to_file_with_control(
            self.private.file_mut(),
            &control,
            self.plan.limits().max_output_bytes(),
        )?;
        if copied != expected.size_bytes {
            return Err(verification_failed());
        }
        let sealed = self
            .private
            .seal_with_limit(self.plan.limits().max_output_bytes())
            .map_err(map_destination_error)?;
        if lower_hex(sealed.sha256()) != expected.file_sha256 {
            return Err(verification_failed());
        }
        sealed
            .assert_staged_current()
            .map_err(map_destination_error)?;
        let output = open_output(
            sealed.private_path_hint(),
            self.deadline,
            &self.cancellation,
        )?;
        require_same_object(sealed.identity(), output.source_identity())?;
        require_expected(&expected, output.identity())?;
        drop(output);
        sealed
            .assert_staged_current()
            .map_err(map_destination_error)?;
        self.source.assert_current_with_control(&control)?;
        Ok(ValidatedExactCopy {
            plan: self.plan,
            source: self.source,
            sealed,
            expected,
            deadline: self.deadline,
            cancellation: self.cancellation,
        })
    }
}

impl ValidatedExactCopy {
    pub fn publish(self) -> Result<PublishedExactCopy, WorkspaceError> {
        self.publish_with_hook(|| {})
    }

    fn publish_with_hook<F>(
        self,
        after_final_output_check: F,
    ) -> Result<PublishedExactCopy, WorkspaceError>
    where
        F: FnOnce(),
    {
        validate_time_window(&self.plan, SystemTime::now())?;
        let control = verification_control(self.deadline, &self.cancellation)?;
        self.source.assert_current_with_control(&control)?;
        self.sealed
            .assert_staged_current()
            .map_err(map_destination_error)?;
        let prepublish = open_output(
            self.sealed.private_path_hint(),
            self.deadline,
            &self.cancellation,
        )?;
        require_same_object(self.sealed.identity(), prepublish.source_identity())?;
        require_expected(&self.expected, prepublish.identity())?;
        drop(prepublish);
        self.sealed
            .assert_staged_current()
            .map_err(map_destination_error)?;
        self.source.assert_current_with_control(&control)?;

        let expected = self.expected.clone();
        let deadline = self.deadline;
        let cancellation = self.cancellation.clone();
        let source = &self.source;
        // SAFETY: this typestate can only be constructed after the held staged
        // file was exhaustively reopened as `VerifiedCopySource`, matched the
        // exact reviewed snapshot digest and identity, and the source was
        // rebound. The callback repeats those checks against the held final
        // file before the low-level no-replace primitive reports success.
        let published = unsafe {
            self.sealed
                .publish_no_replace_unchecked(|reopened, reopened_identity| {
                    let snapshot =
                        snapshot_held_file(reopened, expected.size_bytes, deadline, &cancellation)?;
                    let output =
                        open_output(snapshot.path(), deadline, &cancellation).map_err(|_| {
                            sqlite_capsule_lifecycle::LifecycleError::PostPublishVerification
                        })?;
                    if output.source_identity().bytes != reopened_identity.bytes
                        || require_expected(&expected, output.identity()).is_err()
                    {
                        return Err(
                            sqlite_capsule_lifecycle::LifecycleError::PostPublishVerification,
                        );
                    }
                    after_final_output_check();
                    let control = verification_control(deadline, &cancellation).map_err(|_| {
                        sqlite_capsule_lifecycle::LifecycleError::PostPublishVerification
                    })?;
                    source.assert_current_with_control(&control).map_err(|_| {
                        sqlite_capsule_lifecycle::LifecycleError::PostPublishVerification
                    })?;
                    Ok(())
                })
        }
        .map_err(map_destination_error)?;
        Ok(PublishedExactCopy { inner: published })
    }
}

impl PublishedExactCopy {
    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    pub fn identity(&self) -> &SourceIdentity {
        &self.inner.identity
    }
}

fn validate_exact_shape(plan: &LifecyclePlan) -> Result<(), WorkspaceError> {
    if plan.operation() != Operation::Duplicate
        || plan.inputs().len() != 1
        || plan.inputs()[0].role() != InputRole::Source
        || plan.decisions().len() != 1
        || plan.decisions()[0].scope() != DecisionScope::Application
        || plan.decisions()[0].action() != EXACT_ACTION
        || !plan.limits().row_budgets_within_duplicate_profile()
    {
        return Err(invalid_contract());
    }
    let value = serde_json::to_value(plan).map_err(|_| invalid_contract())?;
    let parameters = value["decisions"][0]["parameters"]
        .as_object()
        .ok_or_else(invalid_contract)?;
    if parameters.len() != 3
        || parameters
            .get("source_profile")
            .and_then(|value| value.as_str())
            != Some(crate::COPY_SOURCE_PROFILE)
        || !matches!(
            parameters
                .get("signature_state")
                .and_then(|value| value.as_str()),
            Some("unsigned" | "signed-valid")
        )
        || parameters
            .get("signature_count")
            .and_then(|value| value.as_u64())
            .is_none_or(|count| count > sqlite_capsule_crypto::MAX_SIGNATURES as u64)
    {
        return Err(invalid_contract());
    }
    Ok(())
}

fn bind_source(plan: &LifecyclePlan, source: &VerifiedCopySource) -> Result<(), WorkspaceError> {
    let input = &plan.inputs()[0];
    let capsule = input.capsule();
    let identity = source.identity();
    let live = source.source_identity();
    let expected = plan.expected();
    let value = serde_json::to_value(plan).map_err(|_| invalid_contract())?;
    let parameters = &value["decisions"][0]["parameters"];
    if input.path_hint() != utf8_path(source.canonical_path())?
        || input.file_sha256() != identity.file_sha256
        || input.snapshot_sha256() != identity.file_sha256
        || input.size_bytes() != identity.size_bytes
        || input.filesystem_identity().platform() != std::env::consts::OS
        || input.filesystem_identity().volume_or_device() != live.device.to_string()
        || input.filesystem_identity().file_id_or_inode() != live.stable_file_id
        || input.filesystem_identity().modified_ns() != live.modified_ns
        || capsule.format_version() != identity.format_version
        || capsule.capsule_id() != Some(identity.capsule_id.as_str())
        || capsule.revision_id() != identity.revision_id.as_deref()
        || capsule.app_id() != identity.app_id
        || capsule.app_version() != identity.app_version
        || capsule.application_digest() != identity.application_digest.as_deref()
        || capsule.data_schema_id() != identity.data_schema_id.as_deref()
        || capsule.data_schema_version() != identity.data_schema_version
        || capsule.publisher_key_id().is_some()
        || expected.capsule_id() != Some(identity.capsule_id.as_str())
        || expected.revision_id() != identity.revision_id.as_deref()
        || expected.app_id() != identity.app_id
        || expected.application_digest() != identity.application_digest.as_deref()
        || expected.data_schema_id() != identity.data_schema_id.as_deref()
        || expected.data_schema_version() != identity.data_schema_version
        || plan.decisions()[0].subject() != identity.app_id
        || parameters["signature_state"].as_str()
            != Some(signature_state_name(identity.signature_state))
        || parameters["signature_count"].as_u64() != Some(u64::from(identity.signature_count))
    {
        return Err(WorkspaceError::new(WorkspaceErrorCode::StalePlan));
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
        || expected.platform() != std::env::consts::OS
        || expected.volume_or_device() != actual.device.to_string()
        || expected.file_id_or_inode() != actual.stable_file_id
    {
        return Err(WorkspaceError::new(WorkspaceErrorCode::StalePlan));
    }
    Ok(())
}

fn require_expected(
    expected: &CopySourceIdentity,
    actual: &CopySourceIdentity,
) -> Result<(), WorkspaceError> {
    if expected == actual {
        Ok(())
    } else if expected.application_digest != actual.application_digest
        || expected.signature_state != actual.signature_state
        || expected.signature_count != actual.signature_count
    {
        Err(WorkspaceError::new(WorkspaceErrorCode::SignatureChanged))
    } else {
        Err(verification_failed())
    }
}

fn open_output(
    path: &Path,
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<VerifiedCopySource, WorkspaceError> {
    let limits = WorkspaceLimits {
        deadline: remaining(deadline)?,
        ..WorkspaceLimits::default()
    };
    VerifiedCopySource::open_with_control(path, &limits, cancellation)
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

fn remaining(deadline: Instant) -> Result<Duration, WorkspaceError> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        Err(limit_exceeded())
    } else {
        Ok(remaining)
    }
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

fn signature_state_name(state: CopySourceSignatureState) -> &'static str {
    match state {
        CopySourceSignatureState::Unsigned => "unsigned",
        CopySourceSignatureState::SignedValid => "signed-valid",
    }
}

fn absolute_output(path: &Path) -> Result<PathBuf, WorkspaceError> {
    if path.as_os_str().is_empty() {
        return Err(invalid_contract());
    }
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InternalError))?
            .join(path)
    };
    if path.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir | std::path::Component::CurDir
        )
    }) {
        return Err(invalid_contract());
    }
    Ok(path)
}

fn utf8_path(path: &Path) -> Result<String, WorkspaceError> {
    path.to_str()
        .filter(|value| !value.is_empty() && value.len() <= 4_096)
        .map(str::to_owned)
        .ok_or_else(invalid_contract)
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

const fn verification_failed() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::VerificationFailed)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{Duration, UNIX_EPOCH},
    };

    use rusqlite::Connection;

    use super::*;
    use crate::copy_source::tests::fixture;

    const CREATED: &str = "2026-08-12T00:00:00Z";
    // These operation tests cross public transitions that intentionally use the
    // real production clock. Keep their reviewed window valid independently of
    // the calendar date on which the suite is run; expiry boundary behavior has
    // dedicated injected-clock tests elsewhere.
    const EXPIRES: &str = "9999-12-31T23:59:59Z";

    fn operation_time() -> SystemTime {
        UNIX_EPOCH
            + Duration::from_secs(
                crate::prepared_plan::parse_utc_seconds(CREATED).expect("created") + 60,
            )
    }

    fn request(path: &Path) -> ExactCopyPlanRequest<'_> {
        ExactCopyPlanRequest {
            output_path: path,
            plan_id: "b68aa4b5-a62c-497b-8d84-830fdbbb68fc",
            created_at: CREATED,
            expires_at: EXPIRES,
            deadline: Duration::from_secs(30),
            max_output_bytes: sqlite_capsule_core::MAX_CAPSULE_BYTES,
        }
    }

    fn prepare_at(
        source: VerifiedCopySource,
        output: &Path,
        cancellation: &CancellationToken,
    ) -> PreparedExactCopy {
        let review = generate_exact_copy_plan(&source, &request(output)).expect("exact review");
        let plan = parse_exact_copy_plan(
            &review
                .plan()
                .canonical_bytes()
                .expect("canonical reviewed plan"),
        )
        .expect("approved plan");
        PreparedExactCopy::prepare_at(
            review,
            plan,
            source,
            operation_time(),
            &WorkspaceLimits::default(),
            cancellation,
        )
        .expect("prepared exact copy")
    }

    #[test]
    fn signed_and_unsigned_v02_v03_publish_byte_exact_without_mutating_source() {
        for version in [2, 3] {
            for signed in [false, true] {
                let (directory, source_path) = fixture(
                    &format!(
                        "exact-v0{version}-{}",
                        if signed { "signed" } else { "unsigned" }
                    ),
                    version,
                    signed,
                );
                let output = directory
                    .path()
                    .join(format!("copy-{version}-{signed}.sqlitecapsule"));
                let before = fs::read(&source_path).expect("source bytes");
                let source = VerifiedCopySource::open(&source_path).expect("copy source");
                let cancellation = CancellationToken::new();
                let published = prepare_at(source, &output, &cancellation)
                    .stage()
                    .expect("stage")
                    .copy_and_validate()
                    .expect("validate")
                    .publish()
                    .expect("publish");
                assert_eq!(published.path(), output);
                assert_eq!(fs::read(&output).expect("output bytes"), before);
                assert_eq!(fs::read(&source_path).expect("source after copy"), before);
                let reopened = VerifiedCopySource::open(&output).expect("published reopens");
                assert_eq!(reopened.identity().format_version, format!("0.{version}"));
            }
        }
    }

    #[test]
    fn actual_exact_preview_is_path_free_for_all_format_and_signature_states() {
        for version in [2, 3] {
            for signed in [false, true] {
                let (directory, source_path) = fixture(
                    &format!("exact-preview-{version}-{signed}"),
                    version,
                    signed,
                );
                let output = directory.path().join("preview-output.sqlitecapsule");
                let source = VerifiedCopySource::open(&source_path).expect("copy source");
                let review = generate_exact_copy_plan(&source, &request(&output)).expect("review");
                let preview = review.preview(&source).expect("actual exact preview");
                assert_eq!(preview.format_version, format!("0.{version}"));
                assert_eq!(
                    preview.signature_state,
                    if signed {
                        CopySourceSignatureState::SignedValid
                    } else {
                        CopySourceSignatureState::Unsigned
                    }
                );
                assert_eq!(preview.source_sha256, preview.expected_output_sha256);
                assert!(!preview.overwrite_allowed);
                let serialized = serde_json::to_string(&preview).expect("serialize preview");
                assert!(!serialized.contains(&source_path.to_string_lossy().to_string()));
                assert!(!serialized.contains(&output.to_string_lossy().to_string()));
                assert!(!serialized.contains("path"));
            }
        }
    }

    #[test]
    fn plan_round_trip_is_immutable_and_semantic_edits_do_not_gain_authority() {
        let (directory, source_path) = fixture("exact-plan", 3, false);
        let output = directory.path().join("copy.sqlitecapsule");
        let source = VerifiedCopySource::open(&source_path).expect("source");
        let review = generate_exact_copy_plan(&source, &request(&output)).expect("review");
        let bytes = review.plan().canonical_bytes().expect("canonical plan");
        assert_eq!(
            parse_exact_copy_plan(&bytes)
                .expect("parse exact plan")
                .canonical_bytes()
                .expect("canonical again"),
            bytes
        );
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).expect("plan JSON");
        value["decisions"][0]["action"] = serde_json::Value::String("compact".into());
        let digest = canonical_digest_value(&value).expect("edited digest");
        value["plan_digest"] = serde_json::Value::String(digest);
        assert_eq!(
            parse_exact_copy_plan(&serde_json::to_vec(&value).expect("edited JSON"))
                .expect_err("semantic edit")
                .kind(),
            WorkspaceErrorCode::InvalidContract
        );
    }

    #[test]
    fn recomputed_same_parent_leaf_rewrite_cannot_use_held_destination_authority() {
        let (directory, source_path) = fixture("exact-leaf-rewrite", 3, false);
        let output = directory.path().join("reviewed.sqlitecapsule");
        let rewritten = directory.path().join("rewritten.sqlitecapsule");
        let source = VerifiedCopySource::open(&source_path).expect("source");
        let review = generate_exact_copy_plan(&source, &request(&output)).expect("review");
        let bytes = review.plan().canonical_bytes().expect("canonical plan");
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).expect("plan JSON");
        value["output"]["path"] =
            serde_json::Value::String(rewritten.to_string_lossy().into_owned());
        value["output"]["leaf_name"] = serde_json::Value::String("rewritten.sqlitecapsule".into());
        let digest = canonical_digest_value(&value).expect("rewritten digest");
        value["plan_digest"] = serde_json::Value::String(digest);
        let rewritten_plan =
            parse_exact_copy_plan(&serde_json::to_vec(&value).expect("rewritten plan JSON"))
                .expect("shape-valid rewritten plan");
        let cancellation = CancellationToken::new();
        let error = match PreparedExactCopy::prepare_at(
            review,
            rewritten_plan,
            source,
            operation_time(),
            &WorkspaceLimits::default(),
            &cancellation,
        ) {
            Ok(_) => panic!("rewritten leaf must not gain destination authority"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::StalePlan);
        assert!(!output.exists());
        assert!(!rewritten.exists());
    }

    #[test]
    fn existing_and_racing_destinations_are_never_overwritten() {
        let (directory, source_path) = fixture("exact-destination", 2, false);
        let output = directory.path().join("copy.sqlitecapsule");
        fs::write(&output, b"existing").expect("existing output");
        let source = VerifiedCopySource::open(&source_path).expect("source");
        let error = match generate_exact_copy_plan(&source, &request(&output)) {
            Ok(_) => panic!("existing destination must fail"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::DestinationExists);
        assert_eq!(fs::read(&output).expect("existing bytes"), b"existing");

        fs::remove_file(&output).expect("remove test output");
        let source = VerifiedCopySource::open(&source_path).expect("source again");
        let cancellation = CancellationToken::new();
        let prepared = prepare_at(source, &output, &cancellation);
        fs::write(&output, b"racer").expect("destination race");
        let error = match prepared.stage() {
            Ok(_) => panic!("race must fail"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::DestinationExists);
        assert_eq!(fs::read(&output).expect("racer bytes"), b"racer");
    }

    #[test]
    fn source_race_and_cancellation_stop_before_publication() {
        let (directory, source_path) = fixture("exact-source-race", 3, false);
        let output = directory.path().join("copy.sqlitecapsule");
        let source = VerifiedCopySource::open(&source_path).expect("source");
        let cancellation = CancellationToken::new();
        let prepared = prepare_at(source, &output, &cancellation);
        let connection = Connection::open(&source_path).expect("external writer");
        connection
            .execute(
                "UPDATE vector_domain SET note = 'race' WHERE id = 'domain'",
                [],
            )
            .expect("source mutation");
        drop(connection);
        let error = match prepared.stage() {
            Ok(_) => panic!("stale source must fail"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::StalePlan);
        assert!(!output.exists());

        let (directory, source_path) = fixture("exact-cancel", 2, false);
        let output = directory.path().join("copy.sqlitecapsule");
        let source = VerifiedCopySource::open(&source_path).expect("source");
        let cancellation = CancellationToken::new();
        let prepared = prepare_at(source, &output, &cancellation);
        cancellation.cancel();
        let error = match prepared.stage() {
            Ok(_) => panic!("cancelled stage must fail"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::Cancelled);
        assert!(!output.exists());
    }

    #[test]
    fn late_source_mutation_or_cancellation_is_quarantined_before_success() {
        let (directory, source_path) = fixture("exact-late-source-race", 3, false);
        let output = directory.path().join("copy.sqlitecapsule");
        let source = VerifiedCopySource::open(&source_path).expect("source");
        let cancellation = CancellationToken::new();
        let validated = prepare_at(source, &output, &cancellation)
            .stage()
            .expect("staged")
            .copy_and_validate()
            .expect("validated");
        let error = match validated.publish_with_hook(|| {
            let connection = Connection::open(&source_path).expect("late source writer");
            connection
                .execute(
                    "UPDATE vector_domain SET note = 'late race' WHERE id = 'domain'",
                    [],
                )
                .expect("late source mutation");
        }) {
            Ok(_) => panic!("late mutation must prevent success"),
            Err(error) => error,
        };
        assert_eq!(
            error.kind(),
            WorkspaceErrorCode::PostpublishVerificationFailed
        );
        assert_quarantine_evidence(directory.path());

        let (directory, source_path) = fixture("exact-late-cancel", 2, false);
        let output = directory.path().join("copy.sqlitecapsule");
        let source = VerifiedCopySource::open(&source_path).expect("source");
        let cancellation = CancellationToken::new();
        let validated = prepare_at(source, &output, &cancellation)
            .stage()
            .expect("staged")
            .copy_and_validate()
            .expect("validated");
        let cancel_at_callback = cancellation.clone();
        let error = match validated.publish_with_hook(move || cancel_at_callback.cancel()) {
            Ok(_) => panic!("late cancellation must prevent success"),
            Err(error) => error,
        };
        assert_eq!(
            error.kind(),
            WorkspaceErrorCode::PostpublishVerificationFailed
        );
        assert_quarantine_evidence(directory.path());
    }

    #[test]
    fn dropping_staged_or_validated_typestate_cleans_private_output() {
        let (directory, source_path) = fixture("exact-drop", 3, true);
        let output = directory.path().join("copy.sqlitecapsule");
        let baseline = fs::read_dir(directory.path()).expect("baseline").count();
        let source = VerifiedCopySource::open(&source_path).expect("source");
        let cancellation = CancellationToken::new();
        let staged = prepare_at(source, &output, &cancellation)
            .stage()
            .expect("staged");
        drop(staged);
        assert!(!output.exists());
        assert_private_payloads_removed(directory.path());

        let source = VerifiedCopySource::open(&source_path).expect("source again");
        let validated = prepare_at(source, &output, &cancellation)
            .stage()
            .expect("staged again")
            .copy_and_validate()
            .expect("validated");
        drop(validated);
        assert!(!output.exists());
        assert_private_payloads_removed(directory.path());
        assert!(
            fs::read_dir(directory.path())
                .expect("bounded residual inventory")
                .count()
                <= baseline + 2
        );
    }

    #[test]
    fn invalid_mixed_signature_source_never_reaches_plan_or_destination() {
        let (directory, source_path) = fixture("exact-invalid-signature", 3, true);
        let output = directory.path().join("copy.sqlitecapsule");
        let connection = Connection::open(&source_path).expect("signature writer");
        connection
            .execute("UPDATE capsule_signature SET signature = zeroblob(64)", [])
            .expect("invalidate signature");
        drop(connection);
        assert_eq!(
            VerifiedCopySource::open(&source_path)
                .expect_err("invalid signature")
                .kind(),
            WorkspaceErrorCode::InvalidSignature
        );
        assert!(!output.exists());
    }

    fn assert_private_payloads_removed(parent: &Path) {
        for entry in fs::read_dir(parent).expect("destination inventory") {
            let entry = entry.expect("inventory entry");
            if entry.file_type().expect("entry type").is_dir() {
                assert_eq!(
                    fs::read_dir(entry.path())
                        .expect("private residual directory")
                        .count(),
                    0,
                    "dropped typestate must remove private payload bytes"
                );
            }
        }
    }

    fn assert_quarantine_evidence(parent: &Path) {
        assert!(
            fs::read_dir(parent)
                .expect("quarantine inventory")
                .any(|entry| {
                    let name = entry.expect("quarantine entry").file_name();
                    let name = name.to_string_lossy();
                    name.contains("failed") || name.contains("quarantine")
                }),
            "post-publish failure must leave quarantine evidence"
        );
    }
}

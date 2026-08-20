//! Owner-private compact duplicate execution and publication typestate.

use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::atomic::Ordering,
    time::{Duration, Instant, SystemTime},
};

use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use serde_json::json;
use sqlite_capsule_lifecycle::{
    DestinationReservation, PrivateOutput, PublishedOutput, SealedPrivateOutput, SourceIdentity,
};

use crate::{
    CancellationToken, CompactLogicalState, CopySourceIdentity, CopySourceSignatureState,
    DecisionScope, InputRole, LifecyclePlan, Operation, VerifiedCompactSource, VerifiedCopySource,
    WorkspaceError, WorkspaceErrorCode, WorkspaceLimits,
    compact_state::digest_connection,
    plan::canonical_digest_value,
    prepared_plan::{map_destination_error, map_prepared_destination_error, validate_time_window},
};

const COMPACT_ACTION: &str = "copy-compact-logical-state";
const HARD_DEADLINE: Duration = Duration::from_secs(30);
pub const COMPACT_COPY_PREVIEW_PROFILE: &str = "org.sqlite-capsule.compact-copy-preview/1";

#[derive(Clone, Debug)]
pub struct CompactCopyPlanRequest<'a> {
    pub output_path: &'a Path,
    pub plan_id: &'a str,
    pub created_at: &'a str,
    pub expires_at: &'a str,
    pub deadline: Duration,
    pub max_output_bytes: u64,
}

pub struct CompactCopyReview {
    plan: LifecyclePlan,
    destination: DestinationReservation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CompactCopyPreview {
    pub profile: &'static str,
    pub format_version: String,
    pub signature_state: CopySourceSignatureState,
    pub signature_count: u8,
    pub source_size_bytes: u64,
    pub source_sha256: String,
    pub logical_state_profile: &'static str,
    pub logical_state_sha256: String,
    pub page_size: u32,
    pub capsule_identity: &'static str,
    pub revision_identity: &'static str,
    pub physical_bytes: &'static str,
    pub destination: &'static str,
    pub overwrite_allowed: bool,
}

pub struct PreparedCompactCopy {
    plan: LifecyclePlan,
    source: VerifiedCompactSource,
    destination: DestinationReservation,
    deadline: Instant,
    cancellation: CancellationToken,
}

pub struct CompactCopyStaging {
    plan: LifecyclePlan,
    source: VerifiedCompactSource,
    private: PrivateOutput,
    deadline: Instant,
    cancellation: CancellationToken,
}

pub struct ValidatedCompactCopy {
    plan: LifecyclePlan,
    source: VerifiedCompactSource,
    sealed: SealedPrivateOutput,
    expected_identity: CopySourceIdentity,
    expected_state: CompactLogicalState,
    deadline: Instant,
    cancellation: CancellationToken,
}

pub struct PublishedCompactCopy {
    inner: PublishedOutput,
}

pub fn generate_compact_copy_plan(
    source: &VerifiedCompactSource,
    request: &CompactCopyPlanRequest<'_>,
) -> Result<CompactCopyReview, WorkspaceError> {
    source.assert_current()?;
    let identity = source.identity();
    let live = source.source().source_identity();
    let state = source.logical_state();
    let output = absolute_output(request.output_path)?;
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(invalid_contract)?;
    let leaf = output
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(invalid_contract)?;
    let reservation =
        DestinationReservation::reserve(parent, OsStr::new(leaf), std::slice::from_ref(live))
            .map_err(map_destination_error)?;
    let deadline_ms = u64::try_from(request.deadline.min(HARD_DEADLINE).as_millis())
        .map_err(|_| limit_exceeded())?;
    let max_output_bytes = request
        .max_output_bytes
        .min(sqlite_capsule_core::MAX_CAPSULE_BYTES);
    if deadline_ms == 0 || max_output_bytes == 0 {
        return Err(limit_exceeded());
    }
    let mut value = json!({
        "profile": crate::PLAN_PROFILE,
        "plan_id": request.plan_id,
        "operation": "compact-duplicate",
        "created_at": request.created_at,
        "expires_at": request.expires_at,
        "inputs": [{
            "role": "source",
            "path_hint": utf8_path(source.source().canonical_path())?,
            "file_sha256": identity.file_sha256,
            "snapshot_sha256": identity.file_sha256,
            "size_bytes": identity.size_bytes,
            "filesystem_identity": {
                "platform": std::env::consts::OS,
                "volume_or_device": live.device.to_string(),
                "file_id_or_inode": live.stable_file_id,
                "modified_ns": live.modified_ns
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
            "path": utf8_path(&reservation.path_hint())?,
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
            "action": COMPACT_ACTION,
            "reason": "Compact duplicate preserves exhaustive logical state while removing free pages.",
            "parameters": {
                "source_profile": crate::COPY_SOURCE_PROFILE,
                "logical_state_profile": state.profile,
                "logical_state_sha256": state.digest_sha256,
                "page_size": state.page_size,
                "signature_state": signature_state_name(identity.signature_state),
                "signature_count": identity.signature_count
            }
        }],
        "limits": {
            "max_input_bytes": identity.size_bytes,
            "max_output_bytes": max_output_bytes,
            "max_rows_inspected": 100000,
            "max_rows_written": 100000,
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
    value["plan_digest"] = serde_json::Value::String(canonical_digest_value(&value)?);
    let plan =
        parse_compact_copy_plan(&serde_json::to_vec(&value).map_err(|_| invalid_contract())?)?;
    reservation
        .assert_reserved_current()
        .map_err(map_prepared_destination_error)?;
    source.assert_current()?;
    Ok(CompactCopyReview {
        plan,
        destination: reservation,
    })
}

pub fn parse_compact_copy_plan(bytes: &[u8]) -> Result<LifecyclePlan, WorkspaceError> {
    let plan = LifecyclePlan::parse(bytes)?;
    validate_shape(&plan)?;
    Ok(plan)
}

impl CompactCopyReview {
    pub fn plan(&self) -> &LifecyclePlan {
        &self.plan
    }

    pub fn preview(
        &self,
        source: &VerifiedCompactSource,
    ) -> Result<CompactCopyPreview, WorkspaceError> {
        self.destination
            .assert_reserved_current()
            .map_err(map_prepared_destination_error)?;
        bind_parent(&self.plan, &self.destination)?;
        bind_source(&self.plan, source)?;
        source.assert_current()?;
        let identity = source.identity();
        let state = source.logical_state();
        Ok(CompactCopyPreview {
            profile: COMPACT_COPY_PREVIEW_PROFILE,
            format_version: identity.format_version.clone(),
            signature_state: identity.signature_state,
            signature_count: identity.signature_count,
            source_size_bytes: identity.size_bytes,
            source_sha256: identity.file_sha256.clone(),
            logical_state_profile: state.profile,
            logical_state_sha256: state.digest_sha256.clone(),
            page_size: state.page_size,
            capsule_identity: "preserved",
            revision_identity: "preserved",
            physical_bytes: "repacked-not-byte-exact",
            destination: "create-new-no-replace",
            overwrite_allowed: false,
        })
    }

    pub fn prepare(
        self,
        approved_plan: LifecyclePlan,
        source: VerifiedCompactSource,
        limits: &WorkspaceLimits,
        cancellation: &CancellationToken,
    ) -> Result<PreparedCompactCopy, WorkspaceError> {
        PreparedCompactCopy::prepare_at(
            self,
            approved_plan,
            source,
            SystemTime::now(),
            limits,
            cancellation,
        )
    }
}

impl PreparedCompactCopy {
    fn prepare_at(
        review: CompactCopyReview,
        plan: LifecyclePlan,
        source: VerifiedCompactSource,
        now: SystemTime,
        limits: &WorkspaceLimits,
        cancellation: &CancellationToken,
    ) -> Result<Self, WorkspaceError> {
        if review.plan.canonical_bytes()? != plan.canonical_bytes()? {
            return Err(stale_plan());
        }
        validate_time_window(&plan, now)?;
        validate_shape(&plan)?;
        let budget = Duration::from_millis(plan.limits().deadline_ms())
            .min(limits.deadline)
            .min(HARD_DEADLINE);
        if cancellation.is_cancelled() {
            return Err(cancelled());
        }
        if budget.is_zero() {
            return Err(limit_exceeded());
        }
        let deadline = Instant::now()
            .checked_add(budget)
            .ok_or_else(limit_exceeded)?;
        bind_source(&plan, &source)?;
        source.assert_current()?;
        bind_parent(&plan, &review.destination)?;
        review
            .destination
            .assert_reserved_current()
            .map_err(map_prepared_destination_error)?;
        Ok(Self {
            plan,
            source,
            destination: review.destination,
            deadline,
            cancellation: cancellation.clone(),
        })
    }

    pub fn stage(self) -> Result<CompactCopyStaging, WorkspaceError> {
        check(self.deadline, &self.cancellation)?;
        self.source.assert_current()?;
        self.destination
            .assert_reserved_current()
            .map_err(map_prepared_destination_error)?;
        let private = self
            .destination
            .stage()
            .map_err(map_prepared_destination_error)?;
        maybe_crash("private-created");
        Ok(CompactCopyStaging {
            plan: self.plan,
            source: self.source,
            private,
            deadline: self.deadline,
            cancellation: self.cancellation,
        })
    }
}

impl CompactCopyStaging {
    pub fn compact_and_validate(mut self) -> Result<ValidatedCompactCopy, WorkspaceError> {
        check(self.deadline, &self.cancellation)?;
        let control = control(self.deadline, &self.cancellation)?;
        let expected_identity = self.source.identity().clone();
        let expected_state = self.source.logical_state().clone();
        let copied = self
            .source
            .source()
            .copy_exact_snapshot_to_file_with_control(
                self.private.file_mut(),
                &control,
                self.plan.limits().max_output_bytes(),
            )?;
        if copied != expected_identity.size_bytes {
            return Err(verification_failed());
        }
        self.private
            .file_mut()
            .flush()
            .map_err(|_| output_failed())?;
        self.private
            .file_mut()
            .sync_all()
            .map_err(|_| output_failed())?;
        maybe_crash("snapshot-copied");
        vacuum_private(
            self.private.private_path_hint(),
            expected_state.page_size,
            self.deadline,
            &self.cancellation,
        )?;
        maybe_crash("vacuumed");
        let sealed = self
            .private
            .seal_with_limit(self.plan.limits().max_output_bytes())
            .map_err(map_destination_error)?;
        sealed
            .assert_staged_current()
            .map_err(map_destination_error)?;
        let output = open_output(
            sealed.private_path_hint(),
            self.deadline,
            &self.cancellation,
        )?;
        require_same_object(sealed.identity(), output.source_identity())?;
        require_logical_identity(&expected_identity, output.identity())?;
        let _guard = output.start_control(output.verification_control())?;
        let actual_state =
            digest_connection(output.verified_connection(), output.verification_control())?;
        require_compact_state(&expected_state, &actual_state)?;
        require_compact_pragmas(output.verified_connection(), expected_state.page_size)?;
        reject_sidecars(sealed.private_path_hint())?;
        drop(output);
        sealed
            .assert_staged_current()
            .map_err(map_destination_error)?;
        self.source.assert_current()?;
        maybe_crash("sealed-and-verified");
        Ok(ValidatedCompactCopy {
            plan: self.plan,
            source: self.source,
            sealed,
            expected_identity,
            expected_state,
            deadline: self.deadline,
            cancellation: self.cancellation,
        })
    }
}

impl ValidatedCompactCopy {
    pub fn publish(self) -> Result<PublishedCompactCopy, WorkspaceError> {
        self.publish_with_hook(|| {})
    }

    fn publish_with_hook<F>(
        self,
        after_final_output_check: F,
    ) -> Result<PublishedCompactCopy, WorkspaceError>
    where
        F: FnOnce(),
    {
        check(self.deadline, &self.cancellation)?;
        self.source.assert_current()?;
        self.sealed
            .assert_staged_current()
            .map_err(map_destination_error)?;
        let prepublish = open_output(
            self.sealed.private_path_hint(),
            self.deadline,
            &self.cancellation,
        )?;
        require_same_object(self.sealed.identity(), prepublish.source_identity())?;
        require_logical_identity(&self.expected_identity, prepublish.identity())?;
        let _guard = prepublish.start_control(prepublish.verification_control())?;
        let state = digest_connection(
            prepublish.verified_connection(),
            prepublish.verification_control(),
        )?;
        require_compact_state(&self.expected_state, &state)?;
        require_compact_pragmas(
            prepublish.verified_connection(),
            self.expected_state.page_size,
        )?;
        drop(prepublish);
        self.sealed
            .assert_staged_current()
            .map_err(map_destination_error)?;
        self.source.assert_current()?;

        let expected_identity = self.expected_identity.clone();
        let expected_state = self.expected_state.clone();
        let deadline = self.deadline;
        let cancellation = self.cancellation.clone();
        let source = &self.source;
        // SAFETY: the held staged file has been exhaustively reopened as a
        // valid duplicate, its full logical state matches the source, and its
        // freelist/journal/page-size postconditions have been proven. The
        // callback repeats all proofs on the held published bytes and performs
        // the last source rebind while quarantine remains available.
        let published = unsafe {
            self.sealed
                .publish_no_replace_unchecked(|reopened, reopened_identity| {
                    let snapshot = snapshot_held_file(
                        reopened,
                        self.plan.limits().max_output_bytes(),
                        deadline,
                        &cancellation,
                    )?;
                    let output =
                        open_output(snapshot.path(), deadline, &cancellation).map_err(|_| {
                            sqlite_capsule_lifecycle::LifecycleError::PostPublishVerification
                        })?;
                    if output.source_identity().bytes != reopened_identity.bytes
                        || require_logical_identity(&expected_identity, output.identity()).is_err()
                    {
                        return Err(
                            sqlite_capsule_lifecycle::LifecycleError::PostPublishVerification,
                        );
                    }
                    let _guard = output
                        .start_control(output.verification_control())
                        .map_err(|_| {
                            sqlite_capsule_lifecycle::LifecycleError::PostPublishVerification
                        })?;
                    let state = digest_connection(
                        output.verified_connection(),
                        output.verification_control(),
                    )
                    .map_err(|_| {
                        sqlite_capsule_lifecycle::LifecycleError::PostPublishVerification
                    })?;
                    if require_compact_state(&expected_state, &state).is_err()
                        || require_compact_pragmas(
                            output.verified_connection(),
                            expected_state.page_size,
                        )
                        .is_err()
                    {
                        return Err(
                            sqlite_capsule_lifecycle::LifecycleError::PostPublishVerification,
                        );
                    }
                    maybe_crash("postrename-reopened");
                    after_final_output_check();
                    check(deadline, &cancellation)
                        .and_then(|_| source.assert_current())
                        .map_err(|_| {
                            sqlite_capsule_lifecycle::LifecycleError::PostPublishVerification
                        })?;
                    Ok(())
                })
        }
        .map_err(map_destination_error)?;
        Ok(PublishedCompactCopy { inner: published })
    }
}

impl PublishedCompactCopy {
    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    pub fn identity(&self) -> &SourceIdentity {
        &self.inner.identity
    }
}

fn vacuum_private(
    path: &Path,
    expected_page_size: u32,
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<(), WorkspaceError> {
    reject_sidecars(path)?;
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| output_failed())?;
    let cancelled_flag = cancellation.shared_flag();
    connection
        .progress_handler(
            1_000,
            Some(move || cancelled_flag.load(Ordering::Relaxed) || Instant::now() >= deadline),
        )
        .map_err(|_| output_failed())?;
    check(deadline, cancellation)?;
    let page_size: u32 = connection
        .pragma_query_value(None, "page_size", |row| row.get(0))
        .map_err(|_| output_failed())?;
    if page_size != expected_page_size {
        return Err(verification_failed());
    }
    let mode: String = connection
        .query_row("PRAGMA journal_mode=DELETE", [], |row| row.get(0))
        .map_err(|_| output_failed())?;
    if !mode.eq_ignore_ascii_case("delete") {
        return Err(verification_failed());
    }
    if connection.execute_batch("VACUUM").is_err() {
        return Err(check(deadline, cancellation)
            .err()
            .unwrap_or_else(output_failed));
    }
    drop(connection);
    check(deadline, cancellation)?;
    reject_sidecars(path)
}

fn validate_shape(plan: &LifecyclePlan) -> Result<(), WorkspaceError> {
    if plan.operation() != Operation::CompactDuplicate
        || plan.inputs().len() != 1
        || plan.inputs()[0].role() != InputRole::Source
        || plan.decisions().len() != 1
        || plan.decisions()[0].scope() != DecisionScope::Application
        || plan.decisions()[0].action() != COMPACT_ACTION
    {
        return Err(invalid_contract());
    }
    let value = serde_json::to_value(plan).map_err(|_| invalid_contract())?;
    let parameters = value["decisions"][0]["parameters"]
        .as_object()
        .ok_or_else(invalid_contract)?;
    if parameters.len() != 6
        || parameters.get("source_profile").and_then(|v| v.as_str())
            != Some(crate::COPY_SOURCE_PROFILE)
        || parameters
            .get("logical_state_profile")
            .and_then(|v| v.as_str())
            != Some(crate::COMPACT_LOGICAL_STATE_PROFILE)
        || parameters
            .get("logical_state_sha256")
            .and_then(|v| v.as_str())
            .is_none_or(|value| !valid_sha256(value))
        || parameters
            .get("page_size")
            .and_then(|v| v.as_u64())
            .is_none()
        || !matches!(
            parameters.get("signature_state").and_then(|v| v.as_str()),
            Some("unsigned" | "signed-valid")
        )
        || parameters
            .get("signature_count")
            .and_then(|v| v.as_u64())
            .is_none()
    {
        return Err(invalid_contract());
    }
    Ok(())
}

fn bind_source(plan: &LifecyclePlan, source: &VerifiedCompactSource) -> Result<(), WorkspaceError> {
    let input = &plan.inputs()[0];
    let capsule = input.capsule();
    let identity = source.identity();
    let live = source.source().source_identity();
    let expected = plan.expected();
    let value = serde_json::to_value(plan).map_err(|_| invalid_contract())?;
    let parameters = &value["decisions"][0]["parameters"];
    if input.path_hint() != utf8_path(source.source().canonical_path())?
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
        || parameters["logical_state_sha256"].as_str()
            != Some(source.logical_state().digest_sha256.as_str())
        || parameters["page_size"].as_u64() != Some(u64::from(source.logical_state().page_size))
        || parameters["signature_state"].as_str()
            != Some(signature_state_name(identity.signature_state))
        || parameters["signature_count"].as_u64() != Some(u64::from(identity.signature_count))
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
        || expected.platform() != std::env::consts::OS
        || expected.volume_or_device() != actual.device.to_string()
        || expected.file_id_or_inode() != actual.stable_file_id
    {
        return Err(stale_plan());
    }
    Ok(())
}

fn require_logical_identity(
    expected: &CopySourceIdentity,
    actual: &CopySourceIdentity,
) -> Result<(), WorkspaceError> {
    if expected.format_version == actual.format_version
        && expected.capsule_id == actual.capsule_id
        && expected.revision_id == actual.revision_id
        && expected.app_id == actual.app_id
        && expected.app_version == actual.app_version
        && expected.data_schema_id == actual.data_schema_id
        && expected.data_schema_version == actual.data_schema_version
        && expected.signature_state == actual.signature_state
        && expected.signature_count == actual.signature_count
        && expected.application_digest == actual.application_digest
    {
        Ok(())
    } else {
        Err(verification_failed())
    }
}

fn require_compact_state(
    expected: &CompactLogicalState,
    actual: &CompactLogicalState,
) -> Result<(), WorkspaceError> {
    if expected == actual {
        Ok(())
    } else {
        Err(verification_failed())
    }
}

fn require_compact_pragmas(connection: &Connection, page_size: u32) -> Result<(), WorkspaceError> {
    let actual_page_size: u32 = connection
        .pragma_query_value(None, "page_size", |row| row.get(0))
        .map_err(|_| verification_failed())?;
    let freelist: i64 = connection
        .pragma_query_value(None, "freelist_count", |row| row.get(0))
        .map_err(|_| verification_failed())?;
    let journal: String = connection
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .map_err(|_| verification_failed())?;
    if actual_page_size == page_size && freelist == 0 && journal.eq_ignore_ascii_case("delete") {
        Ok(())
    } else {
        Err(verification_failed())
    }
}

fn reject_sidecars(path: &Path) -> Result<(), WorkspaceError> {
    for suffix in ["-journal", "-wal", "-shm"] {
        let mut sidecar = OsString::from(path.as_os_str());
        sidecar.push(suffix);
        if PathBuf::from(sidecar).exists() {
            return Err(WorkspaceError::new(
                WorkspaceErrorCode::SourceJournalStateUnsupported,
            ));
        }
    }
    Ok(())
}

fn open_output(
    path: &Path,
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<VerifiedCopySource, WorkspaceError> {
    VerifiedCopySource::open_with_control(
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
    if expected.device == actual.device
        && expected.stable_file_id == actual.stable_file_id
        && expected.bytes == actual.bytes
    {
        Ok(())
    } else {
        Err(verification_failed())
    }
}

#[cfg(test)]
fn maybe_crash(stage: &str) {
    if std::env::var_os("SQLITE_CAPSULE_COMPACT_CRASH_STAGE").is_some_and(|value| value == stage) {
        std::process::exit(97);
    }
}

#[cfg(not(test))]
const fn maybe_crash(_stage: &str) {}

fn control(
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<sqlite_capsule_launch::VerificationControl, WorkspaceError> {
    Ok(sqlite_capsule_launch::VerificationControl::new(
        remaining(deadline)?,
        cancellation.shared_flag(),
    ))
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

fn signature_state_name(state: CopySourceSignatureState) -> &'static str {
    match state {
        CopySourceSignatureState::Unsigned => "unsigned",
        CopySourceSignatureState::SignedValid => "signed-valid",
    }
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn absolute_output(path: &Path) -> Result<PathBuf, WorkspaceError> {
    if !path.is_absolute() {
        return Err(invalid_contract());
    }
    Ok(path.to_path_buf())
}

fn utf8_path(path: &Path) -> Result<String, WorkspaceError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(invalid_contract)
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
    use crate::copy_source::tests::fixture;
    use rusqlite::params;
    use sha2::Digest;

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

    #[test]
    fn compact_crash_worker() {
        let Some(_) = std::env::var_os("SQLITE_CAPSULE_COMPACT_CRASH_STAGE") else {
            return;
        };
        let source_path = PathBuf::from(
            std::env::var_os("SQLITE_CAPSULE_COMPACT_CRASH_SOURCE").expect("crash source"),
        );
        let output_path = PathBuf::from(
            std::env::var_os("SQLITE_CAPSULE_COMPACT_CRASH_OUTPUT").expect("crash output"),
        );
        let source = VerifiedCompactSource::open(&source_path).expect("compact crash source");
        let cancellation = CancellationToken::new();
        prepare_at(source, &output_path, &cancellation)
            .stage()
            .expect("compact crash stage")
            .compact_and_validate()
            .expect("compact crash validation")
            .publish()
            .expect("compact crash publication");
        panic!("configured compact crash stage did not terminate");
    }

    #[test]
    fn abrupt_compact_stages_never_mutate_input_or_report_incomplete_success() {
        let (directory, source_path) = fixture("compact-crash-matrix", 3, false);
        let source_before = sha2::Sha256::digest(fs::read(&source_path).expect("source bytes"));
        let executable = std::env::current_exe().expect("current test executable");
        for stage in [
            "private-created",
            "snapshot-copied",
            "vacuumed",
            "sealed-and-verified",
            "postrename-reopened",
        ] {
            let output_path = directory
                .path()
                .join(format!("crash-{stage}.sqlitecapsule"));
            let status = Command::new(&executable)
                .arg("compact_copy::tests::compact_crash_worker")
                .arg("--exact")
                .env("SQLITE_CAPSULE_COMPACT_CRASH_STAGE", stage)
                .env("SQLITE_CAPSULE_COMPACT_CRASH_SOURCE", &source_path)
                .env("SQLITE_CAPSULE_COMPACT_CRASH_OUTPUT", &output_path)
                .status()
                .expect("run compact crash worker");
            assert_eq!(status.code(), Some(97), "compact crash stage {stage}");
            assert_eq!(
                sha2::Sha256::digest(fs::read(&source_path).unwrap()).as_slice(),
                source_before.as_slice(),
                "source changed at compact crash stage {stage}"
            );
            if stage == "postrename-reopened" {
                let output = VerifiedCompactSource::open(&output_path)
                    .expect("postrename compact residue is a verified capsule");
                require_compact_pragmas(
                    output.source().verified_connection(),
                    output.logical_state().page_size,
                )
                .expect("postrename compact residue has final storage state");
            } else {
                assert!(
                    !output_path.exists(),
                    "incomplete compact final at stage {stage}"
                );
            }
            for entry in fs::read_dir(directory.path()).expect("list compact crash residue") {
                let name = entry.expect("compact crash residue").file_name();
                let name = name.to_string_lossy();
                if name.starts_with(".sqlite-capsule-") {
                    assert!(name.ends_with(".private"), "unexpected residue {name}");
                }
            }
        }
    }

    #[test]
    fn compact_plan_vector_is_canonical_and_uses_the_versioned_operation() {
        let bytes =
            include_bytes!("../../../../compatibility/compact-copy-plan-v1/vector-plan.json");
        let plan = parse_compact_copy_plan(bytes).expect("frozen compact plan vector");
        assert_eq!(plan.operation(), Operation::CompactDuplicate);
        assert_eq!(plan.decisions()[0].action(), COMPACT_ACTION);
        assert_eq!(
            plan.canonical_bytes().expect("canonical compact plan"),
            bytes
        );
    }

    fn request(path: &Path) -> CompactCopyPlanRequest<'_> {
        CompactCopyPlanRequest {
            output_path: path,
            plan_id: "44152cac-a9e7-4633-a567-10fd8cfa5dd1",
            created_at: CREATED,
            expires_at: EXPIRES,
            deadline: Duration::from_secs(30),
            max_output_bytes: sqlite_capsule_core::MAX_CAPSULE_BYTES,
        }
    }

    fn prepare_at(
        source: VerifiedCompactSource,
        output: &Path,
        cancellation: &CancellationToken,
    ) -> PreparedCompactCopy {
        let review = generate_compact_copy_plan(&source, &request(output)).expect("compact review");
        let approved =
            parse_compact_copy_plan(&review.plan().canonical_bytes().expect("canonical review"))
                .expect("approved plan");
        PreparedCompactCopy::prepare_at(
            review,
            approved,
            source,
            operation_time(),
            &WorkspaceLimits::default(),
            cancellation,
        )
        .expect("prepared compact copy")
    }

    #[test]
    fn signed_and_unsigned_v02_v03_compact_in_owner_private_storage() {
        for version in [2, 3] {
            for signed in [false, true] {
                let (directory, source_path) =
                    fixture(&format!("compact-v0{version}-{signed}"), version, signed);
                let output = directory
                    .path()
                    .join(format!("output-{version}-{signed}.sqlite"));
                let before = fs::read(&source_path).expect("source bytes");
                let source = VerifiedCompactSource::open(&source_path).expect("compact source");
                let expected_identity = source.identity().clone();
                let expected_state = source.logical_state().clone();
                let cancellation = CancellationToken::new();
                let published = prepare_at(source, &output, &cancellation)
                    .stage()
                    .expect("private stage")
                    .compact_and_validate()
                    .expect("validated compact")
                    .publish()
                    .expect("published compact");
                assert_eq!(published.path(), output);
                assert_eq!(fs::read(&source_path).expect("source unchanged"), before);
                let reopened = VerifiedCompactSource::open(&output).expect("reopen output");
                require_logical_identity(&expected_identity, reopened.identity())
                    .expect("logical identity preserved");
                assert_eq!(reopened.logical_state(), &expected_state);
                require_compact_pragmas(
                    reopened.source().verified_connection(),
                    expected_state.page_size,
                )
                .expect("compact pragmas");
            }
        }
    }

    #[test]
    fn deleted_high_entropy_sentinel_and_freelist_are_removed() {
        let (directory, source_path) = fixture("compact-sentinel", 3, false);
        let sentinel = b"deleted-7c79e280-a4be-49ab-b371-2ea598fcbf47".repeat(180);
        let connection = Connection::open(&source_path).expect("sentinel writer");
        connection
            .pragma_update(None, "secure_delete", "OFF")
            .expect("disable secure delete for fixture");
        let transaction = connection
            .unchecked_transaction()
            .expect("sentinel transaction");
        for index in 0..48 {
            transaction
                .execute(
                    "INSERT INTO vector_domain(id,note,measurement,payload) VALUES (?1,'deleted',1.0,?2)",
                    params![format!("deleted-{index}"), &sentinel],
                )
                .expect("insert deleted sentinel row");
        }
        transaction
            .execute("DELETE FROM vector_domain WHERE id LIKE 'deleted-%'", [])
            .expect("delete sentinel rows");
        transaction.commit().expect("commit deleted pages");
        let source_freelist: i64 = connection
            .pragma_query_value(None, "freelist_count", |row| row.get(0))
            .expect("source freelist");
        drop(connection);
        let source_bytes = fs::read(&source_path).expect("source raw bytes");
        assert!(source_freelist > 0, "fixture must create free pages");
        assert!(contains(&source_bytes, &sentinel[..128]));

        let output = directory.path().join("compact.sqlite");
        let source = VerifiedCompactSource::open(&source_path).expect("compact source");
        let cancellation = CancellationToken::new();
        prepare_at(source, &output, &cancellation)
            .stage()
            .expect("stage")
            .compact_and_validate()
            .expect("validate")
            .publish()
            .expect("publish");
        let output_bytes = fs::read(&output).expect("compact bytes");
        assert!(!contains(&output_bytes, &sentinel[..128]));
        let connection = Connection::open_with_flags(&output, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("output reader");
        let freelist: i64 = connection
            .pragma_query_value(None, "freelist_count", |row| row.get(0))
            .expect("output freelist");
        assert_eq!(freelist, 0);
        assert_eq!(
            fs::read(&source_path).expect("source still unchanged"),
            source_bytes
        );
    }

    #[test]
    fn implicit_rowid_renumbering_fails_logical_validation_without_publication() {
        let (directory, source_path) = fixture("compact-rowid-gap", 3, false);
        let connection = Connection::open(&source_path).expect("rowid-gap writer");
        connection
            .execute_batch(
                "CREATE TABLE rowid_probe(value TEXT NOT NULL); \
                 INSERT INTO rowid_probe VALUES ('first'),('deleted-gap'),('last'); \
                 DELETE FROM rowid_probe WHERE rowid=2;",
            )
            .expect("rowid-gap fixture");
        let rowids = connection
            .prepare("SELECT rowid FROM rowid_probe ORDER BY rowid")
            .expect("rowid query")
            .query_map([], |row| row.get::<_, i64>(0))
            .expect("rowid rows")
            .collect::<Result<Vec<_>, _>>()
            .expect("rowid values");
        assert_eq!(rowids, vec![1, 3]);
        drop(connection);

        let source_bytes = fs::read(&source_path).expect("source before compact attempt");
        let output = directory.path().join("must-not-publish.sqlite");
        let source = VerifiedCompactSource::open(&source_path).expect("compact source");
        let cancellation = CancellationToken::new();
        let staged = prepare_at(source, &output, &cancellation)
            .stage()
            .expect("private stage");
        let error = match staged.compact_and_validate() {
            Ok(_) => panic!("rowid-renumbered output must not validate"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::VerificationFailed);
        assert!(!output.exists());
        assert_eq!(
            fs::read(&source_path).expect("source after compact rejection"),
            source_bytes
        );
    }

    #[test]
    fn destination_aba_cancellation_and_drop_paths_fail_closed() {
        let (directory, source_path) = fixture("compact-races", 3, false);
        let output = directory.path().join("compact.sqlite");
        fs::write(&output, b"existing").expect("existing output");
        let source = VerifiedCompactSource::open(&source_path).expect("source");
        let error = match generate_compact_copy_plan(&source, &request(&output)) {
            Ok(_) => panic!("existing destination must fail"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::DestinationExists);
        assert_eq!(fs::read(&output).expect("existing bytes"), b"existing");

        fs::remove_file(&output).expect("remove existing fixture");
        let source = VerifiedCompactSource::open(&source_path).expect("source again");
        let cancellation = CancellationToken::new();
        let prepared = prepare_at(source, &output, &cancellation);
        fs::write(&output, b"racer").expect("destination race");
        let error = match prepared.stage() {
            Ok(_) => panic!("destination race must fail"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::DestinationExists);
        assert_eq!(fs::read(&output).expect("racer bytes"), b"racer");

        fs::remove_file(&output).expect("remove racer");
        let source = VerifiedCompactSource::open(&source_path).expect("source for ABA");
        let cancellation = CancellationToken::new();
        let prepared = prepare_at(source, &output, &cancellation);
        let same_bytes = fs::read(&source_path).expect("ABA bytes");
        fs::write(&source_path, &same_bytes).expect("same-byte ABA rewrite");
        let error = match prepared.stage() {
            Ok(_) => panic!("ABA source rewrite must fail"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::StalePlan);
        assert!(!output.exists());

        let source = VerifiedCompactSource::open(&source_path).expect("source for cancel");
        let cancellation = CancellationToken::new();
        let prepared = prepare_at(source, &output, &cancellation);
        cancellation.cancel();
        assert_eq!(
            match prepared.stage() {
                Ok(_) => panic!("cancelled stage must fail"),
                Err(error) => error.kind(),
            },
            WorkspaceErrorCode::Cancelled
        );
        assert!(!output.exists());

        let source = VerifiedCompactSource::open(&source_path).expect("source for drop");
        let cancellation = CancellationToken::new();
        let staged = prepare_at(source, &output, &cancellation)
            .stage()
            .expect("private stage");
        drop(staged);
        assert!(!output.exists());

        let source = VerifiedCompactSource::open(&source_path).expect("source for validated drop");
        let validated = prepare_at(source, &output, &cancellation)
            .stage()
            .expect("private stage for validation")
            .compact_and_validate()
            .expect("validated private compact output");
        drop(validated);
        assert!(!output.exists());

        let source =
            VerifiedCompactSource::open(&source_path).expect("source for late destination race");
        let validated = prepare_at(source, &output, &cancellation)
            .stage()
            .expect("late race stage")
            .compact_and_validate()
            .expect("late race validation");
        fs::write(&output, b"late racer").expect("late destination racer");
        let error = match validated.publish() {
            Ok(_) => panic!("late destination race must fail"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::DestinationExists);
        assert_eq!(fs::read(&output).expect("late racer bytes"), b"late racer");
    }

    #[test]
    fn destination_sidecars_are_preserved_and_never_reported_as_success() {
        let (directory, source_path) = fixture("compact-sidecar-races", 3, false);
        let output = directory.path().join("compact.sqlite");
        let source_before = fs::read(&source_path).expect("source before sidecar races");

        let wal = PathBuf::from(format!("{}-wal", output.to_string_lossy()));
        fs::write(&wal, b"foreign wal before review").expect("pre-review WAL");
        let source = VerifiedCompactSource::open(&source_path).expect("pre-review source");
        let error = match generate_compact_copy_plan(&source, &request(&output)) {
            Ok(_) => panic!("a pre-existing destination sidecar must block review"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::DestinationExists);
        assert_eq!(
            fs::read(&wal).expect("preserved pre-review WAL"),
            b"foreign wal before review"
        );
        fs::remove_file(&wal).expect("remove pre-review WAL fixture");

        let source = VerifiedCompactSource::open(&source_path).expect("post-review source");
        let review = generate_compact_copy_plan(&source, &request(&output)).expect("review");
        let approved =
            parse_compact_copy_plan(&review.plan().canonical_bytes().expect("canonical review"))
                .expect("approved review");
        let shm = PathBuf::from(format!("{}-shm", output.to_string_lossy()));
        fs::write(&shm, b"foreign shm after review").expect("post-review SHM");
        let cancellation = CancellationToken::new();
        let error = match PreparedCompactCopy::prepare_at(
            review,
            approved,
            source,
            operation_time(),
            &WorkspaceLimits::default(),
            &cancellation,
        ) {
            Ok(_) => panic!("a sidecar raced after review must block preparation"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::DestinationExists);
        assert_eq!(
            fs::read(&shm).expect("preserved post-review SHM"),
            b"foreign shm after review"
        );
        fs::remove_file(&shm).expect("remove post-review SHM fixture");

        let source = VerifiedCompactSource::open(&source_path).expect("postrename source");
        let validated = prepare_at(source, &output, &cancellation)
            .stage()
            .expect("postrename sidecar stage")
            .compact_and_validate()
            .expect("postrename sidecar validation");
        let journal = PathBuf::from(format!("{}-journal", output.to_string_lossy()));
        let journal_for_hook = journal.clone();
        let error = match validated.publish_with_hook(move || {
            fs::write(&journal_for_hook, b"foreign journal during callback")
                .expect("callback journal race");
        }) {
            Ok(_) => panic!("a final-name sidecar race must never report success"),
            Err(error) => error,
        };
        assert_eq!(
            error.kind(),
            WorkspaceErrorCode::PostpublishVerificationFailed
        );
        assert_eq!(
            fs::read(&journal).expect("foreign callback journal preserved"),
            b"foreign journal during callback"
        );
        assert!(
            fs::read_dir(directory.path())
                .expect("sidecar quarantine inventory")
                .any(|entry| {
                    let name = entry.expect("sidecar quarantine entry").file_name();
                    let name = name.to_string_lossy();
                    name.contains("failed") || name.contains("quarantine")
                }),
            "postrename sidecar race must leave quarantine or marker evidence"
        );
        assert_eq!(
            fs::read(&source_path).expect("source after sidecar races"),
            source_before
        );
    }

    #[test]
    fn compact_plan_tampering_and_late_source_change_never_report_success() {
        let (directory, source_path) = fixture("compact-plan", 2, false);
        let output = directory.path().join("compact.sqlite");
        let source = VerifiedCompactSource::open(&source_path).expect("source");
        let review = generate_compact_copy_plan(&source, &request(&output)).expect("review");
        let preview = review.preview(&source).expect("path-free preview");
        let preview_json = serde_json::to_string(&preview).expect("serialize preview");
        assert_eq!(preview.format_version, "0.2");
        assert_eq!(
            preview.logical_state_sha256,
            source.logical_state().digest_sha256
        );
        assert!(!preview.overwrite_allowed);
        assert!(!preview_json.contains(&source_path.to_string_lossy().to_string()));
        assert!(!preview_json.contains(&output.to_string_lossy().to_string()));
        let mut value: serde_json::Value =
            serde_json::from_slice(&review.plan().canonical_bytes().expect("review bytes"))
                .expect("review JSON");
        value["decisions"][0]["parameters"]["logical_state_sha256"] =
            serde_json::Value::String("00".repeat(32));
        value["plan_digest"] =
            serde_json::Value::String(canonical_digest_value(&value).expect("edited digest"));
        let edited = parse_compact_copy_plan(&serde_json::to_vec(&value).expect("edited bytes"))
            .expect("shape-valid edit");
        let cancellation = CancellationToken::new();
        let error = match PreparedCompactCopy::prepare_at(
            review,
            edited,
            source,
            operation_time(),
            &WorkspaceLimits::default(),
            &cancellation,
        ) {
            Ok(_) => panic!("edited logical digest must not gain authority"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::StalePlan);

        let source = VerifiedCompactSource::open(&source_path).expect("late source");
        let validated = prepare_at(source, &output, &cancellation)
            .stage()
            .expect("stage")
            .compact_and_validate()
            .expect("validate");
        let error = match validated.publish_with_hook(|| {
            let connection = Connection::open(&source_path).expect("late writer");
            connection
                .execute("UPDATE vector_domain SET note='late' WHERE id='domain'", [])
                .expect("late mutation");
        }) {
            Ok(_) => panic!("late source mutation must not report success"),
            Err(error) => error,
        };
        assert_eq!(
            error.kind(),
            WorkspaceErrorCode::PostpublishVerificationFailed
        );
        assert!(
            fs::read_dir(directory.path())
                .expect("quarantine inventory")
                .any(|entry| {
                    let name = entry.expect("entry").file_name();
                    let name = name.to_string_lossy();
                    name.contains("failed") || name.contains("quarantine")
                })
        );

        let (directory, source_path) = fixture("compact-late-cancel", 3, false);
        let output = directory.path().join("compact.sqlite");
        let source = VerifiedCompactSource::open(&source_path).expect("late cancel source");
        let cancellation = CancellationToken::new();
        let validated = prepare_at(source, &output, &cancellation)
            .stage()
            .expect("late cancel stage")
            .compact_and_validate()
            .expect("late cancel validation");
        let cancel_at_callback = cancellation.clone();
        let error = match validated.publish_with_hook(move || cancel_at_callback.cancel()) {
            Ok(_) => panic!("late cancellation must not report success"),
            Err(error) => error,
        };
        assert_eq!(
            error.kind(),
            WorkspaceErrorCode::PostpublishVerificationFailed
        );
        assert!(
            fs::read_dir(directory.path())
                .expect("cancel quarantine inventory")
                .any(|entry| {
                    let name = entry.expect("entry").file_name();
                    let name = name.to_string_lossy();
                    name.contains("failed") || name.contains("quarantine")
                })
        );
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack
            .windows(needle.len())
            .any(|window| window == needle)
    }
}

//! Safe workspace-owned output staging and publication typestate.

use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
    time::{Instant, SystemTime},
};

use sqlite_capsule_lifecycle::{
    PrivateOutput, PublishedOutput, SealedPrivateOutput, SourceIdentity,
};

use crate::{
    CancellationToken, LifecyclePlan, PreparedPlan, PreparedPlanInput, VerifiedWorkspaceSource,
    WorkspaceError, WorkspaceErrorCode, WorkspaceLimits,
    prepared_plan::{
        map_destination_error, map_prepared_destination_error, rebind_prepared_inputs,
        validate_operation_specific, validate_output_expected,
    },
};

pub struct WorkspaceStagingOutput {
    plan: LifecyclePlan,
    inputs: Vec<PreparedPlanInput>,
    private: PrivateOutput,
    deadline: Instant,
}

pub struct ValidatedWorkspaceOutput {
    plan: LifecyclePlan,
    inputs: Vec<PreparedPlanInput>,
    sealed: SealedPrivateOutput,
    verified_sha256: [u8; 32],
    deadline: Instant,
}

pub struct PublishedCapsule {
    inner: PublishedOutput,
}

impl PreparedPlan {
    pub fn stage_output(
        self,
        limits: &WorkspaceLimits,
        cancellation: &CancellationToken,
    ) -> Result<WorkspaceStagingOutput, WorkspaceError> {
        self.stage_output_at(SystemTime::now(), limits, cancellation)
    }

    pub(crate) fn stage_output_at(
        self,
        now: SystemTime,
        limits: &WorkspaceLimits,
        cancellation: &CancellationToken,
    ) -> Result<WorkspaceStagingOutput, WorkspaceError> {
        self.assert_current_at(now, limits, cancellation)?;
        let (plan, inputs, destination, deadline) = self.into_parts();
        let private = destination
            .stage()
            .map_err(map_prepared_destination_error)?;
        Ok(WorkspaceStagingOutput {
            plan,
            inputs,
            private,
            deadline,
        })
    }
}

impl WorkspaceStagingOutput {
    pub fn copy_input_snapshot(
        &mut self,
        role: crate::InputRole,
        limits: &WorkspaceLimits,
        cancellation: &CancellationToken,
    ) -> Result<u64, WorkspaceError> {
        let input = self
            .inputs
            .iter()
            .find(|input| input.role() == role)
            .ok_or_else(|| WorkspaceError::new(WorkspaceErrorCode::InvalidContract))?;
        let limits = remaining_limits(limits, self.deadline)?;
        let control = sqlite_capsule_launch::VerificationControl::new(
            limits.deadline,
            cancellation.shared_flag(),
        );
        input
            .source()
            .verified
            .copy_snapshot_to_file_with_control(
                self.private.file_mut(),
                &control,
                self.plan.limits().max_output_bytes(),
            )
            .map_err(|error| match error {
                sqlite_capsule_launch::LaunchError::Cancelled => {
                    WorkspaceError::new(WorkspaceErrorCode::Cancelled)
                }
                sqlite_capsule_launch::LaunchError::LimitExceeded => {
                    WorkspaceError::new(WorkspaceErrorCode::LimitExceeded)
                }
                _ => WorkspaceError::new(WorkspaceErrorCode::VerificationFailed),
            })
    }

    pub fn validate(
        self,
        limits: &WorkspaceLimits,
        cancellation: &CancellationToken,
    ) -> Result<ValidatedWorkspaceOutput, WorkspaceError> {
        self.validate_at(SystemTime::now(), limits, cancellation)
    }

    pub(crate) fn validate_at(
        self,
        now: SystemTime,
        limits: &WorkspaceLimits,
        cancellation: &CancellationToken,
    ) -> Result<ValidatedWorkspaceOutput, WorkspaceError> {
        let limits = remaining_limits(limits, self.deadline)?;
        rebind_prepared_inputs(&self.plan, &self.inputs, now, &limits, cancellation)?;
        let sealed = self
            .private
            .seal_with_limit(self.plan.limits().max_output_bytes())
            .map_err(map_destination_error)?;
        if sealed.identity().bytes > self.plan.limits().max_output_bytes() {
            return Err(WorkspaceError::new(WorkspaceErrorCode::LimitExceeded));
        }
        sealed
            .assert_staged_current()
            .map_err(map_destination_error)?;
        let output = VerifiedWorkspaceSource::open_with_control(
            sealed.private_path_hint(),
            &limits,
            cancellation,
        )?;
        require_same_object(sealed.identity(), output.source_identity())?;
        if sealed.sha256() != &output.verified.source_sha256 {
            return Err(WorkspaceError::new(WorkspaceErrorCode::VerificationFailed));
        }
        validate_output_expected(&self.plan, &output)?;
        validate_operation_specific(&self.plan, &self.inputs, &output)?;
        drop(output);
        sealed
            .assert_staged_current()
            .map_err(map_destination_error)?;
        rebind_prepared_inputs(&self.plan, &self.inputs, now, &limits, cancellation)?;
        let verified_sha256 = *sealed.sha256();
        Ok(ValidatedWorkspaceOutput {
            plan: self.plan,
            inputs: self.inputs,
            sealed,
            verified_sha256,
            deadline: self.deadline,
        })
    }
}

impl ValidatedWorkspaceOutput {
    pub fn publish(
        self,
        limits: &WorkspaceLimits,
        cancellation: &CancellationToken,
    ) -> Result<PublishedCapsule, WorkspaceError> {
        self.publish_at(SystemTime::now(), limits, cancellation)
    }

    pub(crate) fn publish_at(
        self,
        now: SystemTime,
        limits: &WorkspaceLimits,
        cancellation: &CancellationToken,
    ) -> Result<PublishedCapsule, WorkspaceError> {
        self.publish_at_with_hook(now, limits, cancellation, || {})
    }

    fn publish_at_with_hook<F>(
        self,
        now: SystemTime,
        limits: &WorkspaceLimits,
        cancellation: &CancellationToken,
        after_publish_before_final_input_rebind: F,
    ) -> Result<PublishedCapsule, WorkspaceError>
    where
        F: FnOnce(),
    {
        let limits = remaining_limits(limits, self.deadline)?;
        rebind_prepared_inputs(&self.plan, &self.inputs, now, &limits, cancellation)?;
        self.sealed
            .assert_staged_current()
            .map_err(map_destination_error)?;
        let prepublish = VerifiedWorkspaceSource::open_with_control(
            self.sealed.private_path_hint(),
            &limits,
            cancellation,
        )?;
        require_same_object(self.sealed.identity(), prepublish.source_identity())?;
        if prepublish.verified.source_sha256 != self.verified_sha256 {
            return Err(WorkspaceError::new(WorkspaceErrorCode::VerificationFailed));
        }
        validate_output_expected(&self.plan, &prepublish)?;
        validate_operation_specific(&self.plan, &self.inputs, &prepublish)?;
        drop(prepublish);
        self.sealed
            .assert_staged_current()
            .map_err(map_destination_error)?;
        rebind_prepared_inputs(&self.plan, &self.inputs, now, &limits, cancellation)?;

        let plan = &self.plan;
        let verified_sha256 = self.verified_sha256;
        // SAFETY: `ValidatedWorkspaceOutput` is private-field typestate created
        // only after exhaustive verification of the exact held staged file.
        // Inputs and staged identity/digest were rebound immediately above. The
        // callback repeats exhaustive verification on the reopened final file;
        // the low-level primitive then performs one more name/identity/digest
        // rebind before returning success.
        let published = unsafe {
            self.sealed
                .publish_no_replace_unchecked(|reopened, reopened_identity| {
                    let temporary = snapshot_held_file(
                        reopened,
                        plan.limits().max_output_bytes(),
                        self.deadline,
                        cancellation,
                    )?;
                    let output = VerifiedWorkspaceSource::open_with_control(
                        temporary.path(),
                        &remaining_limits(&limits, self.deadline).map_err(|_| {
                            sqlite_capsule_lifecycle::LifecycleError::PostPublishVerification
                        })?,
                        cancellation,
                    )
                    .map_err(|_| {
                        sqlite_capsule_lifecycle::LifecycleError::PostPublishVerification
                    })?;
                    if output.source_identity().bytes != reopened_identity.bytes {
                        return Err(
                            sqlite_capsule_lifecycle::LifecycleError::PostPublishVerification,
                        );
                    }
                    if output.verified.source_sha256 != verified_sha256
                        || validate_output_expected(plan, &output).is_err()
                        || validate_operation_specific(plan, &self.inputs, &output).is_err()
                    {
                        return Err(
                            sqlite_capsule_lifecycle::LifecycleError::PostPublishVerification,
                        );
                    }
                    after_publish_before_final_input_rebind();
                    if rebind_prepared_inputs(plan, &self.inputs, now, &limits, cancellation)
                        .is_err()
                    {
                        return Err(
                            sqlite_capsule_lifecycle::LifecycleError::PostPublishVerification,
                        );
                    }
                    Ok(())
                })
        }
        .map_err(map_destination_error)?;
        Ok(PublishedCapsule { inner: published })
    }
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

fn remaining_limits(
    base: &WorkspaceLimits,
    deadline: Instant,
) -> Result<WorkspaceLimits, WorkspaceError> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(WorkspaceError::new(WorkspaceErrorCode::LimitExceeded));
    }
    let mut limits = base.clone();
    limits.deadline = remaining.min(base.deadline);
    Ok(limits)
}

impl PublishedCapsule {
    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    pub fn identity(&self) -> &SourceIdentity {
        &self.inner.identity
    }
}

fn require_same_object(
    expected: &SourceIdentity,
    actual: &SourceIdentity,
) -> Result<(), WorkspaceError> {
    if same_object(expected, actual) && expected.bytes == actual.bytes {
        Ok(())
    } else {
        Err(WorkspaceError::new(WorkspaceErrorCode::VerificationFailed))
    }
}

fn same_object(left: &SourceIdentity, right: &SourceIdentity) -> bool {
    left.device == right.device
        && if left.stable_file_id.is_empty() || right.stable_file_id.is_empty() {
            left.file == right.file
        } else {
            left.stable_file_id == right.stable_file_id
        }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        process::Command,
        time::{Duration, UNIX_EPOCH},
    };

    use serde_json::json;
    use sha2::{Digest, Sha256};
    use sqlite_capsule_crypto::{application_digest, verify_signatures};
    use sqlite_capsule_lifecycle::DestinationReservation;

    use super::*;
    use crate::{InputRole, plan::canonical_digest_value, tests::signed_fixture};

    const CREATED: &str = "2026-08-12T12:00:00Z";
    const EXPIRES: &str = "2026-08-12T12:05:00Z";

    fn plan_for(source_path: &Path, output_path: &Path) -> LifecyclePlan {
        let source = VerifiedWorkspaceSource::open(source_path).expect("verified source");
        let identity = source.identity();
        let source_identity = source.source_identity();
        let schema = identity.overview.data_schema.as_ref().expect("data schema");
        let application_digest = lower_hex(
            &application_digest(source.verified.connection()).expect("application digest"),
        );
        let signature = verify_signatures(source.verified.connection())
            .expect("signatures")
            .into_iter()
            .find(|signature| signature.cryptographically_valid && signature.digest_matches)
            .expect("valid signature");
        let output_parent = output_path.parent().expect("output parent");
        let leaf = output_path.file_name().expect("output leaf");
        let reservation = DestinationReservation::reserve(output_parent, leaf, &[])
            .expect("probe destination identity");
        let parent_identity = reservation.identity().clone();
        drop(reservation);
        let source_sha256 = lower_hex(&source.verified.source_sha256);
        let mut value = json!({
            "profile": "org.sqlite-capsule.lifecycle-plan/1",
            "plan_id": "c5f44498-1a23-4c15-a384-e8a4782d0984",
            "operation": "duplicate",
            "created_at": CREATED,
            "expires_at": EXPIRES,
            "inputs": [{
                "role": "source",
                "path_hint": source_path.to_string_lossy(),
                "file_sha256": source_sha256,
                "snapshot_sha256": source_sha256,
                "size_bytes": source_identity.bytes,
                "filesystem_identity": {
                    "platform": std::env::consts::OS,
                    "volume_or_device": source_identity.device.to_string(),
                    "file_id_or_inode": source_identity.stable_file_id,
                    "modified_ns": source_identity.modified_ns
                },
                "capsule": {
                    "format_version": "0.3",
                    "capsule_id": identity.capsule_id,
                    "revision_id": identity.overview.instance.revision_id,
                    "app_id": identity.app_id,
                    "app_version": identity.app_version,
                    "application_digest": application_digest,
                    "data_schema_id": schema.data_schema_id,
                    "data_schema_version": schema.data_schema_version,
                    "publisher_key_id": signature.key_id
                }
            }],
            "output": {
                "path": output_path.to_string_lossy(),
                "leaf_name": leaf.to_string_lossy(),
                "parent_filesystem_identity": {
                    "platform": std::env::consts::OS,
                    "volume_or_device": parent_identity.device.to_string(),
                    "file_id_or_inode": parent_identity.stable_file_id
                },
                "must_not_exist": true,
                "publish_mode": "create-new-no-replace"
            },
            "decisions": [{
                "scope": "application",
                "subject": identity.app_id,
                "action": "copy-exact-snapshot",
                "reason": "Duplicate preserves the exact verified Capsule snapshot.",
                "parameters": {}
            }],
            "limits": {
                "max_input_bytes": 67108864,
                "max_output_bytes": 67108864,
                "max_rows_inspected": 100000,
                "max_rows_written": 100000,
                "deadline_ms": 30000
            },
            "expected": {
                "capsule_id": identity.capsule_id,
                "revision_id": identity.overview.instance.revision_id,
                "app_id": identity.app_id,
                "application_digest": application_digest,
                "data_schema_id": schema.data_schema_id,
                "data_schema_version": schema.data_schema_version
            },
            "plan_digest": "0000000000000000000000000000000000000000000000000000000000000000"
        });
        let digest = canonical_digest_value(&value).expect("plan digest");
        value["plan_digest"] = serde_json::Value::String(digest);
        LifecyclePlan::parse(&serde_json::to_vec(&value).expect("plan JSON")).expect("plan")
    }

    fn operation_time() -> SystemTime {
        UNIX_EPOCH
            + Duration::from_secs(
                super::super::prepared_plan::parse_utc_seconds(CREATED).unwrap() + 60,
            )
    }

    fn time(value: &str) -> SystemTime {
        UNIX_EPOCH
            + Duration::from_secs(super::super::prepared_plan::parse_utc_seconds(value).unwrap())
    }

    #[test]
    fn plan_time_boundaries_use_the_exact_stable_codes() {
        let (_directory, source_path) = signed_fixture("plan-time-boundaries");
        let output_path = source_path
            .parent()
            .unwrap()
            .join("time-copy.capsule.sqlite");
        let canonical = plan_for(&source_path, &output_path)
            .canonical_bytes()
            .expect("canonical plan");
        let limits = WorkspaceLimits::default();
        for (label, now, code) in [
            (
                "before-created",
                "2026-08-11T23:59:59Z",
                WorkspaceErrorCode::StalePlan,
            ),
            ("at-expiry", EXPIRES, WorkspaceErrorCode::SessionExpired),
            (
                "after-expiry",
                "2026-08-12T12:05:01Z",
                WorkspaceErrorCode::SessionExpired,
            ),
        ] {
            let plan = LifecyclePlan::parse(&canonical).expect("parse plan again");
            let error =
                match PreparedPlan::prepare_at(plan, time(now), &limits, &CancellationToken::new())
                {
                    Ok(_) => panic!("{label} must not prepare"),
                    Err(error) => error,
                };
            assert_eq!(error.kind(), code, "time boundary {label}");
        }
    }

    #[test]
    fn substituted_prepared_destination_maps_to_stale_plan() {
        let (directory, source_path) = signed_fixture("prepared-parent-substitution");
        let parent = directory.path().join("destination");
        let moved = directory.path().join("destination-moved");
        fs::create_dir(&parent).expect("create destination parent");
        let output_path = parent.join("copy.capsule.sqlite");
        let plan = plan_for(&source_path, &output_path);
        let limits = WorkspaceLimits::default();
        let cancellation = CancellationToken::new();
        let prepared = PreparedPlan::prepare_at(plan, operation_time(), &limits, &cancellation)
            .expect("prepare destination capability");
        fs::rename(&parent, &moved).expect("move prepared destination parent");
        fs::create_dir(&parent).expect("substitute destination parent");
        let error = match prepared.stage_output_at(operation_time(), &limits, &cancellation) {
            Ok(_) => panic!("substituted parent must not stage"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::StalePlan);
        assert!(!output_path.exists());
    }

    #[test]
    fn safe_publication_requires_full_validation_and_preserves_the_source() {
        let (_directory, source_path) = signed_fixture("safe-publication");
        let output_path = source_path.parent().unwrap().join("copy.capsule.sqlite");
        let before = Sha256::digest(fs::read(&source_path).expect("source bytes"));
        let plan = plan_for(&source_path, &output_path);
        let limits = WorkspaceLimits::default();
        let cancellation = CancellationToken::new();
        let prepared = PreparedPlan::prepare_at(plan, operation_time(), &limits, &cancellation)
            .expect("prepare plan");
        let mut staging = prepared
            .stage_output_at(operation_time(), &limits, &cancellation)
            .expect("stage output");
        staging
            .copy_input_snapshot(InputRole::Source, &limits, &cancellation)
            .expect("copy exact verified snapshot");
        let validated = staging
            .validate_at(operation_time(), &limits, &cancellation)
            .expect("validate staged output");
        let published = validated
            .publish_at(operation_time(), &limits, &cancellation)
            .expect("publish output");
        assert_eq!(published.path(), output_path);
        VerifiedWorkspaceSource::open(published.path()).expect("published output reopens");
        assert_eq!(
            before.as_slice(),
            Sha256::digest(fs::read(&source_path).unwrap()).as_slice()
        );
    }

    #[test]
    fn invalid_or_tampered_staging_never_reaches_the_final_name() {
        let (_directory, source_path) = signed_fixture("invalid-staging");
        let invalid_output = source_path.parent().unwrap().join("invalid.capsule.sqlite");
        let plan = plan_for(&source_path, &invalid_output);
        let limits = WorkspaceLimits::default();
        let cancellation = CancellationToken::new();
        let prepared =
            PreparedPlan::prepare_at(plan, operation_time(), &limits, &cancellation).unwrap();
        let mut staging = prepared
            .stage_output_at(operation_time(), &limits, &cancellation)
            .unwrap();
        use std::io::Write;
        staging
            .private
            .file_mut()
            .write_all(b"not a capsule")
            .unwrap();
        assert!(
            staging
                .validate_at(operation_time(), &limits, &cancellation)
                .is_err()
        );
        assert!(!invalid_output.exists());

        let tampered_output = source_path
            .parent()
            .unwrap()
            .join("tampered.capsule.sqlite");
        let plan = plan_for(&source_path, &tampered_output);
        let prepared =
            PreparedPlan::prepare_at(plan, operation_time(), &limits, &cancellation).unwrap();
        let mut staging = prepared
            .stage_output_at(operation_time(), &limits, &cancellation)
            .unwrap();
        staging
            .copy_input_snapshot(InputRole::Source, &limits, &cancellation)
            .unwrap();
        let validated = staging
            .validate_at(operation_time(), &limits, &cancellation)
            .unwrap();
        let path = validated.sealed.private_path_hint().to_path_buf();
        let mut bytes = fs::read(&path).unwrap();
        let last = bytes.last_mut().unwrap();
        *last ^= 1;
        fs::write(&path, bytes).unwrap();
        assert!(
            validated
                .publish_at(operation_time(), &limits, &cancellation)
                .is_err()
        );
        assert!(!tampered_output.exists());
    }

    #[test]
    fn source_mutation_in_the_final_publication_window_fails_closed() {
        use std::io::{Read, Seek, SeekFrom, Write};

        let (_directory, source_path) = signed_fixture("late-source-mutation");
        let output_path = source_path
            .parent()
            .unwrap()
            .join("late-copy.capsule.sqlite");
        let original = fs::read(&source_path).expect("original source bytes");
        let plan = plan_for(&source_path, &output_path);
        let limits = WorkspaceLimits::default();
        let cancellation = CancellationToken::new();
        let prepared = PreparedPlan::prepare_at(plan, operation_time(), &limits, &cancellation)
            .expect("prepare plan");
        let mut staging = prepared
            .stage_output_at(operation_time(), &limits, &cancellation)
            .expect("stage output");
        staging
            .copy_input_snapshot(InputRole::Source, &limits, &cancellation)
            .expect("copy source snapshot");
        let validated = staging
            .validate_at(operation_time(), &limits, &cancellation)
            .expect("validate output");

        let mutation_path = source_path.clone();
        let result =
            validated.publish_at_with_hook(operation_time(), &limits, &cancellation, move || {
                let mut file = fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&mutation_path)
                    .expect("open pinned source for same-object mutation");
                let offset = file.metadata().expect("source metadata").len() - 1;
                file.seek(SeekFrom::Start(offset)).expect("seek source");
                let mut byte = [0_u8; 1];
                file.read_exact(&mut byte).expect("read source byte");
                file.seek(SeekFrom::Start(offset)).expect("rewind source");
                file.write_all(&[byte[0] ^ 1]).expect("mutate source");
                file.sync_all().expect("sync source mutation");
            });
        let error = match result {
            Ok(_) => panic!("late source mutation must not publish"),
            Err(error) => error,
        };
        assert_eq!(
            error.kind(),
            WorkspaceErrorCode::PostpublishVerificationFailed
        );
        fs::write(&source_path, &original).expect("restore source fixture bytes");
        assert_eq!(fs::read(&source_path).unwrap(), original);
        assert!(
            !output_path.exists()
                || fs::read_dir(output_path.parent().unwrap())
                    .expect("list failure evidence")
                    .any(|entry| {
                        let name = entry.expect("entry").file_name();
                        let name = name.to_string_lossy();
                        name.contains("failed") || name.contains("quarantine")
                    })
        );
    }

    #[test]
    fn source_mutation_before_capture_and_during_transform_fails_closed() {
        use std::io::{Read, Seek, SeekFrom, Write};

        fn toggle_last_byte(path: &Path) {
            let mut file = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .expect("open same source object");
            let offset = file.metadata().unwrap().len() - 1;
            file.seek(SeekFrom::Start(offset)).unwrap();
            let mut byte = [0_u8; 1];
            file.read_exact(&mut byte).unwrap();
            file.seek(SeekFrom::Start(offset)).unwrap();
            file.write_all(&[byte[0] ^ 1]).unwrap();
            file.sync_all().unwrap();
        }

        let (_directory, source_path) = signed_fixture("source-race-stages");
        let original = fs::read(&source_path).expect("source bytes");
        let parent = source_path.parent().unwrap();
        let limits = WorkspaceLimits::default();
        let cancellation = CancellationToken::new();

        let before_capture_output = parent.join("before-capture.capsule.sqlite");
        let plan = plan_for(&source_path, &before_capture_output);
        toggle_last_byte(&source_path);
        let error = match PreparedPlan::prepare_at(plan, operation_time(), &limits, &cancellation) {
            Ok(_) => panic!("pre-capture mutation must not prepare"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::StalePlan);
        toggle_last_byte(&source_path);
        assert_eq!(fs::read(&source_path).unwrap(), original);
        assert!(!before_capture_output.exists());

        let transform_output = parent.join("during-transform.capsule.sqlite");
        let plan = plan_for(&source_path, &transform_output);
        let prepared = PreparedPlan::prepare_at(plan, operation_time(), &limits, &cancellation)
            .expect("prepare transform race");
        let mut staging = prepared
            .stage_output_at(operation_time(), &limits, &cancellation)
            .expect("stage transform race");
        staging
            .copy_input_snapshot(InputRole::Source, &limits, &cancellation)
            .expect("copy exact snapshot");
        toggle_last_byte(&source_path);
        let error = match staging.validate_at(operation_time(), &limits, &cancellation) {
            Ok(_) => panic!("mutation during transform must not validate"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), WorkspaceErrorCode::StalePlan);
        toggle_last_byte(&source_path);
        assert_eq!(fs::read(&source_path).unwrap(), original);
        assert!(!transform_output.exists());
    }

    #[test]
    fn publication_crash_worker() {
        let Some(stage) = std::env::var_os("SQLITE_CAPSULE_WORKSPACE_CRASH_STAGE") else {
            return;
        };
        let stage = stage.to_string_lossy();
        let source_path = PathBuf::from(
            std::env::var_os("SQLITE_CAPSULE_WORKSPACE_CRASH_SOURCE").expect("crash source"),
        );
        let output_path = PathBuf::from(
            std::env::var_os("SQLITE_CAPSULE_WORKSPACE_CRASH_OUTPUT").expect("crash output"),
        );
        let limits = WorkspaceLimits::default();
        let cancellation = CancellationToken::new();
        let plan = plan_for(&source_path, &output_path);
        let prepared = PreparedPlan::prepare_at(plan, operation_time(), &limits, &cancellation)
            .expect("crash worker prepare");
        let mut staging = prepared
            .stage_output_at(operation_time(), &limits, &cancellation)
            .expect("crash worker stage");
        if stage == "private-created" {
            std::process::exit(97);
        }
        staging
            .copy_input_snapshot(InputRole::Source, &limits, &cancellation)
            .expect("crash worker copy");
        if stage == "snapshot-copied" {
            std::process::exit(97);
        }
        let validated = staging
            .validate_at(operation_time(), &limits, &cancellation)
            .expect("crash worker validate");
        if stage == "sealed-and-verified" {
            std::process::exit(97);
        }
        if stage == "postrename-reopened" {
            let _ =
                validated.publish_at_with_hook(operation_time(), &limits, &cancellation, || {
                    std::process::exit(97)
                });
            panic!("postrename crash hook did not terminate");
        }
        panic!("unexpected crash stage {stage}");
    }

    #[test]
    fn abrupt_publication_stages_never_mutate_input_or_report_incomplete_success() {
        let (_directory, source_path) = signed_fixture("publication-crash-matrix");
        let source_before = Sha256::digest(fs::read(&source_path).expect("source bytes"));
        let executable = std::env::current_exe().expect("current test executable");
        for stage in [
            "private-created",
            "snapshot-copied",
            "sealed-and-verified",
            "postrename-reopened",
        ] {
            let output_path = source_path
                .parent()
                .unwrap()
                .join(format!("crash-{stage}.capsule.sqlite"));
            let status = Command::new(&executable)
                .arg("publication::tests::publication_crash_worker")
                .arg("--exact")
                .env("SQLITE_CAPSULE_WORKSPACE_CRASH_STAGE", stage)
                .env("SQLITE_CAPSULE_WORKSPACE_CRASH_SOURCE", &source_path)
                .env("SQLITE_CAPSULE_WORKSPACE_CRASH_OUTPUT", &output_path)
                .status()
                .expect("run crash worker");
            assert_eq!(status.code(), Some(97), "crash stage {stage}");
            assert_eq!(
                Sha256::digest(fs::read(&source_path).unwrap()).as_slice(),
                source_before.as_slice(),
                "source changed at crash stage {stage}"
            );
            if stage == "postrename-reopened" {
                VerifiedWorkspaceSource::open(&output_path)
                    .expect("postrename residue is a complete verified capsule");
            } else {
                assert!(!output_path.exists(), "incomplete final at stage {stage}");
            }
            for entry in fs::read_dir(source_path.parent().unwrap()).expect("list crash residue") {
                let entry = entry.expect("crash residue entry");
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with(".sqlite-capsule-") {
                    assert!(name.ends_with(".private"), "unexpected residue {name}");
                }
            }
        }
    }

    fn lower_hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

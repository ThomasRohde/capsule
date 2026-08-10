//! Durable, host-owned update staging and startup-health state.
//!
//! This crate does not fetch bytes or execute installers. It accepts only an
//! already verified update, preserves exact staged bytes and a prior installer,
//! and records immutable transitions that a platform host can recover after a
//! crash. Capsule application code has no handle to this API.

use std::{
    collections::BTreeSet,
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlite_capsule_distribution::{
    DistributionError, InstalledReleaseContext, RELEASE_PROFILE, ReleaseArtifact,
    ReleaseCandidateContext, SignedReleaseManifest, VerifiedInstallableUpdate,
    VerifiedReleaseCandidate, verify_installable_artifact_bytes,
    verify_installable_sigstore_bundle_bytes, verify_installed_release, verify_release_candidate,
};
#[cfg(test)]
use sqlite_capsule_distribution::{
    VerifiedUpdate, verify_artifact_bytes, verify_sigstore_bundle_bytes,
};
use sqlite_capsule_lifecycle::{LifecycleError, prepare_private_directory, protect_private_file};
use sqlite_capsule_platform::validate_platform_signing_identity;
use thiserror::Error;

pub const UPDATE_STAGE_PROFILE: &str = "org.sqlite-capsule.host-update-stage/0.4";
const MARKER_NAME: &str = "stage.in-progress";
const RECORD_NAME: &str = "stage.json";
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum UpdateStageError {
    #[error("update stage field is invalid")]
    Field,
    #[error("update stage already exists")]
    Exists,
    #[error("update stage is incomplete")]
    Incomplete,
    #[error("update stage inventory is invalid")]
    Invalid,
    #[error("update stage transition is invalid")]
    Transition,
    #[error("running version does not match the staged update")]
    Version,
    #[error(transparent)]
    Distribution(#[from] DistributionError),
    #[error(transparent)]
    Lifecycle(#[from] LifecycleError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Clone, Copy, Debug)]
pub struct PreviousInstaller<'a> {
    pub path: &'a Path,
    pub staged_name: &'a str,
    pub version: &'a str,
}

#[derive(Clone, Copy, Debug)]
pub struct StageRequest<'a> {
    pub artifact_name: &'a str,
    pub artifact_bytes: &'a [u8],
    pub sigstore_name: &'a str,
    pub sigstore_bytes: &'a [u8],
    pub previous_installer: Option<PreviousInstaller<'a>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateStageRecord {
    pub profile: String,
    pub stage_id: String,
    pub version: String,
    pub sequence: u64,
    pub target: String,
    pub platform_signing: String,
    pub platform_signing_identity: String,
    pub platform_timestamp_required: bool,
    pub sigstore_certificate_identity: String,
    pub sigstore_oidc_issuer: String,
    pub artifact_name: String,
    pub artifact_bytes: u64,
    pub artifact_sha256: String,
    pub sigstore_name: String,
    pub sigstore_bytes: u64,
    pub sigstore_sha256: String,
    pub previous_installer_name: Option<String>,
    pub previous_installer_version: Option<String>,
    pub previous_installer_bytes: Option<u64>,
    pub previous_installer_sha256: Option<String>,
    pub signed_release: SignedReleaseManifest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateStageState {
    Prepared,
    InstallerStarted,
    AwaitingHealth,
    Healthy,
    RollbackRequired,
    RollbackStarted,
    AwaitingRollbackHealth,
    RolledBack,
    RollbackFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RollbackReason {
    InstallerFailed,
    StartupFailed,
    VersionMismatch,
    OperatorRequested,
    RollbackInstallerFailed,
    RollbackVersionMismatch,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TransitionRecord {
    profile: String,
    stage_id: String,
    state: UpdateStageState,
    recorded_at_unix: u64,
    running_version: Option<String>,
    rollback_reason: Option<RollbackReason>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StagedUpdate {
    pub record: UpdateStageRecord,
    pub state: UpdateStageState,
    pub running_version: Option<String>,
    pub rollback_reason: Option<RollbackReason>,
}

/// A prepared stage rebound to a freshly verified copy of its persisted signed
/// release envelope. Construction is restricted to `prepare_installation`.
#[derive(Debug)]
pub struct PreparedInstallation {
    stage: StagedUpdate,
    candidate: VerifiedReleaseCandidate,
    artifact_path: PathBuf,
    previous_installer_path: Option<PathBuf>,
}

/// A rollback-required stage bound to its exact preserved installer. Only
/// `prepare_rollback` can construct this value.
#[derive(Debug)]
pub struct PreparedRollback {
    stage: StagedUpdate,
    installer_path: PathBuf,
}

/// Exact installer bytes from a previously healthy update stage, rebound to
/// the compiled release root and the currently running version/sequence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedInstallerSource {
    path: PathBuf,
    staged_name: String,
}

impl VerifiedInstallerSource {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn staged_name(&self) -> &str {
        &self.staged_name
    }
}

impl PreparedInstallation {
    pub fn stage(&self) -> &StagedUpdate {
        &self.stage
    }

    pub fn artifact_path(&self) -> &Path {
        &self.artifact_path
    }

    pub fn candidate(&self) -> &VerifiedReleaseCandidate {
        &self.candidate
    }

    pub fn previous_installer_path(&self) -> Option<&Path> {
        self.previous_installer_path.as_deref()
    }
}

impl PreparedRollback {
    pub fn stage(&self) -> &StagedUpdate {
        &self.stage
    }

    pub fn installer_path(&self) -> &Path {
        &self.installer_path
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UpdateInventoryReport {
    pub verified: Vec<StagedUpdate>,
    pub incomplete: Vec<String>,
    pub invalid: Vec<String>,
}

#[derive(Debug)]
pub struct UpdateStager {
    root: PathBuf,
}

impl UpdateStager {
    pub fn open(root: &Path) -> Result<Self, UpdateStageError> {
        if let Ok(metadata) = std::fs::symlink_metadata(root)
            && (metadata.file_type().is_symlink() || !metadata.is_dir())
        {
            return Err(UpdateStageError::Invalid);
        }
        prepare_private_directory(root)?;
        let root = root.canonicalize()?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn stage_verified(
        &self,
        update: &VerifiedInstallableUpdate,
        request: StageRequest<'_>,
    ) -> Result<StagedUpdate, UpdateStageError> {
        verify_installable_artifact_bytes(update, request.artifact_bytes)?;
        verify_installable_sigstore_bundle_bytes(update, request.sigstore_bytes)?;
        self.stage_release(
            update.version(),
            update.sequence(),
            update.artifact(),
            update.signed_release(),
            request,
        )
    }

    #[cfg(test)]
    fn stage_test_verified(
        &self,
        update: &VerifiedUpdate,
        signed_release: &SignedReleaseManifest,
        request: StageRequest<'_>,
    ) -> Result<StagedUpdate, UpdateStageError> {
        verify_artifact_bytes(update, request.artifact_bytes)?;
        verify_sigstore_bundle_bytes(update, request.sigstore_bytes)?;
        self.stage_release(
            &update.version,
            update.sequence,
            &update.artifact,
            signed_release,
            request,
        )
    }

    fn stage_release(
        &self,
        version: &str,
        sequence: u64,
        artifact: &ReleaseArtifact,
        signed_release: &SignedReleaseManifest,
        request: StageRequest<'_>,
    ) -> Result<StagedUpdate, UpdateStageError> {
        if !safe_name(request.artifact_name)
            || !safe_name(request.sigstore_name)
            || request.artifact_name == request.sigstore_name
            || request.artifact_bytes.len() as u64 > MAX_FILE_BYTES
            || request.sigstore_bytes.len() as u64 > MAX_FILE_BYTES
        {
            return Err(UpdateStageError::Field);
        }

        let stage_id = format!("{sequence:020}-{}", artifact.target);
        if !safe_stage_id(&stage_id) {
            return Err(UpdateStageError::Field);
        }
        let directory = self.root.join(&stage_id);
        match std::fs::create_dir(&directory) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(UpdateStageError::Exists);
            }
            Err(error) => return Err(error.into()),
        }
        prepare_private_directory(&directory)?;
        let mut cleanup = StageCleanup::new(directory.clone());

        write_json_new(
            &directory.join(MARKER_NAME),
            &serde_json::json!({"profile": UPDATE_STAGE_PROFILE, "stage_id": &stage_id}),
        )?;
        write_bytes_new(
            &directory.join(request.artifact_name),
            request.artifact_bytes,
        )?;
        fault_point("artifact-synced");
        write_bytes_new(
            &directory.join(request.sigstore_name),
            request.sigstore_bytes,
        )?;

        let previous = request
            .previous_installer
            .map(|installer| copy_previous_installer(&directory, installer))
            .transpose()?;
        let record = UpdateStageRecord {
            profile: UPDATE_STAGE_PROFILE.to_owned(),
            stage_id: stage_id.clone(),
            version: version.to_owned(),
            sequence,
            target: artifact.target.clone(),
            platform_signing: artifact.platform_signing.clone(),
            platform_signing_identity: artifact.platform_signing_identity.clone(),
            platform_timestamp_required: artifact.platform_timestamp_required,
            sigstore_certificate_identity: artifact.sigstore_certificate_identity.clone(),
            sigstore_oidc_issuer: artifact.sigstore_oidc_issuer.clone(),
            artifact_name: request.artifact_name.to_owned(),
            artifact_bytes: request.artifact_bytes.len() as u64,
            artifact_sha256: artifact.sha256.clone(),
            sigstore_name: request.sigstore_name.to_owned(),
            sigstore_bytes: request.sigstore_bytes.len() as u64,
            sigstore_sha256: artifact.sigstore_bundle_sha256.clone(),
            previous_installer_name: previous.as_ref().map(|entry| entry.0.clone()),
            previous_installer_version: previous.as_ref().map(|entry| entry.1.clone()),
            previous_installer_bytes: previous.as_ref().map(|entry| entry.2),
            previous_installer_sha256: previous.as_ref().map(|entry| entry.3.clone()),
            signed_release: signed_release.clone(),
        };
        if !record_matches_signed_release(&record) {
            return Err(UpdateStageError::Invalid);
        }
        write_json_new(&directory.join(RECORD_NAME), &record)?;
        fault_point("manifest-synced");
        std::fs::remove_file(directory.join(MARKER_NAME))?;
        write_transition(
            &directory,
            &stage_id,
            UpdateStageState::Prepared,
            0,
            None,
            None,
        )?;
        fault_point("prepared-synced");
        cleanup.keep();
        Ok(StagedUpdate {
            record,
            state: UpdateStageState::Prepared,
            running_version: None,
            rollback_reason: None,
        })
    }

    pub fn mark_installer_started(
        &self,
        stage_id: &str,
        now_unix: u64,
    ) -> Result<StagedUpdate, UpdateStageError> {
        self.transition(
            stage_id,
            UpdateStageState::InstallerStarted,
            now_unix,
            None,
            None,
        )
    }

    pub fn mark_awaiting_health(
        &self,
        stage_id: &str,
        now_unix: u64,
    ) -> Result<StagedUpdate, UpdateStageError> {
        self.transition(
            stage_id,
            UpdateStageState::AwaitingHealth,
            now_unix,
            None,
            None,
        )
    }

    pub fn mark_healthy(
        &self,
        stage_id: &str,
        running_version: &str,
        now_unix: u64,
    ) -> Result<StagedUpdate, UpdateStageError> {
        let current = self.load(stage_id)?;
        if current.record.version != running_version {
            return Err(UpdateStageError::Version);
        }
        self.transition(
            stage_id,
            UpdateStageState::Healthy,
            now_unix,
            Some(running_version.to_owned()),
            None,
        )
    }

    pub fn mark_rollback_required(
        &self,
        stage_id: &str,
        reason: RollbackReason,
        now_unix: u64,
    ) -> Result<StagedUpdate, UpdateStageError> {
        if matches!(
            reason,
            RollbackReason::RollbackInstallerFailed | RollbackReason::RollbackVersionMismatch
        ) {
            return Err(UpdateStageError::Field);
        }
        self.transition(
            stage_id,
            UpdateStageState::RollbackRequired,
            now_unix,
            None,
            Some(reason),
        )
    }

    pub fn rollback_installer(&self, stage_id: &str) -> Result<Option<PathBuf>, UpdateStageError> {
        let current = self.load(stage_id)?;
        if current.state != UpdateStageState::RollbackRequired {
            return Err(UpdateStageError::Transition);
        }
        Ok(current
            .record
            .previous_installer_name
            .map(|name| self.root.join(stage_id).join(name)))
    }

    pub fn prepare_rollback(
        &self,
        stage_id: &str,
        running_version: &str,
    ) -> Result<PreparedRollback, UpdateStageError> {
        let stage = self.load(stage_id)?;
        if stage.state != UpdateStageState::RollbackRequired
            || stage.record.version != running_version
        {
            return Err(UpdateStageError::Transition);
        }
        let installer_name = stage
            .record
            .previous_installer_name
            .as_deref()
            .ok_or(UpdateStageError::Invalid)?;
        Ok(PreparedRollback {
            installer_path: self.root.join(stage_id).join(installer_name),
            stage,
        })
    }

    pub fn mark_rollback_started(
        &self,
        stage_id: &str,
        rollback_version: &str,
        now_unix: u64,
    ) -> Result<StagedUpdate, UpdateStageError> {
        let current = self.load(stage_id)?;
        if current.record.previous_installer_version.as_deref() != Some(rollback_version) {
            return Err(UpdateStageError::Version);
        }
        self.transition(
            stage_id,
            UpdateStageState::RollbackStarted,
            now_unix,
            Some(rollback_version.to_owned()),
            None,
        )
    }

    pub fn mark_awaiting_rollback_health(
        &self,
        stage_id: &str,
        now_unix: u64,
    ) -> Result<StagedUpdate, UpdateStageError> {
        let current = self.load(stage_id)?;
        let rollback_version = current
            .running_version
            .clone()
            .ok_or(UpdateStageError::Invalid)?;
        self.transition(
            stage_id,
            UpdateStageState::AwaitingRollbackHealth,
            now_unix,
            Some(rollback_version),
            None,
        )
    }

    pub fn mark_rollback_failed(
        &self,
        stage_id: &str,
        reason: RollbackReason,
        now_unix: u64,
    ) -> Result<StagedUpdate, UpdateStageError> {
        if !matches!(
            reason,
            RollbackReason::RollbackInstallerFailed | RollbackReason::RollbackVersionMismatch
        ) {
            return Err(UpdateStageError::Field);
        }
        self.transition(
            stage_id,
            UpdateStageState::RollbackFailed,
            now_unix,
            None,
            Some(reason),
        )
    }

    pub fn mark_rolled_back(
        &self,
        stage_id: &str,
        running_version: &str,
        now_unix: u64,
    ) -> Result<StagedUpdate, UpdateStageError> {
        let current = self.load(stage_id)?;
        if current.record.previous_installer_version.as_deref() != Some(running_version) {
            return Err(UpdateStageError::Version);
        }
        self.transition(
            stage_id,
            UpdateStageState::RolledBack,
            now_unix,
            Some(running_version.to_owned()),
            None,
        )
    }

    pub fn load(&self, stage_id: &str) -> Result<StagedUpdate, UpdateStageError> {
        if !safe_stage_id(stage_id) {
            return Err(UpdateStageError::Field);
        }
        inspect_stage(&self.root.join(stage_id), stage_id)
    }

    /// Rebind one prepared durable stage to a candidate freshly verified from
    /// the exact signed envelope stored in that stage. The returned path is not
    /// sufficient evidence by itself; the platform adapter must still verify
    /// and lock it immediately before launch.
    pub fn prepare_installation(
        &self,
        stage_id: &str,
        trusted_root: &[u8; 32],
        context: &ReleaseCandidateContext<'_>,
    ) -> Result<PreparedInstallation, UpdateStageError> {
        let stage = self.load(stage_id)?;
        if stage.state != UpdateStageState::Prepared {
            return Err(UpdateStageError::Transition);
        }
        let candidate =
            verify_release_candidate(&stage.record.signed_release, trusted_root, context)?;
        if !record_matches_candidate(&stage.record, &candidate) {
            return Err(UpdateStageError::Invalid);
        }
        let artifact_path = self.root.join(stage_id).join(&stage.record.artifact_name);
        let previous_installer_path = stage
            .record
            .previous_installer_name
            .as_ref()
            .map(|name| self.root.join(stage_id).join(name));
        Ok(PreparedInstallation {
            stage,
            candidate,
            artifact_path,
            previous_installer_path,
        })
    }

    pub fn inventory(&self) -> Result<UpdateInventoryReport, UpdateStageError> {
        let mut report = UpdateInventoryReport::default();
        for entry in std::fs::read_dir(&self.root)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let metadata = entry.path().symlink_metadata()?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() || !safe_stage_id(&name) {
                report.invalid.push(name);
                continue;
            }
            let directory = entry.path();
            if directory.join(MARKER_NAME).exists()
                || !directory.join(RECORD_NAME).is_file()
                || !directory
                    .join(state_file_name(UpdateStageState::Prepared))
                    .is_file()
            {
                report.incomplete.push(name);
                continue;
            }
            match inspect_stage(&directory, &name) {
                Ok(stage) => report.verified.push(stage),
                Err(_) => report.invalid.push(name),
            }
        }
        report.verified.sort_by(|left, right| {
            left.record
                .stage_id
                .as_bytes()
                .cmp(right.record.stage_id.as_bytes())
        });
        report
            .incomplete
            .sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
        report
            .invalid
            .sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
        Ok(report)
    }

    /// Find the one package that previously completed exact-version startup
    /// health and reverify its historical release envelope under the compiled
    /// root. This is suitable as rollback material for the next update. It
    /// intentionally returns none for a clean installer bootstrap, which has no
    /// healthy update stage yet.
    pub fn installed_installer_source(
        &self,
        trusted_root: &[u8; 32],
        context: &InstalledReleaseContext<'_>,
    ) -> Result<Option<VerifiedInstallerSource>, UpdateStageError> {
        let inventory = self.inventory()?;
        if !inventory.incomplete.is_empty() || !inventory.invalid.is_empty() {
            return Err(UpdateStageError::Invalid);
        }
        let mut matching = inventory.verified.into_iter().filter(|stage| {
            stage.state == UpdateStageState::Healthy
                && stage.record.version == context.version
                && stage.record.sequence == context.sequence
                && stage.record.target == context.target
        });
        let Some(stage) = matching.next() else {
            return Ok(None);
        };
        if matching.next().is_some() {
            return Err(UpdateStageError::Invalid);
        }
        let installed =
            verify_installed_release(&stage.record.signed_release, trusted_root, context)?;
        if !record_matches_artifact(&stage.record, installed.artifact()) {
            return Err(UpdateStageError::Invalid);
        }
        Ok(Some(VerifiedInstallerSource {
            path: self
                .root
                .join(&stage.record.stage_id)
                .join(&stage.record.artifact_name),
            staged_name: stage.record.artifact_name,
        }))
    }

    /// Reconcile the one in-flight update after the trusted host core and UI
    /// have loaded. A prepared stage remains pending. A crash after installer
    /// start requires rollback; an awaiting-health stage becomes healthy only
    /// under the exact candidate version, otherwise rollback is required.
    pub fn reconcile_startup(
        &self,
        running_version: &str,
        now_unix: u64,
    ) -> Result<Option<StagedUpdate>, UpdateStageError> {
        if running_version.is_empty() || now_unix == 0 {
            return Err(UpdateStageError::Field);
        }
        let inventory = self.inventory()?;
        if !inventory.incomplete.is_empty() || !inventory.invalid.is_empty() {
            return Err(UpdateStageError::Invalid);
        }
        let mut active = inventory
            .verified
            .into_iter()
            .filter(|stage| {
                matches!(
                    stage.state,
                    UpdateStageState::Prepared
                        | UpdateStageState::InstallerStarted
                        | UpdateStageState::AwaitingHealth
                        | UpdateStageState::RollbackStarted
                        | UpdateStageState::AwaitingRollbackHealth
                )
            })
            .collect::<Vec<_>>();
        if active.len() > 1 {
            return Err(UpdateStageError::Invalid);
        }
        let Some(stage) = active.pop() else {
            return Ok(None);
        };
        match stage.state {
            UpdateStageState::Prepared => Ok(Some(stage)),
            UpdateStageState::InstallerStarted => self
                .mark_rollback_required(
                    &stage.record.stage_id,
                    RollbackReason::StartupFailed,
                    now_unix,
                )
                .map(Some),
            UpdateStageState::AwaitingHealth if stage.record.version == running_version => self
                .mark_healthy(&stage.record.stage_id, running_version, now_unix)
                .map(Some),
            UpdateStageState::AwaitingHealth => self
                .mark_rollback_required(
                    &stage.record.stage_id,
                    RollbackReason::VersionMismatch,
                    now_unix,
                )
                .map(Some),
            UpdateStageState::RollbackStarted | UpdateStageState::AwaitingRollbackHealth
                if stage.record.previous_installer_version.as_deref() == Some(running_version) =>
            {
                self.mark_rolled_back(&stage.record.stage_id, running_version, now_unix)
                    .map(Some)
            }
            UpdateStageState::RollbackStarted | UpdateStageState::AwaitingRollbackHealth => self
                .mark_rollback_failed(
                    &stage.record.stage_id,
                    RollbackReason::RollbackVersionMismatch,
                    now_unix,
                )
                .map(Some),
            UpdateStageState::Healthy
            | UpdateStageState::RollbackRequired
            | UpdateStageState::RolledBack
            | UpdateStageState::RollbackFailed => Ok(None),
        }
    }

    fn transition(
        &self,
        stage_id: &str,
        next: UpdateStageState,
        now_unix: u64,
        running_version: Option<String>,
        rollback_reason: Option<RollbackReason>,
    ) -> Result<StagedUpdate, UpdateStageError> {
        let current = self.load(stage_id)?;
        let allowed = matches!(
            (current.state, next),
            (
                UpdateStageState::Prepared,
                UpdateStageState::InstallerStarted
            ) | (
                UpdateStageState::InstallerStarted,
                UpdateStageState::AwaitingHealth
            ) | (
                UpdateStageState::InstallerStarted,
                UpdateStageState::RollbackRequired
            ) | (UpdateStageState::AwaitingHealth, UpdateStageState::Healthy)
                | (
                    UpdateStageState::AwaitingHealth,
                    UpdateStageState::RollbackRequired
                )
                | (
                    UpdateStageState::RollbackRequired,
                    UpdateStageState::RollbackStarted
                )
                | (
                    UpdateStageState::RollbackStarted,
                    UpdateStageState::AwaitingRollbackHealth
                )
                | (
                    UpdateStageState::RollbackStarted,
                    UpdateStageState::RolledBack
                )
                | (
                    UpdateStageState::RollbackStarted,
                    UpdateStageState::RollbackFailed
                )
                | (
                    UpdateStageState::AwaitingRollbackHealth,
                    UpdateStageState::RolledBack
                )
                | (
                    UpdateStageState::AwaitingRollbackHealth,
                    UpdateStageState::RollbackFailed
                )
        );
        if !allowed || now_unix == 0 {
            return Err(UpdateStageError::Transition);
        }
        write_transition(
            &self.root.join(stage_id),
            stage_id,
            next,
            now_unix,
            running_version,
            rollback_reason,
        )?;
        match next {
            UpdateStageState::InstallerStarted => fault_point("installer-started-synced"),
            UpdateStageState::AwaitingHealth => fault_point("awaiting-health-synced"),
            UpdateStageState::Healthy => fault_point("healthy-synced"),
            UpdateStageState::RollbackRequired => fault_point("rollback-required-synced"),
            UpdateStageState::RollbackStarted => fault_point("rollback-started-synced"),
            UpdateStageState::AwaitingRollbackHealth => {
                fault_point("awaiting-rollback-health-synced")
            }
            UpdateStageState::RolledBack => fault_point("rolled-back-synced"),
            UpdateStageState::RollbackFailed => fault_point("rollback-failed-synced"),
            UpdateStageState::Prepared => {}
        }
        self.load(stage_id)
    }
}

fn inspect_stage(directory: &Path, stage_id: &str) -> Result<StagedUpdate, UpdateStageError> {
    if directory.join(MARKER_NAME).exists() {
        return Err(UpdateStageError::Incomplete);
    }
    let record: UpdateStageRecord = read_json_regular(&directory.join(RECORD_NAME))?;
    if record.profile != UPDATE_STAGE_PROFILE
        || record.stage_id != stage_id
        || record.sequence == 0
        || validate_platform_signing_identity(
            &record.platform_signing,
            &record.platform_signing_identity,
        )
        .is_err()
        || (matches!(
            record.platform_signing.as_str(),
            "authenticode" | "developer-id-notarized"
        ) && !record.platform_timestamp_required)
        || !valid_sigstore_identity(&record.sigstore_certificate_identity)
        || !valid_sigstore_issuer(&record.sigstore_oidc_issuer)
        || !safe_name(&record.artifact_name)
        || !safe_name(&record.sigstore_name)
        || record.artifact_name == record.sigstore_name
        || record
            .previous_installer_name
            .as_ref()
            .is_some_and(|name| name == &record.artifact_name || name == &record.sigstore_name)
        || !valid_optional_previous(&record)
        || !record_matches_signed_release(&record)
    {
        return Err(UpdateStageError::Invalid);
    }
    verify_file(
        &directory.join(&record.artifact_name),
        record.artifact_bytes,
        &record.artifact_sha256,
    )?;
    verify_file(
        &directory.join(&record.sigstore_name),
        record.sigstore_bytes,
        &record.sigstore_sha256,
    )?;
    if let (Some(name), Some(bytes), Some(sha256)) = (
        record.previous_installer_name.as_deref(),
        record.previous_installer_bytes,
        record.previous_installer_sha256.as_deref(),
    ) {
        verify_file(&directory.join(name), bytes, sha256)?;
    }

    let mut transitions = Vec::new();
    let allowed_names = [
        record.artifact_name.as_str(),
        record.sigstore_name.as_str(),
        RECORD_NAME,
    ]
    .into_iter()
    .chain(record.previous_installer_name.as_deref())
    .collect::<BTreeSet<_>>();
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if allowed_names.contains(name.as_str()) {
            continue;
        }
        if !name.starts_with("state-") || !name.ends_with(".json") {
            return Err(UpdateStageError::Invalid);
        }
        let transition: TransitionRecord = read_json_regular(&entry.path())?;
        if transition.profile != UPDATE_STAGE_PROFILE
            || transition.stage_id != stage_id
            || name != state_file_name(transition.state)
            || transition.recorded_at_unix == 0 && transition.state != UpdateStageState::Prepared
            || transition.running_version.is_some()
                != matches!(
                    transition.state,
                    UpdateStageState::Healthy
                        | UpdateStageState::RollbackStarted
                        | UpdateStageState::AwaitingRollbackHealth
                        | UpdateStageState::RolledBack
                )
            || transition.state == UpdateStageState::Healthy
                && transition.running_version.as_deref() != Some(record.version.as_str())
            || matches!(
                transition.state,
                UpdateStageState::RollbackStarted
                    | UpdateStageState::AwaitingRollbackHealth
                    | UpdateStageState::RolledBack
            ) && transition.running_version.as_deref()
                != record.previous_installer_version.as_deref()
            || transition.rollback_reason.is_some()
                != matches!(
                    transition.state,
                    UpdateStageState::RollbackRequired | UpdateStageState::RollbackFailed
                )
            || transition.state == UpdateStageState::RollbackFailed
                && !matches!(
                    transition.rollback_reason,
                    Some(
                        RollbackReason::RollbackInstallerFailed
                            | RollbackReason::RollbackVersionMismatch
                    )
                )
            || transition.state == UpdateStageState::RollbackRequired
                && matches!(
                    transition.rollback_reason,
                    Some(
                        RollbackReason::RollbackInstallerFailed
                            | RollbackReason::RollbackVersionMismatch
                    )
                )
        {
            return Err(UpdateStageError::Invalid);
        }
        transitions.push(transition);
    }
    transitions.sort_by_key(|entry| state_code(entry.state));
    if transitions.is_empty() || transitions[0].state != UpdateStageState::Prepared {
        return Err(UpdateStageError::Incomplete);
    }
    let mut current = UpdateStageState::Prepared;
    for transition in transitions.iter().skip(1) {
        let valid = matches!(
            (current, transition.state),
            (
                UpdateStageState::Prepared,
                UpdateStageState::InstallerStarted
            ) | (
                UpdateStageState::InstallerStarted,
                UpdateStageState::AwaitingHealth
            ) | (
                UpdateStageState::InstallerStarted,
                UpdateStageState::RollbackRequired
            ) | (UpdateStageState::AwaitingHealth, UpdateStageState::Healthy)
                | (
                    UpdateStageState::AwaitingHealth,
                    UpdateStageState::RollbackRequired
                )
                | (
                    UpdateStageState::RollbackRequired,
                    UpdateStageState::RollbackStarted
                )
                | (
                    UpdateStageState::RollbackStarted,
                    UpdateStageState::AwaitingRollbackHealth
                )
                | (
                    UpdateStageState::RollbackStarted,
                    UpdateStageState::RolledBack
                )
                | (
                    UpdateStageState::RollbackStarted,
                    UpdateStageState::RollbackFailed
                )
                | (
                    UpdateStageState::AwaitingRollbackHealth,
                    UpdateStageState::RolledBack
                )
                | (
                    UpdateStageState::AwaitingRollbackHealth,
                    UpdateStageState::RollbackFailed
                )
        );
        if !valid {
            return Err(UpdateStageError::Invalid);
        }
        current = transition.state;
    }
    Ok(StagedUpdate {
        record,
        state: current,
        running_version: transitions
            .last()
            .and_then(|entry| entry.running_version.clone()),
        rollback_reason: transitions.last().and_then(|entry| entry.rollback_reason),
    })
}

fn record_matches_signed_release(record: &UpdateStageRecord) -> bool {
    let signed = &record.signed_release;
    if signed.manifest.profile != RELEASE_PROFILE
        || signed.manifest.version != record.version
        || signed.manifest.sequence != record.sequence
        || !signed.signing_key_id.starts_with("ed25519:sha256:")
        || !valid_sha256(
            signed
                .signing_key_id
                .strip_prefix("ed25519:sha256:")
                .unwrap_or_default(),
        )
        || signed.signature_hex.len() != 128
        || !signed
            .signature_hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return false;
    }
    let matching = signed
        .manifest
        .artifacts
        .iter()
        .filter(|artifact| artifact.target == record.target)
        .collect::<Vec<_>>();
    matching.len() == 1 && record_matches_artifact(record, matching[0])
}

fn record_matches_candidate(
    record: &UpdateStageRecord,
    candidate: &VerifiedReleaseCandidate,
) -> bool {
    record.version == candidate.version()
        && record.sequence == candidate.sequence()
        && &record.signed_release == candidate.signed_release()
        && record_matches_artifact(record, candidate.artifact())
}

fn record_matches_artifact(record: &UpdateStageRecord, artifact: &ReleaseArtifact) -> bool {
    record.target == artifact.target
        && record.artifact_bytes == artifact.bytes
        && record.artifact_sha256 == artifact.sha256
        && record.sigstore_sha256 == artifact.sigstore_bundle_sha256
        && record.platform_signing == artifact.platform_signing
        && record.platform_signing_identity == artifact.platform_signing_identity
        && record.platform_timestamp_required == artifact.platform_timestamp_required
        && record.sigstore_certificate_identity == artifact.sigstore_certificate_identity
        && record.sigstore_oidc_issuer == artifact.sigstore_oidc_issuer
        && artifact_name_matches_policy(
            &record.artifact_name,
            &artifact.url,
            &artifact.platform_signing,
        )
}

fn artifact_name_matches_policy(name: &str, url: &str, platform_signing: &str) -> bool {
    let suffix = match platform_signing {
        "authenticode" if url.ends_with(".msi") => ".msi",
        "authenticode" if url.ends_with(".exe") => ".exe",
        "developer-id-notarized" if url.ends_with(".pkg") => ".pkg",
        "linux-detached" if url.ends_with(".deb") => ".deb",
        "linux-detached" if url.ends_with(".rpm") => ".rpm",
        "linux-detached" if url.ends_with(".AppImage") => ".AppImage",
        _ => return false,
    };
    name.ends_with(suffix)
}

fn valid_sigstore_identity(value: &str) -> bool {
    !value.is_empty() && value.len() <= 2_048 && value.bytes().all(|byte| byte.is_ascii_graphic())
}

fn valid_sigstore_issuer(value: &str) -> bool {
    let remainder = match value.strip_prefix("https://") {
        Some(remainder) => remainder,
        None => return false,
    };
    let authority_end = remainder.find('/').unwrap_or(remainder.len());
    let authority = &remainder[..authority_end];
    !value.is_empty()
        && value.len() <= 2_048
        && !value.contains(['?', '#'])
        && !authority.is_empty()
        && !authority.contains(['@', ':'])
        && authority.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-')
        })
}

fn copy_previous_installer(
    directory: &Path,
    installer: PreviousInstaller<'_>,
) -> Result<(String, String, u64, String), UpdateStageError> {
    if !safe_name(installer.staged_name) || !valid_version(installer.version) {
        return Err(UpdateStageError::Field);
    }
    let metadata = installer.path.symlink_metadata()?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() == 0
        || metadata.len() > MAX_FILE_BYTES
    {
        return Err(UpdateStageError::Invalid);
    }
    let output_path = directory.join(installer.staged_name);
    let mut source = File::open(installer.path)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output_path)?;
    let mut digest = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = source.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        output.write_all(&buffer[..read])?;
        digest.update(&buffer[..read]);
        bytes += read as u64;
    }
    output.sync_all()?;
    protect_private_file(&output_path)?;
    Ok((
        installer.staged_name.to_owned(),
        installer.version.to_owned(),
        bytes,
        lower_hex(&digest.finalize()),
    ))
}

fn write_transition(
    directory: &Path,
    stage_id: &str,
    state: UpdateStageState,
    recorded_at_unix: u64,
    running_version: Option<String>,
    rollback_reason: Option<RollbackReason>,
) -> Result<(), UpdateStageError> {
    write_json_new(
        &directory.join(state_file_name(state)),
        &TransitionRecord {
            profile: UPDATE_STAGE_PROFILE.to_owned(),
            stage_id: stage_id.to_owned(),
            state,
            recorded_at_unix,
            running_version,
            rollback_reason,
        },
    )
}

fn state_code(state: UpdateStageState) -> u8 {
    match state {
        UpdateStageState::Prepared => 0,
        UpdateStageState::InstallerStarted => 10,
        UpdateStageState::AwaitingHealth => 20,
        UpdateStageState::Healthy | UpdateStageState::RollbackRequired => 30,
        UpdateStageState::RollbackStarted => 40,
        UpdateStageState::AwaitingRollbackHealth => 50,
        UpdateStageState::RolledBack | UpdateStageState::RollbackFailed => 60,
    }
}

fn state_file_name(state: UpdateStageState) -> &'static str {
    match state {
        UpdateStageState::Prepared => "state-000-prepared.json",
        UpdateStageState::InstallerStarted => "state-010-installer-started.json",
        UpdateStageState::AwaitingHealth => "state-020-awaiting-health.json",
        UpdateStageState::Healthy => "state-030-healthy.json",
        UpdateStageState::RollbackRequired => "state-030-rollback-required.json",
        UpdateStageState::RollbackStarted => "state-040-rollback-started.json",
        UpdateStageState::AwaitingRollbackHealth => "state-050-awaiting-rollback-health.json",
        UpdateStageState::RolledBack => "state-060-rolled-back.json",
        UpdateStageState::RollbackFailed => "state-060-rollback-failed.json",
    }
}

fn write_json_new<T: Serialize>(path: &Path, value: &T) -> Result<(), UpdateStageError> {
    let file = OpenOptions::new().write(true).create_new(true).open(path)?;
    serde_json::to_writer_pretty(&file, value)?;
    file.sync_all()?;
    protect_private_file(path)?;
    Ok(())
}

fn write_bytes_new(path: &Path, bytes: &[u8]) -> Result<(), UpdateStageError> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    protect_private_file(path)?;
    Ok(())
}

fn read_json_regular<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, UpdateStageError> {
    let metadata = path.symlink_metadata()?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > 1024 * 1024 {
        return Err(UpdateStageError::Invalid);
    }
    Ok(serde_json::from_reader(File::open(path)?)?)
}

fn verify_file(
    path: &Path,
    expected_bytes: u64,
    expected_sha256: &str,
) -> Result<(), UpdateStageError> {
    let metadata = path.symlink_metadata()?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() != expected_bytes
        || expected_bytes > MAX_FILE_BYTES
        || !valid_sha256(expected_sha256)
        || hash_file(path)? != expected_sha256
    {
        return Err(UpdateStageError::Invalid);
    }
    Ok(())
}

fn hash_file(path: &Path) -> Result<String, UpdateStageError> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(lower_hex(&digest.finalize()))
}

fn valid_optional_previous(record: &UpdateStageRecord) -> bool {
    match (
        record.previous_installer_name.as_deref(),
        record.previous_installer_version.as_deref(),
        record.previous_installer_bytes,
        record.previous_installer_sha256.as_deref(),
    ) {
        (None, None, None, None) => true,
        (Some(name), Some(version), Some(bytes), Some(sha256)) => {
            safe_name(name)
                && valid_version(version)
                && bytes > 0
                && bytes <= MAX_FILE_BYTES
                && valid_sha256(sha256)
        }
        _ => false,
    }
}

fn valid_version(value: &str) -> bool {
    let parts = value.split('.').collect::<Vec<_>>();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && (part.len() == 1 || !part.starts_with('0'))
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && part.parse::<u16>().is_ok()
        })
}

fn safe_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 160
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn safe_stage_id(value: &str) -> bool {
    safe_name(value) && value.len() <= 220
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

struct StageCleanup {
    directory: PathBuf,
    keep: bool,
}

impl StageCleanup {
    fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            keep: false,
        }
    }

    fn keep(&mut self) {
        self.keep = true;
    }
}

impl Drop for StageCleanup {
    fn drop(&mut self) {
        if !self.keep {
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }
}

#[cfg(test)]
fn fault_point(point: &str) {
    if std::env::var("SQLITE_CAPSULE_UPDATE_FAULT_STAGE").as_deref() == Ok(point) {
        std::process::exit(97);
    }
}

#[cfg(not(test))]
fn fault_point(_point: &str) {}

#[cfg(test)]
mod tests {
    use std::{
        process::Command,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use sqlite_capsule_distribution::{
        ReleaseArtifact, ReleaseManifest, SignedReleaseManifest, key_id,
    };

    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    const ARTIFACT: &[u8] = b"verified installer bytes";
    const SIGSTORE: &[u8] = b"verified sigstore bundle";

    fn temp_dir(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "sqlite-capsule-update-{label}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).expect("create test directory");
        path
    }

    fn sha256(bytes: &[u8]) -> String {
        lower_hex(&Sha256::digest(bytes))
    }

    fn artifact() -> ReleaseArtifact {
        ReleaseArtifact {
            target: "x86_64-pc-windows-msvc".to_owned(),
            url: "https://updates.example.com/host.msi".to_owned(),
            bytes: ARTIFACT.len() as u64,
            sha256: sha256(ARTIFACT),
            sigstore_bundle_sha256: sha256(SIGSTORE),
            platform_signing: "authenticode".to_owned(),
            platform_signing_identity: format!(
                "authenticode-certificate-sha256:{}",
                "ab".repeat(32)
            ),
            platform_timestamp_required: true,
            sigstore_certificate_identity:
                "https://github.com/sqlite-capsule/sqlite-capsule/.github/workflows/release.yml@refs/tags/v0.3.0"
                    .to_owned(),
            sigstore_oidc_issuer: "https://token.actions.githubusercontent.com".to_owned(),
        }
    }

    fn update() -> VerifiedUpdate {
        VerifiedUpdate {
            version: "0.3.0".to_owned(),
            sequence: 2,
            artifact: artifact(),
        }
    }

    fn root() -> SigningKey {
        SigningKey::from_bytes(&[41_u8; 32])
    }

    fn signed_release() -> SignedReleaseManifest {
        const RELEASE_CONTEXT: &[u8] = b"SQLite Capsule host release manifest v2\0";
        let manifest = ReleaseManifest {
            profile: RELEASE_PROFILE.to_owned(),
            sequence: 2,
            version: "0.3.0".to_owned(),
            issued_at: "2026-01-01T00:00:00Z".to_owned(),
            expires_at: "2030-01-01T00:00:00Z".to_owned(),
            artifacts: vec![artifact()],
        };
        let canonical = serde_json_canonicalizer::to_vec(&manifest).expect("canonical release");
        let mut message = RELEASE_CONTEXT.to_vec();
        message.extend_from_slice(&canonical);
        let root = root();
        SignedReleaseManifest {
            manifest,
            signing_key_id: key_id(&root.verifying_key().to_bytes()),
            signature_hex: lower_hex(&root.sign(&message).to_bytes()),
        }
    }

    fn candidate_context<'a>() -> ReleaseCandidateContext<'a> {
        ReleaseCandidateContext {
            current_version: "0.2.0",
            current_sequence: 1,
            target: "x86_64-pc-windows-msvc",
            allowed_hosts: &["updates.example.com"],
            now_unix: 1_800_000_000,
        }
    }

    fn request<'a>(previous: Option<PreviousInstaller<'a>>) -> StageRequest<'a> {
        StageRequest {
            artifact_name: "host-0.3.0.msi",
            artifact_bytes: ARTIFACT,
            sigstore_name: "host-0.3.0.sigstore.json",
            sigstore_bytes: SIGSTORE,
            previous_installer: previous,
        }
    }

    #[test]
    fn stages_verified_bytes_preserves_previous_and_records_health() {
        let directory = temp_dir("healthy");
        let previous = directory.join("previous.msi");
        std::fs::write(&previous, b"previous signed installer").expect("write previous");
        let stager = UpdateStager::open(&directory.join("staging")).expect("open stager");
        let staged = stager
            .stage_test_verified(
                &update(),
                &signed_release(),
                request(Some(PreviousInstaller {
                    path: &previous,
                    staged_name: "previous-installer.msi",
                    version: "0.2.0",
                })),
            )
            .expect("stage update");
        assert_eq!(staged.state, UpdateStageState::Prepared);
        assert!(staged.record.previous_installer_sha256.is_some());
        let started = stager
            .mark_installer_started(&staged.record.stage_id, 1)
            .expect("mark installer started");
        assert_eq!(started.state, UpdateStageState::InstallerStarted);
        let awaiting = stager
            .mark_awaiting_health(&staged.record.stage_id, 2)
            .expect("mark awaiting health");
        assert_eq!(awaiting.state, UpdateStageState::AwaitingHealth);
        assert!(matches!(
            stager.mark_healthy(&staged.record.stage_id, "0.2.0", 3),
            Err(UpdateStageError::Version)
        ));
        let healthy = stager
            .reconcile_startup("0.3.0", 3)
            .expect("reconcile startup")
            .expect("active update");
        assert_eq!(healthy.state, UpdateStageState::Healthy);
        assert!(matches!(
            stager.mark_rollback_required(
                &staged.record.stage_id,
                RollbackReason::OperatorRequested,
                4
            ),
            Err(UpdateStageError::Transition)
        ));
        let inventory = stager.inventory().expect("inventory");
        assert_eq!(inventory.verified, vec![healthy]);
        assert!(inventory.incomplete.is_empty());
        assert!(inventory.invalid.is_empty());
        let hosts = ["updates.example.com"];
        let source = stager
            .installed_installer_source(
                &root().verifying_key().to_bytes(),
                &InstalledReleaseContext {
                    version: "0.3.0",
                    sequence: 2,
                    target: "x86_64-pc-windows-msvc",
                    allowed_hosts: &hosts,
                },
            )
            .expect("discover healthy installer")
            .expect("healthy installer source");
        assert_eq!(source.staged_name(), "host-0.3.0.msi");
        assert_eq!(
            std::fs::read(source.path()).expect("read healthy installer"),
            ARTIFACT
        );
        assert!(
            stager
                .installed_installer_source(
                    &root().verifying_key().to_bytes(),
                    &InstalledReleaseContext {
                        version: "0.2.0",
                        sequence: 1,
                        target: "x86_64-pc-windows-msvc",
                        allowed_hosts: &hosts,
                    },
                )
                .expect("no bootstrap installer")
                .is_none()
        );
        std::fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[test]
    fn rollback_exposes_only_the_preserved_exact_installer() {
        let directory = temp_dir("rollback");
        let previous = directory.join("previous.msi");
        std::fs::write(&previous, b"previous signed installer").expect("write previous");
        let stager = UpdateStager::open(&directory.join("staging")).expect("open stager");
        let staged = stager
            .stage_test_verified(
                &update(),
                &signed_release(),
                request(Some(PreviousInstaller {
                    path: &previous,
                    staged_name: "previous-installer.msi",
                    version: "0.2.0",
                })),
            )
            .expect("stage update");
        stager
            .mark_installer_started(&staged.record.stage_id, 1)
            .expect("mark started");
        let rollback = stager
            .reconcile_startup("0.2.0", 2)
            .expect("reconcile interrupted installer")
            .expect("active update");
        assert_eq!(rollback.state, UpdateStageState::RollbackRequired);
        let installer = stager
            .rollback_installer(&staged.record.stage_id)
            .expect("rollback installer")
            .expect("preserved installer");
        assert_eq!(
            std::fs::read(&installer).expect("read installer"),
            b"previous signed installer"
        );
        let prepared = stager
            .prepare_rollback(&staged.record.stage_id, "0.3.0")
            .expect("prepare rollback");
        assert_eq!(prepared.installer_path(), installer);
        let started = stager
            .mark_rollback_started(&staged.record.stage_id, "0.2.0", 3)
            .expect("mark rollback started");
        assert_eq!(started.state, UpdateStageState::RollbackStarted);
        assert_eq!(started.running_version.as_deref(), Some("0.2.0"));
        let awaiting = stager
            .mark_awaiting_rollback_health(&staged.record.stage_id, 4)
            .expect("mark awaiting rollback health");
        assert_eq!(awaiting.state, UpdateStageState::AwaitingRollbackHealth);
        let complete = stager
            .reconcile_startup("0.2.0", 5)
            .expect("reconcile rollback startup")
            .expect("active rollback");
        assert_eq!(complete.state, UpdateStageState::RolledBack);
        assert_eq!(complete.running_version.as_deref(), Some("0.2.0"));
        std::fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[test]
    fn rollback_health_mismatch_is_terminal_and_preserves_reason() {
        let directory = temp_dir("rollback-mismatch");
        let previous = directory.join("previous.msi");
        std::fs::write(&previous, b"previous signed installer").expect("write previous");
        let stager = UpdateStager::open(&directory.join("staging")).expect("open stager");
        let staged = stager
            .stage_test_verified(
                &update(),
                &signed_release(),
                request(Some(PreviousInstaller {
                    path: &previous,
                    staged_name: "previous-installer.msi",
                    version: "0.2.0",
                })),
            )
            .expect("stage update");
        stager
            .mark_installer_started(&staged.record.stage_id, 1)
            .expect("mark started");
        stager
            .mark_rollback_required(&staged.record.stage_id, RollbackReason::InstallerFailed, 2)
            .expect("require rollback");
        stager
            .mark_rollback_started(&staged.record.stage_id, "0.2.0", 3)
            .expect("mark rollback started");
        stager
            .mark_awaiting_rollback_health(&staged.record.stage_id, 4)
            .expect("mark awaiting rollback health");
        let failed = stager
            .reconcile_startup("0.3.0", 5)
            .expect("reconcile mismatched rollback startup")
            .expect("active rollback");
        assert_eq!(failed.state, UpdateStageState::RollbackFailed);
        assert_eq!(
            failed.rollback_reason,
            Some(RollbackReason::RollbackVersionMismatch)
        );
        assert!(
            stager
                .prepare_rollback(&staged.record.stage_id, "0.3.0")
                .is_err()
        );
        std::fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[test]
    fn tamper_and_unverified_input_fail_closed() {
        let directory = temp_dir("tamper");
        let stager = UpdateStager::open(&directory.join("staging")).expect("open stager");
        let mut bad = update();
        bad.artifact.sha256 = "0".repeat(64);
        assert!(matches!(
            stager.stage_test_verified(&bad, &signed_release(), request(None)),
            Err(UpdateStageError::Distribution(DistributionError::Artifact))
        ));
        let staged = stager
            .stage_test_verified(&update(), &signed_release(), request(None))
            .expect("stage update");
        let artifact = stager
            .root()
            .join(&staged.record.stage_id)
            .join(&staged.record.artifact_name);
        std::fs::write(artifact, b"tampered").expect("tamper artifact");
        let inventory = stager.inventory().expect("inventory");
        assert_eq!(inventory.invalid, vec![staged.record.stage_id]);
        std::fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[test]
    fn prepared_installation_reverifies_the_persisted_release_envelope() {
        let directory = temp_dir("persisted-release");
        let stager = UpdateStager::open(&directory.join("staging")).expect("open stager");
        let signed = signed_release();
        let staged = stager
            .stage_test_verified(&update(), &signed, request(None))
            .expect("stage update");
        let verifying_key = root().verifying_key().to_bytes();
        let context = candidate_context();
        let prepared = stager
            .prepare_installation(&staged.record.stage_id, &verifying_key, &context)
            .expect("reverify persisted release");
        assert_eq!(prepared.stage().record, staged.record);
        assert_eq!(prepared.candidate().signed_release(), &signed);
        assert_eq!(
            prepared.artifact_path(),
            stager
                .root()
                .join(&staged.record.stage_id)
                .join(&staged.record.artifact_name)
        );
        drop(prepared);

        let record_path = stager
            .root()
            .join(&staged.record.stage_id)
            .join(RECORD_NAME);
        let mut record: UpdateStageRecord =
            serde_json::from_reader(File::open(&record_path).expect("open stage record"))
                .expect("read stage record");
        let replacement = if record.signed_release.signature_hex.starts_with('0') {
            "1"
        } else {
            "0"
        };
        record
            .signed_release
            .signature_hex
            .replace_range(0..1, replacement);
        let file = File::create(&record_path).expect("rewrite tampered stage record");
        serde_json::to_writer_pretty(&file, &record).expect("write tampered stage record");
        file.sync_all().expect("sync tampered stage record");
        assert!(matches!(
            stager.prepare_installation(&staged.record.stage_id, &verifying_key, &context),
            Err(UpdateStageError::Distribution(DistributionError::Signature))
        ));
        std::fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[test]
    fn update_fault_crash_worker() {
        let Some(stage) = std::env::var_os("SQLITE_CAPSULE_UPDATE_FAULT_STAGE") else {
            return;
        };
        let root = PathBuf::from(
            std::env::var_os("SQLITE_CAPSULE_UPDATE_FAULT_ROOT").expect("fault root"),
        );
        let stager = UpdateStager::open(&root).expect("open fault stager");
        let previous = root
            .parent()
            .expect("fault root parent")
            .join("previous-installer.msi");
        std::fs::write(&previous, b"previous signed installer").expect("write fault previous");
        let staged = stager
            .stage_test_verified(
                &update(),
                &signed_release(),
                request(Some(PreviousInstaller {
                    path: &previous,
                    staged_name: "previous-installer.msi",
                    version: "0.2.0",
                })),
            )
            .expect("stage fault update");
        match stage.to_string_lossy().as_ref() {
            "artifact-synced" | "manifest-synced" | "prepared-synced" => {}
            "installer-started-synced" => {
                stager
                    .mark_installer_started(&staged.record.stage_id, 1)
                    .expect("fault installer started");
            }
            "awaiting-health-synced" => {
                stager
                    .mark_installer_started(&staged.record.stage_id, 1)
                    .expect("fault installer started");
                stager
                    .mark_awaiting_health(&staged.record.stage_id, 2)
                    .expect("fault awaiting health");
            }
            "rollback-required-synced"
            | "rollback-started-synced"
            | "awaiting-rollback-health-synced"
            | "rolled-back-synced"
            | "rollback-failed-synced" => {
                stager
                    .mark_installer_started(&staged.record.stage_id, 1)
                    .expect("fault installer started");
                stager
                    .mark_rollback_required(
                        &staged.record.stage_id,
                        RollbackReason::InstallerFailed,
                        2,
                    )
                    .expect("fault rollback required");
                if stage != "rollback-required-synced" {
                    stager
                        .mark_rollback_started(&staged.record.stage_id, "0.2.0", 3)
                        .expect("fault rollback started");
                }
                if matches!(
                    stage.to_string_lossy().as_ref(),
                    "awaiting-rollback-health-synced" | "rolled-back-synced"
                ) {
                    stager
                        .mark_awaiting_rollback_health(&staged.record.stage_id, 4)
                        .expect("fault awaiting rollback health");
                }
                if stage == "rolled-back-synced" {
                    stager
                        .mark_rolled_back(&staged.record.stage_id, "0.2.0", 5)
                        .expect("fault rolled back");
                }
                if stage == "rollback-failed-synced" {
                    stager
                        .mark_rollback_failed(
                            &staged.record.stage_id,
                            RollbackReason::RollbackInstallerFailed,
                            4,
                        )
                        .expect("fault rollback failed");
                }
            }
            value => panic!("unexpected fault stage {value}"),
        }
        panic!("fault point did not terminate worker");
    }

    #[test]
    fn process_exit_inventory_distinguishes_incomplete_and_recoverable_stages() {
        let cases = [
            ("artifact-synced", None),
            ("manifest-synced", None),
            ("prepared-synced", Some(UpdateStageState::Prepared)),
            (
                "installer-started-synced",
                Some(UpdateStageState::InstallerStarted),
            ),
            (
                "awaiting-health-synced",
                Some(UpdateStageState::AwaitingHealth),
            ),
            (
                "rollback-required-synced",
                Some(UpdateStageState::RollbackRequired),
            ),
            (
                "rollback-started-synced",
                Some(UpdateStageState::RollbackStarted),
            ),
            (
                "awaiting-rollback-health-synced",
                Some(UpdateStageState::AwaitingRollbackHealth),
            ),
            ("rolled-back-synced", Some(UpdateStageState::RolledBack)),
            (
                "rollback-failed-synced",
                Some(UpdateStageState::RollbackFailed),
            ),
        ];
        for (stage, expected_state) in cases {
            let directory = temp_dir(stage);
            let root = directory.join("staging");
            let status = Command::new(std::env::current_exe().expect("current test binary"))
                .arg("tests::update_fault_crash_worker")
                .arg("--exact")
                .arg("--nocapture")
                .env("SQLITE_CAPSULE_UPDATE_FAULT_STAGE", stage)
                .env("SQLITE_CAPSULE_UPDATE_FAULT_ROOT", &root)
                .status()
                .expect("run fault worker");
            assert_eq!(status.code(), Some(97), "fault stage {stage}");
            let stager = UpdateStager::open(&root).expect("reopen stager");
            let inventory = stager.inventory().expect("fault inventory");
            if let Some(expected_state) = expected_state {
                assert_eq!(inventory.verified.len(), 1, "fault stage {stage}");
                assert_eq!(inventory.verified[0].state, expected_state);
                assert!(inventory.incomplete.is_empty());
            } else {
                assert!(inventory.verified.is_empty());
                assert_eq!(inventory.incomplete.len(), 1, "fault stage {stage}");
            }
            assert!(inventory.invalid.is_empty());
            std::fs::remove_dir_all(directory).expect("remove fault directory");
        }
    }
}

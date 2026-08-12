//! Generic verified native runtime for SQLite Capsule assets and named endpoints.
//!
//! No public method accepts SQL, a database handle, an arbitrary filesystem
//! path, or a trust mutation. Construction rechecks launch evidence and an
//! executable policy decision before opening a runtime connection.

mod conformance;
mod endpoint;

use std::path::Path;
use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, OpenFlags, backup::Backup, limits::Limit};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlite_capsule_core::{InspectError, inspect_header};
use sqlite_capsule_launch::{LaunchInspection, inspect_launch};
use sqlite_capsule_lifecycle::{
    LifecycleError, PinnedSource, SourceIdentity, WriterLease, prepare_private_directory,
    protect_private_file,
};
use sqlite_capsule_policy::{CapabilityDecision, LaunchDecision};
use thiserror::Error;

pub const MAX_ASSET_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_RESULT_ROWS: usize = 1_000;
pub const MAX_RESULT_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_ENDPOINT_STEPS: usize = 16;
pub const ENDPOINT_TIMEOUT_SECONDS: u64 = 3;
pub const MAX_FOREIGN_KEY_CASCADE_DEPTH: i32 = 32;
const MAX_WRITES_BETWEEN_BACKUPS: u32 = 10;

#[derive(Clone, Copy)]
enum BackupPurpose {
    Prewrite,
    Checkpoint,
    Close,
    Update,
}

#[derive(Clone, Copy)]
enum BackupFaultPoint {
    MarkerSynced,
    DatabaseCopied,
    ManifestSynced,
}

#[derive(Clone, Copy)]
enum RestoreFaultPoint {
    MarkerSynced,
    DatabaseCopied,
    Verified,
}

#[derive(Clone, Copy)]
enum OpenFaultPoint {
    SourcePinned,
    SqliteOpened,
    Verified,
}

#[cfg(test)]
fn requested_runtime_fault_stage() -> Option<String> {
    std::env::var("SQLITE_CAPSULE_RUNTIME_FAULT_STAGE").ok()
}

#[cfg(any(test, all(feature = "debug-fault-injection", debug_assertions)))]
fn guarded_debug_fault_stage(
    enabled: Option<&str>,
    state_root: Option<&Path>,
    parent_port: Option<u16>,
    raw_port: Option<u16>,
    stage: Option<String>,
) -> Option<String> {
    if enabled != Some("enabled") {
        return None;
    }
    let state_root = state_root?;
    if !state_root.is_absolute() || !state_root.is_dir() {
        return None;
    }
    let parent_port = parent_port.filter(|port| *port != 0)?;
    let raw_port = raw_port.filter(|port| *port != 0)?;
    if parent_port == raw_port {
        return None;
    }
    let stage = stage?;
    matches!(
        stage.as_str(),
        "open.source-pinned"
            | "open.sqlite-opened"
            | "open.verified"
            | "prewrite.marker-synced"
            | "prewrite.database-copied"
            | "prewrite.manifest-synced"
            | "checkpoint.marker-synced"
            | "checkpoint.database-copied"
            | "checkpoint.manifest-synced"
            | "close.marker-synced"
            | "close.database-copied"
            | "close.manifest-synced"
            | "update.marker-synced"
            | "update.database-copied"
            | "update.manifest-synced"
            | "restore.marker-synced"
            | "restore.database-copied"
            | "restore.verified"
    )
    .then_some(stage)
}

#[cfg(all(not(test), feature = "debug-fault-injection", debug_assertions))]
fn requested_runtime_fault_stage() -> Option<String> {
    let enabled = std::env::var("SQLITE_CAPSULE_NATIVE_E2E_RUNTIME_FAULTS").ok();
    let state_root = std::env::var_os("SQLITE_CAPSULE_NATIVE_E2E_STATE_ROOT").map(PathBuf::from);
    let parent_port = std::env::var("SQLITE_CAPSULE_NATIVE_PARENT_E2E_PORT")
        .ok()?
        .parse::<u16>()
        .ok();
    let raw_port = std::env::var("SQLITE_CAPSULE_NATIVE_RAW_E2E_PORT")
        .ok()?
        .parse::<u16>()
        .ok();
    guarded_debug_fault_stage(
        enabled.as_deref(),
        state_root.as_deref(),
        parent_port,
        raw_port,
        std::env::var("SQLITE_CAPSULE_RUNTIME_FAULT_STAGE").ok(),
    )
}

#[cfg(any(test, all(feature = "debug-fault-injection", debug_assertions)))]
fn backup_fault_point(purpose: BackupPurpose, point: BackupFaultPoint) {
    let purpose = match purpose {
        BackupPurpose::Prewrite => "prewrite",
        BackupPurpose::Checkpoint => "checkpoint",
        BackupPurpose::Close => "close",
        BackupPurpose::Update => "update",
    };
    let point = match point {
        BackupFaultPoint::MarkerSynced => "marker-synced",
        BackupFaultPoint::DatabaseCopied => "database-copied",
        BackupFaultPoint::ManifestSynced => "manifest-synced",
    };
    if requested_runtime_fault_stage().is_some_and(|value| value == format!("{purpose}.{point}")) {
        std::process::exit(98);
    }
}

#[cfg(not(any(test, all(feature = "debug-fault-injection", debug_assertions))))]
fn backup_fault_point(_purpose: BackupPurpose, _point: BackupFaultPoint) {}

#[cfg(any(test, all(feature = "debug-fault-injection", debug_assertions)))]
fn restore_fault_point(point: RestoreFaultPoint) {
    let point = match point {
        RestoreFaultPoint::MarkerSynced => "marker-synced",
        RestoreFaultPoint::DatabaseCopied => "database-copied",
        RestoreFaultPoint::Verified => "verified",
    };
    if requested_runtime_fault_stage().is_some_and(|value| value == format!("restore.{point}")) {
        std::process::exit(98);
    }
}

#[cfg(not(any(test, all(feature = "debug-fault-injection", debug_assertions))))]
fn restore_fault_point(_point: RestoreFaultPoint) {}

#[cfg(any(test, all(feature = "debug-fault-injection", debug_assertions)))]
fn open_fault_point(point: OpenFaultPoint) {
    let point = match point {
        OpenFaultPoint::SourcePinned => "source-pinned",
        OpenFaultPoint::SqliteOpened => "sqlite-opened",
        OpenFaultPoint::Verified => "verified",
    };
    if requested_runtime_fault_stage().is_some_and(|value| value == format!("open.{point}")) {
        std::process::exit(98);
    }
}

#[cfg(not(any(test, all(feature = "debug-fault-injection", debug_assertions))))]
fn open_fault_point(_point: OpenFaultPoint) {}

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("launch evidence changed before the runtime opened")]
    LaunchEvidenceChanged,
    #[error("launch policy does not authorise executable assets")]
    ExecutionDenied,
    #[error("required database capability is not allowed")]
    CapabilityDenied,
    #[error("capsule verification failed: {0}")]
    Verification(String),
    #[error("asset path is unsafe or missing")]
    AssetPath,
    #[error("asset is oversized or malformed")]
    AssetPolicy,
    #[error("asset hash does not match its declaration")]
    AssetHash,
    #[error("endpoint request is invalid: {0}")]
    Endpoint(String),
    #[error("endpoint result exceeds runtime policy")]
    ResultPolicy,
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("launch inspection error: {0}")]
    Launch(#[from] sqlite_capsule_launch::LaunchError),
    #[error("capsule header inspection failed: {0}")]
    Inspect(#[from] InspectError),
    #[error("capsule lifecycle error: {0}")]
    Lifecycle(#[from] LifecycleError),
    #[error("writable runtime requires a host-owned writer-lock directory")]
    WriterLeaseRequired,
    #[error("writable runtime requires a host-owned backup directory")]
    BackupRootRequired,
    #[error("capsule backup directory must be outside the source directory")]
    BackupLocation,
    #[error("verified capsule backup failed: {0}")]
    Backup(String),
    #[error("SQLite crash recovery failed: {0}")]
    Recovery(String),
    #[error("capsule changed outside this host session")]
    SourceConflict,
}

impl RuntimeError {
    pub fn session_must_close(&self) -> bool {
        matches!(
            self,
            Self::LaunchEvidenceChanged
                | Self::SourceConflict
                | Self::Lifecycle(LifecycleError::Replaced)
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RuntimeAsset {
    pub path: String,
    pub media_type: String,
    #[serde(skip)]
    pub content: Vec<u8>,
    pub sha256: String,
    pub executable: bool,
    pub cache_policy: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RuntimeManifest {
    pub capsule_id: String,
    pub app_id: String,
    pub app_version: String,
    pub title: String,
    pub summary: String,
    pub entry_asset: String,
    pub format_version: String,
    pub runtime_protocol: String,
    pub effective_permissions: Value,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CheckResult {
    pub id: String,
    pub severity: String,
    pub passed: bool,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct VerificationReport {
    pub check_results: Vec<CheckResult>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupRecord {
    pub backup_id: String,
    pub created_at_unix: u64,
    pub verified_at_unix: u64,
    pub bytes: u64,
    pub sha256: String,
    pub source_identity: SourceIdentity,
    pub source_sha256: String,
    pub capsule_id: String,
    pub application_digest: Option<String>,
    pub change_position: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RestoreRecord {
    pub backup_id: String,
    pub output_sha256: String,
    pub output_bytes: u64,
    pub capsule_id: String,
    pub application_digest: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RecoveryReport {
    pub sqlite_recovery_attempted: bool,
    pub rollback_journal_bytes_before: u64,
    pub rollback_journal_sha256_before: String,
    pub rollback_journal_hot_candidate_before: bool,
    pub rollback_journal_present_after: bool,
    pub source_sha256_before: String,
    pub source_sha256_after: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct BackupInventoryReport {
    pub verified: Vec<BackupRecord>,
    pub incomplete_artifacts: Vec<String>,
    pub invalid_artifacts: Vec<String>,
}

/// Perform only SQLite's own rollback-journal recovery, then run the complete
/// launch inspection while the source identity and host writer lease remain
/// pinned. The host never deletes or rewrites a journal itself.
pub fn inspect_launch_with_recovery(
    path: &Path,
    writer_lock_root: &Path,
) -> Result<(LaunchInspection, Option<RecoveryReport>), RuntimeError> {
    let restore_marker = restore_marker_path(path);
    if restore_marker.exists() {
        return Err(RuntimeError::Recovery(
            "an interrupted restore marker is present; preserve the file and marker for explicit recovery"
                .to_owned(),
        ));
    }
    let header = inspect_header(path)?;
    let journal_path = sidecar_path(&header.canonical_path, "-journal");
    let Some(journal_before) = rollback_journal_snapshot(&journal_path)? else {
        return Ok((inspect_launch(&header.canonical_path)?, None));
    };

    let mut source = PinnedSource::open(&header.canonical_path, true)?;
    let _writer_lease = WriterLease::acquire(writer_lock_root, &source)?;
    let pinned_header = inspect_header(source.canonical_path())?;
    if pinned_header.canonical_path != header.canonical_path || pinned_header.bytes != header.bytes
    {
        return Err(RuntimeError::LaunchEvidenceChanged);
    }
    let source_sha256_before = hash_file(source.canonical_path())?;

    let connection = Connection::open_with_flags(
        source.canonical_path(),
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    harden_connection(&connection, true)?;
    let _: i64 = connection.pragma_query_value(None, "schema_version", |row| row.get(0))?;
    let integrity: String =
        connection.query_row("PRAGMA integrity_check(1)", [], |row| row.get(0))?;
    if integrity != "ok" {
        return Err(RuntimeError::Recovery(format!(
            "post-recovery integrity_check returned {integrity}"
        )));
    }
    drop(connection);

    source.accept_host_write()?;
    source.assert_current()?;
    let recovered_header = inspect_header(source.canonical_path())?;
    if recovered_header.application_id != header.application_id {
        return Err(RuntimeError::Recovery(
            "capsule application ID changed during recovery".to_owned(),
        ));
    }
    let inspection = inspect_launch(source.canonical_path())?;
    source.assert_current()?;
    let report = RecoveryReport {
        sqlite_recovery_attempted: true,
        rollback_journal_bytes_before: journal_before.bytes,
        rollback_journal_sha256_before: journal_before.sha256,
        rollback_journal_hot_candidate_before: journal_before.hot_candidate,
        rollback_journal_present_after: journal_path.exists(),
        source_sha256_before,
        source_sha256_after: hash_file(source.canonical_path())?,
    };
    Ok((inspection, Some(report)))
}

pub fn restore_verified_backup(
    backup_root: &Path,
    backup_id: &str,
    output: &Path,
) -> Result<RestoreRecord, RuntimeError> {
    if !safe_backup_id(backup_id) {
        return Err(RuntimeError::Backup("invalid backup identifier".to_owned()));
    }
    if output.exists() {
        return Err(RuntimeError::Backup(
            "restore output already exists".to_owned(),
        ));
    }
    let restore_marker = restore_marker_path(output);
    if restore_marker.exists() {
        return Err(RuntimeError::Recovery(
            "an interrupted restore marker already exists at the selected path".to_owned(),
        ));
    }
    let canonical_root = backup_root.canonicalize()?;
    let backup_path = canonical_root.join(backup_id);
    let canonical_backup = backup_path.canonicalize()?;
    if !canonical_backup.starts_with(&canonical_root) || !canonical_backup.is_file() {
        return Err(RuntimeError::Backup(
            "backup is outside the managed directory".to_owned(),
        ));
    }
    let manifest_path = canonical_root.join(format!("{backup_id}.json"));
    let record: BackupRecord = serde_json::from_slice(&std::fs::read(manifest_path)?)?;
    if record.backup_id != backup_id
        || record.bytes != std::fs::metadata(&canonical_backup)?.len()
        || record.sha256 != hash_file(&canonical_backup)?
    {
        return Err(RuntimeError::Backup(
            "backup bytes do not match the verified inventory".to_owned(),
        ));
    }

    let backup_inspection = inspect_launch(&canonical_backup)?;
    if backup_inspection.identity.capsule_id != record.capsule_id
        || backup_inspection
            .evidence
            .application_digest
            .map(|digest| lower_hex(&digest))
            != record.application_digest
    {
        return Err(RuntimeError::Backup(
            "backup launch identity does not match its inventory".to_owned(),
        ));
    }
    let backup_connection = Connection::open_with_flags(
        &canonical_backup,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    harden_connection(&backup_connection, false)?;
    conformance::verify(&backup_connection, &backup_inspection.identity)?;

    let parent = output.parent().ok_or(RuntimeError::BackupLocation)?;
    if !parent.is_dir() {
        return Err(RuntimeError::Backup(
            "restore output parent does not exist".to_owned(),
        ));
    }
    let marker = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&restore_marker)?;
    serde_json::to_writer(
        &marker,
        &serde_json::json!({"version": 1, "backup_id": backup_id}),
    )?;
    marker.sync_all()?;
    restore_fault_point(RestoreFaultPoint::MarkerSynced);
    let mut temporary = RestoreOutput::new(output.to_path_buf(), restore_marker.clone());
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?
        .sync_all()?;
    let mut destination = Connection::open_with_flags(
        output,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    {
        let backup = Backup::new(&backup_connection, &mut destination)?;
        backup.run_to_completion(128, Duration::from_millis(5), None)?;
    }
    destination.execute_batch("PRAGMA journal_mode=DELETE;")?;
    drop(destination);
    drop(backup_connection);
    restore_fault_point(RestoreFaultPoint::DatabaseCopied);

    let restored = inspect_launch(output)?;
    if restored.identity.capsule_id != record.capsule_id
        || restored
            .evidence
            .application_digest
            .map(|digest| lower_hex(&digest))
            != record.application_digest
    {
        return Err(RuntimeError::Backup(
            "restored capsule identity changed".to_owned(),
        ));
    }
    let restored_connection = Connection::open_with_flags(
        output,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    harden_connection(&restored_connection, false)?;
    conformance::verify(&restored_connection, &restored.identity)?;
    drop(restored_connection);
    let output_sha256 = hash_file(output)?;
    let output_bytes = std::fs::metadata(output)?.len();
    restore_fault_point(RestoreFaultPoint::Verified);
    std::fs::remove_file(&restore_marker)?;
    temporary.keep();
    Ok(RestoreRecord {
        backup_id: backup_id.to_owned(),
        output_sha256,
        output_bytes,
        capsule_id: record.capsule_id,
        application_digest: record.application_digest,
    })
}

pub struct VerifiedCapsule {
    inspection: LaunchInspection,
    decision: LaunchDecision,
    connection: Connection,
    writable: bool,
    verification: VerificationReport,
    source: PinnedSource,
    _writer_lease: Option<WriterLease>,
    opened_data_version: i64,
    change_position: i64,
    backup_root: Option<PathBuf>,
    backup_record: Option<BackupRecord>,
    writes_since_backup: u32,
}

impl VerifiedCapsule {
    /// Recheck an already authorised launch and open one verified runtime.
    ///
    /// A writable connection requires both `executable_allowed` and an
    /// effective `database.write = allow` decision. The caller cannot obtain
    /// assets or named endpoints from a failed constructor.
    pub fn open(
        path: &Path,
        expected: &LaunchInspection,
        decision: &LaunchDecision,
        writable: bool,
        writer_lock_root: Option<&Path>,
        backup_root: Option<&Path>,
    ) -> Result<Self, RuntimeError> {
        if !decision.executable_allowed {
            return Err(RuntimeError::ExecutionDenied);
        }
        let required_capability = if writable {
            "database.write"
        } else {
            "database.read"
        };
        let capability_allowed = decision
            .capabilities
            .get(required_capability)
            .is_some_and(|capability| capability.decision == CapabilityDecision::Allow);
        if !capability_allowed {
            return Err(RuntimeError::CapabilityDenied);
        }
        let source = PinnedSource::open(path, writable)?;
        let writer_lease = if writable {
            let lock_root = writer_lock_root.ok_or(RuntimeError::WriterLeaseRequired)?;
            Some(WriterLease::acquire(lock_root, &source)?)
        } else {
            None
        };
        if writable && backup_root.is_none() {
            return Err(RuntimeError::BackupRootRequired);
        }
        open_fault_point(OpenFaultPoint::SourcePinned);
        let inspection = inspect_launch(source.canonical_path())?;
        if inspection.identity != expected.identity
            || inspection.evidence != expected.evidence
            || inspection
                .evidence
                .application_digest
                .map(|digest| lower_hex(&digest))
                != decision.application_digest_hex
        {
            return Err(RuntimeError::LaunchEvidenceChanged);
        }
        let flags = if writable {
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX
        } else {
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX
        };
        let connection = Connection::open_with_flags(source.canonical_path(), flags)?;
        harden_connection(&connection, writable)?;
        source.assert_current()?;
        open_fault_point(OpenFaultPoint::SqliteOpened);
        let post_open = inspect_launch(source.canonical_path())?;
        if post_open.identity != inspection.identity || post_open.evidence != inspection.evidence {
            return Err(RuntimeError::LaunchEvidenceChanged);
        }
        let verification = conformance::verify(&connection, &inspection.identity)?;
        let opened_data_version = data_version(&connection)?;
        let change_position = change_position(&connection)?;
        open_fault_point(OpenFaultPoint::Verified);
        let mut runtime_decision = decision.clone();
        if !writable
            && let Some(database_write) = runtime_decision.capabilities.get_mut("database.write")
        {
            database_write.decision = CapabilityDecision::Deny;
            database_write.reason = "the current native session is read-only".to_owned();
        }
        Ok(Self {
            inspection,
            decision: runtime_decision,
            connection,
            writable,
            verification,
            source,
            _writer_lease: writer_lease,
            opened_data_version,
            change_position,
            backup_root: backup_root.map(Path::to_path_buf),
            backup_record: None,
            writes_since_backup: 0,
        })
    }

    pub fn verification(&self) -> &VerificationReport {
        &self.verification
    }

    pub fn manifest(&self) -> RuntimeManifest {
        let identity = &self.inspection.identity;
        RuntimeManifest {
            capsule_id: identity.capsule_id.clone(),
            app_id: identity.app_id.clone(),
            app_version: identity.app_version.clone(),
            title: identity.title.clone(),
            summary: identity.summary.clone(),
            entry_asset: identity.entry_asset.clone(),
            format_version: identity.format_version.clone(),
            runtime_protocol: identity.runtime_protocol.clone(),
            effective_permissions: serde_json::to_value(&self.decision.capabilities)
                .expect("serialising typed capability decisions cannot fail"),
        }
    }

    pub fn entry_asset(&self) -> Result<RuntimeAsset, RuntimeError> {
        self.asset(&self.inspection.identity.entry_asset)
    }

    pub fn permissions(&self) -> Value {
        json_object([
            ("requested", self.inspection.identity.permissions.clone()),
            (
                "effective",
                serde_json::to_value(&self.decision.capabilities)
                    .expect("serialising typed capability decisions cannot fail"),
            ),
        ])
    }

    pub fn backup_record(&self) -> Option<&BackupRecord> {
        self.backup_record.as_ref()
    }

    pub fn writable(&self) -> bool {
        self.writable
    }

    /// Create a verified current-state checkpoint when the writable session
    /// has committed changes since its last recovery point.
    pub fn checkpoint_if_dirty(&mut self) -> Result<Option<BackupRecord>, RuntimeError> {
        self.checkpoint_if_dirty_for(BackupPurpose::Checkpoint)
    }

    /// Create the final verified checkpoint used by the trusted host's close
    /// guard. Keeping this distinct makes close-stage interruption observable
    /// without changing the ordinary bounded-checkpoint policy.
    pub fn checkpoint_for_close(&mut self) -> Result<Option<BackupRecord>, RuntimeError> {
        self.checkpoint_if_dirty_for(BackupPurpose::Close)
    }

    /// Quiesce a writable capsule for replacement of the native host.
    ///
    /// Unlike an ordinary checkpoint, update preparation always establishes a
    /// verified recovery point for a writable session, including a newly
    /// opened session that has not committed a write yet. A clean session may
    /// reuse its already verified current-state backup. Read-only sessions have
    /// no mutable capsule state to preserve and return `None`.
    pub fn prepare_for_host_update(&mut self) -> Result<Option<BackupRecord>, RuntimeError> {
        if !self.writable {
            return Ok(None);
        }
        self.assert_session_current()?;
        if self.backup_record.is_none() || self.writes_since_backup > 0 {
            self.create_verified_backup(BackupPurpose::Update)?;
        }
        Ok(self.backup_record.clone())
    }

    fn checkpoint_if_dirty_for(
        &mut self,
        purpose: BackupPurpose,
    ) -> Result<Option<BackupRecord>, RuntimeError> {
        if !self.writable || self.writes_since_backup == 0 {
            return Ok(None);
        }
        self.assert_session_current()?;
        self.create_verified_backup(purpose)?;
        Ok(self.backup_record.clone())
    }

    pub fn asset(&self, path: &str) -> Result<RuntimeAsset, RuntimeError> {
        self.assert_session_current()?;
        if !safe_asset_path(path) {
            return Err(RuntimeError::AssetPath);
        }
        let row = self
            .connection
            .query_row(
                "SELECT path, media_type, content, sha256, executable, cache_policy \
                 FROM capsule_asset WHERE path = ?1",
                [path],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Vec<u8>>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => RuntimeError::AssetPath,
                other => RuntimeError::Sqlite(other),
            })?;
        let (path, media_type, content, sha256, executable, cache_policy) = row;
        if content.len() > MAX_ASSET_BYTES
            || !safe_media_type(&media_type)
            || cache_policy != "no-store"
            || !matches!(executable, 0 | 1)
        {
            return Err(RuntimeError::AssetPolicy);
        }
        let actual = lower_hex(&Sha256::digest(&content));
        if sha256 != actual {
            return Err(RuntimeError::AssetHash);
        }
        Ok(RuntimeAsset {
            path,
            media_type,
            content,
            sha256,
            executable: executable == 1,
            cache_policy,
        })
    }

    pub fn read_endpoint(
        &mut self,
        name: &str,
        arguments: &serde_json::Map<String, Value>,
    ) -> Result<Value, RuntimeError> {
        self.assert_session_current()?;
        endpoint::execute(&mut self.connection, false, name, arguments)
    }

    pub fn write_endpoint(
        &mut self,
        name: &str,
        arguments: &serde_json::Map<String, Value>,
    ) -> Result<Value, RuntimeError> {
        if !self.writable {
            return Err(RuntimeError::CapabilityDenied);
        }
        self.assert_session_current()?;
        self.ensure_prewrite_backup()?;
        let result = endpoint::execute(&mut self.connection, true, name, arguments)?;
        self.source.accept_host_write()?;
        if data_version(&self.connection)? != self.opened_data_version {
            return Err(RuntimeError::SourceConflict);
        }
        self.change_position = change_position(&self.connection)?;
        self.writes_since_backup = self.writes_since_backup.saturating_add(1);
        Ok(result)
    }

    fn ensure_prewrite_backup(&mut self) -> Result<(), RuntimeError> {
        match (&self.backup_record, self.writes_since_backup) {
            (None, _) => self.create_verified_backup(BackupPurpose::Prewrite),
            (Some(_), writes) if writes >= MAX_WRITES_BETWEEN_BACKUPS => {
                // Rotate the current-state recovery point before accepting an
                // eleventh write. Running this before the next transaction
                // means a failed checkpoint never reports a committed named
                // write as failed or invites an unsafe retry.
                self.create_verified_backup(BackupPurpose::Checkpoint)
            }
            (Some(_), _) => Ok(()),
        }
    }

    fn create_verified_backup(&mut self, purpose: BackupPurpose) -> Result<(), RuntimeError> {
        let root = self
            .backup_root
            .as_ref()
            .ok_or(RuntimeError::BackupRootRequired)?;
        prepare_private_directory(root)?;
        let canonical_root = root.canonicalize()?;
        let source_parent = self
            .source
            .canonical_path()
            .parent()
            .ok_or(RuntimeError::BackupLocation)?;
        if canonical_root.starts_with(source_parent) {
            return Err(RuntimeError::BackupLocation);
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| RuntimeError::Backup("system clock is before Unix epoch".to_owned()))?;
        let backup_id = format!(
            "{:020}-{}-{}.capsule-backup.sqlite",
            now.as_nanos(),
            std::process::id(),
            &lower_hex(&self.inspection.evidence.source_sha256)[..16]
        );
        let backup_path = canonical_root.join(&backup_id);
        let manifest_path = canonical_root.join(format!("{backup_id}.json"));
        let marker_path = canonical_root.join(format!("{backup_id}.in-progress"));
        let marker = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&marker_path)?;
        serde_json::to_writer(
            &marker,
            &serde_json::json!({"version": 1, "backup_id": &backup_id}),
        )?;
        marker.sync_all()?;
        protect_private_file(&marker_path)?;
        backup_fault_point(purpose, BackupFaultPoint::MarkerSynced);
        let mut temporary = TemporaryBackup::new(
            backup_path.clone(),
            manifest_path.clone(),
            marker_path.clone(),
        );
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&backup_path)?
            .sync_all()?;
        protect_private_file(&backup_path)?;

        let mut destination = Connection::open_with_flags(
            &backup_path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        {
            let backup = Backup::new(&self.connection, &mut destination)?;
            backup.run_to_completion(128, Duration::from_millis(5), None)?;
        }
        destination.execute_batch("PRAGMA journal_mode=DELETE;")?;
        drop(destination);
        protect_private_file(&backup_path)?;
        backup_fault_point(purpose, BackupFaultPoint::DatabaseCopied);

        let backup_inspection = inspect_launch(&backup_path)?;
        if backup_inspection.identity.capsule_id != self.inspection.identity.capsule_id
            || backup_inspection.identity.app_id != self.inspection.identity.app_id
            || backup_inspection.identity.format_version != self.inspection.identity.format_version
            || backup_inspection.evidence.application_digest
                != self.inspection.evidence.application_digest
        {
            return Err(RuntimeError::Backup(
                "backup identity or signed application digest changed".to_owned(),
            ));
        }
        let backup_connection = Connection::open_with_flags(
            &backup_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        harden_connection(&backup_connection, false)?;
        let backup_verification =
            conformance::verify(&backup_connection, &backup_inspection.identity)?;
        if backup_verification
            .check_results
            .iter()
            .any(|check| !check.passed && check.severity == "error")
        {
            return Err(RuntimeError::Backup(
                "backup failed a capsule-declared error check".to_owned(),
            ));
        }
        drop(backup_connection);

        let sha256 = hash_file(&backup_path)?;
        let bytes = std::fs::metadata(&backup_path)?.len();
        let seconds = now.as_secs();
        let record = BackupRecord {
            backup_id: backup_id.clone(),
            created_at_unix: seconds,
            verified_at_unix: seconds,
            bytes,
            sha256,
            source_identity: self.source.identity().clone(),
            source_sha256: hash_file(self.source.canonical_path())?,
            capsule_id: self.inspection.identity.capsule_id.clone(),
            application_digest: self
                .inspection
                .evidence
                .application_digest
                .map(|digest| lower_hex(&digest)),
            change_position: self.change_position,
        };
        let manifest = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&manifest_path)?;
        serde_json::to_writer_pretty(&manifest, &record)?;
        manifest.sync_all()?;
        protect_private_file(&manifest_path)?;
        backup_fault_point(purpose, BackupFaultPoint::ManifestSynced);
        std::fs::remove_file(&marker_path)?;
        temporary.keep();
        enforce_backup_retention(&canonical_root, seconds)?;
        self.backup_record = Some(record);
        self.writes_since_backup = 0;
        Ok(())
    }

    fn assert_session_current(&self) -> Result<(), RuntimeError> {
        self.source.assert_current()?;
        if data_version(&self.connection)? != self.opened_data_version
            || change_position(&self.connection)? != self.change_position
        {
            return Err(RuntimeError::SourceConflict);
        }
        Ok(())
    }
}

struct TemporaryBackup {
    database: PathBuf,
    manifest: PathBuf,
    marker: PathBuf,
    keep: bool,
}

struct RestoreOutput {
    path: PathBuf,
    marker: PathBuf,
    keep: bool,
}

impl RestoreOutput {
    fn new(path: PathBuf, marker: PathBuf) -> Self {
        Self {
            path,
            marker,
            keep: false,
        }
    }

    fn keep(&mut self) {
        self.keep = true;
    }
}

impl Drop for RestoreOutput {
    fn drop(&mut self) {
        if !self.keep {
            let _ = std::fs::remove_file(&self.path);
            let _ = std::fs::remove_file(&self.marker);
        }
    }
}

impl TemporaryBackup {
    fn new(database: PathBuf, manifest: PathBuf, marker: PathBuf) -> Self {
        Self {
            database,
            manifest,
            marker,
            keep: false,
        }
    }

    fn keep(&mut self) {
        self.keep = true;
    }
}

impl Drop for TemporaryBackup {
    fn drop(&mut self) {
        if !self.keep {
            let _ = std::fs::remove_file(&self.database);
            let _ = std::fs::remove_file(&self.manifest);
            let _ = std::fs::remove_file(&self.marker);
        }
    }
}

struct RollbackJournalSnapshot {
    bytes: u64,
    sha256: String,
    hot_candidate: bool,
}

fn sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value = OsString::from(path.as_os_str());
    value.push(suffix);
    PathBuf::from(value)
}

fn rollback_journal_snapshot(path: &Path) -> Result<Option<RollbackJournalSnapshot>, RuntimeError> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(RuntimeError::Recovery(
            "rollback journal sidecar is not a regular file".to_owned(),
        ));
    }
    let mut file = File::open(path)?;
    let mut header = [0_u8; 8];
    let header_bytes = file.read(&mut header)?;
    file.seek(SeekFrom::Start(0))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(Some(RollbackJournalSnapshot {
        bytes: metadata.len(),
        sha256: lower_hex(&digest.finalize()),
        // This is deliberately named a candidate: SQLite also considers lock
        // state when deciding whether a rollback journal is hot.
        hot_candidate: metadata.len() > 512
            && header_bytes == header.len()
            && header.iter().any(|byte| *byte != 0),
    }))
}

pub fn inspect_backup_inventory(root: &Path) -> Result<BackupInventoryReport, RuntimeError> {
    let metadata = match std::fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(BackupInventoryReport::default());
        }
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(RuntimeError::Backup(
            "managed backup root is not a regular directory".to_owned(),
        ));
    }

    let mut databases = BTreeSet::new();
    let mut manifests = BTreeSet::new();
    let mut markers = BTreeSet::new();
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".capsule-backup.sqlite") {
            databases.insert(name);
        } else if let Some(backup_id) = name.strip_suffix(".capsule-backup.sqlite.json") {
            manifests.insert(format!("{backup_id}.capsule-backup.sqlite"));
        } else if let Some(backup_id) = name.strip_suffix(".capsule-backup.sqlite.in-progress") {
            markers.insert(format!("{backup_id}.capsule-backup.sqlite"));
        }
    }

    let backup_ids = databases
        .union(&manifests)
        .cloned()
        .collect::<BTreeSet<_>>()
        .union(&markers)
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut report = BackupInventoryReport::default();
    for backup_id in backup_ids {
        if !safe_backup_id(&backup_id) {
            report.invalid_artifacts.push(backup_id);
            continue;
        }
        if markers.contains(&backup_id)
            || !databases.contains(&backup_id)
            || !manifests.contains(&backup_id)
        {
            report.incomplete_artifacts.push(backup_id);
            continue;
        }
        let database_path = root.join(&backup_id);
        let manifest_path = root.join(format!("{backup_id}.json"));
        let regular_files = [database_path.as_path(), manifest_path.as_path()]
            .into_iter()
            .all(|path| {
                std::fs::symlink_metadata(path)
                    .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
            });
        if !regular_files {
            report.invalid_artifacts.push(backup_id);
            continue;
        }
        let record = std::fs::read(&manifest_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<BackupRecord>(&bytes).ok());
        let Some(record) = record else {
            report.invalid_artifacts.push(backup_id);
            continue;
        };
        let valid = record.backup_id == backup_id
            && std::fs::metadata(&database_path)
                .is_ok_and(|metadata| metadata.len() == record.bytes)
            && hash_file(&database_path).is_ok_and(|sha256| sha256 == record.sha256);
        if valid {
            report.verified.push(record);
        } else {
            report.invalid_artifacts.push(backup_id);
        }
    }
    Ok(report)
}

fn restore_marker_path(path: &Path) -> PathBuf {
    sidecar_path(path, ".capsule-restore-in-progress")
}

fn hash_file(path: &Path) -> Result<String, RuntimeError> {
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

fn enforce_backup_retention(root: &Path, now_unix: u64) -> Result<(), RuntimeError> {
    const MAX_BACKUPS: usize = 10;
    const MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;
    const MAX_AGE_SECONDS: u64 = 90 * 24 * 60 * 60;

    let mut backups = std::fs::read_dir(root)?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.ends_with(".capsule-backup.sqlite") {
                return None;
            }
            let bytes = entry.metadata().ok()?.len();
            let manifest_path = root.join(format!("{name}.json"));
            let created_at = std::fs::read(&manifest_path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<BackupRecord>(&bytes).ok())
                .map(|record| record.created_at_unix)
                .unwrap_or(0);
            Some((name, entry.path(), manifest_path, bytes, created_at))
        })
        .collect::<Vec<_>>();
    backups.sort_by(|left, right| left.0.cmp(&right.0));
    let mut total = backups.iter().map(|entry| entry.3).sum::<u64>();
    while backups.len() > 1
        && (backups.len() > MAX_BACKUPS
            || total > MAX_BYTES
            || now_unix.saturating_sub(backups[0].4) > MAX_AGE_SECONDS)
    {
        let (_name, path, manifest, bytes, _created_at) = backups.remove(0);
        std::fs::remove_file(path)?;
        if manifest.exists() {
            std::fs::remove_file(manifest)?;
        }
        total = total.saturating_sub(bytes);
    }
    Ok(())
}

fn safe_backup_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 160
        && value.ends_with(".capsule-backup.sqlite")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn data_version(connection: &Connection) -> Result<i64, RuntimeError> {
    connection
        .pragma_query_value(None, "data_version", |row| row.get(0))
        .map_err(RuntimeError::Sqlite)
}

fn change_position(connection: &Connection) -> Result<i64, RuntimeError> {
    connection
        .query_row(
            "SELECT coalesce(max(id), 0) FROM capsule_change_log",
            [],
            |row| row.get(0),
        )
        .map_err(RuntimeError::Sqlite)
}

fn harden_connection(connection: &Connection, writable: bool) -> Result<(), RuntimeError> {
    connection.load_extension_disable()?;
    connection.execute_batch(
        "PRAGMA trusted_schema=OFF; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;",
    )?;
    connection.pragma_update(None, "query_only", !writable)?;
    connection.set_limit(Limit::SQLITE_LIMIT_LENGTH, MAX_ASSET_BYTES as i32)?;
    connection.set_limit(Limit::SQLITE_LIMIT_SQL_LENGTH, 1024 * 1024)?;
    connection.set_limit(Limit::SQLITE_LIMIT_COLUMN, 256)?;
    connection.set_limit(Limit::SQLITE_LIMIT_EXPR_DEPTH, 100)?;
    connection.set_limit(Limit::SQLITE_LIMIT_COMPOUND_SELECT, 50)?;
    connection.set_limit(Limit::SQLITE_LIMIT_VDBE_OP, 1_000_000)?;
    connection.set_limit(Limit::SQLITE_LIMIT_FUNCTION_ARG, 64)?;
    connection.set_limit(Limit::SQLITE_LIMIT_ATTACHED, 0)?;
    connection.set_limit(Limit::SQLITE_LIMIT_VARIABLE_NUMBER, 128)?;
    // SQLite implements declared foreign-key actions with its trigger VM even
    // when the verified schema contains no SQL triggers. Keep that internal
    // machinery bounded instead of disabling legitimate cascades entirely.
    connection.set_limit(
        Limit::SQLITE_LIMIT_TRIGGER_DEPTH,
        MAX_FOREIGN_KEY_CASCADE_DEPTH,
    )?;
    connection.set_limit(Limit::SQLITE_LIMIT_WORKER_THREADS, 0)?;
    Ok(())
}

pub(crate) fn safe_asset_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.chars().any(|character| character.is_control())
        && path
            .split('/')
            .all(|component| !matches!(component, "" | "." | ".."))
}

fn safe_media_type(value: &str) -> bool {
    if value.is_empty()
        || !value.is_ascii()
        || !value.bytes().all(|byte| (32..=126).contains(&byte))
    {
        return false;
    }
    let mut parts = value.split(';');
    let Some(essence) = parts.next().map(str::trim) else {
        return false;
    };
    let Some((kind, subtype)) = essence.split_once('/') else {
        return false;
    };
    if !media_token(kind, false) || !media_token(subtype, false) {
        return false;
    }
    parts.all(|parameter| {
        let Some((name, value)) = parameter.trim().split_once('=') else {
            return false;
        };
        media_token(name.trim(), false) && media_token(value.trim(), true)
    })
}

fn media_token(value: &str, allow_apostrophe: bool) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#' | b'$' | b'&' | b'^' | b'_' | b'.' | b'+' | b'-'
                )
                || (allow_apostrophe && byte == b'\'')
        })
}

pub(crate) fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn json_object<const N: usize>(entries: [(&str, Value); N]) -> Value {
    Value::Object(
        entries
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        ffi::OsString,
        process::Command,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use serde_json::{Map, json};
    use sqlite_capsule_policy::{EvaluationContext, TrustStore};

    use super::*;

    #[test]
    fn asset_paths_reject_control_characters() {
        assert!(safe_asset_path("app/naïve.json"));
        for character in (0_u8..=31).chain([127]) {
            assert!(!safe_asset_path(&format!(
                "app/bad{}path",
                char::from(character)
            )));
        }
    }

    static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let suffix = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "sqlite-capsule-runtime-retention-{}-{suffix}",
                std::process::id()
            ));
            std::fs::create_dir(&path).expect("create retention directory");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn fake_backup(root: &Path, index: u64, created_at_unix: u64) {
        let backup_id = format!("{index:020}-test.capsule-backup.sqlite");
        let bytes = b"test";
        std::fs::write(root.join(&backup_id), bytes).expect("write backup");
        let record = BackupRecord {
            backup_id: backup_id.clone(),
            created_at_unix,
            verified_at_unix: created_at_unix,
            bytes: bytes.len() as u64,
            sha256: "fixture".to_owned(),
            source_identity: SourceIdentity {
                device: 1,
                file: index,
                bytes: bytes.len() as u64,
            },
            source_sha256: "source".to_owned(),
            capsule_id: "capsule".to_owned(),
            application_digest: None,
            change_position: index as i64,
        };
        std::fs::write(
            root.join(format!("{backup_id}.json")),
            serde_json::to_vec(&record).expect("record JSON"),
        )
        .expect("write record");
    }

    #[test]
    fn retention_bounds_count_and_age_without_deleting_the_last_copy() {
        const NOW: u64 = 20_000_000;
        let counted = TestDirectory::new();
        for index in 0..12 {
            fake_backup(&counted.0, index, NOW - index);
        }
        enforce_backup_retention(&counted.0, NOW).expect("count retention");
        assert_eq!(
            std::fs::read_dir(&counted.0)
                .expect("read retained files")
                .filter_map(Result::ok)
                .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "sqlite"))
                .count(),
            10
        );

        let aged = TestDirectory::new();
        fake_backup(&aged.0, 0, 1);
        fake_backup(&aged.0, 1, 2);
        enforce_backup_retention(&aged.0, NOW).expect("age retention");
        assert_eq!(
            std::fs::read_dir(&aged.0)
                .expect("read age-retained files")
                .filter_map(Result::ok)
                .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "sqlite"))
                .count(),
            1
        );
    }

    fn checked_capsule() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .join("capsules/diagram-studio.capsule.sqlite")
    }

    fn authorised_decision(root: &Path, inspection: &LaunchInspection) -> LaunchDecision {
        let mut store = TrustStore::open(&root.join("trust/trust.sqlite"))
            .expect("open fault-worker trust store");
        let capabilities = inspection.evidence.requested_capabilities.clone();
        let context = EvaluationContext {
            host_policy: capabilities
                .iter()
                .map(|capability| (capability.clone(), CapabilityDecision::Allow))
                .collect::<BTreeMap<_, _>>(),
            operating_system_permission: capabilities
                .iter()
                .map(|capability| (capability.clone(), CapabilityDecision::Allow))
                .collect::<BTreeMap<_, _>>(),
            allow_once: capabilities,
            trust_once: true,
        };
        store
            .evaluate(&inspection.evidence, &context)
            .expect("authorise fault worker")
    }

    fn rename_arguments(stage: &str) -> Map<String, Value> {
        json!({
            "operation_id": format!("operation-fault-{stage}"),
            "expected_cursor": 0,
            "diagram_id": "diagram-main",
            "from_title": "Design and present architecture diagrams",
            "to_title": format!("Fault injection {stage}")
        })
        .as_object()
        .expect("object arguments")
        .clone()
    }

    #[test]
    fn debug_fault_request_requires_an_isolated_dual_port_guard() {
        let directory = TestDirectory::new();
        let stage = || Some("restore.database-copied".to_owned());
        assert_eq!(
            guarded_debug_fault_stage(
                Some("enabled"),
                Some(&directory.0),
                Some(41_001),
                Some(41_002),
                stage(),
            ),
            stage()
        );
        assert!(
            guarded_debug_fault_stage(
                None,
                Some(&directory.0),
                Some(41_001),
                Some(41_002),
                stage(),
            )
            .is_none()
        );
        assert!(
            guarded_debug_fault_stage(
                Some("enabled"),
                Some(Path::new("relative-state")),
                Some(41_001),
                Some(41_002),
                stage(),
            )
            .is_none()
        );
        assert!(
            guarded_debug_fault_stage(
                Some("enabled"),
                Some(&directory.0),
                Some(41_001),
                Some(41_001),
                stage(),
            )
            .is_none()
        );
        assert!(
            guarded_debug_fault_stage(
                Some("enabled"),
                Some(&directory.0),
                Some(41_001),
                Some(41_002),
                Some("restore.unknown".to_owned()),
            )
            .is_none()
        );
        for open_stage in ["open.source-pinned", "open.sqlite-opened", "open.verified"] {
            assert_eq!(
                guarded_debug_fault_stage(
                    Some("enabled"),
                    Some(&directory.0),
                    Some(41_001),
                    Some(41_002),
                    Some(open_stage.to_owned()),
                ),
                Some(open_stage.to_owned())
            );
        }
    }

    #[test]
    fn lifecycle_fault_crash_worker() {
        let Some(stage) = std::env::var_os("SQLITE_CAPSULE_RUNTIME_FAULT_STAGE") else {
            return;
        };
        let stage = stage.to_string_lossy().into_owned();
        let capsule = PathBuf::from(
            std::env::var_os("SQLITE_CAPSULE_RUNTIME_FAULT_CAPSULE")
                .expect("fault-worker capsule path"),
        );
        let root = PathBuf::from(
            std::env::var_os("SQLITE_CAPSULE_RUNTIME_FAULT_ROOT").expect("fault-worker root"),
        );
        let inspection = inspect_launch(&capsule).expect("fault-worker launch inspection");
        let decision = authorised_decision(&root, &inspection);
        let lock_root = root.join("writer-locks");
        let backup_root = root.join("backups");
        let mut runtime = VerifiedCapsule::open(
            &capsule,
            &inspection,
            &decision,
            true,
            Some(&lock_root),
            Some(&backup_root),
        )
        .expect("open fault-worker runtime");
        runtime
            .write_endpoint("diagram.rename", &rename_arguments(&stage))
            .expect("fault-worker write");
        match stage.split_once('.').map(|entry| entry.0) {
            Some("checkpoint") => {
                runtime
                    .checkpoint_if_dirty()
                    .expect("fault-worker checkpoint");
            }
            Some("close") => {
                runtime
                    .checkpoint_for_close()
                    .expect("fault-worker close checkpoint");
            }
            Some("update") => {
                runtime
                    .prepare_for_host_update()
                    .expect("fault-worker update checkpoint");
            }
            Some("restore") => {
                let backup_id = runtime
                    .backup_record()
                    .expect("fault-worker prewrite backup")
                    .backup_id
                    .clone();
                drop(runtime);
                let restore_parent = root.join("restored");
                std::fs::create_dir_all(&restore_parent).expect("create restore parent");
                restore_verified_backup(
                    &backup_root,
                    &backup_id,
                    &restore_parent.join("result.sqlitecapsule"),
                )
                .expect("fault-worker restore");
            }
            Some("prewrite") => {}
            _ => panic!("unexpected fault stage {stage}"),
        }
        panic!("fault point {stage} did not terminate the worker");
    }

    #[test]
    fn abrupt_runtime_open_releases_the_writer_and_leaves_source_and_inventory_clean() {
        for stage in ["open.source-pinned", "open.sqlite-opened", "open.verified"] {
            let directory = TestDirectory::new();
            let source_parent = directory.0.join("source");
            std::fs::create_dir(&source_parent).expect("create open-fault source directory");
            let capsule = source_parent.join("open-fault.sqlitecapsule");
            std::fs::copy(checked_capsule(), &capsule).expect("copy open-fault capsule");
            let source_sha256 = hash_file(&capsule).expect("hash source before open fault");
            let root = directory.0.join("host");
            let status = Command::new(std::env::current_exe().expect("current test executable"))
                .arg("--exact")
                .arg("tests::lifecycle_fault_crash_worker")
                .arg("--nocapture")
                .env("SQLITE_CAPSULE_RUNTIME_FAULT_STAGE", stage)
                .env("SQLITE_CAPSULE_RUNTIME_FAULT_CAPSULE", &capsule)
                .env("SQLITE_CAPSULE_RUNTIME_FAULT_ROOT", &root)
                .status()
                .expect("run open fault worker");
            assert_eq!(status.code(), Some(98), "fault stage {stage}");
            assert_eq!(
                hash_file(&capsule).expect("hash source after open fault"),
                source_sha256,
                "{stage}"
            );
            assert!(!capsule.with_extension("sqlitecapsule-journal").exists());
            let inventory = inspect_backup_inventory(&root.join("backups"))
                .expect("inspect clean open-fault inventory");
            assert!(inventory.verified.is_empty(), "{stage}");
            assert!(inventory.incomplete_artifacts.is_empty(), "{stage}");
            assert!(inventory.invalid_artifacts.is_empty(), "{stage}");

            let source = Connection::open(&capsule).expect("open source evidence");
            assert_eq!(
                source
                    .query_row("SELECT count(*) FROM capsule_change_log", [], |row| {
                        row.get::<_, i64>(0)
                    })
                    .expect("source change count"),
                0,
                "{stage}"
            );
            drop(source);

            let inspection = inspect_launch(&capsule).expect("reinspect after open fault");
            let decision = authorised_decision(&root, &inspection);
            let reopened = VerifiedCapsule::open(
                &capsule,
                &inspection,
                &decision,
                true,
                Some(&root.join("writer-locks")),
                Some(&root.join("backups")),
            )
            .expect("writer lease is released after open fault");
            drop(reopened);
        }
    }

    #[test]
    fn abrupt_backup_checkpoint_restore_and_close_leave_bounded_recovery_evidence() {
        let cases = [
            ("prewrite.marker-synced", 0_usize, 1_usize, 0_i64),
            ("prewrite.database-copied", 0, 1, 0),
            ("prewrite.manifest-synced", 0, 1, 0),
            ("checkpoint.marker-synced", 1, 1, 1),
            ("checkpoint.database-copied", 1, 1, 1),
            ("checkpoint.manifest-synced", 1, 1, 1),
            ("close.marker-synced", 1, 1, 1),
            ("close.database-copied", 1, 1, 1),
            ("close.manifest-synced", 1, 1, 1),
            ("update.marker-synced", 1, 1, 1),
            ("update.database-copied", 1, 1, 1),
            ("update.manifest-synced", 1, 1, 1),
            ("restore.marker-synced", 1, 0, 1),
            ("restore.database-copied", 1, 0, 1),
            ("restore.verified", 1, 0, 1),
        ];
        for (stage, verified_count, incomplete_count, change_count) in cases {
            let directory = TestDirectory::new();
            let source_parent = directory.0.join("source");
            std::fs::create_dir(&source_parent).expect("create fault source directory");
            let capsule = source_parent.join("fault.sqlitecapsule");
            std::fs::copy(checked_capsule(), &capsule).expect("copy fault capsule");
            let root = directory.0.join("host");
            let status = Command::new(std::env::current_exe().expect("current test executable"))
                .arg("--exact")
                .arg("tests::lifecycle_fault_crash_worker")
                .arg("--nocapture")
                .env("SQLITE_CAPSULE_RUNTIME_FAULT_STAGE", stage)
                .env("SQLITE_CAPSULE_RUNTIME_FAULT_CAPSULE", &capsule)
                .env("SQLITE_CAPSULE_RUNTIME_FAULT_ROOT", &root)
                .status()
                .expect("run lifecycle fault worker");
            assert_eq!(status.code(), Some(98), "fault stage {stage}");

            let backup_root = root.join("backups");
            let inventory = inspect_backup_inventory(&backup_root)
                .expect("inspect interrupted backup inventory");
            assert_eq!(inventory.verified.len(), verified_count, "{stage}");
            assert_eq!(
                inventory.incomplete_artifacts.len(),
                incomplete_count,
                "{stage}"
            );
            assert!(inventory.invalid_artifacts.is_empty(), "{stage}");

            let source = Connection::open(&capsule).expect("open source evidence");
            assert_eq!(
                source
                    .query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
                    .expect("source integrity"),
                "ok",
                "{stage}"
            );
            assert_eq!(
                source
                    .query_row("SELECT count(*) FROM capsule_change_log", [], |row| {
                        row.get::<_, i64>(0)
                    })
                    .expect("source change count"),
                change_count,
                "{stage}"
            );
            drop(source);

            let (purpose, point) = stage.split_once('.').expect("fault stage shape");
            if purpose != "restore" {
                let backup_id = &inventory.incomplete_artifacts[0];
                assert!(
                    backup_root
                        .join(format!("{backup_id}.in-progress"))
                        .is_file()
                );
                assert_eq!(
                    backup_root.join(backup_id).is_file(),
                    point != "marker-synced",
                    "{stage} database artifact"
                );
                assert_eq!(
                    backup_root.join(format!("{backup_id}.json")).is_file(),
                    point == "manifest-synced",
                    "{stage} manifest artifact"
                );
            } else {
                let restored = root.join("restored/result.sqlitecapsule");
                let mut marker = OsString::from(restored.as_os_str());
                marker.push(".capsule-restore-in-progress");
                assert!(PathBuf::from(marker).is_file());
                assert_eq!(
                    restored.is_file(),
                    point != "marker-synced",
                    "{stage} restored database"
                );
                assert!(matches!(
                    inspect_launch_with_recovery(&restored, &root.join("recovery-locks")),
                    Err(RuntimeError::Recovery(_))
                ));
                if restored.is_file() {
                    let restored_connection = Connection::open_with_flags(
                        &restored,
                        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
                    )
                    .expect("open interrupted restore evidence");
                    assert_eq!(
                        restored_connection
                            .query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
                            .expect("restored integrity"),
                        "ok"
                    );
                }
            }
        }
    }
}

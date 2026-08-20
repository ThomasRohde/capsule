//! Shared fail-closed launch evidence for native hosts and administrative CLI.

mod conformance;

use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use rusqlite::{Connection, OpenFlags, OptionalExtension, limits::Limit};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlite_capsule_core::{
    CapsuleIdentity, InspectError, MAX_CAPSULE_BYTES, inspect_header, inspect_metadata_connection,
};
use sqlite_capsule_crypto::{
    CryptoError, application_digest, publisher_identity, signature_inventory, verify_signatures,
};
use sqlite_capsule_policy::{LaunchEvidence, PolicyError, PublisherEvidence, SignatureEvidence};
use tempfile::NamedTempFile;
use thiserror::Error;

#[cfg(test)]
thread_local! {
    static CAPTURE_POST_COPY_HOOK: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        std::cell::RefCell::new(None);
}

#[cfg(test)]
fn capture_post_copy_hook() {
    CAPTURE_POST_COPY_HOOK.with(|hook| {
        if let Some(callback) = hook.borrow_mut().take() {
            callback();
        }
    });
}

#[cfg(not(test))]
fn capture_post_copy_hook() {}

#[derive(Debug, Error)]
pub enum LaunchError {
    #[error("capsule structure is invalid: {0}")]
    Structure(String),
    #[error("partial signed-app extension is not accepted")]
    PartialExtension,
    #[error("signature inventory changed during verification")]
    SignatureRace,
    #[error("capsule source changed during verification")]
    SourceRace,
    #[error("capsule source has unsupported SQLite sidecar or journal state")]
    SourceSidecar,
    #[error("verification was cancelled")]
    Cancelled,
    #[error("verification exceeded its deadline or resource limit")]
    LimitExceeded,
    #[error("permissions must be an object")]
    Permissions,
    #[error("capsule inspection failed: {0}")]
    Inspect(#[from] InspectError),
    #[error("signed-app verification failed: {0}")]
    Crypto(#[from] CryptoError),
    #[error("policy evidence is malformed: {0}")]
    Policy(#[from] PolicyError),
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Clone, Debug)]
pub struct LaunchInspection {
    pub identity: CapsuleIdentity,
    pub evidence: LaunchEvidence,
}

/// Launch evidence retained together with the exact private read-only snapshot
/// from which it was derived.
///
/// This is a host-internal projection boundary: callers can run a bounded read
/// projection against the already-verified snapshot, but cannot obtain a
/// source path from this API or keep the SQLite connection past the callback.
pub struct RetainedLaunchInspection {
    inspection: LaunchInspection,
    verified: VerifiedReadOnlyCapsule,
}

impl RetainedLaunchInspection {
    pub fn inspection(&self) -> &LaunchInspection {
        &self.inspection
    }

    pub fn project_snapshot<T>(&self, projection: impl FnOnce(&Connection) -> T) -> T {
        projection(self.verified.connection())
    }

    pub fn assert_source_current(&self) -> Result<(), LaunchError> {
        self.verified.assert_source_current()
    }

    pub fn into_inspection(self) -> LaunchInspection {
        self.inspection
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DeclaredCheckResult {
    pub id: String,
    pub severity: String,
    pub passed: bool,
    pub detail: String,
}

pub struct VerifiedReadOnlyCapsule {
    pub identity: CapsuleIdentity,
    pub source_sha256: [u8; 32],
    pub declared_checks: Vec<DeclaredCheckResult>,
    connection: Connection,
    _snapshot: NamedTempFile,
}

#[derive(Clone)]
pub struct VerificationControl {
    deadline: Duration,
    cancelled: Arc<AtomicBool>,
    started: Instant,
    max_bytes: u64,
}

impl Default for VerificationControl {
    fn default() -> Self {
        Self {
            deadline: Duration::from_secs(30),
            cancelled: Arc::new(AtomicBool::new(false)),
            started: Instant::now(),
            max_bytes: MAX_CAPSULE_BYTES,
        }
    }
}

impl VerificationControl {
    pub fn new(deadline: Duration, cancelled: Arc<AtomicBool>) -> Self {
        Self {
            deadline: deadline.min(Duration::from_secs(3_600)),
            cancelled,
            started: Instant::now(),
            max_bytes: MAX_CAPSULE_BYTES,
        }
    }

    pub fn with_max_bytes(mut self, max_bytes: u64) -> Self {
        self.max_bytes = max_bytes.min(MAX_CAPSULE_BYTES);
        self
    }

    pub fn check(&self) -> Result<(), LaunchError> {
        if self.cancelled.load(Ordering::Relaxed) {
            Err(LaunchError::Cancelled)
        } else if self.deadline.is_zero() || self.started.elapsed() >= self.deadline {
            Err(LaunchError::LimitExceeded)
        } else {
            Ok(())
        }
    }

    fn remaining(&self) -> Duration {
        self.deadline.saturating_sub(self.started.elapsed())
    }
}

pub struct VerificationGuard {
    stop: Option<mpsc::Sender<()>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl VerificationGuard {
    fn start(connection: &Connection, control: &VerificationControl) -> Self {
        let interrupt = connection.get_interrupt_handle();
        let cancelled = Arc::clone(&control.cancelled);
        let deadline = Instant::now()
            .checked_add(control.remaining())
            .unwrap_or_else(Instant::now);
        let (stop, receiver) = mpsc::channel();
        let worker = thread::spawn(move || {
            loop {
                if cancelled.load(Ordering::Relaxed) || Instant::now() >= deadline {
                    interrupt.interrupt();
                    break;
                }
                let remaining = deadline.saturating_duration_since(Instant::now());
                match receiver.recv_timeout(remaining.min(Duration::from_millis(10))) {
                    Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
        });
        Self {
            stop: Some(stop),
            worker: Some(worker),
        }
    }
}

impl Drop for VerificationGuard {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl VerifiedReadOnlyCapsule {
    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    pub fn assert_source_current(&self) -> Result<(), LaunchError> {
        self.assert_source_current_with_control(&VerificationControl::default())
    }

    pub fn assert_source_current_with_control(
        &self,
        control: &VerificationControl,
    ) -> Result<(), LaunchError> {
        reject_source_sidecars(&self.identity.canonical_path)?;
        if file_sha256_controlled(&self.identity.canonical_path, control)? == self.source_sha256 {
            Ok(())
        } else {
            Err(LaunchError::SourceRace)
        }
    }

    /// Keep the same absolute caller deadline active across post-verification
    /// signature and signed-contract queries on this exact snapshot.
    pub fn start_control(
        &self,
        control: &VerificationControl,
    ) -> Result<VerificationGuard, LaunchError> {
        control.check()?;
        Ok(VerificationGuard::start(&self.connection, control))
    }

    /// Copy the exact private standalone snapshot into an already-opened empty
    /// caller-owned file. The method refuses non-empty handles before writing
    /// and never truncates, so it cannot be used to overwrite an input capsule.
    pub fn copy_snapshot_to_file(&self, destination: &mut File) -> Result<u64, LaunchError> {
        self.copy_snapshot_to_file_with_control(
            destination,
            &VerificationControl::default(),
            MAX_CAPSULE_BYTES,
        )
    }

    /// Copy the exact snapshot to an empty private output while enforcing the
    /// caller's absolute verification budget and byte ceiling.
    pub fn copy_snapshot_to_file_with_control(
        &self,
        destination: &mut File,
        control: &VerificationControl,
        max_bytes: u64,
    ) -> Result<u64, LaunchError> {
        control.check()?;
        if destination.metadata()?.len() != 0 {
            return Err(LaunchError::SourceRace);
        }
        let mut source = self._snapshot.as_file().try_clone()?;
        source.seek(SeekFrom::Start(0))?;
        destination.seek(SeekFrom::Start(0))?;
        let ceiling = max_bytes.min(MAX_CAPSULE_BYTES);
        let mut copied = 0_u64;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            control.check()?;
            let read = source.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            copied = copied
                .checked_add(read as u64)
                .ok_or(LaunchError::LimitExceeded)?;
            if copied > ceiling {
                return Err(LaunchError::LimitExceeded);
            }
            destination.write_all(&buffer[..read])?;
        }
        destination.flush()?;
        Ok(copied)
    }
}

pub fn inspect_launch(path: &Path) -> Result<LaunchInspection, LaunchError> {
    inspect_launch_retained(path).map(RetainedLaunchInspection::into_inspection)
}

/// Inspect a capsule and retain the exact verified private snapshot for
/// subsequent host-owned, read-only metadata projections.
pub fn inspect_launch_retained(path: &Path) -> Result<RetainedLaunchInspection, LaunchError> {
    let control = VerificationControl::default();
    let verified = verify_read_only_with_control(path, &control)?;
    let _guard = verified.start_control(&control)?;
    let identity = &verified.identity;
    let connection = verified.connection();
    let publisher_present = has_table(connection, "capsule_publisher")?;
    let signature_present = has_table(connection, "capsule_signature")?;
    if publisher_present != signature_present {
        return Err(LaunchError::PartialExtension);
    }
    let (application_digest, publisher, signatures) = if publisher_present {
        let publisher = publisher_identity(connection)?;
        let digest = application_digest(connection)?;
        let envelopes = signature_inventory(connection)?;
        let reports = verify_signatures(connection)?;
        if envelopes.len() != reports.len() {
            return Err(LaunchError::SignatureRace);
        }
        let mut signatures = Vec::with_capacity(envelopes.len());
        for (envelope, report) in envelopes.into_iter().zip(reports) {
            if envelope.key_id != report.key_id {
                return Err(LaunchError::SignatureRace);
            }
            signatures.push(SignatureEvidence {
                key_id: envelope.key_id,
                public_key: envelope.public_key,
                cryptographically_valid: report.cryptographically_valid,
                digest_matches: report.digest_matches,
            });
        }
        (
            Some(digest),
            Some(PublisherEvidence {
                publisher_id: publisher.publisher_id,
                publisher_name: publisher.publisher_name,
            }),
            signatures,
        )
    } else {
        (None, None, Vec::new())
    };

    let declarations = identity
        .permissions
        .as_object()
        .ok_or(LaunchError::Permissions)?;
    let mut requested_capabilities = BTreeSet::new();
    let mut required_capabilities = BTreeSet::new();
    for (capability, declaration) in declarations {
        if capability == "network"
            && declaration.get("value").and_then(serde_json::Value::as_str) == Some("none")
        {
            continue;
        }
        requested_capabilities.insert(capability.clone());
        if declaration
            .get("required")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        {
            required_capabilities.insert(capability.clone());
        }
    }
    let evidence = LaunchEvidence {
        structure_verified: true,
        capsule_id: identity.capsule_id.clone(),
        application_id: identity.app_id.clone(),
        source_sha256: verified.source_sha256,
        application_digest,
        publisher,
        signatures,
        requested_capabilities,
        required_capabilities,
    };
    verified.assert_source_current_with_control(&control)?;
    let inspection = LaunchInspection {
        identity: verified.identity.clone(),
        evidence,
    };
    Ok(RetainedLaunchInspection {
        inspection,
        verified,
    })
}

pub fn verify_structure(path: &Path) -> Result<CapsuleIdentity, LaunchError> {
    let control = VerificationControl::default();
    let (identity, _connection, _snapshot, source_sha256, _guard) =
        open_conforming(path, &control)?;
    if file_sha256_controlled(&identity.canonical_path, &control)? != source_sha256 {
        return Err(LaunchError::SourceRace);
    }
    Ok(identity)
}

/// Open one read-only connection and carry the exact manifest identity,
/// exhaustive conformance, declared checks, and signature-compartment shape
/// through it. Before/after hashes detect replacement or mutation while those
/// phases run; callers that continue using the connection can recheck with
/// `assert_source_current`.
pub fn verify_read_only(path: &Path) -> Result<VerifiedReadOnlyCapsule, LaunchError> {
    verify_read_only_with_control(path, &VerificationControl::default())
}

pub fn verify_read_only_with_control(
    path: &Path,
    control: &VerificationControl,
) -> Result<VerifiedReadOnlyCapsule, LaunchError> {
    control.check()?;
    let (identity, connection, snapshot, source_sha256, _guard) = open_conforming(path, control)?;
    let declared_checks = controlled(control, conformance::run_declared_checks(&connection))?;
    reject_error_check_failures(&declared_checks)?;
    if file_sha256_controlled(&identity.canonical_path, control)? != source_sha256 {
        return Err(LaunchError::SourceRace);
    }
    Ok(VerifiedReadOnlyCapsule {
        identity,
        source_sha256,
        declared_checks,
        connection,
        _snapshot: snapshot,
    })
}

fn open_conforming(
    path: &Path,
    control: &VerificationControl,
) -> Result<
    (
        CapsuleIdentity,
        Connection,
        NamedTempFile,
        [u8; 32],
        VerificationGuard,
    ),
    LaunchError,
> {
    let header = inspect_header(path)?;
    if header.bytes > control.max_bytes {
        return Err(LaunchError::LimitExceeded);
    }
    let (mut snapshot, source_sha256) =
        capture_private_snapshot(&header.canonical_path, header.bytes, control)?;
    let (connection, guard) = open_read_only(snapshot.path(), control)?;
    let identity = controlled(control, inspect_metadata_connection(header, &connection))?;
    controlled(
        control,
        verify_conformance_connection(&connection, &identity),
    )?;
    // Keep both the private bytes and their read transaction alive for every
    // subsequent declared-check/signature phase.
    snapshot.as_file_mut().seek(SeekFrom::Start(0))?;
    Ok((identity, connection, snapshot, source_sha256, guard))
}

/// Verify the complete non-executing v0.2 conformance phase on an already
/// opened capsule connection. This validates declared endpoint and check SQL by
/// compiling it through a deny-by-default authorizer; the declarations are not
/// executed. Callers that will release assets must still run declared checks,
/// evaluate signatures, and apply policy as separate phases.
pub fn verify_conformance_connection(
    connection: &Connection,
    identity: &CapsuleIdentity,
) -> Result<(), LaunchError> {
    conformance::verify(connection, identity)?;
    verify_signature_compartment(connection)
}

/// Execute the already-conformance-checked capsule's declared validation
/// queries under the bounded read-only check phase. Error-severity failures
/// reject the capsule; warnings and informational results remain reportable.
pub fn verify_declared_checks(path: &Path) -> Result<Vec<DeclaredCheckResult>, LaunchError> {
    Ok(verify_read_only(path)?.declared_checks)
}

fn reject_error_check_failures(results: &[DeclaredCheckResult]) -> Result<(), LaunchError> {
    let failures: Vec<_> = results
        .iter()
        .filter(|result| !result.passed && result.severity == "error")
        .map(|result| format!("check {} failed: {}", result.id, result.detail))
        .collect();
    if failures.is_empty() {
        Ok(())
    } else {
        Err(LaunchError::Structure(format!(
            "declared checks failed: {}",
            failures.join(" | ")
        )))
    }
}

/// Run declared checks on an exact already-opened connection. The caller must
/// first run `verify_conformance_connection` on the same connection.
pub fn run_declared_checks_connection(
    connection: &Connection,
) -> Result<Vec<DeclaredCheckResult>, LaunchError> {
    conformance::run_declared_checks(connection)
}

/// Execute and enforce error-severity declared checks on the caller's exact
/// already-conformance-checked SQLite read snapshot.
pub fn verify_declared_checks_connection(
    connection: &Connection,
) -> Result<Vec<DeclaredCheckResult>, LaunchError> {
    let results = run_declared_checks_connection(connection)?;
    reject_error_check_failures(&results)?;
    Ok(results)
}

fn verify_signature_compartment(connection: &Connection) -> Result<(), LaunchError> {
    let publisher_present = has_table(connection, "capsule_publisher")?;
    let signature_present = has_table(connection, "capsule_signature")?;
    if publisher_present != signature_present {
        return Err(LaunchError::PartialExtension);
    }
    if publisher_present {
        publisher_identity(connection)?;
        application_digest(connection)?;
        signature_inventory(connection)?;
    }
    Ok(())
}

fn file_sha256_controlled(
    path: &Path,
    control: &VerificationControl,
) -> Result<[u8; 32], LaunchError> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    let mut total = 0_u64;
    loop {
        control.check()?;
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        total = total.saturating_add(count as u64);
        if total > MAX_CAPSULE_BYTES {
            return Err(InspectError::SizePolicy.into());
        }
        digest.update(&buffer[..count]);
    }
    if total == 0 {
        return Err(InspectError::SizePolicy.into());
    }
    Ok(digest.finalize().into())
}

/// Rebind an already-reviewed private-snapshot digest to the live standalone
/// main database. Callers must hold their SQLite read transaction while using
/// this check so an ordinary SQLite writer cannot commit between the digest
/// comparison and logical verification.
pub fn assert_source_binding(path: &Path, expected: &[u8; 32]) -> Result<(), LaunchError> {
    assert_source_binding_with_control(path, expected, &VerificationControl::default())
}

pub fn assert_source_binding_with_control(
    path: &Path,
    expected: &[u8; 32],
    control: &VerificationControl,
) -> Result<(), LaunchError> {
    reject_source_sidecars(path)?;
    if &file_sha256_controlled(path, control)? == expected {
        Ok(())
    } else {
        Err(LaunchError::SourceRace)
    }
}

fn capture_private_snapshot(
    path: &Path,
    expected_bytes: u64,
    control: &VerificationControl,
) -> Result<(NamedTempFile, [u8; 32]), LaunchError> {
    control.check()?;
    if expected_bytes == 0 || expected_bytes > MAX_CAPSULE_BYTES {
        return Err(InspectError::SizePolicy.into());
    }
    reject_source_sidecars(path)?;
    let mut source = File::open(path)?;
    let source_before = source.metadata()?;
    if !source_before.is_file() || source_before.len() != expected_bytes {
        return Err(LaunchError::SourceRace);
    }
    let mut sqlite_header = [0_u8; 20];
    source.read_exact(&mut sqlite_header)?;
    if sqlite_header[18] != 1 || sqlite_header[19] != 1 {
        return Err(LaunchError::SourceSidecar);
    }
    source.seek(SeekFrom::Start(0))?;
    let mut snapshot = NamedTempFile::new()?;
    let mut digest = Sha256::new();
    let mut copied = 0_u64;
    let mut buffer = vec![0_u8; 1024 * 1024];
    while copied < expected_bytes {
        control.check()?;
        let remaining = usize::try_from((expected_bytes - copied).min(buffer.len() as u64))
            .expect("bounded read size fits usize");
        let count = source.read(&mut buffer[..remaining])?;
        if count == 0 {
            return Err(LaunchError::SourceRace);
        }
        snapshot.as_file_mut().write_all(&buffer[..count])?;
        digest.update(&buffer[..count]);
        copied += count as u64;
    }
    let mut extra = [0_u8; 1];
    if source.read(&mut extra)? != 0 {
        return Err(InspectError::SizePolicy.into());
    }
    if source.metadata()?.len() != expected_bytes {
        return Err(LaunchError::SourceRace);
    }
    snapshot.as_file_mut().flush()?;
    snapshot.as_file_mut().sync_all()?;
    capture_post_copy_hook();
    if snapshot.as_file().metadata()?.len() != expected_bytes {
        return Err(LaunchError::SourceRace);
    }
    let snapshot_sha256: [u8; 32] = digest.finalize().into();
    reject_source_sidecars(path)?;
    reject_wal_header(path)?;
    if std::fs::metadata(path)?.len() != expected_bytes {
        return Err(LaunchError::SourceRace);
    }
    let after = file_sha256_controlled(path, control)?;
    if after != snapshot_sha256 {
        return Err(LaunchError::SourceRace);
    }
    Ok((snapshot, snapshot_sha256))
}

fn reject_wal_header(path: &Path) -> Result<(), LaunchError> {
    let mut header = [0_u8; 20];
    File::open(path)?.read_exact(&mut header)?;
    if header[18] == 1 && header[19] == 1 {
        Ok(())
    } else {
        Err(LaunchError::SourceSidecar)
    }
}

fn reject_source_sidecars(path: &Path) -> Result<(), LaunchError> {
    let file_name = path.file_name().ok_or(LaunchError::SourceSidecar)?;
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut name = OsString::from(file_name);
        name.push(suffix);
        let candidate: PathBuf = path.with_file_name(name);
        if candidate.try_exists()? {
            return Err(LaunchError::SourceSidecar);
        }
    }
    Ok(())
}

fn open_read_only(
    path: &Path,
    control: &VerificationControl,
) -> Result<(Connection, VerificationGuard), LaunchError> {
    control.check()?;
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.load_extension_disable()?;
    connection.set_limit(Limit::SQLITE_LIMIT_LENGTH, 16 * 1024 * 1024)?;
    connection.set_limit(Limit::SQLITE_LIMIT_SQL_LENGTH, 1024 * 1024)?;
    connection.set_limit(Limit::SQLITE_LIMIT_COLUMN, 256)?;
    connection.set_limit(Limit::SQLITE_LIMIT_EXPR_DEPTH, 100)?;
    connection.set_limit(Limit::SQLITE_LIMIT_COMPOUND_SELECT, 50)?;
    connection.set_limit(Limit::SQLITE_LIMIT_VDBE_OP, 1_000_000)?;
    connection.set_limit(Limit::SQLITE_LIMIT_FUNCTION_ARG, 64)?;
    connection.set_limit(Limit::SQLITE_LIMIT_ATTACHED, 0)?;
    connection.set_limit(Limit::SQLITE_LIMIT_VARIABLE_NUMBER, 128)?;
    connection.set_limit(Limit::SQLITE_LIMIT_TRIGGER_DEPTH, 32)?;
    connection.set_limit(Limit::SQLITE_LIMIT_WORKER_THREADS, 0)?;
    let guard = VerificationGuard::start(&connection, control);
    controlled(
        control,
        connection.execute_batch(
            "PRAGMA trusted_schema=OFF; PRAGMA foreign_keys=ON; PRAGMA query_only=ON; \
         PRAGMA busy_timeout=5000; BEGIN; SELECT count(*) FROM sqlite_schema;",
        ),
    )?;
    Ok((connection, guard))
}

fn controlled<T, E>(control: &VerificationControl, result: Result<T, E>) -> Result<T, LaunchError>
where
    E: Into<LaunchError>,
{
    match result {
        Ok(value) => {
            control.check()?;
            Ok(value)
        }
        Err(error) => {
            control.check()?;
            Err(error.into())
        }
    }
}

fn has_table(connection: &Connection, name: &str) -> rusqlite::Result<bool> {
    Ok(connection
        .query_row(
            "SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1",
            [name],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use super::*;

    static TEST_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

    struct TestCapsule(PathBuf);

    impl TestCapsule {
        fn path(name: &str) -> PathBuf {
            let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            std::env::temp_dir().join(format!(
                "sqlite-capsule-launch-{name}-{}-{sequence}.sqlitecapsule",
                std::process::id()
            ))
        }

        fn copied(name: &str) -> Self {
            let path = Self::path(name);
            fs::copy(checked_capsule(), &path).expect("copy checked capsule");
            Self(path)
        }

        fn v03(name: &str) -> Self {
            let path = Self::path(name);
            let connection = Connection::open(&path).expect("create v0.3 capsule");
            connection
                .execute_batch(include_str!("../../../../format/capsule-v0.3.sql"))
                .expect("create v0.3 format");
            connection
                .execute_batch(include_str!(
                    "../../../../format/capsule-signed-app-v0.3.sql"
                ))
                .expect("create v0.3 signed-app extension");
            connection
                .execute_batch(include_str!(
                    "../../../../compatibility/signed-app-v0.3/fixture-v0.3.sql"
                ))
                .expect("seed v0.3 fixture");
            drop(connection);
            Self(path)
        }
    }

    impl Drop for TestCapsule {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    fn checked_capsule() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .join("capsules/diagram-studio.capsule.sqlite")
    }

    #[test]
    fn checked_capsule_produces_unsigned_bounded_launch_evidence() {
        let path = checked_capsule();
        let inspection = inspect_launch(&path).expect("checked capsule evidence");
        assert_eq!(
            inspection.identity.app_id,
            "org.sqlite-capsule.diagram-studio"
        );
        assert!(inspection.evidence.publisher.is_none());
        assert!(
            inspection
                .evidence
                .required_capabilities
                .contains("database.read")
        );
        assert!(
            inspection
                .evidence
                .required_capabilities
                .contains("database.write")
        );
        assert!(
            !inspection
                .evidence
                .requested_capabilities
                .contains("network")
        );
    }

    #[test]
    fn retained_inspection_projects_only_from_the_verified_read_only_snapshot() {
        let capsule = TestCapsule::v03("retained-snapshot");
        let retained =
            inspect_launch_retained(&capsule.0).expect("retain verified launch snapshot");
        assert_eq!(retained.inspection().identity.format_version, "0.3");
        let application_name: String = retained.project_snapshot(|connection| {
            connection
                .query_row(
                    "SELECT name FROM capsule_application WHERE id = 1",
                    [],
                    |row| row.get(0),
                )
                .expect("read application name from retained snapshot")
        });
        assert_eq!(application_name, "Café Vector");

        let mutation = retained.project_snapshot(|connection| {
            connection.execute(
                "UPDATE capsule_application SET name = 'mutated' WHERE id = 1",
                [],
            )
        });
        assert!(
            mutation.is_err(),
            "retained snapshot must remain query-only"
        );
        retained
            .assert_source_current()
            .expect("source is still the inspected capsule");
    }

    #[test]
    fn retained_inspection_keeps_launch_evidence_bound_to_its_snapshot() {
        let capsule = TestCapsule::v03("retained-evidence");
        let retained =
            inspect_launch_retained(&capsule.0).expect("retain verified launch snapshot");
        assert!(retained.inspection().evidence.structure_verified);
        assert_eq!(
            retained.inspection().evidence.source_sha256,
            retained.verified.source_sha256
        );
        assert!(retained.inspection().evidence.application_digest.is_some());
    }

    #[test]
    fn exhaustive_verifier_rejects_each_non_executing_conformance_surface() {
        let cases = [
            (
                "asset",
                "UPDATE capsule_asset SET sha256 = printf('%064d', 0) \
                 WHERE path = (SELECT path FROM capsule_asset ORDER BY path LIMIT 1)",
                "hash mismatch",
            ),
            (
                "permission",
                "UPDATE capsule_manifest SET permissions_json = '{}' WHERE id = 1",
                "is not declared by database.",
            ),
            (
                "endpoint",
                "UPDATE capsule_endpoint \
                 SET operation = 'read', sql_text = 'SELECT :undeclared', \
                     parameters_json = '{}', result_mode = 'scalar' \
                 WHERE name = (SELECT name FROM capsule_endpoint \
                   WHERE name NOT IN (SELECT endpoint_name FROM capsule_endpoint_step) \
                   ORDER BY name LIMIT 1)",
                "parameters do not match SQL placeholders",
            ),
            (
                "check",
                "UPDATE capsule_check SET sql_text = 'DELETE FROM diagram_document' \
                 WHERE id = (SELECT id FROM capsule_check ORDER BY id LIMIT 1)",
                "not one read-only statement",
            ),
            (
                "forbidden-schema",
                "CREATE TRIGGER forbidden_capsule_trigger AFTER UPDATE ON capsule_manifest \
                 BEGIN SELECT 1; END",
                "triggers are forbidden",
            ),
        ];
        for (name, mutation, expected) in cases {
            let capsule = TestCapsule::copied(name);
            let connection = Connection::open(&capsule.0).expect("open mutable fixture");
            connection.execute_batch(mutation).expect("mutate fixture");
            drop(connection);
            let error = verify_structure(&capsule.0).expect_err("conformance rejection");
            assert!(
                matches!(&error, LaunchError::Structure(message) if message.contains(expected)),
                "{name} produced unexpected error: {error}"
            );
        }
    }

    #[test]
    fn exhaustive_verifier_rejects_an_invalid_later_v03_endpoint_step() {
        let capsule = TestCapsule::v03("invalid-later-step");
        let connection = Connection::open(&capsule.0).expect("open mutable v0.3 fixture");
        connection
            .execute(
                "UPDATE capsule_endpoint_step \
                 SET sql_text = 'UPDATE capsule_manifest SET app_version = app_version' \
                 WHERE endpoint_name = 'vector.write' AND sequence = 2",
                [],
            )
            .expect("mutate later v0.3 endpoint step");
        drop(connection);

        let error = verify_structure(&capsule.0).expect_err("later step must be rejected");
        assert!(
            matches!(&error, LaunchError::Structure(message)
                if message.contains("endpoint vector.write does not compile")),
            "unexpected v0.3 later-step error: {error}"
        );
    }

    #[test]
    fn declared_check_results_are_a_separate_bounded_launch_phase() {
        let capsule = TestCapsule::copied("failing-declared-check");
        let connection = Connection::open(&capsule.0).expect("open mutable fixture");
        connection
            .execute(
                "UPDATE capsule_check \
                 SET severity = 'error', sql_text = ?1, \
                     result_mode = 'scalar', expected_json = ?2 \
                 WHERE id = (SELECT id FROM capsule_check ORDER BY id LIMIT 1)",
                rusqlite::params![
                    "SELECT 'actual-private-row-value'",
                    "\"expected-private-row-value\""
                ],
            )
            .expect("make declared check fail");
        drop(connection);

        verify_structure(&capsule.0).expect("declaration remains conformant");
        let (_, connection, _, _, _guard) =
            open_conforming(&capsule.0, &VerificationControl::default())
                .expect("open conforming fixture");
        let results = conformance::run_declared_checks(&connection).expect("run declared checks");
        let failure = results
            .iter()
            .find(|result| !result.passed && result.severity == "error")
            .expect("error check failure");
        assert_eq!(
            failure.detail,
            "result did not match the declared expectation"
        );
        let error = inspect_launch(&capsule.0).expect_err("launch requires passing error checks");
        let message = error.to_string();
        assert!(
            matches!(&error, LaunchError::Structure(message) if message.contains("declared checks failed")),
            "unexpected launch error: {error}"
        );
        assert!(!message.contains("actual-private-row-value"));
        assert!(!message.contains("expected-private-row-value"));
    }

    #[test]
    fn verified_connection_detects_a_source_change_before_handoff() {
        let capsule = TestCapsule::copied("source-race");
        let verified = verify_read_only(&capsule.0).expect("verify source");
        let connection = Connection::open(&capsule.0).expect("open external writer");
        connection
            .execute(
                "UPDATE diagram_document SET updated_at = updated_at || 'Z' \
                 WHERE id = (SELECT id FROM diagram_document ORDER BY id LIMIT 1)",
                [],
            )
            .expect("mutate source after verification");
        drop(connection);
        assert!(matches!(
            verified.assert_source_current(),
            Err(LaunchError::SourceRace)
        ));
    }

    #[test]
    fn source_capture_rejects_adjacent_sidecars_and_wal_header_state() {
        let capsule = TestCapsule::copied("sidecar-source");
        let mut sidecar_name = capsule.0.as_os_str().to_os_string();
        sidecar_name.push("-wal");
        let sidecar = PathBuf::from(sidecar_name);
        fs::write(&sidecar, b"unreviewed WAL state").expect("create hostile sidecar");
        assert!(matches!(
            verify_read_only(&capsule.0),
            Err(LaunchError::SourceSidecar)
        ));
        fs::remove_file(&sidecar).expect("remove test sidecar");

        let connection = Connection::open(&capsule.0).expect("open WAL fixture");
        let mode: String = connection
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .expect("read journal mode");
        assert_eq!(mode, "delete");
        let changed: String = connection
            .query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))
            .expect("switch fixture to WAL");
        assert_eq!(changed, "wal");
        drop(connection);
        assert!(matches!(
            verify_read_only(&capsule.0),
            Err(LaunchError::SourceSidecar)
        ));
        for suffix in ["-wal", "-shm"] {
            let mut name = capsule.0.as_os_str().to_os_string();
            name.push(suffix);
            let _ = fs::remove_file(PathBuf::from(name));
        }
    }

    #[test]
    fn source_capture_rejects_growth_after_the_header_probe() {
        let capsule = TestCapsule::copied("grown-after-header");
        let expected_bytes = fs::metadata(&capsule.0).expect("fixture metadata").len();
        let mut source = std::fs::OpenOptions::new()
            .append(true)
            .open(&capsule.0)
            .expect("open fixture for growth");
        source.write_all(&[0]).expect("grow fixture by one byte");
        source.sync_all().expect("sync hostile growth");
        drop(source);

        assert!(matches!(
            capture_private_snapshot(&capsule.0, expected_bytes, &VerificationControl::default()),
            Err(LaunchError::SourceRace)
        ));
    }

    #[test]
    fn source_capture_rejects_change_capture_restore_aba() {
        let capsule = TestCapsule::copied("capture-restore-aba");
        let original = fs::read(&capsule.0).expect("original source bytes");
        let mut transient = original.clone();
        *transient.last_mut().expect("source has bytes") ^= 1;
        fs::write(&capsule.0, &transient).expect("install transient same-size state");
        let restore_path = capsule.0.clone();
        let restore_bytes = original.clone();
        CAPTURE_POST_COPY_HOOK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                fs::write(&restore_path, &restore_bytes).expect("restore reviewed source bytes");
            }));
        });
        assert!(matches!(
            capture_private_snapshot(
                &capsule.0,
                original.len() as u64,
                &VerificationControl::default()
            ),
            Err(LaunchError::SourceRace)
        ));
        assert_eq!(fs::read(&capsule.0).unwrap(), original);
    }
}

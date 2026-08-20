//! Duplicate-only verification typestate.
//!
//! Unlike [`crate::VerifiedWorkspaceSource`], this source does not require a
//! signed v0.3 data contract and cannot be used for semantic transformations.
//! It admits exhaustively verified v0.2/v0.3 capsules for exact or compact
//! duplication, provided an optional signed-app extension is wholly valid.

use std::{fmt, fs::File, path::Path};

use serde::Serialize;
use sqlite_capsule_crypto::{signature_inventory, verify_signatures};
use sqlite_capsule_launch::{
    LaunchError, VerificationControl, VerifiedReadOnlyCapsule, verify_read_only_with_control,
};
use sqlite_capsule_lifecycle::{LifecycleError, PinnedSource};

use crate::{
    CancellationToken, EffectiveLimits, WorkspaceControl, WorkspaceError, WorkspaceErrorCode,
    WorkspaceLimits,
};

pub const COPY_SOURCE_PROFILE: &str = "org.sqlite-capsule.copy-source/1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CopySourceSignatureState {
    Unsigned,
    SignedValid,
}

/// Bounded review identity. It intentionally contains no path, connection,
/// publisher-trust conclusion or execution capability.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CopySourceIdentity {
    pub profile: &'static str,
    pub format_version: String,
    pub capsule_id: String,
    pub revision_id: Option<String>,
    pub app_id: String,
    pub app_version: String,
    pub data_schema_id: Option<String>,
    pub data_schema_version: Option<u64>,
    pub size_bytes: u64,
    pub file_sha256: String,
    pub signature_state: CopySourceSignatureState,
    pub signature_count: u8,
    pub application_digest: Option<String>,
}

/// Pinned, exhaustively verified source authority for exact/compact duplicate
/// preparation only.
///
/// The exact private snapshot and one caller-owned absolute deadline/cancel
/// control are retained. No connection, live path or semantic data contract is
/// exposed.
pub struct VerifiedCopySource {
    pinned: PinnedSource,
    verified: VerifiedReadOnlyCapsule,
    control: VerificationControl,
    identity: CopySourceIdentity,
}

impl fmt::Debug for VerifiedCopySource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VerifiedCopySource")
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}

impl VerifiedCopySource {
    pub fn open(path: &Path) -> Result<Self, WorkspaceError> {
        Self::open_with_control(path, &WorkspaceLimits::default(), &CancellationToken::new())
    }

    pub fn open_with_control(
        path: &Path,
        limits: &WorkspaceLimits,
        cancellation: &CancellationToken,
    ) -> Result<Self, WorkspaceError> {
        let (limits, deadline) = EffectiveLimits::from_caller(limits)?;
        let workspace_control = WorkspaceControl::new(deadline, cancellation);
        workspace_control.check()?;
        let control =
            VerificationControl::new(workspace_control.remaining()?, cancellation.shared_flag())
                .with_max_bytes(limits.max_capsule_bytes);
        let pinned = PinnedSource::open(path, false).map_err(map_pin_error)?;
        control.check().map_err(map_launch_error)?;
        let verified = verify_read_only_with_control(pinned.canonical_path(), &control)
            .map_err(map_launch_error)?;
        pinned.assert_current().map_err(|_| stale_plan())?;
        verified
            .assert_source_current_with_control(&control)
            .map_err(map_launch_error)?;

        let (signature_state, signature_count, application_digest) =
            verify_complete_signature_inventory(&verified, &control)?;
        control.check().map_err(map_launch_error)?;
        pinned.assert_current().map_err(|_| stale_plan())?;
        verified
            .assert_source_current_with_control(&control)
            .map_err(map_launch_error)?;

        let capsule = &verified.identity;
        let data_schema = capsule.overview.data_schema.as_ref();
        let identity = CopySourceIdentity {
            profile: COPY_SOURCE_PROFILE,
            format_version: capsule.format_version.clone(),
            capsule_id: capsule.capsule_id.clone(),
            revision_id: capsule.overview.instance.revision_id.clone(),
            app_id: capsule.app_id.clone(),
            app_version: capsule.app_version.clone(),
            data_schema_id: data_schema.map(|schema| schema.data_schema_id.clone()),
            data_schema_version: data_schema
                .and_then(|schema| u64::try_from(schema.data_schema_version).ok()),
            size_bytes: pinned.identity().bytes,
            file_sha256: lower_hex(&verified.source_sha256),
            signature_state,
            signature_count,
            application_digest,
        };
        Ok(Self {
            pinned,
            verified,
            control,
            identity,
        })
    }

    pub fn identity(&self) -> &CopySourceIdentity {
        &self.identity
    }

    /// Rebinds both the pinned filesystem object and its complete byte digest
    /// under the original absolute operation deadline.
    pub fn assert_current(&self) -> Result<(), WorkspaceError> {
        self.control.check().map_err(map_launch_error)?;
        self.pinned.assert_current().map_err(|_| stale_plan())?;
        self.verified
            .assert_source_current_with_control(&self.control)
            .map_err(map_launch_error)
    }

    /// Requires this freshly pinned duplicate source to be the exact file
    /// object observed by the trusted Overview, not merely byte-identical
    /// replacement content at the same path.
    pub fn assert_source_binding(
        &self,
        expected: &sqlite_capsule_lifecycle::SourceIdentity,
    ) -> Result<(), WorkspaceError> {
        self.assert_current()?;
        if self.pinned.identity() == expected {
            Ok(())
        } else {
            Err(stale_plan())
        }
    }

    pub(crate) fn source_identity(&self) -> &sqlite_capsule_lifecycle::SourceIdentity {
        self.pinned.identity()
    }

    pub(crate) fn canonical_path(&self) -> &Path {
        self.pinned.canonical_path()
    }

    pub(crate) fn verified_connection(&self) -> &rusqlite::Connection {
        self.verified.connection()
    }

    pub(crate) fn verification_control(&self) -> &VerificationControl {
        &self.control
    }

    pub(crate) fn start_control(
        &self,
        control: &VerificationControl,
    ) -> Result<sqlite_capsule_launch::VerificationGuard, WorkspaceError> {
        self.verified
            .start_control(control)
            .map_err(map_launch_error)
    }

    pub(crate) fn assert_current_with_control(
        &self,
        control: &VerificationControl,
    ) -> Result<(), WorkspaceError> {
        self.control.check().map_err(map_launch_error)?;
        control.check().map_err(map_launch_error)?;
        self.pinned.assert_current().map_err(|_| stale_plan())?;
        self.verified
            .assert_source_current_with_control(control)
            .map_err(map_launch_error)
    }

    pub(crate) fn copy_exact_snapshot_to_file_with_control(
        &self,
        destination: &mut File,
        control: &VerificationControl,
        max_bytes: u64,
    ) -> Result<u64, WorkspaceError> {
        self.assert_current_with_control(control)?;
        let copied = self
            .verified
            .copy_snapshot_to_file_with_control(destination, control, max_bytes)
            .map_err(map_launch_error)?;
        self.assert_current_with_control(control)?;
        Ok(copied)
    }
}

fn verify_complete_signature_inventory(
    verified: &VerifiedReadOnlyCapsule,
    control: &VerificationControl,
) -> Result<(CopySourceSignatureState, u8, Option<String>), WorkspaceError> {
    let connection = verified.connection();
    let publisher_present = has_table(connection, "capsule_publisher")?;
    let signature_present = has_table(connection, "capsule_signature")?;
    if !publisher_present && !signature_present {
        return Ok((CopySourceSignatureState::Unsigned, 0, None));
    }
    if publisher_present != signature_present {
        return Err(invalid_signature());
    }

    let _guard = verified.start_control(control).map_err(map_launch_error)?;
    let inventory = signature_inventory(connection).map_err(|_| invalid_signature())?;
    if inventory.is_empty() {
        return Err(invalid_signature());
    }
    let reports = verify_signatures(connection).map_err(|_| invalid_signature())?;
    if inventory.len() != reports.len()
        || inventory.iter().zip(&reports).any(|(envelope, report)| {
            envelope.key_id != report.key_id
                || !report.cryptographically_valid
                || !report.digest_matches
        })
    {
        return Err(invalid_signature());
    }
    let count = u8::try_from(inventory.len()).map_err(|_| invalid_signature())?;
    let digest = lower_hex(&inventory[0].application_digest);
    Ok((CopySourceSignatureState::SignedValid, count, Some(digest)))
}

fn has_table(connection: &rusqlite::Connection, name: &str) -> Result<bool, WorkspaceError> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1)",
            [name],
            |row| row.get(0),
        )
        .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InvalidCapsule))
}

fn map_pin_error(error: LifecycleError) -> WorkspaceError {
    match error {
        LifecycleError::ChangedDuringOpen | LifecycleError::Replaced => stale_plan(),
        LifecycleError::NotRegularFile | LifecycleError::SymbolicLink => {
            WorkspaceError::new(WorkspaceErrorCode::InvalidCapsule)
        }
        _ => WorkspaceError::new(WorkspaceErrorCode::InternalError),
    }
}

fn map_launch_error(error: LaunchError) -> WorkspaceError {
    match error {
        LaunchError::PartialExtension | LaunchError::Crypto(_) | LaunchError::SignatureRace => {
            invalid_signature()
        }
        LaunchError::SourceRace => stale_plan(),
        LaunchError::SourceSidecar => {
            WorkspaceError::new(WorkspaceErrorCode::SourceJournalStateUnsupported)
        }
        LaunchError::Cancelled => WorkspaceError::new(WorkspaceErrorCode::Cancelled),
        LaunchError::LimitExceeded => WorkspaceError::new(WorkspaceErrorCode::LimitExceeded),
        LaunchError::Inspect(sqlite_capsule_core::InspectError::UnsupportedFormat { .. }) => {
            WorkspaceError::new(WorkspaceErrorCode::UnsupportedFormat)
        }
        _ => WorkspaceError::new(WorkspaceErrorCode::InvalidCapsule),
    }
}

fn lower_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

const fn invalid_signature() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::InvalidSignature)
}

const fn stale_plan() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::StalePlan)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::{ffi::OsString, fs};

    use ed25519_dalek::SigningKey;
    use rusqlite::Connection;
    use sha2::{Digest, Sha256};
    use sqlite_capsule_crypto::{
        PROFILE, PROFILE_V03, application_digest, sign_digest_for_profile,
    };

    use super::*;

    const V02_SCHEMA: &str = include_str!("../../../../format/capsule-v0.2.sql");
    const V02_SIGNED_SCHEMA: &str = include_str!("../../../../format/capsule-signed-app-v0.2.sql");
    const V02_FIXTURE: &str =
        include_str!("../../../../compatibility/signed-app-v0.2/fixture-v0.2.sql");
    const V03_SCHEMA: &str = include_str!("../../../../format/capsule-v0.3.sql");
    const V03_SIGNED_SCHEMA: &str = include_str!("../../../../format/capsule-signed-app-v0.3.sql");
    const V03_FIXTURE: &str =
        include_str!("../../../../compatibility/signed-app-v0.3/fixture-v0.3.sql");
    const DEVELOPMENT_SEED: &str =
        include_str!("../../../../compatibility/signed-app-v0.2/development-seed.hex");

    pub(crate) fn fixture(
        name: &str,
        version: u8,
        signed: bool,
    ) -> (tempfile::TempDir, std::path::PathBuf) {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join(format!("{name}.sqlitecapsule"));
        let connection = Connection::open(&path).expect("create fixture");
        let (schema, signed_schema, data) = match version {
            2 => (V02_SCHEMA, V02_SIGNED_SCHEMA, V02_FIXTURE),
            3 => (V03_SCHEMA, V03_SIGNED_SCHEMA, V03_FIXTURE),
            _ => panic!("unsupported test version"),
        };
        connection.execute_batch(schema).expect("base schema");
        connection
            .execute_batch(signed_schema)
            .expect("signed schema");
        connection.execute_batch(data).expect("fixture data");
        if version == 2 {
            normalize_v02_launch_fixture(&connection);
        }
        refresh_asset_hashes(&connection);
        if signed {
            resign(&connection, version);
        } else {
            connection
                .execute_batch("DROP TABLE capsule_signature; DROP TABLE capsule_publisher;")
                .expect("remove complete signed extension");
        }
        drop(connection);
        sqlite_capsule_launch::verify_read_only(&path)
            .unwrap_or_else(|error| panic!("copy fixture must exhaustively verify: {error:?}"));
        (directory, path)
    }

    fn normalize_v02_launch_fixture(connection: &Connection) {
        connection
            .execute_batch(
                 "INSERT INTO capsule_asset \
                 (path, media_type, content, sha256, executable, cache_policy, description) \
                 VALUES ('app/index.html', 'text/html', X'3c68746d6c3e3c2f68746d6c3e', \
                         '0000000000000000000000000000000000000000000000000000000000000000', \
                         1, 'no-store', 'Executable test entry'); \
                 UPDATE capsule_manifest SET entry_asset = 'app/index.html', \
                    permissions_json = '{\"database.read\":{\"required\":true},\"database.write\":{\"required\":true},\"network\":{\"value\":\"none\"}}'; \
                 DELETE FROM capsule_asset WHERE path <> 'app/index.html'; \
                 UPDATE capsule_command SET argv_json = '[\"vector\",\"å\"]'; \
                 UPDATE capsule_runbook SET sequence = 1; \
                 UPDATE capsule_doc SET sequence = 0; \
                 UPDATE capsule_endpoint SET \
                    sql_text = 'UPDATE vector_domain SET note = :value WHERE id = ''domain''', \
                    parameters_json = \
                    '{\"value\":{\"type\":\"string\",\"required\":true}}'; \
                 DELETE FROM capsule_endpoint_step; \
                 INSERT INTO capsule_endpoint_step \
                    (endpoint_name, sequence, sql_text, required_changes) VALUES \
                    ('vector.write', 1, \
                     'UPDATE vector_domain SET note = :value WHERE id = ''domain''', 1), \
                    ('vector.write', 2, \
                     'UPDATE vector_domain SET note = note WHERE id = ''domain''', 1); \
                 UPDATE capsule_check SET expected_json = '1';",
            )
            .expect("normalize v0.2 launch fixture");
    }

    fn refresh_asset_hashes(connection: &Connection) {
        let assets = {
            let mut statement = connection
                .prepare("SELECT path, content FROM capsule_asset ORDER BY path")
                .expect("asset query");
            statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
                })
                .expect("asset rows")
                .collect::<Result<Vec<_>, _>>()
                .expect("asset values")
        };
        for (path, content) in assets {
            let digest = Sha256::digest(content);
            let hex = digest
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            connection
                .execute(
                    "UPDATE capsule_asset SET sha256 = ?1 WHERE path = ?2",
                    [&hex, &path],
                )
                .expect("refresh asset hash");
        }
    }

    fn resign(connection: &Connection, version: u8) {
        connection
            .execute("DELETE FROM capsule_signature", [])
            .expect("remove fixture signature");
        let digest = application_digest(connection).expect("application digest");
        let key = development_key();
        let profile = if version == 2 { PROFILE } else { PROFILE_V03 };
        let envelope = sign_digest_for_profile(&key, digest, "2026-08-08T12:34:56Z", profile)
            .expect("sign copy fixture");
        connection
            .execute(
                "INSERT INTO capsule_signature \
                 (key_id, algorithm, public_key, application_digest, signature, signed_at) \
                 VALUES (?1, 'ed25519', ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    envelope.key_id,
                    envelope.public_key.as_slice(),
                    envelope.application_digest.as_slice(),
                    envelope.signature.as_slice(),
                    envelope.signed_at,
                ],
            )
            .expect("store fixture signature");
    }

    fn development_key() -> SigningKey {
        let seed_text = DEVELOPMENT_SEED.trim();
        let mut seed = [0_u8; 32];
        for (index, byte) in seed.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&seed_text[index * 2..index * 2 + 2], 16)
                .expect("development seed hex");
        }
        let key = SigningKey::from_bytes(&seed);
        seed.fill(0);
        key
    }

    fn sha256(path: &Path) -> Vec<u8> {
        Sha256::digest(fs::read(path).expect("fixture bytes")).to_vec()
    }

    #[test]
    fn accepts_unsigned_v02_and_v03_without_synthesizing_signature_trust() {
        for version in [2, 3] {
            let (_directory, path) = fixture(&format!("unsigned-v0{version}"), version, false);
            let before = sha256(&path);
            let source = VerifiedCopySource::open(&path).expect("unsigned copy source");
            assert_eq!(
                source.identity().format_version,
                if version == 2 { "0.2" } else { "0.3" }
            );
            assert_eq!(
                source.identity().signature_state,
                CopySourceSignatureState::Unsigned
            );
            assert_eq!(source.identity().signature_count, 0);
            assert_eq!(source.identity().application_digest, None);
            assert_eq!(sha256(&path), before);
        }
    }

    #[test]
    fn accepts_only_complete_valid_signed_v02_and_v03_inventories() {
        for version in [2, 3] {
            let (_directory, path) = fixture(&format!("signed-v0{version}"), version, true);
            let source = VerifiedCopySource::open(&path).expect("signed copy source");
            assert_eq!(
                source.identity().signature_state,
                CopySourceSignatureState::SignedValid
            );
            assert_eq!(source.identity().signature_count, 1);
            assert_eq!(
                source
                    .identity()
                    .application_digest
                    .as_deref()
                    .expect("signed digest")
                    .len(),
                64
            );
        }
    }

    #[test]
    fn invalid_empty_and_mixed_signature_inventories_fail_closed() {
        for mutation in [
            "UPDATE capsule_signature SET signature = zeroblob(64);",
            "DELETE FROM capsule_signature;",
            "INSERT INTO capsule_signature \
             (key_id, algorithm, public_key, application_digest, signature, signed_at) \
             SELECT 'ed25519:sha256:0000000000000000000000000000000000000000000000000000000000000000', \
                    algorithm, zeroblob(32), \
                    application_digest, zeroblob(64), signed_at \
             FROM capsule_signature LIMIT 1;",
        ] {
            let (_directory, path) = fixture("invalid-signature", 3, true);
            let connection = Connection::open(&path).expect("mutate signature inventory");
            connection
                .execute_batch(mutation)
                .expect("signature mutation");
            drop(connection);
            assert_eq!(
                VerifiedCopySource::open(&path)
                    .expect_err("invalid inventory")
                    .kind(),
                WorkspaceErrorCode::InvalidSignature
            );
        }
    }

    #[test]
    fn partial_signed_extensions_are_rejected() {
        let (_directory, path) = fixture("partial-extension", 2, true);
        let connection = Connection::open(&path).expect("mutate extension");
        connection
            .execute_batch("DROP TABLE capsule_signature;")
            .expect("make partial extension");
        drop(connection);
        assert_eq!(
            VerifiedCopySource::open(&path)
                .expect_err("partial signed extension")
                .kind(),
            WorkspaceErrorCode::InvalidSignature
        );
    }

    #[test]
    fn digest_mismatch_and_malformed_envelope_schema_are_rejected() {
        let (_directory, path) = fixture("digest-mismatch", 3, true);
        let connection = Connection::open(&path).expect("mutate envelope digest");
        let envelope = sign_digest_for_profile(
            &development_key(),
            [0x44; 32],
            "2026-08-08T12:34:56Z",
            PROFILE_V03,
        )
        .expect("valid envelope over wrong digest");
        connection
            .execute(
                "UPDATE capsule_signature SET key_id = ?1, public_key = ?2, \
                 application_digest = ?3, signature = ?4, signed_at = ?5",
                rusqlite::params![
                    envelope.key_id,
                    envelope.public_key.as_slice(),
                    envelope.application_digest.as_slice(),
                    envelope.signature.as_slice(),
                    envelope.signed_at,
                ],
            )
            .expect("store digest-mismatching valid envelope");
        drop(connection);
        assert_eq!(
            VerifiedCopySource::open(&path)
                .expect_err("digest mismatch")
                .kind(),
            WorkspaceErrorCode::InvalidSignature
        );

        let (_directory, path) = fixture("malformed-envelope", 2, true);
        let connection = Connection::open(&path).expect("mutate envelope schema");
        connection
            .execute_batch("ALTER TABLE capsule_signature RENAME COLUMN algorithm TO scheme;")
            .expect("malformed signature table");
        drop(connection);
        assert_eq!(
            VerifiedCopySource::open(&path)
                .expect_err("malformed envelope schema")
                .kind(),
            WorkspaceErrorCode::InvalidSignature
        );
    }

    #[test]
    fn adjacent_sidecars_fail_before_copy_admission() {
        let (_directory, path) = fixture("copy-sidecar", 3, false);
        let mut sidecar = OsString::from(path.as_os_str());
        sidecar.push("-wal");
        fs::write(&sidecar, b"untrusted sidecar").expect("sidecar fixture");
        assert_eq!(
            VerifiedCopySource::open(&path)
                .expect_err("sidecar must fail")
                .kind(),
            WorkspaceErrorCode::SourceJournalStateUnsupported
        );
    }

    #[test]
    fn live_source_change_after_review_is_stale_and_private_snapshot_is_not_reopened() {
        let (_directory, path) = fixture("copy-source-race", 3, false);
        let source = VerifiedCopySource::open(&path).expect("verified source");
        let connection = Connection::open(&path).expect("external writer");
        connection
            .execute(
                "UPDATE vector_domain SET note = 'changed' WHERE id = 'domain'",
                [],
            )
            .expect("mutate live source");
        drop(connection);
        assert_eq!(
            source.assert_current().expect_err("stale source").kind(),
            WorkspaceErrorCode::StalePlan
        );
    }

    #[test]
    fn one_absolute_cancellation_control_survives_admission() {
        let (_directory, path) = fixture("copy-cancel", 3, false);
        let cancellation = CancellationToken::new();
        let source = VerifiedCopySource::open_with_control(
            &path,
            &WorkspaceLimits::default(),
            &cancellation,
        )
        .expect("verified source");
        cancellation.cancel();
        assert_eq!(
            source
                .assert_current()
                .expect_err("cancelled rebind")
                .kind(),
            WorkspaceErrorCode::Cancelled
        );
    }
}

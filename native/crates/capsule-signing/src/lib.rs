//! Safe publisher signing for SQLite Capsules.
//!
//! This crate owns the product-independent file workflow shared by the native
//! CLI and trusted desktop shell: verify a source, copy it to a private
//! same-directory temporary file, prepare the signed application compartment,
//! sign the exact reviewed digest, verify the result, and publish only to a new
//! destination. Private keys are loaded into Rust memory only and are never
//! written into a capsule.

use std::{
    fmt, fs,
    fs::File,
    io::{Read, Seek},
    path::{Path, PathBuf},
};

use ed25519_dalek::{SigningKey, pkcs8::DecodePrivateKey};
use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};
use sqlite_capsule_core::InspectError;
use sqlite_capsule_crypto::{
    ALGORITHM, CryptoError, application_digest, key_id, publisher_identity,
    sign_digest_for_profile, signed_app_profile, validate_signed_at, verify_signatures,
};
use sqlite_capsule_launch::{LaunchError, verify_read_only};
use tempfile::{Builder as TemporaryBuilder, NamedTempFile};
use thiserror::Error;

const SIGNED_SCHEMA_V02: &str = include_str!("../../../../format/capsule-signed-app-v0.2.sql");
const SIGNED_SCHEMA_V03: &str = include_str!("../../../../format/capsule-signed-app-v0.3.sql");
pub const MAX_SIGNING_KEY_FILE_BYTES: u64 = 64 * 1024;

#[derive(Debug, Error)]
pub enum SigningError {
    #[error("signing key file is empty, oversized, or not a regular file")]
    KeyFilePolicy,
    #[error("encrypted PKCS#8 keys are not supported by the use-once signer")]
    EncryptedKeyUnsupported,
    #[error("signing key must be a 32-byte seed, 64 hexadecimal digits, or Ed25519 PKCS#8 PEM/DER")]
    KeyEncoding,
    #[error("publisher id and name must each contain 1 to 512 characters")]
    PublisherIdentity,
    #[error("refusing to replace an existing output")]
    ExistingOutput,
    #[error("refusing in-place signing")]
    InPlace,
    #[error("output must have an existing parent directory")]
    OutputParent,
    #[error("partial signed-app extension is not accepted")]
    PartialExtension,
    #[error("existing signed publisher identity does not match")]
    PublisherMismatch,
    #[error("prepared application digest changed before signing")]
    PreparedDigestChanged,
    #[error("finished output failed signature verification")]
    SignatureVerification,
    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("capsule inspection failed: {0}")]
    Inspect(#[from] InspectError),
    #[error("capsule verification failed: {0}")]
    Launch(#[from] LaunchError),
    #[error("capsule signature failed: {0}")]
    Crypto(#[from] CryptoError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SigningKeyFormat {
    RawSeed,
    HexSeed,
    Pkcs8Pem,
    Pkcs8Der,
}

impl SigningKeyFormat {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RawSeed => "raw-seed",
            Self::HexSeed => "hex-seed",
            Self::Pkcs8Pem => "pkcs8-pem",
            Self::Pkcs8Der => "pkcs8-der",
        }
    }
}

impl fmt::Display for SigningKeyFormat {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SigningKeyInfo {
    pub format: SigningKeyFormat,
    pub key_id: String,
    pub public_key_hex: String,
}

pub struct LoadedSigningKey {
    signing_key: SigningKey,
    info: SigningKeyInfo,
}

impl LoadedSigningKey {
    pub fn from_file(path: &Path) -> Result<Self, SigningError> {
        let mut file = File::open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_SIGNING_KEY_FILE_BYTES
        {
            return Err(SigningError::KeyFilePolicy);
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        file.by_ref()
            .take(MAX_SIGNING_KEY_FILE_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.is_empty() || bytes.len() as u64 > MAX_SIGNING_KEY_FILE_BYTES {
            bytes.fill(0);
            return Err(SigningError::KeyFilePolicy);
        }
        let parsed = parse_key_bytes(&bytes);
        bytes.fill(0);
        let (signing_key, format) = parsed?;
        let public_key = signing_key.verifying_key().to_bytes();
        let info = SigningKeyInfo {
            format,
            key_id: key_id(&public_key),
            public_key_hex: lower_hex(&public_key),
        };
        Ok(Self { signing_key, info })
    }

    pub fn info(&self) -> &SigningKeyInfo {
        &self.info
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SigningSource {
    pub canonical_path: PathBuf,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SigningPreview {
    pub source: SigningSource,
    pub output: PathBuf,
    pub publisher_id: String,
    pub publisher_name: String,
    pub application_digest: String,
    pub signed_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SigningReport {
    pub preview: SigningPreview,
    pub key: SigningKeyInfo,
    pub output_bytes: u64,
    pub output_sha256: String,
    pub signature_valid: bool,
    pub publisher_trusted: bool,
}

pub struct PreparedCapsule {
    preview: SigningPreview,
    application_digest: [u8; 32],
    temporary: NamedTempFile,
}

impl PreparedCapsule {
    pub fn preview(&self) -> &SigningPreview {
        &self.preview
    }

    pub fn sign(self, key: LoadedSigningKey) -> Result<SigningReport, SigningError> {
        if self.preview.output.exists() {
            return Err(SigningError::ExistingOutput);
        }
        let mut destination = Connection::open(self.temporary.path())?;
        destination.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA foreign_keys=ON;")?;
        let transaction = destination.transaction()?;
        let fresh_digest = application_digest(&transaction)?;
        if fresh_digest != self.application_digest {
            return Err(SigningError::PreparedDigestChanged);
        }
        let profile = signed_app_profile(&transaction)?.profile;
        let envelope = sign_digest_for_profile(
            &key.signing_key,
            fresh_digest,
            &self.preview.signed_at,
            profile,
        )?;
        transaction.execute(
            "INSERT INTO capsule_signature \
             (key_id, algorithm, public_key, application_digest, signature, signed_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                envelope.key_id,
                ALGORITHM,
                envelope.public_key.as_slice(),
                envelope.application_digest.as_slice(),
                envelope.signature.as_slice(),
                envelope.signed_at,
            ],
        )?;
        transaction.commit()?;
        drop(destination);

        let finished = verify_read_only(self.temporary.path())?;
        let reports = verify_signatures(finished.connection())?;
        let key_info = key.info.clone();
        if !reports.iter().any(|report| {
            report.key_id == key_info.key_id
                && report.cryptographically_valid
                && report.digest_matches
        }) {
            return Err(SigningError::SignatureVerification);
        }
        drop(finished);
        let mut published = match self.temporary.persist_noclobber(&self.preview.output) {
            Ok(file) => file,
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(SigningError::ExistingOutput);
            }
            Err(error) => return Err(SigningError::Io(error.error)),
        };
        published.sync_all()?;
        let output_bytes = published.metadata()?.len();
        let output_sha256 = file_sha256_handle(&mut published)?;
        let published_verification = verify_read_only(&self.preview.output)?;
        if lower_hex(&published_verification.source_sha256) != output_sha256 {
            return Err(SigningError::Launch(LaunchError::SourceRace));
        }
        drop(published_verification);
        drop(published);
        Ok(SigningReport {
            preview: self.preview.clone(),
            key: key_info,
            output_bytes,
            output_sha256,
            signature_valid: true,
            publisher_trusted: false,
        })
    }
}

pub fn inspect_signing_source(path: &Path) -> Result<SigningSource, SigningError> {
    let verified = verify_read_only(path)?;
    Ok(SigningSource {
        canonical_path: verified.identity.canonical_path.clone(),
        bytes: verified.identity.bytes,
        sha256: lower_hex(&verified.source_sha256),
    })
}

pub fn prepare_capsule_signing(
    source: &Path,
    output: &Path,
    publisher_id: &str,
    publisher_name: &str,
    signed_at: Option<&str>,
) -> Result<PreparedCapsule, SigningError> {
    validate_publisher(publisher_id, publisher_name)?;
    let verified_source = verify_read_only(source)?;
    let source = SigningSource {
        canonical_path: verified_source.identity.canonical_path.clone(),
        bytes: verified_source.identity.bytes,
        sha256: lower_hex(&verified_source.source_sha256),
    };
    let output = validate_output_path(&source.canonical_path, output)?;
    let mut temporary = temporary_file(&output)?;
    verified_source.copy_snapshot_to_file(temporary.as_file_mut())?;
    verified_source.assert_source_current()?;
    let snapshot_sha256 = file_sha256(temporary.path())?;
    if snapshot_sha256 != source.sha256 {
        return Err(SigningError::Launch(LaunchError::SourceRace));
    }
    // Verify the exact private byte snapshot before adding signed-extension
    // rows. This catches incomplete SQLite sidecar state and binds signing to
    // the reviewed source hash.
    verify_read_only(temporary.path())?;
    let mut destination = Connection::open(temporary.path())?;
    destination.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA foreign_keys=ON;")?;
    let transaction = destination.transaction()?;
    let publisher_present = has_table(&transaction, "capsule_publisher")?;
    let signature_present = has_table(&transaction, "capsule_signature")?;
    if publisher_present != signature_present {
        return Err(SigningError::PartialExtension);
    }
    let profile = signed_app_profile(&transaction)?;
    if !publisher_present {
        let signed_schema = match profile.user_version {
            2 => SIGNED_SCHEMA_V02,
            3 => SIGNED_SCHEMA_V03,
            _ => return Err(SigningError::Crypto(CryptoError::UnsupportedFormat)),
        };
        transaction.execute_batch(signed_schema)?;
        transaction.execute(
            "INSERT INTO capsule_publisher \
             (id, profile, publisher_id, publisher_name) VALUES (1, ?1, ?2, ?3)",
            params![profile.profile, publisher_id, publisher_name],
        )?;
    } else {
        let publisher = publisher_identity(&transaction)?;
        if publisher.publisher_id != publisher_id || publisher.publisher_name != publisher_name {
            return Err(SigningError::PublisherMismatch);
        }
    }
    let signed_at = match signed_at {
        Some(value) => value.to_owned(),
        None => {
            transaction.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%SZ', 'now')", [], |row| {
                row.get(0)
            })?
        }
    };
    let digest = application_digest(&transaction)?;
    validate_signed_at(&signed_at)?;
    transaction.commit()?;
    drop(destination);
    verify_read_only(temporary.path())?;

    let preview = SigningPreview {
        source,
        output,
        publisher_id: publisher_id.to_owned(),
        publisher_name: publisher_name.to_owned(),
        application_digest: lower_hex(&digest),
        signed_at,
    };
    Ok(PreparedCapsule {
        preview,
        application_digest: digest,
        temporary,
    })
}

pub fn sign_capsule_from_file(
    source: &Path,
    output: &Path,
    publisher_id: &str,
    publisher_name: &str,
    key_path: &Path,
    signed_at: &str,
) -> Result<SigningReport, SigningError> {
    let key = LoadedSigningKey::from_file(key_path)?;
    prepare_capsule_signing(
        source,
        output,
        publisher_id,
        publisher_name,
        Some(signed_at),
    )?
    .sign(key)
}

fn parse_key_bytes(bytes: &[u8]) -> Result<(SigningKey, SigningKeyFormat), SigningError> {
    if bytes.len() == 32 {
        let mut seed: [u8; 32] = bytes.try_into().map_err(|_| SigningError::KeyEncoding)?;
        let key = SigningKey::from_bytes(&seed);
        seed.fill(0);
        return Ok((key, SigningKeyFormat::RawSeed));
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        let text = text.trim();
        if text.starts_with("-----BEGIN ENCRYPTED PRIVATE KEY-----") {
            return Err(SigningError::EncryptedKeyUnsupported);
        }
        if text.starts_with("-----BEGIN PRIVATE KEY-----") {
            return SigningKey::from_pkcs8_pem(text)
                .map(|key| (key, SigningKeyFormat::Pkcs8Pem))
                .map_err(|_| SigningError::KeyEncoding);
        }
        if text.len() == 64 {
            let mut seed = decode_hex_32(text)?;
            let key = SigningKey::from_bytes(&seed);
            seed.fill(0);
            return Ok((key, SigningKeyFormat::HexSeed));
        }
    }
    SigningKey::from_pkcs8_der(bytes)
        .map(|key| (key, SigningKeyFormat::Pkcs8Der))
        .map_err(|_| SigningError::KeyEncoding)
}

fn decode_hex_32(value: &str) -> Result<[u8; 32], SigningError> {
    let mut bytes = [0_u8; 32];
    for (index, chunk) in value.as_bytes().chunks_exact(2).enumerate() {
        let high = hex_value(chunk[0]).ok_or(SigningError::KeyEncoding)?;
        let low = hex_value(chunk[1]).ok_or(SigningError::KeyEncoding)?;
        bytes[index] = (high << 4) | low;
    }
    Ok(bytes)
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn validate_publisher(publisher_id: &str, publisher_name: &str) -> Result<(), SigningError> {
    if publisher_id.is_empty()
        || publisher_id.len() > 512
        || publisher_name.is_empty()
        || publisher_name.len() > 512
    {
        return Err(SigningError::PublisherIdentity);
    }
    Ok(())
}

fn validate_output_path(source: &Path, output: &Path) -> Result<PathBuf, SigningError> {
    if output.exists() {
        return Err(SigningError::ExistingOutput);
    }
    let parent = output.parent().ok_or(SigningError::OutputParent)?;
    if !parent.is_dir() {
        return Err(SigningError::OutputParent);
    }
    if fs::symlink_metadata(parent)?.file_type().is_symlink() {
        return Err(SigningError::OutputParent);
    }
    let parent = fs::canonicalize(parent)?;
    let filename = output
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or(SigningError::OutputParent)?;
    if filename.is_empty()
        || filename.contains(':')
        || filename.contains('/')
        || filename.contains('\\')
    {
        return Err(SigningError::OutputParent);
    }
    let normalized = parent.join(filename);
    if source == normalized {
        return Err(SigningError::InPlace);
    }
    Ok(normalized)
}

fn temporary_file(output: &Path) -> Result<NamedTempFile, SigningError> {
    let parent = output.parent().ok_or(SigningError::OutputParent)?;
    let filename = output
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or(SigningError::OutputParent)?;
    TemporaryBuilder::new()
        .prefix(&format!(".{filename}.signing-"))
        .suffix(".tmp")
        .tempfile_in(parent)
        .map_err(SigningError::Io)
}

fn has_table(connection: &Connection, name: &str) -> rusqlite::Result<bool> {
    connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1)",
        [name],
        |row| row.get(0),
    )
}

fn file_sha256(path: &Path) -> Result<String, SigningError> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(lower_hex(&digest.finalize()))
}

fn file_sha256_handle(file: &mut File) -> Result<String, SigningError> {
    file.seek(std::io::SeekFrom::Start(0))?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(lower_hex(&digest.finalize()))
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use std::{
        env,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use ed25519_dalek::pkcs8::EncodePrivateKey;
    use pkcs8::LineEnding;

    use super::*;

    static TEST_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(name: &str) -> Self {
            let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = env::temp_dir().join(format!(
                "sqlite-capsule-signing-{name}-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir(&path).expect("create test directory");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn checked_capsule() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .join("capsules/diagram-studio.capsule.sqlite")
    }

    fn v03_capsule(directory: &TestDirectory) -> PathBuf {
        let path = directory.path().join("vector-v03.sqlitecapsule");
        let connection = Connection::open(&path).expect("create v0.3 fixture");
        connection
            .execute_batch(include_str!("../../../../format/capsule-v0.3.sql"))
            .expect("v0.3 schema");
        connection
            .execute_batch(include_str!(
                "../../../../format/capsule-signed-app-v0.3.sql"
            ))
            .expect("v0.3 signed extension");
        connection
            .execute_batch(include_str!(
                "../../../../compatibility/signed-app-v0.3/fixture-v0.3.sql"
            ))
            .expect("v0.3 fixture data");
        drop(connection);
        path
    }

    #[test]
    fn loads_raw_hex_and_pkcs8_keys_without_changing_identity() {
        let directory = TestDirectory::new("key-formats");
        let signing_key = SigningKey::from_bytes(&[42_u8; 32]);
        let expected = key_id(&signing_key.verifying_key().to_bytes());
        let raw = directory.path().join("key.seed");
        fs::write(&raw, [42_u8; 32]).expect("raw key");
        let hex = directory.path().join("key.hex");
        fs::write(&hex, "2a".repeat(32)).expect("hex key");
        let pem = directory.path().join("key.pem");
        fs::write(
            &pem,
            signing_key
                .to_pkcs8_pem(LineEnding::LF)
                .expect("PEM")
                .as_bytes(),
        )
        .expect("PEM key");
        let der = directory.path().join("key.der");
        fs::write(&der, signing_key.to_pkcs8_der().expect("DER").as_bytes()).expect("DER key");

        for (path, format) in [
            (raw, SigningKeyFormat::RawSeed),
            (hex, SigningKeyFormat::HexSeed),
            (pem, SigningKeyFormat::Pkcs8Pem),
            (der, SigningKeyFormat::Pkcs8Der),
        ] {
            let loaded = LoadedSigningKey::from_file(&path).expect("load key format");
            assert_eq!(loaded.info().format, format);
            assert_eq!(loaded.info().key_id, expected);
        }
    }

    #[test]
    fn prepares_exact_digest_then_signs_a_new_verified_copy() {
        let directory = TestDirectory::new("prepare-sign");
        let key_path = directory.path().join("publisher.seed");
        fs::write(&key_path, [42_u8; 32]).expect("key");
        let output = directory.path().join("signed.sqlitecapsule");
        let prepared = prepare_capsule_signing(
            &checked_capsule(),
            &output,
            "org.example.publisher",
            "Example Publisher",
            Some("2026-08-08T12:34:56Z"),
        )
        .expect("prepare");
        let preview = prepared.preview().clone();
        assert!(!output.exists());
        let report = prepared
            .sign(LoadedSigningKey::from_file(&key_path).expect("load key"))
            .expect("sign");
        assert_eq!(report.preview, preview);
        assert!(report.signature_valid);
        assert!(!report.publisher_trusted);
        assert!(output.is_file());
        assert_eq!(report.output_sha256, file_sha256(&output).expect("hash"));
    }

    #[test]
    fn signs_and_verifies_a_conformant_v03_capsule_without_mutating_the_source() {
        let directory = TestDirectory::new("v03-prepare-sign");
        let source = v03_capsule(&directory);
        let source_before = fs::read(&source).expect("v0.3 source bytes");
        let key_path = directory.path().join("publisher.seed");
        fs::write(&key_path, [43_u8; 32]).expect("key");
        let output = directory.path().join("signed-v03.sqlitecapsule");
        let report = prepare_capsule_signing(
            &source,
            &output,
            "org.example.vector",
            "Vector Publisher",
            Some("2026-08-08T12:34:56Z"),
        )
        .expect("prepare v0.3")
        .sign(LoadedSigningKey::from_file(&key_path).expect("load key"))
        .expect("sign v0.3");
        assert!(report.signature_valid);
        assert_eq!(
            source_before,
            fs::read(&source).expect("unchanged v0.3 source")
        );
        let verified = verify_read_only(&output).expect("verify v0.3 output");
        assert_eq!(verified.identity.user_version, 3);
        assert_eq!(
            verified.identity.overview.instance.revision_id.as_deref(),
            Some("22222222-2222-4222-8222-222222222222")
        );
        assert!(
            verify_signatures(verified.connection())
                .expect("v0.3 signatures")
                .iter()
                .any(|item| item.cryptographically_valid && item.digest_matches)
        );
    }

    #[test]
    fn prepared_copy_is_removed_when_signing_is_cancelled() {
        let directory = TestDirectory::new("cancel");
        let output = directory.path().join("signed.sqlitecapsule");
        let prepared = prepare_capsule_signing(
            &checked_capsule(),
            &output,
            "org.example.publisher",
            "Example Publisher",
            Some("2026-08-08T12:34:56Z"),
        )
        .expect("prepare");
        let temporary = prepared.temporary.path().to_path_buf();
        assert!(temporary.is_file());
        drop(prepared);
        assert!(!temporary.exists());
        assert!(!output.exists());
    }

    #[test]
    fn direct_signing_rejects_a_runtime_invalid_source_before_preparation() {
        let directory = TestDirectory::new("invalid-runtime-source");
        let source = directory.path().join("invalid.sqlitecapsule");
        fs::copy(checked_capsule(), &source).expect("copy source");
        let connection = Connection::open(&source).expect("open mutable source");
        connection
            .execute(
                "UPDATE capsule_asset SET sha256 = printf('%064d', 0) \
                 WHERE path = (SELECT path FROM capsule_asset ORDER BY path LIMIT 1)",
                [],
            )
            .expect("invalidate asset hash");
        drop(connection);

        let output = directory.path().join("must-not-exist.sqlitecapsule");
        let error = match prepare_capsule_signing(
            &source,
            &output,
            "org.example.publisher",
            "Example Publisher",
            Some("2026-08-08T12:34:56Z"),
        ) {
            Ok(_) => panic!("invalid source must not reach signing preparation"),
            Err(error) => error,
        };
        assert!(
            matches!(
                &error,
                SigningError::Launch(LaunchError::Structure(message))
                    if message.contains("hash mismatch")
            ),
            "unexpected signing error: {error}"
        );
        assert!(!output.exists());
    }

    #[test]
    fn direct_signing_rejects_a_failing_error_check_without_publishing() {
        let directory = TestDirectory::new("failing-declared-check");
        let source = directory.path().join("invalid-check.sqlitecapsule");
        fs::copy(checked_capsule(), &source).expect("copy source");
        let connection = Connection::open(&source).expect("open mutable source");
        connection
            .execute(
                "UPDATE capsule_check \
                 SET severity = 'error', sql_text = 'SELECT 1', \
                     result_mode = 'scalar', expected_json = '2' \
                 WHERE id = (SELECT id FROM capsule_check ORDER BY id LIMIT 1)",
                [],
            )
            .expect("make declared check fail");
        drop(connection);

        let assert_rejected = |output: &Path| {
            let error = match prepare_capsule_signing(
                &source,
                output,
                "org.example.publisher",
                "Example Publisher",
                Some("2026-08-08T12:34:56Z"),
            ) {
                Ok(_) => panic!("failing error check must not reach signing preparation"),
                Err(error) => error,
            };
            assert!(
                matches!(
                    &error,
                    SigningError::Launch(LaunchError::Structure(message))
                        if message.contains("declared checks failed")
                ),
                "unexpected signing error: {error}"
            );
        };

        let absent = directory.path().join("must-not-exist.sqlitecapsule");
        assert_rejected(&absent);
        assert!(!absent.exists());

        let existing = directory.path().join("preserve.sqlitecapsule");
        fs::write(&existing, b"preserve existing destination").expect("existing output");
        assert_rejected(&existing);
        assert_eq!(
            fs::read(existing).expect("preserved existing output"),
            b"preserve existing destination"
        );
    }

    #[test]
    fn signing_refuses_existing_or_in_place_output() {
        let directory = TestDirectory::new("refusal");
        let source = checked_capsule();
        assert!(matches!(
            prepare_capsule_signing(
                &source,
                &source,
                "org.example.publisher",
                "Example Publisher",
                Some("2026-08-08T12:34:56Z")
            ),
            Err(SigningError::ExistingOutput)
        ));
        let output = directory.path().join("existing.sqlitecapsule");
        fs::write(&output, b"preserve me").expect("existing output");
        assert!(matches!(
            prepare_capsule_signing(
                &source,
                &output,
                "org.example.publisher",
                "Example Publisher",
                Some("2026-08-08T12:34:56Z")
            ),
            Err(SigningError::ExistingOutput)
        ));
        assert_eq!(fs::read(output).expect("preserved"), b"preserve me");
    }

    #[test]
    fn signing_never_clobbers_an_output_created_after_review() {
        let directory = TestDirectory::new("late-output");
        let key_path = directory.path().join("publisher.seed");
        fs::write(&key_path, [42_u8; 32]).expect("key");
        let output = directory.path().join("signed.sqlitecapsule");
        let prepared = prepare_capsule_signing(
            &checked_capsule(),
            &output,
            "org.example.publisher",
            "Example Publisher",
            Some("2026-08-08T12:34:56Z"),
        )
        .expect("prepare");
        fs::write(&output, b"preserve late output").expect("late output");
        assert!(matches!(
            prepared.sign(LoadedSigningKey::from_file(&key_path).expect("load key")),
            Err(SigningError::ExistingOutput)
        ));
        assert_eq!(
            fs::read(output).expect("preserved late output"),
            b"preserve late output"
        );
    }

    #[test]
    fn rejects_encrypted_or_oversized_key_files() {
        let directory = TestDirectory::new("invalid-key");
        let encrypted = directory.path().join("encrypted.pem");
        fs::write(
            &encrypted,
            "-----BEGIN ENCRYPTED PRIVATE KEY-----\ninvalid\n-----END ENCRYPTED PRIVATE KEY-----\n",
        )
        .expect("encrypted fixture");
        assert!(matches!(
            LoadedSigningKey::from_file(&encrypted),
            Err(SigningError::EncryptedKeyUnsupported)
        ));
        let oversized = directory.path().join("oversized.key");
        fs::write(
            &oversized,
            vec![0_u8; MAX_SIGNING_KEY_FILE_BYTES as usize + 1],
        )
        .expect("oversized fixture");
        assert!(matches!(
            LoadedSigningKey::from_file(&oversized),
            Err(SigningError::KeyFilePolicy)
        ));
    }
}

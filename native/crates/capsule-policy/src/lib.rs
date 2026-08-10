//! Protected host-local trust state and capability policy.
//!
//! Capsule databases are never attached to this connection. Callers provide
//! already-bounded verification evidence, and receive a serialisable decision.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlite_capsule_distribution::VerifiedRevocationBundle;
use thiserror::Error;

const SCHEMA_VERSION: i64 = 2;
const SCHEMA_V1: &str = include_str!("schema-v1.sql");
const SCHEMA_V2: &str = include_str!("schema-v2.sql");
const MAX_AUDIT_ROWS: usize = 10_000;

pub const SUPPORTED_CAPABILITIES: &[&str] = &[
    "database.read",
    "database.write",
    "clipboard.read",
    "clipboard.write",
    "file.read.user-selected",
    "download",
    "fullscreen",
    "camera",
    "microphone",
];

const OS_MEDIATED_CAPABILITIES: &[&str] = &[
    "clipboard.read",
    "clipboard.write",
    "file.read.user-selected",
    "download",
    "fullscreen",
    "camera",
    "microphone",
];

#[derive(Debug, Error)]
pub enum PolicyError {
    #[error("trust-store schema is unsupported or malformed")]
    Schema,
    #[error("trust-store field is missing, malformed, or oversized")]
    Field,
    #[error("persistent allow-once grants are forbidden")]
    AllowOnce,
    #[error("refusing to replace an existing output")]
    ExistingOutput,
    #[error("trust-store backup failed verification")]
    Backup,
    #[error("revocation bundle sequence is not newer than last-known-good")]
    RevocationRollback,
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustState {
    Unverified,
    StructurallyVerifiedUnsigned,
    SignatureValidUnknownPublisher,
    SignedTrustedPublisher,
    LocallyTrustedExactRelease,
    ModifiedAfterSignature,
    InvalidSignature,
    DeniedByUser,
    Revoked,
}

impl TrustState {
    fn can_prompt(self) -> bool {
        matches!(
            self,
            Self::StructurallyVerifiedUnsigned
                | Self::SignatureValidUnknownPublisher
                | Self::SignedTrustedPublisher
                | Self::LocallyTrustedExactRelease
        )
    }

    fn is_trusted(self) -> bool {
        matches!(
            self,
            Self::SignedTrustedPublisher | Self::LocallyTrustedExactRelease
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityDecision {
    Allow,
    Prompt,
    Deny,
}

impl CapabilityDecision {
    fn intersection(self, other: Self) -> Self {
        use CapabilityDecision::{Allow, Deny, Prompt};
        match (self, other) {
            (Deny, _) | (_, Deny) => Deny,
            (Prompt, _) | (_, Prompt) => Prompt,
            (Allow, Allow) => Allow,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignatureEvidence {
    pub key_id: String,
    pub public_key: [u8; 32],
    pub cryptographically_valid: bool,
    pub digest_matches: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublisherEvidence {
    pub publisher_id: String,
    pub publisher_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchEvidence {
    pub structure_verified: bool,
    pub capsule_id: String,
    pub application_id: String,
    pub source_sha256: [u8; 32],
    pub application_digest: Option<[u8; 32]>,
    pub publisher: Option<PublisherEvidence>,
    pub signatures: Vec<SignatureEvidence>,
    pub requested_capabilities: BTreeSet<String>,
    pub required_capabilities: BTreeSet<String>,
}

#[derive(Clone, Debug, Default)]
pub struct EvaluationContext {
    pub host_policy: BTreeMap<String, CapabilityDecision>,
    pub operating_system_permission: BTreeMap<String, CapabilityDecision>,
    pub allow_once: BTreeSet<String>,
    pub trust_once: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityEvaluation {
    pub decision: CapabilityDecision,
    pub requested: bool,
    pub required: bool,
    pub supported: bool,
    pub persisted_grant: Option<CapabilityDecision>,
    pub allow_once: bool,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchDecision {
    pub trust_state: TrustState,
    pub signature_valid: bool,
    pub publisher_known: bool,
    pub publisher_trusted: bool,
    pub revocation_status: String,
    pub executable_allowed: bool,
    pub application_digest_hex: Option<String>,
    pub capabilities: BTreeMap<String, CapabilityEvaluation>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEvent {
    pub id: i64,
    pub occurred_at: String,
    pub severity: String,
    pub action: String,
    pub capsule_id: Option<String>,
    pub publisher_id: Option<String>,
    pub key_id: Option<String>,
    pub details: Value,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RevocationRootRecord {
    pub key_id: String,
    #[serde(skip_serializing)]
    pub public_key: [u8; 32],
    pub decision: String,
    pub bundle_sequence: i64,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ForgottenDecisionReport {
    pub local_exceptions: usize,
    pub exact_releases: usize,
    pub capability_grants: usize,
}

pub struct TrustStore {
    path: PathBuf,
    connection: Connection,
}

impl TrustStore {
    pub fn open(path: &Path) -> Result<Self, PolicyError> {
        let path = absolute_path(path)?;
        let existed = path.exists();
        if let Some(parent) = path.parent() {
            let parent_existed = parent.exists();
            fs::create_dir_all(parent)?;
            if !parent_existed {
                secure_directory(parent)?;
            }
        }
        let connection = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        connection.execute_batch(
            "PRAGMA trusted_schema=OFF; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;",
        )?;
        let mode: String = connection.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
        if !mode.eq_ignore_ascii_case("wal") {
            connection.pragma_update(None, "journal_mode", "WAL")?;
        }
        connection.pragma_update(None, "synchronous", "FULL")?;
        let mut store = Self { path, connection };
        store.migrate()?;
        secure_store_files(&store.path)?;
        if existed {
            store.validate_schema()?;
        }
        Ok(store)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn migrate(&mut self) -> Result<(), PolicyError> {
        let version: i64 = self
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))?;
        match version {
            0 => {
                let transaction = self
                    .connection
                    .transaction_with_behavior(TransactionBehavior::Immediate)?;
                transaction.execute_batch(SCHEMA_V1)?;
                transaction.execute_batch(SCHEMA_V2)?;
                transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
                transaction.commit()?;
            }
            1 => {
                let transaction = self
                    .connection
                    .transaction_with_behavior(TransactionBehavior::Immediate)?;
                transaction.execute_batch(SCHEMA_V2)?;
                transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
                transaction.commit()?;
            }
            SCHEMA_VERSION => {}
            _ => return Err(PolicyError::Schema),
        }
        self.validate_schema()
    }

    fn validate_schema(&self) -> Result<(), PolicyError> {
        let version: i64 = self
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))?;
        let meta: Option<(i64, i64)> = self
            .connection
            .query_row(
                "SELECT id, schema_version FROM trust_meta WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let integrity: String = self
            .connection
            .query_row("PRAGMA quick_check", [], |row| row.get(0))?;
        if version != SCHEMA_VERSION || meta != Some((1, SCHEMA_VERSION)) || integrity != "ok" {
            return Err(PolicyError::Schema);
        }
        Ok(())
    }

    pub fn backup_to(&self, output: &Path) -> Result<(), PolicyError> {
        if output.exists() {
            return Err(PolicyError::ExistingOutput);
        }
        let parent = output.parent().ok_or(PolicyError::Field)?;
        let parent_existed = parent.exists();
        fs::create_dir_all(parent)?;
        if !parent_existed {
            secure_directory(parent)?;
        }
        let mut guard = TemporaryFile::new(output.to_path_buf());
        self.connection.backup(rusqlite::MAIN_DB, output, None)?;
        let backup = Connection::open_with_flags(
            output,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let integrity: String = backup.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        let version: i64 = backup.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if integrity != "ok" || version != SCHEMA_VERSION {
            return Err(PolicyError::Backup);
        }
        drop(backup);
        secure_file(output)?;
        guard.keep();
        Ok(())
    }

    pub fn trust_publisher_key(
        &mut self,
        publisher: &PublisherEvidence,
        key_id: &str,
        public_key: &[u8; 32],
        reason: &str,
    ) -> Result<(), PolicyError> {
        validate_identity(publisher)?;
        validate_text(key_id, 1, 256)?;
        validate_text(reason, 1, 2048)?;
        let expected_key_id = format!("ed25519:sha256:{}", lower_hex(&Sha256::digest(public_key)));
        if key_id != expected_key_id {
            return Err(PolicyError::Field);
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "INSERT INTO publisher (publisher_id, publisher_name, status, created_at, updated_at) \
             VALUES (?1, ?2, 'active', strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), \
                     strftime('%Y-%m-%dT%H:%M:%SZ', 'now')) \
             ON CONFLICT(publisher_id) DO UPDATE SET publisher_name = excluded.publisher_name, \
                 updated_at = excluded.updated_at",
            params![publisher.publisher_id, publisher.publisher_name],
        )?;
        let existing: Option<(String, Vec<u8>)> = transaction
            .query_row(
                "SELECT publisher_id, public_key FROM publisher_key WHERE key_id = ?1",
                [key_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if existing
            .as_ref()
            .is_some_and(|(existing_publisher, existing_key)| {
                existing_publisher != &publisher.publisher_id
                    || existing_key.as_slice() != public_key
            })
        {
            return Err(PolicyError::Field);
        }
        transaction.execute(
            "INSERT INTO publisher_key \
             (key_id, publisher_id, public_key, decision, reason, decided_at) \
             VALUES (?1, ?2, ?3, 'trusted', ?4, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')) \
             ON CONFLICT(key_id) DO UPDATE SET decision = 'trusted', reason = excluded.reason, \
                 decided_at = excluded.decided_at",
            params![
                key_id,
                publisher.publisher_id,
                public_key.as_slice(),
                reason
            ],
        )?;
        insert_audit(
            &transaction,
            "security",
            "publisher_key.trust",
            None,
            Some(&publisher.publisher_id),
            Some(key_id),
            &json!({"reason": reason}),
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn install_revocation_bundle(
        &mut self,
        verified: &VerifiedRevocationBundle,
    ) -> Result<(), PolicyError> {
        let bundle = verified.bundle();
        let sequence = i64::try_from(bundle.sequence).map_err(|_| PolicyError::Field)?;
        let current: i64 = self.connection.query_row(
            "SELECT coalesce(max(sequence), 0) FROM revocation_bundle",
            [],
            |row| row.get(0),
        )?;
        if sequence <= current {
            return Err(PolicyError::RevocationRollback);
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "UPDATE revocation_bundle SET active = 0 WHERE active = 1",
            [],
        )?;
        transaction.execute(
            "INSERT INTO revocation_bundle \
             (sequence, issued_at, next_update, payload_digest, installed_at, active) \
             VALUES (?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), 1)",
            params![
                sequence,
                bundle.issued_at,
                bundle.next_update,
                verified.payload_digest().as_slice(),
            ],
        )?;
        transaction.execute("DELETE FROM remote_key_revocation", [])?;
        transaction.execute("DELETE FROM remote_release_revocation", [])?;
        transaction.execute("DELETE FROM revocation_root", [])?;
        for entry in &bundle.revoked_keys {
            transaction.execute(
                "INSERT INTO remote_key_revocation (key_id, bundle_sequence, reason) \
                 VALUES (?1, ?2, ?3)",
                params![entry.key_id, sequence, entry.reason],
            )?;
        }
        for entry in &bundle.revoked_releases {
            let digest = decode_lower_hex::<32>(&entry.application_digest_sha256)?;
            transaction.execute(
                "INSERT INTO remote_release_revocation \
                 (application_id, application_digest, bundle_sequence, reason) \
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    entry.application_id,
                    digest.as_slice(),
                    sequence,
                    entry.reason
                ],
            )?;
        }
        for entry in &bundle.emergency_roots {
            let public_key = decode_lower_hex::<32>(&entry.public_key_hex)?;
            let decision = match entry.action.as_str() {
                "delegate" => "delegated",
                "revoke" => "revoked",
                _ => return Err(PolicyError::Field),
            };
            transaction.execute(
                "INSERT INTO revocation_root \
                 (key_id, public_key, decision, bundle_sequence, reason) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    entry.key_id,
                    public_key.as_slice(),
                    decision,
                    sequence,
                    entry.reason,
                ],
            )?;
        }
        insert_audit(
            &transaction,
            "security",
            "revocation.install",
            None,
            None,
            None,
            &json!({
                "sequence": bundle.sequence,
                "issued_at": bundle.issued_at,
                "next_update": bundle.next_update,
                "payload_digest_sha256": lower_hex(verified.payload_digest()),
                "freshness_at_verification": verified.freshness(),
                "revoked_keys": bundle.revoked_keys.len(),
                "revoked_releases": bundle.revoked_releases.len(),
                "emergency_roots": bundle.emergency_roots.len(),
            }),
        )?;
        transaction.commit()?;
        secure_store_files(&self.path)?;
        Ok(())
    }

    pub fn revocation_roots(&self) -> Result<Vec<RevocationRootRecord>, PolicyError> {
        let mut statement = self.connection.prepare(
            "SELECT key_id, public_key, decision, bundle_sequence, reason \
             FROM revocation_root ORDER BY key_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;
        let mut roots = Vec::new();
        for row in rows {
            let (key_id, public_key, decision, bundle_sequence, reason) = row?;
            let public_key = public_key.try_into().map_err(|_| PolicyError::Schema)?;
            roots.push(RevocationRootRecord {
                key_id,
                public_key,
                decision,
                bundle_sequence,
                reason,
            });
        }
        Ok(roots)
    }

    pub fn revoke_publisher_key(&mut self, key_id: &str, reason: &str) -> Result<(), PolicyError> {
        validate_text(key_id, 1, 256)?;
        validate_text(reason, 1, 2048)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let publisher_id: String = transaction.query_row(
            "SELECT publisher_id FROM publisher_key WHERE key_id = ?1",
            [key_id],
            |row| row.get(0),
        )?;
        transaction.execute(
            "UPDATE publisher_key SET decision = 'revoked', reason = ?2, \
             decided_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE key_id = ?1",
            params![key_id, reason],
        )?;
        insert_audit(
            &transaction,
            "security",
            "publisher_key.revoke",
            None,
            Some(&publisher_id),
            Some(key_id),
            &json!({"reason": reason}),
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn trust_exact_release(
        &mut self,
        evidence: &LaunchEvidence,
        key_id: &str,
        reason: &str,
    ) -> Result<(), PolicyError> {
        self.set_exact_release_decision(evidence, key_id, "trusted", reason)
    }

    pub fn trust_exact_release_with_grants(
        &mut self,
        evidence: &LaunchEvidence,
        key_id: &str,
        grants: &BTreeMap<String, CapabilityDecision>,
        reason: &str,
    ) -> Result<(), PolicyError> {
        validate_evidence(evidence)?;
        validate_text(reason, 1, 2048)?;
        if grants.len() != evidence.requested_capabilities.len()
            || !grants
                .keys()
                .all(|capability| evidence.requested_capabilities.contains(capability))
        {
            return Err(PolicyError::Field);
        }
        let digest = evidence.application_digest.ok_or(PolicyError::Field)?;
        let publisher = evidence.publisher.as_ref().ok_or(PolicyError::Field)?;
        if !evidence.signatures.iter().any(|signature| {
            signature.key_id == key_id
                && signature.cryptographically_valid
                && signature.digest_matches
        }) {
            return Err(PolicyError::Field);
        }
        let prepared_grants = grants
            .iter()
            .map(|(capability, decision)| {
                validate_text(capability, 1, 128)?;
                let decision = match decision {
                    CapabilityDecision::Allow => "allow",
                    CapabilityDecision::Deny => "deny",
                    CapabilityDecision::Prompt => return Err(PolicyError::AllowOnce),
                };
                Ok((capability.as_str(), decision))
            })
            .collect::<Result<Vec<_>, PolicyError>>()?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "INSERT INTO exact_release \
             (capsule_id, application_id, application_digest, key_id, publisher_id, \
              decision, reason, decided_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, 'trusted', ?6, \
                     strftime('%Y-%m-%dT%H:%M:%SZ', 'now')) \
             ON CONFLICT(capsule_id, application_id, application_digest, key_id) \
             DO UPDATE SET decision = 'trusted', reason = excluded.reason, \
                 decided_at = excluded.decided_at",
            params![
                evidence.capsule_id,
                evidence.application_id,
                digest.as_slice(),
                key_id,
                publisher.publisher_id,
                reason,
            ],
        )?;
        insert_audit(
            &transaction,
            "security",
            "release.trust_exact",
            Some(&evidence.capsule_id),
            Some(&publisher.publisher_id),
            Some(key_id),
            &json!({
                "application_digest": lower_hex(&digest),
                "decision": "trusted",
                "reason": reason,
                "capabilities_in_same_transaction": prepared_grants.len()
            }),
        )?;
        for (capability, decision) in prepared_grants {
            transaction.execute(
                "INSERT INTO capability_grant \
                 (capsule_id, application_id, application_digest, capability, decision, reason, decided_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')) \
                 ON CONFLICT(capsule_id, application_id, application_digest, capability) \
                 DO UPDATE SET decision = excluded.decision, reason = excluded.reason, \
                     decided_at = excluded.decided_at",
                params![
                    evidence.capsule_id,
                    evidence.application_id,
                    digest.as_slice(),
                    capability,
                    decision,
                    reason,
                ],
            )?;
            insert_audit(
                &transaction,
                "security",
                "capability.persist",
                Some(&evidence.capsule_id),
                Some(&publisher.publisher_id),
                Some(key_id),
                &json!({
                    "application_digest": lower_hex(&digest),
                    "capability": capability,
                    "decision": decision,
                    "reason": reason,
                    "release_transaction": true
                }),
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn deny_exact_release(
        &mut self,
        evidence: &LaunchEvidence,
        key_id: &str,
        reason: &str,
    ) -> Result<(), PolicyError> {
        self.set_exact_release_decision(evidence, key_id, "denied", reason)
    }

    fn set_exact_release_decision(
        &mut self,
        evidence: &LaunchEvidence,
        key_id: &str,
        decision: &str,
        reason: &str,
    ) -> Result<(), PolicyError> {
        validate_evidence(evidence)?;
        if !matches!(decision, "trusted" | "denied") {
            return Err(PolicyError::Field);
        }
        validate_text(reason, 1, 2048)?;
        let digest = evidence.application_digest.ok_or(PolicyError::Field)?;
        let publisher = evidence.publisher.as_ref().ok_or(PolicyError::Field)?;
        if !evidence.signatures.iter().any(|signature| {
            signature.key_id == key_id
                && signature.cryptographically_valid
                && signature.digest_matches
        }) {
            return Err(PolicyError::Field);
        }
        self.connection.execute(
            "INSERT INTO exact_release \
             (capsule_id, application_id, application_digest, key_id, publisher_id, \
              decision, reason, decided_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, \
                     strftime('%Y-%m-%dT%H:%M:%SZ', 'now')) \
             ON CONFLICT(capsule_id, application_id, application_digest, key_id) \
             DO UPDATE SET decision = excluded.decision, reason = excluded.reason, \
                 decided_at = excluded.decided_at",
            params![
                evidence.capsule_id,
                evidence.application_id,
                digest.as_slice(),
                key_id,
                publisher.publisher_id,
                decision,
                reason,
            ],
        )?;
        self.record_audit(
            "security",
            if decision == "trusted" {
                "release.trust_exact"
            } else {
                "release.deny_exact"
            },
            Some(&evidence.capsule_id),
            Some(&publisher.publisher_id),
            Some(key_id),
            &json!({
                "application_digest": lower_hex(&digest),
                "decision": decision,
                "reason": reason
            }),
        )
    }

    pub fn trust_exact_file(
        &mut self,
        evidence: &LaunchEvidence,
        reason: &str,
    ) -> Result<(), PolicyError> {
        self.set_exact_file_decision(evidence, "trusted", reason)
    }

    pub fn deny_exact_file(
        &mut self,
        evidence: &LaunchEvidence,
        reason: &str,
    ) -> Result<(), PolicyError> {
        self.set_exact_file_decision(evidence, "denied", reason)
    }

    fn set_exact_file_decision(
        &mut self,
        evidence: &LaunchEvidence,
        decision: &str,
        reason: &str,
    ) -> Result<(), PolicyError> {
        validate_evidence(evidence)?;
        if !matches!(decision, "trusted" | "denied") {
            return Err(PolicyError::Field);
        }
        validate_text(reason, 1, 2048)?;
        self.connection.execute(
            "INSERT INTO local_exception \
             (capsule_id, application_id, source_sha256, decision, reason, decided_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')) \
             ON CONFLICT(capsule_id, application_id, source_sha256) \
             DO UPDATE SET decision = excluded.decision, reason = excluded.reason, \
                 decided_at = excluded.decided_at",
            params![
                evidence.capsule_id,
                evidence.application_id,
                evidence.source_sha256.as_slice(),
                decision,
                reason,
            ],
        )?;
        self.record_audit(
            "security",
            if decision == "trusted" {
                "file.trust_exact"
            } else {
                "file.deny_exact"
            },
            Some(&evidence.capsule_id),
            None,
            None,
            &json!({
                "source_sha256": lower_hex(&evidence.source_sha256),
                "decision": decision,
                "reason": reason
            }),
        )
    }

    pub fn set_persistent_grant(
        &mut self,
        evidence: &LaunchEvidence,
        capability: &str,
        decision: CapabilityDecision,
        reason: &str,
    ) -> Result<(), PolicyError> {
        validate_evidence(evidence)?;
        validate_text(capability, 1, 128)?;
        validate_text(reason, 1, 2048)?;
        let digest = evidence.application_digest.ok_or(PolicyError::Field)?;
        let decision_text = match decision {
            CapabilityDecision::Allow => "allow",
            CapabilityDecision::Deny => "deny",
            CapabilityDecision::Prompt => return Err(PolicyError::AllowOnce),
        };
        self.connection.execute(
            "INSERT INTO capability_grant \
             (capsule_id, application_id, application_digest, capability, decision, reason, decided_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')) \
             ON CONFLICT(capsule_id, application_id, application_digest, capability) \
             DO UPDATE SET decision = excluded.decision, reason = excluded.reason, \
                 decided_at = excluded.decided_at",
            params![
                evidence.capsule_id,
                evidence.application_id,
                digest.as_slice(),
                capability,
                decision_text,
                reason,
            ],
        )?;
        self.record_audit(
            "security",
            "capability.persist",
            Some(&evidence.capsule_id),
            evidence
                .publisher
                .as_ref()
                .map(|value| value.publisher_id.as_str()),
            None,
            &json!({
                "application_digest": lower_hex(&digest),
                "capability": capability,
                "decision": decision_text,
                "reason": reason
            }),
        )
    }

    pub fn evaluate(
        &mut self,
        evidence: &LaunchEvidence,
        context: &EvaluationContext,
    ) -> Result<LaunchDecision, PolicyError> {
        validate_evidence(evidence)?;
        self.observe_capsule(evidence)?;
        let trust_state = self.trust_state(evidence)?;
        let signature_valid = evidence
            .signatures
            .iter()
            .any(|signature| signature.cryptographically_valid && signature.digest_matches);
        let (publisher_known, publisher_trusted) = self.publisher_status(evidence)?;
        let capabilities = self.evaluate_capabilities(evidence, trust_state, context)?;
        let session_trusted =
            trust_state.is_trusted() || (context.trust_once && trust_state.can_prompt());
        let executable_allowed = session_trusted
            && capabilities
                .values()
                .filter(|capability| capability.required)
                .all(|capability| capability.decision == CapabilityDecision::Allow);
        let revocation_status = if trust_state == TrustState::Revoked {
            "revoked".to_owned()
        } else {
            self.active_revocation_status()?
        };
        let decision = LaunchDecision {
            trust_state,
            signature_valid,
            publisher_known,
            publisher_trusted,
            revocation_status,
            executable_allowed,
            application_digest_hex: evidence
                .application_digest
                .as_ref()
                .map(|value| lower_hex(value)),
            capabilities,
        };
        self.record_audit(
            "info",
            "launch.evaluate",
            Some(&evidence.capsule_id),
            evidence
                .publisher
                .as_ref()
                .map(|value| value.publisher_id.as_str()),
            None,
            &serde_json::to_value(&decision)?,
        )?;
        Ok(decision)
    }

    fn trust_state(&self, evidence: &LaunchEvidence) -> Result<TrustState, PolicyError> {
        if !evidence.structure_verified {
            return Ok(TrustState::Unverified);
        }
        let exact_file = self.exact_file_decision(evidence)?;
        match exact_file.as_deref() {
            Some("denied") => return Ok(TrustState::DeniedByUser),
            Some("trusted") | None => {}
            Some(_) => return Err(PolicyError::Schema),
        }
        if evidence.publisher.is_none()
            || evidence.application_digest.is_none()
            || evidence.signatures.is_empty()
        {
            return Ok(if exact_file.as_deref() == Some("trusted") {
                TrustState::LocallyTrustedExactRelease
            } else {
                TrustState::StructurallyVerifiedUnsigned
            });
        }
        let valid: Vec<_> = evidence
            .signatures
            .iter()
            .filter(|signature| signature.cryptographically_valid && signature.digest_matches)
            .collect();
        if valid.is_empty() {
            if evidence
                .signatures
                .iter()
                .any(|signature| signature.cryptographically_valid && !signature.digest_matches)
            {
                return Ok(TrustState::ModifiedAfterSignature);
            }
            return Ok(TrustState::InvalidSignature);
        }
        if self.any_exact_release(evidence, &valid, "denied")? {
            return Ok(TrustState::DeniedByUser);
        }
        if self.any_revoked(evidence, &valid)? {
            return Ok(TrustState::Revoked);
        }
        if exact_file.as_deref() == Some("trusted") {
            return Ok(TrustState::LocallyTrustedExactRelease);
        }
        if self.any_exact_release(evidence, &valid, "trusted")? {
            return Ok(TrustState::LocallyTrustedExactRelease);
        }
        if self.any_trusted_key(evidence, &valid)? {
            return Ok(TrustState::SignedTrustedPublisher);
        }
        Ok(TrustState::SignatureValidUnknownPublisher)
    }

    fn publisher_status(&self, evidence: &LaunchEvidence) -> Result<(bool, bool), PolicyError> {
        let Some(publisher) = &evidence.publisher else {
            return Ok((false, false));
        };
        let known = self
            .connection
            .query_row(
                "SELECT 1 FROM publisher WHERE publisher_id = ?1",
                [&publisher.publisher_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        let mut trusted = false;
        for signature in &evidence.signatures {
            if signature.cryptographically_valid
                && signature.digest_matches
                && self
                    .key_decision(&publisher.publisher_id, signature)?
                    .as_deref()
                    == Some("trusted")
            {
                trusted = true;
                break;
            }
        }
        Ok((known, trusted))
    }

    fn evaluate_capabilities(
        &self,
        evidence: &LaunchEvidence,
        trust_state: TrustState,
        context: &EvaluationContext,
    ) -> Result<BTreeMap<String, CapabilityEvaluation>, PolicyError> {
        let supported: BTreeSet<_> = SUPPORTED_CAPABILITIES.iter().copied().collect();
        let session_trusted =
            trust_state.is_trusted() || (context.trust_once && trust_state.can_prompt());
        let mut output = BTreeMap::new();
        for capability in &evidence.requested_capabilities {
            validate_text(capability, 1, 128)?;
            let is_supported = supported.contains(capability.as_str());
            let persisted = self.persisted_grant(evidence, capability)?;
            let allow_once = context.allow_once.contains(capability);
            let trust_decision = if session_trusted {
                CapabilityDecision::Allow
            } else if trust_state.can_prompt() {
                CapabilityDecision::Prompt
            } else {
                CapabilityDecision::Deny
            };
            let host = context
                .host_policy
                .get(capability)
                .copied()
                .unwrap_or(CapabilityDecision::Prompt);
            let grant = if allow_once {
                CapabilityDecision::Allow
            } else {
                persisted.unwrap_or(CapabilityDecision::Prompt)
            };
            let os = context
                .operating_system_permission
                .get(capability)
                .copied()
                .unwrap_or_else(|| {
                    if OS_MEDIATED_CAPABILITIES.contains(&capability.as_str()) {
                        CapabilityDecision::Prompt
                    } else {
                        CapabilityDecision::Allow
                    }
                });
            let decision = if is_supported {
                trust_decision
                    .intersection(host)
                    .intersection(grant)
                    .intersection(os)
            } else {
                CapabilityDecision::Deny
            };
            let reason = if !is_supported {
                "unsupported capability"
            } else if trust_decision == CapabilityDecision::Deny {
                "trust state denies application capabilities"
            } else if decision == CapabilityDecision::Allow {
                "all policy layers allow"
            } else if decision == CapabilityDecision::Deny {
                "one or more policy layers deny"
            } else {
                "user or operating-system decision required"
            };
            output.insert(
                capability.clone(),
                CapabilityEvaluation {
                    decision,
                    requested: true,
                    required: evidence.required_capabilities.contains(capability),
                    supported: is_supported,
                    persisted_grant: persisted,
                    allow_once,
                    reason: reason.to_owned(),
                },
            );
        }
        Ok(output)
    }

    fn persisted_grant(
        &self,
        evidence: &LaunchEvidence,
        capability: &str,
    ) -> Result<Option<CapabilityDecision>, PolicyError> {
        let Some(digest) = evidence.application_digest else {
            return Ok(None);
        };
        let decision: Option<String> = self
            .connection
            .query_row(
                "SELECT decision FROM capability_grant WHERE capsule_id = ?1 \
                 AND application_id = ?2 AND application_digest = ?3 AND capability = ?4",
                params![
                    evidence.capsule_id,
                    evidence.application_id,
                    digest.as_slice(),
                    capability,
                ],
                |row| row.get(0),
            )
            .optional()?;
        Ok(match decision.as_deref() {
            Some("allow") => Some(CapabilityDecision::Allow),
            Some("deny") => Some(CapabilityDecision::Deny),
            Some(_) => return Err(PolicyError::Schema),
            None => None,
        })
    }

    fn exact_file_decision(
        &self,
        evidence: &LaunchEvidence,
    ) -> Result<Option<String>, PolicyError> {
        Ok(self
            .connection
            .query_row(
                "SELECT decision FROM local_exception WHERE capsule_id = ?1 \
                 AND application_id = ?2 AND source_sha256 = ?3",
                params![
                    evidence.capsule_id,
                    evidence.application_id,
                    evidence.source_sha256.as_slice(),
                ],
                |row| row.get(0),
            )
            .optional()?)
    }

    fn any_revoked(
        &self,
        evidence: &LaunchEvidence,
        signatures: &[&SignatureEvidence],
    ) -> Result<bool, PolicyError> {
        if self.any_exact_release(evidence, signatures, "revoked")? {
            return Ok(true);
        }
        let digest = evidence.application_digest.ok_or(PolicyError::Field)?;
        let remote_release = self
            .connection
            .query_row(
                "SELECT 1 FROM remote_release_revocation \
                 WHERE application_id = ?1 AND application_digest = ?2",
                params![evidence.application_id, digest.as_slice()],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if remote_release {
            return Ok(true);
        }
        let publisher = evidence.publisher.as_ref().ok_or(PolicyError::Field)?;
        for signature in signatures {
            let remote_key = self
                .connection
                .query_row(
                    "SELECT 1 FROM remote_key_revocation WHERE key_id = ?1",
                    [&signature.key_id],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if remote_key {
                return Ok(true);
            }
            if self
                .key_decision(&publisher.publisher_id, signature)?
                .as_deref()
                == Some("revoked")
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn active_revocation_status(&self) -> Result<String, PolicyError> {
        let status = self
            .connection
            .query_row(
                "SELECT CASE WHEN next_update >= strftime('%Y-%m-%dT%H:%M:%SZ', 'now') \
                        THEN 'fresh' ELSE 'stale' END \
                 FROM revocation_bundle WHERE active = 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        Ok(status.unwrap_or_else(|| "not_checked".to_owned()))
    }

    fn any_exact_release(
        &self,
        evidence: &LaunchEvidence,
        signatures: &[&SignatureEvidence],
        decision: &str,
    ) -> Result<bool, PolicyError> {
        let digest = evidence.application_digest.ok_or(PolicyError::Field)?;
        for signature in signatures {
            let found = self
                .connection
                .query_row(
                    "SELECT 1 FROM exact_release WHERE capsule_id = ?1 AND application_id = ?2 \
                     AND application_digest = ?3 AND key_id = ?4 AND decision = ?5",
                    params![
                        evidence.capsule_id,
                        evidence.application_id,
                        digest.as_slice(),
                        signature.key_id,
                        decision,
                    ],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if found {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn any_trusted_key(
        &self,
        evidence: &LaunchEvidence,
        signatures: &[&SignatureEvidence],
    ) -> Result<bool, PolicyError> {
        let publisher = evidence.publisher.as_ref().ok_or(PolicyError::Field)?;
        for signature in signatures {
            if self
                .key_decision(&publisher.publisher_id, signature)?
                .as_deref()
                == Some("trusted")
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn key_decision(
        &self,
        publisher_id: &str,
        signature: &SignatureEvidence,
    ) -> Result<Option<String>, PolicyError> {
        Ok(self
            .connection
            .query_row(
                "SELECT decision FROM publisher_key WHERE key_id = ?1 AND publisher_id = ?2 \
                 AND public_key = ?3",
                params![
                    signature.key_id,
                    publisher_id,
                    signature.public_key.as_slice()
                ],
                |row| row.get(0),
            )
            .optional()?)
    }

    fn observe_capsule(&mut self, evidence: &LaunchEvidence) -> Result<(), PolicyError> {
        self.connection.execute(
            "INSERT INTO capsule_identity \
             (capsule_id, application_id, last_source_sha256, first_seen_at, last_seen_at) \
             VALUES (?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), \
                     strftime('%Y-%m-%dT%H:%M:%SZ', 'now')) \
             ON CONFLICT(capsule_id, application_id) DO UPDATE SET \
                 last_source_sha256 = excluded.last_source_sha256, \
                 last_seen_at = excluded.last_seen_at",
            params![
                evidence.capsule_id,
                evidence.application_id,
                evidence.source_sha256.as_slice(),
            ],
        )?;
        Ok(())
    }

    pub fn audit_events(&self, limit: usize) -> Result<Vec<AuditEvent>, PolicyError> {
        let limit = limit.clamp(1, MAX_AUDIT_ROWS) as i64;
        let mut statement = self.connection.prepare(
            "SELECT id, occurred_at, severity, action, capsule_id, publisher_id, key_id, details_json \
             FROM audit_event ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = statement.query_map([limit], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, String>(7)?,
            ))
        })?;
        let mut events = Vec::new();
        for row in rows {
            let (id, occurred_at, severity, action, capsule_id, publisher_id, key_id, details) =
                row?;
            events.push(AuditEvent {
                id,
                occurred_at,
                severity,
                action,
                capsule_id,
                publisher_id,
                key_id,
                details: serde_json::from_str(&details)?,
            });
        }
        Ok(events)
    }

    pub fn export_redacted(&self) -> Result<Value, PolicyError> {
        let publishers = query_json_rows(
            &self.connection,
            "SELECT publisher_id, publisher_name, status, created_at, updated_at \
             FROM publisher ORDER BY publisher_id",
            |row| {
                Ok(json!({
                    "publisher_id": row.get::<_, String>(0)?,
                    "publisher_name": row.get::<_, String>(1)?,
                    "status": row.get::<_, String>(2)?,
                    "created_at": row.get::<_, String>(3)?,
                    "updated_at": row.get::<_, String>(4)?,
                }))
            },
        )?;
        let keys = query_json_rows(
            &self.connection,
            "SELECT key_id, publisher_id, decision, reason, decided_at \
             FROM publisher_key ORDER BY key_id",
            |row| {
                Ok(json!({
                    "key_id": row.get::<_, String>(0)?,
                    "publisher_id": row.get::<_, String>(1)?,
                    "decision": row.get::<_, String>(2)?,
                    "reason": row.get::<_, String>(3)?,
                    "decided_at": row.get::<_, String>(4)?,
                }))
            },
        )?;
        let delegations = query_json_rows(
            &self.connection,
            "SELECT from_key_id, to_key_id, publisher_id, application_id, \
                    hex(evidence_digest), approved_at FROM key_delegation \
             ORDER BY from_key_id, to_key_id, application_id",
            |row| {
                Ok(json!({
                    "from_key_id": row.get::<_, String>(0)?,
                    "to_key_id": row.get::<_, String>(1)?,
                    "publisher_id": row.get::<_, String>(2)?,
                    "application_id": row.get::<_, String>(3)?,
                    "evidence_digest_sha256": row.get::<_, String>(4)?.to_lowercase(),
                    "approved_at": row.get::<_, String>(5)?,
                }))
            },
        )?;
        let exact_releases = query_json_rows(
            &self.connection,
            "SELECT capsule_id, application_id, hex(application_digest), key_id, \
                    publisher_id, decision, reason, decided_at FROM exact_release \
             ORDER BY capsule_id, application_id, application_digest, key_id",
            |row| {
                Ok(json!({
                    "capsule_id": row.get::<_, String>(0)?,
                    "application_id": row.get::<_, String>(1)?,
                    "application_digest_sha256": row.get::<_, String>(2)?.to_lowercase(),
                    "key_id": row.get::<_, String>(3)?,
                    "publisher_id": row.get::<_, String>(4)?,
                    "decision": row.get::<_, String>(5)?,
                    "reason": row.get::<_, String>(6)?,
                    "decided_at": row.get::<_, String>(7)?,
                }))
            },
        )?;
        let local_exceptions = query_json_rows(
            &self.connection,
            "SELECT capsule_id, application_id, hex(source_sha256), decision, reason, decided_at \
             FROM local_exception ORDER BY capsule_id, application_id, source_sha256",
            |row| {
                Ok(json!({
                    "capsule_id": row.get::<_, String>(0)?,
                    "application_id": row.get::<_, String>(1)?,
                    "source_sha256": row.get::<_, String>(2)?.to_lowercase(),
                    "decision": row.get::<_, String>(3)?,
                    "reason": row.get::<_, String>(4)?,
                    "decided_at": row.get::<_, String>(5)?,
                }))
            },
        )?;
        let capsule_identities = query_json_rows(
            &self.connection,
            "SELECT capsule_id, application_id, hex(last_source_sha256), first_seen_at, last_seen_at \
             FROM capsule_identity ORDER BY capsule_id, application_id",
            |row| {
                Ok(json!({
                    "capsule_id": row.get::<_, String>(0)?,
                    "application_id": row.get::<_, String>(1)?,
                    "last_source_sha256": row.get::<_, String>(2)?.to_lowercase(),
                    "first_seen_at": row.get::<_, String>(3)?,
                    "last_seen_at": row.get::<_, String>(4)?,
                }))
            },
        )?;
        let grants = query_json_rows(
            &self.connection,
            "SELECT capsule_id, application_id, hex(application_digest), capability, decision, \
                    reason, decided_at FROM capability_grant \
             ORDER BY capsule_id, application_id, application_digest, capability",
            |row| {
                Ok(json!({
                    "capsule_id": row.get::<_, String>(0)?,
                    "application_id": row.get::<_, String>(1)?,
                    "application_digest_sha256": row.get::<_, String>(2)?.to_lowercase(),
                    "capability": row.get::<_, String>(3)?,
                    "decision": row.get::<_, String>(4)?,
                    "reason": row.get::<_, String>(5)?,
                    "decided_at": row.get::<_, String>(6)?,
                }))
            },
        )?;
        let revocation_bundles = query_json_rows(
            &self.connection,
            "SELECT sequence, issued_at, next_update, hex(payload_digest), installed_at, active \
             FROM revocation_bundle ORDER BY sequence",
            |row| {
                Ok(json!({
                    "sequence": row.get::<_, i64>(0)?,
                    "issued_at": row.get::<_, String>(1)?,
                    "next_update": row.get::<_, String>(2)?,
                    "payload_digest_sha256": row.get::<_, String>(3)?.to_lowercase(),
                    "installed_at": row.get::<_, String>(4)?,
                    "active": row.get::<_, i64>(5)? == 1,
                }))
            },
        )?;
        let remote_key_revocations = query_json_rows(
            &self.connection,
            "SELECT key_id, bundle_sequence, reason FROM remote_key_revocation ORDER BY key_id",
            |row| {
                Ok(json!({
                    "key_id": row.get::<_, String>(0)?,
                    "bundle_sequence": row.get::<_, i64>(1)?,
                    "reason": row.get::<_, String>(2)?,
                }))
            },
        )?;
        let remote_release_revocations = query_json_rows(
            &self.connection,
            "SELECT application_id, hex(application_digest), bundle_sequence, reason \
             FROM remote_release_revocation ORDER BY application_id, application_digest",
            |row| {
                Ok(json!({
                    "application_id": row.get::<_, String>(0)?,
                    "application_digest_sha256": row.get::<_, String>(1)?.to_lowercase(),
                    "bundle_sequence": row.get::<_, i64>(2)?,
                    "reason": row.get::<_, String>(3)?,
                }))
            },
        )?;
        let revocation_roots = query_json_rows(
            &self.connection,
            "SELECT key_id, decision, bundle_sequence, reason \
             FROM revocation_root ORDER BY key_id",
            |row| {
                Ok(json!({
                    "key_id": row.get::<_, String>(0)?,
                    "decision": row.get::<_, String>(1)?,
                    "bundle_sequence": row.get::<_, i64>(2)?,
                    "reason": row.get::<_, String>(3)?,
                }))
            },
        )?;
        let backups = query_json_rows(
            &self.connection,
            "SELECT backup_id, source_capsule_id, hex(database_digest), byte_length, \
                    created_at, verified_at FROM backup_inventory ORDER BY created_at, backup_id",
            |row| {
                Ok(json!({
                    "backup_id": row.get::<_, String>(0)?,
                    "source_capsule_id": row.get::<_, String>(1)?,
                    "canonical_path": "redacted",
                    "database_digest_sha256": row.get::<_, String>(2)?.to_lowercase(),
                    "byte_length": row.get::<_, i64>(3)?,
                    "created_at": row.get::<_, String>(4)?,
                    "verified_at": row.get::<_, String>(5)?,
                }))
            },
        )?;
        Ok(json!({
            "format": "org.sqlite-capsule.trust-export/0.2",
            "schema_version": SCHEMA_VERSION,
            "store_path": "redacted",
            "private_keys_present": false,
            "publishers": publishers,
            "keys": keys,
            "delegations": delegations,
            "exact_releases": exact_releases,
            "local_exceptions": local_exceptions,
            "capsule_identities": capsule_identities,
            "grants": grants,
            "revocation_bundles": revocation_bundles,
            "remote_key_revocations": remote_key_revocations,
            "remote_release_revocations": remote_release_revocations,
            "revocation_roots": revocation_roots,
            "backups": backups,
            "audit": self.audit_events(MAX_AUDIT_ROWS)?.into_iter().map(|event| json!({
                "id": event.id,
                "occurred_at": event.occurred_at,
                "severity": event.severity,
                "action": event.action,
                "capsule_id": event.capsule_id,
                "publisher_id": event.publisher_id,
                "key_id": event.key_id,
                "details": event.details,
            })).collect::<Vec<_>>()
        }))
    }

    /// Removes only decisions attached to the exact current file and signed
    /// application digest. Publisher trust, revocations, other capsules, other
    /// file digests, backups, and their audit history are preserved. Removing a
    /// denial returns the capsule to a promptable state; it does not grant
    /// authority.
    pub fn forget_current_decision(
        &mut self,
        evidence: &LaunchEvidence,
        confirmation: &str,
    ) -> Result<ForgottenDecisionReport, PolicyError> {
        if confirmation != "FORGET-CURRENT-DECISION" {
            return Err(PolicyError::Field);
        }
        validate_evidence(evidence)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let local_exceptions = transaction.execute(
            "DELETE FROM local_exception \
             WHERE capsule_id = ?1 AND application_id = ?2 AND source_sha256 = ?3",
            params![
                evidence.capsule_id,
                evidence.application_id,
                evidence.source_sha256.as_slice(),
            ],
        )?;
        let (exact_releases, capability_grants) =
            if let Some(application_digest) = evidence.application_digest {
                let capability_grants = transaction.execute(
                    "DELETE FROM capability_grant \
                     WHERE capsule_id = ?1 AND application_id = ?2 AND application_digest = ?3",
                    params![
                        evidence.capsule_id,
                        evidence.application_id,
                        application_digest.as_slice(),
                    ],
                )?;
                let exact_releases = transaction.execute(
                    "DELETE FROM exact_release \
                     WHERE capsule_id = ?1 AND application_id = ?2 AND application_digest = ?3",
                    params![
                        evidence.capsule_id,
                        evidence.application_id,
                        application_digest.as_slice(),
                    ],
                )?;
                (exact_releases, capability_grants)
            } else {
                (0, 0)
            };
        if local_exceptions + exact_releases + capability_grants == 0 {
            return Err(PolicyError::Field);
        }
        insert_audit(
            &transaction,
            "security",
            "decision.forget_exact",
            Some(&evidence.capsule_id),
            evidence
                .publisher
                .as_ref()
                .map(|publisher| publisher.publisher_id.as_str()),
            None,
            &json!({
                "source_sha256": lower_hex(&evidence.source_sha256),
                "application_digest": evidence.application_digest.map(|digest| lower_hex(&digest)),
                "local_exceptions": local_exceptions,
                "exact_releases": exact_releases,
                "capability_grants": capability_grants,
                "authority_granted": false,
            }),
        )?;
        transaction.commit()?;
        Ok(ForgottenDecisionReport {
            local_exceptions,
            exact_releases,
            capability_grants,
        })
    }

    pub fn reset_decisions(&mut self, confirmation: &str) -> Result<(), PolicyError> {
        if confirmation != "ERASE-TRUST-DECISIONS" {
            return Err(PolicyError::Field);
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Exclusive)?;
        transaction.execute_batch(
            "DELETE FROM key_delegation;
             DELETE FROM exact_release;
             DELETE FROM local_exception;
             DELETE FROM capability_grant;
             DELETE FROM remote_key_revocation;
             DELETE FROM remote_release_revocation;
             DELETE FROM revocation_root;
             DELETE FROM revocation_bundle;
             DELETE FROM backup_inventory;
             DELETE FROM capsule_identity;
             DELETE FROM publisher_key;
             DELETE FROM publisher;
             DELETE FROM audit_event;
             DELETE FROM sqlite_sequence WHERE name = 'audit_event';",
        )?;
        insert_audit(
            &transaction,
            "security",
            "trust_store.reset",
            None,
            None,
            None,
            &json!({"backup_required_by_cli": true}),
        )?;
        transaction.commit()?;
        Ok(())
    }

    fn record_audit(
        &self,
        severity: &str,
        action: &str,
        capsule_id: Option<&str>,
        publisher_id: Option<&str>,
        key_id: Option<&str>,
        details: &Value,
    ) -> Result<(), PolicyError> {
        insert_audit(
            &self.connection,
            severity,
            action,
            capsule_id,
            publisher_id,
            key_id,
            details,
        )
    }
}

fn insert_audit(
    connection: &Connection,
    severity: &str,
    action: &str,
    capsule_id: Option<&str>,
    publisher_id: Option<&str>,
    key_id: Option<&str>,
    details: &Value,
) -> Result<(), PolicyError> {
    validate_text(action, 1, 128)?;
    let details = serde_json::to_string(details)?;
    connection.execute(
        "INSERT INTO audit_event \
         (occurred_at, severity, action, capsule_id, publisher_id, key_id, details_json) \
         VALUES (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), ?1, ?2, ?3, ?4, ?5, ?6)",
        params![severity, action, capsule_id, publisher_id, key_id, details],
    )?;
    Ok(())
}

fn validate_evidence(evidence: &LaunchEvidence) -> Result<(), PolicyError> {
    validate_text(&evidence.capsule_id, 1, 1024)?;
    validate_text(&evidence.application_id, 1, 512)?;
    if let Some(publisher) = &evidence.publisher {
        validate_identity(publisher)?;
    }
    if evidence.signatures.len() > 1024
        || evidence.requested_capabilities.len() > 128
        || !evidence
            .required_capabilities
            .is_subset(&evidence.requested_capabilities)
    {
        return Err(PolicyError::Field);
    }
    for signature in &evidence.signatures {
        validate_text(&signature.key_id, 1, 256)?;
    }
    Ok(())
}

fn validate_identity(publisher: &PublisherEvidence) -> Result<(), PolicyError> {
    validate_text(&publisher.publisher_id, 1, 512)?;
    validate_text(&publisher.publisher_name, 1, 512)
}

fn validate_text(value: &str, minimum: usize, maximum: usize) -> Result<(), PolicyError> {
    if value.len() < minimum || value.len() > maximum || value.contains('\0') {
        return Err(PolicyError::Field);
    }
    Ok(())
}

fn absolute_path(path: &Path) -> Result<PathBuf, PolicyError> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

#[cfg(unix)]
fn secure_directory(path: &Path) -> Result<(), PolicyError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(windows)]
fn secure_directory(path: &Path) -> Result<(), PolicyError> {
    windows_acl::protect(path, true)
}

#[cfg(not(any(unix, windows)))]
fn secure_directory(_path: &Path) -> Result<(), PolicyError> {
    Err(PolicyError::Field)
}

#[cfg(unix)]
fn secure_file(path: &Path) -> Result<(), PolicyError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(windows)]
fn secure_file(path: &Path) -> Result<(), PolicyError> {
    windows_acl::protect(path, false)
}

#[cfg(not(any(unix, windows)))]
fn secure_file(_path: &Path) -> Result<(), PolicyError> {
    Err(PolicyError::Field)
}

fn secure_store_files(path: &Path) -> Result<(), PolicyError> {
    secure_file(path)?;
    for suffix in ["-wal", "-shm"] {
        let sidecar = PathBuf::from(format!("{}{suffix}", path.display()));
        if sidecar.exists() {
            secure_file(&sidecar)?;
        }
    }
    Ok(())
}

#[cfg(windows)]
mod windows_acl {
    use std::{
        ffi::OsStr,
        io,
        mem::{size_of, size_of_val},
        os::windows::ffi::OsStrExt,
        path::Path,
        ptr,
    };

    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE},
        Security::{
            ACCESS_ALLOWED_ACE, ACL, ACL_REVISION, AddAccessAllowedAceEx,
            Authorization::{SE_FILE_OBJECT, SetNamedSecurityInfoW},
            CONTAINER_INHERIT_ACE, DACL_SECURITY_INFORMATION, GetLengthSid, GetTokenInformation,
            InitializeAcl, OBJECT_INHERIT_ACE, PROTECTED_DACL_SECURITY_INFORMATION, PSID,
            TOKEN_QUERY, TOKEN_USER, TokenUser,
        },
        Storage::FileSystem::FILE_ALL_ACCESS,
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    use super::PolicyError;

    struct Handle(HANDLE);

    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: this wrapper uniquely owns a successful token handle.
                unsafe {
                    CloseHandle(self.0);
                }
            }
        }
    }

    pub(super) fn protect(path: &Path, directory: bool) -> Result<(), PolicyError> {
        // SAFETY: every pointer below references an aligned live buffer for the
        // duration of the corresponding Win32 call, and return values are checked.
        unsafe {
            let mut raw_token: HANDLE = ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw_token) == 0 {
                return Err(io::Error::last_os_error().into());
            }
            let token = Handle(raw_token);

            let mut token_bytes = 0_u32;
            GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &mut token_bytes);
            if token_bytes < size_of::<TOKEN_USER>() as u32 {
                return Err(io::Error::last_os_error().into());
            }
            let token_words = (token_bytes as usize).div_ceil(size_of::<usize>());
            let mut token_buffer = vec![0_usize; token_words];
            if GetTokenInformation(
                token.0,
                TokenUser,
                token_buffer.as_mut_ptr().cast(),
                token_bytes,
                &mut token_bytes,
            ) == 0
            {
                return Err(io::Error::last_os_error().into());
            }
            let token_user = &*token_buffer.as_ptr().cast::<TOKEN_USER>();
            let sid: PSID = token_user.User.Sid;
            let sid_bytes = GetLengthSid(sid);
            if sid_bytes == 0 {
                return Err(io::Error::last_os_error().into());
            }

            let acl_bytes = size_of::<ACL>() + size_of::<ACCESS_ALLOWED_ACE>() - size_of::<u32>()
                + sid_bytes as usize;
            let acl_words = acl_bytes.div_ceil(size_of::<usize>());
            let mut acl_buffer = vec![0_usize; acl_words];
            let acl = acl_buffer.as_mut_ptr().cast::<ACL>();
            let allocated_bytes = size_of_val(acl_buffer.as_slice()) as u32;
            if InitializeAcl(acl, allocated_bytes, ACL_REVISION) == 0 {
                return Err(io::Error::last_os_error().into());
            }
            let inheritance = if directory {
                OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE
            } else {
                0
            };
            if AddAccessAllowedAceEx(acl, ACL_REVISION, inheritance, FILE_ALL_ACCESS, sid) == 0 {
                return Err(io::Error::last_os_error().into());
            }

            let mut wide: Vec<u16> = OsStr::new(path).encode_wide().collect();
            wide.push(0);
            let result = SetNamedSecurityInfoW(
                wide.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                acl,
                ptr::null(),
            );
            if result != 0 {
                return Err(io::Error::from_raw_os_error(result as i32).into());
            }
        }
        Ok(())
    }
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

fn decode_lower_hex<const N: usize>(value: &str) -> Result<[u8; N], PolicyError> {
    if value.len() != N * 2
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(PolicyError::Field);
    }
    let mut output = [0_u8; N];
    for (index, item) in output.iter_mut().enumerate() {
        let nibble = |byte: u8| match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            _ => unreachable!("validated lowercase hexadecimal"),
        };
        *item = nibble(value.as_bytes()[index * 2]) << 4 | nibble(value.as_bytes()[index * 2 + 1]);
    }
    Ok(output)
}

fn query_json_rows<F>(
    connection: &Connection,
    sql: &str,
    transform: F,
) -> Result<Vec<Value>, PolicyError>
where
    F: Fn(&rusqlite::Row<'_>) -> rusqlite::Result<Value>,
{
    Ok(connection
        .prepare(sql)?
        .query_map([], transform)?
        .collect::<Result<Vec<_>, _>>()?)
}

struct TemporaryFile {
    path: PathBuf,
    remove: bool,
}

impl TemporaryFile {
    fn new(path: PathBuf) -> Self {
        Self { path, remove: true }
    }

    fn keep(&mut self) {
        self.remove = false;
    }
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        if self.remove {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use ed25519_dalek::{Signer, SigningKey};
    use sqlite_capsule_distribution::{
        EmergencyRoot, KeyRevocation, REVOCATION_PROFILE, ReleaseRevocation, RevocationBundle,
        SignedRevocationBundle, key_id, verify_revocation_bundle,
    };

    use super::*;

    static SEQUENCE: AtomicUsize = AtomicUsize::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "sqlite-capsule-policy-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir(&path).expect("create test directory");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn publisher() -> PublisherEvidence {
        PublisherEvidence {
            publisher_id: "org.example.publisher".to_owned(),
            publisher_name: "Example Publisher".to_owned(),
        }
    }

    fn evidence() -> LaunchEvidence {
        let public_key = [3_u8; 32];
        LaunchEvidence {
            structure_verified: true,
            capsule_id: "urn:test:capsule".to_owned(),
            application_id: "org.example.app".to_owned(),
            source_sha256: [1_u8; 32],
            application_digest: Some([2_u8; 32]),
            publisher: Some(publisher()),
            signatures: vec![SignatureEvidence {
                key_id: format!("ed25519:sha256:{}", lower_hex(&Sha256::digest(public_key))),
                public_key,
                cryptographically_valid: true,
                digest_matches: true,
            }],
            requested_capabilities: BTreeSet::from([
                "database.read".to_owned(),
                "database.write".to_owned(),
            ]),
            required_capabilities: BTreeSet::from([
                "database.read".to_owned(),
                "database.write".to_owned(),
            ]),
        }
    }

    fn open_store(directory: &TestDirectory) -> TrustStore {
        TrustStore::open(&directory.0.join("state/trust.sqlite")).expect("open trust store")
    }

    fn permissive_context() -> EvaluationContext {
        EvaluationContext {
            host_policy: BTreeMap::from([
                ("database.read".to_owned(), CapabilityDecision::Allow),
                ("database.write".to_owned(), CapabilityDecision::Allow),
            ]),
            operating_system_permission: BTreeMap::new(),
            allow_once: BTreeSet::new(),
            trust_once: false,
        }
    }

    fn verified_revocations(
        sequence: u64,
        next_update: &str,
        revoke_key: bool,
        revoke_release: bool,
    ) -> VerifiedRevocationBundle {
        let signing_key = SigningKey::from_bytes(&[41_u8; 32]);
        let evidence = evidence();
        let emergency = SigningKey::from_bytes(&[42_u8; 32]);
        let bundle = RevocationBundle {
            profile: REVOCATION_PROFILE.to_owned(),
            sequence,
            issued_at: "2026-08-08T10:00:00Z".to_owned(),
            next_update: next_update.to_owned(),
            revoked_keys: revoke_key
                .then(|| KeyRevocation {
                    key_id: evidence.signatures[0].key_id.clone(),
                    reason: "fixture key revocation".to_owned(),
                })
                .into_iter()
                .collect(),
            revoked_releases: revoke_release
                .then(|| ReleaseRevocation {
                    application_id: evidence.application_id.clone(),
                    application_digest_sha256: lower_hex(
                        evidence.application_digest.as_ref().expect("digest"),
                    ),
                    reason: "fixture release revocation".to_owned(),
                })
                .into_iter()
                .collect(),
            emergency_roots: vec![EmergencyRoot {
                key_id: key_id(&emergency.verifying_key().to_bytes()),
                public_key_hex: lower_hex(&emergency.verifying_key().to_bytes()),
                action: "delegate".to_owned(),
                reason: "fixture emergency delegation".to_owned(),
            }],
        };
        let canonical = serde_json_canonicalizer::to_vec(&bundle).expect("canonical bundle");
        let mut message = b"SQLite Capsule revocation bundle v1\0".to_vec();
        message.extend_from_slice(&canonical);
        let signed = SignedRevocationBundle {
            bundle,
            signing_key_id: key_id(&signing_key.verifying_key().to_bytes()),
            signature_hex: lower_hex(&signing_key.sign(&message).to_bytes()),
        };
        verify_revocation_bundle(
            &signed,
            &signing_key.verifying_key().to_bytes(),
            sequence.saturating_sub(1),
            1_786_190_400,
        )
        .expect("verified fixture bundle")
    }

    #[test]
    fn migrates_backs_up_and_reopens_the_host_local_store() {
        let directory = TestDirectory::new();
        let store = open_store(&directory);
        assert_eq!(
            store
                .connection
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .expect("version"),
            SCHEMA_VERSION
        );
        let backup = directory.0.join("backup/trust-backup.sqlite");
        store.backup_to(&backup).expect("verified backup");
        assert!(backup.is_file());
        drop(store);
        TrustStore::open(&directory.0.join("state/trust.sqlite")).expect("reopen store");
    }

    #[test]
    fn migrates_an_existing_v1_store_atomically() {
        let directory = TestDirectory::new();
        let path = directory.0.join("state/trust.sqlite");
        fs::create_dir_all(path.parent().expect("parent")).expect("state directory");
        let connection = Connection::open(&path).expect("legacy store");
        connection.execute_batch(SCHEMA_V1).expect("v1 schema");
        connection
            .pragma_update(None, "user_version", 1_i64)
            .expect("v1 version");
        drop(connection);
        let store = TrustStore::open(&path).expect("migrated v2 store");
        assert_eq!(
            store
                .connection
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .expect("version"),
            2
        );
        assert_eq!(
            store
                .connection
                .query_row(
                    "SELECT schema_version FROM trust_meta WHERE id = 1",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .expect("meta version"),
            2
        );
    }

    #[test]
    fn signed_remote_revocations_install_atomically_and_override_local_trust() {
        let directory = TestDirectory::new();
        let mut store = open_store(&directory);
        let signed = evidence();
        store
            .trust_publisher_key(
                signed.publisher.as_ref().expect("publisher"),
                &signed.signatures[0].key_id,
                &signed.signatures[0].public_key,
                "fixture publisher",
            )
            .expect("trusted publisher");
        store
            .trust_exact_file(&signed, "local exact-file override attempt")
            .expect("exact file trust");
        let mut allowed = permissive_context();
        allowed.allow_once = signed.requested_capabilities.clone();
        assert!(
            store
                .evaluate(&signed, &allowed)
                .expect("pre-revocation")
                .executable_allowed
        );

        let bundle = verified_revocations(7, "2099-01-01T00:00:00Z", true, true);
        store
            .install_revocation_bundle(&bundle)
            .expect("install bundle");
        let blocked = store
            .evaluate(&signed, &allowed)
            .expect("revoked evaluation");
        assert_eq!(blocked.trust_state, TrustState::Revoked);
        assert_eq!(blocked.revocation_status, "revoked");
        assert!(!blocked.executable_allowed);
        assert!(matches!(
            store.install_revocation_bundle(&bundle),
            Err(PolicyError::RevocationRollback)
        ));

        let export = store.export_redacted().expect("redacted export");
        assert_eq!(
            export["remote_key_revocations"]
                .as_array()
                .expect("keys")
                .len(),
            1
        );
        assert_eq!(
            export["remote_release_revocations"]
                .as_array()
                .expect("releases")
                .len(),
            1
        );
        assert_eq!(
            export["revocation_roots"].as_array().expect("roots").len(),
            1
        );
        assert!(!export.to_string().contains("public_key_hex"));
        let roots = store.revocation_roots().expect("trusted root records");
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].decision, "delegated");
        assert_eq!(roots[0].bundle_sequence, 7);
    }

    #[test]
    fn stale_last_known_good_is_reported_without_forgetting_known_revocations() {
        let directory = TestDirectory::new();
        let mut store = open_store(&directory);
        let signed = evidence();
        let bundle = verified_revocations(8, "2026-08-08T11:00:00Z", false, true);
        store
            .install_revocation_bundle(&bundle)
            .expect("install stale bundle");
        let blocked = store
            .evaluate(&signed, &permissive_context())
            .expect("known revoked release");
        assert_eq!(blocked.trust_state, TrustState::Revoked);
        assert_eq!(blocked.revocation_status, "revoked");

        let mut unrelated = signed.clone();
        unrelated.application_digest = Some([99_u8; 32]);
        let stale = store
            .evaluate(&unrelated, &permissive_context())
            .expect("unrelated stale evaluation");
        assert_eq!(stale.revocation_status, "stale");
    }

    #[test]
    fn launch_states_do_not_confuse_valid_signatures_with_trust() {
        let directory = TestDirectory::new();
        let mut store = open_store(&directory);
        let mut signed = evidence();
        let unknown = store
            .evaluate(&signed, &permissive_context())
            .expect("unknown evaluation");
        assert_eq!(
            unknown.trust_state,
            TrustState::SignatureValidUnknownPublisher
        );
        assert!(unknown.signature_valid);
        assert!(!unknown.publisher_trusted);
        assert!(!unknown.executable_allowed);

        store
            .trust_publisher_key(
                signed.publisher.as_ref().expect("publisher"),
                &signed.signatures[0].key_id,
                &signed.signatures[0].public_key,
                "fixture approval",
            )
            .expect("trust key");
        for capability in &signed.requested_capabilities {
            store
                .set_persistent_grant(
                    &signed,
                    capability,
                    CapabilityDecision::Allow,
                    "fixture grant",
                )
                .expect("grant");
        }
        let trusted = store
            .evaluate(&signed, &permissive_context())
            .expect("trusted evaluation");
        assert_eq!(trusted.trust_state, TrustState::SignedTrustedPublisher);
        assert!(trusted.publisher_trusted);
        assert!(trusted.executable_allowed);

        signed.signatures[0].digest_matches = false;
        let modified = store
            .evaluate(&signed, &permissive_context())
            .expect("modified evaluation");
        assert_eq!(modified.trust_state, TrustState::ModifiedAfterSignature);
        assert!(!modified.executable_allowed);
    }

    #[test]
    fn revocation_overrides_trust_and_grants() {
        let directory = TestDirectory::new();
        let mut store = open_store(&directory);
        let signed = evidence();
        let signature = &signed.signatures[0];
        store
            .trust_publisher_key(
                signed.publisher.as_ref().expect("publisher"),
                &signature.key_id,
                &signature.public_key,
                "fixture approval",
            )
            .expect("trust key");
        store
            .revoke_publisher_key(&signature.key_id, "fixture revocation")
            .expect("revoke key");
        let decision = store
            .evaluate(&signed, &permissive_context())
            .expect("revoked evaluation");
        assert_eq!(decision.trust_state, TrustState::Revoked);
        assert_eq!(decision.revocation_status, "revoked");
        assert!(!decision.executable_allowed);
    }

    #[test]
    fn grants_are_scoped_to_capsule_application_and_digest() {
        let directory = TestDirectory::new();
        let mut store = open_store(&directory);
        let mut signed = evidence();
        store
            .trust_publisher_key(
                signed.publisher.as_ref().expect("publisher"),
                &signed.signatures[0].key_id,
                &signed.signatures[0].public_key,
                "fixture approval",
            )
            .expect("trust key");
        store
            .set_persistent_grant(
                &signed,
                "database.read",
                CapabilityDecision::Allow,
                "fixture grant",
            )
            .expect("grant");
        let original = store
            .evaluate(&signed, &permissive_context())
            .expect("original evaluation");
        assert_eq!(
            original.capabilities["database.read"].decision,
            CapabilityDecision::Allow
        );

        signed.application_digest = Some([9_u8; 32]);
        let changed = store
            .evaluate(&signed, &permissive_context())
            .expect("changed evaluation");
        assert_eq!(
            changed.capabilities["database.read"].decision,
            CapabilityDecision::Prompt
        );
        assert_eq!(changed.capabilities["database.read"].persisted_grant, None);
    }

    #[test]
    fn allow_once_is_ephemeral_and_prompt_cannot_be_persisted() {
        let directory = TestDirectory::new();
        let mut store = open_store(&directory);
        let signed = evidence();
        store
            .trust_publisher_key(
                signed.publisher.as_ref().expect("publisher"),
                &signed.signatures[0].key_id,
                &signed.signatures[0].public_key,
                "fixture approval",
            )
            .expect("trust key");
        assert!(matches!(
            store.set_persistent_grant(
                &signed,
                "database.read",
                CapabilityDecision::Prompt,
                "not persistent",
            ),
            Err(PolicyError::AllowOnce)
        ));
        let mut once = permissive_context();
        once.allow_once.insert("database.read".to_owned());
        let first = store
            .evaluate(&signed, &once)
            .expect("allow-once evaluation");
        assert_eq!(
            first.capabilities["database.read"].decision,
            CapabilityDecision::Allow
        );
        let next = store
            .evaluate(&signed, &permissive_context())
            .expect("next evaluation");
        assert_eq!(
            next.capabilities["database.read"].decision,
            CapabilityDecision::Prompt
        );
    }

    #[test]
    fn optional_prompts_do_not_block_required_capabilities_or_persist_session_trust() {
        let directory = TestDirectory::new();
        let mut store = open_store(&directory);
        let mut signed = evidence();
        signed.required_capabilities = BTreeSet::from(["database.read".to_owned()]);
        store
            .trust_publisher_key(
                signed.publisher.as_ref().expect("publisher"),
                &signed.signatures[0].key_id,
                &signed.signatures[0].public_key,
                "fixture approval",
            )
            .expect("trust key");
        store
            .set_persistent_grant(
                &signed,
                "database.read",
                CapabilityDecision::Allow,
                "required capability",
            )
            .expect("grant required capability");
        let decision = store
            .evaluate(&signed, &permissive_context())
            .expect("evaluate optional prompt");
        assert!(decision.executable_allowed);
        assert_eq!(
            decision.capabilities["database.write"].decision,
            CapabilityDecision::Prompt
        );

        let mut unsigned = signed;
        unsigned.publisher = None;
        unsigned.application_digest = None;
        unsigned.signatures.clear();
        unsigned.required_capabilities = BTreeSet::from(["database.read".to_owned()]);
        let mut once = permissive_context();
        once.trust_once = true;
        once.allow_once.insert("database.read".to_owned());
        assert!(
            store
                .evaluate(&unsigned, &once)
                .expect("session trust")
                .executable_allowed
        );
        assert!(
            !store
                .evaluate(&unsigned, &permissive_context())
                .expect("session trust expired")
                .executable_allowed
        );
    }

    #[test]
    fn exact_file_trust_does_not_cross_a_source_hash_change() {
        let directory = TestDirectory::new();
        let mut store = open_store(&directory);
        let mut unsigned = evidence();
        unsigned.publisher = None;
        unsigned.application_digest = None;
        unsigned.signatures.clear();
        store
            .trust_exact_file(&unsigned, "local development fixture")
            .expect("trust exact file");
        assert_eq!(
            store
                .evaluate(&unsigned, &permissive_context())
                .expect("trusted exact file")
                .trust_state,
            TrustState::LocallyTrustedExactRelease
        );
        unsigned.source_sha256 = [8_u8; 32];
        assert_eq!(
            store
                .evaluate(&unsigned, &permissive_context())
                .expect("changed exact file")
                .trust_state,
            TrustState::StructurallyVerifiedUnsigned
        );
    }

    #[test]
    fn explicit_denies_are_persisted_and_exactly_scoped() {
        let directory = TestDirectory::new();
        let mut store = open_store(&directory);
        let signed = evidence();
        store
            .deny_exact_release(
                &signed,
                &signed.signatures[0].key_id,
                "user denied this release",
            )
            .expect("deny exact release");
        assert_eq!(
            store
                .evaluate(&signed, &permissive_context())
                .expect("denied release")
                .trust_state,
            TrustState::DeniedByUser
        );

        let mut unsigned = signed.clone();
        unsigned.publisher = None;
        unsigned.application_digest = None;
        unsigned.signatures.clear();
        store
            .deny_exact_file(&unsigned, "user denied this file")
            .expect("deny exact file");
        assert_eq!(
            store
                .evaluate(&unsigned, &permissive_context())
                .expect("denied file")
                .trust_state,
            TrustState::DeniedByUser
        );
        unsigned.source_sha256 = [7_u8; 32];
        assert_eq!(
            store
                .evaluate(&unsigned, &permissive_context())
                .expect("different file")
                .trust_state,
            TrustState::StructurallyVerifiedUnsigned
        );
    }

    #[test]
    fn forgetting_an_unsigned_decision_is_confirmed_exact_and_does_not_grant_authority() {
        let directory = TestDirectory::new();
        let mut store = open_store(&directory);
        let mut current = evidence();
        current.publisher = None;
        current.application_digest = None;
        current.signatures.clear();
        let mut other = current.clone();
        other.source_sha256 = [7_u8; 32];
        store
            .deny_exact_file(&current, "current denial")
            .expect("deny current file");
        store
            .deny_exact_file(&other, "other denial")
            .expect("deny other file");

        assert!(matches!(
            store.forget_current_decision(&current, "wrong"),
            Err(PolicyError::Field)
        ));
        let forgotten = store
            .forget_current_decision(&current, "FORGET-CURRENT-DECISION")
            .expect("forget current decision");
        assert_eq!(
            forgotten,
            ForgottenDecisionReport {
                local_exceptions: 1,
                exact_releases: 0,
                capability_grants: 0,
            }
        );
        let current_decision = store
            .evaluate(&current, &permissive_context())
            .expect("current decision after forget");
        assert_eq!(
            current_decision.trust_state,
            TrustState::StructurallyVerifiedUnsigned
        );
        assert!(!current_decision.executable_allowed);
        assert_eq!(
            store
                .evaluate(&other, &permissive_context())
                .expect("other decision remains")
                .trust_state,
            TrustState::DeniedByUser
        );
        let audit = store.audit_events(10).expect("audit after forget");
        let forgotten_event = audit
            .iter()
            .find(|event| event.action == "decision.forget_exact")
            .expect("forget audit event");
        assert_eq!(forgotten_event.details["authority_granted"], false);
    }

    #[test]
    fn forgetting_a_signed_release_removes_only_its_exact_release_and_grants() {
        let directory = TestDirectory::new();
        let mut store = open_store(&directory);
        let signed = evidence();
        let grants = BTreeMap::from([
            ("database.read".to_owned(), CapabilityDecision::Allow),
            ("database.write".to_owned(), CapabilityDecision::Deny),
        ]);
        store
            .trust_exact_release_with_grants(
                &signed,
                &signed.signatures[0].key_id,
                &grants,
                "exact release fixture",
            )
            .expect("trust exact release");

        let forgotten = store
            .forget_current_decision(&signed, "FORGET-CURRENT-DECISION")
            .expect("forget signed decision");
        assert_eq!(
            forgotten,
            ForgottenDecisionReport {
                local_exceptions: 0,
                exact_releases: 1,
                capability_grants: 2,
            }
        );
        let decision = store
            .evaluate(&signed, &permissive_context())
            .expect("signed decision after forget");
        assert_eq!(
            decision.trust_state,
            TrustState::SignatureValidUnknownPublisher
        );
        assert!(!decision.executable_allowed);
        assert!(
            decision
                .capabilities
                .values()
                .all(|capability| capability.persisted_grant.is_none())
        );
    }

    #[test]
    fn release_and_complete_grant_set_commit_atomically() {
        let directory = TestDirectory::new();
        let mut store = open_store(&directory);
        let signed = evidence();
        let incomplete = BTreeMap::from([("database.read".to_owned(), CapabilityDecision::Allow)]);
        assert!(matches!(
            store.trust_exact_release_with_grants(
                &signed,
                &signed.signatures[0].key_id,
                &incomplete,
                "incomplete fixture",
            ),
            Err(PolicyError::Field)
        ));
        let after_rejection = store.export_redacted().expect("export rejection");
        assert!(
            after_rejection["exact_releases"]
                .as_array()
                .expect("releases")
                .is_empty()
        );
        assert!(
            after_rejection["grants"]
                .as_array()
                .expect("grants")
                .is_empty()
        );

        let complete = BTreeMap::from([
            ("database.read".to_owned(), CapabilityDecision::Allow),
            ("database.write".to_owned(), CapabilityDecision::Deny),
        ]);
        store
            .trust_exact_release_with_grants(
                &signed,
                &signed.signatures[0].key_id,
                &complete,
                "complete fixture",
            )
            .expect("atomic release and grants");
        let export = store.export_redacted().expect("export committed set");
        assert_eq!(export["exact_releases"].as_array().unwrap().len(), 1);
        assert_eq!(export["grants"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn key_ids_are_derived_and_cannot_be_rebound() {
        let directory = TestDirectory::new();
        let mut store = open_store(&directory);
        let signed = evidence();
        assert!(matches!(
            store.trust_publisher_key(
                signed.publisher.as_ref().expect("publisher"),
                "ed25519:sha256:not-the-key",
                &signed.signatures[0].public_key,
                "invalid fixture",
            ),
            Err(PolicyError::Field)
        ));
        store
            .trust_publisher_key(
                signed.publisher.as_ref().expect("publisher"),
                &signed.signatures[0].key_id,
                &signed.signatures[0].public_key,
                "valid fixture",
            )
            .expect("trust valid key");
        let other_publisher = PublisherEvidence {
            publisher_id: "org.example.other".to_owned(),
            publisher_name: "Other Publisher".to_owned(),
        };
        assert!(matches!(
            store.trust_publisher_key(
                &other_publisher,
                &signed.signatures[0].key_id,
                &signed.signatures[0].public_key,
                "rebind attempt",
            ),
            Err(PolicyError::Field)
        ));
    }

    #[test]
    fn redacted_export_and_audit_contain_no_private_key_material() {
        let directory = TestDirectory::new();
        let mut store = open_store(&directory);
        let signed = evidence();
        store
            .trust_publisher_key(
                signed.publisher.as_ref().expect("publisher"),
                &signed.signatures[0].key_id,
                &signed.signatures[0].public_key,
                "fixture approval",
            )
            .expect("trust key");
        store
            .trust_exact_release(&signed, &signed.signatures[0].key_id, "fixture release")
            .expect("trust release");
        store
            .set_persistent_grant(
                &signed,
                "database.read",
                CapabilityDecision::Allow,
                "fixture grant",
            )
            .expect("persist grant");
        store
            .evaluate(&signed, &permissive_context())
            .expect("observe capsule");
        store
            .connection
            .execute(
                "INSERT INTO backup_inventory \
                 (backup_id, source_capsule_id, canonical_path, database_digest, byte_length, \
                  created_at, verified_at) VALUES \
                 ('backup-1', ?1, 'C:/private/capsules/backup.sqlite', zeroblob(32), 4096, \
                  '2026-08-08T00:00:00Z', '2026-08-08T00:00:01Z')",
                [&signed.capsule_id],
            )
            .expect("record backup fixture");
        let export = store.export_redacted().expect("redacted export");
        assert_eq!(export["private_keys_present"], false);
        assert_eq!(export["store_path"], "redacted");
        for section in [
            "publishers",
            "keys",
            "delegations",
            "exact_releases",
            "local_exceptions",
            "capsule_identities",
            "grants",
            "revocation_bundles",
            "backups",
            "audit",
        ] {
            assert!(export[section].is_array(), "missing {section}");
        }
        assert_eq!(export["backups"][0]["canonical_path"], "redacted");
        assert!(!export.to_string().contains("public_key"));
        assert!(!export.to_string().contains("C:/private"));
        assert!(!store.audit_events(100).expect("audit").is_empty());
    }

    #[test]
    fn reset_requires_exact_confirmation_and_can_follow_a_verified_backup() {
        let directory = TestDirectory::new();
        let mut store = open_store(&directory);
        let signed = evidence();
        store
            .trust_publisher_key(
                signed.publisher.as_ref().expect("publisher"),
                &signed.signatures[0].key_id,
                &signed.signatures[0].public_key,
                "fixture approval",
            )
            .expect("trust key");
        assert!(matches!(
            store.reset_decisions("wrong"),
            Err(PolicyError::Field)
        ));
        let backup = directory.0.join("backup-before-reset.sqlite");
        store.backup_to(&backup).expect("verified backup");
        store
            .reset_decisions("ERASE-TRUST-DECISIONS")
            .expect("confirmed reset");
        assert!(
            store.export_redacted().expect("export")["publishers"]
                .as_array()
                .expect("publishers")
                .is_empty()
        );
        assert_eq!(
            store.audit_events(10).expect("audit")[0].action,
            "trust_store.reset"
        );
        TrustStore::open(&backup).expect("open verified backup");
    }
}

//! Shared fail-closed launch evidence for native hosts and administrative CLI.

use std::{collections::BTreeSet, fs::File, io::Read, path::Path};

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use sha2::{Digest, Sha256};
use sqlite_capsule_core::{CapsuleIdentity, InspectError, inspect_metadata};
use sqlite_capsule_crypto::{
    CryptoError, application_digest, publisher_identity, signature_inventory, verify_signatures,
};
use sqlite_capsule_policy::{LaunchEvidence, PolicyError, PublisherEvidence, SignatureEvidence};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LaunchError {
    #[error("capsule structure is invalid: {0}")]
    Structure(String),
    #[error("partial signed-app extension is not accepted")]
    PartialExtension,
    #[error("signature inventory changed during verification")]
    SignatureRace,
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

pub fn inspect_launch(path: &Path) -> Result<LaunchInspection, LaunchError> {
    let identity = verify_structure(path)?;
    let source_sha256 = file_sha256(&identity.canonical_path)?;
    let connection = open_read_only(&identity.canonical_path)?;
    let publisher_present = has_table(&connection, "capsule_publisher")?;
    let signature_present = has_table(&connection, "capsule_signature")?;
    if publisher_present != signature_present {
        return Err(LaunchError::PartialExtension);
    }
    let (application_digest, publisher, signatures) = if publisher_present {
        let publisher = publisher_identity(&connection)?;
        let digest = application_digest(&connection)?;
        let envelopes = signature_inventory(&connection)?;
        let reports = verify_signatures(&connection)?;
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
        source_sha256,
        application_digest,
        publisher,
        signatures,
        requested_capabilities,
        required_capabilities,
    };
    Ok(LaunchInspection { identity, evidence })
}

pub fn verify_structure(path: &Path) -> Result<CapsuleIdentity, LaunchError> {
    let identity = inspect_metadata(path)?;
    let connection = open_read_only(path)?;
    let integrity_messages = connection
        .prepare("PRAGMA integrity_check")?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    if integrity_messages != ["ok"] {
        return Err(LaunchError::Structure(format!(
            "SQLite integrity_check failed: {}",
            integrity_messages
                .into_iter()
                .take(10)
                .collect::<Vec<_>>()
                .join("; ")
        )));
    }
    let foreign_key_violations: i64 =
        connection.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })?;
    if foreign_key_violations != 0 {
        return Err(LaunchError::Structure(format!(
            "foreign-key violations: {foreign_key_violations}"
        )));
    }
    Ok(identity)
}

fn file_sha256(path: &Path) -> Result<[u8; 32], LaunchError> {
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
    Ok(digest.finalize().into())
}

fn open_read_only(path: &Path) -> rusqlite::Result<Connection> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA foreign_keys=ON;")?;
    Ok(connection)
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
    use super::*;

    #[test]
    fn checked_capsule_produces_unsigned_bounded_launch_evidence() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .join("capsules/diagram-studio.capsule.sqlite");
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
}

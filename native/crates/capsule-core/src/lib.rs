//! Product-independent native SQLite Capsule inspection.
//!
//! Inspection deliberately returns manifest identity only. It never reads or
//! releases executable assets and never accepts SQL from callers.

pub mod protocol;

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use serde_json::Value;
use thiserror::Error;

pub const SQLITE_CAPSULE_APPLICATION_ID: i64 = 1_129_337_676;
pub const MAX_CAPSULE_BYTES: u64 = 512 * 1024 * 1024;
const SQLITE_HEADER: &[u8; 16] = b"SQLite format 3\0";

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CapsuleIdentity {
    pub canonical_path: PathBuf,
    pub bytes: u64,
    pub application_id: i64,
    pub user_version: i64,
    pub format_id: String,
    pub format_version: String,
    pub runtime_protocol: String,
    pub capsule_id: String,
    pub app_id: String,
    pub app_version: String,
    pub title: String,
    pub summary: String,
    pub entry_asset: String,
    pub permissions: Value,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CapsuleHeader {
    pub canonical_path: PathBuf,
    pub bytes: u64,
    pub application_id: i64,
}

#[derive(Debug, Error)]
pub enum InspectError {
    #[error("capsule path does not exist")]
    Missing,
    #[error("capsule path is not a regular file")]
    NotFile,
    #[error("symbolic links are not accepted during metadata inspection")]
    SymbolicLink,
    #[error("capsule is empty or exceeds the {MAX_CAPSULE_BYTES} byte policy")]
    SizePolicy,
    #[error("file does not have a SQLite 3 header")]
    NotSqlite,
    #[error("capsule application_id is {actual}, expected {SQLITE_CAPSULE_APPLICATION_ID}")]
    ApplicationId { actual: i64 },
    #[error("capsule_manifest must be a table")]
    ManifestObject,
    #[error("capsule_manifest must contain exactly one row with id = 1")]
    ManifestCardinality,
    #[error(
        "unsupported capsule identity: user_version={user_version}, format={format_version}, runtime={runtime_protocol}"
    )]
    UnsupportedFormat {
        user_version: i64,
        format_version: String,
        runtime_protocol: String,
    },
    #[error("manifest format_id must be org.sqlite-capsule")]
    FormatId,
    #[error("manifest permissions_json must be a JSON object")]
    Permissions,
    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("manifest JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Inspect a capsule without executing application assets or declared checks.
pub fn inspect_metadata(path: impl AsRef<Path>) -> Result<CapsuleIdentity, InspectError> {
    let path = path.as_ref();
    let header = inspect_header(path)?;
    let canonical_path = header.canonical_path;
    let bytes = header.bytes;

    let connection = Connection::open_with_flags(
        &canonical_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA foreign_keys=ON;")?;

    let application_id: i64 =
        connection.pragma_query_value(None, "application_id", |row| row.get(0))?;
    if application_id != SQLITE_CAPSULE_APPLICATION_ID {
        return Err(InspectError::ApplicationId {
            actual: application_id,
        });
    }
    let user_version: i64 =
        connection.pragma_query_value(None, "user_version", |row| row.get(0))?;

    let manifest_type: Option<String> = connection
        .query_row(
            "SELECT type FROM sqlite_schema WHERE name = 'capsule_manifest'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if manifest_type.as_deref() != Some("table") {
        return Err(InspectError::ManifestObject);
    }

    let cardinality: i64 = connection.query_row(
        "SELECT count(*) FROM capsule_manifest WHERE id = 1",
        [],
        |row| row.get(0),
    )?;
    let total: i64 = connection.query_row("SELECT count(*) FROM capsule_manifest", [], |row| {
        row.get(0)
    })?;
    if cardinality != 1 || total != 1 {
        return Err(InspectError::ManifestCardinality);
    }

    let manifest = connection.query_row(
        "SELECT format_id, format_version, runtime_protocol, capsule_id, app_id, \
                app_version, title, summary, entry_asset, permissions_json \
         FROM capsule_manifest WHERE id = 1",
        [],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, String>(9)?,
            ))
        },
    )?;

    let (
        format_id,
        format_version,
        runtime_protocol,
        capsule_id,
        app_id,
        app_version,
        title,
        summary,
        entry_asset,
        permissions_json,
    ) = manifest;
    if format_id != "org.sqlite-capsule" {
        return Err(InspectError::FormatId);
    }
    let supported = (
        user_version,
        format_version.as_str(),
        runtime_protocol.as_str(),
    ) == (2, "0.2", "capsule-http/0.2");
    if !supported {
        return Err(InspectError::UnsupportedFormat {
            user_version,
            format_version,
            runtime_protocol,
        });
    }

    let permissions: Value = serde_json::from_str(&permissions_json)?;
    if !permissions.is_object() {
        return Err(InspectError::Permissions);
    }

    Ok(CapsuleIdentity {
        canonical_path,
        bytes,
        application_id,
        user_version,
        format_id,
        format_version,
        runtime_protocol,
        capsule_id,
        app_id,
        app_version,
        title,
        summary,
        entry_asset,
        permissions,
    })
}

/// Validate only filesystem policy, SQLite magic, and the fixed capsule
/// application ID. This does not open SQLite, execute schema text, or inspect
/// application assets, so a native host can safely recognize a capsule whose
/// rollback journal must be recovered before normal read-only inspection.
pub fn inspect_header(path: impl AsRef<Path>) -> Result<CapsuleHeader, InspectError> {
    let path = path.as_ref();
    if !path.exists() {
        return Err(InspectError::Missing);
    }

    let link_metadata = fs::symlink_metadata(path)?;
    if link_metadata.file_type().is_symlink() {
        return Err(InspectError::SymbolicLink);
    }
    if !link_metadata.is_file() {
        return Err(InspectError::NotFile);
    }
    let bytes = link_metadata.len();
    if bytes == 0 || bytes > MAX_CAPSULE_BYTES {
        return Err(InspectError::SizePolicy);
    }

    let mut file = File::open(path)?;
    let mut sqlite_magic = [0_u8; 16];
    if file.read_exact(&mut sqlite_magic).is_err() || &sqlite_magic != SQLITE_HEADER {
        return Err(InspectError::NotSqlite);
    }
    if bytes < 72 {
        return Err(InspectError::NotSqlite);
    }
    let mut application_id_bytes = [0_u8; 4];
    file.seek(SeekFrom::Start(68))?;
    file.read_exact(&mut application_id_bytes)?;
    let canonical_path = fs::canonicalize(path)?;
    let application_id = i64::from(u32::from_be_bytes(application_id_bytes));
    if application_id != SQLITE_CAPSULE_APPLICATION_ID {
        return Err(InspectError::ApplicationId {
            actual: application_id,
        });
    }
    Ok(CapsuleHeader {
        canonical_path,
        bytes,
        application_id,
    })
}

use rusqlite::OptionalExtension;

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use rusqlite::params;

    use super::*;

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

    struct TempPath(PathBuf);

    impl TempPath {
        fn new(suffix: &str) -> Self {
            let number = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            Self(std::env::temp_dir().join(format!(
                "sqlite-capsule-native-{}-{number}.{suffix}",
                std::process::id()
            )))
        }
    }

    impl Drop for TempPath {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    fn minimal_capsule(permissions: &str) -> TempPath {
        let path = TempPath::new("sqlitecapsule");
        let connection = Connection::open(&path.0).expect("create fixture");
        connection
            .pragma_update(None, "application_id", SQLITE_CAPSULE_APPLICATION_ID)
            .expect("application id");
        connection
            .pragma_update(None, "user_version", 2_i64)
            .expect("user version");
        connection
            .execute_batch(
                "CREATE TABLE capsule_manifest (\
                    id INTEGER PRIMARY KEY, format_id TEXT NOT NULL, \
                    format_version TEXT NOT NULL, runtime_protocol TEXT NOT NULL, \
                    capsule_id TEXT NOT NULL, app_id TEXT NOT NULL, \
                    app_version TEXT NOT NULL, title TEXT NOT NULL, \
                    summary TEXT NOT NULL, entry_asset TEXT NOT NULL, \
                    permissions_json TEXT NOT NULL\
                );",
            )
            .expect("schema");
        connection
            .execute(
                "INSERT INTO capsule_manifest VALUES \
                 (1, 'org.sqlite-capsule', '0.2', 'capsule-http/0.2', \
                  'urn:test', 'org.test', '1.0.0', 'Test capsule', \
                  'Metadata-only test fixture', 'app/index.html', ?1)",
                params![permissions],
            )
            .expect("manifest");
        drop(connection);
        path
    }

    #[test]
    fn inspects_supported_capsule_metadata_read_only() {
        let path = minimal_capsule(r#"{"database.read":{"required":true}}"#);
        let header = inspect_header(&path.0).expect("header probe");
        assert_eq!(header.application_id, SQLITE_CAPSULE_APPLICATION_ID);
        assert_eq!(header.bytes, fs::metadata(&path.0).expect("metadata").len());
        let identity = inspect_metadata(&path.0).expect("inspect");
        assert_eq!(identity.capsule_id, "urn:test");
        assert_eq!(identity.user_version, 2);
        assert_eq!(identity.permissions["database.read"]["required"], true);
    }

    #[test]
    fn rejects_non_sqlite_input_before_opening_it() {
        let path = TempPath::new("sqlitecapsule");
        fs::write(&path.0, b"not a sqlite database").expect("fixture");
        assert!(matches!(
            inspect_metadata(&path.0),
            Err(InspectError::NotSqlite)
        ));
    }

    #[test]
    fn rejects_non_object_permission_declarations() {
        let path = minimal_capsule("[]");
        assert!(matches!(
            inspect_metadata(&path.0),
            Err(InspectError::Permissions)
        ));
    }

    #[test]
    fn rejects_unsupported_format_triples() {
        let path = minimal_capsule("{}");
        let connection = Connection::open(&path.0).expect("open fixture");
        connection
            .execute(
                "UPDATE capsule_manifest SET format_version = '9.0' WHERE id = 1",
                [],
            )
            .expect("mutate fixture");
        drop(connection);
        assert!(matches!(
            inspect_metadata(&path.0),
            Err(InspectError::UnsupportedFormat { .. })
        ));
    }
}

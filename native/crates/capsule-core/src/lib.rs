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
pub const MAX_CAPSULE_BYTES: u64 = 64 * 1024 * 1024;
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
    pub overview: CapsuleOverview,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ApplicationReleaseIdentity {
    pub app_id: String,
    pub app_version: String,
    pub name: String,
    pub description: String,
    pub category: Option<String>,
    pub icon_asset: Option<String>,
    pub release_notes_doc: Option<String>,
    pub released_at: Option<String>,
    pub legacy_fallback: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CapsuleInstanceIdentity {
    pub capsule_id: String,
    pub revision_id: Option<String>,
    pub title: String,
    pub description: String,
    pub document_kind: String,
    pub tags: Vec<String>,
    pub icon_asset_id: Option<String>,
    pub cover_asset_id: Option<String>,
    pub created_at: String,
    pub content_updated_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DataSchemaIdentity {
    pub data_schema_id: String,
    pub data_schema_version: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CapsuleOverview {
    pub application: ApplicationReleaseIdentity,
    pub instance: CapsuleInstanceIdentity,
    pub data_schema: Option<DataSchemaIdentity>,
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
    #[error("v0.3 application and instance identity must each contain exactly one id=1 row")]
    IdentityCardinality,
    #[error("v0.3 identity metadata violates its bounded contract")]
    IdentityMetadata,
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
    let connection = Connection::open_with_flags(
        &header.canonical_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA foreign_keys=ON;")?;
    inspect_metadata_connection(header, &connection)
}

/// Project capsule identity from an already-opened connection and the header
/// evidence used to open it. Callers must open the connection read-only and
/// retain responsibility for binding the filesystem object across inspection.
pub fn inspect_metadata_connection(
    header: CapsuleHeader,
    connection: &Connection,
) -> Result<CapsuleIdentity, InspectError> {
    let canonical_path = header.canonical_path;
    let bytes = header.bytes;

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

    let (
        format_id,
        format_version,
        runtime_protocol,
        app_id,
        app_version,
        entry_asset,
        permissions_json,
    ) = connection.query_row(
        "SELECT format_id, format_version, runtime_protocol, app_id, app_version, \
                    entry_asset, permissions_json FROM capsule_manifest WHERE id = 1",
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
            ))
        },
    )?;
    if format_id != "org.sqlite-capsule" {
        return Err(InspectError::FormatId);
    }
    let minimum_host_profile = if user_version == 3 {
        Some(connection.query_row(
            "SELECT minimum_host_profile FROM capsule_manifest WHERE id = 1",
            [],
            |row| row.get::<_, String>(0),
        )?)
    } else {
        None
    };
    let supported = matches!(
        (
            user_version,
            format_version.as_str(),
            runtime_protocol.as_str(),
            minimum_host_profile.as_deref()
        ),
        (2, "0.2", "capsule-http/0.2", None)
            | (
                3,
                "0.3",
                "capsule-http/0.2",
                Some("org.sqlite-capsule.host-profile/0.3")
            )
    );
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

    let overview = if user_version == 2 {
        let (capsule_id, title, summary, created_at, updated_at): (
            String,
            String,
            String,
            String,
            String,
        ) = connection.query_row(
            "SELECT capsule_id, title, summary, created_at, updated_at \
             FROM capsule_manifest WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )?;
        CapsuleOverview {
            application: ApplicationReleaseIdentity {
                app_id: app_id.clone(),
                app_version: app_version.clone(),
                name: title.clone(),
                description: summary.clone(),
                category: None,
                icon_asset: None,
                release_notes_doc: None,
                released_at: None,
                legacy_fallback: true,
            },
            instance: CapsuleInstanceIdentity {
                capsule_id,
                revision_id: None,
                title,
                description: summary,
                document_kind: "legacy-v0.2".to_owned(),
                tags: Vec::new(),
                icon_asset_id: None,
                cover_asset_id: None,
                created_at,
                content_updated_at: updated_at,
            },
            data_schema: None,
        }
    } else {
        inspect_v03_overview(connection, &app_id, &app_version)?
    };
    let capsule_id = overview.instance.capsule_id.clone();
    let title = overview.instance.title.clone();
    let summary = overview.instance.description.clone();

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
        overview,
    })
}

fn inspect_v03_overview(
    connection: &Connection,
    app_id: &str,
    app_version: &str,
) -> Result<CapsuleOverview, InspectError> {
    let application_total: i64 =
        connection.query_row("SELECT count(*) FROM capsule_application", [], |row| {
            row.get(0)
        })?;
    let instance_total: i64 =
        connection.query_row("SELECT count(*) FROM capsule_instance", [], |row| {
            row.get(0)
        })?;
    if application_total != 1 || instance_total != 1 {
        return Err(InspectError::IdentityCardinality);
    }
    let (name, description, category, icon_asset, release_notes_doc): (
        String,
        String,
        String,
        Option<String>,
        Option<String>,
    ) = connection.query_row(
        "SELECT name, description, category, icon_asset, release_notes_doc \
         FROM capsule_application WHERE id = 1",
        [],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        },
    )?;
    let (
        capsule_id,
        revision_id,
        title,
        instance_description,
        document_kind,
        tags_json,
        icon_asset_id,
        cover_asset_id,
        created_at,
        content_updated_at,
    ) = connection.query_row(
        "SELECT capsule_id, revision_id, title, description, document_kind, tags_json, \
                icon_asset_id, cover_asset_id, created_at, content_updated_at \
         FROM capsule_instance WHERE id = 1",
        [],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, String>(9)?,
            ))
        },
    )?;
    let (data_schema_id, data_schema_version, released_at): (String, i64, String) = connection
        .query_row(
            "SELECT data_schema_id, data_schema_version, released_at \
             FROM capsule_manifest WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
    let tags: Vec<String> = serde_json::from_str(&tags_json)?;
    let tags_unique = tags.iter().collect::<std::collections::BTreeSet<_>>().len() == tags.len();
    if !valid_uuid(&capsule_id)
        || !valid_uuid(&revision_id)
        || !bounded(app_id, 1, 512)
        || !bounded(app_version, 1, 128)
        || !app_version.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (index > 0 && matches!(byte, b'.' | b'+' | b'_' | b'-'))
        })
        || !bounded(&name, 1, 256)
        || !bounded(&description, 0, 4096)
        || !bounded(&category, 1, 128)
        || !bounded(&title, 1, 512)
        || !bounded(&instance_description, 0, 8192)
        || !bounded(&document_kind, 1, 128)
        || !document_kind.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (index > 0 && b"._-".contains(&byte))
        })
        || tags.len() > 64
        || !tags_unique
        || tags.iter().any(|tag| !bounded(tag, 1, 128))
        || !utc_seconds(&created_at)
        || !utc_seconds(&content_updated_at)
        || !utc_seconds(&released_at)
        || data_schema_version < 1
        || !bounded(&data_schema_id, 1, 512)
        || icon_asset
            .as_deref()
            .is_some_and(|value| !bounded(value, 1, 1024))
        || release_notes_doc
            .as_deref()
            .is_some_and(|value| !bounded(value, 0, 1024))
        || icon_asset_id
            .as_deref()
            .is_some_and(|value| !bounded(value, 1, 256))
        || cover_asset_id
            .as_deref()
            .is_some_and(|value| !bounded(value, 1, 256))
    {
        return Err(InspectError::IdentityMetadata);
    }
    Ok(CapsuleOverview {
        application: ApplicationReleaseIdentity {
            app_id: app_id.to_owned(),
            app_version: app_version.to_owned(),
            name,
            description,
            category: Some(category),
            icon_asset,
            release_notes_doc,
            released_at: Some(released_at),
            legacy_fallback: false,
        },
        instance: CapsuleInstanceIdentity {
            capsule_id,
            revision_id: Some(revision_id),
            title,
            description: instance_description,
            document_kind,
            tags,
            icon_asset_id,
            cover_asset_id,
            created_at,
            content_updated_at,
        },
        data_schema: Some(DataSchemaIdentity {
            data_schema_id,
            data_schema_version,
        }),
    })
}

fn bounded(value: &str, minimum: usize, maximum: usize) -> bool {
    (minimum..=maximum).contains(&value.chars().count()) && value.len() <= maximum
}

fn valid_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && [8, 13, 18, 23]
            .into_iter()
            .all(|index| bytes[index] == b'-')
        && bytes.iter().enumerate().all(|(index, byte)| {
            [8, 13, 18, 23].contains(&index)
                || byte.is_ascii_digit()
                || (b'a'..=b'f').contains(byte)
        })
        && matches!(bytes[14], b'1'..=b'5')
        && matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
}

fn utc_seconds(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 20
        || !matches!(
            bytes,
            [
                _,
                _,
                _,
                _,
                b'-',
                _,
                _,
                b'-',
                _,
                _,
                b'T',
                _,
                _,
                b':',
                _,
                _,
                b':',
                _,
                _,
                b'Z'
            ]
        )
    {
        return false;
    }
    if ![0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18]
        .into_iter()
        .all(|index| bytes[index].is_ascii_digit())
    {
        return false;
    }

    let digits = |start: usize, end: usize| -> u32 {
        bytes[start..end]
            .iter()
            .fold(0, |value, byte| value * 10 + u32::from(byte - b'0'))
    };
    let year = digits(0, 4);
    let month = digits(5, 7);
    let day = digits(8, 10);
    let hour = digits(11, 13);
    let minute = digits(14, 16);
    let second = digits(17, 19);
    let leap_year =
        year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap_year => 29,
        2 => 28,
        _ => return false,
    };
    year != 0 && (1..=days_in_month).contains(&day) && hour <= 23 && minute <= 59 && second <= 59
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
    use std::io::Write;
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
                    permissions_json TEXT NOT NULL, created_at TEXT NOT NULL, \
                    updated_at TEXT NOT NULL\
                );",
            )
            .expect("schema");
        connection
            .execute(
                "INSERT INTO capsule_manifest VALUES \
                 (1, 'org.sqlite-capsule', '0.2', 'capsule-http/0.2', \
                  'urn:test', 'org.test', '1.0.0', 'Test capsule', \
                  'Metadata-only test fixture', 'app/index.html', ?1, \
                  '2026-08-08T00:00:00Z', '2026-08-08T00:00:00Z')",
                params![permissions],
            )
            .expect("manifest");
        drop(connection);
        path
    }

    fn v03_capsule() -> TempPath {
        let path = TempPath::new("sqlitecapsule");
        let connection = Connection::open(&path.0).expect("create v0.3 fixture");
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
        path
    }

    fn sized_header_fixture(bytes: u64) -> TempPath {
        let path = TempPath::new("sqlitecapsule");
        let mut file = File::create(&path.0).expect("create sized fixture");
        file.write_all(SQLITE_HEADER).expect("SQLite header");
        file.seek(SeekFrom::Start(68))
            .expect("application id offset");
        file.write_all(&(SQLITE_CAPSULE_APPLICATION_ID as u32).to_be_bytes())
            .expect("application id");
        file.set_len(bytes).expect("fixture length");
        drop(file);
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

    #[test]
    fn rejects_v03_with_the_wrong_minimum_host_profile() {
        let path = v03_capsule();
        inspect_metadata(&path.0).expect("normative v0.3 host profile is accepted");
        let connection = Connection::open(&path.0).expect("open v0.3 fixture");
        connection
            .execute_batch(
                "PRAGMA ignore_check_constraints=ON;
                 UPDATE capsule_manifest SET minimum_host_profile = \
                   'org.sqlite-capsule.host-profile/9.9' WHERE id = 1;
                 PRAGMA ignore_check_constraints=OFF;",
            )
            .expect("mutate minimum host profile");
        drop(connection);

        assert!(matches!(
            inspect_metadata(&path.0),
            Err(InspectError::UnsupportedFormat { .. })
        ));
    }

    #[test]
    fn rejects_every_v03_identity_metadata_boundary_family() {
        let cases = [
            (
                "application-id-bytes",
                "UPDATE capsule_manifest SET app_id = ?1 WHERE id = 1",
                "é".repeat(512),
            ),
            (
                "application-version-shape",
                "UPDATE capsule_manifest SET app_version = ?1 WHERE id = 1",
                "!".to_owned(),
            ),
            (
                "application-icon-reference",
                "UPDATE capsule_application SET icon_asset = ?1 WHERE id = 1",
                "é".repeat(1024),
            ),
            (
                "release-doc-reference",
                "UPDATE capsule_application SET release_notes_doc = ?1 WHERE id = 1",
                "é".repeat(1024),
            ),
            (
                "instance-icon-reference",
                "UPDATE capsule_instance SET icon_asset_id = ?1 WHERE id = 1",
                "é".repeat(256),
            ),
            (
                "instance-cover-reference",
                "UPDATE capsule_instance SET cover_asset_id = ?1 WHERE id = 1",
                "é".repeat(256),
            ),
            (
                "tag-count",
                "UPDATE capsule_instance SET tags_json = ?1 WHERE id = 1",
                serde_json::to_string(
                    &(0..65)
                        .map(|index| format!("tag-{index}"))
                        .collect::<Vec<_>>(),
                )
                .expect("tag JSON"),
            ),
        ];
        for (name, sql, value) in cases {
            let path = v03_capsule();
            let connection = Connection::open(&path.0).expect("open v0.3 fixture");
            connection
                .pragma_update(None, "foreign_keys", false)
                .expect("disable fixture foreign keys");
            connection
                .execute(sql, [value])
                .unwrap_or_else(|error| panic!("mutate {name}: {error}"));
            drop(connection);
            assert!(
                matches!(
                    inspect_metadata(&path.0),
                    Err(InspectError::IdentityMetadata)
                ),
                "accepted invalid v0.3 identity case {name}"
            );
        }
    }

    #[test]
    fn enforces_the_normative_64_mib_capsule_boundary() {
        let at_limit = sized_header_fixture(MAX_CAPSULE_BYTES);
        let header = inspect_header(&at_limit.0).expect("64 MiB boundary is admitted");
        assert_eq!(header.bytes, MAX_CAPSULE_BYTES);

        let over_limit = sized_header_fixture(MAX_CAPSULE_BYTES + 1);
        assert!(matches!(
            inspect_header(&over_limit.0),
            Err(InspectError::SizePolicy)
        ));
    }

    #[test]
    fn validates_exact_utc_seconds_as_calendar_timestamps() {
        assert!(utc_seconds("2024-02-29T23:59:59Z"));
        assert!(utc_seconds("2000-02-29T00:00:00Z"));
        for invalid in [
            "2023-02-29T00:00:00Z",
            "1900-02-29T00:00:00Z",
            "2026-13-01T00:00:00Z",
            "2026-04-31T00:00:00Z",
            "2026-01-01T24:00:00Z",
            "2026-01-01T00:60:00Z",
            "2026-01-01T00:00:60Z",
            "0000-01-01T00:00:00Z",
        ] {
            assert!(
                !utc_seconds(invalid),
                "accepted invalid timestamp {invalid}"
            );
        }
    }
}

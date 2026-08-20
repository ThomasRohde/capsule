//! Exhaustive, order-independent logical-state digest for compact duplicate.

use rusqlite::{Connection, types::ValueRef};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlite_capsule_launch::VerificationControl;

use crate::{
    CancellationToken, VerifiedCopySource, WorkspaceError, WorkspaceErrorCode, WorkspaceLimits,
};

pub const COMPACT_LOGICAL_STATE_PROFILE: &str = "org.sqlite-capsule.compact-logical-state/1";
const MAX_ROWS: u64 = 100_000;
const MAX_STREAM_BYTES: u64 = 512 * 1024 * 1024;
const MAX_OBJECTS: usize = 4_096;
const MAX_COLUMNS: usize = 256;
const PSEUDO_ROWID_FIELD: &str = "org.sqlite-capsule.compact.pseudo-rowid/1";
const ROWID_ALIASES: [&str; 3] = ["_rowid_", "rowid", "oid"];

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CompactLogicalState {
    pub profile: &'static str,
    pub digest_sha256: String,
    pub schema_objects: u16,
    pub tables: u16,
    pub rows: u64,
    pub stream_bytes: u64,
    pub page_size: u32,
}

/// Duplicate-only source authority with its exhaustive logical-state proof.
pub struct VerifiedCompactSource {
    source: VerifiedCopySource,
    logical_state: CompactLogicalState,
}

impl std::fmt::Debug for VerifiedCompactSource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VerifiedCompactSource")
            .field("identity", self.source.identity())
            .field("logical_state", &self.logical_state)
            .finish_non_exhaustive()
    }
}

impl VerifiedCompactSource {
    pub fn open(path: &std::path::Path) -> Result<Self, WorkspaceError> {
        Self::open_with_control(path, &WorkspaceLimits::default(), &CancellationToken::new())
    }

    pub fn open_with_control(
        path: &std::path::Path,
        limits: &WorkspaceLimits,
        cancellation: &CancellationToken,
    ) -> Result<Self, WorkspaceError> {
        let source = VerifiedCopySource::open_with_control(path, limits, cancellation)?;
        let _guard = source.start_control(source.verification_control())?;
        let logical_state =
            digest_connection(source.verified_connection(), source.verification_control())?;
        source.assert_current()?;
        Ok(Self {
            source,
            logical_state,
        })
    }

    pub fn identity(&self) -> &crate::CopySourceIdentity {
        self.source.identity()
    }

    pub fn logical_state(&self) -> &CompactLogicalState {
        &self.logical_state
    }

    pub fn assert_current(&self) -> Result<(), WorkspaceError> {
        self.source.assert_current()
    }

    pub fn assert_source_binding(
        &self,
        expected: &sqlite_capsule_lifecycle::SourceIdentity,
    ) -> Result<(), WorkspaceError> {
        self.source.assert_source_binding(expected)
    }

    pub(crate) fn source(&self) -> &VerifiedCopySource {
        &self.source
    }
}

struct SchemaRow {
    object_type: String,
    name: String,
    table_name: String,
    sql: Option<String>,
}

pub(crate) fn digest_connection(
    connection: &Connection,
    control: &VerificationControl,
) -> Result<CompactLogicalState, WorkspaceError> {
    control.check().map_err(map_control)?;
    let mut digest = Sha256::new();
    let mut streamed = 0_u64;
    frame(
        &mut digest,
        COMPACT_LOGICAL_STATE_PROFILE.as_bytes(),
        &mut streamed,
    )?;

    for (name, value) in [
        (
            "application_id",
            pragma_i64(connection, "application_id", control)?,
        ),
        (
            "user_version",
            pragma_i64(connection, "user_version", control)?,
        ),
        (
            "auto_vacuum",
            pragma_i64(connection, "auto_vacuum", control)?,
        ),
        (
            "default_cache_size",
            pragma_i64(connection, "default_cache_size", control)?,
        ),
    ] {
        frame(&mut digest, name.as_bytes(), &mut streamed)?;
        fixed(&mut digest, &value.to_be_bytes(), &mut streamed)?;
    }
    let encoding = pragma_text(connection, "encoding", control)?;
    frame(&mut digest, b"encoding", &mut streamed)?;
    frame(&mut digest, encoding.as_bytes(), &mut streamed)?;
    let page_size_i64 = pragma_i64(connection, "page_size", control)?;
    let page_size = u32::try_from(page_size_i64).map_err(|_| verification_failed())?;

    let schema = schema_rows(connection, control)?;
    if schema.len() > MAX_OBJECTS {
        return Err(limit_exceeded());
    }
    fixed(
        &mut digest,
        &(schema.len() as u64).to_be_bytes(),
        &mut streamed,
    )?;
    for row in &schema {
        frame(&mut digest, row.object_type.as_bytes(), &mut streamed)?;
        frame(&mut digest, row.name.as_bytes(), &mut streamed)?;
        frame(&mut digest, row.table_name.as_bytes(), &mut streamed)?;
        match &row.sql {
            Some(sql) => {
                fixed(&mut digest, &[1], &mut streamed)?;
                frame(&mut digest, sql.as_bytes(), &mut streamed)?;
            }
            None => fixed(&mut digest, &[0], &mut streamed)?,
        }
    }

    let mut table_names = schema
        .iter()
        .filter(|row| row.object_type == "table" && logical_table(&row.name))
        .map(|row| row.name.clone())
        .collect::<Vec<_>>();
    table_names.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    table_names.dedup();
    fixed(
        &mut digest,
        &(table_names.len() as u64).to_be_bytes(),
        &mut streamed,
    )?;

    let mut rows_total = 0_u64;
    for table in &table_names {
        if table.starts_with("sqlite_stat")
            && !matches!(
                table.as_str(),
                "sqlite_stat1" | "sqlite_stat2" | "sqlite_stat3" | "sqlite_stat4"
            )
        {
            return Err(WorkspaceError::new(
                WorkspaceErrorCode::UnsupportedOperation,
            ));
        }
        let columns = table_columns(connection, table, control)?;
        if columns.is_empty() || columns.len() > MAX_COLUMNS {
            return Err(if columns.len() > MAX_COLUMNS {
                limit_exceeded()
            } else {
                verification_failed()
            });
        }
        let without_rowid = table_without_rowid(connection, table, control)?;
        let rowid_alias = accessible_rowid_alias(&columns, without_rowid);
        let logical_column_count = columns.len() + usize::from(rowid_alias.is_some());
        let mut projections = Vec::with_capacity(logical_column_count);
        if let Some(alias) = rowid_alias {
            projections.push(quote_identifier(alias)?);
        }
        projections.extend(
            columns
                .iter()
                .map(|column| quote_identifier(column))
                .collect::<Result<Vec<_>, _>>()?,
        );
        let select = format!(
            "SELECT {} FROM {}",
            projections.join(","),
            quote_identifier(table)?
        );
        let mut statement = connection
            .prepare(&select)
            .map_err(|_| verification_failed())?;
        let mut rows = statement.query([]).map_err(|_| verification_failed())?;
        let mut row_digests = Vec::<[u8; 32]>::new();
        loop {
            control.check().map_err(map_control)?;
            let Some(row) = rows.next().map_err(|_| query_failed(control))? else {
                break;
            };
            rows_total = rows_total.checked_add(1).ok_or_else(limit_exceeded)?;
            if rows_total > MAX_ROWS {
                return Err(limit_exceeded());
            }
            let mut row_digest = Sha256::new();
            frame(&mut row_digest, b"row", &mut streamed)?;
            frame(&mut row_digest, table.as_bytes(), &mut streamed)?;
            fixed(
                &mut row_digest,
                &(logical_column_count as u64).to_be_bytes(),
                &mut streamed,
            )?;
            let value_offset = usize::from(rowid_alias.is_some());
            if rowid_alias.is_some() {
                frame(
                    &mut row_digest,
                    PSEUDO_ROWID_FIELD.as_bytes(),
                    &mut streamed,
                )?;
                let rowid = row.get_ref(0).map_err(|_| verification_failed())?;
                if !matches!(rowid, ValueRef::Integer(_)) {
                    return Err(verification_failed());
                }
                encode_value(&mut row_digest, rowid, &mut streamed)?;
            }
            for (index, column) in columns.iter().enumerate() {
                frame(&mut row_digest, column.as_bytes(), &mut streamed)?;
                encode_value(
                    &mut row_digest,
                    row.get_ref(index + value_offset)
                        .map_err(|_| verification_failed())?,
                    &mut streamed,
                )?;
            }
            row_digests.push(row_digest.finalize().into());
        }
        row_digests.sort_unstable();
        frame(&mut digest, b"table", &mut streamed)?;
        frame(&mut digest, table.as_bytes(), &mut streamed)?;
        fixed(
            &mut digest,
            &(logical_column_count as u64).to_be_bytes(),
            &mut streamed,
        )?;
        if rowid_alias.is_some() {
            frame(&mut digest, PSEUDO_ROWID_FIELD.as_bytes(), &mut streamed)?;
        }
        for column in &columns {
            frame(&mut digest, column.as_bytes(), &mut streamed)?;
        }
        fixed(
            &mut digest,
            &(row_digests.len() as u64).to_be_bytes(),
            &mut streamed,
        )?;
        for row_digest in row_digests {
            fixed(&mut digest, &row_digest, &mut streamed)?;
        }
    }
    control.check().map_err(map_control)?;
    Ok(CompactLogicalState {
        profile: COMPACT_LOGICAL_STATE_PROFILE,
        digest_sha256: lower_hex(&digest.finalize()),
        schema_objects: u16::try_from(schema.len()).map_err(|_| limit_exceeded())?,
        tables: u16::try_from(table_names.len()).map_err(|_| limit_exceeded())?,
        rows: rows_total,
        stream_bytes: streamed,
        page_size,
    })
}

fn schema_rows(
    connection: &Connection,
    control: &VerificationControl,
) -> Result<Vec<SchemaRow>, WorkspaceError> {
    let mut statement = connection
        .prepare(
            "SELECT type, name, tbl_name, sql FROM sqlite_schema \
             ORDER BY type COLLATE BINARY, name COLLATE BINARY, \
                      tbl_name COLLATE BINARY, sql COLLATE BINARY",
        )
        .map_err(|_| verification_failed())?;
    let mapped = statement
        .query_map([], |row| {
            Ok(SchemaRow {
                object_type: row.get(0)?,
                name: row.get(1)?,
                table_name: row.get(2)?,
                sql: row.get(3)?,
            })
        })
        .map_err(|_| query_failed(control))?;
    let mut result = Vec::new();
    for row in mapped {
        control.check().map_err(map_control)?;
        result.push(row.map_err(|_| verification_failed())?);
        if result.len() > MAX_OBJECTS {
            return Err(limit_exceeded());
        }
    }
    Ok(result)
}

fn table_columns(
    connection: &Connection,
    table: &str,
    control: &VerificationControl,
) -> Result<Vec<String>, WorkspaceError> {
    let mut statement = connection
        .prepare("SELECT cid, name FROM pragma_table_xinfo(?1) ORDER BY cid")
        .map_err(|_| verification_failed())?;
    let mapped = statement
        .query_map([table], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|_| query_failed(control))?;
    let mut result = Vec::new();
    for (expected_cid, item) in mapped.enumerate() {
        control.check().map_err(map_control)?;
        let (cid, name) = item.map_err(|_| verification_failed())?;
        if cid != i64::try_from(expected_cid).map_err(|_| limit_exceeded())?
            || name.is_empty()
            || name.as_bytes().contains(&0)
        {
            return Err(verification_failed());
        }
        result.push(name);
        if result.len() > MAX_COLUMNS {
            return Err(limit_exceeded());
        }
    }
    Ok(result)
}

fn table_without_rowid(
    connection: &Connection,
    table: &str,
    control: &VerificationControl,
) -> Result<bool, WorkspaceError> {
    let without_rowid = connection
        .query_row(
            "SELECT wr FROM pragma_table_list WHERE schema = 'main' AND name = ?1",
            [table],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|_| query_failed(control))?;
    match without_rowid {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(verification_failed()),
    }
}

fn accessible_rowid_alias(columns: &[String], without_rowid: bool) -> Option<&'static str> {
    if without_rowid {
        return None;
    }
    ROWID_ALIASES.into_iter().find(|alias| {
        columns
            .iter()
            .all(|column| !column.eq_ignore_ascii_case(alias))
    })
}

fn logical_table(name: &str) -> bool {
    !name.starts_with("sqlite_") || name == "sqlite_sequence" || name.starts_with("sqlite_stat")
}

fn encode_value(
    digest: &mut Sha256,
    value: ValueRef<'_>,
    streamed: &mut u64,
) -> Result<(), WorkspaceError> {
    match value {
        ValueRef::Null => fixed(digest, &[0], streamed),
        ValueRef::Integer(value) => {
            fixed(digest, &[1], streamed)?;
            fixed(digest, &value.to_be_bytes(), streamed)
        }
        ValueRef::Real(value) => {
            if !value.is_finite() {
                return Err(WorkspaceError::new(
                    WorkspaceErrorCode::UnsupportedOperation,
                ));
            }
            fixed(digest, &[2], streamed)?;
            fixed(digest, &value.to_bits().to_be_bytes(), streamed)
        }
        ValueRef::Text(value) => {
            fixed(digest, &[3], streamed)?;
            frame(digest, value, streamed)
        }
        ValueRef::Blob(value) => {
            fixed(digest, &[4], streamed)?;
            frame(digest, value, streamed)
        }
    }
}

fn pragma_i64(
    connection: &Connection,
    name: &str,
    control: &VerificationControl,
) -> Result<i64, WorkspaceError> {
    connection
        .pragma_query_value(None, name, |row| row.get(0))
        .map_err(|_| query_failed(control))
}

fn pragma_text(
    connection: &Connection,
    name: &str,
    control: &VerificationControl,
) -> Result<String, WorkspaceError> {
    connection
        .pragma_query_value(None, name, |row| row.get(0))
        .map_err(|_| query_failed(control))
}

fn quote_identifier(value: &str) -> Result<String, WorkspaceError> {
    if value.is_empty() || value.as_bytes().contains(&0) {
        return Err(verification_failed());
    }
    Ok(format!("\"{}\"", value.replace('"', "\"\"")))
}

fn frame(digest: &mut Sha256, bytes: &[u8], streamed: &mut u64) -> Result<(), WorkspaceError> {
    fixed(digest, &(bytes.len() as u64).to_be_bytes(), streamed)?;
    fixed(digest, bytes, streamed)
}

fn fixed(digest: &mut Sha256, bytes: &[u8], streamed: &mut u64) -> Result<(), WorkspaceError> {
    *streamed = streamed
        .checked_add(bytes.len() as u64)
        .ok_or_else(limit_exceeded)?;
    if *streamed > MAX_STREAM_BYTES {
        return Err(limit_exceeded());
    }
    digest.update(bytes);
    Ok(())
}

fn query_failed(control: &VerificationControl) -> WorkspaceError {
    control
        .check()
        .map_or_else(map_control, |_| verification_failed())
}

fn map_control(error: sqlite_capsule_launch::LaunchError) -> WorkspaceError {
    match error {
        sqlite_capsule_launch::LaunchError::Cancelled => {
            WorkspaceError::new(WorkspaceErrorCode::Cancelled)
        }
        sqlite_capsule_launch::LaunchError::LimitExceeded => limit_exceeded(),
        _ => verification_failed(),
    }
}

fn lower_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

const fn limit_exceeded() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::LimitExceeded)
}

const fn verification_failed() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::VerificationFailed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_matches_the_independent_compact_logical_state_vector() {
        let vectors: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../compatibility/compact-logical-state-v1/vectors.json"
        ))
        .expect("compact vectors JSON");
        let fixture =
            include_str!("../../../../compatibility/compact-logical-state-v1/fixture.sql");
        let connection = Connection::open_in_memory().expect("memory vector database");
        connection
            .execute_batch(fixture)
            .expect("compact vector fixture");
        let actual = digest_connection(&connection, &VerificationControl::default())
            .expect("Rust compact vector");
        let expected = &vectors["baseline"];
        assert_eq!(
            actual.digest_sha256,
            expected["digest_sha256"].as_str().unwrap()
        );
        assert_eq!(
            u64::from(actual.schema_objects),
            expected["schema_objects"].as_u64().unwrap()
        );
        assert_eq!(
            u64::from(actual.tables),
            expected["tables"].as_u64().unwrap()
        );
        assert_eq!(actual.rows, expected["rows"].as_u64().unwrap());
        assert_eq!(
            actual.stream_bytes,
            expected["stream_bytes"].as_u64().unwrap()
        );
        assert_eq!(
            u64::from(actual.page_size),
            expected["page_size"].as_u64().unwrap()
        );
    }

    #[test]
    fn digest_covers_rows_platform_signature_sequence_and_schema_but_not_storage_layout() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("logical.sqlite");
        let connection = Connection::open(&path).expect("logical fixture");
        connection
            .execute_batch(
                "CREATE TABLE domain(id INTEGER PRIMARY KEY AUTOINCREMENT, t TEXT, b BLOB, r REAL); \
                 CREATE TABLE capsule_profile(k TEXT PRIMARY KEY, v TEXT); \
                 CREATE TABLE capsule_signature(k TEXT PRIMARY KEY, v BLOB); \
                 CREATE INDEX domain_t ON domain(t); \
                 CREATE VIEW domain_view AS SELECT id,t FROM domain; \
                 INSERT INTO domain(t,b,r) VALUES ('a',X'00ff',-0.0),('b',X'10',1.5),(NULL,NULL,NULL); \
                 INSERT INTO capsule_profile VALUES ('title','one'); \
                 INSERT INTO capsule_signature VALUES ('key',X'0102');",
            )
            .expect("logical schema");
        let control = VerificationControl::default();
        let baseline = digest_connection(&connection, &control).expect("baseline digest");

        connection.execute_batch("VACUUM").expect("storage rewrite");
        assert_eq!(
            digest_connection(&connection, &VerificationControl::default())
                .expect("post-vacuum digest")
                .digest_sha256,
            baseline.digest_sha256
        );

        for mutation in [
            "UPDATE domain SET t='changed' WHERE id=1",
            "UPDATE capsule_profile SET v='two' WHERE k='title'",
            "UPDATE capsule_signature SET v=X'0304' WHERE k='key'",
            "UPDATE sqlite_sequence SET seq=99 WHERE name='domain'",
            "DROP VIEW domain_view; CREATE VIEW domain_view AS SELECT id,b FROM domain",
            "DROP INDEX domain_t; CREATE INDEX domain_t ON domain(t DESC)",
        ] {
            let before = digest_connection(&connection, &VerificationControl::default())
                .expect("before mutation");
            connection
                .execute_batch(mutation)
                .expect("logical mutation");
            let after = digest_connection(&connection, &VerificationControl::default())
                .expect("after mutation");
            assert_ne!(before.digest_sha256, after.digest_sha256, "{mutation}");
        }
    }

    #[test]
    fn deleted_gap_rowid_renumbering_changes_digest_without_primary_key_assumptions() {
        let connection = Connection::open_in_memory().expect("memory database");
        connection
            .execute_batch(
                "CREATE TABLE bag(v TEXT); \
                 INSERT INTO bag VALUES ('same'),('deleted-gap'),('same'); \
                 DELETE FROM bag WHERE rowid=2;",
            )
            .expect("bag fixture");
        let baseline =
            digest_connection(&connection, &VerificationControl::default()).expect("baseline");
        connection
            .execute_batch("VACUUM")
            .expect("rowid-renumbering vacuum");
        let renumbered =
            digest_connection(&connection, &VerificationControl::default()).expect("renumbered");
        assert_ne!(baseline.digest_sha256, renumbered.digest_sha256);
        connection
            .execute(
                "DELETE FROM bag WHERE rowid=(SELECT MIN(rowid) FROM bag)",
                [],
            )
            .expect("remove duplicate");
        let fewer = digest_connection(&connection, &VerificationControl::default()).expect("fewer");
        assert_ne!(renumbered.digest_sha256, fewer.digest_sha256);
    }

    #[test]
    fn rowid_projection_detects_without_rowid_and_shadowed_aliases() {
        let connection = Connection::open_in_memory().expect("memory database");
        connection
            .execute_batch(
                "CREATE TABLE keyed(k TEXT PRIMARY KEY) WITHOUT ROWID; \
                 INSERT INTO keyed VALUES ('key'); \
                 CREATE TABLE partial(rowid TEXT, v TEXT); \
                 INSERT INTO partial VALUES ('shadow','value'); \
                 CREATE TABLE fully(rowid TEXT, _rowid_ TEXT, oid TEXT, v TEXT); \
                 INSERT INTO fully VALUES ('r','u','o','value');",
            )
            .expect("rowid projection fixture");
        let control = VerificationControl::default();
        let keyed = table_columns(&connection, "keyed", &control).expect("keyed columns");
        assert!(table_without_rowid(&connection, "keyed", &control).expect("keyed table kind"));
        assert_eq!(accessible_rowid_alias(&keyed, true), None);
        let partial = table_columns(&connection, "partial", &control).expect("partial columns");
        assert_eq!(accessible_rowid_alias(&partial, false), Some("_rowid_"));
        let fully = table_columns(&connection, "fully", &control).expect("fully columns");
        assert_eq!(accessible_rowid_alias(&fully, false), None);
        digest_connection(&connection, &control).expect("shadow-safe digest");
    }

    #[test]
    fn known_sqlite_statistics_are_included_and_survive_storage_compaction() {
        let connection = Connection::open_in_memory().expect("memory database");
        connection
            .execute_batch(
                "CREATE TABLE measured(id INTEGER PRIMARY KEY, v TEXT); \
                 CREATE INDEX measured_v ON measured(v); \
                 INSERT INTO measured VALUES (1,'a'),(2,'b'),(3,'b'); \
                 ANALYZE;",
            )
            .expect("statistics fixture");
        let analyzed = digest_connection(&connection, &VerificationControl::default())
            .expect("analyzed digest");
        connection
            .execute_batch("VACUUM")
            .expect("compact statistics");
        assert_eq!(
            digest_connection(&connection, &VerificationControl::default())
                .expect("compacted statistics digest")
                .digest_sha256,
            analyzed.digest_sha256
        );
        connection
            .execute(
                "UPDATE sqlite_stat1 SET stat='hostile' WHERE idx='measured_v'",
                [],
            )
            .expect("mutate statistics");
        assert_ne!(
            digest_connection(&connection, &VerificationControl::default())
                .expect("mutated statistics digest")
                .digest_sha256,
            analyzed.digest_sha256
        );
    }

    #[test]
    fn real_bits_preserve_signed_zero_and_reject_non_finite_values() {
        let mut negative = Sha256::new();
        let mut negative_bytes = 0;
        encode_value(&mut negative, ValueRef::Real(-0.0), &mut negative_bytes)
            .expect("negative zero frame");
        let mut positive = Sha256::new();
        let mut positive_bytes = 0;
        encode_value(&mut positive, ValueRef::Real(0.0), &mut positive_bytes)
            .expect("positive zero frame");
        assert_ne!(negative.finalize(), positive.finalize());

        let mut non_finite = Sha256::new();
        let mut non_finite_bytes = 0;
        assert_eq!(
            encode_value(
                &mut non_finite,
                ValueRef::Real(f64::INFINITY),
                &mut non_finite_bytes,
            )
            .expect_err("non-finite values are unsupported")
            .kind(),
            WorkspaceErrorCode::UnsupportedOperation
        );
    }
}

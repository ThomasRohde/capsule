//! Publisher signatures over an immutable SQLite Capsule application compartment.
//!
//! The compartment signs platform/application schema and executable declarations,
//! while excluding domain rows, grants, change history, and signature envelopes.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rusqlite::{Connection, OptionalExtension, types::ValueRef};
use serde::{Deserialize, Deserializer, de};
use serde_json::{Map, Number, Value};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const PROFILE: &str = "org.sqlite-capsule.signed-app/0.2";
pub const ALGORITHM: &str = "ed25519";
pub const MAX_CANONICAL_STREAM_BYTES: usize = 512 * 1024 * 1024;
pub const MAX_CANONICAL_JSON_BYTES: usize = 1024 * 1024;
const STREAM_CONTEXT: &[u8] = b"SQLite Capsule signed-app canonical stream v1\0";
const SIGNATURE_CONTEXT: &[u8] = b"SQLite Capsule signed-app signature v1\0";

const SIGNED_TABLES: &[&str] = &[
    "capsule_manifest",
    "capsule_asset",
    "capsule_command",
    "capsule_runbook",
    "capsule_doc",
    "capsule_endpoint",
    "capsule_endpoint_step",
    "capsule_check",
    "capsule_prompt",
    "capsule_publisher",
];

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("unsupported capsule user_version for signed-app/0.2")]
    UnsupportedFormat,
    #[error("signed-app extension objects are missing or malformed")]
    Extension,
    #[error("unexpected capsule platform table: {0}")]
    UnexpectedPlatformTable(String),
    #[error("signed table has no primary key: {0}")]
    MissingPrimaryKey(String),
    #[error("canonical JSON is invalid, duplicated, or oversized")]
    Json,
    #[error("canonical stream exceeds the {MAX_CANONICAL_STREAM_BYTES} byte limit")]
    StreamTooLarge,
    #[error("non-finite SQLite real values cannot be signed")]
    NonFiniteReal,
    #[error("SQLite text is not valid UTF-8")]
    Utf8,
    #[error("signature key id does not match its public key")]
    KeyId,
    #[error("signed_at must be exact UTC RFC 3339 seconds")]
    SignedAt,
    #[error("invalid Ed25519 key or signature bytes")]
    Ed25519,
    #[error("Ed25519 signature verification failed")]
    Signature,
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublisherIdentity {
    pub publisher_id: String,
    pub publisher_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignatureEnvelope {
    pub key_id: String,
    pub public_key: [u8; 32],
    pub application_digest: [u8; 32],
    pub signature: [u8; 64],
    pub signed_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignatureVerification {
    pub key_id: String,
    pub cryptographically_valid: bool,
    pub digest_matches: bool,
    pub signed_at: String,
}

pub fn publisher_identity(connection: &Connection) -> Result<PublisherIdentity, CryptoError> {
    ensure_extension_schema(connection)?;
    let row = connection
        .query_row(
            "SELECT profile, publisher_id, publisher_name FROM capsule_publisher WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?;
    let Some((profile, publisher_id, publisher_name)) = row else {
        return Err(CryptoError::Extension);
    };
    let count: i64 = connection.query_row("SELECT count(*) FROM capsule_publisher", [], |row| {
        row.get(0)
    })?;
    if count != 1
        || profile != PROFILE
        || publisher_id.is_empty()
        || publisher_id.len() > 512
        || publisher_name.is_empty()
        || publisher_name.len() > 512
    {
        return Err(CryptoError::Extension);
    }
    Ok(PublisherIdentity {
        publisher_id,
        publisher_name,
    })
}

pub fn canonical_stream(connection: &Connection) -> Result<Vec<u8>, CryptoError> {
    publisher_identity(connection)?;
    ensure_signature_table(connection)?;
    let signed_tables = signed_tables(connection)?;
    reject_unknown_platform_tables(connection, signed_tables)?;

    let mut writer = CanonicalWriter::new();
    writer.raw(STREAM_CONTEXT)?;
    write_schema_records(connection, &mut writer)?;
    for table in signed_tables {
        write_table_rows(connection, table, &mut writer)?;
    }
    Ok(writer.finish())
}

pub fn application_digest(connection: &Connection) -> Result<[u8; 32], CryptoError> {
    Ok(Sha256::digest(canonical_stream(connection)?).into())
}

pub fn key_id(public_key: &[u8; 32]) -> String {
    format!("ed25519:sha256:{}", lower_hex(&Sha256::digest(public_key)))
}

pub fn sign_digest(
    signing_key: &SigningKey,
    application_digest: [u8; 32],
    signed_at: &str,
) -> Result<SignatureEnvelope, CryptoError> {
    validate_signed_at(signed_at)?;
    let public_key = signing_key.verifying_key().to_bytes();
    let message = signature_message(&application_digest, signed_at)?;
    let signature = signing_key.sign(&message).to_bytes();
    Ok(SignatureEnvelope {
        key_id: key_id(&public_key),
        public_key,
        application_digest,
        signature,
        signed_at: signed_at.to_owned(),
    })
}

pub fn verify_envelope(envelope: &SignatureEnvelope) -> Result<(), CryptoError> {
    if envelope.key_id != key_id(&envelope.public_key) {
        return Err(CryptoError::KeyId);
    }
    validate_signed_at(&envelope.signed_at)?;
    let verifying_key =
        VerifyingKey::from_bytes(&envelope.public_key).map_err(|_| CryptoError::Ed25519)?;
    let signature = Signature::from_bytes(&envelope.signature);
    let message = signature_message(&envelope.application_digest, &envelope.signed_at)?;
    verifying_key
        .verify(&message, &signature)
        .map_err(|_| CryptoError::Signature)
}

pub fn signature_inventory(connection: &Connection) -> Result<Vec<SignatureEnvelope>, CryptoError> {
    ensure_signature_table(connection)?;
    let mut statement = connection.prepare(
        "SELECT key_id, algorithm, public_key, application_digest, signature, signed_at \
         FROM capsule_signature ORDER BY key_id COLLATE BINARY",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Vec<u8>>(2)?,
            row.get::<_, Vec<u8>>(3)?,
            row.get::<_, Vec<u8>>(4)?,
            row.get::<_, String>(5)?,
        ))
    })?;

    let mut envelopes = Vec::new();
    for row in rows {
        let (key_id, algorithm, public_key, digest, signature, signed_at) = row?;
        if algorithm != ALGORITHM {
            return Err(CryptoError::Extension);
        }
        envelopes.push(SignatureEnvelope {
            key_id,
            public_key: public_key.try_into().map_err(|_| CryptoError::Extension)?,
            application_digest: digest.try_into().map_err(|_| CryptoError::Extension)?,
            signature: signature.try_into().map_err(|_| CryptoError::Extension)?,
            signed_at,
        });
    }
    Ok(envelopes)
}

pub fn verify_signatures(
    connection: &Connection,
) -> Result<Vec<SignatureVerification>, CryptoError> {
    let digest = application_digest(connection)?;
    Ok(signature_inventory(connection)?
        .into_iter()
        .map(|envelope| SignatureVerification {
            key_id: envelope.key_id.clone(),
            cryptographically_valid: verify_envelope(&envelope).is_ok(),
            digest_matches: envelope.application_digest == digest,
            signed_at: envelope.signed_at,
        })
        .collect())
}

fn ensure_signature_table(connection: &Connection) -> Result<(), CryptoError> {
    ensure_extension_schema(connection)
}

#[derive(Clone, Copy)]
struct ExpectedColumn {
    name: &'static str,
    declared_type: &'static str,
    not_null: bool,
    primary_key: i64,
}

const PUBLISHER_COLUMNS: &[ExpectedColumn] = &[
    ExpectedColumn {
        name: "id",
        declared_type: "INTEGER",
        not_null: false,
        primary_key: 1,
    },
    ExpectedColumn {
        name: "profile",
        declared_type: "TEXT",
        not_null: true,
        primary_key: 0,
    },
    ExpectedColumn {
        name: "publisher_id",
        declared_type: "TEXT",
        not_null: true,
        primary_key: 0,
    },
    ExpectedColumn {
        name: "publisher_name",
        declared_type: "TEXT",
        not_null: true,
        primary_key: 0,
    },
];

const SIGNATURE_COLUMNS: &[ExpectedColumn] = &[
    ExpectedColumn {
        name: "key_id",
        declared_type: "TEXT",
        not_null: true,
        primary_key: 1,
    },
    ExpectedColumn {
        name: "algorithm",
        declared_type: "TEXT",
        not_null: true,
        primary_key: 0,
    },
    ExpectedColumn {
        name: "public_key",
        declared_type: "BLOB",
        not_null: true,
        primary_key: 0,
    },
    ExpectedColumn {
        name: "application_digest",
        declared_type: "BLOB",
        not_null: true,
        primary_key: 0,
    },
    ExpectedColumn {
        name: "signature",
        declared_type: "BLOB",
        not_null: true,
        primary_key: 0,
    },
    ExpectedColumn {
        name: "signed_at",
        declared_type: "TEXT",
        not_null: true,
        primary_key: 0,
    },
];

fn ensure_extension_schema(connection: &Connection) -> Result<(), CryptoError> {
    ensure_table_columns(connection, "capsule_publisher", PUBLISHER_COLUMNS)?;
    ensure_table_columns(connection, "capsule_signature", SIGNATURE_COLUMNS)
}

fn ensure_table_columns(
    connection: &Connection,
    table: &str,
    expected: &[ExpectedColumn],
) -> Result<(), CryptoError> {
    let table_type: Option<String> = connection
        .query_row(
            "SELECT type FROM sqlite_schema WHERE name = ?1",
            [table],
            |row| row.get(0),
        )
        .optional()?;
    if table_type.as_deref() != Some("table") {
        return Err(CryptoError::Extension);
    }

    let mut statement =
        connection.prepare(&format!("PRAGMA table_xinfo({})", quote_identifier(table)))?;
    let actual = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if actual.len() != expected.len()
        || actual.iter().zip(expected).any(
            |((name, declared_type, not_null, primary_key, hidden), expected)| {
                name != expected.name
                    || !declared_type.eq_ignore_ascii_case(expected.declared_type)
                    || (*not_null != 0) != expected.not_null
                    || *primary_key != expected.primary_key
                    || *hidden != 0
            },
        )
    {
        return Err(CryptoError::Extension);
    }
    Ok(())
}

fn signed_tables(connection: &Connection) -> Result<&'static [&'static str], CryptoError> {
    let user_version: i64 =
        connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    (user_version == 2)
        .then_some(SIGNED_TABLES)
        .ok_or(CryptoError::UnsupportedFormat)
}

fn reject_unknown_platform_tables(
    connection: &Connection,
    signed_tables: &[&str],
) -> Result<(), CryptoError> {
    let mut allowed: BTreeSet<&str> = signed_tables.iter().copied().collect();
    allowed.extend(["capsule_grant", "capsule_change_log", "capsule_signature"]);
    let mut statement = connection.prepare(
        "SELECT name FROM sqlite_schema \
         WHERE type = 'table' AND name GLOB 'capsule_*' \
         ORDER BY name COLLATE BINARY",
    )?;
    let names = statement.query_map([], |row| row.get::<_, String>(0))?;
    for name in names {
        let name = name?;
        if !allowed.contains(name.as_str()) {
            return Err(CryptoError::UnexpectedPlatformTable(name));
        }
    }
    Ok(())
}

fn write_schema_records(
    connection: &Connection,
    writer: &mut CanonicalWriter,
) -> Result<(), CryptoError> {
    let mut statement = connection.prepare(
        "SELECT type, name, tbl_name, sql FROM sqlite_schema \
         WHERE name NOT LIKE 'sqlite\\_%' ESCAPE '\\' AND sql IS NOT NULL \
         ORDER BY type COLLATE BINARY, name COLLATE BINARY, tbl_name COLLATE BINARY",
    )?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let object_type: String = row.get(0)?;
        let name: String = row.get(1)?;
        let table_name: String = row.get(2)?;
        let sql: String = row.get(3)?;
        writer.record(
            1,
            &format!("schema/{object_type}/{name}"),
            &[
                ("type", CanonicalValue::Text(object_type.as_bytes())),
                ("name", CanonicalValue::Text(name.as_bytes())),
                ("table", CanonicalValue::Text(table_name.as_bytes())),
                ("sql", CanonicalValue::Text(sql.as_bytes())),
            ],
        )?;
    }
    Ok(())
}

fn write_table_rows(
    connection: &Connection,
    table: &str,
    writer: &mut CanonicalWriter,
) -> Result<(), CryptoError> {
    let columns = table_columns(connection, table)?;
    let mut primary_key: Vec<_> = columns
        .iter()
        .filter(|column| column.primary_key > 0)
        .collect();
    primary_key.sort_by_key(|column| column.primary_key);
    if primary_key.is_empty() {
        return Err(CryptoError::MissingPrimaryKey(table.to_owned()));
    }

    let select_columns = columns
        .iter()
        .map(|column| quote_identifier(&column.name))
        .collect::<Vec<_>>()
        .join(", ");
    let order_columns = primary_key
        .iter()
        .map(|column| format!("{} COLLATE BINARY", quote_identifier(&column.name)))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT {select_columns} FROM {} ORDER BY {order_columns}",
        quote_identifier(table)
    );
    let mut statement = connection.prepare(&sql)?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        writer.record_start(2, &format!("row/{table}"), columns.len())?;
        for (index, column) in columns.iter().enumerate() {
            let value = row.get_ref(index)?;
            writer.field(&column.name, sqlite_value(table, &column.name, value)?)?;
        }
    }
    Ok(())
}

#[derive(Debug)]
struct TableColumn {
    name: String,
    primary_key: i64,
}

fn table_columns(connection: &Connection, table: &str) -> Result<Vec<TableColumn>, CryptoError> {
    let mut statement =
        connection.prepare(&format!("PRAGMA table_info({})", quote_identifier(table)))?;
    let columns = statement
        .query_map([], |row| {
            Ok(TableColumn {
                name: row.get(1)?,
                primary_key: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if columns.is_empty() {
        return Err(CryptoError::Extension);
    }
    Ok(columns)
}

fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn is_json_column(table: &str, column: &str) -> bool {
    matches!(
        (table, column),
        ("capsule_manifest", "permissions_json")
            | ("capsule_command", "argv_json")
            | ("capsule_endpoint", "parameters_json")
            | ("capsule_check", "expected_json")
    )
}

fn sqlite_value<'a>(
    table: &str,
    column: &str,
    value: ValueRef<'a>,
) -> Result<CanonicalValue<'a>, CryptoError> {
    match value {
        ValueRef::Null => Ok(CanonicalValue::Null),
        ValueRef::Integer(value) => Ok(CanonicalValue::Integer(value)),
        ValueRef::Real(value) if value.is_finite() => Ok(CanonicalValue::Real(value)),
        ValueRef::Real(_) => Err(CryptoError::NonFiniteReal),
        ValueRef::Text(bytes) if is_json_column(table, column) => {
            Ok(CanonicalValue::Json(canonical_json(bytes)?))
        }
        ValueRef::Text(bytes) => {
            std::str::from_utf8(bytes).map_err(|_| CryptoError::Utf8)?;
            Ok(CanonicalValue::Text(bytes))
        }
        ValueRef::Blob(bytes) => Ok(CanonicalValue::Blob(bytes)),
    }
}

fn canonical_json(bytes: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if bytes.len() > MAX_CANONICAL_JSON_BYTES {
        return Err(CryptoError::Json);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| CryptoError::Json)?;
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let unique = UniqueJson::deserialize(&mut deserializer).map_err(|_| CryptoError::Json)?;
    deserializer.end().map_err(|_| CryptoError::Json)?;
    serde_json_canonicalizer::to_vec(&unique.0).map_err(|_| CryptoError::Json)
}

struct UniqueJson(Value);

impl<'de> Deserialize<'de> for UniqueJson {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(UniqueJsonVisitor)
    }
}

struct UniqueJsonVisitor;

impl<'de> de::Visitor<'de> for UniqueJsonVisitor {
    type Value = UniqueJson;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("JSON without duplicate object keys")
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(UniqueJson(Value::Null))
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(UniqueJson(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(UniqueJson(Value::Number(Number::from(value))))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(UniqueJson(Value::Number(Number::from(value))))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        let number = Number::from_f64(value).ok_or_else(|| E::custom("non-finite number"))?;
        Ok(UniqueJson(Value::Number(number)))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.visit_string(value.to_owned())
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(UniqueJson(Value::String(value)))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(UniqueJson(Value::Null))
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        UniqueJson::deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: de::SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element::<UniqueJson>()? {
            values.push(value.0);
        }
        Ok(UniqueJson(Value::Array(values)))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: de::MapAccess<'de>,
    {
        let mut values = BTreeMap::new();
        while let Some((key, value)) = map.next_entry::<String, UniqueJson>()? {
            if values.insert(key, value.0).is_some() {
                return Err(de::Error::custom("duplicate object key"));
            }
        }
        Ok(UniqueJson(Value::Object(
            values.into_iter().collect::<Map<String, Value>>(),
        )))
    }
}

enum CanonicalValue<'a> {
    Null,
    Integer(i64),
    Real(f64),
    Text(&'a [u8]),
    Blob(&'a [u8]),
    Json(Vec<u8>),
}

struct CanonicalWriter {
    bytes: Vec<u8>,
}

impl CanonicalWriter {
    fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    fn finish(self) -> Vec<u8> {
        self.bytes
    }

    fn raw(&mut self, value: &[u8]) -> Result<(), CryptoError> {
        if self.bytes.len().saturating_add(value.len()) > MAX_CANONICAL_STREAM_BYTES {
            return Err(CryptoError::StreamTooLarge);
        }
        self.bytes.extend_from_slice(value);
        Ok(())
    }

    fn byte(&mut self, value: u8) -> Result<(), CryptoError> {
        self.raw(&[value])
    }

    fn u32(&mut self, value: usize) -> Result<(), CryptoError> {
        let value = u32::try_from(value).map_err(|_| CryptoError::StreamTooLarge)?;
        self.raw(&value.to_be_bytes())
    }

    fn bytes(&mut self, value: &[u8]) -> Result<(), CryptoError> {
        let length = u64::try_from(value.len()).map_err(|_| CryptoError::StreamTooLarge)?;
        self.raw(&length.to_be_bytes())?;
        self.raw(value)
    }

    fn record(
        &mut self,
        tag: u8,
        name: &str,
        fields: &[(&str, CanonicalValue<'_>)],
    ) -> Result<(), CryptoError> {
        self.record_start(tag, name, fields.len())?;
        for (field_name, value) in fields {
            self.field(field_name, value.borrowed())?;
        }
        Ok(())
    }

    fn record_start(&mut self, tag: u8, name: &str, field_count: usize) -> Result<(), CryptoError> {
        self.byte(tag)?;
        self.bytes(name.as_bytes())?;
        self.u32(field_count)
    }

    fn field(&mut self, name: &str, value: CanonicalValue<'_>) -> Result<(), CryptoError> {
        self.bytes(name.as_bytes())?;
        match value {
            CanonicalValue::Null => {
                self.byte(0)?;
                self.bytes(&[])
            }
            CanonicalValue::Integer(value) => {
                self.byte(1)?;
                self.bytes(&value.to_be_bytes())
            }
            CanonicalValue::Real(value) => {
                self.byte(2)?;
                self.bytes(&value.to_bits().to_be_bytes())
            }
            CanonicalValue::Text(value) => {
                self.byte(3)?;
                self.bytes(value)
            }
            CanonicalValue::Blob(value) => {
                self.byte(4)?;
                self.bytes(value)
            }
            CanonicalValue::Json(value) => {
                self.byte(5)?;
                self.bytes(&value)
            }
        }
    }
}

impl CanonicalValue<'_> {
    fn borrowed(&self) -> CanonicalValue<'_> {
        match self {
            Self::Null => CanonicalValue::Null,
            Self::Integer(value) => CanonicalValue::Integer(*value),
            Self::Real(value) => CanonicalValue::Real(*value),
            Self::Text(value) => CanonicalValue::Text(value),
            Self::Blob(value) => CanonicalValue::Blob(value),
            Self::Json(value) => CanonicalValue::Json(value.clone()),
        }
    }
}

fn signature_message(digest: &[u8; 32], signed_at: &str) -> Result<Vec<u8>, CryptoError> {
    validate_signed_at(signed_at)?;
    let mut message = Vec::with_capacity(SIGNATURE_CONTEXT.len() + 32 + 8 + signed_at.len());
    message.extend_from_slice(SIGNATURE_CONTEXT);
    message.extend_from_slice(digest);
    message.extend_from_slice(&(signed_at.len() as u64).to_be_bytes());
    message.extend_from_slice(signed_at.as_bytes());
    Ok(message)
}

pub fn validate_signed_at(value: &str) -> Result<(), CryptoError> {
    let bytes = value.as_bytes();
    let punctuation = matches!(
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
    );
    let digit_positions = [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18];
    if bytes.len() != 20
        || !punctuation
        || !digit_positions
            .into_iter()
            .all(|index| bytes[index].is_ascii_digit())
    {
        return Err(CryptoError::SignedAt);
    }

    let year = decimal(bytes, 0, 4)?;
    let month = decimal(bytes, 5, 2)?;
    let day = decimal(bytes, 8, 2)?;
    let hour = decimal(bytes, 11, 2)?;
    let minute = decimal(bytes, 14, 2)?;
    let second = decimal(bytes, 17, 2)?;
    let leap_year = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap_year => 29,
        2 => 28,
        _ => return Err(CryptoError::SignedAt),
    };
    if year == 0 || day == 0 || day > days_in_month || hour > 23 || minute > 59 || second > 59 {
        return Err(CryptoError::SignedAt);
    }
    Ok(())
}

fn decimal(bytes: &[u8], start: usize, length: usize) -> Result<u32, CryptoError> {
    bytes[start..start + length]
        .iter()
        .try_fold(0_u32, |value, digit| {
            digit
                .is_ascii_digit()
                .then_some(value * 10 + u32::from(*digit - b'0'))
                .ok_or(CryptoError::SignedAt)
        })
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
    use rusqlite::params;

    use super::*;

    const V02_SCHEMA: &str = include_str!("../../../../format/capsule-v0.2.sql");
    const SIGNED_SCHEMA: &str = include_str!("../../../../format/capsule-signed-app-v0.2.sql");
    const GOLDEN_V02_DATA: &str =
        include_str!("../../../../compatibility/signed-app-v0.2/fixture-v0.2.sql");
    const GOLDEN_VECTORS: &str =
        include_str!("../../../../compatibility/signed-app-v0.2/vectors.json");

    fn fixture() -> Connection {
        let connection = Connection::open_in_memory().expect("memory database");
        connection.execute_batch(V02_SCHEMA).expect("v0.2 schema");
        connection
            .execute_batch(SIGNED_SCHEMA)
            .expect("signed extension schema");
        connection
            .execute(
                "INSERT INTO capsule_manifest VALUES \
                 (1, 'org.sqlite-capsule', '0.2', 'urn:test', 'Test', 'Summary', \
                  'org.test', '1.0.0', 'app/index.html', 'capsule-http/0.2', \
                  ?1, '2026-08-08T00:00:00Z', '2026-08-08T00:00:00Z')",
                [r#"{"database.write":{"required":true},"database.read":{"required":true}}"#],
            )
            .expect("manifest");
        connection
            .execute(
                "INSERT INTO capsule_publisher VALUES (1, ?1, 'org.example', 'Example Publisher')",
                [PROFILE],
            )
            .expect("publisher");
        connection
            .execute_batch(
                "CREATE TABLE domain_item (id TEXT PRIMARY KEY, value TEXT NOT NULL); \
                 INSERT INTO domain_item VALUES ('one', 'mutable');",
            )
            .expect("domain table");
        connection
    }

    fn golden_fixture(schema: &str, data: &str) -> Connection {
        let connection = Connection::open_in_memory().expect("memory database");
        connection.execute_batch(schema).expect("format schema");
        connection
            .execute_batch(SIGNED_SCHEMA)
            .expect("signed extension schema");
        connection.execute_batch(data).expect("golden fixture data");
        connection
    }

    #[test]
    fn rust_matches_the_independent_v02_golden_vector() {
        let declared: Value = serde_json::from_str(GOLDEN_VECTORS).expect("golden vector JSON");
        let fixtures = declared["fixtures"].as_array().expect("fixture array");
        for (index, (schema, data)) in [(V02_SCHEMA, GOLDEN_V02_DATA)].into_iter().enumerate() {
            let connection = golden_fixture(schema, data);
            let stream = canonical_stream(&connection).expect("canonical stream");
            let digest = Sha256::digest(&stream);
            assert_eq!(
                lower_hex(&digest),
                fixtures[index]["application_digest_sha256"]
                    .as_str()
                    .expect("declared digest")
            );
            assert_eq!(
                stream.len() as u64,
                fixtures[index]["canonical_stream_bytes"]
                    .as_u64()
                    .expect("declared stream size")
            );
        }
    }

    #[test]
    fn digest_ignores_domain_grant_change_and_signature_rows() {
        let connection = fixture();
        let before = application_digest(&connection).expect("digest");
        connection
            .execute(
                "UPDATE domain_item SET value = 'changed' WHERE id = 'one'",
                [],
            )
            .expect("domain change");
        connection
            .execute(
                "INSERT INTO capsule_grant VALUES ('database.write', 'allow', 'fixture', NULL)",
                [],
            )
            .expect("grant");
        connection
            .execute(
                "INSERT INTO capsule_change_log \
                 (endpoint_name, parameters_json, changed_rows, occurred_at) \
                 VALUES ('test', '{}', 1, '2026-08-08T00:00:00Z')",
                [],
            )
            .expect("log");
        let after = application_digest(&connection).expect("digest");
        assert_eq!(before, after);
    }

    #[test]
    fn digest_changes_for_application_or_schema_mutation() {
        let connection = fixture();
        let before = application_digest(&connection).expect("digest");
        connection
            .execute(
                "UPDATE capsule_manifest SET entry_asset = 'app/other.html' WHERE id = 1",
                [],
            )
            .expect("application change");
        let changed = application_digest(&connection).expect("digest");
        assert_ne!(before, changed);

        connection
            .execute_batch("CREATE INDEX idx_domain_value ON domain_item(value);")
            .expect("schema change");
        let schema_changed = application_digest(&connection).expect("digest");
        assert_ne!(changed, schema_changed);
    }

    #[test]
    fn canonical_json_ignores_property_order_and_rejects_duplicates() {
        let connection = fixture();
        let before = application_digest(&connection).expect("digest");
        connection
            .execute(
                "UPDATE capsule_manifest SET permissions_json = ?1 WHERE id = 1",
                [r#"{ "database.read": {"required":true}, "database.write":{"required":true} }"#],
            )
            .expect("reorder JSON");
        assert_eq!(before, application_digest(&connection).expect("digest"));

        connection
            .execute(
                "UPDATE capsule_manifest SET permissions_json = ?1 WHERE id = 1",
                [r#"{"database.read":{},"database.read":{}}"#],
            )
            .expect("duplicate JSON");
        assert!(matches!(
            application_digest(&connection),
            Err(CryptoError::Json)
        ));
    }

    #[test]
    fn ed25519_envelope_verifies_and_detects_tampering() {
        let digest = [7_u8; 32];
        let signing_key = SigningKey::from_bytes(&[3_u8; 32]);
        let mut envelope =
            sign_digest(&signing_key, digest, "2026-08-08T12:34:56Z").expect("sign envelope");
        verify_envelope(&envelope).expect("verify envelope");
        envelope.signature[0] ^= 1;
        assert!(matches!(
            verify_envelope(&envelope),
            Err(CryptoError::Signature)
        ));
    }

    #[test]
    fn signature_inventory_reports_digest_match_separately() {
        let connection = fixture();
        let digest = application_digest(&connection).expect("digest");
        let signing_key = SigningKey::from_bytes(&[9_u8; 32]);
        let envelope = sign_digest(&signing_key, digest, "2026-08-08T12:34:56Z").expect("sign");
        connection
            .execute(
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
            )
            .expect("signature row");
        let report = verify_signatures(&connection).expect("verification report");
        assert_eq!(report.len(), 1);
        assert!(report[0].cryptographically_valid);
        assert!(report[0].digest_matches);
    }

    #[test]
    fn signed_at_requires_a_real_utc_calendar_second() {
        let signing_key = SigningKey::from_bytes(&[5_u8; 32]);
        assert!(sign_digest(&signing_key, [0_u8; 32], "2024-02-29T23:59:59Z").is_ok());
        for invalid in [
            "0000-01-01T00:00:00Z",
            "2023-02-29T00:00:00Z",
            "2026-13-01T00:00:00Z",
            "2026-01-32T00:00:00Z",
            "2026-01-01T24:00:00Z",
            "2026-01-01T00:60:00Z",
            "2026-01-01T00:00:60Z",
            "2026-01-01T00:00:00+00:00",
        ] {
            assert!(matches!(
                sign_digest(&signing_key, [0_u8; 32], invalid),
                Err(CryptoError::SignedAt)
            ));
        }
    }

    #[test]
    fn signed_extension_rejects_extra_or_mistyped_columns() {
        let missing = Connection::open_in_memory().expect("memory database");
        missing.execute_batch(V02_SCHEMA).expect("v0.2 schema");
        missing
            .execute_batch(
                "CREATE TABLE capsule_publisher (
                    id INTEGER PRIMARY KEY,
                    profile TEXT NOT NULL,
                    publisher_id BLOB NOT NULL,
                    publisher_name TEXT NOT NULL,
                    extra TEXT
                );
                CREATE TABLE capsule_signature (
                    key_id TEXT PRIMARY KEY NOT NULL,
                    algorithm TEXT NOT NULL,
                    public_key BLOB NOT NULL,
                    application_digest BLOB NOT NULL,
                    signature BLOB NOT NULL,
                    signed_at TEXT NOT NULL
                );",
            )
            .expect("malformed extension");
        assert!(matches!(
            publisher_identity(&missing),
            Err(CryptoError::Extension)
        ));
    }
}

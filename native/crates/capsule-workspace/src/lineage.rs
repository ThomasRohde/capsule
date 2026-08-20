//! Bounded projection of mutable lineage claims.
//!
//! Lineage is useful provenance, but its rows are deliberately outside the
//! signed application compartment. This public projection labels that status
//! explicitly and never serializes raw `details_json`.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, params};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{EffectiveLimits, WorkspaceControl, WorkspaceError, WorkspaceErrorCode};

pub const LINEAGE_PROFILE: &str = "org.sqlite-capsule.lineage/0.3";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProvenanceStatus {
    MutableUntrusted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LineageReport {
    pub profile: &'static str,
    pub capsule_id: String,
    pub provenance_status: ProvenanceStatus,
    pub events: Vec<LineageEvent>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LineageEvent {
    pub event_id: String,
    pub sequence: u64,
    pub operation: LineageOperation,
    pub result_capsule_id: String,
    pub result_revision_id: String,
    pub occurred_at: String,
    pub application_digest: String,
    pub data_schema_id: String,
    pub data_schema_version: u64,
    pub plan_digest: String,
    pub details_sha256: String,
    pub details_property_count: usize,
    pub details_redacted: bool,
    pub parents: Vec<LineageParent>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LineageParent {
    pub relation: ParentRelation,
    pub capsule_id: Option<String>,
    pub revision_id: Option<String>,
    pub file_sha256: String,
}

macro_rules! string_enum {
    ($name:ident { $($variant:ident => $value:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
        #[serde(rename_all = "kebab-case")]
        pub enum $name { $($variant),+ }

        impl $name {
            fn parse(value: &str) -> Option<Self> {
                match value { $($value => Some(Self::$variant),)+ _ => None }
            }
        }
    };
}

string_enum!(LineageOperation {
    Created => "created",
    CreatedFromTemplate => "created-from-template",
    Fork => "fork",
    Reconcile => "reconcile",
    ApplicationUpgrade => "application-upgrade",
    Import => "import",
});

string_enum!(ParentRelation {
    CreatedFrom => "created-from",
    ForkedFrom => "forked-from",
    TargetDerivedFrom => "target-derived-from",
    ChangesAppliedFrom => "changes-applied-from",
    UpgradedFrom => "upgraded-from",
    ApplicationRelease => "application-release",
});

struct EventRow {
    event: LineageEvent,
    raw_details: String,
}

struct ParentRow {
    event_id: String,
    ordinal: usize,
    parent: LineageParent,
}

pub(crate) fn load(
    connection: &Connection,
    capsule_id: &str,
    limits: &EffectiveLimits,
    control: &WorkspaceControl,
) -> Result<LineageReport, WorkspaceError> {
    control.install(connection)?;
    let result = load_inner(connection, capsule_id, limits, control);
    let _ = connection.progress_handler(0, None::<fn() -> bool>);
    control.check()?;
    result
}

fn load_inner(
    connection: &Connection,
    capsule_id: &str,
    limits: &EffectiveLimits,
    control: &WorkspaceControl,
) -> Result<LineageReport, WorkspaceError> {
    validate_uuid(capsule_id)?;
    let mut events = load_events(connection, limits, control)?;
    let parents = load_parents(connection, limits, control)?;
    let event_ids: BTreeSet<_> = events
        .iter()
        .map(|row| row.event.event_id.clone())
        .collect();
    if event_ids.len() != events.len() {
        return Err(invalid_lineage());
    }

    let mut parents_by_event: BTreeMap<String, Vec<ParentRow>> = BTreeMap::new();
    for parent in parents {
        control.check()?;
        if !event_ids.contains(&parent.event_id) {
            return Err(invalid_lineage());
        }
        parents_by_event
            .entry(parent.event_id.clone())
            .or_default()
            .push(parent);
    }

    let mut projected = Vec::with_capacity(events.len());
    for (index, mut row) in events.drain(..).enumerate() {
        control.check()?;
        if row.event.sequence != u64::try_from(index + 1).map_err(|_| limit_exceeded())? {
            return Err(invalid_lineage());
        }
        let mut parents = parents_by_event
            .remove(&row.event.event_id)
            .unwrap_or_default();
        if parents.len() > limits.max_lineage_parents_per_event {
            return Err(limit_exceeded());
        }
        parents.sort_by_key(|parent| parent.ordinal);
        if parents
            .iter()
            .enumerate()
            .any(|(ordinal, parent)| parent.ordinal != ordinal + 1)
        {
            return Err(invalid_lineage());
        }
        row.event.parents = parents.into_iter().map(|parent| parent.parent).collect();
        row.event.details_sha256 = lower_hex(&Sha256::digest(row.raw_details.as_bytes()));
        projected.push(row.event);
    }
    if !parents_by_event.is_empty() {
        return Err(invalid_lineage());
    }

    Ok(LineageReport {
        profile: LINEAGE_PROFILE,
        capsule_id: capsule_id.to_owned(),
        provenance_status: ProvenanceStatus::MutableUntrusted,
        events: projected,
    })
}

fn load_events(
    connection: &Connection,
    limits: &EffectiveLimits,
    control: &WorkspaceControl,
) -> Result<Vec<EventRow>, WorkspaceError> {
    let mut statement = connection
        .prepare(
            "SELECT \
             CASE WHEN length(CAST(event_id AS BLOB)) = 36 THEN event_id END, sequence, \
             CASE WHEN length(CAST(operation AS BLOB)) BETWEEN 1 AND 32 THEN operation END, \
             CASE WHEN length(CAST(result_capsule_id AS BLOB)) = 36 THEN result_capsule_id END, \
             CASE WHEN length(CAST(result_revision_id AS BLOB)) = 36 THEN result_revision_id END, \
             CASE WHEN length(CAST(occurred_at AS BLOB)) = 20 THEN occurred_at END, \
             CASE WHEN length(CAST(application_digest AS BLOB)) = 64 THEN application_digest END, \
             CASE WHEN length(CAST(data_schema_id AS BLOB)) BETWEEN 1 AND 512 THEN data_schema_id END, \
             data_schema_version, \
             CASE WHEN length(CAST(plan_digest AS BLOB)) = 64 THEN plan_digest END, \
             CASE WHEN length(CAST(details_json AS BLOB)) <= ?1 THEN details_json END \
             FROM capsule_lineage_event ORDER BY sequence, event_id COLLATE BINARY LIMIT ?2",
        )
        .map_err(|_| invalid_lineage())?;
    let mut rows = statement
        .query(params![
            i64::try_from(limits.max_lineage_details_bytes).map_err(|_| limit_exceeded())?,
            limit_parameter(limits.max_lineage_events)?
        ])
        .map_err(|_| invalid_lineage())?;
    let mut result = Vec::new();
    while let Some(row) = rows.next().map_err(|_| invalid_lineage())? {
        control.check()?;
        if result.len() == limits.max_lineage_events {
            return Err(limit_exceeded());
        }
        let event_id = exact_text(row, 0)?;
        let sequence: i64 = row.get(1).map_err(|_| invalid_lineage())?;
        let operation = bounded_text(row, 2)?;
        let result_capsule_id = exact_text(row, 3)?;
        let result_revision_id = exact_text(row, 4)?;
        let occurred_at = exact_text(row, 5)?;
        let application_digest = exact_text(row, 6)?;
        let data_schema_id = bounded_text(row, 7)?;
        let data_schema_version: i64 = row.get(8).map_err(|_| invalid_lineage())?;
        let plan_digest = exact_text(row, 9)?;
        let raw_details = bounded_text(row, 10)?;

        validate_uuid(&event_id)?;
        validate_uuid(&result_capsule_id)?;
        validate_uuid(&result_revision_id)?;
        validate_utc_seconds(&occurred_at)?;
        validate_sha256(&application_digest)?;
        validate_sha256(&plan_digest)?;
        if data_schema_id.is_empty() || data_schema_id.len() > 512 {
            return Err(invalid_lineage());
        }
        let sequence = u64::try_from(sequence).map_err(|_| invalid_lineage())?;
        let data_schema_version =
            u64::try_from(data_schema_version).map_err(|_| invalid_lineage())?;
        if sequence == 0 || data_schema_version == 0 {
            return Err(invalid_lineage());
        }
        let details: Value = serde_json::from_str(&raw_details).map_err(|_| invalid_lineage())?;
        let object = details.as_object().ok_or_else(invalid_lineage)?;
        let property_count = total_properties(&details);
        if json_depth(&details) > limits.max_lineage_details_depth
            || object.len() > limits.max_lineage_detail_properties
            || property_count > limits.max_lineage_detail_properties
        {
            return Err(limit_exceeded());
        }
        result.push(EventRow {
            event: LineageEvent {
                event_id,
                sequence,
                operation: LineageOperation::parse(&operation).ok_or_else(invalid_lineage)?,
                result_capsule_id,
                result_revision_id,
                occurred_at,
                application_digest,
                data_schema_id,
                data_schema_version,
                plan_digest,
                details_sha256: String::new(),
                details_property_count: property_count,
                details_redacted: true,
                parents: Vec::new(),
            },
            raw_details,
        });
    }
    Ok(result)
}

fn load_parents(
    connection: &Connection,
    limits: &EffectiveLimits,
    control: &WorkspaceControl,
) -> Result<Vec<ParentRow>, WorkspaceError> {
    let maximum = limits
        .max_lineage_events
        .checked_mul(limits.max_lineage_parents_per_event)
        .ok_or_else(limit_exceeded)?;
    let mut statement = connection
        .prepare(
            "SELECT \
             CASE WHEN length(CAST(event_id AS BLOB)) = 36 THEN event_id END, ordinal, \
             CASE WHEN length(CAST(relation AS BLOB)) BETWEEN 1 AND 32 THEN relation END, \
             CASE WHEN parent_capsule_id IS NULL OR length(CAST(parent_capsule_id AS BLOB)) = 36 THEN parent_capsule_id END, \
             parent_capsule_id IS NULL, \
             CASE WHEN parent_revision_id IS NULL OR length(CAST(parent_revision_id AS BLOB)) = 36 THEN parent_revision_id END, \
             parent_revision_id IS NULL, \
             CASE WHEN length(CAST(parent_file_sha256 AS BLOB)) = 64 THEN parent_file_sha256 END \
             FROM capsule_lineage_parent ORDER BY event_id COLLATE BINARY, ordinal LIMIT ?1",
        )
        .map_err(|_| invalid_lineage())?;
    let mut rows = statement
        .query([limit_parameter(maximum)?])
        .map_err(|_| invalid_lineage())?;
    let mut result = Vec::new();
    while let Some(row) = rows.next().map_err(|_| invalid_lineage())? {
        control.check()?;
        if result.len() == maximum {
            return Err(limit_exceeded());
        }
        let event_id = exact_text(row, 0)?;
        let ordinal: i64 = row.get(1).map_err(|_| invalid_lineage())?;
        let relation = bounded_text(row, 2)?;
        let capsule_id: Option<String> = row.get(3).map_err(|_| invalid_lineage())?;
        let capsule_id_is_null: bool = row.get(4).map_err(|_| invalid_lineage())?;
        let revision_id: Option<String> = row.get(5).map_err(|_| invalid_lineage())?;
        let revision_id_is_null: bool = row.get(6).map_err(|_| invalid_lineage())?;
        let file_sha256 = exact_text(row, 7)?;
        validate_uuid(&event_id)?;
        if capsule_id.is_none() != capsule_id_is_null
            || revision_id.is_none() != revision_id_is_null
        {
            return Err(invalid_lineage());
        }
        if let Some(value) = &capsule_id {
            validate_uuid(value)?;
        }
        if let Some(value) = &revision_id {
            validate_uuid(value)?;
        }
        validate_sha256(&file_sha256)?;
        let ordinal = usize::try_from(ordinal).map_err(|_| invalid_lineage())?;
        if ordinal == 0 {
            return Err(invalid_lineage());
        }
        result.push(ParentRow {
            event_id,
            ordinal,
            parent: LineageParent {
                relation: ParentRelation::parse(&relation).ok_or_else(invalid_lineage)?,
                capsule_id,
                revision_id,
                file_sha256,
            },
        });
    }
    Ok(result)
}

fn validate_uuid(value: &str) -> Result<(), WorkspaceError> {
    let bytes = value.as_bytes();
    if bytes.len() != 36
        || [8, 13, 18, 23].iter().any(|index| bytes[*index] != b'-')
        || !matches!(bytes[14], b'1'..=b'5')
        || !matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
        || bytes.iter().enumerate().any(|(index, byte)| {
            ![8, 13, 18, 23].contains(&index)
                && !(byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        })
    {
        return Err(invalid_lineage());
    }
    Ok(())
}

fn validate_sha256(value: &str) -> Result<(), WorkspaceError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(invalid_lineage());
    }
    Ok(())
}

fn validate_utc_seconds(value: &str) -> Result<(), WorkspaceError> {
    let bytes = value.as_bytes();
    if bytes.len() != 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'Z'
        || bytes
            .iter()
            .enumerate()
            .any(|(index, byte)| ![4, 7, 10, 13, 16, 19].contains(&index) && !byte.is_ascii_digit())
    {
        return Err(invalid_lineage());
    }
    let year = decimal(bytes, 0, 4)?;
    let month = decimal(bytes, 5, 7)?;
    let day = decimal(bytes, 8, 10)?;
    let hour = decimal(bytes, 11, 13)?;
    let minute = decimal(bytes, 14, 16)?;
    let second = decimal(bytes, 17, 19)?;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return Err(invalid_lineage()),
    };
    if year == 0 || day == 0 || day > days || hour > 23 || minute > 59 || second > 59 {
        return Err(invalid_lineage());
    }
    Ok(())
}

fn decimal(bytes: &[u8], start: usize, end: usize) -> Result<u32, WorkspaceError> {
    bytes[start..end].iter().try_fold(0_u32, |value, byte| {
        value
            .checked_mul(10)
            .and_then(|value| value.checked_add(u32::from(byte - b'0')))
            .ok_or_else(invalid_lineage)
    })
}

fn json_depth(value: &Value) -> usize {
    match value {
        Value::Array(values) => 1 + values.iter().map(json_depth).max().unwrap_or(0),
        Value::Object(values) => 1 + values.values().map(json_depth).max().unwrap_or(0),
        _ => 0,
    }
}

fn total_properties(value: &Value) -> usize {
    match value {
        Value::Array(values) => values.iter().map(total_properties).sum(),
        Value::Object(values) => {
            values.len() + values.values().map(total_properties).sum::<usize>()
        }
        _ => 0,
    }
}

fn bounded_text(row: &rusqlite::Row<'_>, index: usize) -> Result<String, WorkspaceError> {
    row.get::<_, Option<String>>(index)
        .map_err(|_| invalid_lineage())?
        .ok_or_else(limit_exceeded)
}

fn exact_text(row: &rusqlite::Row<'_>, index: usize) -> Result<String, WorkspaceError> {
    row.get::<_, Option<String>>(index)
        .map_err(|_| invalid_lineage())?
        .ok_or_else(invalid_lineage)
}

fn limit_parameter(maximum: usize) -> Result<i64, WorkspaceError> {
    i64::try_from(maximum.checked_add(1).ok_or_else(limit_exceeded)?).map_err(|_| limit_exceeded())
}

fn lower_hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(text, "{byte:02x}").expect("writing hexadecimal to String cannot fail");
    }
    text
}

const fn invalid_lineage() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::InvalidCapsule)
}

const fn limit_exceeded() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::LimitExceeded)
}

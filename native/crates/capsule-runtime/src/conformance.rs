use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlite_capsule_launch::verify_structure;

use crate::{
    MAX_ASSET_BYTES, RuntimeError, VerificationReport, endpoint, lower_hex, safe_asset_path,
    safe_media_type,
};

const V02_CONFORMANCE: &str = include_str!("../../../../format/capsule-v0.2.conformance.json");

#[derive(Debug, Deserialize)]
struct Contract {
    required_objects: ObjectContracts,
    #[serde(default)]
    optional_objects: ObjectContracts,
    #[serde(default)]
    minimum_nonempty_tables: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ObjectContracts {
    #[serde(default)]
    tables: BTreeMap<String, TableContract>,
    #[serde(default)]
    views: BTreeMap<String, ViewContract>,
}

#[derive(Debug, Deserialize)]
struct TableContract {
    primary_key: Vec<String>,
    #[serde(default)]
    foreign_keys: Vec<ForeignKeyContract>,
    columns: BTreeMap<String, ColumnContract>,
}

#[derive(Debug, Deserialize)]
struct ForeignKeyContract {
    from: String,
    table: String,
    to: String,
}

#[derive(Debug, Deserialize)]
struct ColumnContract {
    #[serde(rename = "type")]
    column_type: String,
    notnull: bool,
    #[serde(default)]
    pk: i64,
}

#[derive(Debug, Deserialize)]
struct ViewContract {
    columns: Vec<String>,
}

#[derive(Debug)]
struct ColumnInfo {
    name: String,
    column_type: String,
    notnull: bool,
    pk: i64,
}

pub(crate) fn verify(
    connection: &Connection,
    identity: &sqlite_capsule_core::CapsuleIdentity,
) -> Result<VerificationReport, RuntimeError> {
    // `verify_structure` opens independently and provides an additional path-
    // level check. The checks below repeat integrity/FK on this exact runtime
    // connection before any asset can be returned.
    verify_structure(&identity.canonical_path)?;
    let integrity = connection
        .prepare("PRAGMA integrity_check")?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    if integrity != ["ok"] {
        return Err(RuntimeError::Verification(format!(
            "integrity_check: {}",
            integrity
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
        return Err(RuntimeError::Verification(format!(
            "foreign-key violations: {foreign_key_violations}"
        )));
    }

    let contract: Contract = serde_json::from_str(V02_CONFORMANCE)?;
    let mut errors = Vec::new();
    verify_no_triggers(connection, &mut errors)?;
    for (name, table) in &contract.required_objects.tables {
        verify_table(connection, name, table, true, &mut errors)?;
    }
    for (name, table) in &contract.optional_objects.tables {
        verify_table(connection, name, table, false, &mut errors)?;
    }
    for (name, view) in &contract.required_objects.views {
        verify_view(connection, name, view, &mut errors)?;
    }
    for table in &contract.minimum_nonempty_tables {
        let sql = format!("SELECT count(*) FROM {}", quote_identifier(table));
        let count: i64 = connection.query_row(&sql, [], |row| row.get(0))?;
        if count == 0 {
            errors.push(format!("required content table {table} is empty"));
        }
    }
    verify_assets(connection, identity, &mut errors)?;
    verify_commands(connection, &mut errors)?;
    endpoint::verify_declarations(connection, identity, &mut errors)?;
    let check_results = endpoint::run_declared_checks(connection)?;
    for result in &check_results {
        if !result.passed && result.severity == "error" {
            errors.push(format!("check {} failed: {}", result.id, result.detail));
        }
    }
    if !errors.is_empty() {
        return Err(RuntimeError::Verification(errors.join(" | ")));
    }
    Ok(VerificationReport { check_results })
}

fn verify_no_triggers(
    connection: &Connection,
    errors: &mut Vec<String>,
) -> Result<(), RuntimeError> {
    let triggers = connection
        .prepare("SELECT name FROM sqlite_schema WHERE type = 'trigger' ORDER BY name")?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    if !triggers.is_empty() {
        errors.push(format!("triggers are forbidden: {}", triggers.join(", ")));
    }
    Ok(())
}

fn verify_table(
    connection: &Connection,
    name: &str,
    contract: &TableContract,
    required: bool,
    errors: &mut Vec<String>,
) -> Result<(), RuntimeError> {
    let object_type: Option<String> = connection
        .query_row(
            "SELECT type FROM sqlite_schema WHERE name = ?1",
            [name],
            |row| row.get(0),
        )
        .optional()?;
    match object_type.as_deref() {
        Some("table") => {}
        None if !required => return Ok(()),
        Some(other) => {
            errors.push(format!("{name} is {other}, expected table"));
            return Ok(());
        }
        None => {
            errors.push(format!("missing required table {name}"));
            return Ok(());
        }
    }
    let pragma = format!("PRAGMA table_xinfo({})", quote_identifier(name));
    let columns = connection
        .prepare(&pragma)?
        .query_map([], |row| {
            Ok(ColumnInfo {
                name: row.get(1)?,
                column_type: row.get::<_, String>(2)?.to_ascii_uppercase(),
                notnull: row.get::<_, i64>(3)? == 1,
                pk: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let by_name: BTreeMap<_, _> = columns
        .iter()
        .map(|column| (column.name.as_str(), column))
        .collect();
    for (column_name, expected) in &contract.columns {
        let Some(actual) = by_name.get(column_name.as_str()) else {
            errors.push(format!("{name} is missing column {column_name}"));
            continue;
        };
        if actual.column_type != expected.column_type
            || actual.notnull != expected.notnull
            || (expected.pk != 0 && actual.pk != expected.pk)
        {
            errors.push(format!(
                "{name}.{column_name} contract mismatch: type={} notnull={} pk={}",
                actual.column_type, actual.notnull, actual.pk
            ));
        }
    }
    let primary_key: Vec<_> = columns
        .iter()
        .filter(|column| column.pk > 0)
        .collect::<Vec<_>>()
        .into_iter()
        .map(|column| (column.pk, column.name.clone()))
        .collect::<BTreeMap<_, _>>()
        .into_values()
        .collect();
    if primary_key != contract.primary_key {
        errors.push(format!("{name} primary key is incompatible"));
    }
    if !contract.foreign_keys.is_empty() {
        let pragma = format!("PRAGMA foreign_key_list({})", quote_identifier(name));
        let actual = connection
            .prepare(&pragma)?
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(4)?,
                ))
            })?
            .collect::<Result<BTreeSet<_>, _>>()?;
        for expected in &contract.foreign_keys {
            if !actual.contains(&(
                expected.from.clone(),
                expected.table.clone(),
                expected.to.clone(),
            )) {
                errors.push(format!("{name} is missing a required foreign key"));
            }
        }
    }
    Ok(())
}

fn verify_view(
    connection: &Connection,
    name: &str,
    contract: &ViewContract,
    errors: &mut Vec<String>,
) -> Result<(), RuntimeError> {
    let object_type: Option<String> = connection
        .query_row(
            "SELECT type FROM sqlite_schema WHERE name = ?1",
            [name],
            |row| row.get(0),
        )
        .optional()?;
    if object_type.as_deref() != Some("view") {
        errors.push(format!("missing required view {name}"));
        return Ok(());
    }
    let sql = format!("SELECT * FROM {} LIMIT 0", quote_identifier(name));
    let statement = connection.prepare(&sql)?;
    let columns: Vec<_> = statement
        .column_names()
        .into_iter()
        .map(str::to_owned)
        .collect();
    if columns != contract.columns {
        errors.push(format!("{name} columns are incompatible"));
    }
    Ok(())
}

fn verify_assets(
    connection: &Connection,
    identity: &sqlite_capsule_core::CapsuleIdentity,
    errors: &mut Vec<String>,
) -> Result<(), RuntimeError> {
    let mut statement = connection.prepare(
        "SELECT path, media_type, content, sha256, executable, cache_policy \
         FROM capsule_asset ORDER BY path",
    )?;
    let mut rows = statement.query([])?;
    let mut entry_found = false;
    while let Some(row) = rows.next()? {
        let path: String = row.get(0)?;
        let media_type: String = row.get(1)?;
        let content: Vec<u8> = row.get(2)?;
        let declared_sha: String = row.get(3)?;
        let executable: i64 = row.get(4)?;
        let cache_policy: String = row.get(5)?;
        if !safe_asset_path(&path)
            || !safe_media_type(&media_type)
            || content.len() > MAX_ASSET_BYTES
            || !matches!(executable, 0 | 1)
            || cache_policy != "no-store"
        {
            errors.push(format!("asset {path:?} violates asset policy"));
            continue;
        }
        if declared_sha != lower_hex(&Sha256::digest(&content)) {
            errors.push(format!("asset {path:?} hash mismatch"));
        }
        if path == identity.entry_asset {
            entry_found = true;
            if executable != 1 || media_type.split(';').next().map(str::trim) != Some("text/html") {
                errors.push("entry asset must be executable text/html".to_owned());
            }
        }
    }
    if !entry_found {
        errors.push("manifest entry asset is missing".to_owned());
    }
    Ok(())
}

fn verify_commands(connection: &Connection, errors: &mut Vec<String>) -> Result<(), RuntimeError> {
    let mut statement =
        connection.prepare("SELECT id, argv_json FROM capsule_command ORDER BY id")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let argv: Option<String> = row.get(1)?;
        let Some(argv) = argv else { continue };
        let value: serde_json::Value = match serde_json::from_str(&argv) {
            Ok(value) => value,
            Err(error) => {
                errors.push(format!("command {id} argv_json: {error}"));
                continue;
            }
        };
        if value.is_null() {
            continue;
        }
        if !value.as_array().is_some_and(|items| {
            !items.is_empty()
                && items
                    .iter()
                    .all(|item| item.as_str().is_some_and(|text| !text.is_empty()))
        }) {
            errors.push(format!(
                "command {id} argv_json is not a non-empty string array"
            ));
        }
    }
    Ok(())
}

fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

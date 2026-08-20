use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

use rusqlite::{
    Connection, OptionalExtension,
    hooks::{AuthAction, AuthContext, Authorization},
    types::{Null, ValueRef},
};
use serde::Deserialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use sqlite_capsule_core::CapsuleIdentity;

use crate::{DeclaredCheckResult, LaunchError};

const V02_CONFORMANCE: &str = include_str!("../../../../format/capsule-v0.2.conformance.json");
const V03_CONFORMANCE: &str = include_str!("../../../../format/capsule-v0.3.conformance.json");
const MAX_ASSET_BYTES: usize = 16 * 1024 * 1024;
const MAX_ENDPOINT_STEPS: usize = 16;
const MAX_PARAMETERS: usize = 128;
const MAX_PARAMETER_NAME_BYTES: usize = 128;
const MAX_JSON_DEPTH: usize = 32;
const COMPILE_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_RESULT_ROWS: usize = 1_000;
const MAX_RESULT_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Deserialize)]
struct Contract {
    required_objects: ObjectContracts,
    #[serde(default)]
    optional_objects: ObjectContracts,
    #[serde(default)]
    minimum_nonempty_tables: Vec<String>,
    #[serde(default)]
    reject_unknown_platform_objects: bool,
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
    #[serde(default)]
    exact_columns: bool,
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

#[derive(Clone, Debug, Deserialize)]
struct ParameterRule {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    nullable: bool,
    #[serde(default)]
    default: Option<Value>,
}

#[derive(Debug)]
struct EndpointStep {
    sequence: i64,
    sql_text: String,
}

pub(super) fn verify(
    connection: &Connection,
    identity: &CapsuleIdentity,
) -> Result<(), LaunchError> {
    let integrity = connection
        .prepare("PRAGMA integrity_check")?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    if integrity != ["ok"] {
        return Err(LaunchError::Structure(format!(
            "SQLite integrity_check failed: {}",
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
        return Err(LaunchError::Structure(format!(
            "foreign-key violations: {foreign_key_violations}"
        )));
    }

    let contract_text = match identity.user_version {
        2 => V02_CONFORMANCE,
        3 => V03_CONFORMANCE,
        _ => {
            return Err(LaunchError::Structure(
                "unsupported format contract".to_owned(),
            ));
        }
    };
    let contract: Contract = serde_json::from_str(contract_text)
        .map_err(|error| LaunchError::Structure(format!("invalid format contract: {error}")))?;
    let mut errors = Vec::new();
    verify_forbidden_schema_objects(connection, &mut errors)?;
    if contract.reject_unknown_platform_objects {
        verify_known_platform_objects(connection, &contract, &mut errors)?;
    }
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
    if identity.user_version == 3 {
        verify_instance_assets(connection, &mut errors)?;
    }
    verify_commands(connection, &mut errors)?;
    verify_endpoint_declarations(connection, identity, &mut errors)?;
    verify_check_declarations(connection, &mut errors)?;
    if errors.is_empty() {
        Ok(())
    } else {
        Err(LaunchError::Structure(errors.join(" | ")))
    }
}

fn verify_known_platform_objects(
    connection: &Connection,
    contract: &Contract,
    errors: &mut Vec<String>,
) -> Result<(), LaunchError> {
    let allowed: BTreeSet<_> = contract
        .required_objects
        .tables
        .keys()
        .chain(contract.optional_objects.tables.keys())
        .cloned()
        .collect();
    let names = connection
        .prepare("SELECT name FROM sqlite_schema WHERE name GLOB 'capsule_*' ORDER BY name")?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let unexpected: Vec<_> = names
        .into_iter()
        .filter(|name| !allowed.contains(name))
        .collect();
    if !unexpected.is_empty() {
        errors.push(format!(
            "unexpected capsule platform objects: {}",
            unexpected.join(", ")
        ));
    }
    Ok(())
}

fn verify_forbidden_schema_objects(
    connection: &Connection,
    errors: &mut Vec<String>,
) -> Result<(), LaunchError> {
    let triggers = connection
        .prepare("SELECT name FROM sqlite_schema WHERE type = 'trigger' ORDER BY name")?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    if !triggers.is_empty() {
        errors.push(format!("triggers are forbidden: {}", triggers.join(", ")));
    }
    let virtual_tables = connection
        .prepare("PRAGMA table_list")?
        .query_map([], |row| {
            Ok((row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })?
        .filter_map(|row| match row {
            Ok((name, kind)) if kind.eq_ignore_ascii_case("virtual") => Some(Ok(name)),
            Ok(_) => None,
            Err(error) => Some(Err(error)),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if !virtual_tables.is_empty() {
        errors.push(format!(
            "virtual tables are forbidden: {}",
            virtual_tables.join(", ")
        ));
    }
    Ok(())
}

fn verify_table(
    connection: &Connection,
    name: &str,
    contract: &TableContract,
    required: bool,
    errors: &mut Vec<String>,
) -> Result<(), LaunchError> {
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
    if contract.exact_columns && columns.len() != contract.columns.len() {
        errors.push(format!("{name} has non-contract columns"));
    }
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
) -> Result<(), LaunchError> {
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
    identity: &CapsuleIdentity,
    errors: &mut Vec<String>,
) -> Result<(), LaunchError> {
    let mut statement = connection.prepare(
        "SELECT path, media_type, content, sha256, executable, cache_policy \
         FROM capsule_asset ORDER BY path",
    )?;
    let mut rows = statement.query([])?;
    let mut entry_found = false;
    let application_icon = identity.overview.application.icon_asset.as_deref();
    let mut icon_found = application_icon.is_none();
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
        if Some(path.as_str()) == application_icon {
            icon_found = true;
            if !matches!(media_type.as_str(), "image/png" | "image/webp")
                || content.len() > 512 * 1024
            {
                errors.push("application icon must be bounded PNG or WebP content".to_owned());
            }
        }
    }
    if !entry_found {
        errors.push("manifest entry asset is missing".to_owned());
    }
    if !icon_found {
        errors.push("application icon asset is missing".to_owned());
    }
    Ok(())
}

fn verify_instance_assets(
    connection: &Connection,
    errors: &mut Vec<String>,
) -> Result<(), LaunchError> {
    let mut statement = connection.prepare(
        "SELECT id, media_type, content, sha256, width, height FROM capsule_instance_asset ORDER BY id",
    )?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let media_type: String = row.get(1)?;
        let content: Vec<u8> = row.get(2)?;
        let declared_sha: String = row.get(3)?;
        let width: i64 = row.get(4)?;
        let height: i64 = row.get(5)?;
        if !matches!(media_type.as_str(), "image/png" | "image/webp")
            || content.len() > 512 * 1024
            || !(1..=1024).contains(&width)
            || !(1..=1024).contains(&height)
            || declared_sha != lower_hex(&Sha256::digest(&content))
        {
            errors.push(format!(
                "instance asset {id:?} violates bounded media policy"
            ));
        }
    }
    Ok(())
}

fn verify_commands(connection: &Connection, errors: &mut Vec<String>) -> Result<(), LaunchError> {
    let mut statement =
        connection.prepare("SELECT id, argv_json FROM capsule_command ORDER BY id")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let argv: Option<String> = row.get(1)?;
        let Some(argv) = argv else { continue };
        let value: Value = match serde_json::from_str(&argv) {
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

fn verify_endpoint_declarations(
    connection: &Connection,
    identity: &CapsuleIdentity,
    errors: &mut Vec<String>,
) -> Result<(), LaunchError> {
    let rows = connection
        .prepare(
            "SELECT name, operation, sql_text, parameters_json, result_mode, enabled \
             FROM capsule_endpoint ORDER BY name",
        )?
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (name, operation, sql_text, parameters_json, result_mode, enabled) in rows {
        let parameters: BTreeMap<String, ParameterRule> =
            match serde_json::from_str(&parameters_json) {
                Ok(parameters) => parameters,
                Err(error) => {
                    errors.push(format!("endpoint {name} parameter schema: {error}"));
                    continue;
                }
            };
        if let Err(error) = validate_parameter_spec(&parameters) {
            errors.push(format!("endpoint {name}: {error}"));
            continue;
        }
        if !matches!(operation.as_str(), "read" | "write")
            || !matches!(result_mode.as_str(), "rows" | "row" | "scalar" | "changes")
            || !matches!(enabled, 0 | 1)
        {
            errors.push(format!(
                "endpoint {name} has invalid operation/result/enabled fields"
            ));
            continue;
        }
        if enabled == 1 {
            let capability = format!("database.{operation}");
            if !identity
                .permissions
                .get(&capability)
                .is_some_and(Value::is_object)
            {
                errors.push(format!("endpoint {name} is not declared by {capability}"));
            }
        }
        let steps = load_steps(connection, identity.user_version, &name)?;
        let statements: Vec<_> = if steps.is_empty() {
            vec![sql_text.as_str()]
        } else {
            if !matches!(identity.user_version, 2 | 3)
                || operation != "write"
                || result_mode != "changes"
                || !(2..=MAX_ENDPOINT_STEPS).contains(&steps.len())
                || steps
                    .iter()
                    .enumerate()
                    .any(|(index, step)| step.sequence != (index + 1) as i64)
                || steps[0].sql_text != sql_text
            {
                errors.push(format!(
                    "endpoint {name} has an invalid compound-step declaration"
                ));
                continue;
            }
            steps.iter().map(|step| step.sql_text.as_str()).collect()
        };
        let mut used = BTreeSet::new();
        let mut unsupported = BTreeSet::new();
        for sql in statements {
            if !single_statement(sql) {
                errors.push(format!("endpoint {name} is not one SQL statement per step"));
                continue;
            }
            let kind = statement_kind(sql);
            let kind_allowed = if operation == "read" {
                matches!(kind, "SELECT" | "WITH")
            } else {
                matches!(kind, "INSERT" | "UPDATE" | "DELETE" | "REPLACE" | "WITH")
            };
            if !kind_allowed {
                errors.push(format!("endpoint {name} starts with disallowed {kind}"));
            }
            let (names, markers) = sql_parameters(sql);
            used.extend(names);
            unsupported.extend(markers);
            if let Err(error) = compile_statement(connection, sql, operation == "write") {
                errors.push(format!("endpoint {name} does not compile: {error}"));
            }
        }
        if !unsupported.is_empty() {
            errors.push(format!(
                "endpoint {name} uses unsupported parameter markers"
            ));
        }
        if used != parameters.keys().cloned().collect() {
            errors.push(format!(
                "endpoint {name} parameters do not match SQL placeholders"
            ));
        }
    }
    Ok(())
}

fn verify_check_declarations(
    connection: &Connection,
    errors: &mut Vec<String>,
) -> Result<(), LaunchError> {
    let checks = connection
        .prepare(
            "SELECT id, severity, sql_text, result_mode, expected_json \
             FROM capsule_check ORDER BY id",
        )?
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (id, severity, sql, result_mode, expected_json) in checks {
        if !matches!(severity.as_str(), "error" | "warning" | "info")
            || !matches!(result_mode.as_str(), "scalar" | "rows" | "empty")
        {
            errors.push(format!("check {id} has invalid severity or result mode"));
        }
        if serde_json::from_str::<Value>(&expected_json).is_err() {
            errors.push(format!("check {id} expected_json is invalid"));
        }
        if !single_statement(&sql) || !matches!(statement_kind(&sql), "SELECT" | "WITH") {
            errors.push(format!("check {id} is not one read-only statement"));
            continue;
        }
        let (parameters, unsupported) = sql_parameters(&sql);
        if !parameters.is_empty() || !unsupported.is_empty() {
            errors.push(format!("check {id} must not declare SQL parameters"));
            continue;
        }
        if let Err(error) = compile_statement(connection, &sql, false) {
            errors.push(format!("check {id} does not compile: {error}"));
        }
    }
    Ok(())
}

pub(super) fn run_declared_checks(
    connection: &Connection,
) -> Result<Vec<DeclaredCheckResult>, LaunchError> {
    let previous_query_only: bool =
        connection.pragma_query_value(None, "query_only", |row| row.get(0))?;
    connection.pragma_update(None, "query_only", true)?;
    let execution = (|| -> Result<Vec<DeclaredCheckResult>, LaunchError> {
        let checks = connection
            .prepare(
                "SELECT id, severity, sql_text, result_mode, expected_json \
                 FROM capsule_check ORDER BY id",
            )?
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut output = Vec::with_capacity(checks.len());
        for (id, severity, sql, mode, expected_json) in checks {
            let result = (|| -> Result<(bool, String), String> {
                if !single_statement(&sql) || !matches!(statement_kind(&sql), "SELECT" | "WITH") {
                    return Ok((false, "not one read-only statement".to_owned()));
                }
                let expected: Value =
                    serde_json::from_str(&expected_json).map_err(|error| error.to_string())?;
                install_check_guards(connection).map_err(|error| error.to_string())?;
                let actual = run_check_query(connection, &sql, &mode);
                clear_check_guards(connection);
                let actual = actual?;
                let passed = if mode == "empty" {
                    actual.as_array().is_some_and(Vec::is_empty)
                } else {
                    actual == expected
                };
                Ok((
                    passed,
                    if passed {
                        "ok".to_owned()
                    } else {
                        "result did not match the declared expectation".to_owned()
                    },
                ))
            })();
            match result {
                Ok((passed, detail)) => output.push(DeclaredCheckResult {
                    id,
                    severity,
                    passed,
                    detail,
                }),
                Err(detail) => output.push(DeclaredCheckResult {
                    id,
                    severity,
                    passed: false,
                    detail,
                }),
            }
        }
        Ok(output)
    })();
    clear_check_guards(connection);
    connection.pragma_update(None, "query_only", previous_query_only)?;
    execution
}

fn install_check_guards(connection: &Connection) -> rusqlite::Result<()> {
    let deadline = Instant::now() + COMPILE_TIMEOUT;
    connection.progress_handler(1_000, Some(move || Instant::now() > deadline))?;
    install_authorizer(connection, false)
}

fn install_authorizer(connection: &Connection, write: bool) -> rusqlite::Result<()> {
    connection.authorizer(Some(move |context: AuthContext<'_>| {
        use AuthAction::*;
        match context.action {
            CreateIndex { .. }
            | CreateTable { .. }
            | CreateTempIndex { .. }
            | CreateTempTable { .. }
            | CreateTempTrigger { .. }
            | CreateTempView { .. }
            | CreateTrigger { .. }
            | CreateView { .. }
            | DropIndex { .. }
            | DropTable { .. }
            | DropTempIndex { .. }
            | DropTempTable { .. }
            | DropTempTrigger { .. }
            | DropTempView { .. }
            | DropTrigger { .. }
            | DropView { .. }
            | Pragma { .. }
            | Attach { .. }
            | Detach { .. }
            | AlterTable { .. }
            | Reindex { .. }
            | Analyze { .. }
            | CreateVtable { .. }
            | DropVtable { .. }
            | Savepoint { .. }
            | Unknown { .. } => Authorization::Deny,
            Function { function_name } if function_name.eq_ignore_ascii_case("load_extension") => {
                Authorization::Deny
            }
            Insert { table_name } | Delete { table_name } | Update { table_name, .. } => {
                let protected = table_name.to_ascii_lowercase();
                if !write || protected.starts_with("capsule_") || protected.starts_with("sqlite_") {
                    Authorization::Deny
                } else {
                    Authorization::Allow
                }
            }
            _ => Authorization::Allow,
        }
    }))
}

fn clear_check_guards(connection: &Connection) {
    let _ = connection.authorizer(None::<fn(AuthContext<'_>) -> Authorization>);
    let _ = connection.progress_handler(0, None::<fn() -> bool>);
}

fn run_check_query(connection: &Connection, sql: &str, mode: &str) -> Result<Value, String> {
    let mut statement = connection.prepare(sql).map_err(|error| error.to_string())?;
    let column_names: Vec<_> = statement
        .column_names()
        .into_iter()
        .map(str::to_owned)
        .collect();
    let mut rows = statement.raw_query();
    match mode {
        "rows" | "empty" => {
            let mut output = Vec::new();
            let mut encoded_size = 2_usize;
            while let Some(row) = rows.next().map_err(|error| error.to_string())? {
                if output.len() >= MAX_RESULT_ROWS {
                    return Err("declared check result exceeds row policy".to_owned());
                }
                let value = decode_check_row(row, &column_names)?;
                encoded_size = encoded_size
                    .checked_add(
                        serde_json::to_vec(&value)
                            .map_err(|error| error.to_string())?
                            .len()
                            + usize::from(!output.is_empty()),
                    )
                    .ok_or_else(|| "declared check result exceeds byte policy".to_owned())?;
                if encoded_size > MAX_RESULT_BYTES {
                    return Err("declared check result exceeds byte policy".to_owned());
                }
                output.push(value);
            }
            Ok(Value::Array(output))
        }
        "scalar" => {
            let value = rows
                .next()
                .map_err(|error| error.to_string())?
                .map(|row| {
                    row.get_ref(0)
                        .map_err(|error| error.to_string())
                        .and_then(check_sqlite_value)
                })
                .transpose()?
                .unwrap_or(Value::Null);
            ensure_check_result_size(&value)?;
            Ok(value)
        }
        _ => Err("unsupported declared check result mode".to_owned()),
    }
}

fn decode_check_row(row: &rusqlite::Row<'_>, names: &[String]) -> Result<Value, String> {
    let mut output = Map::new();
    for (index, name) in names.iter().enumerate() {
        let mut value = row
            .get_ref(index)
            .map_err(|error| error.to_string())
            .and_then(check_sqlite_value)?;
        if name.ends_with("_json")
            && let Some(text) = value.as_str()
            && let Ok(decoded) = serde_json::from_str(text)
        {
            value = decoded;
        }
        output.insert(name.clone(), value);
    }
    Ok(Value::Object(output))
}

fn check_sqlite_value(value: ValueRef<'_>) -> Result<Value, String> {
    Ok(match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(value) => Value::from(value),
        ValueRef::Real(value) if value.is_finite() => Value::from(value),
        ValueRef::Real(_) => return Err("non-finite declared check result".to_owned()),
        ValueRef::Text(value) => Value::String(
            std::str::from_utf8(value)
                .map_err(|_| "declared check result is not UTF-8".to_owned())?
                .to_owned(),
        ),
        ValueRef::Blob(_) => return Err("blob declared check results are not exposed".to_owned()),
    })
}

fn ensure_check_result_size(value: &Value) -> Result<(), String> {
    if serde_json::to_vec(value)
        .map_err(|error| error.to_string())?
        .len()
        > MAX_RESULT_BYTES
    {
        Err("declared check result exceeds byte policy".to_owned())
    } else {
        Ok(())
    }
}

fn load_steps(
    connection: &Connection,
    user_version: i64,
    endpoint: &str,
) -> Result<Vec<EndpointStep>, LaunchError> {
    if !matches!(user_version, 2 | 3) {
        return Ok(Vec::new());
    }
    Ok(connection
        .prepare(
            "SELECT sequence, sql_text FROM capsule_endpoint_step \
             WHERE endpoint_name = ?1 ORDER BY sequence",
        )?
        .query_map([endpoint], |row| {
            Ok(EndpointStep {
                sequence: row.get(0)?,
                sql_text: row.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

fn validate_parameter_spec(spec: &BTreeMap<String, ParameterRule>) -> Result<(), &'static str> {
    if spec.len() > MAX_PARAMETERS {
        return Err("too many parameters");
    }
    for (name, rule) in spec {
        if name.is_empty()
            || name.len() > MAX_PARAMETER_NAME_BYTES
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            || !matches!(
                rule.kind.as_str(),
                "string" | "number" | "integer" | "boolean" | "json"
            )
        {
            return Err("invalid parameter schema");
        }
        if let Some(default) = &rule.default
            && !valid_default(default, rule)
        {
            return Err("invalid parameter default");
        }
    }
    Ok(())
}

fn valid_default(value: &Value, rule: &ParameterRule) -> bool {
    if value.is_null() {
        return rule.nullable;
    }
    match rule.kind.as_str() {
        "string" => value.is_string(),
        "number" => {
            value.as_i64().is_some()
                || value.as_f64().is_some_and(f64::is_finite)
                || value
                    .as_str()
                    .and_then(|text| text.parse::<f64>().ok())
                    .is_some_and(f64::is_finite)
        }
        "integer" => {
            value.as_i64().is_some()
                || value.as_f64().is_some_and(|number| {
                    number.is_finite()
                        && number.fract() == 0.0
                        && (i64::MIN as f64..=i64::MAX as f64).contains(&number)
                })
                || value
                    .as_str()
                    .and_then(|text| text.parse::<i64>().ok())
                    .is_some()
        }
        "boolean" => {
            value.is_boolean()
                || value.as_str().is_some_and(|text| {
                    matches!(
                        text.to_ascii_lowercase().as_str(),
                        "true" | "1" | "false" | "0"
                    )
                })
        }
        "json" => valid_json_depth(value, 0),
        _ => false,
    }
}

fn valid_json_depth(value: &Value, depth: usize) -> bool {
    if depth > MAX_JSON_DEPTH {
        return false;
    }
    match value {
        Value::Array(items) => items.iter().all(|item| valid_json_depth(item, depth + 1)),
        Value::Object(items) => items.values().all(|item| valid_json_depth(item, depth + 1)),
        _ => true,
    }
}

/// Compile a declaration through SQLite's `EXPLAIN` VM. The endpoint or check
/// statement itself is never run.
fn compile_statement(connection: &Connection, sql: &str, write: bool) -> Result<(), String> {
    let deadline = Instant::now() + COMPILE_TIMEOUT;
    connection
        .progress_handler(1_000, Some(move || Instant::now() > deadline))
        .map_err(|error| error.to_string())?;
    install_authorizer(connection, write).map_err(|error| error.to_string())?;
    let result = (|| -> Result<(), rusqlite::Error> {
        let mut statement = connection.prepare(&format!("EXPLAIN {sql}"))?;
        for index in 1..=statement.parameter_count() {
            statement.raw_bind_parameter(index, Null)?;
        }
        let mut rows = statement.raw_query();
        while rows.next()?.is_some() {}
        Ok(())
    })();
    let _ = connection.authorizer(None::<fn(AuthContext<'_>) -> Authorization>);
    let _ = connection.progress_handler(0, None::<fn() -> bool>);
    result.map_err(|error| error.to_string())
}

fn single_statement(sql: &str) -> bool {
    let mut text = sql.trim();
    if text.is_empty() {
        return false;
    }
    if let Some(without) = text.strip_suffix(';') {
        text = without.trim_end();
    }
    !text.contains(';')
}

fn statement_kind(sql: &str) -> &str {
    let mut text = sql.trim_start();
    while let Some(comment) = text.strip_prefix("--") {
        text = comment
            .split_once('\n')
            .map_or("", |(_, remainder)| remainder)
            .trim_start();
    }
    text.split_whitespace().next().unwrap_or("")
}

fn sql_parameters(sql: &str) -> (BTreeSet<String>, BTreeSet<char>) {
    let chars: Vec<char> = sql.chars().collect();
    let mut named = BTreeSet::new();
    let mut unsupported = BTreeSet::new();
    let mut index = 0;
    while index < chars.len() {
        let current = chars[index];
        let next = chars.get(index + 1).copied();
        if current == '-' && next == Some('-') {
            index += 2;
            while index < chars.len() && !matches!(chars[index], '\r' | '\n') {
                index += 1;
            }
            continue;
        }
        if current == '/' && next == Some('*') {
            index += 2;
            while index + 1 < chars.len() && !(chars[index] == '*' && chars[index + 1] == '/') {
                index += 1;
            }
            index = (index + 2).min(chars.len());
            continue;
        }
        if matches!(current, '\'' | '"' | '`') {
            let quote = current;
            index += 1;
            while index < chars.len() {
                if chars[index] == quote {
                    if chars.get(index + 1) == Some(&quote) {
                        index += 2;
                        continue;
                    }
                    index += 1;
                    break;
                }
                index += 1;
            }
            continue;
        }
        if current == '[' {
            index += 1;
            while index < chars.len() && chars[index] != ']' {
                index += 1;
            }
            index = (index + 1).min(chars.len());
            continue;
        }
        if current == ':' && next.is_some_and(|value| value.is_ascii_alphabetic() || value == '_') {
            let mut end = index + 2;
            while end < chars.len() && (chars[end].is_ascii_alphanumeric() || chars[end] == '_') {
                end += 1;
            }
            named.insert(chars[index + 1..end].iter().collect());
            index = end;
            continue;
        }
        if matches!(current, '?' | '@' | '$') {
            unsupported.insert(current);
        }
        index += 1;
    }
    (named, unsupported)
}

fn safe_asset_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.chars().any(char::is_control)
        && path
            .split('/')
            .all(|component| !matches!(component, "" | "." | ".."))
}

fn safe_media_type(value: &str) -> bool {
    if value.is_empty()
        || !value.is_ascii()
        || !value.bytes().all(|byte| (32..=126).contains(&byte))
    {
        return false;
    }
    let mut parts = value.split(';');
    let Some(essence) = parts.next().map(str::trim) else {
        return false;
    };
    let Some((kind, subtype)) = essence.split_once('/') else {
        return false;
    };
    if !media_token(kind, false) || !media_token(subtype, false) {
        return false;
    }
    parts.all(|parameter| {
        let Some((name, value)) = parameter.trim().split_once('=') else {
            return false;
        };
        media_token(name.trim(), false) && media_token(value.trim(), true)
    })
}

fn media_token(value: &str, allow_apostrophe: bool) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#' | b'$' | b'&' | b'^' | b'_' | b'.' | b'+' | b'-'
                )
                || (allow_apostrophe && byte == b'\'')
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

fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

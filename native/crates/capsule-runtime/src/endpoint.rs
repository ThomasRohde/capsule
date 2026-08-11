use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

use rusqlite::{
    Connection, OptionalExtension,
    hooks::{AuthAction, AuthContext, Authorization},
    types::{Value as SqlValue, ValueRef},
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sqlite_capsule_core::CapsuleIdentity;

use crate::{
    CheckResult, ENDPOINT_TIMEOUT_SECONDS, MAX_ENDPOINT_STEPS, MAX_RESULT_BYTES, MAX_RESULT_ROWS,
    RuntimeError,
};

#[derive(Debug)]
struct Endpoint {
    name: String,
    operation: String,
    sql_text: String,
    parameters: BTreeMap<String, ParameterRule>,
    result_mode: String,
}

#[derive(Debug)]
struct EndpointStep {
    sequence: i64,
    sql_text: String,
    required_changes: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
struct ParameterRule {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    required: bool,
    #[serde(default)]
    nullable: bool,
    #[serde(default)]
    default: Option<Value>,
}

#[derive(Clone, Debug)]
struct BoundValue {
    sql: SqlValue,
    log: Value,
}

pub(crate) fn verify_declarations(
    connection: &Connection,
    identity: &CapsuleIdentity,
    errors: &mut Vec<String>,
) -> Result<(), RuntimeError> {
    let mut statement = connection.prepare(
        "SELECT name, operation, sql_text, parameters_json, result_mode, enabled \
         FROM capsule_endpoint ORDER BY name",
    )?;
    let rows = statement
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
    drop(statement);
    for (name, operation, sql_text, parameters_json, result_mode, enabled) in rows {
        let parameters: BTreeMap<String, ParameterRule> =
            match serde_json::from_str(&parameters_json) {
                Ok(parameters) => parameters,
                Err(error) => {
                    errors.push(format!("endpoint {name} parameter schema: {error}"));
                    continue;
                }
            };
        if let Err(error) = validate_spec(&parameters) {
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
            if identity.user_version != 2
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
            if let Err(error) =
                compile_statement(connection, sql, &parameters, operation == "write")
            {
                errors.push(format!("endpoint {name} does not compile: {error}"));
            }
        }
        if !unsupported.is_empty() {
            errors.push(format!(
                "endpoint {name} uses unsupported parameter markers"
            ));
        }
        let declared: BTreeSet<_> = parameters.keys().cloned().collect();
        if used != declared {
            errors.push(format!(
                "endpoint {name} parameters do not match SQL placeholders"
            ));
        }
    }
    Ok(())
}

pub(crate) fn run_declared_checks(
    connection: &Connection,
) -> Result<Vec<CheckResult>, RuntimeError> {
    let query_only: bool = connection.pragma_query_value(None, "query_only", |row| row.get(0))?;
    connection.pragma_update(None, "query_only", true)?;
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
        let result = (|| -> Result<(bool, String), RuntimeError> {
            if !single_statement(&sql) || !matches!(statement_kind(&sql), "SELECT" | "WITH") {
                return Ok((false, "not one read-only statement".to_owned()));
            }
            let expected: Value = serde_json::from_str(&expected_json)?;
            install_runtime_guards(connection, false)?;
            let actual = run_query(connection, &sql, &BTreeMap::new(), &mode, false);
            clear_runtime_guards(connection);
            let actual = actual?;
            let (actual, passed) = if mode == "empty" {
                let passed = actual.as_array().is_some_and(Vec::is_empty);
                (actual, passed)
            } else {
                let passed = actual == expected;
                (actual, passed)
            };
            Ok((
                passed,
                if passed {
                    "ok".to_owned()
                } else {
                    format!("expected {expected}, got {actual}")
                },
            ))
        })();
        match result {
            Ok((passed, detail)) => output.push(CheckResult {
                id,
                severity,
                passed,
                detail,
            }),
            Err(error) => output.push(CheckResult {
                id,
                severity,
                passed: false,
                detail: error.to_string(),
            }),
        }
    }
    connection.pragma_update(None, "query_only", query_only)?;
    Ok(output)
}

pub(crate) fn execute(
    connection: &mut Connection,
    write: bool,
    name: &str,
    arguments: &Map<String, Value>,
) -> Result<Value, RuntimeError> {
    if name.is_empty()
        || name.len() > 128
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
    {
        return Err(RuntimeError::Endpoint("invalid endpoint name".to_owned()));
    }
    let endpoint = load_endpoint(connection, name)?;
    let expected_operation = if write { "write" } else { "read" };
    if endpoint.operation != expected_operation {
        return Err(RuntimeError::Endpoint(format!(
            "endpoint {name} is {}, not {expected_operation}",
            endpoint.operation
        )));
    }
    let bound = bind_arguments(&endpoint.parameters, arguments)?;
    let steps = load_steps(
        connection,
        connection.pragma_query_value(None, "user_version", |row| row.get(0))?,
        name,
    )?;
    validate_runtime_declaration(&endpoint, &steps)?;
    if write {
        execute_write(connection, &endpoint, &steps, &bound)
    } else {
        execute_read(connection, &endpoint, &bound)
    }
}

fn load_endpoint(connection: &Connection, name: &str) -> Result<Endpoint, RuntimeError> {
    let row = connection
        .query_row(
            "SELECT name, operation, sql_text, parameters_json, result_mode \
             FROM capsule_endpoint WHERE name = ?1 AND enabled = 1",
            [name],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| RuntimeError::Endpoint("unknown or disabled endpoint".to_owned()))?;
    let parameters = serde_json::from_str(&row.3)?;
    validate_spec(&parameters)?;
    Ok(Endpoint {
        name: row.0,
        operation: row.1,
        sql_text: row.2,
        parameters,
        result_mode: row.4,
    })
}

fn load_steps(
    connection: &Connection,
    user_version: i64,
    endpoint: &str,
) -> Result<Vec<EndpointStep>, RuntimeError> {
    if user_version != 2 {
        return Ok(Vec::new());
    }
    Ok(connection
        .prepare(
            "SELECT sequence, sql_text, required_changes FROM capsule_endpoint_step \
             WHERE endpoint_name = ?1 ORDER BY sequence",
        )?
        .query_map([endpoint], |row| {
            Ok(EndpointStep {
                sequence: row.get(0)?,
                sql_text: row.get(1)?,
                required_changes: row.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

fn validate_runtime_declaration(
    endpoint: &Endpoint,
    steps: &[EndpointStep],
) -> Result<(), RuntimeError> {
    let statements: Vec<_> = if steps.is_empty() {
        vec![endpoint.sql_text.as_str()]
    } else {
        if endpoint.operation != "write"
            || endpoint.result_mode != "changes"
            || !(2..=MAX_ENDPOINT_STEPS).contains(&steps.len())
            || steps
                .iter()
                .enumerate()
                .any(|(index, step)| step.sequence != (index + 1) as i64)
            || steps[0].sql_text != endpoint.sql_text
        {
            return Err(RuntimeError::Endpoint(
                "invalid compound endpoint declaration".to_owned(),
            ));
        }
        steps.iter().map(|step| step.sql_text.as_str()).collect()
    };
    let mut used = BTreeSet::new();
    for sql in statements {
        if !single_statement(sql) {
            return Err(RuntimeError::Endpoint(
                "multiple statements in one endpoint step".to_owned(),
            ));
        }
        let kind = statement_kind(sql);
        let allowed = if endpoint.operation == "read" {
            matches!(kind, "SELECT" | "WITH")
        } else {
            matches!(kind, "INSERT" | "UPDATE" | "DELETE" | "REPLACE" | "WITH")
        };
        if !allowed {
            return Err(RuntimeError::Endpoint(
                "disallowed SQL statement kind".to_owned(),
            ));
        }
        let (parameters, unsupported) = sql_parameters(sql);
        if !unsupported.is_empty() {
            return Err(RuntimeError::Endpoint(
                "unsupported SQL parameter marker".to_owned(),
            ));
        }
        used.extend(parameters);
    }
    if used != endpoint.parameters.keys().cloned().collect() {
        return Err(RuntimeError::Endpoint(
            "parameters do not match SQL placeholders".to_owned(),
        ));
    }
    Ok(())
}

fn execute_read(
    connection: &Connection,
    endpoint: &Endpoint,
    bound: &BTreeMap<String, BoundValue>,
) -> Result<Value, RuntimeError> {
    let previous_query_only: bool =
        connection.pragma_query_value(None, "query_only", |row| row.get(0))?;
    connection.pragma_update(None, "query_only", true)?;
    install_runtime_guards(connection, false)?;
    let result = run_query(
        connection,
        &endpoint.sql_text,
        bound,
        &endpoint.result_mode,
        false,
    );
    clear_runtime_guards(connection);
    connection.pragma_update(None, "query_only", previous_query_only)?;
    result.map_err(|error| RuntimeError::Endpoint(format!("read {}: {error}", endpoint.name)))
}

fn execute_write(
    connection: &mut Connection,
    endpoint: &Endpoint,
    steps: &[EndpointStep],
    bound: &BTreeMap<String, BoundValue>,
) -> Result<Value, RuntimeError> {
    connection.execute_batch("BEGIN IMMEDIATE")?;
    install_runtime_guards(connection, true)?;
    let execution = (|| -> Result<(Value, Vec<i64>, i64), RuntimeError> {
        let statements: Vec<_> = if steps.is_empty() {
            vec![(endpoint.sql_text.as_str(), None)]
        } else {
            steps
                .iter()
                .map(|step| (step.sql_text.as_str(), step.required_changes))
                .collect()
        };
        let mut step_changes = Vec::with_capacity(statements.len());
        let mut lastrowid = 0_i64;
        let mut result = Value::Null;
        for (index, (sql, required_changes)) in statements.into_iter().enumerate() {
            result = run_query(connection, sql, bound, &endpoint.result_mode, true)?;
            let changes =
                i64::try_from(connection.changes()).map_err(|_| RuntimeError::ResultPolicy)?;
            if required_changes.is_some_and(|required| required != changes) {
                return Err(RuntimeError::Endpoint(format!(
                    "compound step {} changed {changes} rows; expected {}",
                    index + 1,
                    required_changes.unwrap()
                )));
            }
            step_changes.push(changes);
            lastrowid = connection.last_insert_rowid();
        }
        Ok((result, step_changes, lastrowid))
    })();
    clear_runtime_guards(connection);
    let (mut result, step_changes, lastrowid) = match execution {
        Ok(value) => value,
        Err(error) => {
            let _ = connection.execute_batch("ROLLBACK");
            return Err(error);
        }
    };
    let total_changes: i64 = step_changes.iter().sum();
    let log_parameters: Map<_, _> = bound
        .iter()
        .map(|(name, value)| (name.clone(), value.log.clone()))
        .collect();
    let parameters_json = serde_json::to_string(&log_parameters)?;
    if let Err(error) = connection.execute(
        "INSERT INTO capsule_change_log \
         (endpoint_name, parameters_json, changed_rows, occurred_at) \
         VALUES (?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))",
        rusqlite::params![endpoint.name, parameters_json, total_changes],
    ) {
        let _ = connection.execute_batch("ROLLBACK");
        return Err(RuntimeError::Sqlite(error));
    }
    connection.execute_batch("COMMIT")?;
    if endpoint.result_mode == "changes" {
        result = if steps.is_empty() {
            json!({"changes": total_changes, "lastrowid": lastrowid})
        } else {
            json!({
                "changes": total_changes,
                "step_changes": step_changes,
                "lastrowid": lastrowid
            })
        };
    }
    Ok(result)
}

fn run_query(
    connection: &Connection,
    sql: &str,
    bound: &BTreeMap<String, BoundValue>,
    result_mode: &str,
    drain_remaining: bool,
) -> Result<Value, RuntimeError> {
    let mut statement = connection.prepare(sql)?;
    bind_statement(&mut statement, bound)?;
    let column_names: Vec<_> = statement
        .column_names()
        .into_iter()
        .map(str::to_owned)
        .collect();
    let mut rows = statement.raw_query();
    match result_mode {
        "rows" | "empty" => {
            let mut output = Vec::new();
            let mut encoded_size = 2_usize;
            while let Some(row) = rows.next()? {
                if output.len() >= MAX_RESULT_ROWS {
                    return Err(RuntimeError::ResultPolicy);
                }
                let value = decode_row(row, &column_names)?;
                encoded_size = encoded_size
                    .checked_add(
                        serde_json::to_vec(&value)?.len() + usize::from(!output.is_empty()),
                    )
                    .ok_or(RuntimeError::ResultPolicy)?;
                if encoded_size > MAX_RESULT_BYTES {
                    return Err(RuntimeError::ResultPolicy);
                }
                output.push(value);
            }
            Ok(Value::Array(output))
        }
        "row" => {
            let value = rows
                .next()?
                .map(|row| decode_row(row, &column_names))
                .transpose()?
                .unwrap_or(Value::Null);
            ensure_result_size(&value)?;
            if drain_remaining {
                while rows.next()?.is_some() {}
            }
            Ok(value)
        }
        "scalar" => {
            let value = rows
                .next()?
                .map(|row| sqlite_value(row.get_ref(0)?))
                .transpose()?
                .unwrap_or(Value::Null);
            ensure_result_size(&value)?;
            if drain_remaining {
                while rows.next()?.is_some() {}
            }
            Ok(value)
        }
        "changes" => {
            while rows.next()?.is_some() {}
            Ok(Value::Null)
        }
        _ => Err(RuntimeError::Endpoint("unsupported result mode".to_owned())),
    }
}

fn decode_row(row: &rusqlite::Row<'_>, names: &[String]) -> Result<Value, RuntimeError> {
    let mut output = Map::new();
    for (index, name) in names.iter().enumerate() {
        let mut value = sqlite_value(row.get_ref(index)?)?;
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

fn sqlite_value(value: ValueRef<'_>) -> Result<Value, RuntimeError> {
    Ok(match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(value) => Value::from(value),
        ValueRef::Real(value) if value.is_finite() => Value::from(value),
        ValueRef::Real(_) => return Err(RuntimeError::ResultPolicy),
        ValueRef::Text(value) => Value::String(
            std::str::from_utf8(value)
                .map_err(|_| RuntimeError::ResultPolicy)?
                .to_owned(),
        ),
        ValueRef::Blob(_) => {
            return Err(RuntimeError::Endpoint(
                "blob values are not exposed by the JSON bridge".to_owned(),
            ));
        }
    })
}

fn ensure_result_size(value: &Value) -> Result<(), RuntimeError> {
    if serde_json::to_vec(value)?.len() > MAX_RESULT_BYTES {
        Err(RuntimeError::ResultPolicy)
    } else {
        Ok(())
    }
}

fn bind_statement(
    statement: &mut rusqlite::Statement<'_>,
    bound: &BTreeMap<String, BoundValue>,
) -> Result<(), RuntimeError> {
    for index in 1..=statement.parameter_count() {
        let parameter = statement
            .parameter_name(index)
            .ok_or_else(|| RuntimeError::Endpoint("unnamed SQL parameter".to_owned()))?;
        let name = parameter
            .strip_prefix(':')
            .ok_or_else(|| RuntimeError::Endpoint("unsupported SQL parameter marker".to_owned()))?;
        let value = bound
            .get(name)
            .ok_or_else(|| RuntimeError::Endpoint("missing bound SQL parameter".to_owned()))?;
        statement.raw_bind_parameter(index, &value.sql)?;
    }
    Ok(())
}

fn validate_spec(spec: &BTreeMap<String, ParameterRule>) -> Result<(), RuntimeError> {
    if spec.len() > 128 {
        return Err(RuntimeError::Endpoint("too many parameters".to_owned()));
    }
    for (name, rule) in spec {
        if name.is_empty()
            || name.len() > 128
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            || !matches!(
                rule.kind.as_str(),
                "string" | "number" | "integer" | "boolean" | "json"
            )
        {
            return Err(RuntimeError::Endpoint(
                "invalid parameter schema".to_owned(),
            ));
        }
        if let Some(default) = &rule.default {
            if default.is_null() && !rule.nullable {
                return Err(RuntimeError::Endpoint(
                    "null default is not nullable".to_owned(),
                ));
            }
            if !default.is_null() {
                coerce(name, default, rule)?;
            }
        }
    }
    Ok(())
}

fn bind_arguments(
    spec: &BTreeMap<String, ParameterRule>,
    arguments: &Map<String, Value>,
) -> Result<BTreeMap<String, BoundValue>, RuntimeError> {
    if arguments.len() > spec.len() || arguments.keys().any(|name| !spec.contains_key(name)) {
        return Err(RuntimeError::Endpoint("unknown parameter".to_owned()));
    }
    let mut output = BTreeMap::new();
    for (name, rule) in spec {
        let value = match arguments.get(name) {
            Some(value) => value.clone(),
            None if rule.default.is_some() => rule.default.clone().unwrap(),
            None if rule.required => {
                return Err(RuntimeError::Endpoint(format!(
                    "missing required parameter {name}"
                )));
            }
            None => Value::Null,
        };
        if value.is_null() {
            if (arguments.contains_key(name) || rule.default.is_some()) && !rule.nullable {
                return Err(RuntimeError::Endpoint(format!(
                    "parameter {name} may not be null"
                )));
            }
            output.insert(
                name.clone(),
                BoundValue {
                    sql: SqlValue::Null,
                    log: Value::Null,
                },
            );
        } else {
            output.insert(name.clone(), coerce(name, &value, rule)?);
        }
    }
    Ok(output)
}

fn coerce(name: &str, value: &Value, rule: &ParameterRule) -> Result<BoundValue, RuntimeError> {
    let invalid = || RuntimeError::Endpoint(format!("parameter {name} must be {}", rule.kind));
    match rule.kind.as_str() {
        "string" => value
            .as_str()
            .map(|value| BoundValue {
                sql: SqlValue::Text(value.to_owned()),
                log: Value::String(value.to_owned()),
            })
            .ok_or_else(invalid),
        "number" => {
            if let Some(integer) = value.as_i64() {
                return Ok(BoundValue {
                    sql: SqlValue::Integer(integer),
                    log: Value::from(integer),
                });
            }
            let number = value
                .as_f64()
                .or_else(|| value.as_str().and_then(|text| text.parse::<f64>().ok()))
                .filter(|number| number.is_finite())
                .ok_or_else(invalid)?;
            Ok(BoundValue {
                sql: SqlValue::Real(number),
                log: Value::from(number),
            })
        }
        "integer" => {
            let integer = value
                .as_i64()
                .or_else(|| {
                    value
                        .as_f64()
                        .filter(|number| number.is_finite() && number.fract() == 0.0)
                        .and_then(|number| {
                            if (i64::MIN as f64..=i64::MAX as f64).contains(&number) {
                                Some(number as i64)
                            } else {
                                None
                            }
                        })
                })
                .or_else(|| value.as_str().and_then(|text| text.parse::<i64>().ok()))
                .ok_or_else(invalid)?;
            Ok(BoundValue {
                sql: SqlValue::Integer(integer),
                log: Value::from(integer),
            })
        }
        "boolean" => {
            let boolean = value.as_bool().or_else(|| {
                value
                    .as_str()
                    .and_then(|text| match text.to_ascii_lowercase().as_str() {
                        "true" | "1" => Some(true),
                        "false" | "0" => Some(false),
                        _ => None,
                    })
            });
            let boolean = boolean.ok_or_else(invalid)?;
            Ok(BoundValue {
                sql: SqlValue::Integer(i64::from(boolean)),
                log: Value::from(i64::from(boolean)),
            })
        }
        "json" => {
            ensure_json_depth(value, 0)?;
            let text = serde_json::to_string(value)?;
            Ok(BoundValue {
                sql: SqlValue::Text(text.clone()),
                log: Value::String(text),
            })
        }
        _ => Err(invalid()),
    }
}

fn ensure_json_depth(value: &Value, depth: usize) -> Result<(), RuntimeError> {
    if depth > 32 {
        return Err(RuntimeError::Endpoint("JSON nesting exceeds 32".to_owned()));
    }
    match value {
        Value::Array(items) => {
            for item in items {
                ensure_json_depth(item, depth + 1)?;
            }
        }
        Value::Object(items) => {
            for item in items.values() {
                ensure_json_depth(item, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn compile_statement(
    connection: &Connection,
    sql: &str,
    spec: &BTreeMap<String, ParameterRule>,
    write: bool,
) -> Result<(), RuntimeError> {
    let probe: BTreeMap<_, _> = spec
        .iter()
        .map(|(name, rule)| {
            let value = match rule.kind.as_str() {
                "string" => Value::String(String::new()),
                "number" => Value::from(0.0),
                "integer" => Value::from(0),
                "boolean" => Value::Bool(false),
                "json" => Value::Null,
                _ => Value::Null,
            };
            Ok((name.clone(), coerce(name, &value, rule)?))
        })
        .collect::<Result<_, RuntimeError>>()?;
    install_authorizer(connection, write)?;
    let result = (|| -> Result<(), RuntimeError> {
        let mut statement = connection.prepare(&format!("EXPLAIN {sql}"))?;
        bind_statement(&mut statement, &probe)?;
        let mut rows = statement.raw_query();
        while rows.next()?.is_some() {}
        Ok(())
    })();
    clear_authorizer(connection);
    result
}

fn install_runtime_guards(connection: &Connection, write: bool) -> Result<(), RuntimeError> {
    let deadline = Instant::now() + Duration::from_secs(ENDPOINT_TIMEOUT_SECONDS);
    connection.progress_handler(1_000, Some(move || Instant::now() > deadline))?;
    install_authorizer(connection, write)
}

fn clear_runtime_guards(connection: &Connection) {
    clear_authorizer(connection);
    let _ = connection.progress_handler(0, None::<fn() -> bool>);
}

fn install_authorizer(connection: &Connection, write: bool) -> Result<(), RuntimeError> {
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
    }))?;
    Ok(())
}

fn clear_authorizer(connection: &Connection) {
    let _ = connection.authorizer(None::<fn(AuthContext<'_>) -> Authorization>);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_returning_drains_rows_before_changes_are_observed() {
        let connection = Connection::open_in_memory().expect("open fixture");
        connection
            .execute_batch("CREATE TABLE item (id INTEGER PRIMARY KEY, value TEXT NOT NULL)")
            .expect("create fixture");
        let result = run_query(
            &connection,
            "INSERT INTO item(value) VALUES ('a'), ('b'), ('c') RETURNING id, value",
            &BTreeMap::new(),
            "row",
            true,
        )
        .expect("execute RETURNING write");
        assert_eq!(result, json!({"id": 1, "value": "a"}));
        assert_eq!(connection.changes(), 3);
        assert_eq!(
            connection
                .query_row("SELECT count(*) FROM item", [], |row| row.get::<_, i64>(0))
                .expect("count statement rows"),
            3
        );
    }
}

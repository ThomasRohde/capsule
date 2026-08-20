//! Bounded projection and validation of the signed v0.3 dataset contract.
//!
//! No path- or connection-based entry point is public. The only caller is the
//! verified workspace source, whose connection targets the exact private
//! snapshot already checked by `capsule-launch`.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde_json::Value;

use crate::{EffectiveLimits, WorkspaceControl, WorkspaceError, WorkspaceErrorCode};

pub const DATA_CONTRACT_PROFILE: &str = "org.sqlite-capsule.data-contract/0.3";

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct DataContract {
    pub profile: &'static str,
    pub app_id: String,
    pub data_schema_id: String,
    pub data_schema_version: i64,
    pub datasets: Vec<Dataset>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Dataset {
    pub id: String,
    pub role: DatasetRole,
    pub description: String,
    pub sensitivity: Sensitivity,
    pub required: bool,
    pub fork: ForkPolicy,
    pub compare: ComparePolicy,
    pub reconcile: ReconcilePolicy,
    pub upgrade: UpgradePolicy,
    pub tables: Vec<DatasetTable>,
    pub dependencies: Vec<DatasetDependency>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct DatasetTable {
    pub name: String,
    pub sequence: u16,
    pub primary_key: Vec<String>,
    pub ignored_columns: Vec<String>,
    pub immutable_columns: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct DatasetDependency {
    pub dataset_id: String,
    pub reason: String,
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

string_enum!(DatasetRole {
    Seed => "seed",
    UserContent => "user-content",
    Settings => "settings",
    History => "history",
    Derived => "derived",
    Cache => "cache",
});
string_enum!(Sensitivity { Normal => "normal", Sensitive => "sensitive" });
string_enum!(ForkPolicy {
    Copy => "copy",
    Reset => "reset",
    Omit => "omit",
    Prompt => "prompt",
    Forbid => "forbid",
});
string_enum!(ComparePolicy {
    Ignore => "ignore",
    Summary => "summary",
    Row => "row",
    Field => "field",
});
string_enum!(ReconcilePolicy {
    Ignore => "ignore",
    Manual => "manual",
    ThreeWay => "three-way",
    Forbid => "forbid",
});
string_enum!(UpgradePolicy {
    Copy => "copy",
    Target => "target",
    Migrate => "migrate",
    Rebuild => "rebuild",
    Omit => "omit",
    Forbid => "forbid",
});

struct DatasetRow {
    id: String,
    role: DatasetRole,
    description: String,
    fork: ForkPolicy,
    compare: ComparePolicy,
    reconcile: ReconcilePolicy,
    upgrade: UpgradePolicy,
    sensitivity: Sensitivity,
    required: bool,
}

struct TableRow {
    dataset_id: String,
    table_name: String,
    sequence: u16,
    primary_key: Vec<String>,
    ignored_columns: Vec<String>,
    immutable_columns: Vec<String>,
}

struct DependencyRow {
    dataset_id: String,
    depends_on: String,
    reason: String,
}

#[derive(Debug)]
struct ColumnInfo {
    name: String,
    declared_type: String,
    not_null: bool,
    primary_key: usize,
    hidden: i64,
}

pub(crate) fn load(
    connection: &Connection,
    app_id: &str,
    data_schema_id: &str,
    data_schema_version: i64,
    limits: &EffectiveLimits,
    control: &WorkspaceControl,
) -> Result<DataContract, WorkspaceError> {
    control.install(connection)?;
    let result = load_inner(
        connection,
        app_id,
        data_schema_id,
        data_schema_version,
        limits,
        control,
    );
    let _ = connection.progress_handler(0, None::<fn() -> bool>);
    control.check()?;
    result
}

fn load_inner(
    connection: &Connection,
    app_id: &str,
    data_schema_id: &str,
    data_schema_version: i64,
    limits: &EffectiveLimits,
    control: &WorkspaceControl,
) -> Result<DataContract, WorkspaceError> {
    let dataset_rows = load_datasets(connection, limits, control)?;
    let table_rows = load_tables(connection, limits, control)?;
    let dependency_rows = load_dependencies(connection, limits, control)?;

    let mut tables_by_dataset: BTreeMap<String, Vec<DatasetTable>> = BTreeMap::new();
    let mut dataset_by_table = BTreeMap::new();
    let mut classified = BTreeSet::new();
    for row in table_rows {
        control.check()?;
        if !classified.insert(row.table_name.clone()) {
            return Err(invalid_contract());
        }
        validate_table_schema(connection, &row, limits, control)?;
        dataset_by_table.insert(row.table_name.clone(), row.dataset_id.clone());
        tables_by_dataset
            .entry(row.dataset_id)
            .or_default()
            .push(DatasetTable {
                name: row.table_name,
                sequence: row.sequence,
                primary_key: row.primary_key,
                ignored_columns: row.ignored_columns,
                immutable_columns: row.immutable_columns,
            });
    }

    let dataset_ids: BTreeSet<_> = dataset_rows.iter().map(|row| row.id.clone()).collect();
    if dataset_ids.len() != dataset_rows.len() {
        return Err(invalid_contract());
    }
    if tables_by_dataset.keys().any(|id| !dataset_ids.contains(id)) {
        return Err(invalid_contract());
    }

    let mut dependencies_by_dataset: BTreeMap<String, Vec<DatasetDependency>> = BTreeMap::new();
    let mut edges: BTreeMap<String, Vec<String>> = dataset_ids
        .iter()
        .map(|id| (id.clone(), Vec::new()))
        .collect();
    for row in dependency_rows {
        if !dataset_ids.contains(&row.dataset_id) || !dataset_ids.contains(&row.depends_on) {
            return Err(invalid_contract());
        }
        let targets = edges
            .get_mut(&row.dataset_id)
            .ok_or_else(invalid_contract)?;
        if targets.contains(&row.depends_on) {
            return Err(invalid_contract());
        }
        targets.push(row.depends_on.clone());
        dependencies_by_dataset
            .entry(row.dataset_id)
            .or_default()
            .push(DatasetDependency {
                dataset_id: row.depends_on,
                reason: row.reason,
            });
    }
    reject_dependency_cycles(&edges, control)?;
    validate_exhaustive_classification(connection, &classified, limits, control)?;
    validate_foreign_key_dependencies(connection, &dataset_by_table, &edges, limits, control)?;

    let mut datasets = Vec::with_capacity(dataset_rows.len());
    for row in dataset_rows {
        let mut tables = tables_by_dataset.remove(&row.id).unwrap_or_default();
        if tables.is_empty() || tables.len() > limits.max_tables_per_dataset {
            return Err(invalid_contract());
        }
        tables.sort_by(|left, right| {
            (left.sequence, left.name.as_str()).cmp(&(right.sequence, right.name.as_str()))
        });
        let dependencies = dependencies_by_dataset.remove(&row.id).unwrap_or_default();
        if dependencies.len() > limits.max_dependencies_per_dataset {
            return Err(limit_exceeded());
        }
        validate_policy_combination(&row)?;
        datasets.push(Dataset {
            id: row.id,
            role: row.role,
            description: row.description,
            sensitivity: row.sensitivity,
            required: row.required,
            fork: row.fork,
            compare: row.compare,
            reconcile: row.reconcile,
            upgrade: row.upgrade,
            tables,
            dependencies,
        });
    }

    Ok(DataContract {
        profile: DATA_CONTRACT_PROFILE,
        app_id: app_id.to_owned(),
        data_schema_id: data_schema_id.to_owned(),
        data_schema_version,
        datasets,
    })
}

fn load_datasets(
    connection: &Connection,
    limits: &EffectiveLimits,
    control: &WorkspaceControl,
) -> Result<Vec<DatasetRow>, WorkspaceError> {
    let mut statement = connection
        .prepare(
            "SELECT \
             CASE WHEN length(CAST(id AS BLOB)) BETWEEN 1 AND 256 THEN id END, \
             CASE WHEN length(CAST(role AS BLOB)) BETWEEN 1 AND 32 THEN role END, \
             CASE WHEN length(CAST(description AS BLOB)) BETWEEN 1 AND 2048 THEN description END, \
             CASE WHEN length(CAST(fork_policy AS BLOB)) BETWEEN 1 AND 32 THEN fork_policy END, \
             CASE WHEN length(CAST(compare_policy AS BLOB)) BETWEEN 1 AND 32 THEN compare_policy END, \
             CASE WHEN length(CAST(reconcile_policy AS BLOB)) BETWEEN 1 AND 32 THEN reconcile_policy END, \
             CASE WHEN length(CAST(upgrade_policy AS BLOB)) BETWEEN 1 AND 32 THEN upgrade_policy END, \
             CASE WHEN length(CAST(sensitivity AS BLOB)) BETWEEN 1 AND 32 THEN sensitivity END, \
             required FROM capsule_dataset ORDER BY id COLLATE BINARY LIMIT ?1",
        )
        .map_err(|_| invalid_contract())?;
    let mut rows = statement
        .query([limit_parameter(limits.max_datasets)?])
        .map_err(|_| invalid_contract())?;
    let mut result = Vec::new();
    while let Some(row) = rows.next().map_err(|_| invalid_contract())? {
        control.check()?;
        if result.len() == limits.max_datasets {
            return Err(limit_exceeded());
        }
        let id = bounded_text(row, 0)?;
        let role_text = bounded_text(row, 1)?;
        let description = bounded_text(row, 2)?;
        let fork_text = bounded_text(row, 3)?;
        let compare_text = bounded_text(row, 4)?;
        let reconcile_text = bounded_text(row, 5)?;
        let upgrade_text = bounded_text(row, 6)?;
        let sensitivity_text = bounded_text(row, 7)?;
        validate_dataset_id(&id)?;
        let required: i64 = row.get(8).map_err(|_| invalid_contract())?;
        result.push(DatasetRow {
            id,
            role: DatasetRole::parse(&role_text).ok_or_else(invalid_contract)?,
            description,
            fork: ForkPolicy::parse(&fork_text).ok_or_else(invalid_contract)?,
            compare: ComparePolicy::parse(&compare_text).ok_or_else(invalid_contract)?,
            reconcile: ReconcilePolicy::parse(&reconcile_text).ok_or_else(invalid_contract)?,
            upgrade: UpgradePolicy::parse(&upgrade_text).ok_or_else(invalid_contract)?,
            sensitivity: Sensitivity::parse(&sensitivity_text).ok_or_else(invalid_contract)?,
            required: match required {
                0 => false,
                1 => true,
                _ => return Err(invalid_contract()),
            },
        });
    }
    if result.is_empty() {
        return Err(invalid_contract());
    }
    Ok(result)
}

fn load_tables(
    connection: &Connection,
    limits: &EffectiveLimits,
    control: &WorkspaceControl,
) -> Result<Vec<TableRow>, WorkspaceError> {
    let mut statement = connection
        .prepare(
            "WITH enumerated AS (SELECT *, ROW_NUMBER() OVER (\
                 PARTITION BY dataset_id \
                 ORDER BY sequence, table_name COLLATE BINARY\
             ) AS dataset_ordinal FROM capsule_dataset_table) \
             SELECT \
             CASE WHEN length(CAST(dataset_id AS BLOB)) BETWEEN 1 AND 256 THEN dataset_id END, \
             CASE WHEN length(CAST(table_name AS BLOB)) BETWEEN 1 AND 256 THEN table_name END, \
             sequence, \
             CASE WHEN length(CAST(primary_key_json AS BLOB)) <= ?1 THEN primary_key_json END, \
             CASE WHEN length(CAST(ignored_columns_json AS BLOB)) <= ?1 THEN ignored_columns_json END, \
             CASE WHEN length(CAST(immutable_columns_json AS BLOB)) <= ?1 THEN immutable_columns_json END \
             FROM enumerated WHERE dataset_ordinal <= ?2 \
             ORDER BY dataset_id COLLATE BINARY, sequence, table_name COLLATE BINARY LIMIT ?3",
        )
        .map_err(|_| invalid_contract())?;
    let mut rows = statement
        .query(params![
            i64::try_from(limits.max_json_bytes).map_err(|_| limit_exceeded())?,
            limit_parameter(limits.max_tables_per_dataset)?,
            limit_parameter(limits.max_tables_total)?
        ])
        .map_err(|_| invalid_contract())?;
    let mut result = Vec::new();
    let mut per_dataset = BTreeMap::<String, usize>::new();
    while let Some(row) = rows.next().map_err(|_| invalid_contract())? {
        control.check()?;
        if result.len() == limits.max_tables_total {
            return Err(limit_exceeded());
        }
        let dataset_id = bounded_text(row, 0)?;
        let table_name = bounded_text(row, 1)?;
        validate_dataset_id(&dataset_id)?;
        validate_sqlite_identifier(&table_name)?;
        let count = per_dataset.entry(dataset_id.clone()).or_default();
        *count += 1;
        if *count > limits.max_tables_per_dataset {
            return Err(limit_exceeded());
        }
        let sequence: i64 = row.get(2).map_err(|_| invalid_contract())?;
        let sequence = u16::try_from(sequence).map_err(|_| invalid_contract())?;
        let primary_key_json = bounded_text(row, 3)?;
        let ignored_json = bounded_text(row, 4)?;
        let immutable_json = bounded_text(row, 5)?;
        let primary_key = parse_sqlite_identifier_array(
            &primary_key_json,
            1,
            limits.max_primary_key_columns,
            limits.max_json_depth,
        )?;
        let ignored_columns = parse_sqlite_identifier_array(
            &ignored_json,
            0,
            limits.max_ignored_columns,
            limits.max_json_depth,
        )?;
        let immutable_columns = parse_sqlite_identifier_array(
            &immutable_json,
            0,
            limits.max_immutable_columns,
            limits.max_json_depth,
        )?;
        result.push(TableRow {
            dataset_id,
            table_name,
            sequence,
            primary_key,
            ignored_columns,
            immutable_columns,
        });
    }
    if result.is_empty() {
        return Err(invalid_contract());
    }
    Ok(result)
}

fn load_dependencies(
    connection: &Connection,
    limits: &EffectiveLimits,
    control: &WorkspaceControl,
) -> Result<Vec<DependencyRow>, WorkspaceError> {
    let maximum = limits
        .max_datasets
        .checked_mul(limits.max_dependencies_per_dataset)
        .ok_or_else(limit_exceeded)?;
    let mut statement = connection
        .prepare(
            "WITH enumerated AS (SELECT *, ROW_NUMBER() OVER (\
                 PARTITION BY dataset_id \
                 ORDER BY depends_on_dataset_id COLLATE BINARY\
             ) AS dataset_ordinal FROM capsule_dataset_dependency) \
             SELECT \
             CASE WHEN length(CAST(dataset_id AS BLOB)) BETWEEN 1 AND 256 THEN dataset_id END, \
             CASE WHEN length(CAST(depends_on_dataset_id AS BLOB)) BETWEEN 1 AND 256 THEN depends_on_dataset_id END, \
             CASE WHEN length(CAST(reason AS BLOB)) <= 2048 THEN reason END \
             FROM enumerated WHERE dataset_ordinal <= ?1 \
             ORDER BY dataset_id COLLATE BINARY, depends_on_dataset_id COLLATE BINARY LIMIT ?2",
        )
        .map_err(|_| invalid_contract())?;
    let mut rows = statement
        .query(params![
            limit_parameter(limits.max_dependencies_per_dataset)?,
            limit_parameter(maximum)?
        ])
        .map_err(|_| invalid_contract())?;
    let mut result = Vec::new();
    let mut per_dataset = BTreeMap::<String, usize>::new();
    while let Some(row) = rows.next().map_err(|_| invalid_contract())? {
        control.check()?;
        if result.len() == maximum {
            return Err(limit_exceeded());
        }
        let dataset_id = bounded_text(row, 0)?;
        let depends_on = bounded_text(row, 1)?;
        let reason = bounded_text(row, 2)?;
        validate_dataset_id(&dataset_id)?;
        validate_dataset_id(&depends_on)?;
        let count = per_dataset.entry(dataset_id.clone()).or_default();
        *count += 1;
        if *count > limits.max_dependencies_per_dataset {
            return Err(limit_exceeded());
        }
        result.push(DependencyRow {
            dataset_id,
            depends_on,
            reason,
        });
    }
    Ok(result)
}

fn validate_table_schema(
    connection: &Connection,
    declaration: &TableRow,
    limits: &EffectiveLimits,
    control: &WorkspaceControl,
) -> Result<(), WorkspaceError> {
    control.check()?;
    let object_type: Option<String> = connection
        .query_row(
            "SELECT type FROM sqlite_schema WHERE name = ?1 COLLATE BINARY LIMIT 1",
            [&declaration.table_name],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| invalid_contract())?;
    if object_type.as_deref() != Some("table") {
        return Err(invalid_contract());
    }

    let mut statement = connection
        .prepare(
            "SELECT \
             CASE WHEN length(CAST(name AS BLOB)) BETWEEN 1 AND 256 THEN name END, \
             CASE WHEN length(CAST(type AS BLOB)) <= 64 THEN type END, \
             \"notnull\", pk, hidden FROM pragma_table_xinfo(?1) ORDER BY cid LIMIT ?2",
        )
        .map_err(|_| invalid_contract())?;
    let mut rows = statement
        .query(params![
            &declaration.table_name,
            limit_parameter(limits.max_columns_per_table)?
        ])
        .map_err(|_| invalid_contract())?;
    let mut columns = Vec::new();
    while let Some(row) = rows.next().map_err(|_| invalid_contract())? {
        control.check()?;
        if columns.len() == limits.max_columns_per_table {
            return Err(limit_exceeded());
        }
        let name = bounded_text(row, 0)?;
        validate_sqlite_identifier(&name)?;
        let declared_type = bounded_text(row, 1)?;
        let not_null: i64 = row.get(2).map_err(|_| invalid_contract())?;
        let primary_key: i64 = row.get(3).map_err(|_| invalid_contract())?;
        let hidden: i64 = row.get(4).map_err(|_| invalid_contract())?;
        columns.push(ColumnInfo {
            name,
            declared_type,
            not_null: match not_null {
                0 => false,
                1 => true,
                _ => return Err(invalid_contract()),
            },
            primary_key: usize::try_from(primary_key).map_err(|_| invalid_contract())?,
            hidden,
        });
    }
    if columns.is_empty() {
        return Err(invalid_contract());
    }

    let names: BTreeSet<_> = columns.iter().map(|column| column.name.as_str()).collect();
    if declaration
        .ignored_columns
        .iter()
        .chain(&declaration.immutable_columns)
        .any(|column| !names.contains(column.as_str()))
    {
        return Err(invalid_contract());
    }

    let mut primary_key: Vec<_> = columns
        .iter()
        .filter(|column| column.primary_key > 0)
        .collect();
    primary_key.sort_by_key(|column| column.primary_key);
    if primary_key.is_empty()
        || primary_key.len() > limits.max_primary_key_columns
        || primary_key
            .iter()
            .enumerate()
            .any(|(index, column)| column.primary_key != index + 1 || column.hidden != 0)
    {
        return Err(missing_primary_key());
    }
    let actual: Vec<_> = primary_key
        .iter()
        .map(|column| column.name.as_str())
        .collect();
    let declared: Vec<_> = declaration.primary_key.iter().map(String::as_str).collect();
    if actual != declared {
        return Err(missing_primary_key());
    }
    if declaration
        .ignored_columns
        .iter()
        .any(|column| actual.contains(&column.as_str()))
    {
        return Err(invalid_contract());
    }

    let without_rowid: i64 = connection
        .query_row(
            "SELECT wr FROM pragma_table_list WHERE schema = 'main' \
             AND name = ?1 COLLATE BINARY AND type = 'table' LIMIT 1",
            [&declaration.table_name],
            |row| row.get(0),
        )
        .map_err(|_| invalid_contract())?;
    let integer_rowid_alias = without_rowid == 0
        && primary_key.len() == 1
        && primary_key[0].declared_type.eq_ignore_ascii_case("INTEGER");
    if !integer_rowid_alias && primary_key.iter().any(|column| !column.not_null) {
        return Err(missing_primary_key());
    }
    validate_primary_key_index(
        connection,
        &declaration.table_name,
        &primary_key,
        without_rowid != 0,
    )
}

fn validate_primary_key_index(
    connection: &Connection,
    table_name: &str,
    primary_key: &[&ColumnInfo],
    without_rowid: bool,
) -> Result<(), WorkspaceError> {
    let mut statement = connection
        .prepare(
            "SELECT CASE WHEN length(CAST(name AS BLOB)) BETWEEN 1 AND 512 THEN name END \
             FROM pragma_index_list(?1) WHERE origin = 'pk' ORDER BY seq LIMIT 2",
        )
        .map_err(|_| invalid_contract())?;
    let indexes = statement
        .query_map([table_name], |row| row.get::<_, Option<String>>(0))
        .map_err(|_| invalid_contract())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| invalid_contract())?;
    if indexes.len() > 1 || indexes.iter().any(Option::is_none) {
        return Err(missing_primary_key());
    }
    let Some(index_name) = indexes.into_iter().next().flatten() else {
        if !without_rowid
            && primary_key.len() == 1
            && primary_key[0].declared_type.eq_ignore_ascii_case("INTEGER")
        {
            return Ok(());
        }
        return Err(missing_primary_key());
    };

    let mut statement = connection
        .prepare(
            "SELECT \
             CASE WHEN name IS NULL OR length(CAST(name AS BLOB)) <= 256 THEN name END, \
             desc, CASE WHEN coll IS NULL OR length(CAST(coll AS BLOB)) <= 32 THEN coll END, key \
             FROM pragma_index_xinfo(?1) WHERE key = 1 ORDER BY seqno LIMIT ?2",
        )
        .map_err(|_| invalid_contract())?;
    let mut rows = statement
        .query(params![index_name, limit_parameter(primary_key.len())?])
        .map_err(|_| invalid_contract())?;
    let mut keys = Vec::new();
    while let Some(row) = rows.next().map_err(|_| invalid_contract())? {
        if keys.len() == primary_key.len() {
            return Err(missing_primary_key());
        }
        let key: i64 = row.get(3).map_err(|_| invalid_contract())?;
        if key != 1 {
            return Err(missing_primary_key());
        }
        let name: Option<String> = row.get(0).map_err(|_| invalid_contract())?;
        let descending: i64 = row.get(1).map_err(|_| invalid_contract())?;
        let collation: Option<String> = row.get(2).map_err(|_| invalid_contract())?;
        if collation.as_deref() != Some("BINARY") {
            return Err(WorkspaceError::new(
                WorkspaceErrorCode::UnsupportedCollation,
            ));
        }
        if descending != 0 {
            return Err(missing_primary_key());
        }
        keys.push(name.ok_or_else(missing_primary_key)?);
    }
    if keys.len() != primary_key.len()
        || keys
            .iter()
            .zip(primary_key)
            .any(|(actual, expected)| actual != &expected.name)
    {
        return Err(missing_primary_key());
    }
    Ok(())
}

fn validate_exhaustive_classification(
    connection: &Connection,
    classified: &BTreeSet<String>,
    limits: &EffectiveLimits,
    control: &WorkspaceControl,
) -> Result<(), WorkspaceError> {
    let mut statement = connection
        .prepare(
            "SELECT CASE WHEN length(CAST(name AS BLOB)) BETWEEN 1 AND 256 THEN name END \
             FROM sqlite_schema WHERE type = 'table' \
             AND name NOT GLOB 'sqlite_*' \
             AND name NOT GLOB 'capsule_*' \
             ORDER BY name COLLATE BINARY LIMIT ?1",
        )
        .map_err(|_| invalid_contract())?;
    let mut rows = statement
        .query([limit_parameter(limits.max_tables_total)?])
        .map_err(|_| invalid_contract())?;
    let mut count = 0_usize;
    while let Some(row) = rows.next().map_err(|_| invalid_contract())? {
        control.check()?;
        if count == limits.max_tables_total {
            return Err(limit_exceeded());
        }
        count += 1;
        let table = bounded_text(row, 0)?;
        if !classified.contains(&table) {
            return Err(WorkspaceError::new(WorkspaceErrorCode::UndeclaredTable));
        }
    }
    if count != classified.len() {
        return Err(invalid_contract());
    }
    Ok(())
}

/// Proves that the signed dependency graph covers the SQLite graph that can
/// affect more than one dataset. The first transform profile permits only
/// restrictive cross-dataset actions; CASCADE/SET NULL/SET DEFAULT would make
/// one dataset decision silently mutate another and therefore fail closed.
fn validate_foreign_key_dependencies(
    connection: &Connection,
    dataset_by_table: &BTreeMap<String, String>,
    declared_edges: &BTreeMap<String, Vec<String>>,
    limits: &EffectiveLimits,
    control: &WorkspaceControl,
) -> Result<(), WorkspaceError> {
    for (child_table, child_dataset) in dataset_by_table {
        control.check()?;
        let mut statement = connection
            .prepare(
                "SELECT \
                 CASE WHEN length(CAST(\"table\" AS BLOB)) BETWEEN 1 AND 256 THEN \"table\" END, \
                 CASE WHEN length(CAST(on_update AS BLOB)) BETWEEN 1 AND 16 THEN on_update END, \
                 CASE WHEN length(CAST(on_delete AS BLOB)) BETWEEN 1 AND 16 THEN on_delete END \
                 FROM pragma_foreign_key_list(?1) ORDER BY id, seq LIMIT ?2",
            )
            .map_err(|_| invalid_contract())?;
        let mut rows = statement
            .query(params![
                child_table,
                limit_parameter(limits.max_columns_per_table)?
            ])
            .map_err(|_| invalid_contract())?;
        let mut count = 0_usize;
        while let Some(row) = rows.next().map_err(|_| invalid_contract())? {
            control.check()?;
            if count == limits.max_columns_per_table {
                return Err(limit_exceeded());
            }
            count += 1;
            let parent_table = bounded_text(row, 0)?;
            let on_update = bounded_text(row, 1)?;
            let on_delete = bounded_text(row, 2)?;
            let parent_dataset = dataset_by_table
                .get(&parent_table)
                .ok_or_else(invalid_contract)?;
            if child_dataset == parent_dataset {
                continue;
            }
            if !declared_edges
                .get(child_dataset)
                .is_some_and(|dependencies| dependencies.contains(parent_dataset))
            {
                return Err(invalid_contract());
            }
            if !is_restrictive_foreign_key_action(&on_update)
                || !is_restrictive_foreign_key_action(&on_delete)
            {
                return Err(invalid_contract());
            }
        }
    }
    Ok(())
}

fn is_restrictive_foreign_key_action(value: &str) -> bool {
    matches!(value, "NO ACTION" | "RESTRICT")
}

fn validate_policy_combination(row: &DatasetRow) -> Result<(), WorkspaceError> {
    if row.required && (row.fork == ForkPolicy::Omit || row.upgrade == UpgradePolicy::Omit) {
        return Err(invalid_contract());
    }
    if row.reconcile == ReconcilePolicy::ThreeWay
        && !matches!(row.compare, ComparePolicy::Row | ComparePolicy::Field)
    {
        return Err(invalid_contract());
    }
    Ok(())
}

fn reject_dependency_cycles(
    edges: &BTreeMap<String, Vec<String>>,
    control: &WorkspaceControl,
) -> Result<(), WorkspaceError> {
    fn visit(
        node: &str,
        edges: &BTreeMap<String, Vec<String>>,
        visiting: &mut BTreeSet<String>,
        visited: &mut BTreeSet<String>,
        control: &WorkspaceControl,
    ) -> Result<(), WorkspaceError> {
        control.check()?;
        if visited.contains(node) {
            return Ok(());
        }
        if !visiting.insert(node.to_owned()) {
            return Err(invalid_contract());
        }
        for dependency in edges.get(node).ok_or_else(invalid_contract)? {
            visit(dependency, edges, visiting, visited, control)?;
        }
        visiting.remove(node);
        visited.insert(node.to_owned());
        Ok(())
    }

    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for node in edges.keys() {
        visit(node, edges, &mut visiting, &mut visited, control)?;
    }
    Ok(())
}

fn parse_sqlite_identifier_array(
    text: &str,
    minimum: usize,
    maximum: usize,
    maximum_depth: usize,
) -> Result<Vec<String>, WorkspaceError> {
    let value: Value = serde_json::from_str(text).map_err(|_| invalid_contract())?;
    if json_depth(&value) > maximum_depth {
        return Err(limit_exceeded());
    }
    let array = value.as_array().ok_or_else(invalid_contract)?;
    if array.len() < minimum {
        return Err(invalid_contract());
    }
    if array.len() > maximum {
        return Err(limit_exceeded());
    }
    let mut result = Vec::with_capacity(array.len());
    let mut unique = BTreeSet::new();
    for item in array {
        let identifier = item.as_str().ok_or_else(invalid_contract)?;
        validate_sqlite_identifier(identifier)?;
        if !unique.insert(identifier) {
            return Err(invalid_contract());
        }
        result.push(identifier.to_owned());
    }
    Ok(result)
}

fn json_depth(value: &Value) -> usize {
    match value {
        Value::Array(values) => 1 + values.iter().map(json_depth).max().unwrap_or(0),
        Value::Object(values) => 1 + values.values().map(json_depth).max().unwrap_or(0),
        _ => 0,
    }
}

fn validate_dataset_id(value: &str) -> Result<(), WorkspaceError> {
    if value.is_empty() || value.len() > 256 || !value.is_ascii() {
        return Err(invalid_contract());
    }
    let mut bytes = value.bytes();
    let first = bytes.next().ok_or_else(invalid_contract)?;
    if !(first.is_ascii_alphabetic() || first == b'_')
        || bytes.any(|byte| {
            !(byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':' | b'-'))
        })
    {
        return Err(invalid_contract());
    }
    Ok(())
}

fn validate_sqlite_identifier(value: &str) -> Result<(), WorkspaceError> {
    // SQLite identifiers are Unicode strings and may contain whitespace or
    // punctuation. They are always passed as bound values to table-valued
    // PRAGMAs; no identifier is interpolated into SQL.
    if value.is_empty() || value.len() > 256 {
        return Err(invalid_contract());
    }
    Ok(())
}

fn bounded_text(row: &rusqlite::Row<'_>, index: usize) -> Result<String, WorkspaceError> {
    row.get::<_, Option<String>>(index)
        .map_err(|_| invalid_contract())?
        .ok_or_else(limit_exceeded)
}

fn limit_parameter(maximum: usize) -> Result<i64, WorkspaceError> {
    i64::try_from(maximum.checked_add(1).ok_or_else(limit_exceeded)?).map_err(|_| limit_exceeded())
}

const fn invalid_contract() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::InvalidContract)
}

const fn missing_primary_key() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::MissingPrimaryKey)
}

const fn limit_exceeded() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::LimitExceeded)
}

//! Bounded, value-free detail for the signed application compartment.
//!
//! Callers provide only two retained verified sources and the summary they
//! previously reviewed. SQL identifiers and the fixed family/table projection
//! remain private to this module. The DTO contains counts and digests only.

use std::{cmp::Ordering, time::Duration};

use rusqlite::{Connection, types::ValueRef};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{
    CancellationToken, CompareSummary, VerifiedWorkspaceSource, WorkspaceControl, WorkspaceError,
    WorkspaceErrorCode,
};

pub const COMPARE_APPLICATION_PROFILE: &str = "org.sqlite-capsule.compare-application/1";

const ROW_PROFILE: &str = "org.sqlite-capsule.compare-application-row/1";
const FAMILY_PROFILE: &str = "org.sqlite-capsule.compare-application-family/1";
const HARD_DEADLINE: Duration = Duration::from_secs(30);
const HARD_TABLES: usize = 32;
const HARD_ROWS_PER_TABLE: u64 = 100_000;
const HARD_TOTAL_ROWS: u64 = 100_000;
const HARD_VALUE_BYTES: u64 = 1024 * 1024;
const HARD_STREAM_BYTES: u64 = 256 * 1024 * 1024;
const HARD_REPORT_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug)]
pub struct CompareApplicationLimits {
    pub deadline: Duration,
    pub operation_deadline: Option<Duration>,
    pub max_tables: usize,
    pub max_rows_per_table: u64,
    pub max_total_rows: u64,
    pub max_value_bytes: u64,
    pub max_stream_bytes: u64,
    pub max_report_bytes: u64,
}

impl Default for CompareApplicationLimits {
    fn default() -> Self {
        Self {
            deadline: HARD_DEADLINE,
            operation_deadline: None,
            max_tables: HARD_TABLES,
            max_rows_per_table: HARD_ROWS_PER_TABLE,
            max_total_rows: HARD_TOTAL_ROWS,
            max_value_bytes: HARD_VALUE_BYTES,
            max_stream_bytes: HARD_STREAM_BYTES,
            max_report_bytes: HARD_REPORT_BYTES,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CompareApplicationLimitsApplied {
    pub max_tables: usize,
    pub max_rows_per_table: u64,
    pub max_total_rows: u64,
    pub max_value_bytes: u64,
    pub max_stream_bytes: u64,
    pub max_report_bytes: u64,
    pub deadline_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompareApplicationFamily {
    ManifestPermissions,
    ApplicationIdentity,
    PublisherIdentity,
    Assets,
    Commands,
    Runbooks,
    Documents,
    Prompts,
    Checks,
    Endpoints,
    DataContracts,
    MigrationContracts,
    SignatureInventory,
}

impl CompareApplicationFamily {
    const fn as_str(self) -> &'static str {
        match self {
            Self::ManifestPermissions => "manifest-permissions",
            Self::ApplicationIdentity => "application-identity",
            Self::PublisherIdentity => "publisher-identity",
            Self::Assets => "assets",
            Self::Commands => "commands",
            Self::Runbooks => "runbooks",
            Self::Documents => "documents",
            Self::Prompts => "prompts",
            Self::Checks => "checks",
            Self::Endpoints => "endpoints",
            Self::DataContracts => "data-contracts",
            Self::MigrationContracts => "migration-contracts",
            Self::SignatureInventory => "signature-inventory",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompareApplicationFamilyState {
    Same,
    Different,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CompareApplicationFamilySummary {
    pub family: CompareApplicationFamily,
    pub state: CompareApplicationFamilyState,
    pub table_count: usize,
    pub left_rows: u64,
    pub right_rows: u64,
    pub left_digest: String,
    pub right_digest: String,
    pub change_count: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CompareApplicationDetail {
    pub profile: &'static str,
    pub comparison_report_digest: String,
    pub left_file_sha256: String,
    pub right_file_sha256: String,
    pub families: Vec<CompareApplicationFamilySummary>,
    pub limits: CompareApplicationLimitsApplied,
    pub detail_digest: String,
}

#[derive(Clone, Copy)]
struct ColumnSpec {
    name: &'static str,
    json: bool,
}

const fn column(name: &'static str) -> ColumnSpec {
    ColumnSpec { name, json: false }
}

const fn json_column(name: &'static str) -> ColumnSpec {
    ColumnSpec { name, json: true }
}

#[derive(Clone, Copy)]
struct TableSpec {
    name: &'static str,
    columns: &'static [ColumnSpec],
}

#[derive(Clone, Copy)]
struct FamilySpec {
    family: CompareApplicationFamily,
    tables: &'static [TableSpec],
}

const MANIFEST_PERMISSIONS_COLUMNS: &[ColumnSpec] =
    &[column("id"), json_column("permissions_json")];
const MANIFEST_IDENTITY_COLUMNS: &[ColumnSpec] = &[
    column("id"),
    column("format_id"),
    column("format_version"),
    column("app_id"),
    column("app_version"),
    column("entry_asset"),
    column("runtime_protocol"),
    column("data_schema_id"),
    column("data_schema_version"),
    column("minimum_host_profile"),
    column("released_at"),
];
const APPLICATION_COLUMNS: &[ColumnSpec] = &[
    column("id"),
    column("name"),
    column("description"),
    column("category"),
    column("icon_asset"),
    column("release_notes_doc"),
];
const PUBLISHER_COLUMNS: &[ColumnSpec] = &[
    column("id"),
    column("profile"),
    column("publisher_id"),
    column("publisher_name"),
];
const ASSET_COLUMNS: &[ColumnSpec] = &[
    column("path"),
    column("media_type"),
    // Conformance already verifies this digest against the retained asset
    // bytes. Comparing the digest avoids materialising valid 1–16 MiB assets
    // into the bounded application-detail response path.
    column("sha256"),
    column("executable"),
    column("cache_policy"),
    column("description"),
];
const COMMAND_COLUMNS: &[ColumnSpec] = &[
    column("id"),
    column("purpose"),
    column("platform"),
    column("cwd"),
    column("command_template"),
    json_column("argv_json"),
    column("risk_class"),
    column("success_condition"),
];
const RUNBOOK_COLUMNS: &[ColumnSpec] = &[
    column("id"),
    column("audience"),
    column("sequence"),
    column("title"),
    column("body_md"),
    column("command_id"),
];
const DOC_COLUMNS: &[ColumnSpec] = &[
    column("slug"),
    column("title"),
    column("media_type"),
    column("content"),
    column("sequence"),
];
const PROMPT_COLUMNS: &[ColumnSpec] = &[
    column("id"),
    column("title"),
    column("prompt_text"),
    column("sequence"),
];
const CHECK_COLUMNS: &[ColumnSpec] = &[
    column("id"),
    column("severity"),
    column("description"),
    column("sql_text"),
    column("result_mode"),
    json_column("expected_json"),
];
const ENDPOINT_COLUMNS: &[ColumnSpec] = &[
    column("name"),
    column("operation"),
    column("sql_text"),
    json_column("parameters_json"),
    column("result_mode"),
    column("description"),
    column("enabled"),
];
const ENDPOINT_STEP_COLUMNS: &[ColumnSpec] = &[
    column("endpoint_name"),
    column("sequence"),
    column("sql_text"),
    column("required_changes"),
];
const DATASET_COLUMNS: &[ColumnSpec] = &[
    column("id"),
    column("role"),
    column("description"),
    column("fork_policy"),
    column("compare_policy"),
    column("reconcile_policy"),
    column("upgrade_policy"),
    column("sensitivity"),
    column("required"),
];
const DATASET_TABLE_COLUMNS: &[ColumnSpec] = &[
    column("dataset_id"),
    column("table_name"),
    column("sequence"),
    json_column("primary_key_json"),
    json_column("ignored_columns_json"),
    json_column("immutable_columns_json"),
];
const DATASET_DEPENDENCY_COLUMNS: &[ColumnSpec] = &[
    column("dataset_id"),
    column("depends_on_dataset_id"),
    column("reason"),
];
const MIGRATION_COLUMNS: &[ColumnSpec] = &[
    column("id"),
    column("data_schema_id"),
    column("from_version"),
    column("to_version"),
    column("description"),
    column("operation_profile"),
    column("reversible"),
];
const MIGRATION_STEP_COLUMNS: &[ColumnSpec] = &[
    column("migration_id"),
    column("sequence"),
    column("operation"),
    json_column("definition_json"),
];
const MIGRATION_CHECK_COLUMNS: &[ColumnSpec] = &[
    column("migration_id"),
    column("sequence"),
    column("stage"),
    column("severity"),
    column("description"),
    json_column("definition_json"),
];
const SIGNATURE_COLUMNS: &[ColumnSpec] = &[
    column("key_id"),
    column("algorithm"),
    column("public_key"),
    column("application_digest"),
    column("signature"),
    column("signed_at"),
];

const MANIFEST_PERMISSIONS_TABLES: &[TableSpec] = &[TableSpec {
    name: "capsule_manifest",
    columns: MANIFEST_PERMISSIONS_COLUMNS,
}];
const APPLICATION_IDENTITY_TABLES: &[TableSpec] = &[
    TableSpec {
        name: "capsule_manifest",
        columns: MANIFEST_IDENTITY_COLUMNS,
    },
    TableSpec {
        name: "capsule_application",
        columns: APPLICATION_COLUMNS,
    },
];
const PUBLISHER_TABLES: &[TableSpec] = &[TableSpec {
    name: "capsule_publisher",
    columns: PUBLISHER_COLUMNS,
}];
const ASSET_TABLES: &[TableSpec] = &[TableSpec {
    name: "capsule_asset",
    columns: ASSET_COLUMNS,
}];
const COMMAND_TABLES: &[TableSpec] = &[TableSpec {
    name: "capsule_command",
    columns: COMMAND_COLUMNS,
}];
const RUNBOOK_TABLES: &[TableSpec] = &[TableSpec {
    name: "capsule_runbook",
    columns: RUNBOOK_COLUMNS,
}];
const DOC_TABLES: &[TableSpec] = &[TableSpec {
    name: "capsule_doc",
    columns: DOC_COLUMNS,
}];
const PROMPT_TABLES: &[TableSpec] = &[TableSpec {
    name: "capsule_prompt",
    columns: PROMPT_COLUMNS,
}];
const CHECK_TABLES: &[TableSpec] = &[TableSpec {
    name: "capsule_check",
    columns: CHECK_COLUMNS,
}];
const ENDPOINT_TABLES: &[TableSpec] = &[
    TableSpec {
        name: "capsule_endpoint",
        columns: ENDPOINT_COLUMNS,
    },
    TableSpec {
        name: "capsule_endpoint_step",
        columns: ENDPOINT_STEP_COLUMNS,
    },
];
const DATA_CONTRACT_TABLES: &[TableSpec] = &[
    TableSpec {
        name: "capsule_dataset",
        columns: DATASET_COLUMNS,
    },
    TableSpec {
        name: "capsule_dataset_table",
        columns: DATASET_TABLE_COLUMNS,
    },
    TableSpec {
        name: "capsule_dataset_dependency",
        columns: DATASET_DEPENDENCY_COLUMNS,
    },
];
const MIGRATION_CONTRACT_TABLES: &[TableSpec] = &[
    TableSpec {
        name: "capsule_migration",
        columns: MIGRATION_COLUMNS,
    },
    TableSpec {
        name: "capsule_migration_step",
        columns: MIGRATION_STEP_COLUMNS,
    },
    TableSpec {
        name: "capsule_migration_check",
        columns: MIGRATION_CHECK_COLUMNS,
    },
];
const SIGNATURE_TABLES: &[TableSpec] = &[TableSpec {
    name: "capsule_signature",
    columns: SIGNATURE_COLUMNS,
}];

const FAMILY_SPECS: &[FamilySpec] = &[
    FamilySpec {
        family: CompareApplicationFamily::ManifestPermissions,
        tables: MANIFEST_PERMISSIONS_TABLES,
    },
    FamilySpec {
        family: CompareApplicationFamily::ApplicationIdentity,
        tables: APPLICATION_IDENTITY_TABLES,
    },
    FamilySpec {
        family: CompareApplicationFamily::PublisherIdentity,
        tables: PUBLISHER_TABLES,
    },
    FamilySpec {
        family: CompareApplicationFamily::Assets,
        tables: ASSET_TABLES,
    },
    FamilySpec {
        family: CompareApplicationFamily::Commands,
        tables: COMMAND_TABLES,
    },
    FamilySpec {
        family: CompareApplicationFamily::Runbooks,
        tables: RUNBOOK_TABLES,
    },
    FamilySpec {
        family: CompareApplicationFamily::Documents,
        tables: DOC_TABLES,
    },
    FamilySpec {
        family: CompareApplicationFamily::Prompts,
        tables: PROMPT_TABLES,
    },
    FamilySpec {
        family: CompareApplicationFamily::Checks,
        tables: CHECK_TABLES,
    },
    FamilySpec {
        family: CompareApplicationFamily::Endpoints,
        tables: ENDPOINT_TABLES,
    },
    FamilySpec {
        family: CompareApplicationFamily::DataContracts,
        tables: DATA_CONTRACT_TABLES,
    },
    FamilySpec {
        family: CompareApplicationFamily::MigrationContracts,
        tables: MIGRATION_CONTRACT_TABLES,
    },
    FamilySpec {
        family: CompareApplicationFamily::SignatureInventory,
        tables: SIGNATURE_TABLES,
    },
];

pub fn compare_application_detail(
    left: &VerifiedWorkspaceSource,
    right: &VerifiedWorkspaceSource,
    expected_summary: &CompareSummary,
    requested: &CompareApplicationLimits,
    cancellation: &CancellationToken,
) -> Result<CompareApplicationDetail, WorkspaceError> {
    let (limits, operation_deadline) = effective_limits(requested)?;
    let control = WorkspaceControl::new(operation_deadline, cancellation);
    control.install(left.verified.connection())?;
    if let Err(error) = control.install(right.verified.connection()) {
        let _ = left
            .verified
            .connection()
            .progress_handler(0, None::<fn() -> bool>);
        return Err(error);
    }
    let result = compare_inner(left, right, expected_summary, &limits, &control);
    let _ = left
        .verified
        .connection()
        .progress_handler(0, None::<fn() -> bool>);
    let _ = right
        .verified
        .connection()
        .progress_handler(0, None::<fn() -> bool>);
    let mut detail = match result {
        Ok(detail) => detail,
        Err(error) => {
            control.check()?;
            return Err(error);
        }
    };
    control.check()?;
    let rebind = crate::WorkspaceLimits {
        deadline: control.remaining()?,
        ..crate::WorkspaceLimits::default()
    };
    left.assert_current_with_control(&rebind, cancellation)?;
    let rebind = crate::WorkspaceLimits {
        deadline: control.remaining()?,
        ..crate::WorkspaceLimits::default()
    };
    right.assert_current_with_control(&rebind, cancellation)?;
    detail.detail_digest = detail_digest(&detail)?;
    let report_bytes = serde_json::to_vec(&detail).map_err(|_| invalid_contract())?;
    if u64::try_from(report_bytes.len()).map_err(|_| limit_exceeded())? > limits.max_report_bytes {
        return Err(limit_exceeded());
    }
    control.check()?;
    Ok(detail)
}

fn compare_inner(
    left: &VerifiedWorkspaceSource,
    right: &VerifiedWorkspaceSource,
    expected: &CompareSummary,
    limits: &CompareApplicationLimitsApplied,
    control: &WorkspaceControl,
) -> Result<CompareApplicationDetail, WorkspaceError> {
    validate_binding(left, right, expected)?;
    validate_signature_inventory(left)?;
    validate_signature_inventory(right)?;
    let table_count: usize = FAMILY_SPECS.iter().map(|family| family.tables.len()).sum();
    if table_count > limits.max_tables {
        return Err(limit_exceeded());
    }
    let mut total_rows = 0_u64;
    let mut stream_bytes = 0_u64;
    let mut families = Vec::with_capacity(FAMILY_SPECS.len());
    for spec in FAMILY_SPECS {
        control.check()?;
        let left_side = hash_family(
            left.verified.connection(),
            spec,
            limits,
            &mut total_rows,
            &mut stream_bytes,
            control,
        )?;
        let right_side = hash_family(
            right.verified.connection(),
            spec,
            limits,
            &mut total_rows,
            &mut stream_bytes,
            control,
        )?;
        if spec.family == CompareApplicationFamily::SignatureInventory
            && (left_side.rows
                != u64::try_from(left.signature_reports().len()).map_err(|_| limit_exceeded())?
                || right_side.rows
                    != u64::try_from(right.signature_reports().len())
                        .map_err(|_| limit_exceeded())?)
        {
            return Err(WorkspaceError::new(WorkspaceErrorCode::InvalidSignature));
        }
        let change_count = changed_rows(&left_side.row_hashes, &right_side.row_hashes)?;
        families.push(CompareApplicationFamilySummary {
            family: spec.family,
            state: if left_side.digest == right_side.digest {
                CompareApplicationFamilyState::Same
            } else {
                CompareApplicationFamilyState::Different
            },
            table_count: spec.tables.len(),
            left_rows: left_side.rows,
            right_rows: right_side.rows,
            left_digest: left_side.digest,
            right_digest: right_side.digest,
            change_count,
        });
    }
    Ok(CompareApplicationDetail {
        profile: COMPARE_APPLICATION_PROFILE,
        comparison_report_digest: expected.report_digest.clone(),
        left_file_sha256: expected.left.file_sha256.clone(),
        right_file_sha256: expected.right.file_sha256.clone(),
        families,
        limits: limits.clone(),
        detail_digest: String::new(),
    })
}

fn validate_binding(
    left: &VerifiedWorkspaceSource,
    right: &VerifiedWorkspaceSource,
    expected: &CompareSummary,
) -> Result<(), WorkspaceError> {
    let mut value = serde_json::to_value(expected).map_err(|_| invalid_contract())?;
    value
        .as_object_mut()
        .ok_or_else(invalid_contract)?
        .remove("report_digest");
    let digest = lower_hex(&Sha256::digest(crate::plan::canonical_json(&value)?));
    if expected.profile != crate::COMPARE_SUMMARY_PROFILE
        || expected.report_digest != digest
        || expected.left.file_sha256 != left.source_sha256()
        || expected.right.file_sha256 != right.source_sha256()
        || expected.left.application_digest != lower_hex(left.application_digest())
        || expected.right.application_digest != lower_hex(right.application_digest())
    {
        return Err(WorkspaceError::new(WorkspaceErrorCode::StalePlan));
    }
    Ok(())
}

fn validate_signature_inventory(source: &VerifiedWorkspaceSource) -> Result<(), WorkspaceError> {
    if source.signature_reports().is_empty()
        || source.signature_reports().len() > sqlite_capsule_crypto::MAX_SIGNATURES
        || !source
            .signature_reports()
            .iter()
            .all(|report| report.cryptographically_valid && report.digest_matches)
    {
        return Err(WorkspaceError::new(WorkspaceErrorCode::InvalidSignature));
    }
    Ok(())
}

struct FamilySide {
    rows: u64,
    row_hashes: Vec<[u8; 32]>,
    digest: String,
}

fn hash_family(
    connection: &Connection,
    spec: &FamilySpec,
    limits: &CompareApplicationLimitsApplied,
    total_rows: &mut u64,
    stream_bytes: &mut u64,
    control: &WorkspaceControl,
) -> Result<FamilySide, WorkspaceError> {
    let mut row_hashes: Vec<[u8; 32]> = Vec::new();
    let mut rows = 0_u64;
    for table in spec.tables {
        control.check()?;
        let select = table
            .columns
            .iter()
            .map(|column| quote_identifier(column.name))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT {select} FROM {} LIMIT ?1",
            quote_identifier(table.name)
        );
        let mut statement = connection.prepare(&sql).map_err(|_| invalid_contract())?;
        let limit = i64::try_from(
            limits
                .max_rows_per_table
                .checked_add(1)
                .ok_or_else(limit_exceeded)?,
        )
        .map_err(|_| limit_exceeded())?;
        let mut query = statement.query([limit]).map_err(|_| invalid_contract())?;
        let mut table_rows = 0_u64;
        while let Some(row) = query.next().map_err(|_| invalid_contract())? {
            control.check()?;
            if table_rows == limits.max_rows_per_table || *total_rows == limits.max_total_rows {
                return Err(limit_exceeded());
            }
            let mut frame = Vec::new();
            frame_text(&mut frame, ROW_PROFILE)?;
            frame_text(&mut frame, table.name)?;
            frame_u32(&mut frame, table.columns.len())?;
            for (index, column) in table.columns.iter().enumerate() {
                frame_text(&mut frame, column.name)?;
                frame_value(
                    &mut frame,
                    row.get_ref(index).map_err(|_| invalid_contract())?,
                    column.json,
                    limits.max_value_bytes,
                )?;
            }
            let bytes = u64::try_from(frame.len()).map_err(|_| limit_exceeded())?;
            *stream_bytes = stream_bytes.checked_add(bytes).ok_or_else(limit_exceeded)?;
            if *stream_bytes > limits.max_stream_bytes {
                return Err(limit_exceeded());
            }
            let digest = Sha256::digest(frame);
            let mut row_hash = [0_u8; 32];
            row_hash.copy_from_slice(&digest);
            row_hashes.push(row_hash);
            table_rows += 1;
            rows += 1;
            *total_rows += 1;
        }
    }
    row_hashes.sort_unstable();
    let mut family_frame = Vec::new();
    frame_text(&mut family_frame, FAMILY_PROFILE)?;
    frame_text(&mut family_frame, spec.family.as_str())?;
    family_frame.extend_from_slice(&rows.to_be_bytes());
    for hash in &row_hashes {
        family_frame.extend_from_slice(hash);
    }
    Ok(FamilySide {
        rows,
        row_hashes,
        digest: lower_hex(&Sha256::digest(family_frame)),
    })
}

fn frame_value(
    output: &mut Vec<u8>,
    value: ValueRef<'_>,
    json: bool,
    max_value_bytes: u64,
) -> Result<(), WorkspaceError> {
    match value {
        ValueRef::Null => output.push(0),
        ValueRef::Integer(value) => {
            output.push(1);
            output.extend_from_slice(&value.to_be_bytes());
        }
        ValueRef::Real(value) if value.is_finite() => {
            output.push(2);
            output.extend_from_slice(&value.to_bits().to_be_bytes());
        }
        ValueRef::Real(_) => return Err(invalid_contract()),
        ValueRef::Text(bytes) if json => {
            check_value_length(bytes, max_value_bytes)?;
            let value = crate::plan::strict_json(bytes)?;
            let canonical = crate::plan::canonical_json(&value)?;
            frame_bytes(output, 5, &canonical, max_value_bytes)?;
        }
        ValueRef::Text(bytes) => {
            std::str::from_utf8(bytes).map_err(|_| invalid_contract())?;
            frame_bytes(output, 3, bytes, max_value_bytes)?;
        }
        ValueRef::Blob(bytes) => frame_bytes(output, 4, bytes, max_value_bytes)?,
    }
    Ok(())
}

fn frame_bytes(
    output: &mut Vec<u8>,
    tag: u8,
    bytes: &[u8],
    max: u64,
) -> Result<(), WorkspaceError> {
    check_value_length(bytes, max)?;
    output.push(tag);
    output.extend_from_slice(
        &u64::try_from(bytes.len())
            .map_err(|_| limit_exceeded())?
            .to_be_bytes(),
    );
    output.extend_from_slice(bytes);
    Ok(())
}

fn check_value_length(bytes: &[u8], max: u64) -> Result<(), WorkspaceError> {
    if u64::try_from(bytes.len()).map_err(|_| limit_exceeded())? > max {
        return Err(limit_exceeded());
    }
    Ok(())
}

fn frame_text(output: &mut Vec<u8>, value: &str) -> Result<(), WorkspaceError> {
    output.extend_from_slice(
        &u64::try_from(value.len())
            .map_err(|_| limit_exceeded())?
            .to_be_bytes(),
    );
    output.extend_from_slice(value.as_bytes());
    Ok(())
}

fn frame_u32(output: &mut Vec<u8>, value: usize) -> Result<(), WorkspaceError> {
    output.extend_from_slice(
        &u32::try_from(value)
            .map_err(|_| limit_exceeded())?
            .to_be_bytes(),
    );
    Ok(())
}

fn changed_rows(left: &[[u8; 32]], right: &[[u8; 32]]) -> Result<u64, WorkspaceError> {
    let (mut left_index, mut right_index, mut left_only, mut right_only) = (0, 0, 0_u64, 0_u64);
    while left_index < left.len() && right_index < right.len() {
        match left[left_index].cmp(&right[right_index]) {
            Ordering::Less => {
                left_only += 1;
                left_index += 1;
            }
            Ordering::Greater => {
                right_only += 1;
                right_index += 1;
            }
            Ordering::Equal => {
                left_index += 1;
                right_index += 1;
            }
        }
    }
    left_only = left_only
        .checked_add(u64::try_from(left.len() - left_index).map_err(|_| limit_exceeded())?)
        .ok_or_else(limit_exceeded)?;
    right_only = right_only
        .checked_add(u64::try_from(right.len() - right_index).map_err(|_| limit_exceeded())?)
        .ok_or_else(limit_exceeded)?;
    Ok(left_only.max(right_only))
}

fn detail_digest(detail: &CompareApplicationDetail) -> Result<String, WorkspaceError> {
    let mut value = serde_json::to_value(detail).map_err(|_| invalid_contract())?;
    value
        .as_object_mut()
        .ok_or_else(invalid_contract)?
        .remove("detail_digest");
    Ok(lower_hex(&Sha256::digest(crate::plan::canonical_json(
        &value,
    )?)))
}

fn effective_limits(
    requested: &CompareApplicationLimits,
) -> Result<(CompareApplicationLimitsApplied, Duration), WorkspaceError> {
    let declared_deadline = requested.deadline.min(HARD_DEADLINE);
    let operation_deadline = requested
        .operation_deadline
        .unwrap_or(declared_deadline)
        .min(declared_deadline)
        .min(HARD_DEADLINE);
    let applied = CompareApplicationLimitsApplied {
        max_tables: requested.max_tables.min(HARD_TABLES),
        max_rows_per_table: requested.max_rows_per_table.min(HARD_ROWS_PER_TABLE),
        max_total_rows: requested.max_total_rows.min(HARD_TOTAL_ROWS),
        max_value_bytes: requested.max_value_bytes.min(HARD_VALUE_BYTES),
        max_stream_bytes: requested.max_stream_bytes.min(HARD_STREAM_BYTES),
        max_report_bytes: requested.max_report_bytes.min(HARD_REPORT_BYTES),
        deadline_ms: u64::try_from(declared_deadline.as_millis()).map_err(|_| limit_exceeded())?,
    };
    if applied.max_tables == 0
        || applied.max_rows_per_table == 0
        || applied.max_total_rows == 0
        || applied.max_value_bytes == 0
        || applied.max_stream_bytes == 0
        || applied.max_report_bytes == 0
        || applied.deadline_ms == 0
        || operation_deadline.is_zero()
    {
        return Err(limit_exceeded());
    }
    Ok((applied, operation_deadline))
}

fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}
fn lower_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
const fn invalid_contract() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::InvalidContract)
}
const fn limit_exceeded() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::LimitExceeded)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use ed25519_dalek::SigningKey;
    use rusqlite::{Connection, params};
    use sqlite_capsule_crypto::{PROFILE_V03, application_digest, sign_digest_for_profile};

    use super::*;

    const DEVELOPMENT_SEED: &str =
        include_str!("../../../../compatibility/signed-app-v0.2/development-seed.hex");

    #[test]
    fn deterministic_same_detail_is_value_free_and_read_only() {
        let (_left_dir, left_path) = crate::tests::signed_fixture("application-detail-left");
        let (_right_dir, right_path) = crate::tests::signed_fixture("application-detail-right");
        let left_before = fs::read(&left_path).unwrap();
        let right_before = fs::read(&right_path).unwrap();
        let left = VerifiedWorkspaceSource::open(&left_path).unwrap();
        let right = VerifiedWorkspaceSource::open(&right_path).unwrap();
        let summary = crate::compare_sources(
            &left,
            &right,
            &crate::CompareLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        let first = compare_application_detail(
            &left,
            &right,
            &summary,
            &CompareApplicationLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        let second = compare_application_detail(
            &left,
            &right,
            &summary,
            &CompareApplicationLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        assert_eq!(first, second);
        assert_eq!(first.families.len(), FAMILY_SPECS.len());
        assert!(
            first
                .families
                .iter()
                .all(|family| family.state == CompareApplicationFamilyState::Same
                    && family.change_count == 0)
        );
        let json = serde_json::to_string(&first).unwrap();
        assert!(!json.contains("vector.read"));
        assert!(!json.contains("SELECT"));
        assert!(!json.contains(left_path.to_string_lossy().as_ref()));
        assert_eq!(fs::read(&left_path).unwrap(), left_before);
        assert_eq!(fs::read(&right_path).unwrap(), right_before);
    }

    #[test]
    fn fixed_families_detect_signed_permission_asset_endpoint_and_contract_changes_without_values()
    {
        let (_left_dir, left_path) = crate::tests::signed_fixture("application-families-left");
        let (_right_dir, right_path) = crate::tests::signed_fixture("application-families-right");
        let connection = Connection::open(&right_path).unwrap();
        connection
            .execute(
                "UPDATE capsule_manifest SET \
                 permissions_json=json_set(permissions_json,'$.z',1)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE capsule_application SET description='secret application value'",
                [],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE capsule_publisher SET publisher_name='Secret Publisher'",
                [],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE capsule_asset SET description='secret asset value' \
                 WHERE path='app/index.html'",
                [],
            )
            .unwrap();
        connection.execute("UPDATE capsule_endpoint SET description='secret endpoint value' WHERE name='vector.write'", []).unwrap();
        connection
            .execute(
                "UPDATE capsule_dataset SET description='secret contract value' WHERE id='content'",
                [],
            )
            .unwrap();
        resign(&connection);
        drop(connection);
        let left = VerifiedWorkspaceSource::open(&left_path).unwrap();
        let right = VerifiedWorkspaceSource::open(&right_path).unwrap();
        let summary = crate::compare_sources(
            &left,
            &right,
            &crate::CompareLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        let detail = compare_application_detail(
            &left,
            &right,
            &summary,
            &CompareApplicationLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        for family in [
            CompareApplicationFamily::ManifestPermissions,
            CompareApplicationFamily::ApplicationIdentity,
            CompareApplicationFamily::PublisherIdentity,
            CompareApplicationFamily::Assets,
            CompareApplicationFamily::Endpoints,
            CompareApplicationFamily::DataContracts,
        ] {
            assert_eq!(
                find(&detail, family).state,
                CompareApplicationFamilyState::Different
            );
            assert!(find(&detail, family).change_count > 0);
        }
        let json = serde_json::to_string(&detail).unwrap();
        for secret in [
            "secret application value",
            "Secret Publisher",
            "secret asset value",
            "secret endpoint value",
            "secret contract value",
        ] {
            assert!(!json.contains(secret));
        }
    }

    #[test]
    fn valid_multi_megabyte_asset_uses_verified_digest_without_materialising_content() {
        let (_left_dir, left_path) = crate::tests::signed_fixture("application-large-asset-left");
        let (_right_dir, right_path) =
            crate::tests::signed_fixture("application-large-asset-right");
        let content = vec![0x5a_u8; 2 * 1024 * 1024];
        let digest = lower_hex(&Sha256::digest(&content));
        let connection = Connection::open(&right_path).unwrap();
        connection
            .execute(
                "UPDATE capsule_asset SET content=?1,sha256=?2 WHERE path='app/index.html'",
                params![content, digest],
            )
            .unwrap();
        resign(&connection);
        drop(connection);

        let left = VerifiedWorkspaceSource::open(&left_path).unwrap();
        let right = VerifiedWorkspaceSource::open(&right_path).unwrap();
        let summary = crate::compare_sources(
            &left,
            &right,
            &crate::CompareLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        let detail = compare_application_detail(
            &left,
            &right,
            &summary,
            &CompareApplicationLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        let assets = find(&detail, CompareApplicationFamily::Assets);
        assert_eq!(assets.state, CompareApplicationFamilyState::Different);
        assert_eq!((assets.left_rows, assets.right_rows), (3, 3));
        assert!(serde_json::to_vec(&detail).unwrap().len() < 1024 * 1024);
    }

    #[test]
    fn complete_valid_signature_inventory_is_compared_and_invalid_inventory_fails_closed() {
        let (_left_dir, left_path) = crate::tests::signed_fixture("application-signature-left");
        let (_right_dir, right_path) = crate::tests::signed_fixture("application-signature-right");
        let connection = Connection::open(&right_path).unwrap();
        add_valid_signature(&connection, [7_u8; 32], "2026-08-09T12:34:56Z");
        drop(connection);
        let left = VerifiedWorkspaceSource::open(&left_path).unwrap();
        let right = VerifiedWorkspaceSource::open(&right_path).unwrap();
        let summary = crate::compare_sources(
            &left,
            &right,
            &crate::CompareLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        let detail = compare_application_detail(
            &left,
            &right,
            &summary,
            &CompareApplicationLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        let signatures = find(&detail, CompareApplicationFamily::SignatureInventory);
        assert_eq!(signatures.state, CompareApplicationFamilyState::Different);
        assert_eq!((signatures.left_rows, signatures.right_rows), (1, 2));

        let (_invalid_dir, invalid_path) =
            crate::tests::signed_fixture("application-signature-invalid");
        let connection = Connection::open(&invalid_path).unwrap();
        connection
            .execute("UPDATE capsule_signature SET signature=zeroblob(64)", [])
            .unwrap();
        drop(connection);
        assert_eq!(
            VerifiedWorkspaceSource::open(&invalid_path)
                .err()
                .expect("invalid signature must fail admission")
                .kind(),
            WorkspaceErrorCode::InvalidSignature
        );
    }

    #[test]
    fn binding_limits_cancellation_and_source_change_fail_closed_without_leaks() {
        let (_left_dir, left_path) = crate::tests::signed_fixture("application-control-left");
        let (_right_dir, right_path) = crate::tests::signed_fixture("application-control-right");
        let left = VerifiedWorkspaceSource::open(&left_path).unwrap();
        let right = VerifiedWorkspaceSource::open(&right_path).unwrap();
        let summary = crate::compare_sources(
            &left,
            &right,
            &crate::CompareLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        let mut edited = summary.clone();
        edited.report_digest = "0".repeat(64);
        assert_eq!(
            compare_application_detail(
                &left,
                &right,
                &edited,
                &CompareApplicationLimits::default(),
                &CancellationToken::new()
            )
            .unwrap_err()
            .kind(),
            WorkspaceErrorCode::StalePlan
        );
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        assert_eq!(
            compare_application_detail(
                &left,
                &right,
                &summary,
                &CompareApplicationLimits::default(),
                &cancellation
            )
            .unwrap_err()
            .kind(),
            WorkspaceErrorCode::Cancelled
        );
        for limits in [
            CompareApplicationLimits {
                max_tables: 1,
                ..CompareApplicationLimits::default()
            },
            CompareApplicationLimits {
                max_total_rows: 1,
                ..CompareApplicationLimits::default()
            },
            CompareApplicationLimits {
                max_stream_bytes: 1,
                ..CompareApplicationLimits::default()
            },
            CompareApplicationLimits {
                max_report_bytes: 1,
                ..CompareApplicationLimits::default()
            },
        ] {
            assert_eq!(
                compare_application_detail(
                    &left,
                    &right,
                    &summary,
                    &limits,
                    &CancellationToken::new()
                )
                .unwrap_err()
                .kind(),
                WorkspaceErrorCode::LimitExceeded
            );
        }
        let connection = Connection::open(&right_path).unwrap();
        connection
            .execute("UPDATE capsule_instance SET title='secret raced title'", [])
            .unwrap();
        drop(connection);
        let error = compare_application_detail(
            &left,
            &right,
            &summary,
            &CompareApplicationLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap_err();
        assert_eq!(error.kind(), WorkspaceErrorCode::StalePlan);
        assert!(!format!("{error:?}").contains("secret raced title"));
    }

    fn find(
        detail: &CompareApplicationDetail,
        family: CompareApplicationFamily,
    ) -> &CompareApplicationFamilySummary {
        detail
            .families
            .iter()
            .find(|item| item.family == family)
            .unwrap()
    }

    fn resign(connection: &Connection) {
        connection
            .execute("DELETE FROM capsule_signature", [])
            .unwrap();
        let digest = application_digest(connection).unwrap();
        let mut seed = [0_u8; 32];
        let seed_text = DEVELOPMENT_SEED.trim();
        for (index, byte) in seed.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&seed_text[index * 2..index * 2 + 2], 16).unwrap();
        }
        let key = SigningKey::from_bytes(&seed);
        seed.fill(0);
        let envelope =
            sign_digest_for_profile(&key, digest, "2026-08-08T12:34:56Z", PROFILE_V03).unwrap();
        connection
            .execute(
                "INSERT INTO capsule_signature VALUES (?1,'ed25519',?2,?3,?4,?5)",
                params![
                    envelope.key_id,
                    envelope.public_key.as_slice(),
                    envelope.application_digest.as_slice(),
                    envelope.signature.as_slice(),
                    envelope.signed_at
                ],
            )
            .unwrap();
    }

    fn add_valid_signature(connection: &Connection, mut seed: [u8; 32], signed_at: &str) {
        let key = SigningKey::from_bytes(&seed);
        seed.fill(0);
        let envelope = sign_digest_for_profile(
            &key,
            application_digest(connection).unwrap(),
            signed_at,
            PROFILE_V03,
        )
        .unwrap();
        connection
            .execute(
                "INSERT INTO capsule_signature VALUES (?1,'ed25519',?2,?3,?4,?5)",
                params![
                    envelope.key_id,
                    envelope.public_key.as_slice(),
                    envelope.application_digest.as_slice(),
                    envelope.signature.as_slice(),
                    envelope.signed_at
                ],
            )
            .unwrap();
    }
}

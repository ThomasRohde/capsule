//! Opaque, bounded detail pagination for a verified comparison pair.
//!
//! Callers select only numeric positions in the already verified signed data
//! contract. The continuation cursor is an in-memory Rust capability: it has
//! private fields, is consumed on use, cannot be serialized, and never carries
//! row values.

use std::{cmp::Ordering, fmt, time::Duration};

use rusqlite::{Connection, params, types::ValueRef};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{
    CancellationToken, ComparePolicy, Dataset, DatasetTable, Sensitivity, VerifiedWorkspaceSource,
    WorkspaceControl, WorkspaceError, WorkspaceErrorCode, compare::CompareValue,
};

pub const COMPARE_PAGE_PROFILE: &str = "org.sqlite-capsule.compare-page/1";

const HARD_DEADLINE: Duration = Duration::from_secs(30);
const HARD_PAGE_SIZE: usize = 100;
const HARD_VALUE_BYTES: u64 = 1024 * 1024;
const HARD_STREAM_BYTES: u64 = 256 * 1024 * 1024;
const HARD_DISPLAY_BYTES: usize = 4096;
const HARD_PAIR_ROWS: u64 = 100_000;
const HARD_PAGE_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct CompareDetailLimits {
    pub deadline: Duration,
    pub page_size: usize,
    pub max_value_bytes: u64,
    pub max_stream_bytes: u64,
    pub max_display_bytes: usize,
}

impl Default for CompareDetailLimits {
    fn default() -> Self {
        Self {
            deadline: HARD_DEADLINE,
            page_size: 50,
            max_value_bytes: HARD_VALUE_BYTES,
            max_stream_bytes: 64 * 1024 * 1024,
            max_display_bytes: HARD_DISPLAY_BYTES,
        }
    }
}

struct AppliedLimits {
    deadline: Duration,
    page_size: usize,
    max_value_bytes: u64,
    max_stream_bytes: u64,
    max_display_bytes: usize,
}

/// Non-serializable, one-use continuation authority. Debug output is
/// deliberately opaque so logs cannot acquire source bindings or positions.
pub struct CompareCursor {
    binding: [u8; 32],
    limits_binding: [u8; 32],
    dataset_index: usize,
    table_index: usize,
    left_offset: u64,
    right_offset: u64,
    reveal_sensitive: bool,
}

impl fmt::Debug for CompareCursor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CompareCursor(<opaque>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CompareStorageClass {
    Null,
    Integer,
    Real,
    Text,
    Blob,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CompareDetailRowKind {
    Added,
    Removed,
    Changed,
    Unchanged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CompareDetailFieldKind {
    Same,
    Added,
    Removed,
    Changed,
    Redacted,
    Truncated,
}

#[derive(PartialEq, Eq, Serialize)]
pub struct CompareValueProjection {
    pub storage_class: CompareStorageClass,
    pub display: Option<String>,
    pub byte_count: u64,
    pub sha256: String,
    pub truncated: bool,
    pub redacted: bool,
}

#[derive(PartialEq, Eq, Serialize)]
pub struct CompareFieldDetail {
    pub column: String,
    pub kind: CompareDetailFieldKind,
    pub left: Option<CompareValueProjection>,
    pub right: Option<CompareValueProjection>,
}

#[derive(PartialEq, Eq, Serialize)]
pub struct CompareRowDetail {
    pub kind: CompareDetailRowKind,
    pub key_digest: String,
    pub left_digest: Option<String>,
    pub right_digest: Option<String>,
    pub fields: Vec<CompareFieldDetail>,
}

#[derive(Serialize)]
pub struct CompareDetailPage {
    pub profile: &'static str,
    pub dataset_label: String,
    pub table_label: String,
    pub sensitivity: Sensitivity,
    pub revealed: bool,
    pub rows: Vec<CompareRowDetail>,
    #[serde(skip)]
    pub next_cursor: Option<CompareCursor>,
}

impl fmt::Debug for CompareDetailPage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CompareDetailPage")
            .field("dataset_label", &self.dataset_label)
            .field("table_label", &self.table_label)
            .field("sensitivity", &sensitivity_label(self.sensitivity))
            .field("revealed", &self.revealed)
            .field("row_count", &self.rows.len())
            .field("has_more", &self.next_cursor.is_some())
            .finish()
    }
}

#[allow(clippy::too_many_arguments)]
pub fn comparison_detail_page(
    left: &VerifiedWorkspaceSource,
    right: &VerifiedWorkspaceSource,
    dataset_index: usize,
    table_index: usize,
    cursor: Option<CompareCursor>,
    reveal_sensitive: bool,
    requested: &CompareDetailLimits,
    cancellation: &CancellationToken,
) -> Result<CompareDetailPage, WorkspaceError> {
    let limits = applied_limits(requested)?;
    let control = WorkspaceControl::new(limits.deadline, cancellation);
    control.install(left.verified.connection())?;
    if let Err(error) = control.install(right.verified.connection()) {
        clear_progress(left.verified.connection());
        return Err(error);
    }
    let result = detail_inner(
        left,
        right,
        dataset_index,
        table_index,
        cursor,
        reveal_sensitive,
        &limits,
        &control,
    );
    clear_progress(left.verified.connection());
    clear_progress(right.verified.connection());
    let page = match result {
        Ok(page) => page,
        Err(error) => {
            control.check()?;
            return Err(error);
        }
    };

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
    control.check()?;
    Ok(page)
}

#[allow(clippy::too_many_arguments)]
fn detail_inner(
    left: &VerifiedWorkspaceSource,
    right: &VerifiedWorkspaceSource,
    dataset_index: usize,
    table_index: usize,
    cursor: Option<CompareCursor>,
    reveal_sensitive: bool,
    limits: &AppliedLimits,
    control: &WorkspaceControl,
) -> Result<CompareDetailPage, WorkspaceError> {
    validate_pair(left, right)?;
    let dataset = left
        .data_contract()
        .datasets
        .get(dataset_index)
        .ok_or_else(invalid_contract)?;
    let table = dataset
        .tables
        .get(table_index)
        .ok_or_else(invalid_contract)?;
    if !matches!(dataset.compare, ComparePolicy::Row | ComparePolicy::Field) {
        return Err(WorkspaceError::new(
            WorkspaceErrorCode::UnsupportedOperation,
        ));
    }
    if dataset.sensitivity == Sensitivity::Sensitive && !reveal_sensitive {
        return Err(WorkspaceError::new(
            WorkspaceErrorCode::UnsupportedOperation,
        ));
    }
    let right_dataset = &right.data_contract().datasets[dataset_index];
    let right_table = &right_dataset.tables[table_index];
    if dataset != right_dataset || table != right_table {
        return Err(WorkspaceError::new(WorkspaceErrorCode::IncompatibleSchema));
    }

    let binding = pair_binding(left, right, dataset_index, table_index);
    let limits_binding = limits_binding(limits);
    let (left_offset, right_offset) = match cursor {
        None => (0, 0),
        Some(cursor)
            if cursor.binding == binding
                && cursor.limits_binding == limits_binding
                && cursor.dataset_index == dataset_index
                && cursor.table_index == table_index
                && cursor.reveal_sensitive == reveal_sensitive =>
        {
            (cursor.left_offset, cursor.right_offset)
        }
        Some(_) => return Err(stale_cursor()),
    };

    let left_count = table_count(left.verified.connection(), &table.name)?;
    let right_count = table_count(right.verified.connection(), &table.name)?;
    if left_count
        .checked_add(right_count)
        .ok_or_else(limit_exceeded)?
        > HARD_PAIR_ROWS
    {
        return Err(limit_exceeded());
    }
    if left_offset > left_count || right_offset > right_count {
        return Err(stale_cursor());
    }

    let columns = crate::compare::compared_columns(left.verified.connection(), table, control)?;
    if columns
        != crate::compare::compared_columns(right.verified.connection(), right_table, control)?
    {
        return Err(WorkspaceError::new(WorkspaceErrorCode::IncompatibleSchema));
    }
    let fetch = limits.page_size.checked_add(1).ok_or_else(limit_exceeded)?;
    let mut stream_bytes = 0_u64;
    let left_rows = load_window(
        left.verified.connection(),
        table,
        &columns,
        left_offset,
        fetch,
        limits,
        &mut stream_bytes,
        control,
    )?;
    let right_rows = load_window(
        right.verified.connection(),
        table,
        &columns,
        right_offset,
        fetch,
        limits,
        &mut stream_bytes,
        control,
    )?;

    let mut left_index = 0_usize;
    let mut right_index = 0_usize;
    let mut rows = Vec::with_capacity(limits.page_size);
    while rows.len() < limits.page_size
        && (left_index < left_rows.len() || right_index < right_rows.len())
    {
        control.check()?;
        match (left_rows.get(left_index), right_rows.get(right_index)) {
            (Some(left_row), Some(right_row)) => {
                match crate::compare::compare_keys(&left_row.key, &right_row.key)? {
                    Ordering::Less => {
                        rows.push(project_row(
                            CompareDetailRowKind::Removed,
                            Some(left_row),
                            None,
                            &columns,
                            dataset,
                            reveal_sensitive,
                            limits.max_display_bytes,
                        )?);
                        left_index += 1;
                    }
                    Ordering::Greater => {
                        rows.push(project_row(
                            CompareDetailRowKind::Added,
                            None,
                            Some(right_row),
                            &columns,
                            dataset,
                            reveal_sensitive,
                            limits.max_display_bytes,
                        )?);
                        right_index += 1;
                    }
                    Ordering::Equal => {
                        let kind = if left_row.row_digest == right_row.row_digest {
                            CompareDetailRowKind::Unchanged
                        } else {
                            CompareDetailRowKind::Changed
                        };
                        rows.push(project_row(
                            kind,
                            Some(left_row),
                            Some(right_row),
                            &columns,
                            dataset,
                            reveal_sensitive,
                            limits.max_display_bytes,
                        )?);
                        left_index += 1;
                        right_index += 1;
                    }
                }
            }
            (Some(left_row), None) => {
                rows.push(project_row(
                    CompareDetailRowKind::Removed,
                    Some(left_row),
                    None,
                    &columns,
                    dataset,
                    reveal_sensitive,
                    limits.max_display_bytes,
                )?);
                left_index += 1;
            }
            (None, Some(right_row)) => {
                rows.push(project_row(
                    CompareDetailRowKind::Added,
                    None,
                    Some(right_row),
                    &columns,
                    dataset,
                    reveal_sensitive,
                    limits.max_display_bytes,
                )?);
                right_index += 1;
            }
            (None, None) => break,
        }
    }
    let has_more = left_index < left_rows.len() || right_index < right_rows.len();
    let next_cursor = has_more.then(|| CompareCursor {
        binding,
        limits_binding,
        dataset_index,
        table_index,
        left_offset: left_offset + left_index as u64,
        right_offset: right_offset + right_index as u64,
        reveal_sensitive,
    });
    let revealed = dataset.sensitivity == Sensitivity::Sensitive && reveal_sensitive;
    let page = CompareDetailPage {
        profile: COMPARE_PAGE_PROFILE,
        dataset_label: dataset.id.clone(),
        table_label: table.name.clone(),
        sensitivity: dataset.sensitivity,
        revealed,
        rows,
        next_cursor,
    };
    let response_bytes = serde_json::to_vec(&page).map_err(|_| invalid_contract())?;
    if response_bytes.len() > HARD_PAGE_RESPONSE_BYTES {
        return Err(limit_exceeded());
    }
    Ok(page)
}

fn applied_limits(requested: &CompareDetailLimits) -> Result<AppliedLimits, WorkspaceError> {
    let applied = AppliedLimits {
        deadline: requested.deadline.min(HARD_DEADLINE),
        page_size: requested.page_size.min(HARD_PAGE_SIZE),
        max_value_bytes: requested.max_value_bytes.min(HARD_VALUE_BYTES),
        max_stream_bytes: requested.max_stream_bytes.min(HARD_STREAM_BYTES),
        max_display_bytes: requested.max_display_bytes.min(HARD_DISPLAY_BYTES),
    };
    if applied.deadline.is_zero()
        || applied.page_size == 0
        || applied.max_value_bytes == 0
        || applied.max_stream_bytes == 0
        || applied.max_display_bytes == 0
    {
        return Err(limit_exceeded());
    }
    Ok(applied)
}

fn validate_pair(
    left: &VerifiedWorkspaceSource,
    right: &VerifiedWorkspaceSource,
) -> Result<(), WorkspaceError> {
    if left.identity().app_id != right.identity().app_id {
        return Err(WorkspaceError::new(
            WorkspaceErrorCode::IncompatibleApplication,
        ));
    }
    if left.data_contract() != right.data_contract() {
        return Err(WorkspaceError::new(WorkspaceErrorCode::IncompatibleSchema));
    }
    Ok(())
}

fn pair_binding(
    left: &VerifiedWorkspaceSource,
    right: &VerifiedWorkspaceSource,
    dataset_index: usize,
    table_index: usize,
) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"org.sqlite-capsule.compare-cursor-binding/1");
    digest.update(left.source_sha256().as_bytes());
    digest.update(right.source_sha256().as_bytes());
    digest.update(dataset_index.to_be_bytes());
    digest.update(table_index.to_be_bytes());
    digest.finalize().into()
}

fn limits_binding(limits: &AppliedLimits) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"org.sqlite-capsule.compare-cursor-limits/1");
    // The trusted session enforces one absolute deadline. Remaining time
    // necessarily decreases between pages and is not a disclosure policy.
    // Bind only stable limits so a valid continuation does not invalidate
    // itself while preserving every row/value/stream/display ceiling.
    digest.update(limits.page_size.to_be_bytes());
    digest.update(limits.max_value_bytes.to_be_bytes());
    digest.update(limits.max_stream_bytes.to_be_bytes());
    digest.update(limits.max_display_bytes.to_be_bytes());
    digest.finalize().into()
}

struct DetailRow {
    key: Vec<CompareValue>,
    key_digest: String,
    row_digest: String,
    values: Vec<CompareValue>,
}

#[allow(clippy::too_many_arguments)]
fn load_window(
    connection: &Connection,
    table: &DatasetTable,
    columns: &[String],
    offset: u64,
    count: usize,
    limits: &AppliedLimits,
    stream_bytes: &mut u64,
    control: &WorkspaceControl,
) -> Result<Vec<DetailRow>, WorkspaceError> {
    let projection = columns
        .iter()
        .map(|column| crate::compare::quote_identifier(column))
        .collect::<Vec<_>>()
        .join(",");
    let ordering = table
        .primary_key
        .iter()
        .map(|column| {
            format!(
                "{} COLLATE BINARY ASC",
                crate::compare::quote_identifier(column)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT {projection} FROM {} ORDER BY {ordering} LIMIT ?1 OFFSET ?2",
        crate::compare::quote_identifier(&table.name)
    );
    let mut statement = connection.prepare(&sql).map_err(|_| invalid_contract())?;
    let mut rows = statement
        .query(params![
            i64::try_from(count).map_err(|_| limit_exceeded())?,
            i64::try_from(offset).map_err(|_| stale_cursor())?,
        ])
        .map_err(|_| invalid_contract())?;
    let key_indexes = table
        .primary_key
        .iter()
        .map(|key| {
            columns
                .iter()
                .position(|column| column == key)
                .ok_or_else(invalid_contract)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut result = Vec::new();
    while let Some(row) = rows.next().map_err(|_| invalid_contract())? {
        control.check()?;
        if result.len() == count {
            return Err(limit_exceeded());
        }
        let values = (0..columns.len())
            .map(|index| {
                owned_value(
                    row.get_ref(index).map_err(|_| invalid_contract())?,
                    limits.max_value_bytes,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let key_fields = key_indexes
            .iter()
            .map(|index| (columns[*index].clone(), values[*index].clone()))
            .collect::<Vec<_>>();
        let compared = columns
            .iter()
            .cloned()
            .zip(values.iter().cloned())
            .collect::<Vec<_>>();
        let key_frame = crate::compare::canonical_compare_key(
            &table.name,
            &key_fields,
            limits.max_value_bytes,
        )?;
        let row_frame = crate::compare::canonical_compare_row(
            &table.name,
            &key_fields,
            &compared,
            limits.max_value_bytes,
        )?;
        let row_bytes =
            u64::try_from(key_frame.len() + row_frame.len()).map_err(|_| limit_exceeded())?;
        *stream_bytes = stream_bytes
            .checked_add(row_bytes)
            .ok_or_else(limit_exceeded)?;
        if *stream_bytes > limits.max_stream_bytes {
            return Err(limit_exceeded());
        }
        result.push(DetailRow {
            key: key_fields.into_iter().map(|(_, value)| value).collect(),
            key_digest: crate::compare::lower_hex(&Sha256::digest(&key_frame)),
            row_digest: crate::compare::lower_hex(&Sha256::digest(&row_frame)),
            values,
        });
    }
    Ok(result)
}

fn owned_value(value: ValueRef<'_>, maximum: u64) -> Result<CompareValue, WorkspaceError> {
    let projected = match value {
        ValueRef::Null => CompareValue::Null,
        ValueRef::Integer(value) => CompareValue::Integer(value),
        ValueRef::Real(value) if value.is_finite() => CompareValue::Real(value),
        ValueRef::Real(_) => return Err(invalid_contract()),
        ValueRef::Text(value) => {
            if value.len() as u64 > maximum {
                return Err(limit_exceeded());
            }
            std::str::from_utf8(value).map_err(|_| invalid_contract())?;
            CompareValue::Text(value.to_vec())
        }
        ValueRef::Blob(value) => {
            if value.len() as u64 > maximum {
                return Err(limit_exceeded());
            }
            CompareValue::Blob(value.to_vec())
        }
    };
    Ok(projected)
}

#[allow(clippy::too_many_arguments)]
fn project_row(
    kind: CompareDetailRowKind,
    left: Option<&DetailRow>,
    right: Option<&DetailRow>,
    columns: &[String],
    dataset: &Dataset,
    reveal_sensitive: bool,
    max_display_bytes: usize,
) -> Result<CompareRowDetail, WorkspaceError> {
    let key_digest = left
        .map(|row| row.key_digest.clone())
        .or_else(|| right.map(|row| row.key_digest.clone()))
        .ok_or_else(invalid_contract)?;
    let fields = if dataset.compare == ComparePolicy::Field {
        columns
            .iter()
            .enumerate()
            .map(|(index, column)| {
                project_field(
                    column,
                    left.map(|row| &row.values[index]),
                    right.map(|row| &row.values[index]),
                    dataset.sensitivity,
                    reveal_sensitive,
                    max_display_bytes,
                )
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        Vec::new()
    };
    Ok(CompareRowDetail {
        kind,
        key_digest,
        left_digest: left.map(|row| row.row_digest.clone()),
        right_digest: right.map(|row| row.row_digest.clone()),
        fields,
    })
}

fn project_field(
    column: &str,
    left: Option<&CompareValue>,
    right: Option<&CompareValue>,
    sensitivity: Sensitivity,
    reveal_sensitive: bool,
    max_display_bytes: usize,
) -> Result<CompareFieldDetail, WorkspaceError> {
    let redact = sensitivity == Sensitivity::Sensitive && !reveal_sensitive;
    let left_projection = left
        .map(|value| project_value(value, redact, max_display_bytes))
        .transpose()?;
    let right_projection = right
        .map(|value| project_value(value, redact, max_display_bytes))
        .transpose()?;
    let base_kind = match (left, right) {
        (None, Some(_)) => CompareDetailFieldKind::Added,
        (Some(_), None) => CompareDetailFieldKind::Removed,
        (Some(left), Some(right)) if same_typed_value(left, right)? => CompareDetailFieldKind::Same,
        (Some(_), Some(_)) => CompareDetailFieldKind::Changed,
        (None, None) => return Err(invalid_contract()),
    };
    let kind = if redact {
        CompareDetailFieldKind::Redacted
    } else if left_projection
        .as_ref()
        .is_some_and(|value| value.truncated)
        || right_projection
            .as_ref()
            .is_some_and(|value| value.truncated)
    {
        CompareDetailFieldKind::Truncated
    } else {
        base_kind
    };
    Ok(CompareFieldDetail {
        column: column.to_owned(),
        kind,
        left: left_projection,
        right: right_projection,
    })
}

fn same_typed_value(left: &CompareValue, right: &CompareValue) -> Result<bool, WorkspaceError> {
    Ok(
        crate::compare::canonical_value_bytes(left, HARD_VALUE_BYTES)?
            == crate::compare::canonical_value_bytes(right, HARD_VALUE_BYTES)?,
    )
}

fn project_value(
    value: &CompareValue,
    redacted: bool,
    max_display_bytes: usize,
) -> Result<CompareValueProjection, WorkspaceError> {
    let canonical = crate::compare::canonical_value_bytes(value, HARD_VALUE_BYTES)?;
    let (storage_class, byte_count, display, truncated) = match value {
        CompareValue::Null => (
            CompareStorageClass::Null,
            0,
            (!redacted).then(|| "NULL".to_owned()),
            false,
        ),
        CompareValue::Integer(value) => (
            CompareStorageClass::Integer,
            8,
            (!redacted).then(|| value.to_string()),
            false,
        ),
        CompareValue::Real(value) => {
            let display = if value.to_bits() == (-0.0_f64).to_bits() {
                "-0.0".to_owned()
            } else {
                value.to_string()
            };
            (
                CompareStorageClass::Real,
                8,
                (!redacted).then_some(display),
                false,
            )
        }
        CompareValue::Text(value) => {
            let text = std::str::from_utf8(value).map_err(|_| invalid_contract())?;
            let (display, truncated) = if redacted {
                (None, false)
            } else {
                let (display, truncated) = truncate_utf8(text, max_display_bytes);
                (Some(display), truncated)
            };
            (
                CompareStorageClass::Text,
                value.len() as u64,
                display,
                truncated,
            )
        }
        CompareValue::Blob(value) => (CompareStorageClass::Blob, value.len() as u64, None, false),
    };
    Ok(CompareValueProjection {
        storage_class,
        display,
        byte_count,
        sha256: crate::compare::lower_hex(&Sha256::digest(canonical)),
        truncated,
        redacted,
    })
}

fn truncate_utf8(value: &str, maximum_bytes: usize) -> (String, bool) {
    if value.len() <= maximum_bytes {
        return (value.to_owned(), false);
    }
    let mut end = maximum_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    (value[..end].to_owned(), true)
}

fn table_count(connection: &Connection, table: &str) -> Result<u64, WorkspaceError> {
    let sql = format!(
        "SELECT count(*) FROM {}",
        crate::compare::quote_identifier(table)
    );
    let value: i64 = connection
        .query_row(&sql, [], |row| row.get(0))
        .map_err(|_| invalid_contract())?;
    u64::try_from(value).map_err(|_| invalid_contract())
}

fn clear_progress(connection: &Connection) {
    let _ = connection.progress_handler(0, None::<fn() -> bool>);
}

const fn sensitivity_label(value: Sensitivity) -> &'static str {
    match value {
        Sensitivity::Normal => "normal",
        Sensitivity::Sensitive => "sensitive",
    }
}

const fn invalid_contract() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::InvalidContract)
}

const fn limit_exceeded() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::LimitExceeded)
}

const fn stale_cursor() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::StalePlan)
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, fs};

    use ed25519_dalek::SigningKey;
    use rusqlite::Connection;
    use sqlite_capsule_crypto::{PROFILE_V03, application_digest, sign_digest_for_profile};

    use super::*;

    const DEVELOPMENT_SEED: &str =
        include_str!("../../../../compatibility/signed-app-v0.2/development-seed.hex");

    #[test]
    fn page_boundaries_are_continuous_clamped_and_do_not_mutate_sources() {
        let (_left_directory, left_path) = crate::tests::signed_fixture("detail-pages-left");
        let (_right_directory, right_path) = crate::tests::signed_fixture("detail-pages-right");
        let connection = Connection::open(&right_path).unwrap();
        for index in 0..205 {
            connection
                .execute(
                    "INSERT INTO vector_domain VALUES (?1, ?2, ?3, ?4)",
                    params![
                        format!("row-{index:04}"),
                        format!("value-{index}"),
                        index as f64,
                        vec![index as u8],
                    ],
                )
                .unwrap();
        }
        drop(connection);
        let left_before = fs::read(&left_path).unwrap();
        let right_before = fs::read(&right_path).unwrap();
        let left = VerifiedWorkspaceSource::open(&left_path).unwrap();
        let right = VerifiedWorkspaceSource::open(&right_path).unwrap();
        let limits = CompareDetailLimits {
            page_size: 1_000,
            ..CompareDetailLimits::default()
        };
        let first = comparison_detail_page(
            &left,
            &right,
            0,
            0,
            None,
            false,
            &limits,
            &CancellationToken::new(),
        )
        .unwrap();
        assert_eq!(first.rows.len(), HARD_PAGE_SIZE);
        let mut cursor = first.next_cursor;
        let mut digests = first
            .rows
            .into_iter()
            .map(|row| row.key_digest)
            .collect::<BTreeSet<_>>();
        let mut total = digests.len();
        while let Some(next) = cursor {
            let page = comparison_detail_page(
                &left,
                &right,
                0,
                0,
                Some(next),
                false,
                &limits,
                &CancellationToken::new(),
            )
            .unwrap();
            for row in page.rows {
                assert!(digests.insert(row.key_digest), "page repeated a row key");
                total += 1;
            }
            cursor = page.next_cursor;
        }
        assert_eq!(total, 206);
        assert_eq!(fs::read(&left_path).unwrap(), left_before);
        assert_eq!(fs::read(&right_path).unwrap(), right_before);
    }

    #[test]
    fn blob_bytes_never_render_and_normal_text_is_utf8_safely_truncated() {
        let (_left_directory, left_path) = crate::tests::signed_fixture("detail-values-left");
        let (_right_directory, right_path) = crate::tests::signed_fixture("detail-values-right");
        let left = VerifiedWorkspaceSource::open(&left_path).unwrap();
        let right = VerifiedWorkspaceSource::open(&right_path).unwrap();
        let page = comparison_detail_page(
            &left,
            &right,
            0,
            0,
            None,
            false,
            &CompareDetailLimits {
                max_display_bytes: 3,
                ..CompareDetailLimits::default()
            },
            &CancellationToken::new(),
        )
        .unwrap();
        let fields = &page.rows[0].fields;
        let note = fields.iter().find(|field| field.column == "note").unwrap();
        let note_value = note.left.as_ref().unwrap();
        assert_eq!(note.kind, CompareDetailFieldKind::Truncated);
        assert_eq!(note_value.display.as_deref(), Some("mut"));
        assert!(note_value.truncated);
        let payload = fields
            .iter()
            .find(|field| field.column == "payload")
            .unwrap();
        let payload_value = payload.left.as_ref().unwrap();
        assert_eq!(payload_value.storage_class, CompareStorageClass::Blob);
        assert_eq!(payload_value.byte_count, 3);
        assert_eq!(payload_value.display, None);
        assert!(!payload_value.redacted);
        let expected = crate::compare::canonical_value_bytes(
            &CompareValue::Blob(vec![0x10, 0x20, 0x30]),
            HARD_VALUE_BYTES,
        )
        .unwrap();
        assert_eq!(
            payload_value.sha256,
            crate::compare::lower_hex(&Sha256::digest(expected))
        );
        let json = serde_json::to_string(&page).unwrap();
        assert!(!json.contains("102030"));
        assert!(!json.contains("ECAw"));
    }

    #[test]
    fn sensitive_pages_are_counts_only_until_a_fresh_explicit_reveal() {
        let (_left_directory, left_path) = crate::tests::signed_fixture("detail-sensitive-left");
        let (_right_directory, right_path) = crate::tests::signed_fixture("detail-sensitive-right");
        for path in [&left_path, &right_path] {
            let connection = Connection::open(path).unwrap();
            connection
                .execute(
                    "UPDATE capsule_dataset SET sensitivity='sensitive' WHERE id='content'",
                    [],
                )
                .unwrap();
            resign(&connection);
        }
        let connection = Connection::open(&right_path).unwrap();
        connection
            .execute(
                "UPDATE vector_domain SET note='classified-secret' WHERE id='domain'",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO vector_domain VALUES ('second','second-secret',2.0,X'99')",
                [],
            )
            .unwrap();
        drop(connection);
        let left = VerifiedWorkspaceSource::open(&left_path).unwrap();
        let right = VerifiedWorkspaceSource::open(&right_path).unwrap();
        let limits = CompareDetailLimits {
            page_size: 1,
            ..CompareDetailLimits::default()
        };
        assert_eq!(
            comparison_detail_page(
                &left,
                &right,
                0,
                0,
                None,
                false,
                &limits,
                &CancellationToken::new(),
            )
            .expect_err("sensitive detail requires an explicit trusted reveal")
            .kind(),
            WorkspaceErrorCode::UnsupportedOperation
        );
        let revealed = comparison_detail_page(
            &left,
            &right,
            0,
            0,
            None,
            true,
            &limits,
            &CancellationToken::new(),
        )
        .unwrap();
        assert!(revealed.revealed);
        assert!(
            serde_json::to_string(&revealed)
                .unwrap()
                .contains("classified-secret")
        );
    }

    #[test]
    fn ignore_and_summary_policies_deny_detail_natively() {
        for (name, policy) in [("ignore", "ignore"), ("summary", "summary")] {
            let (_left_directory, left_path) =
                crate::tests::signed_fixture(&format!("detail-policy-{name}-left"));
            let (_right_directory, right_path) =
                crate::tests::signed_fixture(&format!("detail-policy-{name}-right"));
            for path in [&left_path, &right_path] {
                let connection = Connection::open(path).unwrap();
                connection
                    .execute(
                        "UPDATE capsule_dataset SET compare_policy=?1, reconcile_policy='ignore' \
                         WHERE id='settings'",
                        [policy],
                    )
                    .unwrap();
                resign(&connection);
            }
            let left = VerifiedWorkspaceSource::open(&left_path).unwrap();
            let right = VerifiedWorkspaceSource::open(&right_path).unwrap();
            assert_eq!(
                comparison_detail_page(
                    &left,
                    &right,
                    1,
                    0,
                    None,
                    false,
                    &CompareDetailLimits::default(),
                    &CancellationToken::new(),
                )
                .expect_err("policy must deny detail")
                .kind(),
                WorkspaceErrorCode::UnsupportedOperation
            );
        }
    }

    #[test]
    fn row_policy_returns_only_row_identity_and_never_field_values() {
        let (_left_directory, left_path) = crate::tests::signed_fixture("detail-row-left");
        let (_right_directory, right_path) = crate::tests::signed_fixture("detail-row-right");
        let connection = Connection::open(&right_path).unwrap();
        connection
            .execute(
                "UPDATE vector_settings SET value='dark' WHERE key='theme'",
                [],
            )
            .unwrap();
        drop(connection);
        let left = VerifiedWorkspaceSource::open(&left_path).unwrap();
        let right = VerifiedWorkspaceSource::open(&right_path).unwrap();
        let page = comparison_detail_page(
            &left,
            &right,
            1,
            0,
            None,
            false,
            &CompareDetailLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.rows[0].kind, CompareDetailRowKind::Changed);
        assert!(page.rows[0].fields.is_empty());
        assert!(!serde_json::to_string(&page).unwrap().contains("dark"));
    }

    #[test]
    fn cursor_is_bound_to_pair_table_limits_and_disclosure() {
        let (_left_directory, left_path) = crate::tests::signed_fixture("detail-cursor-left");
        let (_right_directory, right_path) = crate::tests::signed_fixture("detail-cursor-right");
        let connection = Connection::open(&right_path).unwrap();
        connection
            .execute(
                "INSERT INTO vector_domain VALUES ('second','value',2.0,X'01')",
                [],
            )
            .unwrap();
        drop(connection);
        let left = VerifiedWorkspaceSource::open(&left_path).unwrap();
        let right = VerifiedWorkspaceSource::open(&right_path).unwrap();
        let limits = CompareDetailLimits {
            page_size: 1,
            ..CompareDetailLimits::default()
        };
        let first = comparison_detail_page(
            &left,
            &right,
            0,
            0,
            None,
            false,
            &limits,
            &CancellationToken::new(),
        )
        .unwrap();
        assert_eq!(
            format!("{:?}", first.next_cursor.as_ref().unwrap()),
            "CompareCursor(<opaque>)"
        );
        assert_eq!(
            comparison_detail_page(
                &left,
                &right,
                1,
                0,
                first.next_cursor,
                false,
                &limits,
                &CancellationToken::new(),
            )
            .expect_err("cross-table cursor must fail")
            .kind(),
            WorkspaceErrorCode::StalePlan
        );

        let first = comparison_detail_page(
            &left,
            &right,
            0,
            0,
            None,
            false,
            &limits,
            &CancellationToken::new(),
        )
        .unwrap();
        comparison_detail_page(
            &left,
            &right,
            0,
            0,
            first.next_cursor,
            false,
            &CompareDetailLimits {
                deadline: Duration::from_secs(1),
                ..limits.clone()
            },
            &CancellationToken::new(),
        )
        .expect("remaining session time may decrease between valid pages");

        let first = comparison_detail_page(
            &left,
            &right,
            0,
            0,
            None,
            false,
            &limits,
            &CancellationToken::new(),
        )
        .unwrap();
        assert_eq!(
            comparison_detail_page(
                &left,
                &right,
                0,
                0,
                first.next_cursor,
                false,
                &CompareDetailLimits {
                    page_size: 2,
                    ..CompareDetailLimits::default()
                },
                &CancellationToken::new(),
            )
            .expect_err("limits-bound cursor must fail")
            .kind(),
            WorkspaceErrorCode::StalePlan
        );
    }

    #[test]
    fn cancellation_and_live_source_mutation_fail_with_value_free_errors_and_debug() {
        let (_left_directory, left_path) = crate::tests::signed_fixture("detail-control-left");
        let (_right_directory, right_path) = crate::tests::signed_fixture("detail-control-right");
        let connection = Connection::open(&right_path).unwrap();
        connection
            .execute(
                "INSERT INTO vector_domain VALUES ('second','secret-value',2.0,X'01')",
                [],
            )
            .unwrap();
        drop(connection);
        let left = VerifiedWorkspaceSource::open(&left_path).unwrap();
        let right = VerifiedWorkspaceSource::open(&right_path).unwrap();
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let error = comparison_detail_page(
            &left,
            &right,
            0,
            0,
            None,
            false,
            &CompareDetailLimits::default(),
            &cancellation,
        )
        .expect_err("cancelled detail must fail");
        assert_eq!(error.kind(), WorkspaceErrorCode::Cancelled);
        assert!(!format!("{error:?}").contains("secret-value"));

        let limits = CompareDetailLimits {
            page_size: 1,
            ..CompareDetailLimits::default()
        };
        let first = comparison_detail_page(
            &left,
            &right,
            0,
            0,
            None,
            false,
            &limits,
            &CancellationToken::new(),
        )
        .unwrap();
        assert!(!format!("{first:?}").contains("secret-value"));
        let connection = Connection::open(&right_path).unwrap();
        connection
            .execute(
                "UPDATE vector_domain SET note='raced-secret' WHERE id='second'",
                [],
            )
            .unwrap();
        drop(connection);
        let error = comparison_detail_page(
            &left,
            &right,
            0,
            0,
            first.next_cursor,
            false,
            &limits,
            &CancellationToken::new(),
        )
        .expect_err("source mutation must invalidate detail");
        assert_eq!(error.kind(), WorkspaceErrorCode::StalePlan);
        assert!(!format!("{error:?}").contains("raced-secret"));
    }

    #[test]
    fn composite_mixed_storage_keys_follow_typed_order_without_coercion() {
        let (_left_directory, left_path) = crate::tests::signed_fixture("detail-mixed-left");
        let (_right_directory, right_path) = crate::tests::signed_fixture("detail-mixed-right");
        install_mixed_key_fixture(&left_path, false);
        install_mixed_key_fixture(&right_path, true);
        let left = VerifiedWorkspaceSource::open(&left_path).unwrap();
        let right = VerifiedWorkspaceSource::open(&right_path).unwrap();
        let page = comparison_detail_page(
            &left,
            &right,
            0,
            0,
            None,
            false,
            &CompareDetailLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        assert_eq!(page.rows.len(), 3);
        assert_eq!(page.rows[0].kind, CompareDetailRowKind::Removed);
        assert_eq!(page.rows[1].kind, CompareDetailRowKind::Added);
        assert_eq!(page.rows[2].kind, CompareDetailRowKind::Unchanged);
        let integer_key = crate::compare::canonical_compare_key(
            "vector_domain",
            &[
                ("kind".to_owned(), CompareValue::Text(b"1".to_vec())),
                ("id".to_owned(), CompareValue::Integer(1)),
            ],
            HARD_VALUE_BYTES,
        )
        .unwrap();
        let real_key = crate::compare::canonical_compare_key(
            "vector_domain",
            &[
                ("kind".to_owned(), CompareValue::Text(b"1".to_vec())),
                ("id".to_owned(), CompareValue::Real(1.0)),
            ],
            HARD_VALUE_BYTES,
        )
        .unwrap();
        assert_eq!(
            page.rows[0].key_digest,
            crate::compare::lower_hex(&Sha256::digest(integer_key))
        );
        assert_eq!(
            page.rows[1].key_digest,
            crate::compare::lower_hex(&Sha256::digest(real_key))
        );
    }

    fn install_mixed_key_fixture(path: &std::path::Path, real_key: bool) {
        let connection = Connection::open(path).unwrap();
        connection
            .execute_batch(
                "ALTER TABLE vector_domain RENAME TO vector_domain_old; \
                 CREATE TABLE vector_domain ( \
                    kind NOT NULL, id NOT NULL, note TEXT NOT NULL, \
                    measurement REAL NOT NULL, payload BLOB NOT NULL, \
                    PRIMARY KEY (kind, id) \
                 ); \
                 DROP TABLE vector_domain_old; \
                 UPDATE capsule_dataset_table \
                 SET primary_key_json='[\"kind\",\"id\"]', \
                     immutable_columns_json='[\"kind\",\"id\"]' \
                 WHERE table_name='vector_domain';",
            )
            .unwrap();
        if real_key {
            connection
                .execute(
                    "INSERT INTO vector_domain VALUES ('1',1.0,'right',1.0,X'01')",
                    [],
                )
                .unwrap();
        } else {
            connection
                .execute(
                    "INSERT INTO vector_domain VALUES ('1',1,'left',1.0,X'01')",
                    [],
                )
                .unwrap();
        }
        connection
            .execute(
                "INSERT INTO vector_domain VALUES (X'31',0.0,'same',-0.0,X'02')",
                [],
            )
            .unwrap();
        resign(&connection);
    }

    fn resign(connection: &Connection) {
        connection
            .execute("DELETE FROM capsule_signature", [])
            .unwrap();
        let digest = application_digest(connection).unwrap();
        let seed_text = DEVELOPMENT_SEED.trim();
        let mut seed = [0_u8; 32];
        for (index, byte) in seed.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&seed_text[index * 2..index * 2 + 2], 16).unwrap();
        }
        let key = SigningKey::from_bytes(&seed);
        seed.fill(0);
        let envelope =
            sign_digest_for_profile(&key, digest, "2026-08-08T12:34:56Z", PROFILE_V03).unwrap();
        connection
            .execute(
                "INSERT INTO capsule_signature \
                 (key_id, algorithm, public_key, application_digest, signature, signed_at) \
                 VALUES (?1, 'ed25519', ?2, ?3, ?4, ?5)",
                params![
                    envelope.key_id,
                    envelope.public_key.as_slice(),
                    envelope.application_digest.as_slice(),
                    envelope.signature.as_slice(),
                    envelope.signed_at,
                ],
            )
            .unwrap();
    }
}

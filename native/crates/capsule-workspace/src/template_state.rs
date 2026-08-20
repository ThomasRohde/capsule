//! Authenticated clean-template state verification for M04.
//!
//! The proof is one reserved `capsule_doc` row in the signed application
//! compartment. Domain rows remain outside the application signature, so the
//! proof is accepted only after every declared dataset count and canonical
//! state digest is reproduced from the same verified private snapshot.

use std::{collections::BTreeSet, time::Duration};

use rusqlite::types::ValueRef;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    CancellationToken, Dataset, VerifiedWorkspaceSource, WorkspaceControl, WorkspaceError,
    WorkspaceErrorCode, WorkspaceLimits,
};

pub const TEMPLATE_STATE_PROFILE: &str = "org.sqlite-capsule.template-state/1";
pub const DATASET_STATE_PROFILE: &str = "org.sqlite-capsule.dataset-state/1";
pub const TEMPLATE_PLATFORM_RESET_PROFILE: &str = "org.sqlite-capsule.template-platform-reset/1";
pub const TEMPLATE_STATE_DOC_SLUG: &str = "org.sqlite-capsule.template-state";

const TEMPLATE_STATE_DOC_TITLE: &str = "SQLite Capsule authenticated template state";
const TEMPLATE_STATE_MEDIA_TYPE: &str = "application/vnd.sqlite-capsule.template-state+json";
const DATASET_STREAM_CONTEXT: &[u8] = b"SQLite Capsule dataset-state canonical stream v1\0";
const HARD_MAX_PROOF_BYTES: usize = 256 * 1024;
const HARD_MAX_STREAM_BYTES: u64 = 512 * 1024 * 1024;
const HARD_MAX_ROWS: u64 = 100_000;
const HARD_MAX_DEADLINE: Duration = Duration::from_secs(30);

#[derive(Clone, Debug)]
pub struct TemplateStateLimits {
    pub deadline: Duration,
    pub max_rows: u64,
    pub max_stream_bytes: u64,
}

impl Default for TemplateStateLimits {
    fn default() -> Self {
        Self {
            deadline: HARD_MAX_DEADLINE,
            max_rows: HARD_MAX_ROWS,
            max_stream_bytes: HARD_MAX_STREAM_BYTES,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TemplateDatasetDisposition {
    Seed,
    Empty,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateDatasetProof {
    pub dataset_id: String,
    pub disposition: TemplateDatasetDisposition,
    pub stored_row_count: u64,
    pub state_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateStateProof {
    pub profile: String,
    pub app_id: String,
    pub app_version: String,
    pub data_schema_id: String,
    pub data_schema_version: u64,
    pub dataset_state_profile: String,
    pub mutable_platform_state_profile: String,
    pub datasets: Vec<TemplateDatasetProof>,
}

/// Reproduces every proof digest from the retained verified snapshot.
///
/// No source path is reopened and no template state is returned unless the
/// proof is canonical, exhaustive, signed by an entirely valid signature
/// inventory, bounded, and current at both source rebinds.
pub fn verify_template_state(
    source: &VerifiedWorkspaceSource,
    limits: &TemplateStateLimits,
    cancellation: &CancellationToken,
) -> Result<TemplateStateProof, WorkspaceError> {
    let deadline = limits.deadline.min(HARD_MAX_DEADLINE);
    let max_rows = limits.max_rows.min(HARD_MAX_ROWS);
    let max_stream_bytes = limits.max_stream_bytes.min(HARD_MAX_STREAM_BYTES);
    if deadline.is_zero() || max_rows == 0 || max_stream_bytes == 0 {
        return Err(limit_exceeded());
    }
    let control = WorkspaceControl::new(deadline, cancellation);
    verify_template_state_with_control(
        source,
        max_rows,
        max_rows,
        max_stream_bytes,
        &control,
        cancellation,
    )
}

pub(crate) fn verify_template_state_with_control(
    source: &VerifiedWorkspaceSource,
    max_rows_total: u64,
    max_rows_per_dataset: u64,
    max_stream_bytes: u64,
    control: &WorkspaceControl,
    cancellation: &CancellationToken,
) -> Result<TemplateStateProof, WorkspaceError> {
    if max_rows_total == 0
        || max_rows_total > HARD_MAX_ROWS
        || max_rows_per_dataset == 0
        || max_rows_per_dataset > HARD_MAX_ROWS
        || max_stream_bytes == 0
    {
        return Err(limit_exceeded());
    }
    let max_stream_bytes = max_stream_bytes.min(HARD_MAX_STREAM_BYTES);
    if source.signature_reports().is_empty()
        || source
            .signature_reports()
            .iter()
            .any(|report| !report.cryptographically_valid || !report.digest_matches)
    {
        return Err(WorkspaceError::new(WorkspaceErrorCode::InvalidSignature));
    }
    assert_current(source, control, cancellation)?;
    control.install(source.verified.connection())?;
    let result = verify_inner(
        source,
        max_rows_total,
        max_rows_per_dataset,
        max_stream_bytes,
        control,
    );
    let _ = source
        .verified
        .connection()
        .progress_handler(0, None::<fn() -> bool>);
    let proof = result?;
    control.check()?;
    assert_current(source, control, cancellation)?;
    Ok(proof)
}

fn verify_inner(
    source: &VerifiedWorkspaceSource,
    max_rows_total: u64,
    max_rows_per_dataset: u64,
    max_stream_bytes: u64,
    control: &WorkspaceControl,
) -> Result<TemplateStateProof, WorkspaceError> {
    let connection = source.verified.connection();
    let row: (String, String, String, i64) = connection
        .query_row(
            "SELECT title, media_type, \
             CASE WHEN length(CAST(content AS BLOB)) BETWEEN 1 AND ?2 THEN content END, sequence \
             FROM capsule_doc WHERE slug = ?1 COLLATE BINARY",
            rusqlite::params![TEMPLATE_STATE_DOC_SLUG, HARD_MAX_PROOF_BYTES as i64],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(|_| invalid_contract())?;
    if row.0 != TEMPLATE_STATE_DOC_TITLE || row.1 != TEMPLATE_STATE_MEDIA_TYPE || row.3 != 0 {
        return Err(invalid_contract());
    }
    let bytes = row.2.as_bytes();
    let strict = crate::plan::strict_json(bytes).map_err(|_| invalid_contract())?;
    if crate::plan::canonical_json(&strict).map_err(|_| invalid_contract())? != bytes {
        return Err(invalid_contract());
    }
    let proof: TemplateStateProof =
        serde_json::from_value(strict).map_err(|_| invalid_contract())?;
    validate_proof_identity(source, &proof)?;

    let expected_ids: Vec<_> = source
        .data_contract()
        .datasets
        .iter()
        .map(|dataset| dataset.id.as_str())
        .collect();
    let proof_ids: Vec<_> = proof
        .datasets
        .iter()
        .map(|dataset| dataset.dataset_id.as_str())
        .collect();
    if proof.datasets.is_empty()
        || proof.datasets.len() > 256
        || expected_ids != proof_ids
        || proof_ids
            .windows(2)
            .any(|pair| pair[0].as_bytes() >= pair[1].as_bytes())
    {
        return Err(invalid_contract());
    }

    let mut rows_remaining = max_rows_total;
    let mut stream_bytes_remaining = max_stream_bytes;
    for (dataset, expected) in source.data_contract().datasets.iter().zip(&proof.datasets) {
        control.check()?;
        let (row_count, digest) = dataset_state_digest(
            source,
            dataset,
            &mut rows_remaining,
            max_rows_per_dataset,
            &mut stream_bytes_remaining,
            control,
        )?;
        if row_count != expected.stored_row_count
            || lower_hex(&digest) != expected.state_sha256
            || (expected.disposition == TemplateDatasetDisposition::Empty && row_count != 0)
        {
            return Err(invalid_contract());
        }
    }
    Ok(proof)
}

fn validate_proof_identity(
    source: &VerifiedWorkspaceSource,
    proof: &TemplateStateProof,
) -> Result<(), WorkspaceError> {
    let identity = source.identity();
    let schema = identity
        .overview
        .data_schema
        .as_ref()
        .ok_or_else(invalid_contract)?;
    if proof.profile != TEMPLATE_STATE_PROFILE
        || proof.dataset_state_profile != DATASET_STATE_PROFILE
        || proof.mutable_platform_state_profile != TEMPLATE_PLATFORM_RESET_PROFILE
        || proof.app_id != identity.app_id
        || proof.app_version != identity.app_version
        || proof.data_schema_id != schema.data_schema_id
        || proof.data_schema_version != u64::try_from(schema.data_schema_version).unwrap_or(0)
        || proof.app_id.len() > 512
        || proof.app_version.len() > 128
        || proof.data_schema_id.len() > 512
    {
        return Err(invalid_contract());
    }
    let mut ids = BTreeSet::new();
    for dataset in &proof.datasets {
        if dataset.dataset_id.is_empty()
            || dataset.dataset_id.len() > 256
            || !dataset.dataset_id.is_ascii()
            || !ids.insert(dataset.dataset_id.as_str())
            || !is_lower_sha256(&dataset.state_sha256)
            || dataset.stored_row_count > i64::MAX as u64
        {
            return Err(invalid_contract());
        }
    }
    Ok(())
}

fn dataset_state_digest(
    source: &VerifiedWorkspaceSource,
    dataset: &Dataset,
    rows_remaining: &mut u64,
    max_rows_per_dataset: u64,
    stream_bytes_remaining: &mut u64,
    control: &WorkspaceControl,
) -> Result<(u64, [u8; 32]), WorkspaceError> {
    let connection = source.verified.connection();
    let schema = source
        .identity()
        .overview
        .data_schema
        .as_ref()
        .ok_or_else(invalid_contract)?;
    let mut stream = DigestStream::new(*stream_bytes_remaining, control);
    stream.raw(DATASET_STREAM_CONTEXT)?;
    stream.text(&source.identity().app_id)?;
    stream.text(&schema.data_schema_id)?;
    stream.u64(u64::try_from(schema.data_schema_version).map_err(|_| invalid_contract())?)?;
    stream.text(&dataset.id)?;
    stream.u32(u32::try_from(dataset.tables.len()).map_err(|_| limit_exceeded())?)?;
    let mut total_rows = 0_u64;

    for table in &dataset.tables {
        control.check()?;
        let columns = table_columns(connection, &table.name, control)?;
        let dataset_remaining = max_rows_per_dataset
            .checked_sub(total_rows)
            .ok_or_else(limit_exceeded)?;
        let allowed = (*rows_remaining).min(dataset_remaining);
        let probe = allowed.checked_add(1).ok_or_else(limit_exceeded)?;
        let count_sql = format!(
            "SELECT count(*) FROM (SELECT 1 FROM {} LIMIT ?1)",
            quote_identifier(&table.name)
        );
        let table_rows: i64 = connection
            .query_row(
                &count_sql,
                [i64::try_from(probe).map_err(|_| limit_exceeded())?],
                |row| row.get(0),
            )
            .map_err(|_| invalid_contract())?;
        let table_rows = u64::try_from(table_rows).map_err(|_| invalid_contract())?;
        if table_rows > allowed {
            return Err(limit_exceeded());
        }
        *rows_remaining -= table_rows;
        total_rows = total_rows
            .checked_add(table_rows)
            .ok_or_else(limit_exceeded)?;

        stream.u32(u32::from(table.sequence))?;
        stream.text(&table.name)?;
        stream.u32(u32::try_from(table.primary_key.len()).map_err(|_| limit_exceeded())?)?;
        for key in &table.primary_key {
            stream.text(key)?;
        }
        stream.u32(u32::try_from(columns.len()).map_err(|_| limit_exceeded())?)?;
        for column in &columns {
            stream.text(column)?;
        }
        stream.u64(table_rows)?;

        let select_columns = columns
            .iter()
            .map(|column| quote_identifier(column))
            .collect::<Vec<_>>()
            .join(", ");
        let ordering = table
            .primary_key
            .iter()
            .map(|column| format!("{} COLLATE BINARY ASC", quote_identifier(column)))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT {select_columns} FROM {} ORDER BY {ordering}",
            quote_identifier(&table.name)
        );
        let mut statement = connection.prepare(&sql).map_err(|_| invalid_contract())?;
        let mut rows = statement.query([]).map_err(|_| invalid_contract())?;
        let mut observed = 0_u64;
        while let Some(row) = rows.next().map_err(|_| invalid_contract())? {
            control.check()?;
            observed = observed.checked_add(1).ok_or_else(limit_exceeded)?;
            for index in 0..columns.len() {
                stream.value(row.get_ref(index).map_err(|_| invalid_contract())?)?;
            }
        }
        if observed != table_rows {
            return Err(invalid_contract());
        }
    }
    let (digest, bytes) = stream.finish();
    *stream_bytes_remaining = stream_bytes_remaining
        .checked_sub(bytes)
        .ok_or_else(limit_exceeded)?;
    Ok((total_rows, digest))
}

pub(crate) fn dataset_state_with_budget(
    source: &VerifiedWorkspaceSource,
    dataset: &Dataset,
    rows_remaining: &mut u64,
    stream_bytes_remaining: &mut u64,
    deadline: std::time::Instant,
    cancellation: &CancellationToken,
) -> Result<(u64, String), WorkspaceError> {
    let duration = deadline.saturating_duration_since(std::time::Instant::now());
    if duration.is_zero() {
        return Err(limit_exceeded());
    }
    let control = WorkspaceControl::new(duration, cancellation);
    control.install(source.verified.connection())?;
    let max_rows_per_dataset = *rows_remaining;
    let (count, digest) = dataset_state_digest(
        source,
        dataset,
        rows_remaining,
        max_rows_per_dataset,
        stream_bytes_remaining,
        &control,
    )?;
    let _ = source
        .verified
        .connection()
        .progress_handler(0, None::<fn() -> bool>);
    Ok((count, lower_hex(&digest)))
}

#[cfg(test)]
pub(crate) fn dataset_state_for_test(
    source: &VerifiedWorkspaceSource,
    dataset: &Dataset,
) -> Result<(u64, String), WorkspaceError> {
    let (count, _bytes, digest) = dataset_state_vector_for_test(source, dataset)?;
    Ok((count, digest))
}

#[cfg(test)]
pub(crate) fn dataset_state_vector_for_test(
    source: &VerifiedWorkspaceSource,
    dataset: &Dataset,
) -> Result<(u64, u64, String), WorkspaceError> {
    let cancellation = CancellationToken::new();
    let control = WorkspaceControl::new(HARD_MAX_DEADLINE, &cancellation);
    control.install(source.verified.connection())?;
    let mut rows = HARD_MAX_ROWS;
    let mut bytes = HARD_MAX_STREAM_BYTES;
    let (count, digest) = dataset_state_digest(
        source,
        dataset,
        &mut rows,
        HARD_MAX_ROWS,
        &mut bytes,
        &control,
    )?;
    let _ = source
        .verified
        .connection()
        .progress_handler(0, None::<fn() -> bool>);
    Ok((count, HARD_MAX_STREAM_BYTES - bytes, lower_hex(&digest)))
}

fn table_columns(
    connection: &rusqlite::Connection,
    table: &str,
    control: &WorkspaceControl,
) -> Result<Vec<String>, WorkspaceError> {
    let mut statement = connection
        .prepare(
            "SELECT CASE WHEN length(CAST(name AS BLOB)) BETWEEN 1 AND 256 THEN name END \
             FROM pragma_table_xinfo(?1) ORDER BY cid LIMIT 257",
        )
        .map_err(|_| invalid_contract())?;
    let mut rows = statement.query([table]).map_err(|_| invalid_contract())?;
    let mut columns = Vec::new();
    while let Some(row) = rows.next().map_err(|_| invalid_contract())? {
        control.check()?;
        if columns.len() == 256 {
            return Err(limit_exceeded());
        }
        let name: Option<String> = row.get(0).map_err(|_| invalid_contract())?;
        columns.push(name.ok_or_else(invalid_contract)?);
    }
    if columns.is_empty() {
        return Err(invalid_contract());
    }
    Ok(columns)
}

struct DigestStream<'a> {
    digest: Sha256,
    bytes: u64,
    maximum: u64,
    control: &'a WorkspaceControl,
}

impl<'a> DigestStream<'a> {
    fn new(maximum: u64, control: &'a WorkspaceControl) -> Self {
        Self {
            digest: Sha256::new(),
            bytes: 0,
            maximum,
            control,
        }
    }

    fn raw(&mut self, bytes: &[u8]) -> Result<(), WorkspaceError> {
        self.control.check()?;
        self.bytes = self
            .bytes
            .checked_add(bytes.len() as u64)
            .ok_or_else(limit_exceeded)?;
        if self.bytes > self.maximum {
            return Err(limit_exceeded());
        }
        self.digest.update(bytes);
        Ok(())
    }

    fn u32(&mut self, value: u32) -> Result<(), WorkspaceError> {
        self.raw(&value.to_be_bytes())
    }

    fn u64(&mut self, value: u64) -> Result<(), WorkspaceError> {
        self.raw(&value.to_be_bytes())
    }

    fn text(&mut self, value: &str) -> Result<(), WorkspaceError> {
        self.u64(value.len() as u64)?;
        self.raw(value.as_bytes())
    }

    fn value(&mut self, value: ValueRef<'_>) -> Result<(), WorkspaceError> {
        match value {
            ValueRef::Null => self.raw(&[0]),
            ValueRef::Integer(value) => {
                self.raw(&[1])?;
                self.raw(&value.to_be_bytes())
            }
            ValueRef::Real(value) => {
                if !value.is_finite() {
                    return Err(invalid_contract());
                }
                self.raw(&[2])?;
                self.raw(&value.to_bits().to_be_bytes())
            }
            ValueRef::Text(value) => {
                std::str::from_utf8(value).map_err(|_| invalid_contract())?;
                self.raw(&[3])?;
                self.u64(value.len() as u64)?;
                self.raw(value)
            }
            ValueRef::Blob(value) => {
                self.raw(&[4])?;
                self.u64(value.len() as u64)?;
                self.raw(value)
            }
        }
    }

    fn finish(self) -> ([u8; 32], u64) {
        (self.digest.finalize().into(), self.bytes)
    }
}

fn assert_current(
    source: &VerifiedWorkspaceSource,
    control: &WorkspaceControl,
    cancellation: &CancellationToken,
) -> Result<(), WorkspaceError> {
    let limits = WorkspaceLimits {
        deadline: control.remaining()?,
        ..WorkspaceLimits::default()
    };
    source.assert_current_with_control(&limits, cancellation)
}

fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn is_lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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

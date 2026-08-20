//! Bounded, execution-free comparison over two retained signed-v0.3 sources.
//!
//! The summary surface deliberately contains no row values. Both SQLite
//! connections are the private read-only snapshots retained by
//! [`VerifiedWorkspaceSource`]; callers cannot supply SQL or table names.

use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

use rusqlite::{Connection, params, types::ValueRef};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::{
    CancellationToken, ComparePolicy, Dataset, DatasetTable, Sensitivity, VerifiedWorkspaceSource,
    WorkspaceControl, WorkspaceError, WorkspaceErrorCode,
};

pub const COMPARE_SUMMARY_PROFILE: &str = "org.sqlite-capsule.compare-report/1";
pub const COMPARE_KEY_PROFILE: &str = "org.sqlite-capsule.compare-key/1";
pub const COMPARE_ROW_PROFILE: &str = "org.sqlite-capsule.compare-row/1";

const HARD_DEADLINE: Duration = Duration::from_secs(30);
const HARD_ROWS_PER_TABLE: u64 = 100_000;
const HARD_TOTAL_ROWS: u64 = 100_000;
const HARD_VALUE_BYTES: u64 = 1024 * 1024;
const HARD_STREAM_BYTES: u64 = 256 * 1024 * 1024;
const HARD_MIGRATION_EDGES: usize = 256;
const MAX_COMMON_ANCESTOR_CLAIMS: usize = 32;
const HARD_MAX_COMPARE_REPORT_BYTES: usize = 8 * 1024 * 1024;

#[cfg(test)]
thread_local! {
    static TEST_ROW_CANCELLATION: std::cell::RefCell<Option<(usize, CancellationToken)>> =
        const { std::cell::RefCell::new(None) };
}

#[derive(Clone, Debug)]
pub struct CompareLimits {
    /// Stable reviewed deadline budget serialized into the report.
    pub deadline: Duration,
    /// Optional remaining time from an enclosing public-request deadline.
    /// This affects execution only and is never serialized or hashed.
    pub operation_deadline: Option<Duration>,
    pub max_rows_per_table: u64,
    pub max_total_rows: u64,
    pub max_value_bytes: u64,
    pub max_stream_bytes: u64,
}

impl Default for CompareLimits {
    fn default() -> Self {
        Self {
            deadline: HARD_DEADLINE,
            operation_deadline: None,
            max_rows_per_table: 100_000,
            max_total_rows: HARD_TOTAL_ROWS,
            max_value_bytes: 64 * 1024,
            max_stream_bytes: 64 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CompareLimitsApplied {
    pub max_rows_per_table: u64,
    pub max_total_rows: u64,
    pub max_value_bytes: u64,
    pub max_stream_bytes: u64,
    pub deadline_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompatibilityState {
    SameReleaseSameSchema,
    SameAppSameSchema,
    SameAppMigrationAvailable,
    SameAppIncompatibleSchema,
    DifferentApplication,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CompareCompatibility {
    pub state: CompatibilityState,
    pub can_compare_data: bool,
    pub can_reconcile: bool,
    pub reasons: Vec<&'static str>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CompareInputRef {
    pub file_sha256: String,
    pub capsule_id: String,
    pub revision_id: String,
    pub app_id: String,
    pub app_version: String,
    pub application_digest: String,
    pub publisher: ComparePublisherEvidence,
    pub data_schema_id: String,
    pub data_schema_version: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ComparePublisherEvidence {
    pub publisher_id: String,
    pub publisher_name: String,
    pub signature_count: u32,
    pub signatures: Vec<CompareSignatureEvidence>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CompareSignatureEvidence {
    pub key_id: String,
    pub status: CompareSignatureStatus,
    pub signed_at: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompareSignatureStatus {
    Valid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompareSectionState {
    Same,
    Different,
    Unavailable,
    Invalid,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CompareSection {
    pub state: CompareSectionState,
    pub left_digest: String,
    pub right_digest: String,
    pub change_count: u64,
}

/// Bounded comparison of mutable, unauthenticated lineage claims.
///
/// A direct relationship means only that a mutable parent claim names the
/// exact SHA-256 of the other retained source bytes. It does not authenticate
/// the lineage event, its operation, or its publisher provenance.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CompareLineageSection {
    pub state: CompareSectionState,
    pub left_digest: String,
    pub right_digest: String,
    pub change_count: u64,
    pub direct_relationship: CompareDirectRelationship,
    pub direct_evidence: Vec<CompareDirectLineageEvidence>,
    pub common_ancestor_claims: Vec<CompareCommonAncestorClaim>,
    pub common_ancestor_claims_truncated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompareDirectRelationship {
    None,
    LeftIsParentOfRight,
    RightIsParentOfLeft,
    MutualParentClaims,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompareSide {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompareDirectEvidenceVerification {
    ClaimedParentFileDigestMatchesOtherRetainedSourceBytes,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CompareDirectLineageEvidence {
    pub child_side: CompareSide,
    pub relation: crate::ParentRelation,
    pub parent_file_sha256: String,
    pub verification: CompareDirectEvidenceVerification,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompareAncestorClaimedBy {
    Left,
    Right,
    Both,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompareAncestorClaimVerification {
    MutableUntrustedClaimOnly,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CompareCommonAncestorClaim {
    pub file_sha256: String,
    pub claimed_by: CompareAncestorClaimedBy,
    pub verification: CompareAncestorClaimVerification,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompareDataState {
    Same,
    Different,
    Ignored,
    Unavailable,
    Truncated,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CompareCounts {
    pub added: Option<u64>,
    pub removed: Option<u64>,
    pub changed: Option<u64>,
    pub unchanged: Option<u64>,
}

impl CompareCounts {
    const fn zero() -> Self {
        Self {
            added: Some(0),
            removed: Some(0),
            changed: Some(0),
            unchanged: Some(0),
        }
    }

    const fn unavailable() -> Self {
        Self {
            added: None,
            removed: None,
            changed: None,
            unchanged: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CompareTableSummary {
    pub table: String,
    pub primary_key: Vec<String>,
    pub state: CompareDataState,
    pub left_rows: u64,
    pub right_rows: u64,
    pub counts: CompareCounts,
    pub left_digest: Option<String>,
    pub right_digest: Option<String>,
    pub details_redacted: bool,
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CompareDatasetSummary {
    pub dataset_id: String,
    pub policy: ComparePolicy,
    pub sensitivity: Sensitivity,
    pub state: CompareDataState,
    pub left_rows: u64,
    pub right_rows: u64,
    pub counts: CompareCounts,
    pub left_digest: Option<String>,
    pub right_digest: Option<String>,
    pub tables: Vec<CompareTableSummary>,
    pub details_redacted: bool,
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CompareSummary {
    pub profile: &'static str,
    pub left: CompareInputRef,
    pub right: CompareInputRef,
    pub compatibility: CompareCompatibility,
    pub identity: CompareSection,
    pub lineage: CompareLineageSection,
    pub application: CompareSection,
    pub schema: CompareSection,
    pub datasets: Vec<CompareDatasetSummary>,
    pub limits: CompareLimitsApplied,
    pub truncated: bool,
    pub report_digest: String,
}

/// Owned SQLite value used by the normative compare-key/row canonicalizer.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum CompareValue {
    Null,
    Integer(i64),
    Real(f64),
    Text(Vec<u8>),
    Blob(Vec<u8>),
}

pub(crate) fn canonical_compare_key(
    table: &str,
    key: &[(String, CompareValue)],
    max_value_bytes: u64,
) -> Result<Vec<u8>, WorkspaceError> {
    canonical_frame(COMPARE_KEY_PROFILE, table, key, &[], max_value_bytes)
}

pub(crate) fn canonical_compare_row(
    table: &str,
    key: &[(String, CompareValue)],
    compared: &[(String, CompareValue)],
    max_value_bytes: u64,
) -> Result<Vec<u8>, WorkspaceError> {
    canonical_frame(COMPARE_ROW_PROFILE, table, key, compared, max_value_bytes)
}

fn canonical_frame(
    profile: &str,
    table: &str,
    key: &[(String, CompareValue)],
    compared: &[(String, CompareValue)],
    max_value_bytes: u64,
) -> Result<Vec<u8>, WorkspaceError> {
    if key.is_empty() || key.len() > 16 || compared.len() > 256 {
        return Err(invalid_contract());
    }
    let mut out = Vec::new();
    frame_text(&mut out, profile)?;
    frame_text(&mut out, table)?;
    frame_u32(&mut out, key.len())?;
    for (name, value) in key {
        frame_text(&mut out, name)?;
        frame_value(&mut out, value, max_value_bytes)?;
    }
    if profile == COMPARE_ROW_PROFILE {
        frame_u32(&mut out, compared.len())?;
        for (name, value) in compared {
            frame_text(&mut out, name)?;
            frame_value(&mut out, value, max_value_bytes)?;
        }
    }
    Ok(out)
}

fn frame_u32(out: &mut Vec<u8>, value: usize) -> Result<(), WorkspaceError> {
    out.extend_from_slice(
        &u32::try_from(value)
            .map_err(|_| limit_exceeded())?
            .to_be_bytes(),
    );
    Ok(())
}

fn frame_text(out: &mut Vec<u8>, value: &str) -> Result<(), WorkspaceError> {
    out.extend_from_slice(
        &u64::try_from(value.len())
            .map_err(|_| limit_exceeded())?
            .to_be_bytes(),
    );
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

fn frame_value(
    out: &mut Vec<u8>,
    value: &CompareValue,
    max_value_bytes: u64,
) -> Result<(), WorkspaceError> {
    match value {
        CompareValue::Null => out.push(0),
        CompareValue::Integer(value) => {
            out.push(1);
            out.extend_from_slice(&value.to_be_bytes());
        }
        CompareValue::Real(value) => {
            if !value.is_finite() {
                return Err(invalid_contract());
            }
            out.push(2);
            out.extend_from_slice(&value.to_bits().to_be_bytes());
        }
        CompareValue::Text(value) => {
            std::str::from_utf8(value).map_err(|_| invalid_contract())?;
            frame_bytes(out, 3, value, max_value_bytes)?;
        }
        CompareValue::Blob(value) => frame_bytes(out, 4, value, max_value_bytes)?,
    }
    Ok(())
}

pub(crate) fn canonical_value_bytes(
    value: &CompareValue,
    max_value_bytes: u64,
) -> Result<Vec<u8>, WorkspaceError> {
    let mut bytes = Vec::new();
    frame_value(&mut bytes, value, max_value_bytes)?;
    Ok(bytes)
}

fn frame_bytes(
    out: &mut Vec<u8>,
    tag: u8,
    value: &[u8],
    max_value_bytes: u64,
) -> Result<(), WorkspaceError> {
    let length = u64::try_from(value.len()).map_err(|_| limit_exceeded())?;
    if length > max_value_bytes {
        return Err(limit_exceeded());
    }
    out.push(tag);
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(value);
    Ok(())
}

pub fn compare_sources(
    left: &VerifiedWorkspaceSource,
    right: &VerifiedWorkspaceSource,
    requested: &CompareLimits,
    cancellation: &CancellationToken,
) -> Result<CompareSummary, WorkspaceError> {
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
    let result = compare_inner(left, right, &limits, &control);
    let _ = left
        .verified
        .connection()
        .progress_handler(0, None::<fn() -> bool>);
    let _ = right
        .verified
        .connection()
        .progress_handler(0, None::<fn() -> bool>);
    let mut summary = match result {
        Ok(summary) => summary,
        Err(error) => {
            control.check()?;
            return Err(error);
        }
    };
    control.check()?;
    let rebind_limits = crate::WorkspaceLimits {
        deadline: control.remaining()?,
        ..crate::WorkspaceLimits::default()
    };
    left.assert_current_with_control(&rebind_limits, cancellation)?;
    let rebind_limits = crate::WorkspaceLimits {
        deadline: control.remaining()?,
        ..crate::WorkspaceLimits::default()
    };
    right.assert_current_with_control(&rebind_limits, cancellation)?;
    summary.report_digest = report_digest(&summary)?;
    control.check()?;
    Ok(summary)
}

fn compare_inner(
    left: &VerifiedWorkspaceSource,
    right: &VerifiedWorkspaceSource,
    limits: &CompareLimitsApplied,
    control: &WorkspaceControl,
) -> Result<CompareSummary, WorkspaceError> {
    let left_ref = input_ref(left)?;
    let right_ref = input_ref(right)?;
    let compatibility = classify(left, right, &left_ref, &right_ref, control)?;
    let identity = section(
        &json!({
            "file_sha256": left_ref.file_sha256,
            "capsule_id": left_ref.capsule_id,
            "revision_id": left_ref.revision_id,
        }),
        &json!({
            "file_sha256": right_ref.file_sha256,
            "capsule_id": right_ref.capsule_id,
            "revision_id": right_ref.revision_id,
        }),
    )?;
    let lineage = compare_lineage(left, right, &left_ref, &right_ref, control)?;
    let application = section(
        &json!({
            "app_id": left_ref.app_id,
            "app_version": left_ref.app_version,
            "application_digest": left_ref.application_digest,
            "publisher": left_ref.publisher,
        }),
        &json!({
            "app_id": right_ref.app_id,
            "app_version": right_ref.app_version,
            "application_digest": right_ref.application_digest,
            "publisher": right_ref.publisher,
        }),
    )?;
    let left_schema = schema_digest(left.verified.connection(), left.data_contract(), control)?;
    let right_schema = schema_digest(right.verified.connection(), right.data_contract(), control)?;
    let schema = digest_section(&left_schema, &right_schema);
    let mut datasets = Vec::new();
    let mut truncated = false;
    if compatibility.can_compare_data {
        let mut total_rows = 0_u64;
        let mut stream_bytes = 0_u64;
        for (left_dataset, right_dataset) in left
            .data_contract()
            .datasets
            .iter()
            .zip(&right.data_contract().datasets)
        {
            control.check()?;
            if left_dataset.id != right_dataset.id
                || left_dataset.compare != right_dataset.compare
                || left_dataset.sensitivity != right_dataset.sensitivity
                || left_dataset.tables != right_dataset.tables
            {
                return Err(invalid_contract());
            }
            let summary = compare_dataset(
                left.verified.connection(),
                right.verified.connection(),
                left_dataset,
                limits,
                &mut total_rows,
                &mut stream_bytes,
                control,
            )?;
            truncated |= summary.truncated;
            datasets.push(summary);
        }
    } else if left_ref.app_id == right_ref.app_id
        && left.data_contract().data_schema_id == right.data_contract().data_schema_id
        && left.data_contract().data_schema_version == right.data_contract().data_schema_version
    {
        datasets = unavailable_datasets(left, right, control)?;
    }
    Ok(CompareSummary {
        profile: COMPARE_SUMMARY_PROFILE,
        left: left_ref,
        right: right_ref,
        compatibility,
        identity,
        lineage,
        application,
        schema,
        datasets,
        limits: limits.clone(),
        truncated,
        report_digest: String::new(),
    })
}

fn compare_lineage(
    left: &VerifiedWorkspaceSource,
    right: &VerifiedWorkspaceSource,
    left_ref: &CompareInputRef,
    right_ref: &CompareInputRef,
    control: &WorkspaceControl,
) -> Result<CompareLineageSection, WorkspaceError> {
    let CompareSection {
        state,
        left_digest,
        right_digest,
        change_count,
    } = section(left.lineage(), right.lineage())?;

    let mut direct_evidence = Vec::new();
    collect_direct_evidence(
        left,
        left_ref,
        &right_ref.file_sha256,
        CompareSide::Left,
        control,
        &mut direct_evidence,
    )?;
    collect_direct_evidence(
        right,
        right_ref,
        &left_ref.file_sha256,
        CompareSide::Right,
        control,
        &mut direct_evidence,
    )?;
    direct_evidence.sort_by(|left, right| {
        left.child_side
            .cmp(&right.child_side)
            .then_with(|| relation_rank(left.relation).cmp(&relation_rank(right.relation)))
            .then_with(|| left.parent_file_sha256.cmp(&right.parent_file_sha256))
    });
    // Retain one deterministic mutable claim per side. Duplicate events and
    // relation labels cannot grow the bounded public evidence projection or
    // imply stronger provenance.
    let mut retained_left = false;
    let mut retained_right = false;
    direct_evidence.retain(|item| match item.child_side {
        CompareSide::Left if !retained_left => {
            retained_left = true;
            true
        }
        CompareSide::Right if !retained_right => {
            retained_right = true;
            true
        }
        CompareSide::Left | CompareSide::Right => false,
    });

    let right_is_parent_of_left = direct_evidence
        .iter()
        .any(|evidence| evidence.child_side == CompareSide::Left);
    let left_is_parent_of_right = direct_evidence
        .iter()
        .any(|evidence| evidence.child_side == CompareSide::Right);
    let direct_relationship = match (left_is_parent_of_right, right_is_parent_of_left) {
        (false, false) => CompareDirectRelationship::None,
        (true, false) => CompareDirectRelationship::LeftIsParentOfRight,
        (false, true) => CompareDirectRelationship::RightIsParentOfLeft,
        (true, true) => CompareDirectRelationship::MutualParentClaims,
    };

    let left_claims = lineage_parent_digests(left, control)?;
    let right_claims = lineage_parent_digests(right, control)?;
    let mut common_ancestor_claims = Vec::new();
    let mut common_ancestor_claims_truncated = false;
    for file_sha256 in left_claims.intersection(&right_claims) {
        control.check()?;
        // A retained input recognized through direct evidence is not a third
        // common-ancestor claim. It is reported only in direct_evidence.
        if file_sha256 == &left_ref.file_sha256 || file_sha256 == &right_ref.file_sha256 {
            continue;
        }
        if common_ancestor_claims.len() == MAX_COMMON_ANCESTOR_CLAIMS {
            common_ancestor_claims_truncated = true;
            break;
        }
        common_ancestor_claims.push(CompareCommonAncestorClaim {
            file_sha256: file_sha256.clone(),
            claimed_by: CompareAncestorClaimedBy::Both,
            verification: CompareAncestorClaimVerification::MutableUntrustedClaimOnly,
        });
    }

    Ok(CompareLineageSection {
        state,
        left_digest,
        right_digest,
        change_count,
        direct_relationship,
        direct_evidence,
        common_ancestor_claims,
        common_ancestor_claims_truncated,
    })
}

fn collect_direct_evidence(
    child: &VerifiedWorkspaceSource,
    child_ref: &CompareInputRef,
    other_file_sha256: &str,
    child_side: CompareSide,
    control: &WorkspaceControl,
    evidence: &mut Vec<CompareDirectLineageEvidence>,
) -> Result<(), WorkspaceError> {
    for event in &child.lineage().events {
        control.check()?;
        // Mutable history rows are not assumed to describe the current
        // revision. Only an event claiming the retained child's exact IDs is
        // eligible for direct relationship recognition.
        if event.result_capsule_id != child_ref.capsule_id
            || event.result_revision_id != child_ref.revision_id
        {
            continue;
        }
        for parent in &event.parents {
            control.check()?;
            if parent.file_sha256 != other_file_sha256 {
                continue;
            }
            let item = CompareDirectLineageEvidence {
                child_side,
                relation: parent.relation,
                parent_file_sha256: parent.file_sha256.clone(),
                verification:
                    CompareDirectEvidenceVerification::ClaimedParentFileDigestMatchesOtherRetainedSourceBytes,
            };
            if !evidence.contains(&item) {
                evidence.push(item);
            }
        }
    }
    Ok(())
}

fn lineage_parent_digests(
    source: &VerifiedWorkspaceSource,
    control: &WorkspaceControl,
) -> Result<BTreeSet<String>, WorkspaceError> {
    let mut digests = BTreeSet::new();
    for event in &source.lineage().events {
        control.check()?;
        for parent in &event.parents {
            control.check()?;
            digests.insert(parent.file_sha256.clone());
        }
    }
    Ok(digests)
}

const fn relation_rank(relation: crate::ParentRelation) -> u8 {
    match relation {
        crate::ParentRelation::CreatedFrom => 0,
        crate::ParentRelation::ForkedFrom => 1,
        crate::ParentRelation::TargetDerivedFrom => 2,
        crate::ParentRelation::ChangesAppliedFrom => 3,
        crate::ParentRelation::UpgradedFrom => 4,
        crate::ParentRelation::ApplicationRelease => 5,
    }
}

fn effective_limits(
    requested: &CompareLimits,
) -> Result<(CompareLimitsApplied, Duration), WorkspaceError> {
    let deadline = requested.deadline.min(HARD_DEADLINE);
    let operation_deadline = requested
        .operation_deadline
        .unwrap_or(deadline)
        .min(deadline)
        .min(HARD_DEADLINE);
    let applied = CompareLimitsApplied {
        max_rows_per_table: requested.max_rows_per_table.min(HARD_ROWS_PER_TABLE),
        max_total_rows: requested.max_total_rows.min(HARD_TOTAL_ROWS),
        max_value_bytes: requested.max_value_bytes.min(HARD_VALUE_BYTES),
        max_stream_bytes: requested.max_stream_bytes.min(HARD_STREAM_BYTES),
        deadline_ms: u64::try_from(deadline.as_millis()).map_err(|_| limit_exceeded())?,
    };
    if applied.deadline_ms == 0
        || operation_deadline.is_zero()
        || applied.max_rows_per_table == 0
        || applied.max_total_rows == 0
        || applied.max_value_bytes == 0
        || applied.max_stream_bytes == 0
    {
        return Err(limit_exceeded());
    }
    Ok((applied, operation_deadline))
}

fn input_ref(source: &VerifiedWorkspaceSource) -> Result<CompareInputRef, WorkspaceError> {
    let identity = source.identity();
    let schema = identity
        .overview
        .data_schema
        .as_ref()
        .ok_or_else(invalid_contract)?;
    let publisher = sqlite_capsule_crypto::publisher_identity(source.verified.connection())
        .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InvalidSignature))?;
    if source.signature_reports().is_empty() || source.signature_reports().len() > 32 {
        return Err(WorkspaceError::new(WorkspaceErrorCode::InvalidSignature));
    }
    let mut signatures = source
        .signature_reports()
        .iter()
        .map(|report| {
            if !report.cryptographically_valid || !report.digest_matches {
                return Err(WorkspaceError::new(WorkspaceErrorCode::InvalidSignature));
            }
            Ok(CompareSignatureEvidence {
                key_id: report.key_id.clone(),
                status: CompareSignatureStatus::Valid,
                signed_at: report.signed_at.clone(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    signatures.sort_by(|left, right| {
        left.key_id
            .cmp(&right.key_id)
            .then(left.signed_at.cmp(&right.signed_at))
    });
    Ok(CompareInputRef {
        file_sha256: source.source_sha256(),
        capsule_id: identity.capsule_id.clone(),
        revision_id: identity
            .overview
            .instance
            .revision_id
            .clone()
            .ok_or_else(invalid_contract)?,
        app_id: identity.app_id.clone(),
        app_version: identity.app_version.clone(),
        application_digest: lower_hex(source.application_digest()),
        publisher: ComparePublisherEvidence {
            publisher_id: publisher.publisher_id,
            publisher_name: publisher.publisher_name,
            signature_count: u32::try_from(signatures.len()).map_err(|_| limit_exceeded())?,
            signatures,
        },
        data_schema_id: schema.data_schema_id.clone(),
        data_schema_version: schema.data_schema_version,
    })
}

fn classify(
    left: &VerifiedWorkspaceSource,
    right: &VerifiedWorkspaceSource,
    l: &CompareInputRef,
    r: &CompareInputRef,
    control: &WorkspaceControl,
) -> Result<CompareCompatibility, WorkspaceError> {
    let (state, can_compare_data, reason) = if l.app_id != r.app_id {
        (
            CompatibilityState::DifferentApplication,
            false,
            "Application identities differ.",
        )
    } else if l.data_schema_id == r.data_schema_id
        && l.data_schema_version == r.data_schema_version
        && left.data_contract() != right.data_contract()
    {
        (
            CompatibilityState::SameAppSameSchema,
            false,
            "The schema identity matches but the signed dataset contracts differ; data comparison is unavailable.",
        )
    } else if l.data_schema_id == r.data_schema_id && l.data_schema_version == r.data_schema_version
    {
        if l.application_digest == r.application_digest {
            (
                CompatibilityState::SameReleaseSameSchema,
                true,
                "Exact application release and data schema match.",
            )
        } else {
            (
                CompatibilityState::SameAppSameSchema,
                true,
                "Application identity and data schema match; release digests differ.",
            )
        }
    } else if l.data_schema_id == r.data_schema_id
        && migration_available(
            left,
            right,
            l.data_schema_version,
            r.data_schema_version,
            control,
        )?
    {
        (
            CompatibilityState::SameAppMigrationAvailable,
            false,
            "A unique signed migration path exists; normalization is not enabled in this compare profile.",
        )
    } else {
        (
            CompatibilityState::SameAppIncompatibleSchema,
            false,
            "No unique signed migration path makes the schemas directly comparable.",
        )
    };
    let can_reconcile = can_compare_data
        && left.data_contract().datasets.iter().any(|dataset| {
            matches!(dataset.compare, ComparePolicy::Row | ComparePolicy::Field)
                && matches!(
                    dataset.reconcile,
                    crate::ReconcilePolicy::Manual | crate::ReconcilePolicy::ThreeWay
                )
        });
    Ok(CompareCompatibility {
        state,
        can_compare_data,
        can_reconcile,
        reasons: vec![reason],
    })
}

fn migration_available(
    left: &VerifiedWorkspaceSource,
    right: &VerifiedWorkspaceSource,
    left_version: i64,
    right_version: i64,
    control: &WorkspaceControl,
) -> Result<bool, WorkspaceError> {
    let schema_id = &left.data_contract().data_schema_id;
    let forward = path_count(
        right.verified.connection(),
        schema_id,
        left_version,
        right_version,
        control,
    )?;
    let reverse = path_count(
        left.verified.connection(),
        schema_id,
        right_version,
        left_version,
        control,
    )?;
    Ok((forward == 1 && reverse == 0) || (forward == 0 && reverse == 1))
}

fn path_count(
    connection: &Connection,
    schema_id: &str,
    from: i64,
    to: i64,
    control: &WorkspaceControl,
) -> Result<u8, WorkspaceError> {
    let mut statement = connection
        .prepare(
            "SELECT from_version, to_version FROM capsule_migration WHERE data_schema_id=?1 \
         ORDER BY from_version, to_version, id LIMIT ?2",
        )
        .map_err(|_| invalid_contract())?;
    let mut rows = statement
        .query(params![
            schema_id,
            i64::try_from(HARD_MIGRATION_EDGES + 1).unwrap()
        ])
        .map_err(|_| invalid_contract())?;
    let mut graph: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
    let mut count = 0_usize;
    while let Some(row) = rows.next().map_err(|_| invalid_contract())? {
        control.check()?;
        if count == HARD_MIGRATION_EDGES {
            return Err(limit_exceeded());
        }
        count += 1;
        graph
            .entry(row.get(0).map_err(|_| invalid_contract())?)
            .or_default()
            .push(row.get(1).map_err(|_| invalid_contract())?);
    }
    validate_acyclic(&graph)?;
    let mut memo = BTreeMap::new();
    count_paths(&graph, from, to, &mut Vec::new(), &mut memo)
}

fn validate_acyclic(graph: &BTreeMap<i64, Vec<i64>>) -> Result<(), WorkspaceError> {
    let mut complete = std::collections::BTreeSet::new();
    let mut stack = Vec::new();
    for node in graph.keys().chain(graph.values().flatten()) {
        visit_migration_node(graph, *node, &mut stack, &mut complete)?;
    }
    Ok(())
}

fn visit_migration_node(
    graph: &BTreeMap<i64, Vec<i64>>,
    node: i64,
    stack: &mut Vec<i64>,
    complete: &mut std::collections::BTreeSet<i64>,
) -> Result<(), WorkspaceError> {
    if complete.contains(&node) {
        return Ok(());
    }
    if stack.contains(&node) {
        return Err(invalid_contract());
    }
    stack.push(node);
    for next in graph.get(&node).into_iter().flatten() {
        visit_migration_node(graph, *next, stack, complete)?;
    }
    stack.pop();
    complete.insert(node);
    Ok(())
}

fn count_paths(
    graph: &BTreeMap<i64, Vec<i64>>,
    current: i64,
    target: i64,
    stack: &mut Vec<i64>,
    memo: &mut BTreeMap<i64, u8>,
) -> Result<u8, WorkspaceError> {
    if current == target {
        return Ok(1);
    }
    if stack.contains(&current) {
        return Err(invalid_contract());
    }
    if let Some(value) = memo.get(&current) {
        return Ok(*value);
    }
    stack.push(current);
    let mut total = 0_u8;
    for next in graph.get(&current).into_iter().flatten() {
        total = total
            .saturating_add(count_paths(graph, *next, target, stack, memo)?)
            .min(2);
    }
    stack.pop();
    memo.insert(current, total);
    Ok(total)
}

fn section<T: Serialize>(left: &T, right: &T) -> Result<CompareSection, WorkspaceError> {
    let left = canonical_digest(left)?;
    let right = canonical_digest(right)?;
    Ok(digest_section(&left, &right))
}

fn digest_section(left: &str, right: &str) -> CompareSection {
    let same = left == right;
    CompareSection {
        state: if same {
            CompareSectionState::Same
        } else {
            CompareSectionState::Different
        },
        left_digest: left.to_owned(),
        right_digest: right.to_owned(),
        change_count: u64::from(!same),
    }
}

fn schema_digest(
    connection: &Connection,
    contract: &crate::DataContract,
    control: &WorkspaceControl,
) -> Result<String, WorkspaceError> {
    let mut objects = Vec::new();
    for dataset in &contract.datasets {
        for table in &dataset.tables {
            control.check()?;
            let schema = query_metadata(
                connection,
                "SELECT type, name, tbl_name, sql FROM sqlite_schema \
                 WHERE name=?1 COLLATE BINARY OR tbl_name=?1 COLLATE BINARY \
                 ORDER BY type COLLATE BINARY, name COLLATE BINARY",
                &table.name,
                control,
            )?;
            let columns = query_metadata(
                connection,
                "SELECT cid, name, type, \"notnull\", dflt_value, pk, hidden \
                 FROM pragma_table_xinfo(?1) ORDER BY cid",
                &table.name,
                control,
            )?;
            let foreign_keys = query_metadata(
                connection,
                "SELECT id, seq, `table`, `from`, `to`, on_update, on_delete, match \
                 FROM pragma_foreign_key_list(?1) ORDER BY id, seq",
                &table.name,
                control,
            )?;
            let indexes = query_metadata(
                connection,
                "SELECT seq, name, `unique`, origin, partial FROM pragma_index_list(?1) \
                 ORDER BY seq",
                &table.name,
                control,
            )?;
            let mut index_columns = Vec::new();
            for index in &indexes {
                let Some(index_name) = index.get(1).and_then(serde_json::Value::as_str) else {
                    return Err(invalid_contract());
                };
                index_columns.push(json!({
                    "name": index_name,
                    "columns": query_metadata(
                        connection,
                        "SELECT seqno, cid, name, desc, coll, key FROM pragma_index_xinfo(?1) \
                         ORDER BY seqno",
                        index_name,
                        control,
                    )?,
                }));
            }
            objects.push(json!({
                "dataset": dataset.id,
                "declaration": table,
                "sqlite_schema": schema,
                "columns": columns,
                "foreign_keys": foreign_keys,
                "indexes": indexes,
                "index_columns": index_columns,
            }));
        }
    }
    canonical_digest(&json!({"contract": contract, "objects": objects}))
}

fn query_metadata(
    connection: &Connection,
    sql: &str,
    parameter: &str,
    control: &WorkspaceControl,
) -> Result<Vec<Vec<serde_json::Value>>, WorkspaceError> {
    const MAX_ROWS: usize = 4096;
    const MAX_TEXT_BYTES: usize = 1024 * 1024;
    let mut statement = connection.prepare(sql).map_err(|_| invalid_contract())?;
    let columns = statement.column_count();
    let mut rows = statement
        .query([parameter])
        .map_err(|_| invalid_contract())?;
    let mut result = Vec::new();
    while let Some(row) = rows.next().map_err(|_| invalid_contract())? {
        control.check()?;
        if result.len() == MAX_ROWS {
            return Err(limit_exceeded());
        }
        let mut values = Vec::with_capacity(columns);
        for index in 0..columns {
            values.push(match row.get_ref(index).map_err(|_| invalid_contract())? {
                ValueRef::Null => serde_json::Value::Null,
                ValueRef::Integer(value) => json!(value),
                ValueRef::Real(_) | ValueRef::Blob(_) => return Err(invalid_contract()),
                ValueRef::Text(value) => {
                    if value.len() > MAX_TEXT_BYTES {
                        return Err(limit_exceeded());
                    }
                    json!(std::str::from_utf8(value).map_err(|_| invalid_contract())?)
                }
            });
        }
        result.push(values);
    }
    Ok(result)
}

fn unavailable_datasets(
    left: &VerifiedWorkspaceSource,
    right: &VerifiedWorkspaceSource,
    control: &WorkspaceControl,
) -> Result<Vec<CompareDatasetSummary>, WorkspaceError> {
    let left_datasets: BTreeMap<&str, &Dataset> = left
        .data_contract()
        .datasets
        .iter()
        .map(|dataset| (dataset.id.as_str(), dataset))
        .collect();
    let right_datasets: BTreeMap<&str, &Dataset> = right
        .data_contract()
        .datasets
        .iter()
        .map(|dataset| (dataset.id.as_str(), dataset))
        .collect();
    let mut ids = left_datasets
        .keys()
        .chain(right_datasets.keys())
        .copied()
        .collect::<Vec<_>>();
    ids.sort_unstable();
    ids.dedup();
    let mut summaries = Vec::with_capacity(ids.len());
    for id in ids {
        control.check()?;
        let left_dataset = left_datasets.get(id).copied();
        let right_dataset = right_datasets.get(id).copied();
        let projection = left_dataset
            .or(right_dataset)
            .ok_or_else(invalid_contract)?;
        let sensitive = left_dataset
            .is_some_and(|dataset| dataset.sensitivity == Sensitivity::Sensitive)
            || right_dataset.is_some_and(|dataset| dataset.sensitivity == Sensitivity::Sensitive);
        let left_tables: BTreeMap<&str, &DatasetTable> = left_dataset
            .into_iter()
            .flat_map(|dataset| &dataset.tables)
            .map(|table| (table.name.as_str(), table))
            .collect();
        let right_tables: BTreeMap<&str, &DatasetTable> = right_dataset
            .into_iter()
            .flat_map(|dataset| &dataset.tables)
            .map(|table| (table.name.as_str(), table))
            .collect();
        let mut table_names = left_tables
            .keys()
            .chain(right_tables.keys())
            .copied()
            .collect::<Vec<_>>();
        table_names.sort_unstable();
        table_names.dedup();
        let mut tables = Vec::with_capacity(table_names.len());
        let mut left_rows = 0_u64;
        let mut right_rows = 0_u64;
        for table_name in table_names {
            let left_count = if left_tables.contains_key(table_name) {
                table_count(left.verified.connection(), table_name)?
            } else {
                0
            };
            let right_count = if right_tables.contains_key(table_name) {
                table_count(right.verified.connection(), table_name)?
            } else {
                0
            };
            left_rows = left_rows
                .checked_add(left_count)
                .ok_or_else(limit_exceeded)?;
            right_rows = right_rows
                .checked_add(right_count)
                .ok_or_else(limit_exceeded)?;
            let declaration = left_tables
                .get(table_name)
                .copied()
                .or_else(|| right_tables.get(table_name).copied())
                .ok_or_else(invalid_contract)?;
            tables.push(CompareTableSummary {
                table: table_name.to_owned(),
                primary_key: declaration.primary_key.clone(),
                state: CompareDataState::Unavailable,
                left_rows: left_count,
                right_rows: right_count,
                counts: CompareCounts::unavailable(),
                left_digest: None,
                right_digest: None,
                details_redacted: sensitive,
                truncated: false,
            });
        }
        summaries.push(CompareDatasetSummary {
            dataset_id: id.to_owned(),
            policy: projection.compare,
            sensitivity: if sensitive {
                Sensitivity::Sensitive
            } else {
                Sensitivity::Normal
            },
            state: CompareDataState::Unavailable,
            left_rows,
            right_rows,
            counts: CompareCounts::unavailable(),
            left_digest: None,
            right_digest: None,
            tables,
            details_redacted: sensitive,
            truncated: false,
        });
    }
    Ok(summaries)
}

fn compare_dataset(
    left: &Connection,
    right: &Connection,
    dataset: &Dataset,
    limits: &CompareLimitsApplied,
    total_rows: &mut u64,
    stream_bytes: &mut u64,
    control: &WorkspaceControl,
) -> Result<CompareDatasetSummary, WorkspaceError> {
    let mut tables = Vec::new();
    let mut aggregate = CompareCounts::zero();
    let mut left_rows = 0_u64;
    let mut right_rows = 0_u64;
    let mut truncated = false;
    let mut dataset_left = Sha256::new();
    let mut dataset_right = Sha256::new();
    for table in &dataset.tables {
        let summary = compare_table(
            left,
            right,
            table,
            dataset,
            limits,
            total_rows,
            stream_bytes,
            control,
        )?;
        left_rows = left_rows
            .checked_add(summary.left_rows)
            .ok_or_else(limit_exceeded)?;
        right_rows = right_rows
            .checked_add(summary.right_rows)
            .ok_or_else(limit_exceeded)?;
        if !summary.truncated
            && summary.state != CompareDataState::Ignored
            && dataset.compare != ComparePolicy::Summary
        {
            add_counts(&mut aggregate, &summary.counts)?;
        }
        if let Some(digest) = &summary.left_digest {
            dataset_left.update(digest.as_bytes());
        }
        if let Some(digest) = &summary.right_digest {
            dataset_right.update(digest.as_bytes());
        }
        truncated |= summary.truncated;
        tables.push(summary);
    }
    let ignored = dataset.compare == ComparePolicy::Ignore;
    let counts = if ignored || truncated || dataset.compare == ComparePolicy::Summary {
        CompareCounts::unavailable()
    } else {
        aggregate.clone()
    };
    let different = tables
        .iter()
        .any(|table| table.state == CompareDataState::Different);
    let state = if ignored {
        CompareDataState::Ignored
    } else if truncated {
        CompareDataState::Truncated
    } else if different {
        CompareDataState::Different
    } else {
        CompareDataState::Same
    };
    let expose_digest = dataset.sensitivity == Sensitivity::Normal && !ignored && !truncated;
    Ok(CompareDatasetSummary {
        dataset_id: dataset.id.clone(),
        policy: dataset.compare,
        sensitivity: dataset.sensitivity,
        state,
        left_rows,
        right_rows,
        counts: if dataset.compare == ComparePolicy::Summary {
            CompareCounts::unavailable()
        } else {
            counts
        },
        left_digest: expose_digest.then(|| lower_hex(&dataset_left.finalize())),
        right_digest: expose_digest.then(|| lower_hex(&dataset_right.finalize())),
        tables,
        details_redacted: dataset.sensitivity == Sensitivity::Sensitive,
        truncated,
    })
}

#[allow(clippy::too_many_arguments)]
fn compare_table(
    left: &Connection,
    right: &Connection,
    table: &DatasetTable,
    dataset: &Dataset,
    limits: &CompareLimitsApplied,
    total_rows: &mut u64,
    stream_bytes: &mut u64,
    control: &WorkspaceControl,
) -> Result<CompareTableSummary, WorkspaceError> {
    let left_count = table_count(left, &table.name)?;
    let right_count = table_count(right, &table.name)?;
    let ignored = dataset.compare == ComparePolicy::Ignore;
    let pair_rows = left_count
        .checked_add(right_count)
        .ok_or_else(limit_exceeded)?;
    let row_limit = left_count > limits.max_rows_per_table
        || right_count > limits.max_rows_per_table
        || total_rows.saturating_add(pair_rows) > limits.max_total_rows;
    if ignored || row_limit {
        return Ok(CompareTableSummary {
            table: table.name.clone(),
            primary_key: table.primary_key.clone(),
            state: if ignored {
                CompareDataState::Ignored
            } else {
                CompareDataState::Truncated
            },
            left_rows: left_count,
            right_rows: right_count,
            counts: CompareCounts::unavailable(),
            left_digest: None,
            right_digest: None,
            details_redacted: dataset.sensitivity == Sensitivity::Sensitive,
            truncated: row_limit,
        });
    }
    *total_rows += pair_rows;
    let columns = compared_columns(left, table, control)?;
    if columns != compared_columns(right, table, control)? {
        return Err(invalid_contract());
    }
    let sql = compared_row_sql(table, &columns);
    let key_indexes = compared_key_indexes(table, &columns)?;
    let mut left_statement = left.prepare(&sql).map_err(|_| invalid_contract())?;
    let mut right_statement = right.prepare(&sql).map_err(|_| invalid_contract())?;
    let mut left_query = left_statement.query([]).map_err(|_| invalid_contract())?;
    let mut right_query = right_statement.query([]).map_err(|_| invalid_contract())?;
    let mut left_digest = Sha256::new();
    let mut right_digest = Sha256::new();
    let mut left_row = next_compared_row(
        &mut left_query,
        table,
        &columns,
        &key_indexes,
        limits,
        stream_bytes,
        control,
    )?;
    let mut right_row = next_compared_row(
        &mut right_query,
        table,
        &columns,
        &key_indexes,
        limits,
        stream_bytes,
        control,
    )?;
    if matches!(left_row, StreamRow::Truncated) || matches!(right_row, StreamRow::Truncated) {
        return Ok(truncated_table(table, dataset, left_count, right_count));
    }
    let mut counts = CompareCounts::zero();
    loop {
        control.check()?;
        match (&left_row, &right_row) {
            (StreamRow::Row(l), StreamRow::Row(r)) => {
                match compare_keys(&l.key_values, &r.key_values)? {
                    Ordering::Less => {
                        left_digest.update(l.row_digest);
                        increment(&mut counts.removed)?;
                        left_row = next_compared_row(
                            &mut left_query,
                            table,
                            &columns,
                            &key_indexes,
                            limits,
                            stream_bytes,
                            control,
                        )?;
                    }
                    Ordering::Greater => {
                        right_digest.update(r.row_digest);
                        increment(&mut counts.added)?;
                        right_row = next_compared_row(
                            &mut right_query,
                            table,
                            &columns,
                            &key_indexes,
                            limits,
                            stream_bytes,
                            control,
                        )?;
                    }
                    Ordering::Equal => {
                        left_digest.update(l.row_digest);
                        right_digest.update(r.row_digest);
                        if l.row_digest == r.row_digest {
                            increment(&mut counts.unchanged)?;
                        } else {
                            increment(&mut counts.changed)?;
                        }
                        left_row = next_compared_row(
                            &mut left_query,
                            table,
                            &columns,
                            &key_indexes,
                            limits,
                            stream_bytes,
                            control,
                        )?;
                        right_row = next_compared_row(
                            &mut right_query,
                            table,
                            &columns,
                            &key_indexes,
                            limits,
                            stream_bytes,
                            control,
                        )?;
                    }
                }
            }
            (StreamRow::Row(l), StreamRow::End) => {
                left_digest.update(l.row_digest);
                increment(&mut counts.removed)?;
                left_row = next_compared_row(
                    &mut left_query,
                    table,
                    &columns,
                    &key_indexes,
                    limits,
                    stream_bytes,
                    control,
                )?;
            }
            (StreamRow::End, StreamRow::Row(r)) => {
                right_digest.update(r.row_digest);
                increment(&mut counts.added)?;
                right_row = next_compared_row(
                    &mut right_query,
                    table,
                    &columns,
                    &key_indexes,
                    limits,
                    stream_bytes,
                    control,
                )?;
            }
            (StreamRow::End, StreamRow::End) => break,
            (StreamRow::Truncated, _) | (_, StreamRow::Truncated) => {
                return Ok(truncated_table(table, dataset, left_count, right_count));
            }
        }
    }
    let different = changed_total(&counts)? > 0;
    let sensitive = dataset.sensitivity == Sensitivity::Sensitive;
    Ok(CompareTableSummary {
        table: table.name.clone(),
        primary_key: table.primary_key.clone(),
        state: if different {
            CompareDataState::Different
        } else {
            CompareDataState::Same
        },
        left_rows: left_count,
        right_rows: right_count,
        counts: if dataset.compare == ComparePolicy::Summary {
            CompareCounts::unavailable()
        } else {
            counts
        },
        left_digest: (!sensitive).then(|| lower_hex(&left_digest.finalize())),
        right_digest: (!sensitive).then(|| lower_hex(&right_digest.finalize())),
        details_redacted: sensitive,
        truncated: false,
    })
}

fn truncated_table(
    table: &DatasetTable,
    dataset: &Dataset,
    left_rows: u64,
    right_rows: u64,
) -> CompareTableSummary {
    CompareTableSummary {
        table: table.name.clone(),
        primary_key: table.primary_key.clone(),
        state: CompareDataState::Truncated,
        left_rows,
        right_rows,
        counts: CompareCounts::unavailable(),
        left_digest: None,
        right_digest: None,
        details_redacted: dataset.sensitivity == Sensitivity::Sensitive,
        truncated: true,
    }
}

fn increment(value: &mut Option<u64>) -> Result<(), WorkspaceError> {
    let current = value.as_mut().ok_or_else(invalid_contract)?;
    *current = current.checked_add(1).ok_or_else(limit_exceeded)?;
    Ok(())
}

fn add_counts(total: &mut CompareCounts, add: &CompareCounts) -> Result<(), WorkspaceError> {
    for (total, add) in [
        (&mut total.added, add.added),
        (&mut total.removed, add.removed),
        (&mut total.changed, add.changed),
        (&mut total.unchanged, add.unchanged),
    ] {
        let total = total.as_mut().ok_or_else(invalid_contract)?;
        *total = total
            .checked_add(add.ok_or_else(invalid_contract)?)
            .ok_or_else(limit_exceeded)?;
    }
    Ok(())
}

fn changed_total(counts: &CompareCounts) -> Result<u64, WorkspaceError> {
    counts
        .added
        .ok_or_else(invalid_contract)?
        .checked_add(counts.removed.ok_or_else(invalid_contract)?)
        .and_then(|value| value.checked_add(counts.changed?))
        .ok_or_else(invalid_contract)
}

fn table_count(connection: &Connection, table: &str) -> Result<u64, WorkspaceError> {
    let sql = format!("SELECT count(*) FROM {}", quote_identifier(table));
    let value: i64 = connection
        .query_row(&sql, [], |row| row.get(0))
        .map_err(|_| invalid_contract())?;
    u64::try_from(value).map_err(|_| invalid_contract())
}

pub(crate) fn compared_columns(
    connection: &Connection,
    table: &DatasetTable,
    control: &WorkspaceControl,
) -> Result<Vec<String>, WorkspaceError> {
    let mut statement = connection
        .prepare(
            "SELECT name FROM pragma_table_xinfo(?1) \
             WHERE hidden IN (0, 3) ORDER BY cid LIMIT 257",
        )
        .map_err(|_| invalid_contract())?;
    let mut rows = statement
        .query([&table.name])
        .map_err(|_| invalid_contract())?;
    let mut columns = Vec::new();
    while let Some(row) = rows.next().map_err(|_| invalid_contract())? {
        control.check()?;
        if columns.len() == 256 {
            return Err(limit_exceeded());
        }
        let name: String = row.get(0).map_err(|_| invalid_contract())?;
        if !table.ignored_columns.contains(&name) {
            columns.push(name);
        }
    }
    if columns.is_empty() {
        return Err(invalid_contract());
    }
    Ok(columns)
}

struct ComparedRow {
    key_values: Vec<CompareValue>,
    row_digest: [u8; 32],
}

fn compared_row_sql(table: &DatasetTable, columns: &[String]) -> String {
    let projection = columns
        .iter()
        .map(|v| quote_identifier(v))
        .collect::<Vec<_>>()
        .join(",");
    let ordering = table
        .primary_key
        .iter()
        .map(|v| format!("{} COLLATE BINARY ASC", quote_identifier(v)))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "SELECT {projection} FROM {} ORDER BY {ordering}",
        quote_identifier(&table.name)
    )
}

fn compared_key_indexes(
    table: &DatasetTable,
    columns: &[String],
) -> Result<Vec<usize>, WorkspaceError> {
    table
        .primary_key
        .iter()
        .map(|key| {
            columns
                .iter()
                .position(|column| column == key)
                .ok_or_else(invalid_contract)
        })
        .collect()
}

enum StreamRow {
    End,
    Row(ComparedRow),
    Truncated,
}

fn next_compared_row(
    rows: &mut rusqlite::Rows<'_>,
    table: &DatasetTable,
    columns: &[String],
    key_indexes: &[usize],
    limits: &CompareLimitsApplied,
    stream_bytes: &mut u64,
    control: &WorkspaceControl,
) -> Result<StreamRow, WorkspaceError> {
    control.check()?;
    #[cfg(test)]
    TEST_ROW_CANCELLATION.with(|slot| {
        let mut slot = slot.borrow_mut();
        if let Some((remaining, cancellation)) = slot.as_mut() {
            if *remaining == 0 {
                cancellation.cancel();
                *slot = None;
            } else {
                *remaining -= 1;
            }
        }
    });
    let Some(row) = rows.next().map_err(|_| invalid_contract())? else {
        return Ok(StreamRow::End);
    };
    let mut values = Vec::with_capacity(columns.len());
    for index in 0..columns.len() {
        let Some(value) = owned_value(
            row.get_ref(index).map_err(|_| invalid_contract())?,
            limits.max_value_bytes,
        )?
        else {
            return Ok(StreamRow::Truncated);
        };
        values.push(value);
    }
    let key = key_indexes
        .iter()
        .map(|index| (columns[*index].clone(), values[*index].clone()))
        .collect::<Vec<_>>();
    let compared = columns
        .iter()
        .cloned()
        .zip(values.iter().cloned())
        .collect::<Vec<_>>();
    let frame = canonical_compare_row(&table.name, &key, &compared, limits.max_value_bytes)?;
    let Some(next_stream_bytes) = stream_bytes.checked_add(frame.len() as u64) else {
        return Ok(StreamRow::Truncated);
    };
    if next_stream_bytes > limits.max_stream_bytes {
        return Ok(StreamRow::Truncated);
    }
    *stream_bytes = next_stream_bytes;
    Ok(StreamRow::Row(ComparedRow {
        key_values: key.into_iter().map(|(_, value)| value).collect(),
        row_digest: Sha256::digest(frame).into(),
    }))
}

fn owned_value(value: ValueRef<'_>, max: u64) -> Result<Option<CompareValue>, WorkspaceError> {
    let value = match value {
        ValueRef::Null => CompareValue::Null,
        ValueRef::Integer(value) => CompareValue::Integer(value),
        ValueRef::Real(value) => CompareValue::Real(value),
        ValueRef::Text(value) => {
            if value.len() as u64 > max {
                return Ok(None);
            }
            CompareValue::Text(value.to_vec())
        }
        ValueRef::Blob(value) => {
            if value.len() as u64 > max {
                return Ok(None);
            }
            CompareValue::Blob(value.to_vec())
        }
    };
    // Validate finite REAL and UTF-8 even before framing.
    let mut sink = Vec::new();
    frame_value(&mut sink, &value, max)?;
    Ok(Some(value))
}

pub(crate) fn compare_keys(
    left: &[CompareValue],
    right: &[CompareValue],
) -> Result<Ordering, WorkspaceError> {
    if left.len() != right.len() {
        return Err(invalid_contract());
    }
    for (left, right) in left.iter().zip(right) {
        let ordering = compare_value(left, right)?;
        if ordering != Ordering::Equal {
            return Ok(ordering);
        }
    }
    Ok(Ordering::Equal)
}

fn compare_value(left: &CompareValue, right: &CompareValue) -> Result<Ordering, WorkspaceError> {
    let rank = |value: &CompareValue| match value {
        CompareValue::Null => 0,
        CompareValue::Integer(_) | CompareValue::Real(_) => 1,
        CompareValue::Text(_) => 2,
        CompareValue::Blob(_) => 3,
    };
    let ranks = rank(left).cmp(&rank(right));
    if ranks != Ordering::Equal {
        return Ok(ranks);
    }
    match (left, right) {
        (CompareValue::Null, CompareValue::Null) => Ok(Ordering::Equal),
        (CompareValue::Integer(l), CompareValue::Integer(r)) => Ok(l.cmp(r)),
        (CompareValue::Real(l), CompareValue::Real(r)) => {
            if !l.is_finite() || !r.is_finite() {
                return Err(invalid_contract());
            }
            match l.partial_cmp(r).ok_or_else(invalid_contract)? {
                Ordering::Equal => Ok(l.to_bits().cmp(&r.to_bits())),
                ordering => Ok(ordering),
            }
        }
        (CompareValue::Integer(l), CompareValue::Real(r)) => {
            Ok(compare_integer_real(*l, *r)?.then(Ordering::Less))
        }
        (CompareValue::Real(l), CompareValue::Integer(r)) => Ok(compare_integer_real(*r, *l)?
            .reverse()
            .then(Ordering::Greater)),
        (CompareValue::Text(l), CompareValue::Text(r))
        | (CompareValue::Blob(l), CompareValue::Blob(r)) => Ok(l.cmp(r)),
        _ => Err(invalid_contract()),
    }
}

/// Mirrors SQLite's exact finite INTEGER-vs-REAL numeric ordering without the
/// precision loss caused by first converting the integer to `f64`.
fn compare_integer_real(integer: i64, real: f64) -> Result<Ordering, WorkspaceError> {
    if !real.is_finite() {
        return Err(invalid_contract());
    }
    const I64_MIN_AS_REAL: f64 = -9_223_372_036_854_775_808.0;
    const I64_UPPER_BOUND_AS_REAL: f64 = 9_223_372_036_854_775_808.0;
    if real < I64_MIN_AS_REAL {
        return Ok(Ordering::Greater);
    }
    if real >= I64_UPPER_BOUND_AS_REAL {
        return Ok(Ordering::Less);
    }
    let truncated = real as i64;
    match integer.cmp(&truncated) {
        Ordering::Equal => {
            let integer_as_real = integer as f64;
            if real < integer_as_real {
                Ok(Ordering::Greater)
            } else if real > integer_as_real {
                Ok(Ordering::Less)
            } else {
                Ok(Ordering::Equal)
            }
        }
        other => Ok(other),
    }
}

fn canonical_digest<T: Serialize>(value: &T) -> Result<String, WorkspaceError> {
    let value = serde_json::to_value(value).map_err(|_| invalid_contract())?;
    let bytes = crate::plan::canonical_json(&value)?;
    Ok(lower_hex(&Sha256::digest(bytes)))
}

fn report_digest(summary: &CompareSummary) -> Result<String, WorkspaceError> {
    let mut value = serde_json::to_value(summary).map_err(|_| invalid_contract())?;
    value
        .as_object_mut()
        .ok_or_else(invalid_contract)?
        .remove("report_digest");
    let bytes = crate::plan::canonical_json(&value)?;
    if bytes.len().saturating_add(128) > HARD_MAX_COMPARE_REPORT_BYTES {
        return Err(WorkspaceError::new(WorkspaceErrorCode::LimitExceeded));
    }
    Ok(lower_hex(&Sha256::digest(bytes)))
}

pub(crate) fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}
pub(crate) fn lower_hex(bytes: &[u8]) -> String {
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
    use rusqlite::Connection;
    use sqlite_capsule_crypto::{PROFILE_V03, application_digest, sign_digest_for_profile};

    use super::*;

    const DEVELOPMENT_SEED: &str =
        include_str!("../../../../compatibility/signed-app-v0.2/development-seed.hex");

    #[test]
    fn rust_matches_independent_compare_row_vectors() {
        let vectors: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../compatibility/compare-row-v1/vectors.json"
        ))
        .unwrap();
        for case in vectors["cases"].as_array().unwrap() {
            let parse = |field: &str| {
                case[field]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|item| {
                        (
                            item["column"].as_str().unwrap().to_owned(),
                            vector_value(&item["value"]),
                        )
                    })
                    .collect::<Vec<_>>()
            };
            let key = parse("key");
            let compared = parse("compared");
            let key_bytes =
                canonical_compare_key(case["table"].as_str().unwrap(), &key, HARD_VALUE_BYTES)
                    .unwrap();
            let row_bytes = canonical_compare_row(
                case["table"].as_str().unwrap(),
                &key,
                &compared,
                HARD_VALUE_BYTES,
            )
            .unwrap();
            assert_eq!(hex(&key_bytes), case["key_bytes_hex"].as_str().unwrap());
            assert_eq!(
                lower_hex(&Sha256::digest(&key_bytes)),
                case["key_sha256"].as_str().unwrap()
            );
            assert_eq!(hex(&row_bytes), case["row_bytes_hex"].as_str().unwrap());
            assert_eq!(
                lower_hex(&Sha256::digest(&row_bytes)),
                case["row_sha256"].as_str().unwrap()
            );
        }

        for case in vectors["invalid"].as_array().unwrap() {
            let value = vector_value(&case["value"]);
            assert_eq!(
                canonical_compare_row(
                    "hostile",
                    &[("id".to_owned(), value)],
                    &[],
                    HARD_VALUE_BYTES,
                )
                .expect_err("hostile typed value must fail")
                .kind(),
                WorkspaceErrorCode::InvalidContract,
                "{}",
                case["id"]
            );
        }
    }

    #[test]
    fn summary_is_deterministic_read_only_and_classifies_domain_changes() {
        let (_left_directory, left_path) = crate::tests::signed_fixture("compare-left");
        let (_right_directory, right_path) = crate::tests::signed_fixture("compare-right");
        let connection = Connection::open(&right_path).unwrap();
        connection
            .execute(
                "UPDATE vector_domain SET note='secret-right-value' WHERE id='domain'",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO vector_domain VALUES \
                 ('second','new secret',1.5,X'0102')",
                [],
            )
            .unwrap();
        drop(connection);
        let left_before = fs::read(&left_path).unwrap();
        let right_before = fs::read(&right_path).unwrap();
        let left = VerifiedWorkspaceSource::open(&left_path).unwrap();
        let right = VerifiedWorkspaceSource::open(&right_path).unwrap();

        let first = compare_sources(
            &left,
            &right,
            &CompareLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        let second = compare_sources(
            &left,
            &right,
            &CompareLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        assert_eq!(first, second);
        assert_eq!(first.profile, COMPARE_SUMMARY_PROFILE);
        assert_eq!(
            first.compatibility.state,
            CompatibilityState::SameReleaseSameSchema
        );
        let content = first
            .datasets
            .iter()
            .find(|dataset| dataset.dataset_id == "content")
            .unwrap();
        assert_eq!(content.counts.added, Some(1));
        assert_eq!(content.counts.changed, Some(1));
        assert_eq!(content.counts.removed, Some(0));
        assert_eq!(content.left_rows, 1);
        assert_eq!(content.right_rows, 2);
        let json = serde_json::to_string(&first).unwrap();
        assert!(!json.contains("secret-right-value"));
        assert!(!json.contains("new secret"));
        assert_eq!(fs::read(&left_path).unwrap(), left_before);
        assert_eq!(fs::read(&right_path).unwrap(), right_before);
    }

    #[test]
    fn sensitive_contract_returns_counts_only_and_never_row_digests() {
        let (_left_directory, left_path) = crate::tests::signed_fixture("compare-sensitive-left");
        let (_right_directory, right_path) =
            crate::tests::signed_fixture("compare-sensitive-right");
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
                "UPDATE vector_domain SET note='classified' WHERE id='domain'",
                [],
            )
            .unwrap();
        drop(connection);
        let left = VerifiedWorkspaceSource::open(&left_path).unwrap();
        let right = VerifiedWorkspaceSource::open(&right_path).unwrap();
        let summary = compare_sources(
            &left,
            &right,
            &CompareLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        let content = summary
            .datasets
            .iter()
            .find(|dataset| dataset.dataset_id == "content")
            .unwrap();
        assert!(content.details_redacted);
        assert_eq!(content.counts.changed, Some(1));
        assert_eq!(content.left_digest, None);
        assert_eq!(content.right_digest, None);
        assert!(
            content
                .tables
                .iter()
                .all(|table| table.left_digest.is_none()
                    && table.right_digest.is_none()
                    && table.details_redacted)
        );
        assert!(
            !serde_json::to_string(&summary)
                .unwrap()
                .contains("classified")
        );
    }

    #[test]
    fn summary_policy_keeps_counts_and_digests_but_hides_row_classification() {
        let (_left_directory, left_path) = crate::tests::signed_fixture("compare-summary-left");
        let (_right_directory, right_path) = crate::tests::signed_fixture("compare-summary-right");
        for path in [&left_path, &right_path] {
            let connection = Connection::open(path).unwrap();
            connection
                .execute(
                    "UPDATE capsule_dataset SET compare_policy='summary', \
                     reconcile_policy='ignore' WHERE id='settings'",
                    [],
                )
                .unwrap();
            resign(&connection);
        }
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
        let summary = compare_sources(
            &left,
            &right,
            &CompareLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        let settings = summary
            .datasets
            .iter()
            .find(|dataset| dataset.dataset_id == "settings")
            .unwrap();
        assert_eq!(settings.state, CompareDataState::Different);
        assert_eq!(settings.left_rows, 1);
        assert_eq!(settings.right_rows, 1);
        assert_eq!(settings.counts, CompareCounts::unavailable());
        assert!(settings.left_digest.is_some());
        assert!(settings.right_digest.is_some());
        assert_ne!(settings.left_digest, settings.right_digest);
        assert_eq!(settings.tables[0].counts, CompareCounts::unavailable());
        assert_eq!(settings.tables[0].state, CompareDataState::Different);
    }

    #[test]
    fn summary_only_manual_contract_does_not_advertise_reconciliation() {
        let (_left_directory, left_path) =
            crate::tests::signed_fixture("compare-no-reconcile-left");
        let (_right_directory, right_path) =
            crate::tests::signed_fixture("compare-no-reconcile-right");
        for path in [&left_path, &right_path] {
            let connection = Connection::open(path).unwrap();
            connection
                .execute(
                    "UPDATE capsule_dataset SET compare_policy='summary', \
                     reconcile_policy='manual'",
                    [],
                )
                .unwrap();
            resign(&connection);
        }
        let left = VerifiedWorkspaceSource::open(&left_path).unwrap();
        let right = VerifiedWorkspaceSource::open(&right_path).unwrap();
        let summary = compare_sources(
            &left,
            &right,
            &CompareLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        assert!(summary.compatibility.can_compare_data);
        assert!(
            !summary.compatibility.can_reconcile,
            "summary-only datasets expose no stable row selection authority"
        );
    }

    #[test]
    fn input_refs_include_bounded_authenticated_publisher_evidence_without_trust_claims() {
        let (_left_directory, left_path) = crate::tests::signed_fixture("compare-publisher-left");
        let (_right_directory, right_path) =
            crate::tests::signed_fixture("compare-publisher-right");
        let left = VerifiedWorkspaceSource::open(&left_path).unwrap();
        let right = VerifiedWorkspaceSource::open(&right_path).unwrap();
        let summary = compare_sources(
            &left,
            &right,
            &CompareLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        assert_eq!(summary.left.publisher.publisher_id, "org.example.vector");
        assert_eq!(summary.left.publisher.publisher_name, "Vector Publisher");
        assert_eq!(summary.left.publisher.signature_count, 1);
        assert_eq!(summary.left.publisher.signatures.len(), 1);
        assert_eq!(
            summary.left.publisher.signatures[0].status,
            CompareSignatureStatus::Valid
        );
        let json = serde_json::to_value(&summary.left.publisher).unwrap();
        assert!(json.get("trusted").is_none());
        assert!(json.get("trust").is_none());
    }

    #[test]
    fn direct_lineage_requires_current_child_claim_and_other_retained_source_digest() {
        let (_left_directory, left_path) = crate::tests::signed_fixture("compare-parent-left");
        let (_right_directory, right_path) = crate::tests::signed_fixture("compare-parent-right");
        let left_before = fs::read(&left_path).unwrap();
        let left = VerifiedWorkspaceSource::open(&left_path).unwrap();
        let left_sha256 = left.source_sha256();
        let connection = Connection::open(&right_path).unwrap();
        insert_parent_claim(&connection, 1, "forked-from", &left_sha256);
        insert_parent_claim(&connection, 2, "application-release", &left_sha256);
        drop(connection);
        let right_before = fs::read(&right_path).unwrap();
        let right = VerifiedWorkspaceSource::open(&right_path).unwrap();

        let summary = compare_sources(
            &left,
            &right,
            &CompareLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        assert_eq!(
            summary.lineage.direct_relationship,
            CompareDirectRelationship::LeftIsParentOfRight
        );
        assert_eq!(
            summary.lineage.direct_evidence.len(),
            1,
            "multiple mutable relation labels are canonically bounded per side"
        );
        assert_eq!(
            summary.lineage.direct_evidence[0],
            CompareDirectLineageEvidence {
                child_side: CompareSide::Right,
                relation: crate::ParentRelation::ForkedFrom,
                parent_file_sha256: left_sha256.clone(),
                verification: CompareDirectEvidenceVerification::
                    ClaimedParentFileDigestMatchesOtherRetainedSourceBytes,
            }
        );
        let json = serde_json::to_value(&summary.lineage).unwrap();
        assert_eq!(
            json["direct_evidence"][0]["verification"],
            "claimed-parent-file-digest-matches-other-retained-source-bytes"
        );
        assert_eq!(fs::read(&left_path).unwrap(), left_before);
        assert_eq!(fs::read(&right_path).unwrap(), right_before);

        let (_stale_directory, stale_path) =
            crate::tests::signed_fixture("compare-parent-stale-child");
        let connection = Connection::open(&stale_path).unwrap();
        connection
            .execute(
                "UPDATE capsule_lineage_event SET \
                 result_revision_id='99999999-9999-4999-8999-999999999999'",
                [],
            )
            .unwrap();
        insert_parent_claim(&connection, 1, "forked-from", &left_sha256);
        drop(connection);
        let stale = VerifiedWorkspaceSource::open(&stale_path).unwrap();
        let summary = compare_sources(
            &left,
            &stale,
            &CompareLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        assert_eq!(
            summary.lineage.direct_relationship,
            CompareDirectRelationship::None
        );
        assert!(summary.lineage.direct_evidence.is_empty());
    }

    #[test]
    fn shared_third_parent_hash_is_only_an_explicit_mutable_untrusted_claim() {
        let (_left_directory, left_path) =
            crate::tests::signed_fixture("compare-common-claim-left");
        let (_right_directory, right_path) =
            crate::tests::signed_fixture("compare-common-claim-right");
        let shared = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
        let left_only = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
        let right_only = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
        let left_connection = Connection::open(&left_path).unwrap();
        insert_parent_claim(&left_connection, 1, "created-from", shared);
        insert_parent_claim(&left_connection, 2, "forked-from", left_only);
        drop(left_connection);
        let right_connection = Connection::open(&right_path).unwrap();
        insert_parent_claim(&right_connection, 1, "created-from", shared);
        insert_parent_claim(&right_connection, 2, "forked-from", right_only);
        drop(right_connection);
        let left = VerifiedWorkspaceSource::open(&left_path).unwrap();
        let right = VerifiedWorkspaceSource::open(&right_path).unwrap();

        let summary = compare_sources(
            &left,
            &right,
            &CompareLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        assert_eq!(
            summary.lineage.direct_relationship,
            CompareDirectRelationship::None
        );
        assert_eq!(
            summary.lineage.common_ancestor_claims,
            vec![CompareCommonAncestorClaim {
                file_sha256: shared.to_owned(),
                claimed_by: CompareAncestorClaimedBy::Both,
                verification: CompareAncestorClaimVerification::MutableUntrustedClaimOnly,
            }]
        );
        assert!(!summary.lineage.common_ancestor_claims_truncated);
        let json = serde_json::to_value(&summary.lineage).unwrap();
        assert_eq!(
            json["common_ancestor_claims"][0]["verification"],
            "mutable-untrusted-claim-only"
        );
        assert!(!json.to_string().contains(left_only));
        assert!(!json.to_string().contains(right_only));
    }

    #[test]
    fn common_ancestor_claim_projection_is_deterministically_bounded() {
        let (_left_directory, left_path) =
            crate::tests::signed_fixture("compare-common-bound-left");
        let (_right_directory, right_path) =
            crate::tests::signed_fixture("compare-common-bound-right");
        for path in [&left_path, &right_path] {
            let connection = Connection::open(path).unwrap();
            for sequence in 2..=34_i64 {
                let event_id = format!("00000000-0000-4000-8000-{sequence:012}");
                connection
                    .execute(
                        "INSERT INTO capsule_lineage_event VALUES \
                         (?1,?2,'fork','11111111-1111-4111-8111-111111111111', \
                          '22222222-2222-4222-8222-222222222222', \
                          '2026-08-08T00:00:00Z', \
                          'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', \
                          'org.sqlite-capsule.vector-data',2, \
                          'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb','{}')",
                        params![event_id, sequence],
                    )
                    .unwrap();
                connection
                    .execute(
                        "INSERT INTO capsule_lineage_parent VALUES \
                         (?1,1,'forked-from',NULL,NULL,?2)",
                        params![event_id, format!("{sequence:064x}")],
                    )
                    .unwrap();
            }
            drop(connection);
        }
        let left = VerifiedWorkspaceSource::open(&left_path).unwrap();
        let right = VerifiedWorkspaceSource::open(&right_path).unwrap();
        let first = compare_sources(
            &left,
            &right,
            &CompareLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        let second = compare_sources(
            &left,
            &right,
            &CompareLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        assert_eq!(first, second);
        assert_eq!(
            first.lineage.common_ancestor_claims.len(),
            MAX_COMMON_ANCESTOR_CLAIMS
        );
        assert!(first.lineage.common_ancestor_claims_truncated);
        assert_eq!(
            first.lineage.common_ancestor_claims[0].file_sha256,
            format!("{:064x}", 2)
        );
    }

    #[test]
    fn row_value_and_stream_limits_preserve_counts_and_mark_truncation() {
        let (_left_directory, left_path) = crate::tests::signed_fixture("compare-limits-left");
        let (_right_directory, right_path) = crate::tests::signed_fixture("compare-limits-right");
        let left = VerifiedWorkspaceSource::open(&left_path).unwrap();
        let right = VerifiedWorkspaceSource::open(&right_path).unwrap();
        for limits in [
            CompareLimits {
                max_total_rows: 1,
                ..CompareLimits::default()
            },
            CompareLimits {
                max_value_bytes: 1,
                ..CompareLimits::default()
            },
            CompareLimits {
                max_stream_bytes: 1,
                ..CompareLimits::default()
            },
        ] {
            let summary =
                compare_sources(&left, &right, &limits, &CancellationToken::new()).unwrap();
            assert!(summary.truncated);
            let content = summary
                .datasets
                .iter()
                .find(|dataset| dataset.dataset_id == "content")
                .unwrap();
            assert_eq!(content.left_rows, 1);
            assert_eq!(content.right_rows, 1);
            assert_eq!(content.counts, CompareCounts::unavailable());
            assert!(content.truncated);
        }
    }

    #[test]
    fn cancellation_zero_limits_and_final_source_rebinding_fail_closed() {
        let (_left_directory, left_path) = crate::tests::signed_fixture("compare-control-left");
        let (_right_directory, right_path) = crate::tests::signed_fixture("compare-control-right");
        let left = VerifiedWorkspaceSource::open(&left_path).unwrap();
        let right = VerifiedWorkspaceSource::open(&right_path).unwrap();
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        assert_eq!(
            compare_sources(&left, &right, &CompareLimits::default(), &cancellation)
                .unwrap_err()
                .kind(),
            WorkspaceErrorCode::Cancelled
        );
        assert_eq!(
            compare_sources(
                &left,
                &right,
                &CompareLimits {
                    max_total_rows: 0,
                    ..CompareLimits::default()
                },
                &CancellationToken::new(),
            )
            .unwrap_err()
            .kind(),
            WorkspaceErrorCode::LimitExceeded
        );

        let connection = Connection::open(&right_path).unwrap();
        connection
            .execute(
                "UPDATE vector_domain SET note='raced' WHERE id='domain'",
                [],
            )
            .unwrap();
        drop(connection);
        assert_eq!(
            compare_sources(
                &left,
                &right,
                &CompareLimits::default(),
                &CancellationToken::new(),
            )
            .unwrap_err()
            .kind(),
            WorkspaceErrorCode::StalePlan
        );
    }

    #[test]
    fn cancellation_during_live_row_stream_is_not_misreported_as_invalid_contract() {
        let (_left_directory, left_path) =
            crate::tests::signed_fixture("compare-inflight-cancel-left");
        let (_right_directory, right_path) =
            crate::tests::signed_fixture("compare-inflight-cancel-right");
        for path in [&left_path, &right_path] {
            let connection = Connection::open(path).unwrap();
            for ordinal in 0..8 {
                connection
                    .execute(
                        "INSERT INTO vector_domain VALUES (?1,?2,?3,?4)",
                        params![
                            format!("row-{ordinal:02}"),
                            "bounded",
                            ordinal as f64,
                            [ordinal as u8]
                        ],
                    )
                    .unwrap();
            }
        }
        let left_before = fs::read(&left_path).unwrap();
        let right_before = fs::read(&right_path).unwrap();
        let left = VerifiedWorkspaceSource::open(&left_path).unwrap();
        let right = VerifiedWorkspaceSource::open(&right_path).unwrap();
        let cancellation = CancellationToken::new();
        TEST_ROW_CANCELLATION.with(|slot| {
            *slot.borrow_mut() = Some((1, cancellation.clone()));
        });

        assert_eq!(
            compare_sources(&left, &right, &CompareLimits::default(), &cancellation)
                .unwrap_err()
                .kind(),
            WorkspaceErrorCode::Cancelled
        );
        assert_eq!(fs::read(&left_path).unwrap(), left_before);
        assert_eq!(fs::read(&right_path).unwrap(), right_before);
    }

    #[test]
    fn signed_contract_drift_is_successfully_reported_but_data_is_unavailable() {
        let (_left_directory, left_path) = crate::tests::signed_fixture("compare-contract-left");
        let (_right_directory, right_path) = crate::tests::signed_fixture("compare-contract-right");
        let connection = Connection::open(&right_path).unwrap();
        connection
            .execute(
                "UPDATE capsule_dataset SET description='new signed description' WHERE id='content'",
                [],
            )
            .unwrap();
        resign(&connection);
        let left = VerifiedWorkspaceSource::open(&left_path).unwrap();
        let right = VerifiedWorkspaceSource::open(&right_path).unwrap();
        let summary = compare_sources(
            &left,
            &right,
            &CompareLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        assert_eq!(
            summary.compatibility.state,
            CompatibilityState::SameAppSameSchema
        );
        assert!(!summary.compatibility.can_compare_data);
        assert_eq!(summary.schema.state, CompareSectionState::Different);
        assert!(!summary.datasets.is_empty());
        assert!(
            summary
                .datasets
                .iter()
                .all(|dataset| dataset.state == CompareDataState::Unavailable
                    && dataset.counts == CompareCounts::unavailable())
        );
    }

    #[test]
    fn schema_index_drift_is_visible_without_blocking_compatible_data() {
        let (_left_directory, left_path) = crate::tests::signed_fixture("compare-schema-left");
        let (_right_directory, right_path) = crate::tests::signed_fixture("compare-schema-right");
        let connection = Connection::open(&right_path).unwrap();
        connection
            .execute("CREATE INDEX vector_domain_note ON vector_domain(note)", [])
            .unwrap();
        resign(&connection);
        let left = VerifiedWorkspaceSource::open(&left_path).unwrap();
        let right = VerifiedWorkspaceSource::open(&right_path).unwrap();
        let summary = compare_sources(
            &left,
            &right,
            &CompareLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        assert_eq!(
            summary.compatibility.state,
            CompatibilityState::SameAppSameSchema
        );
        assert!(summary.compatibility.can_compare_data);
        assert_eq!(summary.schema.state, CompareSectionState::Different);
        assert!(
            summary
                .datasets
                .iter()
                .all(|dataset| dataset.state == CompareDataState::Same)
        );
    }

    #[test]
    fn every_verified_source_compatibility_class_is_explicit() {
        let (_base_directory, base_path) = crate::tests::signed_fixture("compare-classes-base");
        let base = VerifiedWorkspaceSource::open(&base_path).unwrap();
        let same = compare_sources(
            &base,
            &base,
            &CompareLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        assert_eq!(
            same.compatibility.state,
            CompatibilityState::SameReleaseSameSchema
        );

        let (_migration_directory, migration_path) =
            crate::tests::signed_fixture("compare-classes-migration");
        let connection = Connection::open(&migration_path).unwrap();
        connection
            .execute(
                "UPDATE capsule_manifest SET data_schema_version=1 WHERE id=1",
                [],
            )
            .unwrap();
        resign(&connection);
        let migration = VerifiedWorkspaceSource::open(&migration_path).unwrap();
        let summary = compare_sources(
            &migration,
            &base,
            &CompareLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        assert_eq!(
            summary.compatibility.state,
            CompatibilityState::SameAppMigrationAvailable
        );
        assert!(!summary.compatibility.can_compare_data);

        let (_incompatible_directory, incompatible_path) =
            crate::tests::signed_fixture("compare-classes-incompatible");
        let connection = Connection::open(&incompatible_path).unwrap();
        connection
            .execute(
                "UPDATE capsule_manifest SET data_schema_version=3 WHERE id=1",
                [],
            )
            .unwrap();
        resign(&connection);
        let incompatible = VerifiedWorkspaceSource::open(&incompatible_path).unwrap();
        let summary = compare_sources(
            &base,
            &incompatible,
            &CompareLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        assert_eq!(
            summary.compatibility.state,
            CompatibilityState::SameAppIncompatibleSchema
        );

        let (_different_directory, different_path) =
            crate::tests::signed_fixture("compare-classes-different");
        let connection = Connection::open(&different_path).unwrap();
        connection
            .execute(
                "UPDATE capsule_manifest SET app_id='org.sqlite-capsule.other' WHERE id=1",
                [],
            )
            .unwrap();
        resign(&connection);
        let different = VerifiedWorkspaceSource::open(&different_path).unwrap();
        let summary = compare_sources(
            &base,
            &different,
            &CompareLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        assert_eq!(
            summary.compatibility.state,
            CompatibilityState::DifferentApplication
        );
        assert!(!summary.compatibility.can_compare_data);
        assert!(
            summary.datasets.is_empty(),
            "different applications must not disclose domain inventory or counts"
        );
    }

    #[test]
    fn invalid_input_is_an_admission_error_not_a_misleading_summary() {
        let (_directory, path) = crate::tests::signed_fixture("compare-invalid-admission");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute(
                "UPDATE capsule_application SET description='unsigned mutation' WHERE id=1",
                [],
            )
            .unwrap();
        drop(connection);
        assert_eq!(
            VerifiedWorkspaceSource::open(&path)
                .err()
                .expect("invalid input must fail admission")
                .kind(),
            WorkspaceErrorCode::InvalidSignature
        );
    }

    #[test]
    fn cyclic_and_ambiguous_signed_migration_graphs_never_gain_compatibility() {
        let (_base_directory, base_path) = crate::tests::signed_fixture("compare-graph-base");
        let connection = Connection::open(&base_path).unwrap();
        connection
            .execute(
                "UPDATE capsule_manifest SET data_schema_version=1 WHERE id=1",
                [],
            )
            .unwrap();
        resign(&connection);
        let base = VerifiedWorkspaceSource::open(&base_path).unwrap();

        let (_ambiguous_directory, ambiguous_path) =
            crate::tests::signed_fixture("compare-graph-ambiguous");
        let connection = Connection::open(&ambiguous_path).unwrap();
        connection
            .execute(
                "INSERT INTO capsule_migration VALUES \
                 ('vector-1-to-4','org.sqlite-capsule.vector-data',1,4,'alternate first',\
                  'org.sqlite-capsule.migration-ops/1',0), \
                 ('vector-4-to-2','org.sqlite-capsule.vector-data',4,2,'alternate second',\
                  'org.sqlite-capsule.migration-ops/1',0)",
                [],
            )
            .unwrap();
        resign(&connection);
        let ambiguous = VerifiedWorkspaceSource::open(&ambiguous_path).unwrap();
        let summary = compare_sources(
            &base,
            &ambiguous,
            &CompareLimits::default(),
            &CancellationToken::new(),
        )
        .unwrap();
        assert_eq!(
            summary.compatibility.state,
            CompatibilityState::SameAppIncompatibleSchema
        );

        let (_cycle_directory, cycle_path) = crate::tests::signed_fixture("compare-graph-cycle");
        let connection = Connection::open(&cycle_path).unwrap();
        connection
            .execute(
                "INSERT INTO capsule_migration VALUES \
                 ('vector-4-to-5','org.sqlite-capsule.vector-data',4,5,'cycle first',\
                  'org.sqlite-capsule.migration-ops/1',0), \
                 ('vector-5-to-4','org.sqlite-capsule.vector-data',5,4,'cycle second',\
                  'org.sqlite-capsule.migration-ops/1',0)",
                [],
            )
            .unwrap();
        resign(&connection);
        let cycle = VerifiedWorkspaceSource::open(&cycle_path).unwrap();
        assert_eq!(
            compare_sources(
                &base,
                &cycle,
                &CompareLimits::default(),
                &CancellationToken::new(),
            )
            .unwrap_err()
            .kind(),
            WorkspaceErrorCode::InvalidContract
        );
    }

    #[test]
    fn exact_sqlite_numeric_ordering_handles_precision_boundaries_and_signed_zero() {
        assert_eq!(
            compare_value(
                &CompareValue::Integer(9_007_199_254_740_993),
                &CompareValue::Real(9_007_199_254_740_992.0),
            )
            .unwrap(),
            Ordering::Greater
        );
        assert_eq!(
            compare_value(
                &CompareValue::Integer(i64::MAX),
                &CompareValue::Real(9_223_372_036_854_775_808.0),
            )
            .unwrap(),
            Ordering::Less
        );
        assert_eq!(
            compare_value(&CompareValue::Real(-0.0), &CompareValue::Integer(0)).unwrap(),
            Ordering::Greater
        );
        assert_eq!(
            compare_value(&CompareValue::Real(-0.0), &CompareValue::Real(0.0)).unwrap(),
            Ordering::Greater
        );
        assert_eq!(
            canonical_compare_key(
                "values",
                &[("id".to_owned(), CompareValue::Real(f64::NAN))],
                8,
            )
            .unwrap_err()
            .kind(),
            WorkspaceErrorCode::InvalidContract
        );
    }

    fn vector_value(value: &serde_json::Value) -> CompareValue {
        match value["type"].as_str().unwrap() {
            "null" => CompareValue::Null,
            "integer" => CompareValue::Integer(value["decimal"].as_str().unwrap().parse().unwrap()),
            "real-bits" => CompareValue::Real(f64::from_bits(
                u64::from_str_radix(value["hex"].as_str().unwrap(), 16).unwrap(),
            )),
            "text" => CompareValue::Text(decode_hex(value["utf8_hex"].as_str().unwrap())),
            "blob" => CompareValue::Blob(decode_hex(value["hex"].as_str().unwrap())),
            _ => panic!("unknown vector value"),
        }
    }

    fn decode_hex(value: &str) -> Vec<u8> {
        value
            .as_bytes()
            .chunks(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }
    fn hex(value: &[u8]) -> String {
        value.iter().map(|byte| format!("{byte:02x}")).collect()
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
                rusqlite::params![
                    envelope.key_id,
                    envelope.public_key.as_slice(),
                    envelope.application_digest.as_slice(),
                    envelope.signature.as_slice(),
                    envelope.signed_at,
                ],
            )
            .unwrap();
    }

    fn insert_parent_claim(
        connection: &Connection,
        ordinal: i64,
        relation: &str,
        file_sha256: &str,
    ) {
        connection
            .execute(
                "INSERT INTO capsule_lineage_parent VALUES \
                 ('33333333-3333-4333-8333-333333333333',?1,?2,NULL,NULL,?3)",
                params![ordinal, relation, file_sha256],
            )
            .unwrap();
    }
}

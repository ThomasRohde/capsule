//! Deterministic, review-only lifecycle plan generation.
//!
//! Generated JSON does not authorize execution. Callers must parse it and
//! construct a [`crate::PreparedPlan`], which reopens and rebinds every source
//! and reserves the destination again.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;
use sqlite_capsule_core::MAX_CAPSULE_BYTES;
use sqlite_capsule_lifecycle::DestinationReservation;

use crate::plan::canonical_digest_value;
use crate::prepared_plan::map_destination_error;
use crate::{LifecyclePlan, VerifiedWorkspaceSource, WorkspaceError, WorkspaceErrorCode};

const HARD_MAX_ROWS_INSPECTED: u64 = 100_000;
const HARD_MAX_ROWS_WRITTEN: u64 = 100_000;
const HARD_MAX_PLAN_DEADLINE: Duration = Duration::from_secs(30);

/// Requested serialized operation budgets. Values are clamped to the host
/// profile before being covered by `plan_digest`.
#[derive(Clone, Debug)]
pub struct DuplicatePlanLimits {
    pub max_input_bytes: u64,
    pub max_output_bytes: u64,
    pub max_rows_inspected: u64,
    pub max_rows_written: u64,
    pub deadline: Duration,
}

impl Default for DuplicatePlanLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: MAX_CAPSULE_BYTES,
            max_output_bytes: MAX_CAPSULE_BYTES,
            max_rows_inspected: HARD_MAX_ROWS_INSPECTED,
            max_rows_written: HARD_MAX_ROWS_WRITTEN,
            deadline: HARD_MAX_PLAN_DEADLINE,
        }
    }
}

/// Explicit values which make duplicate-plan generation reproducible.
pub struct DuplicatePlanRequest<'a> {
    pub output_path: &'a Path,
    pub plan_id: &'a str,
    pub created_at: &'a str,
    pub expires_at: &'a str,
    pub limits: DuplicatePlanLimits,
}

/// Generates deterministic review JSON for an exact duplicate operation.
///
/// This opens the destination parent read-only to bind its stable identity and
/// refuses an existing leaf. It neither creates the output nor retains any
/// execution capability; [`crate::PreparedPlan::prepare`] remains mandatory.
pub fn generate_duplicate_plan(
    source: &VerifiedWorkspaceSource,
    request: &DuplicatePlanRequest<'_>,
) -> Result<LifecyclePlan, WorkspaceError> {
    source.assert_current()?;
    let effective_limits = effective_limits(&request.limits, source.source_identity().bytes)?;
    let source_identity = source.source_identity();
    let identity = source.identity();
    let schema = identity
        .overview
        .data_schema
        .as_ref()
        .ok_or_else(invalid_contract)?;
    let schema_version =
        u64::try_from(schema.data_schema_version).map_err(|_| invalid_contract())?;
    let revision_id = identity
        .overview
        .instance
        .revision_id
        .as_deref()
        .ok_or_else(invalid_contract)?;
    let publisher_key_id = source
        .signature_reports()
        .iter()
        .filter(|report| report.cryptographically_valid && report.digest_matches)
        .map(|report| report.key_id.as_str())
        .min()
        .ok_or_else(|| WorkspaceError::new(WorkspaceErrorCode::InvalidSignature))?;
    let application_digest = lower_hex(source.application_digest());
    let snapshot_sha256 = lower_hex(&source.verified.source_sha256);
    let source_path = utf8_path(&identity.canonical_path)?;

    let output = absolute_output(request.output_path)?;
    let leaf = output
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(invalid_contract)?;
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(invalid_contract)?;
    let reservation = DestinationReservation::reserve(
        parent,
        OsStr::new(leaf),
        std::slice::from_ref(source_identity),
    )
    .map_err(map_destination_error)?;
    let output_path = utf8_path(&reservation.path_hint())?;

    let mut value = json!({
        "profile": crate::PLAN_PROFILE,
        "plan_id": request.plan_id,
        "operation": "duplicate",
        "created_at": request.created_at,
        "expires_at": request.expires_at,
        "inputs": [{
            "role": "source",
            "path_hint": source_path,
            "file_sha256": snapshot_sha256,
            "snapshot_sha256": snapshot_sha256,
            "size_bytes": source_identity.bytes,
            "filesystem_identity": {
                "platform": std::env::consts::OS,
                "volume_or_device": source_identity.device.to_string(),
                "file_id_or_inode": source_identity.stable_file_id,
                "modified_ns": source_identity.modified_ns
            },
            "capsule": {
                "format_version": identity.format_version,
                "capsule_id": identity.capsule_id,
                "revision_id": revision_id,
                "app_id": identity.app_id,
                "app_version": identity.app_version,
                "application_digest": application_digest,
                "data_schema_id": schema.data_schema_id,
                "data_schema_version": schema_version,
                "publisher_key_id": publisher_key_id
            }
        }],
        "output": {
            "path": output_path,
            "leaf_name": leaf,
            "parent_filesystem_identity": {
                "platform": std::env::consts::OS,
                "volume_or_device": reservation.identity().device.to_string(),
                "file_id_or_inode": reservation.identity().stable_file_id
            },
            "must_not_exist": true,
            "publish_mode": "create-new-no-replace"
        },
        "decisions": [{
            "scope": "application",
            "subject": identity.app_id,
            "action": "copy-exact-snapshot",
            "reason": "Duplicate preserves the exact verified Capsule snapshot.",
            "parameters": {}
        }],
        "limits": effective_limits,
        "expected": {
            "capsule_id": identity.capsule_id,
            "revision_id": revision_id,
            "app_id": identity.app_id,
            "application_digest": application_digest,
            "data_schema_id": schema.data_schema_id,
            "data_schema_version": schema_version
        },
        "plan_digest": ""
    });
    let digest = canonical_digest_value(&value)?;
    value["plan_digest"] = serde_json::Value::String(digest);
    let plan = LifecyclePlan::parse(&serde_json::to_vec(&value).map_err(|_| invalid_contract())?)?;

    reservation
        .assert_reserved_current()
        .map_err(map_destination_error)?;
    source.assert_current()?;
    Ok(plan)
}

fn effective_limits(
    requested: &DuplicatePlanLimits,
    source_bytes: u64,
) -> Result<serde_json::Value, WorkspaceError> {
    let max_input_bytes = requested.max_input_bytes.min(MAX_CAPSULE_BYTES);
    let max_output_bytes = requested.max_output_bytes.min(MAX_CAPSULE_BYTES);
    let max_rows_inspected = requested.max_rows_inspected.min(HARD_MAX_ROWS_INSPECTED);
    let max_rows_written = requested.max_rows_written.min(HARD_MAX_ROWS_WRITTEN);
    let deadline = requested.deadline.min(HARD_MAX_PLAN_DEADLINE);
    let deadline_ms = u64::try_from(deadline.as_millis()).map_err(|_| limit_exceeded())?;
    if source_bytes > max_input_bytes
        || source_bytes > max_output_bytes
        || max_rows_inspected == 0
        || max_rows_written == 0
        || deadline_ms == 0
    {
        return Err(limit_exceeded());
    }
    Ok(json!({
        "max_input_bytes": max_input_bytes,
        "max_output_bytes": max_output_bytes,
        "max_rows_inspected": max_rows_inspected,
        "max_rows_written": max_rows_written,
        "deadline_ms": deadline_ms
    }))
}

fn absolute_output(path: &Path) -> Result<PathBuf, WorkspaceError> {
    if path.as_os_str().is_empty() {
        return Err(invalid_contract());
    }
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InternalError))?
            .join(path)
    };
    if path.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir | std::path::Component::CurDir
        )
    }) {
        return Err(invalid_contract());
    }
    Ok(path)
}

fn utf8_path(path: &Path) -> Result<String, WorkspaceError> {
    path.to_str()
        .filter(|value| !value.is_empty() && value.len() <= 4_096)
        .map(str::to_owned)
        .ok_or_else(invalid_contract)
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

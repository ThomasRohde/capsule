//! Strict, deterministic lifecycle-plan parsing and digest verification.
//!
//! This is deliberately not RFC 8785/JCS. The lifecycle contract orders object
//! keys by Unicode scalar value, permits only integral JSON numbers, and hashes
//! the canonical UTF-8 JSON object after removing `plan_digest`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Number, Value};
use sha2::{Digest, Sha256};

use crate::{WorkspaceError, WorkspaceErrorCode};

pub const PLAN_PROFILE: &str = "org.sqlite-capsule.lifecycle-plan/1";
pub const MAX_PLAN_BYTES: usize = 1_048_576;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LifecyclePlan {
    profile: String,
    plan_id: String,
    operation: Operation,
    created_at: String,
    expires_at: String,
    inputs: Vec<PlanInput>,
    output: PlanOutput,
    decisions: Vec<PlanDecision>,
    limits: PlanLimits,
    expected: ExpectedOutput,
    plan_digest: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LifecyclePlanWire {
    profile: String,
    plan_id: String,
    operation: Operation,
    created_at: String,
    expires_at: String,
    inputs: Vec<PlanInput>,
    output: PlanOutput,
    decisions: Vec<PlanDecision>,
    limits: PlanLimits,
    expected: ExpectedOutput,
    plan_digest: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Operation {
    Duplicate,
    CompactDuplicate,
    Fork,
    CreateFromTemplate,
    SelectiveFork,
    ReconcileToCopy,
    ApplicationUpgrade,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanInput {
    role: InputRole,
    path_hint: String,
    file_sha256: String,
    snapshot_sha256: String,
    size_bytes: u64,
    filesystem_identity: SourceFilesystemIdentity,
    capsule: PlanCapsuleIdentity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InputRole {
    Source,
    Target,
    Ancestor,
    Template,
    ApplicationRelease,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceFilesystemIdentity {
    platform: String,
    volume_or_device: String,
    file_id_or_inode: String,
    modified_ns: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanCapsuleIdentity {
    format_version: String,
    #[serde(default)]
    capsule_id: Option<String>,
    #[serde(default)]
    revision_id: Option<String>,
    app_id: String,
    app_version: String,
    #[serde(default)]
    application_digest: Option<String>,
    #[serde(default)]
    data_schema_id: Option<String>,
    #[serde(default)]
    data_schema_version: Option<u64>,
    #[serde(default)]
    publisher_key_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanOutput {
    path: String,
    leaf_name: String,
    parent_filesystem_identity: ParentFilesystemIdentity,
    must_not_exist: bool,
    publish_mode: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParentFilesystemIdentity {
    platform: String,
    volume_or_device: String,
    file_id_or_inode: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanDecision {
    scope: DecisionScope,
    subject: String,
    action: String,
    reason: String,
    #[serde(default)]
    parameters: BTreeMap<String, Value>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DecisionScope {
    Dataset,
    Table,
    Row,
    Field,
    Application,
    Profile,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanLimits {
    max_input_bytes: u64,
    max_output_bytes: u64,
    max_rows_inspected: u64,
    max_rows_written: u64,
    deadline_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedOutput {
    #[serde(default)]
    capsule_id: Option<String>,
    #[serde(default)]
    revision_id: Option<String>,
    app_id: String,
    #[serde(default)]
    application_digest: Option<String>,
    #[serde(default)]
    data_schema_id: Option<String>,
    #[serde(default)]
    data_schema_version: Option<u64>,
}

impl LifecyclePlan {
    /// Parses a complete untrusted plan and verifies its embedded digest.
    pub fn parse(bytes: &[u8]) -> Result<Self, WorkspaceError> {
        if bytes.is_empty() || bytes.len() > MAX_PLAN_BYTES {
            return Err(invalid_plan());
        }
        let mut deserializer = serde_json::Deserializer::from_slice(bytes);
        let strict = StrictValue::deserialize(&mut deserializer).map_err(|_| invalid_plan())?;
        deserializer.end().map_err(|_| invalid_plan())?;
        validate_required_members(&strict.0)?;
        let wire: LifecyclePlanWire =
            serde_json::from_value(strict.0.clone()).map_err(|_| invalid_plan())?;
        let plan = Self {
            profile: wire.profile,
            plan_id: wire.plan_id,
            operation: wire.operation,
            created_at: wire.created_at,
            expires_at: wire.expires_at,
            inputs: wire.inputs,
            output: wire.output,
            decisions: wire.decisions,
            limits: wire.limits,
            expected: wire.expected,
            plan_digest: wire.plan_digest,
        };
        plan.validate()?;
        let actual = canonical_digest_value(&strict.0)?;
        if !constant_time_equal(plan.plan_digest.as_bytes(), actual.as_bytes()) {
            return Err(invalid_plan());
        }
        Ok(plan)
    }

    pub fn operation(&self) -> Operation {
        self.operation
    }

    pub fn plan_id(&self) -> &str {
        &self.plan_id
    }

    pub fn plan_digest(&self) -> &str {
        &self.plan_digest
    }

    pub fn expires_at(&self) -> &str {
        &self.expires_at
    }

    pub(crate) fn inputs(&self) -> &[PlanInput] {
        &self.inputs
    }

    pub(crate) fn output(&self) -> &PlanOutput {
        &self.output
    }

    pub(crate) fn expected(&self) -> &ExpectedOutput {
        &self.expected
    }

    pub(crate) fn created_at(&self) -> &str {
        &self.created_at
    }

    pub(crate) fn limits(&self) -> &PlanLimits {
        &self.limits
    }

    pub(crate) fn decisions(&self) -> &[PlanDecision] {
        &self.decisions
    }

    /// Returns the canonical plan bytes including the verified digest.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, WorkspaceError> {
        let value = serde_json::to_value(self).map_err(|_| invalid_plan())?;
        canonical_json(&value)
    }

    fn validate(&self) -> Result<(), WorkspaceError> {
        if self.profile != PLAN_PROFILE
            || !valid_uuid(&self.plan_id)
            || !utc_seconds(&self.created_at)
            || !utc_seconds(&self.expires_at)
            || self.expires_at <= self.created_at
            || self.inputs.is_empty()
            || self.inputs.len() > 8
            || self.decisions.len() > 2_048
            || !sha256(&self.plan_digest)
        {
            return Err(invalid_plan());
        }
        if self.inputs.iter().any(|input| {
            input
                .capsule
                .data_schema_version
                .is_some_and(|value| value > i64::MAX as u64)
        }) || self
            .expected
            .data_schema_version
            .is_some_and(|value| value > i64::MAX as u64)
        {
            return Err(invalid_plan());
        }
        for input in &self.inputs {
            input.validate()?;
        }
        self.output.validate()?;
        for decision in &self.decisions {
            decision.validate()?;
        }
        self.limits.validate()?;
        self.expected.validate()?;
        Ok(())
    }
}

impl PlanInput {
    pub(crate) fn role(&self) -> InputRole {
        self.role
    }

    pub(crate) fn path_hint(&self) -> &str {
        &self.path_hint
    }

    pub(crate) fn file_sha256(&self) -> &str {
        &self.file_sha256
    }

    pub(crate) fn snapshot_sha256(&self) -> &str {
        &self.snapshot_sha256
    }

    pub(crate) fn size_bytes(&self) -> u64 {
        self.size_bytes
    }

    pub(crate) fn filesystem_identity(&self) -> &SourceFilesystemIdentity {
        &self.filesystem_identity
    }

    pub(crate) fn capsule(&self) -> &PlanCapsuleIdentity {
        &self.capsule
    }

    fn validate(&self) -> Result<(), WorkspaceError> {
        if !bounded(&self.path_hint, 1, 4_096)
            || !sha256(&self.file_sha256)
            || !sha256(&self.snapshot_sha256)
            || !bounded(&self.filesystem_identity.platform, 1, 64)
            || !bounded(&self.filesystem_identity.volume_or_device, 1, 256)
            || !bounded(&self.filesystem_identity.file_id_or_inode, 1, 256)
        {
            return Err(invalid_plan());
        }
        self.capsule.validate()
    }
}

impl PlanCapsuleIdentity {
    fn validate(&self) -> Result<(), WorkspaceError> {
        if !bounded(&self.format_version, 1, 64)
            || !bounded(&self.app_id, 1, 512)
            || !bounded(&self.app_version, 1, 128)
            || !optional_bounded(&self.capsule_id, 512)
            || !optional_bounded(&self.revision_id, 512)
            || !optional_bounded(&self.data_schema_id, 512)
            || !optional_bounded(&self.publisher_key_id, 1_024)
            || self
                .application_digest
                .as_deref()
                .is_some_and(|value| !sha256(value))
            || self.data_schema_version == Some(0)
        {
            return Err(invalid_plan());
        }
        Ok(())
    }
}

impl PlanOutput {
    fn validate(&self) -> Result<(), WorkspaceError> {
        let path = Path::new(&self.path);
        let path_leaf = path.file_name().and_then(|name| name.to_str());
        if !bounded(&self.path, 1, 4_096)
            || !valid_leaf(&self.leaf_name)
            || path_leaf != Some(self.leaf_name.as_str())
            || !self.must_not_exist
            || self.publish_mode != "create-new-no-replace"
            || !bounded(&self.parent_filesystem_identity.platform, 1, 64)
            || !bounded(&self.parent_filesystem_identity.volume_or_device, 1, 256)
            || !bounded(&self.parent_filesystem_identity.file_id_or_inode, 1, 256)
        {
            return Err(invalid_plan());
        }
        Ok(())
    }
}

impl SourceFilesystemIdentity {
    pub(crate) fn platform(&self) -> &str {
        &self.platform
    }

    pub(crate) fn volume_or_device(&self) -> &str {
        &self.volume_or_device
    }

    pub(crate) fn file_id_or_inode(&self) -> &str {
        &self.file_id_or_inode
    }

    pub(crate) fn modified_ns(&self) -> u64 {
        self.modified_ns
    }
}

impl PlanCapsuleIdentity {
    pub(crate) fn format_version(&self) -> &str {
        &self.format_version
    }

    pub(crate) fn app_id(&self) -> &str {
        &self.app_id
    }

    pub(crate) fn publisher_key_id(&self) -> Option<&str> {
        self.publisher_key_id.as_deref()
    }

    pub(crate) fn app_version(&self) -> &str {
        &self.app_version
    }

    pub(crate) fn capsule_id(&self) -> Option<&str> {
        self.capsule_id.as_deref()
    }

    pub(crate) fn revision_id(&self) -> Option<&str> {
        self.revision_id.as_deref()
    }

    pub(crate) fn application_digest(&self) -> Option<&str> {
        self.application_digest.as_deref()
    }

    pub(crate) fn data_schema_id(&self) -> Option<&str> {
        self.data_schema_id.as_deref()
    }

    pub(crate) fn data_schema_version(&self) -> Option<u64> {
        self.data_schema_version
    }
}

impl PlanOutput {
    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    pub(crate) fn leaf_name(&self) -> &str {
        &self.leaf_name
    }

    pub(crate) fn parent_identity(&self) -> &ParentFilesystemIdentity {
        &self.parent_filesystem_identity
    }
}

impl ParentFilesystemIdentity {
    pub(crate) fn platform(&self) -> &str {
        &self.platform
    }

    pub(crate) fn volume_or_device(&self) -> &str {
        &self.volume_or_device
    }

    pub(crate) fn file_id_or_inode(&self) -> &str {
        &self.file_id_or_inode
    }
}

impl PlanLimits {
    pub(crate) fn deadline_ms(&self) -> u64 {
        self.deadline_ms
    }

    pub(crate) fn max_input_bytes(&self) -> u64 {
        self.max_input_bytes
    }

    pub(crate) fn max_output_bytes(&self) -> u64 {
        self.max_output_bytes
    }

    pub(crate) fn row_budgets_within_duplicate_profile(&self) -> bool {
        self.max_rows_inspected <= 100_000 && self.max_rows_written <= 100_000
    }
}

impl ExpectedOutput {
    pub(crate) fn app_id(&self) -> &str {
        &self.app_id
    }

    pub(crate) fn capsule_id(&self) -> Option<&str> {
        self.capsule_id.as_deref()
    }

    pub(crate) fn revision_id(&self) -> Option<&str> {
        self.revision_id.as_deref()
    }

    pub(crate) fn application_digest(&self) -> Option<&str> {
        self.application_digest.as_deref()
    }

    pub(crate) fn data_schema_id(&self) -> Option<&str> {
        self.data_schema_id.as_deref()
    }

    pub(crate) fn data_schema_version(&self) -> Option<u64> {
        self.data_schema_version
    }
}

impl PlanDecision {
    pub(crate) fn scope(&self) -> DecisionScope {
        self.scope
    }

    pub(crate) fn action(&self) -> &str {
        &self.action
    }

    pub(crate) fn subject(&self) -> &str {
        &self.subject
    }

    pub(crate) fn parameters_are_empty(&self) -> bool {
        self.parameters.is_empty()
    }

    fn validate(&self) -> Result<(), WorkspaceError> {
        if !bounded(&self.subject, 1, 2_048)
            || !bounded(&self.action, 1, 128)
            || !bounded(&self.reason, 1, 4_096)
            || self.parameters.len() > 128
        {
            return Err(invalid_plan());
        }
        for (key, value) in &self.parameters {
            if !valid_parameter_key(key)
                || !matches!(
                    value,
                    Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)
                )
                || value
                    .as_str()
                    .is_some_and(|text| text.chars().count() > 4_096)
                || value
                    .as_u64()
                    .is_some_and(|number| number > i64::MAX as u64)
            {
                return Err(invalid_plan());
            }
        }
        Ok(())
    }
}

impl PlanLimits {
    fn validate(&self) -> Result<(), WorkspaceError> {
        if self.max_input_bytes == 0
            || self.max_output_bytes == 0
            || self.max_rows_inspected == 0
            || self.max_rows_written == 0
            || !(1..=3_600_000).contains(&self.deadline_ms)
        {
            return Err(invalid_plan());
        }
        Ok(())
    }
}

impl ExpectedOutput {
    fn validate(&self) -> Result<(), WorkspaceError> {
        if !optional_bounded(&self.capsule_id, 512)
            || !optional_bounded(&self.revision_id, 512)
            || !bounded(&self.app_id, 1, 512)
            || self
                .application_digest
                .as_deref()
                .is_some_and(|value| !sha256(value))
            || !optional_bounded(&self.data_schema_id, 512)
            || self.data_schema_version == Some(0)
        {
            return Err(invalid_plan());
        }
        Ok(())
    }
}

/// Computes the plan digest from an already parsed JSON object.
///
/// Callers accepting untrusted bytes must use [`LifecyclePlan::parse`] so that
/// duplicate keys and non-integral numbers are rejected before this function.
pub(crate) fn canonical_digest_value(value: &Value) -> Result<String, WorkspaceError> {
    let mut unsigned = value.clone();
    let object = unsigned.as_object_mut().ok_or_else(invalid_plan)?;
    if object.remove("plan_digest").is_none() {
        return Err(invalid_plan());
    }
    let bytes = canonical_json(&unsigned)?;
    let digest = Sha256::digest(bytes);
    Ok(lower_hex(&digest))
}

pub(crate) fn canonical_json(value: &Value) -> Result<Vec<u8>, WorkspaceError> {
    // serde_json's default Map is a BTreeMap. Rust string ordering follows the
    // UTF-8 encoding, whose lexicographic order preserves Unicode scalar order.
    serde_json::to_vec(value).map_err(|_| invalid_plan())
}

pub(crate) fn strict_json(bytes: &[u8]) -> Result<Value, WorkspaceError> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let strict = StrictValue::deserialize(&mut deserializer).map_err(|_| invalid_plan())?;
    deserializer.end().map_err(|_| invalid_plan())?;
    Ok(strict.0)
}

fn validate_required_members(value: &Value) -> Result<(), WorkspaceError> {
    let root = value.as_object().ok_or_else(invalid_plan)?;
    let inputs = root
        .get("inputs")
        .and_then(Value::as_array)
        .ok_or_else(invalid_plan)?;
    for input in inputs {
        let input = input.as_object().ok_or_else(invalid_plan)?;
        require_keys(
            input,
            &[
                "role",
                "path_hint",
                "file_sha256",
                "snapshot_sha256",
                "size_bytes",
                "filesystem_identity",
                "capsule",
            ],
        )?;
        let capsule = input
            .get("capsule")
            .and_then(Value::as_object)
            .ok_or_else(invalid_plan)?;
        require_keys(
            capsule,
            &[
                "format_version",
                "app_id",
                "app_version",
                "application_digest",
                "data_schema_id",
                "data_schema_version",
            ],
        )?;
    }
    let expected = root
        .get("expected")
        .and_then(Value::as_object)
        .ok_or_else(invalid_plan)?;
    require_keys(
        expected,
        &[
            "capsule_id",
            "revision_id",
            "app_id",
            "application_digest",
            "data_schema_id",
            "data_schema_version",
        ],
    )
}

fn require_keys(object: &Map<String, Value>, keys: &[&str]) -> Result<(), WorkspaceError> {
    if keys.iter().all(|key| object.contains_key(*key)) {
        Ok(())
    } else {
        Err(invalid_plan())
    }
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}

fn sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn bounded(value: &str, minimum: usize, maximum: usize) -> bool {
    let length = value.chars().count();
    (minimum..=maximum).contains(&length) && value.len() <= maximum.saturating_mul(4)
}

fn optional_bounded(value: &Option<String>, maximum: usize) -> bool {
    value
        .as_deref()
        .is_none_or(|text| bounded(text, 1, maximum))
}

fn valid_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && [8, 13, 18, 23]
            .into_iter()
            .all(|index| bytes[index] == b'-')
        && bytes.iter().enumerate().all(|(index, byte)| {
            [8, 13, 18, 23].contains(&index)
                || byte.is_ascii_digit()
                || (b'a'..=b'f').contains(byte)
        })
        && matches!(bytes[14], b'1'..=b'5')
        && matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
}

fn utc_seconds(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'Z'
        || ![0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18]
            .into_iter()
            .all(|index| bytes[index].is_ascii_digit())
    {
        return false;
    }
    let digits = |start: usize, end: usize| -> u32 {
        bytes[start..end]
            .iter()
            .fold(0, |number, byte| number * 10 + u32::from(byte - b'0'))
    };
    let year = digits(0, 4);
    let month = digits(5, 7);
    let day = digits(8, 10);
    let hour = digits(11, 13);
    let minute = digits(14, 16);
    let second = digits(17, 19);
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    year != 0 && (1..=days).contains(&day) && hour < 24 && minute < 60 && second < 60
}

fn valid_parameter_key(value: &str) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == b'_')
        && value.len() <= 128
        && bytes
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':' | b'-'))
}

fn valid_leaf(value: &str) -> bool {
    if value.is_empty()
        || value.len() > 255
        || value == "."
        || value == ".."
        || value.ends_with([' ', '.'])
        || value.chars().any(|character| {
            character.is_control()
                || matches!(
                    character,
                    '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
                )
        })
    {
        return false;
    }
    let stem = value
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches([' ', '.'])
        .to_ascii_uppercase();
    !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !(stem.starts_with("COM") || stem.starts_with("LPT"))
            .then(|| stem.chars().nth(3))
            .flatten()
            .is_some_and(|suffix| {
                stem.chars().count() == 4 && matches!(suffix, '1'..='9' | '¹' | '²' | '³')
            })
}

const fn invalid_plan() -> WorkspaceError {
    WorkspaceError::new(WorkspaceErrorCode::InvalidContract)
}

/// A recursive JSON value that rejects duplicate object members and floating
/// point numbers while parsing, before serde can discard those distinctions.
struct StrictValue(Value);

impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(StrictValueVisitor)
    }
}

struct StrictValueVisitor;

impl<'de> Visitor<'de> for StrictValueVisitor {
    type Value = StrictValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("JSON without duplicate keys or floating-point numbers")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Number(Number::from(value))))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Number(Number::from(value))))
    }

    fn visit_f64<E>(self, _value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Err(E::custom("floating-point numbers are forbidden"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(StrictValue(Value::String(value.to_owned())))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::String(value)))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Null))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Null))
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element::<StrictValue>()? {
            values.push(value.0);
        }
        Ok(StrictValue(Value::Array(values)))
    }

    fn visit_map<A>(self, mut object: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut keys = BTreeSet::new();
        let mut values = Map::new();
        while let Some(key) = object.next_key::<String>()? {
            if !keys.insert(key.clone()) {
                return Err(de::Error::custom("duplicate object member"));
            }
            values.insert(key, object.next_value::<StrictValue>()?.0);
        }
        Ok(StrictValue(Value::Object(values)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan_value() -> Value {
        serde_json::json!({
            "profile": PLAN_PROFILE,
            "plan_id": "c5f44498-1a23-4c15-a384-e8a4782d0984",
            "operation": "duplicate",
            "created_at": "2026-08-12T12:00:00Z",
            "expires_at": "2026-08-12T12:05:00Z",
            "inputs": [{
                "role": "source",
                "path_hint": "source.capsule.sqlite",
                "file_sha256": "11".repeat(32),
                "snapshot_sha256": "22".repeat(32),
                "size_bytes": 4096,
                "filesystem_identity": {
                    "platform": "windows",
                    "volume_or_device": "volume:1",
                    "file_id_or_inode": "file:2",
                    "modified_ns": 123
                },
                "capsule": {
                    "format_version": "0.3",
                    "capsule_id": "c7e6586e-d489-48d0-b1a0-e98499fb542a",
                    "revision_id": "80b1d11e-76fe-45be-a941-a448614d1a59",
                    "app_id": "org.example.app",
                    "app_version": "1.0.0",
                    "application_digest": "33".repeat(32),
                    "data_schema_id": "org.example.data",
                    "data_schema_version": 1,
                    "publisher_key_id": "ed25519:sha256:test"
                }
            }],
            "output": {
                "path": "Copy.capsule.sqlite",
                "leaf_name": "Copy.capsule.sqlite",
                "parent_filesystem_identity": {
                    "platform": "windows",
                    "volume_or_device": "volume:1",
                    "file_id_or_inode": "directory:2"
                },
                "must_not_exist": true,
                "publish_mode": "create-new-no-replace"
            },
            "decisions": [{
                "scope": "dataset",
                "subject": "content",
                "action": "copy",
                "reason": "User-owned content is retained.",
                "parameters": {"count": 1, "enabled": true}
            }],
            "limits": {
                "max_input_bytes": 67108864,
                "max_output_bytes": 67108864,
                "max_rows_inspected": 100000,
                "max_rows_written": 100000,
                "deadline_ms": 300000
            },
            "expected": {
                "capsule_id": "c7e6586e-d489-48d0-b1a0-e98499fb542a",
                "revision_id": "e3e10345-524c-4e02-a16e-2ed513bd4638",
                "app_id": "org.example.app",
                "application_digest": "33".repeat(32),
                "data_schema_id": "org.example.data",
                "data_schema_version": 1
            },
            "plan_digest": "00".repeat(32)
        })
    }

    fn sealed_plan() -> Vec<u8> {
        let mut value = plan_value();
        let digest = canonical_digest_value(&value).expect("digest");
        value["plan_digest"] = Value::String(digest);
        serde_json::to_vec(&value).expect("JSON")
    }

    #[test]
    fn plan_digest_is_deterministic_and_verified() {
        let bytes = sealed_plan();
        let first = LifecyclePlan::parse(&bytes).expect("valid plan");
        let second = LifecyclePlan::parse(&bytes).expect("same plan");
        assert_eq!(first.plan_digest(), second.plan_digest());
        assert_eq!(first.canonical_bytes().expect("canonical"), bytes);

        let mut changed: Value = serde_json::from_slice(&bytes).expect("value");
        changed["output"]["leaf_name"] = Value::String("Changed.capsule.sqlite".into());
        assert_eq!(
            LifecyclePlan::parse(&serde_json::to_vec(&changed).expect("JSON"))
                .expect_err("digest binding")
                .kind(),
            WorkspaceErrorCode::InvalidContract
        );
    }

    #[test]
    fn rejects_duplicate_keys_floats_and_unsafe_publication_modes() {
        let bytes = sealed_plan();
        let text = String::from_utf8(bytes).expect("UTF-8");
        let duplicate = text.replacen(
            "\"profile\":",
            "\"profile\":\"org.sqlite-capsule.lifecycle-plan/1\",\"profile\":",
            1,
        );
        assert!(LifecyclePlan::parse(duplicate.as_bytes()).is_err());

        let float = text.replacen("\"deadline_ms\":300000", "\"deadline_ms\":1.5", 1);
        assert!(LifecyclePlan::parse(float.as_bytes()).is_err());

        let unsafe_mode = text.replacen("create-new-no-replace", "atomic-rename", 1);
        assert!(LifecyclePlan::parse(unsafe_mode.as_bytes()).is_err());
    }

    #[test]
    fn canonical_key_order_is_unicode_scalar_order_not_utf16_jcs_order() {
        let value = serde_json::json!({
            "plan_digest": "00".repeat(32),
            "\u{10000}": 1,
            "\u{e000}": 2
        });
        let mut unsigned = value;
        unsigned
            .as_object_mut()
            .expect("object")
            .remove("plan_digest");
        let bytes = canonical_json(&unsigned).expect("canonical JSON");
        let text = String::from_utf8(bytes).expect("UTF-8");
        assert!(
            text.find('\u{e000}').expect("BMP key") < text.find('\u{10000}').expect("astral key")
        );
    }

    #[test]
    fn rust_matches_the_external_python_canonical_vectors() {
        let plan_bytes =
            include_bytes!("../../../../compatibility/lifecycle-plan-v1/vector-plan.json");
        let parsed = LifecyclePlan::parse(plan_bytes).expect("external lifecycle plan vector");
        let canonical = parsed.canonical_bytes().expect("canonical plan");
        assert_eq!(plan_bytes.as_slice(), canonical.as_slice());

        let vectors: Value = serde_json::from_str(include_str!(
            "../../../../compatibility/lifecycle-plan-v1/vectors.json"
        ))
        .expect("external vector catalogue");
        for case in vectors["canonical_cases"]
            .as_array()
            .expect("canonical cases")
        {
            let bytes = canonical_json(&case["value"]).expect("canonical case");
            assert_eq!(
                String::from_utf8(bytes.clone()).expect("UTF-8"),
                case["canonical"].as_str().expect("canonical bytes")
            );
            assert_eq!(
                lower_hex(&Sha256::digest(bytes)),
                case["sha256"].as_str().expect("case digest")
            );
        }
    }

    #[test]
    fn required_nullable_members_may_be_null_but_may_not_be_missing() {
        let bytes = sealed_plan();
        let mut value: Value = serde_json::from_slice(&bytes).expect("plan value");
        value["inputs"][0].as_object_mut().expect("input")["capsule"]
            .as_object_mut()
            .expect("capsule")
            .remove("application_digest");
        let digest = canonical_digest_value(&value).expect("digest changed wire plan");
        value["plan_digest"] = Value::String(digest);
        assert!(LifecyclePlan::parse(&serde_json::to_vec(&value).expect("JSON")).is_err());

        let mut value: Value = serde_json::from_slice(&bytes).expect("plan value");
        value["expected"]
            .as_object_mut()
            .expect("expected")
            .remove("revision_id");
        let digest = canonical_digest_value(&value).expect("digest changed wire plan");
        value["plan_digest"] = Value::String(digest);
        assert!(LifecyclePlan::parse(&serde_json::to_vec(&value).expect("JSON")).is_err());
    }
}

use serde::Serialize;
use std::fmt;

pub const ERROR_PROFILE: &str = "org.sqlite-capsule.lifecycle-errors/1";

macro_rules! error_catalogue {
    ($($variant:ident => ($code:literal, $category:literal, $retryable:literal, $detail:literal)),+ $(,)?) => {
        /// Stable lifecycle error catalogue. All user-facing fields are
        /// derived here; internal database, path, SQL and row details are not
        /// represented by the serializable envelope.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum WorkspaceErrorCode { $($variant),+ }

        impl WorkspaceErrorCode {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            pub const fn code(self) -> &'static str {
                match self { $(Self::$variant => $code),+ }
            }

            pub const fn category(self) -> &'static str {
                match self { $(Self::$variant => $category),+ }
            }

            pub const fn retryable(self) -> bool {
                match self { $(Self::$variant => $retryable),+ }
            }

            pub const fn safe_detail(self) -> &'static str {
                match self { $(Self::$variant => $detail),+ }
            }
        }
    };
}

error_catalogue! {
    UnsupportedFormat => (
        "unsupported_format", "compatibility", false,
        "The capsule format or profile is not supported by this host."
    ),
    UnsupportedOperation => (
        "unsupported_operation", "compatibility", false,
        "This lifecycle operation is not supported for the selected capsule profile."
    ),
    InvalidCapsule => (
        "invalid_capsule", "input", false,
        "The capsule failed structural or integrity inspection."
    ),
    InvalidSignature => (
        "invalid_signature", "trust", false,
        "The signed application could not be verified."
    ),
    InvalidContract => (
        "invalid_contract", "input", false,
        "The signed contract or serialized lifecycle plan is invalid."
    ),
    UndeclaredTable => (
        "undeclared_table", "contract", false,
        "A table required for this operation is not covered by the signed data contract."
    ),
    MissingPrimaryKey => (
        "missing_primary_key", "contract", false,
        "A compared or reconciled table lacks a supported stable primary key."
    ),
    UnsupportedCollation => (
        "unsupported_collation", "contract", false,
        "A declared key uses a collation for which deterministic comparison is unavailable."
    ),
    SensitiveConfirmationRequired => (
        "sensitive_confirmation_required", "policy", true,
        "The selected operation includes sensitive data and requires explicit confirmation."
    ),
    IncompatibleApplication => (
        "incompatible_application", "compatibility", false,
        "The capsules use different application identities."
    ),
    IncompatibleSchema => (
        "incompatible_schema", "compatibility", false,
        "The data schemas are not directly compatible."
    ),
    PublisherMismatch => (
        "publisher_mismatch", "trust", false,
        "The target release is not signed by the accepted publisher identity."
    ),
    CapabilityReviewRequired => (
        "capability_review_required", "trust", true,
        "The target release requests changed capabilities that require review."
    ),
    VersionNotNewer => (
        "version_not_newer", "compatibility", false,
        "The target application version is not strictly newer under SemVer precedence."
    ),
    MigrationPathMissing => (
        "migration_path_missing", "migration", false,
        "No verified migration path exists between the data schema versions."
    ),
    MigrationPathAmbiguous => (
        "migration_path_ambiguous", "migration", false,
        "More than one migration path exists; the host will not choose implicitly."
    ),
    MigrationOperationUnsupported => (
        "migration_operation_unsupported", "migration", false,
        "The migration contains an operation outside the supported declarative profile."
    ),
    MigrationAssertionFailed => (
        "migration_assertion_failed", "migration", false,
        "A required migration precondition or postcondition failed."
    ),
    StalePlan => (
        "stale_plan", "concurrency", true,
        "An input changed after the operation was reviewed. Prepare a new plan."
    ),
    SessionExpired => (
        "session_expired", "concurrency", true,
        "The prepared plan or comparison session expired. Prepare it again."
    ),
    SourceJournalStateUnsupported => (
        "source_journal_state_unsupported", "concurrency", true,
        "The source has active or recoverable SQLite journal state and cannot be snapshotted without changing it."
    ),
    DestinationExists => (
        "destination_exists", "filesystem", true,
        "The chosen destination already exists. Select a new path."
    ),
    DestinationAliasesInput => (
        "destination_aliases_input", "filesystem", true,
        "The destination refers to an input file or alias."
    ),
    OutputPublishFailed => (
        "output_publish_failed", "filesystem", true,
        "The verified temporary output could not be published safely."
    ),
    LimitExceeded => (
        "limit_exceeded", "resource", true,
        "The operation exceeded a configured row, byte or time limit."
    ),
    Cancelled => (
        "cancelled", "operation", true,
        "The operation was cancelled before publication."
    ),
    ConflictsUnresolved => (
        "conflicts_unresolved", "reconcile", true,
        "All reconciliation conflicts must be resolved before execution."
    ),
    RowPreconditionFailed => (
        "row_precondition_failed", "reconcile", true,
        "A reviewed target row changed. Refresh the comparison and plan."
    ),
    ImmutableColumn => (
        "immutable_column", "reconcile", false,
        "The selected change would modify a column declared immutable by the signed data contract."
    ),
    SignatureChanged => (
        "signature_changed", "verification", false,
        "The output application digest does not match the expected release."
    ),
    VerificationFailed => (
        "verification_failed", "verification", false,
        "The output failed required verification and was not published."
    ),
    PostpublishVerificationFailed => (
        "postpublish_verification_failed", "verification", false,
        "The published file could not be reopened safely and has been quarantined rather than reported as success."
    ),
    InternalError => (
        "internal_error", "internal", false,
        "The host could not complete the operation. Inputs were not changed."
    ),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WorkspaceError {
    pub profile: &'static str,
    pub code: &'static str,
    pub category: &'static str,
    pub retryable: bool,
    pub safe_detail: &'static str,
    #[serde(skip)]
    kind: WorkspaceErrorCode,
}

impl WorkspaceError {
    pub const fn new(kind: WorkspaceErrorCode) -> Self {
        Self {
            profile: ERROR_PROFILE,
            code: kind.code(),
            category: kind.category(),
            retryable: kind.retryable(),
            safe_detail: kind.safe_detail(),
            kind,
        }
    }

    pub const fn kind(&self) -> WorkspaceErrorCode {
        self.kind
    }
}

impl fmt::Display for WorkspaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.safe_detail)
    }
}

impl std::error::Error for WorkspaceError {}

#[cfg(test)]
mod tests {
    use super::*;

    const CATALOGUE: &str = include_str!(
        "../../../../docs/plans/capsule-lifecycle/contracts/lifecycle-error-codes-v1.json"
    );

    #[test]
    fn serialized_error_contains_only_stable_safe_fields() {
        let error = WorkspaceError::new(WorkspaceErrorCode::InvalidSignature);
        assert_eq!(
            serde_json::to_value(error).expect("serialize safe error"),
            serde_json::json!({
                "profile": "org.sqlite-capsule.lifecycle-errors/1",
                "code": "invalid_signature",
                "category": "trust",
                "retryable": false,
                "safe_detail": "The signed application could not be verified."
            })
        );
    }

    #[test]
    fn rust_error_catalogue_exactly_matches_the_versioned_json_contract() {
        let document: serde_json::Value = serde_json::from_str(CATALOGUE).expect("catalogue JSON");
        assert_eq!(document["profile"], ERROR_PROFILE);
        let declared = document["errors"]
            .as_array()
            .expect("error catalogue array");
        assert_eq!(declared.len(), WorkspaceErrorCode::ALL.len());
        for (json, rust) in declared.iter().zip(WorkspaceErrorCode::ALL) {
            assert_eq!(json["code"], rust.code());
            assert_eq!(json["category"], rust.category());
            assert_eq!(json["retryable"], rust.retryable());
            assert_eq!(json["safe_detail"], rust.safe_detail());
        }
    }
}

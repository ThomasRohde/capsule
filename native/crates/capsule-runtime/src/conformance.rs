use rusqlite::Connection;
use sqlite_capsule_launch::{run_declared_checks_connection, verify_conformance_connection};

use crate::{CheckResult, RuntimeError, VerificationReport};

pub(crate) fn verify(
    connection: &Connection,
    identity: &sqlite_capsule_core::CapsuleIdentity,
) -> Result<VerificationReport, RuntimeError> {
    // Re-run the shared non-executing phase on the exact connection that will
    // back the runtime. This closes the path-level inspection/open race before
    // any asset or protocol session can be released.
    verify_conformance_connection(connection, identity)?;

    // Declared checks are a separate phase: unlike declaration compilation in
    // the shared verifier, these read-only application queries are executed
    // under the existing authorizer, progress deadline, and result bounds.
    let check_results: Vec<_> = run_declared_checks_connection(connection)?
        .into_iter()
        .map(|result| CheckResult {
            id: result.id,
            severity: result.severity,
            passed: result.passed,
            detail: result.detail,
        })
        .collect();
    let errors: Vec<_> = check_results
        .iter()
        .filter(|result| !result.passed && result.severity == "error")
        .map(|result| format!("check {} failed: {}", result.id, result.detail))
        .collect();
    if !errors.is_empty() {
        return Err(RuntimeError::Verification(errors.join(" | ")));
    }
    Ok(VerificationReport { check_results })
}

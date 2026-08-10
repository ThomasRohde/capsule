//! Offline Sigstore bundle verification for host-update artifacts.
//!
//! The verifier uses the production trust root embedded by the pinned Sigstore
//! library. It performs no network refresh and exposes no signing, download,
//! staging, or installer API.

use serde::Serialize;
use sha2::{Digest, Sha256};
use sigstore_verify::{
    VerificationPolicy,
    trust_root::{SIGSTORE_PRODUCTION_TRUSTED_ROOT, TrustedRoot},
    types::Bundle,
    verify,
};
use thiserror::Error;

pub const MAX_SIGSTORE_BUNDLE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_SIGSTORE_ARTIFACT_BYTES: usize = 512 * 1024 * 1024;
pub const TRUST_ROOT_PROFILE: &str = "sigstore-public-good-embedded/0.11.0";

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SigstoreVerificationReport {
    certificate_identity: String,
    oidc_issuer: String,
    integrated_time_unix: i64,
    trust_root_profile: String,
    trust_root_sha256: String,
    certificate_chain_verified: bool,
    signed_certificate_timestamp_verified: bool,
    transparency_log_verified: bool,
    artifact_signature_verified: bool,
    artifact_bytes: u64,
    artifact_sha256: String,
}

impl SigstoreVerificationReport {
    pub fn certificate_identity(&self) -> &str {
        &self.certificate_identity
    }

    pub fn oidc_issuer(&self) -> &str {
        &self.oidc_issuer
    }

    pub fn integrated_time_unix(&self) -> i64 {
        self.integrated_time_unix
    }

    pub fn trust_root_profile(&self) -> &str {
        &self.trust_root_profile
    }

    pub fn trust_root_sha256(&self) -> &str {
        &self.trust_root_sha256
    }

    pub fn certificate_chain_verified(&self) -> bool {
        self.certificate_chain_verified
    }

    pub fn signed_certificate_timestamp_verified(&self) -> bool {
        self.signed_certificate_timestamp_verified
    }

    pub fn transparency_log_verified(&self) -> bool {
        self.transparency_log_verified
    }

    pub fn artifact_signature_verified(&self) -> bool {
        self.artifact_signature_verified
    }

    pub fn artifact_bytes(&self) -> u64 {
        self.artifact_bytes
    }

    pub fn artifact_sha256(&self) -> &str {
        &self.artifact_sha256
    }

    pub fn all_evidence_verified(&self) -> bool {
        self.certificate_chain_verified
            && self.signed_certificate_timestamp_verified
            && self.transparency_log_verified
            && self.artifact_signature_verified
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SigstoreVerificationError {
    #[error("Sigstore policy field or input bound is invalid")]
    Field,
    #[error("Sigstore bundle is malformed or unsupported")]
    Bundle,
    #[error("embedded Sigstore trust root is unavailable")]
    TrustRoot,
    #[error("Sigstore signature, certificate, identity, or transparency proof failed")]
    Verification,
    #[error("Sigstore verification returned a non-fatal warning and was rejected")]
    Warning,
}

pub fn verify_sigstore_bundle(
    artifact_bytes: &[u8],
    bundle_bytes: &[u8],
    certificate_identity: &str,
    oidc_issuer: &str,
) -> Result<SigstoreVerificationReport, SigstoreVerificationError> {
    if artifact_bytes.is_empty()
        || artifact_bytes.len() > MAX_SIGSTORE_ARTIFACT_BYTES
        || bundle_bytes.is_empty()
        || bundle_bytes.len() > MAX_SIGSTORE_BUNDLE_BYTES
        || !valid_identity(certificate_identity)
        || !valid_issuer(oidc_issuer)
    {
        return Err(SigstoreVerificationError::Field);
    }
    let bundle_json =
        std::str::from_utf8(bundle_bytes).map_err(|_| SigstoreVerificationError::Bundle)?;
    let bundle = Bundle::from_json(bundle_json).map_err(|_| SigstoreVerificationError::Bundle)?;
    let trusted_root = TrustedRoot::from_json(SIGSTORE_PRODUCTION_TRUSTED_ROOT)
        .map_err(|_| SigstoreVerificationError::TrustRoot)?;
    let policy = VerificationPolicy::default()
        .require_identity(certificate_identity)
        .require_issuer(oidc_issuer);
    let result = verify(artifact_bytes, &bundle, &policy, &trusted_root)
        .map_err(|_| SigstoreVerificationError::Verification)?;
    if !result.warnings.is_empty() {
        return Err(SigstoreVerificationError::Warning);
    }
    let identity = result
        .identity
        .filter(|identity| identity == certificate_identity)
        .ok_or(SigstoreVerificationError::Verification)?;
    let issuer = result
        .issuer
        .filter(|issuer| issuer == oidc_issuer)
        .ok_or(SigstoreVerificationError::Verification)?;
    let integrated_time_unix = result
        .integrated_time
        .filter(|time| *time > 0)
        .ok_or(SigstoreVerificationError::Verification)?;
    Ok(SigstoreVerificationReport {
        certificate_identity: identity,
        oidc_issuer: issuer,
        integrated_time_unix,
        trust_root_profile: TRUST_ROOT_PROFILE.to_owned(),
        trust_root_sha256: lower_hex(&Sha256::digest(SIGSTORE_PRODUCTION_TRUSTED_ROOT)),
        certificate_chain_verified: true,
        signed_certificate_timestamp_verified: true,
        transparency_log_verified: true,
        artifact_signature_verified: true,
        artifact_bytes: artifact_bytes.len() as u64,
        artifact_sha256: lower_hex(&Sha256::digest(artifact_bytes)),
    })
}

fn valid_identity(value: &str) -> bool {
    !value.is_empty() && value.len() <= 2_048 && value.bytes().all(|byte| byte.is_ascii_graphic())
}

fn valid_issuer(value: &str) -> bool {
    let remainder = match value.strip_prefix("https://") {
        Some(remainder) => remainder,
        None => return false,
    };
    let authority_end = remainder.find('/').unwrap_or(remainder.len());
    let authority = &remainder[..authority_end];
    value.len() <= 2_048
        && !value.contains(['?', '#'])
        && !authority.is_empty()
        && !authority.contains(['@', ':'])
        && authority.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-')
        })
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_and_unbounded_inputs_fail_closed() {
        assert_eq!(
            verify_sigstore_bundle(
                b"artifact",
                b"not-json",
                "person@example.com",
                "https://accounts.example.com",
            ),
            Err(SigstoreVerificationError::Bundle)
        );
        assert_eq!(
            verify_sigstore_bundle(
                b"artifact",
                b"{}",
                "person@example.com",
                "http://accounts.example.com",
            ),
            Err(SigstoreVerificationError::Field)
        );
    }

    #[test]
    fn public_bundle_binds_artifact_identity_issuer_and_transparency() {
        let artifact = include_bytes!("../../../compatibility/sigstore-v0.3/cosign-v3-blob.txt");
        let bundle =
            include_bytes!("../../../compatibility/sigstore-v0.3/cosign-v3-blob.sigstore.json");
        let report = verify_sigstore_bundle(
            artifact,
            bundle,
            "w.vollprecht@gmail.com",
            "https://github.com/login/oauth",
        )
        .expect("official cosign bundle should verify offline");
        assert_eq!(report.integrated_time_unix(), 1_764_787_003);
        assert!(report.transparency_log_verified());
        assert!(report.artifact_signature_verified());
        assert_eq!(report.artifact_bytes(), artifact.len() as u64);
        assert_eq!(
            report.artifact_sha256(),
            lower_hex(&Sha256::digest(artifact))
        );

        assert_eq!(
            verify_sigstore_bundle(
                b"tampered content",
                bundle,
                "w.vollprecht@gmail.com",
                "https://github.com/login/oauth",
            ),
            Err(SigstoreVerificationError::Verification)
        );
        assert_eq!(
            verify_sigstore_bundle(
                artifact,
                bundle,
                "different@example.com",
                "https://github.com/login/oauth",
            ),
            Err(SigstoreVerificationError::Verification)
        );
        assert_eq!(
            verify_sigstore_bundle(
                artifact,
                bundle,
                "w.vollprecht@gmail.com",
                "https://token.actions.githubusercontent.com",
            ),
            Err(SigstoreVerificationError::Verification)
        );
    }
}

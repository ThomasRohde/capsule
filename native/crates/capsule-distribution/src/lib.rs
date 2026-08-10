//! Offline-verifiable release, update, and revocation policy.
//!
//! This crate has no network or installer API. The trusted host may fetch bytes,
//! but only these exact signed policies can authorize a target and artifact.

use std::collections::BTreeSet;

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlite_capsule_platform::{
    PlatformTimestampEvidence, PlatformVerificationReport, validate_platform_signing_identity,
};
use sqlite_capsule_sigstore::SigstoreVerificationReport;
use thiserror::Error;

pub const RELEASE_PROFILE: &str = "org.sqlite-capsule.host-release/0.2";
pub const REVOCATION_PROFILE: &str = "org.sqlite-capsule.revocations/0.2";
const RELEASE_CONTEXT: &[u8] = b"SQLite Capsule host release manifest v2\0";
const REVOCATION_CONTEXT: &[u8] = b"SQLite Capsule revocation bundle v1\0";
const MAX_ARTIFACTS: usize = 32;
pub const MAX_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;
const MAX_REVOCATIONS: usize = 10_000;
const MAX_REASON_BYTES: usize = 2_048;
const CLOCK_SKEW_SECONDS: i64 = 300;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DistributionError {
    #[error("signed document profile is unsupported")]
    Profile,
    #[error("signed document field is invalid or out of bounds")]
    Field,
    #[error("signature key ID does not match the compiled root")]
    KeyId,
    #[error("signed document signature is invalid")]
    Signature,
    #[error("signed document canonicalization failed")]
    Canonical,
    #[error("release manifest is expired or not yet valid")]
    Time,
    #[error("release sequence or version is not an upgrade")]
    Downgrade,
    #[error("signed release does not match the installed host identity")]
    InstalledRelease,
    #[error("release does not contain exactly one matching target")]
    Target,
    #[error("update requires explicit user consent")]
    Consent,
    #[error("capsule sessions must be quiesced before update")]
    SessionActive,
    #[error("update requires a completed verified backup")]
    BackupRequired,
    #[error("artifact URL or redirect is outside the HTTPS allowlist")]
    Url,
    #[error("downloaded artifact is partial or has the wrong digest")]
    Artifact,
    #[error("platform-signing evidence is not bound to the selected artifact and policy")]
    PlatformEvidence,
    #[error("Sigstore evidence is not bound to the selected artifact and policy")]
    SigstoreEvidence,
    #[error("revocation sequence is not newer than last-known-good")]
    RevocationRollback,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseArtifact {
    pub target: String,
    pub url: String,
    pub bytes: u64,
    pub sha256: String,
    pub sigstore_bundle_sha256: String,
    pub platform_signing: String,
    pub platform_signing_identity: String,
    pub platform_timestamp_required: bool,
    pub sigstore_certificate_identity: String,
    pub sigstore_oidc_issuer: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseManifest {
    pub profile: String,
    pub sequence: u64,
    pub version: String,
    pub issued_at: String,
    pub expires_at: String,
    pub artifacts: Vec<ReleaseArtifact>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedReleaseManifest {
    pub manifest: ReleaseManifest,
    pub signing_key_id: String,
    pub signature_hex: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateContext<'a> {
    pub current_version: &'a str,
    pub current_sequence: u64,
    pub target: &'a str,
    pub allowed_hosts: &'a [&'a str],
    pub now_unix: i64,
    pub user_consented: bool,
    pub sessions_quiesced: bool,
    pub verified_backup_complete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseCandidateContext<'a> {
    pub current_version: &'a str,
    pub current_sequence: u64,
    pub target: &'a str,
    pub allowed_hosts: &'a [&'a str],
    pub now_unix: i64,
}

/// Exact identity of the release currently running. Historical verification
/// proves that a retained installer came from the signed release channel, but
/// deliberately does not treat it as a new or currently downloadable update.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledReleaseContext<'a> {
    pub version: &'a str,
    pub sequence: u64,
    pub target: &'a str,
    pub allowed_hosts: &'a [&'a str],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UpdateAuthorization {
    pub user_consented: bool,
    pub sessions_quiesced: bool,
    pub verified_backup_complete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedReleaseCandidate {
    version: String,
    sequence: u64,
    artifact: ReleaseArtifact,
    signed_release: SignedReleaseManifest,
}

impl VerifiedReleaseCandidate {
    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    pub fn artifact(&self) -> &ReleaseArtifact {
        &self.artifact
    }

    /// The exact signed release envelope from which this candidate was
    /// selected. Durable staging retains it so a later installer launch can
    /// reverify the authorization under the compiled release root.
    pub fn signed_release(&self) -> &SignedReleaseManifest {
        &self.signed_release
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedInstalledRelease {
    version: String,
    sequence: u64,
    artifact: ReleaseArtifact,
    signed_release: SignedReleaseManifest,
}

impl VerifiedInstalledRelease {
    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    pub fn artifact(&self) -> &ReleaseArtifact {
        &self.artifact
    }

    pub fn signed_release(&self) -> &SignedReleaseManifest {
        &self.signed_release
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedUpdate {
    pub version: String,
    pub sequence: u64,
    pub artifact: ReleaseArtifact,
}

/// A selected release whose exact downloaded package has passed both the
/// platform trust adapter and the offline Sigstore verifier. Construction is
/// restricted to `accept_downloaded_update` so downstream code cannot claim
/// this state from digest matches alone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedDownloadedUpdate {
    candidate: VerifiedReleaseCandidate,
    platform_verification: PlatformVerificationReport,
    sigstore_verification: SigstoreVerificationReport,
}

impl VerifiedDownloadedUpdate {
    pub fn candidate(&self) -> &VerifiedReleaseCandidate {
        &self.candidate
    }

    pub fn platform_verification(&self) -> &PlatformVerificationReport {
        &self.platform_verification
    }

    pub fn sigstore_verification(&self) -> &SigstoreVerificationReport {
        &self.sigstore_verification
    }
}

/// A cryptographically accepted download whose installation preconditions
/// have also been explicitly authorized by the trusted host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedInstallableUpdate {
    downloaded: VerifiedDownloadedUpdate,
}

impl VerifiedInstallableUpdate {
    pub fn version(&self) -> &str {
        self.downloaded.candidate.version()
    }

    pub fn sequence(&self) -> u64 {
        self.downloaded.candidate.sequence()
    }

    pub fn artifact(&self) -> &ReleaseArtifact {
        self.downloaded.candidate.artifact()
    }

    pub fn signed_release(&self) -> &SignedReleaseManifest {
        self.downloaded.candidate.signed_release()
    }

    pub fn platform_verification(&self) -> &PlatformVerificationReport {
        self.downloaded.platform_verification()
    }

    pub fn sigstore_verification(&self) -> &SigstoreVerificationReport {
        self.downloaded.sigstore_verification()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyRevocation {
    pub key_id: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseRevocation {
    pub application_id: String,
    pub application_digest_sha256: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmergencyRoot {
    pub key_id: String,
    pub public_key_hex: String,
    pub action: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevocationBundle {
    pub profile: String,
    pub sequence: u64,
    pub issued_at: String,
    pub next_update: String,
    pub revoked_keys: Vec<KeyRevocation>,
    pub revoked_releases: Vec<ReleaseRevocation>,
    pub emergency_roots: Vec<EmergencyRoot>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedRevocationBundle {
    pub bundle: RevocationBundle,
    pub signing_key_id: String,
    pub signature_hex: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RevocationFreshness {
    Fresh,
    Stale,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedRevocationBundle {
    bundle: RevocationBundle,
    payload_digest: [u8; 32],
    freshness: RevocationFreshness,
}

pub fn key_id(public_key: &[u8; 32]) -> String {
    format!("ed25519:sha256:{}", lower_hex(&Sha256::digest(public_key)))
}

pub fn verify_release_manifest(
    signed: &SignedReleaseManifest,
    trusted_root: &[u8; 32],
    context: &UpdateContext<'_>,
) -> Result<VerifiedUpdate, DistributionError> {
    let candidate = verify_release_candidate(
        signed,
        trusted_root,
        &ReleaseCandidateContext {
            current_version: context.current_version,
            current_sequence: context.current_sequence,
            target: context.target,
            allowed_hosts: context.allowed_hosts,
            now_unix: context.now_unix,
        },
    )?;
    authorize_update(
        candidate,
        UpdateAuthorization {
            user_consented: context.user_consented,
            sessions_quiesced: context.sessions_quiesced,
            verified_backup_complete: context.verified_backup_complete,
        },
    )
}

/// Verify and select a displayable update candidate without claiming that the
/// user has consented or that installation is safe to begin.
pub fn verify_release_candidate(
    signed: &SignedReleaseManifest,
    trusted_root: &[u8; 32],
    context: &ReleaseCandidateContext<'_>,
) -> Result<VerifiedReleaseCandidate, DistributionError> {
    let (issued, expires) = verify_release_envelope(signed, trusted_root)?;
    if issued > context.now_unix.saturating_add(CLOCK_SKEW_SECONDS) || expires < context.now_unix {
        return Err(DistributionError::Time);
    }
    let current = parse_version(context.current_version)?;
    let candidate = parse_version(&signed.manifest.version)?;
    if signed.manifest.sequence <= context.current_sequence || candidate <= current {
        return Err(DistributionError::Downgrade);
    }
    let artifact = select_release_artifact(signed, context.target)?;
    validate_artifact(artifact, context.allowed_hosts)?;
    Ok(VerifiedReleaseCandidate {
        version: signed.manifest.version.clone(),
        sequence: signed.manifest.sequence,
        artifact: artifact.clone(),
        signed_release: signed.clone(),
    })
}

/// Verify an exact retained installer for the already-running release. Expiry
/// is intentionally not re-applied: the envelope is historical provenance for
/// a release that previously reached healthy startup, not authorization to
/// download it now. Signature, canonical fields, date ordering, exact
/// version/sequence/target, and origin policy are still enforced.
pub fn verify_installed_release(
    signed: &SignedReleaseManifest,
    trusted_root: &[u8; 32],
    context: &InstalledReleaseContext<'_>,
) -> Result<VerifiedInstalledRelease, DistributionError> {
    verify_release_envelope(signed, trusted_root)?;
    parse_version(context.version)?;
    if signed.manifest.version != context.version
        || signed.manifest.sequence != context.sequence
        || context.sequence == 0
    {
        return Err(DistributionError::InstalledRelease);
    }
    let artifact = select_release_artifact(signed, context.target)?;
    validate_artifact(artifact, context.allowed_hosts)?;
    Ok(VerifiedInstalledRelease {
        version: signed.manifest.version.clone(),
        sequence: signed.manifest.sequence,
        artifact: artifact.clone(),
        signed_release: signed.clone(),
    })
}

/// Consume a verified candidate only after the trusted host has recorded the
/// explicit install decision and completed its capsule-session preflight.
pub fn authorize_update(
    candidate: VerifiedReleaseCandidate,
    authorization: UpdateAuthorization,
) -> Result<VerifiedUpdate, DistributionError> {
    validate_authorization(authorization)?;
    Ok(VerifiedUpdate {
        version: candidate.version,
        sequence: candidate.sequence,
        artifact: candidate.artifact,
    })
}

/// Bind independent platform and Sigstore verification reports to the exact
/// selected artifact and its signed policy. Digest checks are repeated here so
/// no caller can combine reports produced for different bytes.
pub fn accept_downloaded_update(
    candidate: VerifiedReleaseCandidate,
    artifact_bytes: &[u8],
    sigstore_bundle_bytes: &[u8],
    platform_verification: PlatformVerificationReport,
    sigstore_verification: SigstoreVerificationReport,
) -> Result<VerifiedDownloadedUpdate, DistributionError> {
    verify_candidate_artifact_bytes(&candidate, artifact_bytes)?;
    verify_candidate_sigstore_bundle_bytes(&candidate, sigstore_bundle_bytes)?;
    let artifact = candidate.artifact();
    if platform_verification.platform_signing() != artifact.platform_signing
        || platform_verification.platform_signing_identity() != artifact.platform_signing_identity
        || !platform_verification.offline_revocation_mode()
        || platform_verification.artifact_bytes() != artifact.bytes
        || platform_verification.artifact_sha256() != artifact.sha256
        || (artifact.platform_timestamp_required
            && platform_verification.timestamp_evidence() == PlatformTimestampEvidence::None)
    {
        return Err(DistributionError::PlatformEvidence);
    }
    if sigstore_verification.certificate_identity() != artifact.sigstore_certificate_identity
        || sigstore_verification.oidc_issuer() != artifact.sigstore_oidc_issuer
        || sigstore_verification.artifact_bytes() != artifact.bytes
        || sigstore_verification.artifact_sha256() != artifact.sha256
        || !sigstore_verification.all_evidence_verified()
    {
        return Err(DistributionError::SigstoreEvidence);
    }
    Ok(VerifiedDownloadedUpdate {
        candidate,
        platform_verification,
        sigstore_verification,
    })
}

/// Cross the installation boundary only after cryptographic acceptance and
/// the trusted host's explicit consent/session/backup preflight.
pub fn authorize_installable_update(
    downloaded: VerifiedDownloadedUpdate,
    authorization: UpdateAuthorization,
) -> Result<VerifiedInstallableUpdate, DistributionError> {
    validate_authorization(authorization)?;
    Ok(VerifiedInstallableUpdate { downloaded })
}

fn validate_authorization(authorization: UpdateAuthorization) -> Result<(), DistributionError> {
    if !authorization.user_consented {
        return Err(DistributionError::Consent);
    }
    if !authorization.sessions_quiesced {
        return Err(DistributionError::SessionActive);
    }
    if !authorization.verified_backup_complete {
        return Err(DistributionError::BackupRequired);
    }
    Ok(())
}

pub fn verify_artifact_bytes(
    update: &VerifiedUpdate,
    bytes: &[u8],
) -> Result<(), DistributionError> {
    verify_release_artifact_bytes(&update.artifact, bytes)
}

pub fn verify_sigstore_bundle_bytes(
    update: &VerifiedUpdate,
    bytes: &[u8],
) -> Result<(), DistributionError> {
    verify_release_sigstore_bundle_bytes(&update.artifact, bytes)
}

pub fn verify_installable_artifact_bytes(
    update: &VerifiedInstallableUpdate,
    bytes: &[u8],
) -> Result<(), DistributionError> {
    verify_release_artifact_bytes(update.artifact(), bytes)
}

pub fn verify_installable_sigstore_bundle_bytes(
    update: &VerifiedInstallableUpdate,
    bytes: &[u8],
) -> Result<(), DistributionError> {
    verify_release_sigstore_bundle_bytes(update.artifact(), bytes)
}

/// Verify downloaded bytes against a signed and selected candidate before the
/// separate user-consent/session/backup authorization boundary is crossed.
pub fn verify_candidate_artifact_bytes(
    candidate: &VerifiedReleaseCandidate,
    bytes: &[u8],
) -> Result<(), DistributionError> {
    verify_release_artifact_bytes(&candidate.artifact, bytes)
}

/// Bind the fetched Sigstore evidence to the signed release candidate without
/// treating the digest match as cryptographic Sigstore verification.
pub fn verify_candidate_sigstore_bundle_bytes(
    candidate: &VerifiedReleaseCandidate,
    bytes: &[u8],
) -> Result<(), DistributionError> {
    verify_release_sigstore_bundle_bytes(&candidate.artifact, bytes)
}

pub fn verify_redirect(
    redirected_url: &str,
    expected_url: &str,
    allowed_hosts: &[&str],
) -> Result<(), DistributionError> {
    let expected = https_host(expected_url)?;
    let redirected = https_host(redirected_url)?;
    if expected != redirected || !allowed_hosts.contains(&redirected) {
        return Err(DistributionError::Url);
    }
    Ok(())
}

pub fn verify_revocation_bundle(
    signed: &SignedRevocationBundle,
    trusted_root: &[u8; 32],
    last_sequence: u64,
    now_unix: i64,
) -> Result<VerifiedRevocationBundle, DistributionError> {
    let bundle = &signed.bundle;
    if bundle.profile != REVOCATION_PROFILE
        || bundle.sequence == 0
        || bundle.sequence > i64::MAX as u64
    {
        return Err(DistributionError::Profile);
    }
    if bundle.sequence <= last_sequence {
        return Err(DistributionError::RevocationRollback);
    }
    verify_signed(
        REVOCATION_CONTEXT,
        bundle,
        &signed.signing_key_id,
        &signed.signature_hex,
        trusted_root,
    )?;
    let issued = parse_utc_seconds(&bundle.issued_at)?;
    let next_update = parse_utc_seconds(&bundle.next_update)?;
    if next_update <= issued || issued > now_unix.saturating_add(CLOCK_SKEW_SECONDS) {
        return Err(DistributionError::Time);
    }
    validate_revocations(bundle)?;
    let canonical = canonical_bytes(bundle)?;
    Ok(VerifiedRevocationBundle {
        bundle: bundle.clone(),
        payload_digest: Sha256::digest(&canonical).into(),
        freshness: if now_unix <= next_update {
            RevocationFreshness::Fresh
        } else {
            RevocationFreshness::Stale
        },
    })
}

impl VerifiedRevocationBundle {
    pub fn bundle(&self) -> &RevocationBundle {
        &self.bundle
    }

    pub fn payload_digest(&self) -> &[u8; 32] {
        &self.payload_digest
    }

    pub fn freshness(&self) -> RevocationFreshness {
        self.freshness
    }

    pub fn revokes_key(&self, key_id: &str) -> bool {
        self.bundle
            .revoked_keys
            .iter()
            .any(|entry| entry.key_id == key_id)
    }

    pub fn revokes_release(&self, application_id: &str, digest: &[u8; 32]) -> bool {
        let digest = lower_hex(digest);
        self.bundle.revoked_releases.iter().any(|entry| {
            entry.application_id == application_id && entry.application_digest_sha256 == digest
        })
    }
}

fn verify_release_envelope(
    signed: &SignedReleaseManifest,
    trusted_root: &[u8; 32],
) -> Result<(i64, i64), DistributionError> {
    if signed.manifest.profile != RELEASE_PROFILE
        || signed.manifest.sequence == 0
        || signed.manifest.sequence > i64::MAX as u64
        || signed.manifest.artifacts.is_empty()
        || signed.manifest.artifacts.len() > MAX_ARTIFACTS
    {
        return Err(DistributionError::Profile);
    }
    verify_signed(
        RELEASE_CONTEXT,
        &signed.manifest,
        &signed.signing_key_id,
        &signed.signature_hex,
        trusted_root,
    )?;
    let issued = parse_utc_seconds(&signed.manifest.issued_at)?;
    let expires = parse_utc_seconds(&signed.manifest.expires_at)?;
    if expires <= issued {
        return Err(DistributionError::Time);
    }
    Ok((issued, expires))
}

fn select_release_artifact<'a>(
    signed: &'a SignedReleaseManifest,
    target: &str,
) -> Result<&'a ReleaseArtifact, DistributionError> {
    let mut matching = signed
        .manifest
        .artifacts
        .iter()
        .filter(|artifact| artifact.target == target);
    let artifact = matching.next().ok_or(DistributionError::Target)?;
    if matching.next().is_some() {
        return Err(DistributionError::Target);
    }
    Ok(artifact)
}

fn verify_signed<T: Serialize>(
    context: &[u8],
    payload: &T,
    signing_key_id: &str,
    signature_hex: &str,
    trusted_root: &[u8; 32],
) -> Result<(), DistributionError> {
    if signing_key_id != key_id(trusted_root) {
        return Err(DistributionError::KeyId);
    }
    let signature = decode_hex::<64>(signature_hex)?;
    let key = VerifyingKey::from_bytes(trusted_root).map_err(|_| DistributionError::Signature)?;
    let canonical = canonical_bytes(payload)?;
    let mut message = Vec::with_capacity(context.len() + canonical.len());
    message.extend_from_slice(context);
    message.extend_from_slice(&canonical);
    key.verify(&message, &Signature::from_bytes(&signature))
        .map_err(|_| DistributionError::Signature)
}

fn validate_artifact(
    artifact: &ReleaseArtifact,
    allowed_hosts: &[&str],
) -> Result<(), DistributionError> {
    if artifact.target.is_empty()
        || artifact.target.len() > 128
        || !artifact
            .target
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        || artifact.bytes == 0
        || artifact.bytes > MAX_ARTIFACT_BYTES
        || decode_hex::<32>(&artifact.sha256).is_err()
        || decode_hex::<32>(&artifact.sigstore_bundle_sha256).is_err()
        || !matches!(
            artifact.platform_signing.as_str(),
            "authenticode" | "developer-id-notarized" | "linux-detached"
        )
        || validate_platform_signing_identity(
            &artifact.platform_signing,
            &artifact.platform_signing_identity,
        )
        .is_err()
        || (matches!(
            artifact.platform_signing.as_str(),
            "authenticode" | "developer-id-notarized"
        ) && !artifact.platform_timestamp_required)
        || !valid_sigstore_identity(&artifact.sigstore_certificate_identity)
        || !valid_sigstore_issuer(&artifact.sigstore_oidc_issuer)
    {
        return Err(DistributionError::Field);
    }
    let host = https_host(&artifact.url)?;
    if !allowed_hosts.contains(&host) {
        return Err(DistributionError::Url);
    }
    Ok(())
}

fn valid_sigstore_identity(value: &str) -> bool {
    !value.is_empty() && value.len() <= 2_048 && value.bytes().all(|byte| byte.is_ascii_graphic())
}

fn valid_sigstore_issuer(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 2_048
        && !value.contains(['?', '#'])
        && https_host(value).is_ok()
}

fn verify_release_artifact_bytes(
    artifact: &ReleaseArtifact,
    bytes: &[u8],
) -> Result<(), DistributionError> {
    if bytes.len() as u64 != artifact.bytes || lower_hex(&Sha256::digest(bytes)) != artifact.sha256
    {
        return Err(DistributionError::Artifact);
    }
    Ok(())
}

fn verify_release_sigstore_bundle_bytes(
    artifact: &ReleaseArtifact,
    bytes: &[u8],
) -> Result<(), DistributionError> {
    if lower_hex(&Sha256::digest(bytes)) != artifact.sigstore_bundle_sha256 {
        return Err(DistributionError::Artifact);
    }
    Ok(())
}

fn validate_revocations(bundle: &RevocationBundle) -> Result<(), DistributionError> {
    if bundle.revoked_keys.len() > MAX_REVOCATIONS
        || bundle.revoked_releases.len() > MAX_REVOCATIONS
        || bundle.emergency_roots.len() > 16
    {
        return Err(DistributionError::Field);
    }
    let mut keys = BTreeSet::new();
    for entry in &bundle.revoked_keys {
        if !valid_key_id(&entry.key_id)
            || !valid_reason(&entry.reason)
            || !keys.insert(entry.key_id.as_str())
        {
            return Err(DistributionError::Field);
        }
    }
    let mut releases = BTreeSet::new();
    for entry in &bundle.revoked_releases {
        if entry.application_id.is_empty()
            || entry.application_id.len() > 512
            || decode_hex::<32>(&entry.application_digest_sha256).is_err()
            || !valid_reason(&entry.reason)
            || !releases.insert((
                entry.application_id.as_str(),
                entry.application_digest_sha256.as_str(),
            ))
        {
            return Err(DistributionError::Field);
        }
    }
    let mut roots = BTreeSet::new();
    for entry in &bundle.emergency_roots {
        let public_key = decode_hex::<32>(&entry.public_key_hex)?;
        if entry.key_id != key_id(&public_key)
            || !matches!(entry.action.as_str(), "delegate" | "revoke")
            || !valid_reason(&entry.reason)
            || !roots.insert(entry.key_id.as_str())
        {
            return Err(DistributionError::Field);
        }
    }
    Ok(())
}

fn valid_key_id(value: &str) -> bool {
    value
        .strip_prefix("ed25519:sha256:")
        .is_some_and(|hex| decode_hex::<32>(hex).is_ok())
}

fn valid_reason(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_REASON_BYTES && !value.contains(['\r', '\n', '\0'])
}

fn https_host(url: &str) -> Result<&str, DistributionError> {
    let remainder = url.strip_prefix("https://").ok_or(DistributionError::Url)?;
    let authority_end = remainder.find(['/', '?', '#']).unwrap_or(remainder.len());
    let authority = &remainder[..authority_end];
    if authority.is_empty()
        || authority.contains(['@', ':'])
        || authority.bytes().any(|byte| {
            !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-'))
        })
        || url.contains('#')
    {
        return Err(DistributionError::Url);
    }
    Ok(authority)
}

fn parse_version(value: &str) -> Result<(u64, u64, u64), DistributionError> {
    let values = value
        .split('.')
        .map(|part| {
            if part.is_empty()
                || (part.len() > 1 && part.starts_with('0'))
                || !part.bytes().all(|byte| byte.is_ascii_digit())
            {
                return Err(DistributionError::Field);
            }
            part.parse::<u64>().map_err(|_| DistributionError::Field)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if values.len() != 3 {
        return Err(DistributionError::Field);
    }
    Ok((values[0], values[1], values[2]))
}

fn parse_utc_seconds(value: &str) -> Result<i64, DistributionError> {
    let bytes = value.as_bytes();
    if bytes.len() != 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'Z'
    {
        return Err(DistributionError::Field);
    }
    let number = |start: usize, end: usize| -> Result<i64, DistributionError> {
        let slice = &bytes[start..end];
        if !slice.iter().all(u8::is_ascii_digit) {
            return Err(DistributionError::Field);
        }
        std::str::from_utf8(slice)
            .map_err(|_| DistributionError::Field)?
            .parse::<i64>()
            .map_err(|_| DistributionError::Field)
    };
    let year = number(0, 4)?;
    let month = number(5, 7)?;
    let day = number(8, 10)?;
    let hour = number(11, 13)?;
    let minute = number(14, 16)?;
    let second = number(17, 19)?;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let month_days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if !(1970..=9999).contains(&year)
        || !(1..=12).contains(&month)
        || day < 1
        || day > month_days[(month - 1) as usize]
        || hour > 23
        || minute > 59
        || second > 59
    {
        return Err(DistributionError::Field);
    }
    let days = days_from_civil(year, month, day);
    Ok(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let adjusted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * adjusted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn canonical_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, DistributionError> {
    serde_json_canonicalizer::to_vec(value).map_err(|_| DistributionError::Canonical)
}

fn decode_hex<const N: usize>(value: &str) -> Result<[u8; N], DistributionError> {
    if value.len() != N * 2 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(DistributionError::Field);
    }
    let mut output = [0_u8; N];
    for (index, byte) in output.iter_mut().enumerate() {
        let high = hex_nibble(value.as_bytes()[index * 2])?;
        let low = hex_nibble(value.as_bytes()[index * 2 + 1])?;
        *byte = high << 4 | low;
    }
    Ok(output)
}

fn hex_nibble(value: u8) -> Result<u8, DistributionError> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        _ => Err(DistributionError::Field),
    }
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
    use ed25519_dalek::{Signer, SigningKey};

    use super::*;

    fn root() -> SigningKey {
        SigningKey::from_bytes(&[17_u8; 32])
    }

    fn sign_release(manifest: ReleaseManifest) -> SignedReleaseManifest {
        let root = root();
        let canonical = canonical_bytes(&manifest).expect("canonical release");
        let mut message = RELEASE_CONTEXT.to_vec();
        message.extend_from_slice(&canonical);
        SignedReleaseManifest {
            manifest,
            signing_key_id: key_id(&root.verifying_key().to_bytes()),
            signature_hex: lower_hex(&root.sign(&message).to_bytes()),
        }
    }

    fn sign_revocations(bundle: RevocationBundle) -> SignedRevocationBundle {
        let root = root();
        let canonical = canonical_bytes(&bundle).expect("canonical revocations");
        let mut message = REVOCATION_CONTEXT.to_vec();
        message.extend_from_slice(&canonical);
        SignedRevocationBundle {
            bundle,
            signing_key_id: key_id(&root.verifying_key().to_bytes()),
            signature_hex: lower_hex(&root.sign(&message).to_bytes()),
        }
    }

    fn artifact(bytes: &[u8]) -> ReleaseArtifact {
        ReleaseArtifact {
            target: "x86_64-pc-windows-msvc".to_owned(),
            url: "https://downloads.example.com/sqlite-capsule-1.2.0.msi".to_owned(),
            bytes: bytes.len() as u64,
            sha256: lower_hex(&Sha256::digest(bytes)),
            sigstore_bundle_sha256: lower_hex(&[4_u8; 32]),
            platform_signing: "authenticode".to_owned(),
            platform_signing_identity: format!(
                "authenticode-certificate-sha256:{}",
                "ab".repeat(32)
            ),
            platform_timestamp_required: true,
            sigstore_certificate_identity:
                "https://github.com/sqlite-capsule/sqlite-capsule/.github/workflows/release.yml@refs/tags/v1.2.0"
                    .to_owned(),
            sigstore_oidc_issuer: "https://token.actions.githubusercontent.com".to_owned(),
        }
    }

    fn release(bytes: &[u8]) -> ReleaseManifest {
        ReleaseManifest {
            profile: RELEASE_PROFILE.to_owned(),
            sequence: 12,
            version: "1.2.0".to_owned(),
            issued_at: "2026-08-08T10:00:00Z".to_owned(),
            expires_at: "2026-08-09T10:00:00Z".to_owned(),
            artifacts: vec![artifact(bytes)],
        }
    }

    fn context<'a>(allowed_hosts: &'a [&'a str]) -> UpdateContext<'a> {
        UpdateContext {
            current_version: "1.1.9",
            current_sequence: 11,
            target: "x86_64-pc-windows-msvc",
            allowed_hosts,
            now_unix: parse_utc_seconds("2026-08-08T12:00:00Z").expect("now"),
            user_consented: true,
            sessions_quiesced: true,
            verified_backup_complete: true,
        }
    }

    #[test]
    fn signed_release_requires_consent_quiescence_backup_and_exact_artifact() {
        let bytes = b"signed installer fixture";
        let signed = sign_release(release(bytes));
        let hosts = ["downloads.example.com"];
        let update = verify_release_manifest(
            &signed,
            &root().verifying_key().to_bytes(),
            &context(&hosts),
        )
        .expect("verified update");
        verify_artifact_bytes(&update, bytes).expect("artifact bytes");
        assert_eq!(
            verify_sigstore_bundle_bytes(&update, b"wrong bundle"),
            Err(DistributionError::Artifact)
        );
        assert_eq!(
            verify_artifact_bytes(&update, b"partial"),
            Err(DistributionError::Artifact)
        );

        let mut denied = context(&hosts);
        denied.user_consented = false;
        assert_eq!(
            verify_release_manifest(&signed, &root().verifying_key().to_bytes(), &denied),
            Err(DistributionError::Consent)
        );
        denied.user_consented = true;
        denied.sessions_quiesced = false;
        assert_eq!(
            verify_release_manifest(&signed, &root().verifying_key().to_bytes(), &denied),
            Err(DistributionError::SessionActive)
        );
        denied.sessions_quiesced = true;
        denied.verified_backup_complete = false;
        assert_eq!(
            verify_release_manifest(&signed, &root().verifying_key().to_bytes(), &denied),
            Err(DistributionError::BackupRequired)
        );
    }

    #[test]
    fn candidate_review_is_distinct_from_install_authorization() {
        let bytes = b"signed installer fixture";
        let signed = sign_release(release(bytes));
        let hosts = ["downloads.example.com"];
        let candidate = verify_release_candidate(
            &signed,
            &root().verifying_key().to_bytes(),
            &ReleaseCandidateContext {
                current_version: "1.1.9",
                current_sequence: 11,
                target: "x86_64-pc-windows-msvc",
                allowed_hosts: &hosts,
                now_unix: parse_utc_seconds("2026-08-08T12:00:00Z").expect("now"),
            },
        )
        .expect("reviewed candidate");
        assert_eq!(candidate.version(), "1.2.0");
        assert_eq!(candidate.sequence(), 12);
        assert_eq!(candidate.artifact().platform_signing, "authenticode");
        verify_candidate_artifact_bytes(&candidate, bytes).expect("reviewed candidate bytes");
        assert_eq!(
            verify_candidate_artifact_bytes(&candidate, b"partial"),
            Err(DistributionError::Artifact)
        );

        for (authorization, expected) in [
            (
                UpdateAuthorization {
                    user_consented: false,
                    sessions_quiesced: true,
                    verified_backup_complete: true,
                },
                DistributionError::Consent,
            ),
            (
                UpdateAuthorization {
                    user_consented: true,
                    sessions_quiesced: false,
                    verified_backup_complete: true,
                },
                DistributionError::SessionActive,
            ),
            (
                UpdateAuthorization {
                    user_consented: true,
                    sessions_quiesced: true,
                    verified_backup_complete: false,
                },
                DistributionError::BackupRequired,
            ),
        ] {
            assert_eq!(
                authorize_update(candidate.clone(), authorization),
                Err(expected)
            );
        }
        let update = authorize_update(
            candidate,
            UpdateAuthorization {
                user_consented: true,
                sessions_quiesced: true,
                verified_backup_complete: true,
            },
        )
        .expect("authorized update");
        verify_artifact_bytes(&update, bytes).expect("authorized artifact bytes");
    }

    #[test]
    fn historical_release_proves_exact_installed_identity_after_expiry() {
        let bytes = b"signed installed package";
        let signed = sign_release(release(bytes));
        let hosts = ["downloads.example.com"];
        let public = root().verifying_key().to_bytes();
        let installed = verify_installed_release(
            &signed,
            &public,
            &InstalledReleaseContext {
                version: "1.2.0",
                sequence: 12,
                target: "x86_64-pc-windows-msvc",
                allowed_hosts: &hosts,
            },
        )
        .expect("verify historical installed release");
        assert_eq!(installed.version(), "1.2.0");
        assert_eq!(installed.sequence(), 12);
        assert_eq!(installed.artifact().sha256, artifact(bytes).sha256);
        assert_eq!(installed.signed_release(), &signed);

        assert_eq!(
            verify_installed_release(
                &signed,
                &public,
                &InstalledReleaseContext {
                    version: "1.2.1",
                    sequence: 12,
                    target: "x86_64-pc-windows-msvc",
                    allowed_hosts: &hosts,
                },
            ),
            Err(DistributionError::InstalledRelease)
        );
        let mut tampered = signed;
        tampered.manifest.expires_at = "2040-01-01T00:00:00Z".to_owned();
        assert_eq!(
            verify_installed_release(
                &tampered,
                &public,
                &InstalledReleaseContext {
                    version: "1.2.0",
                    sequence: 12,
                    target: "x86_64-pc-windows-msvc",
                    allowed_hosts: &hosts,
                },
            ),
            Err(DistributionError::Signature)
        );
    }

    #[test]
    fn release_policy_bounds_in_memory_artifact_downloads() {
        let mut manifest = release(b"bounded fixture");
        manifest.artifacts[0].bytes = MAX_ARTIFACT_BYTES + 1;
        let signed = sign_release(manifest);
        let hosts = ["downloads.example.com"];
        assert_eq!(
            verify_release_candidate(
                &signed,
                &root().verifying_key().to_bytes(),
                &ReleaseCandidateContext {
                    current_version: "1.1.9",
                    current_sequence: 11,
                    target: "x86_64-pc-windows-msvc",
                    allowed_hosts: &hosts,
                    now_unix: parse_utc_seconds("2026-08-08T12:00:00Z").expect("now"),
                },
            ),
            Err(DistributionError::Field)
        );
    }

    #[test]
    fn release_policy_binds_canonical_platform_and_sigstore_identities() {
        let bytes = b"identity-bound installer";
        let hosts = ["downloads.example.com"];
        let public = root().verifying_key().to_bytes();
        let rejects = |manifest: ReleaseManifest| {
            assert_eq!(
                verify_release_manifest(&sign_release(manifest), &public, &context(&hosts)),
                Err(DistributionError::Field)
            );
        };

        let mut bad_platform_identity = release(bytes);
        bad_platform_identity.artifacts[0].platform_signing_identity =
            format!("authenticode-certificate-sha256:{}", "AB".repeat(32));
        rejects(bad_platform_identity);

        let mut timestamp_optional = release(bytes);
        timestamp_optional.artifacts[0].platform_timestamp_required = false;
        rejects(timestamp_optional);

        let mut empty_sigstore_identity = release(bytes);
        empty_sigstore_identity.artifacts[0]
            .sigstore_certificate_identity
            .clear();
        rejects(empty_sigstore_identity);

        let mut insecure_issuer = release(bytes);
        insecure_issuer.artifacts[0].sigstore_oidc_issuer =
            "http://token.actions.githubusercontent.com".to_owned();
        rejects(insecure_issuer);
    }

    #[test]
    fn release_rejects_tamper_downgrade_expiry_target_and_unsafe_url() {
        let bytes = b"signed installer fixture";
        let hosts = ["downloads.example.com"];
        let public = root().verifying_key().to_bytes();

        let mut tampered = sign_release(release(bytes));
        tampered.manifest.artifacts[0].bytes += 1;
        assert_eq!(
            verify_release_manifest(&tampered, &public, &context(&hosts)),
            Err(DistributionError::Signature)
        );

        let mut downgrade = release(bytes);
        downgrade.version = "1.1.9".to_owned();
        assert_eq!(
            verify_release_manifest(&sign_release(downgrade), &public, &context(&hosts)),
            Err(DistributionError::Downgrade)
        );

        let mut expired = release(bytes);
        expired.expires_at = "2026-08-08T11:00:00Z".to_owned();
        assert_eq!(
            verify_release_manifest(&sign_release(expired), &public, &context(&hosts)),
            Err(DistributionError::Time)
        );

        let mut duplicate = release(bytes);
        duplicate.artifacts.push(duplicate.artifacts[0].clone());
        assert_eq!(
            verify_release_manifest(&sign_release(duplicate), &public, &context(&hosts)),
            Err(DistributionError::Target)
        );

        let mut unsafe_url = release(bytes);
        unsafe_url.artifacts[0].url = "http://downloads.example.com/update.msi".to_owned();
        assert_eq!(
            verify_release_manifest(&sign_release(unsafe_url), &public, &context(&hosts)),
            Err(DistributionError::Url)
        );
        assert!(
            verify_redirect(
                "https://downloads.example.com/redirected.msi",
                "https://downloads.example.com/update.msi",
                &hosts
            )
            .is_ok()
        );
        assert_eq!(
            verify_redirect(
                "https://mirror.example.net/update.msi",
                "https://downloads.example.com/update.msi",
                &hosts
            ),
            Err(DistributionError::Url)
        );
    }

    fn revocations(next_update: &str) -> RevocationBundle {
        let emergency = SigningKey::from_bytes(&[29_u8; 32]);
        RevocationBundle {
            profile: REVOCATION_PROFILE.to_owned(),
            sequence: 7,
            issued_at: "2026-08-08T10:00:00Z".to_owned(),
            next_update: next_update.to_owned(),
            revoked_keys: vec![KeyRevocation {
                key_id: format!("ed25519:sha256:{}", lower_hex(&[3_u8; 32])),
                reason: "publisher signing key compromised".to_owned(),
            }],
            revoked_releases: vec![ReleaseRevocation {
                application_id: "org.example.app".to_owned(),
                application_digest_sha256: lower_hex(&[5_u8; 32]),
                reason: "malicious release".to_owned(),
            }],
            emergency_roots: vec![EmergencyRoot {
                key_id: key_id(&emergency.verifying_key().to_bytes()),
                public_key_hex: lower_hex(&emergency.verifying_key().to_bytes()),
                action: "delegate".to_owned(),
                reason: "scheduled emergency-root rotation".to_owned(),
            }],
        }
    }

    #[test]
    fn revocations_are_monotonic_and_stale_bundles_still_block_known_entries() {
        let public = root().verifying_key().to_bytes();
        let now = parse_utc_seconds("2026-08-08T12:00:00Z").expect("now");
        let signed = sign_revocations(revocations("2026-08-09T10:00:00Z"));
        let verified =
            verify_revocation_bundle(&signed, &public, 6, now).expect("fresh revocation bundle");
        assert_eq!(verified.freshness(), RevocationFreshness::Fresh);
        assert!(verified.revokes_key(&format!("ed25519:sha256:{}", lower_hex(&[3_u8; 32]))));
        assert!(verified.revokes_release("org.example.app", &[5_u8; 32]));
        assert_eq!(
            verify_revocation_bundle(&signed, &public, 7, now),
            Err(DistributionError::RevocationRollback)
        );

        let stale = sign_revocations(revocations("2026-08-08T11:00:00Z"));
        let verified =
            verify_revocation_bundle(&stale, &public, 0, now).expect("stale last-known-good");
        assert_eq!(verified.freshness(), RevocationFreshness::Stale);
        assert!(verified.revokes_release("org.example.app", &[5_u8; 32]));
    }

    #[test]
    fn revocation_signature_clock_and_entries_fail_closed() {
        let public = root().verifying_key().to_bytes();
        let now = parse_utc_seconds("2026-08-08T12:00:00Z").expect("now");
        let mut tampered = sign_revocations(revocations("2026-08-09T10:00:00Z"));
        tampered.bundle.revoked_keys[0].reason = "changed".to_owned();
        assert_eq!(
            verify_revocation_bundle(&tampered, &public, 0, now),
            Err(DistributionError::Signature)
        );

        let mut future = revocations("2026-08-10T10:00:00Z");
        future.issued_at = "2026-08-08T12:06:00Z".to_owned();
        assert_eq!(
            verify_revocation_bundle(&sign_revocations(future), &public, 0, now),
            Err(DistributionError::Time)
        );

        let mut duplicate = revocations("2026-08-09T10:00:00Z");
        duplicate
            .revoked_keys
            .push(duplicate.revoked_keys[0].clone());
        assert_eq!(
            verify_revocation_bundle(&sign_revocations(duplicate), &public, 0, now),
            Err(DistributionError::Field)
        );
    }
}

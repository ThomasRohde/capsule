//! Host-owned Capsule Overview projection.
//!
//! This module deliberately has no Tauri command. It verifies one exact,
//! read-only snapshot and projects only bounded display metadata. Executable
//! application bytes, entry assets, permissions, filesystem paths, publisher
//! trust, and raw database access never enter the view model.

use std::{error::Error, fmt};

use serde::Serialize;
use sqlite_capsule_core::{ApplicationReleaseIdentity, CapsuleIdentity};
use sqlite_capsule_launch::{LaunchError, LaunchInspection};
use sqlite_capsule_policy::{LaunchDecision, TrustState};

const VIEW_MODEL_PROFILE: &str = "org.sqlite-capsule.tauri-overview/1";
const V03_APPLICATION_PROFILE: &str = "org.sqlite-capsule.application-profile/0.3";
const V03_INSTANCE_PROFILE: &str = "org.sqlite-capsule.instance-profile/0.3";
const V03_HOST_PROFILE: &str = "org.sqlite-capsule.host-profile/0.3";

/// Bounded metadata that the trusted host shell may render before execution.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CapsuleOverviewViewModel {
    profile: &'static str,
    selection_id: String,
    format_version: String,
    compatibility: OverviewCompatibility,
    application: ApplicationOverview,
    #[serde(skip_serializing_if = "Option::is_none")]
    instance: Option<InstanceOverview>,
    #[serde(skip_serializing_if = "Option::is_none")]
    data_schema: Option<DataSchemaOverview>,
    file: FileOverview,
    actions: OverviewActions,
    authority: OverviewAuthority,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum OverviewCompatibility {
    LegacyV02,
    LifecycleV03,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct ApplicationOverview {
    identity_kind: ApplicationIdentityKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    profile: Option<&'static str>,
    app_id: String,
    app_version: String,
    name: String,
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    safe_image: Option<SafeImageToken>,
    #[serde(skip_serializing_if = "Option::is_none")]
    released_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    minimum_host_profile: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    digest: Option<String>,
    publisher: PublisherOverview,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ApplicationIdentityKind {
    LegacyV02Fallback,
    V03ApplicationProfile,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct InstanceOverview {
    identity_kind: InstanceIdentityKind,
    profile: &'static str,
    capsule_id: String,
    revision_id: String,
    title: String,
    description: String,
    document_kind: String,
    tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    safe_image: Option<SafeImageToken>,
    created_at: String,
    content_updated_at: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum InstanceIdentityKind {
    MutableV03Instance,
}

/// Host-decoded, metadata-free PNG derivative. Raw capsule asset IDs are
/// intentionally not part of the public Overview projection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct SafeImageToken {
    data_url: String,
    width: u32,
    height: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct DataSchemaOverview {
    id: String,
    version: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct PublisherOverview {
    state: PublisherDisplayState,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    key_id: Option<String>,
    host_trust: HostTrustDisplay,
    revocation: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum PublisherDisplayState {
    Unsigned,
    InvalidSignature,
    SignatureValid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum HostTrustDisplay {
    NotApplicable,
    Unknown,
    Trusted,
    Revoked,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct FileOverview {
    display_path: String,
    size_bytes: u64,
    writability: FileWritability,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum FileWritability {
    Writable,
    ReadOnly,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct OverviewActions {
    open: ActionAvailability,
    duplicate: ActionAvailability,
    fork: ActionAvailability,
    compare: ActionAvailability,
    upgrade: ActionAvailability,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct ActionAvailability {
    enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
struct OverviewAuthority {
    metadata_verification: MetadataVerification,
    publisher_trust: PublisherTrustStatus,
    assets_released: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum MetadataVerification {
    ExhaustiveReadOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum PublisherTrustStatus {
    Evaluated,
}

/// Safe outer error for a future trusted-shell integration.
#[derive(Debug)]
pub enum CapsuleOverviewError {
    Verification(LaunchError),
    InconsistentVerifiedIdentity,
}

impl fmt::Display for CapsuleOverviewError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Verification(_) => formatter.write_str("capsule overview verification failed"),
            Self::InconsistentVerifiedIdentity => {
                formatter.write_str("verified capsule identity was inconsistent")
            }
        }
    }
}

impl Error for CapsuleOverviewError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Verification(error) => Some(error),
            Self::InconsistentVerifiedIdentity => None,
        }
    }
}

impl From<LaunchError> for CapsuleOverviewError {
    fn from(error: LaunchError) -> Self {
        Self::Verification(error)
    }
}

impl CapsuleOverviewViewModel {
    /// Build the trusted-shell projection from the exact inspection and policy
    /// evidence retained by `HostState`. This never reopens the source path.
    pub(crate) fn from_launch_inspection(
        selection_id: &str,
        inspection: &LaunchInspection,
        decision: &LaunchDecision,
        assets_released: bool,
    ) -> Result<Self, CapsuleOverviewError> {
        if selection_id.len() != 43
            || !selection_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(CapsuleOverviewError::InconsistentVerifiedIdentity);
        }
        let identity = &inspection.identity;
        let evidence = &inspection.evidence;
        let valid_key = evidence
            .signatures
            .iter()
            .find(|signature| signature.cryptographically_valid && signature.digest_matches);
        let publisher_state = if evidence.signatures.is_empty() {
            PublisherDisplayState::Unsigned
        } else if decision.signature_valid && valid_key.is_some() {
            PublisherDisplayState::SignatureValid
        } else {
            PublisherDisplayState::InvalidSignature
        };
        let host_trust = if matches!(decision.trust_state, TrustState::Revoked) {
            HostTrustDisplay::Revoked
        } else if decision.publisher_trusted {
            HostTrustDisplay::Trusted
        } else if evidence.publisher.is_some() {
            HostTrustDisplay::Unknown
        } else {
            HostTrustDisplay::NotApplicable
        };
        let publisher = PublisherOverview {
            state: publisher_state,
            name: evidence
                .publisher
                .as_ref()
                .map(|publisher| publisher.publisher_name.clone()),
            key_id: valid_key.map(|signature| signature.key_id.clone()),
            host_trust,
            revocation: decision.revocation_status.clone(),
        };
        let authority = OverviewAuthority {
            metadata_verification: MetadataVerification::ExhaustiveReadOnly,
            publisher_trust: PublisherTrustStatus::Evaluated,
            assets_released,
        };
        let file = FileOverview {
            display_path: bounded_display_path(identity),
            size_bytes: identity.bytes,
            writability: std::fs::metadata(&identity.canonical_path)
                .map(|metadata| {
                    if metadata.permissions().readonly() {
                        FileWritability::ReadOnly
                    } else {
                        FileWritability::Writable
                    }
                })
                .unwrap_or(FileWritability::Unknown),
        };
        let actions = OverviewActions {
            open: ActionAvailability {
                enabled: decision.executable_allowed && !assets_released,
                reason: (!decision.executable_allowed)
                    .then_some("Review trust and capabilities before opening."),
            },
            duplicate: disabled_action("Duplicate is delivered in M04."),
            fork: disabled_action("Fork is delivered in M04."),
            compare: disabled_action("Compare is delivered in M05."),
            upgrade: disabled_action("Upgrade is delivered in M07-M08."),
        };
        let digest = evidence.application_digest.as_ref().map(lower_hex);
        match (identity.user_version, identity.format_version.as_str()) {
            (2, "0.2") => Self::legacy_v02(
                selection_id,
                identity,
                digest,
                publisher,
                file,
                actions,
                authority,
            ),
            (3, "0.3") => Self::lifecycle_v03(
                selection_id,
                identity,
                digest,
                publisher,
                file,
                actions,
                authority,
            ),
            _ => Err(CapsuleOverviewError::InconsistentVerifiedIdentity),
        }
    }

    /// Attach only a host-generated PNG derivative projected from the same
    /// retained verified snapshot as this Overview. A raw asset name, external
    /// URL, SVG, or unbounded data value cannot enter this view model.
    pub(crate) fn attach_application_image(
        &mut self,
        data_url: &str,
        width: u32,
        height: u32,
    ) -> Result<(), CapsuleOverviewError> {
        const MAX_DATA_URL_BYTES: usize = 5_700_000;
        if !data_url.starts_with("data:image/png;base64,")
            || data_url.len() > MAX_DATA_URL_BYTES
            || width == 0
            || height == 0
            || width > 1024
            || height > 1024
            || u64::from(width)
                .checked_mul(u64::from(height))
                .and_then(|pixels| pixels.checked_mul(4))
                .is_none_or(|bytes| bytes > 4 * 1024 * 1024)
        {
            return Err(CapsuleOverviewError::InconsistentVerifiedIdentity);
        }
        self.application.safe_image = Some(SafeImageToken {
            data_url: data_url.to_owned(),
            width,
            height,
        });
        Ok(())
    }

    fn legacy_v02(
        selection_id: &str,
        identity: &CapsuleIdentity,
        digest: Option<String>,
        publisher: PublisherOverview,
        file: FileOverview,
        actions: OverviewActions,
        authority: OverviewAuthority,
    ) -> Result<Self, CapsuleOverviewError> {
        let application = &identity.overview.application;
        if !application.legacy_fallback || identity.overview.data_schema.is_some() {
            return Err(CapsuleOverviewError::InconsistentVerifiedIdentity);
        }
        Ok(Self {
            profile: VIEW_MODEL_PROFILE,
            selection_id: selection_id.to_owned(),
            format_version: identity.format_version.clone(),
            compatibility: OverviewCompatibility::LegacyV02,
            application: application_view(
                application,
                ApplicationIdentityKind::LegacyV02Fallback,
                None,
                None,
                digest,
                publisher,
            ),
            // Core inspection carries a compatibility-only synthetic instance
            // for legacy callers. It must not be presented as v0.3 mutable
            // identity in the Cabinet.
            instance: None,
            data_schema: None,
            file,
            actions,
            authority,
        })
    }

    fn lifecycle_v03(
        selection_id: &str,
        identity: &CapsuleIdentity,
        digest: Option<String>,
        publisher: PublisherOverview,
        file: FileOverview,
        actions: OverviewActions,
        authority: OverviewAuthority,
    ) -> Result<Self, CapsuleOverviewError> {
        let application = &identity.overview.application;
        let instance = &identity.overview.instance;
        let revision_id = instance
            .revision_id
            .clone()
            .ok_or(CapsuleOverviewError::InconsistentVerifiedIdentity)?;
        let data_schema = identity
            .overview
            .data_schema
            .as_ref()
            .ok_or(CapsuleOverviewError::InconsistentVerifiedIdentity)?;
        if application.legacy_fallback
            || application.category.is_none()
            || application.released_at.is_none()
        {
            return Err(CapsuleOverviewError::InconsistentVerifiedIdentity);
        }
        Ok(Self {
            profile: VIEW_MODEL_PROFILE,
            selection_id: selection_id.to_owned(),
            format_version: identity.format_version.clone(),
            compatibility: OverviewCompatibility::LifecycleV03,
            application: application_view(
                application,
                ApplicationIdentityKind::V03ApplicationProfile,
                Some(V03_APPLICATION_PROFILE),
                Some(V03_HOST_PROFILE),
                digest,
                publisher,
            ),
            instance: Some(InstanceOverview {
                identity_kind: InstanceIdentityKind::MutableV03Instance,
                profile: V03_INSTANCE_PROFILE,
                capsule_id: instance.capsule_id.clone(),
                revision_id,
                title: instance.title.clone(),
                description: instance.description.clone(),
                document_kind: instance.document_kind.clone(),
                tags: instance.tags.clone(),
                safe_image: None,
                created_at: instance.created_at.clone(),
                content_updated_at: instance.content_updated_at.clone(),
            }),
            data_schema: Some(DataSchemaOverview {
                id: data_schema.data_schema_id.clone(),
                version: data_schema.data_schema_version,
            }),
            file,
            actions,
            authority,
        })
    }
}

fn application_view(
    application: &ApplicationReleaseIdentity,
    identity_kind: ApplicationIdentityKind,
    profile: Option<&'static str>,
    minimum_host_profile: Option<&'static str>,
    digest: Option<String>,
    publisher: PublisherOverview,
) -> ApplicationOverview {
    ApplicationOverview {
        identity_kind,
        profile,
        app_id: application.app_id.clone(),
        app_version: application.app_version.clone(),
        name: application.name.clone(),
        description: application.description.clone(),
        category: application.category.clone(),
        safe_image: None,
        released_at: application.released_at.clone(),
        minimum_host_profile,
        digest,
        publisher,
    }
}

fn disabled_action(reason: &'static str) -> ActionAvailability {
    ActionAvailability {
        enabled: false,
        reason: Some(reason),
    }
}

fn lower_hex(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn bounded_display_path(identity: &CapsuleIdentity) -> String {
    let display = identity.canonical_path.to_string_lossy();
    if display.len() <= 1024 {
        return display.into_owned();
    }
    identity
        .canonical_path
        .file_name()
        .map(|name| format!("…{}{}", std::path::MAIN_SEPARATOR, name.to_string_lossy()))
        .unwrap_or_else(|| "Path too long to display".to_owned())
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicUsize, Ordering},
    };

    use rusqlite::Connection;
    use sqlite_capsule_launch::inspect_launch;

    use super::*;

    static TEST_SEQUENCE: AtomicUsize = AtomicUsize::new(0);
    const SELECTION_ID: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    struct TestCapsule(PathBuf);

    impl TestCapsule {
        fn path(name: &str) -> PathBuf {
            let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            std::env::temp_dir().join(format!(
                "sqlite-capsule-overview-{name}-{}-{sequence}.sqlitecapsule",
                std::process::id()
            ))
        }

        fn legacy_v02() -> Self {
            let path = Self::path("legacy-v02");
            fs::copy(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../..")
                    .join("capsules/diagram-studio.capsule.sqlite"),
                &path,
            )
            .expect("copy verified legacy fixture");
            Self(path)
        }

        fn v03(name: &str) -> Self {
            let path = Self::path(name);
            let connection = Connection::open(&path).expect("create v0.3 capsule");
            connection
                .execute_batch(include_str!("../../../../format/capsule-v0.3.sql"))
                .expect("create v0.3 format");
            connection
                .execute_batch(include_str!(
                    "../../../../format/capsule-signed-app-v0.3.sql"
                ))
                .expect("create v0.3 signed-app extension");
            connection
                .execute_batch(include_str!(
                    "../../../../compatibility/signed-app-v0.3/fixture-v0.3.sql"
                ))
                .expect("seed v0.3 fixture");
            drop(connection);
            Self(path)
        }
    }

    impl Drop for TestCapsule {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    fn decision(inspection: &LaunchInspection) -> LaunchDecision {
        let signature_valid = inspection
            .evidence
            .signatures
            .iter()
            .any(|signature| signature.cryptographically_valid && signature.digest_matches);
        LaunchDecision {
            trust_state: if signature_valid {
                TrustState::SignatureValidUnknownPublisher
            } else {
                TrustState::Unverified
            },
            signature_valid,
            publisher_known: inspection.evidence.publisher.is_some(),
            publisher_trusted: false,
            revocation_status: "not-revoked".to_owned(),
            executable_allowed: false,
            application_digest_hex: inspection
                .evidence
                .application_digest
                .as_ref()
                .map(lower_hex),
            capabilities: BTreeMap::new(),
        }
    }

    fn inspect_overview(path: &Path) -> Result<CapsuleOverviewViewModel, CapsuleOverviewError> {
        let inspection = inspect_launch(path)?;
        CapsuleOverviewViewModel::from_launch_inspection(
            SELECTION_ID,
            &inspection,
            &decision(&inspection),
            false,
        )
    }

    #[test]
    fn legacy_v02_is_explicit_and_never_synthesizes_mutable_identity() {
        let capsule = TestCapsule::legacy_v02();
        let source_before = fs::read(&capsule.0).expect("read source before Overview");

        let overview = inspect_overview(&capsule.0).expect("legacy Overview");

        assert_eq!(overview.format_version, "0.2");
        assert_eq!(overview.compatibility, OverviewCompatibility::LegacyV02);
        assert_eq!(
            overview.application.identity_kind,
            ApplicationIdentityKind::LegacyV02Fallback
        );
        assert_eq!(overview.application.profile, None);
        assert_eq!(overview.application.minimum_host_profile, None);
        assert!(overview.instance.is_none());
        assert!(overview.data_schema.is_none());
        assert!(!overview.authority.assets_released);
        assert_eq!(
            overview.authority.publisher_trust,
            PublisherTrustStatus::Evaluated
        );
        assert_eq!(
            fs::read(&capsule.0).expect("read source after Overview"),
            source_before
        );
    }

    #[test]
    fn verified_v03_projects_application_and_mutable_instance_without_assets() {
        let capsule = TestCapsule::v03("verified-v03");
        let source_before = fs::read(&capsule.0).expect("read source before Overview");

        let overview = inspect_overview(&capsule.0).expect("v0.3 Overview");

        assert_eq!(overview.compatibility, OverviewCompatibility::LifecycleV03);
        assert_eq!(
            overview.application.identity_kind,
            ApplicationIdentityKind::V03ApplicationProfile
        );
        assert_eq!(overview.application.profile, Some(V03_APPLICATION_PROFILE));
        assert_eq!(overview.application.app_id, "org.sqlite-capsule.vector");
        assert_eq!(overview.application.name, "Café Vector");
        assert_eq!(
            overview.application.minimum_host_profile,
            Some(V03_HOST_PROFILE)
        );
        assert_eq!(
            overview.data_schema,
            Some(DataSchemaOverview {
                id: "org.sqlite-capsule.vector-data".to_owned(),
                version: 2,
            })
        );
        let instance = overview.instance.as_ref().expect("v0.3 instance");
        assert_eq!(instance.profile, V03_INSTANCE_PROFILE);
        assert_eq!(
            instance.identity_kind,
            InstanceIdentityKind::MutableV03Instance
        );
        assert_eq!(instance.title, "Vector document");

        let serialized = serde_json::to_string(&overview).expect("serialize Overview");
        assert!(!serialized.contains("<html></html>"));
        assert!(!serialized.contains("entry_asset"));
        assert!(!serialized.contains("permissions"));
        assert!(!serialized.contains("canonical_path"));
        assert!(!serialized.contains("publisher_id"));
        assert!(!serialized.contains("icon_asset"));
        assert!(!serialized.contains("release_notes_doc"));
        assert!(!serialized.contains("instance-icon"));
        assert!(!overview.authority.assets_released);
        assert_eq!(
            fs::read(&capsule.0).expect("read source after Overview"),
            source_before
        );
    }

    #[test]
    fn signature_and_host_trust_states_remain_visibly_separate() {
        let valid = TestCapsule::v03("valid-signature-state");
        let valid_inspection = inspect_launch(&valid.0).expect("inspect valid signature");
        let unknown = CapsuleOverviewViewModel::from_launch_inspection(
            SELECTION_ID,
            &valid_inspection,
            &decision(&valid_inspection),
            false,
        )
        .expect("unknown-publisher Overview");
        assert_eq!(
            unknown.application.publisher.state,
            PublisherDisplayState::SignatureValid
        );
        assert_eq!(
            unknown.application.publisher.host_trust,
            HostTrustDisplay::Unknown
        );

        let mut trusted_decision = decision(&valid_inspection);
        trusted_decision.publisher_trusted = true;
        trusted_decision.trust_state = TrustState::SignedTrustedPublisher;
        let trusted = CapsuleOverviewViewModel::from_launch_inspection(
            SELECTION_ID,
            &valid_inspection,
            &trusted_decision,
            false,
        )
        .expect("trusted-publisher Overview");
        assert_eq!(
            trusted.application.publisher.state,
            PublisherDisplayState::SignatureValid
        );
        assert_eq!(
            trusted.application.publisher.host_trust,
            HostTrustDisplay::Trusted
        );

        let mut revoked_decision = trusted_decision;
        revoked_decision.trust_state = TrustState::Revoked;
        revoked_decision.revocation_status = "revoked".to_owned();
        let revoked = CapsuleOverviewViewModel::from_launch_inspection(
            SELECTION_ID,
            &valid_inspection,
            &revoked_decision,
            false,
        )
        .expect("revoked Overview");
        assert_eq!(
            revoked.application.publisher.state,
            PublisherDisplayState::SignatureValid
        );
        assert_eq!(
            revoked.application.publisher.host_trust,
            HostTrustDisplay::Revoked
        );

        let invalid = TestCapsule::v03("invalid-signature-state");
        let connection = Connection::open(&invalid.0).expect("open invalid signature fixture");
        connection
            .execute(
                "UPDATE capsule_signature SET signature = zeroblob(length(signature))",
                [],
            )
            .expect("invalidate signature without changing signed content");
        drop(connection);
        let invalid_inspection = inspect_launch(&invalid.0).expect("inspect invalid signature");
        let invalid_overview = CapsuleOverviewViewModel::from_launch_inspection(
            SELECTION_ID,
            &invalid_inspection,
            &decision(&invalid_inspection),
            false,
        )
        .expect("invalid-signature Overview");
        assert_eq!(
            invalid_overview.application.publisher.state,
            PublisherDisplayState::InvalidSignature
        );
        assert_ne!(
            invalid_overview.application.publisher.host_trust,
            HostTrustDisplay::Trusted
        );

        let unsigned = TestCapsule::v03("unsigned-state");
        let connection = Connection::open(&unsigned.0).expect("open unsigned fixture");
        connection
            .execute_batch("DROP TABLE capsule_signature; DROP TABLE capsule_publisher;")
            .expect("remove the optional signed-app extension");
        drop(connection);
        let unsigned_inspection = inspect_launch(&unsigned.0).expect("inspect unsigned capsule");
        let unsigned_overview = CapsuleOverviewViewModel::from_launch_inspection(
            SELECTION_ID,
            &unsigned_inspection,
            &decision(&unsigned_inspection),
            false,
        )
        .expect("unsigned Overview");
        assert_eq!(
            unsigned_overview.application.publisher.state,
            PublisherDisplayState::Unsigned
        );
        assert_eq!(
            unsigned_overview.application.publisher.host_trust,
            HostTrustDisplay::NotApplicable
        );
    }

    #[test]
    fn oversized_v03_metadata_fails_before_a_view_model_exists() {
        let capsule = TestCapsule::v03("oversized-v03");
        let connection = Connection::open(&capsule.0).expect("open hostile fixture");
        connection
            .execute_batch(
                "PRAGMA ignore_check_constraints=ON;
                 UPDATE capsule_application SET description = printf('%.*c', 4097, 'x')
                 WHERE id = 1;
                 PRAGMA ignore_check_constraints=OFF;",
            )
            .expect("install oversized metadata");
        drop(connection);
        let source_before = fs::read(&capsule.0).expect("read hostile source");

        assert!(inspect_overview(&capsule.0).is_err());
        assert_eq!(
            fs::read(&capsule.0).expect("read hostile source after rejection"),
            source_before
        );
    }
}

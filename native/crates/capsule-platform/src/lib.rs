//! Platform package-signature verification for host updates.
//!
//! This crate verifies a package file with the operating system's native trust
//! policy and then binds the verified leaf signer to the exact identity pinned
//! by the signed release manifest. It does not fetch, stage, or execute files.

use std::{fs::File, path::Path};

use serde::Serialize;
use thiserror::Error;

pub const AUTHENTICODE_IDENTITY_PREFIX: &str = "authenticode-certificate-sha256:";
pub const DEVELOPER_IDENTITY_PREFIX: &str = "developer-id-team:";
pub const OPENPGP_IDENTITY_PREFIX: &str = "openpgp-primary-fingerprint:";

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformTimestampEvidence {
    AuthenticodeCountersignature,
    None,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PlatformVerificationReport {
    platform_signing: String,
    platform_signing_identity: String,
    signer_subject: String,
    timestamp_evidence: PlatformTimestampEvidence,
    offline_revocation_mode: bool,
    artifact_bytes: u64,
    artifact_sha256: String,
}

impl PlatformVerificationReport {
    pub fn platform_signing(&self) -> &str {
        &self.platform_signing
    }

    pub fn platform_signing_identity(&self) -> &str {
        &self.platform_signing_identity
    }

    pub fn signer_subject(&self) -> &str {
        &self.signer_subject
    }

    pub fn timestamp_evidence(&self) -> PlatformTimestampEvidence {
        self.timestamp_evidence.clone()
    }

    pub fn offline_revocation_mode(&self) -> bool {
        self.offline_revocation_mode
    }

    pub fn artifact_bytes(&self) -> u64 {
        self.artifact_bytes
    }

    pub fn artifact_sha256(&self) -> &str {
        &self.artifact_sha256
    }
}

/// A platform-verified artifact whose read handle remains open with write and
/// delete sharing denied. Keep this guard alive through any path-based launch
/// operation so the verified file cannot be replaced between trust evaluation
/// and the operating-system handoff.
#[derive(Debug)]
pub struct LockedPlatformArtifact {
    #[allow(dead_code)]
    locked_file: File,
    report: PlatformVerificationReport,
}

impl LockedPlatformArtifact {
    pub fn report(&self) -> &PlatformVerificationReport {
        &self.report
    }

    pub fn into_report(self) -> PlatformVerificationReport {
        self.report
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PlatformVerificationError {
    #[error("platform-signing policy field is invalid")]
    Field,
    #[error("this platform-signing adapter is unavailable on the current host")]
    Unsupported,
    #[error("the operating system rejected the package signature with status {0}")]
    Untrusted(String),
    #[error("the platform trust provider did not expose verified signer data")]
    SignerData,
    #[error("the verified signer certificate could not be identified")]
    Certificate,
    #[error("the verified platform signer does not match the signed release identity")]
    IdentityMismatch,
    #[error("the signed release requires trusted timestamp evidence")]
    TimestampRequired,
    #[error("the platform trust-provider state could not be closed: {0}")]
    StateClose(String),
}

/// Validate a signed-manifest identity without invoking a platform adapter.
pub fn validate_platform_signing_identity(
    platform_signing: &str,
    identity: &str,
) -> Result<(), PlatformVerificationError> {
    let valid = match platform_signing {
        "authenticode" => identity
            .strip_prefix(AUTHENTICODE_IDENTITY_PREFIX)
            .is_some_and(|value| lowercase_hex(value, 64)),
        "developer-id-notarized" => identity
            .strip_prefix(DEVELOPER_IDENTITY_PREFIX)
            .is_some_and(|value| {
                value.len() == 10
                    && value
                        .bytes()
                        .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
            }),
        "linux-detached" => identity
            .strip_prefix(OPENPGP_IDENTITY_PREFIX)
            .is_some_and(|value| {
                matches!(value.len(), 40 | 64)
                    && value
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'A'..=b'F').contains(&byte))
            }),
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(PlatformVerificationError::Field)
    }
}

/// Verify native package trust and require an exact match with the signed
/// release identity. Verification is deliberately offline and fail-closed.
pub fn verify_platform_artifact(
    path: &Path,
    platform_signing: &str,
    identity: &str,
    timestamp_required: bool,
) -> Result<PlatformVerificationReport, PlatformVerificationError> {
    verify_platform_artifact_locked(path, platform_signing, identity, timestamp_required)
        .map(LockedPlatformArtifact::into_report)
}

/// Verify native package trust while retaining the no-write/no-delete-sharing
/// file handle used for the complete hash/trust/hash sequence.
pub fn verify_platform_artifact_locked(
    path: &Path,
    platform_signing: &str,
    identity: &str,
    timestamp_required: bool,
) -> Result<LockedPlatformArtifact, PlatformVerificationError> {
    validate_platform_signing_identity(platform_signing, identity)?;
    #[cfg(windows)]
    {
        if platform_signing != "authenticode" {
            return Err(PlatformVerificationError::Unsupported);
        }
        let (locked_file, report) =
            windows::verify_authenticode_locked(path, Some(identity), timestamp_required)?;
        Ok(LockedPlatformArtifact {
            locked_file,
            report,
        })
    }
    #[cfg(not(windows))]
    {
        let _ = (path, timestamp_required);
        Err(PlatformVerificationError::Unsupported)
    }
}

/// Inspect a package under native trust policy and return its canonical signer
/// identity while retaining the no-replacement file guard. Callers must still
/// compare that identity and exact digest with separately signed policy before
/// accepting or launching the artifact.
pub fn inspect_platform_artifact_locked(
    path: &Path,
    platform_signing: &str,
    timestamp_required: bool,
) -> Result<LockedPlatformArtifact, PlatformVerificationError> {
    #[cfg(windows)]
    {
        if platform_signing != "authenticode" {
            return Err(PlatformVerificationError::Unsupported);
        }
        let (locked_file, report) =
            windows::verify_authenticode_locked(path, None, timestamp_required)?;
        Ok(LockedPlatformArtifact {
            locked_file,
            report,
        })
    }
    #[cfg(not(windows))]
    {
        let _ = (path, platform_signing, timestamp_required);
        Err(PlatformVerificationError::Unsupported)
    }
}

fn lowercase_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(windows)]
mod windows {
    use std::{
        ffi::c_void,
        fs::OpenOptions,
        io::{Read, Seek, SeekFrom},
        mem::size_of,
        os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, io::AsRawHandle},
        path::Path,
        ptr,
    };

    use sha2::{Digest, Sha256};

    use windows_sys::{
        Win32::{
            Foundation::{FreeLibrary, HANDLE, INVALID_HANDLE_VALUE},
            Security::{
                Cryptography::{
                    CERT_CONTEXT, CERT_NAME_SIMPLE_DISPLAY_TYPE, CERT_SHA256_HASH_PROP_ID,
                    CertGetCertificateContextProperty, CertGetNameStringW,
                },
                WinTrust::{
                    CRYPT_PROVIDER_DATA, CRYPT_PROVIDER_SGNR, WINTRUST_ACTION_GENERIC_VERIFY_V2,
                    WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_FILE_INFO,
                    WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_FILE, WTD_DISABLE_MD2_MD4,
                    WTD_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT, WTD_REVOKE_WHOLECHAIN,
                    WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY, WTD_UI_NONE,
                    WTD_UICONTEXT_INSTALL, WinVerifyTrust,
                },
            },
            Storage::FileSystem::FILE_SHARE_READ,
            System::LibraryLoader::{GetProcAddress, LoadLibraryW},
        },
        core::GUID,
    };

    use super::{
        AUTHENTICODE_IDENTITY_PREFIX, PlatformTimestampEvidence, PlatformVerificationError,
        PlatformVerificationReport,
    };

    type ProviderDataFn = unsafe extern "system" fn(HANDLE) -> *mut CRYPT_PROVIDER_DATA;
    type ProviderSignerFn = unsafe extern "system" fn(
        *mut CRYPT_PROVIDER_DATA,
        u32,
        i32,
        u32,
    ) -> *mut CRYPT_PROVIDER_SGNR;

    struct WintrustHelpers {
        module: windows_sys::Win32::Foundation::HMODULE,
        provider_data: ProviderDataFn,
        provider_signer: ProviderSignerFn,
    }

    impl WintrustHelpers {
        fn load() -> Result<Self, PlatformVerificationError> {
            let library = "wintrust.dll\0".encode_utf16().collect::<Vec<_>>();
            let module = unsafe { LoadLibraryW(library.as_ptr()) };
            if module.is_null() {
                return Err(PlatformVerificationError::SignerData);
            }
            let provider_data =
                unsafe { GetProcAddress(module, c"WTHelperProvDataFromStateData".as_ptr().cast()) };
            let provider_signer = unsafe {
                GetProcAddress(module, c"WTHelperGetProvSignerFromChain".as_ptr().cast())
            };
            let (Some(provider_data), Some(provider_signer)) = (provider_data, provider_signer)
            else {
                unsafe {
                    FreeLibrary(module);
                }
                return Err(PlatformVerificationError::SignerData);
            };
            Ok(Self {
                module,
                provider_data: unsafe {
                    std::mem::transmute::<unsafe extern "system" fn() -> isize, ProviderDataFn>(
                        provider_data,
                    )
                },
                provider_signer: unsafe {
                    std::mem::transmute::<unsafe extern "system" fn() -> isize, ProviderSignerFn>(
                        provider_signer,
                    )
                },
            })
        }
    }

    impl Drop for WintrustHelpers {
        fn drop(&mut self) {
            unsafe {
                FreeLibrary(self.module);
            }
        }
    }

    pub(super) fn verify_authenticode_locked(
        path: &Path,
        expected_identity: Option<&str>,
        timestamp_required: bool,
    ) -> Result<(std::fs::File, PlatformVerificationReport), PlatformVerificationError> {
        // Hold a read handle that denies write/delete sharing for the complete
        // hash -> trust-provider -> hash sequence. This binds the trust result
        // to the exact bytes reported to the caller instead of only a path.
        let mut locked_file = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(path)
            .map_err(|_| PlatformVerificationError::SignerData)?;
        let (artifact_bytes, artifact_sha256) = hash_file(&mut locked_file)?;
        locked_file
            .seek(SeekFrom::Start(0))
            .map_err(|_| PlatformVerificationError::SignerData)?;
        let mut wide_path = path.as_os_str().encode_wide().collect::<Vec<_>>();
        if wide_path.is_empty() || wide_path.contains(&0) {
            return Err(PlatformVerificationError::Field);
        }
        wide_path.push(0);
        let mut file = WINTRUST_FILE_INFO {
            cbStruct: size_of::<WINTRUST_FILE_INFO>() as u32,
            pcwszFilePath: wide_path.as_ptr(),
            hFile: locked_file.as_raw_handle().cast(),
            pgKnownSubject: ptr::null_mut(),
        };
        let mut data = WINTRUST_DATA {
            cbStruct: size_of::<WINTRUST_DATA>() as u32,
            pPolicyCallbackData: ptr::null_mut(),
            pSIPClientData: ptr::null_mut(),
            dwUIChoice: WTD_UI_NONE,
            fdwRevocationChecks: WTD_REVOKE_WHOLECHAIN,
            dwUnionChoice: WTD_CHOICE_FILE,
            Anonymous: WINTRUST_DATA_0 { pFile: &mut file },
            dwStateAction: WTD_STATEACTION_VERIFY,
            hWVTStateData: ptr::null_mut(),
            pwszURLReference: ptr::null_mut(),
            dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL
                | WTD_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT
                | WTD_DISABLE_MD2_MD4,
            dwUIContext: WTD_UICONTEXT_INSTALL,
            pSignatureSettings: ptr::null_mut(),
        };
        let mut action: GUID = WINTRUST_ACTION_GENERIC_VERIFY_V2;
        let verify_status = unsafe {
            WinVerifyTrust(
                INVALID_HANDLE_VALUE,
                &mut action,
                (&mut data as *mut WINTRUST_DATA).cast::<c_void>(),
            )
        };
        let report = if verify_status == 0 {
            extract_report(
                &data,
                expected_identity,
                timestamp_required,
                artifact_bytes,
                artifact_sha256.clone(),
            )
        } else {
            Err(PlatformVerificationError::Untrusted(status(verify_status)))
        };
        data.dwStateAction = WTD_STATEACTION_CLOSE;
        let close_status = unsafe {
            WinVerifyTrust(
                INVALID_HANDLE_VALUE,
                &mut action,
                (&mut data as *mut WINTRUST_DATA).cast::<c_void>(),
            )
        };
        if close_status != 0 {
            return Err(PlatformVerificationError::StateClose(status(close_status)));
        }
        locked_file
            .seek(SeekFrom::Start(0))
            .map_err(|_| PlatformVerificationError::SignerData)?;
        let after = hash_file(&mut locked_file)?;
        if after != (artifact_bytes, artifact_sha256) {
            return Err(PlatformVerificationError::SignerData);
        }
        report.map(|report| (locked_file, report))
    }

    fn extract_report(
        data: &WINTRUST_DATA,
        expected_identity: Option<&str>,
        timestamp_required: bool,
        artifact_bytes: u64,
        artifact_sha256: String,
    ) -> Result<PlatformVerificationReport, PlatformVerificationError> {
        if data.hWVTStateData.is_null() {
            return Err(PlatformVerificationError::SignerData);
        }
        let helpers = WintrustHelpers::load()?;
        let provider_data = unsafe { (helpers.provider_data)(data.hWVTStateData) };
        if provider_data.is_null() {
            return Err(PlatformVerificationError::SignerData);
        }
        let signer = unsafe { (helpers.provider_signer)(provider_data, 0, 0, 0) };
        if signer.is_null() {
            return Err(PlatformVerificationError::SignerData);
        }
        let signer = unsafe { &*signer };
        if signer.dwError != 0 || signer.csCertChain == 0 || signer.pasCertChain.is_null() {
            return Err(PlatformVerificationError::SignerData);
        }
        let certificate = unsafe { &*signer.pasCertChain };
        if certificate.dwError != 0 || certificate.pCert.is_null() {
            return Err(PlatformVerificationError::Certificate);
        }
        let fingerprint = certificate_sha256(certificate.pCert)?;
        let identity = format!("{AUTHENTICODE_IDENTITY_PREFIX}{fingerprint}");
        if expected_identity.is_some_and(|expected| expected != identity) {
            return Err(PlatformVerificationError::IdentityMismatch);
        }
        let timestamp_evidence = if signer.csCounterSigners > 0
            && !signer.pasCounterSigners.is_null()
            && unsafe { (*signer.pasCounterSigners).dwError == 0 }
        {
            PlatformTimestampEvidence::AuthenticodeCountersignature
        } else {
            PlatformTimestampEvidence::None
        };
        if timestamp_required && timestamp_evidence == PlatformTimestampEvidence::None {
            return Err(PlatformVerificationError::TimestampRequired);
        }
        Ok(PlatformVerificationReport {
            platform_signing: "authenticode".to_owned(),
            platform_signing_identity: identity,
            signer_subject: certificate_subject(certificate.pCert)?,
            timestamp_evidence,
            offline_revocation_mode: true,
            artifact_bytes,
            artifact_sha256,
        })
    }

    fn hash_file(file: &mut std::fs::File) -> Result<(u64, String), PlatformVerificationError> {
        let mut hasher = Sha256::new();
        let mut length = 0_u64;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = file
                .read(&mut buffer)
                .map_err(|_| PlatformVerificationError::SignerData)?;
            if read == 0 {
                break;
            }
            length = length
                .checked_add(read as u64)
                .ok_or(PlatformVerificationError::SignerData)?;
            hasher.update(&buffer[..read]);
        }
        Ok((length, lower_hex(&hasher.finalize())))
    }

    fn certificate_sha256(
        certificate: *const CERT_CONTEXT,
    ) -> Result<String, PlatformVerificationError> {
        let mut bytes = [0_u8; 32];
        let mut length = bytes.len() as u32;
        let ok = unsafe {
            CertGetCertificateContextProperty(
                certificate,
                CERT_SHA256_HASH_PROP_ID,
                bytes.as_mut_ptr().cast::<c_void>(),
                &mut length,
            )
        };
        if ok == 0 || length != bytes.len() as u32 {
            return Err(PlatformVerificationError::Certificate);
        }
        Ok(lower_hex(&bytes))
    }

    fn certificate_subject(
        certificate: *const CERT_CONTEXT,
    ) -> Result<String, PlatformVerificationError> {
        let length = unsafe {
            CertGetNameStringW(
                certificate,
                CERT_NAME_SIMPLE_DISPLAY_TYPE,
                0,
                ptr::null(),
                ptr::null_mut(),
                0,
            )
        };
        if !(2..=4096).contains(&length) {
            return Err(PlatformVerificationError::Certificate);
        }
        let mut name = vec![0_u16; length as usize];
        let written = unsafe {
            CertGetNameStringW(
                certificate,
                CERT_NAME_SIMPLE_DISPLAY_TYPE,
                0,
                ptr::null(),
                name.as_mut_ptr(),
                length,
            )
        };
        if written != length || name.last() != Some(&0) {
            return Err(PlatformVerificationError::Certificate);
        }
        name.pop();
        let name = String::from_utf16(&name).map_err(|_| PlatformVerificationError::Certificate)?;
        if name.is_empty() || name.contains(['\r', '\n', '\0']) {
            return Err(PlatformVerificationError::Certificate);
        }
        Ok(name)
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

    fn status(value: i32) -> String {
        format!("0x{:08x}", value as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signing_identities_are_class_specific_and_canonical() {
        assert!(
            validate_platform_signing_identity(
                "authenticode",
                &format!("{AUTHENTICODE_IDENTITY_PREFIX}{}", "ab".repeat(32)),
            )
            .is_ok()
        );
        assert!(
            validate_platform_signing_identity(
                "developer-id-notarized",
                "developer-id-team:AB12CD34EF",
            )
            .is_ok()
        );
        assert!(
            validate_platform_signing_identity(
                "linux-detached",
                &format!("{OPENPGP_IDENTITY_PREFIX}{}", "AB".repeat(20)),
            )
            .is_ok()
        );
        for (class, identity) in [
            ("authenticode", "authenticode-certificate-sha256:ABCDEF"),
            ("developer-id-notarized", "developer-id-team:too-short"),
            ("linux-detached", "openpgp-primary-fingerprint:abcdef"),
            ("unknown", "anything"),
        ] {
            assert_eq!(
                validate_platform_signing_identity(class, identity),
                Err(PlatformVerificationError::Field)
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn authenticode_adapter_pins_a_real_signer_and_rejects_unsigned_bytes() {
        // These are embedded Authenticode signatures, unlike many Windows
        // system binaries whose trust is supplied only by an OS catalog.
        let signed_binary = [
            Path::new(r"C:\Program Files\PowerShell\7\pwsh.exe"),
            Path::new(r"C:\Program Files\nodejs\node.exe"),
        ]
        .into_iter()
        .find(|path| path.exists());
        let Some(signed_binary) = signed_binary else {
            return;
        };
        let (_, inspected) = windows::verify_authenticode_locked(signed_binary, None, false)
            .expect("installed binary should have trusted embedded Authenticode");
        assert!(inspected.offline_revocation_mode());
        let pinned = verify_platform_artifact(
            signed_binary,
            "authenticode",
            inspected.platform_signing_identity(),
            true,
        )
        .expect("exact signer pin and countersignature should verify");
        assert_eq!(
            pinned.platform_signing_identity(),
            inspected.platform_signing_identity()
        );
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let locked_copy = std::env::temp_dir().join(format!(
            "sqlite-capsule-authenticode-lock-{}-{nonce}.exe",
            std::process::id()
        ));
        std::fs::copy(signed_binary, &locked_copy).expect("copy signed fixture");
        let guard = verify_platform_artifact_locked(
            &locked_copy,
            "authenticode",
            inspected.platform_signing_identity(),
            true,
        )
        .expect("copied signed fixture should remain verified and locked");
        assert_eq!(
            guard.report().artifact_sha256(),
            inspected.artifact_sha256()
        );
        assert!(
            std::fs::OpenOptions::new()
                .write(true)
                .open(&locked_copy)
                .is_err(),
            "write access must remain denied while the verification guard is alive"
        );
        drop(guard);
        std::fs::OpenOptions::new()
            .write(true)
            .open(&locked_copy)
            .expect("write access should be available after dropping the guard");
        std::fs::remove_file(&locked_copy).expect("remove signed fixture copy");
        assert_eq!(
            verify_platform_artifact(
                signed_binary,
                "authenticode",
                &format!("{AUTHENTICODE_IDENTITY_PREFIX}{}", "00".repeat(32)),
                false,
            ),
            Err(PlatformVerificationError::IdentityMismatch)
        );

        let unsigned = std::env::temp_dir().join(format!(
            "sqlite-capsule-authenticode-{}.bin",
            std::process::id()
        ));
        std::fs::write(&unsigned, b"unsigned package bytes").expect("write unsigned fixture");
        let result = verify_platform_artifact(
            &unsigned,
            "authenticode",
            inspected.platform_signing_identity(),
            false,
        );
        let _ = std::fs::remove_file(unsigned);
        assert!(matches!(
            result,
            Err(PlatformVerificationError::Untrusted(_))
        ));
    }
}

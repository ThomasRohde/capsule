//! Host-only installer launch coordination.
//!
//! This crate accepts only an opaque `PreparedInstallation`, repeats native
//! platform verification while retaining the no-replacement file handle,
//! requires an equally verified preserved prior installer, and records durable
//! launch/health state around the operating-system handoff. Capsule application
//! code has no handle to this API.

use std::path::{Path, PathBuf};

use sqlite_capsule_platform::{
    LockedPlatformArtifact, PlatformTimestampEvidence, PlatformVerificationError,
    verify_platform_artifact_locked,
};
use sqlite_capsule_update::{
    PreparedInstallation, PreparedRollback, RollbackReason, UpdateStageError, UpdateStageState,
    UpdateStager,
};
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallerKind {
    WindowsMsi,
    WindowsNsis,
    MacPkg,
    LinuxDeb,
    LinuxRpm,
    LinuxAppImage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallerLaunchPlan {
    artifact_path: PathBuf,
    kind: InstallerKind,
}

impl InstallerLaunchPlan {
    pub fn artifact_path(&self) -> &Path {
        &self.artifact_path
    }

    pub fn kind(&self) -> InstallerKind {
        self.kind
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallerLaunchReceipt {
    pub stage_id: String,
    pub version: String,
    pub state: UpdateStageState,
}

pub const BOOTSTRAP_MSI_NAME: &str = "sqlite-capsule-host-current.msi";
pub const BOOTSTRAP_NSIS_NAME: &str = "sqlite-capsule-host-current.exe";

/// A signed installer retained by the bootstrap installer. Its native lock is
/// held while the update stager copies and hashes the file.
#[derive(Debug)]
pub struct LockedInstallerSource {
    path: PathBuf,
    #[allow(dead_code)]
    guard: LockedPlatformArtifact,
}

impl LockedInstallerSource {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn staged_name(&self) -> &str {
        self.path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("bootstrap installer names are static UTF-8")
    }
}

#[derive(Debug, Error)]
pub enum InstallerLaunchError {
    #[error("installer launch input is invalid")]
    Field,
    #[error("a verified preserved prior installer is required before update launch")]
    RollbackUnavailable,
    #[error("the staged package no longer matches its signed platform evidence")]
    PlatformEvidence,
    #[error("installer launch is unsupported on this platform")]
    Unsupported,
    #[error("bootstrap installer cache is ambiguous")]
    BootstrapAmbiguous,
    #[error("bootstrap installer version does not match the running host")]
    BootstrapVersion,
    #[error("candidate installer version does not match the signed release")]
    CandidateVersion,
    #[error("preserved rollback installer version does not match durable staging evidence")]
    RollbackVersion,
    #[error("bootstrap installer metadata could not be read: {0}")]
    Metadata(String),
    #[error("the operating system did not launch the verified installer: {0}")]
    Launch(String),
    #[error(transparent)]
    Platform(#[from] PlatformVerificationError),
    #[error(transparent)]
    Stage(#[from] UpdateStageError),
}

pub trait PlatformInstallerLauncher {
    fn launch(&self, plan: &InstallerLaunchPlan) -> Result<(), String>;
}

/// Discover an installer retained by the clean-install package. Exactly one
/// class-specific cache file may exist. It must pass the same native signer and
/// timestamp policy as the accepted update and carry the exact running host
/// version in signed package metadata.
#[cfg(windows)]
pub fn discover_bootstrap_installer(
    cache_directory: &Path,
    running_version: &str,
    platform_signing: &str,
    platform_identity: &str,
    timestamp_required: bool,
) -> Result<Option<LockedInstallerSource>, InstallerLaunchError> {
    if platform_signing != "authenticode" || !valid_version(running_version) {
        return Err(InstallerLaunchError::Field);
    }
    let mut sources = Vec::new();
    for name in [BOOTSTRAP_MSI_NAME, BOOTSTRAP_NSIS_NAME] {
        let path = cache_directory.join(name);
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                return Err(InstallerLaunchError::PlatformEvidence);
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(InstallerLaunchError::Metadata(error.to_string())),
        }
        sources.push(lock_installer_source(
            &path,
            running_version,
            platform_signing,
            platform_identity,
            timestamp_required,
        )?);
    }
    match sources.len() {
        0 => Ok(None),
        1 => Ok(sources.pop()),
        _ => Err(InstallerLaunchError::BootstrapAmbiguous),
    }
}

#[cfg(not(windows))]
pub fn discover_bootstrap_installer(
    _cache_directory: &Path,
    _running_version: &str,
    _platform_signing: &str,
    _platform_identity: &str,
    _timestamp_required: bool,
) -> Result<Option<LockedInstallerSource>, InstallerLaunchError> {
    Err(InstallerLaunchError::Unsupported)
}

/// Lock and verify one retained current-version installer before the stager
/// copies it. The package version comes from signed MSI/PE metadata, not its
/// filename or adjacent mutable state.
#[cfg(windows)]
pub fn lock_installer_source(
    path: &Path,
    running_version: &str,
    platform_signing: &str,
    platform_identity: &str,
    timestamp_required: bool,
) -> Result<LockedInstallerSource, InstallerLaunchError> {
    if platform_signing != "authenticode" || !valid_version(running_version) {
        return Err(InstallerLaunchError::Field);
    }
    let metadata = path
        .symlink_metadata()
        .map_err(|error| InstallerLaunchError::Metadata(error.to_string()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(InstallerLaunchError::PlatformEvidence);
    }
    let kind = if path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".msi"))
    {
        InstallerKind::WindowsMsi
    } else if path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".exe"))
    {
        InstallerKind::WindowsNsis
    } else {
        return Err(InstallerLaunchError::Field);
    };
    let guard = verify_platform_artifact_locked(
        path,
        platform_signing,
        platform_identity,
        timestamp_required,
    )?;
    if !windows::package_version_matches(path, kind, running_version)? {
        return Err(InstallerLaunchError::BootstrapVersion);
    }
    Ok(LockedInstallerSource {
        path: path.to_owned(),
        guard,
    })
}

#[cfg(not(windows))]
pub fn lock_installer_source(
    _path: &Path,
    _running_version: &str,
    _platform_signing: &str,
    _platform_identity: &str,
    _timestamp_required: bool,
) -> Result<LockedInstallerSource, InstallerLaunchError> {
    Err(InstallerLaunchError::Unsupported)
}

/// Reverify and launch one exact prepared update. Both the new package and the
/// preserved rollback package must match the same pinned platform signer. The
/// file guards remain alive through `launcher.launch`.
pub fn launch_prepared_with<L: PlatformInstallerLauncher>(
    stager: &UpdateStager,
    prepared: PreparedInstallation,
    now_unix: u64,
    launcher: &L,
) -> Result<InstallerLaunchReceipt, InstallerLaunchError> {
    if now_unix == 0 || now_unix == u64::MAX {
        return Err(InstallerLaunchError::Field);
    }
    let record = &prepared.stage().record;
    let previous_path = prepared
        .previous_installer_path()
        .ok_or(InstallerLaunchError::RollbackUnavailable)?;
    let previous_bytes = record
        .previous_installer_bytes
        .ok_or(InstallerLaunchError::RollbackUnavailable)?;
    let previous_sha256 = record
        .previous_installer_sha256
        .as_deref()
        .ok_or(InstallerLaunchError::RollbackUnavailable)?;
    let artifact = prepared.candidate().artifact();
    let kind = installer_kind(&record.artifact_name, &artifact.platform_signing)?;

    let artifact_guard = verify_platform_artifact_locked(
        prepared.artifact_path(),
        &artifact.platform_signing,
        &artifact.platform_signing_identity,
        artifact.platform_timestamp_required,
    )?;
    require_exact_platform_evidence(
        &artifact_guard,
        artifact.bytes,
        &artifact.sha256,
        &artifact.platform_signing,
        &artifact.platform_signing_identity,
        artifact.platform_timestamp_required,
    )?;
    #[cfg(windows)]
    if !windows::package_version_matches(prepared.artifact_path(), kind, &record.version)? {
        return Err(InstallerLaunchError::CandidateVersion);
    }
    let previous_guard = verify_platform_artifact_locked(
        previous_path,
        &artifact.platform_signing,
        &artifact.platform_signing_identity,
        artifact.platform_timestamp_required,
    )?;
    require_exact_platform_evidence(
        &previous_guard,
        previous_bytes,
        previous_sha256,
        &artifact.platform_signing,
        &artifact.platform_signing_identity,
        artifact.platform_timestamp_required,
    )?;

    let stage_id = record.stage_id.clone();
    let version = record.version.clone();
    stager.mark_installer_started(&stage_id, now_unix)?;
    let plan = InstallerLaunchPlan {
        artifact_path: prepared.artifact_path().to_owned(),
        kind,
    };
    if let Err(error) = launcher.launch(&plan) {
        stager.mark_rollback_required(&stage_id, RollbackReason::InstallerFailed, now_unix + 1)?;
        return Err(InstallerLaunchError::Launch(error));
    }
    let awaiting = stager.mark_awaiting_health(&stage_id, now_unix + 1)?;
    drop(previous_guard);
    drop(artifact_guard);
    Ok(InstallerLaunchReceipt {
        stage_id,
        version,
        state: awaiting.state,
    })
}

#[cfg(windows)]
pub fn launch_prepared(
    stager: &UpdateStager,
    prepared: PreparedInstallation,
    now_unix: u64,
) -> Result<InstallerLaunchReceipt, InstallerLaunchError> {
    launch_prepared_with(stager, prepared, now_unix, &windows::WindowsShellLauncher)
}

/// Reverify and launch the exact preserved installer from a rollback-required
/// stage. The native guard remains alive through the operating-system handoff,
/// and the signed package version must match the version captured at staging.
#[cfg(windows)]
pub fn launch_rollback_with<L: PlatformInstallerLauncher>(
    stager: &UpdateStager,
    prepared: PreparedRollback,
    now_unix: u64,
    launcher: &L,
) -> Result<InstallerLaunchReceipt, InstallerLaunchError> {
    if now_unix == 0 || now_unix == u64::MAX {
        return Err(InstallerLaunchError::Field);
    }
    let record = &prepared.stage().record;
    let installer_name = record
        .previous_installer_name
        .as_deref()
        .ok_or(InstallerLaunchError::RollbackUnavailable)?;
    let rollback_version = record
        .previous_installer_version
        .as_deref()
        .ok_or(InstallerLaunchError::RollbackUnavailable)?;
    let installer_bytes = record
        .previous_installer_bytes
        .ok_or(InstallerLaunchError::RollbackUnavailable)?;
    let installer_sha256 = record
        .previous_installer_sha256
        .as_deref()
        .ok_or(InstallerLaunchError::RollbackUnavailable)?;
    let kind = installer_kind(installer_name, &record.platform_signing)?;
    let guard = verify_platform_artifact_locked(
        prepared.installer_path(),
        &record.platform_signing,
        &record.platform_signing_identity,
        record.platform_timestamp_required,
    )?;
    require_exact_platform_evidence(
        &guard,
        installer_bytes,
        installer_sha256,
        &record.platform_signing,
        &record.platform_signing_identity,
        record.platform_timestamp_required,
    )?;
    if !windows::package_version_matches(prepared.installer_path(), kind, rollback_version)? {
        return Err(InstallerLaunchError::RollbackVersion);
    }

    let stage_id = record.stage_id.clone();
    let version = rollback_version.to_owned();
    stager.mark_rollback_started(&stage_id, rollback_version, now_unix)?;
    let plan = InstallerLaunchPlan {
        artifact_path: prepared.installer_path().to_owned(),
        kind,
    };
    if let Err(error) = launcher.launch(&plan) {
        stager.mark_rollback_failed(
            &stage_id,
            RollbackReason::RollbackInstallerFailed,
            now_unix + 1,
        )?;
        return Err(InstallerLaunchError::Launch(error));
    }
    let awaiting = stager.mark_awaiting_rollback_health(&stage_id, now_unix + 1)?;
    drop(guard);
    Ok(InstallerLaunchReceipt {
        stage_id,
        version,
        state: awaiting.state,
    })
}

#[cfg(windows)]
pub fn launch_rollback(
    stager: &UpdateStager,
    prepared: PreparedRollback,
    now_unix: u64,
) -> Result<InstallerLaunchReceipt, InstallerLaunchError> {
    launch_rollback_with(stager, prepared, now_unix, &windows::WindowsShellLauncher)
}

#[cfg(not(windows))]
pub fn launch_rollback(
    _stager: &UpdateStager,
    _prepared: PreparedRollback,
    _now_unix: u64,
) -> Result<InstallerLaunchReceipt, InstallerLaunchError> {
    Err(InstallerLaunchError::Unsupported)
}

#[cfg(not(windows))]
pub fn launch_prepared(
    _stager: &UpdateStager,
    _prepared: PreparedInstallation,
    _now_unix: u64,
) -> Result<InstallerLaunchReceipt, InstallerLaunchError> {
    Err(InstallerLaunchError::Unsupported)
}

fn installer_kind(
    artifact_name: &str,
    platform_signing: &str,
) -> Result<InstallerKind, InstallerLaunchError> {
    match (platform_signing, artifact_name) {
        ("authenticode", name) if name.ends_with(".msi") => Ok(InstallerKind::WindowsMsi),
        ("authenticode", name) if name.ends_with(".exe") => Ok(InstallerKind::WindowsNsis),
        ("developer-id-notarized", name) if name.ends_with(".pkg") => Ok(InstallerKind::MacPkg),
        ("linux-detached", name) if name.ends_with(".deb") => Ok(InstallerKind::LinuxDeb),
        ("linux-detached", name) if name.ends_with(".rpm") => Ok(InstallerKind::LinuxRpm),
        ("linux-detached", name) if name.ends_with(".AppImage") => Ok(InstallerKind::LinuxAppImage),
        _ => Err(InstallerLaunchError::Field),
    }
}

fn valid_version(value: &str) -> bool {
    let parts = value.split('.').collect::<Vec<_>>();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && (part.len() == 1 || !part.starts_with('0'))
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && part.parse::<u16>().is_ok()
        })
}

fn require_exact_platform_evidence(
    guard: &LockedPlatformArtifact,
    expected_bytes: u64,
    expected_sha256: &str,
    expected_class: &str,
    expected_identity: &str,
    timestamp_required: bool,
) -> Result<(), InstallerLaunchError> {
    let report = guard.report();
    if report.artifact_bytes() != expected_bytes
        || report.artifact_sha256() != expected_sha256
        || report.platform_signing() != expected_class
        || report.platform_signing_identity() != expected_identity
        || !report.offline_revocation_mode()
        || (timestamp_required && report.timestamp_evidence() == PlatformTimestampEvidence::None)
    {
        return Err(InstallerLaunchError::PlatformEvidence);
    }
    Ok(())
}

#[cfg(windows)]
mod windows {
    use std::{
        ffi::c_void,
        ffi::{OsStr, OsString},
        mem::size_of,
        os::windows::ffi::{OsStrExt, OsStringExt},
        path::{Path, PathBuf},
        ptr,
    };

    use windows_sys::Win32::{
        Foundation::{
            CloseHandle, ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, GetLastError,
        },
        Storage::FileSystem::{
            GetFileVersionInfoSizeW, GetFileVersionInfoW, VS_FFI_SIGNATURE, VS_FIXEDFILEINFO,
            VerQueryValueW,
        },
        System::{
            ApplicationInstallationAndServicing::{
                MSIDBOPEN_READONLY, MSIHANDLE, MsiCloseHandle, MsiDatabaseOpenViewW,
                MsiOpenDatabaseW, MsiRecordGetStringW, MsiViewExecute, MsiViewFetch,
            },
            Com::{
                COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
            },
            SystemInformation::GetSystemDirectoryW,
        },
        UI::{
            Shell::{
                SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
                ShellExecuteExW,
            },
            WindowsAndMessaging::SW_SHOW,
        },
    };

    use super::{
        InstallerKind, InstallerLaunchError, InstallerLaunchPlan, PlatformInstallerLauncher,
    };

    pub(super) struct WindowsShellLauncher;

    impl PlatformInstallerLauncher for WindowsShellLauncher {
        fn launch(&self, plan: &InstallerLaunchPlan) -> Result<(), String> {
            let plan = plan.clone();
            std::thread::spawn(move || launch_on_sta_thread(&plan))
                .join()
                .map_err(|_| "Windows installer launch thread terminated unexpectedly".to_owned())?
        }
    }

    fn launch_on_sta_thread(plan: &InstallerLaunchPlan) -> Result<(), String> {
        let initialize = unsafe {
            CoInitializeEx(
                ptr::null(),
                (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
            )
        };
        if initialize < 0 {
            return Err(format!(
                "could not initialize the Windows installer launch apartment: 0x{:08x}",
                initialize as u32
            ));
        }
        let result = launch_initialized(plan);
        unsafe {
            CoUninitialize();
        }
        result
    }

    fn launch_initialized(plan: &InstallerLaunchPlan) -> Result<(), String> {
        let (program, parameters) = match plan.kind() {
            InstallerKind::WindowsMsi => {
                let program = system_directory()?.join("msiexec.exe");
                let parameters = format!(
                    "/i \"{}\" /promptrestart AUTOLAUNCHAPP=True",
                    plan.artifact_path().display()
                );
                (program, parameters)
            }
            InstallerKind::WindowsNsis => (plan.artifact_path().to_owned(), "/UPDATE".to_owned()),
            _ => return Err("installer kind is not supported on Windows".to_owned()),
        };
        let directory = plan
            .artifact_path()
            .parent()
            .ok_or_else(|| "verified installer has no parent directory".to_owned())?;
        let verb = wide(OsStr::new("open"))?;
        let program = wide(program.as_os_str())?;
        let parameters = wide(OsStr::new(&parameters))?;
        let directory = wide(directory.as_os_str())?;
        let mut execute: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
        execute.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
        execute.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI;
        execute.lpVerb = verb.as_ptr();
        execute.lpFile = program.as_ptr();
        execute.lpParameters = parameters.as_ptr();
        execute.lpDirectory = directory.as_ptr();
        execute.nShow = SW_SHOW;
        let launched = unsafe { ShellExecuteExW(&mut execute) };
        if launched == 0 {
            let error = unsafe { GetLastError() };
            return Err(format!("ShellExecuteExW failed with status 0x{error:08x}"));
        }
        if execute.hProcess.is_null() {
            return Err("ShellExecuteExW returned no installer process handle".to_owned());
        }
        unsafe {
            CloseHandle(execute.hProcess);
        }
        Ok(())
    }

    fn system_directory() -> Result<PathBuf, String> {
        let mut buffer = vec![0_u16; 32_768];
        let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
        if length == 0 || length as usize >= buffer.len() {
            return Err("Windows system directory is unavailable or oversized".to_owned());
        }
        buffer.truncate(length as usize);
        Ok(PathBuf::from(OsString::from_wide(&buffer)))
    }

    pub(super) fn package_version_matches(
        path: &Path,
        kind: InstallerKind,
        expected: &str,
    ) -> Result<bool, InstallerLaunchError> {
        match kind {
            InstallerKind::WindowsMsi => Ok(msi_product_version(path)? == expected),
            InstallerKind::WindowsNsis => {
                let expected = version_components(expected)?;
                let actual = executable_product_version(path)?;
                Ok(actual == [expected[0], expected[1], expected[2], 0])
            }
            _ => Err(InstallerLaunchError::Unsupported),
        }
    }

    #[cfg(test)]
    pub(super) fn package_version(
        path: &Path,
        kind: InstallerKind,
    ) -> Result<String, InstallerLaunchError> {
        match kind {
            InstallerKind::WindowsMsi => msi_product_version(path),
            InstallerKind::WindowsNsis => executable_product_version(path).map(|version| {
                if version[3] == 0 {
                    format!("{}.{}.{}", version[0], version[1], version[2])
                } else {
                    format!(
                        "{}.{}.{}.{}",
                        version[0], version[1], version[2], version[3]
                    )
                }
            }),
            _ => Err(InstallerLaunchError::Unsupported),
        }
    }

    #[cfg(test)]
    #[derive(Debug, PartialEq, Eq)]
    pub(super) struct MsiAssociationMetadata {
        pub(super) extensions: Vec<Vec<String>>,
        pub(super) prog_ids: Vec<Vec<String>>,
        pub(super) verbs: Vec<Vec<String>>,
    }

    #[cfg(test)]
    pub(super) fn msi_association_metadata(
        path: &Path,
    ) -> Result<MsiAssociationMetadata, InstallerLaunchError> {
        Ok(MsiAssociationMetadata {
            extensions: msi_string_rows(
                path,
                "SELECT `Extension`,`Component_`,`ProgId_`,`Feature_` FROM `Extension`",
                4,
            )?,
            prog_ids: msi_string_rows(path, "SELECT `ProgId`,`Description` FROM `ProgId`", 2)?,
            verbs: msi_string_rows(
                path,
                "SELECT `Extension_`,`Verb`,`Command`,`Argument` FROM `Verb`",
                4,
            )?,
        })
    }

    #[cfg(test)]
    fn msi_string_rows(
        path: &Path,
        query: &str,
        fields: u32,
    ) -> Result<Vec<Vec<String>>, InstallerLaunchError> {
        const MAX_ROWS: usize = 64;
        if fields == 0 || fields > 8 {
            return Err(InstallerLaunchError::Metadata(
                "MSI query field count is outside the inspection bound".to_owned(),
            ));
        }
        let path = wide(path.as_os_str()).map_err(InstallerLaunchError::Metadata)?;
        let mut database = 0;
        let status = unsafe { MsiOpenDatabaseW(path.as_ptr(), MSIDBOPEN_READONLY, &mut database) };
        if status != ERROR_SUCCESS {
            return Err(InstallerLaunchError::Metadata(format!(
                "MsiOpenDatabaseW failed with status {status}"
            )));
        }
        let database = MsiHandle(database);
        let query = wide(OsStr::new(query)).map_err(InstallerLaunchError::Metadata)?;
        let mut view = 0;
        let status = unsafe { MsiDatabaseOpenViewW(database.0, query.as_ptr(), &mut view) };
        if status != ERROR_SUCCESS {
            return Err(InstallerLaunchError::Metadata(format!(
                "MsiDatabaseOpenViewW failed with status {status}"
            )));
        }
        let view = MsiHandle(view);
        let status = unsafe { MsiViewExecute(view.0, 0) };
        if status != ERROR_SUCCESS {
            return Err(InstallerLaunchError::Metadata(format!(
                "MsiViewExecute failed with status {status}"
            )));
        }

        let mut rows = Vec::new();
        loop {
            let mut record = 0;
            let status = unsafe { MsiViewFetch(view.0, &mut record) };
            if status == ERROR_NO_MORE_ITEMS {
                break;
            }
            if status != ERROR_SUCCESS {
                return Err(InstallerLaunchError::Metadata(format!(
                    "MSI row enumeration failed with status {status}"
                )));
            }
            if rows.len() == MAX_ROWS {
                return Err(InstallerLaunchError::Metadata(
                    "MSI association table exceeds the inspection row bound".to_owned(),
                ));
            }
            let record = MsiHandle(record);
            let mut row = Vec::with_capacity(fields as usize);
            for field in 1..=fields {
                row.push(msi_record_string(record.0, field)?);
            }
            rows.push(row);
        }
        Ok(rows)
    }

    fn msi_product_version(path: &Path) -> Result<String, InstallerLaunchError> {
        let path = wide(path.as_os_str()).map_err(InstallerLaunchError::Metadata)?;
        let mut database = 0;
        let status = unsafe { MsiOpenDatabaseW(path.as_ptr(), MSIDBOPEN_READONLY, &mut database) };
        if status != ERROR_SUCCESS {
            return Err(InstallerLaunchError::Metadata(format!(
                "MsiOpenDatabaseW failed with status {status}"
            )));
        }
        let database = MsiHandle(database);
        let query = wide(OsStr::new(
            "SELECT `Value` FROM `Property` WHERE `Property`='ProductVersion'",
        ))
        .map_err(InstallerLaunchError::Metadata)?;
        let mut view = 0;
        let status = unsafe { MsiDatabaseOpenViewW(database.0, query.as_ptr(), &mut view) };
        if status != ERROR_SUCCESS {
            return Err(InstallerLaunchError::Metadata(format!(
                "MsiDatabaseOpenViewW failed with status {status}"
            )));
        }
        let view = MsiHandle(view);
        let status = unsafe { MsiViewExecute(view.0, 0) };
        if status != ERROR_SUCCESS {
            return Err(InstallerLaunchError::Metadata(format!(
                "MsiViewExecute failed with status {status}"
            )));
        }
        let mut record = 0;
        let status = unsafe { MsiViewFetch(view.0, &mut record) };
        if status != ERROR_SUCCESS {
            return Err(InstallerLaunchError::Metadata(format!(
                "MSI ProductVersion is unavailable with status {status}"
            )));
        }
        let record = MsiHandle(record);
        let value = msi_record_string(record.0, 1)?;
        let mut extra = 0;
        let status = unsafe { MsiViewFetch(view.0, &mut extra) };
        if status == ERROR_SUCCESS {
            drop(MsiHandle(extra));
            return Err(InstallerLaunchError::Metadata(
                "MSI contains duplicate ProductVersion rows".to_owned(),
            ));
        }
        if status != ERROR_NO_MORE_ITEMS {
            return Err(InstallerLaunchError::Metadata(format!(
                "MSI ProductVersion enumeration failed with status {status}"
            )));
        }
        Ok(value)
    }

    fn msi_record_string(record: MSIHANDLE, field: u32) -> Result<String, InstallerLaunchError> {
        let mut buffer = vec![0_u16; 64];
        loop {
            let mut length = buffer.len() as u32;
            let status =
                unsafe { MsiRecordGetStringW(record, field, buffer.as_mut_ptr(), &mut length) };
            if status == ERROR_MORE_DATA {
                if length == 0 || length > 4096 {
                    return Err(InstallerLaunchError::Metadata(
                        "MSI string exceeds the inspection bound".to_owned(),
                    ));
                }
                buffer.resize(length as usize + 1, 0);
                continue;
            }
            if status != ERROR_SUCCESS
                || length == 0
                || length as usize >= buffer.len()
                || length > 4096
            {
                return Err(InstallerLaunchError::Metadata(format!(
                    "MsiRecordGetStringW failed with status {status}"
                )));
            }
            return String::from_utf16(&buffer[..length as usize]).map_err(|_| {
                InstallerLaunchError::Metadata("MSI version is not UTF-16".to_owned())
            });
        }
    }

    fn executable_product_version(path: &Path) -> Result<[u16; 4], InstallerLaunchError> {
        let path = wide(path.as_os_str()).map_err(InstallerLaunchError::Metadata)?;
        let mut ignored = 0;
        let size = unsafe { GetFileVersionInfoSizeW(path.as_ptr(), &mut ignored) };
        if size == 0 || size > 16 * 1024 * 1024 {
            return Err(InstallerLaunchError::Metadata(
                "executable version resource is unavailable or oversized".to_owned(),
            ));
        }
        let mut bytes = vec![0_u8; size as usize];
        if unsafe { GetFileVersionInfoW(path.as_ptr(), 0, size, bytes.as_mut_ptr().cast()) } == 0 {
            return Err(InstallerLaunchError::Metadata(
                "GetFileVersionInfoW failed".to_owned(),
            ));
        }
        let root = wide(OsStr::new("\\")).map_err(InstallerLaunchError::Metadata)?;
        let mut value: *mut c_void = ptr::null_mut();
        let mut length = 0;
        if unsafe {
            VerQueryValueW(
                bytes.as_ptr().cast(),
                root.as_ptr(),
                &mut value,
                &mut length,
            )
        } == 0
            || value.is_null()
            || length < size_of::<VS_FIXEDFILEINFO>() as u32
        {
            return Err(InstallerLaunchError::Metadata(
                "VerQueryValueW did not return fixed version metadata".to_owned(),
            ));
        }
        let info = unsafe { value.cast::<VS_FIXEDFILEINFO>().read_unaligned() };
        if info.dwSignature != VS_FFI_SIGNATURE as u32 {
            return Err(InstallerLaunchError::Metadata(
                "executable fixed version signature is invalid".to_owned(),
            ));
        }
        Ok([
            (info.dwProductVersionMS >> 16) as u16,
            info.dwProductVersionMS as u16,
            (info.dwProductVersionLS >> 16) as u16,
            info.dwProductVersionLS as u16,
        ])
    }

    fn version_components(value: &str) -> Result<[u16; 3], InstallerLaunchError> {
        let mut parts = value.split('.');
        let parsed = [
            parts.next().and_then(|part| part.parse().ok()),
            parts.next().and_then(|part| part.parse().ok()),
            parts.next().and_then(|part| part.parse().ok()),
        ];
        if parts.next().is_some() || parsed.iter().any(Option::is_none) {
            return Err(InstallerLaunchError::Field);
        }
        Ok([
            parsed[0].expect("checked"),
            parsed[1].expect("checked"),
            parsed[2].expect("checked"),
        ])
    }

    struct MsiHandle(MSIHANDLE);

    impl Drop for MsiHandle {
        fn drop(&mut self) {
            if self.0 != 0 {
                unsafe {
                    MsiCloseHandle(self.0);
                }
            }
        }
    }

    fn wide(value: &OsStr) -> Result<Vec<u16>, String> {
        let mut encoded = value.encode_wide().collect::<Vec<_>>();
        if encoded.is_empty() || encoded.contains(&0) {
            return Err("Windows installer launch text is empty or contains NUL".to_owned());
        }
        encoded.push(0);
        Ok(encoded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installer_kind_is_class_and_suffix_specific() {
        assert_eq!(
            installer_kind("host.msi", "authenticode").expect("MSI"),
            InstallerKind::WindowsMsi
        );
        assert_eq!(
            installer_kind("host.exe", "authenticode").expect("NSIS"),
            InstallerKind::WindowsNsis
        );
        assert_eq!(
            installer_kind("host.pkg", "developer-id-notarized").expect("pkg"),
            InstallerKind::MacPkg
        );
        assert_eq!(
            installer_kind("host.AppImage", "linux-detached").expect("AppImage"),
            InstallerKind::LinuxAppImage
        );
        assert!(installer_kind("host.exe", "linux-detached").is_err());
        assert!(installer_kind("host.msi.exe", "authenticode").is_ok());
        assert!(installer_kind("host.MSI", "authenticode").is_err());
    }

    #[cfg(windows)]
    mod windows_integration {
        use std::sync::atomic::{AtomicUsize, Ordering};

        use ed25519_dalek::{Signer, SigningKey};
        use sha2::{Digest, Sha256};
        use sqlite_capsule_distribution::{
            RELEASE_PROFILE, ReleaseArtifact, ReleaseCandidateContext, ReleaseManifest,
            SignedReleaseManifest, key_id,
        };
        use sqlite_capsule_platform::inspect_platform_artifact_locked;
        use sqlite_capsule_update::{UPDATE_STAGE_PROFILE, UpdateStageRecord, UpdateStageState};

        use super::*;

        const SIGSTORE: &[u8] = b"test-only Sigstore evidence placeholder";

        struct FakeLauncher {
            fail: bool,
            calls: AtomicUsize,
        }

        struct FakeRollbackLauncher {
            fail: bool,
            calls: AtomicUsize,
        }

        impl FakeLauncher {
            fn succeeding() -> Self {
                Self {
                    fail: false,
                    calls: AtomicUsize::new(0),
                }
            }

            fn failing() -> Self {
                Self {
                    fail: true,
                    calls: AtomicUsize::new(0),
                }
            }
        }

        impl PlatformInstallerLauncher for FakeLauncher {
            fn launch(&self, plan: &InstallerLaunchPlan) -> Result<(), String> {
                assert_eq!(plan.kind(), InstallerKind::WindowsNsis);
                assert!(plan.artifact_path().ends_with("host-candidate.exe"));
                self.calls.fetch_add(1, Ordering::Relaxed);
                if self.fail {
                    Err("simulated launch rejection".to_owned())
                } else {
                    Ok(())
                }
            }
        }

        impl PlatformInstallerLauncher for FakeRollbackLauncher {
            fn launch(&self, plan: &InstallerLaunchPlan) -> Result<(), String> {
                assert_eq!(plan.kind(), InstallerKind::WindowsNsis);
                assert!(plan.artifact_path().ends_with("host-previous.exe"));
                self.calls.fetch_add(1, Ordering::Relaxed);
                if self.fail {
                    Err("simulated rollback launch rejection".to_owned())
                } else {
                    Ok(())
                }
            }
        }

        #[test]
        fn real_platform_evidence_guards_success_and_failure_transitions() {
            let Some((signed_binary, platform_report)) = signed_binary() else {
                return;
            };
            let artifact_bytes = std::fs::read(&signed_binary).expect("read signed fixture");
            assert_eq!(
                artifact_bytes.len() as u64,
                platform_report.artifact_bytes()
            );
            assert_eq!(sha256(&artifact_bytes), platform_report.artifact_sha256());

            let success_directory = temp_dir("success");
            let (success_stager, success_prepared) = prepare_stage(
                &success_directory,
                &signed_binary,
                &artifact_bytes,
                platform_report.platform_signing_identity(),
            );
            let success_id = success_prepared.stage().record.stage_id.clone();
            let launcher = FakeLauncher::succeeding();
            let receipt =
                launch_prepared_with(&success_stager, success_prepared, 1_800_000_001, &launcher)
                    .expect("launch verified staged update");
            assert_eq!(launcher.calls.load(Ordering::Relaxed), 1);
            assert_eq!(receipt.stage_id, success_id);
            assert_eq!(receipt.state, UpdateStageState::AwaitingHealth);
            assert_eq!(
                success_stager
                    .load(&success_id)
                    .expect("load success stage")
                    .state,
                UpdateStageState::AwaitingHealth
            );
            std::fs::remove_dir_all(success_directory).expect("remove success directory");

            let failure_directory = temp_dir("failure");
            let (failure_stager, failure_prepared) = prepare_stage(
                &failure_directory,
                &signed_binary,
                &artifact_bytes,
                platform_report.platform_signing_identity(),
            );
            let failure_id = failure_prepared.stage().record.stage_id.clone();
            let launcher = FakeLauncher::failing();
            assert!(matches!(
                launch_prepared_with(&failure_stager, failure_prepared, 1_800_000_010, &launcher,),
                Err(InstallerLaunchError::Launch(_))
            ));
            assert_eq!(launcher.calls.load(Ordering::Relaxed), 1);
            assert_eq!(
                failure_stager
                    .load(&failure_id)
                    .expect("load failed stage")
                    .state,
                UpdateStageState::RollbackRequired
            );
            assert!(
                failure_stager
                    .rollback_installer(&failure_id)
                    .expect("load rollback path")
                    .is_some()
            );
            std::fs::remove_dir_all(failure_directory).expect("remove failure directory");
        }

        #[test]
        fn real_platform_evidence_guards_rollback_handoff_and_health() {
            let Some((signed_binary, platform_report)) = signed_binary() else {
                return;
            };
            let rollback_version =
                crate::windows::package_version(&signed_binary, InstallerKind::WindowsNsis)
                    .expect("read signed rollback version");
            if !valid_version(&rollback_version) {
                return;
            }
            let artifact_bytes = std::fs::read(&signed_binary).expect("read signed fixture");

            let success_directory = temp_dir("rollback-success");
            let (success_stager, success_prepared) = prepare_stage(
                &success_directory,
                &signed_binary,
                &artifact_bytes,
                platform_report.platform_signing_identity(),
            );
            let success_id = success_prepared.stage().record.stage_id.clone();
            let success_version = success_prepared.stage().record.version.clone();
            assert!(
                launch_prepared_with(
                    &success_stager,
                    success_prepared,
                    1_800_000_020,
                    &FakeLauncher::failing(),
                )
                .is_err()
            );
            let prepared = success_stager
                .prepare_rollback(&success_id, &success_version)
                .expect("prepare exact rollback");
            let launcher = FakeRollbackLauncher {
                fail: false,
                calls: AtomicUsize::new(0),
            };
            let receipt = launch_rollback_with(&success_stager, prepared, 1_800_000_030, &launcher)
                .expect("launch verified rollback");
            assert_eq!(launcher.calls.load(Ordering::Relaxed), 1);
            assert_eq!(receipt.version, rollback_version);
            assert_eq!(receipt.state, UpdateStageState::AwaitingRollbackHealth);
            let reconciled = success_stager
                .reconcile_startup(&receipt.version, 1_800_000_032)
                .expect("reconcile rollback health")
                .expect("active rollback");
            assert_eq!(reconciled.state, UpdateStageState::RolledBack);
            std::fs::remove_dir_all(success_directory).expect("remove rollback success directory");

            let failure_directory = temp_dir("rollback-failure");
            let (failure_stager, failure_prepared) = prepare_stage(
                &failure_directory,
                &signed_binary,
                &artifact_bytes,
                platform_report.platform_signing_identity(),
            );
            let failure_id = failure_prepared.stage().record.stage_id.clone();
            let failure_version = failure_prepared.stage().record.version.clone();
            assert!(
                launch_prepared_with(
                    &failure_stager,
                    failure_prepared,
                    1_800_000_040,
                    &FakeLauncher::failing(),
                )
                .is_err()
            );
            let prepared = failure_stager
                .prepare_rollback(&failure_id, &failure_version)
                .expect("prepare rejected rollback");
            let launcher = FakeRollbackLauncher {
                fail: true,
                calls: AtomicUsize::new(0),
            };
            assert!(matches!(
                launch_rollback_with(&failure_stager, prepared, 1_800_000_050, &launcher,),
                Err(InstallerLaunchError::Launch(_))
            ));
            assert_eq!(launcher.calls.load(Ordering::Relaxed), 1);
            let failed = failure_stager
                .load(&failure_id)
                .expect("load failed rollback");
            assert_eq!(failed.state, UpdateStageState::RollbackFailed);
            assert_eq!(
                failed.rollback_reason,
                Some(RollbackReason::RollbackInstallerFailed)
            );
            std::fs::remove_dir_all(failure_directory).expect("remove rollback failure directory");
        }

        #[test]
        fn signed_bootstrap_cache_requires_exact_package_version() {
            let Some((signed_binary, platform_report)) = signed_binary() else {
                return;
            };
            let version =
                crate::windows::package_version(&signed_binary, InstallerKind::WindowsNsis)
                    .expect("read signed executable version");
            if !valid_version(&version) {
                return;
            }
            let directory = temp_dir("bootstrap-cache");
            let cache = directory.join("installer-cache");
            std::fs::create_dir(&cache).expect("create bootstrap cache");
            std::fs::copy(&signed_binary, cache.join(BOOTSTRAP_NSIS_NAME))
                .expect("copy signed bootstrap fixture");
            let source = discover_bootstrap_installer(
                &cache,
                &version,
                "authenticode",
                platform_report.platform_signing_identity(),
                true,
            )
            .expect("discover signed bootstrap installer")
            .expect("bootstrap installer source");
            assert_eq!(source.staged_name(), BOOTSTRAP_NSIS_NAME);
            assert_eq!(source.path(), cache.join(BOOTSTRAP_NSIS_NAME));
            drop(source);
            assert!(matches!(
                discover_bootstrap_installer(
                    &cache,
                    "0.0.1",
                    "authenticode",
                    platform_report.platform_signing_identity(),
                    true,
                ),
                Err(InstallerLaunchError::BootstrapVersion)
            ));
            std::fs::remove_dir_all(directory).expect("remove bootstrap cache");
        }

        #[test]
        fn bundled_windows_installers_declare_only_unambiguous_association_when_available() {
            let native_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
            let version = env!("CARGO_PKG_VERSION");
            let msi = native_root.join(format!(
                "target/x86_64-pc-windows-msvc/release/bundle/msi/SQLite Capsule Host_{version}_x64_en-US.msi"
            ));
            let nsis_installer = native_root.join(format!(
                "target/x86_64-pc-windows-msvc/release/bundle/nsis/SQLite Capsule Host_{version}_x64-setup.exe"
            ));
            let nsis_script =
                native_root.join("target/x86_64-pc-windows-msvc/release/nsis/x64/installer.nsi");
            if !msi.is_file() || !nsis_installer.is_file() || !nsis_script.is_file() {
                return;
            }
            assert_eq!(
                crate::windows::package_version(&msi, InstallerKind::WindowsMsi)
                    .expect("read bundled MSI ProductVersion"),
                version
            );
            assert_eq!(
                crate::windows::package_version(&nsis_installer, InstallerKind::WindowsNsis)
                    .expect("read bundled NSIS ProductVersion"),
                version
            );
            let association = crate::windows::msi_association_metadata(&msi)
                .expect("inspect bundled MSI association tables");
            assert_eq!(
                association.extensions,
                vec![vec![
                    "sqlitecapsule".to_owned(),
                    "Path".to_owned(),
                    "SQLite Capsule Host.sqlitecapsule".to_owned(),
                    "ShortcutsFeature".to_owned(),
                ]]
            );
            assert_eq!(
                association.prog_ids,
                vec![vec![
                    "SQLite Capsule Host.sqlitecapsule".to_owned(),
                    "Self-describing SQLite Capsule application".to_owned(),
                ]]
            );
            assert_eq!(
                association.verbs,
                vec![vec![
                    "sqlitecapsule".to_owned(),
                    "open".to_owned(),
                    "Open with SQLite Capsule Host".to_owned(),
                    "\"%1\"".to_owned(),
                ]]
            );

            let nsis = std::fs::read_to_string(nsis_script).expect("read generated NSIS source");
            let registrations = nsis
                .lines()
                .map(str::trim)
                .filter(|line| line.starts_with("!insertmacro APP_ASSOCIATE "))
                .collect::<Vec<_>>();
            assert_eq!(
                registrations,
                vec![
                    r#"!insertmacro APP_ASSOCIATE "sqlitecapsule" "SQLite Capsule" "Self-describing SQLite Capsule application" "$INSTDIR\${MAINBINARYNAME}.exe,0" "Open with ${PRODUCTNAME}" "$INSTDIR\${MAINBINARYNAME}.exe $\"%1$\"""#
                ]
            );
            let removals = nsis
                .lines()
                .map(str::trim)
                .filter(|line| line.starts_with("!insertmacro APP_UNASSOCIATE "))
                .collect::<Vec<_>>();
            assert_eq!(
                removals,
                vec![r#"!insertmacro APP_UNASSOCIATE "sqlitecapsule" "SQLite Capsule""#]
            );
        }

        fn signed_binary() -> Option<(PathBuf, sqlite_capsule_platform::PlatformVerificationReport)>
        {
            for path in [
                Path::new(r"C:\Program Files\PowerShell\7\pwsh.exe"),
                Path::new(r"C:\Program Files\nodejs\node.exe"),
            ] {
                if !path.exists() {
                    continue;
                }
                if let Ok(guard) = inspect_platform_artifact_locked(path, "authenticode", true) {
                    let report = guard.report().clone();
                    drop(guard);
                    return Some((path.to_owned(), report));
                }
            }
            None
        }

        fn prepare_stage(
            directory: &Path,
            previous_installer: &Path,
            artifact_bytes: &[u8],
            platform_identity: &str,
        ) -> (UpdateStager, PreparedInstallation) {
            let package_version =
                crate::windows::package_version(previous_installer, InstallerKind::WindowsNsis)
                    .expect("read signed package version");
            let artifact = ReleaseArtifact {
                target: "x86_64-pc-windows-msvc".to_owned(),
                url: "https://updates.example.com/host.exe".to_owned(),
                bytes: artifact_bytes.len() as u64,
                sha256: sha256(artifact_bytes),
                sigstore_bundle_sha256: sha256(SIGSTORE),
                platform_signing: "authenticode".to_owned(),
                platform_signing_identity: platform_identity.to_owned(),
                platform_timestamp_required: true,
                sigstore_certificate_identity:
                    "https://github.com/sqlite-capsule/sqlite-capsule/.github/workflows/release.yml@refs/tags/v0.2.0"
                        .to_owned(),
                sigstore_oidc_issuer: "https://token.actions.githubusercontent.com".to_owned(),
            };
            let signed_release = sign_release(artifact.clone(), &package_version);
            let stager = UpdateStager::open(&directory.join("staging")).expect("open stager");
            let stage_id = "00000000000000000002-x86_64-pc-windows-msvc";
            let stage_directory = stager.root().join(stage_id);
            std::fs::create_dir(&stage_directory).expect("create stage fixture directory");
            std::fs::write(stage_directory.join("host-candidate.exe"), artifact_bytes)
                .expect("write staged artifact");
            std::fs::write(stage_directory.join("host-0.2.0.sigstore.json"), SIGSTORE)
                .expect("write staged Sigstore evidence");
            std::fs::copy(
                previous_installer,
                stage_directory.join("host-previous.exe"),
            )
            .expect("copy prior installer fixture");
            let record = UpdateStageRecord {
                profile: UPDATE_STAGE_PROFILE.to_owned(),
                stage_id: stage_id.to_owned(),
                version: package_version.clone(),
                sequence: 2,
                target: artifact.target.clone(),
                platform_signing: artifact.platform_signing.clone(),
                platform_signing_identity: artifact.platform_signing_identity.clone(),
                platform_timestamp_required: artifact.platform_timestamp_required,
                sigstore_certificate_identity: artifact.sigstore_certificate_identity.clone(),
                sigstore_oidc_issuer: artifact.sigstore_oidc_issuer.clone(),
                artifact_name: "host-candidate.exe".to_owned(),
                artifact_bytes: artifact_bytes.len() as u64,
                artifact_sha256: artifact.sha256.clone(),
                sigstore_name: "host-0.2.0.sigstore.json".to_owned(),
                sigstore_bytes: SIGSTORE.len() as u64,
                sigstore_sha256: artifact.sigstore_bundle_sha256.clone(),
                previous_installer_name: Some("host-previous.exe".to_owned()),
                previous_installer_version: Some(package_version),
                previous_installer_bytes: Some(artifact_bytes.len() as u64),
                previous_installer_sha256: Some(sha256(artifact_bytes)),
                signed_release,
            };
            std::fs::write(
                stage_directory.join("stage.json"),
                serde_json::to_vec_pretty(&record).expect("serialize stage fixture"),
            )
            .expect("write stage fixture");
            std::fs::write(
                stage_directory.join("state-000-prepared.json"),
                serde_json::to_vec_pretty(&serde_json::json!({
                    "profile": UPDATE_STAGE_PROFILE,
                    "stage_id": stage_id,
                    "state": "prepared",
                    "recorded_at_unix": 0,
                    "running_version": null,
                    "rollback_reason": null
                }))
                .expect("serialize prepared transition"),
            )
            .expect("write prepared transition");
            let allowed_hosts = ["updates.example.com"];
            let context = ReleaseCandidateContext {
                current_version: "0.0.0",
                current_sequence: 1,
                target: "x86_64-pc-windows-msvc",
                allowed_hosts: &allowed_hosts,
                now_unix: 1_800_000_000,
            };
            let prepared = stager
                .prepare_installation(stage_id, &root().verifying_key().to_bytes(), &context)
                .expect("prepare signed fixture");
            (stager, prepared)
        }

        fn sign_release(artifact: ReleaseArtifact, version: &str) -> SignedReleaseManifest {
            const RELEASE_CONTEXT: &[u8] = b"SQLite Capsule host release manifest v2\0";
            let manifest = ReleaseManifest {
                profile: RELEASE_PROFILE.to_owned(),
                sequence: 2,
                version: version.to_owned(),
                issued_at: "2026-01-01T00:00:00Z".to_owned(),
                expires_at: "2030-01-01T00:00:00Z".to_owned(),
                artifacts: vec![artifact],
            };
            let canonical = serde_json_canonicalizer::to_vec(&manifest).expect("canonical release");
            let mut message = RELEASE_CONTEXT.to_vec();
            message.extend_from_slice(&canonical);
            let root = root();
            SignedReleaseManifest {
                manifest,
                signing_key_id: key_id(&root.verifying_key().to_bytes()),
                signature_hex: lower_hex(&root.sign(&message).to_bytes()),
            }
        }

        fn root() -> SigningKey {
            SigningKey::from_bytes(&[53_u8; 32])
        }

        fn temp_dir(label: &str) -> PathBuf {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "sqlite-capsule-installer-{label}-{}-{nonce}",
                std::process::id()
            ));
            std::fs::create_dir(&path).expect("create installer test directory");
            path
        }

        fn sha256(bytes: &[u8]) -> String {
            lower_hex(&Sha256::digest(bytes))
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
    }
}

//! Host-owned capsule source identity and one-writer coordination.
//!
//! The opened source is kept alive for the session. On Windows it is opened
//! without delete sharing so rename/replacement is blocked while SQLite uses
//! the path. POSIX keeps the inode open and rechecks the canonical path before
//! and after writes so a replacement is detected rather than silently used.

use std::{
    fs::{File, OpenOptions},
    io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileSystemKind {
    Fixed,
    Removable,
    Remote,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceIdentity {
    pub device: u64,
    pub file: u64,
    pub bytes: u64,
}

#[derive(Debug, Error)]
pub enum LifecycleError {
    #[error("capsule source is not a regular file")]
    NotRegularFile,
    #[error("capsule source must not be a symbolic link")]
    SymbolicLink,
    #[error("capsule source changed while it was being opened")]
    ChangedDuringOpen,
    #[error("capsule source was replaced during the session")]
    Replaced,
    #[error("writable session filesystem does not meet the platform safety policy")]
    UnsafeWritableFileSystem,
    #[error("another host session already owns the capsule writer lease")]
    WriterBusy,
    #[error("operating-system file operation failed: {0}")]
    Io(#[from] io::Error),
    #[error("operating-system lifecycle primitive failed: {0}")]
    Platform(String),
}

pub struct PinnedSource {
    canonical_path: PathBuf,
    file: File,
    identity: SourceIdentity,
    file_system: FileSystemKind,
}

impl PinnedSource {
    pub fn open(path: &Path, writable: bool) -> Result<Self, LifecycleError> {
        let unresolved = std::fs::symlink_metadata(path)?;
        if unresolved.file_type().is_symlink() {
            return Err(LifecycleError::SymbolicLink);
        }
        if !unresolved.is_file() {
            return Err(LifecycleError::NotRegularFile);
        }
        let canonical_path = path.canonicalize()?;
        let file_system = classify_file_system(&canonical_path)?;
        if writable && !writable_file_system_supported(file_system) {
            return Err(LifecycleError::UnsafeWritableFileSystem);
        }
        let file = open_pinned(&canonical_path, writable)?;
        let identity = identity_from_file(&file)?;
        let path_identity = identity_from_path(&canonical_path)?;
        if identity != path_identity {
            return Err(LifecycleError::ChangedDuringOpen);
        }
        Ok(Self {
            canonical_path,
            file,
            identity,
            file_system,
        })
    }

    pub fn canonical_path(&self) -> &Path {
        &self.canonical_path
    }

    pub fn identity(&self) -> &SourceIdentity {
        &self.identity
    }

    pub fn file_system(&self) -> FileSystemKind {
        self.file_system
    }

    pub fn assert_current(&self) -> Result<(), LifecycleError> {
        let held = identity_from_file(&self.file)?;
        let path =
            identity_from_path(&self.canonical_path).map_err(|_| LifecycleError::Replaced)?;
        if held != self.identity || path != self.identity {
            return Err(LifecycleError::Replaced);
        }
        Ok(())
    }

    /// Accept a file-length change only at the narrow point immediately after
    /// the verified runtime committed its own SQLite transaction. The volume
    /// and file/inode identity must still match both the held handle and path.
    pub fn accept_host_write(&mut self) -> Result<(), LifecycleError> {
        let held = identity_from_file(&self.file)?;
        let path =
            identity_from_path(&self.canonical_path).map_err(|_| LifecycleError::Replaced)?;
        if !same_file_object(&held, &self.identity)
            || !same_file_object(&path, &self.identity)
            || held.bytes != path.bytes
        {
            return Err(LifecycleError::Replaced);
        }
        self.identity.bytes = held.bytes;
        Ok(())
    }
}

fn same_file_object(left: &SourceIdentity, right: &SourceIdentity) -> bool {
    left.device == right.device && left.file == right.file
}

fn writable_file_system_supported(file_system: FileSystemKind) -> bool {
    #[cfg(windows)]
    {
        matches!(file_system, FileSystemKind::Fixed)
    }
    #[cfg(not(windows))]
    {
        !matches!(
            file_system,
            FileSystemKind::Removable | FileSystemKind::Remote
        )
    }
}

pub struct WriterLease {
    #[cfg(windows)]
    handle: windows_sys::Win32::Foundation::HANDLE,
    #[cfg(unix)]
    file: File,
}

// SAFETY: a Windows mutex HANDLE is process-wide, contains no thread-affine
// state, and CloseHandle is valid from any thread. The type is deliberately not
// Sync; desktop access remains serialized by the host mutex.
#[cfg(windows)]
unsafe impl Send for WriterLease {}

impl WriterLease {
    pub fn acquire(lock_root: &Path, source: &PinnedSource) -> Result<Self, LifecycleError> {
        source.assert_current()?;
        acquire_writer_lease(lock_root, source)
    }
}

pub fn prepare_private_directory(path: &Path) -> Result<(), LifecycleError> {
    std::fs::create_dir_all(path)?;
    protect_path(path, true)
}

pub fn protect_private_file(path: &Path) -> Result<(), LifecycleError> {
    protect_path(path, false)
}

#[cfg(unix)]
fn protect_path(path: &Path, directory: bool) -> Result<(), LifecycleError> {
    use std::os::unix::fs::PermissionsExt;
    let mode = if directory { 0o700 } else { 0o600 };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
    Ok(())
}

#[cfg(windows)]
fn protect_path(path: &Path, directory: bool) -> Result<(), LifecycleError> {
    windows_acl::protect(path, directory)
}

#[cfg(not(any(unix, windows)))]
fn protect_path(_path: &Path, _directory: bool) -> Result<(), LifecycleError> {
    Err(LifecycleError::Platform(
        "private filesystem protection is unavailable".to_owned(),
    ))
}

#[cfg(windows)]
impl Drop for WriterLease {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}

#[cfg(unix)]
impl Drop for WriterLease {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        unsafe {
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

#[cfg(unix)]
fn identity_from_path(path: &Path) -> Result<SourceIdentity, LifecycleError> {
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(LifecycleError::NotRegularFile);
    }
    identity_from_metadata(&metadata, None)
}

#[cfg(windows)]
fn identity_from_path(path: &Path) -> Result<SourceIdentity, LifecycleError> {
    let file = open_pinned(path, false)?;
    identity_from_file(&file)
}

fn identity_from_file(file: &File) -> Result<SourceIdentity, LifecycleError> {
    let metadata = file.metadata()?;
    identity_from_metadata(&metadata, Some(file))
}

#[cfg(unix)]
fn identity_from_metadata(
    metadata: &std::fs::Metadata,
    _file: Option<&File>,
) -> Result<SourceIdentity, LifecycleError> {
    use std::os::unix::fs::MetadataExt;
    Ok(SourceIdentity {
        device: metadata.dev(),
        file: metadata.ino(),
        bytes: metadata.len(),
    })
}

#[cfg(windows)]
fn identity_from_metadata(
    _metadata: &std::fs::Metadata,
    file: Option<&File>,
) -> Result<SourceIdentity, LifecycleError> {
    use std::{mem::MaybeUninit, os::windows::io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    let file = file.ok_or_else(|| {
        LifecycleError::Platform("Windows identity requires an open file handle".to_owned())
    })?;
    let mut info = MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::zeroed();
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) };
    if ok == 0 {
        return Err(LifecycleError::Io(io::Error::last_os_error()));
    }
    let owned = unsafe { info.assume_init() };
    Ok(SourceIdentity {
        device: u64::from(owned.dwVolumeSerialNumber),
        file: (u64::from(owned.nFileIndexHigh) << 32) | u64::from(owned.nFileIndexLow),
        bytes: (u64::from(owned.nFileSizeHigh) << 32) | u64::from(owned.nFileSizeLow),
    })
}

#[cfg(windows)]
fn open_pinned(path: &Path, writable: bool) -> Result<File, LifecycleError> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};

    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(writable)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE);
    Ok(options.open(path)?)
}

#[cfg(unix)]
fn open_pinned(path: &Path, writable: bool) -> Result<File, LifecycleError> {
    let mut options = OpenOptions::new();
    options.read(true).write(writable);
    Ok(options.open(path)?)
}

#[cfg(windows)]
fn classify_file_system(path: &Path) -> Result<FileSystemKind, LifecycleError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetVolumePathNameW};

    let path_wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut root = vec![0_u16; 32_768];
    let ok = unsafe {
        GetVolumePathNameW(
            path_wide.as_ptr(),
            root.as_mut_ptr(),
            u32::try_from(root.len()).expect("Windows path buffer fits u32"),
        )
    };
    if ok == 0 {
        return Err(LifecycleError::Io(io::Error::last_os_error()));
    }
    Ok(classify_windows_drive_type(unsafe {
        GetDriveTypeW(root.as_ptr())
    }))
}

#[cfg(windows)]
fn classify_windows_drive_type(drive_type: u32) -> FileSystemKind {
    use windows_sys::Win32::System::WindowsProgramming::{
        DRIVE_FIXED, DRIVE_REMOTE, DRIVE_REMOVABLE,
    };

    match drive_type {
        DRIVE_FIXED => FileSystemKind::Fixed,
        DRIVE_REMOVABLE => FileSystemKind::Removable,
        DRIVE_REMOTE => FileSystemKind::Remote,
        _ => FileSystemKind::Unknown,
    }
}

#[cfg(unix)]
fn classify_file_system(_path: &Path) -> Result<FileSystemKind, LifecycleError> {
    Ok(FileSystemKind::Unknown)
}

#[cfg(windows)]
fn acquire_writer_lease(
    _lock_root: &Path,
    source: &PinnedSource,
) -> Result<WriterLease, LifecycleError> {
    use std::{os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::{
        Foundation::{ERROR_ALREADY_EXISTS, GetLastError},
        System::Threading::CreateMutexW,
    };

    let digest = source_key(source);
    let name = format!("Local\\SQLiteCapsule-Writer-{digest}");
    let wide: Vec<u16> = std::ffi::OsStr::new(&name)
        .encode_wide()
        .chain(Some(0))
        .collect();
    let handle = unsafe { CreateMutexW(ptr::null(), 0, wide.as_ptr()) };
    if handle.is_null() {
        return Err(LifecycleError::Io(io::Error::last_os_error()));
    }
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(handle);
        }
        return Err(LifecycleError::WriterBusy);
    }
    Ok(WriterLease { handle })
}

#[cfg(unix)]
fn acquire_writer_lease(
    lock_root: &Path,
    source: &PinnedSource,
) -> Result<WriterLease, LifecycleError> {
    use std::os::{
        fd::AsRawFd,
        unix::fs::{OpenOptionsExt, PermissionsExt},
    };

    std::fs::create_dir_all(lock_root)?;
    std::fs::set_permissions(lock_root, std::fs::Permissions::from_mode(0o700))?;
    let path = lock_root.join(format!("{}.lock", source_key(source)));
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .open(path)?;
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if result != 0 {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::WouldBlock {
            return Err(LifecycleError::WriterBusy);
        }
        return Err(LifecycleError::Io(error));
    }
    Ok(WriterLease { file })
}

fn source_key(source: &PinnedSource) -> String {
    let mut digest = Sha256::new();
    digest.update(source.identity.device.to_le_bytes());
    digest.update(source.identity.file.to_le_bytes());
    digest.update(source.canonical_path.to_string_lossy().as_bytes());
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(windows)]
mod windows_acl {
    use std::{
        ffi::OsStr,
        io,
        mem::{size_of, size_of_val},
        os::windows::ffi::OsStrExt,
        path::Path,
        ptr,
    };

    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE},
        Security::{
            ACCESS_ALLOWED_ACE, ACL, ACL_REVISION, AddAccessAllowedAceEx,
            Authorization::{SE_FILE_OBJECT, SetNamedSecurityInfoW},
            CONTAINER_INHERIT_ACE, DACL_SECURITY_INFORMATION, GetLengthSid, GetTokenInformation,
            InitializeAcl, OBJECT_INHERIT_ACE, PROTECTED_DACL_SECURITY_INFORMATION, PSID,
            TOKEN_QUERY, TOKEN_USER, TokenUser,
        },
        Storage::FileSystem::FILE_ALL_ACCESS,
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    use super::LifecycleError;

    struct Handle(HANDLE);

    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    CloseHandle(self.0);
                }
            }
        }
    }

    pub(super) fn protect(path: &Path, directory: bool) -> Result<(), LifecycleError> {
        unsafe {
            let mut raw_token: HANDLE = ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw_token) == 0 {
                return Err(io::Error::last_os_error().into());
            }
            let token = Handle(raw_token);
            let mut token_bytes = 0_u32;
            GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &mut token_bytes);
            if token_bytes < size_of::<TOKEN_USER>() as u32 {
                return Err(io::Error::last_os_error().into());
            }
            let mut token_buffer =
                vec![0_usize; (token_bytes as usize).div_ceil(size_of::<usize>())];
            if GetTokenInformation(
                token.0,
                TokenUser,
                token_buffer.as_mut_ptr().cast(),
                token_bytes,
                &mut token_bytes,
            ) == 0
            {
                return Err(io::Error::last_os_error().into());
            }
            let sid: PSID = (*token_buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid;
            let sid_bytes = GetLengthSid(sid);
            if sid_bytes == 0 {
                return Err(io::Error::last_os_error().into());
            }
            let acl_bytes = size_of::<ACL>() + size_of::<ACCESS_ALLOWED_ACE>() - size_of::<u32>()
                + sid_bytes as usize;
            let mut acl_buffer = vec![0_usize; acl_bytes.div_ceil(size_of::<usize>())];
            let acl = acl_buffer.as_mut_ptr().cast::<ACL>();
            if InitializeAcl(acl, size_of_val(acl_buffer.as_slice()) as u32, ACL_REVISION) == 0 {
                return Err(io::Error::last_os_error().into());
            }
            let inheritance = if directory {
                OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE
            } else {
                0
            };
            if AddAccessAllowedAceEx(acl, ACL_REVISION, inheritance, FILE_ALL_ACCESS, sid) == 0 {
                return Err(io::Error::last_os_error().into());
            }
            let mut wide: Vec<u16> = OsStr::new(path).encode_wide().collect();
            wide.push(0);
            let result = SetNamedSecurityInfoW(
                wide.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                acl,
                ptr::null(),
            );
            if result != 0 {
                return Err(io::Error::from_raw_os_error(result as i32).into());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::Write,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use super::*;

    static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let suffix = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "sqlite-capsule-lifecycle-test-{}-{suffix}",
                std::process::id()
            ));
            fs::create_dir(&path).expect("create test directory");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn source(directory: &TestDirectory) -> PathBuf {
        let path = directory.0.join("source.sqlitecapsule");
        fs::write(&path, b"SQLite format 3\0fixture").expect("write fixture");
        path
    }

    #[test]
    fn pins_a_regular_source_and_reports_stable_identity() {
        let directory = TestDirectory::new();
        let path = source(&directory);
        let pinned = PinnedSource::open(&path, false).expect("pin source");
        assert_eq!(pinned.identity().bytes, 23);
        pinned.assert_current().expect("identity remains current");
    }

    #[cfg(windows)]
    #[test]
    fn windows_drive_types_and_writable_policy_fail_closed() {
        use windows_sys::Win32::System::WindowsProgramming::{
            DRIVE_CDROM, DRIVE_FIXED, DRIVE_NO_ROOT_DIR, DRIVE_RAMDISK, DRIVE_REMOTE,
            DRIVE_REMOVABLE, DRIVE_UNKNOWN,
        };

        assert_eq!(
            classify_windows_drive_type(DRIVE_FIXED),
            FileSystemKind::Fixed
        );
        assert_eq!(
            classify_windows_drive_type(DRIVE_REMOVABLE),
            FileSystemKind::Removable
        );
        assert_eq!(
            classify_windows_drive_type(DRIVE_REMOTE),
            FileSystemKind::Remote
        );
        for drive_type in [DRIVE_UNKNOWN, DRIVE_NO_ROOT_DIR, DRIVE_CDROM, DRIVE_RAMDISK] {
            assert_eq!(
                classify_windows_drive_type(drive_type),
                FileSystemKind::Unknown
            );
        }

        assert!(writable_file_system_supported(FileSystemKind::Fixed));
        assert!(!writable_file_system_supported(FileSystemKind::Removable));
        assert!(!writable_file_system_supported(FileSystemKind::Remote));
        assert!(!writable_file_system_supported(FileSystemKind::Unknown));
    }

    #[cfg(windows)]
    #[test]
    fn windows_writable_open_matches_the_classified_volume_policy() {
        let directory = TestDirectory::new();
        let path = source(&directory);
        let read_only = PinnedSource::open(&path, false).expect("read-only pin");
        let classified = read_only.file_system();
        drop(read_only);

        let writable = PinnedSource::open(&path, true);
        assert_eq!(writable.is_ok(), classified == FileSystemKind::Fixed);
        if classified != FileSystemKind::Fixed {
            assert!(matches!(
                writable,
                Err(LifecycleError::UnsafeWritableFileSystem)
            ));
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn unclassified_platforms_preserve_the_existing_writable_policy() {
        assert!(writable_file_system_supported(FileSystemKind::Fixed));
        assert!(writable_file_system_supported(FileSystemKind::Unknown));
        assert!(!writable_file_system_supported(FileSystemKind::Removable));
        assert!(!writable_file_system_supported(FileSystemKind::Remote));
    }

    #[test]
    fn one_writer_lease_is_released_on_drop() {
        let directory = TestDirectory::new();
        let path = source(&directory);
        let pinned = PinnedSource::open(&path, true).expect("pin source");
        let first =
            WriterLease::acquire(&directory.0.join("locks"), &pinned).expect("first writer");
        assert!(matches!(
            WriterLease::acquire(&directory.0.join("locks"), &pinned),
            Err(LifecycleError::WriterBusy)
        ));
        drop(first);
        WriterLease::acquire(&directory.0.join("locks"), &pinned).expect("writer after release");
    }

    #[test]
    fn replacement_is_blocked_or_detected() {
        let directory = TestDirectory::new();
        let path = source(&directory);
        let pinned = PinnedSource::open(&path, true).expect("pin source");
        let moved = directory.0.join("moved.sqlitecapsule");
        match fs::rename(&path, &moved) {
            Ok(()) => {
                fs::write(&path, b"SQLite format 3\0replacement").expect("replacement");
                assert!(matches!(
                    pinned.assert_current(),
                    Err(LifecycleError::Replaced)
                ));
            }
            Err(_) => pinned
                .assert_current()
                .expect("Windows pin remains current"),
        }
    }

    #[test]
    fn host_write_may_refresh_size_but_not_file_identity() {
        let directory = TestDirectory::new();
        let path = source(&directory);
        let mut pinned = PinnedSource::open(&path, true).expect("pin source");
        let mut writer = OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open writer");
        writer.write_all(b"-host-growth").expect("grow source");
        writer.sync_all().expect("sync growth");
        assert!(matches!(
            pinned.assert_current(),
            Err(LifecycleError::Replaced)
        ));
        pinned
            .accept_host_write()
            .expect("accept host-owned growth");
        assert_eq!(pinned.identity().bytes, 35);
        pinned.assert_current().expect("refreshed size is current");
    }

    #[cfg(unix)]
    #[test]
    fn direct_symbolic_links_are_rejected() {
        use std::os::unix::fs::symlink;
        let directory = TestDirectory::new();
        let path = source(&directory);
        let link = directory.0.join("link.sqlitecapsule");
        symlink(path, &link).expect("symlink");
        assert!(matches!(
            PinnedSource::open(&link, false),
            Err(LifecycleError::SymbolicLink)
        ));
    }
}

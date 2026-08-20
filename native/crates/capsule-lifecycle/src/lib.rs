//! Host-owned capsule source identity and one-writer coordination.
//!
//! The opened source is kept alive for the session. On Windows it is opened
//! without delete sharing so rename/replacement is blocked while SQLite uses
//! the path. POSIX keeps the inode open and rechecks the canonical path before
//! and after writes so a replacement is detected rather than silently used.

use std::{
    ffi::{OsStr, OsString},
    fs::{File, OpenOptions},
    io::{self, Write},
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
    #[serde(default)]
    pub stable_file_id: String,
    pub bytes: u64,
    #[serde(default)]
    pub modified_ns: u64,
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
    #[error("destination parent is not a stable ordinary directory")]
    UnsafeDestinationParent,
    #[error("destination leaf is not a safe portable file name")]
    UnsafeDestinationLeaf,
    #[error("destination already exists")]
    DestinationExists,
    #[error("destination aliases an input")]
    DestinationAliasesInput,
    #[error("private output was not completely written and synchronized")]
    PrivateOutputIncomplete,
    #[error("safe create-new publication is unavailable or failed")]
    PublicationFailed,
    #[error("published output identity did not survive reopen")]
    PostPublishVerification,
    #[error("operating-system file operation failed: {0}")]
    Io(#[from] io::Error),
    #[error("operating-system lifecycle primitive failed: {0}")]
    Platform(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectoryIdentity {
    pub device: u64,
    pub file: u64,
    #[serde(default)]
    pub stable_file_id: String,
}

/// A one-use create-new destination capability bound to a held parent handle
/// and a validated leaf. Paths are hints only after this object is created.
pub struct DestinationReservation {
    parent_path: PathBuf,
    parent: File,
    identity: DirectoryIdentity,
    leaf: OsString,
    input_identities: Vec<(u64, u64, String)>,
}

/// Owner-private create-new staging file under a held destination parent.
/// Dropping an unpublished value removes only this private temporary file.
pub struct PrivateOutput {
    reservation: Option<DestinationReservation>,
    staging_parent: File,
    staging_directory_leaf: OsString,
    file: File,
    temporary_leaf: OsString,
    temporary_path_hint: PathBuf,
    identity: SourceIdentity,
    published: bool,
}

/// A staged output whose write handle has been flushed, synchronized and
/// sealed. This low-level filesystem state is not evidence that the bytes are
/// a valid Capsule; only the workspace validator may promote it to a trusted
/// lifecycle output.
pub struct SealedPrivateOutput {
    inner: PrivateOutput,
    sealed_identity: SourceIdentity,
    sealed_sha256: [u8; 32],
}

/// Required post-publication verification result. Returning success is the
/// only route by which a published path is reported to the caller.
pub struct PublishedOutput {
    pub path: PathBuf,
    pub identity: SourceIdentity,
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
        self.identity.modified_ns = held.modified_ns;
        Ok(())
    }
}

impl DestinationReservation {
    pub fn reserve(
        parent: &Path,
        leaf: &OsStr,
        inputs: &[SourceIdentity],
    ) -> Result<Self, LifecycleError> {
        validate_destination_leaf(leaf)?;
        if !parent.is_absolute() {
            return Err(LifecycleError::UnsafeDestinationParent);
        }
        let parent_path = normalize_display_path(parent.to_path_buf());
        let file_system = classify_file_system(&parent_path)?;
        if !writable_file_system_supported(file_system) {
            return Err(LifecycleError::UnsafeWritableFileSystem);
        }
        let parent_handle = open_parent_directory_walk(&parent_path)?;
        let held = directory_identity_from_file(&parent_handle)?;
        require_destination_family_absent(&parent_handle, leaf)?;
        Ok(Self {
            parent_path,
            parent: parent_handle,
            identity: held,
            leaf: leaf.to_owned(),
            input_identities: inputs
                .iter()
                .map(|identity| {
                    (
                        identity.device,
                        identity.file,
                        identity.stable_file_id.clone(),
                    )
                })
                .collect(),
        })
    }

    pub fn identity(&self) -> &DirectoryIdentity {
        &self.identity
    }

    pub fn leaf(&self) -> &OsStr {
        &self.leaf
    }

    pub fn path_hint(&self) -> PathBuf {
        self.parent_path.join(&self.leaf)
    }

    pub fn assert_reserved_current(&self) -> Result<(), LifecycleError> {
        self.assert_current()?;
        require_destination_family_absent(&self.parent, &self.leaf)?;
        Ok(())
    }

    pub fn stage(self) -> Result<PrivateOutput, LifecycleError> {
        self.assert_current()
            .map_err(|_| LifecycleError::UnsafeDestinationParent)?;
        require_destination_family_absent(&self.parent, &self.leaf)?;
        for _ in 0..32 {
            let staging_directory_leaf = private_temporary_leaf()?;
            match create_private_directory_relative(&self.parent, &staging_directory_leaf) {
                Ok(staging_parent) => {
                    let temporary_leaf = OsString::from("payload.capsule.sqlite");
                    let file = match create_private_relative(&staging_parent, &temporary_leaf) {
                        Ok(file) => file,
                        Err(error) => {
                            let _ = remove_directory_relative(
                                &self.parent,
                                &staging_directory_leaf,
                                &staging_parent,
                            );
                            return Err(error);
                        }
                    };
                    let identity = match identity_from_file(&file) {
                        Ok(identity) => identity,
                        Err(error) => {
                            let _ = unlink_relative(&staging_parent, &temporary_leaf);
                            let _ = remove_directory_relative(
                                &self.parent,
                                &staging_directory_leaf,
                                &staging_parent,
                            );
                            return Err(LifecycleError::Platform(format!(
                                "stage identity: {error}"
                            )));
                        }
                    };
                    if self
                        .input_identities
                        .iter()
                        .any(|input| input_matches_identity(input, &identity))
                    {
                        let _ = unlink_relative(&staging_parent, &temporary_leaf);
                        let _ = remove_directory_relative(
                            &self.parent,
                            &staging_directory_leaf,
                            &staging_parent,
                        );
                        return Err(LifecycleError::DestinationAliasesInput);
                    }
                    let temporary_path_hint = self
                        .parent_path
                        .join(&staging_directory_leaf)
                        .join(&temporary_leaf);
                    return Ok(PrivateOutput {
                        reservation: Some(self),
                        staging_parent,
                        staging_directory_leaf,
                        file,
                        temporary_leaf,
                        temporary_path_hint,
                        identity,
                        published: false,
                    });
                }
                Err(LifecycleError::DestinationExists) => continue,
                Err(error) => return Err(error),
            }
        }
        Err(LifecycleError::PublicationFailed)
    }

    fn assert_current(&self) -> Result<(), LifecycleError> {
        let held = directory_identity_from_file(&self.parent)
            .map_err(|error| LifecycleError::Platform(format!("held parent identity: {error}")))?;
        let path = directory_identity_from_path(&self.parent_path)
            .map_err(|error| LifecycleError::Platform(format!("path parent identity: {error}")))?;
        if held != self.identity || path != self.identity {
            return Err(LifecycleError::UnsafeDestinationParent);
        }
        Ok(())
    }
}

impl PrivateOutput {
    pub fn file_mut(&mut self) -> &mut File {
        &mut self.file
    }

    /// Path hint usable by host-owned SQLite verification only. Publication
    /// remains relative to the held parent handle, never authoritative by path.
    pub fn private_path_hint(&self) -> &Path {
        &self.temporary_path_hint
    }

    pub fn seal(self) -> Result<SealedPrivateOutput, LifecycleError> {
        self.seal_with_limit(64 * 1024 * 1024)
    }

    pub fn seal_with_limit(
        mut self,
        max_bytes: u64,
    ) -> Result<SealedPrivateOutput, LifecycleError> {
        self.file.flush()?;
        self.file.sync_all()?;
        let current = identity_from_file(&self.file)?;
        if current.device != self.identity.device
            || current.file != self.identity.file
            || current.bytes == 0
            || current.bytes > max_bytes
        {
            return Err(LifecycleError::PrivateOutputIncomplete);
        }
        self.identity.bytes = current.bytes;
        self.identity.modified_ns = current.modified_ns;
        let sealed_sha256 = sha256_file_handle(&self.file, current.bytes)?;
        self.file = self.file.try_clone()?;
        Ok(SealedPrivateOutput {
            sealed_identity: current,
            sealed_sha256,
            inner: self,
        })
    }
}

impl SealedPrivateOutput {
    pub fn private_path_hint(&self) -> &Path {
        &self.inner.temporary_path_hint
    }

    pub fn identity(&self) -> &SourceIdentity {
        &self.sealed_identity
    }

    pub fn sha256(&self) -> &[u8; 32] {
        &self.sealed_sha256
    }

    pub fn assert_staged_current(&self) -> Result<(), LifecycleError> {
        let reservation = self
            .inner
            .reservation
            .as_ref()
            .ok_or(LifecycleError::PublicationFailed)?;
        reservation.assert_current()?;
        let held = identity_from_file(&self.inner.file)?;
        let reopened =
            open_relative_readonly(&self.inner.staging_parent, &self.inner.temporary_leaf)?;
        let named = identity_from_file(&reopened)?;
        if held != self.sealed_identity
            || named != self.sealed_identity
            || sha256_file_handle(&self.inner.file, self.sealed_identity.bytes)?
                != self.sealed_sha256
        {
            return Err(LifecycleError::PrivateOutputIncomplete);
        }
        Ok(())
    }

    /// Perform the low-level no-replace rename after the trusted workspace has
    /// exhaustively verified these exact staged bytes and rebound every input.
    ///
    /// # Safety
    ///
    /// The caller must be the trusted workspace publication state machine. It
    /// must have verified this held file's exact identity and digest for
    /// structure, signatures, declared checks, integrity, foreign keys and the
    /// operation-specific postconditions immediately before this call. The
    /// callback must repeat the same required checks against the reopened held
    /// output before returning success. Calling this for arbitrary bytes can
    /// publish an invalid capsule.
    pub unsafe fn publish_no_replace_unchecked<F>(
        mut self,
        verify_postpublish: F,
    ) -> Result<PublishedOutput, LifecycleError>
    where
        F: FnOnce(&File, &SourceIdentity) -> Result<(), LifecycleError>,
    {
        let reservation = self
            .inner
            .reservation
            .as_ref()
            .ok_or(LifecycleError::PublicationFailed)?;
        reservation.assert_current()?;
        require_destination_family_absent(&reservation.parent, &reservation.leaf)?;
        let publication_file =
            open_relative_for_publish(&self.inner.staging_parent, &self.inner.temporary_leaf)?;
        if identity_from_file(&publication_file)? != self.sealed_identity
            || sha256_file_handle(&publication_file, self.sealed_identity.bytes)?
                != self.sealed_sha256
        {
            return Err(LifecycleError::PrivateOutputIncomplete);
        }
        rename_relative_no_replace(
            &publication_file,
            &self.inner.staging_parent,
            &reservation.parent,
            &self.inner.temporary_leaf,
            &reservation.leaf,
        )?;
        self.inner.published = true;
        drop(publication_file);
        let _ = remove_directory_relative(
            &reservation.parent,
            &self.inner.staging_directory_leaf,
            &self.inner.staging_parent,
        );
        let postpublish = (|| -> Result<(File, SourceIdentity), LifecycleError> {
            sync_directory(&reservation.parent)?;
            reservation.assert_current()?;
            require_destination_sidecars_absent(&reservation.parent, &reservation.leaf)?;
            let reopened = open_relative_readonly(&reservation.parent, &reservation.leaf)?;
            let reopened_identity = identity_from_file(&reopened)?;
            Ok((reopened, reopened_identity))
        })();
        let (reopened, reopened_identity) = match postpublish {
            Ok(value) => value,
            Err(_) => {
                quarantine_or_mark(reservation, &self.inner.file);
                return Err(LifecycleError::PostPublishVerification);
            }
        };
        let reopened_sha256 = match sha256_file_handle(&reopened, self.sealed_identity.bytes) {
            Ok(digest) => digest,
            Err(_) => {
                quarantine_or_mark(reservation, &reopened);
                return Err(LifecycleError::PostPublishVerification);
            }
        };
        if reopened_identity != self.sealed_identity || reopened_sha256 != self.sealed_sha256 {
            quarantine_or_mark(reservation, &reopened);
            return Err(LifecycleError::PostPublishVerification);
        }
        if reservation
            .input_identities
            .iter()
            .any(|input| input_matches_identity(input, &reopened_identity))
        {
            quarantine_or_mark(reservation, &reopened);
            return Err(LifecycleError::DestinationAliasesInput);
        }
        if verify_postpublish(&reopened, &reopened_identity).is_err() {
            quarantine_or_mark(reservation, &reopened);
            return Err(LifecycleError::PostPublishVerification);
        }
        if require_destination_sidecars_absent(&reservation.parent, &reservation.leaf).is_err() {
            quarantine_or_mark(reservation, &reopened);
            return Err(LifecycleError::PostPublishVerification);
        }
        let final_reopened = match open_relative_readonly(&reservation.parent, &reservation.leaf) {
            Ok(file) => file,
            Err(_) => {
                quarantine_or_mark(reservation, &reopened);
                return Err(LifecycleError::PostPublishVerification);
            }
        };
        if identity_from_file(&final_reopened).ok().as_ref() != Some(&reopened_identity)
            || sha256_file_handle(&final_reopened, self.sealed_identity.bytes)
                .ok()
                .as_ref()
                != Some(&self.sealed_sha256)
        {
            quarantine_or_mark(reservation, &reopened);
            return Err(LifecycleError::PostPublishVerification);
        }
        if sync_directory(&reservation.parent).is_err()
            || reservation.assert_current().is_err()
            || require_destination_sidecars_absent(&reservation.parent, &reservation.leaf).is_err()
        {
            quarantine_or_mark(reservation, &reopened);
            return Err(LifecycleError::PostPublishVerification);
        }
        Ok(PublishedOutput {
            path: reservation.parent_path.join(&reservation.leaf),
            identity: reopened_identity,
        })
    }
}

fn require_destination_family_absent(parent: &File, leaf: &OsStr) -> Result<(), LifecycleError> {
    if relative_leaf_exists(parent, leaf)? {
        return Err(LifecycleError::DestinationExists);
    }
    require_destination_sidecars_absent(parent, leaf)
}

fn require_destination_sidecars_absent(parent: &File, leaf: &OsStr) -> Result<(), LifecycleError> {
    for suffix in ["-journal", "-wal", "-shm"] {
        let mut companion = OsString::from(leaf);
        companion.push(suffix);
        if relative_leaf_exists(parent, &companion)? {
            return Err(LifecycleError::DestinationExists);
        }
    }
    Ok(())
}

fn normalize_display_path(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        strip_windows_verbatim_prefix(&path)
    }
    #[cfg(not(windows))]
    {
        path
    }
}

fn quarantine_or_mark(reservation: &DestinationReservation, published: &File) {
    for _ in 0..8 {
        if let Ok(quarantine) = private_quarantine_leaf()
            && quarantine_relative(
                published,
                &reservation.parent,
                &reservation.leaf,
                &quarantine,
            )
            .is_ok()
        {
            let _ = sync_directory(&reservation.parent);
            return;
        }
    }
    // If exact-handle quarantine is unavailable, leave a private create-new
    // marker with a bounded random name. Never reuse the destination leaf in
    // the marker name: it may already be at the portable component limit.
    for _ in 0..8 {
        if let Ok(random_leaf) = private_quarantine_leaf() {
            let marker = OsString::from(format!("{}.marker", random_leaf.to_string_lossy()));
            if let Ok(mut file) = create_private_relative(&reservation.parent, &marker) {
                let _ = file.write_all(b"SQLite Capsule publication verification failed.\n");
                let _ = file.sync_all();
                let _ = sync_directory(&reservation.parent);
                return;
            }
        }
    }
    let _ = sync_directory(&reservation.parent);
}

impl Drop for PrivateOutput {
    fn drop(&mut self) {
        if self.published {
            return;
        }
        if let Some(reservation) = self.reservation.as_ref() {
            let _ = unlink_relative(&self.staging_parent, &self.temporary_leaf);
            let _ = remove_directory_relative(
                &reservation.parent,
                &self.staging_directory_leaf,
                &self.staging_parent,
            );
            let _ = sync_directory(&reservation.parent);
        }
    }
}

fn same_file_object(left: &SourceIdentity, right: &SourceIdentity) -> bool {
    left.device == right.device
        && if left.stable_file_id.is_empty() || right.stable_file_id.is_empty() {
            left.file == right.file
        } else {
            left.stable_file_id == right.stable_file_id
        }
}

fn sha256_file_handle(file: &File, expected_bytes: u64) -> Result<[u8; 32], LifecycleError> {
    use std::io::{Read, Seek, SeekFrom};
    let mut reader = file.try_clone()?;
    reader.seek(SeekFrom::Start(0))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(read as u64)
            .ok_or(LifecycleError::PrivateOutputIncomplete)?;
        if total > expected_bytes {
            return Err(LifecycleError::PrivateOutputIncomplete);
        }
        digest.update(&buffer[..read]);
    }
    if total != expected_bytes {
        return Err(LifecycleError::PrivateOutputIncomplete);
    }
    Ok(digest.finalize().into())
}

fn input_matches_identity(input: &(u64, u64, String), identity: &SourceIdentity) -> bool {
    input.0 == identity.device
        && if input.2.is_empty() || identity.stable_file_id.is_empty() {
            input.1 == identity.file
        } else {
            input.2 == identity.stable_file_id
        }
}

fn validate_destination_leaf(leaf: &OsStr) -> Result<(), LifecycleError> {
    let value = leaf.to_str().ok_or(LifecycleError::UnsafeDestinationLeaf)?;
    if value.is_empty()
        || value.len() > 255
        || matches!(value, "." | "..")
        || value.ends_with([' ', '.'])
        || value.chars().any(|character| {
            character.is_control()
                || matches!(
                    character,
                    '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
                )
        })
    {
        return Err(LifecycleError::UnsafeDestinationLeaf);
    }
    let stem = value
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches([' ', '.'])
        .to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        || ["COM¹", "COM²", "COM³", "LPT¹", "LPT²", "LPT³"].contains(&stem.as_str())
    {
        return Err(LifecycleError::UnsafeDestinationLeaf);
    }
    Ok(())
}

fn private_temporary_leaf() -> Result<OsString, LifecycleError> {
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random)
        .map_err(|error| LifecycleError::Platform(format!("random source failed: {error}")))?;
    Ok(OsString::from(format!(
        ".sqlite-capsule-{}-{}.private",
        std::process::id(),
        lower_hex(&random)
    )))
}

fn private_quarantine_leaf() -> Result<OsString, LifecycleError> {
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random)
        .map_err(|error| LifecycleError::Platform(format!("random source failed: {error}")))?;
    Ok(OsString::from(format!(
        ".sqlite-capsule-failed-{}-{}.quarantine",
        std::process::id(),
        lower_hex(&random)
    )))
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

fn directory_identity_from_file(file: &File) -> Result<DirectoryIdentity, LifecycleError> {
    let identity = identity_from_file(file)?;
    Ok(DirectoryIdentity {
        device: identity.device,
        file: identity.file,
        stable_file_id: identity.stable_file_id,
    })
}

fn directory_identity_from_path(path: &Path) -> Result<DirectoryIdentity, LifecycleError> {
    let file = open_parent_directory_walk(path)?;
    directory_identity_from_file(&file)
}

#[cfg(windows)]
fn strip_windows_verbatim_prefix(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(ordinary) = text.strip_prefix(r"\\?\") {
        PathBuf::from(ordinary)
    } else {
        path.to_path_buf()
    }
}

#[cfg(unix)]
fn open_parent_directory_walk(path: &Path) -> Result<File, LifecycleError> {
    use std::os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::OpenOptionsExt,
    };

    if !path.is_absolute() {
        return Err(LifecycleError::UnsafeDestinationParent);
    }
    let mut current = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(Path::new("/"))?;
    for component in path.components() {
        let std::path::Component::Normal(name) = component else {
            if matches!(component, std::path::Component::RootDir) {
                continue;
            }
            return Err(LifecycleError::UnsafeDestinationParent);
        };
        let name = relative_cstring(name)?;
        let descriptor = unsafe {
            libc::openat(
                current.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if descriptor < 0 {
            return Err(LifecycleError::UnsafeDestinationParent);
        }
        current = unsafe { File::from_raw_fd(descriptor) };
    }
    Ok(current)
}

#[cfg(unix)]
fn relative_cstring(leaf: &OsStr) -> Result<std::ffi::CString, LifecycleError> {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::CString::new(leaf.as_bytes()).map_err(|_| LifecycleError::UnsafeDestinationLeaf)
}

#[cfg(unix)]
fn create_private_relative(parent: &File, leaf: &OsStr) -> Result<File, LifecycleError> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let leaf = relative_cstring(leaf)?;
    let descriptor = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            leaf.as_ptr(),
            libc::O_CREAT | libc::O_EXCL | libc::O_RDWR | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if descriptor < 0 {
        let error = io::Error::last_os_error();
        return if error.kind() == io::ErrorKind::AlreadyExists {
            Err(LifecycleError::DestinationExists)
        } else {
            Err(error.into())
        };
    }
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

#[cfg(unix)]
fn create_private_directory_relative(parent: &File, leaf: &OsStr) -> Result<File, LifecycleError> {
    use std::os::fd::AsRawFd;
    let leaf_name = leaf.to_owned();
    let leaf = relative_cstring(leaf)?;
    if unsafe { libc::mkdirat(parent.as_raw_fd(), leaf.as_ptr(), 0o700) } != 0 {
        let error = io::Error::last_os_error();
        return if error.kind() == io::ErrorKind::AlreadyExists {
            Err(LifecycleError::DestinationExists)
        } else {
            Err(error.into())
        };
    }
    match open_relative_directory(parent, &leaf_name) {
        Ok(directory) => Ok(directory),
        Err(error) => {
            let _ =
                unsafe { libc::unlinkat(parent.as_raw_fd(), leaf.as_ptr(), libc::AT_REMOVEDIR) };
            Err(error)
        }
    }
}

#[cfg(unix)]
fn open_relative_directory(parent: &File, leaf: &OsStr) -> Result<File, LifecycleError> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let leaf = relative_cstring(leaf)?;
    let descriptor = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            leaf.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

#[cfg(unix)]
fn relative_leaf_exists(parent: &File, leaf: &OsStr) -> Result<bool, LifecycleError> {
    use std::os::fd::AsRawFd;
    let leaf = relative_cstring(leaf)?;
    let mut metadata = std::mem::MaybeUninit::<libc::stat>::zeroed();
    let result = unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            leaf.as_ptr(),
            metadata.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result == 0 {
        Ok(true)
    } else {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::NotFound {
            Ok(false)
        } else {
            Err(error.into())
        }
    }
}

#[cfg(unix)]
fn open_relative_readonly(parent: &File, leaf: &OsStr) -> Result<File, LifecycleError> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let leaf = relative_cstring(leaf)?;
    let descriptor = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            leaf.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

#[cfg(unix)]
fn open_relative_for_publish(parent: &File, leaf: &OsStr) -> Result<File, LifecycleError> {
    open_relative_readonly(parent, leaf)
}

#[cfg(unix)]
fn rename_relative_no_replace(
    _file: &File,
    source_parent: &File,
    destination_parent: &File,
    source: &OsStr,
    destination: &OsStr,
) -> Result<(), LifecycleError> {
    use std::os::fd::AsRawFd;
    let source = relative_cstring(source)?;
    let destination = relative_cstring(destination)?;
    #[cfg(target_os = "linux")]
    {
        let result = unsafe {
            libc::renameat2(
                source_parent.as_raw_fd(),
                source.as_ptr(),
                destination_parent.as_raw_fd(),
                destination.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if result == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        return if error.kind() == io::ErrorKind::AlreadyExists {
            Err(LifecycleError::DestinationExists)
        } else {
            Err(LifecycleError::Platform(format!(
                "SetFileInformationByHandle failed: {error}"
            )))
        };
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (
            _file,
            source_parent,
            destination_parent,
            source,
            destination,
        );
        // A portable link-then-unlink fallback has a crash window in which the
        // final name exists while the operation reports failure. Platforms
        // without a proven exclusive-rename primitive therefore fail closed.
        Err(LifecycleError::PublicationFailed)
    }
}

#[cfg(unix)]
fn remove_directory_relative(
    parent: &File,
    leaf: &OsStr,
    _directory: &File,
) -> Result<(), LifecycleError> {
    use std::os::fd::AsRawFd;
    let leaf = relative_cstring(leaf)?;
    if unsafe { libc::unlinkat(parent.as_raw_fd(), leaf.as_ptr(), libc::AT_REMOVEDIR) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error().into())
    }
}

#[cfg(unix)]
fn quarantine_relative(
    _file: &File,
    _parent: &File,
    _source: &OsStr,
    _quarantine: &OsStr,
) -> Result<(), LifecycleError> {
    // Renaming by a mutable directory entry could quarantine an attacker-
    // substituted inode. The caller creates an owner-private failure marker
    // instead; the untrusted final name is never reported as successful.
    Err(LifecycleError::PostPublishVerification)
}

#[cfg(unix)]
fn unlink_relative(parent: &File, leaf: &OsStr) -> Result<(), LifecycleError> {
    use std::os::fd::AsRawFd;
    let leaf = relative_cstring(leaf)?;
    if unsafe { libc::unlinkat(parent.as_raw_fd(), leaf.as_ptr(), 0) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error().into())
    }
}

#[cfg(unix)]
fn sync_directory(parent: &File) -> Result<(), LifecycleError> {
    parent.sync_all()?;
    Ok(())
}

#[cfg(windows)]
fn open_parent_directory(path: &Path) -> Result<File, LifecycleError> {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        Storage::FileSystem::{
            CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
            FILE_TRAVERSE, OPEN_EXISTING,
        },
    };
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_READ_ATTRIBUTES | FILE_TRAVERSE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error().into());
    }
    let file = unsafe { File::from_raw_handle(handle) };
    validate_windows_directory_handle(&file)?;
    Ok(file)
}

#[cfg(windows)]
fn open_parent_directory_walk(path: &Path) -> Result<File, LifecycleError> {
    use std::path::Component;

    if !path.is_absolute() {
        return Err(LifecycleError::UnsafeDestinationParent);
    }
    let mut components = path.components();
    let Component::Prefix(prefix) = components
        .next()
        .ok_or(LifecycleError::UnsafeDestinationParent)?
    else {
        return Err(LifecycleError::UnsafeDestinationParent);
    };
    if !matches!(components.next(), Some(Component::RootDir)) {
        return Err(LifecycleError::UnsafeDestinationParent);
    }
    let mut root = PathBuf::from(prefix.as_os_str());
    root.push(Path::new(r"\"));
    let mut current = open_parent_directory(&root)?;
    for component in components {
        let Component::Normal(name) = component else {
            return Err(LifecycleError::UnsafeDestinationParent);
        };
        current = windows_relative_open_directory(&current, name)
            .map_err(|_| LifecycleError::UnsafeDestinationParent)?;
    }
    Ok(current)
}

#[cfg(windows)]
fn validate_windows_directory_handle(file: &File) -> Result<(), LifecycleError> {
    use std::{mem::MaybeUninit, os::windows::io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO,
        FileAttributeTagInfo, GetFileInformationByHandleEx,
    };

    let mut info = MaybeUninit::<FILE_ATTRIBUTE_TAG_INFO>::zeroed();
    let ok = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileAttributeTagInfo,
            info.as_mut_ptr().cast(),
            std::mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    };
    if ok == 0 {
        return Err(LifecycleError::Io(io::Error::last_os_error()));
    }
    let info = unsafe { info.assume_init() };
    if info.FileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
        || info.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err(LifecycleError::UnsafeDestinationParent);
    }
    Ok(())
}

#[cfg(windows)]
fn windows_relative_open_directory(parent: &File, leaf: &OsStr) -> Result<File, LifecycleError> {
    use std::{
        mem::size_of,
        os::windows::{
            ffi::OsStrExt,
            io::{AsRawHandle, FromRawHandle},
        },
    };
    use windows_sys::{
        Wdk::{
            Foundation::OBJECT_ATTRIBUTES,
            Storage::FileSystem::{
                FILE_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_REPARSE_POINT, NtCreateFile,
            },
        },
        Win32::{
            Foundation::{HANDLE, OBJ_CASE_INSENSITIVE, UNICODE_STRING},
            Storage::FileSystem::{
                FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
            },
            System::IO::IO_STATUS_BLOCK,
        },
    };

    let mut wide: Vec<u16> = leaf.encode_wide().collect();
    let byte_length = u16::try_from(
        wide.len()
            .checked_mul(2)
            .ok_or(LifecycleError::UnsafeDestinationParent)?,
    )
    .map_err(|_| LifecycleError::UnsafeDestinationParent)?;
    let name = UNICODE_STRING {
        Length: byte_length,
        MaximumLength: byte_length,
        Buffer: wide.as_mut_ptr(),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: parent.as_raw_handle(),
        ObjectName: &name,
        Attributes: OBJ_CASE_INSENSITIVE,
        SecurityDescriptor: std::ptr::null(),
        SecurityQualityOfService: std::ptr::null(),
    };
    let mut status = IO_STATUS_BLOCK::default();
    let mut handle: HANDLE = std::ptr::null_mut();
    let ntstatus = unsafe {
        NtCreateFile(
            &mut handle,
            FILE_READ_ATTRIBUTES,
            &attributes,
            &mut status,
            std::ptr::null(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            FILE_OPEN,
            FILE_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT,
            std::ptr::null(),
            0,
        )
    };
    if ntstatus < 0 {
        return Err(LifecycleError::Platform(format!(
            "NtCreateFile directory component failed: 0x{ntstatus:08x}"
        )));
    }
    let file = unsafe { File::from_raw_handle(handle) };
    validate_windows_directory_handle(&file)?;
    Ok(file)
}

#[cfg(windows)]
fn windows_relative_create_directory(parent: &File, leaf: &OsStr) -> Result<File, LifecycleError> {
    use std::{
        mem::size_of,
        os::windows::{
            ffi::OsStrExt,
            io::{AsRawHandle, FromRawHandle},
        },
    };
    use windows_sys::{
        Wdk::{
            Foundation::OBJECT_ATTRIBUTES,
            Storage::FileSystem::{
                FILE_CREATE, FILE_DIRECTORY_FILE, FILE_OPEN_REPARSE_POINT,
                FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
            },
        },
        Win32::{
            Foundation::{
                HANDLE, OBJ_CASE_INSENSITIVE, STATUS_OBJECT_NAME_COLLISION, UNICODE_STRING,
            },
            Storage::FileSystem::{
                DELETE, FILE_ADD_FILE, FILE_ADD_SUBDIRECTORY, FILE_ATTRIBUTE_NORMAL,
                FILE_DELETE_CHILD, FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE,
                FILE_SHARE_READ, FILE_SHARE_WRITE, SYNCHRONIZE,
            },
            System::IO::IO_STATUS_BLOCK,
        },
    };

    let mut wide: Vec<u16> = leaf.encode_wide().collect();
    let byte_length = u16::try_from(
        wide.len()
            .checked_mul(2)
            .ok_or(LifecycleError::UnsafeDestinationLeaf)?,
    )
    .map_err(|_| LifecycleError::UnsafeDestinationLeaf)?;
    let name = UNICODE_STRING {
        Length: byte_length,
        MaximumLength: byte_length,
        Buffer: wide.as_mut_ptr(),
    };
    let mut owner_only = windows_acl::OwnerOnlySecurity::new(true)?;
    let attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: parent.as_raw_handle(),
        ObjectName: &name,
        Attributes: OBJ_CASE_INSENSITIVE,
        SecurityDescriptor: owner_only.descriptor(),
        SecurityQualityOfService: std::ptr::null(),
    };
    let mut status = IO_STATUS_BLOCK::default();
    let mut handle: HANDLE = std::ptr::null_mut();
    let ntstatus = unsafe {
        NtCreateFile(
            &mut handle,
            FILE_LIST_DIRECTORY
                | FILE_ADD_FILE
                | FILE_ADD_SUBDIRECTORY
                | FILE_DELETE_CHILD
                | FILE_READ_ATTRIBUTES
                | DELETE
                | SYNCHRONIZE,
            &attributes,
            &mut status,
            std::ptr::null(),
            FILE_ATTRIBUTE_NORMAL,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            FILE_CREATE,
            FILE_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
            std::ptr::null(),
            0,
        )
    };
    if ntstatus < 0 {
        return if ntstatus == STATUS_OBJECT_NAME_COLLISION {
            Err(LifecycleError::DestinationExists)
        } else {
            Err(LifecycleError::Platform(format!(
                "NtCreateFile private directory failed: 0x{ntstatus:08x}"
            )))
        };
    }
    let directory = unsafe { File::from_raw_handle(handle) };
    validate_windows_directory_handle(&directory)?;
    Ok(directory)
}

#[cfg(windows)]
fn windows_relative_open(
    parent: &File,
    leaf: &OsStr,
    create: bool,
    writable: bool,
    delete_access: bool,
) -> Result<File, LifecycleError> {
    use std::{
        mem::size_of,
        os::windows::{
            ffi::OsStrExt,
            io::{AsRawHandle, FromRawHandle},
        },
    };
    use windows_sys::{
        Wdk::{
            Foundation::OBJECT_ATTRIBUTES,
            Storage::FileSystem::{
                FILE_CREATE, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_REPARSE_POINT,
                FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
            },
        },
        Win32::{
            Foundation::{
                HANDLE, OBJ_CASE_INSENSITIVE, STATUS_OBJECT_NAME_COLLISION, UNICODE_STRING,
            },
            Storage::FileSystem::{
                DELETE, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
                FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, SYNCHRONIZE,
            },
            System::IO::IO_STATUS_BLOCK,
        },
    };
    let mut wide: Vec<u16> = leaf.encode_wide().collect();
    let byte_length = u16::try_from(
        wide.len()
            .checked_mul(2)
            .ok_or(LifecycleError::UnsafeDestinationLeaf)?,
    )
    .map_err(|_| LifecycleError::UnsafeDestinationLeaf)?;
    let name = UNICODE_STRING {
        Length: byte_length,
        MaximumLength: byte_length,
        Buffer: wide.as_mut_ptr(),
    };
    let mut owner_only = if create {
        Some(windows_acl::OwnerOnlySecurity::new(false)?)
    } else {
        None
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: parent.as_raw_handle(),
        ObjectName: &name,
        Attributes: OBJ_CASE_INSENSITIVE,
        SecurityDescriptor: owner_only
            .as_mut()
            .map_or(std::ptr::null_mut(), |security| security.descriptor()),
        SecurityQualityOfService: std::ptr::null(),
    };
    let mut status = IO_STATUS_BLOCK::default();
    let mut handle: HANDLE = std::ptr::null_mut();
    let access = FILE_GENERIC_READ
        | SYNCHRONIZE
        | if writable { FILE_GENERIC_WRITE } else { 0 }
        | if delete_access { DELETE } else { 0 };
    let ntstatus = unsafe {
        NtCreateFile(
            &mut handle,
            access,
            &attributes,
            &mut status,
            std::ptr::null(),
            FILE_ATTRIBUTE_NORMAL,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            if create { FILE_CREATE } else { FILE_OPEN },
            FILE_NON_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
            std::ptr::null(),
            0,
        )
    };
    if ntstatus < 0 {
        return if ntstatus == STATUS_OBJECT_NAME_COLLISION {
            Err(LifecycleError::DestinationExists)
        } else {
            Err(LifecycleError::Platform(format!(
                "NtCreateFile failed: 0x{ntstatus:08x}"
            )))
        };
    }
    Ok(unsafe { File::from_raw_handle(handle) })
}

#[cfg(windows)]
fn create_private_relative(parent: &File, leaf: &OsStr) -> Result<File, LifecycleError> {
    windows_relative_open(parent, leaf, true, true, false)
}

#[cfg(windows)]
fn create_private_directory_relative(parent: &File, leaf: &OsStr) -> Result<File, LifecycleError> {
    windows_relative_create_directory(parent, leaf)
}

#[cfg(windows)]
fn open_relative_readonly(parent: &File, leaf: &OsStr) -> Result<File, LifecycleError> {
    windows_relative_open(parent, leaf, false, false, false)
}

#[cfg(windows)]
fn open_relative_for_publish(parent: &File, leaf: &OsStr) -> Result<File, LifecycleError> {
    windows_relative_open(parent, leaf, false, true, true)
}

#[cfg(windows)]
fn relative_leaf_exists(parent: &File, leaf: &OsStr) -> Result<bool, LifecycleError> {
    match windows_relative_open(parent, leaf, false, false, false) {
        Ok(_) => Ok(true),
        Err(LifecycleError::Platform(status))
            if status.contains("0xc0000034") || status.contains("0xc000003a") =>
        {
            Ok(false)
        }
        Err(error) => Err(error),
    }
}

#[cfg(windows)]
fn rename_relative_no_replace(
    file: &File,
    _source_parent: &File,
    destination_parent: &File,
    _source: &OsStr,
    destination: &OsStr,
) -> Result<(), LifecycleError> {
    use std::{
        mem::size_of,
        os::windows::{ffi::OsStrExt, io::AsRawHandle},
    };
    use windows_sys::{
        Wdk::Storage::FileSystem::{FileRenameInformation, NtSetInformationFile},
        Win32::{Storage::FileSystem::FILE_RENAME_INFO, System::IO::IO_STATUS_BLOCK},
    };
    let name: Vec<u16> = destination.encode_wide().collect();
    // Windows requires at least sizeof(FILE_RENAME_INFO) plus the complete
    // non-NUL-terminated relative name payload.
    let header = size_of::<FILE_RENAME_INFO>();
    let bytes = header
        .checked_add(
            name.len()
                .checked_mul(2)
                .ok_or(LifecycleError::UnsafeDestinationLeaf)?,
        )
        .ok_or(LifecycleError::UnsafeDestinationLeaf)?;
    let words = bytes.div_ceil(size_of::<usize>());
    let mut buffer = vec![0_usize; words];
    let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    unsafe {
        (*info).Anonymous.Flags = 0;
        (*info).RootDirectory = destination_parent.as_raw_handle();
        (*info).FileNameLength =
            u32::try_from(name.len() * 2).map_err(|_| LifecycleError::UnsafeDestinationLeaf)?;
        std::ptr::copy_nonoverlapping(name.as_ptr(), (*info).FileName.as_mut_ptr(), name.len());
    }
    let mut status = IO_STATUS_BLOCK::default();
    let ntstatus = unsafe {
        NtSetInformationFile(
            file.as_raw_handle(),
            &mut status,
            buffer.as_ptr().cast(),
            u32::try_from(bytes).map_err(|_| LifecycleError::UnsafeDestinationLeaf)?,
            FileRenameInformation,
        )
    };
    if ntstatus < 0 {
        return if ntstatus == windows_sys::Win32::Foundation::STATUS_OBJECT_NAME_COLLISION {
            Err(LifecycleError::DestinationExists)
        } else {
            Err(LifecycleError::Platform(format!(
                "NtSetInformationFile rename failed: 0x{ntstatus:08x}"
            )))
        };
    }
    Ok(())
}

#[cfg(windows)]
fn quarantine_relative(
    file: &File,
    parent: &File,
    source: &OsStr,
    quarantine: &OsStr,
) -> Result<(), LifecycleError> {
    rename_relative_no_replace(file, parent, parent, source, quarantine)
}

#[cfg(windows)]
fn remove_directory_relative(
    _parent: &File,
    _leaf: &OsStr,
    directory: &File,
) -> Result<(), LifecycleError> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_DISPOSITION_INFO, FileDispositionInfo, SetFileInformationByHandle,
    };
    let info = FILE_DISPOSITION_INFO { DeleteFile: true };
    let ok = unsafe {
        SetFileInformationByHandle(
            directory.as_raw_handle(),
            FileDispositionInfo,
            (&info as *const FILE_DISPOSITION_INFO).cast(),
            std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    };
    if ok == 0 {
        Err(io::Error::last_os_error().into())
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn unlink_relative(parent: &File, leaf: &OsStr) -> Result<(), LifecycleError> {
    // Removal by handle is not needed on the success path. Dropped private
    // outputs are removed using a handle-relative quarantine name and deletion
    // on the verified parent path only after parent identity revalidation.
    let file = windows_relative_open(parent, leaf, false, true, true)?;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_DISPOSITION_INFO, FileDispositionInfo, SetFileInformationByHandle,
    };
    let info = FILE_DISPOSITION_INFO { DeleteFile: true };
    let ok = unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileDispositionInfo,
            (&info as *const FILE_DISPOSITION_INFO).cast(),
            std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    };
    if ok == 0 {
        Err(io::Error::last_os_error().into())
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn sync_directory(parent: &File) -> Result<(), LifecycleError> {
    // Windows exposes no portable directory-fsync equivalent for the held
    // FILE_LIST_DIRECTORY handle. File data is already FlushFileBuffers'd via
    // File::sync_all before the atomic no-replace rename. Keep the parent handle
    // pinned and revalidated; do not turn a successful rename into false failure.
    let _ = parent;
    Ok(())
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

#[cfg(not(windows))]
fn identity_from_file(file: &File) -> Result<SourceIdentity, LifecycleError> {
    let metadata = file.metadata()?;
    identity_from_metadata(&metadata, Some(file))
}

#[cfg(windows)]
fn identity_from_file(file: &File) -> Result<SourceIdentity, LifecycleError> {
    identity_from_windows_handle(file)
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
        stable_file_id: format!("{:016x}", metadata.ino()),
        bytes: metadata.len(),
        modified_ns: if metadata.mtime() < 0 {
            0
        } else {
            u64::try_from(metadata.mtime())
                .unwrap_or(u64::MAX)
                .saturating_mul(1_000_000_000)
                .saturating_add(u64::try_from(metadata.mtime_nsec()).unwrap_or(0))
        },
    })
}

#[cfg(windows)]
fn identity_from_windows_handle(file: &File) -> Result<SourceIdentity, LifecycleError> {
    use std::{mem::MaybeUninit, os::windows::io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_ID_INFO, FileIdInfo, GetFileInformationByHandle,
        GetFileInformationByHandleEx,
    };

    let mut info = MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::zeroed();
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) };
    if ok == 0 {
        return Err(LifecycleError::Io(io::Error::last_os_error()));
    }
    let owned = unsafe { info.assume_init() };
    let mut id_info = MaybeUninit::<FILE_ID_INFO>::zeroed();
    let id_ok = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileIdInfo,
            id_info.as_mut_ptr().cast(),
            std::mem::size_of::<FILE_ID_INFO>() as u32,
        )
    };
    if id_ok == 0 {
        return Err(LifecycleError::Io(io::Error::last_os_error()));
    }
    let id_info = unsafe { id_info.assume_init() };
    let last_write_100ns = (u64::from(owned.ftLastWriteTime.dwHighDateTime) << 32)
        | u64::from(owned.ftLastWriteTime.dwLowDateTime);
    let modified_ns = last_write_100ns
        .saturating_sub(116_444_736_000_000_000)
        .saturating_mul(100);
    Ok(SourceIdentity {
        device: id_info.VolumeSerialNumber,
        file: (u64::from(owned.nFileIndexHigh) << 32) | u64::from(owned.nFileIndexLow),
        stable_file_id: lower_hex(&id_info.FileId.Identifier),
        bytes: (u64::from(owned.nFileSizeHigh) << 32) | u64::from(owned.nFileSizeLow),
        modified_ns,
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
    digest.update(source.identity.stable_file_id.as_bytes());
    if source.identity.stable_file_id.is_empty() {
        digest.update(source.identity.file.to_le_bytes());
    }
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
            InitializeAcl, InitializeSecurityDescriptor, OBJECT_INHERIT_ACE,
            PROTECTED_DACL_SECURITY_INFORMATION, PSID, SE_DACL_PROTECTED, SECURITY_DESCRIPTOR,
            SetSecurityDescriptorDacl, TOKEN_QUERY, TOKEN_USER, TokenUser,
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

    pub(super) struct OwnerOnlySecurity {
        _acl_buffer: Vec<usize>,
        descriptor: Box<SECURITY_DESCRIPTOR>,
    }

    impl OwnerOnlySecurity {
        pub(super) fn new(directory: bool) -> Result<Self, LifecycleError> {
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
                let acl_bytes = size_of::<ACL>() + size_of::<ACCESS_ALLOWED_ACE>()
                    - size_of::<u32>()
                    + sid_bytes as usize;
                let mut acl_buffer = vec![0_usize; acl_bytes.div_ceil(size_of::<usize>())];
                let acl = acl_buffer.as_mut_ptr().cast::<ACL>();
                if InitializeAcl(acl, size_of_val(acl_buffer.as_slice()) as u32, ACL_REVISION) == 0
                {
                    return Err(io::Error::last_os_error().into());
                }
                let inheritance = if directory {
                    OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE
                } else {
                    0
                };
                if AddAccessAllowedAceEx(acl, ACL_REVISION, inheritance, FILE_ALL_ACCESS, sid) == 0
                {
                    return Err(io::Error::last_os_error().into());
                }
                let mut descriptor = Box::<SECURITY_DESCRIPTOR>::default();
                let descriptor_ptr = (&mut *descriptor as *mut SECURITY_DESCRIPTOR).cast();
                if InitializeSecurityDescriptor(descriptor_ptr, 1) == 0
                    || SetSecurityDescriptorDacl(descriptor_ptr, 1, acl, 0) == 0
                {
                    return Err(io::Error::last_os_error().into());
                }
                descriptor.Control |= SE_DACL_PROTECTED;
                Ok(Self {
                    _acl_buffer: acl_buffer,
                    descriptor,
                })
            }
        }

        pub(super) fn descriptor(&mut self) -> *const SECURITY_DESCRIPTOR {
            &*self.descriptor
        }

        fn acl(&mut self) -> *mut ACL {
            self._acl_buffer.as_mut_ptr().cast()
        }
    }

    pub(super) fn protect(path: &Path, directory: bool) -> Result<(), LifecycleError> {
        unsafe {
            let mut security = OwnerOnlySecurity::new(directory)?;
            let mut wide: Vec<u16> = OsStr::new(path).encode_wide().collect();
            wide.push(0);
            let result = SetNamedSecurityInfoW(
                wide.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                security.acl(),
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

    #[cfg(windows)]
    fn make_parent_acl_world_writable(path: &Path) {
        use std::{mem::size_of, os::windows::ffi::OsStrExt, ptr};
        use windows_sys::Win32::{
            Security::{
                ACCESS_ALLOWED_ACE, ACL, ACL_REVISION, AddAccessAllowedAceEx,
                Authorization::{SE_FILE_OBJECT, SetNamedSecurityInfoW},
                CONTAINER_INHERIT_ACE, CreateWellKnownSid, DACL_SECURITY_INFORMATION,
                InitializeAcl, OBJECT_INHERIT_ACE, SECURITY_MAX_SID_SIZE,
                UNPROTECTED_DACL_SECURITY_INFORMATION, WinWorldSid,
            },
            Storage::FileSystem::FILE_ALL_ACCESS,
        };

        let mut world_sid = vec![0_u8; SECURITY_MAX_SID_SIZE as usize];
        let mut world_sid_bytes = SECURITY_MAX_SID_SIZE;
        assert_ne!(
            unsafe {
                CreateWellKnownSid(
                    WinWorldSid,
                    ptr::null_mut(),
                    world_sid.as_mut_ptr().cast(),
                    &mut world_sid_bytes,
                )
            },
            0,
            "create Everyone SID: {}",
            io::Error::last_os_error()
        );
        let acl_bytes = size_of::<ACL>() + size_of::<ACCESS_ALLOWED_ACE>() - size_of::<u32>()
            + world_sid_bytes as usize;
        let mut acl_storage = vec![0_usize; acl_bytes.div_ceil(size_of::<usize>())];
        let acl = acl_storage.as_mut_ptr().cast::<ACL>();
        assert_ne!(
            unsafe {
                InitializeAcl(
                    acl,
                    (acl_storage.len() * size_of::<usize>()) as u32,
                    ACL_REVISION,
                )
            },
            0,
            "initialize hostile parent ACL: {}",
            io::Error::last_os_error()
        );
        assert_ne!(
            unsafe {
                AddAccessAllowedAceEx(
                    acl,
                    ACL_REVISION,
                    OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE,
                    FILE_ALL_ACCESS,
                    world_sid.as_mut_ptr().cast(),
                )
            },
            0,
            "add hostile parent ACE: {}",
            io::Error::last_os_error()
        );
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        assert_eq!(
            unsafe {
                SetNamedSecurityInfoW(
                    wide.as_ptr(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION | UNPROTECTED_DACL_SECURITY_INFORMATION,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    acl,
                    ptr::null(),
                )
            },
            0,
            "install hostile inheritable parent ACL"
        );
    }

    #[cfg(windows)]
    fn protected_dacl_ace_count(path: &Path) -> (bool, u16) {
        use std::{os::windows::ffi::OsStrExt, ptr};
        use windows_sys::Win32::{
            Foundation::LocalFree,
            Security::{
                ACL,
                Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT},
                DACL_SECURITY_INFORMATION, GetSecurityDescriptorControl, PSECURITY_DESCRIPTOR,
                SE_DACL_PROTECTED,
            },
        };

        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut dacl: *mut ACL = ptr::null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        assert_eq!(
            unsafe {
                GetNamedSecurityInfoW(
                    wide.as_ptr(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    &mut dacl,
                    ptr::null_mut(),
                    &mut descriptor,
                )
            },
            0,
            "read staging DACL"
        );
        assert!(!descriptor.is_null(), "security descriptor is present");
        assert!(!dacl.is_null(), "staging DACL is present");
        let mut control = 0_u16;
        let mut revision = 0_u32;
        assert_ne!(
            unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) },
            0,
            "read security descriptor control"
        );
        let ace_count = unsafe { (*dacl).AceCount };
        unsafe {
            LocalFree(descriptor);
        }
        (control & SE_DACL_PROTECTED != 0, ace_count)
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
    fn windows_identity_binds_full_file_id_and_observed_mtime() {
        let directory = TestDirectory::new();
        let path = source(&directory);
        let pinned = PinnedSource::open(&path, false).expect("pin source");
        assert_eq!(pinned.identity().stable_file_id.len(), 32);
        assert!(
            pinned
                .identity()
                .stable_file_id
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        );
        assert!(pinned.identity().modified_ns > 0);
        pinned.assert_current().expect("full identity rebinds");
    }

    #[cfg(windows)]
    #[test]
    fn windows_intermediate_junction_is_rejected() {
        use std::process::Command;

        let directory = TestDirectory::new();
        let target = directory.0.join("target");
        let junction = directory.0.join("junction");
        fs::create_dir(&target).expect("create junction target");
        let output = Command::new("cmd")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&junction)
            .arg(&target)
            .output()
            .expect("run mklink");
        assert!(
            output.status.success(),
            "create test junction: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(matches!(
            DestinationReservation::reserve(&junction, OsStr::new("blocked.capsule.sqlite"), &[]),
            Err(LifecycleError::UnsafeDestinationParent)
        ));
        fs::remove_dir(&junction).expect("remove junction without traversing it");
    }

    #[test]
    fn parent_substitution_is_blocked_or_detected_before_staging() {
        let directory = TestDirectory::new();
        let parent = directory.0.join("destination");
        let moved = directory.0.join("destination-moved");
        fs::create_dir(&parent).expect("create destination parent");
        let reservation =
            DestinationReservation::reserve(&parent, OsStr::new("blocked.capsule.sqlite"), &[])
                .expect("reserve destination");
        match fs::rename(&parent, &moved) {
            Ok(()) => {
                fs::create_dir(&parent).expect("substitute destination parent");
                assert!(reservation.stage().is_err());
            }
            Err(_) => reservation
                .assert_reserved_current()
                .expect("held parent prevented substitution and remains current"),
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_staging_dacl_is_owner_only_and_protected_from_hostile_inheritance() {
        let directory = TestDirectory::new();
        make_parent_acl_world_writable(&directory.0);
        let reservation = DestinationReservation::reserve(
            &directory.0,
            OsStr::new("protected.capsule.sqlite"),
            &[],
        )
        .expect("reserve destination");
        let private = reservation.stage().expect("stage output");
        let file_path = private.private_path_hint();
        let staging_directory = file_path.parent().expect("staging parent path");
        assert_eq!(protected_dacl_ace_count(staging_directory), (true, 1));
        assert_eq!(protected_dacl_ace_count(file_path), (true, 1));
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

    #[test]
    fn low_level_publication_is_private_create_new_and_no_replace() {
        let directory = TestDirectory::new();
        let inputs = Vec::new();
        let reservation = DestinationReservation::reserve(
            &directory.0,
            OsStr::new("published.lowlevel.tmp"),
            &inputs,
        )
        .expect("reserve destination");
        let mut private = reservation.stage().expect("stage private output");
        private
            .file_mut()
            .write_all(b"low-level publication fixture")
            .expect("write private output");
        let sealed = private.seal().expect("seal output");
        // SAFETY: this is the low-level filesystem-primitive test, not a
        // lifecycle output. It verifies the exact held bytes before and after
        // the rename. Safe Capsule publication is tested by capsule-workspace.
        let published = unsafe {
            sealed.publish_no_replace_unchecked(|file, identity| {
                if file.metadata()?.len() != identity.bytes {
                    return Err(LifecycleError::PostPublishVerification);
                }
                Ok(())
            })
        }
        .expect("publish verified output");
        assert_eq!(published.path, directory.0.join("published.lowlevel.tmp"));
        assert_eq!(
            fs::read(&published.path).expect("read published file"),
            b"low-level publication fixture"
        );
        assert!(matches!(
            DestinationReservation::reserve(
                &directory.0,
                OsStr::new("published.lowlevel.tmp"),
                &inputs
            ),
            Err(LifecycleError::DestinationExists)
        ));
    }

    #[test]
    fn publication_rejects_unsafe_leaves_and_cleans_unpublished_private_files() {
        let directory = TestDirectory::new();
        for leaf in [
            "..",
            "name:stream",
            "CON",
            "COM1.txt",
            "LPT¹.txt",
            "trailing.",
            "trailing ",
        ] {
            assert!(matches!(
                DestinationReservation::reserve(&directory.0, OsStr::new(leaf), &[]),
                Err(LifecycleError::UnsafeDestinationLeaf)
            ));
        }
        let reservation =
            DestinationReservation::reserve(&directory.0, OsStr::new("unused.capsule.sqlite"), &[])
                .expect("reserve output");
        let temporary_path = {
            let private = reservation.stage().expect("stage output");
            private.private_path_hint().to_path_buf()
        };
        assert!(!temporary_path.exists(), "private output cleaned on drop");
        assert!(!directory.0.join("unused.capsule.sqlite").exists());
    }

    #[test]
    fn last_moment_destination_race_fails_without_replacement() {
        let directory = TestDirectory::new();
        let reservation =
            DestinationReservation::reserve(&directory.0, OsStr::new("raced.capsule.sqlite"), &[])
                .expect("reserve output");
        let mut private = reservation.stage().expect("stage output");
        private
            .file_mut()
            .write_all(b"private bytes")
            .expect("write output");
        let sealed = private.seal().expect("seal output");
        fs::write(directory.0.join("raced.capsule.sqlite"), b"racer bytes")
            .expect("racer creates destination");
        assert!(matches!(
            // SAFETY: deliberately races only the low-level no-replace
            // primitive; no Capsule is reported or trusted by this test.
            unsafe { sealed.publish_no_replace_unchecked(|_, _| Ok(())) },
            Err(LifecycleError::DestinationExists)
        ));
        assert_eq!(
            fs::read(directory.0.join("raced.capsule.sqlite")).expect("racer remains"),
            b"racer bytes"
        );
    }

    #[test]
    fn postpublish_failure_is_never_reported_as_success() {
        let directory = TestDirectory::new();
        let reservation =
            DestinationReservation::reserve(&directory.0, OsStr::new("bad.capsule.sqlite"), &[])
                .expect("reserve output");
        let mut private = reservation.stage().expect("stage output");
        private
            .file_mut()
            .write_all(b"private bytes")
            .expect("write output");
        let sealed = private.seal().expect("seal output");
        assert!(matches!(
            // SAFETY: this low-level test forces post-publish rejection and
            // asserts it can never be reported as success.
            unsafe {
                sealed.publish_no_replace_unchecked(|_, _| {
                    Err(LifecycleError::PostPublishVerification)
                })
            },
            Err(LifecycleError::PostPublishVerification)
        ));
        assert!(
            fs::read_dir(&directory.0)
                .expect("list quarantine")
                .any(|entry| {
                    let name = entry.expect("entry").file_name();
                    let name = name.to_string_lossy();
                    name.contains("failed") || name.contains("quarantine")
                })
        );
    }

    #[cfg(windows)]
    #[test]
    fn final_leaf_substitution_during_verification_never_reports_success() {
        let directory = TestDirectory::new();
        let final_path = directory.0.join("substituted.capsule.sqlite");
        let displaced_path = directory.0.join("displaced.capsule.sqlite");
        let reservation = DestinationReservation::reserve(
            &directory.0,
            OsStr::new("substituted.capsule.sqlite"),
            &[],
        )
        .expect("reserve output");
        let mut private = reservation.stage().expect("stage output");
        private
            .file_mut()
            .write_all(b"verified original bytes")
            .expect("write output");
        let sealed = private.seal().expect("seal output");
        assert!(matches!(
            // SAFETY: this deliberately attacks only the low-level primitive's
            // final-name rebind. It never reports the bytes as a Capsule.
            unsafe {
                sealed.publish_no_replace_unchecked(|_, _| {
                    fs::rename(&final_path, &displaced_path).map_err(LifecycleError::Io)?;
                    fs::write(&final_path, b"attacker replacement").map_err(LifecycleError::Io)?;
                    Ok(())
                })
            },
            Err(LifecycleError::PostPublishVerification)
        ));
        assert_eq!(
            fs::read(&final_path).expect("replacement remains untrusted"),
            b"attacker replacement"
        );
        assert!(
            fs::read_dir(&directory.0)
                .expect("list quarantine evidence")
                .any(|entry| {
                    let name = entry.expect("entry").file_name();
                    let name = name.to_string_lossy();
                    name.contains("failed") || name.contains("quarantine")
                })
        );
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

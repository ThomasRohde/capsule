//! Rebuildable, non-authoritative Cabinet recent-file cache.
//!
//! The cache is deliberately separate from the trust database. It stores only
//! bounded display hints, a path/file identity last observed by the host, and
//! a last-observed badge. Every reopen must inspect, verify, and evaluate trust
//! again; no value in this module grants authority.

use std::{
    collections::BTreeSet,
    error::Error,
    ffi::OsStr,
    fmt,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

#[cfg(windows)]
use std::os::windows::fs::MetadataExt;

#[cfg(unix)]
use std::fs::File;

use serde::{Deserialize, Serialize};
use sqlite_capsule_lifecycle::{SourceIdentity, prepare_private_directory, protect_private_file};

const CACHE_PROFILE: &str = "org.sqlite-capsule.cabinet-recents/1";
const CACHE_SCHEMA_VERSION: u64 = 1;
const MAX_CACHE_BYTES: u64 = 256 * 1024;
const MAX_ENTRIES: usize = 32;
const MAX_SNAPSHOT_FILES: usize = 8;
const RETAINED_SNAPSHOTS: usize = 2;
const SNAPSHOT_PREFIX: &str = "recent-v1-";
const SNAPSHOT_SUFFIX: &str = ".json";

/// A host observation accepted only after bounded validation by [`record`].
/// This is never deserialized from the trusted-shell renderer.
#[derive(Clone, Debug)]
pub struct CabinetObservation {
    pub path: PathBuf,
    pub source_identity: SourceIdentity,
    pub format_version: String,
    pub application_name: String,
    pub instance_title: Option<String>,
    pub description: String,
    pub app_id: String,
    pub app_version: String,
    pub last_opened_at: String,
    pub last_observed_badge: LastObservedBadge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LastObservedBadge {
    LegacyV02,
    V03SignatureValid,
    V03Unsigned,
    V03InvalidSignature,
}

/// Serializable recent-file display data. `authoritative` is always false.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CabinetRecentSnapshot {
    profile: &'static str,
    authoritative: bool,
    entries: Vec<CabinetRecentView>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct CabinetRecentView {
    recent_id: String,
    format_version: String,
    application_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    instance_title: Option<String>,
    description: String,
    app_id: String,
    app_version: String,
    last_opened_at: String,
    last_observed_badge: LastObservedBadge,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CabinetRecentEntry {
    recent_id: String,
    path_hint: String,
    path_identity: CachedPathIdentity,
    format_version: String,
    application_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    instance_title: Option<String>,
    description: String,
    app_id: String,
    app_version: String,
    last_opened_at: String,
    last_observed_badge: LastObservedBadge,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CachedPathIdentity {
    device: u64,
    file: u64,
    stable_file_id: String,
    bytes: u64,
    modified_ns: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredCache {
    profile: String,
    schema_version: u64,
    generation: u64,
    entries: Vec<CabinetRecentEntry>,
}

#[derive(Clone, Debug)]
pub struct CabinetRecentCache {
    root: PathBuf,
}

#[derive(Debug)]
pub enum CabinetCacheError {
    UnsafeRoot,
    InvalidObservation(&'static str),
    Io(io::Error),
    Serialization(serde_json::Error),
    Random,
}

impl fmt::Display for CabinetCacheError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsafeRoot => formatter.write_str("Cabinet cache root is unsafe"),
            Self::InvalidObservation(detail) => {
                write!(formatter, "Cabinet observation is invalid: {detail}")
            }
            Self::Io(_) => formatter.write_str("Cabinet cache storage failed"),
            Self::Serialization(_) => formatter.write_str("Cabinet cache serialization failed"),
            Self::Random => formatter.write_str("Cabinet cache random source failed"),
        }
    }
}

impl Error for CabinetCacheError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Serialization(error) => Some(error),
            Self::UnsafeRoot | Self::InvalidObservation(_) | Self::Random => None,
        }
    }
}

impl From<io::Error> for CabinetCacheError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for CabinetCacheError {
    fn from(error: serde_json::Error) -> Self {
        Self::Serialization(error)
    }
}

impl CabinetRecentCache {
    /// `root` should be a dedicated child of host app-data, never the trust DB
    /// directory itself.
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Load display hints. Missing, corrupt, oversized, or future-schema
    /// snapshots are ignored and yield an empty, rebuildable result.
    pub fn load(&self) -> Result<CabinetRecentSnapshot, CabinetCacheError> {
        let Some(stored) = self.load_stored()? else {
            return Ok(empty_snapshot());
        };
        Ok(CabinetRecentSnapshot {
            profile: CACHE_PROFILE,
            authoritative: false,
            // Displaying recents never probes cached paths (including stale
            // UNC/network locations). A later host-owned open resolves the
            // opaque recent ID and performs a fresh bounded inspection.
            entries: stored.entries.into_iter().map(recent_view).collect(),
        })
    }

    /// Add or refresh one observation and publish a new owner-private snapshot.
    /// Publication is a create-new temporary file followed by a same-directory
    /// rename to a unique, non-existing final name; prior valid snapshots remain
    /// usable until that rename succeeds.
    pub fn record(
        &self,
        observation: CabinetObservation,
    ) -> Result<CabinetRecentSnapshot, CabinetCacheError> {
        validate_observation(&observation)?;
        self.prepare_root()?;
        self.cleanup_temporary_files();
        let current = self.load_stored()?.unwrap_or_else(empty_stored);
        let generation =
            current
                .generation
                .checked_add(1)
                .ok_or(CabinetCacheError::InvalidObservation(
                    "cache generation exhausted",
                ))?;
        let mut entries = current.entries;
        let mut entry = entry_from_observation(observation)?;
        if let Some(existing) = entries.iter().find(|candidate| {
            candidate.path_hint == entry.path_hint
                || same_observed_file(&candidate.path_identity, &entry.path_identity)
        }) {
            entry.recent_id.clone_from(&existing.recent_id);
        }
        entries.retain(|candidate| {
            candidate.path_hint != entry.path_hint
                && !same_observed_file(&candidate.path_identity, &entry.path_identity)
        });
        entries.push(entry);
        entries.sort_by(|left, right| {
            right
                .last_opened_at
                .cmp(&left.last_opened_at)
                .then_with(|| left.path_hint.cmp(&right.path_hint))
        });
        entries.truncate(MAX_ENTRIES);
        let stored = StoredCache {
            profile: CACHE_PROFILE.to_owned(),
            schema_version: CACHE_SCHEMA_VERSION,
            generation,
            entries,
        };
        validate_stored(&stored)?;
        let bytes = serde_json::to_vec(&stored)?;
        if bytes.len() as u64 > MAX_CACHE_BYTES {
            return Err(CabinetCacheError::InvalidObservation(
                "serialized cache exceeds its byte ceiling",
            ));
        }
        let published = self.publish_snapshot(generation, &bytes)?;
        self.cleanup_snapshots(&published);
        self.cleanup_temporary_files();
        self.load()
    }

    /// Resolve an opaque trusted-shell selection back to its cached path hint.
    /// The returned path remains non-authoritative and must go through the
    /// ordinary host-owned inspection path before any use.
    pub fn resolve_path_hint(&self, recent_id: &str) -> Result<Option<PathBuf>, CabinetCacheError> {
        if !lower_hex(recent_id, 32) {
            return Ok(None);
        }
        Ok(self.load_stored()?.and_then(|stored| {
            stored
                .entries
                .into_iter()
                .find(|entry| entry.recent_id == recent_id)
                .map(|entry| PathBuf::from(entry.path_hint))
        }))
    }

    fn prepare_root(&self) -> Result<(), CabinetCacheError> {
        reject_reparse_components(&self.root)?;
        if let Ok(metadata) = fs::symlink_metadata(&self.root)
            && (!metadata.is_dir() || metadata_is_reparse(&metadata))
        {
            return Err(CabinetCacheError::UnsafeRoot);
        }
        prepare_private_directory(&self.root).map_err(|error| {
            CabinetCacheError::Io(io::Error::other(format!(
                "protect Cabinet cache root: {error}"
            )))
        })?;
        reject_reparse_components(&self.root)
    }

    fn load_stored(&self) -> Result<Option<StoredCache>, CabinetCacheError> {
        if !self.root.exists() {
            return Ok(None);
        }
        reject_reparse_components(&self.root)?;
        let metadata = fs::symlink_metadata(&self.root)?;
        if !metadata.is_dir() || metadata_is_reparse(&metadata) {
            return Err(CabinetCacheError::UnsafeRoot);
        }
        prepare_private_directory(&self.root).map_err(|error| {
            CabinetCacheError::Io(io::Error::other(format!(
                "protect Cabinet cache root: {error}"
            )))
        })?;
        reject_reparse_components(&self.root)?;
        let mut candidates = Vec::new();
        for item in fs::read_dir(&self.root)? {
            let item = item?;
            let name = item.file_name();
            if snapshot_name(&name) {
                candidates.push(item.path());
                if candidates.len() > MAX_SNAPSHOT_FILES {
                    return Ok(None);
                }
            }
        }
        let mut newest: Option<StoredCache> = None;
        for path in candidates {
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            if !metadata.is_file()
                || metadata_is_reparse(&metadata)
                || metadata.len() == 0
                || metadata.len() > MAX_CACHE_BYTES
            {
                continue;
            }
            if protect_private_file(&path).is_err() {
                continue;
            }
            let Ok(bytes) = fs::read(&path) else {
                continue;
            };
            let Ok(candidate) = serde_json::from_slice::<StoredCache>(&bytes) else {
                continue;
            };
            if validate_stored(&candidate).is_err() {
                continue;
            }
            if newest
                .as_ref()
                .is_none_or(|stored| candidate.generation > stored.generation)
            {
                newest = Some(candidate);
            }
        }
        Ok(newest)
    }

    fn publish_snapshot(
        &self,
        generation: u64,
        bytes: &[u8],
    ) -> Result<PathBuf, CabinetCacheError> {
        let token = random_token()?;
        let temporary = self.root.join(format!(".{SNAPSHOT_PREFIX}{token}.tmp"));
        let final_path = self.root.join(format!(
            "{SNAPSHOT_PREFIX}{generation:020}-{token}{SNAPSHOT_SUFFIX}"
        ));
        let mut remove_temporary = true;
        let result = (|| -> Result<PathBuf, CabinetCacheError> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            protect_private_file(&temporary).map_err(|error| {
                CabinetCacheError::Io(io::Error::other(format!(
                    "protect Cabinet snapshot: {error}"
                )))
            })?;
            file.write_all(bytes)?;
            file.flush()?;
            file.sync_all()?;
            drop(file);
            // Publish a complete file through a create-new directory entry.
            // Hard-link creation fails if the unique final path already exists
            // on both supported desktop platforms; it never replaces.
            fs::hard_link(&temporary, &final_path)?;
            sync_directory(&self.root)?;
            let _ = fs::remove_file(&temporary);
            remove_temporary = false;
            Ok(final_path.clone())
        })();
        if remove_temporary {
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    fn cleanup_snapshots(&self, published: &Path) {
        let Ok(items) = fs::read_dir(&self.root) else {
            return;
        };
        let mut snapshots: Vec<_> = items
            .filter_map(Result::ok)
            .filter(|item| snapshot_name(&item.file_name()))
            .filter(|item| {
                fs::symlink_metadata(item.path())
                    .is_ok_and(|metadata| metadata.is_file() && !metadata_is_reparse(&metadata))
            })
            .collect();
        snapshots.sort_by_key(|right| std::cmp::Reverse(right.file_name()));
        let mut retained = 1;
        for stale in snapshots {
            if stale.path() == published {
                continue;
            }
            if retained < RETAINED_SNAPSHOTS {
                retained += 1;
                continue;
            }
            let _ = fs::remove_file(stale.path());
        }
    }

    fn cleanup_temporary_files(&self) {
        let Ok(items) = fs::read_dir(&self.root) else {
            return;
        };
        for item in items.filter_map(Result::ok) {
            if !temporary_snapshot_name(&item.file_name()) {
                continue;
            }
            let path = item.path();
            if fs::symlink_metadata(&path)
                .is_ok_and(|metadata| metadata.is_file() && !metadata_is_reparse(&metadata))
            {
                let _ = fs::remove_file(path);
            }
        }
    }
}

fn empty_snapshot() -> CabinetRecentSnapshot {
    CabinetRecentSnapshot {
        profile: CACHE_PROFILE,
        authoritative: false,
        entries: Vec::new(),
    }
}

fn empty_stored() -> StoredCache {
    StoredCache {
        profile: CACHE_PROFILE.to_owned(),
        schema_version: CACHE_SCHEMA_VERSION,
        generation: 0,
        entries: Vec::new(),
    }
}

fn recent_view(entry: CabinetRecentEntry) -> CabinetRecentView {
    CabinetRecentView {
        recent_id: entry.recent_id,
        format_version: entry.format_version,
        application_name: entry.application_name,
        instance_title: entry.instance_title,
        description: entry.description,
        app_id: entry.app_id,
        app_version: entry.app_version,
        last_opened_at: entry.last_opened_at,
        last_observed_badge: entry.last_observed_badge,
    }
}

fn entry_from_observation(
    observation: CabinetObservation,
) -> Result<CabinetRecentEntry, CabinetCacheError> {
    let path_hint = observation
        .path
        .to_str()
        .ok_or(CabinetCacheError::InvalidObservation(
            "path must be valid UTF-8",
        ))?
        .to_owned();
    Ok(CabinetRecentEntry {
        recent_id: random_token()?,
        path_hint,
        path_identity: CachedPathIdentity {
            device: observation.source_identity.device,
            file: observation.source_identity.file,
            stable_file_id: observation.source_identity.stable_file_id,
            bytes: observation.source_identity.bytes,
            modified_ns: observation.source_identity.modified_ns,
        },
        format_version: observation.format_version,
        application_name: observation.application_name,
        instance_title: observation.instance_title,
        description: observation.description,
        app_id: observation.app_id,
        app_version: observation.app_version,
        last_opened_at: observation.last_opened_at,
        last_observed_badge: observation.last_observed_badge,
    })
}

fn validate_observation(observation: &CabinetObservation) -> Result<(), CabinetCacheError> {
    let path = observation
        .path
        .to_str()
        .ok_or(CabinetCacheError::InvalidObservation(
            "path must be valid UTF-8",
        ))?;
    if !bounded(path, 1, 4096) || !ordinary_existing_file(&observation.path) {
        return Err(CabinetCacheError::InvalidObservation(
            "path must name an existing ordinary file",
        ));
    }
    validate_entry(&CabinetRecentEntry {
        recent_id: "0".repeat(32),
        path_hint: path.to_owned(),
        path_identity: CachedPathIdentity {
            device: observation.source_identity.device,
            file: observation.source_identity.file,
            stable_file_id: observation.source_identity.stable_file_id.clone(),
            bytes: observation.source_identity.bytes,
            modified_ns: observation.source_identity.modified_ns,
        },
        format_version: observation.format_version.clone(),
        application_name: observation.application_name.clone(),
        instance_title: observation.instance_title.clone(),
        description: observation.description.clone(),
        app_id: observation.app_id.clone(),
        app_version: observation.app_version.clone(),
        last_opened_at: observation.last_opened_at.clone(),
        last_observed_badge: observation.last_observed_badge,
    })
}

fn validate_stored(stored: &StoredCache) -> Result<(), CabinetCacheError> {
    if stored.profile != CACHE_PROFILE
        || stored.schema_version != CACHE_SCHEMA_VERSION
        || stored.generation == u64::MAX
    {
        return Err(CabinetCacheError::InvalidObservation(
            "unsupported cache schema",
        ));
    }
    if stored.entries.len() > MAX_ENTRIES {
        return Err(CabinetCacheError::InvalidObservation(
            "too many recent entries",
        ));
    }
    let mut recent_ids = BTreeSet::new();
    let mut paths = BTreeSet::new();
    let mut identities = BTreeSet::new();
    for entry in &stored.entries {
        validate_entry(entry)?;
        let identity = if entry.path_identity.stable_file_id.is_empty() {
            format!(
                "{}:inode:{}",
                entry.path_identity.device, entry.path_identity.file
            )
        } else {
            format!(
                "{}:stable:{}",
                entry.path_identity.device, entry.path_identity.stable_file_id
            )
        };
        if !recent_ids.insert(&entry.recent_id)
            || !paths.insert(&entry.path_hint)
            || !identities.insert(identity)
        {
            return Err(CabinetCacheError::InvalidObservation(
                "duplicate recent entry",
            ));
        }
    }
    Ok(())
}

fn validate_entry(entry: &CabinetRecentEntry) -> Result<(), CabinetCacheError> {
    if !lower_hex(&entry.recent_id, 32)
        || !bounded(&entry.path_hint, 1, 4096)
        || !matches!(entry.format_version.as_str(), "0.2" | "0.3")
        || !bounded(&entry.application_name, 1, 512)
        || entry
            .instance_title
            .as_deref()
            .is_some_and(|value| !bounded(value, 1, 512))
        || !bounded(&entry.description, 0, 8192)
        || !bounded(&entry.app_id, 1, 512)
        || !bounded(&entry.app_version, 1, 128)
        || !utc_seconds(&entry.last_opened_at)
        || entry.path_identity.bytes == 0
        || !bounded(&entry.path_identity.stable_file_id, 0, 128)
    {
        return Err(CabinetCacheError::InvalidObservation(
            "recent entry exceeds its bounded display contract",
        ));
    }
    Ok(())
}

fn bounded(value: &str, minimum: usize, maximum_utf8: usize) -> bool {
    value.chars().count() >= minimum && value.len() <= maximum_utf8
}

fn lower_hex(value: &str, exact_bytes: usize) -> bool {
    value.len() == exact_bytes
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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
        || bytes.iter().enumerate().any(|(index, byte)| {
            !matches!(index, 4 | 7 | 10 | 13 | 16 | 19) && !byte.is_ascii_digit()
        })
    {
        return false;
    }
    let number = |start: usize, end: usize| value[start..end].parse::<u32>().unwrap_or(u32::MAX);
    let year = number(0, 4);
    let month = number(5, 7);
    let day = number(8, 10);
    let hour = number(11, 13);
    let minute = number(14, 16);
    let second = number(17, 19);
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let month_days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    year > 0 && (1..=month_days).contains(&day) && hour < 24 && minute < 60 && second < 60
}

fn ordinary_existing_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && !metadata_is_reparse(&metadata))
}

fn reject_reparse_components(path: &Path) -> Result<(), CabinetCacheError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        if !matches!(component, std::path::Component::Normal(_)) {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata_is_reparse(&metadata) => {
                return Err(CabinetCacheError::UnsafeRoot);
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => break,
            Err(error) => return Err(CabinetCacheError::Io(error)),
        }
    }
    Ok(())
}

#[cfg(windows)]
fn metadata_is_reparse(metadata: &fs::Metadata) -> bool {
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn metadata_is_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

fn same_observed_file(left: &CachedPathIdentity, right: &CachedPathIdentity) -> bool {
    left.device == right.device
        && if left.stable_file_id.is_empty() || right.stable_file_id.is_empty() {
            left.file == right.file
        } else {
            left.stable_file_id == right.stable_file_id
        }
}

fn snapshot_name(name: &OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    let Some(body) = name
        .strip_prefix(SNAPSHOT_PREFIX)
        .and_then(|body| body.strip_suffix(SNAPSHOT_SUFFIX))
    else {
        return false;
    };
    let Some((generation, token)) = body.split_once('-') else {
        return false;
    };
    generation.len() == 20
        && generation.bytes().all(|byte| byte.is_ascii_digit())
        && token.len() == 32
        && token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn temporary_snapshot_name(name: &OsStr) -> bool {
    name.to_str().is_some_and(|name| {
        name.starts_with(&format!(".{SNAPSHOT_PREFIX}"))
            && name.ends_with(".tmp")
            && name.len() == 1 + SNAPSHOT_PREFIX.len() + 32 + 4
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.'))
    })
}

fn random_token() -> Result<String, CabinetCacheError> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| CabinetCacheError::Random)?;
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(32);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    Ok(output)
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    static TEST_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "sqlite-capsule-cabinet-test-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir(&path).expect("create test parent");
            Self(path)
        }

        fn source(&self, name: &str) -> PathBuf {
            let path = self.0.join(name);
            fs::write(&path, b"SQLite format 3\0test cache source").expect("write source fixture");
            path
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn observation(path: PathBuf, index: u64) -> CabinetObservation {
        CabinetObservation {
            path,
            source_identity: SourceIdentity {
                device: 7,
                file: index + 10,
                stable_file_id: format!("{index:032x}"),
                bytes: 32,
                modified_ns: index + 100,
            },
            format_version: if index.is_multiple_of(2) {
                "0.3"
            } else {
                "0.2"
            }
            .to_owned(),
            application_name: format!("Application {index}"),
            instance_title: Some(format!("Document {index}")),
            description: "Bounded display metadata".to_owned(),
            app_id: format!("org.example.application-{index}"),
            app_version: "1.0.0".to_owned(),
            last_opened_at: format!("2026-08-12T12:{:02}:00Z", index % 60),
            last_observed_badge: if index.is_multiple_of(2) {
                LastObservedBadge::V03SignatureValid
            } else {
                LastObservedBadge::LegacyV02
            },
        }
    }

    #[test]
    fn record_is_private_bounded_rebuildable_and_non_authoritative() {
        let directory = TestDirectory::new();
        let cache_root = directory.0.join("cabinet-recents");
        let cache = CabinetRecentCache::new(cache_root.clone());
        let source = directory.source("one.sqlitecapsule");

        let snapshot = cache
            .record(observation(source.clone(), 2))
            .expect("record recent observation");

        assert_eq!(snapshot.profile, CACHE_PROFILE);
        assert!(!snapshot.authoritative);
        assert_eq!(snapshot.entries.len(), 1);
        let recent_id = snapshot.entries[0].recent_id.clone();
        assert!(lower_hex(&recent_id, 32));
        assert_eq!(
            cache.resolve_path_hint(&recent_id).unwrap(),
            Some(source.clone())
        );
        assert_eq!(cache.resolve_path_hint("not-an-id").unwrap(), None);
        let public_json = serde_json::to_string(&snapshot).unwrap();
        assert!(!public_json.contains(&source.to_string_lossy().into_owned()));
        assert!(!public_json.contains("path_hint"));
        assert!(!public_json.contains("path_identity"));
        let files: Vec<_> = fs::read_dir(&cache_root)
            .expect("read cache root")
            .map(|item| item.expect("cache item").path())
            .collect();
        assert_eq!(files.len(), 1);
        assert!(snapshot_name(files[0].file_name().expect("snapshot name")));
        assert!(
            files
                .iter()
                .all(|path| path.extension() == Some(OsStr::new("json")))
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&cache_root).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                fs::metadata(&files[0]).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_junction_cache_root_is_rejected_without_touching_target() {
        use std::process::Command;

        let directory = TestDirectory::new();
        let target = directory.0.join("redirect-target");
        let junction = directory.0.join("cabinet-recents");
        fs::create_dir(&target).expect("create junction target");
        let sentinel = target.join("do-not-touch.txt");
        fs::write(&sentinel, b"owner data").expect("write target sentinel");
        let output = Command::new("cmd")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&junction)
            .arg(&target)
            .output()
            .expect("run mklink");
        assert!(
            output.status.success(),
            "create Cabinet test junction: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let cache = CabinetRecentCache::new(junction.clone());
        assert!(matches!(cache.load(), Err(CabinetCacheError::UnsafeRoot)));
        assert_eq!(fs::read(&sentinel).unwrap(), b"owner data");
        assert_eq!(fs::read_dir(&target).unwrap().count(), 1);

        fs::remove_dir(&junction).expect("remove junction without traversing it");
    }

    #[test]
    fn corrupt_oversized_future_and_missing_cache_are_empty() {
        let missing = TestDirectory::new();
        let missing_snapshot = CabinetRecentCache::new(missing.0.join("not-created"))
            .load()
            .expect("missing cache is rebuildable");
        assert!(missing_snapshot.entries.is_empty());
        assert!(!missing_snapshot.authoritative);

        for (name, bytes) in [
            ("corrupt", b"{".to_vec()),
            ("oversized", vec![b'x'; MAX_CACHE_BYTES as usize + 1]),
            (
                "future",
                serde_json::to_vec(&serde_json::json!({
                    "profile": "org.sqlite-capsule.cabinet-recents/2",
                    "schema_version": 2,
                    "generation": 9,
                    "entries": []
                }))
                .unwrap(),
            ),
        ] {
            let directory = TestDirectory::new();
            let root = directory.0.join("cabinet-recents");
            prepare_private_directory(&root).expect("prepare cache root");
            let token = match name {
                "corrupt" => "a".repeat(32),
                "oversized" => "b".repeat(32),
                "future" => "c".repeat(32),
                _ => unreachable!(),
            };
            fs::write(
                root.join(format!(
                    "{SNAPSHOT_PREFIX}00000000000000000001-{token}{SNAPSHOT_SUFFIX}"
                )),
                bytes,
            )
            .expect("write hostile cache");
            let snapshot = CabinetRecentCache::new(root)
                .load()
                .expect("hostile cache remains rebuildable");
            assert!(snapshot.entries.is_empty(), "case {name}");
            assert!(!snapshot.authoritative);
        }

        let directory = TestDirectory::new();
        let root = directory.0.join("cabinet-recents");
        let source = directory.source("deleted.sqlitecapsule");
        let cache = CabinetRecentCache::new(root);
        cache.record(observation(source.clone(), 3)).unwrap();
        fs::remove_file(source).expect("remove recent source");
        let stale_hint = cache.load().unwrap();
        assert_eq!(stale_hint.entries.len(), 1);
        assert!(!stale_hint.authoritative);
    }

    #[test]
    fn exact_entry_ceiling_evicts_oldest_and_updates_are_create_new() {
        let directory = TestDirectory::new();
        let root = directory.0.join("cabinet-recents");
        let cache = CabinetRecentCache::new(root.clone());
        for index in 0..=(MAX_ENTRIES as u64) {
            let source = directory.source(&format!("source-{index}.sqlitecapsule"));
            cache.record(observation(source, index)).unwrap();
        }
        let snapshot = cache.load().unwrap();
        assert_eq!(snapshot.entries.len(), MAX_ENTRIES);
        assert!(
            snapshot
                .entries
                .iter()
                .all(|entry| entry.application_name != "Application 0")
        );
        let files: Vec<_> = fs::read_dir(root)
            .unwrap()
            .map(|item| item.unwrap().path())
            .collect();
        assert_eq!(files.len(), RETAINED_SNAPSHOTS);
        assert!(
            files
                .iter()
                .all(|path| snapshot_name(path.file_name().unwrap()))
        );
        assert!(
            files
                .iter()
                .all(|path| fs::metadata(path).unwrap().len() > 0)
        );
    }

    #[test]
    fn too_many_snapshots_and_crash_temps_rebuild_to_one_valid_generation() {
        let directory = TestDirectory::new();
        let root = directory.0.join("cabinet-recents");
        prepare_private_directory(&root).unwrap();
        for index in 0..=MAX_SNAPSHOT_FILES {
            fs::write(
                root.join(format!(
                    "{SNAPSHOT_PREFIX}{index:020}-{:032x}{SNAPSHOT_SUFFIX}",
                    index + 100
                )),
                b"corrupt",
            )
            .unwrap();
        }
        let crash_temp = root.join(format!(".{SNAPSHOT_PREFIX}{}.tmp", "a".repeat(32)));
        fs::write(&crash_temp, b"partial").unwrap();
        let cache = CabinetRecentCache::new(root.clone());
        assert!(cache.load().unwrap().entries.is_empty());

        let source = directory.source("rebuilt.sqlitecapsule");
        let snapshot = cache.record(observation(source, 12)).unwrap();

        assert_eq!(snapshot.entries.len(), 1);
        assert!(!crash_temp.exists());
        let files: Vec<_> = fs::read_dir(root)
            .unwrap()
            .map(|item| item.unwrap().path())
            .collect();
        assert_eq!(files.len(), RETAINED_SNAPSHOTS);
        assert!(
            files
                .iter()
                .all(|path| snapshot_name(path.file_name().unwrap()))
        );
    }

    #[test]
    fn unsafe_root_and_unbounded_observations_fail_without_touching_trust_store() {
        let directory = TestDirectory::new();
        let trust_store = directory.0.join("trust-v1.sqlite");
        fs::write(&trust_store, b"protected trust bytes").unwrap();
        let root = directory.0.join("cabinet-recents");
        fs::write(&root, b"not a directory").unwrap();
        let cache = CabinetRecentCache::new(root);
        let source = directory.source("source.sqlitecapsule");
        assert!(matches!(
            cache.record(observation(source, 4)),
            Err(CabinetCacheError::UnsafeRoot)
        ));
        assert_eq!(fs::read(&trust_store).unwrap(), b"protected trust bytes");

        let valid_root = directory.0.join("bounded-recents");
        let cache = CabinetRecentCache::new(valid_root.clone());
        let source = directory.source("bounded.sqlitecapsule");
        let mut hostile = observation(source, 5);
        hostile.application_name = "x".repeat(513);
        assert!(matches!(
            cache.record(hostile),
            Err(CabinetCacheError::InvalidObservation(_))
        ));
        assert!(!valid_root.exists());
        assert_eq!(fs::read(trust_store).unwrap(), b"protected trust bytes");
    }
}

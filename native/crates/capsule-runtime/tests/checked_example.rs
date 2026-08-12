use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

use rusqlite::Connection;
use serde_json::{Map, Value, json};
use sqlite_capsule_launch::{LaunchInspection, inspect_launch};
use sqlite_capsule_policy::{CapabilityDecision, EvaluationContext, LaunchDecision, TrustStore};
use sqlite_capsule_runtime::{
    BackupRecord, RuntimeError, VerifiedCapsule, inspect_backup_inventory,
    inspect_launch_with_recovery, restore_verified_backup,
};

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let suffix = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "sqlite-capsule-runtime-test-{}-{suffix}",
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

fn checked_capsule() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("capsules/diagram-studio.capsule.sqlite")
}

fn authorise(
    directory: &TestDirectory,
    inspection: &LaunchInspection,
) -> (TrustStore, LaunchDecision) {
    let mut store = TrustStore::open(&directory.0.join("trust/trust.sqlite"))
        .expect("open protected test store");
    let capabilities = inspection.evidence.requested_capabilities.clone();
    let context = EvaluationContext {
        host_policy: capabilities
            .iter()
            .map(|capability| (capability.clone(), CapabilityDecision::Allow))
            .collect::<BTreeMap<_, _>>(),
        operating_system_permission: capabilities
            .iter()
            .map(|capability| (capability.clone(), CapabilityDecision::Allow))
            .collect::<BTreeMap<_, _>>(),
        allow_once: capabilities,
        trust_once: true,
    };
    let decision = store
        .evaluate(&inspection.evidence, &context)
        .expect("allow-once evaluation");
    assert!(decision.executable_allowed);
    (store, decision)
}

fn arguments(values: Value) -> Map<String, Value> {
    values.as_object().expect("object arguments").clone()
}

fn writable_capsule(directory: &TestDirectory, name: &str) -> PathBuf {
    let source = directory.0.join("source");
    fs::create_dir(&source).expect("create source directory");
    let capsule = source.join(name);
    fs::copy(checked_capsule(), &capsule).expect("copy capsule");
    capsule
}

fn writer_lifecycle_roots(directory: &TestDirectory) -> (PathBuf, PathBuf) {
    (
        directory.0.join("host/writer-locks"),
        directory.0.join("host/backups"),
    )
}

#[test]
fn rollback_journal_crash_worker() {
    let Some(path) = std::env::var_os("SQLITE_CAPSULE_CRASH_WORKER_PATH") else {
        return;
    };
    let connection = Connection::open(PathBuf::from(path)).expect("open crash fixture");
    connection
        .execute_batch(
            "PRAGMA journal_mode=DELETE; \
             PRAGMA synchronous=FULL; \
             PRAGMA cache_size=1; \
             BEGIN IMMEDIATE;",
        )
        .expect("begin crash transaction");
    let crash_value = format!("CRASH-PROBE-{}", "x".repeat(2_000_000));
    connection
        .execute(
            "UPDATE diagram_document SET description = ?1 WHERE id = 'diagram-main'",
            [&crash_value],
        )
        .expect("spill uncommitted pages");
    std::mem::forget(connection);
    std::process::exit(97);
}

#[test]
fn hot_rollback_journal_is_recovered_then_fully_reverified_before_runtime_open() {
    let directory = TestDirectory::new();
    let capsule = writable_capsule(&directory, "hot-journal.sqlitecapsule");
    let (lock_root, _backup_root) = writer_lifecycle_roots(&directory);
    let status = Command::new(std::env::current_exe().expect("current test executable"))
        .arg("--exact")
        .arg("rollback_journal_crash_worker")
        .arg("--nocapture")
        .env("SQLITE_CAPSULE_CRASH_WORKER_PATH", &capsule)
        .status()
        .expect("run abrupt crash worker");
    assert_eq!(status.code(), Some(97));

    let mut journal_name = OsString::from(capsule.as_os_str());
    journal_name.push("-journal");
    let journal = PathBuf::from(journal_name);
    assert!(
        fs::metadata(&journal)
            .expect("rollback journal left by worker")
            .len()
            > 512
    );
    assert!(
        inspect_launch(&capsule).is_err(),
        "ordinary read-only inspection cannot silently clean a hot journal"
    );

    let (inspection, recovery) =
        inspect_launch_with_recovery(&capsule, &lock_root).expect("SQLite recovery and inspection");
    let recovery = recovery.expect("recovery evidence");
    assert!(recovery.sqlite_recovery_attempted);
    assert!(recovery.rollback_journal_hot_candidate_before);
    assert!(!recovery.rollback_journal_present_after);
    assert_ne!(recovery.source_sha256_before, recovery.source_sha256_after);
    assert!(!journal.exists());

    let (_store, decision) = authorise(&directory, &inspection);
    let mut runtime = VerifiedCapsule::open(&capsule, &inspection, &decision, false, None, None)
        .expect("verified runtime after recovery");
    let document = runtime
        .read_endpoint(
            "diagram.get",
            &arguments(json!({"diagram_id": "diagram-main"})),
        )
        .expect("recovered domain state");
    assert_eq!(
        document["description"],
        "Create nodes and connectors, organise them in layers, and turn the same diagram into a sequence of presentation scenes."
    );
    drop(runtime);

    let direct = Connection::open(&capsule).expect("direct recovery evidence");
    assert_eq!(
        direct
            .query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
            .expect("integrity evidence"),
        "ok"
    );
    assert_eq!(
        direct
            .query_row("SELECT count(*) FROM capsule_change_log", [], |row| row
                .get::<_, i64>(0))
            .expect("change-log evidence"),
        0
    );
}

#[test]
fn verified_read_only_runtime_returns_assets_manifest_and_named_rows() {
    let directory = TestDirectory::new();
    let capsule = checked_capsule();
    let inspection = inspect_launch(&capsule).expect("launch evidence");
    let (_store, decision) = authorise(&directory, &inspection);
    let mut runtime = VerifiedCapsule::open(&capsule, &inspection, &decision, false, None, None)
        .expect("verified runtime");
    assert_eq!(runtime.verification().check_results.len(), 18);
    assert!(
        runtime
            .verification()
            .check_results
            .iter()
            .all(|check| check.passed)
    );
    let manifest = runtime.manifest();
    assert_eq!(manifest.app_id, "org.sqlite-capsule.diagram-studio");
    assert!(!manifest.effective_permissions.is_null());
    assert_eq!(
        manifest.effective_permissions["database.write"]["decision"],
        "deny"
    );
    let entry = runtime.entry_asset().expect("verified entry asset");
    assert_eq!(entry.path, "app/index.html");
    assert_eq!(entry.media_type, "text/html; charset=utf-8");
    assert!(entry.executable);
    assert!(entry.content.starts_with(b"<!doctype html>"));
    let document = runtime
        .read_endpoint(
            "diagram.get",
            &arguments(json!({"diagram_id": "diagram-main"})),
        )
        .expect("named read");
    assert_eq!(document["id"], "diagram-main");
    assert_eq!(
        document["title"],
        "Design and present architecture diagrams"
    );
    assert!(
        runtime
            .write_endpoint("diagram.rename", &Map::new())
            .is_err(),
        "read-only runtime must not expose writes"
    );
}

#[test]
fn compound_named_write_is_atomic_and_records_one_change_log_row() {
    let directory = TestDirectory::new();
    let capsule = writable_capsule(&directory, "writable.capsule.sqlite");
    let inspection = inspect_launch(&capsule).expect("launch evidence");
    let (_store, decision) = authorise(&directory, &inspection);
    let (lock_root, backup_root) = writer_lifecycle_roots(&directory);
    let mut runtime = VerifiedCapsule::open(
        &capsule,
        &inspection,
        &decision,
        true,
        Some(&lock_root),
        Some(&backup_root),
    )
    .expect("writable runtime");
    assert!(runtime.backup_record().is_none());
    let old_title = "Design and present architecture diagrams";
    let write = runtime
        .write_endpoint(
            "diagram.rename",
            &arguments(json!({
                "operation_id": "operation-native-runtime-test",
                "expected_cursor": 0,
                "diagram_id": "diagram-main",
                "from_title": old_title,
                "to_title": "Native runtime transaction proof"
            })),
        )
        .expect("compound write");
    assert_eq!(write["changes"], 3);
    assert_eq!(write["step_changes"], json!([0, 1, 1, 1]));
    let backup_record = runtime
        .backup_record()
        .expect("verified pre-write backup")
        .clone();
    let backup_path = backup_root.join(&backup_record.backup_id);
    assert!(
        !backup_root
            .join(format!("{}.in-progress", backup_record.backup_id))
            .exists()
    );
    assert_eq!(
        fs::metadata(&backup_path).expect("backup metadata").len(),
        backup_record.bytes
    );
    let stored_record: BackupRecord = serde_json::from_slice(
        &fs::read(backup_root.join(format!("{}.json", backup_record.backup_id)))
            .expect("backup manifest"),
    )
    .expect("backup record JSON");
    assert_eq!(stored_record, backup_record);
    let backup_connection = Connection::open(&backup_path).expect("open backup evidence");
    let backup_title: String = backup_connection
        .query_row(
            "SELECT title FROM diagram_document WHERE id = 'diagram-main'",
            [],
            |row| row.get(0),
        )
        .expect("backup title");
    let backup_log_count: i64 = backup_connection
        .query_row("SELECT count(*) FROM capsule_change_log", [], |row| {
            row.get(0)
        })
        .expect("backup log count");
    assert_eq!(backup_title, old_title);
    assert_eq!(backup_log_count, 0);
    assert_eq!(
        runtime
            .read_endpoint(
                "diagram.get",
                &arguments(json!({"diagram_id": "diagram-main"}))
            )
            .expect("read renamed document")["title"],
        "Native runtime transaction proof"
    );
    let checkpoint = runtime
        .checkpoint_if_dirty()
        .expect("verified checkpoint")
        .expect("dirty runtime creates a checkpoint");
    assert_ne!(checkpoint.backup_id, backup_record.backup_id);
    assert_ne!(checkpoint.source_sha256, backup_record.source_sha256);
    let checkpoint_connection =
        Connection::open(backup_root.join(&checkpoint.backup_id)).expect("open checkpoint");
    assert_eq!(
        checkpoint_connection
            .query_row(
                "SELECT title FROM diagram_document WHERE id = 'diagram-main'",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("checkpoint title"),
        "Native runtime transaction proof"
    );
    assert_eq!(
        checkpoint_connection
            .query_row("SELECT count(*) FROM capsule_change_log", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("checkpoint log count"),
        1
    );
    assert!(
        runtime
            .checkpoint_if_dirty()
            .expect("clean checkpoint request")
            .is_none()
    );
    let inventory = inspect_backup_inventory(&backup_root).expect("backup inventory");
    assert_eq!(inventory.verified.len(), 2);
    assert!(inventory.incomplete_artifacts.is_empty());
    assert!(inventory.invalid_artifacts.is_empty());

    let interrupted_id = "99999999999999999999-test.capsule-backup.sqlite";
    fs::write(backup_root.join(interrupted_id), b"partial").expect("partial backup");
    fs::write(
        backup_root.join(format!("{interrupted_id}.in-progress")),
        b"{}",
    )
    .expect("interrupted marker");
    let inventory = inspect_backup_inventory(&backup_root).expect("interrupted inventory");
    assert_eq!(inventory.incomplete_artifacts, [interrupted_id.to_owned()]);
    drop(runtime);

    let connection = Connection::open(&capsule).expect("direct SQLite evidence");
    assert_eq!(
        connection
            .query_row(
                "SELECT title FROM diagram_document WHERE id = 'diagram-main'",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("title"),
        "Native runtime transaction proof"
    );
    let (endpoint, changed_rows, log_count): (String, i64, i64) = connection
        .query_row(
            "SELECT endpoint_name, changed_rows, \
                    (SELECT count(*) FROM capsule_change_log WHERE endpoint_name = 'diagram.rename') \
             FROM capsule_change_log ORDER BY id DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("change log");
    assert_eq!(endpoint, "diagram.rename");
    assert_eq!(changed_rows, 3);
    assert_eq!(log_count, 1);
}

#[test]
fn native_runtime_keeps_declared_foreign_key_cascades_reversible() {
    let directory = TestDirectory::new();
    let capsule = writable_capsule(&directory, "cascade-delete.capsule.sqlite");
    let inspection = inspect_launch(&capsule).expect("launch evidence");
    let (_store, decision) = authorise(&directory, &inspection);
    let (lock_root, backup_root) = writer_lifecycle_roots(&directory);
    let mut runtime = VerifiedCapsule::open(
        &capsule,
        &inspection,
        &decision,
        true,
        Some(&lock_root),
        Some(&backup_root),
    )
    .expect("writable runtime");

    let operation_id = "operation-native-cascade-delete";
    runtime
        .write_endpoint(
            "nodes.delete",
            &arguments(json!({
                "operation_id": operation_id,
                "expected_cursor": 0,
                "diagram_id": "diagram-main",
                "node_ids_json": ["node-trusted-host"]
            })),
        )
        .expect("node delete with declared foreign-key cascades");
    let nodes = runtime
        .read_endpoint(
            "diagram.nodes",
            &arguments(json!({"diagram_id": "diagram-main"})),
        )
        .expect("nodes after delete");
    assert!(
        nodes
            .as_array()
            .expect("node rows")
            .iter()
            .all(|node| node["id"] != "node-trusted-host")
    );

    runtime
        .write_endpoint(
            "nodes.delete.undo",
            &arguments(json!({
                "operation_id": operation_id,
                "expected_cursor": 1,
                "diagram_id": "diagram-main"
            })),
        )
        .expect("undo cascaded node delete");
    let nodes = runtime
        .read_endpoint(
            "diagram.nodes",
            &arguments(json!({"diagram_id": "diagram-main"})),
        )
        .expect("nodes after undo");
    assert!(
        nodes
            .as_array()
            .expect("node rows")
            .iter()
            .any(|node| node["id"] == "node-trusted-host")
    );
    assert_eq!(
        runtime
            .read_endpoint(
                "diagram.history",
                &arguments(json!({"diagram_id": "diagram-main"}))
            )
            .expect("history after undo")["cursor"],
        0
    );
}

#[test]
fn host_update_preflight_establishes_a_current_verified_backup_before_quiescence() {
    let directory = TestDirectory::new();
    let capsule = writable_capsule(&directory, "host-update.capsule.sqlite");
    let inspection = inspect_launch(&capsule).expect("launch evidence");
    let (_store, decision) = authorise(&directory, &inspection);
    let (lock_root, backup_root) = writer_lifecycle_roots(&directory);
    let mut runtime = VerifiedCapsule::open(
        &capsule,
        &inspection,
        &decision,
        true,
        Some(&lock_root),
        Some(&backup_root),
    )
    .expect("writable runtime");

    let initial = runtime
        .prepare_for_host_update()
        .expect("initial update preflight")
        .expect("writable update preflight must establish a recovery point");
    assert_eq!(
        inspect_backup_inventory(&backup_root)
            .unwrap()
            .verified
            .len(),
        1
    );

    runtime
        .write_endpoint(
            "diagram.rename",
            &arguments(json!({
                "operation_id": "operation-host-update-preflight",
                "expected_cursor": 0,
                "diagram_id": "diagram-main",
                "from_title": "Design and present architecture diagrams",
                "to_title": "Preserved before host replacement"
            })),
        )
        .expect("write before update");
    let current = runtime
        .prepare_for_host_update()
        .expect("dirty update preflight")
        .expect("dirty writable session must create a current recovery point");
    assert_ne!(current.backup_id, initial.backup_id);
    assert_ne!(current.source_sha256, initial.source_sha256);
    assert_eq!(
        inspect_backup_inventory(&backup_root)
            .unwrap()
            .verified
            .len(),
        2
    );

    let reused = runtime
        .prepare_for_host_update()
        .expect("clean update preflight")
        .expect("clean writable session keeps its current recovery point");
    assert_eq!(reused.backup_id, current.backup_id);
    drop(runtime);

    let current_inspection = inspect_launch(&capsule).expect("current launch evidence");
    let (_current_store, current_decision) = authorise(&directory, &current_inspection);
    let read_only = VerifiedCapsule::open(
        &capsule,
        &current_inspection,
        &current_decision,
        false,
        None,
        None,
    )
    .expect("read-only runtime");
    let mut read_only = read_only;
    assert!(
        read_only
            .prepare_for_host_update()
            .expect("read-only update preflight")
            .is_none()
    );
}

#[test]
fn failed_compound_precondition_rolls_back_every_step_and_log() {
    let directory = TestDirectory::new();
    let capsule = writable_capsule(&directory, "rollback.capsule.sqlite");
    let inspection = inspect_launch(&capsule).expect("launch evidence");
    let (_store, decision) = authorise(&directory, &inspection);
    let (lock_root, backup_root) = writer_lifecycle_roots(&directory);
    let mut runtime = VerifiedCapsule::open(
        &capsule,
        &inspection,
        &decision,
        true,
        Some(&lock_root),
        Some(&backup_root),
    )
    .expect("writable runtime");
    assert!(
        runtime
            .write_endpoint(
                "diagram.rename",
                &arguments(json!({
                    "operation_id": "operation-must-rollback",
                    "expected_cursor": 0,
                    "diagram_id": "diagram-main",
                    "from_title": "Incorrect precondition",
                    "to_title": "Must not persist"
                })),
            )
            .is_err()
    );
    assert!(runtime.backup_record().is_some());
    drop(runtime);
    let connection = Connection::open(&capsule).expect("direct SQLite evidence");
    let title: String = connection
        .query_row(
            "SELECT title FROM diagram_document WHERE id = 'diagram-main'",
            [],
            |row| row.get(0),
        )
        .expect("title");
    assert_eq!(title, "Design and present architecture diagrams");
    let operation_count: i64 = connection
        .query_row(
            "SELECT count(*) FROM diagram_operation WHERE id = 'operation-must-rollback'",
            [],
            |row| row.get(0),
        )
        .expect("operation count");
    let log_count: i64 = connection
        .query_row(
            "SELECT count(*) FROM capsule_change_log WHERE endpoint_name = 'diagram.rename'",
            [],
            |row| row.get(0),
        )
        .expect("log count");
    assert_eq!(operation_count, 0);
    assert_eq!(log_count, 0);
}

#[test]
fn a_second_native_writer_is_denied_until_the_first_session_closes() {
    let directory = TestDirectory::new();
    let capsule = writable_capsule(&directory, "one-writer.capsule.sqlite");
    let inspection = inspect_launch(&capsule).expect("launch evidence");
    let (_store, decision) = authorise(&directory, &inspection);
    let (lock_root, backup_root) = writer_lifecycle_roots(&directory);
    let first = VerifiedCapsule::open(
        &capsule,
        &inspection,
        &decision,
        true,
        Some(&lock_root),
        Some(&backup_root),
    )
    .expect("first writer");
    assert!(matches!(
        VerifiedCapsule::open(
            &capsule,
            &inspection,
            &decision,
            true,
            Some(&lock_root),
            Some(&backup_root)
        ),
        Err(RuntimeError::Lifecycle(
            sqlite_capsule_lifecycle::LifecycleError::WriterBusy
        ))
    ));
    drop(first);
    VerifiedCapsule::open(
        &capsule,
        &inspection,
        &decision,
        true,
        Some(&lock_root),
        Some(&backup_root),
    )
    .expect("writer after close");
}

#[test]
fn an_external_sqlite_commit_stops_the_session_before_another_host_write() {
    let directory = TestDirectory::new();
    let capsule = writable_capsule(&directory, "external-conflict.capsule.sqlite");
    let inspection = inspect_launch(&capsule).expect("launch evidence");
    let (_store, decision) = authorise(&directory, &inspection);
    let (lock_root, backup_root) = writer_lifecycle_roots(&directory);
    let mut runtime = VerifiedCapsule::open(
        &capsule,
        &inspection,
        &decision,
        true,
        Some(&lock_root),
        Some(&backup_root),
    )
    .expect("writable runtime");

    let external = Connection::open(&capsule).expect("external connection");
    external
        .execute(
            "UPDATE diagram_document SET title = title || ' external' WHERE id = 'diagram-main'",
            [],
        )
        .expect("external commit");
    drop(external);

    assert!(matches!(
        runtime.write_endpoint(
            "diagram.rename",
            &arguments(json!({
                "operation_id": "operation-after-conflict",
                "expected_cursor": 0,
                "diagram_id": "diagram-main",
                "from_title": "Design and present architecture diagrams",
                "to_title": "Must not commit"
            }))
        ),
        Err(RuntimeError::SourceConflict)
    ));
    drop(runtime);

    let connection = Connection::open(&capsule).expect("direct evidence");
    let count: i64 = connection
        .query_row(
            "SELECT count(*) FROM diagram_operation WHERE id = 'operation-after-conflict'",
            [],
            |row| row.get(0),
        )
        .expect("operation count");
    assert_eq!(count, 0);
}

#[test]
fn a_verified_backup_restores_only_to_a_new_path_and_detects_tampering() {
    let directory = TestDirectory::new();
    let capsule = writable_capsule(&directory, "restore-source.capsule.sqlite");
    let inspection = inspect_launch(&capsule).expect("launch evidence");
    let (_store, decision) = authorise(&directory, &inspection);
    let (lock_root, backup_root) = writer_lifecycle_roots(&directory);
    let mut runtime = VerifiedCapsule::open(
        &capsule,
        &inspection,
        &decision,
        true,
        Some(&lock_root),
        Some(&backup_root),
    )
    .expect("writable runtime");
    runtime
        .write_endpoint(
            "diagram.rename",
            &arguments(json!({
                "operation_id": "operation-before-restore",
                "expected_cursor": 0,
                "diagram_id": "diagram-main",
                "from_title": "Design and present architecture diagrams",
                "to_title": "Source changed after backup"
            })),
        )
        .expect("write after backup");
    let record = runtime.backup_record().expect("backup record").clone();
    drop(runtime);

    let restore_parent = directory.0.join("restored");
    fs::create_dir(&restore_parent).expect("restore parent");
    let restored_path = restore_parent.join("recovered.sqlitecapsule");
    let restored = restore_verified_backup(&backup_root, &record.backup_id, &restored_path)
        .expect("verified restore");
    let mut restore_marker_name = OsString::from(restored_path.as_os_str());
    restore_marker_name.push(".capsule-restore-in-progress");
    assert!(!PathBuf::from(restore_marker_name).exists());
    assert_eq!(restored.capsule_id, inspection.identity.capsule_id);
    let restored_connection = Connection::open(&restored_path).expect("restored database");
    let restored_title: String = restored_connection
        .query_row(
            "SELECT title FROM diagram_document WHERE id = 'diagram-main'",
            [],
            |row| row.get(0),
        )
        .expect("restored title");
    assert_eq!(restored_title, "Design and present architecture diagrams");
    assert!(restore_verified_backup(&backup_root, &record.backup_id, &restored_path).is_err());

    let tampered_backup = backup_root.join(&record.backup_id);
    let mut bytes = fs::read(&tampered_backup).expect("backup bytes");
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    fs::write(&tampered_backup, bytes).expect("tamper disposable backup");
    let rejected_output = restore_parent.join("must-not-exist.sqlitecapsule");
    assert!(restore_verified_backup(&backup_root, &record.backup_id, &rejected_output).is_err());
    assert!(!rejected_output.exists());

    let interrupted_output = restore_parent.join("interrupted.sqlitecapsule");
    fs::copy(checked_capsule(), &interrupted_output).expect("interrupted output bytes");
    let mut marker_name = OsString::from(interrupted_output.as_os_str());
    marker_name.push(".capsule-restore-in-progress");
    fs::write(PathBuf::from(marker_name), b"{}").expect("interrupted restore marker");
    assert!(matches!(
        inspect_launch_with_recovery(&interrupted_output, &lock_root),
        Err(RuntimeError::Recovery(_))
    ));
}

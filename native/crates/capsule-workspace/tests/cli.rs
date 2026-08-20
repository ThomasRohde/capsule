use ed25519_dalek::SigningKey;
use rusqlite::Connection;
use std::fs;
use std::process::Command;

use sqlite_capsule_crypto::{PROFILE_V03, application_digest, sign_digest_for_profile};
use sqlite_capsule_workspace::{LifecyclePlan, VerifiedCopySource, VerifiedWorkspaceSource};

const V03_SCHEMA: &str = include_str!("../../../../format/capsule-v0.3.sql");
const SIGNED_SCHEMA: &str = include_str!("../../../../format/capsule-signed-app-v0.3.sql");
const SIGNED_FIXTURE: &str =
    include_str!("../../../../compatibility/signed-app-v0.3/fixture-v0.3.sql");
const DEVELOPMENT_SEED: &str =
    include_str!("../../../../compatibility/signed-app-v0.2/development-seed.hex");

fn signed_fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("cli-source.sqlitecapsule");
    let connection = Connection::open(&path).expect("create CLI fixture");
    connection.execute_batch(V03_SCHEMA).expect("v0.3 schema");
    connection
        .execute_batch(SIGNED_SCHEMA)
        .expect("signed schema");
    connection
        .execute_batch(SIGNED_FIXTURE)
        .expect("signed fixture");
    connection
        .execute("DELETE FROM capsule_signature", [])
        .expect("remove vector signature");
    let digest = application_digest(&connection).expect("application digest");
    let seed_text = DEVELOPMENT_SEED.trim();
    let mut seed = [0_u8; 32];
    for (index, byte) in seed.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&seed_text[index * 2..index * 2 + 2], 16)
            .expect("development seed hex");
    }
    let key = SigningKey::from_bytes(&seed);
    seed.fill(0);
    let envelope = sign_digest_for_profile(&key, digest, "2026-08-08T12:34:56Z", PROFILE_V03)
        .expect("sign CLI fixture");
    connection
        .execute(
            "INSERT INTO capsule_signature \
             (key_id, algorithm, public_key, application_digest, signature, signed_at) \
             VALUES (?1, 'ed25519', ?2, ?3, ?4, ?5)",
            rusqlite::params![
                envelope.key_id,
                envelope.public_key.as_slice(),
                envelope.application_digest.as_slice(),
                envelope.signature.as_slice(),
                envelope.signed_at,
            ],
        )
        .expect("store CLI fixture signature");
    drop(connection);
    (directory, path)
}

#[test]
fn cli_rejects_unknown_commands_with_only_the_safe_error_envelope() {
    let output = Command::new(env!("CARGO_BIN_EXE_capsule-workspace"))
        .arg("unknown-command")
        .output()
        .expect("run capsule-workspace CLI");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let value: serde_json::Value =
        serde_json::from_slice(&output.stderr).expect("safe JSON error envelope");
    assert_eq!(value["profile"], "org.sqlite-capsule.lifecycle-errors/1");
    assert_eq!(value["code"], "invalid_contract");
    assert_eq!(value["category"], "input");
    assert_eq!(value["retryable"], false);
    assert!(value["safe_detail"].is_string());
    assert_eq!(value.as_object().expect("error object").len(), 5);
}

#[test]
fn cli_plan_duplicate_returns_repeatable_canonical_review_json_without_writing() {
    let (directory, source) = signed_fixture();
    let output = directory.path().join("CLI Copy.capsule.sqlite");
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_capsule-workspace"))
            .args([
                "plan-duplicate",
                source.to_str().expect("source path"),
                output.to_str().expect("output path"),
                "c61f92e6-20fd-465c-b16f-b923900aba76",
                "2026-08-12T12:00:00Z",
                "2026-08-12T12:05:00Z",
            ])
            .output()
            .expect("run duplicate planner")
    };
    let first = run();
    let second = run();
    assert!(first.status.success(), "first stderr: {:?}", first.stderr);
    assert!(
        second.status.success(),
        "second stderr: {:?}",
        second.stderr
    );
    assert!(first.stderr.is_empty());
    assert!(second.stderr.is_empty());
    assert_eq!(first.stdout, second.stdout);
    let plan = LifecyclePlan::parse(&first.stdout).expect("CLI emits a valid lifecycle plan");
    assert_eq!(
        plan.canonical_bytes().expect("canonical CLI plan"),
        first.stdout
    );
    assert!(!output.exists(), "CLI planning must not create its output");
}

#[test]
fn cli_compare_emits_bounded_profiled_json_without_values_or_source_writes() {
    let (_left_directory, left) = signed_fixture();
    let (_right_directory, right) = signed_fixture();
    let connection = Connection::open(&right).expect("open right compare fixture");
    connection
        .execute(
            "UPDATE vector_domain SET note='cli-private-difference' WHERE id='domain'",
            [],
        )
        .expect("mutate unsigned domain row");
    drop(connection);
    let left_before = fs::read(&left).expect("left bytes before compare");
    let right_before = fs::read(&right).expect("right bytes before compare");

    let run = || {
        Command::new(env!("CARGO_BIN_EXE_capsule-workspace"))
            .args([
                "compare",
                left.to_str().expect("left path"),
                right.to_str().expect("right path"),
            ])
            .output()
            .expect("run compare command")
    };
    let output = run();
    let repeated = run();
    assert!(
        output.status.success(),
        "compare stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert!(repeated.status.success());
    assert!(repeated.stderr.is_empty());
    assert_eq!(
        output.stdout, repeated.stdout,
        "fixed limits and inputs must produce byte-identical CLI JSON"
    );
    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("compare report JSON");
    assert_eq!(report["profile"], "org.sqlite-capsule.compare-report/1");
    assert_eq!(report["compatibility"]["can_compare_data"], true);
    assert_eq!(report["limits"]["deadline_ms"], 30_000);
    assert_eq!(report["datasets"][0]["counts"]["changed"], 1);
    assert!(
        report["report_digest"]
            .as_str()
            .is_some_and(|value| value.len() == 64)
    );
    let emitted = String::from_utf8(output.stdout).expect("UTF-8 compare JSON");
    assert!(!emitted.contains("cli-private-difference"));
    assert!(!emitted.contains(left.to_string_lossy().as_ref()));
    assert!(!emitted.contains(right.to_string_lossy().as_ref()));
    assert_eq!(
        fs::read(&left).expect("left bytes after compare"),
        left_before
    );
    assert_eq!(
        fs::read(&right).expect("right bytes after compare"),
        right_before
    );
}

#[test]
fn cli_executes_exact_compact_and_fork_through_workspace_typestates() {
    let (directory, source) = signed_fixture();
    let source_bytes = fs::read(&source).expect("source bytes");
    let source_copy = VerifiedCopySource::open(&source).expect("source copy identity");
    let source_capsule_id = source_copy.identity().capsule_id.clone();
    let source_application_digest = source_copy.identity().application_digest.clone();
    drop(source_copy);

    let cases = [
        ("copy-exact", "exact.sqlitecapsule"),
        ("copy-compact", "compact.sqlitecapsule"),
        ("copy-fork", "fork.sqlitecapsule"),
    ];
    for (index, (command, leaf)) in cases.into_iter().enumerate() {
        let output_path = directory.path().join(leaf);
        let plan_id = format!("00000000-0000-4000-8000-{index:012}");
        let output = Command::new(env!("CARGO_BIN_EXE_capsule-workspace"))
            .args([
                command,
                source.to_str().expect("source path"),
                output_path.to_str().expect("output path"),
                &plan_id,
                "2020-01-01T00:00:00Z",
                "2099-01-01T00:00:00Z",
            ])
            .output()
            .expect("run copy command");
        assert!(
            output.status.success(),
            "{command} stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let response: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("copy result JSON");
        assert_eq!(
            response["profile"],
            "org.sqlite-capsule.workspace-copy-result/1"
        );
        assert_eq!(response["output_leaf"], leaf);
        assert!(
            response["output_bytes"]
                .as_u64()
                .is_some_and(|value| value > 0)
        );
        VerifiedCopySource::open(&output_path).expect("published output verifies");

        if command == "copy-exact" {
            assert_eq!(fs::read(&output_path).expect("exact bytes"), source_bytes);
        }
        if command == "copy-fork" {
            let fork = VerifiedWorkspaceSource::open(&output_path).expect("fork workspace");
            assert_ne!(fork.identity().capsule_id, source_capsule_id);
            let fork_copy = VerifiedCopySource::open(&output_path).expect("fork copy identity");
            assert_eq!(
                fork_copy.identity().application_digest,
                source_application_digest
            );
        }
        assert_eq!(fs::read(&source).expect("source after copy"), source_bytes);
    }
}

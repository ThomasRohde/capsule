use std::path::PathBuf;

use sqlite_capsule_core::inspect_metadata;

#[test]
fn checked_example_has_expected_native_identity() {
    let capsule = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../capsules/diagram-studio.capsule.sqlite");
    let identity = inspect_metadata(capsule).expect("inspect checked example");

    assert_eq!(
        identity.capsule_id,
        "urn:uuid:4f5e31aa-19ce-4b49-bd5e-256d611201f4"
    );
    assert_eq!(identity.format_version, "0.2");
    assert_eq!(identity.runtime_protocol, "capsule-http/0.2");
    assert_eq!(identity.app_id, "org.sqlite-capsule.diagram-studio");
}

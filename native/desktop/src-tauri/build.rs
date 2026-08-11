fn main() {
    println!("cargo:rerun-if-env-changed=SQLITE_CAPSULE_UPDATER_ENDPOINT");
    println!("cargo:rerun-if-env-changed=SQLITE_CAPSULE_UPDATER_PUBLIC_KEY");
    println!("cargo:rerun-if-env-changed=SQLITE_CAPSULE_RELEASE_PUBLIC_KEY_HEX");
    println!("cargo:rerun-if-env-changed=SQLITE_CAPSULE_HOST_RELEASE_SEQUENCE");
    for icon in [
        "icons/32x32.png",
        "icons/128x128.png",
        "icons/128x128@2x.png",
        "icons/icon.icns",
        "icons/icon.ico",
        "icons/icon.png",
        "icons/icon.svg",
    ] {
        println!("cargo:rerun-if-changed={icon}");
    }
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(&["startup_report"])),
    )
    .expect("failed to prepare Tauri build");
}

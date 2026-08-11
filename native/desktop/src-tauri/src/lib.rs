use std::{
    borrow::Cow,
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(debug_assertions)]
use std::ffi::OsStr;

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use minisign_verify::{PublicKey as MinisignPublicKey, Signature as MinisignSignature};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlite_capsule_core::{
    CapsuleIdentity,
    protocol::{ProtocolParams, ProtocolSession},
};
use sqlite_capsule_distribution::{
    InstalledReleaseContext, ReleaseCandidateContext, SignedReleaseManifest, UpdateAuthorization,
    VerifiedDownloadedUpdate, VerifiedReleaseCandidate, accept_downloaded_update,
    authorize_installable_update, verify_candidate_artifact_bytes,
    verify_candidate_sigstore_bundle_bytes, verify_release_candidate,
};
use sqlite_capsule_installer::{
    discover_bootstrap_installer, launch_prepared, launch_rollback, lock_installer_source,
};
use sqlite_capsule_launch::LaunchInspection;
use sqlite_capsule_lifecycle::{prepare_private_directory, protect_private_file};
use sqlite_capsule_platform::{PlatformVerificationReport, verify_platform_artifact};
use sqlite_capsule_policy::{
    CapabilityDecision, EvaluationContext, LaunchDecision, LaunchEvidence, SUPPORTED_CAPABILITIES,
    TrustState, TrustStore,
};
use sqlite_capsule_runtime::{
    BackupInventoryReport, BackupRecord, RecoveryReport, RestoreRecord, RuntimeError,
    VerifiedCapsule, inspect_backup_inventory, inspect_launch_with_recovery,
    restore_verified_backup,
};
use sqlite_capsule_signing::{
    LoadedSigningKey, PreparedCapsule, SigningPreview, SigningReport as NativeSigningReport,
    SigningSource, inspect_signing_source, prepare_capsule_signing as prepare_signing_copy,
};
use sqlite_capsule_sigstore::verify_sigstore_bundle;
use sqlite_capsule_update::{
    PreparedInstallation, PreviousInstaller, StageRequest, StagedUpdate, UpdateInventoryReport,
    UpdateStageState, UpdateStager,
};
use tauri::{AppHandle, DragDropEvent, Emitter, Manager, State, WindowEvent};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_updater::{Update as TransportUpdate, UpdaterExt};
#[cfg(target_os = "windows")]
use wry::WebViewBuilderExtWindows;
use wry::{
    NewWindowResponse, Rect, WebView, WebViewBuilder,
    dpi::{LogicalPosition, LogicalSize},
    http::{Method, Request, Response, StatusCode, header},
};

const CAPSULE_WINDOW_LABEL: &str = "capsule";
const CAPSULE_WINDOW_TITLE: &str = "SQLite Capsule — application";
const CAPSULE_WINDOW_WIDTH: f64 = 1280.0;
const CAPSULE_WINDOW_HEIGHT: f64 = 900.0;
const CAPSULE_WINDOW_MIN_WIDTH: f64 = 640.0;
const CAPSULE_WINDOW_MIN_HEIGHT: f64 = 480.0;
const CUSTOM_PROTOCOL: &str = "capsule";
const CUSTOM_HOST: &str = "app";
const COMPILED_UPDATER_ENDPOINT: Option<&str> = option_env!("SQLITE_CAPSULE_UPDATER_ENDPOINT");
const COMPILED_UPDATER_PUBLIC_KEY: Option<&str> = option_env!("SQLITE_CAPSULE_UPDATER_PUBLIC_KEY");
const COMPILED_RELEASE_PUBLIC_KEY_HEX: Option<&str> =
    option_env!("SQLITE_CAPSULE_RELEASE_PUBLIC_KEY_HEX");
const COMPILED_HOST_RELEASE_SEQUENCE: Option<&str> =
    option_env!("SQLITE_CAPSULE_HOST_RELEASE_SEQUENCE");
const MAX_SIGSTORE_BUNDLE_BYTES: u64 = 16 * 1024 * 1024;
const INSPECTION_STACK_BYTES: usize = 8 * 1024 * 1024;
const RUNTIME_WORKER_STACK_BYTES: usize = 8 * 1024 * 1024;
const CHILD_CSP: &str = "default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self'; img-src 'self' data:; connect-src 'self'; font-src 'none'; media-src 'none'; object-src 'none'; frame-src 'none'; worker-src 'none'; child-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'; navigate-to 'self'";
const CHILD_PERMISSIONS_POLICY: &str = "camera=(), microphone=(), geolocation=(), payment=(), usb=(), serial=(), hid=(), bluetooth=(), browsing-topics=(), publickey-credentials-get=()";

thread_local! {
    static SANDBOX_WEBVIEW: RefCell<Option<WebView>> = const { RefCell::new(None) };
}

#[derive(Default)]
struct RuntimeBridge {
    runtime: Option<VerifiedCapsule>,
    protocol: Option<ProtocolSession>,
    session_token: Option<String>,
    writer_lock_root: PathBuf,
    backup_root: PathBuf,
    conflict_backup: Option<BackupRecord>,
    conflict_renderer_locked: bool,
    mode: String,
}

struct RuntimeWorkerPermit(Arc<AtomicBool>);

impl Drop for RuntimeWorkerPermit {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl RuntimeBridge {
    fn new(writer_lock_root: PathBuf, backup_root: PathBuf) -> Self {
        Self {
            runtime: None,
            protocol: None,
            session_token: None,
            writer_lock_root,
            backup_root,
            conflict_backup: None,
            conflict_renderer_locked: false,
            mode: "locked".to_owned(),
        }
    }

    fn activate(
        &mut self,
        inspection: &LaunchInspection,
        decision: &LaunchDecision,
    ) -> Result<(), String> {
        self.deactivate();
        let writable = decision
            .capabilities
            .get("database.write")
            .is_some_and(|capability| capability.decision == CapabilityDecision::Allow);
        let (runtime, mode) = match VerifiedCapsule::open(
            &inspection.identity.canonical_path,
            inspection,
            decision,
            writable,
            writable.then_some(self.writer_lock_root.as_path()),
            writable.then_some(self.backup_root.as_path()),
        ) {
            Ok(runtime) => (runtime, if writable { "writable" } else { "read_only" }),
            Err(error) => match read_only_fallback_mode(&error, writable) {
                Some(mode) => (
                    VerifiedCapsule::open(
                        &inspection.identity.canonical_path,
                        inspection,
                        decision,
                        false,
                        None,
                        None,
                    )
                    .map_err(|error| error.to_string())?,
                    mode,
                ),
                None => return Err(error.to_string()),
            },
        };
        self.install(runtime, mode)
    }

    fn activate_read_only(
        &mut self,
        inspection: &LaunchInspection,
        decision: &LaunchDecision,
    ) -> Result<(), String> {
        self.deactivate();
        let runtime = VerifiedCapsule::open(
            &inspection.identity.canonical_path,
            inspection,
            decision,
            false,
            None,
            None,
        )
        .map_err(|error| error.to_string())?;
        self.install(runtime, "read_only_user_selected")
    }

    fn install(&mut self, runtime: VerifiedCapsule, mode: &str) -> Result<(), String> {
        let token = generate_session_token()?;
        let protocol = ProtocolSession::new(token.clone()).map_err(|error| error.to_string())?;
        self.runtime = Some(runtime);
        self.protocol = Some(protocol);
        self.session_token = Some(token);
        self.mode = mode.to_owned();
        Ok(())
    }

    fn deactivate(&mut self) {
        self.runtime = None;
        self.protocol = None;
        self.session_token = None;
        self.conflict_backup = None;
        self.conflict_renderer_locked = false;
        self.mode = "locked".to_owned();
    }

    /// Establish the capsule recovery point required before replacing the
    /// native host, then release both the runtime and its renderer session.
    /// A failed checkpoint leaves the active session untouched.
    fn prepare_for_host_update(&mut self) -> Result<UpdatePreflightReport, String> {
        let Some(runtime) = self.runtime.as_mut() else {
            return Ok(UpdatePreflightReport {
                had_active_session: false,
                writable_session: false,
                session_quiesced: true,
                verified_backup: None,
            });
        };
        let writable_session = runtime.writable();
        let verified_backup = runtime
            .prepare_for_host_update()
            .map_err(|error| error.to_string())?;
        if writable_session && verified_backup.is_none() {
            return Err("writable update preflight did not produce a verified backup".to_owned());
        }
        self.deactivate();
        Ok(UpdatePreflightReport {
            had_active_session: true,
            writable_session,
            session_quiesced: true,
            verified_backup,
        })
    }

    fn close_for_conflict(&mut self) {
        let recovery_point = self
            .runtime
            .as_ref()
            .and_then(VerifiedCapsule::backup_record)
            .cloned();
        self.runtime = None;
        self.protocol = None;
        self.session_token = None;
        self.conflict_backup = recovery_point;
        self.conflict_renderer_locked = false;
        self.mode = "conflict_closed".to_owned();
    }

    fn take_conflict_renderer_lock(&mut self) -> bool {
        if self.mode != "conflict_closed" || self.conflict_renderer_locked {
            return false;
        }
        self.conflict_renderer_locked = true;
        true
    }

    fn handle(&mut self, request: Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
        let path = request.uri().path();
        if request.method() == Method::GET && path == "/__host/locked" {
            return response_builder(StatusCode::OK)
                .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
                .header(header::CACHE_CONTROL, "no-store")
                .body(Cow::Owned(RAW_SANDBOX_PROBE.as_bytes().to_vec()))
                .expect("static response headers are valid");
        }
        if request.method() == Method::GET && path == "/__host/locked.css" {
            return response_builder(StatusCode::OK)
                .header(header::CONTENT_TYPE, "text/css; charset=utf-8")
                .header(header::CACHE_CONTROL, "no-store")
                .body(Cow::Owned(RAW_SANDBOX_PROBE_CSS.as_bytes().to_vec()))
                .expect("static response headers are valid");
        }
        if request.method() == Method::GET && path == "/__host/locked.js" {
            return response_builder(StatusCode::OK)
                .header(header::CONTENT_TYPE, "text/javascript; charset=utf-8")
                .header(header::CACHE_CONTROL, "no-store")
                .body(Cow::Owned(RAW_SANDBOX_PROBE_JS.as_bytes().to_vec()))
                .expect("static response headers are valid");
        }
        if request.method() == Method::GET && path == "/__capsule/native-session" {
            return match &self.session_token {
                Some(token) => json_response(
                    StatusCode::OK,
                    &json!({"ok": true, "version": 1, "session": token}),
                ),
                None => error_response(StatusCode::LOCKED, "runtime is locked"),
            };
        }
        if request.method() == Method::POST && path == "/__capsule/rpc" {
            return self.handle_rpc(request.body());
        }
        if request.method() != Method::GET || path.starts_with("/__capsule/") {
            return error_response(StatusCode::NOT_FOUND, "resource not found");
        }
        let Some(runtime) = self.runtime.as_ref() else {
            return error_response(StatusCode::LOCKED, "runtime is locked");
        };
        let Some(asset_path) = decode_asset_request_path(path) else {
            return error_response(StatusCode::BAD_REQUEST, "invalid asset path");
        };
        match runtime.asset(&asset_path) {
            Ok(asset) => asset_response(asset),
            Err(error) if error.session_must_close() => {
                self.close_for_conflict();
                error_response(
                    StatusCode::CONFLICT,
                    "capsule session changed and was closed",
                )
            }
            Err(_) => error_response(StatusCode::NOT_FOUND, "asset not found"),
        }
    }

    fn handle_rpc(&mut self, body: &[u8]) -> Response<Cow<'static, [u8]>> {
        let Some(protocol) = self.protocol.as_mut() else {
            return error_response(StatusCode::LOCKED, "runtime is locked");
        };
        let request = match protocol.accept(body) {
            Ok(request) => request,
            Err(error) => return protocol_parse_error_response(&error),
        };
        let Some(runtime) = self.runtime.as_mut() else {
            return protocol_operation_error_response(
                StatusCode::LOCKED,
                request.sequence,
                &request.id,
                "runtime_locked",
                "runtime is locked",
            );
        };
        let result: Result<Value, (String, bool)> = match request.params {
            ProtocolParams::Manifest => {
                serde_json::to_value(runtime.manifest()).map_err(|error| (error.to_string(), false))
            }
            ProtocolParams::Permissions => Ok(runtime.permissions()),
            ProtocolParams::Read(parameters) => runtime
                .read_endpoint(&parameters.endpoint, &parameters.arguments)
                .map_err(runtime_operation_error),
            ProtocolParams::Write(parameters) => runtime
                .write_endpoint(&parameters.endpoint, &parameters.arguments)
                .map_err(runtime_operation_error),
        };
        match result {
            Ok(result) => json_response(
                StatusCode::OK,
                &json!({
                    "version": 1,
                    "ok": true,
                    "id": request.id,
                    "sequence": request.sequence,
                    "result": result
                }),
            ),
            Err((error, close_session)) => {
                if close_session {
                    self.close_for_conflict();
                }
                protocol_operation_error_response(
                    if close_session {
                        StatusCode::CONFLICT
                    } else {
                        StatusCode::BAD_REQUEST
                    },
                    request.sequence,
                    &request.id,
                    if close_session {
                        "session_conflict"
                    } else {
                        "operation_failed"
                    },
                    &error,
                )
            }
        }
    }
}

fn read_only_fallback_mode(error: &RuntimeError, writable: bool) -> Option<&'static str> {
    if !writable {
        return None;
    }
    match error {
        RuntimeError::Lifecycle(sqlite_capsule_lifecycle::LifecycleError::WriterBusy) => {
            Some("read_only_writer_busy")
        }
        RuntimeError::Lifecycle(
            sqlite_capsule_lifecycle::LifecycleError::UnsafeWritableFileSystem,
        ) => Some("read_only_unsafe_filesystem"),
        _ => None,
    }
}

fn handle_protocol_request(
    protocol_bridge: Arc<Mutex<RuntimeBridge>>,
    worker_gate: Arc<AtomicBool>,
    request: Request<Vec<u8>>,
) -> Response<Cow<'static, [u8]>> {
    if request.method() != Method::POST || request.uri().path() != "/__capsule/rpc" {
        return match protocol_bridge.try_lock() {
            Ok(mut bridge) => bridge.handle(request),
            Err(_) => error_response(StatusCode::SERVICE_UNAVAILABLE, "runtime busy"),
        };
    }

    // Wry invokes the Windows custom-protocol callback on the native UI thread,
    // whose stack is too small for the bounded SQLite backup plus independent
    // verification required before the first named write. Keep the synchronous
    // response contract, but perform every sequenced RPC on a fixed-stack worker
    // so untrusted endpoint input cannot exhaust the event-loop stack.
    if worker_gate
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return error_response(StatusCode::SERVICE_UNAVAILABLE, "runtime busy");
    }
    let _permit = RuntimeWorkerPermit(worker_gate);
    let worker = std::thread::Builder::new()
        .name("sqlite-capsule-runtime-rpc".to_owned())
        .stack_size(RUNTIME_WORKER_STACK_BYTES)
        .spawn(move || match protocol_bridge.try_lock() {
            Ok(mut bridge) => bridge.handle(request),
            Err(_) => error_response(StatusCode::SERVICE_UNAVAILABLE, "runtime busy"),
        });
    match worker {
        Ok(worker) => worker.join().unwrap_or_else(|_| {
            error_response(StatusCode::SERVICE_UNAVAILABLE, "runtime worker failed")
        }),
        Err(_) => error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "runtime worker unavailable",
        ),
    }
}

fn runtime_operation_error(error: RuntimeError) -> (String, bool) {
    let close_session = error.session_must_close();
    (error.to_string(), close_session)
}

fn protocol_parse_error_response(
    error: &sqlite_capsule_core::protocol::ProtocolError,
) -> Response<Cow<'static, [u8]>> {
    use sqlite_capsule_core::protocol::ProtocolError;

    let (status, code) = match error {
        ProtocolError::TooLarge => (StatusCode::PAYLOAD_TOO_LARGE, "request_too_large"),
        ProtocolError::Malformed => (StatusCode::BAD_REQUEST, "malformed_request"),
        ProtocolError::Version => (StatusCode::BAD_REQUEST, "unsupported_version"),
        ProtocolError::Session => (StatusCode::BAD_REQUEST, "invalid_session"),
        ProtocolError::Sequence => (StatusCode::BAD_REQUEST, "invalid_sequence"),
        ProtocolError::RequestId => (StatusCode::BAD_REQUEST, "invalid_request_id"),
        ProtocolError::Endpoint => (StatusCode::BAD_REQUEST, "invalid_endpoint"),
        ProtocolError::SessionExhausted => (StatusCode::BAD_REQUEST, "session_exhausted"),
    };
    json_response(
        status,
        &json!({
            "ok": false,
            "error": {"code": code, "message": error.to_string()}
        }),
    )
}

fn protocol_operation_error_response(
    status: StatusCode,
    sequence: u64,
    id: &str,
    code: &str,
    message: &str,
) -> Response<Cow<'static, [u8]>> {
    json_response(
        status,
        &json!({
            "version": 1,
            "sequence": sequence,
            "id": id,
            "ok": false,
            "error": {"code": code, "message": message}
        }),
    )
}

fn generate_session_token() -> Result<String, String> {
    let mut random = [0_u8; 32];
    getrandom::fill(&mut random).map_err(|error| format!("OS random source failed: {error}"))?;
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut output = String::with_capacity(43);
    let mut index = 0;
    while index + 3 <= random.len() {
        let block = u32::from(random[index]) << 16
            | u32::from(random[index + 1]) << 8
            | u32::from(random[index + 2]);
        output.push(ALPHABET[((block >> 18) & 63) as usize] as char);
        output.push(ALPHABET[((block >> 12) & 63) as usize] as char);
        output.push(ALPHABET[((block >> 6) & 63) as usize] as char);
        output.push(ALPHABET[(block & 63) as usize] as char);
        index += 3;
    }
    let remainder = &random[index..];
    if remainder.len() == 2 {
        let block = u32::from(remainder[0]) << 16 | u32::from(remainder[1]) << 8;
        output.push(ALPHABET[((block >> 18) & 63) as usize] as char);
        output.push(ALPHABET[((block >> 12) & 63) as usize] as char);
        output.push(ALPHABET[((block >> 6) & 63) as usize] as char);
    } else if remainder.len() == 1 {
        let block = u32::from(remainder[0]) << 16;
        output.push(ALPHABET[((block >> 18) & 63) as usize] as char);
        output.push(ALPHABET[((block >> 12) & 63) as usize] as char);
    }
    if output.len() != 43 {
        return Err("session token encoder produced an invalid length".to_owned());
    }
    Ok(output)
}

fn asset_response(asset: sqlite_capsule_runtime::RuntimeAsset) -> Response<Cow<'static, [u8]>> {
    response_builder(StatusCode::OK)
        .header(header::CONTENT_TYPE, asset.media_type)
        .header(header::CACHE_CONTROL, "no-store")
        .body(Cow::Owned(asset.content))
        .expect("static response headers are valid")
}

fn json_response(status: StatusCode, value: &Value) -> Response<Cow<'static, [u8]>> {
    let body = serde_json::to_vec(value)
        .unwrap_or_else(|_| b"{\"ok\":false,\"error\":\"response serialization failed\"}".to_vec());
    response_builder(status)
        .header(header::CONTENT_TYPE, "application/json; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Cow::Owned(body))
        .expect("static response headers are valid")
}

fn error_response(status: StatusCode, message: &str) -> Response<Cow<'static, [u8]>> {
    json_response(status, &json!({"ok": false, "error": message}))
}

fn response_builder(status: StatusCode) -> wry::http::response::Builder {
    Response::builder()
        .status(status)
        .header("Content-Security-Policy", CHILD_CSP)
        .header("Permissions-Policy", CHILD_PERMISSIONS_POLICY)
        .header("X-Content-Type-Options", "nosniff")
        .header("Referrer-Policy", "no-referrer")
        .header("Cross-Origin-Resource-Policy", "same-origin")
}

fn decode_asset_request_path(path: &str) -> Option<String> {
    let mut decoded = Vec::new();
    for segment in path.trim_start_matches('/').split('/') {
        if segment.is_empty() {
            return None;
        }
        let segment = percent_decode(segment)?;
        if segment.contains(['/', '\\']) || matches!(segment.as_str(), "." | "..") {
            return None;
        }
        decoded.push(segment);
    }
    Some(decoded.join("/"))
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = hex_value(*bytes.get(index + 1)?)?;
            let low = hex_value(*bytes.get(index + 2)?)?;
            output.push(high << 4 | low);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(output).ok()
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

const RAW_SANDBOX_PROBE: &str = r##"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <title>Raw child renderer probe</title>
  <link rel="stylesheet" href="/__host/locked.css">
</head>
<body>
  <header><h1>Untrusted renderer · raw Wry child</h1><span id="decision">Running fail-closed probe…</span></header>
  <dl id="checks"></dl>
  <script src="/__host/locked.js"></script>
</body>
</html>"##;

const RAW_SANDBOX_PROBE_CSS: &str = r#"
:root { color-scheme: dark; font: 12px/1.35 Consolas, 'Cascadia Mono', monospace; background: #0f1519; color: #dce7eb; }
* { box-sizing: border-box; }
body { margin: 0; min-height: 100vh; padding: 12px 16px; border-left: 4px solid #d69055; background: linear-gradient(90deg, rgba(214,144,85,.09), transparent 30%), #0f1519; }
header { display: flex; align-items: center; justify-content: space-between; gap: 16px; margin-bottom: 9px; }
h1 { margin: 0; color: #e9f0f2; font-size: 12px; letter-spacing: .11em; text-transform: uppercase; }
#decision { color: #9ca9af; font-weight: 700; }
#decision.pass { color: #84d9bf; }
#decision.fail { color: #ffaaa0; }
#checks { display: grid; grid-template-columns: repeat(6, minmax(0, 1fr)); gap: 7px; margin: 0; }
#checks div { min-width: 0; padding: 8px 9px; border: 1px solid #2a3941; background: #141d22; }
dt { overflow: hidden; color: #819099; font-size: 9px; font-weight: 700; letter-spacing: .08em; text-overflow: ellipsis; text-transform: uppercase; white-space: nowrap; }
dd { margin: 3px 0 0; overflow: hidden; color: #edf3f5; font-size: 11px; text-overflow: ellipsis; white-space: nowrap; }
.ok { color: #84d9bf; }
.bad { color: #ffaaa0; }
"#;

const RAW_SANDBOX_PROBE_JS: &str = r##"
(async () => {
  const blocked = async url => {
    try { await fetch(url, { cache: "no-store" }); return false; }
    catch { return true; }
  };
  const checks = [
    ["Tauri global", globalThis.__TAURI__ ? "Exposed" : "Absent", !globalThis.__TAURI__],
    ["Tauri internals", globalThis.__TAURI_INTERNALS__ ? "Exposed" : "Absent", !globalThis.__TAURI_INTERNALS__],
    ["Wry transport", globalThis.ipc ? "Present · unbound" : "Absent", true],
    ["Shell parent", parent === window ? "Separate view" : "Reachable", parent === window],
    ["Network", await blocked("https://example.invalid/"), true],
    ["Local file", await blocked("file:///__sqlite_capsule_probe__"), true],
  ];
  checks[4][1] = checks[4][1] ? "Blocked" : "Reached";
  checks[4][2] = checks[4][1] === "Blocked";
  checks[5][1] = checks[5][1] ? "Blocked" : "Reached";
  checks[5][2] = checks[5][1] === "Blocked";
  document.querySelector("#checks").replaceChildren(...checks.map(([label, value, ok]) => {
    const row = document.createElement("div");
    const term = document.createElement("dt");
    const description = document.createElement("dd");
    term.textContent = label;
    description.textContent = value;
    description.className = ok ? "ok" : "bad";
    row.append(term, description);
    return row;
  }));
  const passed = checks.every(([, , ok]) => ok);
  const decision = document.querySelector("#decision");
  decision.textContent = passed ? "PASS · no native handler" : "FAIL CLOSED · code stays locked";
  decision.className = passed ? "pass" : "fail";
})();
"##;

fn application_bounds(width: f64, height: f64) -> Rect {
    Rect {
        position: LogicalPosition::new(0.0, 0.0).into(),
        size: LogicalSize::new(width.max(320.0), height.max(240.0)).into(),
    }
}

fn resize_application(webview: &WebView, width: f64, height: f64) {
    if let Err(error) = webview.set_bounds(application_bounds(width, height)) {
        eprintln!("failed to resize raw application webview: {error}");
    }
}

#[derive(Clone, Debug, Serialize)]
struct PublisherReport {
    id: String,
    name: String,
}

#[derive(Clone, Debug, Serialize)]
struct SignatureReport {
    key_id: String,
    cryptographically_valid: bool,
    digest_matches: bool,
}

#[derive(Clone, Debug, Serialize)]
struct CapsuleReport {
    identity: CapsuleIdentity,
    source_sha256: String,
    application_digest: Option<String>,
    publisher: Option<PublisherReport>,
    signatures: Vec<SignatureReport>,
    decision: LaunchDecision,
    assets_released: bool,
}

#[derive(Clone, Debug, Serialize)]
struct StartupReport {
    stage: String,
    capsule: Option<CapsuleReport>,
    recovery: Option<RecoveryReport>,
    error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct LifecycleStatus {
    active: bool,
    writable: bool,
    mode: String,
    backup: Option<BackupRecord>,
    backup_inventory: BackupInventoryReport,
}

#[derive(Clone, Debug, Serialize)]
struct SigningKeyReport {
    file_name: String,
    format: String,
    key_id: String,
    public_key_hex: String,
}

#[derive(Clone, Debug, Serialize)]
struct SigningSourceReport {
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Clone, Debug, Serialize)]
struct SigningPreviewReport {
    source: SigningSourceReport,
    output: String,
    publisher_id: String,
    publisher_name: String,
    application_digest: String,
    signed_at: String,
}

#[derive(Clone, Debug, Serialize)]
struct SigningSessionReport {
    key: Option<SigningKeyReport>,
    source: Option<SigningSourceReport>,
    output: Option<String>,
    preview: Option<SigningPreviewReport>,
    busy: bool,
}

#[derive(Clone, Debug, Serialize)]
struct SigningResultReport {
    source: SigningSourceReport,
    output: String,
    output_bytes: u64,
    output_sha256: String,
    publisher_id: String,
    publisher_name: String,
    key_id: String,
    public_key_hex: String,
    application_digest: String,
    signed_at: String,
    signature_valid: bool,
    publisher_trusted: bool,
}

#[derive(Debug, Deserialize)]
struct PrepareSigningRequest {
    publisher_id: String,
    publisher_name: String,
}

#[derive(Debug, Deserialize)]
struct ExecuteSigningRequest {
    confirmation_key_id: String,
    confirmation_application_digest: String,
}

#[derive(Default)]
struct SigningSession {
    key: Option<LoadedSigningKey>,
    key_file_name: Option<String>,
    source: Option<SigningSource>,
    output: Option<PathBuf>,
    prepared: Option<PreparedCapsule>,
    busy: bool,
}

#[derive(Clone, Default)]
struct SigningState(Arc<Mutex<SigningSession>>);

impl SigningSession {
    fn report(&self) -> SigningSessionReport {
        SigningSessionReport {
            key: self.key.as_ref().map(|key| {
                let info = key.info();
                SigningKeyReport {
                    file_name: self
                        .key_file_name
                        .clone()
                        .unwrap_or_else(|| "Selected private key".to_owned()),
                    format: info.format.as_str().to_owned(),
                    key_id: info.key_id.clone(),
                    public_key_hex: info.public_key_hex.clone(),
                }
            }),
            source: self.source.as_ref().map(signing_source_report),
            output: self
                .output
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned()),
            preview: self
                .prepared
                .as_ref()
                .map(|prepared| signing_preview_report(prepared.preview())),
            busy: self.busy,
        }
    }

    fn invalidate_preview(&mut self) {
        self.prepared = None;
    }
}

fn signing_source_report(source: &SigningSource) -> SigningSourceReport {
    SigningSourceReport {
        path: source.canonical_path.to_string_lossy().into_owned(),
        bytes: source.bytes,
        sha256: source.sha256.clone(),
    }
}

fn signing_preview_report(preview: &SigningPreview) -> SigningPreviewReport {
    SigningPreviewReport {
        source: signing_source_report(&preview.source),
        output: preview.output.to_string_lossy().into_owned(),
        publisher_id: preview.publisher_id.clone(),
        publisher_name: preview.publisher_name.clone(),
        application_digest: preview.application_digest.clone(),
        signed_at: preview.signed_at.clone(),
    }
}

fn signing_result_report(report: NativeSigningReport) -> SigningResultReport {
    SigningResultReport {
        source: signing_source_report(&report.preview.source),
        output: report.preview.output.to_string_lossy().into_owned(),
        output_bytes: report.output_bytes,
        output_sha256: report.output_sha256,
        publisher_id: report.preview.publisher_id,
        publisher_name: report.preview.publisher_name,
        key_id: report.key.key_id,
        public_key_hex: report.key.public_key_hex,
        application_digest: report.preview.application_digest,
        signed_at: report.preview.signed_at,
        signature_valid: report.signature_valid,
        publisher_trusted: report.publisher_trusted,
    }
}

#[derive(Clone, Debug, Serialize)]
struct UpdatePreflightReport {
    had_active_session: bool,
    writable_session: bool,
    session_quiesced: bool,
    verified_backup: Option<BackupRecord>,
}

#[derive(Debug, Deserialize)]
struct PrepareUpdateRequest {
    stage_id: String,
    confirmation: String,
}

#[derive(Debug, Serialize)]
struct PrepareUpdateResponse {
    stage_id: String,
    candidate_version: String,
    preflight: UpdatePreflightReport,
}

#[derive(Debug, Deserialize)]
struct ExecuteUpdateRequest {
    stage_id: String,
    confirmation: String,
}

#[derive(Debug, Serialize)]
struct ExecuteUpdateResponse {
    stage_id: String,
    candidate_version: String,
    state: UpdateStageState,
    installer_launched: bool,
    preflight: UpdatePreflightReport,
}

#[derive(Debug, Deserialize)]
struct ExecuteRollbackRequest {
    stage_id: String,
    confirmation: String,
}

#[derive(Debug, Serialize)]
struct ExecuteRollbackResponse {
    stage_id: String,
    rollback_version: String,
    state: UpdateStageState,
    installer_launched: bool,
    preflight: UpdatePreflightReport,
}

#[derive(Debug, Deserialize)]
struct StageDownloadedUpdateRequest {
    candidate_version: String,
    confirmation: String,
}

#[derive(Debug, Serialize)]
struct StageDownloadedUpdateResponse {
    stage_id: String,
    candidate_version: String,
    artifact_name: String,
    sigstore_name: String,
    install_authorized: bool,
    staged: bool,
    installed: bool,
    preflight: UpdatePreflightReport,
}

#[derive(Debug)]
struct StageDownloadedUpdateError {
    message: String,
    session_quiesced: bool,
}

impl StageDownloadedUpdateError {
    fn before_preflight(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            session_quiesced: false,
        }
    }

    fn after_preflight(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            session_quiesced: true,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct HostUpdateStatus {
    current_version: String,
    transport_configured: bool,
    transport_endpoint_origin: Option<String>,
    review_state: String,
    stage_id: Option<String>,
    candidate_version: Option<String>,
    candidate_sequence: Option<u64>,
    release_policy_verified: bool,
    downloaded: bool,
    downloaded_artifact_bytes: Option<u64>,
    downloaded_sigstore_bundle_bytes: Option<u64>,
    platform_signature_verified: bool,
    platform_signer_subject: Option<String>,
    sigstore_signature_verified: bool,
    sigstore_certificate_identity: Option<String>,
    sigstore_integrated_time_unix: Option<i64>,
    state: Option<UpdateStageState>,
    rollback_available: bool,
    incomplete_artifacts: Vec<String>,
    invalid_artifacts: Vec<String>,
    error: Option<String>,
}

#[derive(Clone, Debug)]
struct CompiledUpdaterConfig {
    endpoint: tauri::Url,
    public_key: String,
    release_public_key: [u8; 32],
    current_release_sequence: u64,
    endpoint_origin: String,
}

struct ReviewedHostUpdate {
    transport: TransportUpdate,
    candidate: VerifiedReleaseCandidate,
    sigstore_bundle_url: tauri::Url,
}

struct DownloadedHostUpdate {
    reviewed: Box<ReviewedHostUpdate>,
    artifact_bytes: Vec<u8>,
    sigstore_bundle_bytes: Vec<u8>,
    accepted: VerifiedDownloadedUpdate,
}

#[derive(Clone, Debug)]
struct BusyHostUpdate {
    candidate_version: String,
}

#[derive(Default)]
enum HostUpdateFlow {
    #[default]
    Idle,
    Reviewed(Box<ReviewedHostUpdate>),
    Busy(BusyHostUpdate),
    Downloaded(Box<DownloadedHostUpdate>),
}

struct UpdateCheckGate(AtomicBool);

impl UpdateCheckGate {
    fn acquire(&self) -> Result<UpdateCheckLease<'_>, String> {
        self.0
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "a host-update check is already in progress".to_owned())?;
        Ok(UpdateCheckLease(&self.0))
    }
}

struct UpdateCheckLease<'a>(&'a AtomicBool);

impl Drop for UpdateCheckLease<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[derive(Clone, Debug, Serialize)]
struct UpdateCheckReport {
    available: bool,
    current_version: String,
    candidate_version: Option<String>,
    candidate_sequence: Option<u64>,
    target: Option<String>,
    transport_endpoint_origin: String,
    release_policy_verified: bool,
    downloaded: bool,
    install_authorized: bool,
}

#[derive(Debug, Deserialize)]
struct DownloadUpdateRequest {
    candidate_version: String,
    confirmation: String,
}

#[derive(Clone, Debug, Serialize)]
struct UpdateDownloadReport {
    candidate_version: String,
    candidate_sequence: u64,
    target: String,
    artifact_bytes: u64,
    sigstore_bundle_bytes: u64,
    updater_signature_verified: bool,
    release_digest_verified: bool,
    sigstore_digest_matched: bool,
    platform_signature_verified: bool,
    sigstore_signature_verified: bool,
    install_authorized: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CapsuleUpdateMetadata {
    signed_release: SignedReleaseManifest,
    sigstore_bundle_url: String,
}

fn validate_compiled_updater_config(
    endpoint: Option<&str>,
    public_key: Option<&str>,
    release_public_key_hex: Option<&str>,
    current_release_sequence: Option<&str>,
) -> Result<Option<CompiledUpdaterConfig>, String> {
    let (
        Some(endpoint),
        Some(public_key),
        Some(release_public_key_hex),
        Some(current_release_sequence),
    ) = (
        endpoint,
        public_key,
        release_public_key_hex,
        current_release_sequence,
    )
    else {
        return if endpoint.is_none()
            && public_key.is_none()
            && release_public_key_hex.is_none()
            && current_release_sequence.is_none()
        {
            Ok(None)
        } else {
            Err("compiled updater endpoint, updater key, release root, and current sequence must be supplied together".to_owned())
        };
    };
    let endpoint = endpoint
        .parse::<tauri::Url>()
        .map_err(|_| "compiled updater endpoint is not a valid URL".to_owned())?;
    if endpoint.scheme() != "https"
        || endpoint.host_str().is_none()
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.fragment().is_some()
    {
        return Err(
            "compiled updater endpoint must be credential-free HTTPS without a fragment".to_owned(),
        );
    }
    let public_key = public_key.trim();
    if public_key.is_empty() || public_key.len() > 16 * 1024 {
        return Err("compiled updater public key is empty or oversized".to_owned());
    }
    let release_public_key =
        decode_lower_hex_32(release_public_key_hex.trim()).ok_or_else(|| {
            "compiled release public key must be 32 lowercase hexadecimal bytes".to_owned()
        })?;
    let current_release_sequence = current_release_sequence
        .parse::<u64>()
        .ok()
        .filter(|sequence| *sequence > 0 && *sequence <= i64::MAX as u64)
        .ok_or_else(|| {
            "compiled current release sequence must be a positive signed-64-bit integer".to_owned()
        })?;
    let endpoint_origin = https_origin(&endpoint)
        .ok_or_else(|| "compiled updater endpoint has no permitted HTTPS origin".to_owned())?;
    Ok(Some(CompiledUpdaterConfig {
        endpoint,
        public_key: public_key.to_owned(),
        release_public_key,
        current_release_sequence,
        endpoint_origin,
    }))
}

fn compiled_updater_config() -> Result<Option<CompiledUpdaterConfig>, String> {
    validate_compiled_updater_config(
        COMPILED_UPDATER_ENDPOINT,
        COMPILED_UPDATER_PUBLIC_KEY,
        COMPILED_RELEASE_PUBLIC_KEY_HEX,
        COMPILED_HOST_RELEASE_SEQUENCE,
    )
}

fn decode_lower_hex_32(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return None;
    }
    let mut output = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let nibble = |byte: u8| {
            if byte.is_ascii_digit() {
                byte - b'0'
            } else {
                byte - b'a' + 10
            }
        };
        output[index] = (nibble(pair[0]) << 4) | nibble(pair[1]);
    }
    Some(output)
}

fn https_origin(url: &tauri::Url) -> Option<String> {
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    let host = url.host_str()?;
    Some(match url.port() {
        Some(port) => format!("https://{host}:{port}"),
        None => format!("https://{host}"),
    })
}

fn restricted_redirect_policy(origin: String) -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() >= 5 {
            return attempt.error("too many host-update redirects");
        }
        if https_origin(attempt.url()).as_deref() == Some(origin.as_str()) {
            attempt.follow()
        } else {
            attempt.stop()
        }
    })
}

#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
fn host_release_target() -> &'static str {
    "x86_64-pc-windows-msvc"
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn host_release_target() -> &'static str {
    "x86_64-unknown-linux-gnu"
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn host_release_target() -> &'static str {
    "aarch64-apple-darwin"
}

#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
fn host_release_target() -> &'static str {
    "x86_64-apple-darwin"
}

#[cfg(not(any(
    all(target_os = "windows", target_arch = "x86_64"),
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "macos", target_arch = "aarch64"),
    all(target_os = "macos", target_arch = "x86_64")
)))]
fn host_release_target() -> &'static str {
    "unsupported-native-target"
}

fn expected_platform_signing() -> &'static str {
    if cfg!(target_os = "windows") {
        "authenticode"
    } else if cfg!(target_os = "macos") {
        "developer-id-notarized"
    } else {
        "linux-detached"
    }
}

#[derive(Clone, Debug, Serialize)]
struct HostMessage {
    kind: String,
    message: String,
}

#[derive(Clone, Debug, Serialize)]
struct RestoreReport {
    restored_path: String,
    record: RestoreRecord,
}

#[derive(Clone, Debug, Serialize)]
struct SupportBundle {
    format: String,
    created_at_unix: u64,
    host_version: String,
    platform: String,
    architecture: String,
    content_policy: SupportBundleContentPolicy,
    startup: StartupReport,
    lifecycle: LifecycleStatus,
    update: HostUpdateStatus,
    trust: Value,
    #[serde(skip)]
    redactions: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
struct SupportBundleContentPolicy {
    capsule_controlled_text: String,
    host_severity_source: String,
    embedded_instructions_executed: bool,
    capsule_database_bytes_included: bool,
    trust_store_bytes_included: bool,
    selected_file_contents_included: bool,
    shutdown_tokens_included: bool,
    private_keys_included: bool,
}

struct HostState {
    inspection: Option<LaunchInspection>,
    trust_store: Option<TrustStore>,
    trust_backup_directory: PathBuf,
    update_root: PathBuf,
    bridge: Arc<Mutex<RuntimeBridge>>,
    report: StartupReport,
}

impl HostState {
    fn from_process(
        trust_path: PathBuf,
        trust_backup_directory: PathBuf,
        update_root: PathBuf,
        bridge: Arc<Mutex<RuntimeBridge>>,
    ) -> Self {
        let trust_store = match TrustStore::open(&trust_path) {
            Ok(store) => store,
            Err(error) => {
                return Self::rejected(
                    trust_backup_directory,
                    update_root,
                    bridge,
                    format!("protected trust store: {error}"),
                );
            }
        };
        let mut state = Self {
            inspection: None,
            trust_store: Some(trust_store),
            trust_backup_directory,
            update_root,
            bridge,
            report: StartupReport {
                stage: "no-capsule".to_owned(),
                capsule: None,
                recovery: None,
                error: None,
            },
        };
        if let Some(path) = initial_capsule_path_from_process() {
            state.load_capsule(&path);
        }
        state
    }

    fn load_capsule(&mut self, path: &Path) {
        let writer_lock_root = match self.bridge.lock() {
            Ok(mut bridge) => {
                bridge.deactivate();
                bridge.writer_lock_root.clone()
            }
            Err(_) => {
                self.inspection = None;
                self.report = StartupReport {
                    stage: "rejected".to_owned(),
                    capsule: None,
                    recovery: None,
                    error: Some("capsule runtime state is unavailable".to_owned()),
                };
                return;
            }
        };
        let (inspection, recovery) = match inspect_capsule_on_worker_stack(path, &writer_lock_root)
        {
            Ok(result) => result,
            Err(error) => {
                self.inspection = None;
                self.report = StartupReport {
                    stage: "rejected".to_owned(),
                    capsule: None,
                    recovery: None,
                    error: Some(error.to_string()),
                };
                return;
            }
        };
        let Some(trust_store) = self.trust_store.as_mut() else {
            self.inspection = None;
            self.report = StartupReport {
                stage: "rejected".to_owned(),
                capsule: None,
                recovery,
                error: Some("protected trust store is unavailable".to_owned()),
            };
            return;
        };
        let decision = match trust_store.evaluate(
            &inspection.evidence,
            &host_policy_context(&inspection.evidence),
        ) {
            Ok(decision) => decision,
            Err(error) => {
                self.inspection = None;
                self.report = StartupReport {
                    stage: "rejected".to_owned(),
                    capsule: None,
                    recovery,
                    error: Some(format!("trust evaluation: {error}")),
                };
                return;
            }
        };
        let executable_allowed = decision.executable_allowed;
        if executable_allowed {
            let activation = self
                .bridge
                .lock()
                .map_err(|_| "runtime bridge is unavailable".to_owned())
                .and_then(|mut bridge| bridge.activate(&inspection, &decision));
            if let Err(error) = activation {
                let mut report = report_for("runtime-rejected", &inspection, decision);
                report.recovery = recovery;
                report.error = Some(format!("verified runtime activation: {error}"));
                self.report = report;
                self.inspection = Some(inspection);
                return;
            }
        }
        self.report = report_for(
            if executable_allowed {
                "remembered-authorized"
            } else {
                "first-open"
            },
            &inspection,
            decision,
        );
        self.report.recovery = recovery;
        if let Some(capsule) = self.report.capsule.as_mut() {
            capsule.assets_released = executable_allowed;
        }
        self.inspection = Some(inspection);
    }

    fn reject_file_delivery(&mut self, stage: &str, error: impl Into<String>) {
        let error = match self.bridge.lock() {
            Ok(mut bridge) => {
                bridge.deactivate();
                error.into()
            }
            Err(_) => "capsule runtime state is unavailable".to_owned(),
        };
        self.inspection = None;
        self.report = StartupReport {
            stage: stage.to_owned(),
            capsule: None,
            recovery: None,
            error: Some(error),
        };
    }

    fn rejected(
        trust_backup_directory: PathBuf,
        update_root: PathBuf,
        bridge: Arc<Mutex<RuntimeBridge>>,
        error: String,
    ) -> Self {
        Self {
            inspection: None,
            trust_store: None,
            trust_backup_directory,
            update_root,
            bridge,
            report: StartupReport {
                stage: "rejected".to_owned(),
                capsule: None,
                recovery: None,
                error: Some(error),
            },
        }
    }
}

fn initial_capsule_path_from_process() -> Option<PathBuf> {
    let automation = native_e2e_enabled();
    let automation_path = std::env::var_os("SQLITE_CAPSULE_NATIVE_E2E_PATH");
    initial_capsule_path(automation, automation_path, std::env::args_os())
}

fn webview_automation_enabled() -> bool {
    cfg!(debug_assertions)
        && std::env::var_os("TAURI_WEBVIEW_AUTOMATION").is_some_and(|value| value == "true")
}

fn host_app_data_root_from_process(default: PathBuf) -> PathBuf {
    host_app_data_root(
        default,
        native_e2e_enabled(),
        std::env::var_os("SQLITE_CAPSULE_NATIVE_E2E_STATE_ROOT"),
    )
}

fn native_e2e_restore_path_from_process() -> Option<PathBuf> {
    native_e2e_restore_path(
        native_e2e_enabled(),
        std::env::var_os("SQLITE_CAPSULE_NATIVE_E2E_STATE_ROOT"),
        std::env::var_os("SQLITE_CAPSULE_NATIVE_E2E_RESTORE_PATH"),
    )
}

fn native_e2e_restore_path(
    automation: bool,
    state_root: Option<OsString>,
    requested: Option<OsString>,
) -> Option<PathBuf> {
    if !automation {
        return None;
    }
    let state_root = PathBuf::from(state_root.filter(|value| !value.is_empty())?);
    let requested = PathBuf::from(requested.filter(|value| !value.is_empty())?);
    if !state_root.is_absolute() || !requested.is_absolute() || requested.exists() {
        return None;
    }
    let canonical_root = state_root.canonicalize().ok()?;
    let canonical_parent = requested.parent()?.canonicalize().ok()?;
    if !canonical_parent.starts_with(&canonical_root) {
        return None;
    }
    Some(canonical_parent.join(requested.file_name()?))
}

#[cfg(debug_assertions)]
fn native_e2e_support_path_from_process() -> Option<PathBuf> {
    native_e2e_support_path(
        cdp_e2e_debug_ports_from_process().is_some(),
        std::env::var_os("SQLITE_CAPSULE_NATIVE_E2E_STATE_ROOT"),
        std::env::var_os("SQLITE_CAPSULE_NATIVE_E2E_SUPPORT_PATH"),
    )
}

#[cfg(not(debug_assertions))]
fn native_e2e_support_path_from_process() -> Option<PathBuf> {
    None
}

#[cfg(debug_assertions)]
fn native_e2e_support_path(
    cdp_automation: bool,
    state_root: Option<OsString>,
    requested: Option<OsString>,
) -> Option<PathBuf> {
    if !cdp_automation {
        return None;
    }
    let state_root = PathBuf::from(state_root.filter(|value| !value.is_empty())?);
    let requested = PathBuf::from(requested.filter(|value| !value.is_empty())?);
    if !state_root.is_absolute()
        || !state_root.is_dir()
        || !requested.is_absolute()
        || requested.extension() != Some(OsStr::new("json"))
    {
        return None;
    }
    let canonical_root = state_root.canonicalize().ok()?;
    let canonical_parent = requested.parent()?.canonicalize().ok()?;
    if !canonical_parent.starts_with(&canonical_root) {
        return None;
    }
    Some(canonical_parent.join(requested.file_name()?))
}

#[cfg(debug_assertions)]
fn native_e2e_update_preflight_requested_from_process() -> bool {
    native_e2e_update_preflight_requested(
        cdp_e2e_debug_ports_from_process().is_some(),
        std::env::var_os("SQLITE_CAPSULE_NATIVE_E2E_STATE_ROOT"),
        std::env::var_os("SQLITE_CAPSULE_NATIVE_E2E_RUNTIME_FAULTS"),
        std::env::var_os("SQLITE_CAPSULE_RUNTIME_FAULT_STAGE"),
    )
}

#[cfg(not(debug_assertions))]
fn native_e2e_update_preflight_requested_from_process() -> bool {
    false
}

#[cfg(debug_assertions)]
fn native_e2e_update_preflight_requested(
    cdp_automation: bool,
    state_root: Option<OsString>,
    fault_guard: Option<OsString>,
    fault_stage: Option<OsString>,
) -> bool {
    if !cdp_automation || fault_guard.as_deref() != Some(OsStr::new("enabled")) {
        return false;
    }
    let Some(state_root) = state_root
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    else {
        return false;
    };
    if !state_root.is_absolute() || !state_root.is_dir() {
        return false;
    }
    matches!(
        fault_stage.as_deref(),
        Some(stage)
            if stage == OsStr::new("update.marker-synced")
                || stage == OsStr::new("update.database-copied")
                || stage == OsStr::new("update.manifest-synced")
    )
}

fn native_e2e_enabled() -> bool {
    webview_automation_enabled() || cdp_e2e_debug_ports_from_process().is_some()
}

#[cfg(target_os = "windows")]
fn cdp_e2e_debug_ports_from_process() -> Option<(u16, u16)> {
    cdp_e2e_debug_ports(
        cfg!(debug_assertions),
        std::env::var_os("SQLITE_CAPSULE_NATIVE_PARENT_E2E_PORT"),
        std::env::var_os("SQLITE_CAPSULE_NATIVE_RAW_E2E_PORT"),
    )
}

#[cfg(not(target_os = "windows"))]
fn cdp_e2e_debug_ports_from_process() -> Option<(u16, u16)> {
    None
}

fn cdp_e2e_debug_ports(
    debug_build: bool,
    parent_value: Option<OsString>,
    raw_value: Option<OsString>,
) -> Option<(u16, u16)> {
    if !debug_build {
        return None;
    }
    let parse = |value: Option<OsString>| {
        value
            .and_then(|value| value.into_string().ok())
            .and_then(|value| value.parse::<u16>().ok())
            .filter(|port| *port >= 1024)
    };
    let parent = parse(parent_value)?;
    let raw = parse(raw_value)?;
    (parent != raw).then_some((parent, raw))
}

fn host_app_data_root(
    default: PathBuf,
    automation: bool,
    automation_root: Option<OsString>,
) -> PathBuf {
    // Native E2E may exercise durable trust transitions, but must never write
    // them into the user's real host-local authority database. The override is
    // ignored by every ordinary launch and is meaningful only while the host
    // has explicitly enabled one of its debug-only native E2E modes.
    if automation && let Some(root) = automation_root.filter(|root| !root.is_empty()) {
        return PathBuf::from(root);
    }
    default
}

fn initial_capsule_path(
    automation: bool,
    automation_path: Option<OsString>,
    args: impl IntoIterator<Item = OsString>,
) -> Option<PathBuf> {
    // ChromeDriver prepends browser switches before application arguments, and
    // the raw-child CDP harness launches without a file argument. A debug-E2E
    // environment value selects the candidate path without bypassing content
    // inspection, trust, or capability decisions.
    if automation && let Some(path) = automation_path.filter(|path| !path.is_empty()) {
        return Some(PathBuf::from(path));
    }
    args.into_iter().nth(1).map(PathBuf::from)
}

fn inspect_capsule_on_worker_stack(
    path: &Path,
    writer_lock_root: &Path,
) -> Result<(LaunchInspection, Option<RecoveryReport>), String> {
    let path = path.to_owned();
    let writer_lock_root = writer_lock_root.to_owned();
    let worker = std::thread::Builder::new()
        .name("capsule-launch-inspection".to_owned())
        .stack_size(INSPECTION_STACK_BYTES)
        .spawn(move || {
            inspect_launch_with_recovery(&path, &writer_lock_root)
                .map_err(|error| error.to_string())
        })
        .map_err(|error| format!("could not start capsule inspection worker: {error}"))?;
    worker
        .join()
        .map_err(|_| "capsule inspection worker terminated unexpectedly".to_owned())?
}

fn forwarded_capsule_path(args: &[String], cwd: &str) -> Result<Option<PathBuf>, String> {
    let forwarded = args.get(1..).unwrap_or_default();
    if forwarded.is_empty() {
        return Ok(None);
    }
    if forwarded.len() != 1 || forwarded[0].is_empty() || forwarded[0].starts_with('-') {
        return Err("secondary launch must contain exactly one capsule path".to_owned());
    }
    let path = PathBuf::from(&forwarded[0]);
    Ok(Some(if path.is_absolute() {
        path
    } else {
        Path::new(cwd).join(path)
    }))
}

fn schedule_forwarded_launch(app: &AppHandle, args: Vec<String>, cwd: String) {
    let app = app.clone();
    // The Windows single-instance plugin invokes its callback synchronously
    // from WM_COPYDATA. Inspection or WebView event delivery inside that
    // procedure blocks the sender and can suppress the trusted-shell update.
    // A blocking-runtime task lets the native callback return first while the
    // same bounded load path remains authoritative on every platform.
    let task = tauri::async_runtime::spawn_blocking(move || {
        let completion = match forwarded_capsule_path(&args, &cwd) {
            Ok(Some(path)) => Ok(load_host_path_state(&app, &path)),
            Ok(None) => Err((
                "focused".to_owned(),
                "The existing SQLite Capsule Host window is already open.".to_owned(),
            )),
            Err(error) => Err(("error".to_owned(), error)),
        };
        let app_for_main = app.clone();
        if let Err(error) = app.run_on_main_thread(move || {
            // Raise the native shell before emitting the report so the
            // bundled UI's report-specific focus target wins last.
            focus_main_window(&app_for_main);
            match completion {
                Ok(report) => publish_host_report(&app_for_main, &report),
                Err((kind, message)) => emit_host_message(&app_for_main, &kind, message),
            }
        }) {
            eprintln!("could not publish the secondary launch: {error}");
        }
    });
    // Secondary delivery must outlive this synchronous platform callback.
    // Explicitly dropping the join handle documents that the task is detached.
    drop(task);
}

fn load_host_path_state(app: &AppHandle, path: &Path) -> StartupReport {
    match app.try_state::<Mutex<HostState>>() {
        Some(state) => match state.lock() {
            Ok(mut state) => {
                state.load_capsule(path);
                state.report.clone()
            }
            Err(_) => StartupReport {
                stage: "open-rejected".to_owned(),
                capsule: None,
                recovery: None,
                error: Some("host trust state is unavailable".to_owned()),
            },
        },
        None => StartupReport {
            stage: "open-rejected".to_owned(),
            capsule: None,
            recovery: None,
            error: Some("host trust state is not ready".to_owned()),
        },
    }
}

fn reject_host_file_delivery_state(
    app: &AppHandle,
    stage: &str,
    error: impl Into<String>,
) -> StartupReport {
    match app.try_state::<Mutex<HostState>>() {
        Some(state) => match state.lock() {
            Ok(mut state) => {
                state.reject_file_delivery(stage, error);
                state.report.clone()
            }
            Err(_) => StartupReport {
                stage: stage.to_owned(),
                capsule: None,
                recovery: None,
                error: Some("host trust state is unavailable".to_owned()),
            },
        },
        None => StartupReport {
            stage: stage.to_owned(),
            capsule: None,
            recovery: None,
            error: Some("host trust state is not ready".to_owned()),
        },
    }
}

fn publish_host_report(app: &AppHandle, report: &StartupReport) {
    let entry_asset = released_entry_asset(report);
    if let Err(error) = navigate_sandbox(app, entry_asset.as_deref()) {
        eprintln!("failed to apply raw renderer launch state: {error}");
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.emit("host-report", report);
    }
}

fn load_host_path(app: &AppHandle, path: &Path) -> StartupReport {
    let report = load_host_path_state(app, path);
    publish_host_report(app, &report);
    report
}

fn emit_host_message(app: &AppHandle, kind: &str, message: impl Into<String>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.emit(
            "host-message",
            HostMessage {
                kind: kind.to_owned(),
                message: message.into(),
            },
        );
    }
}

fn focus_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

#[tauri::command]
fn startup_report(state: State<'_, Mutex<HostState>>) -> Result<StartupReport, String> {
    state
        .lock()
        .map(|state| state.report.clone())
        .map_err(|_| "host trust state is unavailable".to_owned())
}

#[tauri::command]
fn lifecycle_status(
    state: State<'_, Mutex<HostState>>,
    app: AppHandle,
) -> Result<LifecycleStatus, String> {
    let bridge = state
        .lock()
        .map_err(|_| "host trust state is unavailable".to_owned())?
        .bridge
        .clone();
    let status = lifecycle_status_on_worker(bridge.clone())?;
    let lock_renderer = bridge
        .lock()
        .map_err(|_| "capsule runtime state is unavailable".to_owned())?
        .take_conflict_renderer_lock();
    if lock_renderer {
        navigate_sandbox(&app, None)?;
    }
    Ok(status)
}

fn lifecycle_status_on_worker(
    bridge: Arc<Mutex<RuntimeBridge>>,
) -> Result<LifecycleStatus, String> {
    let worker = std::thread::Builder::new()
        .name("sqlite-capsule-lifecycle-status".to_owned())
        .stack_size(RUNTIME_WORKER_STACK_BYTES)
        .spawn(move || {
            let bridge = bridge
                .lock()
                .map_err(|_| "capsule runtime state is unavailable".to_owned())?;
            lifecycle_status_for(&bridge)
        })
        .map_err(|_| "lifecycle-status worker is unavailable".to_owned())?;
    worker
        .join()
        .map_err(|_| "lifecycle-status worker failed".to_owned())?
}

fn checkpoint_for_close_on_worker(bridge: Arc<Mutex<RuntimeBridge>>) -> Result<(), String> {
    let worker = std::thread::Builder::new()
        .name("sqlite-capsule-close-checkpoint".to_owned())
        .stack_size(RUNTIME_WORKER_STACK_BYTES)
        .spawn(move || {
            let mut bridge = bridge
                .lock()
                .map_err(|_| "capsule runtime state is unavailable".to_owned())?;
            if let Some(runtime) = bridge.runtime.as_mut() {
                runtime
                    .checkpoint_for_close()
                    .map_err(|error| error.to_string())?;
            }
            Ok(())
        })
        .map_err(|_| "close-checkpoint worker is unavailable".to_owned())?;
    worker
        .join()
        .map_err(|_| "close-checkpoint worker failed".to_owned())?
}

fn handle_close_request(app: &AppHandle, api: &tauri::CloseRequestApi) {
    let checkpoint = (|| -> Result<(), String> {
        let state = app.state::<Mutex<HostState>>();
        let state = state
            .lock()
            .map_err(|_| "host trust state is unavailable".to_owned())?;
        let bridge = state.bridge.clone();
        drop(state);
        checkpoint_for_close_on_worker(bridge)
    })();
    match checkpoint {
        Ok(()) => app.exit(0),
        Err(error) => {
            api.prevent_close();
            emit_host_message(
                app,
                "close-error",
                format!("Close was stopped because the final verified checkpoint failed: {error}"),
            );
        }
    }
}

fn prepare_for_host_update_on_worker(
    bridge: Arc<Mutex<RuntimeBridge>>,
) -> Result<UpdatePreflightReport, String> {
    let worker = std::thread::Builder::new()
        .name("sqlite-capsule-update-preflight".to_owned())
        .stack_size(RUNTIME_WORKER_STACK_BYTES)
        .spawn(move || {
            bridge
                .lock()
                .map_err(|_| "capsule runtime state is unavailable".to_owned())?
                .prepare_for_host_update()
        })
        .map_err(|_| "update-preflight worker is unavailable".to_owned())?;
    worker
        .join()
        .map_err(|_| "update-preflight worker failed".to_owned())?
}

fn lifecycle_status_for(bridge: &RuntimeBridge) -> Result<LifecycleStatus, String> {
    let backup_inventory =
        inspect_backup_inventory(&bridge.backup_root).map_err(|error| error.to_string())?;
    Ok(match bridge.runtime.as_ref() {
        Some(runtime) => LifecycleStatus {
            active: true,
            writable: runtime.writable(),
            mode: bridge.mode.clone(),
            backup: runtime
                .backup_record()
                .cloned()
                .or_else(|| bridge.conflict_backup.clone()),
            backup_inventory,
        },
        None => LifecycleStatus {
            active: false,
            writable: false,
            mode: bridge.mode.clone(),
            backup: bridge.conflict_backup.clone(),
            backup_inventory,
        },
    })
}

#[tauri::command]
fn update_status(
    state: State<'_, Mutex<HostState>>,
    flow: State<'_, Mutex<HostUpdateFlow>>,
) -> Result<HostUpdateStatus, String> {
    let update_root = state
        .lock()
        .map_err(|_| "host trust state is unavailable".to_owned())?
        .update_root
        .clone();
    let mut status = update_status_for(&update_root, true);
    let flow = flow
        .lock()
        .map_err(|_| "host-update review state is unavailable".to_owned())?;
    apply_update_flow_status(&mut status, &flow);
    Ok(status)
}

#[tauri::command]
async fn check_host_update(
    app: AppHandle,
    gate: State<'_, UpdateCheckGate>,
    flow: State<'_, Mutex<HostUpdateFlow>>,
) -> Result<UpdateCheckReport, String> {
    let _lease = gate.acquire()?;
    {
        let mut flow = flow
            .lock()
            .map_err(|_| "host-update review state is unavailable".to_owned())?;
        *flow = HostUpdateFlow::Idle;
    }
    let (report, reviewed) = check_host_update_once(&app).await?;
    if let Some(reviewed) = reviewed {
        let mut flow = flow
            .lock()
            .map_err(|_| "host-update review state is unavailable".to_owned())?;
        *flow = HostUpdateFlow::Reviewed(Box::new(reviewed));
    }
    Ok(report)
}

async fn check_host_update_once(
    app: &AppHandle,
) -> Result<(UpdateCheckReport, Option<ReviewedHostUpdate>), String> {
    let configuration = compiled_updater_config()?.ok_or_else(|| {
        "this build has no complete compiled updater trust configuration".to_owned()
    })?;
    let redirect_origin = configuration.endpoint_origin.clone();
    let update = app
        .updater_builder()
        .endpoints(vec![configuration.endpoint.clone()])
        .map_err(|error| error.to_string())?
        .target(host_release_target())
        .pubkey(configuration.public_key.clone())
        .timeout(std::time::Duration::from_secs(30))
        .configure_client(move |builder| {
            builder.redirect(restricted_redirect_policy(redirect_origin.clone()))
        })
        .build()
        .map_err(|error| error.to_string())?
        .check()
        .await
        .map_err(|_| "host-update endpoint check failed closed".to_owned())?;
    Ok(match update {
        Some(update) => {
            let reviewed = review_transport_update(update, &configuration, current_unix_i64()?)?;
            let report = UpdateCheckReport {
                available: true,
                current_version: reviewed.transport.current_version.clone(),
                candidate_version: Some(reviewed.candidate.version().to_owned()),
                candidate_sequence: Some(reviewed.candidate.sequence()),
                target: Some(reviewed.candidate.artifact().target.clone()),
                transport_endpoint_origin: configuration.endpoint_origin,
                release_policy_verified: true,
                downloaded: false,
                install_authorized: false,
            };
            (report, Some(reviewed))
        }
        None => (
            UpdateCheckReport {
                available: false,
                current_version: env!("CARGO_PKG_VERSION").to_owned(),
                candidate_version: None,
                candidate_sequence: None,
                target: None,
                transport_endpoint_origin: configuration.endpoint_origin,
                release_policy_verified: false,
                downloaded: false,
                install_authorized: false,
            },
            None,
        ),
    })
}

fn review_transport_update(
    transport: TransportUpdate,
    configuration: &CompiledUpdaterConfig,
    now_unix: i64,
) -> Result<ReviewedHostUpdate, String> {
    let (candidate, sigstore_bundle_url) = verify_update_announcement(
        &transport.raw_json,
        &transport.current_version,
        &transport.version,
        &transport.target,
        &transport.download_url,
        transport.signature.len(),
        configuration,
        now_unix,
    )?;
    Ok(ReviewedHostUpdate {
        transport,
        candidate,
        sigstore_bundle_url,
    })
}

#[allow(clippy::too_many_arguments)]
fn verify_update_announcement(
    raw_json: &Value,
    current_version: &str,
    announced_version: &str,
    announced_target: &str,
    announced_download_url: &tauri::Url,
    announced_signature_bytes: usize,
    configuration: &CompiledUpdaterConfig,
    now_unix: i64,
) -> Result<(VerifiedReleaseCandidate, tauri::Url), String> {
    let metadata = raw_json.get("sqlite_capsule").cloned().ok_or_else(|| {
        "update announcement lacks SQLite Capsule signed release metadata".to_owned()
    })?;
    let metadata: CapsuleUpdateMetadata = serde_json::from_value(metadata).map_err(|_| {
        "SQLite Capsule release metadata is malformed or has unknown fields".to_owned()
    })?;
    let allowed_host = configuration
        .endpoint
        .host_str()
        .ok_or_else(|| "compiled update origin is invalid".to_owned())?;
    let candidate = verify_release_candidate(
        &metadata.signed_release,
        &configuration.release_public_key,
        &ReleaseCandidateContext {
            current_version,
            current_sequence: configuration.current_release_sequence,
            target: host_release_target(),
            allowed_hosts: &[allowed_host],
            now_unix,
        },
    )
    .map_err(|error| format!("signed host-release policy rejected the announcement: {error}"))?;
    if candidate.version() != announced_version
        || candidate.artifact().target != announced_target
        || candidate.artifact().platform_signing != expected_platform_signing()
    {
        return Err("updater announcement does not match the signed release candidate".to_owned());
    }
    let artifact_url = candidate
        .artifact()
        .url
        .parse::<tauri::Url>()
        .map_err(|_| "signed host artifact URL is invalid".to_owned())?;
    if artifact_url != *announced_download_url
        || https_origin(&artifact_url).as_deref() != Some(configuration.endpoint_origin.as_str())
    {
        return Err("updater artifact URL is not the exact signed same-origin URL".to_owned());
    }
    if announced_signature_bytes == 0 || announced_signature_bytes > 128 * 1024 {
        return Err("updater signature metadata is oversized".to_owned());
    }
    if metadata.sigstore_bundle_url.len() > 4 * 1024 {
        return Err("Sigstore bundle URL is oversized".to_owned());
    }
    let sigstore_bundle_url = metadata
        .sigstore_bundle_url
        .parse::<tauri::Url>()
        .map_err(|_| "Sigstore bundle URL is invalid".to_owned())?;
    if https_origin(&sigstore_bundle_url).as_deref() != Some(configuration.endpoint_origin.as_str())
    {
        return Err("Sigstore bundle URL is outside the compiled update origin".to_owned());
    }
    Ok((candidate, sigstore_bundle_url))
}

#[tauri::command]
async fn download_host_update(
    request: DownloadUpdateRequest,
    gate: State<'_, UpdateCheckGate>,
    flow: State<'_, Mutex<HostUpdateFlow>>,
    app: AppHandle,
) -> Result<UpdateDownloadReport, String> {
    let _lease = gate.acquire()?;
    if request.confirmation != "DOWNLOAD HOST UPDATE" {
        return Err("explicit host-update download confirmation is required".to_owned());
    }
    let reviewed = {
        let mut state = flow
            .lock()
            .map_err(|_| "host-update review state is unavailable".to_owned())?;
        let previous = std::mem::replace(
            &mut *state,
            HostUpdateFlow::Busy(BusyHostUpdate {
                candidate_version: request.candidate_version.clone(),
            }),
        );
        match previous {
            HostUpdateFlow::Reviewed(reviewed)
                if reviewed.candidate.version() == request.candidate_version =>
            {
                reviewed
            }
            other => {
                *state = other;
                return Err("the confirmed download is not the reviewed candidate".to_owned());
            }
        }
    };

    let configuration = match compiled_updater_config()? {
        Some(configuration) => configuration,
        None => {
            let mut state = flow
                .lock()
                .map_err(|_| "host-update review state is unavailable".to_owned())?;
            *state = HostUpdateFlow::Reviewed(reviewed);
            return Err(
                "this build has no complete compiled updater trust configuration".to_owned(),
            );
        }
    };
    let result = download_reviewed_update(&reviewed, &configuration, &app).await;
    match result {
        Ok((artifact_bytes, sigstore_bundle_bytes, accepted)) => {
            let report = UpdateDownloadReport {
                candidate_version: reviewed.candidate.version().to_owned(),
                candidate_sequence: reviewed.candidate.sequence(),
                target: reviewed.candidate.artifact().target.clone(),
                artifact_bytes: artifact_bytes.len() as u64,
                sigstore_bundle_bytes: sigstore_bundle_bytes.len() as u64,
                updater_signature_verified: true,
                release_digest_verified: true,
                sigstore_digest_matched: true,
                platform_signature_verified: true,
                sigstore_signature_verified: true,
                install_authorized: false,
            };
            let mut state = flow
                .lock()
                .map_err(|_| "host-update review state is unavailable".to_owned())?;
            *state = HostUpdateFlow::Downloaded(Box::new(DownloadedHostUpdate {
                reviewed,
                artifact_bytes,
                sigstore_bundle_bytes,
                accepted,
            }));
            Ok(report)
        }
        Err(error) => {
            let mut state = flow
                .lock()
                .map_err(|_| "host-update review state is unavailable".to_owned())?;
            *state = HostUpdateFlow::Reviewed(reviewed);
            Err(error)
        }
    }
}

/// Trusted-shell boundary between an accepted in-memory download and durable
/// staging. This command performs no installer execution: it records explicit
/// install intent, establishes the capsule recovery point, closes the capsule
/// session, mints the opaque installable state, and persists the exact bytes.
#[tauri::command]
fn stage_host_update(
    request: StageDownloadedUpdateRequest,
    gate: State<'_, UpdateCheckGate>,
    flow: State<'_, Mutex<HostUpdateFlow>>,
    state: State<'_, Mutex<HostState>>,
    app: AppHandle,
) -> Result<StageDownloadedUpdateResponse, String> {
    if request.confirmation != "INSTALL HOST UPDATE" {
        return Err("explicit host-update install confirmation is required".to_owned());
    }
    let _lease = gate.acquire()?;
    if native_e2e_update_preflight_requested_from_process() {
        let bridge = state
            .lock()
            .map_err(|_| "host trust state is unavailable".to_owned())?
            .bridge
            .clone();
        prepare_for_host_update_on_worker(bridge)?;
        return Err("native E2E update-preflight fault did not terminate the host".to_owned());
    }
    let downloaded = {
        let mut flow = flow
            .lock()
            .map_err(|_| "host-update review state is unavailable".to_owned())?;
        let previous = std::mem::replace(
            &mut *flow,
            HostUpdateFlow::Busy(BusyHostUpdate {
                candidate_version: request.candidate_version.clone(),
            }),
        );
        match previous {
            HostUpdateFlow::Downloaded(downloaded)
                if downloaded.reviewed.candidate.version() == request.candidate_version =>
            {
                downloaded
            }
            other => {
                *flow = other;
                return Err("the install confirmation is not for the accepted download".to_owned());
            }
        }
    };

    let result = stage_accepted_download(&downloaded, &state);
    match result {
        Ok(response) => {
            let mut flow = flow
                .lock()
                .map_err(|_| "host-update review state is unavailable".to_owned())?;
            *flow = HostUpdateFlow::Idle;
            drop(flow);
            navigate_sandbox(&app, None)?;
            Ok(response)
        }
        Err(error) => {
            let navigation = if error.session_quiesced {
                navigate_sandbox(&app, None)
            } else {
                Ok(())
            };
            let mut flow = flow
                .lock()
                .map_err(|_| "host-update review state is unavailable".to_owned())?;
            *flow = HostUpdateFlow::Downloaded(downloaded);
            match navigation {
                Ok(()) => Err(error.message),
                Err(navigation) => Err(format!("{}; {navigation}", error.message)),
            }
        }
    }
}

fn stage_accepted_download(
    downloaded: &DownloadedHostUpdate,
    state: &Mutex<HostState>,
) -> Result<StageDownloadedUpdateResponse, StageDownloadedUpdateError> {
    let state = state.lock().map_err(|_| {
        StageDownloadedUpdateError::before_preflight("host trust state is unavailable")
    })?;
    let stager = UpdateStager::open(&state.update_root)
        .map_err(|error| StageDownloadedUpdateError::before_preflight(error.to_string()))?;
    let inventory = stager
        .inventory()
        .map_err(|error| StageDownloadedUpdateError::before_preflight(error.to_string()))?;
    if !inventory.incomplete.is_empty()
        || !inventory.invalid.is_empty()
        || inventory.verified.iter().any(|stage| {
            matches!(
                stage.state,
                UpdateStageState::Prepared
                    | UpdateStageState::InstallerStarted
                    | UpdateStageState::AwaitingHealth
                    | UpdateStageState::RollbackRequired
                    | UpdateStageState::RollbackStarted
                    | UpdateStageState::AwaitingRollbackHealth
                    | UpdateStageState::RollbackFailed
            )
        })
    {
        return Err(StageDownloadedUpdateError::before_preflight(
            "host-update inventory must be clean and have no active or rollback stage",
        ));
    }
    let configuration = compiled_updater_config()
        .map_err(StageDownloadedUpdateError::before_preflight)?
        .ok_or_else(|| {
            StageDownloadedUpdateError::before_preflight(
                "host-update transport and release root are not configured",
            )
        })?;
    let allowed_host = configuration
        .endpoint
        .host_str()
        .ok_or_else(|| {
            StageDownloadedUpdateError::before_preflight(
                "compiled updater endpoint has no permitted host",
            )
        })?
        .to_owned();
    let allowed_hosts = [allowed_host.as_str()];
    let healthy_installer = stager
        .installed_installer_source(
            &configuration.release_public_key,
            &InstalledReleaseContext {
                version: env!("CARGO_PKG_VERSION"),
                sequence: configuration.current_release_sequence,
                target: host_release_target(),
                allowed_hosts: &allowed_hosts,
            },
        )
        .map_err(|error| {
            StageDownloadedUpdateError::before_preflight(format!(
                "current host installer failed signed provenance verification: {error}"
            ))
        })?;
    let platform = downloaded.accepted.candidate().artifact();
    let previous_installer = match healthy_installer {
        Some(source) => lock_installer_source(
            source.path(),
            env!("CARGO_PKG_VERSION"),
            &platform.platform_signing,
            &platform.platform_signing_identity,
            platform.platform_timestamp_required,
        )
        .map_err(|error| {
            StageDownloadedUpdateError::before_preflight(format!(
                "healthy current installer failed locked native verification: {error}"
            ))
        })?,
        None => {
            let current_executable = std::env::current_exe().map_err(|error| {
                StageDownloadedUpdateError::before_preflight(format!(
                    "current host executable path is unavailable: {error}"
                ))
            })?;
            let cache_directory = current_executable
                .parent()
                .ok_or_else(|| {
                    StageDownloadedUpdateError::before_preflight(
                        "current host executable has no installer-cache parent",
                    )
                })?
                .join("installer-cache");
            discover_bootstrap_installer(
                &cache_directory,
                env!("CARGO_PKG_VERSION"),
                &platform.platform_signing,
                &platform.platform_signing_identity,
                platform.platform_timestamp_required,
            )
            .map_err(|error| {
                StageDownloadedUpdateError::before_preflight(format!(
                    "bootstrap installer failed signed version verification: {error}"
                ))
            })?
            .ok_or_else(|| {
                StageDownloadedUpdateError::before_preflight(
                    "no verified current installer is available for rollback",
                )
            })?
        }
    };
    let preflight = prepare_for_host_update_on_worker(state.bridge.clone())
        .map_err(StageDownloadedUpdateError::before_preflight)?;
    let verified_backup_complete =
        !preflight.writable_session || preflight.verified_backup.is_some();
    let installable = authorize_installable_update(
        downloaded.accepted.clone(),
        UpdateAuthorization {
            user_consented: true,
            sessions_quiesced: preflight.session_quiesced,
            verified_backup_complete,
        },
    )
    .map_err(|error| {
        StageDownloadedUpdateError::after_preflight(format!(
            "host-update installation authorization failed: {error}"
        ))
    })?;
    let extension = platform_artifact_extension(
        &installable.artifact().platform_signing,
        &installable.artifact().url,
    )
    .map_err(StageDownloadedUpdateError::after_preflight)?;
    let artifact_name = format!("sqlite-capsule-host-{}{}", installable.version(), extension);
    let sigstore_name = format!(
        "sqlite-capsule-host-{}.sigstore.json",
        installable.version()
    );
    let staged = stager
        .stage_verified(
            &installable,
            StageRequest {
                artifact_name: &artifact_name,
                artifact_bytes: &downloaded.artifact_bytes,
                sigstore_name: &sigstore_name,
                sigstore_bytes: &downloaded.sigstore_bundle_bytes,
                previous_installer: Some(PreviousInstaller {
                    path: previous_installer.path(),
                    staged_name: previous_installer.staged_name(),
                    version: env!("CARGO_PKG_VERSION"),
                }),
            },
        )
        .map_err(|error| {
            StageDownloadedUpdateError::after_preflight(format!(
                "host-update staging failed: {error}"
            ))
        })?;
    Ok(StageDownloadedUpdateResponse {
        stage_id: staged.record.stage_id,
        candidate_version: staged.record.version,
        artifact_name,
        sigstore_name,
        install_authorized: true,
        staged: true,
        installed: false,
        preflight,
    })
}

async fn download_reviewed_update(
    reviewed: &ReviewedHostUpdate,
    configuration: &CompiledUpdaterConfig,
    app: &AppHandle,
) -> Result<(Vec<u8>, Vec<u8>, VerifiedDownloadedUpdate), String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .redirect(restricted_redirect_policy(
            configuration.endpoint_origin.clone(),
        ))
        .build()
        .map_err(|_| "could not construct the bounded host-update downloader".to_owned())?;
    let artifact_bytes = download_bounded(
        &client,
        &reviewed.transport.download_url,
        reviewed.candidate.artifact().bytes,
        Some(reviewed.candidate.artifact().bytes),
    )
    .await?;
    verify_updater_minisign(
        &artifact_bytes,
        &reviewed.transport.signature,
        &configuration.public_key,
    )?;
    verify_candidate_artifact_bytes(&reviewed.candidate, &artifact_bytes).map_err(|error| {
        format!("downloaded host artifact failed signed digest policy: {error}")
    })?;
    let platform_verification =
        verify_downloaded_platform_artifact(app, &reviewed.candidate, &artifact_bytes)?;
    let sigstore_bundle_bytes = download_bounded(
        &client,
        &reviewed.sigstore_bundle_url,
        MAX_SIGSTORE_BUNDLE_BYTES,
        None,
    )
    .await?;
    verify_candidate_sigstore_bundle_bytes(&reviewed.candidate, &sigstore_bundle_bytes)
        .map_err(|error| format!("Sigstore evidence failed its signed digest policy: {error}"))?;
    let sigstore_verification = verify_sigstore_bundle(
        &artifact_bytes,
        &sigstore_bundle_bytes,
        &reviewed.candidate.artifact().sigstore_certificate_identity,
        &reviewed.candidate.artifact().sigstore_oidc_issuer,
    )
    .map_err(|error| format!("Sigstore acceptance failed: {error}"))?;
    let accepted = accept_downloaded_update(
        reviewed.candidate.clone(),
        &artifact_bytes,
        &sigstore_bundle_bytes,
        platform_verification,
        sigstore_verification,
    )
    .map_err(|error| format!("downloaded update evidence did not bind to one artifact: {error}"))?;
    Ok((artifact_bytes, sigstore_bundle_bytes, accepted))
}

fn verify_downloaded_platform_artifact(
    app: &AppHandle,
    candidate: &VerifiedReleaseCandidate,
    bytes: &[u8],
) -> Result<PlatformVerificationReport, String> {
    let artifact = candidate.artifact();
    let extension = platform_artifact_extension(&artifact.platform_signing, &artifact.url)?;
    let root = app
        .path()
        .app_data_dir()
        .map_err(|_| "host update verification directory is unavailable".to_owned())?
        .join("update-verification");
    prepare_private_directory(&root).map_err(|error| {
        format!("could not protect the host update verification directory: {error}")
    })?;
    let token = generate_session_token()?;
    let path = root.join(format!("candidate-{token}{extension}"));
    let verification = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| "could not create a private update-verification file".to_owned())?;
        protect_private_file(&path)
            .map_err(|error| format!("could not protect the update-verification file: {error}"))?;
        file.write_all(bytes)
            .map_err(|_| "could not materialize the update for platform verification".to_owned())?;
        file.sync_all()
            .map_err(|_| "could not sync the update-verification file".to_owned())?;
        drop(file);
        verify_platform_artifact(
            &path,
            &artifact.platform_signing,
            &artifact.platform_signing_identity,
            artifact.platform_timestamp_required,
        )
        .map_err(|error| format!("platform package-signature verification failed: {error}"))
    })();
    let cleanup = if path.exists() {
        std::fs::remove_file(&path)
            .map_err(|_| "could not remove the private update-verification file".to_owned())
    } else {
        Ok(())
    };
    match (verification, cleanup) {
        (Ok(report), Ok(())) => Ok(report),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(cleanup)) => Err(cleanup),
        (Err(error), Err(cleanup)) => Err(format!("{error}; {cleanup}")),
    }
}

fn platform_artifact_extension(
    platform_signing: &str,
    artifact_url: &str,
) -> Result<&'static str, String> {
    match platform_signing {
        "authenticode" if artifact_url.ends_with(".msi") => Ok(".msi"),
        "authenticode" if artifact_url.ends_with(".exe") => Ok(".exe"),
        "developer-id-notarized" if artifact_url.ends_with(".pkg") => Ok(".pkg"),
        "linux-detached" if artifact_url.ends_with(".deb") => Ok(".deb"),
        "linux-detached" if artifact_url.ends_with(".rpm") => Ok(".rpm"),
        "linux-detached" if artifact_url.ends_with(".AppImage") => Ok(".AppImage"),
        _ => {
            Err("signed host artifact suffix does not match its platform-signing class".to_owned())
        }
    }
}

async fn download_bounded(
    client: &reqwest::Client,
    url: &tauri::Url,
    maximum_bytes: u64,
    exact_bytes: Option<u64>,
) -> Result<Vec<u8>, String> {
    let mut response = client
        .get(url.clone())
        .header(reqwest::header::ACCEPT, "application/octet-stream")
        .send()
        .await
        .map_err(|_| "host-update download failed closed".to_owned())?;
    if !response.status().is_success() {
        return Err(format!(
            "host-update download returned HTTP {}",
            response.status().as_u16()
        ));
    }
    if response.content_length().is_some_and(|length| {
        length > maximum_bytes || exact_bytes.is_some_and(|exact| length != exact)
    }) {
        return Err("host-update download length is outside the signed bound".to_owned());
    }
    let capacity = exact_bytes
        .unwrap_or(maximum_bytes.min(1024 * 1024))
        .min(usize::MAX as u64) as usize;
    let mut bytes = Vec::with_capacity(capacity);
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "host-update download stream was interrupted".to_owned())?
    {
        if (bytes.len() as u64).saturating_add(chunk.len() as u64) > maximum_bytes {
            return Err("host-update download exceeded its signed bound".to_owned());
        }
        bytes.extend_from_slice(&chunk);
    }
    if exact_bytes.is_some_and(|exact| bytes.len() as u64 != exact) {
        return Err("host-update download is partial".to_owned());
    }
    Ok(bytes)
}

fn verify_updater_minisign(
    artifact_bytes: &[u8],
    encoded_signature: &str,
    encoded_public_key: &str,
) -> Result<(), String> {
    let signature = decode_base64_utf8(encoded_signature, 128 * 1024, "updater signature")?;
    let public_key = decode_base64_utf8(encoded_public_key, 16 * 1024, "updater public key")?;
    let signature = MinisignSignature::decode(&signature)
        .map_err(|_| "updater signature encoding is invalid".to_owned())?;
    let public_key = MinisignPublicKey::decode(&public_key)
        .map_err(|_| "compiled updater public key encoding is invalid".to_owned())?;
    public_key
        .verify(artifact_bytes, &signature, true)
        .map_err(|_| "downloaded host artifact has an invalid updater signature".to_owned())
}

fn decode_base64_utf8(
    value: &str,
    maximum_encoded_bytes: usize,
    label: &str,
) -> Result<String, String> {
    if value.is_empty() || value.len() > maximum_encoded_bytes {
        return Err(format!("{label} is empty or oversized"));
    }
    let bytes = BASE64_STANDARD
        .decode(value)
        .map_err(|_| format!("{label} is not valid base64"))?;
    String::from_utf8(bytes).map_err(|_| format!("{label} is not UTF-8"))
}

fn current_unix_i64() -> Result<i64, String> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "system clock is before the Unix epoch".to_owned())?
        .as_secs();
    i64::try_from(seconds).map_err(|_| "system clock is outside the signed-policy range".to_owned())
}

/// Internal install-orchestration boundary. It is registered so native update
/// code has one audited path, but is intentionally absent from the bundled
/// webview capability until the complete installer-launch flow is enabled.
#[tauri::command]
fn prepare_update_installation(
    request: PrepareUpdateRequest,
    state: State<'_, Mutex<HostState>>,
    app: AppHandle,
) -> Result<PrepareUpdateResponse, String> {
    let state = state
        .lock()
        .map_err(|_| "host trust state is unavailable".to_owned())?;
    let stager = UpdateStager::open(&state.update_root).map_err(|error| error.to_string())?;
    let inventory = stager.inventory().map_err(|error| error.to_string())?;
    let selected =
        select_confirmed_prepared_update(&inventory, &request.stage_id, &request.confirmation)?;
    let prepared = prepare_persisted_installation(&stager, &selected.record.stage_id)?;
    let candidate_version = prepared.candidate().version().to_owned();
    let preflight = prepare_for_host_update_on_worker(state.bridge.clone())?;
    drop(state);
    navigate_sandbox(&app, None)?;
    Ok(PrepareUpdateResponse {
        stage_id: request.stage_id,
        candidate_version,
        preflight,
    })
}

/// Registered host-only execution boundary. It remains absent from the trusted
/// shell capability until prior-installer discovery and live signed-package
/// acceptance are complete. Even native callers must supply a second exact
/// confirmation and a platform-verifiable preserved rollback installer.
#[tauri::command]
fn execute_update_installation(
    request: ExecuteUpdateRequest,
    state: State<'_, Mutex<HostState>>,
    app: AppHandle,
) -> Result<ExecuteUpdateResponse, String> {
    require_execute_confirmation(&request.confirmation)?;
    if !cfg!(windows) {
        return Err(
            "host-update installer execution is not implemented on this platform".to_owned(),
        );
    }
    let state = state
        .lock()
        .map_err(|_| "host trust state is unavailable".to_owned())?;
    let stager = UpdateStager::open(&state.update_root).map_err(|error| error.to_string())?;
    let inventory = stager.inventory().map_err(|error| error.to_string())?;
    let selected =
        select_confirmed_prepared_update(&inventory, &request.stage_id, "INSTALL HOST UPDATE")?;
    let prepared = prepare_persisted_installation(&stager, &selected.record.stage_id)?;
    if prepared.previous_installer_path().is_none() {
        return Err(
            "host-update launch requires a preserved rollback installer before capsule preflight"
                .to_owned(),
        );
    }
    let candidate_version = prepared.candidate().version().to_owned();
    let preflight = prepare_for_host_update_on_worker(state.bridge.clone())?;
    drop(state);
    navigate_sandbox(&app, None)?;
    let receipt = launch_prepared(&stager, prepared, current_unix_u64()?)
        .map_err(|error| format!("verified host installer was not launched: {error}"))?;
    let response = ExecuteUpdateResponse {
        stage_id: receipt.stage_id,
        candidate_version,
        state: receipt.state,
        installer_launched: true,
        preflight,
    };
    app.exit(0);
    Ok(response)
}

/// Registered host-only rollback boundary. It is deliberately absent from the
/// trusted-shell capability until a production-signed clean-machine acceptance
/// run proves installer downgrade and startup-health behavior.
#[tauri::command]
fn execute_update_rollback(
    request: ExecuteRollbackRequest,
    state: State<'_, Mutex<HostState>>,
    app: AppHandle,
) -> Result<ExecuteRollbackResponse, String> {
    require_rollback_confirmation(&request.confirmation)?;
    if !cfg!(windows) {
        return Err("host-update rollback is not implemented on this platform".to_owned());
    }
    let state = state
        .lock()
        .map_err(|_| "host trust state is unavailable".to_owned())?;
    let stager = UpdateStager::open(&state.update_root).map_err(|error| error.to_string())?;
    let inventory = stager.inventory().map_err(|error| error.to_string())?;
    select_confirmed_rollback(&inventory, &request.stage_id)?;
    let prepared = stager
        .prepare_rollback(&request.stage_id, env!("CARGO_PKG_VERSION"))
        .map_err(|error| format!("verified rollback preparation failed: {error}"))?;
    let preflight = prepare_for_host_update_on_worker(state.bridge.clone())?;
    drop(state);
    navigate_sandbox(&app, None)?;
    let receipt = launch_rollback(&stager, prepared, current_unix_u64()?)
        .map_err(|error| format!("verified rollback installer was not launched: {error}"))?;
    let response = ExecuteRollbackResponse {
        stage_id: receipt.stage_id,
        rollback_version: receipt.version,
        state: receipt.state,
        installer_launched: true,
        preflight,
    };
    app.exit(0);
    Ok(response)
}

fn prepare_persisted_installation(
    stager: &UpdateStager,
    stage_id: &str,
) -> Result<PreparedInstallation, String> {
    let configuration = compiled_updater_config()?
        .ok_or_else(|| "host-update transport and release root are not configured".to_owned())?;
    let allowed_host = configuration
        .endpoint
        .host_str()
        .ok_or_else(|| "compiled updater endpoint has no permitted host".to_owned())?
        .to_owned();
    let allowed_hosts = [allowed_host.as_str()];
    let candidate_context = ReleaseCandidateContext {
        current_version: env!("CARGO_PKG_VERSION"),
        current_sequence: configuration.current_release_sequence,
        target: host_release_target(),
        allowed_hosts: &allowed_hosts,
        now_unix: current_unix_i64()?,
    };
    stager
        .prepare_installation(
            stage_id,
            &configuration.release_public_key,
            &candidate_context,
        )
        .map_err(|error| format!("prepared host update failed signed-policy revalidation: {error}"))
}

fn require_execute_confirmation(confirmation: &str) -> Result<(), String> {
    if confirmation != "RUN VERIFIED HOST INSTALLER" {
        return Err("explicit verified-installer execution confirmation is required".to_owned());
    }
    Ok(())
}

fn require_rollback_confirmation(confirmation: &str) -> Result<(), String> {
    if confirmation != "RUN VERIFIED HOST ROLLBACK" {
        return Err("explicit verified-installer rollback confirmation is required".to_owned());
    }
    Ok(())
}

fn current_unix_u64() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| "system clock precedes the Unix epoch".to_owned())
}

fn select_confirmed_prepared_update<'a>(
    inventory: &'a UpdateInventoryReport,
    stage_id: &str,
    confirmation: &str,
) -> Result<&'a StagedUpdate, String> {
    if confirmation != "INSTALL HOST UPDATE" {
        return Err("explicit host-update confirmation is required".to_owned());
    }
    if !inventory.incomplete.is_empty() || !inventory.invalid.is_empty() {
        return Err("host-update inventory contains incomplete or invalid evidence".to_owned());
    }
    let active = inventory
        .verified
        .iter()
        .filter(|stage| {
            matches!(
                stage.state,
                UpdateStageState::Prepared
                    | UpdateStageState::InstallerStarted
                    | UpdateStageState::AwaitingHealth
            )
        })
        .collect::<Vec<_>>();
    if active.len() != 1
        || active[0].record.stage_id != stage_id
        || active[0].state != UpdateStageState::Prepared
    {
        return Err("the confirmed update is not the one prepared candidate".to_owned());
    }
    Ok(active[0])
}

fn select_confirmed_rollback<'a>(
    inventory: &'a UpdateInventoryReport,
    stage_id: &str,
) -> Result<&'a StagedUpdate, String> {
    if !inventory.incomplete.is_empty() || !inventory.invalid.is_empty() {
        return Err("host-update inventory contains incomplete or invalid evidence".to_owned());
    }
    let rollback = inventory
        .verified
        .iter()
        .filter(|stage| stage.state == UpdateStageState::RollbackRequired)
        .collect::<Vec<_>>();
    if rollback.len() != 1 || rollback[0].record.stage_id != stage_id {
        return Err("the confirmed rollback is not the one rollback-required stage".to_owned());
    }
    Ok(rollback[0])
}

fn update_status_for(root: &Path, reconcile: bool) -> HostUpdateStatus {
    let transport = compiled_updater_config();
    let mut status = HostUpdateStatus {
        current_version: env!("CARGO_PKG_VERSION").to_owned(),
        transport_configured: transport
            .as_ref()
            .is_ok_and(|configuration| configuration.is_some()),
        transport_endpoint_origin: transport
            .as_ref()
            .ok()
            .and_then(|configuration| configuration.as_ref())
            .map(|configuration| configuration.endpoint_origin.clone()),
        review_state: "idle".to_owned(),
        stage_id: None,
        candidate_version: None,
        candidate_sequence: None,
        release_policy_verified: false,
        downloaded: false,
        downloaded_artifact_bytes: None,
        downloaded_sigstore_bundle_bytes: None,
        platform_signature_verified: false,
        platform_signer_subject: None,
        sigstore_signature_verified: false,
        sigstore_certificate_identity: None,
        sigstore_integrated_time_unix: None,
        state: None,
        rollback_available: false,
        incomplete_artifacts: Vec::new(),
        invalid_artifacts: Vec::new(),
        error: transport.err(),
    };
    let stager = match UpdateStager::open(root) {
        Ok(stager) => stager,
        Err(error) => {
            status.error = Some(error.to_string());
            return status;
        }
    };
    if reconcile {
        let now_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs());
        match now_unix {
            Ok(now_unix) => {
                if let Err(error) = stager.reconcile_startup(env!("CARGO_PKG_VERSION"), now_unix) {
                    status.error = Some(error.to_string());
                }
            }
            Err(_) => status.error = Some("system clock precedes the Unix epoch".to_owned()),
        }
    }
    match stager.inventory() {
        Ok(inventory) => {
            status.incomplete_artifacts = inventory.incomplete;
            status.invalid_artifacts = inventory.invalid;
            if let Some(stage) = inventory
                .verified
                .into_iter()
                .max_by_key(|stage| stage.record.sequence)
            {
                status.rollback_available = stage.state == UpdateStageState::RollbackRequired
                    && stage.record.previous_installer_name.is_some();
                status.stage_id = Some(stage.record.stage_id);
                status.candidate_version = Some(stage.record.version);
                status.candidate_sequence = Some(stage.record.sequence);
                status.state = Some(stage.state);
            }
        }
        Err(error) => status.error = Some(error.to_string()),
    }
    status
}

fn apply_update_flow_status(status: &mut HostUpdateStatus, flow: &HostUpdateFlow) {
    match flow {
        HostUpdateFlow::Idle => {}
        HostUpdateFlow::Reviewed(reviewed) => {
            status.review_state = "reviewed".to_owned();
            status.candidate_version = Some(reviewed.candidate.version().to_owned());
            status.candidate_sequence = Some(reviewed.candidate.sequence());
            status.release_policy_verified = true;
        }
        HostUpdateFlow::Busy(busy) => {
            status.review_state = "busy".to_owned();
            status.candidate_version = Some(busy.candidate_version.clone());
        }
        HostUpdateFlow::Downloaded(downloaded) => {
            status.review_state = "downloaded".to_owned();
            status.candidate_version = Some(downloaded.reviewed.candidate.version().to_owned());
            status.candidate_sequence = Some(downloaded.reviewed.candidate.sequence());
            status.release_policy_verified = true;
            status.downloaded = true;
            status.downloaded_artifact_bytes = Some(downloaded.artifact_bytes.len() as u64);
            status.downloaded_sigstore_bundle_bytes =
                Some(downloaded.sigstore_bundle_bytes.len() as u64);
            status.platform_signature_verified = true;
            status.platform_signer_subject = Some(
                downloaded
                    .accepted
                    .platform_verification()
                    .signer_subject()
                    .to_owned(),
            );
            status.sigstore_signature_verified = true;
            status.sigstore_certificate_identity = Some(
                downloaded
                    .accepted
                    .sigstore_verification()
                    .certificate_identity()
                    .to_owned(),
            );
            status.sigstore_integrated_time_unix = Some(
                downloaded
                    .accepted
                    .sigstore_verification()
                    .integrated_time_unix(),
            );
        }
    }
}

fn collect_support_bundle(state: &HostState) -> Result<SupportBundle, String> {
    let mut startup = state.report.clone();
    let mut redactions = vec![
        state.trust_backup_directory.clone(),
        state.update_root.clone(),
    ];
    if let Some(capsule) = startup.capsule.as_mut() {
        redactions.push(capsule.identity.canonical_path.clone());
        capsule.identity.canonical_path = PathBuf::from("redacted");
    }
    let bridge = state
        .bridge
        .lock()
        .map_err(|_| "capsule runtime state is unavailable".to_owned())?;
    let lifecycle = lifecycle_status_for(&bridge)?;
    redactions.push(bridge.writer_lock_root.clone());
    redactions.push(bridge.backup_root.clone());
    drop(bridge);
    let update = update_status_for(&state.update_root, false);
    let trust = state
        .trust_store
        .as_ref()
        .ok_or_else(|| "protected trust store is unavailable".to_owned())?
        .export_redacted()
        .map_err(|error| error.to_string())?;
    let created_at_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "system clock precedes the Unix epoch".to_owned())?
        .as_secs();
    Ok(SupportBundle {
        format: "org.sqlite-capsule.support-bundle/0.2".to_owned(),
        created_at_unix,
        host_version: env!("CARGO_PKG_VERSION").to_owned(),
        platform: std::env::consts::OS.to_owned(),
        architecture: std::env::consts::ARCH.to_owned(),
        content_policy: SupportBundleContentPolicy {
            capsule_controlled_text: "untrusted-data-only".to_owned(),
            host_severity_source: "host-owned-structured-fields-only".to_owned(),
            embedded_instructions_executed: false,
            capsule_database_bytes_included: false,
            trust_store_bytes_included: false,
            selected_file_contents_included: false,
            shutdown_tokens_included: false,
            private_keys_included: false,
        },
        startup,
        lifecycle,
        update,
        trust,
        redactions: support_redactions(redactions),
    })
}

fn collect_support_bundle_on_worker(state: &Mutex<HostState>) -> Result<SupportBundle, String> {
    std::thread::scope(|scope| {
        let worker = std::thread::Builder::new()
            .name("sqlite-capsule-support-collect".to_owned())
            .stack_size(RUNTIME_WORKER_STACK_BYTES)
            .spawn_scoped(scope, move || {
                let state = state
                    .lock()
                    .map_err(|_| "host trust state is unavailable".to_owned())?;
                collect_support_bundle(&state)
            })
            .map_err(|_| "support-collection worker is unavailable".to_owned())?;
        worker
            .join()
            .map_err(|_| "support-collection worker failed".to_owned())?
    })
}

fn support_redactions(paths: Vec<PathBuf>) -> Vec<String> {
    let mut redactions = BTreeSet::new();
    for path in paths {
        for candidate in [Some(path.as_path()), path.parent()].into_iter().flatten() {
            let text = candidate.display().to_string();
            if text.len() > 3 {
                redactions.insert(text);
            }
        }
    }
    let mut redactions: Vec<_> = redactions.into_iter().collect();
    redactions.sort_by_key(|value| std::cmp::Reverse(value.len()));
    redactions
}

fn redact_support_value(value: &mut Value, redactions: &[String]) {
    match value {
        Value::Array(values) => {
            for value in values {
                redact_support_value(value, redactions);
            }
        }
        Value::Object(fields) => {
            for value in fields.values_mut() {
                redact_support_value(value, redactions);
            }
        }
        Value::String(text) => {
            for redaction in redactions {
                *text = text.replace(redaction, "[redacted-path]");
            }
        }
        _ => {}
    }
}

fn write_support_bundle(path: &Path, bundle: &SupportBundle) -> Result<(), String> {
    if path.exists() {
        return Err("support export refuses to replace an existing file".to_owned());
    }
    let mut output = serde_json::to_value(bundle).map_err(|error| error.to_string())?;
    redact_support_value(&mut output, &bundle.redactions);
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    if let Err(error) = serde_json::to_writer_pretty(&file, &output) {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(error.to_string());
    }
    if let Err(error) = file.sync_all() {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(error.to_string());
    }
    Ok(())
}

fn write_support_bundle_on_worker(path: PathBuf, bundle: SupportBundle) -> Result<(), String> {
    let worker = std::thread::Builder::new()
        .name("sqlite-capsule-support-write".to_owned())
        .stack_size(RUNTIME_WORKER_STACK_BYTES)
        .spawn(move || write_support_bundle(&path, &bundle))
        .map_err(|_| "support-write worker is unavailable".to_owned())?;
    worker
        .join()
        .map_err(|_| "support-write worker failed".to_owned())?
}

#[tauri::command]
fn export_support_bundle_picker(
    state: State<'_, Mutex<HostState>>,
    app: AppHandle,
) -> Result<(), String> {
    let bundle = collect_support_bundle_on_worker(state.inner())?;
    if let Some(path) = native_e2e_support_path_from_process() {
        write_support_bundle_on_worker(path, bundle)?;
        emit_host_message(
            &app,
            "support-exported",
            "Redacted support bundle exported.",
        );
        return Ok(());
    }
    let callback_app = app.clone();
    app.dialog()
        .file()
        .add_filter("JSON support bundle", &["json"])
        .set_file_name("sqlite-capsule-support.json")
        .save_file(move |selected| match selected {
            Some(selected) => match selected.into_path() {
                Ok(path) => match write_support_bundle_on_worker(path, bundle) {
                    Ok(()) => emit_host_message(
                        &callback_app,
                        "support-exported",
                        "Redacted support bundle exported.",
                    ),
                    Err(error) => emit_host_message(
                        &callback_app,
                        "support-export-error",
                        format!("support export was refused: {error}"),
                    ),
                },
                Err(error) => emit_host_message(
                    &callback_app,
                    "support-export-error",
                    format!("the support destination is not a local path: {error}"),
                ),
            },
            None => emit_host_message(
                &callback_app,
                "support-export-cancelled",
                "Support export cancelled.",
            ),
        });
    Ok(())
}

fn emit_signing_status(app: &AppHandle, state: &Arc<Mutex<SigningSession>>) {
    let report = state.lock().ok().map(|session| session.report());
    if let Some(report) = report {
        let _ = app.emit("signing-status", report);
    }
}

fn finish_signing_picker_error(
    app: &AppHandle,
    state: &Arc<Mutex<SigningSession>>,
    kind: &str,
    message: impl Into<String>,
) {
    if let Ok(mut session) = state.lock() {
        session.busy = false;
    }
    emit_signing_status(app, state);
    emit_host_message(app, kind, message);
}

fn begin_signing_picker(state: &SigningState) -> Result<Arc<Mutex<SigningSession>>, String> {
    let shared = state.0.clone();
    let mut session = shared
        .lock()
        .map_err(|_| "publisher-signing state is unavailable".to_owned())?;
    if session.busy {
        return Err("another publisher-signing operation is still active".to_owned());
    }
    session.busy = true;
    drop(session);
    Ok(shared)
}

#[tauri::command]
fn signing_status(state: State<'_, SigningState>) -> Result<SigningSessionReport, String> {
    state
        .0
        .lock()
        .map_err(|_| "publisher-signing state is unavailable".to_owned())
        .map(|session| session.report())
}

#[tauri::command]
fn select_signing_key_picker(state: State<'_, SigningState>, app: AppHandle) -> Result<(), String> {
    let shared = begin_signing_picker(&state)?;
    emit_signing_status(&app, &shared);
    let callback_app = app.clone();
    app.dialog()
        .file()
        .add_filter("Ed25519 private key", &["seed", "key", "hex", "pem", "der"])
        .pick_file(move |selected| match selected {
            Some(selected) => match selected.into_path() {
                Ok(path) => {
                    let file_name = path
                        .file_name()
                        .map(|value| value.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "Selected private key".to_owned());
                    let worker_state = shared.clone();
                    let worker_app = callback_app.clone();
                    let spawn = std::thread::Builder::new()
                        .name("sqlite-capsule-key-import".to_owned())
                        .stack_size(RUNTIME_WORKER_STACK_BYTES)
                        .spawn(move || match LoadedSigningKey::from_file(&path) {
                            Ok(key) => {
                                if let Ok(mut session) = worker_state.lock() {
                                    session.key = Some(key);
                                    session.key_file_name = Some(file_name);
                                    session.invalidate_preview();
                                    session.busy = false;
                                }
                                emit_signing_status(&worker_app, &worker_state);
                                emit_host_message(
                                    &worker_app,
                                    "signing-key-selected",
                                    "Use-once private key loaded into protected host memory.",
                                );
                            }
                            Err(error) => finish_signing_picker_error(
                                &worker_app,
                                &worker_state,
                                "signing-key-error",
                                format!("private key import was refused: {error}"),
                            ),
                        });
                    if let Err(error) = spawn {
                        finish_signing_picker_error(
                            &callback_app,
                            &shared,
                            "signing-key-error",
                            format!("private key worker is unavailable: {error}"),
                        );
                    }
                }
                Err(error) => finish_signing_picker_error(
                    &callback_app,
                    &shared,
                    "signing-key-error",
                    format!("the selected private key is not a local path: {error}"),
                ),
            },
            None => finish_signing_picker_error(
                &callback_app,
                &shared,
                "signing-key-cancelled",
                "Private key selection cancelled.",
            ),
        });
    Ok(())
}

#[tauri::command]
fn select_signing_source_picker(
    state: State<'_, SigningState>,
    app: AppHandle,
) -> Result<(), String> {
    let shared = begin_signing_picker(&state)?;
    emit_signing_status(&app, &shared);
    let callback_app = app.clone();
    app.dialog()
        .file()
        .add_filter("SQLite Capsule", &["sqlitecapsule", "sqlite"])
        .pick_file(move |selected| match selected {
            Some(selected) => match selected.into_path() {
                Ok(path) => {
                    let worker_state = shared.clone();
                    let worker_app = callback_app.clone();
                    let spawn = std::thread::Builder::new()
                        .name("sqlite-capsule-sign-source".to_owned())
                        .stack_size(INSPECTION_STACK_BYTES)
                        .spawn(move || match inspect_signing_source(&path) {
                            Ok(source) => {
                                if let Ok(mut session) = worker_state.lock() {
                                    session.source = Some(source);
                                    session.output = None;
                                    session.invalidate_preview();
                                    session.busy = false;
                                }
                                emit_signing_status(&worker_app, &worker_state);
                                emit_host_message(
                                    &worker_app,
                                    "signing-source-selected",
                                    "Source capsule verified. No embedded application assets were executed.",
                                );
                            }
                            Err(error) => finish_signing_picker_error(
                                &worker_app,
                                &worker_state,
                                "signing-source-error",
                                format!("source capsule was refused: {error}"),
                            ),
                        });
                    if let Err(error) = spawn {
                        finish_signing_picker_error(
                            &callback_app,
                            &shared,
                            "signing-source-error",
                            format!("source inspection worker is unavailable: {error}"),
                        );
                    }
                }
                Err(error) => finish_signing_picker_error(
                    &callback_app,
                    &shared,
                    "signing-source-error",
                    format!("the selected source is not a local path: {error}"),
                ),
            },
            None => finish_signing_picker_error(
                &callback_app,
                &shared,
                "signing-source-cancelled",
                "Source capsule selection cancelled.",
            ),
        });
    Ok(())
}

#[tauri::command]
fn select_signing_output_picker(
    state: State<'_, SigningState>,
    app: AppHandle,
) -> Result<(), String> {
    let shared = begin_signing_picker(&state)?;
    emit_signing_status(&app, &shared);
    let callback_app = app.clone();
    app.dialog()
        .file()
        .add_filter("Signed SQLite Capsule", &["sqlitecapsule", "sqlite"])
        .set_file_name("signed.sqlitecapsule")
        .save_file(move |selected| match selected {
            Some(selected) => match selected.into_path() {
                Ok(path) => {
                    let error = if path.exists() {
                        Some("refusing to replace an existing output".to_owned())
                    } else if !path.parent().is_some_and(Path::is_dir) {
                        Some("the output parent directory does not exist".to_owned())
                    } else {
                        None
                    };
                    if let Some(error) = error {
                        finish_signing_picker_error(
                            &callback_app,
                            &shared,
                            "signing-output-error",
                            error,
                        );
                        return;
                    }
                    if let Ok(mut session) = shared.lock() {
                        session.output = Some(path);
                        session.invalidate_preview();
                        session.busy = false;
                    }
                    emit_signing_status(&callback_app, &shared);
                    emit_host_message(
                        &callback_app,
                        "signing-output-selected",
                        "New signed-capsule destination selected. Nothing has been written yet.",
                    );
                }
                Err(error) => finish_signing_picker_error(
                    &callback_app,
                    &shared,
                    "signing-output-error",
                    format!("the selected output is not a local path: {error}"),
                ),
            },
            None => finish_signing_picker_error(
                &callback_app,
                &shared,
                "signing-output-cancelled",
                "Signed-capsule destination selection cancelled.",
            ),
        });
    Ok(())
}

#[tauri::command]
async fn prepare_signing(
    state: State<'_, SigningState>,
    request: PrepareSigningRequest,
) -> Result<SigningSessionReport, String> {
    let shared = state.0.clone();
    let (source, output) = {
        let mut session = shared
            .lock()
            .map_err(|_| "publisher-signing state is unavailable".to_owned())?;
        if session.busy {
            return Err("another publisher-signing operation is still active".to_owned());
        }
        if session.key.is_none() {
            return Err("select a private key before preparing the signature".to_owned());
        }
        let source = session
            .source
            .as_ref()
            .map(|source| source.canonical_path.clone())
            .ok_or_else(|| "select and verify a source capsule".to_owned())?;
        let output = session
            .output
            .clone()
            .ok_or_else(|| "select a new output path".to_owned())?;
        session.invalidate_preview();
        session.busy = true;
        (source, output)
    };
    let publisher_id = request.publisher_id;
    let publisher_name = request.publisher_name;
    let prepared = tauri::async_runtime::spawn_blocking(move || {
        prepare_signing_copy(&source, &output, &publisher_id, &publisher_name, None)
    })
    .await;
    let mut session = shared
        .lock()
        .map_err(|_| "publisher-signing state is unavailable".to_owned())?;
    session.busy = false;
    match prepared.map_err(|_| "publisher-signing preparation worker failed".to_owned())? {
        Ok(prepared) => {
            session.prepared = Some(prepared);
            Ok(session.report())
        }
        Err(error) => Err(format!("signature preparation was refused: {error}")),
    }
}

#[tauri::command]
async fn execute_signing(
    state: State<'_, SigningState>,
    request: ExecuteSigningRequest,
) -> Result<SigningResultReport, String> {
    let shared = state.0.clone();
    let (key, prepared) = {
        let mut session = shared
            .lock()
            .map_err(|_| "publisher-signing state is unavailable".to_owned())?;
        if session.busy {
            return Err("another publisher-signing operation is still active".to_owned());
        }
        let key = session
            .key
            .as_ref()
            .ok_or_else(|| "the use-once private key is no longer loaded".to_owned())?;
        let prepared = session
            .prepared
            .as_ref()
            .ok_or_else(|| "prepare and review the signature before signing".to_owned())?;
        if request.confirmation_key_id != key.info().key_id
            || request.confirmation_application_digest != prepared.preview().application_digest
        {
            return Err("the reviewed key or application digest confirmation changed".to_owned());
        }
        session.busy = true;
        let key = session.key.take().expect("checked key is present");
        let prepared = session
            .prepared
            .take()
            .expect("checked prepared capsule is present");
        (key, prepared)
    };
    let result = tauri::async_runtime::spawn_blocking(move || prepared.sign(key)).await;
    let mut session = shared
        .lock()
        .map_err(|_| "publisher-signing state is unavailable".to_owned())?;
    session.busy = false;
    session.key_file_name = None;
    session.source = None;
    session.output = None;
    match result.map_err(|_| "publisher-signing worker failed".to_owned())? {
        Ok(report) => Ok(signing_result_report(report)),
        Err(error) => Err(format!("capsule signing failed closed: {error}")),
    }
}

#[tauri::command]
fn clear_signing_session(state: State<'_, SigningState>) -> Result<SigningSessionReport, String> {
    let mut session = state
        .0
        .lock()
        .map_err(|_| "publisher-signing state is unavailable".to_owned())?;
    if session.busy {
        return Err("the active publisher-signing operation cannot be cleared".to_owned());
    }
    *session = SigningSession::default();
    Ok(session.report())
}

#[tauri::command]
fn open_capsule_picker(app: AppHandle) -> Result<(), String> {
    let callback_app = app.clone();
    app.dialog()
        .file()
        .add_filter("SQLite Capsule", &["sqlitecapsule", "sqlite"])
        .pick_file(move |selected| match selected {
            Some(selected) => match selected.into_path() {
                Ok(path) => {
                    load_host_path(&callback_app, &path);
                    focus_main_window(&callback_app);
                }
                Err(error) => emit_host_message(
                    &callback_app,
                    "error",
                    format!("the selected item is not a local file path: {error}"),
                ),
            },
            None => emit_host_message(&callback_app, "cancelled", "Open cancelled."),
        });
    Ok(())
}

#[tauri::command]
fn reopen_current_capsule(
    state: State<'_, Mutex<HostState>>,
    app: AppHandle,
) -> Result<StartupReport, String> {
    let path = state
        .lock()
        .map_err(|_| "host trust state is unavailable".to_owned())?
        .inspection
        .as_ref()
        .map(|inspection| PathBuf::from(&inspection.identity.canonical_path))
        .ok_or_else(|| "there is no current capsule path to reopen".to_owned())?;
    Ok(load_host_path(&app, &path))
}

#[tauri::command]
fn continue_current_read_only(
    state: State<'_, Mutex<HostState>>,
    app: AppHandle,
) -> Result<StartupReport, String> {
    let mut state = state
        .lock()
        .map_err(|_| "host trust state is unavailable".to_owned())?;
    let prior_inspection = state
        .inspection
        .as_ref()
        .cloned()
        .ok_or_else(|| "there is no current capsule path to reopen".to_owned())?;
    let path = PathBuf::from(&prior_inspection.identity.canonical_path);
    let prior_decision = state
        .report
        .capsule
        .as_ref()
        .map(|capsule| capsule.decision.clone())
        .ok_or_else(|| "the prior launch policy is unavailable".to_owned())?;
    let conflict_backup = state
        .bridge
        .lock()
        .map_err(|_| "runtime bridge is unavailable".to_owned())?
        .conflict_backup
        .clone();
    state.load_capsule(&path);
    let Some(inspection) = state.inspection.as_ref().cloned() else {
        preserve_conflict_backup(&state.bridge, conflict_backup)?;
        let report = state.report.clone();
        drop(state);
        navigate_sandbox(&app, None)?;
        return Ok(report);
    };
    let fresh_decision = state
        .report
        .capsule
        .as_ref()
        .map(|capsule| capsule.decision.clone())
        .ok_or_else(|| "fresh launch policy is unavailable".to_owned())?;
    let decision = if fresh_decision.executable_allowed {
        explicit_read_only_decision(fresh_decision)
    } else if let Some(decision) = continued_read_only_decision(
        &prior_inspection.evidence,
        &inspection.evidence,
        &prior_decision,
        &fresh_decision,
    ) {
        decision
    } else {
        preserve_conflict_backup(&state.bridge, conflict_backup)?;
        let report = state.report.clone();
        drop(state);
        navigate_sandbox(&app, None)?;
        return Ok(report);
    };
    if !decision.executable_allowed {
        preserve_conflict_backup(&state.bridge, conflict_backup)?;
        let report = state.report.clone();
        drop(state);
        navigate_sandbox(&app, None)?;
        return Ok(report);
    }
    {
        let mut bridge = state
            .bridge
            .lock()
            .map_err(|_| "runtime bridge is unavailable".to_owned())?;
        bridge.activate_read_only(&inspection, &decision)?;
        bridge.conflict_backup = conflict_backup;
    }
    let mut report = report_for("reopened-read-only", &inspection, decision);
    if let Some(capsule) = report.capsule.as_mut() {
        capsule.assets_released = true;
    }
    state.report = report.clone();
    let entry_asset = inspection.identity.entry_asset.clone();
    drop(state);
    navigate_sandbox(&app, Some(&entry_asset))?;
    Ok(report)
}

fn preserve_conflict_backup(
    bridge: &Arc<Mutex<RuntimeBridge>>,
    backup: Option<BackupRecord>,
) -> Result<(), String> {
    let mut bridge = bridge
        .lock()
        .map_err(|_| "runtime bridge is unavailable".to_owned())?;
    if backup.is_some() {
        bridge.conflict_backup = backup;
        bridge.mode = "conflict_closed".to_owned();
    }
    Ok(())
}

fn continued_read_only_decision(
    prior: &LaunchEvidence,
    current: &LaunchEvidence,
    prior_decision: &LaunchDecision,
    fresh_decision: &LaunchDecision,
) -> Option<LaunchDecision> {
    let same_signed_application = prior.application_digest.is_some()
        && prior.application_digest == current.application_digest
        && prior.capsule_id == current.capsule_id
        && prior.application_id == current.application_id
        && prior.publisher == current.publisher
        && prior.signatures == current.signatures
        && prior.requested_capabilities == current.requested_capabilities
        && prior.required_capabilities == current.required_capabilities;
    let fresh_state_can_continue = matches!(
        fresh_decision.trust_state,
        TrustState::SignatureValidUnknownPublisher
            | TrustState::SignedTrustedPublisher
            | TrustState::LocallyTrustedExactRelease
    );
    if !same_signed_application
        || !prior_decision.executable_allowed
        || !prior_decision.signature_valid
        || !fresh_decision.signature_valid
        || !fresh_state_can_continue
    {
        return None;
    }

    let mut decision = fresh_decision.clone();
    for (name, capability) in &mut decision.capabilities {
        if name == "database.write" {
            capability.decision = CapabilityDecision::Deny;
            capability.allow_once = false;
            capability.reason = "the user explicitly continued this session read-only".to_owned();
            continue;
        }
        let prior_allowed = prior_decision
            .capabilities
            .get(name)
            .is_some_and(|prior| prior.decision == CapabilityDecision::Allow);
        if prior_allowed && capability.decision == CapabilityDecision::Prompt {
            capability.decision = CapabilityDecision::Allow;
            capability.allow_once = true;
            capability.reason =
                "reused only for this host process after the same signed application was re-verified"
                    .to_owned();
        }
    }
    let required_non_write_allowed = current.required_capabilities.iter().all(|name| {
        name == "database.write"
            || decision
                .capabilities
                .get(name)
                .is_some_and(|capability| capability.decision == CapabilityDecision::Allow)
    });
    let database_read_allowed = decision
        .capabilities
        .get("database.read")
        .is_some_and(|capability| capability.decision == CapabilityDecision::Allow);
    if !required_non_write_allowed || !database_read_allowed {
        return None;
    }
    decision.executable_allowed = true;
    Some(decision)
}

fn explicit_read_only_decision(mut decision: LaunchDecision) -> LaunchDecision {
    if let Some(database_write) = decision.capabilities.get_mut("database.write") {
        database_write.decision = CapabilityDecision::Deny;
        database_write.allow_once = false;
        database_write.reason = "the user explicitly continued this session read-only".to_owned();
    }
    decision
}

#[derive(Debug, Deserialize)]
struct RestoreBackupRequest {
    backup_id: String,
}

#[tauri::command]
fn restore_backup_picker(
    request: RestoreBackupRequest,
    state: State<'_, Mutex<HostState>>,
    app: AppHandle,
) -> Result<(), String> {
    let backup_root = {
        let state = state
            .lock()
            .map_err(|_| "host trust state is unavailable".to_owned())?;
        let bridge = state
            .bridge
            .lock()
            .map_err(|_| "capsule runtime state is unavailable".to_owned())?;
        let current = bridge
            .runtime
            .as_ref()
            .and_then(VerifiedCapsule::backup_record)
            .or(bridge.conflict_backup.as_ref())
            .ok_or_else(|| "the current session has no verified backup".to_owned())?;
        if current.backup_id != request.backup_id {
            return Err("the selected backup is not current for this session".to_owned());
        }
        bridge.backup_root.clone()
    };

    let callback_app = app.clone();
    let backup_id = request.backup_id;
    if let Some(path) = native_e2e_restore_path_from_process() {
        return restore_backup_to_path(&callback_app, &backup_root, &backup_id, &path).map(|_| ());
    }
    app.dialog()
        .file()
        .add_filter("SQLite Capsule", &["sqlitecapsule", "sqlite"])
        .set_file_name("restored.sqlitecapsule")
        .save_file(move |selected| match selected {
            Some(selected) => match selected.into_path() {
                Ok(path) => {
                    match restore_backup_to_path(&callback_app, &backup_root, &backup_id, &path) {
                        Ok(()) => {}
                        Err(error) => emit_host_message(
                            &callback_app,
                            "restore-error",
                            format!("restore was refused: {error}"),
                        ),
                    }
                }
                Err(error) => emit_host_message(
                    &callback_app,
                    "restore-error",
                    format!("the restore destination is not a local path: {error}"),
                ),
            },
            None => emit_host_message(&callback_app, "restore-cancelled", "Restore cancelled."),
        });
    Ok(())
}

fn restore_backup_to_path(
    app: &AppHandle,
    backup_root: &Path,
    backup_id: &str,
    path: &Path,
) -> Result<(), String> {
    let backup_root = backup_root.to_owned();
    let backup_id = backup_id.to_owned();
    let output = path.to_owned();
    let worker = std::thread::Builder::new()
        .name("sqlite-capsule-restore".to_owned())
        .stack_size(RUNTIME_WORKER_STACK_BYTES)
        .spawn(move || {
            restore_verified_backup(&backup_root, &backup_id, &output)
                .map_err(|error| error.to_string())
        })
        .map_err(|_| "restore worker is unavailable".to_owned())?;
    let record = worker
        .join()
        .map_err(|_| "restore worker failed".to_owned())??;
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.emit(
            "restore-report",
            RestoreReport {
                restored_path: path.display().to_string(),
                record,
            },
        );
    }
    load_host_path(app, path);
    focus_main_window(app);
    Ok(())
}

#[derive(Debug, Deserialize)]
struct FirstOpenRequest {
    action: String,
    capabilities: Vec<String>,
}

#[tauri::command]
fn first_open_decide(
    request: FirstOpenRequest,
    state: State<'_, Mutex<HostState>>,
    app: AppHandle,
) -> Result<StartupReport, String> {
    let mut state = state
        .lock()
        .map_err(|_| "host trust state is unavailable".to_owned())?;
    if request.action == "cancel" {
        let mut report = state.report.clone();
        report.stage = "cancelled".to_owned();
        state.report = report.clone();
        return Ok(report);
    }
    if !matches!(request.action.as_str(), "allow_once" | "always" | "deny") {
        return Err("unknown first-open action".to_owned());
    }
    let evidence = state
        .inspection
        .as_ref()
        .map(|inspection| inspection.evidence.clone())
        .ok_or_else(|| "no verified capsule is available".to_owned())?;
    let store = state
        .trust_store
        .as_mut()
        .ok_or_else(|| "protected trust store is unavailable".to_owned())?;
    let decision = apply_first_open_decision(request, &evidence, store)?;
    let stage = if decision.executable_allowed {
        "policy-authorized"
    } else {
        "policy-blocked"
    };
    let inspection = state
        .inspection
        .as_ref()
        .cloned()
        .ok_or_else(|| "verified capsule disappeared".to_owned())?;
    let executable_allowed = decision.executable_allowed;
    if executable_allowed {
        state
            .bridge
            .lock()
            .map_err(|_| "runtime bridge is unavailable".to_owned())?
            .activate(&inspection, &decision)?;
    } else {
        state
            .bridge
            .lock()
            .map_err(|_| "runtime bridge is unavailable".to_owned())?
            .deactivate();
    }
    let recovery = state.report.recovery.clone();
    let mut report = report_for(stage, &inspection, decision);
    report.recovery = recovery;
    if let Some(capsule) = report.capsule.as_mut() {
        capsule.assets_released = executable_allowed;
    }
    state.report = report.clone();
    let entry_asset = executable_allowed.then_some(inspection.identity.entry_asset);
    drop(state);
    navigate_sandbox(&app, entry_asset.as_deref())?;
    Ok(report)
}

#[derive(Debug, Deserialize)]
struct TrustAdminRequest {
    action: String,
    confirmation: Option<String>,
}

#[derive(Debug, Serialize)]
struct TrustAdminResponse {
    action: String,
    output: Value,
    report: StartupReport,
}

#[tauri::command]
fn trust_admin(
    request: TrustAdminRequest,
    state: State<'_, Mutex<HostState>>,
    app: AppHandle,
) -> Result<TrustAdminResponse, String> {
    let mut state = state
        .lock()
        .map_err(|_| "host trust state is unavailable".to_owned())?;
    if !matches!(
        request.action.as_str(),
        "audit" | "export" | "forget_current_decision" | "revoke_current_key" | "reset"
    ) {
        return Err("unknown trust administration action".to_owned());
    }
    let evidence = state
        .inspection
        .as_ref()
        .map(|inspection| inspection.evidence.clone())
        .ok_or_else(|| "no verified capsule is available".to_owned())?;

    let mut refresh_report = false;
    let output = match request.action.as_str() {
        "audit" => {
            let store = state
                .trust_store
                .as_ref()
                .ok_or_else(|| "protected trust store is unavailable".to_owned())?;
            json!({"events": store.audit_events(250).map_err(|error| error.to_string())?})
        }
        "export" => {
            let store = state
                .trust_store
                .as_ref()
                .ok_or_else(|| "protected trust store is unavailable".to_owned())?;
            store.export_redacted().map_err(|error| error.to_string())?
        }
        "forget_current_decision" => {
            let confirmation = request.confirmation.as_deref().unwrap_or_default();
            if confirmation != "FORGET-CURRENT-DECISION" {
                return Err(
                    "forgetting the current decision requires exact confirmation FORGET-CURRENT-DECISION"
                        .to_owned(),
                );
            }
            let store = state
                .trust_store
                .as_mut()
                .ok_or_else(|| "protected trust store is unavailable".to_owned())?;
            let forgotten = store
                .forget_current_decision(&evidence, confirmation)
                .map_err(|error| error.to_string())?;
            refresh_report = true;
            json!({
                "forgotten": forgotten,
                "authority_granted": false,
                "next_step": "review the current capsule again"
            })
        }
        "revoke_current_key" => {
            let signature = current_valid_signature(&evidence)
                .ok_or_else(|| "there is no current valid signing key to revoke".to_owned())?;
            if request.confirmation.as_deref() != Some(signature.key_id.as_str()) {
                return Err("revocation requires the exact current key fingerprint".to_owned());
            }
            let store = state
                .trust_store
                .as_mut()
                .ok_or_else(|| "protected trust store is unavailable".to_owned())?;
            store
                .revoke_publisher_key(&signature.key_id, "revoked in trusted host UI")
                .map_err(|error| error.to_string())?;
            refresh_report = true;
            json!({"revoked_key_id": signature.key_id})
        }
        "reset" => {
            let confirmation = request.confirmation.as_deref().unwrap_or_default();
            if confirmation != "ERASE-TRUST-DECISIONS" {
                return Err("reset requires exact confirmation ERASE-TRUST-DECISIONS".to_owned());
            }
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| "system clock precedes the Unix epoch".to_owned())?
                .as_nanos();
            let backup_name = format!("trust-v1-before-reset-{nonce}.sqlite");
            let backup = state.trust_backup_directory.join(&backup_name);
            let store = state
                .trust_store
                .as_mut()
                .ok_or_else(|| "protected trust store is unavailable".to_owned())?;
            store
                .backup_to(&backup)
                .map_err(|error| error.to_string())?;
            store
                .reset_decisions(confirmation)
                .map_err(|error| error.to_string())?;
            refresh_report = true;
            json!({
                "decisions_erased": true,
                "verified_backup": backup_name,
                "backup_location": "protected host-local backup directory"
            })
        }
        _ => unreachable!("validated action"),
    };

    let mut lock_renderer = false;
    if refresh_report {
        let store = state
            .trust_store
            .as_mut()
            .ok_or_else(|| "protected trust store is unavailable".to_owned())?;
        let decision = store
            .evaluate(&evidence, &host_policy_context(&evidence))
            .map_err(|error| error.to_string())?;
        let inspection = state
            .inspection
            .as_ref()
            .ok_or_else(|| "verified capsule disappeared".to_owned())?;
        let recovery = state.report.recovery.clone();
        state.report = report_for("trust-administration", inspection, decision);
        state.report.recovery = recovery;
        state
            .bridge
            .lock()
            .map_err(|_| "runtime bridge is unavailable".to_owned())?
            .deactivate();
        lock_renderer = true;
    }
    let response = TrustAdminResponse {
        action: request.action,
        output,
        report: state.report.clone(),
    };
    drop(state);
    if lock_renderer {
        navigate_sandbox(&app, None)?;
    }
    Ok(response)
}

fn apply_first_open_decision(
    request: FirstOpenRequest,
    evidence: &LaunchEvidence,
    store: &mut TrustStore,
) -> Result<LaunchDecision, String> {
    let supplied_capability_count = request.capabilities.len();
    let selected: BTreeSet<_> = request.capabilities.into_iter().collect();
    if selected.len() != supplied_capability_count
        || selected.len() > evidence.requested_capabilities.len()
        || !selected.is_subset(&evidence.requested_capabilities)
    {
        return Err("capability selection exceeds the verified manifest request".to_owned());
    }
    let mut context = host_policy_context(evidence);

    match request.action.as_str() {
        "allow_once" => {
            context.trust_once = true;
            context.allow_once = selected;
        }
        "always" => {
            let signature = current_valid_signature(evidence).ok_or_else(|| {
                "always is limited to a currently valid signed application release".to_owned()
            })?;
            let grants: BTreeMap<_, _> = evidence
                .requested_capabilities
                .iter()
                .map(|capability| {
                    let grant = if selected.contains(capability) {
                        CapabilityDecision::Allow
                    } else {
                        CapabilityDecision::Deny
                    };
                    (capability.clone(), grant)
                })
                .collect();
            store
                .trust_exact_release_with_grants(
                    evidence,
                    &signature.key_id,
                    &grants,
                    "selected in trusted first-open UI",
                )
                .map_err(|error| error.to_string())?;
        }
        "deny" => {
            if let Some(signature) = current_valid_signature(evidence) {
                store
                    .deny_exact_release(
                        evidence,
                        &signature.key_id,
                        "denied in trusted first-open UI",
                    )
                    .map_err(|error| error.to_string())?;
            } else {
                store
                    .deny_exact_file(evidence, "denied in trusted first-open UI")
                    .map_err(|error| error.to_string())?;
            }
        }
        _ => unreachable!("validated action"),
    }

    store
        .evaluate(evidence, &context)
        .map_err(|error| error.to_string())
}

fn host_policy_context(evidence: &LaunchEvidence) -> EvaluationContext {
    let supported: BTreeSet<_> = SUPPORTED_CAPABILITIES.iter().copied().collect();
    let host_policy = evidence
        .requested_capabilities
        .iter()
        .map(|capability| {
            let decision = if supported.contains(capability.as_str()) {
                CapabilityDecision::Allow
            } else {
                CapabilityDecision::Deny
            };
            (capability.clone(), decision)
        })
        .collect();
    EvaluationContext {
        host_policy,
        ..EvaluationContext::default()
    }
}

fn current_valid_signature(
    evidence: &LaunchEvidence,
) -> Option<&sqlite_capsule_policy::SignatureEvidence> {
    evidence
        .signatures
        .iter()
        .find(|signature| signature.cryptographically_valid && signature.digest_matches)
}

fn report_for(
    stage: &str,
    inspection: &LaunchInspection,
    decision: LaunchDecision,
) -> StartupReport {
    let evidence = &inspection.evidence;
    StartupReport {
        stage: stage.to_owned(),
        capsule: Some(CapsuleReport {
            identity: inspection.identity.clone(),
            source_sha256: lower_hex(&evidence.source_sha256),
            application_digest: evidence
                .application_digest
                .as_ref()
                .map(|digest| lower_hex(digest)),
            publisher: evidence
                .publisher
                .as_ref()
                .map(|publisher| PublisherReport {
                    id: publisher.publisher_id.clone(),
                    name: publisher.publisher_name.clone(),
                }),
            signatures: evidence
                .signatures
                .iter()
                .map(|signature| SignatureReport {
                    key_id: signature.key_id.clone(),
                    cryptographically_valid: signature.cryptographically_valid,
                    digest_matches: signature.digest_matches,
                })
                .collect(),
            decision,
            // Runtime activation is the only path that changes this to true.
            assets_released: false,
        }),
        recovery: None,
        error: None,
    }
}

fn released_entry_asset(report: &StartupReport) -> Option<String> {
    report
        .capsule
        .as_ref()
        .filter(|capsule| capsule.assets_released && capsule.decision.executable_allowed)
        .map(|capsule| capsule.identity.entry_asset.clone())
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

fn install_sandbox_webview(
    window: &tauri::Window,
    protocol_bridge: Arc<Mutex<RuntimeBridge>>,
) -> Result<(), String> {
    if SANDBOX_WEBVIEW.with(|slot| slot.borrow().is_some()) {
        return Ok(());
    }
    let scale = window
        .scale_factor()
        .map_err(|error| format!("could not read trusted-window scale: {error}"))?;
    let size = window
        .inner_size()
        .map_err(|error| format!("could not read trusted-window size: {error}"))?
        .to_logical::<f64>(scale);
    let raw_e2e_port = cdp_e2e_debug_ports_from_process().map(|(_, raw)| raw);
    let runtime_worker_gate = Arc::new(AtomicBool::new(false));
    let sandbox_builder = WebViewBuilder::new()
        .with_custom_protocol(CUSTOM_PROTOCOL.to_owned(), move |_webview_id, request| {
            handle_protocol_request(
                protocol_bridge.clone(),
                runtime_worker_gate.clone(),
                request,
            )
        })
        .with_url(format!("{CUSTOM_PROTOCOL}://{CUSTOM_HOST}/__host/locked"))
        .with_bounds(application_bounds(size.width, size.height))
        .with_autoplay(false)
        .with_clipboard(false)
        .with_devtools(raw_e2e_port.is_some())
        // The raw child is a separate native focus scope. Starting it focused
        // traps the first Tab sequence outside the trusted review surface.
        .with_focused(false)
        .with_incognito(true)
        .with_navigation_handler(|url| allowed_child_navigation(url.as_str()))
        .with_new_window_req_handler(|_, _| NewWindowResponse::Deny)
        // Deliberately omit `with_ipc_handler`: Wry exposes a transport object,
        // but drops every message when no Rust handler exists.
        ;
    #[cfg(target_os = "windows")]
    let sandbox_builder = if let Some(port) = raw_e2e_port {
        sandbox_builder.with_additional_browser_args(format!(
            "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --remote-debugging-port={port}"
        ))
    } else {
        sandbox_builder
    };
    let sandbox = sandbox_builder
        .build(window)
        .map_err(|error| format!("could not create locked raw renderer: {error}"))?;
    SANDBOX_WEBVIEW.with(|slot| slot.replace(Some(sandbox)));
    Ok(())
}

fn navigate_sandbox(app: &AppHandle, entry_asset: Option<&str>) -> Result<(), String> {
    let entry_asset = entry_asset.map(str::to_owned);
    let app_for_windows = app.clone();
    app.run_on_main_thread(move || {
        navigate_sandbox_on_main_thread(&app_for_windows, entry_asset.as_deref())
    })
    .map_err(|error| format!("could not schedule raw renderer navigation: {error}"))
}

fn navigate_sandbox_on_main_thread(app: &AppHandle, entry_asset: Option<&str>) {
    let target = entry_asset.map(sandbox_navigation_url);
    let application_authorized = target.is_some();
    SANDBOX_WEBVIEW.with(|slot| {
        let borrowed = slot.borrow();
        let Some(webview) = borrowed.as_ref() else {
            return;
        };
        let locked = sandbox_navigation_url("__host/locked");
        let result = webview.load_url(target.as_deref().unwrap_or(&locked));
        if let Err(error) = result {
            eprintln!("failed to navigate raw sandbox webview: {error}");
            return;
        }

        if application_authorized {
            if let Some(window) = app.get_window(CAPSULE_WINDOW_LABEL) {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.maximize();
                let _ = window.set_focus();
            }
        } else {
            if let Some(window) = app.get_window(CAPSULE_WINDOW_LABEL) {
                let _ = window.hide();
            }
            focus_main_window(app);
        }
    });
}

fn sandbox_navigation_url(path: &str) -> String {
    sandbox_navigation_url_for_platform(cfg!(target_os = "windows"), path)
}

fn sandbox_navigation_url_for_platform(windows: bool, path: &str) -> String {
    let encoded = encode_asset_path(path);
    if windows {
        // Wry maps a registered custom protocol to this HTTP origin on WebView2.
        // It performs that mapping for the builder's initial URL but not for
        // later `WebView::load_url` calls, so subsequent navigation must use
        // the mapped form directly.
        format!("http://{CUSTOM_PROTOCOL}.{CUSTOM_HOST}/{encoded}")
    } else {
        format!("{CUSTOM_PROTOCOL}://{CUSTOM_HOST}/{encoded}")
    }
}

fn encode_asset_path(path: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut output = String::with_capacity(path.len());
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.' | b'~') {
            output.push(byte as char);
        } else {
            output.push('%');
            output.push(HEX[(byte >> 4) as usize] as char);
            output.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    output
}

fn allowed_child_navigation(url: &str) -> bool {
    url.starts_with("capsule://app/") || url.starts_with("http://capsule.app/")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut context = tauri::generate_context!();
    #[cfg(target_os = "windows")]
    if let Some((parent_port, _)) = cdp_e2e_debug_ports_from_process() {
        let window = context
            .config_mut()
            .app
            .windows
            .first_mut()
            .expect("the configured main webview window exists");
        window.visible = false;
        window.additional_browser_args = Some(format!(
            "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --remote-debugging-port={parent_port}"
        ));
    }
    let updater_configuration = compiled_updater_config()
        .unwrap_or_else(|error| panic!("invalid compiled updater configuration: {error}"));
    let updater_plugin = match updater_configuration.as_ref() {
        Some(configuration) => tauri_plugin_updater::Builder::new()
            .pubkey(configuration.public_key.clone())
            .build(),
        None => tauri_plugin_updater::Builder::new().build(),
    };
    tauri::Builder::default()
        // Register first so a secondary file-association launch is forwarded
        // before another plugin can act in the short-lived process.
        .plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
            schedule_forwarded_launch(app, args, cwd);
        }))
        .plugin(tauri_plugin_dialog::init())
        // Rust-only updater backend. No `updater:*` permission is granted to
        // either WebView; an unconfigured development build performs no check.
        .plugin(updater_plugin)
        .setup(move |app| {
            if let Some(configuration) = updater_configuration.as_ref() {
                // Build the exact Rust-only transport up front so malformed
                // endpoint/key configuration fails before the UI is released.
                // Building performs no network request.
                let redirect_origin = configuration.endpoint_origin.clone();
                app.updater_builder()
                    .endpoints(vec![configuration.endpoint.clone()])?
                    .target(host_release_target())
                    .pubkey(configuration.public_key.clone())
                    .timeout(std::time::Duration::from_secs(30))
                    .configure_client(move |builder| {
                        builder.redirect(restricted_redirect_policy(redirect_origin.clone()))
                    })
                    .build()?;
            }
            let app_data = host_app_data_root_from_process(app.path().app_local_data_dir()?);
            let trust_path = app_data.join("trust-v1.sqlite");
            let trust_backup_directory = app_data.join("trust-backups");
            let bridge = Arc::new(Mutex::new(RuntimeBridge::new(
                app_data.join("capsule-writer-locks"),
                app_data.join("capsule-backups"),
            )));
            let host_state = HostState::from_process(
                trust_path,
                trust_backup_directory,
                app_data.join("host-updates"),
                bridge.clone(),
            );
            let initial_entry_asset = released_entry_asset(&host_state.report);
            app.manage(Mutex::new(host_state));
            app.manage(SigningState::default());
            app.manage(UpdateCheckGate(AtomicBool::new(false)));
            app.manage(Mutex::new(HostUpdateFlow::default()));
            let window = app
                .get_webview_window("main")
                .expect("the configured main webview window exists");
            let capsule_window = tauri::window::WindowBuilder::new(app, CAPSULE_WINDOW_LABEL)
                .title(CAPSULE_WINDOW_TITLE)
                .inner_size(CAPSULE_WINDOW_WIDTH, CAPSULE_WINDOW_HEIGHT)
                .min_inner_size(CAPSULE_WINDOW_MIN_WIDTH, CAPSULE_WINDOW_MIN_HEIGHT)
                .resizable(true)
                .maximized(true)
                .visible(false)
                .build()?;
            if !webview_automation_enabled() {
                // Creating the raw renderer synchronously in setup can race the
                // parent WebView2 environment. Queue the same locked child onto
                // the first native event-loop turn instead. Native WebDriver
                // mode deliberately exercises only the trusted shell because
                // WebView2 rejects a second child automation environment; that
                // mode remains locked and receives no application assets.
                let sandbox_window = capsule_window.clone();
                let sandbox_bridge = bridge.clone();
                let sandbox_app = app.handle().clone();
                app.run_on_main_thread(move || {
                    if let Err(error) = install_sandbox_webview(&sandbox_window, sandbox_bridge) {
                        eprintln!("{error}");
                        return;
                    }
                    navigate_sandbox_on_main_thread(&sandbox_app, initial_entry_asset.as_deref());
                })?;
            }
            let app_for_drop = app.handle().clone();
            window.on_window_event(move |event| {
                if let WindowEvent::DragDrop(DragDropEvent::Drop { paths, .. }) = event {
                    if paths.len() == 1 {
                        load_host_path(&app_for_drop, &paths[0]);
                    } else {
                        let report = reject_host_file_delivery_state(
                            &app_for_drop,
                            "drop-rejected",
                            "drop exactly one capsule file",
                        );
                        publish_host_report(&app_for_drop, &report);
                    }
                }
            });
            let app_for_close = app.handle().clone();
            window.on_window_event(move |event| {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    handle_close_request(&app_for_close, api);
                }
            });
            let app_for_capsule_close = app.handle().clone();
            let capsule_window_on_resize = capsule_window.clone();
            capsule_window.on_window_event(move |event| {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    handle_close_request(&app_for_capsule_close, api);
                }
                if let WindowEvent::Resized(size) = event {
                    let scale = capsule_window_on_resize.scale_factor().unwrap_or(1.0);
                    let logical = size.to_logical::<f64>(scale);
                    SANDBOX_WEBVIEW.with(|slot| {
                        if let Some(webview) = slot.borrow().as_ref() {
                            resize_application(webview, logical.width, logical.height);
                        }
                    });
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            startup_report,
            lifecycle_status,
            update_status,
            check_host_update,
            download_host_update,
            stage_host_update,
            prepare_update_installation,
            execute_update_installation,
            execute_update_rollback,
            open_capsule_picker,
            reopen_current_capsule,
            continue_current_read_only,
            restore_backup_picker,
            export_support_bundle_picker,
            signing_status,
            select_signing_key_picker,
            select_signing_source_picker,
            select_signing_output_picker,
            prepare_signing,
            execute_signing,
            clear_signing_session,
            first_open_decide,
            trust_admin
        ])
        .run(context)
        .expect("error while running SQLite Capsule Host");
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use ed25519_dalek::{Signer, SigningKey};
    use rusqlite::{Connection, params};
    use sha2::{Digest, Sha256};
    use sqlite_capsule_crypto::{ALGORITHM, PROFILE, application_digest, sign_digest};
    use sqlite_capsule_distribution::{
        RELEASE_PROFILE, ReleaseArtifact, ReleaseManifest, SignedReleaseManifest, key_id,
    };
    use sqlite_capsule_policy::{PublisherEvidence, SignatureEvidence, TrustState};

    use super::*;

    static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let suffix = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "sqlite-capsule-desktop-test-{}-{suffix}",
                std::process::id()
            ));
            std::fs::create_dir(&path).expect("create test directory");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn evidence() -> LaunchEvidence {
        LaunchEvidence {
            structure_verified: true,
            capsule_id: "urn:uuid:phase2-desktop-test".to_owned(),
            application_id: "org.example.phase2".to_owned(),
            source_sha256: [0x11; 32],
            application_digest: Some([0x22; 32]),
            publisher: Some(PublisherEvidence {
                publisher_id: "org.example.publisher".to_owned(),
                publisher_name: "Example Publisher".to_owned(),
            }),
            signatures: vec![SignatureEvidence {
                key_id: "ed25519:sha256:desktop-fixture".to_owned(),
                public_key: [0x33; 32],
                cryptographically_valid: true,
                digest_matches: true,
            }],
            requested_capabilities: BTreeSet::from([
                "clipboard.read".to_owned(),
                "database.read".to_owned(),
                "database.write".to_owned(),
            ]),
            required_capabilities: BTreeSet::from([
                "database.read".to_owned(),
                "database.write".to_owned(),
            ]),
        }
    }

    #[test]
    fn writable_open_failures_fall_back_only_for_safe_read_only_cases() {
        let writer_busy =
            RuntimeError::Lifecycle(sqlite_capsule_lifecycle::LifecycleError::WriterBusy);
        let unsafe_file_system = RuntimeError::Lifecycle(
            sqlite_capsule_lifecycle::LifecycleError::UnsafeWritableFileSystem,
        );
        let execution_denied = RuntimeError::ExecutionDenied;

        assert_eq!(
            read_only_fallback_mode(&writer_busy, true),
            Some("read_only_writer_busy")
        );
        assert_eq!(
            read_only_fallback_mode(&unsafe_file_system, true),
            Some("read_only_unsafe_filesystem")
        );
        assert_eq!(read_only_fallback_mode(&writer_busy, false), None);
        assert_eq!(read_only_fallback_mode(&unsafe_file_system, false), None);
        assert_eq!(read_only_fallback_mode(&execution_denied, true), None);
    }

    fn open_store(directory: &TestDirectory) -> TrustStore {
        TrustStore::open(&directory.0.join("state/trust.sqlite")).expect("open trust store")
    }

    fn signed_example_capsule(directory: &TestDirectory) -> PathBuf {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let capsule = directory.0.join("remembered-release.sqlitecapsule");
        std::fs::copy(
            repository.join("capsules/diagram-studio.capsule.sqlite"),
            &capsule,
        )
        .expect("copy example capsule");

        let signing_key = SigningKey::from_bytes(&[0x5a; 32]);
        let mut connection = Connection::open(&capsule).expect("open signing copy");
        let transaction = connection.transaction().expect("begin signing transaction");
        transaction
            .execute_batch(include_str!(
                "../../../../format/capsule-signed-app-v0.2.sql"
            ))
            .expect("install signed-app extension");
        transaction
            .execute(
                "INSERT INTO capsule_publisher \
                 (id, profile, publisher_id, publisher_name) VALUES (1, ?1, ?2, ?3)",
                params![PROFILE, "org.example.desktop", "Desktop Test Publisher"],
            )
            .expect("insert publisher");
        let digest = application_digest(&transaction).expect("application digest");
        let envelope = sign_digest(&signing_key, digest, "2026-08-11T12:00:00Z")
            .expect("sign application digest");
        transaction
            .execute(
                "INSERT INTO capsule_signature \
                 (key_id, algorithm, public_key, application_digest, signature, signed_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    envelope.key_id,
                    ALGORITHM,
                    envelope.public_key.as_slice(),
                    envelope.application_digest.as_slice(),
                    envelope.signature.as_slice(),
                    envelope.signed_at,
                ],
            )
            .expect("insert signature");
        transaction.commit().expect("commit signed capsule");
        drop(connection);
        capsule
    }

    fn test_lower_hex(bytes: &[u8]) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut output = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            output.push(HEX[(byte >> 4) as usize] as char);
            output.push(HEX[(byte & 0x0f) as usize] as char);
        }
        output
    }

    fn signed_update_announcement() -> (Value, CompiledUpdaterConfig, tauri::Url) {
        let root = SigningKey::from_bytes(&[0x44; 32]);
        let artifact_bytes = b"bounded signed host package";
        let sigstore_bytes = b"bounded sigstore evidence";
        let manifest = ReleaseManifest {
            profile: RELEASE_PROFILE.to_owned(),
            sequence: 8,
            version: "0.3.0".to_owned(),
            issued_at: "2025-01-01T00:00:00Z".to_owned(),
            expires_at: "2030-01-01T00:00:00Z".to_owned(),
            artifacts: vec![ReleaseArtifact {
                target: host_release_target().to_owned(),
                url: "https://updates.example/host-package.bin".to_owned(),
                bytes: artifact_bytes.len() as u64,
                sha256: test_lower_hex(&Sha256::digest(artifact_bytes)),
                sigstore_bundle_sha256: test_lower_hex(&Sha256::digest(sigstore_bytes)),
                platform_signing: expected_platform_signing().to_owned(),
                platform_signing_identity: format!(
                    "authenticode-certificate-sha256:{}",
                    "ab".repeat(32)
                ),
                platform_timestamp_required: true,
                sigstore_certificate_identity:
                    "https://github.com/sqlite-capsule/sqlite-capsule/.github/workflows/release.yml@refs/tags/v0.3.0"
                        .to_owned(),
                sigstore_oidc_issuer: "https://token.actions.githubusercontent.com".to_owned(),
            }],
        };
        let mut message = b"SQLite Capsule host release manifest v2\0".to_vec();
        message.extend_from_slice(
            &serde_json_canonicalizer::to_vec(&manifest).expect("canonical release manifest"),
        );
        let signed_release = SignedReleaseManifest {
            manifest,
            signing_key_id: key_id(&root.verifying_key().to_bytes()),
            signature_hex: test_lower_hex(&root.sign(&message).to_bytes()),
        };
        let endpoint = "https://updates.example/latest.json"
            .parse::<tauri::Url>()
            .expect("test endpoint");
        (
            json!({
                "version": "0.3.0",
                "sqlite_capsule": {
                    "signed_release": signed_release,
                    "sigstore_bundle_url": "https://updates.example/host-package.sigstore.json"
                }
            }),
            CompiledUpdaterConfig {
                endpoint,
                public_key: "encoded updater key fixture".to_owned(),
                release_public_key: root.verifying_key().to_bytes(),
                current_release_sequence: 7,
                endpoint_origin: "https://updates.example".to_owned(),
            },
            "https://updates.example/host-package.bin"
                .parse::<tauri::Url>()
                .expect("test artifact URL"),
        )
    }

    #[test]
    fn signed_update_announcement_binds_every_transport_field_and_sidecar_origin() {
        let (raw, configuration, artifact_url) = signed_update_announcement();
        let (candidate, sidecar) = verify_update_announcement(
            &raw,
            "0.2.0",
            "0.3.0",
            host_release_target(),
            &artifact_url,
            64,
            &configuration,
            1_800_000_000,
        )
        .expect("verified signed announcement");
        assert_eq!(candidate.sequence(), 8);
        assert_eq!(candidate.version(), "0.3.0");
        assert_eq!(
            sidecar.as_str(),
            "https://updates.example/host-package.sigstore.json"
        );

        assert!(
            verify_update_announcement(
                &raw,
                "0.2.0",
                "0.4.0",
                host_release_target(),
                &artifact_url,
                64,
                &configuration,
                1_800_000_000,
            )
            .is_err(),
            "unsigned announced version cannot replace signed version"
        );
        assert!(
            verify_update_announcement(
                &raw,
                "0.2.0",
                "0.3.0",
                host_release_target(),
                &artifact_url,
                0,
                &configuration,
                1_800_000_000,
            )
            .is_err(),
            "missing updater signature is rejected before download"
        );
        let mut foreign_sidecar = raw.clone();
        foreign_sidecar["sqlite_capsule"]["sigstore_bundle_url"] =
            json!("https://elsewhere.example/host-package.sigstore.json");
        assert!(
            verify_update_announcement(
                &foreign_sidecar,
                "0.2.0",
                "0.3.0",
                host_release_target(),
                &artifact_url,
                64,
                &configuration,
                1_800_000_000,
            )
            .is_err(),
            "Sigstore evidence cannot cross the compiled origin"
        );
        let mut unknown_field = raw;
        unknown_field["sqlite_capsule"]["unexpected"] = json!(true);
        assert!(
            verify_update_announcement(
                &unknown_field,
                "0.2.0",
                "0.3.0",
                host_release_target(),
                &artifact_url,
                64,
                &configuration,
                1_800_000_000,
            )
            .is_err(),
            "unknown signed-metadata wrapper fields fail closed"
        );
    }

    #[test]
    fn compiled_updater_configuration_is_complete_https_and_redacted() {
        let release_root = "11".repeat(32);
        assert!(
            validate_compiled_updater_config(None, None, None, None)
                .unwrap()
                .is_none()
        );
        assert!(
            validate_compiled_updater_config(Some("https://updates.example"), None, None, None)
                .is_err()
        );
        for endpoint in [
            "http://updates.example/latest.json",
            "https://user:secret@updates.example/latest.json",
            "https://updates.example/latest.json#fragment",
        ] {
            assert!(
                validate_compiled_updater_config(
                    Some(endpoint),
                    Some("PUBLIC KEY"),
                    Some(&release_root),
                    Some("7")
                )
                .is_err(),
                "{endpoint}"
            );
        }
        assert!(
            validate_compiled_updater_config(
                Some("https://updates.example/latest.json"),
                Some("   "),
                Some(&release_root),
                Some("7")
            )
            .is_err()
        );
        for (root, sequence) in [
            ("11", "7"),
            (
                "AA00000000000000000000000000000000000000000000000000000000000000",
                "7",
            ),
            (release_root.as_str(), "0"),
            (release_root.as_str(), "not-a-sequence"),
        ] {
            assert!(
                validate_compiled_updater_config(
                    Some("https://updates.example/latest.json"),
                    Some("PUBLIC KEY"),
                    Some(root),
                    Some(sequence)
                )
                .is_err()
            );
        }
        let configuration = validate_compiled_updater_config(
            Some("https://updates.example:8443/stable/latest.json?channel=stable"),
            Some("untrusted comment: development public key\nRWQfixture"),
            Some(&release_root),
            Some("7"),
        )
        .expect("valid updater configuration")
        .expect("configured updater");
        assert_eq!(configuration.endpoint.scheme(), "https");
        assert_eq!(
            configuration.endpoint_origin,
            "https://updates.example:8443"
        );
        assert!(!configuration.endpoint_origin.contains("latest.json"));
        assert!(!configuration.endpoint_origin.contains("PUBLIC KEY"));
        assert_eq!(configuration.release_public_key, [0x11; 32]);
        assert_eq!(configuration.current_release_sequence, 7);
    }

    #[test]
    fn update_check_gate_releases_on_scope_exit_and_rejects_overlap() {
        let gate = UpdateCheckGate(AtomicBool::new(false));
        let lease = gate.acquire().expect("first update check lease");
        assert!(gate.acquire().is_err());
        drop(lease);
        assert!(gate.acquire().is_ok());
    }

    #[test]
    fn platform_package_names_are_class_specific_before_staging() {
        for (class, url, extension) in [
            ("authenticode", "https://updates.example/host.msi", ".msi"),
            ("authenticode", "https://updates.example/host.exe", ".exe"),
            (
                "developer-id-notarized",
                "https://updates.example/host.pkg",
                ".pkg",
            ),
            (
                "linux-detached",
                "https://updates.example/host.AppImage",
                ".AppImage",
            ),
        ] {
            assert_eq!(
                platform_artifact_extension(class, url).expect("matching package suffix"),
                extension
            );
        }
        assert!(
            platform_artifact_extension("authenticode", "https://updates.example/host.pkg")
                .is_err()
        );
        assert!(
            platform_artifact_extension(
                "linux-detached",
                "https://updates.example/host.AppImage.exe"
            )
            .is_err()
        );
    }

    #[test]
    fn host_update_preflight_quiesces_only_after_a_writable_recovery_point_exists() {
        let directory = TestDirectory::new();
        let source_root = directory.0.join("source");
        std::fs::create_dir(&source_root).expect("create source directory");
        let capsule = source_root.join("update-preflight.sqlitecapsule");
        let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        std::fs::copy(
            repository.join("capsules/diagram-studio.capsule.sqlite"),
            &capsule,
        )
        .expect("copy capsule");
        let inspection = sqlite_capsule_launch::inspect_launch(&capsule).expect("inspect capsule");
        let mut store = open_store(&directory);
        let decision = apply_first_open_decision(
            FirstOpenRequest {
                action: "allow_once".to_owned(),
                capabilities: inspection
                    .evidence
                    .requested_capabilities
                    .iter()
                    .cloned()
                    .collect(),
            },
            &inspection.evidence,
            &mut store,
        )
        .expect("authorise writable session");
        let backup_root = directory.0.join("host/backups");
        let bridge = Arc::new(Mutex::new(RuntimeBridge::new(
            directory.0.join("host/locks"),
            backup_root.clone(),
        )));
        {
            let mut bridge = bridge.lock().expect("lock writable bridge");
            bridge
                .activate(&inspection, &decision)
                .expect("activate writable bridge");
            assert_eq!(bridge.mode, "writable");
            assert!(bridge.runtime.is_some());
            assert!(bridge.protocol.is_some());
        }

        let report = prepare_for_host_update_on_worker(bridge.clone())
            .expect("prepare active session for update");
        assert!(report.had_active_session);
        assert!(report.writable_session);
        assert!(report.session_quiesced);
        assert!(report.verified_backup.is_some());
        let bridge = bridge.lock().expect("lock quiesced bridge");
        assert!(bridge.runtime.is_none());
        assert!(bridge.protocol.is_none());
        assert!(bridge.session_token.is_none());
        assert_eq!(bridge.mode, "locked");
        let inventory = inspect_backup_inventory(&backup_root).expect("inspect update backup");
        assert_eq!(inventory.verified.len(), 1);
        assert!(inventory.incomplete_artifacts.is_empty());
        assert!(inventory.invalid_artifacts.is_empty());
    }

    #[test]
    fn same_signed_application_may_continue_read_only_after_domain_only_change() {
        let directory = TestDirectory::new();
        let prior_evidence = evidence();
        let mut store = open_store(&directory);
        let prior_decision = apply_first_open_decision(
            FirstOpenRequest {
                action: "allow_once".to_owned(),
                capabilities: prior_evidence
                    .requested_capabilities
                    .iter()
                    .cloned()
                    .collect(),
            },
            &prior_evidence,
            &mut store,
        )
        .expect("allow once");
        assert!(prior_decision.executable_allowed);

        let mut current_evidence = prior_evidence.clone();
        current_evidence.source_sha256 = [0x7a; 32];
        let fresh_decision = store
            .evaluate(&current_evidence, &host_policy_context(&current_evidence))
            .expect("fresh policy");
        assert!(!fresh_decision.executable_allowed);
        let continued = continued_read_only_decision(
            &prior_evidence,
            &current_evidence,
            &prior_decision,
            &fresh_decision,
        )
        .unwrap_or_else(|| {
            panic!(
                "same signed application may continue read-only: prior={prior_decision:?}; fresh={fresh_decision:?}"
            )
        });
        assert!(continued.executable_allowed);
        assert_eq!(
            continued.capabilities["database.read"].decision,
            CapabilityDecision::Allow
        );
        assert_eq!(
            continued.capabilities["database.write"].decision,
            CapabilityDecision::Deny
        );

        let mut changed_application = current_evidence.clone();
        changed_application.application_digest = Some([0x55; 32]);
        assert!(
            continued_read_only_decision(
                &prior_evidence,
                &changed_application,
                &prior_decision,
                &fresh_decision,
            )
            .is_none()
        );
        let mut revoked = fresh_decision;
        revoked.trust_state = TrustState::Revoked;
        revoked.revocation_status = "revoked".to_owned();
        assert!(
            continued_read_only_decision(
                &prior_evidence,
                &current_evidence,
                &prior_decision,
                &revoked,
            )
            .is_none()
        );
    }

    fn staged_update(stage_id: &str, state: UpdateStageState) -> StagedUpdate {
        StagedUpdate {
            record: sqlite_capsule_update::UpdateStageRecord {
                profile: sqlite_capsule_update::UPDATE_STAGE_PROFILE.to_owned(),
                stage_id: stage_id.to_owned(),
                version: "1.2.0".to_owned(),
                sequence: 12,
                target: "x86_64-pc-windows-msvc".to_owned(),
                platform_signing: "authenticode".to_owned(),
                platform_signing_identity: format!(
                    "authenticode-certificate-sha256:{}",
                    "ab".repeat(32)
                ),
                platform_timestamp_required: true,
                sigstore_certificate_identity:
                    "https://github.com/sqlite-capsule/sqlite-capsule/.github/workflows/release.yml@refs/tags/v0.3.0"
                        .to_owned(),
                sigstore_oidc_issuer: "https://token.actions.githubusercontent.com".to_owned(),
                artifact_name: "host.msi".to_owned(),
                artifact_bytes: 1,
                artifact_sha256: "11".repeat(32),
                sigstore_name: "host.sigstore.json".to_owned(),
                sigstore_bytes: 1,
                sigstore_sha256: "22".repeat(32),
                previous_installer_name: None,
                previous_installer_version: None,
                previous_installer_bytes: None,
                previous_installer_sha256: None,
                signed_release: SignedReleaseManifest {
                    manifest: sqlite_capsule_distribution::ReleaseManifest {
                        profile: sqlite_capsule_distribution::RELEASE_PROFILE.to_owned(),
                        sequence: 12,
                        version: "1.2.0".to_owned(),
                        issued_at: "2026-01-01T00:00:00Z".to_owned(),
                        expires_at: "2030-01-01T00:00:00Z".to_owned(),
                        artifacts: vec![sqlite_capsule_distribution::ReleaseArtifact {
                            target: "x86_64-pc-windows-msvc".to_owned(),
                            url: "https://updates.example.com/host.msi".to_owned(),
                            bytes: 1,
                            sha256: "11".repeat(32),
                            sigstore_bundle_sha256: "22".repeat(32),
                            platform_signing: "authenticode".to_owned(),
                            platform_signing_identity: format!(
                                "authenticode-certificate-sha256:{}",
                                "ab".repeat(32)
                            ),
                            platform_timestamp_required: true,
                            sigstore_certificate_identity:
                                "https://github.com/sqlite-capsule/sqlite-capsule/.github/workflows/release.yml@refs/tags/v0.3.0"
                                    .to_owned(),
                            sigstore_oidc_issuer:
                                "https://token.actions.githubusercontent.com".to_owned(),
                        }],
                    },
                    signing_key_id: format!("ed25519:sha256:{}", "33".repeat(32)),
                    signature_hex: "44".repeat(64),
                },
            },
            state,
            running_version: None,
            rollback_reason: None,
        }
    }

    #[test]
    fn install_preparation_selects_only_the_exact_one_confirmed_prepared_stage() {
        assert!(require_execute_confirmation("RUN VERIFIED HOST INSTALLER").is_ok());
        assert!(require_execute_confirmation("INSTALL HOST UPDATE").is_err());
        assert!(require_execute_confirmation("run verified host installer").is_err());
        let stage_id = "00000000000000000012-x86_64-pc-windows-msvc";
        let prepared = staged_update(stage_id, UpdateStageState::Prepared);
        let valid = UpdateInventoryReport {
            verified: vec![prepared.clone()],
            incomplete: Vec::new(),
            invalid: Vec::new(),
        };
        assert_eq!(
            select_confirmed_prepared_update(&valid, stage_id, "INSTALL HOST UPDATE")
                .expect("confirmed exact candidate"),
            &prepared
        );
        assert!(
            select_confirmed_prepared_update(&valid, stage_id, "install").is_err(),
            "confirmation is exact and case-sensitive"
        );
        assert!(
            select_confirmed_prepared_update(
                &valid,
                "00000000000000000013-x86_64-pc-windows-msvc",
                "INSTALL HOST UPDATE"
            )
            .is_err(),
            "confirmation cannot be replayed for another candidate"
        );

        let started = UpdateInventoryReport {
            verified: vec![staged_update(stage_id, UpdateStageState::InstallerStarted)],
            incomplete: Vec::new(),
            invalid: Vec::new(),
        };
        assert!(
            select_confirmed_prepared_update(&started, stage_id, "INSTALL HOST UPDATE").is_err()
        );
        let ambiguous = UpdateInventoryReport {
            verified: vec![
                prepared,
                staged_update(
                    "00000000000000000013-aarch64-apple-darwin",
                    UpdateStageState::Prepared,
                ),
            ],
            incomplete: Vec::new(),
            invalid: Vec::new(),
        };
        assert!(
            select_confirmed_prepared_update(&ambiguous, stage_id, "INSTALL HOST UPDATE").is_err()
        );
        let damaged = UpdateInventoryReport {
            verified: vec![staged_update(stage_id, UpdateStageState::Prepared)],
            incomplete: vec!["partial-stage".to_owned()],
            invalid: Vec::new(),
        };
        assert!(
            select_confirmed_prepared_update(&damaged, stage_id, "INSTALL HOST UPDATE").is_err()
        );
    }

    #[test]
    fn rollback_execution_selects_only_the_exact_required_stage() {
        assert!(require_rollback_confirmation("RUN VERIFIED HOST ROLLBACK").is_ok());
        assert!(require_rollback_confirmation("run verified host rollback").is_err());
        let stage_id = "00000000000000000012-x86_64-pc-windows-msvc";
        let rollback = staged_update(stage_id, UpdateStageState::RollbackRequired);
        let valid = UpdateInventoryReport {
            verified: vec![rollback.clone()],
            incomplete: Vec::new(),
            invalid: Vec::new(),
        };
        assert_eq!(
            select_confirmed_rollback(&valid, stage_id).expect("select exact rollback"),
            &rollback
        );
        assert!(select_confirmed_rollback(&valid, "different-stage").is_err());
        let ambiguous = UpdateInventoryReport {
            verified: vec![
                rollback,
                staged_update(
                    "00000000000000000013-aarch64-apple-darwin",
                    UpdateStageState::RollbackRequired,
                ),
            ],
            incomplete: Vec::new(),
            invalid: Vec::new(),
        };
        assert!(select_confirmed_rollback(&ambiguous, stage_id).is_err());
        let damaged = UpdateInventoryReport {
            verified: vec![staged_update(stage_id, UpdateStageState::RollbackRequired)],
            incomplete: Vec::new(),
            invalid: vec!["tampered-stage".to_owned()],
        };
        assert!(select_confirmed_rollback(&damaged, stage_id).is_err());
    }

    #[test]
    fn host_owned_open_path_accepts_capsule_content_and_rejects_non_sqlite_drop() {
        let directory = TestDirectory::new();
        let bridge = Arc::new(Mutex::new(RuntimeBridge::new(
            directory.0.join("host/locks"),
            directory.0.join("host/backups"),
        )));
        let mut state = HostState {
            inspection: None,
            trust_store: Some(open_store(&directory)),
            trust_backup_directory: directory.0.join("trust-backups"),
            update_root: directory.0.join("host-updates"),
            bridge,
            report: StartupReport {
                stage: "no-capsule".to_owned(),
                capsule: None,
                recovery: None,
                error: None,
            },
        };
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        state.load_capsule(&root.join("capsules/diagram-studio.capsule.sqlite"));
        assert_eq!(state.report.stage, "first-open");
        assert_eq!(
            state
                .report
                .capsule
                .as_ref()
                .expect("capsule report")
                .identity
                .app_id,
            "org.sqlite-capsule.diagram-studio"
        );

        state.load_capsule(&root.join("README.md"));
        assert_eq!(state.report.stage, "rejected");
        assert!(state.report.capsule.is_none());
        assert!(
            state
                .report
                .error
                .as_deref()
                .unwrap_or_default()
                .contains("SQLite")
        );
    }

    #[test]
    fn remembered_signed_release_reopens_without_another_first_open_decision() {
        let directory = TestDirectory::new();
        let capsule = signed_example_capsule(&directory);
        let inspection =
            sqlite_capsule_launch::inspect_launch(&capsule).expect("inspect signed capsule");
        let mut store = open_store(&directory);
        let persisted = apply_first_open_decision(
            FirstOpenRequest {
                action: "always".to_owned(),
                capabilities: inspection
                    .evidence
                    .requested_capabilities
                    .iter()
                    .cloned()
                    .collect(),
            },
            &inspection.evidence,
            &mut store,
        )
        .expect("persist exact release decision");
        assert!(persisted.executable_allowed);

        let bridge = Arc::new(Mutex::new(RuntimeBridge::new(
            directory.0.join("host/locks"),
            directory.0.join("host/backups"),
        )));
        let mut state = HostState {
            inspection: None,
            trust_store: Some(store),
            trust_backup_directory: directory.0.join("trust-backups"),
            update_root: directory.0.join("host-updates"),
            bridge: bridge.clone(),
            report: StartupReport {
                stage: "no-capsule".to_owned(),
                capsule: None,
                recovery: None,
                error: None,
            },
        };

        state.load_capsule(&capsule);

        assert_eq!(state.report.stage, "remembered-authorized");
        let report = state.report.capsule.as_ref().expect("capsule report");
        assert_eq!(
            report.decision.trust_state,
            TrustState::LocallyTrustedExactRelease
        );
        assert!(report.decision.executable_allowed);
        assert!(report.assets_released);
        assert_eq!(
            released_entry_asset(&state.report).as_deref(),
            Some(report.identity.entry_asset.as_str())
        );
        let bridge = bridge.lock().expect("lock activated bridge");
        assert!(bridge.runtime.is_some());
        assert!(bridge.protocol.is_some());
        assert_eq!(bridge.mode, "writable");
    }

    #[test]
    fn rejected_multi_file_drop_deactivates_the_prior_runtime() {
        let directory = TestDirectory::new();
        let bridge = Arc::new(Mutex::new(RuntimeBridge::new(
            directory.0.join("host/locks"),
            directory.0.join("host/backups"),
        )));
        let mut state = HostState {
            inspection: None,
            trust_store: Some(open_store(&directory)),
            trust_backup_directory: directory.0.join("trust-backups"),
            update_root: directory.0.join("host-updates"),
            bridge: bridge.clone(),
            report: StartupReport {
                stage: "no-capsule".to_owned(),
                capsule: None,
                recovery: None,
                error: None,
            },
        };
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        state.load_capsule(&root.join("capsules/diagram-studio.capsule.sqlite"));
        let inspection = state.inspection.clone().expect("launch inspection");
        let decision = apply_first_open_decision(
            FirstOpenRequest {
                action: "allow_once".to_owned(),
                capabilities: inspection
                    .evidence
                    .requested_capabilities
                    .iter()
                    .cloned()
                    .collect(),
            },
            &inspection.evidence,
            state.trust_store.as_mut().expect("trust store"),
        )
        .expect("allow-once runtime decision");
        bridge
            .lock()
            .expect("runtime bridge")
            .activate_read_only(&inspection, &decision)
            .expect("read-only runtime activation");
        assert!(
            lifecycle_status_for(&bridge.lock().expect("runtime bridge"))
                .expect("lifecycle status")
                .active
        );

        state.reject_file_delivery("drop-rejected", "drop exactly one capsule file");

        assert_eq!(state.report.stage, "drop-rejected");
        assert_eq!(
            state.report.error.as_deref(),
            Some("drop exactly one capsule file")
        );
        assert!(state.inspection.is_none());
        let status = lifecycle_status_for(&bridge.lock().expect("runtime bridge"))
            .expect("lifecycle status");
        assert!(!status.active);
        assert_eq!(status.mode, "locked");
    }

    #[test]
    fn launch_inspection_uses_the_fixed_worker_stack() {
        assert_eq!(INSPECTION_STACK_BYTES, 8 * 1024 * 1024);
        let directory = TestDirectory::new();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let (inspection, recovery) = inspect_capsule_on_worker_stack(
            &root.join("capsules/diagram-studio.capsule.sqlite"),
            &directory.0.join("worker-locks"),
        )
        .expect("worker-stack launch inspection");
        assert_eq!(
            inspection.identity.app_id,
            "org.sqlite-capsule.diagram-studio"
        );
        assert!(recovery.is_none());
    }

    #[test]
    fn support_bundle_redacts_selected_paths_and_refuses_replacement() {
        let directory = TestDirectory::new();
        let bridge = Arc::new(Mutex::new(RuntimeBridge::new(
            directory.0.join("host/locks"),
            directory.0.join("host/backups"),
        )));
        let mut state = HostState {
            inspection: None,
            trust_store: Some(open_store(&directory)),
            trust_backup_directory: directory.0.join("trust-backups"),
            update_root: directory.0.join("host-updates"),
            bridge,
            report: StartupReport {
                stage: "no-capsule".to_owned(),
                capsule: None,
                recovery: None,
                error: None,
            },
        };
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let capsule = root.join("capsules/diagram-studio.capsule.sqlite");
        state.load_capsule(&capsule);
        let original_path = state
            .report
            .capsule
            .as_ref()
            .expect("capsule report")
            .identity
            .canonical_path
            .display()
            .to_string();
        state.report.error = Some(format!("selected capsule failed at {original_path}"));
        let state = Mutex::new(state);
        let bundle = collect_support_bundle_on_worker(&state).expect("support bundle");
        assert_eq!(bundle.format, "org.sqlite-capsule.support-bundle/0.2");
        assert_eq!(
            bundle.content_policy.capsule_controlled_text,
            "untrusted-data-only"
        );
        assert_eq!(
            bundle.content_policy.host_severity_source,
            "host-owned-structured-fields-only"
        );
        assert!(!bundle.content_policy.embedded_instructions_executed);
        assert!(!bundle.content_policy.capsule_database_bytes_included);
        assert!(!bundle.content_policy.trust_store_bytes_included);
        assert!(!bundle.content_policy.selected_file_contents_included);
        assert!(!bundle.content_policy.shutdown_tokens_included);
        assert!(!bundle.content_policy.private_keys_included);
        assert_eq!(bundle.update.current_version, "0.2.0");
        assert!(!bundle.update.transport_configured);
        assert!(bundle.update.transport_endpoint_origin.is_none());
        assert!(bundle.update.state.is_none());
        assert!(bundle.update.error.is_none());
        assert_eq!(
            bundle
                .startup
                .capsule
                .as_ref()
                .expect("redacted capsule")
                .identity
                .canonical_path,
            PathBuf::from("redacted")
        );
        let output = directory.0.join("support.json");
        write_support_bundle_on_worker(output.clone(), bundle.clone()).expect("support output");
        let content = std::fs::read_to_string(&output).expect("support JSON");
        assert!(!content.contains(&original_path));
        assert!(!content.contains("diagram-studio.capsule.sqlite"));
        assert!(!content.contains(&directory.0.display().to_string()));
        assert!(content.contains("[redacted-path]"));
        assert!(!content.contains("\"redactions\""));
        assert!(!content.contains("\"public_key\""));
        assert!(!content.contains("\"private_key\""));
        assert!(!content.contains("\"shutdown_token\""));
        assert!(!content.contains("SQLite format 3"));
        assert!(serde_json::from_str::<Value>(&content).is_ok());
        assert!(write_support_bundle_on_worker(output, bundle).is_err());
    }

    #[test]
    fn signing_session_report_exposes_public_metadata_but_not_key_material_or_path() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let key_path = root.join("compatibility/signed-app-v0.2/development-seed.hex");
        let secret = std::fs::read_to_string(&key_path)
            .expect("development key fixture")
            .trim()
            .to_owned();
        let key = LoadedSigningKey::from_file(&key_path).expect("load fixture key");
        let session = SigningSession {
            key: Some(key),
            key_file_name: Some("development-seed.hex".to_owned()),
            ..SigningSession::default()
        };
        let content = serde_json::to_string(&session.report()).expect("signing report JSON");
        assert!(content.contains("development-seed.hex"));
        assert!(content.contains("ed25519:sha256:"));
        assert!(content.contains("public_key_hex"));
        assert!(!content.contains(&secret));
        assert!(!content.contains(&key_path.to_string_lossy().into_owned()));
        assert!(!content.contains("private_key"));
    }

    #[test]
    fn session_tokens_are_url_safe_fixed_length_and_fresh() {
        let first = generate_session_token().expect("first token");
        let second = generate_session_token().expect("second token");
        assert_eq!(first.len(), 43);
        assert!(
            first
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        );
        assert_ne!(first, second);
    }

    #[test]
    fn asset_paths_decode_without_allowing_traversal_or_encoded_separators() {
        assert_eq!(
            decode_asset_request_path("/app/icons%20dark/mark.svg"),
            Some("app/icons dark/mark.svg".to_owned())
        );
        for rejected in [
            "/",
            "/app//main.js",
            "/../secret",
            "/app/%2e%2e/secret",
            "/app%2fsecret",
            "/app%5csecret",
            "/app/%ff",
        ] {
            assert_eq!(decode_asset_request_path(rejected), None, "{rejected}");
        }
    }

    #[test]
    fn locked_bridge_releases_only_the_probe_and_security_headers() {
        let mut bridge = RuntimeBridge::default();
        let locked = bridge.handle(
            Request::builder()
                .method(Method::GET)
                .uri("capsule://app/__host/locked")
                .body(Vec::new())
                .expect("locked request"),
        );
        assert_eq!(locked.status(), StatusCode::OK);
        assert_eq!(locked.headers()["Content-Security-Policy"], CHILD_CSP);
        assert!(CHILD_CSP.contains("script-src 'self' 'wasm-unsafe-eval'"));
        assert!(!CHILD_CSP.contains("'unsafe-eval'"));
        assert_eq!(
            locked.headers()["Permissions-Policy"],
            CHILD_PERMISSIONS_POLICY
        );
        assert!(String::from_utf8_lossy(locked.body()).contains("Raw child renderer probe"));
        assert!(!String::from_utf8_lossy(locked.body()).contains("unsafe-inline"));
        assert!(String::from_utf8_lossy(locked.body()).contains("/__host/locked.css"));
        assert!(String::from_utf8_lossy(locked.body()).contains("/__host/locked.js"));

        for (path, content_type, marker) in [
            ("/__host/locked.css", "text/css; charset=utf-8", "#checks"),
            (
                "/__host/locked.js",
                "text/javascript; charset=utf-8",
                "PASS · no native handler",
            ),
        ] {
            let resource = bridge.handle(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!("capsule://app{path}"))
                    .body(Vec::new())
                    .expect("locked resource request"),
            );
            assert_eq!(resource.status(), StatusCode::OK);
            assert_eq!(resource.headers()[header::CONTENT_TYPE], content_type);
            assert_eq!(resource.headers()["Content-Security-Policy"], CHILD_CSP);
            assert!(String::from_utf8_lossy(resource.body()).contains(marker));
        }

        let session = bridge.handle(
            Request::builder()
                .method(Method::GET)
                .uri("capsule://app/__capsule/native-session")
                .body(Vec::new())
                .expect("session request"),
        );
        assert_eq!(session.status(), StatusCode::LOCKED);
        assert!(!String::from_utf8_lossy(session.body()).contains("session\":"));
    }

    #[test]
    fn rpc_errors_are_stable_and_accepted_requests_remain_correlated() {
        const TOKEN: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGH012345678";
        let mut bridge = RuntimeBridge {
            runtime: None,
            protocol: Some(ProtocolSession::new(TOKEN.to_owned()).expect("protocol")),
            session_token: Some(TOKEN.to_owned()),
            writer_lock_root: PathBuf::new(),
            backup_root: PathBuf::new(),
            conflict_backup: None,
            conflict_renderer_locked: false,
            mode: "locked".to_owned(),
        };
        let correlated = bridge.handle(
            Request::builder()
                .method(Method::POST)
                .uri("capsule://app/__capsule/rpc")
                .body(
                    format!(
                        r#"{{"version":1,"session":"{TOKEN}","sequence":1,"id":"manifest-1","method":"manifest","params":{{}}}}"#
                    )
                    .into_bytes(),
                )
                .expect("correlated request"),
        );
        assert_eq!(correlated.status(), StatusCode::LOCKED);
        let body: Value = serde_json::from_slice(correlated.body()).expect("correlated JSON");
        assert_eq!(body["version"], 1);
        assert_eq!(body["sequence"], 1);
        assert_eq!(body["id"], "manifest-1");
        assert_eq!(body["error"]["code"], "runtime_locked");

        let oversized = bridge.handle(
            Request::builder()
                .method(Method::POST)
                .uri("capsule://app/__capsule/rpc")
                .body(vec![
                    b'x';
                    sqlite_capsule_core::protocol::MAX_REQUEST_BYTES + 1
                ])
                .expect("oversized request"),
        );
        assert_eq!(oversized.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let body: Value = serde_json::from_slice(oversized.body()).expect("oversized JSON");
        assert_eq!(body["error"]["code"], "request_too_large");
        assert!(body.get("session").is_none());
    }

    #[test]
    fn child_navigation_is_confined_to_the_custom_origin() {
        assert!(allowed_child_navigation("capsule://app/app/index.html"));
        assert!(allowed_child_navigation(
            "http://capsule.app/app/index.html"
        ));
        for rejected in [
            "https://capsule.app/app/index.html",
            "capsule://evil/app/index.html",
            "data:text/html,hello",
            "file:///tmp/capsule.sqlite",
            "about:blank",
        ] {
            assert!(!allowed_child_navigation(rejected), "{rejected}");
        }
    }

    #[test]
    fn later_sandbox_navigation_uses_the_webview2_protocol_mapping() {
        assert_eq!(
            sandbox_navigation_url_for_platform(true, "app/index.html"),
            "http://capsule.app/app/index.html"
        );
        assert_eq!(
            sandbox_navigation_url_for_platform(true, "__host/locked"),
            "http://capsule.app/__host/locked"
        );
        assert_eq!(
            sandbox_navigation_url_for_platform(false, "app/index.html"),
            "capsule://app/app/index.html"
        );
    }

    #[test]
    fn runtime_rpc_uses_a_fixed_worker_stack_and_fails_closed_while_locked() {
        assert_eq!(RUNTIME_WORKER_STACK_BYTES, 8 * 1024 * 1024);
        let bridge = Arc::new(Mutex::new(RuntimeBridge::new(
            PathBuf::new(),
            PathBuf::new(),
        )));
        let worker_gate = Arc::new(AtomicBool::new(false));
        let response = handle_protocol_request(
            bridge.clone(),
            worker_gate.clone(),
            Request::builder()
                .method(Method::POST)
                .uri("capsule://app/__capsule/rpc")
                .body(Vec::new())
                .expect("locked worker request"),
        );
        assert_eq!(response.status(), StatusCode::LOCKED);
        assert!(!String::from_utf8_lossy(response.body()).contains("session\""));
        assert!(!worker_gate.load(Ordering::Acquire));
        let lifecycle = lifecycle_status_on_worker(bridge).expect("locked lifecycle status");
        assert!(!lifecycle.active);
        assert_eq!(lifecycle.mode, "locked");

        let busy = handle_protocol_request(
            Arc::new(Mutex::new(RuntimeBridge::new(
                PathBuf::new(),
                PathBuf::new(),
            ))),
            Arc::new(AtomicBool::new(true)),
            Request::builder()
                .method(Method::POST)
                .uri("capsule://app/__capsule/rpc")
                .body(Vec::new())
                .expect("busy worker request"),
        );
        assert_eq!(busy.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn secondary_launch_accepts_only_one_explicit_path() {
        let cwd = Path::new("forwarded-working-directory");
        assert_eq!(
            forwarded_capsule_path(
                &[
                    "capsule-host".to_owned(),
                    "folder/example.sqlitecapsule".to_owned()
                ],
                cwd.to_str().unwrap(),
            )
            .expect("one relative path"),
            Some(cwd.join("folder/example.sqlitecapsule"))
        );
        assert_eq!(
            forwarded_capsule_path(&["capsule-host".to_owned()], cwd.to_str().unwrap())
                .expect("focus-only launch"),
            None
        );
        for rejected in [
            vec!["capsule-host".to_owned(), "--flag".to_owned()],
            vec![
                "capsule-host".to_owned(),
                "first.sqlitecapsule".to_owned(),
                "second.sqlitecapsule".to_owned(),
            ],
        ] {
            assert!(forwarded_capsule_path(&rejected, cwd.to_str().unwrap()).is_err());
        }
    }

    #[test]
    fn webdriver_candidate_path_does_not_bypass_normal_launch_selection() {
        let ordinary = initial_capsule_path(
            false,
            Some(OsString::from("ignored-by-ordinary-launch.sqlitecapsule")),
            [
                OsString::from("capsule-host"),
                OsString::from("ordinary.sqlitecapsule"),
            ],
        );
        assert_eq!(ordinary, Some(PathBuf::from("ordinary.sqlitecapsule")));

        let automated = initial_capsule_path(
            true,
            Some(OsString::from("verified-e2e.sqlitecapsule")),
            [
                OsString::from("capsule-host"),
                OsString::from("--remote-debugging-port=0"),
            ],
        );
        assert_eq!(automated, Some(PathBuf::from("verified-e2e.sqlitecapsule")));
    }

    #[test]
    fn webdriver_state_root_cannot_redirect_ordinary_host_authority() {
        let default = PathBuf::from("protected-user-host-state");
        let requested = Some(OsString::from("isolated-native-e2e-state"));
        assert_eq!(
            host_app_data_root(default.clone(), false, requested.clone()),
            default
        );
        assert_eq!(
            host_app_data_root(default, true, requested),
            PathBuf::from("isolated-native-e2e-state")
        );
        assert_eq!(
            cdp_e2e_debug_ports(
                true,
                Some(OsString::from("9440")),
                Some(OsString::from("9441"))
            ),
            Some((9440, 9441))
        );
        for ports in [
            (false, "9440", "9441"),
            (true, "80", "9441"),
            (true, "9440", "not-a-port"),
            (true, "9440", "9440"),
        ] {
            assert_eq!(
                cdp_e2e_debug_ports(
                    ports.0,
                    Some(OsString::from(ports.1)),
                    Some(OsString::from(ports.2))
                ),
                None
            );
        }
    }

    #[test]
    fn native_e2e_restore_override_is_new_and_confined_to_isolated_state() {
        let directory = TestDirectory::new();
        let state_root = directory.0.join("native-e2e-state");
        let restore_root = state_root.join("restored");
        std::fs::create_dir_all(&restore_root).expect("create restore root");
        let requested = restore_root.join("verified-copy.sqlitecapsule");
        let expected = restore_root
            .canonicalize()
            .expect("canonical restore root")
            .join("verified-copy.sqlitecapsule");
        assert_eq!(
            native_e2e_restore_path(
                true,
                Some(state_root.clone().into_os_string()),
                Some(requested.clone().into_os_string()),
            ),
            Some(expected)
        );
        assert_eq!(
            native_e2e_restore_path(
                false,
                Some(state_root.clone().into_os_string()),
                Some(requested.clone().into_os_string()),
            ),
            None
        );
        assert_eq!(
            native_e2e_restore_path(
                true,
                Some(state_root.clone().into_os_string()),
                Some(directory.0.join("outside.sqlitecapsule").into_os_string()),
            ),
            None
        );
        std::fs::write(&requested, b"existing").expect("create existing restore target");
        assert_eq!(
            native_e2e_restore_path(
                true,
                Some(state_root.into_os_string()),
                Some(requested.into_os_string()),
            ),
            None
        );
    }

    #[test]
    fn native_e2e_support_override_is_json_and_confined_to_isolated_cdp_state() {
        let directory = TestDirectory::new();
        let state_root = directory.0.join("native-e2e-state");
        let support_root = state_root.join("support");
        std::fs::create_dir_all(&support_root).expect("create support root");
        let requested = support_root.join("support.json");
        let expected = support_root
            .canonicalize()
            .expect("canonical support root")
            .join("support.json");
        assert_eq!(
            native_e2e_support_path(
                true,
                Some(state_root.clone().into_os_string()),
                Some(requested.clone().into_os_string()),
            ),
            Some(expected)
        );
        assert_eq!(
            native_e2e_support_path(
                false,
                Some(state_root.clone().into_os_string()),
                Some(requested.clone().into_os_string()),
            ),
            None
        );
        assert_eq!(
            native_e2e_support_path(
                true,
                Some(state_root.clone().into_os_string()),
                Some(directory.0.join("outside.json").into_os_string()),
            ),
            None
        );
        assert_eq!(
            native_e2e_support_path(
                true,
                Some(state_root.into_os_string()),
                Some(support_root.join("support.txt").into_os_string()),
            ),
            None
        );
    }

    #[test]
    fn native_e2e_update_preflight_requires_the_isolated_exact_fault_guard() {
        let directory = TestDirectory::new();
        let state_root = directory.0.join("native-e2e-state");
        std::fs::create_dir_all(&state_root).expect("create isolated state root");
        let root = Some(state_root.clone().into_os_string());
        let enabled = Some(OsString::from("enabled"));
        for stage in [
            "update.marker-synced",
            "update.database-copied",
            "update.manifest-synced",
        ] {
            assert!(native_e2e_update_preflight_requested(
                true,
                root.clone(),
                enabled.clone(),
                Some(OsString::from(stage)),
            ));
        }
        for (automation, requested_root, guard, stage) in [
            (
                false,
                root.clone(),
                enabled.clone(),
                "update.manifest-synced",
            ),
            (
                true,
                root.clone(),
                Some(OsString::from("disabled")),
                "update.manifest-synced",
            ),
            (true, root.clone(), enabled.clone(), "close.manifest-synced"),
            (
                true,
                Some(directory.0.join("missing").into_os_string()),
                enabled.clone(),
                "update.manifest-synced",
            ),
            (
                true,
                Some(PathBuf::from("relative-state").into_os_string()),
                enabled,
                "update.manifest-synced",
            ),
        ] {
            assert!(!native_e2e_update_preflight_requested(
                automation,
                requested_root,
                guard,
                Some(OsString::from(stage)),
            ));
        }
    }

    #[test]
    fn automatic_association_never_claims_the_generic_sqlite_suffix() {
        let config: Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("Tauri config JSON");
        let associations = config["bundle"]["fileAssociations"]
            .as_array()
            .expect("file association list");
        assert_eq!(associations.len(), 1);
        assert_eq!(associations[0]["ext"], json!(["sqlitecapsule"]));
        assert!(!include_str!("../tauri.conf.json").contains("\"sqlite\""));
    }

    #[test]
    fn allow_once_authorizes_only_the_current_evaluation() {
        let directory = TestDirectory::new();
        let mut store = open_store(&directory);
        let evidence = evidence();
        let decision = apply_first_open_decision(
            FirstOpenRequest {
                action: "allow_once".to_owned(),
                capabilities: vec!["database.read".to_owned(), "database.write".to_owned()],
            },
            &evidence,
            &mut store,
        )
        .expect("allow once");
        assert!(decision.executable_allowed);

        let reopened = store
            .evaluate(&evidence, &host_policy_context(&evidence))
            .expect("evaluate without session grant");
        assert!(!reopened.executable_allowed);
        assert_eq!(
            reopened.trust_state,
            TrustState::SignatureValidUnknownPublisher
        );
        let export = store.export_redacted().expect("export");
        assert_eq!(export["exact_releases"].as_array().unwrap().len(), 0);
        assert_eq!(export["grants"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn always_scopes_trust_and_each_grant_to_the_exact_signed_release() {
        let directory = TestDirectory::new();
        let mut store = open_store(&directory);
        let evidence = evidence();
        let decision = apply_first_open_decision(
            FirstOpenRequest {
                action: "always".to_owned(),
                capabilities: vec!["database.read".to_owned(), "database.write".to_owned()],
            },
            &evidence,
            &mut store,
        )
        .expect("always");
        assert!(decision.executable_allowed);
        assert_eq!(decision.trust_state, TrustState::LocallyTrustedExactRelease);
        assert_eq!(
            decision.capabilities["clipboard.read"].decision,
            CapabilityDecision::Deny
        );

        let reopened = store
            .evaluate(&evidence, &host_policy_context(&evidence))
            .expect("re-evaluate persisted release");
        assert!(reopened.executable_allowed);
        let export = store.export_redacted().expect("export");
        assert_eq!(export["exact_releases"].as_array().unwrap().len(), 1);
        assert_eq!(export["grants"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn deny_is_persisted_and_manifest_capability_broadening_is_rejected() {
        let directory = TestDirectory::new();
        let mut store = open_store(&directory);
        let evidence = evidence();
        assert!(
            apply_first_open_decision(
                FirstOpenRequest {
                    action: "allow_once".to_owned(),
                    capabilities: vec!["network".to_owned()],
                },
                &evidence,
                &mut store,
            )
            .is_err()
        );
        let denied = apply_first_open_decision(
            FirstOpenRequest {
                action: "deny".to_owned(),
                capabilities: Vec::new(),
            },
            &evidence,
            &mut store,
        )
        .expect("deny");
        assert!(!denied.executable_allowed);
        assert_eq!(denied.trust_state, TrustState::DeniedByUser);
        assert_eq!(
            store.export_redacted().expect("export")["exact_releases"][0]["decision"],
            "denied"
        );
    }
}

from __future__ import annotations

import json
import re
import unittest
from html.parser import HTMLParser
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
UI = ROOT / "native" / "desktop" / "ui"
HOST_PERMISSION = (
    ROOT / "native" / "desktop" / "src-tauri" / "permissions" / "host-first-open.toml"
)
NATIVE_SHELL = ROOT / "native" / "desktop" / "src-tauri" / "src" / "lib.rs"
RECONCILE_FLOW = ROOT / "native" / "desktop" / "src-tauri" / "src" / "reconcile_flow.rs"
UPGRADE_FLOW = ROOT / "native" / "desktop" / "src-tauri" / "src" / "upgrade_flow.rs"
NATIVE_ENTRYPOINT = ROOT / "native" / "desktop" / "src-tauri" / "src" / "main.rs"
HOST_CAPABILITY = (
    ROOT / "native" / "desktop" / "src-tauri" / "capabilities" / "host-shell.json"
)
TAURI_CONFIG = ROOT / "native" / "desktop" / "src-tauri" / "tauri.conf.json"


class HostMarkupParser(HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.ids: list[str] = []
        self.references: list[tuple[str, str]] = []
        self.buttons: list[dict[str, str | None]] = []
        self.headings: list[dict[str, str | int | None]] = []
        self.page_panels: list[dict[str, str | None]] = []
        self._button_depth = 0
        self._button_text: list[str] = []
        self._heading_stack: list[dict[str, str | int | None]] = []

    def handle_starttag(self, tag: str, attributes: list[tuple[str, str | None]]) -> None:
        values = dict(attributes)
        if identifier := values.get("id"):
            self.ids.append(identifier)
        for name in ("aria-labelledby", "aria-describedby"):
            if target := values.get(name):
                self.references.extend((name, item) for item in target.split())
        if tag == "button":
            self.buttons.append(values)
            self._button_depth += 1
            self._button_text.append("")
        if re.fullmatch(r"h[1-6]", tag):
            heading: dict[str, str | int | None] = {
                "level": int(tag[1]),
                "id": values.get("id"),
                "text": "",
            }
            self.headings.append(heading)
            self._heading_stack.append(heading)
        if tag == "section" and "data-page-panel" in values:
            self.page_panels.append(values)

    def handle_data(self, data: str) -> None:
        if self._button_depth:
            self._button_text[-1] += data
        if self._heading_stack:
            heading = self._heading_stack[-1]
            heading["text"] = f"{heading['text']}{data}"

    def handle_endtag(self, tag: str) -> None:
        if tag == "button" and self._button_depth:
            self.buttons[-1]["_text"] = self._button_text.pop().strip()
            self._button_depth -= 1
        if re.fullmatch(r"h[1-6]", tag) and self._heading_stack:
            heading = self._heading_stack.pop()
            heading["text"] = str(heading["text"]).strip()


class NativeTrustedUiAccessibilityTests(unittest.TestCase):
    def test_every_windows_entrypoint_uses_the_gui_subsystem(self) -> None:
        entrypoint = NATIVE_ENTRYPOINT.read_text(encoding="utf-8")
        self.assertIn('cfg_attr(target_os = "windows"', entrypoint)
        self.assertIn('windows_subsystem = "windows"', entrypoint)
        self.assertNotIn("debug_assertions", entrypoint)

    def test_trusted_shell_has_bounded_keyboard_and_name_baseline(self) -> None:
        parser = HostMarkupParser()
        parser.feed((UI / "index.html").read_text(encoding="utf-8"))
        self.assertEqual(len(parser.ids), len(set(parser.ids)), "duplicate HTML IDs")
        known = set(parser.ids)
        for attribute, target in parser.references:
            self.assertIn(target, known, f"missing {attribute} target {target}")
        for button in parser.buttons:
            self.assertTrue(
                button.get("_text") or button.get("aria-label"),
                "every trusted-host button needs an accessible name",
            )

        html = (UI / "index.html").read_text(encoding="utf-8")
        self.assertIn('class="skip-link"', html)
        self.assertIn('role="status" aria-live="polite"', html)
        self.assertIn('aria-label="Local trust and support output"', html)
        self.assertIn('aria-label="Verified publisher-signing result"', html)
        self.assertIn('id="update-status" role="status" aria-live="polite"', html)
        self.assertIn("Signing keys are use-once and remain in Rust memory only", html)
        self.assertIn("Sign a verified capsule copy", html)
        self.assertIn("Apply selected changes to a new copy", html)
        self.assertIn("Output identity preview", html)
        self.assertIn("Conflicts and resolutions", html)
        self.assertIn("Evidence and postconditions", html)
        self.assertIn("Upgrade the application in a new copy", html)
        self.assertIn("Exhaustive dataset actions", html)
        self.assertIn("M07 accepts only a strictly newer SemVer release", html)
        self.assertIn("Existing files and the source path are never replaced", html)
        self.assertIn("Separate application window · hidden until authorised", html)
        self.assertIn("opens maximized in its own native window", html)
        self.assertNotIn("Native child renderer occupies this fixed region", html)
        self.assertEqual(html.count('class="nav-item'), 9)
        for page in (
            "Cabinet",
            "Overview",
            "Create copy",
            "Lineage",
            "Compare",
            "Versions",
            "Security",
            "Recovery",
            "Settings",
        ):
            self.assertRegex(html, rf'class="nav-item[^\"]*"[^>]*>[\s\S]*?<span>{page}</span>')
        self.assertIn("No executable assets have been released", html)
        self.assertIn("Cached identity and trust are not reused", (UI / "app.js").read_text(encoding="utf-8"))
        self.assertIn('id="window-minimize" aria-label="Minimize window"', html)
        self.assertIn('id="window-maximize" aria-label="Maximize window"', html)
        self.assertIn('id="window-close" aria-label="Close window"', html)
        self.assertIn('data-theme-option="system"', html)

    def test_trusted_shell_has_one_semantic_heading_hierarchy(self) -> None:
        parser = HostMarkupParser()
        parser.feed((UI / "index.html").read_text(encoding="utf-8"))

        self.assertTrue(parser.headings, "the trusted shell needs a document heading")
        self.assertEqual(parser.headings[0]["level"], 1)
        self.assertEqual(parser.headings[0]["id"], "page-title")
        self.assertEqual(
            sum(heading["level"] == 1 for heading in parser.headings),
            1,
            "the changing page title is the shell's single top-level heading",
        )
        self.assertTrue(
            all(heading["level"] in (1, 2) for heading in parser.headings),
            "page sections must not skip heading levels",
        )
        self.assertTrue(
            all(str(heading["text"]).strip() for heading in parser.headings),
            "every heading needs an accessible text label",
        )

        heading_ids = {
            str(heading["id"])
            for heading in parser.headings
            if heading["id"] is not None
        }
        for panel in parser.page_panels:
            label = panel.get("aria-labelledby")
            self.assertIsNotNone(
                label,
                f"page panel {panel.get('data-page-panel')} needs a heading relationship",
            )
            self.assertIn(
                label,
                heading_ids,
                f"page panel {panel.get('data-page-panel')} is not labelled by a heading",
            )

    def test_focus_and_reduced_motion_controls_are_not_colour_only(self) -> None:
        css = (UI / "styles.css").read_text(encoding="utf-8")
        self.assertIn(":focus-visible", css)
        self.assertIn("prefers-reduced-motion: reduce", css)
        self.assertIn("forced-colors: active", css)
        self.assertIn("background: Canvas", css)
        self.assertIn("color: CanvasText", css)
        self.assertIn("outline: 3px solid Highlight", css)
        self.assertRegex(css, re.compile(r"button:disabled\s*\{[^}]*opacity:\s*1", re.S))
        self.assertRegex(css, re.compile(r"\.badge\s*\{"))
        self.assertIn('font-family: "Segoe UI Variable Display"', css)
        self.assertIn("--accent: #60cdff", css)
        self.assertIn(':root[data-theme="light"]', css)
        script = (UI / "app.js").read_text(encoding="utf-8")
        self.assertIn(
            '(report.stage === "recovery-required" ? recoverSelectedButton : verdict).focus()',
            script,
        )
        self.assertIn("promptTitle.focus()", script)
        self.assertIn("cabinetOpenButton.focus()", script)
        self.assertIn("overviewTitle.focus()", script)
        self.assertIn("focusKeyFor(report)", script)
        self.assertIn("report.capsule?.source_sha256", script)
        self.assertIn("trustLabels", script)
        self.assertIn('invokeHost("update_status")', script)
        self.assertIn('invokeHost("check_host_update")', script)
        self.assertIn('invokeHost("download_host_update",', script)
        self.assertIn('invokeHost("stage_host_update",', script)
        self.assertIn('invokeHost("signing_status")', script)
        self.assertIn('invokeHost("cabinet_status")', script)
        self.assertIn('invokeHost("open_recent_capsule"', script)
        self.assertIn('invokeHost("choose_compare_capsule"', script)
        self.assertIn('invokeHost("start_compare"', script)
        self.assertIn('invokeHost(reveal ? "reveal_compare_page" : "get_compare_page"', script)
        self.assertIn('invokeHost("get_compare_application_detail"', script)
        self.assertIn("\\u202a-\\u202e\\u2066-\\u2069", script)
        self.assertIn("compareIsolate", script)
        self.assertIn("Delta classification withheld by signed policy", script)
        self.assertIn('invokeHost("close_compare_session"', script)
        self.assertIn('invokeHost("get_reconcile_options"', script)
        self.assertIn('invokeHost("start_reconcile_review"', script)
        self.assertIn('invokeHost("choose_reconcile_destination"', script)
        self.assertIn('invokeHost("choose_reconcile_ancestor"', script)
        self.assertIn('invokeHost("prepare_reconcile"', script)
        self.assertIn('invokeHost("execute_reconcile"', script)
        self.assertIn('listen("capsule-reconcile-progress-v1"', script)
        self.assertIn('invokeHost("get_reconcile_operation"', script)
        self.assertIn('invokeHost("cancel_reconcile_operation"', script)
        self.assertIn('invokeHost("acknowledge_reconcile_result"', script)
        self.assertIn("reconcileStatusAfterStart(status.operation_token)", script)
        self.assertIn("oncePerReconcileOperation(operationToken", script)
        self.assertIn("reconcileFinalizations.get(operationToken)", script)
        self.assertIn("terminalEventFinalization", script)
        self.assertIn("globalThis.setTimeout(() =>", script)
        self.assertIn('invokeHost("choose_upgrade_release"', script)
        self.assertIn('invokeHost("choose_upgrade_destination"', script)
        self.assertIn('invokeHost("prepare_upgrade"', script)
        self.assertIn('invokeHost("execute_upgrade"', script)
        self.assertIn('listen("capsule-upgrade-progress-v1"', script)
        self.assertIn('invokeHost("get_upgrade_operation"', script)
        self.assertIn('invokeHost("cancel_upgrade_operation"', script)
        self.assertIn('invokeHost("acknowledge_upgrade_result"', script)
        self.assertIn("oncePerUpgradeOperation(operationToken", script)
        self.assertIn("upgradeFinalizations.get(operationToken)", script)
        self.assertIn("Sensitive values remain unavailable until the explicit reveal", script)
        self.assertIn('document.createElement("bdi")', script)
        self.assertIn('invokeHost("choose_copy_destination"', script)
        self.assertIn('invokeHost("preview_copy_profile"', script)
        self.assertIn('invokeHost("prepare_copy"', script)
        self.assertIn('invokeHost("execute_copy"', script)
        self.assertIn('listen("capsule-copy-progress-v1"', script)
        self.assertIn('invokeHost("get_copy_operation"', script)
        self.assertIn('invokeHost("cancel_copy_operation"', script)
        self.assertIn('invokeHost("acknowledge_copy_result"', script)
        self.assertIn('invokeHost("open_selected_capsule"', script)
        self.assertIn('invokeHost("recover_selected_capsule", { selectionId })', script)
        self.assertIn('invokeHost("first_open_decide", {', script)
        self.assertIn('currentReport?.selection_id ?? currentReport?.capsule?.overview?.selection_id', script)
        self.assertIn('startSigningPicker("select_signing_key_picker"', script)
        self.assertIn('startSigningPicker("select_signing_source_picker"', script)
        self.assertIn('startSigningPicker("select_signing_output_picker"', script)
        self.assertIn('invokeHost("prepare_signing",', script)
        self.assertIn('invokeHost("execute_signing",', script)
        self.assertIn('invokeHost("clear_signing_session")', script)
        self.assertIn("confirmation_application_digest", script)
        self.assertNotIn("private_key_hex", script)
        self.assertIn('action === "forget_current_decision"', script)
        self.assertIn("FORGET-CURRENT-DECISION", script)
        self.assertIn("authority and preserves other trust records", script)
        self.assertIn("Nothing was downloaded, staged, authorized, or installed", script)
        self.assertIn("Fulcio chain/SCT, Rekor proof, exact Sigstore identity", script)
        self.assertIn("It is not staged, authorized, or installed", script)
        self.assertIn("No update is treated as healthy", script)
        self.assertIn('status.mode === "read_only_unsafe_filesystem"', script)
        self.assertIn("Use Save a verified copy before editing", script)
        self.assertIn("report.transport_configured", script)
        self.assertIn("no complete compiled updater trust configuration", script)
        self.assertIn('selectPage("capabilities", { focus: false })', script)
        self.assertIn('localStorage.setItem("sqlite-capsule-theme"', script)
        self.assertIn('hostWindow()?.toggleMaximize()', script)
        permission = HOST_PERMISSION.read_text(encoding="utf-8")
        self.assertIn('"update_status"', permission)
        self.assertIn('"check_host_update"', permission)
        self.assertIn('"download_host_update"', permission)
        self.assertIn('"stage_host_update"', permission)
        self.assertIn('"signing_status"', permission)
        self.assertIn('"cabinet_status"', permission)
        self.assertIn('"choose_compare_capsule"', permission)
        self.assertIn('"start_compare"', permission)
        self.assertIn('"get_compare_page"', permission)
        self.assertIn('"reveal_compare_page"', permission)
        self.assertIn('"get_compare_application_detail"', permission)
        self.assertIn('"close_compare_session"', permission)
        self.assertIn('"get_reconcile_options"', permission)
        self.assertIn('"start_reconcile_review"', permission)
        self.assertIn('"choose_reconcile_destination"', permission)
        self.assertIn('"choose_reconcile_ancestor"', permission)
        self.assertIn('"prepare_reconcile"', permission)
        self.assertIn('"execute_reconcile"', permission)
        self.assertIn('"get_reconcile_operation"', permission)
        self.assertIn('"cancel_reconcile_operation"', permission)
        self.assertIn('"acknowledge_reconcile_result"', permission)
        self.assertIn('"choose_copy_destination"', permission)
        self.assertIn('"preview_copy_profile"', permission)
        self.assertIn('"cancel_copy_destination"', permission)
        self.assertIn('"prepare_copy"', permission)
        self.assertIn('"execute_copy"', permission)
        self.assertIn('"get_copy_operation"', permission)
        self.assertIn('"cancel_copy_operation"', permission)
        self.assertIn('"acknowledge_copy_result"', permission)
        self.assertIn('"choose_upgrade_release"', permission)
        self.assertIn('"choose_upgrade_destination"', permission)
        self.assertIn('"prepare_upgrade"', permission)
        self.assertIn('"execute_upgrade"', permission)
        self.assertIn('"get_upgrade_operation"', permission)
        self.assertIn('"cancel_upgrade_operation"', permission)
        self.assertIn('"acknowledge_upgrade_result"', permission)
        self.assertIn('"open_recent_capsule"', permission)
        self.assertIn('"open_selected_capsule"', permission)
        self.assertIn('"recover_selected_capsule"', permission)
        self.assertIn('"select_signing_key_picker"', permission)
        self.assertIn('"prepare_signing"', permission)
        self.assertIn('"execute_signing"', permission)
        self.assertIn('"clear_signing_session"', permission)
        self.assertNotIn('"prepare_update_installation"', permission)
        self.assertNotIn('"execute_update_installation"', permission)
        self.assertNotIn('"execute_update_rollback"', permission)
        self.assertNotIn("updater:default", permission)
        self.assertNotIn("updater:", permission)
        capability = json.loads(HOST_CAPABILITY.read_text(encoding="utf-8"))
        self.assertEqual(capability["webviews"], ["main"])
        self.assertIn("core:event:allow-listen", capability["permissions"])
        self.assertIn("core:event:allow-unlisten", capability["permissions"])
        self.assertNotIn("core:event:allow-emit", capability["permissions"])
        self.assertNotIn("core:event:allow-emit-to", capability["permissions"])
        self.assertIn("core:window:allow-minimize", capability["permissions"])
        self.assertIn("core:window:allow-toggle-maximize", capability["permissions"])
        self.assertIn("core:window:allow-close", capability["permissions"])
        self.assertIn("core:window:allow-start-dragging", capability["permissions"])

    def test_reconcile_renderer_contract_is_opaque_and_main_only(self) -> None:
        source = RECONCILE_FLOW.read_text(encoding="utf-8")
        self.assertIn("pub(crate) struct PrepareReconcileRequest", source)
        self.assertIn("pub review_token: String", source)
        self.assertIn("pub destination_token: String", source)
        self.assertIn("pub selection_tokens: Vec<String>", source)
        self.assertIn("pub ancestor_token: Option<String>", source)
        self.assertIn("pub resolution_tokens: Vec<String>", source)
        self.assertIn("pub(crate) struct ChooseReconcileAncestorRequest", source)
        self.assertIn("pub conflict_token: String", source)
        self.assertIn("pub resolution_token: String", source)
        request_block = source.split("pub(crate) struct PrepareReconcileRequest", 1)[1].split("}", 1)[0]
        for forbidden in (
            "PathBuf",
            "LifecyclePlan",
            "payload",
            "digest",
            "dataset_index",
            "table_index",
            "row_value",
            "sql",
        ):
            self.assertNotIn(forbidden, request_block)
        self.assertIn("HUMAN_REVIEW_LIFETIME: Duration = Duration::from_secs(5 * 60)", source)
        self.assertIn("EXECUTION_LIFETIME: Duration = Duration::from_secs(30)", source)
        capability = json.loads(HOST_CAPABILITY.read_text(encoding="utf-8"))
        self.assertEqual(capability["webviews"], ["main"])
        window = json.loads(TAURI_CONFIG.read_text(encoding="utf-8"))["app"]["windows"][0]
        self.assertGreaterEqual(
            window["height"],
            900,
            "the default trust-review window should fit the complete capabilities prompt",
        )
        self.assertLessEqual(window["minHeight"], window["height"])
        self.assertFalse(window["decorations"])
        self.assertTrue(window["shadow"])
        native_shell = NATIVE_SHELL.read_text(encoding="utf-8")
        child_csp_match = re.search(r'const CHILD_CSP: &str = "([^"]+)";', native_shell)
        self.assertIsNotNone(child_csp_match)
        child_csp = child_csp_match.group(1)
        self.assertIn("script-src 'self' 'wasm-unsafe-eval'", child_csp)
        self.assertNotIn("'unsafe-eval'", child_csp)
        self.assertIn(".with_focused(false)", native_shell)
        self.assertIn("WindowBuilder::new(app, CAPSULE_WINDOW_LABEL)", native_shell)
        self.assertIn(".maximized(true)", native_shell)
        self.assertIn(".visible(false)", native_shell)
        self.assertIn(".build(window)", native_shell)
        self.assertNotIn(".build_as_child(window)", native_shell)
        self.assertIn("focus_main_window(app)", native_shell)
        self.assertIn("handle_close_request(&app_for_capsule_close, api)", native_shell)
        self.assertIn("Ok(()) => app.exit(0)", native_shell)
        self.assertIn("schedule_forwarded_launch(app, args, cwd)", native_shell)
        self.assertIn("app.run_on_main_thread", native_shell)
        self.assertIn("drop(task);", native_shell)
        self.assertIn('"forget_current_decision"', native_shell)
        self.assertIn("app.manage(SigningState::default())", native_shell)
        self.assertIn("LoadedSigningKey::from_file(&path)", native_shell)
        self.assertIn("prepare_signing_copy(", native_shell)
        self.assertIn("prepared.sign(key)", native_shell)

    def test_upgrade_renderer_contract_is_opaque_and_main_only(self) -> None:
        source = UPGRADE_FLOW.read_text(encoding="utf-8")
        self.assertIn("pub(crate) struct PrepareUpgradeRequest", source)
        self.assertIn("pub selection_id: String", source)
        self.assertIn("pub candidate_token: String", source)
        self.assertIn("pub destination_token: String", source)
        request_block = source.split("pub(crate) struct PrepareUpgradeRequest", 1)[1].split("}", 1)[0]
        for forbidden in (
            "PathBuf",
            "LifecyclePlan",
            "publisher_key_id",
            "file_sha256",
            "application_digest",
            "sql",
        ):
            self.assertNotIn(forbidden, request_block)
        execute_block = source.split("pub(crate) struct ExecuteUpgradeRequest", 1)[1].split("}", 1)[0]
        self.assertIn("publisher_key_confirmed: bool", execute_block)
        self.assertIn("capability_changes_confirmed: bool", execute_block)
        self.assertNotIn("accepted_publisher_key_id", execute_block)
        self.assertIn("EXECUTION_LIFETIME: Duration = Duration::from_secs(30)", source)
        capability = json.loads(HOST_CAPABILITY.read_text(encoding="utf-8"))
        self.assertEqual(capability["webviews"], ["main"])
        native_shell = NATIVE_SHELL.read_text(encoding="utf-8")
        self.assertIn("app.manage(UpgradeState::default())", native_shell)
        self.assertIn("controller.prepare_for_close()", native_shell)

    def test_native_webdriver_gate_adds_no_application_ipc_surface(self) -> None:
        package = json.loads((ROOT / "package.json").read_text(encoding="utf-8"))
        self.assertEqual(
            package["scripts"]["test:native"],
            "wdio run tests/native/wdio.conf.mjs",
        )
        self.assertEqual(
            package["scripts"]["test:native:raw"],
            "node tests/native/raw-child.e2e.mjs",
        )
        self.assertEqual(
            package["scripts"]["test:native:window"],
            "node tests/native/standalone-window.e2e.mjs",
        )
        self.assertEqual(
            package["scripts"]["test:native:reconcile"],
            "node tests/native/reconcile.e2e.mjs",
        )
        self.assertEqual(
            package["scripts"]["test:native:reconcile-finalization"],
            "node tests/native/reconcile-finalization.test.mjs",
        )
        self.assertEqual(
            package["scripts"]["test:native:upgrade"],
            "node tests/native/upgrade.e2e.mjs",
        )
        for dependency in (
            "@wdio/cli",
            "@wdio/jasmine-framework",
            "@wdio/local-runner",
            "@wdio/spec-reporter",
        ):
            self.assertRegex(package["devDependencies"][dependency], r"^\d+\.\d+\.\d+$")

        preparer = (ROOT / "native/tools/prepare_native_e2e.py").read_text(
            encoding="utf-8"
        )
        self.assertIn('TAURI_DRIVER_VERSION = "2.0.6"', preparer)
        self.assertIn(
            'EDGE_TOOL_REVISION = "8c4b34f51b45f5cf08013366d703de464ab871d1"',
            preparer,
        )
        webdriver_config = (ROOT / "tests/native/wdio.conf.mjs").read_text(
            encoding="utf-8"
        )
        self.assertIn('"--native-driver", nativeDriver', webdriver_config)
        self.assertIn("SQLITE_CAPSULE_NATIVE_E2E_PATH: capsule", webdriver_config)
        self.assertIn(
            "SQLITE_CAPSULE_NATIVE_E2E_STATE_ROOT: stateRoot", webdriver_config
        )
        self.assertIn('path.join(root, ".tmp", "native-e2e-state")', webdriver_config)

        application_manifest = (
            ROOT / "native/desktop/src-tauri/Cargo.toml"
        ).read_text(encoding="utf-8")
        native_shell = NATIVE_SHELL.read_text(encoding="utf-8")
        self.assertIn("host_app_data_root_from_process", native_shell)
        self.assertIn("cfg!(debug_assertions)", native_shell)
        self.assertIn("SQLITE_CAPSULE_NATIVE_PARENT_E2E_PORT", native_shell)
        self.assertIn("SQLITE_CAPSULE_NATIVE_RAW_E2E_PORT", native_shell)
        self.assertIn("with_additional_browser_args", native_shell)
        self.assertIn('std::env::var_os("SQLITE_CAPSULE_NATIVE_E2E_STATE_ROOT")', native_shell)
        self.assertIn("if automation && let Some(root)", native_shell)
        permission = HOST_PERMISSION.read_text(encoding="utf-8")
        capability = HOST_CAPABILITY.read_text(encoding="utf-8")
        for content in (application_manifest, native_shell, permission, capability):
            self.assertNotIn("tauri-plugin-wdio", content)
            self.assertNotIn("wdio:", content)


if __name__ == "__main__":
    unittest.main()

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
NATIVE_ENTRYPOINT = ROOT / "native" / "desktop" / "src-tauri" / "src" / "main.rs"
HOST_CAPABILITY = (
    ROOT / "native" / "desktop" / "src-tauri" / "capabilities" / "host-shell.json"
)


class HostMarkupParser(HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.ids: list[str] = []
        self.references: list[tuple[str, str]] = []
        self.buttons: list[dict[str, str | None]] = []
        self._button_depth = 0
        self._button_text: list[str] = []

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

    def handle_data(self, data: str) -> None:
        if self._button_depth:
            self._button_text[-1] += data

    def handle_endtag(self, tag: str) -> None:
        if tag == "button" and self._button_depth:
            self.buttons[-1]["_text"] = self._button_text.pop().strip()
            self._button_depth -= 1


class NativeTrustedUiAccessibilityTests(unittest.TestCase):
    def test_packaged_windows_entrypoint_uses_the_gui_subsystem(self) -> None:
        entrypoint = NATIVE_ENTRYPOINT.read_text(encoding="utf-8")
        self.assertIn('all(not(debug_assertions), target_os = "windows")', entrypoint)
        self.assertIn('windows_subsystem = "windows"', entrypoint)

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
        self.assertIn('id="update-status" role="status" aria-live="polite"', html)
        self.assertIn("Separate application window · hidden until authorised", html)
        self.assertIn("opens maximized in its own native window", html)
        self.assertNotIn("Native child renderer occupies this fixed region", html)

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
        script = (UI / "app.js").read_text(encoding="utf-8")
        self.assertIn("verdict.focus()", script)
        self.assertIn("promptTitle.focus()", script)
        self.assertIn("openButton.focus()", script)
        self.assertIn("focusKeyFor(report)", script)
        self.assertIn("report.capsule?.source_sha256", script)
        self.assertIn("trustLabels", script)
        self.assertIn('invokeHost("update_status")', script)
        self.assertIn('invokeHost("check_host_update")', script)
        self.assertIn('invokeHost("download_host_update",', script)
        self.assertIn('invokeHost("stage_host_update",', script)
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
        permission = HOST_PERMISSION.read_text(encoding="utf-8")
        self.assertIn('"update_status"', permission)
        self.assertIn('"check_host_update"', permission)
        self.assertIn('"download_host_update"', permission)
        self.assertIn('"stage_host_update"', permission)
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
        native_shell = NATIVE_SHELL.read_text(encoding="utf-8")
        self.assertIn(".with_focused(false)", native_shell)
        self.assertIn("WindowBuilder::new(app, CAPSULE_WINDOW_LABEL)", native_shell)
        self.assertIn(".maximized(true)", native_shell)
        self.assertIn(".visible(false)", native_shell)
        self.assertIn(".build(window)", native_shell)
        self.assertNotIn(".build_as_child(window)", native_shell)
        self.assertIn("focus_main_window(&app_for_windows)", native_shell)
        self.assertIn("handle_close_request(&app_for_capsule_close, api)", native_shell)
        self.assertIn("Ok(()) => app.exit(0)", native_shell)
        self.assertIn("schedule_forwarded_launch(app, args, cwd)", native_shell)
        self.assertIn("app.run_on_main_thread", native_shell)
        self.assertIn("drop(task);", native_shell)
        self.assertIn('"forget_current_decision"', native_shell)

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

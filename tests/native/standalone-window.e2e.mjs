import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import net from "node:net";
import path from "node:path";
import { spawn, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../..");
const application = process.env.SQLITE_CAPSULE_NATIVE_APPLICATION
  || path.join(root, "native", "target", "debug", "sqlite-capsule-desktop.exe");
const capsule = process.env.SQLITE_CAPSULE_NATIVE_E2E_CAPSULE
  || path.join(root, "capsules", "diagram-studio.capsule.sqlite");
const stateRoot = path.join(root, ".tmp", "native-standalone-window-state");
const evidenceRoot = path.join(root, ".tmp", "native-standalone-window-evidence");
const hostLightScreenshotPath = path.join(evidenceRoot, "host-shell-light.png");
const hostDarkScreenshotPath = path.join(evidenceRoot, "host-shell-dark.png");
const hostSigningScreenshotPath = path.join(evidenceRoot, "publisher-signing.png");
const screenshotPath = path.join(evidenceRoot, "application-window.png");
const applicationTitle = "SQLite Capsule — application";

if (process.platform !== "win32") {
  throw new Error("standalone native-window acceptance is Windows-only");
}
if (!existsSync(application)) {
  throw new Error(`native debug application is absent: ${application}`);
}
if (!existsSync(capsule)) {
  throw new Error(`capsule is absent: ${capsule}`);
}

function sha256File(filePath) {
  return createHash("sha256").update(readFileSync(filePath)).digest("hex");
}

async function freePort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      server.close((error) => error ? reject(error) : resolve(address.port));
    });
  });
}

async function waitForEndpoint(port, label) {
  const endpoint = `http://127.0.0.1:${port}`;
  const deadline = Date.now() + 20_000;
  let lastError;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(`${endpoint}/json/version`);
      if (response.ok) return endpoint;
      lastError = new Error(`${label} returned HTTP ${response.status}`);
    } catch (error) {
      lastError = error;
    }
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  throw new Error(`${label} CDP endpoint did not start: ${lastError}`);
}

const windowInventorySource = [
  "using System;",
  "using System.Collections.Generic;",
  "using System.Runtime.InteropServices;",
  "using System.Text;",
  "public sealed class SQLiteCapsuleWindowRecord {",
  "  public long hwnd { get; set; }",
  "  public bool visible { get; set; }",
  "  public bool maximized { get; set; }",
  "  public int left { get; set; }",
  "  public int top { get; set; }",
  "  public int right { get; set; }",
  "  public int bottom { get; set; }",
  "  public string title { get; set; }",
  "}",
  "public static class SQLiteCapsuleWindowInventory {",
  "  private delegate bool EnumWindowsProc(IntPtr window, IntPtr state);",
  "  [StructLayout(LayoutKind.Sequential)] private struct Rect { public int Left; public int Top; public int Right; public int Bottom; }",
  "  [DllImport(\"user32.dll\")] private static extern bool EnumWindows(EnumWindowsProc callback, IntPtr state);",
  "  [DllImport(\"user32.dll\")] private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);",
  "  [DllImport(\"user32.dll\")] private static extern bool GetWindowRect(IntPtr window, out Rect rect);",
  "  [DllImport(\"user32.dll\")] private static extern bool IsWindowVisible(IntPtr window);",
  "  [DllImport(\"user32.dll\")] private static extern bool IsZoomed(IntPtr window);",
  "  [DllImport(\"user32.dll\", CharSet=CharSet.Unicode)] private static extern int GetWindowText(IntPtr window, StringBuilder text, int count);",
  "  [DllImport(\"user32.dll\")] private static extern bool PostMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);",
  "  public static SQLiteCapsuleWindowRecord[] Read(uint expectedProcessId) {",
  "    var rows = new List<SQLiteCapsuleWindowRecord>();",
  "    EnumWindows((window, state) => {",
  "      uint actualProcessId; GetWindowThreadProcessId(window, out actualProcessId);",
  "      if (actualProcessId == expectedProcessId) {",
  "        var title = new StringBuilder(512); GetWindowText(window, title, title.Capacity);",
  "        Rect rect; GetWindowRect(window, out rect);",
  "        rows.Add(new SQLiteCapsuleWindowRecord {",
  "          hwnd = window.ToInt64(), visible = IsWindowVisible(window), maximized = IsZoomed(window),",
  "          left = rect.Left, top = rect.Top, right = rect.Right, bottom = rect.Bottom, title = title.ToString()",
  "        });",
  "      }",
  "      return true;",
  "    }, IntPtr.Zero);",
  "    return rows.ToArray();",
  "  }",
  "  public static bool CloseTitle(uint expectedProcessId, string expectedTitleFragment) {",
  "    IntPtr target = IntPtr.Zero;",
  "    EnumWindows((window, state) => {",
  "      uint actualProcessId; GetWindowThreadProcessId(window, out actualProcessId);",
  "      var title = new StringBuilder(512); GetWindowText(window, title, title.Capacity);",
  "      if (actualProcessId == expectedProcessId && title.ToString().IndexOf(expectedTitleFragment, StringComparison.OrdinalIgnoreCase) >= 0) { target = window; return false; }",
  "      return true;",
  "    }, IntPtr.Zero);",
  "    return target != IntPtr.Zero && PostMessage(target, 0x0010, IntPtr.Zero, IntPtr.Zero);",
  "  }",
  "}",
].join("\n");

function powershellWindowCommand(statement) {
  return [
    "[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)",
    `$source = @'\n${windowInventorySource}\n'@`,
    "Add-Type -TypeDefinition $source",
    statement,
  ].join("\n");
}

function windowInventory(processId) {
  const result = spawnSync("powershell.exe", [
    "-NoProfile",
    "-NonInteractive",
    "-Command",
    powershellWindowCommand(`ConvertTo-Json -Compress -InputObject @([SQLiteCapsuleWindowInventory]::Read([uint32]${processId}))`),
  ], { cwd: root, encoding: "utf8", windowsHide: true, shell: false });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(result.stderr.trim() || "could not inventory native windows");
  const parsed = JSON.parse(result.stdout.trim());
  return Array.isArray(parsed) ? parsed : [parsed];
}

function closeWindow(processId, title) {
  const escapedTitle = title.replaceAll("'", "''");
  const result = spawnSync("powershell.exe", [
    "-NoProfile",
    "-NonInteractive",
    "-Command",
    powershellWindowCommand(`if (-not [SQLiteCapsuleWindowInventory]::CloseTitle([uint32]${processId}, '${escapedTitle}')) { exit 3 }`),
  ], { cwd: root, encoding: "utf8", windowsHide: true, shell: false });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(result.stderr.trim() || `could not close native window ${title}`);
}

async function waitForWindow(processId, titleFragment, predicate) {
  const deadline = Date.now() + 20_000;
  let lastInventory = [];
  while (Date.now() < deadline) {
    lastInventory = windowInventory(processId);
    const record = lastInventory.find((candidate) =>
      candidate.title?.toLowerCase().includes(titleFragment.toLowerCase())
    );
    if (record && predicate(record)) return record;
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  throw new Error(
    `native window did not reach the expected state: ${titleFragment}; inventory=${JSON.stringify(lastInventory)}`,
  );
}

async function waitForProcessExit(child, label) {
  if (child.exitCode !== null) return { code: child.exitCode, signal: child.signalCode };
  return Promise.race([
    new Promise((resolve) => child.once("exit", (code, signal) => resolve({ code, signal }))),
    new Promise((_, reject) => setTimeout(() => reject(new Error(`${label} did not exit`)), 20_000)),
  ]);
}

async function stopApplication(child) {
  if (!child || child.exitCode !== null) return;
  child.kill();
  await Promise.race([
    new Promise((resolve) => child.once("exit", resolve)),
    new Promise((resolve) => setTimeout(resolve, 2_000)),
  ]);
  if (child.exitCode === null) {
    spawnSync("taskkill", ["/PID", String(child.pid), "/T", "/F"], {
      stdio: "ignore",
      windowsHide: true,
      shell: false,
    });
  }
}

rmSync(stateRoot, { recursive: true, force: true });
rmSync(evidenceRoot, { recursive: true, force: true });
mkdirSync(stateRoot, { recursive: true });
mkdirSync(evidenceRoot, { recursive: true });

const sourceHashBefore = sha256File(capsule);
const parentPort = await freePort();
let rawPort = await freePort();
while (rawPort === parentPort) rawPort = await freePort();
let applicationProcess;
let parentBrowser;
let rawBrowser;
let stderr = "";

try {
  applicationProcess = spawn(application, [], {
    cwd: root,
    env: {
      ...process.env,
      SQLITE_CAPSULE_NATIVE_E2E_PATH: capsule,
      SQLITE_CAPSULE_NATIVE_E2E_STATE_ROOT: stateRoot,
      SQLITE_CAPSULE_NATIVE_PARENT_E2E_PORT: String(parentPort),
      SQLITE_CAPSULE_NATIVE_RAW_E2E_PORT: String(rawPort),
    },
    stdio: ["ignore", "ignore", "pipe"],
    windowsHide: false,
    shell: false,
  });
  applicationProcess.stderr.on("data", (chunk) => { stderr += chunk.toString(); });

  const parentEndpoint = await waitForEndpoint(parentPort, "trusted parent");
  const rawEndpoint = await waitForEndpoint(rawPort, "raw renderer");
  parentBrowser = await chromium.connectOverCDP(parentEndpoint);
  rawBrowser = await chromium.connectOverCDP(rawEndpoint);
  const parentPage = parentBrowser.contexts()[0]?.pages()[0];
  const rawPage = rawBrowser.contexts()[0]?.pages()[0];
  assert.ok(parentPage, "trusted parent page is absent");
  assert.ok(rawPage, "raw renderer page is absent");

  await parentPage.locator("#host-state").waitFor({ state: "visible" });
  await parentPage.waitForFunction(() => document.querySelector("#host-state")?.textContent !== "Verifying before open");
  assert.equal(await parentPage.locator("#host-state").textContent(), "Trust decision required · code locked");
  await parentPage.locator("button[data-page='boundary']").click();
  await parentPage.getByText("Separate application window · hidden until authorised").waitFor();
  assert.equal(await rawPage.title(), "Raw child renderer probe");

  const hiddenApplication = await waitForWindow(
    applicationProcess.pid,
    "application",
    (window) => window.visible === false,
  );
  const trustedHost = await waitForWindow(
    applicationProcess.pid,
    "trust review",
    () => true,
  );

  await parentPage.locator("button[data-page='capabilities']").click();
  const capabilityViewport = await parentPage.locator(".content-surface").evaluate((surface) => ({
    clientHeight: surface.clientHeight,
    scrollHeight: surface.scrollHeight,
  }));
  assert.ok(
    capabilityViewport.scrollHeight <= capabilityViewport.clientHeight + 1,
    `capabilities page unexpectedly requires vertical scrolling: ${JSON.stringify(capabilityViewport)}`,
  );
  await parentPage.locator("button[data-page='signing']").click();
  assert.equal(await parentPage.locator("#page-title").textContent(), "Publisher signing");
  assert.match(await parentPage.locator("#signing-status").textContent(), /Rust memory only/);
  assert.equal(await parentPage.locator("#signing-key-button").isEnabled(), true);
  assert.equal(await parentPage.locator("#signing-source-button").isEnabled(), true);
  assert.equal(await parentPage.locator("#signing-output-button").isEnabled(), false);
  assert.equal(await parentPage.locator("#signing-prepare-button").isEnabled(), false);
  assert.equal(await parentPage.locator("#signing-execute-button").isEnabled(), false);
  const signingStatus = await parentPage.evaluate(() => globalThis.__TAURI__.core.invoke("signing_status"));
  assert.deepEqual(signingStatus, {
    key: null,
    source: null,
    output: null,
    preview: null,
    busy: false,
  });
  assert.equal(JSON.stringify(signingStatus).includes("private"), false);
  assert.equal(JSON.stringify(signingStatus).includes("keyPath"), false);
  await parentPage.screenshot({ path: hostSigningScreenshotPath, animations: "disabled" });
  await parentPage.locator("button[data-page='capabilities']").click();
  await parentPage.locator("button[data-action='allow_once']").click();
  await parentPage.waitForFunction(() => document.querySelector("#host-state")?.textContent?.includes("application running"));
  await rawPage.waitForFunction(() => document.title === "Diagram Studio — SQLite Capsule");

  await parentPage.locator("button[data-page='boundary']").click();
  const originalTheme = await parentPage.locator("[data-theme-option][aria-pressed='true']").getAttribute("data-theme-option");
  await parentPage.locator("button[data-theme-option='light']").click();
  await parentPage.waitForFunction(() => document.documentElement.dataset.theme === "light");
  await parentPage.screenshot({ path: hostLightScreenshotPath, animations: "disabled" });
  await parentPage.locator("button[data-theme-option='dark']").click();
  await parentPage.waitForFunction(() => document.documentElement.dataset.theme === "dark");
  await parentPage.screenshot({ path: hostDarkScreenshotPath, animations: "disabled" });
  await parentPage.locator(`button[data-theme-option='${originalTheme}']`).click();

  const visibleApplication = await waitForWindow(
    applicationProcess.pid,
    "application",
    (window) => window.visible === true && window.maximized === true,
  );
  const applicationWidth = visibleApplication.right - visibleApplication.left;
  const applicationHeight = visibleApplication.bottom - visibleApplication.top;
  const hostWidth = trustedHost.right - trustedHost.left;
  const hostHeight = trustedHost.bottom - trustedHost.top;
  assert.ok(applicationWidth > hostWidth, "application window did not gain more horizontal space than the trust shell");
  assert.ok(applicationHeight > hostHeight, "application window did not gain more vertical space than the trust shell");

  const rawViewport = await rawPage.evaluate(() => ({
    innerWidth,
    innerHeight,
    tauri: typeof globalThis.__TAURI__,
    internals: typeof globalThis.__TAURI_INTERNALS__,
  }));
  assert.equal(rawViewport.tauri, "undefined");
  assert.equal(rawViewport.internals, "undefined");
  assert.ok(rawViewport.innerWidth >= 1200, `raw renderer width is unexpectedly small: ${rawViewport.innerWidth}`);
  assert.ok(rawViewport.innerHeight >= 700, `raw renderer height is unexpectedly small: ${rawViewport.innerHeight}`);
  await rawPage.screenshot({ path: screenshotPath, animations: "disabled" });

  closeWindow(applicationProcess.pid, "application");
  const exit = await waitForProcessExit(applicationProcess, "application-window close");
  assert.equal(exit.code, 0, `application exited through standalone-window close with ${JSON.stringify(exit)}`);
  assert.equal(sha256File(capsule), sourceHashBefore, "standalone-window acceptance changed the source capsule");

  process.stdout.write(`${JSON.stringify({
    ok: true,
    sourceCapsuleSha256: sourceHashBefore,
    hiddenApplication,
    trustedHost,
    capabilityViewport,
    visibleApplication,
    rawViewport,
    closeRequestedThrough: applicationTitle,
    hostLightScreenshot: hostLightScreenshotPath,
    hostDarkScreenshot: hostDarkScreenshotPath,
    hostSigningScreenshot: hostSigningScreenshotPath,
    screenshot: screenshotPath,
  }, null, 2)}\n`);
} catch (error) {
  if (stderr.trim()) process.stderr.write(stderr);
  throw error;
} finally {
  await parentBrowser?.close().catch(() => {});
  await rawBrowser?.close().catch(() => {});
  await stopApplication(applicationProcess);
  rmSync(stateRoot, { recursive: true, force: true });
}

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  renameSync,
  rmSync,
  statSync,
} from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { spawn, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../..");
const nativeRoot = path.join(root, "native");
const suffix = process.platform === "win32" ? ".exe" : "";
const application = process.env.SQLITE_CAPSULE_NATIVE_APPLICATION
  || path.join(nativeRoot, "target", "debug", `sqlite-capsule-desktop${suffix}`);
const sourceCapsule = process.env.SQLITE_CAPSULE_NATIVE_E2E_CAPSULE
  || path.join(root, "capsules", "diagram-studio.capsule.sqlite");
const stateRoot = path.join(root, ".tmp", "native-raw-e2e-state");
const disposableRoot = path.join(root, ".tmp", "native-raw-e2e-capsule");
const capsule = path.join(disposableRoot, "diagram-studio.capsule.sqlite");
const replacementCandidate = path.join(disposableRoot, "replacement-candidate.sqlitecapsule");
const restoreRoot = path.join(stateRoot, "restored");
const restoredCapsule = path.join(restoreRoot, "restored.sqlitecapsule");
const supportRoot = path.join(stateRoot, "support");
const supportBundlePath = path.join(supportRoot, "sqlite-capsule-support.json");
const windowsSaveDialogHelper = path.join(here, "windows-save-dialog.ps1");
const cargo = resolveCargo();

function sha256File(filePath) {
  return createHash("sha256").update(readFileSync(filePath)).digest("hex");
}

function comparableWindowsPath(filePath) {
  let resolved = path.resolve(filePath);
  if (resolved.startsWith("\\\\?\\UNC\\")) resolved = `\\\\${resolved.slice(8)}`;
  else if (resolved.startsWith("\\\\?\\")) resolved = resolved.slice(4);
  return resolved.toLowerCase();
}

function resolveCargo() {
  const candidates = [
    process.env.CARGO,
    process.env.CARGO_HOME && path.join(process.env.CARGO_HOME, "bin", `cargo${suffix}`),
    process.env.USERPROFILE && path.join(process.env.USERPROFILE, ".cargo", "bin", `cargo${suffix}`),
    path.join(os.homedir(), ".cargo", "bin", `cargo${suffix}`),
    `cargo${suffix}`,
  ].filter(Boolean);
  return candidates.find((candidate) => candidate === `cargo${suffix}` || existsSync(candidate));
}

function checked(command, args, cwd) {
  const result = spawnSync(command, args, { cwd, stdio: "inherit", shell: false });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} ${args.join(" ")} exited ${result.status}`);
}

function persistedNodeLabel(filePath, nodeId) {
  const script = [
    "import sqlite3, sys",
    "connection = sqlite3.connect(f'file:{sys.argv[1]}?mode=ro', uri=True)",
    "row = connection.execute('SELECT label FROM diagram_node WHERE id = ?', (sys.argv[2],)).fetchone()",
    "connection.close()",
    "value = row[0] if row else ''",
    "sys.stdout.buffer.write(value.encode('utf-8'))",
  ].join("\n");
  const result = spawnSync("python", ["-c", script, filePath, nodeId], {
    cwd: root,
    encoding: "utf8",
    shell: false,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(result.stderr.trim() || "could not read persisted node label");
  return result.stdout.trim();
}

function persistedHistoryCursor(filePath) {
  const script = [
    "import sqlite3, sys",
    "connection = sqlite3.connect(f'file:{sys.argv[1]}?mode=ro', uri=True)",
    "row = connection.execute(\"SELECT cursor FROM diagram_history WHERE diagram_id = 'diagram-main'\").fetchone()",
    "connection.close()",
    "print(row[0] if row else '')",
  ].join("\n");
  const result = spawnSync("python", ["-c", script, filePath], {
    cwd: root,
    encoding: "utf8",
    shell: false,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(result.stderr.trim() || "could not read persisted history cursor");
  return Number(result.stdout.trim());
}

async function renameNodeThroughBridge(page, { nodeId, fromLabel, toLabel, cursor, operationId }) {
  return page.evaluate((request) => globalThis.SQLiteCapsuleClient.write("node.rename", {
    operation_id: request.operationId,
    diagram_id: "diagram-main",
    expected_cursor: request.cursor,
    id: request.nodeId,
    from_label: request.fromLabel,
    to_label: request.toLabel,
  }), { nodeId, fromLabel, toLabel, cursor, operationId });
}

function externallyRenameNode(filePath, nodeId, label) {
  const script = [
    "import sqlite3, sys",
    "connection = sqlite3.connect(sys.argv[1])",
    "connection.execute('UPDATE diagram_node SET label = ? WHERE id = ?', (sys.argv[3], sys.argv[2]))",
    "connection.commit()",
    "connection.close()",
  ].join("\n");
  const result = spawnSync("python", ["-c", script, filePath, nodeId, label], {
    cwd: root,
    encoding: "utf8",
    shell: false,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(result.stderr.trim() || "could not make external SQLite change");
}

function leaveHotRollbackJournal(filePath) {
  const script = [
    "import os, sqlite3, sys",
    "connection = sqlite3.connect(sys.argv[1])",
    "connection.execute('PRAGMA journal_mode=DELETE').fetchone()",
    "connection.execute('PRAGMA synchronous=FULL')",
    "connection.execute('PRAGMA cache_size=1')",
    "connection.execute('BEGIN IMMEDIATE')",
    "value = 'CRASH-PROBE-' + ('x' * 2000000)",
    "connection.execute(\"UPDATE diagram_document SET description = ? WHERE id = 'diagram-main'\", (value,))",
    "os._exit(97)",
  ].join("\n");
  const result = spawnSync("python", ["-c", script, filePath], {
    cwd: root,
    encoding: "utf8",
    shell: false,
  });
  if (result.error) throw result.error;
  assert.equal(result.status, 97, result.stderr.trim() || "crash worker did not exit at the fault point");
  const journal = `${filePath}-journal`;
  assert.ok(existsSync(journal), "crash worker did not leave a rollback journal");
  assert.ok(statSync(journal).size > 512, "rollback journal is too small to be a hot candidate");
  return journal;
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

async function waitForPath(filePath, label) {
  const deadline = Date.now() + 20_000;
  while (Date.now() < deadline) {
    if (existsSync(filePath)) return;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error(`${label} was not created: ${filePath}`);
}

async function stopApplication(child) {
  if (!child || child.exitCode !== null) return;
  child.kill();
  await Promise.race([
    new Promise((resolve) => child.once("exit", resolve)),
    new Promise((resolve) => setTimeout(resolve, 2_000)),
  ]);
  if (child.exitCode === null && process.platform === "win32") {
    spawnSync("taskkill", ["/PID", String(child.pid), "/T", "/F"], {
      stdio: "ignore",
      windowsHide: true,
      shell: false,
    });
  }
}

async function waitForProcessExit(child, label) {
  if (child.exitCode !== null) return { code: child.exitCode, signal: child.signalCode };
  return Promise.race([
    new Promise((resolve) => child.once("exit", (code, signal) => resolve({ code, signal }))),
    new Promise((_, reject) => setTimeout(
      () => reject(new Error(`${label} did not exit at the requested durable fault point`)),
      20_000,
    )),
  ]);
}

function requestNativeWindowClose(processId) {
  const source = [
    "using System;",
    "using System.Runtime.InteropServices;",
    "public static class SQLiteCapsuleWindowCloser {",
    "  private delegate bool EnumWindowsProc(IntPtr window, IntPtr state);",
    "  [DllImport(\"user32.dll\")] private static extern bool EnumWindows(EnumWindowsProc callback, IntPtr state);",
    "  [DllImport(\"user32.dll\")] private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);",
    "  [DllImport(\"user32.dll\")] private static extern bool PostMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);",
    "  public static bool Close(uint expectedProcessId) {",
    "    IntPtr target = IntPtr.Zero;",
    "    EnumWindows((window, state) => {",
    "      uint actualProcessId;",
    "      GetWindowThreadProcessId(window, out actualProcessId);",
    "      if (actualProcessId == expectedProcessId) { target = window; return false; }",
    "      return true;",
    "    }, IntPtr.Zero);",
    "    return target != IntPtr.Zero && PostMessage(target, 0x0010, IntPtr.Zero, IntPtr.Zero);",
    "  }",
    "}",
  ].join("\n");
  const script = [
    `$source = @'\n${source}\n'@`,
    "Add-Type -TypeDefinition $source",
    `if (-not [SQLiteCapsuleWindowCloser]::Close([uint32]${processId})) { exit 3 }`,
  ].join("\n");
  const result = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", script], {
    cwd: root,
    encoding: "utf8",
    windowsHide: true,
    shell: false,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(result.stderr.trim() || `could not request native window close: ${result.status}`);
  }
}

function driveWindowsSaveDialog(processId, destination, isolatedStateRoot) {
  const result = spawnSync(
    "powershell.exe",
    [
      "-NoProfile",
      "-STA",
      "-ExecutionPolicy",
      "Bypass",
      "-File",
      windowsSaveDialogHelper,
      "-HostProcessId",
      String(processId),
      "-Destination",
      destination,
      "-StateRoot",
      isolatedStateRoot,
      "-TimeoutSeconds",
      "20",
    ],
    {
      cwd: root,
      encoding: "utf8",
      timeout: 30_000,
      windowsHide: true,
      shell: false,
    },
  );
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(result.stderr.trim() || result.stdout.trim() || `save-dialog helper exited ${result.status}`);
  }
  const output = result.stdout.trim().split(/\r?\n/).filter(Boolean).at(-1);
  if (!output) throw new Error("save-dialog helper returned no report");
  return JSON.parse(output);
}

if (process.platform !== "win32") {
  throw new Error("raw-child native E2E is Windows-only until public platform runners are available");
}
checked("python", ["tools/capsule.py", "verify", sourceCapsule], root);
const sourceHashBefore = sha256File(sourceCapsule);
checked(cargo, ["build", "-p", "sqlite-capsule-desktop"], nativeRoot);
assert.ok(existsSync(application), `native debug application is missing: ${application}`);
if (process.argv.includes("--picker-only")) {
  await runRestorePickerScenario();
  process.exit(0);
}
rmSync(stateRoot, { recursive: true, force: true });
rmSync(disposableRoot, { recursive: true, force: true });
mkdirSync(disposableRoot, { recursive: true });
mkdirSync(stateRoot, { recursive: true });
mkdirSync(restoreRoot, { recursive: true });
mkdirSync(supportRoot, { recursive: true });
copyFileSync(sourceCapsule, capsule);
const disposableHashBefore = sha256File(capsule);
assert.equal(disposableHashBefore, sourceHashBefore, "disposable capsule copy differs before launch");
const journal = leaveHotRollbackJournal(capsule);
const dirtyCapsuleHash = sha256File(capsule);
const journalHash = sha256File(journal);

const parentPort = await freePort();
let rawPort = await freePort();
while (rawPort === parentPort) rawPort = await freePort();
let applicationProcess;
let parentBrowser;
let rawBrowser;
let stderr = "";
let parentPage;
let rawPage;
const rawConsole = [];
const rawPageErrors = [];
const rawRequestFailures = [];
try {
  applicationProcess = spawn(application, [], {
    cwd: root,
    env: {
      ...process.env,
      SQLITE_CAPSULE_NATIVE_PARENT_E2E_PORT: String(parentPort),
      SQLITE_CAPSULE_NATIVE_RAW_E2E_PORT: String(rawPort),
      SQLITE_CAPSULE_NATIVE_E2E_PATH: capsule,
      SQLITE_CAPSULE_NATIVE_E2E_STATE_ROOT: stateRoot,
      SQLITE_CAPSULE_NATIVE_E2E_RESTORE_PATH: restoredCapsule,
      SQLITE_CAPSULE_NATIVE_E2E_SUPPORT_PATH: supportBundlePath,
    },
    stdio: ["ignore", "ignore", "pipe"],
    windowsHide: true,
    shell: false,
  });
  applicationProcess.stderr.setEncoding("utf8");
  applicationProcess.stderr.on("data", (chunk) => { stderr += chunk; });

  const parentEndpoint = await waitForEndpoint(parentPort, "trusted parent");
  const rawEndpoint = await waitForEndpoint(rawPort, "raw child");
  parentBrowser = await chromium.connectOverCDP(parentEndpoint);
  rawBrowser = await chromium.connectOverCDP(rawEndpoint);
  parentPage = parentBrowser.contexts()[0]?.pages()[0];
  rawPage = rawBrowser.contexts()[0]?.pages()[0];
  assert.ok(parentPage, "trusted parent page is absent");
  assert.ok(rawPage, "raw child page is absent");
  rawPage.on("console", (message) => rawConsole.push(`${message.type()}: ${message.text()}`));
  rawPage.on("pageerror", (error) => rawPageErrors.push(error.message));
  rawPage.on("requestfailed", (request) => {
    rawRequestFailures.push(`${request.url()}: ${request.failure()?.errorText ?? "unknown error"}`);
  });

  await parentPage.locator("#host-state").waitFor({ state: "visible" });
  await parentPage.waitForFunction(() => document.querySelector("#host-state")?.textContent !== "Verifying before open");
  assert.equal(await parentPage.locator("#host-state").textContent(), "Trust decision required · code locked");
  const recoveredStartup = await parentPage.evaluate(() => globalThis.__TAURI__.core.invoke("startup_report"));
  assert.equal(recoveredStartup.recovery?.sqlite_recovery_attempted, true);
  assert.equal(recoveredStartup.recovery?.rollback_journal_hot_candidate_before, true);
  assert.equal(recoveredStartup.recovery?.rollback_journal_present_after, false);
  assert.equal(recoveredStartup.recovery?.rollback_journal_sha256_before, journalHash);
  assert.notEqual(recoveredStartup.recovery?.source_sha256_before, recoveredStartup.recovery?.source_sha256_after);
  assert.equal(existsSync(journal), false, "SQLite did not clear the recovered rollback journal");
  assert.notEqual(sha256File(capsule), dirtyCapsuleHash, "SQLite recovery did not change the spilled capsule bytes");
  assert.match(await parentPage.locator("#identity-details").textContent(), /SQLite recovery attempted/);
  assert.equal(await rawPage.title(), "Raw child renderer probe");
  await rawPage.locator("#decision").waitFor({ state: "visible" });
  await rawPage.waitForFunction(() => document.querySelector("#decision")?.textContent?.startsWith("PASS"));
  assert.equal(await rawPage.locator("#decision").textContent(), "PASS · no native handler");
  assert.deepEqual(
    await rawPage.evaluate(() => ({
      tauri: typeof globalThis.__TAURI__,
      internals: typeof globalThis.__TAURI_INTERNALS__,
    })),
    { tauri: "undefined", internals: "undefined" },
  );

  await parentPage.locator("button[data-action='deny']").click();
  await parentPage.waitForFunction(() => document.querySelector("#verdict strong")?.textContent === "Execution is blocked");
  assert.equal(await parentPage.locator("#boundary-title").textContent(), "Application window · executable assets locked");
  await rawPage.waitForURL(/\/__host\/locked$/);
  await rawPage.waitForFunction(() => document.title === "Raw child renderer probe");
  assert.equal(await rawPage.title(), "Raw child renderer probe");

  await parentPage.locator("button[data-page='admin']").click();
  const confirmation = new Promise((resolve, reject) => {
    parentPage.once("dialog", async (dialog) => {
      try {
        assert.match(dialog.message(), /FORGET-CURRENT-DECISION/);
        await dialog.accept("FORGET-CURRENT-DECISION");
        resolve();
      } catch (error) {
        reject(error);
      }
    });
  });
  await parentPage.locator("#forget-decision-button").click();
  await confirmation;
  await parentPage.waitForFunction(() => document.querySelector("#verdict strong")?.textContent === "Your decision is required");
  assert.match(await parentPage.locator("#admin-output").textContent(), /"authority_granted": false/);
  await rawPage.waitForFunction(() => document.title === "Raw child renderer probe");
  assert.equal(await rawPage.title(), "Raw child renderer probe");

  await parentPage.locator("button[data-page='capabilities']").click();
  await parentPage.locator("button[data-action='allow_once']").click();
  await parentPage.waitForFunction(() => document.querySelector("#host-state")?.textContent?.includes("application running"));
  assert.match(await parentPage.locator("#verdict").textContent(), /SQLite performed rollback-journal recovery/);
  await rawPage.waitForURL(/\/app\/index\.html$/);
  await rawPage.locator("#app[aria-busy='false']").waitFor({ state: "attached", timeout: 20_000 });
  assert.equal(await rawPage.locator("#diagram-title").textContent(), "A SQLite file that carries its own application");
  assert.equal(await rawPage.locator("#save-state").textContent(), "Local · SQLite");
  assert.equal(await rawPage.locator("#scene-count").textContent(), "5");
  assert.equal(await rawPage.locator(".node").count(), 12);
  assert.deepEqual(
    await rawPage.evaluate(() => ({
      tauri: typeof globalThis.__TAURI__,
      internals: typeof globalThis.__TAURI_INTERNALS__,
    })),
    { tauri: "undefined", internals: "undefined" },
  );
  assert.equal(await parentPage.locator("#boundary-title").textContent(), "Application window · verified assets · exact named bridge");

  const nodeId = "node-agent-prompt";
  const originalLabel = "One-sentence agent prompt";
  const persistedLabel = `${originalLabel} · native raw E2E`;
  await rawPage.locator(`[data-id='${nodeId}']`).click();
  await rawPage.locator("#node-label-input").fill(persistedLabel);
  await rawPage.locator("#save-node-label").click();
  await rawPage.waitForFunction(() => document.querySelector("#save-state")?.textContent === "Saved in SQLite");
  const nodesAfterWrite = await rawPage.evaluate(() => globalThis.SQLiteCapsuleClient.read(
    "diagram.nodes",
    { diagram_id: "diagram-main" },
  ));
  assert.equal(nodesAfterWrite.find((node) => node.id === nodeId)?.label, persistedLabel);
  const lifecycleAfterWrite = await parentPage.evaluate(() => globalThis.__TAURI__.core.invoke("lifecycle_status"));
  assert.equal(lifecycleAfterWrite.active, true);
  assert.equal(lifecycleAfterWrite.writable, true);
  assert.equal(lifecycleAfterWrite.mode, "writable");
  assert.ok(lifecycleAfterWrite.backup?.backup_id, "verified pre-write backup is absent");
  assert.ok(lifecycleAfterWrite.backup_inventory.verified.length >= 1, "verified backup inventory is empty");
  assert.deepEqual(lifecycleAfterWrite.backup_inventory.incomplete_artifacts, []);
  assert.deepEqual(lifecycleAfterWrite.backup_inventory.invalid_artifacts, []);
  assert.equal(persistedNodeLabel(capsule, nodeId), persistedLabel);
  checked("python", ["tools/capsule.py", "verify", capsule], root);

  assert.equal(existsSync(supportBundlePath), false);
  await parentPage.locator("button[data-page='admin']").click();
  await parentPage.locator("#support-button").click();
  await waitForPath(supportBundlePath, "redacted support bundle");
  await parentPage.waitForFunction(
    () => document.querySelector("#admin-output")?.textContent === "Redacted support bundle exported.",
  );
  const supportText = readFileSync(supportBundlePath, "utf8");
  const supportBundle = JSON.parse(supportText);
  assert.equal(supportBundle.format, "org.sqlite-capsule.support-bundle/0.2");
  assert.equal(supportBundle.platform, "windows");
  assert.equal(supportBundle.architecture, "x86_64");
  assert.deepEqual(supportBundle.content_policy, {
    capsule_controlled_text: "untrusted-data-only",
    host_severity_source: "host-owned-structured-fields-only",
    embedded_instructions_executed: false,
    capsule_database_bytes_included: false,
    trust_store_bytes_included: false,
    selected_file_contents_included: false,
    shutdown_tokens_included: false,
    private_keys_included: false,
  });
  assert.equal(supportBundle.startup.capsule.identity.canonical_path, "redacted");
  assert.equal(supportBundle.lifecycle.active, true);
  assert.equal(supportBundle.lifecycle.writable, true);
  assert.equal(supportBundle.lifecycle.mode, "writable");
  assert.equal(supportBundle.lifecycle.backup.backup_id, lifecycleAfterWrite.backup.backup_id);
  for (const forbidden of [
    capsule,
    path.basename(capsule),
    disposableRoot,
    stateRoot,
    sourceCapsule,
    persistedLabel,
  ]) {
    assert.equal(supportText.includes(forbidden), false, `support bundle leaked ${forbidden}`);
  }
  for (const forbidden of ['"public_key"', '"private_key"', '"shutdown_token"', "SQLite format 3"]) {
    assert.equal(supportText.includes(forbidden), false, `support bundle included ${forbidden}`);
  }
  const supportHash = sha256File(supportBundlePath);
  await parentPage.locator("#support-button").click();
  await parentPage.waitForFunction(
    () => document.querySelector("#admin-output")?.textContent?.includes("refuses to replace an existing file"),
  );
  assert.equal(sha256File(supportBundlePath), supportHash, "support re-export replaced existing output");

  copyFileSync(sourceCapsule, replacementCandidate);
  const replacementCandidateHash = sha256File(replacementCandidate);
  const activeHashBeforeReplacement = sha256File(capsule);
  assert.notEqual(
    replacementCandidateHash,
    activeHashBeforeReplacement,
    "replacement candidate does not differ from the active written capsule",
  );
  let replacementError;
  try {
    renameSync(replacementCandidate, capsule);
  } catch (error) {
    replacementError = error;
  }
  assert.ok(replacementError, "Windows allowed an external process to replace the pinned capsule path");
  assert.ok(
    ["EACCES", "EBUSY", "EPERM"].includes(replacementError.code),
    `unexpected replacement-denial error: ${replacementError.code || replacementError}`,
  );
  assert.equal(existsSync(replacementCandidate), true, "failed replacement consumed its candidate");
  assert.equal(
    sha256File(capsule),
    activeHashBeforeReplacement,
    "failed replacement changed the active capsule bytes",
  );
  assert.equal(persistedNodeLabel(capsule, nodeId), persistedLabel);
  const nodesAfterBlockedReplacement = await rawPage.evaluate(() => globalThis.SQLiteCapsuleClient.read(
    "diagram.nodes",
    { diagram_id: "diagram-main" },
  ));
  assert.equal(nodesAfterBlockedReplacement.find((node) => node.id === nodeId)?.label, persistedLabel);
  const lifecycleAfterBlockedReplacement = await parentPage.evaluate(
    () => globalThis.__TAURI__.core.invoke("lifecycle_status"),
  );
  assert.equal(lifecycleAfterBlockedReplacement.active, true);
  assert.equal(lifecycleAfterBlockedReplacement.writable, true);
  assert.equal(lifecycleAfterBlockedReplacement.mode, "writable");
  rmSync(replacementCandidate, { force: true });
  assert.equal(existsSync(replacementCandidate), false, "replacement candidate cleanup failed");

  const externalLabel = `${originalLabel} · external conflict E2E`;
  externallyRenameNode(capsule, nodeId, externalLabel);
  assert.equal(persistedNodeLabel(capsule, nodeId), externalLabel);
  const conflictError = await rawPage.evaluate(async () => {
    try {
      await globalThis.SQLiteCapsuleClient.read("diagram.nodes", { diagram_id: "diagram-main" });
      return null;
    } catch (error) {
      return String(error);
    }
  });
  assert.match(conflictError || "", /capsule changed outside this host session/);
  const lifecycleAfterConflict = await parentPage.evaluate(() => globalThis.__TAURI__.core.invoke("lifecycle_status"));
  assert.equal(lifecycleAfterConflict.active, false);
  assert.equal(lifecycleAfterConflict.writable, false);
  assert.equal(lifecycleAfterConflict.mode, "conflict_closed");
  assert.equal(lifecycleAfterConflict.backup?.backup_id, lifecycleAfterWrite.backup.backup_id);
  assert.equal(lifecycleAfterConflict.backup?.sha256, lifecycleAfterWrite.backup.sha256);
  assert.deepEqual(lifecycleAfterConflict.backup_inventory.incomplete_artifacts, []);
  assert.deepEqual(lifecycleAfterConflict.backup_inventory.invalid_artifacts, []);
  await rawPage.waitForURL(/\/__host\/locked$/, { timeout: 10_000 });
  await rawPage.waitForFunction(() => document.title === "Raw child renderer probe");
  await parentPage.waitForFunction(() => document.querySelector("#host-state")?.textContent === "Session conflict · renderer locked");
  assert.match(await parentPage.locator("#lifecycle-status").textContent(), /no silent merge occurred/);
  assert.equal(await parentPage.locator("#restore-button").isEnabled(), true);
  assert.equal(
    await parentPage.locator("#boundary-title").textContent(),
    "Application window · conflict closed · executable assets locked",
  );
  assert.equal(await parentPage.locator("#read-only-button").isEnabled(), true);
  await parentPage.locator("button[data-page='protection']").click();
  await parentPage.locator("#read-only-button").click();
  await parentPage.waitForFunction(() => document.querySelector("#host-state")?.textContent === "Trust decision required · code locked");
  await rawPage.waitForURL(/\/__host\/locked$/, { timeout: 10_000 });
  assert.match(
    await parentPage.locator("#lifecycle-action-status").textContent(),
    /authorize it again before any code is released/,
  );
  const reportAfterReadOnlyRequest = await parentPage.evaluate(() => globalThis.__TAURI__.core.invoke("startup_report"));
  assert.equal(reportAfterReadOnlyRequest.capsule?.assets_released, false);
  assert.equal(reportAfterReadOnlyRequest.capsule?.decision.trust_state, "structurally_verified_unsigned");
  const lifecycleAfterReadOnlyRequest = await parentPage.evaluate(() => globalThis.__TAURI__.core.invoke("lifecycle_status"));
  assert.equal(lifecycleAfterReadOnlyRequest.active, false);
  assert.equal(lifecycleAfterReadOnlyRequest.writable, false);
  assert.equal(lifecycleAfterReadOnlyRequest.mode, "conflict_closed");
  assert.equal(lifecycleAfterReadOnlyRequest.backup?.backup_id, lifecycleAfterWrite.backup.backup_id);
  assert.equal(await parentPage.locator("#restore-button").isEnabled(), true);
  assert.equal(persistedNodeLabel(capsule, nodeId), externalLabel);
  assert.equal(existsSync(restoredCapsule), false);
  await parentPage.locator("button[data-page='protection']").click();
  await parentPage.locator("#restore-button").click();
  await waitForPath(restoredCapsule, "verified restore output");
  await parentPage.waitForFunction(() => document.querySelector("#lifecycle-action-status")?.textContent?.includes("Verified copy restored"));
  await rawPage.waitForURL(/\/__host\/locked$/, { timeout: 10_000 });
  const restoredReport = await parentPage.evaluate(() => globalThis.__TAURI__.core.invoke("startup_report"));
  assert.equal(restoredReport.stage, "first-open");
  assert.equal(restoredReport.capsule?.assets_released, false);
  assert.match(restoredReport.capsule?.identity.canonical_path || "", /restored\.sqlitecapsule$/);
  assert.equal(restoredReport.capsule?.source_sha256, sha256File(restoredCapsule));
  assert.equal(persistedNodeLabel(restoredCapsule, nodeId), originalLabel);
  assert.equal(existsSync(`${restoredCapsule}.capsule-restore-in-progress`), false);
  checked("python", ["tools/capsule.py", "verify", restoredCapsule], root);
  checked("python", ["tools/capsule.py", "verify", capsule], root);

  const sourceHashAfter = sha256File(sourceCapsule);
  const disposableHashAfter = sha256File(capsule);
  assert.equal(sourceHashAfter, sourceHashBefore, "raw native E2E changed the checked source capsule bytes");
  assert.notEqual(disposableHashAfter, disposableHashBefore, "native named write did not change the disposable capsule");

  process.stdout.write(`${JSON.stringify({
    ok: true,
    parentPort,
    rawPort,
    rawUrl: rawPage.url(),
    nodes: 12,
    scenes: 5,
    trustTransitions: ["deny", "forget-exact-decision", "allow-once"],
    sourceWritePerformed: false,
    sourceSha256: sourceHashAfter,
    disposableWritePerformed: true,
    disposableSha256: disposableHashAfter,
    verifiedBackup: {
      id: lifecycleAfterWrite.backup.backup_id,
      sha256: lifecycleAfterWrite.backup.sha256,
    },
    supportExport: {
      format: supportBundle.format,
      sha256: supportHash,
      bytes: statSync(supportBundlePath).size,
      contentPolicy: supportBundle.content_policy,
      selectedPathRedacted: true,
      capsuleBytesIncluded: false,
      trustStoreBytesIncluded: false,
      existingFileReplaced: false,
      output: supportBundlePath,
    },
    crashRecovery: {
      attempted: true,
      hotJournalCandidate: true,
      journalSha256: journalHash,
      journalClearedBySQLite: true,
      fullyReverifiedBeforeAssetRelease: true,
    },
    persistedNode: { id: nodeId, label: persistedLabel },
    externalReplacement: {
      attempted: true,
      deniedByWindows: true,
      errorCode: replacementError.code,
      candidateSha256: replacementCandidateHash,
      activeSha256: activeHashBeforeReplacement,
      sourceBytesChanged: false,
      writableSessionContinued: true,
    },
    externalConflict: {
      detected: true,
      externalLabel,
      sessionClosed: true,
      rendererLocked: true,
      recoveryPointRetained: true,
      readOnlyContinuationRequiresReauthorization: true,
      unsignedApplicationRemainedLocked: true,
    },
    verifiedRestore: {
      createdAtNewPath: true,
      existingFileReplaced: false,
      sha256: sha256File(restoredCapsule),
      bytes: statSync(restoredCapsule).size,
      restoredPrewriteLabel: originalLabel,
      applicationRemainedLocked: true,
      output: restoredCapsule,
    },
    disposableCapsule: capsule,
    isolatedStateRoot: stateRoot,
  }, null, 2)}\n`);
} catch (error) {
  if (stderr.trim()) process.stderr.write(`native host stderr:\n${stderr}`);
  process.stderr.write(`${JSON.stringify({
    parentUrl: parentPage?.url(),
    rawUrl: rawPage?.url(),
    rawTitle: rawPage ? await rawPage.title().catch(() => null) : null,
    parentState: parentPage ? await parentPage.locator("#host-state").textContent().catch(() => null) : null,
    lifecycleState: parentPage ? await parentPage.locator("#lifecycle-status").textContent().catch(() => null) : null,
    rawSaveState: rawPage ? await rawPage.locator("#save-state").textContent().catch(() => null) : null,
    rawToast: rawPage ? await rawPage.locator("#toast").textContent().catch(() => null) : null,
    rawConsole,
    rawPageErrors,
    rawRequestFailures,
  }, null, 2)}\n`);
  throw error;
} finally {
  await rawBrowser?.close().catch(() => {});
  await parentBrowser?.close().catch(() => {});
  await stopApplication(applicationProcess);
}

async function runOpenCrashScenario() {
  const faultStateRoot = path.join(root, ".tmp", "native-raw-open-fault-e2e-state");
  const faultDisposableRoot = path.join(root, ".tmp", "native-raw-open-fault-e2e-capsule");
  const faultCapsule = path.join(faultDisposableRoot, "diagram-studio.capsule.sqlite");
  rmSync(faultStateRoot, { recursive: true, force: true });
  rmSync(faultDisposableRoot, { recursive: true, force: true });
  mkdirSync(faultStateRoot, { recursive: true });
  mkdirSync(faultDisposableRoot, { recursive: true });
  copyFileSync(sourceCapsule, faultCapsule);
  const sourceHash = sha256File(faultCapsule);

  let crashProcess;
  let crashParentBrowser;
  let crashRawBrowser;
  let crashParentPage;
  let crashRawPage;
  let crashStderr = "";
  const crashParentPort = await freePort();
  let crashRawPort = await freePort();
  while (crashRawPort === crashParentPort) crashRawPort = await freePort();
  try {
    crashProcess = spawn(application, [], {
      cwd: root,
      env: {
        ...process.env,
        SQLITE_CAPSULE_NATIVE_PARENT_E2E_PORT: String(crashParentPort),
        SQLITE_CAPSULE_NATIVE_RAW_E2E_PORT: String(crashRawPort),
        SQLITE_CAPSULE_NATIVE_E2E_PATH: faultCapsule,
        SQLITE_CAPSULE_NATIVE_E2E_STATE_ROOT: faultStateRoot,
        SQLITE_CAPSULE_NATIVE_E2E_RUNTIME_FAULTS: "enabled",
        SQLITE_CAPSULE_RUNTIME_FAULT_STAGE: "open.verified",
      },
      stdio: ["ignore", "ignore", "pipe"],
      windowsHide: true,
      shell: false,
    });
    crashProcess.stderr.setEncoding("utf8");
    crashProcess.stderr.on("data", (chunk) => { crashStderr += chunk; });
    const crashParentEndpoint = await waitForEndpoint(crashParentPort, "open-fault trusted parent");
    const crashRawEndpoint = await waitForEndpoint(crashRawPort, "open-fault raw child");
    crashParentBrowser = await chromium.connectOverCDP(crashParentEndpoint);
    crashRawBrowser = await chromium.connectOverCDP(crashRawEndpoint);
    crashParentPage = crashParentBrowser.contexts()[0]?.pages()[0];
    crashRawPage = crashRawBrowser.contexts()[0]?.pages()[0];
    assert.ok(crashParentPage, "open-fault trusted parent page is absent");
    assert.ok(crashRawPage, "open-fault raw child page is absent");
    await crashParentPage.locator("#host-state").waitFor({ state: "visible" });
    await crashParentPage.waitForFunction(() => document.querySelector("#host-state")?.textContent !== "Verifying before open");
    assert.equal(await crashParentPage.locator("#host-state").textContent(), "Trust decision required · code locked");
    assert.equal(await crashRawPage.title(), "Raw child renderer probe");
    await crashParentPage.locator("button[data-action='allow_once']").evaluate((button) => button.click());
    const crashed = await waitForProcessExit(crashProcess, "open-fault native host");
    assert.equal(crashed.code, 98, `open-fault host exited by ${crashed.signal || "an unexpected status"}`);
    assert.equal(sha256File(faultCapsule), sourceHash, "open fault changed the source bytes");
    assert.equal(persistedHistoryCursor(faultCapsule), 0);
    assert.equal(existsSync(`${faultCapsule}-journal`), false, "open fault left a source journal");
    assert.equal(existsSync(path.join(faultStateRoot, "capsule-backups")), false, "open fault created backup material");
    checked("python", ["tools/capsule.py", "verify", faultCapsule], root);
  } catch (error) {
    if (crashStderr.trim()) process.stderr.write(`open-fault native host stderr:\n${crashStderr}`);
    throw error;
  } finally {
    await crashRawBrowser?.close().catch(() => {});
    await crashParentBrowser?.close().catch(() => {});
    await stopApplication(crashProcess);
  }

  let restartProcess;
  let restartParentBrowser;
  let restartRawBrowser;
  let restartParentPage;
  let restartRawPage;
  let restartStderr = "";
  const restartParentPort = await freePort();
  let restartRawPort = await freePort();
  while (restartRawPort === restartParentPort) restartRawPort = await freePort();
  try {
    restartProcess = spawn(application, [], {
      cwd: root,
      env: {
        ...process.env,
        SQLITE_CAPSULE_NATIVE_PARENT_E2E_PORT: String(restartParentPort),
        SQLITE_CAPSULE_NATIVE_RAW_E2E_PORT: String(restartRawPort),
        SQLITE_CAPSULE_NATIVE_E2E_PATH: faultCapsule,
        SQLITE_CAPSULE_NATIVE_E2E_STATE_ROOT: faultStateRoot,
      },
      stdio: ["ignore", "ignore", "pipe"],
      windowsHide: true,
      shell: false,
    });
    restartProcess.stderr.setEncoding("utf8");
    restartProcess.stderr.on("data", (chunk) => { restartStderr += chunk; });
    const restartParentEndpoint = await waitForEndpoint(restartParentPort, "open-restart trusted parent");
    const restartRawEndpoint = await waitForEndpoint(restartRawPort, "open-restart raw child");
    restartParentBrowser = await chromium.connectOverCDP(restartParentEndpoint);
    restartRawBrowser = await chromium.connectOverCDP(restartRawEndpoint);
    restartParentPage = restartParentBrowser.contexts()[0]?.pages()[0];
    restartRawPage = restartRawBrowser.contexts()[0]?.pages()[0];
    assert.ok(restartParentPage, "open-restart trusted parent page is absent");
    assert.ok(restartRawPage, "open-restart raw child page is absent");
    await restartParentPage.locator("#host-state").waitFor({ state: "visible" });
    await restartParentPage.waitForFunction(() => document.querySelector("#host-state")?.textContent !== "Verifying before open");
    assert.equal(await restartParentPage.locator("#host-state").textContent(), "Trust decision required · code locked");
    const report = await restartParentPage.evaluate(() => globalThis.__TAURI__.core.invoke("startup_report"));
    assert.equal(report.stage, "first-open");
    assert.equal(report.capsule?.assets_released, false);
    assert.equal(report.capsule?.decision.trust_state, "structurally_verified_unsigned");
    assert.equal(report.capsule?.source_sha256, sourceHash);
    await restartRawPage.waitForFunction(() => document.querySelector("#decision")?.textContent?.startsWith("PASS"));
    assert.equal(await restartRawPage.locator("#decision").textContent(), "PASS · no native handler");
    const lockedLifecycle = await restartParentPage.evaluate(() => globalThis.__TAURI__.core.invoke("lifecycle_status"));
    assert.equal(lockedLifecycle.active, false);
    assert.deepEqual(lockedLifecycle.backup_inventory.verified, []);
    assert.deepEqual(lockedLifecycle.backup_inventory.incomplete_artifacts, []);
    assert.deepEqual(lockedLifecycle.backup_inventory.invalid_artifacts, []);

    await restartParentPage.locator("button[data-action='allow_once']").click();
    await restartParentPage.waitForFunction(() => document.querySelector("#host-state")?.textContent?.includes("application running"));
    await restartRawPage.waitForURL(/\/app\/index\.html$/);
    await restartRawPage.locator("#app[aria-busy='false']").waitFor({ state: "attached", timeout: 20_000 });
    const reopenedLifecycle = await restartParentPage.evaluate(() => globalThis.__TAURI__.core.invoke("lifecycle_status"));
    assert.equal(reopenedLifecycle.active, true);
    assert.equal(reopenedLifecycle.writable, true);
    assert.equal(reopenedLifecycle.mode, "writable");
    assert.equal(sha256File(faultCapsule), sourceHash);

    process.stdout.write(`${JSON.stringify({
      ok: true,
      durableFault: "open.verified",
      hostExitCode: 98,
      source: {
        path: faultCapsule,
        sha256: sourceHash,
        changed: false,
        historyCursor: 0,
        journalRetained: false,
      },
      restart: {
        stage: report.stage,
        persistedAuthority: false,
        assetsReleasedBeforeNewDecision: false,
        rawRendererLockedBeforeNewDecision: true,
        backupArtifacts: 0,
        writerLeaseReacquiredAfterNewDecision: true,
      },
      isolatedStateRoot: faultStateRoot,
    }, null, 2)}\n`);
  } catch (error) {
    if (restartStderr.trim()) process.stderr.write(`open-restart native host stderr:\n${restartStderr}`);
    throw error;
  } finally {
    await restartRawBrowser?.close().catch(() => {});
    await restartParentBrowser?.close().catch(() => {});
    await stopApplication(restartProcess);
  }
}

async function runRestoreCrashScenario() {
  const faultStateRoot = path.join(root, ".tmp", "native-raw-fault-e2e-state");
  const faultDisposableRoot = path.join(root, ".tmp", "native-raw-fault-e2e-capsule");
  const faultCapsule = path.join(faultDisposableRoot, "diagram-studio.capsule.sqlite");
  const faultRestoreRoot = path.join(faultStateRoot, "restored");
  const faultRestoredCapsule = path.join(faultRestoreRoot, "interrupted.sqlitecapsule");
  const restoreMarker = `${faultRestoredCapsule}.capsule-restore-in-progress`;
  const nodeId = "node-agent-prompt";
  const originalLabel = "One-sentence agent prompt";
  const changedLabel = `${originalLabel} · restore crash E2E`;

  rmSync(faultStateRoot, { recursive: true, force: true });
  rmSync(faultDisposableRoot, { recursive: true, force: true });
  mkdirSync(faultStateRoot, { recursive: true });
  mkdirSync(faultDisposableRoot, { recursive: true });
  mkdirSync(faultRestoreRoot, { recursive: true });
  copyFileSync(sourceCapsule, faultCapsule);
  const faultSourceHashBefore = sha256File(faultCapsule);

  let crashProcess;
  let crashParentBrowser;
  let crashRawBrowser;
  let crashParentPage;
  let crashRawPage;
  let crashStderr = "";
  let backup;
  const crashParentPort = await freePort();
  let crashRawPort = await freePort();
  while (crashRawPort === crashParentPort) crashRawPort = await freePort();
  try {
    crashProcess = spawn(application, [], {
      cwd: root,
      env: {
        ...process.env,
        SQLITE_CAPSULE_NATIVE_PARENT_E2E_PORT: String(crashParentPort),
        SQLITE_CAPSULE_NATIVE_RAW_E2E_PORT: String(crashRawPort),
        SQLITE_CAPSULE_NATIVE_E2E_PATH: faultCapsule,
        SQLITE_CAPSULE_NATIVE_E2E_STATE_ROOT: faultStateRoot,
        SQLITE_CAPSULE_NATIVE_E2E_RESTORE_PATH: faultRestoredCapsule,
        SQLITE_CAPSULE_NATIVE_E2E_RUNTIME_FAULTS: "enabled",
        SQLITE_CAPSULE_RUNTIME_FAULT_STAGE: "restore.database-copied",
      },
      stdio: ["ignore", "ignore", "pipe"],
      windowsHide: true,
      shell: false,
    });
    crashProcess.stderr.setEncoding("utf8");
    crashProcess.stderr.on("data", (chunk) => { crashStderr += chunk; });

    const crashParentEndpoint = await waitForEndpoint(crashParentPort, "restore-fault trusted parent");
    const crashRawEndpoint = await waitForEndpoint(crashRawPort, "restore-fault raw child");
    crashParentBrowser = await chromium.connectOverCDP(crashParentEndpoint);
    crashRawBrowser = await chromium.connectOverCDP(crashRawEndpoint);
    crashParentPage = crashParentBrowser.contexts()[0]?.pages()[0];
    crashRawPage = crashRawBrowser.contexts()[0]?.pages()[0];
    assert.ok(crashParentPage, "restore-fault trusted parent page is absent");
    assert.ok(crashRawPage, "restore-fault raw child page is absent");
    await crashParentPage.locator("#host-state").waitFor({ state: "visible" });
    await crashParentPage.waitForFunction(() => document.querySelector("#host-state")?.textContent !== "Verifying before open");
    assert.equal(await crashParentPage.locator("#host-state").textContent(), "Trust decision required · code locked");
    await crashRawPage.waitForFunction(() => document.querySelector("#decision")?.textContent?.startsWith("PASS"));
    assert.equal(await crashRawPage.locator("#decision").textContent(), "PASS · no native handler");

    await crashParentPage.locator("button[data-action='allow_once']").click();
    await crashParentPage.waitForFunction(() => document.querySelector("#host-state")?.textContent?.includes("application running"));
    await crashRawPage.waitForURL(/\/app\/index\.html$/);
    await crashRawPage.locator("#app[aria-busy='false']").waitFor({ state: "attached", timeout: 20_000 });
    await crashRawPage.locator(`[data-id='${nodeId}']`).click();
    await crashRawPage.locator("#node-label-input").fill(changedLabel);
    await crashRawPage.locator("#save-node-label").click();
    await crashRawPage.waitForFunction(() => document.querySelector("#save-state")?.textContent === "Saved in SQLite");
    assert.equal(persistedNodeLabel(faultCapsule, nodeId), changedLabel);

    const lifecycle = await crashParentPage.evaluate(() => globalThis.__TAURI__.core.invoke("lifecycle_status"));
    backup = lifecycle.backup;
    assert.ok(backup?.backup_id, "restore-fault pre-write backup is absent");
    assert.equal(
      sha256File(path.join(faultStateRoot, "capsule-backups", backup.backup_id)),
      backup.sha256,
    );
    await crashParentPage.waitForFunction(() => !document.querySelector("#restore-button")?.disabled);
    // The active raw child is a separate native WebView. Dispatch through the
    // trusted document so native hit-testing cannot keep this crash gate from
    // exercising the same host-owned button handler and command.
    await crashParentPage.locator("#restore-button").evaluate((button) => button.click());
    const crashed = await waitForProcessExit(crashProcess, "restore-fault native host");
    assert.equal(crashed.code, 98, `restore-fault host exited by ${crashed.signal || "an unexpected status"}`);
    assert.ok(existsSync(faultRestoredCapsule), "interrupted restore did not leave copied database bytes");
    assert.ok(existsSync(restoreMarker), "interrupted restore did not retain its durable marker");
    assert.equal(sha256File(faultRestoredCapsule), backup.sha256);
    assert.equal(persistedNodeLabel(faultRestoredCapsule, nodeId), originalLabel);
    assert.equal(persistedNodeLabel(faultCapsule, nodeId), changedLabel);
    checked("python", ["tools/capsule.py", "verify", faultCapsule], root);
    checked("python", ["tools/capsule.py", "verify", faultRestoredCapsule], root);
  } catch (error) {
    if (crashStderr.trim()) process.stderr.write(`restore-fault native host stderr:\n${crashStderr}`);
    throw error;
  } finally {
    await crashRawBrowser?.close().catch(() => {});
    await crashParentBrowser?.close().catch(() => {});
    await stopApplication(crashProcess);
  }

  const interruptedHashBeforeRestart = sha256File(faultRestoredCapsule);
  let restartProcess;
  let restartParentBrowser;
  let restartRawBrowser;
  let restartParentPage;
  let restartRawPage;
  let restartStderr = "";
  const restartParentPort = await freePort();
  let restartRawPort = await freePort();
  while (restartRawPort === restartParentPort) restartRawPort = await freePort();
  try {
    restartProcess = spawn(application, [], {
      cwd: root,
      env: {
        ...process.env,
        SQLITE_CAPSULE_NATIVE_PARENT_E2E_PORT: String(restartParentPort),
        SQLITE_CAPSULE_NATIVE_RAW_E2E_PORT: String(restartRawPort),
        SQLITE_CAPSULE_NATIVE_E2E_PATH: faultRestoredCapsule,
        SQLITE_CAPSULE_NATIVE_E2E_STATE_ROOT: faultStateRoot,
      },
      stdio: ["ignore", "ignore", "pipe"],
      windowsHide: true,
      shell: false,
    });
    restartProcess.stderr.setEncoding("utf8");
    restartProcess.stderr.on("data", (chunk) => { restartStderr += chunk; });
    const restartParentEndpoint = await waitForEndpoint(restartParentPort, "restore-restart trusted parent");
    const restartRawEndpoint = await waitForEndpoint(restartRawPort, "restore-restart raw child");
    restartParentBrowser = await chromium.connectOverCDP(restartParentEndpoint);
    restartRawBrowser = await chromium.connectOverCDP(restartRawEndpoint);
    restartParentPage = restartParentBrowser.contexts()[0]?.pages()[0];
    restartRawPage = restartRawBrowser.contexts()[0]?.pages()[0];
    assert.ok(restartParentPage, "restore-restart trusted parent page is absent");
    assert.ok(restartRawPage, "restore-restart raw child page is absent");
    await restartParentPage.locator("#host-state").waitFor({ state: "visible" });
    await restartParentPage.waitForFunction(() => document.querySelector("#host-state")?.textContent !== "Verifying before open");
    assert.equal(await restartParentPage.locator("#host-state").textContent(), "Rejected before execution");
    assert.equal(await restartParentPage.locator("#verdict strong").textContent(), "Capsule rejected");
    const rejected = await restartParentPage.evaluate(() => globalThis.__TAURI__.core.invoke("startup_report"));
    assert.equal(rejected.stage, "rejected");
    assert.equal(rejected.capsule, null);
    assert.match(rejected.error || "", /interrupted restore marker is present/);
    assert.match(await restartParentPage.locator("#identity-details").textContent(), /Executable assetsNot released/);
    await restartRawPage.waitForFunction(() => document.querySelector("#decision")?.textContent?.startsWith("PASS"));
    assert.equal(await restartRawPage.locator("#decision").textContent(), "PASS · no native handler");
    assert.deepEqual(
      await restartRawPage.evaluate(() => ({
        tauri: typeof globalThis.__TAURI__,
        internals: typeof globalThis.__TAURI_INTERNALS__,
      })),
      { tauri: "undefined", internals: "undefined" },
    );
    const restartLifecycle = await restartParentPage.evaluate(() => globalThis.__TAURI__.core.invoke("lifecycle_status"));
    assert.equal(restartLifecycle.active, false);
    assert.equal(restartLifecycle.writable, false);
    assert.equal(restartLifecycle.mode, "locked");
    assert.ok(restartLifecycle.backup_inventory.verified.some((record) => record.backup_id === backup.backup_id));
    assert.equal(sha256File(faultRestoredCapsule), interruptedHashBeforeRestart);
    assert.ok(existsSync(restoreMarker), "restart removed the interrupted restore marker");

    process.stdout.write(`${JSON.stringify({
      ok: true,
      durableFault: "restore.database-copied",
      hostExitCode: 98,
      source: {
        path: faultCapsule,
        beforeSha256: faultSourceHashBefore,
        afterSha256: sha256File(faultCapsule),
        committedLabel: changedLabel,
      },
      recoveryPoint: {
        id: backup.backup_id,
        sha256: backup.sha256,
      },
      interruptedRestore: {
        path: faultRestoredCapsule,
        sha256: interruptedHashBeforeRestart,
        markerRetained: true,
        originalLabel,
      },
      restart: {
        stage: rejected.stage,
        rejectedBeforeExecution: true,
        assetsReleased: false,
        rawRendererLocked: true,
        backupStillVerified: true,
      },
      isolatedStateRoot: faultStateRoot,
    }, null, 2)}\n`);
  } catch (error) {
    if (restartStderr.trim()) process.stderr.write(`restore-restart native host stderr:\n${restartStderr}`);
    throw error;
  } finally {
    await restartRawBrowser?.close().catch(() => {});
    await restartParentBrowser?.close().catch(() => {});
    await stopApplication(restartProcess);
  }
}

async function runPrewriteCrashScenario() {
  const faultStateRoot = path.join(root, ".tmp", "native-raw-prewrite-fault-e2e-state");
  const faultDisposableRoot = path.join(root, ".tmp", "native-raw-prewrite-fault-e2e-capsule");
  const faultCapsule = path.join(faultDisposableRoot, "diagram-studio.capsule.sqlite");
  const nodeId = "node-agent-prompt";
  const originalLabel = "One-sentence agent prompt";
  const attemptedLabel = `${originalLabel} · prewrite crash E2E`;

  rmSync(faultStateRoot, { recursive: true, force: true });
  rmSync(faultDisposableRoot, { recursive: true, force: true });
  mkdirSync(faultStateRoot, { recursive: true });
  mkdirSync(faultDisposableRoot, { recursive: true });
  copyFileSync(sourceCapsule, faultCapsule);
  const sourceHashBefore = sha256File(faultCapsule);

  let crashProcess;
  let crashParentBrowser;
  let crashRawBrowser;
  let crashParentPage;
  let crashRawPage;
  let crashStderr = "";
  const crashParentPort = await freePort();
  let crashRawPort = await freePort();
  while (crashRawPort === crashParentPort) crashRawPort = await freePort();
  try {
    crashProcess = spawn(application, [], {
      cwd: root,
      env: {
        ...process.env,
        SQLITE_CAPSULE_NATIVE_PARENT_E2E_PORT: String(crashParentPort),
        SQLITE_CAPSULE_NATIVE_RAW_E2E_PORT: String(crashRawPort),
        SQLITE_CAPSULE_NATIVE_E2E_PATH: faultCapsule,
        SQLITE_CAPSULE_NATIVE_E2E_STATE_ROOT: faultStateRoot,
        SQLITE_CAPSULE_NATIVE_E2E_RUNTIME_FAULTS: "enabled",
        SQLITE_CAPSULE_RUNTIME_FAULT_STAGE: "prewrite.database-copied",
      },
      stdio: ["ignore", "ignore", "pipe"],
      windowsHide: true,
      shell: false,
    });
    crashProcess.stderr.setEncoding("utf8");
    crashProcess.stderr.on("data", (chunk) => { crashStderr += chunk; });
    const crashParentEndpoint = await waitForEndpoint(crashParentPort, "prewrite-fault trusted parent");
    const crashRawEndpoint = await waitForEndpoint(crashRawPort, "prewrite-fault raw child");
    crashParentBrowser = await chromium.connectOverCDP(crashParentEndpoint);
    crashRawBrowser = await chromium.connectOverCDP(crashRawEndpoint);
    crashParentPage = crashParentBrowser.contexts()[0]?.pages()[0];
    crashRawPage = crashRawBrowser.contexts()[0]?.pages()[0];
    assert.ok(crashParentPage, "prewrite-fault trusted parent page is absent");
    assert.ok(crashRawPage, "prewrite-fault raw child page is absent");
    await crashParentPage.locator("#host-state").waitFor({ state: "visible" });
    await crashParentPage.waitForFunction(() => document.querySelector("#host-state")?.textContent !== "Verifying before open");
    assert.equal(await crashParentPage.locator("#host-state").textContent(), "Trust decision required · code locked");
    await crashParentPage.locator("button[data-action='allow_once']").click();
    await crashParentPage.waitForFunction(() => document.querySelector("#host-state")?.textContent?.includes("application running"));
    await crashRawPage.waitForURL(/\/app\/index\.html$/);
    await crashRawPage.locator("#app[aria-busy='false']").waitFor({ state: "attached", timeout: 20_000 });
    await crashRawPage.locator(`[data-id='${nodeId}']`).click();
    await crashRawPage.locator("#node-label-input").fill(attemptedLabel);
    await crashRawPage.locator("#save-node-label").evaluate((button) => button.click());
    const crashed = await waitForProcessExit(crashProcess, "prewrite-fault native host");
    assert.equal(crashed.code, 98, `prewrite-fault host exited by ${crashed.signal || "an unexpected status"}`);
    assert.equal(sha256File(faultCapsule), sourceHashBefore, "prewrite fault changed the source bytes");
    assert.equal(persistedNodeLabel(faultCapsule, nodeId), originalLabel);
    assert.equal(existsSync(`${faultCapsule}-journal`), false, "prewrite fault left a source journal");
    checked("python", ["tools/capsule.py", "verify", faultCapsule], root);
  } catch (error) {
    if (crashStderr.trim()) process.stderr.write(`prewrite-fault native host stderr:\n${crashStderr}`);
    throw error;
  } finally {
    await crashRawBrowser?.close().catch(() => {});
    await crashParentBrowser?.close().catch(() => {});
    await stopApplication(crashProcess);
  }

  let restartProcess;
  let restartParentBrowser;
  let restartRawBrowser;
  let restartParentPage;
  let restartRawPage;
  let restartStderr = "";
  const restartParentPort = await freePort();
  let restartRawPort = await freePort();
  while (restartRawPort === restartParentPort) restartRawPort = await freePort();
  try {
    restartProcess = spawn(application, [], {
      cwd: root,
      env: {
        ...process.env,
        SQLITE_CAPSULE_NATIVE_PARENT_E2E_PORT: String(restartParentPort),
        SQLITE_CAPSULE_NATIVE_RAW_E2E_PORT: String(restartRawPort),
        SQLITE_CAPSULE_NATIVE_E2E_PATH: faultCapsule,
        SQLITE_CAPSULE_NATIVE_E2E_STATE_ROOT: faultStateRoot,
      },
      stdio: ["ignore", "ignore", "pipe"],
      windowsHide: true,
      shell: false,
    });
    restartProcess.stderr.setEncoding("utf8");
    restartProcess.stderr.on("data", (chunk) => { restartStderr += chunk; });
    const restartParentEndpoint = await waitForEndpoint(restartParentPort, "prewrite-restart trusted parent");
    const restartRawEndpoint = await waitForEndpoint(restartRawPort, "prewrite-restart raw child");
    restartParentBrowser = await chromium.connectOverCDP(restartParentEndpoint);
    restartRawBrowser = await chromium.connectOverCDP(restartRawEndpoint);
    restartParentPage = restartParentBrowser.contexts()[0]?.pages()[0];
    restartRawPage = restartRawBrowser.contexts()[0]?.pages()[0];
    assert.ok(restartParentPage, "prewrite-restart trusted parent page is absent");
    assert.ok(restartRawPage, "prewrite-restart raw child page is absent");
    await restartParentPage.locator("#host-state").waitFor({ state: "visible" });
    await restartParentPage.waitForFunction(() => document.querySelector("#host-state")?.textContent !== "Verifying before open");
    assert.equal(await restartParentPage.locator("#host-state").textContent(), "Trust decision required · code locked");
    const report = await restartParentPage.evaluate(() => globalThis.__TAURI__.core.invoke("startup_report"));
    assert.equal(report.stage, "first-open");
    assert.equal(report.capsule?.assets_released, false);
    assert.equal(report.capsule?.source_sha256, sourceHashBefore);
    await restartRawPage.waitForFunction(() => document.querySelector("#decision")?.textContent?.startsWith("PASS"));
    assert.equal(await restartRawPage.locator("#decision").textContent(), "PASS · no native handler");
    assert.deepEqual(
      await restartRawPage.evaluate(() => ({
        tauri: typeof globalThis.__TAURI__,
        internals: typeof globalThis.__TAURI_INTERNALS__,
      })),
      { tauri: "undefined", internals: "undefined" },
    );
    await restartParentPage.waitForFunction(
      () => document.querySelector("#lifecycle-status")?.textContent?.includes("Recovery inventory requires attention: 1 interrupted and 0 invalid artifact"),
    );
    const lifecycle = await restartParentPage.evaluate(() => globalThis.__TAURI__.core.invoke("lifecycle_status"));
    assert.equal(lifecycle.active, false);
    assert.equal(lifecycle.writable, false);
    assert.equal(lifecycle.mode, "locked");
    assert.equal(lifecycle.backup, null);
    assert.deepEqual(lifecycle.backup_inventory.verified, []);
    assert.equal(lifecycle.backup_inventory.incomplete_artifacts.length, 1);
    assert.deepEqual(lifecycle.backup_inventory.invalid_artifacts, []);
    const incompleteId = lifecycle.backup_inventory.incomplete_artifacts[0];
    const backupPath = path.join(faultStateRoot, "capsule-backups", incompleteId);
    const markerPath = path.join(faultStateRoot, "capsule-backups", `${incompleteId}.in-progress`);
    const manifestPath = path.join(faultStateRoot, "capsule-backups", `${incompleteId}.json`);
    assert.ok(existsSync(backupPath), "prewrite fault did not retain the copied database");
    assert.ok(existsSync(markerPath), "prewrite fault did not retain the durable marker");
    assert.equal(existsSync(manifestPath), false, "prewrite fault unexpectedly completed its manifest");
    checked("python", ["tools/capsule.py", "verify", backupPath], root);
    assert.equal(sha256File(faultCapsule), sourceHashBefore);
    assert.equal(persistedNodeLabel(faultCapsule, nodeId), originalLabel);

    process.stdout.write(`${JSON.stringify({
      ok: true,
      durableFault: "prewrite.database-copied",
      hostExitCode: 98,
      source: {
        path: faultCapsule,
        beforeSha256: sourceHashBefore,
        afterSha256: sha256File(faultCapsule),
        attemptedLabel,
        persistedLabel: originalLabel,
        writeCommitted: false,
        journalRetained: false,
      },
      interruptedBackup: {
        id: incompleteId,
        path: backupPath,
        sha256: sha256File(backupPath),
        databaseCopied: true,
        markerRetained: true,
        manifestPresent: false,
        treatedAsRecoverable: false,
      },
      restart: {
        stage: report.stage,
        assetsReleased: false,
        rawRendererLocked: true,
        lifecycleAttention: true,
        incompleteArtifacts: 1,
        invalidArtifacts: 0,
      },
      isolatedStateRoot: faultStateRoot,
    }, null, 2)}\n`);
  } catch (error) {
    if (restartStderr.trim()) process.stderr.write(`prewrite-restart native host stderr:\n${restartStderr}`);
    throw error;
  } finally {
    await restartRawBrowser?.close().catch(() => {});
    await restartParentBrowser?.close().catch(() => {});
    await stopApplication(restartProcess);
  }
}

async function runCheckpointCrashScenario() {
  const faultStateRoot = path.join(root, ".tmp", "native-raw-checkpoint-fault-e2e-state");
  const faultDisposableRoot = path.join(root, ".tmp", "native-raw-checkpoint-fault-e2e-capsule");
  const faultCapsule = path.join(faultDisposableRoot, "diagram-studio.capsule.sqlite");
  const nodeId = "node-agent-prompt";
  const originalLabel = "One-sentence agent prompt";

  rmSync(faultStateRoot, { recursive: true, force: true });
  rmSync(faultDisposableRoot, { recursive: true, force: true });
  mkdirSync(faultStateRoot, { recursive: true });
  mkdirSync(faultDisposableRoot, { recursive: true });
  copyFileSync(sourceCapsule, faultCapsule);
  const sourceHashBefore = sha256File(faultCapsule);

  let crashProcess;
  let crashParentBrowser;
  let crashRawBrowser;
  let crashParentPage;
  let crashRawPage;
  let crashStderr = "";
  let prewriteBackup;
  let committedLabel = originalLabel;
  const crashParentPort = await freePort();
  let crashRawPort = await freePort();
  while (crashRawPort === crashParentPort) crashRawPort = await freePort();
  try {
    crashProcess = spawn(application, [], {
      cwd: root,
      env: {
        ...process.env,
        SQLITE_CAPSULE_NATIVE_PARENT_E2E_PORT: String(crashParentPort),
        SQLITE_CAPSULE_NATIVE_RAW_E2E_PORT: String(crashRawPort),
        SQLITE_CAPSULE_NATIVE_E2E_PATH: faultCapsule,
        SQLITE_CAPSULE_NATIVE_E2E_STATE_ROOT: faultStateRoot,
        SQLITE_CAPSULE_NATIVE_E2E_RUNTIME_FAULTS: "enabled",
        SQLITE_CAPSULE_RUNTIME_FAULT_STAGE: "checkpoint.manifest-synced",
      },
      stdio: ["ignore", "ignore", "pipe"],
      windowsHide: true,
      shell: false,
    });
    crashProcess.stderr.setEncoding("utf8");
    crashProcess.stderr.on("data", (chunk) => { crashStderr += chunk; });
    const crashParentEndpoint = await waitForEndpoint(crashParentPort, "checkpoint-fault trusted parent");
    const crashRawEndpoint = await waitForEndpoint(crashRawPort, "checkpoint-fault raw child");
    crashParentBrowser = await chromium.connectOverCDP(crashParentEndpoint);
    crashRawBrowser = await chromium.connectOverCDP(crashRawEndpoint);
    crashParentPage = crashParentBrowser.contexts()[0]?.pages()[0];
    crashRawPage = crashRawBrowser.contexts()[0]?.pages()[0];
    assert.ok(crashParentPage, "checkpoint-fault trusted parent page is absent");
    assert.ok(crashRawPage, "checkpoint-fault raw child page is absent");
    await crashParentPage.locator("#host-state").waitFor({ state: "visible" });
    await crashParentPage.waitForFunction(() => document.querySelector("#host-state")?.textContent !== "Verifying before open");
    await crashParentPage.locator("button[data-action='allow_once']").click();
    await crashParentPage.waitForFunction(() => document.querySelector("#host-state")?.textContent?.includes("application running"));
    await crashRawPage.waitForURL(/\/app\/index\.html$/);
    await crashRawPage.locator("#app[aria-busy='false']").waitFor({ state: "attached", timeout: 20_000 });

    for (let index = 1; index <= 10; index += 1) {
      const nextLabel = `${originalLabel} · checkpoint ${index}`;
      await renameNodeThroughBridge(crashRawPage, {
        nodeId,
        fromLabel: committedLabel,
        toLabel: nextLabel,
        cursor: index - 1,
        operationId: `operation-checkpoint-e2e-${index}`,
      });
      committedLabel = nextLabel;
    }
    assert.equal(persistedNodeLabel(faultCapsule, nodeId), committedLabel);
    assert.equal(persistedHistoryCursor(faultCapsule), 10);
    const beforeCheckpoint = await crashParentPage.evaluate(() => globalThis.__TAURI__.core.invoke("lifecycle_status"));
    prewriteBackup = beforeCheckpoint.backup;
    assert.ok(prewriteBackup?.backup_id, "checkpoint-fault pre-write backup is absent");
    assert.equal(beforeCheckpoint.backup_inventory.verified.length, 1);
    assert.deepEqual(beforeCheckpoint.backup_inventory.incomplete_artifacts, []);

    const attemptedLabel = `${originalLabel} · checkpoint 11 must not commit`;
    const attemptedWrite = renameNodeThroughBridge(crashRawPage, {
      nodeId,
      fromLabel: committedLabel,
      toLabel: attemptedLabel,
      cursor: 10,
      operationId: "operation-checkpoint-e2e-11",
    }).catch(() => null);
    const crashed = await waitForProcessExit(crashProcess, "checkpoint-fault native host");
    await attemptedWrite;
    assert.equal(crashed.code, 98, `checkpoint-fault host exited by ${crashed.signal || "an unexpected status"}`);
    assert.equal(persistedNodeLabel(faultCapsule, nodeId), committedLabel);
    assert.equal(persistedHistoryCursor(faultCapsule), 10);
    assert.equal(existsSync(`${faultCapsule}-journal`), false, "checkpoint fault left a source journal");
    checked("python", ["tools/capsule.py", "verify", faultCapsule], root);
  } catch (error) {
    if (crashStderr.trim()) process.stderr.write(`checkpoint-fault native host stderr:\n${crashStderr}`);
    throw error;
  } finally {
    await crashRawBrowser?.close().catch(() => {});
    await crashParentBrowser?.close().catch(() => {});
    await stopApplication(crashProcess);
  }

  const sourceHashAfter = sha256File(faultCapsule);
  let restartProcess;
  let restartParentBrowser;
  let restartRawBrowser;
  let restartParentPage;
  let restartRawPage;
  let restartStderr = "";
  const restartParentPort = await freePort();
  let restartRawPort = await freePort();
  while (restartRawPort === restartParentPort) restartRawPort = await freePort();
  try {
    restartProcess = spawn(application, [], {
      cwd: root,
      env: {
        ...process.env,
        SQLITE_CAPSULE_NATIVE_PARENT_E2E_PORT: String(restartParentPort),
        SQLITE_CAPSULE_NATIVE_RAW_E2E_PORT: String(restartRawPort),
        SQLITE_CAPSULE_NATIVE_E2E_PATH: faultCapsule,
        SQLITE_CAPSULE_NATIVE_E2E_STATE_ROOT: faultStateRoot,
      },
      stdio: ["ignore", "ignore", "pipe"],
      windowsHide: true,
      shell: false,
    });
    restartProcess.stderr.setEncoding("utf8");
    restartProcess.stderr.on("data", (chunk) => { restartStderr += chunk; });
    const restartParentEndpoint = await waitForEndpoint(restartParentPort, "checkpoint-restart trusted parent");
    const restartRawEndpoint = await waitForEndpoint(restartRawPort, "checkpoint-restart raw child");
    restartParentBrowser = await chromium.connectOverCDP(restartParentEndpoint);
    restartRawBrowser = await chromium.connectOverCDP(restartRawEndpoint);
    restartParentPage = restartParentBrowser.contexts()[0]?.pages()[0];
    restartRawPage = restartRawBrowser.contexts()[0]?.pages()[0];
    assert.ok(restartParentPage, "checkpoint-restart trusted parent page is absent");
    assert.ok(restartRawPage, "checkpoint-restart raw child page is absent");
    await restartParentPage.locator("#host-state").waitFor({ state: "visible" });
    await restartParentPage.waitForFunction(() => document.querySelector("#host-state")?.textContent !== "Verifying before open");
    assert.equal(await restartParentPage.locator("#host-state").textContent(), "Trust decision required · code locked");
    const report = await restartParentPage.evaluate(() => globalThis.__TAURI__.core.invoke("startup_report"));
    assert.equal(report.stage, "first-open");
    assert.equal(report.capsule?.assets_released, false);
    assert.equal(report.capsule?.source_sha256, sourceHashAfter);
    await restartRawPage.waitForFunction(() => document.querySelector("#decision")?.textContent?.startsWith("PASS"));
    assert.equal(await restartRawPage.locator("#decision").textContent(), "PASS · no native handler");
    await restartParentPage.waitForFunction(
      () => document.querySelector("#lifecycle-status")?.textContent?.includes("Recovery inventory requires attention: 1 interrupted and 0 invalid artifact"),
    );
    const lifecycle = await restartParentPage.evaluate(() => globalThis.__TAURI__.core.invoke("lifecycle_status"));
    assert.equal(lifecycle.active, false);
    assert.equal(lifecycle.mode, "locked");
    assert.equal(lifecycle.backup_inventory.verified.length, 1);
    assert.equal(lifecycle.backup_inventory.verified[0].backup_id, prewriteBackup.backup_id);
    assert.equal(lifecycle.backup_inventory.incomplete_artifacts.length, 1);
    assert.deepEqual(lifecycle.backup_inventory.invalid_artifacts, []);
    const incompleteId = lifecycle.backup_inventory.incomplete_artifacts[0];
    const backupRoot = path.join(faultStateRoot, "capsule-backups");
    const incompletePath = path.join(backupRoot, incompleteId);
    const incompleteMarker = path.join(backupRoot, `${incompleteId}.in-progress`);
    const incompleteManifest = path.join(backupRoot, `${incompleteId}.json`);
    const prewritePath = path.join(backupRoot, prewriteBackup.backup_id);
    assert.ok(existsSync(incompletePath), "checkpoint fault did not retain checkpoint bytes");
    assert.ok(existsSync(incompleteMarker), "checkpoint fault did not retain its marker");
    assert.ok(existsSync(incompleteManifest), "checkpoint fault did not sync its manifest");
    assert.equal(persistedNodeLabel(incompletePath, nodeId), committedLabel);
    assert.equal(persistedHistoryCursor(incompletePath), 10);
    assert.equal(persistedNodeLabel(prewritePath, nodeId), originalLabel);
    assert.equal(persistedHistoryCursor(prewritePath), 0);
    checked("python", ["tools/capsule.py", "verify", incompletePath], root);
    checked("python", ["tools/capsule.py", "verify", prewritePath], root);
    assert.equal(sha256File(faultCapsule), sourceHashAfter);
    assert.equal(persistedNodeLabel(faultCapsule, nodeId), committedLabel);

    process.stdout.write(`${JSON.stringify({
      ok: true,
      durableFault: "checkpoint.manifest-synced",
      hostExitCode: 98,
      source: {
        path: faultCapsule,
        beforeSha256: sourceHashBefore,
        afterSha256: sourceHashAfter,
        committedLabel,
        committedWrites: 10,
        attemptedEleventhWriteCommitted: false,
        journalRetained: false,
      },
      verifiedPrewriteBackup: {
        id: prewriteBackup.backup_id,
        sha256: prewriteBackup.sha256,
        historyCursor: 0,
      },
      interruptedBoundedCheckpoint: {
        id: incompleteId,
        path: incompletePath,
        sha256: sha256File(incompletePath),
        historyCursor: 10,
        markerRetained: true,
        manifestPresent: true,
        treatedAsRecoverable: false,
      },
      restart: {
        stage: report.stage,
        assetsReleased: false,
        rawRendererLocked: true,
        verifiedArtifacts: 1,
        incompleteArtifacts: 1,
        invalidArtifacts: 0,
      },
      isolatedStateRoot: faultStateRoot,
    }, null, 2)}\n`);
  } catch (error) {
    if (restartStderr.trim()) process.stderr.write(`checkpoint-restart native host stderr:\n${restartStderr}`);
    throw error;
  } finally {
    await restartRawBrowser?.close().catch(() => {});
    await restartParentBrowser?.close().catch(() => {});
    await stopApplication(restartProcess);
  }
}

async function runUpdatePreflightCrashScenario() {
  const faultStateRoot = path.join(root, ".tmp", "native-raw-update-fault-e2e-state");
  const faultDisposableRoot = path.join(root, ".tmp", "native-raw-update-fault-e2e-capsule");
  const faultCapsule = path.join(faultDisposableRoot, "diagram-studio.capsule.sqlite");
  const nodeId = "node-agent-prompt";
  const originalLabel = "One-sentence agent prompt";
  const committedLabel = `${originalLabel} · update preflight crash E2E`;

  rmSync(faultStateRoot, { recursive: true, force: true });
  rmSync(faultDisposableRoot, { recursive: true, force: true });
  mkdirSync(faultStateRoot, { recursive: true });
  mkdirSync(faultDisposableRoot, { recursive: true });
  copyFileSync(sourceCapsule, faultCapsule);
  const sourceHashBefore = sha256File(faultCapsule);

  let crashProcess;
  let crashParentBrowser;
  let crashRawBrowser;
  let crashParentPage;
  let crashRawPage;
  let crashStderr = "";
  let prewriteBackup;
  const crashParentPort = await freePort();
  let crashRawPort = await freePort();
  while (crashRawPort === crashParentPort) crashRawPort = await freePort();
  try {
    crashProcess = spawn(application, [], {
      cwd: root,
      env: {
        ...process.env,
        SQLITE_CAPSULE_NATIVE_PARENT_E2E_PORT: String(crashParentPort),
        SQLITE_CAPSULE_NATIVE_RAW_E2E_PORT: String(crashRawPort),
        SQLITE_CAPSULE_NATIVE_E2E_PATH: faultCapsule,
        SQLITE_CAPSULE_NATIVE_E2E_STATE_ROOT: faultStateRoot,
        SQLITE_CAPSULE_NATIVE_E2E_RUNTIME_FAULTS: "enabled",
        SQLITE_CAPSULE_RUNTIME_FAULT_STAGE: "update.manifest-synced",
      },
      stdio: ["ignore", "ignore", "pipe"],
      windowsHide: true,
      shell: false,
    });
    crashProcess.stderr.setEncoding("utf8");
    crashProcess.stderr.on("data", (chunk) => { crashStderr += chunk; });
    const crashParentEndpoint = await waitForEndpoint(crashParentPort, "update-fault trusted parent");
    const crashRawEndpoint = await waitForEndpoint(crashRawPort, "update-fault raw child");
    crashParentBrowser = await chromium.connectOverCDP(crashParentEndpoint);
    crashRawBrowser = await chromium.connectOverCDP(crashRawEndpoint);
    crashParentPage = crashParentBrowser.contexts()[0]?.pages()[0];
    crashRawPage = crashRawBrowser.contexts()[0]?.pages()[0];
    assert.ok(crashParentPage, "update-fault trusted parent page is absent");
    assert.ok(crashRawPage, "update-fault raw child page is absent");
    await crashParentPage.locator("#host-state").waitFor({ state: "visible" });
    await crashParentPage.waitForFunction(() => document.querySelector("#host-state")?.textContent !== "Verifying before open");
    await crashParentPage.locator("button[data-action='allow_once']").click();
    await crashParentPage.waitForFunction(() => document.querySelector("#host-state")?.textContent?.includes("application running"));
    await crashRawPage.waitForURL(/\/app\/index\.html$/);
    await crashRawPage.locator("#app[aria-busy='false']").waitFor({ state: "attached", timeout: 20_000 });
    await renameNodeThroughBridge(crashRawPage, {
      nodeId,
      fromLabel: originalLabel,
      toLabel: committedLabel,
      cursor: 0,
      operationId: "operation-update-preflight-e2e",
    });
    assert.equal(persistedNodeLabel(faultCapsule, nodeId), committedLabel);
    assert.equal(persistedHistoryCursor(faultCapsule), 1);
    const beforePreflight = await crashParentPage.evaluate(() => globalThis.__TAURI__.core.invoke("lifecycle_status"));
    prewriteBackup = beforePreflight.backup;
    assert.ok(prewriteBackup?.backup_id, "update-fault pre-write backup is absent");
    assert.equal(beforePreflight.backup_inventory.verified.length, 1);
    assert.deepEqual(beforePreflight.backup_inventory.incomplete_artifacts, []);

    const preflight = crashParentPage.evaluate(() => globalThis.__TAURI__.core.invoke("stage_host_update", {
      request: {
        candidate_version: "native-e2e-fault-only",
        confirmation: "INSTALL HOST UPDATE",
      },
    })).catch(() => null);
    const crashed = await waitForProcessExit(crashProcess, "update-preflight-fault native host");
    await preflight;
    assert.equal(crashed.code, 98, `update-fault host exited by ${crashed.signal || "an unexpected status"}`);
    assert.equal(persistedNodeLabel(faultCapsule, nodeId), committedLabel);
    assert.equal(persistedHistoryCursor(faultCapsule), 1);
    assert.equal(existsSync(`${faultCapsule}-journal`), false, "update fault left a source journal");
    checked("python", ["tools/capsule.py", "verify", faultCapsule], root);
  } catch (error) {
    if (crashStderr.trim()) process.stderr.write(`update-fault native host stderr:\n${crashStderr}`);
    throw error;
  } finally {
    await crashRawBrowser?.close().catch(() => {});
    await crashParentBrowser?.close().catch(() => {});
    await stopApplication(crashProcess);
  }

  const sourceHashAfter = sha256File(faultCapsule);
  let restartProcess;
  let restartParentBrowser;
  let restartRawBrowser;
  let restartParentPage;
  let restartRawPage;
  let restartStderr = "";
  const restartParentPort = await freePort();
  let restartRawPort = await freePort();
  while (restartRawPort === restartParentPort) restartRawPort = await freePort();
  try {
    restartProcess = spawn(application, [], {
      cwd: root,
      env: {
        ...process.env,
        SQLITE_CAPSULE_NATIVE_PARENT_E2E_PORT: String(restartParentPort),
        SQLITE_CAPSULE_NATIVE_RAW_E2E_PORT: String(restartRawPort),
        SQLITE_CAPSULE_NATIVE_E2E_PATH: faultCapsule,
        SQLITE_CAPSULE_NATIVE_E2E_STATE_ROOT: faultStateRoot,
      },
      stdio: ["ignore", "ignore", "pipe"],
      windowsHide: true,
      shell: false,
    });
    restartProcess.stderr.setEncoding("utf8");
    restartProcess.stderr.on("data", (chunk) => { restartStderr += chunk; });
    const restartParentEndpoint = await waitForEndpoint(restartParentPort, "update-restart trusted parent");
    const restartRawEndpoint = await waitForEndpoint(restartRawPort, "update-restart raw child");
    restartParentBrowser = await chromium.connectOverCDP(restartParentEndpoint);
    restartRawBrowser = await chromium.connectOverCDP(restartRawEndpoint);
    restartParentPage = restartParentBrowser.contexts()[0]?.pages()[0];
    restartRawPage = restartRawBrowser.contexts()[0]?.pages()[0];
    assert.ok(restartParentPage, "update-restart trusted parent page is absent");
    assert.ok(restartRawPage, "update-restart raw child page is absent");
    await restartParentPage.locator("#host-state").waitFor({ state: "visible" });
    await restartParentPage.waitForFunction(() => document.querySelector("#host-state")?.textContent !== "Verifying before open");
    assert.equal(await restartParentPage.locator("#host-state").textContent(), "Trust decision required · code locked");
    const report = await restartParentPage.evaluate(() => globalThis.__TAURI__.core.invoke("startup_report"));
    assert.equal(report.stage, "first-open");
    assert.equal(report.capsule?.assets_released, false);
    assert.equal(report.capsule?.source_sha256, sourceHashAfter);
    await restartRawPage.waitForFunction(() => document.querySelector("#decision")?.textContent?.startsWith("PASS"));
    assert.equal(await restartRawPage.locator("#decision").textContent(), "PASS · no native handler");
    await restartParentPage.waitForFunction(
      () => document.querySelector("#lifecycle-status")?.textContent?.includes("Recovery inventory requires attention: 1 interrupted and 0 invalid artifact"),
    );
    const lifecycle = await restartParentPage.evaluate(() => globalThis.__TAURI__.core.invoke("lifecycle_status"));
    assert.equal(lifecycle.active, false);
    assert.equal(lifecycle.mode, "locked");
    assert.equal(lifecycle.backup_inventory.verified.length, 1);
    assert.equal(lifecycle.backup_inventory.verified[0].backup_id, prewriteBackup.backup_id);
    assert.equal(lifecycle.backup_inventory.incomplete_artifacts.length, 1);
    assert.deepEqual(lifecycle.backup_inventory.invalid_artifacts, []);
    const incompleteId = lifecycle.backup_inventory.incomplete_artifacts[0];
    const backupRoot = path.join(faultStateRoot, "capsule-backups");
    const incompletePath = path.join(backupRoot, incompleteId);
    const incompleteMarker = path.join(backupRoot, `${incompleteId}.in-progress`);
    const incompleteManifest = path.join(backupRoot, `${incompleteId}.json`);
    const prewritePath = path.join(backupRoot, prewriteBackup.backup_id);
    assert.ok(existsSync(incompletePath), "update fault did not retain checkpoint bytes");
    assert.ok(existsSync(incompleteMarker), "update fault did not retain its marker");
    assert.ok(existsSync(incompleteManifest), "update fault did not sync its manifest");
    assert.equal(persistedNodeLabel(incompletePath, nodeId), committedLabel);
    assert.equal(persistedHistoryCursor(incompletePath), 1);
    assert.equal(persistedNodeLabel(prewritePath, nodeId), originalLabel);
    assert.equal(persistedHistoryCursor(prewritePath), 0);
    checked("python", ["tools/capsule.py", "verify", incompletePath], root);
    checked("python", ["tools/capsule.py", "verify", prewritePath], root);
    assert.equal(sha256File(faultCapsule), sourceHashAfter);

    process.stdout.write(`${JSON.stringify({
      ok: true,
      durableFault: "update.manifest-synced",
      hostExitCode: 98,
      trustedCommand: "stage_host_update",
      signedUpdateAcceptanceClaimed: false,
      source: {
        path: faultCapsule,
        beforeSha256: sourceHashBefore,
        afterSha256: sourceHashAfter,
        committedLabel,
        historyCursor: 1,
        journalRetained: false,
      },
      verifiedPrewriteBackup: {
        id: prewriteBackup.backup_id,
        sha256: prewriteBackup.sha256,
        historyCursor: 0,
      },
      interruptedUpdatePreflight: {
        id: incompleteId,
        path: incompletePath,
        sha256: sha256File(incompletePath),
        historyCursor: 1,
        markerRetained: true,
        manifestPresent: true,
        treatedAsRecoverable: false,
      },
      restart: {
        stage: report.stage,
        assetsReleased: false,
        rawRendererLocked: true,
        verifiedArtifacts: 1,
        incompleteArtifacts: 1,
        invalidArtifacts: 0,
      },
      isolatedStateRoot: faultStateRoot,
    }, null, 2)}\n`);
  } catch (error) {
    if (restartStderr.trim()) process.stderr.write(`update-restart native host stderr:\n${restartStderr}`);
    throw error;
  } finally {
    await restartRawBrowser?.close().catch(() => {});
    await restartParentBrowser?.close().catch(() => {});
    await stopApplication(restartProcess);
  }
}

async function runCloseCrashScenario() {
  const faultStateRoot = path.join(root, ".tmp", "native-raw-close-fault-e2e-state");
  const faultDisposableRoot = path.join(root, ".tmp", "native-raw-close-fault-e2e-capsule");
  const faultCapsule = path.join(faultDisposableRoot, "diagram-studio.capsule.sqlite");
  const nodeId = "node-agent-prompt";
  const originalLabel = "One-sentence agent prompt";
  const committedLabel = `${originalLabel} · close crash E2E`;

  rmSync(faultStateRoot, { recursive: true, force: true });
  rmSync(faultDisposableRoot, { recursive: true, force: true });
  mkdirSync(faultStateRoot, { recursive: true });
  mkdirSync(faultDisposableRoot, { recursive: true });
  copyFileSync(sourceCapsule, faultCapsule);
  const sourceHashBefore = sha256File(faultCapsule);

  let crashProcess;
  let crashParentBrowser;
  let crashRawBrowser;
  let crashParentPage;
  let crashRawPage;
  let crashStderr = "";
  let prewriteBackup;
  const crashParentPort = await freePort();
  let crashRawPort = await freePort();
  while (crashRawPort === crashParentPort) crashRawPort = await freePort();
  try {
    crashProcess = spawn(application, [], {
      cwd: root,
      env: {
        ...process.env,
        SQLITE_CAPSULE_NATIVE_PARENT_E2E_PORT: String(crashParentPort),
        SQLITE_CAPSULE_NATIVE_RAW_E2E_PORT: String(crashRawPort),
        SQLITE_CAPSULE_NATIVE_E2E_PATH: faultCapsule,
        SQLITE_CAPSULE_NATIVE_E2E_STATE_ROOT: faultStateRoot,
        SQLITE_CAPSULE_NATIVE_E2E_RUNTIME_FAULTS: "enabled",
        SQLITE_CAPSULE_RUNTIME_FAULT_STAGE: "close.manifest-synced",
      },
      stdio: ["ignore", "ignore", "pipe"],
      windowsHide: true,
      shell: false,
    });
    crashProcess.stderr.setEncoding("utf8");
    crashProcess.stderr.on("data", (chunk) => { crashStderr += chunk; });
    const crashParentEndpoint = await waitForEndpoint(crashParentPort, "close-fault trusted parent");
    const crashRawEndpoint = await waitForEndpoint(crashRawPort, "close-fault raw child");
    crashParentBrowser = await chromium.connectOverCDP(crashParentEndpoint);
    crashRawBrowser = await chromium.connectOverCDP(crashRawEndpoint);
    crashParentPage = crashParentBrowser.contexts()[0]?.pages()[0];
    crashRawPage = crashRawBrowser.contexts()[0]?.pages()[0];
    assert.ok(crashParentPage, "close-fault trusted parent page is absent");
    assert.ok(crashRawPage, "close-fault raw child page is absent");
    await crashParentPage.locator("#host-state").waitFor({ state: "visible" });
    await crashParentPage.waitForFunction(() => document.querySelector("#host-state")?.textContent !== "Verifying before open");
    await crashParentPage.locator("button[data-action='allow_once']").click();
    await crashParentPage.waitForFunction(() => document.querySelector("#host-state")?.textContent?.includes("application running"));
    await crashRawPage.waitForURL(/\/app\/index\.html$/);
    await crashRawPage.locator("#app[aria-busy='false']").waitFor({ state: "attached", timeout: 20_000 });
    await crashRawPage.locator(`[data-id='${nodeId}']`).click();
    await crashRawPage.locator("#node-label-input").fill(committedLabel);
    await crashRawPage.locator("#save-node-label").click();
    await crashRawPage.waitForFunction(() => document.querySelector("#save-state")?.textContent === "Saved in SQLite");
    assert.equal(persistedNodeLabel(faultCapsule, nodeId), committedLabel);
    const lifecycle = await crashParentPage.evaluate(() => globalThis.__TAURI__.core.invoke("lifecycle_status"));
    prewriteBackup = lifecycle.backup;
    assert.ok(prewriteBackup?.backup_id, "close-fault pre-write backup is absent");
    requestNativeWindowClose(crashProcess.pid);
    const crashed = await waitForProcessExit(crashProcess, "close-fault native host");
    assert.equal(crashed.code, 98, `close-fault host exited by ${crashed.signal || "an unexpected status"}`);
    assert.equal(persistedNodeLabel(faultCapsule, nodeId), committedLabel);
    assert.notEqual(sha256File(faultCapsule), sourceHashBefore);
    assert.equal(existsSync(`${faultCapsule}-journal`), false, "close fault left a source journal");
    checked("python", ["tools/capsule.py", "verify", faultCapsule], root);
  } catch (error) {
    if (crashStderr.trim()) process.stderr.write(`close-fault native host stderr:\n${crashStderr}`);
    throw error;
  } finally {
    await crashRawBrowser?.close().catch(() => {});
    await crashParentBrowser?.close().catch(() => {});
    await stopApplication(crashProcess);
  }

  const sourceHashAfter = sha256File(faultCapsule);
  let restartProcess;
  let restartParentBrowser;
  let restartRawBrowser;
  let restartParentPage;
  let restartRawPage;
  let restartStderr = "";
  const restartParentPort = await freePort();
  let restartRawPort = await freePort();
  while (restartRawPort === restartParentPort) restartRawPort = await freePort();
  try {
    restartProcess = spawn(application, [], {
      cwd: root,
      env: {
        ...process.env,
        SQLITE_CAPSULE_NATIVE_PARENT_E2E_PORT: String(restartParentPort),
        SQLITE_CAPSULE_NATIVE_RAW_E2E_PORT: String(restartRawPort),
        SQLITE_CAPSULE_NATIVE_E2E_PATH: faultCapsule,
        SQLITE_CAPSULE_NATIVE_E2E_STATE_ROOT: faultStateRoot,
      },
      stdio: ["ignore", "ignore", "pipe"],
      windowsHide: true,
      shell: false,
    });
    restartProcess.stderr.setEncoding("utf8");
    restartProcess.stderr.on("data", (chunk) => { restartStderr += chunk; });
    const restartParentEndpoint = await waitForEndpoint(restartParentPort, "close-restart trusted parent");
    const restartRawEndpoint = await waitForEndpoint(restartRawPort, "close-restart raw child");
    restartParentBrowser = await chromium.connectOverCDP(restartParentEndpoint);
    restartRawBrowser = await chromium.connectOverCDP(restartRawEndpoint);
    restartParentPage = restartParentBrowser.contexts()[0]?.pages()[0];
    restartRawPage = restartRawBrowser.contexts()[0]?.pages()[0];
    assert.ok(restartParentPage, "close-restart trusted parent page is absent");
    assert.ok(restartRawPage, "close-restart raw child page is absent");
    await restartParentPage.locator("#host-state").waitFor({ state: "visible" });
    await restartParentPage.waitForFunction(() => document.querySelector("#host-state")?.textContent !== "Verifying before open");
    assert.equal(await restartParentPage.locator("#host-state").textContent(), "Trust decision required · code locked");
    const report = await restartParentPage.evaluate(() => globalThis.__TAURI__.core.invoke("startup_report"));
    assert.equal(report.stage, "first-open");
    assert.equal(report.capsule?.assets_released, false);
    assert.equal(report.capsule?.source_sha256, sourceHashAfter);
    await restartRawPage.waitForFunction(() => document.querySelector("#decision")?.textContent?.startsWith("PASS"));
    assert.equal(await restartRawPage.locator("#decision").textContent(), "PASS · no native handler");
    await restartParentPage.waitForFunction(
      () => document.querySelector("#lifecycle-status")?.textContent?.includes("Recovery inventory requires attention: 1 interrupted and 0 invalid artifact"),
    );
    const lifecycle = await restartParentPage.evaluate(() => globalThis.__TAURI__.core.invoke("lifecycle_status"));
    assert.equal(lifecycle.active, false);
    assert.equal(lifecycle.writable, false);
    assert.equal(lifecycle.mode, "locked");
    assert.equal(lifecycle.backup, null);
    assert.equal(lifecycle.backup_inventory.verified.length, 1);
    assert.equal(lifecycle.backup_inventory.verified[0].backup_id, prewriteBackup.backup_id);
    assert.equal(lifecycle.backup_inventory.incomplete_artifacts.length, 1);
    assert.deepEqual(lifecycle.backup_inventory.invalid_artifacts, []);
    const incompleteId = lifecycle.backup_inventory.incomplete_artifacts[0];
    assert.notEqual(incompleteId, prewriteBackup.backup_id);
    const backupRoot = path.join(faultStateRoot, "capsule-backups");
    const incompletePath = path.join(backupRoot, incompleteId);
    const incompleteMarker = path.join(backupRoot, `${incompleteId}.in-progress`);
    const incompleteManifest = path.join(backupRoot, `${incompleteId}.json`);
    const prewritePath = path.join(backupRoot, prewriteBackup.backup_id);
    assert.ok(existsSync(incompletePath), "close fault did not retain checkpoint bytes");
    assert.ok(existsSync(incompleteMarker), "close fault did not retain its durable marker");
    assert.ok(existsSync(incompleteManifest), "close fault did not sync its manifest");
    assert.equal(persistedNodeLabel(incompletePath, nodeId), committedLabel);
    assert.equal(persistedNodeLabel(prewritePath, nodeId), originalLabel);
    checked("python", ["tools/capsule.py", "verify", incompletePath], root);
    checked("python", ["tools/capsule.py", "verify", prewritePath], root);
    assert.equal(sha256File(faultCapsule), sourceHashAfter);
    assert.equal(persistedNodeLabel(faultCapsule, nodeId), committedLabel);

    process.stdout.write(`${JSON.stringify({
      ok: true,
      durableFault: "close.manifest-synced",
      hostExitCode: 98,
      closeRequestedThroughNativeWindow: true,
      source: {
        path: faultCapsule,
        beforeSha256: sourceHashBefore,
        afterSha256: sourceHashAfter,
        committedLabel,
        writeCommitted: true,
        journalRetained: false,
      },
      verifiedPrewriteBackup: {
        id: prewriteBackup.backup_id,
        sha256: prewriteBackup.sha256,
        originalLabel,
      },
      interruptedCloseCheckpoint: {
        id: incompleteId,
        path: incompletePath,
        sha256: sha256File(incompletePath),
        committedLabel,
        databaseCopied: true,
        markerRetained: true,
        manifestPresent: true,
        treatedAsRecoverable: false,
      },
      restart: {
        stage: report.stage,
        assetsReleased: false,
        rawRendererLocked: true,
        verifiedArtifacts: 1,
        incompleteArtifacts: 1,
        invalidArtifacts: 0,
      },
      isolatedStateRoot: faultStateRoot,
    }, null, 2)}\n`);
  } catch (error) {
    if (restartStderr.trim()) process.stderr.write(`close-restart native host stderr:\n${restartStderr}`);
    throw error;
  } finally {
    await restartRawBrowser?.close().catch(() => {});
    await restartParentBrowser?.close().catch(() => {});
    await stopApplication(restartProcess);
  }
}

async function runRestorePickerScenario() {
  const pickerStateRoot = path.join(root, ".tmp", "native-raw-picker-e2e-state");
  const pickerDisposableRoot = path.join(root, ".tmp", "native-raw-picker-e2e-capsule");
  const pickerCapsule = path.join(pickerDisposableRoot, "diagram-studio.capsule.sqlite");
  const pickerRestoreRoot = path.join(pickerStateRoot, "restored");
  const pickerRestoredCapsule = path.join(pickerRestoreRoot, "picker-restored.sqlitecapsule");
  const nodeId = "node-agent-prompt";
  const originalLabel = "One-sentence agent prompt";
  const committedLabel = `${originalLabel} · native save picker E2E`;
  const externalLabel = `${originalLabel} · native save picker conflict`;

  rmSync(pickerStateRoot, { recursive: true, force: true });
  rmSync(pickerDisposableRoot, { recursive: true, force: true });
  mkdirSync(pickerRestoreRoot, { recursive: true });
  mkdirSync(pickerDisposableRoot, { recursive: true });
  copyFileSync(sourceCapsule, pickerCapsule);
  assert.equal(sha256File(pickerCapsule), sourceHashBefore);
  assert.equal(existsSync(pickerRestoredCapsule), false);

  const pickerParentPort = await freePort();
  let pickerRawPort = await freePort();
  while (pickerRawPort === pickerParentPort) pickerRawPort = await freePort();
  let pickerProcess;
  let pickerParentBrowser;
  let pickerRawBrowser;
  let pickerParentPage;
  let pickerRawPage;
  let pickerStderr = "";
  let completed = false;
  try {
    pickerProcess = spawn(application, [], {
      cwd: root,
      env: {
        ...process.env,
        SQLITE_CAPSULE_NATIVE_PARENT_E2E_PORT: String(pickerParentPort),
        SQLITE_CAPSULE_NATIVE_RAW_E2E_PORT: String(pickerRawPort),
        SQLITE_CAPSULE_NATIVE_E2E_PATH: pickerCapsule,
        SQLITE_CAPSULE_NATIVE_E2E_STATE_ROOT: pickerStateRoot,
      },
      stdio: ["ignore", "ignore", "pipe"],
      windowsHide: true,
      shell: false,
    });
    pickerProcess.stderr.setEncoding("utf8");
    pickerProcess.stderr.on("data", (chunk) => { pickerStderr += chunk; });

    const pickerParentEndpoint = await waitForEndpoint(pickerParentPort, "save-picker trusted parent");
    const pickerRawEndpoint = await waitForEndpoint(pickerRawPort, "save-picker raw child");
    pickerParentBrowser = await chromium.connectOverCDP(pickerParentEndpoint);
    pickerRawBrowser = await chromium.connectOverCDP(pickerRawEndpoint);
    pickerParentPage = pickerParentBrowser.contexts()[0]?.pages()[0];
    pickerRawPage = pickerRawBrowser.contexts()[0]?.pages()[0];
    assert.ok(pickerParentPage, "save-picker trusted parent page is absent");
    assert.ok(pickerRawPage, "save-picker raw child page is absent");

    await pickerParentPage.locator("#host-state").waitFor({ state: "visible" });
    await pickerParentPage.waitForFunction(
      () => document.querySelector("#host-state")?.textContent === "Trust decision required · code locked",
    );
    await pickerParentPage.locator("button[data-action='allow_once']").click();
    await pickerParentPage.waitForFunction(
      () => document.querySelector("#host-state")?.textContent?.includes("application running"),
    );
    await pickerRawPage.waitForURL(/\/app\/index\.html$/);
    await pickerRawPage.locator("#app[aria-busy='false']").waitFor({ state: "attached", timeout: 20_000 });

    await pickerRawPage.locator(`[data-id='${nodeId}']`).click();
    await pickerRawPage.locator("#node-label-input").fill(committedLabel);
    await pickerRawPage.locator("#save-node-label").click();
    await pickerRawPage.waitForFunction(
      () => document.querySelector("#save-state")?.textContent === "Saved in SQLite",
    );
    assert.equal(persistedNodeLabel(pickerCapsule, nodeId), committedLabel);
    const lifecycleAfterWrite = await pickerParentPage.evaluate(
      () => globalThis.__TAURI__.core.invoke("lifecycle_status"),
    );
    assert.ok(lifecycleAfterWrite.backup?.backup_id, "save-picker pre-write backup is absent");
    assert.equal(lifecycleAfterWrite.active, true);
    assert.equal(lifecycleAfterWrite.writable, true);
    assert.equal(lifecycleAfterWrite.mode, "writable");

    externallyRenameNode(pickerCapsule, nodeId, externalLabel);
    const conflictError = await pickerRawPage.evaluate(async () => {
      try {
        await globalThis.SQLiteCapsuleClient.read("diagram.nodes", { diagram_id: "diagram-main" });
        return null;
      } catch (error) {
        return String(error);
      }
    });
    assert.match(conflictError || "", /capsule changed outside this host session/);
    await pickerParentPage.waitForFunction(
      () => document.querySelector("#host-state")?.textContent === "Session conflict · renderer locked",
    );
    await pickerRawPage.waitForURL(/\/__host\/locked$/, { timeout: 10_000 });
    const lifecycleAfterConflict = await pickerParentPage.evaluate(
      () => globalThis.__TAURI__.core.invoke("lifecycle_status"),
    );
    assert.equal(lifecycleAfterConflict.active, false);
    assert.equal(lifecycleAfterConflict.writable, false);
    assert.equal(lifecycleAfterConflict.mode, "conflict_closed");
    assert.equal(lifecycleAfterConflict.backup?.backup_id, lifecycleAfterWrite.backup.backup_id);
    assert.equal(await pickerParentPage.locator("#restore-button").isEnabled(), true);

    await pickerParentPage.locator("#restore-button").evaluate((button) => button.click());
    const dialogReport = driveWindowsSaveDialog(
      pickerProcess.pid,
      pickerRestoredCapsule,
      pickerStateRoot,
    );
    assert.equal(dialogReport.ok, true);
    assert.equal(dialogReport.host_process_id, pickerProcess.pid);
    assert.equal(dialogReport.dialog_class, "#32770");
    assert.equal(dialogReport.file_name_host_automation_id, "1001");
    assert.ok(
      ["ValuePattern", "WindowsSaveDialogKeyboard"].includes(dialogReport.file_name_input_pattern),
      `unexpected file-name input pattern: ${dialogReport.file_name_input_pattern}`,
    );
    assert.equal(dialogReport.save_button_automation_id, "1");
    assert.ok(
      ["InvokePattern", "KeyboardEnter"].includes(dialogReport.save_commit_method),
      `unexpected save commit method: ${dialogReport.save_commit_method}`,
    );
    assert.equal(comparableWindowsPath(dialogReport.destination), comparableWindowsPath(pickerRestoredCapsule));

    await waitForPath(pickerRestoredCapsule, "real save-picker restore output");
    await pickerParentPage.waitForFunction(
      () => document.querySelector("#lifecycle-action-status")?.textContent?.includes("Verified copy restored"),
    );
    await pickerRawPage.waitForURL(/\/__host\/locked$/, { timeout: 10_000 });
    const restoredReport = await pickerParentPage.evaluate(
      () => globalThis.__TAURI__.core.invoke("startup_report"),
    );
    const restoredSha256 = sha256File(pickerRestoredCapsule);
    assert.equal(restoredReport.stage, "first-open");
    assert.equal(restoredReport.capsule?.assets_released, false);
    assert.equal(
      comparableWindowsPath(restoredReport.capsule?.identity.canonical_path || ""),
      comparableWindowsPath(pickerRestoredCapsule),
    );
    assert.equal(restoredReport.capsule?.source_sha256, restoredSha256);
    assert.equal(persistedNodeLabel(pickerRestoredCapsule, nodeId), originalLabel);
    assert.equal(persistedNodeLabel(pickerCapsule, nodeId), externalLabel);
    assert.equal(existsSync(`${pickerRestoredCapsule}.capsule-restore-in-progress`), false);
    checked("python", ["tools/capsule.py", "verify", pickerRestoredCapsule], root);
    checked("python", ["tools/capsule.py", "verify", pickerCapsule], root);
    assert.equal(sha256File(sourceCapsule), sourceHashBefore, "save-picker scenario changed the checked capsule");

    process.stdout.write(`${JSON.stringify({
      ok: true,
      productionRestorePicker: {
        debugDestinationOverridePresent: false,
        dialogClass: dialogReport.dialog_class,
        dialogName: dialogReport.dialog_name,
        fileNameHostAutomationId: dialogReport.file_name_host_automation_id,
        fileNameInputAutomationId: dialogReport.file_name_input_automation_id,
        fileNameInputControlType: dialogReport.file_name_input_control_type,
        fileNameInputClass: dialogReport.file_name_input_class,
        fileNameInputPattern: dialogReport.file_name_input_pattern,
        saveButtonAutomationId: dialogReport.save_button_automation_id,
        saveCommitMethod: dialogReport.save_commit_method,
        hostProcessOwned: true,
        createdAtNewPath: true,
        existingFileReplaced: false,
      },
      verifiedRestore: {
        sha256: restoredSha256,
        bytes: statSync(pickerRestoredCapsule).size,
        restoredPrewriteLabel: originalLabel,
        applicationRemainedLocked: true,
      },
      conflictSource: {
        sha256: sha256File(pickerCapsule),
        externalLabel,
      },
      checkedSourceSha256: sourceHashBefore,
      isolatedStateRoot: pickerStateRoot,
    }, null, 2)}\n`);
    completed = true;
  } catch (error) {
    if (pickerStderr.trim()) process.stderr.write(`save-picker native host stderr:\n${pickerStderr}`);
    throw error;
  } finally {
    await pickerRawBrowser?.close().catch(() => {});
    await pickerParentBrowser?.close().catch(() => {});
    await stopApplication(pickerProcess);
    if (completed) {
      rmSync(pickerStateRoot, { recursive: true, force: true });
      rmSync(pickerDisposableRoot, { recursive: true, force: true });
      assert.equal(existsSync(pickerStateRoot), false, "save-picker isolated state cleanup failed");
      assert.equal(existsSync(pickerDisposableRoot), false, "save-picker disposable cleanup failed");
    }
  }
}

await runOpenCrashScenario();
await runRestoreCrashScenario();
await runPrewriteCrashScenario();
await runCheckpointCrashScenario();
await runUpdatePreflightCrashScenario();
await runCloseCrashScenario();
await runRestorePickerScenario();

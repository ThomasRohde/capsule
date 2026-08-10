import { test, expect } from "@playwright/test";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFileSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { pathToFileURL, fileURLToPath } from "node:url";
import path from "node:path";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const working = path.join(root, ".tmp", "browser-html");
const capsule = path.join(working, "diagram-studio.capsule.sqlite");
const parityCapsule = path.join(working, "parity.capsule.sqlite");
const limitCapsule = path.join(working, "limit.capsule.sqlite");
const allowedStaticOrigins = new Set([
  "http://127.0.0.1:41741",
  "http://127.0.0.1:41742",
]);
const networkGuards = new WeakMap();

function sha256File(file) {
  return createHash("sha256").update(readFileSync(file)).digest("hex");
}

function independentlyExtractedNodeLabel(htmlPath, nodeId, outputPath) {
  const script = [
    "import base64, gzip, sqlite3, sys",
    "from html.parser import HTMLParser",
    "from pathlib import Path",
    "class PayloadParser(HTMLParser):",
    " def __init__(self):",
    "  super().__init__(convert_charrefs=False); self.active=False; self.parts=[]",
    " def handle_starttag(self, tag, attrs):",
    "  self.active = tag == 'script' and dict(attrs).get('id') == 'sqlite-capsule-database'",
    " def handle_endtag(self, tag):",
    "  if tag == 'script': self.active = False",
    " def handle_data(self, data):",
    "  if self.active: self.parts.append(data)",
    "parser=PayloadParser(); parser.feed(Path(sys.argv[1]).read_text(encoding='utf-8'))",
    "compact=''.join(''.join(parser.parts).split())",
    "payload=base64.b64decode(compact, validate=True)",
    "Path(sys.argv[3]).write_bytes(gzip.decompress(payload))",
    "uri='file:' + Path(sys.argv[3]).resolve().as_posix() + '?mode=ro&immutable=1'",
    "connection=sqlite3.connect(uri, uri=True)",
    "try: print(connection.execute('SELECT label FROM diagram_node WHERE id = ?', (sys.argv[2],)).fetchone()[0])",
    "finally: connection.close()",
  ].join("\n");
  return execFileSync(
    "python",
    ["-c", script, htmlPath, nodeId, outputPath],
    { cwd: root, encoding: "utf8" },
  ).trim();
}

function pythonOutcome(name, parameters, database = capsule) {
  const script = [
    "import json, sys",
    "from pathlib import Path",
    "sys.path.insert(0, sys.argv[1])",
    "from runtime.capsule_host import CapsuleDatabase",
    "with CapsuleDatabase(Path(sys.argv[2]), read_only=True) as capsule:",
    " try:",
    "  result = capsule.execute_endpoint(sys.argv[3], 'read', json.loads(sys.argv[4]))",
    "  outcome = {'ok': True, 'result': result}",
    " except Exception as error:",
    "  outcome = {'ok': False, 'error': str(error)}",
    " print(json.dumps(outcome, sort_keys=True, separators=(',', ':')))" ,
  ].join("\n");
  return JSON.parse(execFileSync(
    "python",
    ["-c", script, root, database, name, JSON.stringify(parameters)],
    { cwd: root, encoding: "utf8" },
  ));
}

function pythonRead(name, parameters, database = capsule) {
  const outcome = pythonOutcome(name, parameters, database);
  if (!outcome.ok) throw new Error(outcome.error);
  return outcome.result;
}

async function installNetworkGuard(page) {
  if (networkGuards.has(page)) return networkGuards.get(page);
  const unexpected = [];
  await page.route("http*://**", async (route) => {
    const url = new URL(route.request().url());
    if (allowedStaticOrigins.has(url.origin)) await route.continue();
    else {
      unexpected.push(route.request().url());
      await route.abort("blockedbyclient");
    }
  });
  networkGuards.set(page, unexpected);
  return unexpected;
}

async function openExport(page, profile, url = `/diagram-studio-${profile}.html`) {
  const unexpected = await installNetworkGuard(page);
  await page.goto(url);
  await expect(page.locator("#capsule-host-status")).toHaveAttribute("data-state", "ready");
  await expect(page.locator("#capsule-host-profile")).toHaveText(profile);
  const diagnostics = await page.locator("body").evaluate((body) => ({
    boot: Number(body.dataset.bootMilliseconds),
    database: Number(body.dataset.databaseBytes),
    wasmHeap: Number(body.dataset.wasmHeapBytes),
  }));
  expect(diagnostics.boot).toBeGreaterThan(0);
  expect(diagnostics.boot).toBeLessThan(30_000);
  expect(diagnostics.database).toBeGreaterThan(800_000);
  expect(diagnostics.wasmHeap).toBeGreaterThan(diagnostics.database);
  const app = page.frameLocator("#capsule-app-frame");
  await expect(app.locator("#app")).toHaveAttribute("aria-busy", "false");
  await expect(app.locator(".node")).toHaveCount(12);
  await expect(app.locator(".edge")).toHaveCount(13);
  const networkDependencies = await page.evaluate(() => performance.getEntriesByType("resource")
    .map((entry) => entry.name)
    .filter((name) => /^https?:/i.test(name)));
  expect(networkDependencies).toEqual([]);
  expect(unexpected).toEqual([]);
  return app;
}

async function appRequest(page, operation, argument) {
  const frame = page.frames().find((candidate) => candidate.parentFrame() === page.mainFrame());
  expect(frame).toBeTruthy();
  return frame.evaluate(operation, argument);
}

async function appOutcome(page, name, parameters) {
  return appRequest(page, async ({ name, parameters }) => {
    try {
      return { ok: true, result: await globalThis.SQLiteCapsuleClient.read(name, parameters) };
    } catch (error) {
      return { ok: false, error: String(error?.message || error) };
    }
  }, { name, parameters });
}

async function expectStateScreenshots(page, stem) {
  await page.setViewportSize({ width: 1440, height: 900 });
  await expect(page).toHaveScreenshot(`${stem}.png`, { animations: "disabled", maxDiffPixelRatio: 0.015 });
  await page.setViewportSize({ width: 1280, height: 720 });
  await expect(page).toHaveScreenshot(`${stem}-laptop.png`, { animations: "disabled", maxDiffPixelRatio: 0.015 });
}

for (const profile of ["view", "interactive"]) {
  test(`${profile} profile preserves read parity and denies writes`, async ({ page }) => {
    const app = await openExport(page, profile);
    const expectedNodes = pythonRead("diagram.nodes", { diagram_id: "diagram-main" });
    const actualNodes = await appRequest(page, () => globalThis.SQLiteCapsuleClient.read(
      "diagram.nodes",
      { diagram_id: "diagram-main" },
    ));
    expect(actualNodes).toEqual(expectedNodes);
    const publicClient = await appRequest(page, () => ({
      keys: Object.keys(globalThis.SQLiteCapsuleClient).sort(),
      hasSql: typeof globalThis.SQLiteCapsuleClient.sql !== "undefined",
      bridgeKeys: Object.keys(globalThis.__sqliteCapsuleBridge).sort(),
    }));
    expect(publicClient.hasSql).toBeFalsy();
    expect(publicClient.bridgeKeys).toEqual(["manifest", "permissions", "read", "write"]);
    expect(publicClient.keys).not.toContain("export");
    expect(publicClient.keys).not.toContain("sql");
    await expect(app.locator("#add-node")).toBeHidden();
    await expect(app.locator(".scene-authoring")).toBeHidden();
    if (profile === "view") {
      await expect(app.locator(".inspector-panel")).toBeHidden();
      await expect(app.locator("#copy-selection")).toBeHidden();
      await expect(app.locator("#export-diagram")).toBeHidden();
    } else {
      await expect(app.locator(".inspector-panel")).toBeVisible();
      await app.locator(".overflow-menu > summary").click();
      await expect(app.locator("#copy-selection")).toBeVisible();
      await expect(app.locator("#inspector")).toHaveAttribute("aria-disabled", "true");
    }
    const error = await appRequest(page, async () => {
      try {
        await globalThis.SQLiteCapsuleClient.write("node.rename", {});
        return null;
      } catch (caught) {
        return String(caught.message || caught);
      }
    });
    expect(error).toContain(`Export profile '${profile}' is read-only`);
    await expect(page.locator("body")).toHaveAttribute("data-dirty", "false");
    await expect(page.locator("#capsule-save-html")).toBeHidden();
    await expect(page.locator("#capsule-download-html")).toBeHidden();
  });
}

test("generic endpoint coercion, result, limit, and concurrency behavior matches the Python host", async ({ page }) => {
  await openExport(page, "view", "/parity-view.html");
  const parameters = {
    integer: "42",
    number: "3.5",
    boolean: "true",
    payload: { z: [1, true, null], a: { nested: "value" } },
    text: "capsule parity",
  };
  expect(await appOutcome(page, "parity.parameters", parameters)).toEqual(
    pythonOutcome("parity.parameters", parameters, parityCapsule),
  );
  expect(await appOutcome(page, "parity.scalar-int64", {})).toEqual(
    pythonOutcome("parity.scalar-int64", {}, parityCapsule),
  );

  for (const [name, invalidParameters] of [
    ["parity.parameters", { ...parameters, unknown: true }],
    ["parity.parameters", { ...parameters, integer: null }],
    ["parity.parameters", { integer: 1 }],
    ["parity.blob-rejected", {}],
    ["parity.row-limit", {}],
    ["parity.byte-limit", {}],
  ]) {
    expect(await appOutcome(page, name, invalidParameters)).toEqual(
      pythonOutcome(name, invalidParameters, parityCapsule),
    );
  }

  const concurrent = await appRequest(page, async () => (await Promise.allSettled([
    globalThis.SQLiteCapsuleClient.read("parity.slow", {}),
    ...Array.from({ length: 8 }, () => globalThis.SQLiteCapsuleClient.read(
      "diagram.nodes",
      { diagram_id: "diagram-main" },
    )),
  ])).map((outcome) => outcome.status === "fulfilled"
    ? { status: "fulfilled" }
    : { status: "rejected", error: String(outcome.reason?.message || outcome.reason) }));
  const rejected = concurrent.filter((outcome) => outcome.status === "rejected");
  expect(concurrent.filter((outcome) => outcome.status === "fulfilled")).toHaveLength(8);
  expect(rejected).toHaveLength(1);
  expect(rejected[0].error).toContain("Too many concurrent requests; limit is 8");
});

test("profile keyboard flows, undo/redo, presentation, and reduced motion remain functional", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  const app = await openExport(page, "editable");
  expect(await appRequest(page, () => matchMedia("(prefers-reduced-motion: reduce)").matches)).toBeTruthy();
  await appRequest(page, () => {
    globalThis.__acceptanceKeyEvents = [];
    globalThis.__acceptanceClicks = [];
    document.addEventListener("keydown", (event) => globalThis.__acceptanceKeyEvents.push(`down:${event.key}`));
    document.addEventListener("keyup", (event) => globalThis.__acceptanceKeyEvents.push(`up:${event.key}`));
    document.addEventListener("click", (event) => globalThis.__acceptanceClicks.push(event.target?.id || event.target?.tagName));
  });
  await app.locator("#add-node").focus();
  await expect(app.locator("#add-node")).toBeFocused();
  expect(await page.locator("#capsule-app-frame").evaluate((frame) => document.activeElement === frame)).toBeTruthy();
  await page.keyboard.press("Space");
  expect(await appRequest(page, () => globalThis.__acceptanceKeyEvents)).toEqual(["down: ", "up: "]);
  expect(await appRequest(page, () => globalThis.__acceptanceClicks)).toContain("add-node");
  await page.waitForTimeout(500);
  expect(await appRequest(page, () => ({
    toast: document.querySelector("#toast")?.textContent,
    profile: document.body.dataset.exportProfile,
    nodes: document.querySelectorAll(".node").length,
  }))).toEqual({ toast: "New node inserted into diagram_node.", profile: "editable", nodes: 13 });
  await expect(app.locator(".node")).toHaveCount(13);
  await app.locator("#undo").press("Space");
  await expect(app.locator(".node")).toHaveCount(12);
  await app.locator("#redo").press("Space");
  await expect(app.locator(".node")).toHaveCount(13);

  await app.locator("#present").click();
  await expect(app.locator("body")).toHaveClass(/presenting/);
  const firstIndex = await app.locator("#presentation-index").textContent();
  await app.locator("#next-scene").click();
  await expect(app.locator("#presentation-index")).not.toHaveText(firstIndex || "");
  await app.locator("#exit-presentation").click();
  await expect(app.locator("body")).not.toHaveClass(/presenting/);
});

test("editable profile commits, rolls back failures, and downloads a verifiable revision", async ({ page }, testInfo) => {
  const sourceBefore = sha256File(capsule);
  const exportPath = path.join(working, "diagram-studio-editable.html");
  const exportBefore = sha256File(exportPath);
  const app = await openExport(page, "editable");
  await expect(app.locator("#add-node")).toBeVisible();
  const before = await appRequest(page, async () => ({
    nodes: await globalThis.SQLiteCapsuleClient.read("diagram.nodes", { diagram_id: "diagram-main" }),
    history: await globalThis.SQLiteCapsuleClient.read("diagram.history", { diagram_id: "diagram-main" }),
  }));
  const node = before.nodes[0];
  const rollbackError = await appRequest(page, async ({ node, cursor }) => {
    try {
      await globalThis.SQLiteCapsuleClient.write("node.rename", {
        operation_id: `browser-rollback-${Date.now()}`,
        diagram_id: "diagram-main",
        expected_cursor: cursor,
        id: node.id,
        from_label: "not-the-current-label",
        to_label: "must-not-commit",
      });
      return null;
    } catch (caught) {
      return String(caught.message || caught);
    }
  }, { node, cursor: before.history.cursor });
  expect(rollbackError).toMatch(/changed 0 rows|expected 1/i);
  const afterFailure = await appRequest(page, async ({ id }) => ({
    nodes: await globalThis.SQLiteCapsuleClient.read("diagram.nodes", { diagram_id: "diagram-main" }),
    history: await globalThis.SQLiteCapsuleClient.read("diagram.history", { diagram_id: "diagram-main" }),
    id,
  }), { id: node.id });
  expect(afterFailure.history.cursor).toBe(before.history.cursor);
  expect(afterFailure.nodes.find((item) => item.id === node.id).label).toBe(node.label);

  const renamed = `${node.label} · browser export`;
  await appRequest(page, ({ node, cursor, renamed }) => globalThis.SQLiteCapsuleClient.write("node.rename", {
    operation_id: `browser-rename-${Date.now()}`,
    diagram_id: "diagram-main",
    expected_cursor: cursor,
    id: node.id,
    from_label: node.label,
    to_label: renamed,
  }), { node, cursor: before.history.cursor, renamed });
  const afterSuccess = await appRequest(page, () => globalThis.SQLiteCapsuleClient.read(
    "diagram.nodes",
    { diagram_id: "diagram-main" },
  ));
  expect(afterSuccess.find((item) => item.id === node.id).label).toBe(renamed);
  await expect(page.locator("body")).toHaveAttribute("data-dirty", "true");
  await expect(page.locator("#capsule-download-html")).toBeEnabled();

  const [download] = await Promise.all([
    page.waitForEvent("download"),
    page.locator("#capsule-download-html").click(),
  ]);
  const savedPath = await download.path();
  const report = JSON.parse(execFileSync(
    "python",
    ["tools/capsule.py", "verify-html", savedPath],
    { cwd: root, encoding: "utf8" },
  ));
  expect(report.ok).toBeTruthy();
  expect(report.revision).toBe(2);
  expect(report.parent_database_sha256).toBe(report.source_sha256);
  expect(report.database_sha256).not.toBe(report.source_sha256);
  expect(independentlyExtractedNodeLabel(
    savedPath,
    node.id,
    path.join(working, `extracted-${testInfo.project.name}.capsule.sqlite`),
  )).toBe(renamed);
  await expect(page.locator("body")).toHaveAttribute("data-dirty", "false");
  expect(Number(await page.locator("body").getAttribute("data-save-milliseconds"))).toBeGreaterThan(0);
  const reopenedName = `saved-${testInfo.project.name.replace(/[^a-z0-9-]/gi, "-")}.html`;
  copyFileSync(savedPath, path.join(working, reopenedName));
  await openExport(page, "editable", `/${reopenedName}`);
  await expect(page.locator("#capsule-host-provenance")).toContainText("revision 2");
  const reopenedNodes = await appRequest(page, () => globalThis.SQLiteCapsuleClient.read(
    "diagram.nodes",
    { diagram_id: "diagram-main" },
  ));
  expect(reopenedNodes.find((item) => item.id === node.id).label).toBe(renamed);
  expect(sha256File(capsule)).toBe(sourceBefore);
  expect(sha256File(exportPath)).toBe(exportBefore);
});

test("static-hosted exports have no page-level horizontal overflow", async ({ page }) => {
  const app = await openExport(page, "view");
  await page.locator("#capsule-third-party-button").click();
  await expect(page.locator("#capsule-third-party-panel")).toBeVisible();
  await expect(page.locator("#capsule-third-party-text")).toContainText("Apache License");
  await page.locator("#capsule-third-party-close").click();
  await page.setViewportSize({ width: 720, height: 900 });
  const shell = await page.evaluate(() => ({ width: innerWidth, scrollWidth: document.documentElement.scrollWidth }));
  expect(shell.scrollWidth).toBeLessThanOrEqual(shell.width);
  await expect(app.locator("#app")).toHaveAttribute("aria-busy", "false");
});

test("portable in-memory host works with optional COOP/COEP headers", async ({ page }) => {
  const app = await openExport(page, "interactive", "http://127.0.0.1:41742/diagram-studio-interactive.html");
  expect(await page.evaluate(() => globalThis.crossOriginIsolated)).toBeTruthy();
  await expect(app.locator("#app")).toHaveAttribute("aria-busy", "false");
});

test("editable download fallback does not depend on OPFS or a file picker", async ({ page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(globalThis, "showSaveFilePicker", { configurable: true, value: undefined });
    if (navigator.storage) {
      Object.defineProperty(navigator.storage, "getDirectory", { configurable: true, value: undefined });
    }
  });
  const app = await makeDirty(page);
  await expect(page.locator("#capsule-save-html")).toHaveText("Save HTML (download)");
  await Promise.all([
    page.waitForEvent("download"),
    page.locator("#capsule-save-html").click(),
  ]);
  await expect(page.locator("#capsule-host-status")).toContainText("downloaded");
  await expect(page.locator("body")).toHaveAttribute("data-dirty", "false");
  await expect(app.locator("#app")).toHaveAttribute("aria-busy", "false");
});

test("missing compression support preserves the unsaved working copy", async ({ page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(globalThis, "CompressionStream", { configurable: true, value: undefined });
  });
  const app = await makeDirty(page);
  await page.locator("#capsule-download-html").click();
  await expect(page.locator("#capsule-host-error")).toContainText("gzip CompressionStream");
  await expect(page.locator("body")).toHaveAttribute("data-dirty", "true");
  await expect(app.locator("#app")).toHaveAttribute("aria-busy", "false");
});

for (const fixture of [
  ["oversize-metadata.html", "source.bytes must be a positive bounded integer"],
  ["decompression-overrun.html", "database decompressed data exceeds its declared size"],
]) {
  test(`${fixture[0]} fails within declared bounds before application execution`, async ({ page }) => {
    await installNetworkGuard(page);
    await page.goto(`/${fixture[0]}`);
    await expect(page.locator("#capsule-host-status")).toHaveAttribute("data-state", "error");
    await expect(page.locator("#capsule-host-error")).toContainText(fixture[1]);
    await expect(page.frameLocator("#capsule-app-frame").locator("#app")).toHaveCount(0);
  });
}

test("near-limit capsule boots, edits, saves, verifies, and leaves its source unchanged", async ({ page }, testInfo) => {
  const exportPath = path.join(working, "limit-editable.html");
  const sourceBefore = sha256File(limitCapsule);
  const exportBefore = sha256File(exportPath);
  const app = await openExport(page, "editable", "/limit-editable.html");
  const metrics = await page.locator("body").evaluate((body) => ({
    boot_milliseconds: Number(body.dataset.bootMilliseconds),
    database_bytes: Number(body.dataset.databaseBytes),
    wasm_heap_bytes: Number(body.dataset.wasmHeapBytes),
  }));
  expect(metrics.database_bytes).toBeGreaterThanOrEqual(62 * 1024 * 1024);
  expect(metrics.database_bytes).toBeLessThanOrEqual(64 * 1024 * 1024);
  expect(metrics.wasm_heap_bytes).toBeGreaterThanOrEqual(metrics.database_bytes);
  await app.locator("#add-node").click();
  await expect(app.locator(".node")).toHaveCount(13);
  const [download] = await Promise.all([
    page.waitForEvent("download"),
    page.locator("#capsule-download-html").click(),
  ]);
  const savedPath = await download.path();
  const report = JSON.parse(execFileSync(
    "python",
    ["tools/capsule.py", "verify-html", savedPath],
    { cwd: root, encoding: "utf8" },
  ));
  expect(report.ok).toBeTruthy();
  expect(report.revision).toBe(2);
  const saveMilliseconds = Number(await page.locator("body").getAttribute("data-save-milliseconds"));
  const saveWasmHeapBytes = Number(await page.locator("body").getAttribute("data-save-wasm-heap-bytes"));
  expect(saveMilliseconds).toBeGreaterThan(0);
  expect(saveMilliseconds).toBeLessThan(90_000);
  expect(saveWasmHeapBytes).toBeGreaterThanOrEqual(metrics.wasm_heap_bytes);
  expect(sha256File(limitCapsule)).toBe(sourceBefore);
  expect(sha256File(exportPath)).toBe(exportBefore);
  writeFileSync(path.join(working, `limit-metrics-${testInfo.project.name}.json`), JSON.stringify({
    ...metrics,
    save_milliseconds: saveMilliseconds,
    save_wasm_heap_bytes: saveWasmHeapBytes,
    peak_observed_wasm_heap_bytes: Math.max(metrics.wasm_heap_bytes, saveWasmHeapBytes),
    saved_html_bytes: statSync(savedPath).size,
  }, null, 2));
});

test("file URL profiles, write policy, save, and fresh revision reopen", async ({ page }, testInfo) => {
  test.skip(!process.env.SQLITE_CAPSULE_RUN_FILE_TESTS, "Enable only in an environment that authorizes local file navigation");
  for (const profile of ["view", "interactive"]) {
    const url = pathToFileURL(path.join(working, `diagram-studio-${profile}.html`)).href;
    await openExport(page, profile, url);
    expect(await appRequest(page, () => globalThis.SQLiteCapsuleClient.read(
      "diagram.get",
      { diagram_id: "diagram-main" },
    ))).toEqual(pythonRead("diagram.get", { diagram_id: "diagram-main" }));
    const denied = await appRequest(page, async () => {
      try {
        await globalThis.SQLiteCapsuleClient.write("node.rename", {});
        return "unexpected success";
      } catch (error) {
        return String(error?.message || error);
      }
    });
    expect(denied).toContain(`Export profile '${profile}' is read-only`);
  }

  const editableUrl = pathToFileURL(path.join(working, "diagram-studio-editable.html")).href;
  const app = await openExport(page, "editable", editableUrl);
  await app.locator("#add-node").click();
  await expect(page.locator("body")).toHaveAttribute("data-dirty", "true");
  const [download] = await Promise.all([
    page.waitForEvent("download"),
    page.locator("#capsule-download-html").click(),
  ]);
  const downloadedPath = await download.path();
  const reopenedPath = path.join(working, `file-reopen-${testInfo.project.name}.html`);
  copyFileSync(downloadedPath, reopenedPath);
  const report = JSON.parse(execFileSync(
    "python",
    ["tools/capsule.py", "verify-html", reopenedPath],
    { cwd: root, encoding: "utf8" },
  ));
  expect(report.ok).toBeTruthy();
  expect(report.revision).toBe(2);
  const reopened = await openExport(page, "editable", pathToFileURL(reopenedPath).href);
  await expect(page.locator("#capsule-host-provenance")).toContainText("revision 2");
  await expect(reopened.locator(".node")).toHaveCount(13);
});

for (const capability of [
  ["Worker", "dedicated Web Workers"],
  ["DecompressionStream", "gzip DecompressionStream"],
]) {
  test(`missing ${capability[0]} fails before application execution`, async ({ page }) => {
    await page.addInitScript((name) => {
      Object.defineProperty(globalThis, name, { configurable: true, value: undefined });
    }, capability[0]);
    await page.goto("/diagram-studio-view.html");
    await expect(page.locator("#capsule-host-status")).toHaveAttribute("data-state", "error");
    await expect(page.locator("#capsule-host-error")).toContainText(capability[1]);
    await expect(page.frameLocator("#capsule-app-frame").locator("#app")).toHaveCount(0);
  });
}

for (const fixture of [
  "invalid-capsule.html",
  "invalid-trigger.html",
  "invalid-endpoint.html",
  "invalid-check.html",
]) {
  test(`${fixture} verification failure never executes the entry asset`, async ({ page }) => {
    await installNetworkGuard(page);
    await page.goto(`/${fixture}`);
    await expect(page.locator("#capsule-host-status")).toHaveAttribute("data-state", "error");
    await expect(page.locator("#capsule-host-error")).toContainText("Capsule verification failed");
    await expect(page.frameLocator("#capsule-app-frame").locator("#app")).toHaveCount(0);
  });
}

async function makeDirty(page) {
  const app = await openExport(page, "editable");
  await app.locator("#add-node").click();
  await expect(page.locator("body")).toHaveAttribute("data-dirty", "true");
  return app;
}

test("user-picked save writes and verifies a clean revision", async ({ page, browserName }, testInfo) => {
  test.skip(browserName !== "chromium", "The picker branch is exercised once with a deterministic API double");
  await page.addInitScript(() => {
    globalThis.__pickerWrites = [];
    globalThis.showSaveFilePicker = async () => ({
      createWritable: async () => ({
        write: async (blob) => { globalThis.__pickerWrites.push(await blob.text()); },
        close: async () => {},
      }),
    });
  });
  await makeDirty(page);
  await expect(page.locator("#capsule-save-html")).toHaveText("Save HTML");
  await page.locator("#capsule-save-html").click();
  await expect(page.locator("#capsule-host-status")).toContainText("HTML revision saved");
  await expect(page.locator("body")).toHaveAttribute("data-dirty", "false");
  expect(Number(await page.locator("body").getAttribute("data-save-milliseconds"))).toBeGreaterThan(0);
  const saved = await page.evaluate(() => globalThis.__pickerWrites.at(-1));
  expect(saved.length).toBeGreaterThan(900_000);
  const target = path.join(working, `picker-${testInfo.project.name}.html`);
  writeFileSync(target, saved, "utf8");
  const report = JSON.parse(execFileSync("python", ["tools/capsule.py", "verify-html", target], { cwd: root, encoding: "utf8" }));
  expect(report.ok).toBeTruthy();
  expect(report.revision).toBe(2);
});

for (const scenario of [
  { name: "cancelled", errorName: "AbortError", message: "picker cancelled", expected: "Save cancelled" },
  { name: "denied", errorName: "NotAllowedError", message: "picker denied", expected: "picker denied" },
  { name: "I/O failure", errorName: "UnknownError", message: "disk full", expected: "disk full" },
]) {
  test(`picker ${scenario.name} preserves dirty state`, async ({ page, browserName }) => {
    test.skip(browserName !== "chromium", "The picker branch is exercised once with deterministic API doubles");
    await page.addInitScript(({ errorName, message }) => {
      globalThis.showSaveFilePicker = async () => { throw new DOMException(message, errorName); };
    }, scenario);
    await makeDirty(page);
    await page.locator("#capsule-save-html").click();
    if (scenario.errorName === "AbortError") await expect(page.locator("#capsule-host-status")).toContainText(scenario.expected);
    else await expect(page.locator("#capsule-host-error")).toContainText(scenario.expected);
    await expect(page.locator("body")).toHaveAttribute("data-dirty", "true");
    await expect(page.locator("#capsule-save-html")).toBeEnabled();
  });
}

test("HTML export profile and save/error visual states", async ({ page, browserName }) => {
  test.skip(browserName !== "chromium", "Stable visual evidence is captured once in Chromium");
  await openExport(page, "view");
  await expectStateScreenshots(page, "sqlite-capsule-html-view");
  await openExport(page, "interactive");
  await expectStateScreenshots(page, "sqlite-capsule-html-interactive");
  const app = await openExport(page, "editable");
  await expectStateScreenshots(page, "sqlite-capsule-html-editable");
  await app.locator("#add-node").click();
  await expect(page.locator("body")).toHaveAttribute("data-dirty", "true");
  await expectStateScreenshots(page, "sqlite-capsule-html-dirty");
  await Promise.all([
    page.waitForEvent("download"),
    page.locator("#capsule-download-html").click(),
  ]);
  await expect(page.locator("#capsule-host-status")).toContainText("downloaded");
  await expectStateScreenshots(page, "sqlite-capsule-html-save-success");
  await page.goto("/invalid-capsule.html");
  await expect(page.locator("#capsule-host-status")).toHaveAttribute("data-state", "error");
  await expectStateScreenshots(page, "sqlite-capsule-html-error");
});

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import net from "node:net";
import path from "node:path";
import { spawn, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../..");
const application = process.env.SQLITE_CAPSULE_NATIVE_APPLICATION
  || path.join(root, "native", "target", "debug", "sqlite-capsule-desktop.exe");
const nativeCli = path.join(root, "native", "target", "debug", "capsule-native.exe");
const evidenceRoot = path.join(root, ".tmp", "native-m03-overview-evidence");
const fixtureRoot = path.join(root, ".tmp", "native-m03-overview-fixtures");

if (process.platform !== "win32") throw new Error("Cabinet Overview visual acceptance is Windows-only");
if (!existsSync(application)) throw new Error(`native debug application is absent: ${application}`);

function checked(command, args) {
  const result = spawnSync(command, args, { cwd: root, encoding: "utf8", windowsHide: true, shell: false });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(result.stderr.trim() || `${command} exited ${result.status}`);
}

function sha256File(filePath) {
  return createHash("sha256").update(readFileSync(filePath)).digest("hex");
}

function prepareV03Fixture(output, state) {
  const source = [
    "import pathlib, sqlite3, sys",
    "root, output, state = map(pathlib.Path, sys.argv[1:4])",
    "connection = sqlite3.connect(output)",
    "for relative in ('format/capsule-v0.3.sql', 'format/capsule-signed-app-v0.3.sql', 'compatibility/signed-app-v0.3/fixture-v0.3.sql'):",
    "    connection.executescript((root / relative).read_text(encoding='utf-8'))",
    "if state.name == 'invalid':",
    "    connection.execute('UPDATE capsule_signature SET signature = zeroblob(length(signature))')",
    "elif state.name == 'unsigned':",
    "    connection.executescript('DROP TABLE capsule_signature; DROP TABLE capsule_publisher;')",
    "connection.commit()",
    "connection.close()",
  ].join("\n");
  checked("python", ["-c", source, root, output, state]);
}

function prepareSensitiveV03Fixture(signedSource, unsignedWorking, output) {
  copyFileSync(signedSource, unsignedWorking);
  checked("python", [
    "-c",
    "import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); c.execute(\"UPDATE capsule_dataset SET sensitivity='sensitive' WHERE id='content'\"); c.execute('DELETE FROM capsule_signature'); c.commit(); c.close()",
    unsignedWorking,
  ]);
  checked(nativeCli, [
    "sign", unsignedWorking, output,
    "--publisher-id", "org.example.vector",
    "--publisher-name", "Vector Publisher",
    "--key", path.join(root, "compatibility", "signed-app-v0.2", "development-seed.hex"),
    "--signed-at", "2026-08-13T05:00:00Z",
  ]);
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

async function waitForEndpoint(port) {
  const endpoint = `http://127.0.0.1:${port}`;
  const deadline = Date.now() + 20_000;
  let lastError;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(`${endpoint}/json/version`);
      if (response.ok) return endpoint;
      lastError = new Error(`HTTP ${response.status}`);
    } catch (error) {
      lastError = error;
    }
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  throw new Error(`CDP endpoint did not start: ${lastError}`);
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
      stdio: "ignore", windowsHide: true, shell: false,
    });
  }
}

async function assertTwoHundredPercentScaling(page) {
  const session = await page.context().newCDPSession(page);
  try {
    await session.send("Emulation.setPageScaleFactor", { pageScaleFactor: 2 });
    await page.waitForFunction(() => (window.visualViewport?.scale ?? 1) >= 1.99);
    await page.evaluate(() => document.querySelector("#page-content").scrollIntoView({ block: "start", inline: "start" }));
    await page.locator("#cabinet-open-button").scrollIntoViewIfNeeded();
    const layout = await page.evaluate(() => {
      const visible = (element) => {
        const rect = element.getBoundingClientRect();
        return rect.width > 0 && rect.height > 0 && rect.right > 0 && rect.bottom > 0;
      };
      const title = document.querySelector("#page-title");
      const openButton = document.querySelector("#cabinet-open-button");
      const visualViewport = window.visualViewport;
      const titleRange = document.createRange();
      titleRange.selectNodeContents(title);
      const titleRect = titleRange.getBoundingClientRect();
      const openButtonRect = openButton.getBoundingClientRect();
      const insideVisualViewport = (rect) => rect.left >= visualViewport.offsetLeft
        && rect.top >= visualViewport.offsetTop
        && rect.right <= visualViewport.offsetLeft + visualViewport.width
        && rect.bottom <= visualViewport.offsetTop + visualViewport.height;
      return {
        visualScale: visualViewport?.scale ?? 1,
        visualViewport: {
          left: visualViewport.offsetLeft,
          top: visualViewport.offsetTop,
          width: visualViewport.width,
          height: visualViewport.height,
        },
        viewportWidth: document.documentElement.clientWidth,
        documentWidth: document.documentElement.scrollWidth,
        titleVisible: visible(title),
        titleInsideVisualViewport: insideVisualViewport(titleRect),
        openButtonVisible: visible(openButton),
        openButtonInsideVisualViewport: insideVisualViewport(openButtonRect),
        openButtonRight: openButtonRect.right,
      };
    });
    assert.ok(layout.visualScale >= 1.99, "trusted shell did not adopt 200% scale");
    assert.ok(layout.documentWidth <= layout.viewportWidth + 1, "200% scale introduced horizontal document overflow");
    assert.equal(layout.titleVisible, true, "page heading is not visible at 200% scale");
    assert.equal(layout.titleInsideVisualViewport, true, "page heading falls outside the actual 200% visual viewport");
    assert.equal(layout.openButtonVisible, true, "primary Cabinet action is not visible at 200% scale");
    assert.equal(
      layout.openButtonInsideVisualViewport,
      true,
      "primary Cabinet action falls outside the actual 200% visual viewport",
    );
    return layout;
  } finally {
    await session.send("Emulation.setPageScaleFactor", { pageScaleFactor: 1 });
    await session.detach();
  }
}

async function captureState({ name, capsule, expectedSignature, expectedProfile, expectedCompatibility }) {
  const stateRoot = path.join(root, ".tmp", `native-m03-${name}-state`);
  rmSync(stateRoot, { recursive: true, force: true });
  mkdirSync(stateRoot, { recursive: true });
  let parentPort = await freePort();
  let rawPort = await freePort();
  while (rawPort === parentPort) rawPort = await freePort();
  const sourceBefore = capsule ? sha256File(capsule) : null;
  let comparisonCapsule = null;
  let comparisonBefore = null;
  if (name === "signed-v03" || name === "sensitive-v03") {
    comparisonCapsule = path.join(stateRoot, "comparison-mutated.sqlitecapsule");
    copyFileSync(capsule, comparisonCapsule);
    checked("python", [
      "-c",
      "import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); c.execute(\"UPDATE vector_domain SET note='comparison-e2e' WHERE id='domain'\"); c.executemany(\"INSERT INTO vector_domain VALUES (?,?,?,?)\", [(f'page-{i:03}',f'page-value-{i:03}',float(i),bytes([i%256])) for i in range(60)]); c.execute(\"UPDATE capsule_instance SET revision_id='11111111-2222-4333-8444-555555555555', content_updated_at='2026-08-13T05:30:00Z' WHERE id=1\"); c.commit(); c.close()",
      comparisonCapsule,
    ]);
    comparisonBefore = sha256File(comparisonCapsule);
  }
  const child = spawn(application, [], {
    cwd: root,
    env: {
      ...process.env,
      ...(capsule ? { SQLITE_CAPSULE_NATIVE_E2E_PATH: capsule } : {}),
      ...(comparisonCapsule ? { SQLITE_CAPSULE_NATIVE_E2E_COMPARE_PATH: comparisonCapsule } : {}),
      SQLITE_CAPSULE_NATIVE_E2E_STATE_ROOT: stateRoot,
      SQLITE_CAPSULE_NATIVE_PARENT_E2E_PORT: String(parentPort),
      SQLITE_CAPSULE_NATIVE_RAW_E2E_PORT: String(rawPort),
    },
    stdio: ["ignore", "ignore", "pipe"],
    windowsHide: false,
    shell: false,
  });
  let stderr = "";
  child.stderr.on("data", (chunk) => { stderr += chunk.toString(); });
  let parentBrowser;
  let rawBrowser;
  try {
    const [parentEndpoint, rawEndpoint] = await Promise.all([
      waitForEndpoint(parentPort), waitForEndpoint(rawPort),
    ]);
    parentBrowser = await chromium.connectOverCDP(parentEndpoint);
    rawBrowser = await chromium.connectOverCDP(rawEndpoint);
    const parentPage = parentBrowser.contexts()[0]?.pages()[0];
    const rawPage = rawBrowser.contexts()[0]?.pages()[0];
    assert.ok(parentPage && rawPage, "trusted or raw WebView target is absent");
    await parentPage.locator("#host-state").waitFor({ state: "visible" });
    await parentPage.waitForFunction(() => document.querySelector("#host-state")?.textContent !== "Verifying before open");

    if (!capsule) {
      assert.equal(await parentPage.locator("#host-state").textContent(), "No capsule selected");
      assert.equal(await parentPage.locator("#page-title").textContent(), "Cabinet");
      const scaling = await assertTwoHundredPercentScaling(parentPage);
      await parentPage.screenshot({ path: path.join(evidenceRoot, "cabinet-empty.png"), animations: "disabled" });
      return { name, stage: "no-capsule", scaling, screenshot: path.join(evidenceRoot, "cabinet-empty.png") };
    }

    const report = await parentPage.evaluate(() => globalThis.__TAURI__.core.invoke("startup_report"));
    assert.equal(report.capsule.assets_released, false);
    assert.equal(report.capsule.overview.profile, "org.sqlite-capsule.tauri-overview/1");
    assert.equal(report.capsule.overview.compatibility, expectedCompatibility);
    assert.equal(await parentPage.locator("#application-state-badge").textContent(), expectedSignature);
    assert.equal(await parentPage.locator("#profile-badge").textContent(), expectedProfile);
    await rawPage.waitForFunction(() => document.title === "Raw child renderer probe");
    assert.equal(await rawPage.title(), "Raw child renderer probe");
    const rawAuthority = await rawPage.evaluate(() => ({
      tauri: typeof globalThis.__TAURI__,
      internals: typeof globalThis.__TAURI_INTERNALS__,
    }));
    assert.deepEqual(rawAuthority, { tauri: "undefined", internals: "undefined" });
    const serialized = JSON.stringify(report.capsule);
    for (const forbidden of ["entry_asset", "permissions", "icon_asset", "release_notes_doc", "cover_asset_id"]) {
      assert.equal(serialized.includes(forbidden), false, `${name} leaked ${forbidden}`);
    }
    const screenshot = path.join(evidenceRoot, `overview-${name}.png`);
    await parentPage.locator("button[data-page='overview']").click();
    await parentPage.screenshot({ path: screenshot, animations: "disabled" });
    let copyScreenshot = null;
    let exactCopySha256 = null;
    let compareScreenshot = null;
    let compareReportDigest = null;
    if (name === "signed-v03" || name === "sensitive-v03") {
      if (name === "signed-v03") {
      await parentPage.locator("button[data-page='copy']").click();
      await parentPage.locator("input[name='copy-mode'][value='selective-fork']").check();
      await parentPage.waitForFunction(() => {
        const status = document.querySelector("#copy-status")?.textContent || "";
        return status.includes("Profile inspected") || status.includes("blocked");
      });
      assert.equal(await parentPage.locator("#copy-page-title").textContent(), "Create a verified Capsule copy");
      assert.equal(await parentPage.locator("#copy-destination-button").isDisabled(), false);
      const copyBody = await parentPage.locator("[data-page-panel='copy']").textContent();
      assert.match(copyBody, /Sensitive|signed|dataset/i);
      copyScreenshot = path.join(evidenceRoot, "copy-selective-v03.png");
      await parentPage.screenshot({ path: copyScreenshot, animations: "disabled" });
      }

      await parentPage.locator("button[data-page='compare']").click();
      await parentPage.locator("#compare-choose-button").evaluate((button) => button.click());
      try {
        await parentPage.locator("#compare-report").waitFor({ state: "visible", timeout: 60_000 });
      } catch (error) {
        throw new Error(`compare session did not start: ${await parentPage.locator("#compare-action-status").textContent()} / ${await parentPage.locator("#compare-status").textContent()}`, { cause: error });
      }
      assert.match(await parentPage.locator("#compare-compatibility-badge").textContent(), /same release same schema/i);
      assert.match(await parentPage.locator("#compare-datasets").textContent(), /content.*changed/is);
      await parentPage.locator("#compare-application-button").click();
      await parentPage.locator("#compare-application-detail").waitFor({ state: "visible", timeout: 30_000 });
      assert.equal(await parentPage.locator("#compare-application-detail > div").count(), 13);
      assert.match(await parentPage.locator("#compare-application-detail").textContent(), /signature inventory/i);
      await parentPage.locator(".compare-table-list button").first().click();
      if (name === "sensitive-v03") {
        await parentPage.locator("#compare-sensitive-consent").waitFor({ state: "visible" });
        assert.equal(await parentPage.locator("#compare-detail-table-wrap").isHidden(), true);
        await parentPage.locator("#compare-reveal-button").click();
      }
      await parentPage.locator("#compare-detail-table-wrap").waitFor({ state: "visible", timeout: 30_000 });
      assert.match(await parentPage.locator("#compare-detail-rows").textContent(), /comparison-e2e/);
      const firstPageText = await parentPage.locator("#compare-detail-rows").textContent();
      assert.match(firstPageText, /page-value-000/);
      assert.equal(await parentPage.locator("#compare-next-button").isEnabled(), true);
      await parentPage.locator("#compare-next-button").click();
      await parentPage.waitForFunction(() => {
        const rows = document.querySelector("#compare-detail-rows")?.textContent || "";
        return rows.includes("page-value-059");
      }, undefined, { timeout: 30_000 });
      const secondPageText = await parentPage.locator("#compare-detail-rows").textContent();
      assert.match(secondPageText, /page-value-049/);
      assert.doesNotMatch(secondPageText, /page-value-000/);
      assert.equal(await parentPage.locator("#compare-next-button").isDisabled(), true);
      if (name === "sensitive-v03") {
        assert.match(await parentPage.locator("#compare-detail-badge").textContent(), /Sensitive.*revealed/i);
      }
      compareReportDigest = await parentPage.locator("#compare-pair-details").textContent();
      compareScreenshot = path.join(evidenceRoot, name === "sensitive-v03" ? "compare-sensitive-v03.png" : "compare-field-v03.png");
      await parentPage.screenshot({ path: compareScreenshot, animations: "disabled", fullPage: true });
      assert.equal(sha256File(comparisonCapsule), comparisonBefore, "trusted-shell comparison changed its second source");
    }
    assert.equal(sha256File(capsule), sourceBefore, `${name} Overview changed its source capsule`);
    return {
      name,
      stage: report.stage,
      signature: expectedSignature,
      profile: expectedProfile,
      sourceSha256: sourceBefore,
      screenshot,
      copyScreenshot,
      exactCopySha256,
      compareScreenshot,
      compareReportDigest,
    };
  } catch (error) {
    if (stderr.trim()) process.stderr.write(stderr);
    throw error;
  } finally {
    await parentBrowser?.close().catch(() => {});
    await rawBrowser?.close().catch(() => {});
    await stopApplication(child);
    rmSync(stateRoot, { recursive: true, force: true });
  }
}

async function openHost(capsule, stateRoot) {
  let parentPort = await freePort();
  let rawPort = await freePort();
  while (rawPort === parentPort) rawPort = await freePort();
  const child = spawn(application, [], {
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
  let stderr = "";
  child.stderr.on("data", (chunk) => { stderr += chunk.toString(); });
  try {
    const [parentEndpoint, rawEndpoint] = await Promise.all([
      waitForEndpoint(parentPort), waitForEndpoint(rawPort),
    ]);
    const parentBrowser = await chromium.connectOverCDP(parentEndpoint);
    const rawBrowser = await chromium.connectOverCDP(rawEndpoint);
    const parentPage = parentBrowser.contexts()[0]?.pages()[0];
    const rawPage = rawBrowser.contexts()[0]?.pages()[0];
    assert.ok(parentPage && rawPage, "trusted or raw WebView target is absent");
    await parentPage.locator("#host-state").waitFor({ state: "visible" });
    await parentPage.waitForFunction(() => document.querySelector("#host-state")?.textContent !== "Verifying before open");
    await rawPage.waitForFunction(() => document.title === "Raw child renderer probe");
    return { child, parentBrowser, rawBrowser, parentPage, rawPage, stderr: () => stderr };
  } catch (error) {
    if (stderr.trim()) process.stderr.write(stderr);
    await stopApplication(child);
    throw error;
  }
}

async function closeHost(host) {
  await host.rawBrowser.close().catch(() => {});
  await host.parentBrowser.close().catch(() => {});
  await stopApplication(host.child);
}

async function captureRememberedRelease(capsule) {
  const stateRoot = path.join(root, ".tmp", "native-m03-remembered-state");
  rmSync(stateRoot, { recursive: true, force: true });
  mkdirSync(stateRoot, { recursive: true });
  const sourceBefore = sha256File(capsule);

  let host = await openHost(capsule, stateRoot);
  try {
    await host.parentPage.locator("button[data-page='security']").click();
    await host.parentPage.locator("button[data-route='capabilities']").click();
    await host.parentPage.locator("#always-button").click();
    await host.rawPage.waitForURL(/\/app\/index\.html$/, { timeout: 20_000 });
    const authorized = await host.parentPage.evaluate(() => globalThis.__TAURI__.core.invoke("startup_report"));
    assert.equal(authorized.capsule.assets_released, true);
  } finally {
    await closeHost(host);
  }

  host = await openHost(capsule, stateRoot);
  try {
    const remembered = await host.parentPage.evaluate(() => globalThis.__TAURI__.core.invoke("startup_report"));
    assert.equal(remembered.stage, "remembered-ready");
    assert.equal(remembered.capsule.assets_released, false);
    assert.equal(await host.rawPage.title(), "Raw child renderer probe");
    assert.match(host.rawPage.url(), /\/__host\/locked$/);
    const cabinet = await host.parentPage.evaluate(() => globalThis.__TAURI__.core.invoke("cabinet_status"));
    assert.ok(cabinet.entries.length >= 1, "remembered release was not recorded in Cabinet recents");
    assert.match(cabinet.entries[0].recent_id, /^[0-9a-f]{32}$/);
    const cabinetJson = JSON.stringify(cabinet);
    assert.equal(cabinetJson.includes("path_hint"), false);
    assert.equal(cabinetJson.includes("canonical_path"), false);
    const screenshot = path.join(evidenceRoot, "overview-remembered-ready.png");
    await host.parentPage.locator("button[data-page='overview']").click();
    await host.parentPage.screenshot({ path: screenshot, animations: "disabled" });

    await host.parentPage.locator("#review-capabilities-button").click();
    await host.rawPage.waitForURL(/\/app\/index\.html$/, { timeout: 20_000 });
    const opened = await host.parentPage.evaluate(() => globalThis.__TAURI__.core.invoke("startup_report"));
    assert.equal(opened.capsule.assets_released, true);
    assert.equal(sha256File(capsule), sourceBefore, "remembered release flow changed its source capsule");
    return {
      name: "remembered-ready",
      stage: remembered.stage,
      assetsBeforeExplicitOpen: remembered.capsule.assets_released,
      rawLockedBeforeExplicitOpen: true,
      assetsAfterExplicitOpen: opened.capsule.assets_released,
      sourceSha256: sourceBefore,
      screenshot,
    };
  } finally {
    if (host.stderr().trim()) process.stderr.write(host.stderr());
    await closeHost(host);
    rmSync(stateRoot, { recursive: true, force: true });
  }
}

rmSync(evidenceRoot, { recursive: true, force: true });
rmSync(fixtureRoot, { recursive: true, force: true });
mkdirSync(evidenceRoot, { recursive: true });
mkdirSync(fixtureRoot, { recursive: true });

const signed = path.join(fixtureRoot, "signed-v03.sqlitecapsule");
const invalid = path.join(fixtureRoot, "invalid-v03.sqlitecapsule");
const unsigned = path.join(fixtureRoot, "unsigned-v03.sqlitecapsule");
const sensitiveInput = path.join(fixtureRoot, "sensitive-input-v03.sqlitecapsule");
const sensitive = path.join(fixtureRoot, "sensitive-v03.sqlitecapsule");
prepareV03Fixture(signed, "signed");
prepareV03Fixture(invalid, "invalid");
prepareV03Fixture(unsigned, "unsigned");
prepareSensitiveV03Fixture(signed, sensitiveInput, sensitive);

const results = [];
results.push(await captureState({ name: "empty", capsule: null }));
results.push(await captureState({
  name: "legacy-v02",
  capsule: path.join(root, "capsules", "diagram-studio.capsule.sqlite"),
  expectedSignature: "Unsigned",
  expectedProfile: "Legacy v0.2",
  expectedCompatibility: "legacy-v02",
}));
results.push(await captureState({
  name: "signed-v03", capsule: signed,
  expectedSignature: "Signature valid", expectedProfile: "Lifecycle v0.3", expectedCompatibility: "lifecycle-v03",
}));
results.push(await captureState({
  name: "sensitive-v03", capsule: sensitive,
  expectedSignature: "Signature valid", expectedProfile: "Lifecycle v0.3", expectedCompatibility: "lifecycle-v03",
}));
results.push(await captureState({
  name: "unsigned-v03", capsule: unsigned,
  expectedSignature: "Unsigned", expectedProfile: "Lifecycle v0.3", expectedCompatibility: "lifecycle-v03",
}));
results.push(await captureState({
  name: "invalid-v03", capsule: invalid,
  expectedSignature: "Invalid signature", expectedProfile: "Lifecycle v0.3", expectedCompatibility: "lifecycle-v03",
}));
results.push(await captureRememberedRelease(signed));

process.stdout.write(`${JSON.stringify({ ok: true, results }, null, 2)}\n`);

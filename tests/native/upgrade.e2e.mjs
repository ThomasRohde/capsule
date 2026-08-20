import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import net from "node:net";
import path from "node:path";
import { spawn, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../..");
const application = process.env.SQLITE_CAPSULE_NATIVE_APPLICATION
  || path.join(root, "native", "target", "debug", "sqlite-capsule-desktop.exe");
const nativeCli = process.env.SQLITE_CAPSULE_NATIVE_CLI
  || path.join(root, "native", "target", "debug", "capsule-native.exe");
const stateRoot = path.join(root, ".tmp", "native-m07-upgrade-state");
const fixtureRoot = path.join(stateRoot, "fixtures");
const output = path.join(stateRoot, "upgraded-output.sqlitecapsule");
const evidencePath = path.join(root, ".tmp", "native-m07-upgrade-evidence.json");

if (process.platform !== "win32") throw new Error("Trusted application-upgrade acceptance is Windows-only");
if (!existsSync(application)) throw new Error(`native debug application is absent: ${application}`);
if (!existsSync(nativeCli)) throw new Error(`native debug CLI is absent: ${nativeCli}`);

function checked(command, args) {
  const result = spawnSync(command, args, {
    cwd: root,
    encoding: "utf8",
    windowsHide: true,
    shell: false,
    maxBuffer: 16 * 1024 * 1024,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(result.stderr.trim() || result.stdout.trim() || `${command} exited ${result.status}`);
  return result.stdout.trim();
}

function sha256File(filePath) {
  return createHash("sha256").update(readFileSync(filePath)).digest("hex");
}

function capsuleState(capsule) {
  const script = [
    "import json,sqlite3,sys",
    "c=sqlite3.connect(sys.argv[1])",
    "instance=c.execute('SELECT capsule_id,revision_id,title,description,document_kind,tags_json,icon_asset_id FROM capsule_instance').fetchone()",
    "manifest=c.execute('SELECT app_id,app_version,data_schema_id,data_schema_version FROM capsule_manifest').fetchone()",
    "items=list(c.execute('SELECT id,title,completed FROM item ORDER BY id'))",
    "app_js=c.execute(\"SELECT CAST(content AS TEXT) FROM capsule_asset WHERE path='app/app.js'\").fetchone()[0]",
    "signatures=[(r[0],r[1],r[2].hex(),r[3].hex(),r[4].hex(),r[5]) for r in c.execute('SELECT key_id,algorithm,public_key,application_digest,signature,signed_at FROM capsule_signature ORDER BY key_id')]",
    "event=c.execute('SELECT event_id,operation,application_digest,plan_digest,details_json FROM capsule_lineage_event ORDER BY sequence').fetchone()",
    "parents=[] if event is None else list(c.execute('SELECT ordinal,relation,parent_capsule_id,parent_revision_id,parent_file_sha256 FROM capsule_lineage_parent WHERE event_id=? ORDER BY ordinal',(event[0],)))",
    "assets=list(c.execute('SELECT id,sha256,width,height,description FROM capsule_instance_asset ORDER BY id'))",
    "grants=c.execute('SELECT count(*) FROM capsule_grant').fetchone()[0]",
    "print(json.dumps({'instance':instance,'manifest':manifest,'items':items,'target_asset':('TARGET-RELEASE-ASSET' in app_js),'source_asset':('SOURCE-RELEASE-ASSET' in app_js),'signatures':signatures,'event':None if event is None else [event[0],event[1],event[2],event[3],json.loads(event[4])],'parents':parents,'instance_assets':assets,'grants':grants}))",
  ].join("\n");
  return JSON.parse(checked("python", ["-c", script, capsule]));
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
  const deadline = Date.now() + 30_000;
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
      stdio: "ignore",
      windowsHide: true,
      shell: false,
    });
  }
}

async function openHost(working, release) {
  let parentPort = await freePort();
  let rawPort = await freePort();
  while (rawPort === parentPort) rawPort = await freePort();
  const child = spawn(application, [], {
    cwd: root,
    env: {
      ...process.env,
      SQLITE_CAPSULE_NATIVE_E2E_PATH: working,
      SQLITE_CAPSULE_NATIVE_E2E_UPGRADE_RELEASE_PATH: release,
      SQLITE_CAPSULE_NATIVE_E2E_UPGRADE_OUTPUT_PATH: output,
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
  let browser;
  try {
    browser = await chromium.connectOverCDP(await waitForEndpoint(parentPort));
  } catch (error) {
    const exitState = child.exitCode === null ? "host still running" : `host exited ${child.exitCode}`;
    await stopApplication(child);
    throw new Error(`${error.message} (${exitState})${stderr.trim() ? `\n${stderr.trim()}` : ""}`);
  }
  const page = browser.contexts()[0]?.pages()[0];
  assert.ok(page, "trusted WebView target is absent");
  await page.locator("#host-state").waitFor({ state: "visible" });
  await page.waitForFunction(() => document.querySelector("#host-state")?.textContent !== "Verifying before open");
  return { browser, child, page, stderr: () => stderr };
}

rmSync(stateRoot, { recursive: true, force: true });
mkdirSync(stateRoot, { recursive: true });
const fixtureReport = JSON.parse(checked("python", [
  "tests/native/make_upgrade_fixtures.py",
  "--native-cli",
  nativeCli,
  "--output",
  fixtureRoot,
]));
assert.equal(fixtureReport.ok, true);
assert.equal(fixtureReport.same_publisher_key, true);
const working = fixtureReport.working;
const release = fixtureReport.release;
const workingBefore = sha256File(working);
const releaseBefore = sha256File(release);
const workingState = capsuleState(working);
const releaseState = capsuleState(release);
const reviewScreenshot = path.join(stateRoot, "upgrade-review.png");
const scaledScreenshot = path.join(stateRoot, "upgrade-review-200-percent.png");
const resultScreenshot = path.join(stateRoot, "upgrade-result.png");
const host = await openHost(working, release);
let reviewEvidence;
try {
  await host.page.locator("button[data-page='versions']").click();
  await host.page.locator("#upgrade-release-button").click();
  await host.page.waitForFunction(() => /release screened/i.test(document.querySelector("#upgrade-badge")?.textContent || ""));
  assert.match(await host.page.locator("#upgrade-candidate-details").textContent(), /1\.0\.0 → 1\.1\.0/);
  assert.doesNotMatch(
    await host.page.locator("[data-page-panel='versions']").textContent(),
    new RegExp(stateRoot.replaceAll("\\", "\\\\"), "i"),
  );
  await host.page.locator("#upgrade-destination-button").click();
  await host.page.waitForFunction(() => /selected local folder/i.test(document.querySelector("#upgrade-destination-status")?.textContent || ""));
  await host.page.locator("#upgrade-prepare-button").click();
  await host.page.locator("#upgrade-review").waitFor({ state: "visible", timeout: 60_000 });
  assert.equal(await host.page.locator("#upgrade-capability-confirmation-wrap").isHidden(), true);
  assert.match(await host.page.locator("#upgrade-datasets").textContent(), /items.*copy/i);
  assert.match(await host.page.locator("#upgrade-review-details").textContent(), /same-accepted-key/i);
  reviewEvidence = await host.page.evaluate(() => ({
    heading: document.querySelector("#upgrade-review-title")?.textContent,
    candidate: document.querySelector("#upgrade-candidate-details")?.textContent,
    review: document.querySelector("#upgrade-review-details")?.textContent,
    datasets: document.querySelector("#upgrade-datasets")?.textContent,
    capabilities: document.querySelector("#upgrade-capabilities")?.textContent,
    noHorizontalOverflow: document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1,
  }));
  assert.equal(reviewEvidence.heading, "Same-schema upgrade review");
  assert.equal(reviewEvidence.noHorizontalOverflow, true);
  await host.page.screenshot({ path: reviewScreenshot, animations: "disabled", fullPage: true });
  const session = await host.page.context().newCDPSession(host.page);
  await session.send("Emulation.setPageScaleFactor", { pageScaleFactor: 2 });
  try {
    await host.page.screenshot({ path: scaledScreenshot, animations: "disabled", fullPage: true });
    assert.equal(
      await host.page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1),
      true,
      "upgrade review overflows horizontally at 200 percent scale",
    );
  } finally {
    await session.send("Emulation.setPageScaleFactor", { pageScaleFactor: 1 });
    await session.detach();
  }
  await host.page.locator("#upgrade-publisher-confirmation").check();
  assert.equal(await host.page.locator("#upgrade-execute-button").isEnabled(), true);
  await host.page.locator("#upgrade-execute-button").click();
  await host.page.locator("#upgrade-result").waitFor({ state: "visible", timeout: 90_000 });
  assert.match(await host.page.locator("#upgrade-result-output").textContent(), /Both retained inputs unchanged/i);
  assert.match(await host.page.locator("#upgrade-status").textContent(), /reopened and exhaustively verified/i);
  await host.page.screenshot({ path: resultScreenshot, animations: "disabled", fullPage: true });
} catch (error) {
  const stderr = host.stderr().trim();
  if (stderr) process.stderr.write(`${stderr}\n`);
  throw error;
} finally {
  await host.browser.close().catch(() => {});
  await stopApplication(host.child);
}

assert.equal(existsSync(output), true, "verified upgrade output is absent");
assert.equal(sha256File(working), workingBefore, "application upgrade changed the working Capsule");
assert.equal(sha256File(release), releaseBefore, "application upgrade changed the target release");
checked(nativeCli, ["verify", output]);
const outputState = capsuleState(output);
assert.deepEqual(outputState.manifest, releaseState.manifest, "output did not retain target manifest identity");
assert.equal(outputState.target_asset, true, "output did not retain the target application asset");
assert.equal(outputState.source_asset, false, "output retained the old application asset");
assert.deepEqual(outputState.signatures, releaseState.signatures, "output changed the target signature inventory");
assert.deepEqual(outputState.items, workingState.items, "output did not preserve working user data");
assert.equal(outputState.items.some((row) => row[1].includes("TARGET CLEAN PRESET")), false);
assert.equal(outputState.instance[0], workingState.instance[0], "output changed capsule identity");
assert.notEqual(outputState.instance[1], workingState.instance[1], "output reused the working revision");
assert.deepEqual(outputState.instance.slice(2), workingState.instance.slice(2), "output did not preserve the instance profile");
assert.deepEqual(outputState.instance_assets, workingState.instance_assets, "output did not preserve referenced instance assets");
assert.equal(outputState.grants, 0, "output retained mutable grants");
assert.equal(outputState.event[1], "application-upgrade");
assert.deepEqual(
  outputState.parents,
  [
    [1, "upgraded-from", workingState.instance[0], workingState.instance[1], workingBefore],
    [2, "application-release", releaseState.instance[0], releaseState.instance[1], releaseBefore],
  ],
  "upgrade lineage did not bind both exact input files",
);
const report = {
  profile: "org.sqlite-capsule.native-upgrade-e2e/1",
  working_sha256: workingBefore,
  release_sha256: releaseBefore,
  output_sha256: sha256File(output),
  target_application_digest: fixtureReport.target_application_digest,
  publisher_key_id: fixtureReport.publisher_key_id,
  review: reviewEvidence,
  review_screenshot: reviewScreenshot,
  scaled_review_screenshot: scaledScreenshot,
  result_screenshot: resultScreenshot,
  output_state: outputState,
};
writeFileSync(evidencePath, `${JSON.stringify(report, null, 2)}\n`, { flag: "w" });
process.stdout.write(`${JSON.stringify({ ...report, evidence_path: evidencePath }, null, 2)}\n`);

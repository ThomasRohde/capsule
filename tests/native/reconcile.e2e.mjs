import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import net from "node:net";
import path from "node:path";
import { spawn, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../..");
const application = process.env.SQLITE_CAPSULE_NATIVE_APPLICATION
  || path.join(root, "native", "target", "debug", "sqlite-capsule-desktop.exe");
const workspaceCli = process.env.SQLITE_CAPSULE_WORKSPACE_CLI
  || path.join(root, "native", "target", "debug", "capsule-workspace.exe");
const fixtureRoot = path.join(root, ".tmp", "native-m06-reconcile-fixtures");
const evidencePath = path.join(root, ".tmp", "native-m06-reconcile-evidence.json");

if (process.platform !== "win32") throw new Error("Trusted reconciliation acceptance is Windows-only");
if (!existsSync(application)) throw new Error(`native debug application is absent: ${application}`);

function checked(command, args) {
  const result = spawnSync(command, args, {
    cwd: root,
    encoding: "utf8",
    windowsHide: true,
    shell: false,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(result.stderr.trim() || result.stdout.trim() || `${command} exited ${result.status}`);
  return result.stdout.trim();
}

function sha256File(filePath) {
  return createHash("sha256").update(readFileSync(filePath)).digest("hex");
}

function prepareFixture(output, role) {
  const script = [
    "import pathlib, sqlite3, sys",
    "root, output, role = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]), sys.argv[3]",
    "c = sqlite3.connect(output)",
    "for relative in ('format/capsule-v0.3.sql', 'format/capsule-signed-app-v0.3.sql', 'compatibility/signed-app-v0.3/fixture-v0.3.sql'):",
    "    c.executescript((root / relative).read_text(encoding='utf-8'))",
    "identities = {",
    " 'ancestor': ('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa','aaaaaaaa-aaaa-4aaa-9aaa-aaaaaaaaaaaa'),",
    " 'source': ('bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb','bbbbbbbb-bbbb-4bbb-9bbb-bbbbbbbbbbbb'),",
    " 'source-three-way': ('bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb','bbbbbbbb-bbbb-4bbb-abbb-bbbbbbbbbbbb'),",
    " 'target': ('cccccccc-cccc-4ccc-8ccc-cccccccccccc','cccccccc-cccc-4ccc-9ccc-cccccccccccc'),",
    "}",
    "c.execute(\"UPDATE capsule_instance SET capsule_id=?, revision_id=?, content_updated_at='2026-08-13T08:00:00Z' WHERE id=1\", identities[role])",
    "if role in ('source', 'source-three-way'):",
    "    c.execute(\"UPDATE vector_domain SET note='source-conflict' WHERE id='domain'\")",
    "    c.execute(\"INSERT INTO vector_domain VALUES ('clean-source','clean-source',7.0,X'07')\")",
    "if role == 'source':",
    "    c.execute(\"INSERT INTO vector_settings VALUES ('source-only','from-source')\")",
    "if role == 'target':",
    "    c.execute(\"UPDATE vector_domain SET note='target-conflict' WHERE id='domain'\")",
    "c.commit(); c.close()",
  ].join("\n");
  checked("python", ["-c", script, root, output, role]);
  checked("python", ["tools/capsule.py", "verify", output]);
}

function capsuleState(capsule) {
  const script = [
    "import hashlib, json, sqlite3, sys",
    "c=sqlite3.connect(sys.argv[1])",
    "def rows(table):",
    " cols=[row[1] for row in c.execute(f'PRAGMA table_info({table})')]",
    " order=','.join(str(i) for i in range(1,len(cols)+1))",
    " values=[]",
    " for row in c.execute(f'SELECT * FROM {table} ORDER BY {order}'):",
    "  values.append({name:({'blob_hex':value.hex()} if isinstance(value,bytes) else value) for name,value in zip(cols,row)})",
    " return values",
    "def family(tables):",
    " value={table:rows(table) for table in tables}",
    " canonical=json.dumps(value,sort_keys=True,separators=(',',':')).encode()",
    " return {'rows':value,'sha256':hashlib.sha256(canonical).hexdigest()}",
    "instance=c.execute('SELECT capsule_id,revision_id FROM capsule_instance').fetchone()",
    "app=c.execute('SELECT lower(hex(application_digest)) FROM capsule_signature ORDER BY key_id LIMIT 1').fetchone()[0]",
    "settings=dict(c.execute('SELECT key,value FROM vector_settings'))",
    "content=dict(c.execute('SELECT id,note FROM vector_domain'))",
    "event=c.execute(\"SELECT event_id,operation,details_json FROM capsule_lineage_event ORDER BY sequence DESC LIMIT 1\").fetchone()",
    "parents=[] if event is None else list(c.execute('SELECT relation,parent_capsule_id,parent_revision_id,parent_file_sha256 FROM capsule_lineage_parent WHERE event_id=? ORDER BY ordinal',(event[0],)))",
    "lineage=None if event is None else {'operation':event[1],'details':json.loads(event[2]),'parents':parents}",
    "signed_application=family(('capsule_manifest','capsule_application'))",
    "data_contract=family(('capsule_dataset','capsule_dataset_table','capsule_dataset_dependency'))",
    "signatures=rows('capsule_signature')",
    "print(json.dumps({'capsule_id':instance[0],'revision_id':instance[1],'application_digest':app,'signature_inventory':signatures,'signed_application':signed_application,'data_contract':data_contract,'settings':settings,'content':content,'lineage':lineage},sort_keys=True))",
  ].join("\n");
  return JSON.parse(checked("python", ["-c", script, capsule]));
}

function prepareExpectedOutput(expected, target, mode) {
  copyFileSync(target, expected);
  const script = [
    "import sqlite3, sys",
    "capsule, mode = sys.argv[1], sys.argv[2]",
    "c=sqlite3.connect(capsule)",
    "if mode == 'two-way':",
    " c.execute(\"INSERT INTO vector_settings VALUES ('source-only','from-source')\")",
    "else:",
    " c.execute(\"UPDATE vector_domain SET note='source-conflict' WHERE id='domain'\")",
    " c.execute(\"INSERT INTO vector_domain VALUES ('clean-source','clean-source',7.0,X'07')\")",
    "c.commit(); c.close()",
  ].join("\n");
  checked("python", ["-c", script, expected, mode]);
  checked("python", ["tools/capsule.py", "verify", expected]);
}

function compareWithExpected(expected, actual) {
  if (!existsSync(workspaceCli)) throw new Error(`M05 workspace compare CLI is absent: ${workspaceCli}`);
  const report = JSON.parse(checked(workspaceCli, ["compare", expected, actual]));
  assert.equal(report.compatibility.state, "same-release-same-schema");
  assert.equal(report.application.state, "same");
  assert.equal(report.application.left_digest, report.application.right_digest);
  assert.equal(report.schema.state, "same");
  assert.equal(report.schema.left_digest, report.schema.right_digest);
  for (const dataset of report.datasets) {
    assert.equal(dataset.state, "same", `actual output differs from independent expected dataset ${dataset.dataset_id}`);
    assert.equal(dataset.counts.added, 0);
    assert.equal(dataset.counts.removed, 0);
    assert.equal(dataset.counts.changed, 0);
    assert.equal(dataset.left_digest, dataset.right_digest);
  }
  assert.equal(report.truncated, false);
  return report;
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

async function openHost({ source, target, ancestor, output, stateRoot }) {
  let parentPort = await freePort();
  let rawPort = await freePort();
  while (rawPort === parentPort) rawPort = await freePort();
  const child = spawn(application, [], {
    cwd: root,
    env: {
      ...process.env,
      SQLITE_CAPSULE_NATIVE_E2E_PATH: source,
      SQLITE_CAPSULE_NATIVE_E2E_COMPARE_PATH: target,
      SQLITE_CAPSULE_NATIVE_E2E_RECONCILE_PATH: output,
      ...(ancestor ? { SQLITE_CAPSULE_NATIVE_E2E_RECONCILE_ANCESTOR_PATH: ancestor } : {}),
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
  const browser = await chromium.connectOverCDP(await waitForEndpoint(parentPort));
  const page = browser.contexts()[0]?.pages()[0];
  assert.ok(page, "trusted WebView target is absent");
  await page.locator("#host-state").waitFor({ state: "visible" });
  await page.waitForFunction(() => document.querySelector("#host-state")?.textContent !== "Verifying before open");
  return { browser, child, page, stderr: () => stderr };
}

async function startReconcile(page, mode) {
  await page.locator("button[data-page='compare']").click();
  await page.locator("#compare-choose-button").evaluate((button) => button.click());
  await page.locator("#compare-report").waitFor({ state: "visible", timeout: 60_000 });
  assert.match(await page.locator("#compare-compatibility-badge").textContent(), /same release same schema/i);
  if (mode === "three-way") {
    const contentRow = page.locator(".compare-table-list > div", { hasText: "vector_domain" });
    await contentRow.locator("button").click();
    await page.waitForFunction(() => /source-conflict|clean-source/.test(document.querySelector("#compare-detail-rows")?.textContent || ""));
  } else {
    const settingsRow = page.locator(".compare-table-list > div", { hasText: "vector_settings" });
    await settingsRow.locator("button").click();
    await page.locator("#compare-detail-table-wrap").waitFor({ state: "visible", timeout: 30_000 });
    await page.waitForFunction(() => /added|removed|changed/i.test(document.querySelector("#compare-detail-rows")?.textContent || ""));
    assert.doesNotMatch(
      await page.locator("#compare-detail-rows").textContent(),
      /source-only/,
      "signed row comparison policy leaked the raw settings key",
    );
  }
  await page.locator("#reconcile-open-button").click();
  await page.locator("#reconcile-options").waitFor({ state: "visible" });
  await page.locator("#reconcile-start-button").click();
  await page.locator("#reconcile-session").waitFor({ state: "visible", timeout: 30_000 });
  const selections = page.locator("input[name='reconcile-selection']");
  if (mode === "three-way") {
    assert.equal(await selections.count(), 0, "pure three-way session unexpectedly exposed manual selections");
  } else {
    await selections.first().check();
  }
  return selections.count();
}

async function chooseDestinationWithExistingRace(page, output) {
  writeFileSync(output, "occupied", { flag: "wx" });
  await page.locator("#reconcile-destination-button").click();
  await page.waitForFunction(() => document.querySelector("#reconcile-destination-status")?.classList.contains("error"));
  assert.equal(readFileSync(output, "utf8"), "occupied", "existing destination was replaced");
  rmSync(output);
  await page.locator("#reconcile-destination-button").waitFor({ state: "visible" });
  await page.waitForFunction(() => !document.querySelector("#reconcile-destination-button")?.disabled);
  await page.locator("#reconcile-destination-button").click();
  await page.waitForFunction(() => /selected local folder/i.test(document.querySelector("#reconcile-destination-status")?.textContent || ""));
}

async function prepareAndExecute(page) {
  await page.locator("#reconcile-prepare-button").click();
  await page.locator("#reconcile-review").waitFor({ state: "visible", timeout: 30_000 });
  const preparedDigests = await page.locator("#reconcile-review-identity").evaluate((list) =>
    Object.fromEntries([...list.querySelectorAll("dt")].map((term) => [
      term.textContent,
      term.nextElementSibling?.textContent,
    ])),
  );
  assert.match(preparedDigests["Review digest"], /^[0-9a-f]{64}$/);
  assert.match(preparedDigests["Value-free payload digest"], /^[0-9a-f]{64}$/);
  await page.locator("#reconcile-confirmation").check();
  await page.locator("#reconcile-execute-button").click();
  await page.locator("#reconcile-result").waitFor({ state: "visible", timeout: 60_000 });
  assert.match(await page.locator("#reconcile-result-output").textContent(), /Verified new Capsule/i);
  assert.match(await page.locator("#reconcile-status").textContent(), /reopened, exhaustively validated and rebound/i);
  await page.waitForTimeout(500);
  assert.match(await page.locator("#reconcile-result-output").textContent(), /Verified new Capsule/i);
  assert.match(await page.locator("#reconcile-status").textContent(), /reopened, exhaustively validated and rebound/i);
  assert.equal(await page.locator("#reconcile-status").evaluate((node) => node.classList.contains("error")), false);
  return {
    review_digest: preparedDigests["Review digest"],
    payload_digest: preparedDigests["Value-free payload digest"],
  };
}

async function captureScaledThreeWay(page, screenshot) {
  const session = await page.context().newCDPSession(page);
  try {
    await session.send("Emulation.setPageScaleFactor", { pageScaleFactor: 2 });
    await page.waitForFunction(() => (window.visualViewport?.scale ?? 1) >= 1.99);
    await page.locator("#reconcile-three-way").scrollIntoViewIfNeeded();
    const layout = await page.evaluate(() => {
      const viewport = window.visualViewport;
      const panel = document.querySelector("#reconcile-three-way").getBoundingClientRect();
      return {
        visualScale: viewport?.scale ?? 1,
        documentWidth: document.documentElement.scrollWidth,
        viewportWidth: document.documentElement.clientWidth,
        conflictPanelVisible: panel.width > 0 && panel.height > 0 && panel.bottom > 0,
      };
    });
    assert.ok(layout.visualScale >= 1.99, "three-way review did not adopt 200% scale");
    assert.ok(layout.documentWidth <= layout.viewportWidth + 1, "200% three-way review overflowed horizontally");
    assert.equal(layout.conflictPanelVisible, true, "three-way controls disappeared at 200% scale");
    await page.screenshot({ path: screenshot, animations: "disabled", fullPage: true });
    return layout;
  } finally {
    await session.send("Emulation.setPageScaleFactor", { pageScaleFactor: 1 });
    await session.detach();
  }
}

async function runScenario({ mode, source, target, ancestor }) {
  const stateRoot = path.join(root, ".tmp", `native-m06-reconcile-${mode}-state`);
  const output = path.join(stateRoot, `${mode}-output.sqlitecapsule`);
  rmSync(stateRoot, { recursive: true, force: true });
  mkdirSync(stateRoot, { recursive: true });
  const targetInput = path.join(stateRoot, "target-input.sqlitecapsule");
  copyFileSync(target, targetInput);
  const ancestorInput = ancestor ? path.join(stateRoot, "ancestor-input.sqlitecapsule") : null;
  if (ancestorInput) copyFileSync(ancestor, ancestorInput);
  const expectedOutput = path.join(stateRoot, `${mode}-independent-expected.sqlitecapsule`);
  prepareExpectedOutput(expectedOutput, targetInput, mode);
  const sourceBefore = sha256File(source);
  const targetBefore = sha256File(targetInput);
  const ancestorBefore = ancestorInput ? sha256File(ancestorInput) : null;
  const host = await openHost({ source, target: targetInput, ancestor: ancestorInput, output, stateRoot });
  let threeWayScreenshot = null;
  let threeWayScaledScreenshot = null;
  let scaledLayout = null;
  let accessibility = null;
  let manualSelectionCount = null;
  let preparedDigests = null;
  try {
    manualSelectionCount = await startReconcile(host.page, mode);
    await chooseDestinationWithExistingRace(host.page, output);
    if (mode === "three-way") {
      await host.page.locator("#reconcile-ancestor-button").click();
      await host.page.locator("#reconcile-three-way").waitFor({ state: "visible", timeout: 30_000 });
      assert.match(await host.page.locator("#reconcile-conflicts").textContent(), /clean three-way change/i);
      const conflicts = host.page.locator("#reconcile-three-way fieldset");
      assert.ok(await conflicts.count() >= 1, "three-way fixture produced no conflict");
      for (let index = 0; index < await conflicts.count(); index += 1) {
        const fieldset = conflicts.nth(index);
        const takeSource = fieldset.locator("label", { hasText: "Take source" });
        const choice = await takeSource.count() ? takeSource : fieldset.locator("label").first();
        await choice.locator("input").check();
      }
      accessibility = await host.page.evaluate(() => ({
        title: document.querySelector("#reconcile-title")?.textContent,
        conflictHeading: document.querySelector("#reconcile-conflicts-title")?.textContent,
        conflictLegends: [...document.querySelectorAll("#reconcile-three-way fieldset legend")]
          .map((legend) => legend.textContent),
        choiceLabels: [...document.querySelectorAll("#reconcile-three-way fieldset label")]
          .map((label) => label.textContent),
        noHorizontalOverflow: document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1,
      }));
      assert.equal(accessibility.title, "Apply selected changes to a new copy");
      assert.equal(accessibility.conflictHeading, "Conflicts and resolutions");
      assert.ok(accessibility.conflictLegends.every(Boolean), "a conflict control has no legend");
      assert.ok(accessibility.choiceLabels.every(Boolean), "a resolution radio has no accessible label");
      assert.equal(accessibility.noHorizontalOverflow, true, "three-way review introduced page overflow");
      threeWayScreenshot = path.join(stateRoot, "three-way-conflicts.png");
      await host.page.screenshot({ path: threeWayScreenshot, animations: "disabled", fullPage: true });
      threeWayScaledScreenshot = path.join(stateRoot, "three-way-conflicts-200-percent.png");
      scaledLayout = await captureScaledThreeWay(host.page, threeWayScaledScreenshot);
    }
    preparedDigests = await prepareAndExecute(host.page);
    await host.page.screenshot({
      path: path.join(stateRoot, `${mode}-verified-result.png`),
      animations: "disabled",
      fullPage: true,
    });
    assert.equal(existsSync(output), true, "verified reconciliation output is absent");
    checked("python", ["tools/capsule.py", "verify", output]);
  } catch (error) {
    const stderr = host.stderr().trim();
    if (stderr) process.stderr.write(`${stderr}\n`);
    throw error;
  } finally {
    await host.browser.close().catch(() => {});
    await stopApplication(host.child);
  }
  assert.equal(sha256File(source), sourceBefore, `${mode} reconciliation changed source input`);
  assert.equal(sha256File(targetInput), targetBefore, `${mode} reconciliation changed target input`);
  if (ancestorInput) assert.equal(sha256File(ancestorInput), ancestorBefore, "three-way reconciliation changed ancestor input");
  const sourceState = capsuleState(source);
  const targetState = capsuleState(targetInput);
  const ancestorState = ancestorInput ? capsuleState(ancestorInput) : null;
  const outputState = capsuleState(output);
  const expectedState = capsuleState(expectedOutput);
  assert.equal(outputState.capsule_id, targetState.capsule_id, "output did not preserve target capsule identity");
  assert.notEqual(outputState.revision_id, targetState.revision_id, "output did not mint a new revision");
  assert.equal(outputState.application_digest, targetState.application_digest, "output changed target application digest");
  assert.deepEqual(outputState.signature_inventory, targetState.signature_inventory, "output changed the exact signature inventory");
  assert.deepEqual(outputState.signed_application.rows, targetState.signed_application.rows, "output changed signed application rows");
  assert.equal(outputState.signed_application.sha256, targetState.signed_application.sha256, "output changed signed application row digest");
  assert.deepEqual(outputState.data_contract.rows, targetState.data_contract.rows, "output changed signed data-contract rows");
  assert.equal(outputState.data_contract.sha256, targetState.data_contract.sha256, "output changed signed data-contract row digest");
  assert.deepEqual(outputState.signature_inventory, expectedState.signature_inventory, "expected fixture signature inventory drifted");
  assert.equal(outputState.signed_application.sha256, expectedState.signed_application.sha256, "expected fixture signed application drifted");
  assert.equal(outputState.data_contract.sha256, expectedState.data_contract.sha256, "expected fixture data contract drifted");
  assert.equal(outputState.lineage.operation, "reconcile");
  assert.equal(
    outputState.lineage.details.payload_digest,
    preparedDigests.payload_digest,
    "published lineage payload digest differs from the exact prepared review",
  );
  assert.deepEqual(
    outputState.lineage.parents,
    [
      ["target-derived-from", targetState.capsule_id, targetState.revision_id, targetBefore],
      ["changes-applied-from", sourceState.capsule_id, sourceState.revision_id, sourceBefore],
    ],
    "reconciliation did not bind the exact target/source identities and immutable input hashes",
  );
  if (mode === "two-way") {
    assert.equal(outputState.settings["source-only"], "from-source");
    assert.equal(outputState.content.domain, "target-conflict", "two-way manual selection changed an unselected dataset");
  } else {
    assert.equal(outputState.content.domain, "source-conflict", "take-source conflict resolution was not applied");
    assert.equal(outputState.content["clean-source"], "clean-source", "clean three-way source change was not applied");
    assert.equal(
      outputState.lineage.details.ancestor_evidence?.profile,
      "org.sqlite-capsule.reconcile-ancestor-evidence/1",
      "three-way output omitted bounded ancestor evidence",
    );
    assert.deepEqual(
      {
        capsule_id: outputState.lineage.details.ancestor_evidence?.capsule_id,
        revision_id: outputState.lineage.details.ancestor_evidence?.revision_id,
        file_sha256: outputState.lineage.details.ancestor_evidence?.file_sha256,
      },
      {
        capsule_id: ancestorState.capsule_id,
        revision_id: ancestorState.revision_id,
        file_sha256: ancestorBefore,
      },
      "three-way lineage did not bind the verified ancestor identity and immutable input hash",
    );
  }
  const expectedComparison = compareWithExpected(expectedOutput, output);
  return {
    mode,
    source_sha256: sourceBefore,
    target_sha256: targetBefore,
    ancestor_sha256: ancestorBefore,
    manual_selection_count: manualSelectionCount,
    output_sha256: sha256File(output),
    three_way_screenshot: threeWayScreenshot,
    three_way_scaled_screenshot: threeWayScaledScreenshot,
    scaled_layout: scaledLayout,
    result_screenshot: path.join(stateRoot, `${mode}-verified-result.png`),
    accessibility,
    expected_output_sha256: sha256File(expectedOutput),
    expected_comparison: expectedComparison,
    prepared_digests: preparedDigests,
    output_state: outputState,
  };
}

rmSync(fixtureRoot, { recursive: true, force: true });
mkdirSync(fixtureRoot, { recursive: true });
const ancestor = path.join(fixtureRoot, "ancestor.sqlitecapsule");
const source = path.join(fixtureRoot, "source.sqlitecapsule");
const sourceThreeWay = path.join(fixtureRoot, "source-three-way.sqlitecapsule");
const target = path.join(fixtureRoot, "target.sqlitecapsule");
prepareFixture(ancestor, "ancestor");
prepareFixture(source, "source");
prepareFixture(sourceThreeWay, "source-three-way");
prepareFixture(target, "target");

const evidence = [];
try {
  evidence.push(await runScenario({ mode: "two-way", source, target, ancestor: null }));
  evidence.push(await runScenario({ mode: "three-way", source: sourceThreeWay, target, ancestor }));
  const report = { profile: "org.sqlite-capsule.native-reconcile-e2e/1", evidence };
  writeFileSync(evidencePath, `${JSON.stringify(report, null, 2)}\n`, { flag: "w" });
  process.stdout.write(`${JSON.stringify({ ...report, evidence_path: evidencePath }, null, 2)}\n`);
} finally {
  rmSync(fixtureRoot, { recursive: true, force: true });
}

import { test, expect } from "@playwright/test";
import { readFile } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";


const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const capsule = path.join(root, ".tmp", "playwright", "diagram-studio.capsule.sqlite");
const stateDir = path.join(root, ".tmp", "playwright", "state");
const capsuleEnv = { ...process.env, SQLITE_CAPSULE_STATE_DIR: stateDir };


async function openStudio(page) {
  await page.goto("/");
  await expect(page.locator("#app")).toHaveAttribute("aria-busy", "false");
  await expect(page.locator(".node")).toHaveCount(12);
  await expect(page.locator(".edge")).toHaveCount(13);
  await expect(page.locator(".scene-button")).toHaveCount(5);
  await expect(page.locator(".layer-row")).toHaveCount(3);
}

async function assertNoHorizontalOverflow(page) {
  const metrics = await page.evaluate(() => ({
    innerWidth,
    documentWidth: document.documentElement.scrollWidth,
    bodyWidth: document.body.scrollWidth,
    toolbarRight: document.querySelector(".toolbar")?.getBoundingClientRect().right,
  }));
  expect(metrics.documentWidth).toBeLessThanOrEqual(metrics.innerWidth);
  expect(metrics.bodyWidth).toBeLessThanOrEqual(metrics.innerWidth);
  expect(metrics.toolbarRight).toBeLessThanOrEqual(metrics.innerWidth);
}

async function measureGestureDomChurn(page, locator, delta, steps = 60) {
  const box = await locator.boundingBox();
  expect(box).not.toBeNull();
  await page.evaluate(() => {
    const metrics = window.__gestureMetrics = {
      directChildMutations: 0,
      addedNodes: 0,
      removedNodes: 0,
      attributeChanges: 0,
    };
    const layer = document.querySelector("#element-layer");
    metrics.directObserver = new MutationObserver((records) => {
      for (const record of records) {
        metrics.directChildMutations += 1;
        metrics.addedNodes += record.addedNodes.length;
        metrics.removedNodes += record.removedNodes.length;
      }
    });
    metrics.attributeObserver = new MutationObserver((records) => {
      metrics.attributeChanges += records.length;
    });
    metrics.directObserver.observe(layer, { childList: true });
    metrics.attributeObserver.observe(layer, { attributes: true, subtree: true });
  });
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(
    box.x + box.width / 2 + delta.x,
    box.y + box.height / 2 + delta.y,
    { steps },
  );
  await page.mouse.up();
  await expect(page.locator("#save-state")).toContainText("Saved in SQLite");
  return page.evaluate(() => {
    const metrics = window.__gestureMetrics;
    metrics.directObserver.disconnect();
    metrics.attributeObserver.disconnect();
    return {
      directChildMutations: metrics.directChildMutations,
      addedNodes: metrics.addedNodes,
      removedNodes: metrics.removedNodes,
      attributeChanges: metrics.attributeChanges,
      gestureClassCleared: !document.querySelector("#diagram-canvas").classList.contains("is-gesturing"),
    };
  });
}

test("authoring, accessibility, interchange, and visual regression", async ({ page }) => {
  await openStudio(page);
  await assertNoHorizontalOverflow(page);
  await expect(page.getByRole("button", { name: "Match system theme" })).toHaveAttribute("aria-pressed", "true");
  await page.getByRole("button", { name: "Light theme" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  await page.reload();
  await expect(page.locator("#app")).toHaveAttribute("aria-busy", "false");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  await page.getByRole("button", { name: "Dark theme" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await expect(page).toHaveScreenshot("diagram-studio-v02-desktop.png", { animations: "disabled", maxDiffPixelRatio: 0.015 });

  await page.setViewportSize({ width: 1024, height: 768 });
  await assertNoHorizontalOverflow(page);
  await expect(page).toHaveScreenshot("diagram-studio-v02-laptop.png", { animations: "disabled", maxDiffPixelRatio: 0.015 });

  await page.setViewportSize({ width: 720, height: 900 });
  await assertNoHorizontalOverflow(page);
  await expect(page.locator(".scene-panel")).toBeHidden();
  await expect(page.locator(".inspector-panel")).toBeHidden();
  await expect(page).toHaveScreenshot("diagram-studio-v02-narrow.png", { animations: "disabled", maxDiffPixelRatio: 0.015 });

  await page.setViewportSize({ width: 1440, height: 900 });
  const appAssets = page.getByRole("button", { name: /HTML, CSS and JavaScript, component node/ });
  const domainData = page.getByRole("button", { name: /Semantic domain data, component node/ });
  await appAssets.focus();
  await appAssets.press("Enter");
  await expect(appAssets).toHaveAttribute("aria-pressed", "true");
  await expect(page.locator(".resize-handle")).toHaveCount(1);
  await expect(page).toHaveScreenshot("diagram-studio-v02-selected.png", { animations: "disabled", maxDiffPixelRatio: 0.015 });

  await domainData.focus();
  await domainData.press("Shift+Enter");
  await expect(page.locator(".node.is-selected")).toHaveCount(2);
  await expect(page.getByText("2 nodes", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Left", exact: true }).click();
  await expect(page.locator("#toast")).toContainText("Align left saved");
  await page.getByRole("button", { name: "Group", exact: true }).click();
  await expect(page.locator("#toast")).toContainText("Grouped 2 nodes");
  await expect(page).toHaveScreenshot("diagram-studio-v02-multi-layered.png", { animations: "disabled", maxDiffPixelRatio: 0.015 });
  await page.getByRole("button", { name: "Undo", exact: true }).click();
  await expect(page.locator("#toast")).toContainText("Undid: Group nodes");

  await page.getByRole("button", { name: "Move Connectors layer later" }).click();
  await expect(page.locator("#toast")).toContainText("layer order saved");
  await expect(page.locator(".layer-copy strong")).toHaveText(["Background", "Content", "Connectors"]);
  await page.getByRole("button", { name: "Undo", exact: true }).click();
  await expect(page.locator("#toast")).toContainText("Undid: Reorder layers");

  await page.locator("#selection-layer").selectOption("layer-connectors");
  await page.getByRole("button", { name: "Move to layer", exact: true }).click();
  await expect(page.locator("#toast")).toContainText("Move 2 nodes to layer saved");
  await page.reload();
  await expect(page.locator("#app")).toHaveAttribute("aria-busy", "false");
  await expect(page.getByRole("button", { name: "Undo", exact: true })).toHaveAttribute("title", /Move 2 nodes to layer/);
  await page.getByRole("button", { name: "Undo", exact: true }).click();

  const connector = page.locator(".edge").nth(4);
  await connector.press("Enter");
  await expect(page.locator(".connector-handle")).toHaveCount(2);
  await expect(page.locator("#edge-route-mode")).toHaveValue("orthogonal");
  await expect(page).toHaveScreenshot("diagram-studio-v02-routed-connector.png", { animations: "disabled", maxDiffPixelRatio: 0.015 });

  await page.locator(".scene-button").nth(1).click();
  await expect(page.locator(".scene-button").nth(1)).toHaveClass(/is-active/);
  await expect(page).toHaveScreenshot("diagram-studio-v02-scene-authoring.png", { animations: "disabled", maxDiffPixelRatio: 0.015 });
  await page.locator("#scene-capture").click();
  await expect(page.locator("#toast")).toContainText("Scene capture saved");

  page.once("dialog", (dialog) => dialog.dismiss());
  await page.getByRole("button", { name: "Layout", exact: true }).click();
  await expect(page.locator("#toast")).toContainText("Layout preview cancelled");
  page.once("dialog", (dialog) => dialog.accept());
  await page.getByRole("button", { name: "Layout", exact: true }).click();
  await expect(page.locator("#toast")).toContainText("Apply grid layout saved");
  await page.getByRole("button", { name: "Undo", exact: true }).click();
  await expect(page.locator("#toast")).toContainText("Undid: Apply grid layout");

  await page.locator(".overflow-menu > summary").click();
  await page.locator("#export-picker").selectOption("svg");
  const [svgDownload] = await Promise.all([
    page.waitForEvent("download"),
    page.getByRole("button", { name: "Export as", exact: true }).click(),
  ]);
  const svg = await readFile(await svgDownload.path(), "utf8");
  expect(svg).toMatch(/^<svg /);
  expect(svg).not.toMatch(/<script|<foreignObject|url\(https?:/i);
  expect(svg).toContain("<title>");
  expect(svg).toContain("<desc>");

  await page.locator("#export-picker").selectOption("png");
  const [pngDownload] = await Promise.all([
    page.waitForEvent("download"),
    page.getByRole("button", { name: "Export as", exact: true }).click(),
  ]);
  const png = await readFile(await pngDownload.path());
  expect([...png.subarray(0, 8)]).toEqual([137, 80, 78, 71, 13, 10, 26, 10]);
  expect(png.readUInt32BE(16)).toBe(1600);
  expect(png.readUInt32BE(20)).toBeGreaterThanOrEqual(400);

  await page.getByRole("button", { name: "Present", exact: true }).click();
  await expect(page.locator("body")).toHaveClass(/presenting/);
  await expect(page.locator("#presentation-overlay")).toBeVisible();
  await expect(page).toHaveScreenshot("diagram-studio-v02-presentation.png", { animations: "disabled", maxDiffPixelRatio: 0.015 });
  await page.getByRole("button", { name: "Exit", exact: true }).click();

  const manifestResponse = await page.request.get("/__capsule/manifest");
  expect(manifestResponse.ok()).toBeTruthy();
  const manifestBody = await manifestResponse.json();
  const historyResponse = await page.request.get("/__capsule/read/diagram.history?diagram_id=diagram-main", {
    headers: { "X-Capsule-Token": manifestBody.session_token },
  });
  expect(historyResponse.ok()).toBeTruthy();
  const history = await historyResponse.json();
  expect(history.result.cursor).toBeGreaterThan(0);

  execFileSync("python", ["tools/capsule.py", "stop", capsule], { cwd: root, env: capsuleEnv, stdio: "pipe" });
  execFileSync("python", ["tools/capsule.py", "start", capsule, "--trust-capsule", "--port", "41739"], { cwd: root, env: capsuleEnv, stdio: "pipe" });
  await openStudio(page);
  await expect(page.getByRole("button", { name: "Undo", exact: true })).not.toHaveAttribute("title", "Nothing to undo");
  await page.getByRole("button", { name: "Undo", exact: true }).click();
  await expect(page.locator("#toast")).toContainText("Undid:");
  await page.getByRole("button", { name: "Redo", exact: true }).click();
  await expect(page.locator("#toast")).toContainText("Redid:");
});

test("drag and resize previews avoid repeated canvas reconstruction", async ({ page }) => {
  await openStudio(page);
  const node = page.locator(".node[data-id='node-app-assets']");
  await node.press("Enter");

  const drag = await measureGestureDomChurn(page, node, { x: 180, y: 96 });
  expect(drag.attributeChanges).toBeGreaterThan(0);
  expect(drag.directChildMutations).toBeLessThanOrEqual(12);
  expect(drag.addedNodes).toBeLessThanOrEqual(6);
  expect(drag.removedNodes).toBeLessThanOrEqual(6);
  expect(drag.gestureClassCleared).toBeTruthy();

  const resize = await measureGestureDomChurn(page, page.locator(".resize-handle"), { x: 144, y: 72 });
  expect(resize.attributeChanges).toBeGreaterThan(0);
  expect(resize.directChildMutations).toBeLessThanOrEqual(12);
  expect(resize.addedNodes).toBeLessThanOrEqual(6);
  expect(resize.removedNodes).toBeLessThanOrEqual(6);
  expect(resize.gestureClassCleared).toBeTruthy();

  await page.getByRole("button", { name: "Undo", exact: true }).click();
  await expect(page.locator("#toast")).toContainText("Undid: Resize node");
  await page.getByRole("button", { name: "Undo", exact: true }).click();
  await expect(page.locator("#toast")).toContainText("Undid: Move node");
});

test("clipboard denial uses the explicit prompt fallback", async ({ page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        readText: async () => { throw new DOMException("Denied", "NotAllowedError"); },
        writeText: async () => { throw new DOMException("Denied", "NotAllowedError"); },
      },
    });
  });
  await openStudio(page);
  const node = page.locator(".node").nth(2);
  await node.press("Enter");
  await page.locator(".overflow-menu > summary").click();
  let promptMessage = "";
  const promptHandled = new Promise((resolve) => page.once("dialog", async (dialog) => {
    expect(dialog.type()).toBe("prompt");
    promptMessage = dialog.message();
    await dialog.dismiss();
    resolve();
  }));
  await page.locator("#copy-selection").click();
  await promptHandled;
  expect(promptMessage).toContain("Clipboard access was denied");
  await expect(page.locator("#toast")).toContainText("Clipboard denied");
});

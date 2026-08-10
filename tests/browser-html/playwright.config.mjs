import { defineConfig } from "@playwright/test";
import { fileURLToPath } from "node:url";
import path from "node:path";

const testDir = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(testDir, "..", "..");

export default defineConfig({
  testDir,
  testMatch: "html-export.spec.mjs",
  fullyParallel: false,
  workers: 1,
  retries: 0,
  timeout: 90_000,
  expect: { timeout: 15_000 },
  globalSetup: path.join(testDir, "global-setup.mjs"),
  globalTeardown: path.join(testDir, "global-teardown.mjs"),
  outputDir: path.join(root, ".tmp", "playwright-html-results"),
  snapshotPathTemplate: path.join(root, "tests", "visual-baselines", "html", "{arg}{ext}"),
  use: {
    baseURL: "http://127.0.0.1:41741",
    viewport: { width: 1440, height: 900 },
    colorScheme: "dark",
    reducedMotion: "reduce",
    locale: "en-US",
    timezoneId: "Europe/Copenhagen",
    serviceWorkers: "block",
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    acceptDownloads: true,
  },
  projects: [
    { name: "chromium", use: { browserName: "chromium" } },
    { name: "firefox", use: { browserName: "firefox" } },
    // Playwright WebKit is a compatibility engine, not evidence from actual Safari.
    { name: "webkit-compatibility", use: { browserName: "webkit" } },
  ],
});

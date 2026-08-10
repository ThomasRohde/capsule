import { defineConfig } from "@playwright/test";
import { fileURLToPath } from "node:url";
import path from "node:path";

const testDir = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(testDir, "..", "..");

export default defineConfig({
  testDir,
  testMatch: "diagram-studio.spec.mjs",
  fullyParallel: false,
  workers: 1,
  retries: 0,
  timeout: 90_000,
  expect: { timeout: 10_000 },
  globalSetup: path.join(testDir, "global-setup.mjs"),
  globalTeardown: path.join(testDir, "global-teardown.mjs"),
  outputDir: path.join(root, ".tmp", "playwright-results"),
  snapshotPathTemplate: path.join(root, "tests", "visual-baselines", "{arg}{ext}"),
  use: {
    baseURL: "http://127.0.0.1:41739",
    browserName: "chromium",
    viewport: { width: 1440, height: 900 },
    colorScheme: "dark",
    reducedMotion: "reduce",
    locale: "en-US",
    timezoneId: "Europe/Copenhagen",
    serviceWorkers: "block",
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
});

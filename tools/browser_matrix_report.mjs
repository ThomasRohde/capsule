#!/usr/bin/env node
/** Print exact automated-engine/runtime capability evidence as JSON. */

import os from "node:os";
import http from "node:http";
import { existsSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { chromium, firefox, webkit } from "playwright";

const require = createRequire(import.meta.url);
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const playwrightVersion = require("@playwright/test/package.json").version;
const engines = {};
const server = http.createServer((request, response) => {
  const match = /^\/exports\/(diagram-studio-(?:view|interactive|editable)\.html)$/.exec(request.url || "");
  if (match) {
    const target = path.join(root, "exports", match[1]);
    if (existsSync(target)) {
      response.writeHead(200, { "Content-Type": "text/html; charset=utf-8", "Cache-Control": "no-store" });
      response.end(readFileSync(target));
      return;
    }
  }
  response.writeHead(200, { "Content-Type": "text/html; charset=utf-8", "Cache-Control": "no-store" });
  response.end("<!doctype html><title>capability probe</title>");
});
await new Promise((resolve, reject) => {
  server.once("error", reject);
  server.listen(0, "127.0.0.1", resolve);
});
const address = server.address();
const probeUrl = `http://127.0.0.1:${address.port}/`;

try {
  for (const [name, browserType] of Object.entries({ chromium, firefox, webkit })) {
    const browser = await browserType.launch({ headless: true });
    const page = await browser.newPage();
    await page.goto(probeUrl);
    const report = engines[name] = {
      version: browser.version(),
      user_agent: await page.evaluate(() => navigator.userAgent),
      capabilities: await page.evaluate(() => ({
        worker: typeof Worker === "function",
        web_crypto_sha256: Boolean(globalThis.crypto?.subtle),
        compression_stream: typeof CompressionStream === "function",
        decompression_stream: typeof DecompressionStream === "function",
        file_picker: typeof showSaveFilePicker === "function",
        opfs: typeof navigator.storage?.getDirectory === "function",
      })),
      static_exports: {},
    };
    for (const profile of ["view", "interactive", "editable"]) {
      const started = performance.now();
      await page.goto(`${probeUrl}exports/diagram-studio-${profile}.html`);
      await page.locator("#capsule-host-status[data-state='ready']").waitFor({ timeout: 30_000 });
      report.static_exports[profile] = await page.locator("body").evaluate((body, elapsed) => ({
        end_to_end_milliseconds: Math.round(elapsed * 10) / 10,
        loader_boot_milliseconds: Number(body.dataset.bootMilliseconds),
        database_bytes: Number(body.dataset.databaseBytes),
        wasm_heap_bytes: Number(body.dataset.wasmHeapBytes),
      }), performance.now() - started);
    }
    await browser.close();
  }
} finally {
  await new Promise((resolve) => server.close(resolve));
}

process.stdout.write(`${JSON.stringify({
  generated_at: new Date().toISOString(),
  operating_system: { platform: os.platform(), release: os.release(), architecture: os.arch() },
  playwright_version: playwrightVersion,
  engines,
  actual_safari: { status: "not-run", note: "Playwright WebKit is not actual Safari evidence." },
}, null, 2)}\n`);

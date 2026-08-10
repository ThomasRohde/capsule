import { execFileSync, spawn } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const working = path.join(root, ".tmp", "browser-html");
const capsule = path.join(working, "diagram-studio.capsule.sqlite");
const pidFile = path.join(working, "server-pids.json");

function startServer(port, crossOriginIsolated = false) {
  const args = [
    "tests/browser-html/serve_static.py",
    "--port", String(port),
    "--directory", working,
  ];
  if (crossOriginIsolated) args.push("--cross-origin-isolated");
  const child = spawn("python", args, {
    cwd: root,
    detached: true,
    stdio: "ignore",
    windowsHide: true,
  });
  child.unref();
  return child.pid;
}

export default async function globalSetup() {
  await mkdir(working, { recursive: true });
  execFileSync("python", ["tools/build_example.py", "--output", capsule], { cwd: root, stdio: "pipe" });
  execFileSync("python", ["tools/build_exports.py", "--capsule", capsule, "--output", working], { cwd: root, stdio: "pipe" });
  execFileSync("python", ["tests/browser-html/make_fixtures.py", capsule, path.join(working, "invalid-capsule.html")], { cwd: root, stdio: "pipe" });
  execFileSync("python", [
    "tests/browser-html/make_limit_fixture.py",
    capsule,
    path.join(working, "limit.capsule.sqlite"),
    path.join(working, "limit-editable.html"),
  ], { cwd: root, stdio: "pipe" });
  const servers = [
    { port: 41741, pid: startServer(41741, false) },
    { port: 41742, pid: startServer(41742, true) },
  ];
  await writeFile(pidFile, JSON.stringify(servers), "utf8");
  let lastError;
  for (let attempt = 0; attempt < 50; attempt += 1) {
    try {
      const responses = await Promise.all(servers.map(({ port }) => fetch(
        `http://127.0.0.1:${port}/manifest.json`,
        { cache: "no-store" },
      )));
      if (responses.every((response) => response.ok)) return;
      lastError = new Error(`Static servers returned ${responses.map((response) => response.status).join(", ")}`);
    } catch (error) {
      lastError = error;
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw lastError || new Error("Static HTML export server did not become ready");
}

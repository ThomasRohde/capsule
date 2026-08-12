import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const capsule = path.join(root, ".tmp", "playwright", "diagram-studio.capsule.sqlite");
const stateDir = process.env.SQLITE_CAPSULE_BROWSER_STATE_DIR
  || path.join(root, ".tmp", "playwright", "state");
const env = { ...process.env, SQLITE_CAPSULE_STATE_DIR: stateDir };

export default async function globalTeardown() {
  try {
    execFileSync("python", ["tools/capsule.py", "stop", capsule], { cwd: root, env, stdio: "pipe" });
  } catch {
    // The test runner may call teardown after setup failed before a host existed.
  }
}

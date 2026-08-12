import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const capsule = path.join(root, ".tmp", "playwright", "diagram-studio.capsule.sqlite");
const stateDir = process.env.SQLITE_CAPSULE_BROWSER_STATE_DIR
  || path.join(root, ".tmp", "playwright", "state");
const env = { ...process.env, SQLITE_CAPSULE_STATE_DIR: stateDir };

export default async function globalSetup() {
  execFileSync("python", ["tools/build_example.py", "--output", capsule], { cwd: root, env, stdio: "pipe" });
  execFileSync("python", ["tools/capsule.py", "start", capsule, "--trust-capsule", "--port", "41739"], { cwd: root, env, stdio: "pipe" });
}

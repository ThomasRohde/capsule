import { readFile, rm } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const pidFile = path.join(root, ".tmp", "browser-html", "server-pids.json");

export default async function globalTeardown() {
  try {
    const servers = JSON.parse(await readFile(pidFile, "utf8"));
    for (const server of servers) {
      const pid = Number(server.pid);
      if (Number.isInteger(pid) && pid > 0) {
        try { process.kill(pid); } catch { /* The exact helper may already be gone. */ }
      }
    }
  } catch {
    // Setup can fail before the server starts, or the server can already be gone.
  }
  await rm(pidFile, { force: true });
}

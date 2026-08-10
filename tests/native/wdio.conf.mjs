import { existsSync, mkdirSync, rmSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawn, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../..");
const nativeRoot = path.join(root, "native");
const toolsRoot = path.join(root, ".tmp", "native-e2e-tools");
const executableSuffix = process.platform === "win32" ? ".exe" : "";
const tauriDriver = process.env.TAURI_DRIVER
  || path.join(toolsRoot, "bin", `tauri-driver${executableSuffix}`);
const nativeDriver = process.env.NATIVE_WEBDRIVER
  || path.join(toolsRoot, "edge", `msedgedriver${executableSuffix}`);
const application = process.env.SQLITE_CAPSULE_NATIVE_APPLICATION
  || path.join(nativeRoot, "target", "debug", `sqlite-capsule-desktop${executableSuffix}`);
const capsule = process.env.SQLITE_CAPSULE_NATIVE_E2E_CAPSULE
  || path.join(root, "capsules", "diagram-studio.capsule.sqlite");
const stateRoot = path.join(root, ".tmp", "native-e2e-state");
const cargo = resolveCargo();

let driverProcess;
let shuttingDown = false;

function resolveCargo() {
  const candidates = [
    process.env.CARGO,
    process.env.CARGO_HOME && path.join(process.env.CARGO_HOME, "bin", `cargo${executableSuffix}`),
    process.env.USERPROFILE && path.join(process.env.USERPROFILE, ".cargo", "bin", `cargo${executableSuffix}`),
    path.join(os.homedir(), ".cargo", "bin", `cargo${executableSuffix}`),
    `cargo${executableSuffix}`,
  ].filter(Boolean);
  return candidates.find((candidate) => candidate === `cargo${executableSuffix}` || existsSync(candidate));
}

function checked(command, args, cwd) {
  const result = spawnSync(command, args, { cwd, stdio: "inherit", shell: false });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(" ")} exited ${result.status}`);
  }
}

function requireFile(file, instruction) {
  if (!existsSync(file)) throw new Error(`${file} is missing; ${instruction}`);
}

function stopDriver() {
  shuttingDown = true;
  if (driverProcess && !driverProcess.killed) driverProcess.kill();
}

function installShutdownHandlers() {
  for (const signal of ["SIGINT", "SIGTERM", "SIGHUP", "SIGBREAK"]) {
    process.once(signal, () => {
      stopDriver();
      process.exitCode = 1;
    });
  }
  process.once("exit", stopDriver);
}

installShutdownHandlers();

export const config = {
  runner: "local",
  host: "127.0.0.1",
  port: 4444,
  specs: [path.join(here, "host-shell.e2e.mjs")],
  maxInstances: 1,
  capabilities: [{
    maxInstances: 1,
    "tauri:options": {
      application,
      args: [],
    },
  }],
  logLevel: "warn",
  bail: 0,
  waitforTimeout: 20_000,
  connectionRetryTimeout: 60_000,
  connectionRetryCount: 1,
  framework: "jasmine",
  reporters: ["spec"],
  jasmineOpts: { defaultTimeoutInterval: 60_000 },
  onPrepare() {
    if (process.platform !== "win32") {
      throw new Error("local native E2E execution is Windows-only until public platform runners are available");
    }
    requireFile(tauriDriver, "run npm run test:native:prepare");
    requireFile(nativeDriver, "run npm run test:native:prepare");
    requireFile(capsule, "build the checked example capsule first");
    checked("python", ["tools/capsule.py", "verify", capsule], root);
    checked(cargo, ["build", "-p", "sqlite-capsule-desktop"], nativeRoot);
    requireFile(application, "the native debug build did not produce the expected executable");
    rmSync(stateRoot, { recursive: true, force: true });
    mkdirSync(stateRoot, { recursive: true });
  },
  beforeSession() {
    driverProcess = spawn(
      tauriDriver,
      ["--native-driver", nativeDriver, "--port", "4444"],
      {
        cwd: root,
        env: {
          ...process.env,
          SQLITE_CAPSULE_NATIVE_E2E_PATH: capsule,
          SQLITE_CAPSULE_NATIVE_E2E_STATE_ROOT: stateRoot,
        },
        stdio: ["ignore", "inherit", "inherit"],
        shell: false,
      },
    );
    driverProcess.once("error", (error) => {
      throw error;
    });
    driverProcess.once("exit", (code) => {
      if (!shuttingDown && code !== 0) {
        process.stderr.write(`tauri-driver exited unexpectedly with code ${code}\n`);
      }
    });
  },
  afterSession() {
    stopDriver();
  },
};

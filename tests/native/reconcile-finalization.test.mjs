import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";

const source = readFileSync(new URL("../../native/desktop/ui/app.js", import.meta.url), "utf8");
const start = source.indexOf("function oncePerReconcileOperation(");
const end = source.indexOf("\n\nasync function reconcileStatusAfterStart", start);
assert.ok(start >= 0 && end > start, "finalization gate is absent from the trusted renderer");

const sandbox = { Map, Promise };
vm.createContext(sandbox);
vm.runInContext(`const reconcileFinalizations = new Map();\n${source.slice(start, end)}`, sandbox);

let acknowledgements = 0;
let terminalError = null;
const terminalWork = async () => {
  await Promise.resolve();
  acknowledgements += 1;
  return { phase: "succeeded" };
};

try {
  const [fromPoll, fromEvent] = await vm.runInContext(
    "Promise.all([oncePerReconcileOperation('operation-token', terminalWork), oncePerReconcileOperation('operation-token', terminalWork)])",
    vm.createContext({
      ...sandbox,
      terminalWork,
    }),
  );
  assert.equal(fromPoll.phase, "succeeded");
  assert.equal(fromEvent.phase, "succeeded");
} catch (error) {
  terminalError = error;
}

assert.equal(terminalError, null, "concurrent terminal poll/event finalization surfaced an error");
assert.equal(acknowledgements, 1, "concurrent terminal poll/event acknowledged more than once");

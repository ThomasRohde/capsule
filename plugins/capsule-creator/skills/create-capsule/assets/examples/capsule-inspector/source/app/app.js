import sqlite3InitModule from "/app/vendor/sqlite-wasm/index.mjs";
import { sha256Hex } from "/app/sha256.js";

const CAPSULE_APPLICATION_ID = 1129337676;
const CAPSULE_USER_VERSION = 2;
const MAX_FILE_BYTES = 64 * 1024 * 1024;
const REQUIRED_TABLES = [
  "capsule_manifest", "capsule_asset", "capsule_runbook", "capsule_command",
  "capsule_doc", "capsule_endpoint", "capsule_endpoint_step", "capsule_grant",
  "capsule_check", "capsule_prompt", "capsule_change_log",
];
const PAGE_COPY = {
  overview: ["Inspection overview", "Identity, integrity signals, capability shape, and a cautious inspection verdict."],
  assets: ["Embedded assets", "Integrity metadata for the files carried inside the database. Asset contents stay inert."],
  interfaces: ["Named interfaces", "The declared, parameterised boundary between the application UI and its canonical data."],
  guidance: ["Embedded guidance", "Runbooks, documents, prompts, and commands presented as untrusted text."],
  schema: ["Database schema", "Application-owned objects separated from the Capsule platform contract."],
};

const $ = (selector) => document.querySelector(selector);
const $$ = (selector) => [...document.querySelectorAll(selector)];
let sqlitePromise;
let inspectionSequence = 0;

function node(tag, className, text) {
  const element = document.createElement(tag);
  if (className) element.className = className;
  if (text !== undefined) element.textContent = String(text);
  return element;
}

function bytesLabel(value) {
  if (!Number.isFinite(value)) return "—";
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
  return `${(value / 1024 / 1024).toFixed(1)} MB`;
}

function shortHash(value) {
  return typeof value === "string" && value.length > 18
    ? `${value.slice(0, 10)}…${value.slice(-8)}`
    : (value || "—");
}

async function sqliteEngine() {
  if (!sqlitePromise) {
    globalThis.sqlite3ApiConfig = {
      disable: { vfs: { opfs: true, "opfs-wl": true, "opfs-sahpool": true, kvvfs: true } },
    };
    sqlitePromise = sqlite3InitModule({
      locateFile: (path) => `/app/vendor/sqlite-wasm/${path}`,
    });
  }
  return sqlitePromise;
}

function query(db, sql, bind) {
  return db.exec({
    sql,
    bind,
    rowMode: "object",
    returnValue: "resultRows",
  });
}

function scalar(db, sql) {
  const rows = query(db, sql);
  if (!rows.length) return null;
  return Object.values(rows[0])[0];
}

function parseJson(value) {
  if (typeof value !== "string") return value;
  try { return JSON.parse(value); } catch (_) { return null; }
}

function setStage(stage, state, message) {
  const item = $(`[data-stage="${stage}"]`);
  item.classList.remove("ok", "warn", "fail");
  item.classList.add(state);
  $(`#stage-${stage}`).textContent = message;
}

function setPage(page) {
  $$(".nav-item").forEach((button) => {
    const active = button.dataset.page === page;
    button.classList.toggle("is-active", active);
    if (active) button.setAttribute("aria-current", "page");
    else button.removeAttribute("aria-current");
  });
  $$("[data-page-panel]").forEach((panel) => { panel.hidden = panel.dataset.pagePanel !== page; });
  const [title, description] = PAGE_COPY[page];
  $("#page-title").textContent = title;
  $("#page-description").textContent = description;
}

function showLoading(file) {
  $("#empty-state").hidden = true;
  $("#inspection").hidden = true;
  $("#loading-state").hidden = false;
  $("#status").textContent = "";
  $("#loading-message").textContent = `Opening ${file.name} without executing its contents…`;
}

function showError(error) {
  $("#loading-state").hidden = true;
  $("#inspection").hidden = true;
  $("#empty-state").hidden = false;
  $("#status").textContent = error instanceof Error ? error.message : String(error);
}

function hasSqliteHeader(bytes) {
  if (bytes.byteLength < 16) return false;
  const expected = [83, 81, 76, 105, 116, 101, 32, 102, 111, 114, 109, 97, 116, 32, 51, 0];
  return expected.every((value, index) => bytes[index] === value);
}

async function inspectFile(file) {
  if (file.size > MAX_FILE_BYTES) throw new Error(`File is ${bytesLabel(file.size)}; the safe inspection ceiling is 64 MB.`);
  if (!file.size) throw new Error("The selected file is empty.");
  const bytes = new Uint8Array(await file.arrayBuffer());
  if (!hasSqliteHeader(bytes)) throw new Error("This file does not have a SQLite 3 header.");
  const fileHash = await sha256Hex(bytes);
  const sqlite3 = await sqliteEngine();
  const virtualPath = `/inspection-${++inspectionSequence}.sqlite3`;
  sqlite3.capi.sqlite3_js_posix_create_file(virtualPath, bytes);
  const db = new sqlite3.oo1.DB(virtualPath, "r");
  let virtualFileSystem = 0;
  try {
    db.exec("PRAGMA query_only = ON");
    db.exec("PRAGMA trusted_schema = OFF");
    const quickCheck = String(scalar(db, "PRAGMA quick_check(1)") || "unknown");
    const applicationId = Number(scalar(db, "PRAGMA application_id") || 0);
    const userVersion = Number(scalar(db, "PRAGMA user_version") || 0);
    const pageCount = Number(scalar(db, "PRAGMA page_count") || 0);
    const pageSize = Number(scalar(db, "PRAGMA page_size") || 0);
    const schema = query(db, "SELECT type, name, tbl_name FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name LIMIT 1001");
    const tableNames = new Set(schema.filter((item) => item.type === "table").map((item) => item.name));
    const isCapsuleIdentity = applicationId === CAPSULE_APPLICATION_ID && userVersion === CAPSULE_USER_VERSION;
    const missingTables = REQUIRED_TABLES.filter((name) => !tableNames.has(name));

    let manifest = null;
    if (tableNames.has("capsule_manifest")) {
      try {
        manifest = query(db, "SELECT format_id, format_version, capsule_id, title, summary, app_id, app_version, entry_asset, runtime_protocol, permissions_json, created_at, updated_at FROM capsule_manifest WHERE id = 1 LIMIT 1")[0] || null;
        if (manifest) manifest.permissions_json = parseJson(manifest.permissions_json);
      } catch (_) { manifest = null; }
    }

    const assets = [];
    let assetHashFailures = 0;
    if (tableNames.has("capsule_asset")) {
      try {
        const assetRows = query(db, "SELECT path, media_type, length(content) AS bytes, content, sha256, executable, cache_policy FROM capsule_asset ORDER BY path LIMIT 501");
        for (const row of assetRows) {
          const actual = row.content instanceof Uint8Array ? await sha256Hex(row.content) : "";
          const hashOk = Boolean(actual && actual === row.sha256);
          if (!hashOk) assetHashFailures += 1;
          assets.push({ ...row, content: undefined, actual_sha256: actual, hash_ok: hashOk });
        }
      } catch (_) { assetHashFailures += 1; }
    }

    let endpoints = [];
    if (tableNames.has("capsule_endpoint")) {
      try {
        endpoints = query(db, "SELECT name, operation, parameters_json, result_mode, description, enabled FROM capsule_endpoint ORDER BY operation, name LIMIT 501")
          .map((row) => ({ ...row, parameters_json: parseJson(row.parameters_json) }));
      } catch (_) { endpoints = []; }
    }

    const guidance = [];
    const guidanceQueries = [
      ["Runbook", "capsule_runbook", "SELECT title, body_md AS body FROM capsule_runbook ORDER BY sequence, id LIMIT 101"],
      ["Document", "capsule_doc", "SELECT title, content AS body FROM capsule_doc ORDER BY sequence, slug LIMIT 101"],
      ["Prompt", "capsule_prompt", "SELECT title, prompt_text AS body FROM capsule_prompt ORDER BY sequence, id LIMIT 101"],
      ["Command", "capsule_command", "SELECT purpose AS title, command_template AS body FROM capsule_command ORDER BY id LIMIT 101"],
      ["Check", "capsule_check", "SELECT description AS title, sql_text AS body FROM capsule_check ORDER BY id LIMIT 101"],
    ];
    for (const [kind, table, sql] of guidanceQueries) {
      if (!tableNames.has(table)) continue;
      try { query(db, sql).forEach((row) => guidance.push({ kind, ...row })); } catch (_) {}
    }

    const foreignKeyIssues = tableNames.size
      ? query(db, "SELECT * FROM pragma_foreign_key_check LIMIT 101").length
      : 0;
    const entryAssetPresent = Boolean(manifest && assets.some((asset) => asset.path === manifest.entry_asset));
    const formatCurrent = Boolean(manifest && manifest.format_id === "org.sqlite-capsule" && manifest.format_version === "0.2" && manifest.runtime_protocol === "capsule-http/0.2");
    const permissions = manifest?.permissions_json;
    const offlineDeclared = Boolean(permissions && permissions.network && permissions.network.value === "none");
    const enabledEndpoints = endpoints.filter((endpoint) => Number(endpoint.enabled) === 1);
    const applicationObjects = schema.filter((item) => !item.name.startsWith("capsule_"));
    const platformObjects = schema.filter((item) => item.name.startsWith("capsule_") || item.name === "START_HERE");

    return {
      file: { name: file.name, size: file.size, hash: fileHash },
      sqlite: { version: sqlite3.version.libVersion, quickCheck, applicationId, userVersion, pageCount, pageSize, foreignKeyIssues },
      isCapsuleIdentity, missingTables, manifest, assets, assetHashFailures, endpoints,
      guidance, schema, applicationObjects, platformObjects, entryAssetPresent,
      formatCurrent, offlineDeclared, enabledEndpoints,
    };
  } finally {
    virtualFileSystem = sqlite3.capi.sqlite3_js_db_vfs(db.pointer);
    db.close();
    if (virtualFileSystem) {
      sqlite3.wasm.xCallWrapped(
        "sqlite3__wasm_vfs_unlink",
        "int",
        ["*", "string"],
        virtualFileSystem,
        virtualPath,
      );
    }
  }
}

function renderProperties(manifest, report) {
  const list = $("#identity-list");
  list.replaceChildren();
  const properties = manifest ? [
    ["Title", manifest.title], ["App ID", manifest.app_id], ["Version", manifest.app_version],
    ["Capsule ID", manifest.capsule_id], ["Format", `${manifest.format_id} / ${manifest.format_version}`],
    ["Protocol", manifest.runtime_protocol], ["Entry asset", manifest.entry_asset],
    ["Updated", manifest.updated_at],
  ] : [
    ["Type", "Generic SQLite database"], ["Application ID", report.sqlite.applicationId],
    ["User version", report.sqlite.userVersion], ["File SHA-256", report.file.hash],
  ];
  for (const [term, value] of properties) {
    const row = node("div"); row.append(node("dt", "", term), node("dd", "", value ?? "—")); list.append(row);
  }
}

function renderFindings(report) {
  const findings = [];
  findings.push([report.sqlite.quickCheck === "ok" ? "ok" : "fail", report.sqlite.quickCheck === "ok" ? "SQLite quick check reports ok." : `SQLite quick check reports ${report.sqlite.quickCheck}.`]);
  findings.push([report.sqlite.foreignKeyIssues ? "fail" : "ok", report.sqlite.foreignKeyIssues ? `${report.sqlite.foreignKeyIssues}${report.sqlite.foreignKeyIssues > 100 ? "+" : ""} foreign-key issues detected.` : "No foreign-key issues were returned."]);
  if (report.isCapsuleIdentity) {
    findings.push([report.missingTables.length ? "fail" : "ok", report.missingTables.length ? `Missing platform tables: ${report.missingTables.join(", ")}.` : "All required 0.2 platform tables are present."]);
    findings.push([report.assetHashFailures ? "fail" : "ok", report.assetHashFailures ? `${report.assetHashFailures} embedded asset hashes do not match.` : "Every inspected asset hash matches its bytes."]);
    findings.push([report.offlineDeclared ? "ok" : "warn", report.offlineDeclared ? "Manifest declares network.value none." : "Offline network policy is missing or unreadable."]);
    findings.push(["warn", "Internal hashes show integrity, not publisher authenticity."]);
  } else {
    findings.push(["warn", "The SQLite application ID and user version do not identify Capsule format 0.2."]);
  }
  const list = $("#finding-list"); list.replaceChildren();
  for (const [state, text] of findings) list.append(node("li", state === "ok" ? "" : state, text));
}

function renderMetrics(report) {
  const metrics = [
    ["Database pages", report.sqlite.pageCount.toLocaleString(), `${bytesLabel(report.sqlite.pageSize)} each`],
    ["Embedded assets", report.assets.length > 500 ? "500+" : report.assets.length, `${report.assets.filter((item) => Number(item.executable) === 1).length} executable`],
    ["Named endpoints", report.endpoints.length > 500 ? "500+" : report.endpoints.length, `${report.enabledEndpoints.length} enabled`],
    ["Domain objects", report.applicationObjects.length > 1000 ? "1000+" : report.applicationObjects.length, "tables, views, indexes"],
  ];
  const grid = $("#metric-grid"); grid.replaceChildren();
  for (const [label, value, detail] of metrics) {
    const card = node("div", "metric"); card.append(node("span", "", label), node("strong", "", value), node("small", "", detail)); grid.append(card);
  }
}

function renderAssets(report) {
  $("#asset-count").textContent = report.assets.length > 500 ? "500+" : String(report.assets.length);
  const body = $("#asset-rows"); body.replaceChildren();
  for (const asset of report.assets) {
    const row = node("tr");
    const hashCell = node("td", asset.hash_ok ? "hash-ok" : "hash-bad", `${shortHash(asset.sha256)} ${asset.hash_ok ? "✓" : "×"}`);
    row.append(node("td", "", asset.path), node("td", "", asset.media_type), node("td", "", bytesLabel(Number(asset.bytes))), hashCell, node("td", "", Number(asset.executable) ? "Executable" : "Data"));
    body.append(row);
  }
  if (!report.assets.length) {
    const row = node("tr"); const cell = node("td", "", "No readable capsule assets were found."); cell.colSpan = 5; row.append(cell); body.append(row);
  }
}

function renderEndpoints(report) {
  $("#endpoint-count").textContent = report.endpoints.length > 500 ? "500+" : String(report.endpoints.length);
  const list = $("#endpoint-list"); list.replaceChildren();
  for (const endpoint of report.endpoints) {
    const parameterCount = endpoint.parameters_json && typeof endpoint.parameters_json === "object" ? Object.keys(endpoint.parameters_json).length : 0;
    const card = node("article", "endpoint-card");
    card.append(node("code", "", endpoint.name), node("span", `operation ${endpoint.operation === "write" ? "write" : ""}`, endpoint.operation), node("p", "", `${endpoint.description || "No description."} · ${parameterCount} parameter${parameterCount === 1 ? "" : "s"} · ${endpoint.result_mode}${Number(endpoint.enabled) ? "" : " · disabled"}`));
    list.append(card);
  }
  if (!report.endpoints.length) list.append(node("p", "empty-copy", "No readable named endpoint declarations were found."));
}

function renderGuidance(report) {
  const grid = $("#guidance-grid"); grid.replaceChildren();
  for (const item of report.guidance) {
    const card = node("article", "guidance-card");
    const header = node("header"); header.append(node("h3", "", item.title || `Untitled ${item.kind}`), node("span", "kind", item.kind));
    card.append(header, node("p", "", item.body || "No text.")); grid.append(card);
  }
  if (!report.guidance.length) grid.append(node("p", "empty-copy", "No readable embedded guidance was found."));
}

function renderSchemaGroup(container, objects) {
  container.replaceChildren();
  const byType = new Map();
  for (const object of objects) {
    if (!byType.has(object.type)) byType.set(object.type, []);
    byType.get(object.type).push(object.name);
  }
  for (const [type, names] of byType) {
    const group = node("section", "schema-group"); group.append(node("h3", "", `${type}s · ${names.length}`));
    const tags = node("div", "schema-tags"); names.forEach((name) => tags.append(node("span", "schema-tag", name))); group.append(tags); container.append(group);
  }
  if (!objects.length) container.append(node("p", "empty-copy", "No objects in this category."));
}

function renderReport(report) {
  $("#loading-state").hidden = true;
  $("#empty-state").hidden = true;
  $("#inspection").hidden = false;
  $("#file-name").textContent = report.file.name;
  $("#file-meta").textContent = `${bytesLabel(report.file.size)} · SHA-256 ${shortHash(report.file.hash)} · SQLite WASM ${report.sqlite.version}`;

  const hardFailure = report.sqlite.quickCheck !== "ok" || report.sqlite.foreignKeyIssues > 0 || report.assetHashFailures > 0;
  const currentCapsule = report.isCapsuleIdentity && report.formatCurrent && !report.missingTables.length && report.entryAssetPresent;
  const badge = $("#verdict-badge");
  const panel = $("#verdict-panel");
  badge.className = "verdict-badge"; panel.className = "verdict";
  if (hardFailure) {
    badge.classList.add("fail"); panel.classList.add("fail"); badge.textContent = "Issues found";
    $("#verdict-title").textContent = "The file has integrity or contract problems";
    $("#verdict-detail").textContent = "Do not execute it. Review the findings and use an independent verifier before making any trust decision.";
  } else if (currentCapsule) {
    badge.classList.add("ok"); panel.classList.add("ok"); badge.textContent = "Capsule 0.2";
    $("#verdict-title").textContent = "This looks like a current SQLite Capsule";
    $("#verdict-detail").textContent = "Its identity, required tables, entry asset, and inspected asset hashes are coherent. This inspection does not execute declared checks and does not authenticate the publisher.";
  } else {
    badge.classList.add("warn"); panel.classList.add("warn"); badge.textContent = report.isCapsuleIdentity ? "Incomplete capsule" : "SQLite database";
    $("#verdict-title").textContent = report.isCapsuleIdentity ? "Capsule identity found, but the current contract is incomplete" : "This is SQLite, but not a current Capsule 0.2 file";
    $("#verdict-detail").textContent = report.isCapsuleIdentity ? "Inspect the missing contract elements before execution." : "The schema is still available for black-box inspection; no embedded content has been run.";
  }

  setStage("file", "ok", `${bytesLabel(report.file.size)} · hash read`);
  setStage("sqlite", report.sqlite.quickCheck === "ok" ? "ok" : "fail", report.sqlite.quickCheck === "ok" ? "Header and pages readable" : "Quick check failed");
  setStage("contract", currentCapsule ? "ok" : (report.isCapsuleIdentity ? "fail" : "warn"), currentCapsule ? "Format 0.2 mapped" : (report.isCapsuleIdentity ? "Contract incomplete" : "Not identified"));
  setStage("application", hardFailure ? "fail" : (report.manifest ? "ok" : "warn"), report.manifest ? `${report.enabledEndpoints.length} enabled interfaces` : `${report.applicationObjects.length} domain objects`);

  renderMetrics(report); renderProperties(report.manifest, report); renderFindings(report);
  renderAssets(report); renderEndpoints(report); renderGuidance(report);
  $("#schema-count").textContent = report.schema.length > 1000 ? "1000+" : String(report.schema.length);
  renderSchemaGroup($("#domain-schema"), report.applicationObjects);
  renderSchemaGroup($("#platform-schema"), report.platformObjects);
  setPage("overview");
}

async function openFile(file) {
  if (!file) return;
  showLoading(file);
  try { renderReport(await inspectFile(file)); }
  catch (error) { showError(error); }
  finally { $("#capsule-file").value = ""; }
}

function applyTheme(theme) {
  document.documentElement.dataset.theme = theme;
  const next = theme === "dark" ? "light" : "dark";
  $("#theme-toggle").setAttribute("aria-label", `Use ${next} theme`);
  try { localStorage.setItem("capsule-inspector-theme", theme); } catch (_) {}
}

try {
  const saved = localStorage.getItem("capsule-inspector-theme");
  applyTheme(saved === "light" ? "light" : "dark");
} catch (_) { applyTheme("dark"); }

$("#theme-toggle").addEventListener("click", () => applyTheme(document.documentElement.dataset.theme === "dark" ? "light" : "dark"));
$("#capsule-file").addEventListener("change", (event) => openFile(event.target.files?.[0]));
$$('.nav-item').forEach((button) => button.addEventListener("click", () => setPage(button.dataset.page)));

for (const eventName of ["dragenter", "dragover"]) {
  document.addEventListener(eventName, (event) => { event.preventDefault(); event.dataTransfer.dropEffect = "copy"; });
}
document.addEventListener("drop", (event) => { event.preventDefault(); openFile(event.dataTransfer.files?.[0]); });

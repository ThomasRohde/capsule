/* SQLite Capsule browser host 0.1.
 *
 * This source is appended to the pinned @sqlite.org/sqlite-wasm ES module by
 * the self-contained loader. It deliberately exposes only message operations;
 * no SQLite object or raw SQL function crosses the worker boundary.
 */

const CAPSULE_APPLICATION_ID = 1129337676;
const CAPSULE_FORMATS = new Map([
  [2, { format_version: "0.2", runtime_protocol: "capsule-http/0.2" }],
]);
const MAX_CAPSULE_BYTES = 64 * 1024 * 1024;
const MAX_ASSET_BYTES = 16 * 1024 * 1024;
const MAX_REQUEST_BYTES = 1024 * 1024;
const MAX_RESULT_ROWS = 1000;
const MAX_RESULT_BYTES = 2 * 1024 * 1024;
const MAX_JSON_DEPTH = 32;
const MAX_ENDPOINT_STEPS = 16;
const ENDPOINT_TIMEOUT_MS = 3000;
const REQUIRED_TABLES = [
  "capsule_manifest", "capsule_asset", "capsule_runbook", "capsule_command",
  "capsule_doc", "capsule_endpoint", "capsule_endpoint_step", "capsule_grant",
  "capsule_check", "capsule_prompt",
  "capsule_change_log",
];
const REQUIRED_COLUMNS = {
  capsule_manifest: [
    "id", "format_id", "format_version", "capsule_id", "title", "summary",
    "app_id", "app_version", "entry_asset", "runtime_protocol",
    "permissions_json", "created_at", "updated_at",
  ],
  capsule_asset: [
    "path", "media_type", "content", "sha256", "executable", "cache_policy",
    "description",
  ],
  capsule_endpoint: [
    "name", "operation", "sql_text", "parameters_json", "result_mode",
    "description", "enabled",
  ],
  capsule_endpoint_step: ["endpoint_name", "sequence", "sql_text", "required_changes"],
  capsule_grant: ["capability", "decision", "reason", "granted_at"],
  capsule_check: ["id", "severity", "description", "sql_text", "result_mode", "expected_json"],
};

let sqlite3;
let db;
let profile;
let exportMetadata;
let verified = false;
let manifestCache;
let verificationCache;
let lastRequestId = 0;
let requestQueue = Promise.resolve();

const MESSAGE_FIELDS = {
  init: new Set(["id", "type", "profile", "metadata", "database", "wasm"]),
  manifest: new Set(["id", "type"]),
  permissions: new Set(["id", "type"]),
  read: new Set(["id", "type", "name", "parameters"]),
  write: new Set(["id", "type", "name", "parameters"]),
  assets: new Set(["id", "type"]),
  export: new Set(["id", "type"]),
  close: new Set(["id", "type"]),
};

function fail(message) {
  throw new Error(String(message));
}

function utf8Length(value) {
  return new TextEncoder().encode(value).byteLength;
}

function stableStringify(value) {
  const seen = new Set();
  function encode(item, depth) {
    if (depth > MAX_JSON_DEPTH) fail(`JSON value exceeds the ${MAX_JSON_DEPTH}-level nesting limit`);
    if (item === null) return "null";
    if (typeof item === "bigint") return item.toString();
    if (typeof item === "number") {
      if (!Number.isFinite(item)) fail("JSON values must be finite");
      return JSON.stringify(item);
    }
    if (typeof item === "boolean" || typeof item === "string") return JSON.stringify(item);
    if (Array.isArray(item)) {
      if (seen.has(item)) fail("JSON values must not be cyclic");
      seen.add(item);
      const result = `[${item.map((child) => encode(child, depth + 1)).join(",")}]`;
      seen.delete(item);
      return result;
    }
    if (typeof item === "object") {
      if (seen.has(item)) fail("JSON values must not be cyclic");
      seen.add(item);
      const parts = [];
      for (const key of Object.keys(item).sort()) {
        const child = item[key];
        if (child === undefined || typeof child === "function" || typeof child === "symbol") {
          fail("JSON values must be serialisable");
        }
        parts.push(`${JSON.stringify(key)}:${encode(child, depth + 1)}`);
      }
      seen.delete(item);
      return `{${parts.join(",")}}`;
    }
    fail("JSON values must be serialisable");
  }
  return encode(value, 0);
}

function decodeJsonColumn(key, value) {
  if (value instanceof Uint8Array || value instanceof ArrayBuffer) {
    fail("Blob values are not exposed by the generic JSON bridge");
  }
  if (typeof value === "bigint") {
    // The capsule HTTP bridge serialises SQLite integers as JSON numbers. Match
    // the value observed by browser application code after JSON.parse(), even
    // when a signed 64-bit value is outside JavaScript's exact integer range.
    return Number(value);
  }
  if (key.endsWith("_json") && typeof value === "string") {
    try { return JSON.parse(value); } catch (_) { return value; }
  }
  return value;
}

function decodeRow(row) {
  const result = {};
  for (const [key, value] of Object.entries(row)) result[key] = decodeJsonColumn(key, value);
  return result;
}

function resultBytes(value) {
  return utf8Length(stableStringify(value));
}

function sqlParameters(sqlText) {
  const named = new Set();
  const unsupported = new Set();
  for (let index = 0; index < sqlText.length;) {
    const char = sqlText[index];
    const next = sqlText[index + 1] || "";
    if (char === "-" && next === "-") {
      index += 2;
      while (index < sqlText.length && !"\r\n".includes(sqlText[index])) index += 1;
      continue;
    }
    if (char === "/" && next === "*") {
      const end = sqlText.indexOf("*/", index + 2);
      index = end < 0 ? sqlText.length : end + 2;
      continue;
    }
    if (["'", '"', "`"].includes(char)) {
      const quote = char;
      index += 1;
      while (index < sqlText.length) {
        if (sqlText[index] === quote) {
          if (sqlText[index + 1] === quote) { index += 2; continue; }
          index += 1;
          break;
        }
        index += 1;
      }
      continue;
    }
    if (char === "[") {
      const end = sqlText.indexOf("]", index + 1);
      index = end < 0 ? sqlText.length : end + 1;
      continue;
    }
    if (char === ":" && /[A-Za-z_]/.test(next)) {
      let end = index + 2;
      while (end < sqlText.length && /[A-Za-z0-9_]/.test(sqlText[end])) end += 1;
      named.add(sqlText.slice(index + 1, end));
      index = end;
      continue;
    }
    if (["?", "@", "$"].includes(char)) unsupported.add(char);
    index += 1;
  }
  return { named, unsupported };
}

function statementKind(sqlText) {
  let index = 0;
  while (index < sqlText.length) {
    while (index < sqlText.length && /\s/.test(sqlText[index])) index += 1;
    if (sqlText.startsWith("--", index)) {
      const newline = sqlText.indexOf("\n", index + 2);
      index = newline < 0 ? sqlText.length : newline + 1;
      continue;
    }
    if (sqlText.startsWith("/*", index)) {
      const end = sqlText.indexOf("*/", index + 2);
      index = end < 0 ? sqlText.length : end + 2;
      continue;
    }
    break;
  }
  const text = sqlText.slice(index);
  return (text.split(/\s+/, 1)[0] || "").toUpperCase();
}

function looksLikeSingleStatement(sqlText) {
  let index = 0;
  let hasContent = false;
  let terminated = false;
  while (index < sqlText.length) {
    const char = sqlText[index];
    const next = sqlText[index + 1] || "";
    if (/\s/.test(char)) { index += 1; continue; }
    if (char === "-" && next === "-") {
      const newline = sqlText.indexOf("\n", index + 2);
      index = newline < 0 ? sqlText.length : newline + 1;
      continue;
    }
    if (char === "/" && next === "*") {
      const end = sqlText.indexOf("*/", index + 2);
      index = end < 0 ? sqlText.length : end + 2;
      continue;
    }
    if (terminated) return false;
    if (char === ";") {
      if (!hasContent) return false;
      terminated = true;
      index += 1;
      continue;
    }
    hasContent = true;
    if (["'", '"', "`"].includes(char)) {
      const quote = char;
      index += 1;
      while (index < sqlText.length) {
        if (sqlText[index] === quote) {
          if (sqlText[index + 1] === quote) { index += 2; continue; }
          index += 1;
          break;
        }
        index += 1;
      }
      continue;
    }
    if (char === "[") {
      const end = sqlText.indexOf("]", index + 1);
      index = end < 0 ? sqlText.length : end + 1;
      continue;
    }
    index += 1;
  }
  return hasContent;
}

function safeAssetPath(path) {
  if (typeof path !== "string" || !path || path.length > 1024) return false;
  if (path.startsWith("/") || path.includes("\\") || Array.from(path).some((char) => char.codePointAt(0) < 32 || char.codePointAt(0) === 127)) return false;
  const parts = path.split("/");
  return !parts.some((part) => !part || part === "." || part === "..");
}

function validateParameterSpec(spec) {
  if (!spec || typeof spec !== "object" || Array.isArray(spec)) fail("Parameter schema must be an object");
  const allowed = new Set(["string", "number", "integer", "boolean", "json"]);
  for (const [name, rule] of Object.entries(spec)) {
    if (!name || !rule || typeof rule !== "object" || Array.isArray(rule)) fail(`Invalid rule for ${name}`);
    if (!allowed.has(rule.type)) fail(`Unsupported type for ${name}: ${rule.type}`);
    if ("required" in rule && typeof rule.required !== "boolean") fail(`required for ${name} must be boolean`);
    if ("nullable" in rule && typeof rule.nullable !== "boolean") fail(`nullable for ${name} must be boolean`);
    if ("default" in rule) {
      if (rule.default === null && !rule.nullable) fail(`default for ${name} is null but nullable is not enabled`);
      if (rule.default !== null) coerceValue(name, rule.default, rule.type);
    }
  }
}

function coerceValue(name, value, kind) {
  if (kind === "string") {
    if (typeof value === "string") return value;
    fail(`Parameter '${name}' must be a string`);
  }
  if (kind === "number") {
    if (typeof value === "boolean") fail(`Parameter '${name}' must be a number`);
    const number = typeof value === "number" ? value : (typeof value === "string" ? Number(value) : NaN);
    if (Number.isFinite(number)) return number;
    fail(`Parameter '${name}' must be a number`);
  }
  if (kind === "integer") {
    if (typeof value === "boolean") fail(`Parameter '${name}' must be an integer`);
    if (typeof value === "bigint") {
      if (value >= -(2n ** 63n) && value <= 2n ** 63n - 1n) return value;
    } else if (typeof value === "number" && Number.isInteger(value)) {
      if (Number.isSafeInteger(value)) return value;
    } else if (typeof value === "string" && /^[+-]?\d+$/.test(value.trim())) {
      const integer = BigInt(value.trim());
      if (integer >= -(2n ** 63n) && integer <= 2n ** 63n - 1n) {
        return integer >= Number.MIN_SAFE_INTEGER && integer <= Number.MAX_SAFE_INTEGER ? Number(integer) : integer;
      }
    }
    fail(`Parameter '${name}' must be an integer`);
  }
  if (kind === "boolean") {
    if (typeof value === "boolean") return value ? 1 : 0;
    if (typeof value === "string" && ["true", "false", "1", "0"].includes(value.toLowerCase())) {
      return ["true", "1"].includes(value.toLowerCase()) ? 1 : 0;
    }
    fail(`Parameter '${name}' must be boolean`);
  }
  if (kind === "json") return stableStringify(value);
  fail(`Unsupported parameter type: ${kind}`);
}

function validateParameters(spec, parameters) {
  if (!parameters || typeof parameters !== "object" || Array.isArray(parameters)) fail("Parameters must be an object");
  const unknown = Object.keys(parameters).filter((name) => !(name in spec)).sort();
  if (unknown.length) fail(`Unknown parameters: ${unknown.join(", ")}`);
  const bound = {};
  for (const [name, rule] of Object.entries(spec)) {
    const provided = Object.prototype.hasOwnProperty.call(parameters, name);
    const hasDefault = Object.prototype.hasOwnProperty.call(rule, "default");
    const value = provided ? parameters[name] : (hasDefault ? rule.default : null);
    if (!provided && !hasDefault && rule.required) fail(`Missing required parameter: ${name}`);
    if (value === null) {
      if ((provided || hasDefault) && !rule.nullable) fail(`Parameter '${name}' may not be null`);
      bound[`:${name}`] = null;
    } else {
      bound[`:${name}`] = coerceValue(name, value, rule.type);
    }
  }
  return bound;
}

function probeParameters(spec) {
  const examples = { string: "", number: 0, integer: 0, boolean: 0, json: "null" };
  const result = {};
  for (const [name, rule] of Object.entries(spec)) result[`:${name}`] = examples[rule.type];
  return result;
}

function bindForSql(sqlText, bound) {
  const used = sqlParameters(sqlText).named;
  const result = {};
  for (const name of used) {
    const key = `:${name}`;
    if (Object.prototype.hasOwnProperty.call(bound, key)) result[key] = bound[key];
  }
  return result;
}

function withProgressDeadline(callback) {
  const deadline = performance.now() + ENDPOINT_TIMEOUT_MS;
  sqlite3.capi.sqlite3_progress_handler(db.pointer, 1000, () => performance.now() > deadline ? 1 : 0, 0);
  try { return callback(); }
  finally { sqlite3.capi.sqlite3_progress_handler(db.pointer, 0, null, 0); }
}

function installAuthorizer(operation) {
  const capi = sqlite3.capi;
  const denyNames = [
    "SQLITE_ATTACH", "SQLITE_DETACH", "SQLITE_ALTER_TABLE", "SQLITE_CREATE_INDEX",
    "SQLITE_CREATE_TABLE", "SQLITE_CREATE_TEMP_INDEX", "SQLITE_CREATE_TEMP_TABLE",
    "SQLITE_CREATE_TEMP_TRIGGER", "SQLITE_CREATE_TEMP_VIEW", "SQLITE_CREATE_TRIGGER",
    "SQLITE_CREATE_VIEW", "SQLITE_CREATE_VTABLE", "SQLITE_DROP_INDEX", "SQLITE_DROP_TABLE",
    "SQLITE_DROP_TEMP_INDEX", "SQLITE_DROP_TEMP_TABLE", "SQLITE_DROP_TEMP_TRIGGER",
    "SQLITE_DROP_TEMP_VIEW", "SQLITE_DROP_TRIGGER", "SQLITE_DROP_VIEW", "SQLITE_DROP_VTABLE",
    "SQLITE_PRAGMA", "SQLITE_REINDEX", "SQLITE_ANALYZE",
  ];
  const deny = new Set(denyNames.map((name) => capi[name]).filter((value) => value !== undefined));
  const writes = new Set([capi.SQLITE_INSERT, capi.SQLITE_UPDATE, capi.SQLITE_DELETE]);
  capi.sqlite3_set_authorizer(db.pointer, (_context, action, arg1) => {
    if (deny.has(action)) return capi.SQLITE_DENY;
    if (writes.has(action)) {
      const table = String(arg1 || "").toLowerCase();
      if (operation === "read" || table.startsWith("capsule_") || table.startsWith("sqlite_")) {
        return capi.SQLITE_DENY;
      }
    }
    return capi.SQLITE_OK;
  }, 0);
}

function clearAuthorizer() {
  sqlite3.capi.sqlite3_set_authorizer(db.pointer, null, 0);
}

function executeRows(sql, bind = undefined) {
  const options = { sql, rowMode: "object", returnValue: "resultRows" };
  if (bind !== undefined) options.bind = bind;
  return db.exec(options).map(decodeRow);
}

function encodeReadResult(sql, bind, mode) {
  const rows = executeRows(sql, bind);
  if (mode === "rows") {
    if (rows.length > MAX_RESULT_ROWS) fail(`Result exceeds the ${MAX_RESULT_ROWS}-row limit`);
    if (resultBytes(rows) > MAX_RESULT_BYTES) fail(`Result exceeds the ${MAX_RESULT_BYTES}-byte limit`);
    return rows;
  }
  if (mode === "row") {
    const row = rows.length ? rows[0] : null;
    if (resultBytes(row) > MAX_RESULT_BYTES) fail(`Result exceeds the ${MAX_RESULT_BYTES}-byte limit`);
    return row;
  }
  if (mode === "scalar") {
    const row = rows.length ? rows[0] : null;
    const value = row ? Object.values(row)[0] : null;
    if (resultBytes(value) > MAX_RESULT_BYTES) fail(`Result exceeds the ${MAX_RESULT_BYTES}-byte limit`);
    return value;
  }
  fail(`Unsupported read result mode: ${mode}`);
}

async function sha256Hex(bytes) {
  if (!globalThis.crypto?.subtle) fail("Web Crypto SHA-256 is unavailable");
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
}

async function verifyCapsule(expectedDatabaseSha256) {
  const errors = [];
  const warnings = [];
  const checks = [];
  const addError = (message) => errors.push(String(message));
  try {
    if (Number(db.selectValue("PRAGMA application_id")) !== CAPSULE_APPLICATION_ID) addError("Unexpected PRAGMA application_id");
    const userVersion = Number(db.selectValue("PRAGMA user_version"));
    const format = CAPSULE_FORMATS.get(userVersion);
    if (!format) addError(`Unsupported PRAGMA user_version: ${userVersion}`);
    const integrity = db.selectValue("PRAGMA integrity_check");
    if (integrity !== "ok") addError(`SQLite integrity check failed: ${integrity}`);
    if (executeRows("PRAGMA foreign_key_check").length) addError("Foreign-key check failed");
    const objects = executeRows("SELECT type, name FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'");
    const tables = new Set(objects.filter((row) => row.type === "table").map((row) => row.name));
    const views = new Set(objects.filter((row) => row.type === "view").map((row) => row.name));
    for (const table of REQUIRED_TABLES) if (!tables.has(table)) addError(`Missing required table: ${table}`);
    if (!views.has("START_HERE")) addError("Missing START_HERE view");
    const triggers = objects.filter((row) => row.type === "trigger");
    if (triggers.length) addError("Capsule triggers are not permitted");
    const virtualTables = executeRows("PRAGMA table_list").filter((row) => row.type === "virtual");
    if (virtualTables.length) addError("Capsule virtual tables are not permitted");
    for (const [table, required] of Object.entries(REQUIRED_COLUMNS)) {
      if (!tables.has(table)) continue;
      const columns = new Set(executeRows(`PRAGMA table_info('${table.replaceAll("'", "''")}')`).map((row) => row.name));
      for (const column of required) if (!columns.has(column)) addError(`Table ${table} is missing column ${column}`);
    }
    const manifests = executeRows("SELECT * FROM capsule_manifest");
    if (manifests.length !== 1) addError(`Expected exactly one manifest, found ${manifests.length}`);
    manifestCache = manifests.length ? manifests[0] : null;
    if (manifestCache && format) {
      if (manifestCache.format_id !== "org.sqlite-capsule") addError("Unexpected manifest format_id");
      if (manifestCache.format_version !== format.format_version) addError("Manifest format_version does not match user_version");
      if (manifestCache.runtime_protocol !== format.runtime_protocol) addError("Manifest runtime_protocol does not match user_version");
      if (!safeAssetPath(manifestCache.entry_asset)) addError("Manifest entry_asset is unsafe");
    }
    if (manifestCache && typeof manifestCache.permissions_json !== "object") addError("Manifest permissions_json must decode to an object");
    if (manifestCache && manifestCache.permissions_json && typeof manifestCache.permissions_json === "object") {
      const enabledOperations = new Set(executeRows("SELECT DISTINCT operation FROM capsule_endpoint WHERE enabled = 1").map((row) => row.operation));
      for (const [operation, capability] of [["read", "database.read"], ["write", "database.write"]]) {
        if (enabledOperations.has(operation) && manifestCache.permissions_json[capability]?.required !== true) {
          addError(`Enabled ${operation} endpoints require permission '${capability}'`);
        }
      }
      if (manifestCache.permissions_json.network?.value !== "none") addError("Manifest network permission must be 'none'");
    }
    const assets = db.exec({
      sql: "SELECT path, media_type, content, sha256, executable, cache_policy, description FROM capsule_asset ORDER BY path",
      rowMode: "object",
      returnValue: "resultRows",
    });
    const caseInsensitivePaths = new Map();
    for (const asset of assets) {
      if (!safeAssetPath(asset.path)) { addError(`Unsafe asset path: ${asset.path}`); continue; }
      const foldedPath = asset.path.toLowerCase();
      if (caseInsensitivePaths.has(foldedPath) && caseInsensitivePaths.get(foldedPath) !== asset.path) {
        addError(`Case-insensitive asset collision: ${caseInsensitivePaths.get(foldedPath)} and ${asset.path}`);
      }
      caseInsensitivePaths.set(foldedPath, asset.path);
      if (typeof asset.media_type !== "string" || !/^[A-Za-z0-9!#$&^_.+-]+\/[A-Za-z0-9!#$&^_.+-]+(?:\s*;\s*[A-Za-z0-9!#$&^_.+-]+\s*=\s*[A-Za-z0-9!#$&^_.+'-]+)*$/.test(asset.media_type)) addError(`Asset ${asset.path} has an unsafe media type`);
      if (asset.cache_policy !== "no-store") addError(`Asset ${asset.path} has an unsafe cache policy`);
      if (!(asset.content instanceof Uint8Array)) { addError(`Asset ${asset.path} content is not a blob`); continue; }
      if (asset.content.byteLength > MAX_ASSET_BYTES) addError(`Asset ${asset.path} exceeds size limit`);
      const digest = await sha256Hex(asset.content);
      if (digest !== asset.sha256) addError(`Asset ${asset.path} SHA-256 mismatch`);
    }
    if (manifestCache && !assets.some((asset) => asset.path === manifestCache.entry_asset)) addError("Manifest entry asset is missing");

    if (tables.has("capsule_endpoint")) {
      const endpoints = executeRows("SELECT * FROM capsule_endpoint WHERE enabled = 1 ORDER BY name");
      for (const endpoint of endpoints) {
        try {
          if (!["read", "write"].includes(endpoint.operation)) fail("invalid operation");
          if (!["rows", "row", "scalar", "changes"].includes(endpoint.result_mode)) fail("invalid result mode");
          if (!looksLikeSingleStatement(endpoint.sql_text)) fail("not one statement");
          const kind = statementKind(endpoint.sql_text);
          if (endpoint.operation === "read" && !["SELECT", "WITH"].includes(kind)) fail("read endpoint is not SELECT/WITH");
          const spec = endpoint.parameters_json;
          validateParameterSpec(spec);
          const declared = new Set(Object.keys(spec));
          const probe = probeParameters(spec);
          let statements = [endpoint.sql_text];
          if (userVersion === 2) {
            const steps = executeRows("SELECT sequence, sql_text, required_changes FROM capsule_endpoint_step WHERE endpoint_name = :name ORDER BY sequence", { ":name": endpoint.name });
            if (steps.length) {
              if (endpoint.operation !== "write" || endpoint.result_mode !== "changes") fail("compound endpoint must be a write/changes endpoint");
              if (steps.length < 2 || steps.length > MAX_ENDPOINT_STEPS) fail("invalid compound endpoint step count");
              if (steps[0].sql_text !== endpoint.sql_text) fail("compound step 1 differs from sql_text");
              statements = steps.map((step) => step.sql_text);
              for (let index = 0; index < steps.length; index += 1) {
                if (Number(steps[index].sequence) !== index + 1) fail("compound steps are not contiguous");
              }
            }
          }
          const used = new Set();
          const unsupported = new Set();
          for (const statement of statements) {
            const parameters = sqlParameters(statement);
            for (const name of parameters.named) used.add(name);
            for (const marker of parameters.unsupported) unsupported.add(marker);
            if (!looksLikeSingleStatement(statement)) fail("not one statement");
            const statementKindValue = statementKind(statement);
            if (endpoint.operation === "write" && !["INSERT", "UPDATE", "DELETE", "REPLACE", "WITH"].includes(statementKindValue)) fail(`write statement begins with disallowed ${statementKindValue}`);
            installAuthorizer(endpoint.operation);
            try { db.exec({ sql: `EXPLAIN ${statement}`, bind: bindForSql(statement, probe) }); }
            finally { clearAuthorizer(); }
          }
          if (unsupported.size) fail(`unsupported SQL parameter styles: ${Array.from(unsupported).join(",")}`);
          if (Array.from(used).some((name) => !declared.has(name))) fail("SQL references undeclared parameters");
          if (Array.from(declared).some((name) => !used.has(name))) fail("endpoint declares unused parameters");
        } catch (error) {
          addError(`Endpoint '${endpoint.name}' is invalid: ${error.message || error}`);
        }
      }
    }

    if (tables.has("capsule_check")) {
      const declaredChecks = executeRows("SELECT * FROM capsule_check ORDER BY id");
      for (const check of declaredChecks) {
        let actual;
        let passed = false;
        try {
          if (!looksLikeSingleStatement(check.sql_text) || !["SELECT", "WITH"].includes(statementKind(check.sql_text))) fail("not read-only SQL");
          db.exec("PRAGMA query_only = ON");
          installAuthorizer("read");
          try {
            const rows = withProgressDeadline(() => executeRows(check.sql_text));
            const expected = check.expected_json;
            if (check.result_mode === "scalar") {
              actual = rows.length ? Object.values(rows[0])[0] : null;
              passed = stableStringify(actual) === stableStringify(expected);
            } else if (check.result_mode === "empty") {
              actual = rows;
              passed = rows.length === 0;
            } else {
              actual = rows;
              passed = stableStringify(rows) === stableStringify(expected);
            }
          } finally {
            clearAuthorizer();
            db.exec("PRAGMA query_only = OFF");
          }
        } catch (error) {
          actual = String(error.message || error);
        }
        checks.push({ id: check.id, passed, detail: passed ? "ok" : `expected ${stableStringify(check.expected_json)}, got ${stableStringify(actual)}` });
        if (!passed) {
          const message = `Check '${check.id}' failed`;
          if (check.severity === "error") addError(message); else warnings.push(message);
        }
      }
    }
  } catch (error) {
    addError(error.message || error);
  }
  const currentBytes = sqlite3.capi.sqlite3_js_db_export(db.pointer);
  const currentSha256 = await sha256Hex(currentBytes);
  if (currentSha256 !== expectedDatabaseSha256) addError("Imported database SHA-256 differs from export metadata");
  return { ok: errors.length === 0, errors, warnings, checks, manifest: manifestCache, database_sha256: currentSha256 };
}

function permissionReport() {
  const requested = manifestCache.permissions_json || {};
  const grants = {};
  const hasGrants = Number(db.selectValue("SELECT count(*) FROM sqlite_schema WHERE type='table' AND name='capsule_grant'")) > 0;
  if (hasGrants) {
    for (const row of executeRows("SELECT capability, decision, reason, granted_at FROM capsule_grant ORDER BY capability")) grants[row.capability] = row;
  }
  const effective = {};
  for (const capability of Object.keys(requested)) {
    const base = grants[capability] || { capability, decision: "prompt" };
    effective[capability] = { ...base };
  }
  if (profile !== "editable") effective["database.write"] = { capability: "database.write", decision: "deny", reason: `Denied by ${profile} export profile` };
  effective.network = { capability: "network", decision: "deny", reason: "Self-contained HTML exports run offline" };
  return { requested, grants, effective, profile };
}

function publicManifest() {
  return {
    ...manifestCache,
    export_profile: profile,
    export_provenance: {
      source_sha256: exportMetadata.source.sha256,
      database_sha256: exportMetadata.current.database_sha256,
      revision: exportMetadata.current.revision,
      parent_database_sha256: exportMetadata.current.parent_database_sha256,
    },
    host: { kind: "browser-html", id: "org.sqlite-capsule.browser-host", version: "0.2" },
  };
}

function executeEndpoint(name, operation, parameters) {
  if (!verified) fail("Capsule is not verified");
  if (operation === "write" && profile !== "editable") fail(`Export profile '${profile}' is read-only`);
  if (utf8Length(stableStringify(parameters)) > MAX_REQUEST_BYTES) fail(`Request exceeds ${MAX_REQUEST_BYTES}-byte limit`);
  const endpoint = executeRows("SELECT * FROM capsule_endpoint WHERE name = :name AND enabled = 1", { ":name": name })[0];
  if (!endpoint) fail(`Unknown or disabled endpoint: ${name}`);
  if (endpoint.operation !== operation) fail(`Endpoint '${name}' is '${endpoint.operation}', not '${operation}'`);
  const spec = endpoint.parameters_json;
  validateParameterSpec(spec);
  const bound = validateParameters(spec, parameters);
  const steps = executeRows("SELECT sequence, sql_text, required_changes FROM capsule_endpoint_step WHERE endpoint_name = :name ORDER BY sequence", { ":name": name });
  const statements = steps.length ? steps : [{ sequence: 1, sql_text: endpoint.sql_text, required_changes: null }];
  if (steps.length) {
    if (operation !== "write" || endpoint.result_mode !== "changes") fail("Compound endpoint must be a write/changes endpoint");
    if (steps.length < 2 || steps.length > MAX_ENDPOINT_STEPS) fail("Compound endpoint has an invalid step count");
    if (steps[0].sql_text !== endpoint.sql_text) fail("Compound endpoint step 1 differs from sql_text");
    if (steps.some((step, index) => Number(step.sequence) !== index + 1)) fail("Compound endpoint steps are not contiguous");
  }
  const usedParameters = new Set();
  const unsupportedParameters = new Set();
  for (const statement of statements) {
    if (!looksLikeSingleStatement(statement.sql_text)) fail("Endpoint contains multiple statements in one step");
    const kind = statementKind(statement.sql_text);
    const allowed = operation === "read" ? ["SELECT", "WITH"] : ["INSERT", "UPDATE", "DELETE", "REPLACE", "WITH"];
    if (!allowed.includes(kind)) fail(`Endpoint statement begins with disallowed ${kind}`);
    const sqlParameterSet = sqlParameters(statement.sql_text);
    for (const used of sqlParameterSet.named) usedParameters.add(used);
    for (const marker of sqlParameterSet.unsupported) unsupportedParameters.add(marker);
  }
  if (unsupportedParameters.size) fail("Endpoint contains unsupported parameter markers");
  if (Object.keys(spec).some((declared) => !usedParameters.has(declared)) || Array.from(usedParameters).some((used) => !(used in spec))) {
    fail("Endpoint parameters do not match SQL placeholders");
  }

  if (operation === "read") {
    db.exec("PRAGMA query_only = ON");
    installAuthorizer("read");
    try { return withProgressDeadline(() => encodeReadResult(endpoint.sql_text, bindForSql(endpoint.sql_text, bound), endpoint.result_mode)); }
    finally {
      clearAuthorizer();
      db.exec("PRAGMA query_only = OFF");
    }
  }

  db.exec("BEGIN IMMEDIATE");
  const stepChanges = [];
  let lastrowid = null;
  try {
    installAuthorizer("write");
    withProgressDeadline(() => {
      for (const step of statements) {
        db.exec({ sql: step.sql_text, bind: bindForSql(step.sql_text, bound) });
        const changed = Number(db.selectValue("SELECT changes()"));
        if (step.required_changes !== null && Number(step.required_changes) !== changed) {
          fail(`Compound endpoint '${name}' step ${step.sequence} changed ${changed} rows; expected ${step.required_changes}`);
        }
        stepChanges.push(changed);
        lastrowid = decodeJsonColumn("lastrowid", db.selectValue("SELECT last_insert_rowid()"));
      }
    });
    clearAuthorizer();
    const totalChanges = stepChanges.reduce((sum, value) => sum + value, 0);
    db.exec({
      sql: "INSERT INTO capsule_change_log (endpoint_name, parameters_json, changed_rows, occurred_at) VALUES (:name, :parameters, :changes, :time)",
      bind: {
        ":name": name,
        ":parameters": stableStringify(Object.fromEntries(Object.entries(bound).map(([key, value]) => [key.slice(1), value]))),
        ":changes": totalChanges,
        ":time": new Date().toISOString().replace(/\.\d{3}Z$/, "Z"),
      },
    });
    db.exec("COMMIT");
    return steps.length
      ? { changes: totalChanges, step_changes: stepChanges, lastrowid }
      : { changes: totalChanges, lastrowid };
  } catch (error) {
    clearAuthorizer();
    try { db.exec("ROLLBACK"); } catch (_) {}
    throw error;
  }
}

async function initialise(message) {
  if (verified || db) fail("Worker is already initialised");
  const databaseBytes = new Uint8Array(message.database);
  const wasmBytes = new Uint8Array(message.wasm);
  if (!databaseBytes.byteLength || databaseBytes.byteLength > MAX_CAPSULE_BYTES) fail("Database payload size is invalid");
  if (!["view", "interactive", "editable"].includes(message.profile)) fail("Unknown export profile");
  profile = message.profile;
  exportMetadata = message.metadata;
  globalThis.sqlite3ApiConfig = { disable: { vfs: { opfs: true, "opfs-wl": true, "opfs-sahpool": true, kvvfs: true } } };
  sqlite3 = await sqlite3_bundler_friendly_default({
    wasmBinary: wasmBytes,
    // Emscripten resolves a nominal WASM URL even when wasmBinary is supplied.
    // A blob: module URL cannot act as a relative-URL base, so keep the nominal
    // path inert and let wasmBinary provide the actual bytes.
    locateFile: (path) => path,
  });
  sqlite3.capi.sqlite3_js_posix_create_file("/capsule.sqlite3", databaseBytes);
  db = new sqlite3.oo1.DB("/capsule.sqlite3", "w");
  db.exec("PRAGMA foreign_keys = ON");
  db.exec("PRAGMA trusted_schema = OFF");
  db.exec("PRAGMA busy_timeout = 3000");
  verificationCache = await verifyCapsule(message.metadata.current.database_sha256);
  if (!verificationCache.ok) fail(`Capsule verification failed: ${verificationCache.errors.join("; ")}`);
  verified = true;
  return {
    verification: verificationCache,
    manifest: publicManifest(),
    permissions: permissionReport(),
    sqlite_version: sqlite3.version.libVersion,
    memory: {
      database_bytes: databaseBytes.byteLength,
      wasm_heap_bytes: Number(sqlite3.wasm.memory.buffer.byteLength),
    },
  };
}

async function assetsForApplication() {
  if (!verified) fail("Capsule is not verified");
  const rows = db.exec({
    sql: "SELECT path, media_type, content, sha256, executable, cache_policy, description FROM capsule_asset ORDER BY path",
    rowMode: "object",
    returnValue: "resultRows",
  });
  return rows.map((row) => ({
    path: row.path,
    media_type: row.media_type,
    content: row.content,
    sha256: row.sha256,
    executable: Number(row.executable),
    cache_policy: row.cache_policy,
    description: row.description,
  }));
}

async function handleMessage(message) {
  if (!message || typeof message !== "object" || Array.isArray(message)) fail("Message must be an object");
  if (!Number.isSafeInteger(message.id) || message.id < 1) fail("Message id must be a positive safe integer");
  if (message.id <= lastRequestId) fail("Message id must be strictly increasing and must not be reused");
  lastRequestId = message.id;
  const allowed = MESSAGE_FIELDS[message.type];
  if (!allowed) fail(`Unknown worker message type: ${message.type}`);
  const unknown = Object.keys(message).filter((field) => !allowed.has(field));
  if (unknown.length) fail(`Unknown field(s) for '${message.type}': ${unknown.sort().join(", ")}`);
  if (message.type !== "init" && !verified) fail("Capsule is not verified");
  if (["read", "write"].includes(message.type) && (typeof message.name !== "string" || !message.name || message.name.length > 256)) {
    fail("Endpoint name must be a non-empty string of at most 256 characters");
  }
  switch (message.type) {
    case "init": return initialise(message);
    case "manifest": return publicManifest();
    case "permissions": return permissionReport();
    case "read": return executeEndpoint(message.name, "read", message.parameters || {});
    case "write": return executeEndpoint(message.name, "write", message.parameters || {});
    case "assets": return assetsForApplication();
    case "export": {
      if (!verified || profile !== "editable") fail("Database export is available only to an editable host control path");
      const candidate = sqlite3.capi.sqlite3_js_db_export(db.pointer);
      const candidateSha256 = await sha256Hex(candidate);
      const saveVerification = await verifyCapsule(candidateSha256);
      if (!saveVerification.ok) fail(`Database failed pre-save verification: ${saveVerification.errors.join("; ")}`);
      const bytes = sqlite3.capi.sqlite3_js_db_export(db.pointer);
      const sha256 = await sha256Hex(bytes);
      if (sha256 !== candidateSha256) fail("Database changed during pre-save verification");
      return {
        bytes,
        sha256,
        verification: saveVerification,
        memory: {
          database_bytes: bytes.byteLength,
          wasm_heap_bytes: Number(sqlite3.wasm.memory.buffer.byteLength),
        },
      };
    }
    case "close": {
      if (db) db.close();
      db = null;
      verified = false;
      return { closed: true };
    }
    default: fail(`Unknown worker message type: ${message.type}`);
  }
}

async function processMessage(message) {
  const id = message && message.id;
  try {
    const result = await handleMessage(message);
    const transfer = [];
    if (result?.bytes instanceof Uint8Array) transfer.push(result.bytes.buffer);
    if (Array.isArray(result)) {
      for (const item of result) if (item?.content instanceof Uint8Array) transfer.push(item.content.buffer);
    }
    self.postMessage({ id, ok: true, result }, transfer);
  } catch (error) {
    // Firefox and WebKit stack strings may omit the exception message entirely.
    // The protocol carries the stable, user-actionable message; developer tools
    // still retain the thrown error at its origin when debugging the worker.
    self.postMessage({ id, ok: false, error: String(error?.message || error) });
  }
}

self.addEventListener("message", (event) => {
  const message = event.data;
  requestQueue = requestQueue.then(() => processMessage(message), () => processMessage(message));
});

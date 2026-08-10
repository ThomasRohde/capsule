(() => {
  "use strict";

  const MAX_BLOCK_BYTES = 96 * 1024 * 1024;
  const MAX_CAPSULE_BYTES = 64 * 1024 * 1024;
  const MAX_CONCURRENT_REQUESTS = 8;
  const SHA256 = /^[0-9a-f]{64}$/;
  const bootStarted = performance.now();
  const encoder = new TextEncoder();
  const decoder = new TextDecoder("utf-8", { fatal: true });
  const pending = new Map();
  let nextWorkerId = 1;
  let worker;
  let workerUrl;
  let appUrl;
  let appNonce;
  let metadata;
  let runtimeState;
  let dirty = false;
  let dirtyGeneration = 0;
  let activeAppRequests = 0;
  let lastAppRequestId = 0;

  const elements = {
    status: document.getElementById("capsule-host-status"),
    identity: document.getElementById("capsule-host-identity"),
    profile: document.getElementById("capsule-host-profile"),
    provenance: document.getElementById("capsule-host-provenance"),
    save: document.getElementById("capsule-save-html"),
    download: document.getElementById("capsule-download-html"),
    noticesButton: document.getElementById("capsule-third-party-button"),
    noticesPanel: document.getElementById("capsule-third-party-panel"),
    noticesClose: document.getElementById("capsule-third-party-close"),
    noticesText: document.getElementById("capsule-third-party-text"),
    frame: document.getElementById("capsule-app-frame"),
    error: document.getElementById("capsule-host-error"),
  };

  function setStatus(message, state = "loading") {
    elements.status.textContent = message;
    elements.status.dataset.state = state;
  }

  function showError(error) {
    const message = String(error?.message || error || "Unknown browser host error");
    elements.error.hidden = false;
    elements.error.textContent = message;
    setStatus("Export could not be opened", "error");
    console.error("SQLite Capsule browser host:", error);
  }

  function stableJson(value) {
    if (value === null || typeof value !== "object") return JSON.stringify(value);
    if (Array.isArray(value)) return `[${value.map(stableJson).join(",")}]`;
    return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${stableJson(value[key])}`).join(",")}}`;
  }

  function safeMetadataText(value) {
    return stableJson(value)
      .replaceAll("&", "\\u0026")
      .replaceAll("<", "\\u003c")
      .replaceAll(">", "\\u003e")
      .replaceAll("\u2028", "\\u2028")
      .replaceAll("\u2029", "\\u2029");
  }

  function exactKeys(value, expected, label) {
    if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error(`${label} must be an object`);
    const actual = Object.keys(value).sort();
    const wanted = [...expected].sort();
    if (actual.length !== wanted.length || actual.some((key, index) => key !== wanted[index])) {
      throw new Error(`${label} has missing or unknown fields`);
    }
  }

  function requireSha(value, label) {
    if (typeof value !== "string" || !SHA256.test(value)) throw new Error(`${label} must be a lowercase SHA-256 value`);
  }

  function requireBytes(value, label, maximum = MAX_BLOCK_BYTES) {
    if (!Number.isSafeInteger(value) || value < 1 || value > maximum) throw new Error(`${label} must be a positive bounded integer`);
  }

  function validateMetadata(value) {
    exactKeys(value, ["contract_id", "contract_version", "profile", "created_at", "source", "current", "runtime"], "Export metadata");
    if (value.contract_id !== "org.sqlite-capsule.html-export" || value.contract_version !== "0.2") throw new Error("Unsupported HTML export contract");
    if (!["view", "interactive", "editable"].includes(value.profile)) throw new Error("Unknown HTML export profile");
    if (typeof value.created_at !== "string" || !value.created_at.endsWith("Z") || Number.isNaN(Date.parse(value.created_at))) throw new Error("created_at must be a UTC timestamp");
    exactKeys(value.source, ["sha256", "bytes", "capsule_id", "format_id", "format_version", "runtime_protocol", "app_id", "app_version", "title"], "source");
    requireSha(value.source.sha256, "source.sha256");
    requireBytes(value.source.bytes, "source.bytes", MAX_CAPSULE_BYTES);
    for (const field of ["capsule_id", "format_id", "format_version", "runtime_protocol", "app_id", "app_version", "title"]) {
      if (typeof value.source[field] !== "string" || !value.source[field]) throw new Error(`source.${field} must be a non-empty string`);
    }
    if (value.source.format_id !== "org.sqlite-capsule" || value.source.format_version !== "0.2") throw new Error("Unsupported source capsule format");
    exactKeys(value.current, ["revision", "database_sha256", "parent_database_sha256", "uncompressed_bytes", "compressed_sha256", "compressed_bytes"], "current");
    if (!Number.isSafeInteger(value.current.revision) || value.current.revision < 1 || value.current.revision > 2147483647) throw new Error("current.revision is invalid");
    requireSha(value.current.database_sha256, "current.database_sha256");
    requireSha(value.current.compressed_sha256, "current.compressed_sha256");
    requireBytes(value.current.uncompressed_bytes, "current.uncompressed_bytes", MAX_CAPSULE_BYTES);
    requireBytes(value.current.compressed_bytes, "current.compressed_bytes", MAX_CAPSULE_BYTES);
    if (value.current.revision === 1) {
      if (value.current.parent_database_sha256 !== null || value.current.database_sha256 !== value.source.sha256 || value.current.uncompressed_bytes !== value.source.bytes) {
        throw new Error("Initial export lineage does not match its source capsule");
      }
    } else requireSha(value.current.parent_database_sha256, "current.parent_database_sha256");
    exactKeys(value.runtime, [
      "host_id", "host_version", "sqlite_package", "sqlite_version", "loader_sha256", "loader_bytes",
      "sqlite_js_sha256", "sqlite_js_compressed_sha256", "sqlite_js_bytes",
      "sqlite_wasm_sha256", "sqlite_wasm_compressed_sha256", "sqlite_wasm_bytes",
      "worker_sha256", "worker_compressed_sha256", "worker_bytes",
      "notices_sha256", "notices_bytes",
    ], "runtime");
    if (value.runtime.host_id !== "org.sqlite-capsule.browser-host" || value.runtime.host_version !== "0.2") throw new Error("Unsupported browser host identity/version");
    if (value.runtime.sqlite_package !== "@sqlite.org/sqlite-wasm" || value.runtime.sqlite_version !== "3.53.0-build1") throw new Error("Unsupported SQLite WASM build");
    for (const stem of ["loader", "sqlite_js", "sqlite_wasm", "worker"]) {
      requireSha(value.runtime[`${stem}_sha256`], `runtime.${stem}_sha256`);
      requireBytes(value.runtime[`${stem}_bytes`], `runtime.${stem}_bytes`);
      if (stem !== "loader") requireSha(value.runtime[`${stem}_compressed_sha256`], `runtime.${stem}_compressed_sha256`);
    }
    requireSha(value.runtime.notices_sha256, "runtime.notices_sha256");
    requireBytes(value.runtime.notices_bytes, "runtime.notices_bytes");
  }

  function base64ToBytes(text) {
    const compact = text.replace(/\s+/g, "");
    if (!compact || compact.length > Math.ceil(MAX_BLOCK_BYTES / 3) * 4 + 16) throw new Error("Embedded block is empty or exceeds the encoded-size limit");
    if (!/^[A-Za-z0-9+/]*={0,2}$/.test(compact) || compact.length % 4 !== 0) throw new Error("Embedded block is not canonical base64");
    const binary = atob(compact);
    if (binary.length > MAX_BLOCK_BYTES) throw new Error("Embedded block exceeds the compressed-size limit");
    const bytes = new Uint8Array(binary.length);
    for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
    return bytes;
  }

  function bytesToBase64(bytes) {
    let output = "";
    const chunk = 0x8000;
    for (let index = 0; index < bytes.length; index += chunk) {
      output += String.fromCharCode(...bytes.subarray(index, Math.min(index + chunk, bytes.length)));
    }
    return btoa(output).replace(/.{1,76}/g, "$&\n").trim();
  }

  async function sha256Hex(bytes) {
    if (!globalThis.crypto?.subtle) throw new Error("This browser does not provide SHA-256 through Web Crypto");
    const digest = await crypto.subtle.digest("SHA-256", bytes);
    return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
  }

  async function decompressGzip(compressed, expectedBytes, label) {
    if (typeof globalThis.DecompressionStream !== "function") throw new Error("This browser does not support gzip DecompressionStream");
    if (!Number.isInteger(expectedBytes) || expectedBytes < 1 || expectedBytes > MAX_BLOCK_BYTES) throw new Error(`${label} declares an invalid uncompressed size`);
    const stream = new Blob([compressed]).stream().pipeThrough(new DecompressionStream("gzip"));
    const reader = stream.getReader();
    const bytes = new Uint8Array(expectedBytes);
    let offset = 0;
    try {
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        if (!(value instanceof Uint8Array) || offset + value.byteLength > expectedBytes) {
          await reader.cancel();
          throw new Error(`${label} decompressed data exceeds its declared size`);
        }
        bytes.set(value, offset);
        offset += value.byteLength;
      }
    } finally {
      reader.releaseLock();
    }
    if (offset !== expectedBytes) throw new Error(`${label} decompressed size does not match metadata`);
    return bytes;
  }

  async function compressGzip(bytes) {
    if (typeof globalThis.CompressionStream !== "function") throw new Error("This browser does not support gzip CompressionStream");
    const stream = new Blob([bytes]).stream().pipeThrough(new CompressionStream("gzip"));
    return new Uint8Array(await new Response(stream).arrayBuffer());
  }

  function payloadElement(id) {
    const matches = document.querySelectorAll(`#${CSS.escape(id)}`);
    if (matches.length !== 1) throw new Error(`Expected exactly one #${id} block`);
    return matches[0];
  }

  async function loadPayload(id, compressedSha, rawSha, rawBytes, label, compressedBytes = null) {
    const compressed = base64ToBytes(payloadElement(id).textContent);
    if (compressedBytes !== null && compressed.byteLength !== compressedBytes) throw new Error(`${label} compressed size does not match metadata`);
    if (await sha256Hex(compressed) !== compressedSha) throw new Error(`${label} compressed SHA-256 mismatch`);
    const bytes = await decompressGzip(compressed, rawBytes, label);
    if (await sha256Hex(bytes) !== rawSha) throw new Error(`${label} SHA-256 mismatch`);
    return bytes;
  }

  function classicSqliteWorkerSource(sqliteSource, hostSource) {
    // Module-worker entry scripts are fetched with CORS semantics. That makes a
    // blob-backed module worker unreliable when its owning file:// document has
    // an opaque origin. The reviewed upstream artifact is an ES module, so turn
    // only its known module-only syntax into an equivalent classic worker after
    // the artifact digest has already been checked. Exact shape checks make an
    // upstream packaging change fail closed instead of broadening the rewrite.
    const importMeta = "import.meta.url";
    const exportFooter = "export { sqlite3_bundler_friendly_default as default, sqlite3_worker1_promiser_default as sqlite3Worker1Promiser };";
    const importMetaCount = sqliteSource.split(importMeta).length - 1;
    const exportFooterCount = sqliteSource.split(exportFooter).length - 1;
    if (importMetaCount !== 4 || exportFooterCount !== 1) {
      throw new Error("Pinned SQLite JavaScript has an unexpected module shape");
    }
    const classicSqlite = sqliteSource
      .replaceAll(importMeta, "self.location.href")
      .replace(exportFooter, "");
    return `${classicSqlite}\n${hostSource}`;
  }

  function sendWorker(type, fields = {}, transfer = []) {
    return new Promise((resolve, reject) => {
      const id = nextWorkerId++;
      pending.set(id, { resolve, reject });
      worker.postMessage({ id, type, ...fields }, transfer);
    });
  }

  function workerMessage(event) {
    const message = event.data || {};
    const waiter = pending.get(message.id);
    if (!waiter) return;
    pending.delete(message.id);
    if (message.ok) waiter.resolve(message.result);
    else waiter.reject(new Error(message.error || "Browser host worker failed"));
  }

  function workerFailed(event) {
    const error = new Error(event.message || "Browser host worker terminated unexpectedly");
    for (const waiter of pending.values()) waiter.reject(error);
    pending.clear();
    showError(error);
  }

  function resolveAssetPath(reference, entryPath) {
    if (typeof reference !== "string" || !reference || reference.startsWith("#") || reference.startsWith("data:") || reference.startsWith("blob:")) return null;
    if (/^[A-Za-z][A-Za-z0-9+.-]*:/.test(reference) || reference.startsWith("//")) throw new Error(`Exported application references a remote resource: ${reference}`);
    const withoutQuery = reference.split(/[?#]/, 1)[0];
    const parts = (withoutQuery.startsWith("/") ? withoutQuery.slice(1) : `${entryPath.slice(0, entryPath.lastIndexOf("/") + 1)}${withoutQuery}`).split("/");
    const resolved = [];
    for (const part of parts) {
      if (!part || part === ".") continue;
      if (part === "..") {
        if (!resolved.length) throw new Error(`Asset reference escapes capsule root: ${reference}`);
        resolved.pop();
      } else resolved.push(part);
    }
    return resolved.join("/");
  }

  function assetText(asset) {
    try { return decoder.decode(asset.content); }
    catch (_) { throw new Error(`Asset ${asset.path} is not valid UTF-8`); }
  }

  function dataUrl(asset) {
    return `data:${asset.media_type};base64,${bytesToBase64(asset.content).replace(/\s+/g, "")}`;
  }

  function bridgeSource(nonce) {
    return `(() => {\n` +
      `  const nonce = ${JSON.stringify(nonce)};\n` +
      `  let nextId = 1; const pending = new Map();\n` +
      `  addEventListener("message", (event) => {\n` +
      `    const message = event.data || {}; if (event.source !== parent || message.__sqliteCapsule !== true || message.nonce !== nonce || message.direction !== "response") return;\n` +
      `    const item = pending.get(message.id); if (!item) return; pending.delete(message.id); message.ok ? item.resolve(message.result) : item.reject(new Error(message.error || "Capsule host request failed"));\n` +
      `  });\n` +
      `  function request(method, args) { return new Promise((resolve, reject) => { const id = nextId++; pending.set(id, {resolve, reject}); parent.postMessage({__sqliteCapsule:true, nonce, direction:"request", id, method, args}, "*"); }); }\n` +
      `  globalThis.__sqliteCapsuleBridge = Object.freeze({\n` +
      `    manifest: () => request("manifest", {}), permissions: () => request("permissions", {}),\n` +
      `    read: (name, parameters = {}) => request("read", {name, parameters}),\n` +
      `    write: (name, parameters = {}) => request("write", {name, parameters})\n` +
      `  });\n` +
      `})();`;
  }

  function materialiseApplication(assets, manifest) {
    const assetMap = new Map(assets.map((asset) => [asset.path, asset]));
    const entry = assetMap.get(manifest.entry_asset);
    if (!entry) throw new Error(`Entry asset is unavailable: ${manifest.entry_asset}`);
    const documentText = assetText(entry);
    const parsed = new DOMParser().parseFromString(documentText, "text/html");
    if (parsed.querySelector("parsererror")) throw new Error("Entry asset is not valid HTML");
    parsed.querySelectorAll("base").forEach((node) => node.remove());
    const csp = parsed.createElement("meta");
    csp.httpEquiv = "Content-Security-Policy";
    csp.content = "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; media-src data: blob:; font-src data:; connect-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'";
    parsed.head.prepend(csp);
    const bridge = parsed.createElement("script");
    bridge.textContent = bridgeSource(appNonce);
    parsed.head.append(bridge);

    for (const link of Array.from(parsed.querySelectorAll("link[href]"))) {
      const path = resolveAssetPath(link.getAttribute("href"), manifest.entry_asset);
      if (!path) continue;
      const asset = assetMap.get(path);
      if (!asset) throw new Error(`Entry asset references missing capsule asset: ${path}`);
      if ((link.getAttribute("rel") || "").toLowerCase() !== "stylesheet") throw new Error(`Unsupported local link relation for ${path}`);
      const style = parsed.createElement("style");
      style.dataset.capsuleAsset = path;
      style.textContent = assetText(asset);
      link.replaceWith(style);
    }
    for (const script of Array.from(parsed.querySelectorAll("script[src]"))) {
      const path = resolveAssetPath(script.getAttribute("src"), manifest.entry_asset);
      const asset = path && assetMap.get(path);
      if (!asset) throw new Error(`Entry asset references missing capsule script: ${path || script.getAttribute("src")}`);
      const source = assetText(asset);
      if (/<\/script/i.test(source)) throw new Error(`Script ${path} cannot be safely materialised inline`);
      const inline = parsed.createElement("script");
      inline.dataset.capsuleAsset = path;
      if (script.type) inline.type = script.type;
      inline.textContent = source;
      script.replaceWith(inline);
    }
    for (const node of Array.from(parsed.querySelectorAll("[src], [poster]"))) {
      for (const attribute of ["src", "poster"]) {
        if (!node.hasAttribute(attribute)) continue;
        const path = resolveAssetPath(node.getAttribute(attribute), manifest.entry_asset);
        if (!path) continue;
        const asset = assetMap.get(path);
        if (!asset) throw new Error(`Entry asset references missing media asset: ${path}`);
        node.setAttribute(attribute, dataUrl(asset));
      }
    }
    const html = `<!doctype html>\n${parsed.documentElement.outerHTML}`;
    // srcdoc remains self-contained and works from both opaque file origins and
    // static hosting. The sandbox omits allow-same-origin, so capsule code cannot
    // reach or restyle the host identity/save shell.
    elements.frame.removeAttribute("src");
    elements.frame.srcdoc = html;
  }

  async function handleAppRequest(message, source) {
    if (source !== elements.frame.contentWindow || message.__sqliteCapsule !== true || message.nonce !== appNonce || message.direction !== "request") return;
    const allowedFields = new Set(["__sqliteCapsule", "nonce", "direction", "id", "method", "args"]);
    const response = { __sqliteCapsule: true, nonce: appNonce, direction: "response", id: message.id };
    let counted = false;
    try {
      if (Object.keys(message).some((key) => !allowedFields.has(key))) throw new Error("Unknown bridge message field");
      if (!Number.isSafeInteger(message.id) || message.id < 1 || message.id <= lastAppRequestId) throw new Error("Bridge request id must be positive and strictly increasing");
      lastAppRequestId = message.id;
      if (typeof message.method !== "string") throw new Error("Bridge method must be a string");
      if (!message.args || typeof message.args !== "object" || Array.isArray(message.args)) throw new Error("Bridge arguments must be an object");
      if (["manifest", "permissions"].includes(message.method)) exactKeys(message.args, [], `${message.method} arguments`);
      if (["read", "write"].includes(message.method)) {
        exactKeys(message.args, ["name", "parameters"], `${message.method} arguments`);
        if (typeof message.args.name !== "string" || !message.args.name || message.args.name.length > 256) throw new Error("Endpoint name must be a non-empty string of at most 256 characters");
        if (!message.args.parameters || typeof message.args.parameters !== "object" || Array.isArray(message.args.parameters)) throw new Error("Endpoint parameters must be an object");
        if (activeAppRequests >= MAX_CONCURRENT_REQUESTS) throw new Error(`Too many concurrent requests; limit is ${MAX_CONCURRENT_REQUESTS}`);
        activeAppRequests += 1;
        counted = true;
      }
      let result;
      if (message.method === "manifest") result = runtimeState.manifest;
      else if (message.method === "permissions") result = runtimeState.permissions;
      else if (message.method === "read") result = await sendWorker("read", { name: message.args.name, parameters: message.args.parameters });
      else if (message.method === "write") {
        result = await sendWorker("write", { name: message.args.name, parameters: message.args.parameters });
        dirtyGeneration += 1;
        setDirty(true);
      } else throw new Error(`Unknown capsule bridge method: ${message.method}`);
      source.postMessage({ ...response, ok: true, result }, "*");
    } catch (error) {
      source.postMessage({ ...response, ok: false, error: String(error?.message || error) }, "*");
    } finally {
      if (counted) activeAppRequests -= 1;
    }
  }

  function setDirty(value) {
    dirty = Boolean(value);
    document.body.dataset.dirty = dirty ? "true" : "false";
    if (metadata.profile === "editable") {
      elements.save.disabled = !dirty;
      elements.download.disabled = !dirty;
      setStatus(dirty ? "Unsaved HTML revision" : "Verified and saved", dirty ? "dirty" : "ready");
    }
  }

  function suggestedFilename(nextMetadata) {
    const app = String(nextMetadata.source.app_id || "capsule").split(".").pop().replace(/[^A-Za-z0-9._-]+/g, "-");
    return `${app}-${nextMetadata.profile}-r${nextMetadata.current.revision}.html`;
  }

  function cleanRevisionHtml(nextMetadata, databaseCompressed) {
    payloadElement("sqlite-capsule-export-metadata").textContent = safeMetadataText(nextMetadata);
    payloadElement("sqlite-capsule-database").textContent = `\n${bytesToBase64(databaseCompressed)}\n`;
    const clone = document.documentElement.cloneNode(true);
    const frame = clone.querySelector("#capsule-app-frame");
    frame.removeAttribute("src");
    frame.removeAttribute("srcdoc");
    frame.setAttribute("src", "about:blank");
    const error = clone.querySelector("#capsule-host-error");
    error.hidden = true;
    error.textContent = "";
    const notices = clone.querySelector("#capsule-third-party-panel");
    notices.hidden = true;
    clone.querySelector("#capsule-third-party-text").textContent = "";
    const status = clone.querySelector("#capsule-host-status");
    status.textContent = "Opening embedded SQLite capsule";
    status.dataset.state = "loading";
    clone.querySelector("#capsule-save-html").disabled = true;
    clone.querySelector("#capsule-download-html").disabled = true;
    clone.querySelector("body").dataset.dirty = "false";
    return `<!doctype html>\n${clone.outerHTML}\n`;
  }

  async function prepareRevision() {
    if (metadata.profile !== "editable") throw new Error("This export profile is read-only");
    const exported = await sendWorker("export");
    const databaseBytes = new Uint8Array(exported.bytes);
    if (Number.isSafeInteger(exported.memory?.wasm_heap_bytes)) {
      document.body.dataset.saveWasmHeapBytes = String(exported.memory.wasm_heap_bytes);
    }
    if (await sha256Hex(databaseBytes) !== exported.sha256) throw new Error("Serialised database digest mismatch");
    const compressed = await compressGzip(databaseBytes);
    const next = structuredClone(metadata);
    next.created_at = new Date().toISOString();
    next.current = {
      revision: metadata.current.revision + 1,
      database_sha256: exported.sha256,
      parent_database_sha256: metadata.current.database_sha256,
      uncompressed_bytes: databaseBytes.byteLength,
      compressed_sha256: await sha256Hex(compressed),
      compressed_bytes: compressed.byteLength,
    };
    return { metadata: next, compressed, html: cleanRevisionHtml(next, compressed), dirtyGeneration };
  }

  function downloadRevision(revision) {
    const url = URL.createObjectURL(new Blob([revision.html], { type: "text/html;charset=utf-8" }));
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = suggestedFilename(revision.metadata);
    anchor.rel = "noopener";
    document.body.append(anchor);
    anchor.click();
    anchor.remove();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }

  function acceptSavedRevision(revision, status) {
    metadata = revision.metadata;
    setDirty(revision.dirtyGeneration !== dirtyGeneration);
    elements.provenance.textContent = `Source ${metadata.source.sha256.slice(0, 12)} · revision ${metadata.current.revision} · ${metadata.current.database_sha256.slice(0, 12)}`;
    setStatus(dirty ? `${status}; newer changes remain unsaved` : status, dirty ? "dirty" : "ready");
  }

  async function saveRevision(preferPicker) {
    const saveStarted = performance.now();
    try {
      setStatus("Preparing verified HTML revision", "saving");
      const revision = await prepareRevision();
      if (preferPicker && globalThis.isSecureContext && typeof globalThis.showSaveFilePicker === "function") {
        const handle = await showSaveFilePicker({
          suggestedName: suggestedFilename(revision.metadata),
          types: [{ description: "Self-contained SQLite Capsule HTML", accept: { "text/html": [".html"] } }],
        });
        const writable = await handle.createWritable();
        await writable.write(new Blob([revision.html], { type: "text/html;charset=utf-8" }));
        await writable.close();
        acceptSavedRevision(revision, "HTML revision saved");
      } else {
        downloadRevision(revision);
        acceptSavedRevision(revision, "HTML revision downloaded");
      }
    } catch (error) {
      if (error?.name === "AbortError") setStatus("Save cancelled; revision remains unsaved", "dirty");
      else {
        setStatus("Save failed; revision remains unsaved", "error");
        showError(error);
      }
    } finally {
      document.body.dataset.saveMilliseconds = String(Math.round((performance.now() - saveStarted) * 10) / 10);
    }
  }

  async function boot() {
    setStatus("Checking export envelope", "loading");
    if (typeof globalThis.Worker !== "function") throw new Error("This browser does not support dedicated Web Workers");
    const metadataElement = payloadElement("sqlite-capsule-export-metadata");
    metadata = JSON.parse(metadataElement.textContent);
    validateMetadata(metadata);
    const loaderBytes = encoder.encode(payloadElement("sqlite-capsule-loader").textContent);
    if (loaderBytes.byteLength !== metadata.runtime.loader_bytes || await sha256Hex(loaderBytes) !== metadata.runtime.loader_sha256) throw new Error("Browser loader digest/size mismatch");
    const noticesBytes = encoder.encode(payloadElement("sqlite-capsule-third-party-notices").textContent);
    if (noticesBytes.byteLength !== metadata.runtime.notices_bytes || await sha256Hex(noticesBytes) !== metadata.runtime.notices_sha256) throw new Error("Third-party notices digest/size mismatch");
    elements.noticesButton.addEventListener("click", () => {
      elements.noticesText.textContent = payloadElement("sqlite-capsule-third-party-notices").textContent;
      elements.noticesPanel.hidden = false;
      elements.noticesClose.focus();
    });
    elements.noticesClose.addEventListener("click", () => {
      elements.noticesPanel.hidden = true;
      elements.noticesText.textContent = "";
      elements.noticesButton.focus();
    });

    const current = metadata.current;
    const runtime = metadata.runtime;
    const [databaseBytes, sqliteJsBytes, wasmBytes, workerBytes] = await Promise.all([
      loadPayload("sqlite-capsule-database", current.compressed_sha256, current.database_sha256, current.uncompressed_bytes, "database", current.compressed_bytes),
      loadPayload("sqlite-capsule-sqlite-js", runtime.sqlite_js_compressed_sha256, runtime.sqlite_js_sha256, runtime.sqlite_js_bytes, "SQLite JavaScript"),
      loadPayload("sqlite-capsule-sqlite-wasm", runtime.sqlite_wasm_compressed_sha256, runtime.sqlite_wasm_sha256, runtime.sqlite_wasm_bytes, "SQLite WASM"),
      loadPayload("sqlite-capsule-worker", runtime.worker_compressed_sha256, runtime.worker_sha256, runtime.worker_bytes, "browser worker"),
    ]);

    setStatus("Starting isolated SQLite WASM host", "loading");
    const workerSource = classicSqliteWorkerSource(decoder.decode(sqliteJsBytes), decoder.decode(workerBytes));
    workerUrl = URL.createObjectURL(new Blob([workerSource], { type: "text/javascript" }));
    worker = new Worker(workerUrl, { name: "sqlite-capsule-browser-host" });
    worker.addEventListener("message", workerMessage);
    worker.addEventListener("error", workerFailed);
    runtimeState = await sendWorker("init", {
      profile: metadata.profile,
      metadata,
      database: databaseBytes.buffer,
      wasm: wasmBytes.buffer,
    }, [databaseBytes.buffer, wasmBytes.buffer]);
    document.body.dataset.databaseBytes = String(runtimeState.memory.database_bytes);
    document.body.dataset.wasmHeapBytes = String(runtimeState.memory.wasm_heap_bytes);

    const assets = await sendWorker("assets");
    appNonce = Array.from(crypto.getRandomValues(new Uint8Array(24)), (byte) => byte.toString(16).padStart(2, "0")).join("");
    window.addEventListener("message", (event) => { void handleAppRequest(event.data || {}, event.source); });
    materialiseApplication(assets, runtimeState.manifest);

    elements.identity.textContent = runtimeState.manifest.title;
    elements.profile.textContent = metadata.profile;
    elements.profile.dataset.profile = metadata.profile;
    elements.provenance.textContent = `Source ${metadata.source.sha256.slice(0, 12)} · revision ${metadata.current.revision} · ${metadata.current.database_sha256.slice(0, 12)}`;
    document.title = `${runtimeState.manifest.title} — ${metadata.profile} export`;
    if (metadata.profile === "editable") {
      elements.save.hidden = false;
      elements.download.hidden = false;
      elements.save.textContent = globalThis.isSecureContext && typeof globalThis.showSaveFilePicker === "function" ? "Save HTML" : "Save HTML (download)";
      elements.save.addEventListener("click", () => { void saveRevision(true); });
      elements.download.addEventListener("click", () => { void saveRevision(false); });
      window.addEventListener("beforeunload", (event) => {
        if (!dirty) return;
        event.preventDefault();
        event.returnValue = "";
      });
    }
    setDirty(false);
    setStatus(`Verified with SQLite ${runtimeState.sqlite_version}`, "ready");
    document.body.dataset.bootMilliseconds = String(Math.round((performance.now() - bootStarted) * 10) / 10);
  }

  window.addEventListener("unload", () => {
    try { worker?.terminate(); } catch (_) {}
    if (workerUrl) URL.revokeObjectURL(workerUrl);
    if (appUrl) URL.revokeObjectURL(appUrl);
  });

  void boot().catch(showError);
})();

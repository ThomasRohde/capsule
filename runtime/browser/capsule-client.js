/* Host-neutral SQLite Capsule application client.
 *
 * The browser HTML host injects __sqliteCapsuleBridge into the sandboxed app.
 * The loopback host requires no injection and uses capsule-http/0.2.
 */
(() => {
  "use strict";

  let sessionToken = null;
  let cachedManifest = null;
  let nativeSession = undefined;
  let nativeSequence = 0;
  let nativeRequestTail = Promise.resolve();
  const NATIVE_UNAVAILABLE = Symbol("native-unavailable");

  function bridge() {
    const value = globalThis.__sqliteCapsuleBridge;
    return value && typeof value === "object" ? value : null;
  }

  async function responseJson(response, fallback) {
    let body;
    try { body = await response.json(); }
    catch (_) { throw new Error(fallback); }
    if (!response.ok || !body?.ok) {
      const message = typeof body?.error === "string" ? body.error : body?.error?.message;
      throw new Error(message || fallback);
    }
    return body;
  }

  async function discoverNativeSession() {
    if (nativeSession !== undefined) return nativeSession;
    try {
      const response = await fetch("/__capsule/native-session", { cache: "no-store" });
      if (!response.ok) {
        nativeSession = null;
        return null;
      }
      const body = await responseJson(response, "Could not initialise native capsule session");
      if (body.version !== 1 || typeof body.session !== "string") {
        throw new Error("Native capsule session is malformed");
      }
      nativeSession = body.session;
      return nativeSession;
    } catch (_) {
      nativeSession = null;
      return null;
    }
  }

  async function nativeRequestInOrder(method, params) {
    const session = await discoverNativeSession();
    if (!session) return NATIVE_UNAVAILABLE;
    const sequence = ++nativeSequence;
    const id = `native-${sequence}`;
    const response = await fetch("/__capsule/rpc", {
      method: "POST",
      cache: "no-store",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        version: 1,
        session,
        sequence,
        id,
        method,
        params,
      }),
    });
    const body = await responseJson(response, `Native ${method} request failed`);
    if (body.version !== 1 || body.id !== id || body.sequence !== sequence) {
      throw new Error("Native capsule response does not match the request");
    }
    return body.result;
  }

  function nativeRequest(method, params) {
    // The native bridge deliberately accepts one strictly increasing sequence.
    // Application boot may issue named reads concurrently, so serialize only
    // the native transport while leaving browser and loopback hosts unchanged.
    const current = nativeRequestTail.then(
      () => nativeRequestInOrder(method, params),
      () => nativeRequestInOrder(method, params),
    );
    nativeRequestTail = current.catch(() => undefined);
    return current;
  }

  async function manifest() {
    const injected = bridge();
    if (injected) {
      cachedManifest = await injected.manifest();
      return cachedManifest;
    }
    const native = await nativeRequest("manifest", {});
    if (native !== NATIVE_UNAVAILABLE) {
      cachedManifest = native;
      return cachedManifest;
    }
    const response = await fetch("/__capsule/manifest", { cache: "no-store" });
    const body = await responseJson(response, "Could not read capsule manifest");
    sessionToken = body.session_token;
    cachedManifest = body.manifest;
    return cachedManifest;
  }

  async function permissions() {
    const injected = bridge();
    if (injected) return injected.permissions();
    const native = await nativeRequest("permissions", {});
    if (native !== NATIVE_UNAVAILABLE) return native;
    if (!sessionToken) await manifest();
    const response = await fetch("/__capsule/permissions", {
      cache: "no-store",
      headers: { "X-Capsule-Token": sessionToken },
    });
    return responseJson(response, "Could not read capsule permissions");
  }

  async function read(name, parameters = {}) {
    const injected = bridge();
    if (injected) return injected.read(name, parameters);
    const native = await nativeRequest("read", { endpoint: name, arguments: parameters });
    if (native !== NATIVE_UNAVAILABLE) return native;
    if (!sessionToken) await manifest();
    const url = new URL(`/__capsule/read/${encodeURIComponent(name)}`, globalThis.location.origin);
    for (const [key, value] of Object.entries(parameters)) url.searchParams.set(key, String(value));
    const response = await fetch(url, {
      cache: "no-store",
      headers: { "X-Capsule-Token": sessionToken },
    });
    const body = await responseJson(response, `Read endpoint ${name} failed`);
    return body.result;
  }

  async function write(name, parameters = {}) {
    const injected = bridge();
    if (injected) return injected.write(name, parameters);
    const native = await nativeRequest("write", { endpoint: name, arguments: parameters });
    if (native !== NATIVE_UNAVAILABLE) return native;
    if (!sessionToken) await manifest();
    const response = await fetch(`/__capsule/write/${encodeURIComponent(name)}`, {
      method: "POST",
      cache: "no-store",
      headers: {
        "Content-Type": "application/json",
        "X-Capsule-Token": sessionToken,
      },
      body: JSON.stringify(parameters),
    });
    const body = await responseJson(response, `Write endpoint ${name} failed`);
    return body.result;
  }

  globalThis.SQLiteCapsuleClient = Object.freeze({
    manifest,
    permissions,
    read,
    write,
    get hostKind() {
      return bridge() ? "browser-html" : nativeSession ? "native-custom-protocol" : "loopback-http";
    },
    get profile() { return cachedManifest?.export_profile || "editable"; },
    get provenance() { return cachedManifest?.export_provenance || null; },
  });
})();

#!/usr/bin/env python3
"""Export, inspect, and verify self-contained SQLite Capsule HTML envelopes."""

from __future__ import annotations

import argparse
import base64
import gzip
import hashlib
import html
from html.parser import HTMLParser
import io
import json
import os
from pathlib import Path
import posixpath
import re
import tempfile
from typing import Any, Iterable

from runtime.capsule_host import CapsuleDatabase


ROOT = Path(__file__).resolve().parents[1]
CONTRACT_ID = "org.sqlite-capsule.html-export"
CONTRACT_VERSION = "0.2"
HOST_ID = "org.sqlite-capsule.browser-host"
HOST_VERSION = "0.2"
SQLITE_PACKAGE = "@sqlite.org/sqlite-wasm"
SQLITE_VERSION = "3.53.0-build1"
PROFILES = ("view", "interactive", "editable")
MAX_CAPSULE_BYTES = 64 * 1024 * 1024
MAX_RUNTIME_BYTES = 64 * 1024 * 1024
MAX_HTML_BYTES = 192 * 1024 * 1024
HEX64 = re.compile(r"^[0-9a-f]{64}$")
BLOCK_IDS = (
    "sqlite-capsule-export-metadata",
    "sqlite-capsule-database",
    "sqlite-capsule-sqlite-js",
    "sqlite-capsule-sqlite-wasm",
    "sqlite-capsule-worker",
    "sqlite-capsule-third-party-notices",
    "sqlite-capsule-loader",
)
VENDOR_DIR = ROOT / "runtime" / "browser" / "vendor" / "sqlite-wasm"
SQLITE_JS_PATH = VENDOR_DIR / "index.mjs"
SQLITE_WASM_PATH = VENDOR_DIR / "sqlite3.wasm"
WORKER_PATH = ROOT / "runtime" / "browser" / "worker-host.js"
LOADER_PATH = ROOT / "runtime" / "browser" / "loader.js"
NOTICE_PATH = VENDOR_DIR / "THIRD_PARTY.md"
LICENSE_PATH = VENDOR_DIR / "LICENSE.Apache-2.0.txt"


class HtmlExportError(RuntimeError):
    """Raised when an HTML export is malformed, unsafe, or unverifiable."""


class _AssetReferenceParser(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.references: list[tuple[str, str, str]] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        values = dict(attrs)
        if tag == "script" and values.get("src"):
            self.references.append((tag, "src", str(values["src"])))
        if tag == "link" and "stylesheet" in str(values.get("rel", "")).lower().split() and values.get("href"):
            self.references.append((tag, "href", str(values["href"])))
        if tag not in {"script", "link"}:
            for attribute in ("src", "poster"):
                if values.get(attribute):
                    self.references.append((tag, attribute, str(values[attribute])))


def _sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _json_dump(value: Any, *, indent: int | None = None) -> str:
    return json.dumps(
        value,
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":") if indent is None else None,
        indent=indent,
        allow_nan=False,
    )


def _safe_script_json(value: Any) -> str:
    return (
        _json_dump(value)
        .replace("&", "\\u0026")
        .replace("<", "\\u003c")
        .replace(">", "\\u003e")
        .replace("\u2028", "\\u2028")
        .replace("\u2029", "\\u2029")
    )


def _gzip(data: bytes) -> bytes:
    buffer = io.BytesIO()
    with gzip.GzipFile(filename="", mode="wb", fileobj=buffer, compresslevel=9, mtime=0) as stream:
        stream.write(data)
    return buffer.getvalue()


def _gunzip_limited(data: bytes, expected_size: int, label: str) -> bytes:
    if not 0 < expected_size <= MAX_RUNTIME_BYTES:
        raise HtmlExportError(f"{label} declares an invalid uncompressed size")
    try:
        with gzip.GzipFile(fileobj=io.BytesIO(data), mode="rb") as stream:
            result = stream.read(expected_size + 1)
    except (OSError, EOFError) as exc:
        raise HtmlExportError(f"{label} is not valid gzip: {exc}") from exc
    if len(result) != expected_size:
        raise HtmlExportError(
            f"{label} decompressed to {len(result)} bytes; expected {expected_size}"
        )
    return result


def _base64(data: bytes) -> str:
    return base64.encodebytes(data).decode("ascii").strip()


def _decode_base64(value: str, label: str) -> bytes:
    compact = "".join(value.split())
    if not compact or len(compact) > ((MAX_RUNTIME_BYTES + 2) // 3) * 4 + 16:
        raise HtmlExportError(f"{label} is empty or exceeds the encoded-size limit")
    try:
        return base64.b64decode(compact, validate=True)
    except (ValueError, base64.binascii.Error) as exc:
        raise HtmlExportError(f"{label} is not canonical base64") from exc


def _runtime_inputs() -> dict[str, bytes]:
    paths = {
        "sqlite_js": SQLITE_JS_PATH,
        "sqlite_wasm": SQLITE_WASM_PATH,
        "worker": WORKER_PATH,
        "loader": LOADER_PATH,
        "notices": NOTICE_PATH,
        "license": LICENSE_PATH,
    }
    missing = [str(path) for path in paths.values() if not path.is_file()]
    if missing:
        raise HtmlExportError("Missing browser runtime inputs: " + ", ".join(missing))
    values = {name: path.read_bytes() for name, path in paths.items()}
    values["notices"] = values["notices"] + b"\n\n" + values.pop("license")
    return values


def _resolve_asset_reference(reference: str, entry_path: str) -> str | None:
    if reference.startswith(("#", "data:", "blob:")):
        return None
    if re.match(r"^[A-Za-z][A-Za-z0-9+.-]*:", reference) or reference.startswith("//"):
        raise HtmlExportError(f"Entry asset references a remote resource: {reference}")
    without_suffix = re.split(r"[?#]", reference, maxsplit=1)[0]
    if not without_suffix:
        return None
    candidate = without_suffix.lstrip("/") if without_suffix.startswith("/") else posixpath.join(posixpath.dirname(entry_path), without_suffix)
    normalised = posixpath.normpath(candidate)
    if normalised == ".." or normalised.startswith("../") or normalised.startswith("/"):
        raise HtmlExportError(f"Entry asset reference escapes the capsule root: {reference}")
    return normalised


def _exportability_report(capsule: CapsuleDatabase, manifest: dict[str, Any]) -> dict[str, Any]:
    inventory = {item["path"]: item for item in capsule.list_assets()}
    entry = capsule.asset(manifest["entry_asset"])
    if entry is None:
        raise HtmlExportError(f"Entry asset is missing: {manifest['entry_asset']}")
    try:
        entry_text = entry.content.decode("utf-8")
    except UnicodeDecodeError as exc:
        raise HtmlExportError("Entry asset is not UTF-8 HTML") from exc
    parser = _AssetReferenceParser()
    parser.feed(entry_text)
    parser.close()
    resolved: list[str] = []
    for _, _, reference in parser.references:
        path = _resolve_asset_reference(reference, manifest["entry_asset"])
        if path is None:
            continue
        if path not in inventory:
            raise HtmlExportError(f"Entry asset references missing capsule asset: {path}")
        resolved.append(path)
    dynamic_tokens: dict[str, list[str]] = {}
    token_patterns = {
        "fetch": re.compile(r"\bfetch\s*\("),
        "xml_http_request": re.compile(r"\bXMLHttpRequest\b"),
        "web_socket": re.compile(r"\bWebSocket\b"),
        "dynamic_import": re.compile(r"\bimport\s*\("),
    }
    for path in sorted(set(resolved)):
        asset = capsule.asset(path)
        if asset is None or "javascript" not in asset.media_type:
            continue
        try:
            source = asset.content.decode("utf-8")
        except UnicodeDecodeError:
            continue
        hits = sorted(name for name, pattern in token_patterns.items() if pattern.search(source))
        if hits:
            dynamic_tokens[path] = hits
    return {
        "ok": True,
        "entry_asset": manifest["entry_asset"],
        "static_references": len(parser.references),
        "resolved_capsule_assets": sorted(set(resolved)),
        "dynamic_network_tokens": dynamic_tokens,
        "runtime_network_policy": "blocked-by-export-csp",
        "note": "Dynamic tokens are reported, not followed; the browser bridge and CSP remain authoritative.",
    }


def _metadata(
    capsule_bytes: bytes,
    compressed_database: bytes,
    manifest: dict[str, Any],
    profile: str,
    runtime: dict[str, bytes],
) -> dict[str, Any]:
    compressed_js = _gzip(runtime["sqlite_js"])
    compressed_wasm = _gzip(runtime["sqlite_wasm"])
    compressed_worker = _gzip(runtime["worker"])
    return {
        "contract_id": CONTRACT_ID,
        "contract_version": CONTRACT_VERSION,
        "profile": profile,
        "created_at": manifest["updated_at"],
        "source": {
            "sha256": _sha256(capsule_bytes),
            "bytes": len(capsule_bytes),
            "capsule_id": manifest["capsule_id"],
            "format_id": manifest["format_id"],
            "format_version": manifest["format_version"],
            "runtime_protocol": manifest["runtime_protocol"],
            "app_id": manifest["app_id"],
            "app_version": manifest["app_version"],
            "title": manifest["title"],
        },
        "current": {
            "revision": 1,
            "database_sha256": _sha256(capsule_bytes),
            "parent_database_sha256": None,
            "uncompressed_bytes": len(capsule_bytes),
            "compressed_sha256": _sha256(compressed_database),
            "compressed_bytes": len(compressed_database),
        },
        "runtime": {
            "host_id": HOST_ID,
            "host_version": HOST_VERSION,
            "sqlite_package": SQLITE_PACKAGE,
            "sqlite_version": SQLITE_VERSION,
            "loader_sha256": _sha256(runtime["loader"]),
            "loader_bytes": len(runtime["loader"]),
            "sqlite_js_sha256": _sha256(runtime["sqlite_js"]),
            "sqlite_js_compressed_sha256": _sha256(compressed_js),
            "sqlite_js_bytes": len(runtime["sqlite_js"]),
            "sqlite_wasm_sha256": _sha256(runtime["sqlite_wasm"]),
            "sqlite_wasm_compressed_sha256": _sha256(compressed_wasm),
            "sqlite_wasm_bytes": len(runtime["sqlite_wasm"]),
            "worker_sha256": _sha256(runtime["worker"]),
            "worker_compressed_sha256": _sha256(compressed_worker),
            "worker_bytes": len(runtime["worker"]),
            "notices_sha256": _sha256(runtime["notices"]),
            "notices_bytes": len(runtime["notices"]),
        },
    }


SHELL_CSS = """
:root{color-scheme:dark;font-family:Inter,ui-sans-serif,system-ui,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;background:#07101f;color:#e6edf7}
*{box-sizing:border-box}html,body{height:100%;margin:0;overflow:hidden}body{display:grid;grid-template-rows:auto 1fr;background:#07101f}
.capsule-host-bar{position:relative;z-index:2147483647;display:grid;grid-template-columns:minmax(12rem,1fr) auto minmax(18rem,1fr);align-items:center;gap:.85rem;min-height:48px;padding:.5rem .75rem;border-bottom:1px solid #29415f;background:linear-gradient(180deg,#14233a,#0d192a);box-shadow:0 4px 16px #0008}
.capsule-host-brand{display:flex;align-items:center;gap:.65rem;min-width:0}.capsule-host-mark{display:grid;place-items:center;width:30px;height:30px;border:1px solid #4b79ad;border-radius:9px;background:#0b1728;color:#7dd3fc;font-weight:800}.capsule-host-copy{min-width:0}.capsule-host-copy strong,.capsule-host-copy span{display:block;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.capsule-host-copy strong{font-size:.82rem}.capsule-host-copy span{font-size:.68rem;color:#9db0c8}
.capsule-host-meta{display:flex;align-items:center;justify-content:center;gap:.5rem}.capsule-chip{padding:.25rem .5rem;border:1px solid #416181;border-radius:999px;background:#10213a;color:#bae6fd;font-size:.68rem;font-weight:750;text-transform:uppercase;letter-spacing:.06em}.capsule-chip[data-profile="editable"]{border-color:#2f7d67;color:#a7f3d0}.capsule-chip[data-profile="view"]{border-color:#665188;color:#ddd6fe}
.capsule-host-actions{display:flex;align-items:center;justify-content:flex-end;gap:.5rem;min-width:0}.capsule-host-actions button{appearance:none;border:1px solid #416181;border-radius:7px;background:#152945;color:#eef6ff;padding:.4rem .65rem;font:700 .72rem inherit;cursor:pointer}.capsule-host-actions button:hover:not(:disabled){background:#1e3a5f}.capsule-host-actions button:disabled{opacity:.45;cursor:default}.capsule-host-provenance{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;color:#9db0c8;font-size:.67rem;max-width:26rem}
#capsule-host-status[data-state="ready"]{color:#a7f3d0}#capsule-host-status[data-state="dirty"]{color:#fde68a}#capsule-host-status[data-state="error"]{color:#fca5a5}#capsule-host-status[data-state="saving"]{color:#bae6fd}
.capsule-frame{width:100%;height:100%;border:0;background:#07101f}.capsule-error{position:fixed;z-index:2147483647;inset:4.5rem 1rem auto 1rem;max-width:54rem;margin:auto;padding:1rem 1.1rem;border:1px solid #b94455;border-radius:10px;background:#35141b;color:#fecdd3;box-shadow:0 14px 44px #000b;white-space:pre-wrap}.capsule-error[hidden]{display:none}
.capsule-notices{position:fixed;z-index:2147483647;inset:4rem 1rem 1rem;max-width:70rem;margin:auto;padding:1rem;border:1px solid #416181;border-radius:10px;background:#0d192af5;box-shadow:0 14px 44px #000b}.capsule-notices[hidden]{display:none}.capsule-notices header{display:flex;align-items:center;justify-content:space-between;gap:1rem}.capsule-notices h2{margin:0;font-size:1rem}.capsule-notices pre{height:calc(100% - 3rem);overflow:auto;white-space:pre-wrap;color:#cbd5e1;font:11px/1.45 ui-monospace,monospace}.capsule-notices button{border:1px solid #416181;border-radius:7px;background:#152945;color:#eef6ff;padding:.4rem .65rem;cursor:pointer}
@media(max-width:900px){.capsule-host-bar{grid-template-columns:minmax(9rem,1fr) auto}.capsule-host-meta{display:none}.capsule-host-actions{gap:.3rem}.capsule-host-provenance{display:none}.capsule-host-actions button{padding:.35rem .5rem}}
""".strip()


def _render_html(metadata: dict[str, Any], capsule_gzip: bytes, runtime: dict[str, bytes]) -> bytes:
    sqlite_js_gzip = _gzip(runtime["sqlite_js"])
    sqlite_wasm_gzip = _gzip(runtime["sqlite_wasm"])
    worker_gzip = _gzip(runtime["worker"])
    loader = runtime["loader"].decode("utf-8")
    if "</script" in loader.lower():
        raise HtmlExportError("Browser loader contains an unsafe closing script sequence")
    if b"</script" in runtime["notices"].lower():
        raise HtmlExportError("Third-party notices contain an unsafe closing script sequence")
    title = html.escape(str(metadata["source"]["title"]), quote=True)
    profile = html.escape(str(metadata["profile"]), quote=True)
    source_short = metadata["source"]["sha256"][:12]
    current_short = metadata["current"]["database_sha256"][:12]
    output = f"""<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <meta name="color-scheme" content="dark">
  <meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline' blob: 'wasm-unsafe-eval'; worker-src blob:; frame-src blob:; style-src 'unsafe-inline'; img-src data: blob:; media-src data: blob:; font-src data:; connect-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'">
  <meta name="sqlite-capsule-contract" content="{CONTRACT_ID}/{CONTRACT_VERSION}">
  <title>{title} — {profile} export</title>
  <style>{SHELL_CSS}</style>
</head>
<body data-dirty="false">
  <header class="capsule-host-bar" aria-label="SQLite Capsule export identity">
    <div class="capsule-host-brand"><span class="capsule-host-mark" aria-hidden="true">DB</span><div class="capsule-host-copy"><strong id="capsule-host-identity">{title}</strong><span id="capsule-host-status" data-state="loading">Opening embedded SQLite capsule</span></div></div>
    <div class="capsule-host-meta"><span id="capsule-host-profile" class="capsule-chip" data-profile="{profile}">{profile}</span></div>
    <div class="capsule-host-actions"><span id="capsule-host-provenance" class="capsule-host-provenance">Source {source_short} · revision 1 · {current_short}</span><button id="capsule-third-party-button" type="button">Licenses</button><button id="capsule-save-html" type="button" hidden disabled>Save HTML</button><button id="capsule-download-html" type="button" hidden disabled>Download HTML</button></div>
  </header>
  <iframe id="capsule-app-frame" class="capsule-frame" title="Exported capsule application" sandbox="allow-scripts allow-downloads allow-forms allow-modals allow-pointer-lock allow-presentation" allow="fullscreen; clipboard-read; clipboard-write" src="about:blank"></iframe>
  <div id="capsule-host-error" class="capsule-error" role="alert" hidden></div>
  <section id="capsule-third-party-panel" class="capsule-notices" role="dialog" aria-modal="true" aria-labelledby="capsule-third-party-title" hidden><header><h2 id="capsule-third-party-title">Third-party notices</h2><button id="capsule-third-party-close" type="button">Close</button></header><pre id="capsule-third-party-text"></pre></section>
  <script id="sqlite-capsule-export-metadata" type="application/json">{_safe_script_json(metadata)}</script>
  <script id="sqlite-capsule-database" type="application/octet-stream">
{_base64(capsule_gzip)}
  </script>
  <script id="sqlite-capsule-sqlite-js" type="application/octet-stream">
{_base64(sqlite_js_gzip)}
  </script>
  <script id="sqlite-capsule-sqlite-wasm" type="application/octet-stream">
{_base64(sqlite_wasm_gzip)}
  </script>
  <script id="sqlite-capsule-worker" type="application/octet-stream">
{_base64(worker_gzip)}
  </script>
  <script id="sqlite-capsule-third-party-notices" type="text/plain">{runtime["notices"].decode("utf-8")}</script>
  <script id="sqlite-capsule-loader">{loader}</script>
</body>
</html>
"""
    return output.encode("utf-8")


def export_html(
    capsule_path: Path,
    output_path: Path,
    *,
    profile: str,
    replace: bool = False,
    check: bool = False,
) -> dict[str, Any]:
    capsule_path = capsule_path.resolve()
    output_path = output_path.resolve()
    if profile not in PROFILES:
        raise HtmlExportError(f"Unknown export profile: {profile}")
    if not capsule_path.is_file():
        raise HtmlExportError(f"Capsule not found: {capsule_path}")
    capsule_bytes = capsule_path.read_bytes()
    if not 0 < len(capsule_bytes) <= MAX_CAPSULE_BYTES:
        raise HtmlExportError("Capsule size is outside the HTML export limit")
    # Verify the exact immutable byte snapshot that will be embedded. The source
    # path may be replaced concurrently after read_bytes() returns.
    with tempfile.TemporaryDirectory(prefix="sqlite-capsule-export-") as directory:
        snapshot = Path(directory) / "source.capsule.sqlite"
        snapshot.write_bytes(capsule_bytes)
        with CapsuleDatabase(snapshot, read_only=True) as capsule:
            verification = capsule.verify()
            exportability = (
                _exportability_report(capsule, verification["manifest"])
                if verification["ok"]
                else None
            )
    if not verification["ok"]:
        raise HtmlExportError("Source capsule verification failed: " + "; ".join(verification["errors"]))
    runtime = _runtime_inputs()
    compressed = _gzip(capsule_bytes)
    metadata = _metadata(capsule_bytes, compressed, verification["manifest"], profile, runtime)
    rendered = _render_html(metadata, compressed, runtime)
    if len(rendered) > MAX_HTML_BYTES:
        raise HtmlExportError("Rendered HTML exceeds the export size limit")
    if check:
        if not output_path.is_file():
            raise HtmlExportError(f"Checked export is missing: {output_path}")
        existing = output_path.read_bytes()
        if existing != rendered:
            raise HtmlExportError(
                f"Checked export is stale: expected {_sha256(rendered)}, got {_sha256(existing)}"
            )
        return {
            "ok": True,
            "check": True,
            "output": str(output_path),
            "sha256": _sha256(existing),
            "bytes": len(existing),
            "profile": profile,
            "source_sha256": metadata["source"]["sha256"],
            "exportability": exportability,
        }
    if output_path.exists() and not replace:
        raise HtmlExportError(f"Output already exists: {output_path}; pass --replace explicitly")
    output_path.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary_name = tempfile.mkstemp(prefix=f".{output_path.name}.", suffix=".tmp", dir=output_path.parent)
    temporary = Path(temporary_name)
    try:
        with os.fdopen(fd, "wb") as stream:
            stream.write(rendered)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, output_path)
    finally:
        temporary.unlink(missing_ok=True)
    return {
        "ok": True,
        "output": str(output_path),
        "sha256": _sha256(rendered),
        "bytes": len(rendered),
        "profile": profile,
        "source_sha256": metadata["source"]["sha256"],
        "database_compressed_bytes": len(compressed),
        "exportability": exportability,
    }


class _EnvelopeParser(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=False)
        self.blocks: dict[str, list[str]] = {block_id: [] for block_id in BLOCK_IDS}
        self._active: str | None = None
        self.external_urls: list[str] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        values = dict(attrs)
        element_id = values.get("id")
        if tag == "script" and element_id in self.blocks:
            self.blocks[element_id].append("")
            self._active = element_id
        for name in ("src", "href", "poster"):
            value = values.get(name)
            if value and (re.match(r"^[A-Za-z][A-Za-z0-9+.-]*:", value) or value.startswith("//")):
                if value != "about:blank":
                    self.external_urls.append(value)

    def handle_endtag(self, tag: str) -> None:
        if tag == "script":
            self._active = None

    def handle_data(self, data: str) -> None:
        if self._active is not None:
            self.blocks[self._active][-1] += data


def _parse_envelope(path: Path) -> tuple[dict[str, Any], dict[str, str], _EnvelopeParser]:
    path = path.resolve()
    if not path.is_file():
        raise HtmlExportError(f"HTML export not found: {path}")
    if not 0 < path.stat().st_size <= MAX_HTML_BYTES:
        raise HtmlExportError("HTML export size is outside the verification limit")
    try:
        text = path.read_text(encoding="utf-8")
    except UnicodeDecodeError as exc:
        raise HtmlExportError("HTML export is not UTF-8") from exc
    parser = _EnvelopeParser()
    parser.feed(text)
    parser.close()
    blocks: dict[str, str] = {}
    for block_id, values in parser.blocks.items():
        if len(values) != 1:
            raise HtmlExportError(f"Expected exactly one #{block_id} block, found {len(values)}")
        blocks[block_id] = values[0]
    try:
        metadata = json.loads(blocks["sqlite-capsule-export-metadata"])
    except json.JSONDecodeError as exc:
        raise HtmlExportError("Export metadata is not valid JSON") from exc
    if not isinstance(metadata, dict):
        raise HtmlExportError("Export metadata must be an object")
    return metadata, blocks, parser


def _require_sha(value: Any, label: str) -> str:
    if not isinstance(value, str) or not HEX64.fullmatch(value):
        raise HtmlExportError(f"{label} must be a lowercase SHA-256 value")
    return value


def _require_int(value: Any, label: str, maximum: int = MAX_RUNTIME_BYTES) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or not 0 < value <= maximum:
        raise HtmlExportError(f"{label} must be a positive bounded integer")
    return value


def _validate_metadata(metadata: dict[str, Any]) -> None:
    required = {"contract_id", "contract_version", "profile", "created_at", "source", "current", "runtime"}
    if set(metadata) != required:
        raise HtmlExportError("Export metadata has missing or unknown top-level fields")
    if metadata["contract_id"] != CONTRACT_ID or metadata["contract_version"] != CONTRACT_VERSION:
        raise HtmlExportError("Unsupported HTML export contract")
    if metadata["profile"] not in PROFILES:
        raise HtmlExportError("Unknown HTML export profile")
    if not isinstance(metadata["created_at"], str) or not metadata["created_at"].endswith("Z"):
        raise HtmlExportError("created_at must be a UTC timestamp")
    source = metadata["source"]
    current = metadata["current"]
    runtime = metadata["runtime"]
    if not all(isinstance(value, dict) for value in (source, current, runtime)):
        raise HtmlExportError("source, current, and runtime metadata must be objects")
    if set(source) != {"sha256", "bytes", "capsule_id", "format_id", "format_version", "runtime_protocol", "app_id", "app_version", "title"}:
        raise HtmlExportError("source metadata has missing or unknown fields")
    if set(current) != {"revision", "database_sha256", "parent_database_sha256", "uncompressed_bytes", "compressed_sha256", "compressed_bytes"}:
        raise HtmlExportError("current metadata has missing or unknown fields")
    if set(runtime) != {
        "host_id", "host_version", "sqlite_package", "sqlite_version",
        "loader_sha256", "loader_bytes", "sqlite_js_sha256", "sqlite_js_compressed_sha256",
        "sqlite_js_bytes", "sqlite_wasm_sha256", "sqlite_wasm_compressed_sha256",
        "sqlite_wasm_bytes", "worker_sha256", "worker_compressed_sha256", "worker_bytes",
        "notices_sha256", "notices_bytes",
    }:
        raise HtmlExportError("runtime metadata has missing or unknown fields")
    for field in ("sha256",):
        _require_sha(source.get(field), f"source.{field}")
    _require_int(source.get("bytes"), "source.bytes", MAX_CAPSULE_BYTES)
    for field in ("capsule_id", "format_id", "format_version", "runtime_protocol", "app_id", "app_version", "title"):
        if not isinstance(source.get(field), str) or not source[field]:
            raise HtmlExportError(f"source.{field} must be a non-empty string")
    if source["format_id"] != "org.sqlite-capsule":
        raise HtmlExportError("Unsupported source format_id")
    if source["format_version"] != "0.2":
        raise HtmlExportError("Unsupported source format_version")
    if source["runtime_protocol"] != "capsule-http/0.2":
        raise HtmlExportError("Unsupported source runtime_protocol")
    revision = _require_int(current.get("revision"), "current.revision", 2_147_483_647)
    _require_sha(current.get("database_sha256"), "current.database_sha256")
    parent = current.get("parent_database_sha256")
    if revision == 1 and parent is not None:
        raise HtmlExportError("Revision 1 must have a null parent database digest")
    if revision > 1:
        _require_sha(parent, "current.parent_database_sha256")
    _require_int(current.get("uncompressed_bytes"), "current.uncompressed_bytes", MAX_CAPSULE_BYTES)
    _require_sha(current.get("compressed_sha256"), "current.compressed_sha256")
    _require_int(current.get("compressed_bytes"), "current.compressed_bytes")
    if runtime.get("host_id") != HOST_ID or runtime.get("host_version") != HOST_VERSION:
        raise HtmlExportError("Unsupported browser host identity/version")
    if runtime.get("sqlite_package") != SQLITE_PACKAGE:
        raise HtmlExportError("Unexpected SQLite WASM package")
    if runtime.get("sqlite_version") != SQLITE_VERSION:
        raise HtmlExportError("Unexpected SQLite WASM version")
    for stem in ("loader", "sqlite_js", "sqlite_wasm", "worker"):
        _require_sha(runtime.get(f"{stem}_sha256"), f"runtime.{stem}_sha256")
        _require_int(runtime.get(f"{stem}_bytes"), f"runtime.{stem}_bytes")
        if stem != "loader":
            _require_sha(
                runtime.get(f"{stem}_compressed_sha256"),
                f"runtime.{stem}_compressed_sha256",
            )
    _require_sha(runtime.get("notices_sha256"), "runtime.notices_sha256")
    _require_int(runtime.get("notices_bytes"), "runtime.notices_bytes")


def inspect_html(path: Path) -> dict[str, Any]:
    metadata, blocks, parser = _parse_envelope(path)
    _validate_metadata(metadata)
    inventory = {}
    for block_id in BLOCK_IDS:
        if block_id in {"sqlite-capsule-export-metadata", "sqlite-capsule-loader", "sqlite-capsule-third-party-notices"}:
            inventory[block_id] = {"text_bytes": len(blocks[block_id].encode("utf-8"))}
        else:
            inventory[block_id] = {"encoded_characters": len("".join(blocks[block_id].split()))}
    return {
        "ok": True,
        "path": str(path.resolve()),
        "bytes": path.stat().st_size,
        "sha256": _sha256(path.read_bytes()),
        "metadata": metadata,
        "blocks": inventory,
        "external_urls": parser.external_urls,
    }


def verify_html(path: Path) -> dict[str, Any]:
    metadata, blocks, parser = _parse_envelope(path)
    _validate_metadata(metadata)
    if parser.external_urls:
        raise HtmlExportError("Export shell contains external URLs: " + ", ".join(parser.external_urls))
    current = metadata["current"]
    runtime = metadata["runtime"]
    compressed_database = _decode_base64(blocks["sqlite-capsule-database"], "database payload")
    if len(compressed_database) != current["compressed_bytes"]:
        raise HtmlExportError("Database compressed size differs from metadata")
    if _sha256(compressed_database) != current["compressed_sha256"]:
        raise HtmlExportError("Database compressed SHA-256 mismatch")
    database = _gunzip_limited(compressed_database, current["uncompressed_bytes"], "database payload")
    if _sha256(database) != current["database_sha256"]:
        raise HtmlExportError("Database SHA-256 mismatch")
    if current["revision"] == 1:
        if metadata["source"]["sha256"] != current["database_sha256"]:
            raise HtmlExportError("Initial export database does not match source capsule digest")
        if metadata["source"]["bytes"] != current["uncompressed_bytes"]:
            raise HtmlExportError("Initial export database size does not match source capsule size")

    runtime_blocks = {
        "sqlite_js": "sqlite-capsule-sqlite-js",
        "sqlite_wasm": "sqlite-capsule-sqlite-wasm",
        "worker": "sqlite-capsule-worker",
    }
    runtime_report = {}
    for stem, block_id in runtime_blocks.items():
        compressed = _decode_base64(blocks[block_id], f"{stem} payload")
        if _sha256(compressed) != runtime[f"{stem}_compressed_sha256"]:
            raise HtmlExportError(f"{stem} compressed SHA-256 mismatch")
        raw = _gunzip_limited(compressed, runtime[f"{stem}_bytes"], f"{stem} payload")
        if _sha256(raw) != runtime[f"{stem}_sha256"]:
            raise HtmlExportError(f"{stem} SHA-256 mismatch")
        runtime_report[stem] = {"bytes": len(raw), "sha256": _sha256(raw)}
    loader_bytes = blocks["sqlite-capsule-loader"].encode("utf-8")
    if len(loader_bytes) != runtime["loader_bytes"] or _sha256(loader_bytes) != runtime["loader_sha256"]:
        raise HtmlExportError("Browser loader digest/size mismatch")
    runtime_report["loader"] = {"bytes": len(loader_bytes), "sha256": _sha256(loader_bytes)}
    notices_bytes = blocks["sqlite-capsule-third-party-notices"].encode("utf-8")
    if len(notices_bytes) != runtime["notices_bytes"] or _sha256(notices_bytes) != runtime["notices_sha256"]:
        raise HtmlExportError("Third-party notices digest/size mismatch")
    runtime_report["notices"] = {"bytes": len(notices_bytes), "sha256": _sha256(notices_bytes)}

    with tempfile.TemporaryDirectory(prefix="sqlite-capsule-html-verify-") as temporary:
        extracted = Path(temporary) / "embedded.capsule.sqlite"
        extracted.write_bytes(database)
        with CapsuleDatabase(extracted, read_only=True) as capsule:
            capsule_report = capsule.verify()
    if not capsule_report["ok"]:
        raise HtmlExportError("Embedded capsule verification failed: " + "; ".join(capsule_report["errors"]))
    manifest = capsule_report["manifest"]
    source = metadata["source"]
    for field in ("capsule_id", "format_id", "format_version", "runtime_protocol", "app_id", "app_version"):
        if manifest[field] != source[field]:
            raise HtmlExportError(f"Embedded manifest {field} differs from source provenance")
    return {
        "ok": True,
        "path": str(path.resolve()),
        "bytes": path.stat().st_size,
        "sha256": _sha256(path.read_bytes()),
        "profile": metadata["profile"],
        "revision": current["revision"],
        "source_sha256": source["sha256"],
        "database_sha256": current["database_sha256"],
        "parent_database_sha256": current["parent_database_sha256"],
        "runtime": runtime_report,
        "capsule": capsule_report,
    }


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    export_parser = subparsers.add_parser("export-html", help="Create one self-contained HTML export")
    export_parser.add_argument("capsule", type=Path)
    export_parser.add_argument("output", type=Path)
    export_parser.add_argument("--profile", choices=PROFILES, required=True)
    export_parser.add_argument("--replace", action="store_true")
    export_parser.add_argument("--check", action="store_true", help="Compare with a deterministic fresh export")
    inspect_parser = subparsers.add_parser("inspect-html", help="Inspect an HTML export without executing it")
    inspect_parser.add_argument("html", type=Path)
    verify_parser = subparsers.add_parser("verify-html", help="Verify an HTML envelope and its embedded capsule")
    verify_parser.add_argument("html", type=Path)
    return parser


def main(argv: Iterable[str] | None = None) -> int:
    args = build_parser().parse_args(list(argv) if argv is not None else None)
    try:
        if args.command == "export-html":
            result = export_html(
                args.capsule,
                args.output,
                profile=args.profile,
                replace=args.replace,
                check=args.check,
            )
        elif args.command == "inspect-html":
            result = inspect_html(args.html)
        elif args.command == "verify-html":
            result = verify_html(args.html)
        else:  # pragma: no cover
            raise HtmlExportError(f"Unknown HTML command: {args.command}")
    except (HtmlExportError, OSError, ValueError) as exc:
        print(_json_dump({"ok": False, "error": str(exc)}, indent=2))
        return 2
    print(_json_dump(result, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

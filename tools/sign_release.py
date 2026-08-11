#!/usr/bin/env python3
"""Sign and verify a SQLite Capsule release from explicit configuration.

Command-line arguments take precedence over environment variables. A key-file
secret is preferred. CI systems that can expose only a string secret may use
``SQLITE_CAPSULE_SIGNING_KEY_HEX``; it is materialised in a private temporary
file, removed before exit, and stripped from every child-process environment.
"""

from __future__ import annotations

import argparse
from contextlib import contextmanager
from dataclasses import dataclass
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
from typing import Iterator, Mapping


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_SOURCE = ROOT / "capsules" / "diagram-studio.capsule.sqlite"
DEFAULT_OUTPUT = ROOT / "output" / "diagram-studio.signed.sqlitecapsule"
NATIVE_NAME = "capsule-native.exe" if os.name == "nt" else "capsule-native"
DEFAULT_NATIVE_CANDIDATES = (
    ROOT / "native" / "target" / "release" / NATIVE_NAME,
    ROOT / "native" / "target" / "debug" / NATIVE_NAME,
)
SIGNED_AT_PATTERN = re.compile(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$")

ENV_SOURCE = "SQLITE_CAPSULE_SIGN_SOURCE"
ENV_OUTPUT = "SQLITE_CAPSULE_SIGN_OUTPUT"
ENV_NATIVE_CLI = "SQLITE_CAPSULE_NATIVE_CLI"
ENV_PUBLISHER_ID = "SQLITE_CAPSULE_PUBLISHER_ID"
ENV_PUBLISHER_NAME = "SQLITE_CAPSULE_PUBLISHER_NAME"
ENV_KEY_FILE = "SQLITE_CAPSULE_SIGNING_KEY_FILE"
ENV_KEY_HEX = "SQLITE_CAPSULE_SIGNING_KEY_HEX"
ENV_SIGNED_AT = "SQLITE_CAPSULE_SIGNED_AT"


class ReleaseSigningError(RuntimeError):
    """A release-signing precondition or verification failed."""


@dataclass(frozen=True)
class SigningSettings:
    source: Path
    output: Path
    native_cli: Path
    publisher_id: str
    publisher_name: str
    key_file: Path | None
    key_hex: str | None
    signed_at: str


def _configured(
    argument: str | Path | None,
    environment: Mapping[str, str],
    name: str,
    default: str | Path | None = None,
) -> str | Path | None:
    if argument is not None:
        return argument
    value = environment.get(name)
    return value if value not in (None, "") else default


def _repository_path(value: str | Path, *, strict: bool) -> Path:
    path = Path(value).expanduser()
    if not path.is_absolute():
        path = ROOT / path
    return path.resolve(strict=strict)


def _find_native_cli(value: str | Path | None) -> Path:
    if value is not None:
        candidate = _repository_path(value, strict=False)
        if candidate.is_file():
            return candidate
        raise ReleaseSigningError(f"native signing CLI not found: {candidate}")
    for candidate in DEFAULT_NATIVE_CANDIDATES:
        if candidate.is_file():
            return candidate.resolve()
    ambient = shutil.which("capsule-native")
    if ambient:
        return Path(ambient).resolve()
    raise ReleaseSigningError(
        "capsule-native is not built; run "
        "`cargo build --manifest-path native/Cargo.toml "
        "-p sqlite-capsule-cli --release`"
    )


def _utc_now() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).strftime(
        "%Y-%m-%dT%H:%M:%SZ"
    )


def _validate_signed_at(value: str) -> str:
    if not SIGNED_AT_PATTERN.fullmatch(value):
        raise ReleaseSigningError(
            "signed_at must use exact UTC seconds: YYYY-MM-DDTHH:MM:SSZ"
        )
    try:
        datetime.strptime(value, "%Y-%m-%dT%H:%M:%SZ")
    except ValueError as error:
        raise ReleaseSigningError("signed_at is not a real UTC calendar second") from error
    return value


def _validate_identity(value: str | Path | None, label: str) -> str:
    if value is None:
        raise ReleaseSigningError(f"{label} is required")
    text = str(value)
    if not 1 <= len(text) <= 512:
        raise ReleaseSigningError(f"{label} must contain 1 to 512 characters")
    return text


def settings_from_args(
    arguments: argparse.Namespace,
    environment: Mapping[str, str] | None = None,
) -> SigningSettings:
    environment = os.environ if environment is None else environment
    source_value = _configured(arguments.source, environment, ENV_SOURCE, DEFAULT_SOURCE)
    output_value = _configured(arguments.output, environment, ENV_OUTPUT, DEFAULT_OUTPUT)
    native_value = _configured(arguments.native_cli, environment, ENV_NATIVE_CLI)
    publisher_id = _validate_identity(
        _configured(arguments.publisher_id, environment, ENV_PUBLISHER_ID),
        ENV_PUBLISHER_ID,
    )
    publisher_name = _validate_identity(
        _configured(arguments.publisher_name, environment, ENV_PUBLISHER_NAME),
        ENV_PUBLISHER_NAME,
    )
    key_file_value = _configured(arguments.key_file, environment, ENV_KEY_FILE)
    key_hex = environment.get(ENV_KEY_HEX) or None
    if key_file_value is None and key_hex is None:
        raise ReleaseSigningError(
            f"set exactly one of {ENV_KEY_FILE} or {ENV_KEY_HEX}"
        )
    if key_file_value is not None and key_hex is not None:
        raise ReleaseSigningError(
            f"{ENV_KEY_FILE} and {ENV_KEY_HEX} are mutually exclusive"
        )
    if key_hex is not None:
        if len(key_hex) != 64 or re.fullmatch(r"[0-9a-fA-F]{64}", key_hex) is None:
            raise ReleaseSigningError(
                f"{ENV_KEY_HEX} must contain exactly 64 hexadecimal digits"
            )
    key_file = (
        _repository_path(key_file_value, strict=True)
        if key_file_value is not None
        else None
    )
    if key_file is not None and not key_file.is_file():
        raise ReleaseSigningError(f"signing key is not a regular file: {key_file}")

    assert source_value is not None and output_value is not None
    source = _repository_path(source_value, strict=True)
    output = _repository_path(output_value, strict=False)
    if not source.is_file():
        raise ReleaseSigningError(f"source capsule is not a regular file: {source}")
    if output.exists() or output.is_symlink():
        raise ReleaseSigningError(f"refusing to replace existing output: {output}")
    if source == output:
        raise ReleaseSigningError("refusing in-place signing")

    signed_at_value = _configured(arguments.signed_at, environment, ENV_SIGNED_AT)
    signed_at = _validate_signed_at(str(signed_at_value or _utc_now()))
    return SigningSettings(
        source=source,
        output=output,
        native_cli=_find_native_cli(native_value),
        publisher_id=publisher_id,
        publisher_name=publisher_name,
        key_file=key_file,
        key_hex=key_hex,
        signed_at=signed_at,
    )


def sanitized_child_environment(
    environment: Mapping[str, str] | None = None,
) -> dict[str, str]:
    child = dict(os.environ)
    if environment is not None:
        child.update(environment)
    child.pop(ENV_KEY_HEX, None)
    return child


@contextmanager
def materialized_key(settings: SigningSettings) -> Iterator[Path]:
    if settings.key_file is not None:
        yield settings.key_file
        return
    if settings.key_hex is None:  # pragma: no cover - guarded during resolution
        raise ReleaseSigningError("signing key is unavailable")
    secret = bytearray.fromhex(settings.key_hex)
    try:
        with tempfile.TemporaryDirectory(prefix="sqlite-capsule-signing-") as raw:
            path = Path(raw) / "publisher.seed"
            descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
            try:
                stream = os.fdopen(descriptor, "wb")
            except BaseException:
                os.close(descriptor)
                raise
            with stream:
                stream.write(secret)
                stream.flush()
                os.fsync(stream.fileno())
            yield path
    finally:
        secret[:] = b"\x00" * len(secret)


def _run_json(
    command: list[str],
    *,
    label: str,
    environment: Mapping[str, str],
) -> dict[str, object]:
    try:
        completed = subprocess.run(
            command,
            cwd=ROOT,
            env=dict(environment),
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            timeout=120,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise ReleaseSigningError(f"{label} could not run: {error}") from error
    rendered = completed.stdout.strip() or completed.stderr.strip()
    try:
        payload = json.loads(rendered)
    except json.JSONDecodeError as error:
        raise ReleaseSigningError(f"{label} returned invalid JSON") from error
    if completed.returncode != 0 or payload.get("ok") is not True:
        detail = payload.get("error", f"exit code {completed.returncode}")
        raise ReleaseSigningError(f"{label} failed: {detail}")
    return payload


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def sign_release(
    settings: SigningSettings,
    environment: Mapping[str, str] | None = None,
) -> dict[str, object]:
    child_environment = sanitized_child_environment(environment)
    _run_json(
        [sys.executable, str(ROOT / "tools" / "capsule.py"), "verify", str(settings.source)],
        label="source capsule verification",
        environment=child_environment,
    )
    settings.output.parent.mkdir(parents=True, exist_ok=True)
    if settings.output.parent.is_symlink() or not settings.output.parent.is_dir():
        raise ReleaseSigningError(
            f"output parent must be a regular directory: {settings.output.parent}"
        )
    with materialized_key(settings) as key_path:
        _run_json(
            [
                str(settings.native_cli),
                "sign",
                str(settings.source),
                str(settings.output),
                "--publisher-id",
                settings.publisher_id,
                "--publisher-name",
                settings.publisher_name,
                "--key",
                str(key_path),
                "--signed-at",
                settings.signed_at,
            ],
            label="native capsule signing",
            environment=child_environment,
        )
    native_report = _run_json(
        [str(settings.native_cli), "verify-signature", str(settings.output)],
        label="native signature verification",
        environment=child_environment,
    )
    if native_report.get("signature_valid") is not True:
        raise ReleaseSigningError("native verifier did not report a valid signature")
    _run_json(
        [sys.executable, str(ROOT / "tools" / "capsule.py"), "verify", str(settings.output)],
        label="signed capsule verification",
        environment=child_environment,
    )
    inventory = _run_json(
        [
            sys.executable,
            str(ROOT / "tools" / "capsule.py"),
            "signatures",
            str(settings.output),
            "--native-verifier",
            str(settings.native_cli),
        ],
        label="independent signature inventory",
        environment=child_environment,
    )
    if inventory.get("signature_valid") is not True:
        raise ReleaseSigningError("independent inventory did not report a valid signature")
    signatures = native_report.get("signatures")
    first_signature = signatures[0] if isinstance(signatures, list) and signatures else {}
    return {
        "ok": True,
        "source": str(settings.source),
        "source_sha256": _sha256(settings.source),
        "output": str(settings.output),
        "output_sha256": _sha256(settings.output),
        "publisher": {
            "id": settings.publisher_id,
            "name": settings.publisher_name,
        },
        "key_id": first_signature.get("key_id"),
        "application_digest": native_report.get("application_digest_sha256"),
        "signed_at": settings.signed_at,
        "signature_valid": True,
        "publisher_trusted": False,
        "note": (
            "The signature authenticates the configured key. Trust remains a "
            "separate host-local user decision."
        ),
    }


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--native-cli", type=Path)
    parser.add_argument("--publisher-id")
    parser.add_argument("--publisher-name")
    parser.add_argument("--key-file", type=Path)
    parser.add_argument("--signed-at")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    try:
        settings = settings_from_args(parse_args(argv))
        report = sign_release(settings)
    except (OSError, ReleaseSigningError, ValueError) as error:
        print(
            json.dumps({"ok": False, "error": str(error)}, indent=2, sort_keys=True),
            file=sys.stderr,
        )
        return 1
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

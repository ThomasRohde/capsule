#!/usr/bin/env python3
"""Independent compare-key/row v1 vector checker using the Python stdlib."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import struct
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
DEFAULT = ROOT / "compatibility" / "compare-row-v1" / "vectors.json"
KEY_PROFILE = "org.sqlite-capsule.compare-key/1"
ROW_PROFILE = "org.sqlite-capsule.compare-row/1"


def u32(value: int) -> bytes:
    return value.to_bytes(4, "big", signed=False)


def u64(value: int) -> bytes:
    return value.to_bytes(8, "big", signed=False)


def text(value: str) -> bytes:
    raw = value.encode("utf-8")
    return u64(len(raw)) + raw


def typed(spec: dict[str, str]) -> bytes:
    kind = spec["type"]
    if kind == "null":
        return b"\x00"
    if kind == "integer":
        value = int(spec["decimal"])
        if not -(1 << 63) <= value < (1 << 63):
            raise ValueError("integer outside i64")
        return b"\x01" + value.to_bytes(8, "big", signed=True)
    if kind == "real-bits":
        raw = bytes.fromhex(spec["hex"])
        if len(raw) != 8 or not math.isfinite(struct.unpack(">d", raw)[0]):
            raise ValueError("non-finite or malformed REAL")
        return b"\x02" + raw
    if kind == "text":
        raw = bytes.fromhex(spec["utf8_hex"])
        raw.decode("utf-8", errors="strict")
        return b"\x03" + u64(len(raw)) + raw
    if kind == "blob":
        raw = bytes.fromhex(spec["hex"])
        return b"\x04" + u64(len(raw)) + raw
    raise ValueError("unknown value type")


def key_frame(case: dict[str, object]) -> bytes:
    key = case["key"]
    assert isinstance(key, list) and key
    frame = bytearray(text(KEY_PROFILE))
    frame.extend(text(str(case["table"])))
    frame.extend(u32(len(key)))
    for field in key:
        assert isinstance(field, dict)
        frame.extend(text(str(field["column"])))
        frame.extend(typed(field["value"]))
    return bytes(frame)


def row_frame(case: dict[str, object]) -> bytes:
    key = case["key"]
    assert isinstance(key, list) and key
    compared = case["compared"]
    assert isinstance(compared, list)
    frame = bytearray(text(ROW_PROFILE))
    frame.extend(text(str(case["table"])))
    frame.extend(u32(len(key)))
    for field in key:
        assert isinstance(field, dict)
        frame.extend(text(str(field["column"])))
        frame.extend(typed(field["value"]))
    frame.extend(u32(len(compared)))
    for field in compared:
        assert isinstance(field, dict)
        frame.extend(text(str(field["column"])))
        frame.extend(typed(field["value"]))
    return bytes(frame)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--vectors", type=Path, default=DEFAULT)
    parser.add_argument("--rewrite", action="store_true")
    args = parser.parse_args()
    document = json.loads(args.vectors.read_text(encoding="utf-8"))
    if document["key_profile"] != "org.sqlite-capsule.compare-key/1":
        raise SystemExit("unexpected key profile")
    if document["row_profile"] != "org.sqlite-capsule.compare-row/1":
        raise SystemExit("unexpected row profile")

    changed = False
    for case in document["cases"]:
        key = key_frame(case)
        row = row_frame(case)
        expected = {
            "key_bytes_hex": key.hex(),
            "key_sha256": hashlib.sha256(key).hexdigest(),
            "row_bytes_hex": row.hex(),
            "row_sha256": hashlib.sha256(row).hexdigest(),
        }
        for field, value in expected.items():
            if args.rewrite:
                changed |= case[field] != value
                case[field] = value
            elif case[field] != value:
                raise SystemExit(f"{case['id']}: {field} mismatch")

    for case in document["invalid"]:
        try:
            typed(case["value"])
        except (UnicodeDecodeError, ValueError):
            continue
        raise SystemExit(f"{case['id']}: invalid value accepted")

    if args.rewrite and changed:
        args.vectors.write_text(
            json.dumps(document, ensure_ascii=False, indent=2) + "\n",
            encoding="utf-8",
            newline="\n",
        )
    print(f"ok: {len(document['cases'])} compare rows and {len(document['invalid'])} hostile values")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

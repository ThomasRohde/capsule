#!/usr/bin/env python3
"""Verify the lifecycle-plan/1 canonical JSON vectors using stdlib only."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
VECTOR_DIR = ROOT / "compatibility" / "lifecycle-plan-v1"


class VectorError(ValueError):
    pass


def _object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    value: dict[str, Any] = {}
    for key, item in pairs:
        if key in value:
            raise VectorError(f"duplicate JSON member: {key}")
        value[key] = item
    return value


def _integer(text: str) -> int:
    value = int(text)
    if not -(2**63) <= value <= 2**64 - 1:
        raise VectorError("integer is outside the lifecycle JSON range")
    return value


def strict_loads(data: bytes) -> Any:
    try:
        return json.loads(
            data.decode("utf-8"),
            object_pairs_hook=_object,
            parse_int=_integer,
            parse_float=lambda _text: (_ for _ in ()).throw(
                VectorError("floating-point JSON is forbidden")
            ),
            parse_constant=lambda _text: (_ for _ in ()).throw(
                VectorError("non-finite JSON is forbidden")
            ),
        )
    except UnicodeDecodeError as error:
        raise VectorError("vector is not UTF-8") from error


def canonical_json(value: Any) -> bytes:
    return json.dumps(
        value,
        ensure_ascii=False,
        allow_nan=False,
        separators=(",", ":"),
        sort_keys=True,
    ).encode("utf-8")


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def verify(vector_dir: Path = VECTOR_DIR, *, rewrite_canonical: bool = False) -> list[str]:
    vectors = strict_loads((vector_dir / "vectors.json").read_bytes())
    plan_path = vector_dir / "vector-plan.json"
    raw_plan = plan_path.read_bytes()
    plan = strict_loads(raw_plan)
    if not isinstance(vectors, dict) or not isinstance(plan, dict):
        raise VectorError("vector roots must be objects")

    embedded = plan.get("plan_digest")
    if not isinstance(embedded, str):
        raise VectorError("plan_digest is missing")
    unsigned = dict(plan)
    del unsigned["plan_digest"]
    unsigned_bytes = canonical_json(unsigned)
    full_bytes = canonical_json(plan)
    if rewrite_canonical:
        plan_path.write_bytes(full_bytes)
        raw_plan = full_bytes
    if raw_plan != full_bytes:
        raise VectorError(
            "vector-plan.json is not the exact frozen canonical UTF-8 byte stream"
        )

    expected = vectors["plan"]
    checks = {
        "plan digest": (sha256(unsigned_bytes), embedded),
        "expected plan digest": (sha256(unsigned_bytes), expected["plan_digest"]),
        "canonical plan size": (len(full_bytes), expected["canonical_size"]),
        "canonical plan sha256": (sha256(full_bytes), expected["canonical_sha256"]),
    }
    for label, (actual, wanted) in checks.items():
        if actual != wanted:
            raise VectorError(f"{label}: expected {wanted}, got {actual}")

    unicode_case = vectors["unicode_scalar_order"]
    unicode_bytes = canonical_json(unicode_case["value"])
    if unicode_bytes.decode("utf-8") != unicode_case["canonical"]:
        raise VectorError("non-BMP key order does not follow Unicode scalar values")
    if sha256(unicode_bytes) != unicode_case["sha256"]:
        raise VectorError("non-BMP vector digest mismatch")

    canonical_results = []
    for case in vectors["canonical_cases"]:
        case_bytes = canonical_json(case["value"])
        if case_bytes.decode("utf-8") != case["canonical"]:
            raise VectorError(f"{case['name']}: canonical bytes mismatch")
        if sha256(case_bytes) != case["sha256"]:
            raise VectorError(f"{case['name']}: digest mismatch")
        canonical_results.append(f"{case['name']} {case['sha256']}")

    return [
        f"plan digest {embedded}",
        f"canonical plan {len(full_bytes)} bytes {sha256(full_bytes)}",
        f"Unicode scalar ordering {sha256(unicode_bytes)}",
        *canonical_results,
    ]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--vector-dir", type=Path, default=VECTOR_DIR)
    parser.add_argument(
        "--rewrite-canonical",
        action="store_true",
        help="rewrite vector-plan.json to the exact canonical UTF-8 byte stream",
    )
    args = parser.parse_args()
    try:
        results = verify(args.vector_dir, rewrite_canonical=args.rewrite_canonical)
    except (KeyError, OSError, TypeError, VectorError, json.JSONDecodeError) as error:
        print(f"lifecycle plan vectors: FAIL: {error}")
        return 1
    for result in results:
        print(f"lifecycle plan vectors: {result}")
    print("lifecycle plan vectors: PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

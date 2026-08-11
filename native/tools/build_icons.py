#!/usr/bin/env python3
"""Build deterministic native resources from the verified capsule mark."""

from __future__ import annotations

import argparse
import struct
import zlib
from pathlib import Path


RGBA = tuple[int, int, int, int]
TRANSPARENT: RGBA = (0, 0, 0, 0)
CAPSULE_BLUE: RGBA = (24, 95, 165, 255)
PLAY_LIGHT: RGBA = (230, 241, 251, 255)
AMBER: RGBA = (239, 159, 39, 255)
AMBER_INK: RGBA = (65, 36, 2, 255)
MARK_SCALE = 1.2
MARK_OFFSET_X = 8.0
MARK_OFFSET_Y = 41.6


def inside_round_rect(x: float, y: float, left: float, top: float, right: float, bottom: float, radius: float) -> bool:
    if left + radius <= x <= right - radius or top + radius <= y <= bottom - radius:
        return left <= x <= right and top <= y <= bottom
    center_x = left + radius if x < left + radius else right - radius
    center_y = top + radius if y < top + radius else bottom - radius
    return (x - center_x) ** 2 + (y - center_y) ** 2 <= radius**2


def inside_ellipse(x: float, y: float, center_x: float, center_y: float, radius_x: float, radius_y: float) -> bool:
    return ((x - center_x) / radius_x) ** 2 + ((y - center_y) / radius_y) ** 2 <= 1


def inside_triangle(
    x: float,
    y: float,
    first: tuple[float, float],
    second: tuple[float, float],
    third: tuple[float, float],
) -> bool:
    def cross(
        point: tuple[float, float],
        start: tuple[float, float],
        end: tuple[float, float],
    ) -> float:
        return (point[0] - end[0]) * (start[1] - end[1]) - (
            start[0] - end[0]
        ) * (point[1] - end[1])

    point = (x, y)
    edges = (
        cross(point, first, second),
        cross(point, second, third),
        cross(point, third, first),
    )
    return not (any(edge < 0 for edge in edges) and any(edge > 0 for edge in edges))


def segment_distance(
    x: float,
    y: float,
    x1: float,
    y1: float,
    x2: float,
    y2: float,
) -> float:
    dx, dy = x2 - x1, y2 - y1
    length_sq = dx * dx + dy * dy
    projection = max(
        0.0,
        min(1.0, ((x - x1) * dx + (y - y1) * dy) / length_sq),
    )
    nearest_x, nearest_y = x1 + projection * dx, y1 + projection * dy
    return ((x - nearest_x) ** 2 + (y - nearest_y) ** 2) ** 0.5


def mark_color_at(x: float, y: float) -> RGBA:
    """Sample the fixed-colour verified SVG in its 200 by 144 view box."""

    inside_capsule = inside_round_rect(x, y, 0, 24, 180, 144, 60)
    inside_badge_cutout = inside_ellipse(x, y, 164, 40, 34, 34)
    color = CAPSULE_BLUE if inside_capsule != inside_badge_cutout else TRANSPARENT

    if inside_triangle(x, y, (75, 58), (75, 110), (120, 84)):
        color = PLAY_LIGHT
    if inside_ellipse(x, y, 164, 40, 28, 28):
        color = AMBER
    if (
        segment_distance(x, y, 153, 40, 161, 48) <= 3
        or segment_distance(x, y, 161, 48, 176, 31) <= 3
    ):
        color = AMBER_INK
    return color


def color_at(x: float, y: float) -> RGBA:
    source_x = (x - MARK_OFFSET_X) / MARK_SCALE
    source_y = (y - MARK_OFFSET_Y) / MARK_SCALE
    return mark_color_at(source_x, source_y)


def render(size: int, samples: int = 4) -> bytes:
    pixels = bytearray()
    scale = 256 / size
    for row in range(size):
        pixels.append(0)  # PNG filter: None
        for column in range(size):
            totals = [0, 0, 0, 0]
            for sample_y in range(samples):
                for sample_x in range(samples):
                    x = (column + (sample_x + 0.5) / samples) * scale
                    y = (row + (sample_y + 0.5) / samples) * scale
                    for index, value in enumerate(color_at(x, y)):
                        totals[index] += value
            divisor = samples * samples
            pixels.extend(round(total / divisor) for total in totals)
    return png(size, size, bytes(pixels))


def chunk(kind: bytes, data: bytes) -> bytes:
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)


def png(width: int, height: int, rows: bytes) -> bytes:
    header = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b"")


def ico(images: list[tuple[int, bytes]]) -> bytes:
    directory = bytearray(struct.pack("<HHH", 0, 1, len(images)))
    offset = 6 + 16 * len(images)
    payload = bytearray()
    for size, image in images:
        encoded_size = 0 if size == 256 else size
        directory.extend(struct.pack("<BBBBHHII", encoded_size, encoded_size, 0, 0, 1, 32, len(image), offset))
        payload.extend(image)
        offset += len(image)
    return bytes(directory + payload)


def icns(images: dict[int, bytes]) -> bytes:
    chunk_names = {
        16: b"icp4",
        32: b"icp5",
        64: b"icp6",
        128: b"ic07",
        256: b"ic08",
        512: b"ic09",
        1024: b"ic10",
    }
    payload = bytearray()
    for size, image in images.items():
        data = chunk_names[size] + struct.pack(">I", len(image) + 8) + image
        payload.extend(data)
    return b"icns" + struct.pack(">I", len(payload) + 8) + bytes(payload)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", type=Path, default=Path(__file__).parents[1] / "desktop" / "src-tauri" / "icons")
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)

    images = {size: render(size) for size in (16, 32, 48, 64, 128, 256)}
    mac_images = {
        16: images[16],
        32: images[32],
        64: images[64],
        128: images[128],
        256: images[256],
        512: render(512, samples=2),
        1024: render(1024, samples=1),
    }
    (args.out / "icon.ico").write_bytes(ico(list(images.items())))
    (args.out / "icon.icns").write_bytes(icns(mac_images))
    (args.out / "icon.png").write_bytes(images[256])
    (args.out / "32x32.png").write_bytes(images[32])
    (args.out / "128x128.png").write_bytes(images[128])
    (args.out / "128x128@2x.png").write_bytes(images[256])


if __name__ == "__main__":
    main()

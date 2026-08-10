#!/usr/bin/env python3
"""Build deterministic PNG and ICO resources for the generic native host."""

from __future__ import annotations

import argparse
import struct
import zlib
from pathlib import Path


RGBA = tuple[int, int, int, int]
TRANSPARENT: RGBA = (0, 0, 0, 0)
DARK: RGBA = (16, 21, 26, 255)
PANEL: RGBA = (22, 36, 31, 255)
MINT: RGBA = (131, 212, 190, 255)
MINT_LIGHT: RGBA = (158, 232, 210, 255)
MINT_DARK: RGBA = (36, 93, 79, 255)
AMBER: RGBA = (232, 184, 121, 255)
AMBER_INK: RGBA = (41, 29, 13, 255)


def inside_round_rect(x: float, y: float, left: float, top: float, right: float, bottom: float, radius: float) -> bool:
    if left + radius <= x <= right - radius or top + radius <= y <= bottom - radius:
        return left <= x <= right and top <= y <= bottom
    center_x = left + radius if x < left + radius else right - radius
    center_y = top + radius if y < top + radius else bottom - radius
    return (x - center_x) ** 2 + (y - center_y) ** 2 <= radius**2


def inside_ellipse(x: float, y: float, center_x: float, center_y: float, radius_x: float, radius_y: float) -> bool:
    return ((x - center_x) / radius_x) ** 2 + ((y - center_y) / radius_y) ** 2 <= 1


def color_at(x: float, y: float) -> RGBA:
    if not inside_round_rect(x, y, 12, 12, 244, 244, 56):
        return TRANSPARENT
    color = DARK

    outer_body = 55 <= x <= 201 and 84 <= y <= 172
    lower_outer = inside_ellipse(x, y, 128, 172, 73, 42)
    upper_outer = inside_ellipse(x, y, 128, 84, 73, 42)
    if outer_body or lower_outer or upper_outer:
        color = MINT

    inner_body = 65 <= x <= 191 and 84 <= y <= 170
    lower_inner = inside_ellipse(x, y, 128, 170, 63, 32)
    if inner_body or lower_inner:
        color = PANEL

    if inside_ellipse(x, y, 128, 84, 63, 29):
        color = MINT_DARK
    if inside_ellipse(x, y, 128, 81, 57, 22):
        color = MINT_LIGHT
    if inside_ellipse(x, y, 128, 83, 55, 19):
        color = MINT_DARK

    for center_y in (126, 168):
        outer = inside_ellipse(x, y, 128, center_y, 68, 35)
        inner = inside_ellipse(x, y, 128, center_y - 4, 60, 26)
        if outer and not inner and y >= center_y - 2:
            color = MINT

    if inside_ellipse(x, y, 195, 61, 27, 27):
        color = DARK
    if inside_ellipse(x, y, 195, 61, 20, 20):
        color = AMBER

    # Thick two-segment check mark in the amber trust indicator.
    def segment_distance(x1: float, y1: float, x2: float, y2: float) -> float:
        dx, dy = x2 - x1, y2 - y1
        length_sq = dx * dx + dy * dy
        projection = max(0.0, min(1.0, ((x - x1) * dx + (y - y1) * dy) / length_sq))
        nearest_x, nearest_y = x1 + projection * dx, y1 + projection * dy
        return ((x - nearest_x) ** 2 + (y - nearest_y) ** 2) ** 0.5

    if segment_distance(184, 61, 192, 69) <= 3.5 or segment_distance(192, 69, 207, 50) <= 3.5:
        color = AMBER_INK
    return color


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


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", type=Path, default=Path(__file__).parents[1] / "desktop" / "src-tauri" / "icons")
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)

    images = {size: render(size) for size in (16, 32, 48, 64, 128, 256)}
    (args.out / "icon.ico").write_bytes(ico(list(images.items())))
    (args.out / "icon.png").write_bytes(images[256])
    (args.out / "32x32.png").write_bytes(images[32])
    (args.out / "128x128.png").write_bytes(images[128])
    (args.out / "128x128@2x.png").write_bytes(images[256])


if __name__ == "__main__":
    main()


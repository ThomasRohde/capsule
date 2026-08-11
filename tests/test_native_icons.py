from __future__ import annotations

import json
import struct
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
ICON_ROOT = ROOT / "native" / "desktop" / "src-tauri" / "icons"
BRAND_ROOT = ROOT / "docs" / "images" / "brand"


def png_dimensions(path: Path) -> tuple[int, int]:
    content = path.read_bytes()
    if content[:16] != b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR":
        raise AssertionError(f"{path} is not a PNG with an IHDR first chunk")
    return struct.unpack(">II", content[16:24])


class NativeIconTests(unittest.TestCase):
    def test_canonical_svg_variants_preserve_colour_and_cutout_contracts(self) -> None:
        fixed = (BRAND_ROOT / "sqlite-capsule-mark.svg").read_text(
            encoding="utf-8"
        )
        cutout = (BRAND_ROOT / "sqlite-capsule-mark-currentcolor.svg").read_text(
            encoding="utf-8"
        )
        verified = (BRAND_ROOT / "sqlite-capsule-mark-verified.svg").read_text(
            encoding="utf-8"
        )
        verified_cutout = (
            BRAND_ROOT / "sqlite-capsule-mark-verified-currentcolor.svg"
        ).read_text(encoding="utf-8")

        self.assertIn('fill="#185FA5"', fixed)
        self.assertIn('fill="#E6F1FB"', fixed)
        self.assertNotIn("currentColor", fixed)
        for source in (cutout, verified_cutout):
            self.assertIn('fill-rule="evenodd"', source)
            self.assertIn('clip-rule="evenodd"', source)
            self.assertIn('fill="currentColor"', source)
        self.assertIn('color="#185FA5"', cutout)
        self.assertIn("SQLite Capsule, verified", verified)
        self.assertIn('fill="#EF9F27"', verified)
        self.assertIn('stroke="#412402"', verified)

    def test_native_vector_and_web_marks_use_the_intended_variants(self) -> None:
        native_svg = (ICON_ROOT / "icon.svg").read_text(encoding="utf-8")
        self.assertIn('fill="#185FA5"', native_svg)
        self.assertIn('fill="#E6F1FB"', native_svg)
        self.assertIn('fill="#EF9F27"', native_svg)
        self.assertIn('transform="translate(8 41.6) scale(1.2)"', native_svg)

        host_html = (ROOT / "native/desktop/ui/index.html").read_text(
            encoding="utf-8"
        )
        self.assertIn('class="titlebar-mark"', host_html)
        self.assertIn('fill-rule="evenodd"', host_html)
        self.assertIn('fill="currentColor"', host_html)

        readme = (ROOT / "README.md").read_text(encoding="utf-8")
        self.assertIn(
            "![SQLite Capsule](docs/images/brand/"
            "sqlite-capsule-mark-currentcolor.svg)",
            readme,
        )

        build_script = (
            ROOT / "native/desktop/src-tauri/build.rs"
        ).read_text(encoding="utf-8")
        for icon_name in (
            "32x32.png",
            "128x128.png",
            "128x128@2x.png",
            "icon.icns",
            "icon.ico",
            "icon.png",
            "icon.svg",
        ):
            self.assertIn(f'"icons/{icon_name}"', build_script)

        configuration = json.loads(
            (ROOT / "native/desktop/src-tauri/tauri.conf.json").read_text(
                encoding="utf-8"
            )
        )
        nsis = configuration["bundle"]["windows"]["nsis"]
        self.assertEqual(nsis["installerIcon"], "icons/icon.ico")
        self.assertEqual(nsis["uninstallerIcon"], "icons/icon.ico")

    def test_generated_native_icon_containers_have_complete_size_sets(self) -> None:
        self.assertEqual(png_dimensions(ICON_ROOT / "32x32.png"), (32, 32))
        self.assertEqual(png_dimensions(ICON_ROOT / "128x128.png"), (128, 128))
        self.assertEqual(
            png_dimensions(ICON_ROOT / "128x128@2x.png"), (256, 256)
        )
        self.assertEqual(png_dimensions(ICON_ROOT / "icon.png"), (256, 256))

        ico = (ICON_ROOT / "icon.ico").read_bytes()
        reserved, kind, count = struct.unpack("<HHH", ico[:6])
        self.assertEqual((reserved, kind, count), (0, 1, 6))
        ico_sizes = []
        for index in range(count):
            width, height = struct.unpack_from("<BB", ico, 6 + index * 16)
            ico_sizes.append((width or 256, height or 256))
        self.assertEqual(
            ico_sizes,
            [(16, 16), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
        )

        icns = (ICON_ROOT / "icon.icns").read_bytes()
        self.assertEqual(icns[:4], b"icns")
        self.assertEqual(struct.unpack(">I", icns[4:8])[0], len(icns))
        chunk_names = []
        offset = 8
        while offset < len(icns):
            chunk_names.append(icns[offset : offset + 4])
            chunk_length = struct.unpack(">I", icns[offset + 4 : offset + 8])[0]
            self.assertGreaterEqual(chunk_length, 8)
            offset += chunk_length
        self.assertEqual(offset, len(icns))
        self.assertEqual(
            chunk_names,
            [b"icp4", b"icp5", b"icp6", b"ic07", b"ic08", b"ic09", b"ic10"],
        )


if __name__ == "__main__":
    unittest.main()

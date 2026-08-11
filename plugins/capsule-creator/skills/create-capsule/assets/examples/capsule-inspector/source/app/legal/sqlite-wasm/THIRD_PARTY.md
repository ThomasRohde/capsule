# Vendored SQLite WASM

- Package: `@sqlite.org/sqlite-wasm`
- Version: `3.53.0-build1`
- Package source: <https://www.npmjs.com/package/@sqlite.org/sqlite-wasm/v/3.53.0-build1>
- Upstream repository: <https://github.com/sqlite/sqlite-wasm>
- Underlying SQLite WASM: <https://sqlite.org/wasm/doc/trunk/index.md>
- Package license: Apache-2.0
- License text: [`LICENSE.Apache-2.0.txt`](LICENSE.Apache-2.0.txt)
- SQLite engine: public domain; see <https://www.sqlite.org/copyright.html>
- Build family: official upstream 32-bit-pointer SQLite WASM distribution. The
  package README states that it wraps SQLite WASM without changes apart from
  TypeScript types; this repository does not enable the separate wasm64 build.

Vendored files:

| File | SHA-256 |
| --- | --- |
| `index.mjs` | `f80870f0fa03a39a3338d17ed3fbea04808d344c88e724d90d5f37b9b7b83154` |
| `sqlite3.wasm` | `02d7e48164395fa68f81c6ec33e9da5461be397dc57602ac0cd89b4bbba1d312` |

The files are copied byte-for-byte from the pinned npm package. Normal build,
export, verification, and test commands use these local copies and perform no
runtime download.

## Audited update procedure

1. Select one exact published package version; never use `latest` or a range.
2. Install it as an exact development dependency with npm.
3. Copy `dist/index.mjs` and `dist/sqlite3.wasm` byte-for-byte into this folder.
4. Update the version constants in `tools/capsule_html.py`, the metadata schema,
   and this notice.
5. Recompute both SHA-256 values and update the pinned-digest regression test.
6. Run the Python suite, the Chromium/Firefox/WebKit HTML matrix, actual Safari
   acceptance, deterministic export checks, and all three `verify-html` gates.

The checked files remain unchanged until that entire review is complete.

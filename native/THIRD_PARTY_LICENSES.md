# Native third-party license inventory

This deterministic inventory is generated from `native/Cargo.lock` via
`cargo metadata --locked --all-features`. It includes registry packages for
all resolved target branches; workspace crates are MIT and are not repeated.
A package without a Cargo license expression or declared license file makes
generation fail.

Resolved third-party package records: **580**.

License expressions/files present:

- `(MIT OR Apache-2.0) AND Unicode-3.0`
- `0BSD OR MIT OR Apache-2.0`
- `Apache-2.0`
- `Apache-2.0 / MIT`
- `Apache-2.0 AND ISC`
- `Apache-2.0 AND MIT`
- `Apache-2.0 OR BSL-1.0`
- `Apache-2.0 OR ISC OR MIT`
- `Apache-2.0 OR MIT`
- `Apache-2.0 WITH LLVM-exception`
- `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`
- `Apache-2.0/MIT`
- `BSD-2-Clause OR Apache-2.0 OR MIT`
- `BSD-3-Clause`
- `BSD-3-Clause AND MIT`
- `BSD-3-Clause OR MIT OR Apache-2.0`
- `BSD-3-Clause/MIT`
- `CC0-1.0 OR MIT-0 OR Apache-2.0`
- `CDLA-Permissive-2.0`
- `ISC`
- `ISC AND (Apache-2.0 OR ISC)`
- `ISC AND (Apache-2.0 OR ISC) AND Apache-2.0 AND MIT AND BSD-3-Clause AND (Apache-2.0 OR ISC OR MIT) AND (Apache-2.0 OR ISC OR MIT-0)`
- `MIT`
- `MIT OR Apache-2.0`
- `MIT OR Apache-2.0 OR BSD-1-Clause`
- `MIT OR Apache-2.0 OR LGPL-2.1-or-later`
- `MIT OR Apache-2.0 OR Zlib`
- `MIT OR Zlib OR Apache-2.0`
- `MIT/Apache-2.0`
- `MPL-2.0`
- `Unicode-3.0`
- `Unlicense OR MIT`
- `Unlicense/MIT`
- `Zlib`
- `Zlib OR Apache-2.0 OR MIT`

| Package | Version | License | Locked source |
| --- | --- | --- | --- |
| `adler2` | `2.0.1` | `0BSD OR MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `aho-corasick` | `1.1.5` | `Unlicense OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `alloc-no-stdlib` | `2.0.4` | `BSD-3-Clause` | `registry+https://github.com/rust-lang/crates.io-index` |
| `alloc-stdlib` | `0.2.4` | `BSD-3-Clause` | `registry+https://github.com/rust-lang/crates.io-index` |
| `android_system_properties` | `0.1.6` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `anyhow` | `1.0.104` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `arbitrary` | `1.4.2` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `async-broadcast` | `0.7.2` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `async-channel` | `2.5.0` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `async-executor` | `1.14.0` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `async-io` | `2.6.0` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `async-lock` | `3.4.2` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `async-process` | `2.5.0` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `async-recursion` | `1.1.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `async-signal` | `0.2.14` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `async-task` | `4.7.1` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `async-trait` | `0.1.92` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `atk` | `0.18.2` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `atk-sys` | `0.18.2` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `atomic-waker` | `1.1.2` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `autocfg` | `1.5.1` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `aws-lc-rs` | `1.18.0` | `ISC AND (Apache-2.0 OR ISC)` | `registry+https://github.com/rust-lang/crates.io-index` |
| `aws-lc-sys` | `0.44.0` | `ISC AND (Apache-2.0 OR ISC) AND Apache-2.0 AND MIT AND BSD-3-Clause AND (Apache-2.0 OR ISC OR MIT) AND (Apache-2.0 OR ISC OR MIT-0)` | `registry+https://github.com/rust-lang/crates.io-index` |
| `base64` | `0.21.7` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `base64` | `0.22.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `base64ct` | `1.8.3` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `bit-set` | `0.8.0` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `bit-vec` | `0.8.0` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `bitflags` | `1.3.2` | `MIT/Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `bitflags` | `2.13.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `block-buffer` | `0.10.4` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `block-buffer` | `0.12.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `block2` | `0.6.2` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `blocking` | `1.6.2` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `brotli` | `8.0.4` | `BSD-3-Clause AND MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `brotli-decompressor` | `5.0.3` | `BSD-3-Clause/MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `bs58` | `0.5.1` | `MIT/Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `bumpalo` | `3.20.3` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `bytemuck` | `1.25.2` | `Zlib OR Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `byteorder` | `1.5.0` | `Unlicense OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `bytes` | `1.12.1` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `cairo-rs` | `0.18.5` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `cairo-sys-rs` | `0.18.2` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `camino` | `1.2.5` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `cargo-platform` | `0.1.9` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `cargo_metadata` | `0.19.2` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `cargo_toml` | `0.22.3` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `cc` | `1.4.2` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `cesu8` | `1.1.0` | `Apache-2.0/MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `cfb` | `0.7.3` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `cfg-expr` | `0.15.8` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `cfg-if` | `1.0.4` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `chrono` | `0.4.45` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `cmake` | `0.1.58` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `cmpv2` | `0.2.0` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `cms` | `0.2.3` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `combine` | `4.6.7` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `concurrent-queue` | `2.5.0` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `const-oid` | `0.10.2` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `const-oid` | `0.9.6` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `cookie` | `0.18.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `core-foundation` | `0.10.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `core-foundation-sys` | `0.8.7` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `core-graphics` | `0.25.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `core-graphics-types` | `0.2.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `cpufeatures` | `0.2.17` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `cpufeatures` | `0.3.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `crc32fast` | `1.5.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `crmf` | `0.2.0` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `crossbeam-channel` | `0.5.16` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `crossbeam-utils` | `0.8.22` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `crypto-common` | `0.1.7` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `crypto-common` | `0.2.2` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `cssparser` | `0.36.0` | `MPL-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `cssparser-macros` | `0.6.1` | `MPL-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `ctor` | `0.8.0` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `ctor-proc-macro` | `0.0.7` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `curve25519-dalek` | `5.0.0` | `BSD-3-Clause` | `registry+https://github.com/rust-lang/crates.io-index` |
| `curve25519-dalek-derive` | `0.1.1` | `MIT/Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `darling` | `0.23.0` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `darling_core` | `0.23.0` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `darling_macro` | `0.23.0` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `dbus` | `0.9.12` | `Apache-2.0/MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `defmt` | `1.1.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `defmt-macros` | `1.1.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `defmt-parser` | `1.0.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `der` | `0.7.10` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `der` | `0.8.1` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `der_derive` | `0.7.3` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `deranged` | `0.5.8` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `derive_arbitrary` | `1.4.2` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `derive_more` | `2.1.1` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `derive_more-impl` | `2.1.1` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `digest` | `0.10.7` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `digest` | `0.11.3` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `dirs` | `6.0.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `dirs-sys` | `0.5.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `dispatch2` | `0.3.1` | `Zlib OR Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `displaydoc` | `0.2.7` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `dlopen2` | `0.8.2` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `dlopen2_derive` | `0.4.3` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `dom_query` | `0.27.0` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `dpi` | `0.1.2` | `Apache-2.0 AND MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `dtoa` | `1.0.11` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `dtoa-short` | `0.3.5` | `MPL-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `dtor` | `0.3.0` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `dtor-proc-macro` | `0.0.6` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `dunce` | `1.0.5` | `CC0-1.0 OR MIT-0 OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `dyn-clone` | `1.0.20` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `ed25519` | `3.0.0` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `ed25519-dalek` | `3.0.0` | `BSD-3-Clause` | `registry+https://github.com/rust-lang/crates.io-index` |
| `embed-resource` | `3.0.11` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `embed_plist` | `1.2.2` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `endi` | `1.1.1` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `enumflags2` | `0.7.12` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `enumflags2_derive` | `0.7.12` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `equivalent` | `1.0.2` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `erased-serde` | `0.4.10` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `errno` | `0.3.14` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `event-listener` | `5.4.2` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `event-listener-strategy` | `0.5.4` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `fallible-iterator` | `0.3.0` | `MIT/Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `fallible-streaming-iterator` | `0.1.9` | `MIT/Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `fastrand` | `2.5.0` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `fdeflate` | `0.3.7` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `fiat-crypto` | `0.3.0` | `MIT OR Apache-2.0 OR BSD-1-Clause` | `registry+https://github.com/rust-lang/crates.io-index` |
| `field-offset` | `0.3.6` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `filetime` | `0.2.29` | `MIT/Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `find-msvc-tools` | `0.1.10` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `flagset` | `0.4.7` | `Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `flate2` | `1.1.9` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `fnv` | `1.0.7` | `Apache-2.0 / MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `foldhash` | `0.2.0` | `Zlib` | `registry+https://github.com/rust-lang/crates.io-index` |
| `foreign-types` | `0.5.0` | `MIT/Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `foreign-types-macros` | `0.2.4` | `MIT/Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `foreign-types-shared` | `0.3.1` | `MIT/Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `form_urlencoded` | `1.2.2` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `fs_extra` | `1.3.0` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `futures-channel` | `0.3.33` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `futures-core` | `0.3.33` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `futures-executor` | `0.3.33` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `futures-io` | `0.3.33` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `futures-lite` | `2.6.1` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `futures-macro` | `0.3.33` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `futures-sink` | `0.3.33` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `futures-task` | `0.3.33` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `futures-util` | `0.3.33` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `gdk` | `0.18.2` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `gdk-pixbuf` | `0.18.5` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `gdk-pixbuf-sys` | `0.18.0` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `gdk-sys` | `0.18.2` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `gdkwayland-sys` | `0.18.2` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `gdkx11` | `0.18.2` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `gdkx11-sys` | `0.18.2` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `generic-array` | `0.14.7` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `getrandom` | `0.2.17` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `getrandom` | `0.3.4` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `getrandom` | `0.4.3` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `gio` | `0.18.4` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `gio-sys` | `0.18.1` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `glib` | `0.18.5` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `glib-macros` | `0.18.5` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `glib-sys` | `0.18.1` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `glob` | `0.3.4` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `gobject-sys` | `0.18.0` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `gtk` | `0.18.2` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `gtk-sys` | `0.18.2` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `gtk3-macros` | `0.18.2` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `hashbrown` | `0.12.3` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `hashbrown` | `0.16.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `hashbrown` | `0.17.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `hashlink` | `0.12.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `heck` | `0.4.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `heck` | `0.5.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `hermit-abi` | `0.5.2` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `hex` | `0.4.3` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `html5ever` | `0.38.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `http` | `1.5.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `http-body` | `1.1.0` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `http-body-util` | `0.1.4` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `httparse` | `1.10.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `hybrid-array` | `0.4.14` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `hyper` | `1.11.0` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `hyper-rustls` | `0.27.9` | `Apache-2.0 OR ISC OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `hyper-util` | `0.1.20` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `iana-time-zone` | `0.1.65` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `iana-time-zone-haiku` | `0.1.2` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `ico` | `0.5.0` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `icu_collections` | `2.2.0` | `Unicode-3.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `icu_locale_core` | `2.2.0` | `Unicode-3.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `icu_normalizer` | `2.2.0` | `Unicode-3.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `icu_normalizer_data` | `2.2.0` | `Unicode-3.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `icu_properties` | `2.2.0` | `Unicode-3.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `icu_properties_data` | `2.2.0` | `Unicode-3.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `icu_provider` | `2.2.0` | `Unicode-3.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `ident_case` | `1.0.1` | `MIT/Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `idna` | `1.1.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `idna_adapter` | `1.2.2` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `indexmap` | `1.9.3` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `indexmap` | `2.14.0` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `infer` | `0.19.0` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `ipnet` | `2.12.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `itoa` | `1.0.18` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `javascriptcore-rs` | `1.1.2` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `javascriptcore-rs-sys` | `1.1.1` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `jiff` | `0.2.35` | `Unlicense OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `jiff-core` | `0.1.0` | `Unlicense OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `jiff-static` | `0.2.35` | `Unlicense OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `jiff-tzdb` | `0.1.8` | `Unlicense OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `jiff-tzdb-platform` | `0.1.3` | `Unlicense OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `jni` | `0.21.1` | `MIT/Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `jni` | `0.22.4` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `jni-macros` | `0.22.4` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `jni-sys` | `0.3.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `jni-sys` | `0.4.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `jni-sys-macros` | `0.4.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `jobserver` | `0.1.35` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `js-sys` | `0.3.104` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `json-patch` | `3.0.1` | `MIT/Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `jsonptr` | `0.6.3` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `keyboard-types` | `0.7.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `libappindicator` | `0.9.0` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `libappindicator-sys` | `0.9.0` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `libc` | `0.2.189` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `libdbus-sys` | `0.2.7` | `Apache-2.0/MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `libloading` | `0.7.4` | `ISC` | `registry+https://github.com/rust-lang/crates.io-index` |
| `libredox` | `0.1.19` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `libsqlite3-sys` | `0.38.1` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `linux-raw-sys` | `0.12.1` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `litemap` | `0.8.2` | `Unicode-3.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `lock_api` | `0.4.14` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `log` | `0.4.33` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `markup5ever` | `0.38.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `memchr` | `2.8.3` | `Unlicense OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `memoffset` | `0.9.1` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `mime` | `0.3.17` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `minisign-verify` | `0.2.5` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `miniz_oxide` | `0.8.9` | `MIT OR Zlib OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `mio` | `1.2.2` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `muda` | `0.19.3` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `ndk` | `0.9.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `ndk-sys` | `0.6.0+11769913` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `new_debug_unreachable` | `1.0.6` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `num-conv` | `0.2.2` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `num-traits` | `0.2.19` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `num_enum` | `0.7.6` | `BSD-3-Clause OR MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `num_enum_derive` | `0.7.6` | `BSD-3-Clause OR MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `objc2` | `0.6.4` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `objc2-app-kit` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `objc2-cloud-kit` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `objc2-core-data` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `objc2-core-foundation` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `objc2-core-graphics` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `objc2-core-image` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `objc2-core-location` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `objc2-core-text` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `objc2-encode` | `4.1.0` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `objc2-exception-helper` | `0.1.1` | `Zlib OR Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `objc2-foundation` | `0.3.2` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `objc2-io-surface` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `objc2-osa-kit` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `objc2-quartz-core` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `objc2-ui-kit` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `objc2-user-notifications` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `objc2-web-kit` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `once_cell` | `1.21.4` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `openssl-probe` | `0.2.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `option-ext` | `0.2.0` | `MPL-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `ordered-stream` | `0.2.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `osakit` | `0.3.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `pango` | `0.18.3` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `pango-sys` | `0.18.0` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `parking` | `2.2.1` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `parking_lot` | `0.12.5` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `parking_lot_core` | `0.9.12` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `pem` | `3.0.6` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `pem-rfc7468` | `0.7.0` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `pem-rfc7468` | `1.0.0` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `percent-encoding` | `2.3.2` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `phf` | `0.13.1` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `phf_codegen` | `0.13.1` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `phf_generator` | `0.13.1` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `phf_macros` | `0.13.1` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `phf_shared` | `0.13.1` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `pin-project-lite` | `0.2.17` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `piper` | `0.2.5` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `pkcs8` | `0.11.0` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `pkg-config` | `0.3.33` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `plist` | `1.10.0` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `png` | `0.17.16` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `png` | `0.18.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `polling` | `3.11.0` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `portable-atomic` | `1.14.0` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `portable-atomic-util` | `0.2.7` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `potential_utf` | `0.1.5` | `Unicode-3.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `powerfmt` | `0.2.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `ppv-lite86` | `0.2.21` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `precomputed-hash` | `0.1.1` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `proc-macro-crate` | `1.3.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `proc-macro-crate` | `2.0.2` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `proc-macro-crate` | `3.5.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `proc-macro-error` | `1.0.4` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `proc-macro-error-attr` | `1.0.4` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `proc-macro2` | `1.0.107` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `quick-xml` | `0.41.0` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `quote` | `1.0.47` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `r-efi` | `5.3.0` | `MIT OR Apache-2.0 OR LGPL-2.1-or-later` | `registry+https://github.com/rust-lang/crates.io-index` |
| `r-efi` | `6.0.0` | `MIT OR Apache-2.0 OR LGPL-2.1-or-later` | `registry+https://github.com/rust-lang/crates.io-index` |
| `rand` | `0.9.5` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `rand_chacha` | `0.9.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `rand_core` | `0.6.4` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `rand_core` | `0.9.5` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `raw-window-handle` | `0.6.2` | `MIT OR Apache-2.0 OR Zlib` | `registry+https://github.com/rust-lang/crates.io-index` |
| `redox_syscall` | `0.5.18` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `redox_users` | `0.5.2` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `ref-cast` | `1.0.26` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `ref-cast-impl` | `1.0.26` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `regex` | `1.13.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `regex-automata` | `0.4.18` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `regex-syntax` | `0.8.11` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `reqwest` | `0.13.4` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `rfd` | `0.16.0` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `ring` | `0.17.14` | `Apache-2.0 AND ISC` | `registry+https://github.com/rust-lang/crates.io-index` |
| `rsqlite-vfs` | `0.1.1` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `rusqlite` | `0.40.1` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `rustc-hash` | `2.1.3` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `rustc_version` | `0.4.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `rustix` | `1.1.4` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `rustls` | `0.23.43` | `Apache-2.0 OR ISC OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `rustls-native-certs` | `0.8.4` | `Apache-2.0 OR ISC OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `rustls-pki-types` | `1.15.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `rustls-platform-verifier` | `0.7.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `rustls-platform-verifier-android` | `0.1.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `rustls-webpki` | `0.103.13` | `ISC` | `registry+https://github.com/rust-lang/crates.io-index` |
| `rustversion` | `1.0.23` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `ryu` | `1.0.23` | `Apache-2.0 OR BSL-1.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `ryu-js` | `1.0.3` | `Apache-2.0 OR BSL-1.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `same-file` | `1.0.6` | `Unlicense/MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `schannel` | `0.1.29` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `schemars` | `0.8.22` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `schemars` | `0.9.0` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `schemars` | `1.2.2` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `schemars_derive` | `0.8.22` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `scopeguard` | `1.2.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `security-framework` | `3.7.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `security-framework-sys` | `2.17.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `selectors` | `0.36.1` | `MPL-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `semver` | `1.0.28` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `serde` | `1.0.229` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `serde-untagged` | `0.1.9` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `serde_core` | `1.0.229` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `serde_derive` | `1.0.229` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `serde_derive_internals` | `0.29.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `serde_json` | `1.0.151` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `serde_json_canonicalizer` | `0.3.2` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `serde_repr` | `0.1.21` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `serde_spanned` | `0.6.9` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `serde_spanned` | `1.1.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `serde_urlencoded` | `0.7.1` | `MIT/Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `serde_with` | `3.21.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `serde_with_macros` | `3.21.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `serialize-to-javascript` | `0.1.2` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `serialize-to-javascript-impl` | `0.1.2` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `servo_arc` | `0.4.3` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `sha1` | `0.10.7` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `sha2` | `0.10.9` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `sha2` | `0.11.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `shlex` | `2.0.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `signal-hook-registry` | `1.4.8` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `signature` | `2.2.0` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `signature` | `3.0.0` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `sigstore-bundle` | `0.11.0` | `Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `sigstore-crypto` | `0.11.0` | `Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `sigstore-merkle` | `0.11.0` | `Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `sigstore-rekor` | `0.11.0` | `Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `sigstore-trust-root` | `0.11.0` | `Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `sigstore-tsa` | `0.11.0` | `Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `sigstore-types` | `0.11.0` | `Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `sigstore-verify` | `0.11.0` | `Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `simd-adler32` | `0.3.10` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `simd_cesu8` | `1.2.0` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `simdutf8` | `0.1.5` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `siphasher` | `1.0.3` | `MIT/Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `slab` | `0.4.12` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `smallvec` | `1.15.2` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `socket2` | `0.6.5` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `softbuffer` | `0.4.8` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `soup3` | `0.5.0` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `soup3-sys` | `0.5.0` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `spki` | `0.7.3` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `spki` | `0.8.0` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `sqlite-wasm-rs` | `0.5.5` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `stable_deref_trait` | `1.2.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `string_cache` | `0.9.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `string_cache_codegen` | `0.6.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `strsim` | `0.11.1` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `subtle` | `2.6.1` | `BSD-3-Clause` | `registry+https://github.com/rust-lang/crates.io-index` |
| `swift-rs` | `1.0.7` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `syn` | `1.0.109` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `syn` | `2.0.119` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `syn` | `3.0.3` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `sync_wrapper` | `1.0.2` | `Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `synstructure` | `0.13.2` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `system-deps` | `6.2.2` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `tao` | `0.35.3` | `Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `tao-macros` | `0.1.4` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `tar` | `0.4.46` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `target-lexicon` | `0.12.16` | `Apache-2.0 WITH LLVM-exception` | `registry+https://github.com/rust-lang/crates.io-index` |
| `tauri` | `2.11.5` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `tauri-build` | `2.6.3` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `tauri-codegen` | `2.6.3` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `tauri-macros` | `2.6.3` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `tauri-plugin` | `2.6.3` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `tauri-plugin-dialog` | `2.7.2` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `tauri-plugin-fs` | `2.5.1` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `tauri-plugin-single-instance` | `2.4.3` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `tauri-plugin-updater` | `2.10.1` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `tauri-runtime` | `2.11.3` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `tauri-runtime-wry` | `2.11.4` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `tauri-utils` | `2.9.3` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `tauri-winres` | `0.3.6` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `tempfile` | `3.27.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `tendril` | `0.5.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `thiserror` | `1.0.69` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `thiserror` | `2.0.20` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `thiserror-impl` | `1.0.69` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `thiserror-impl` | `2.0.20` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `time` | `0.3.55` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `time-core` | `0.1.9` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `time-macros` | `0.2.32` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `tinystr` | `0.8.3` | `Unicode-3.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `tinyvec` | `1.12.0` | `Zlib OR Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `tinyvec_macros` | `0.1.1` | `MIT OR Apache-2.0 OR Zlib` | `registry+https://github.com/rust-lang/crates.io-index` |
| `tls_codec` | `0.4.2` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `tls_codec_derive` | `0.4.2` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `tokio` | `1.53.1` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `tokio-rustls` | `0.26.4` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `tokio-util` | `0.7.19` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `toml` | `0.8.2` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `toml` | `0.9.12+spec-1.1.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `toml` | `1.1.4+spec-1.1.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `toml_datetime` | `0.6.3` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `toml_datetime` | `0.7.5+spec-1.1.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `toml_datetime` | `1.1.1+spec-1.1.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `toml_edit` | `0.19.15` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `toml_edit` | `0.20.2` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `toml_edit` | `0.25.13+spec-1.1.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `toml_parser` | `1.1.3+spec-1.1.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `toml_writer` | `1.1.2+spec-1.1.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `tower` | `0.5.3` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `tower-http` | `0.6.11` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `tower-layer` | `0.3.3` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `tower-service` | `0.3.3` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `tracing` | `0.1.44` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `tracing-attributes` | `0.1.31` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `tracing-core` | `0.1.36` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `tray-icon` | `0.24.2` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `try-lock` | `0.2.5` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `typeid` | `1.0.3` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `typenum` | `1.20.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `uds_windows` | `1.2.1` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `unic-char-property` | `0.9.0` | `MIT/Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `unic-char-range` | `0.9.0` | `MIT/Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `unic-common` | `0.9.0` | `MIT/Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `unic-ucd-ident` | `0.9.0` | `MIT/Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `unic-ucd-version` | `0.9.0` | `MIT/Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `unicode-ident` | `1.0.24` | `(MIT OR Apache-2.0) AND Unicode-3.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `unicode-segmentation` | `1.13.3` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `untrusted` | `0.7.1` | `ISC` | `registry+https://github.com/rust-lang/crates.io-index` |
| `untrusted` | `0.9.0` | `ISC` | `registry+https://github.com/rust-lang/crates.io-index` |
| `url` | `2.5.8` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `urlpattern` | `0.3.0` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `utf8_iter` | `1.0.4` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `uuid` | `1.24.0` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `vcpkg` | `0.2.15` | `MIT/Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `version-compare` | `0.2.1` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `version_check` | `0.9.5` | `MIT/Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `vswhom` | `0.1.0` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `vswhom-sys` | `0.1.3` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `walkdir` | `2.5.0` | `Unlicense/MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `want` | `0.3.1` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `wasi` | `0.11.1+wasi-snapshot-preview1` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `wasip2` | `1.0.4+wasi-0.2.12` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `wasm-bindgen` | `0.2.127` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `wasm-bindgen-futures` | `0.4.77` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `wasm-bindgen-macro` | `0.2.127` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `wasm-bindgen-macro-support` | `0.2.127` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `wasm-bindgen-shared` | `0.2.127` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `wasm-streams` | `0.5.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `web-sys` | `0.3.104` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `web_atoms` | `0.2.5` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `webkit2gtk` | `2.0.2` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `webkit2gtk-sys` | `2.0.2` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `webpki-root-certs` | `1.0.9` | `CDLA-Permissive-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `webview2-com` | `0.38.2` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `webview2-com-macros` | `0.8.1` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `webview2-com-sys` | `0.38.2` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `winapi` | `0.3.9` | `MIT/Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `winapi-i686-pc-windows-gnu` | `0.4.0` | `MIT/Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `winapi-util` | `0.1.11` | `Unlicense OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `winapi-x86_64-pc-windows-gnu` | `0.4.0` | `MIT/Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `window-vibrancy` | `0.6.0` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows` | `0.61.3` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows-collections` | `0.2.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows-core` | `0.61.2` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows-core` | `0.62.2` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows-future` | `0.2.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows-implement` | `0.60.2` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows-interface` | `0.59.3` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows-link` | `0.1.3` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows-link` | `0.2.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows-numerics` | `0.2.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows-result` | `0.3.4` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows-result` | `0.4.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows-strings` | `0.4.2` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows-strings` | `0.5.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows-sys` | `0.45.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows-sys` | `0.52.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows-sys` | `0.59.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows-sys` | `0.60.2` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows-sys` | `0.61.2` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows-targets` | `0.42.2` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows-targets` | `0.52.6` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows-targets` | `0.53.5` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows-threading` | `0.1.0` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows-version` | `0.1.7` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows_aarch64_gnullvm` | `0.42.2` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows_aarch64_gnullvm` | `0.52.6` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows_aarch64_gnullvm` | `0.53.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows_aarch64_msvc` | `0.42.2` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows_aarch64_msvc` | `0.52.6` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows_aarch64_msvc` | `0.53.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows_i686_gnu` | `0.42.2` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows_i686_gnu` | `0.52.6` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows_i686_gnu` | `0.53.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows_i686_gnullvm` | `0.52.6` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows_i686_gnullvm` | `0.53.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows_i686_msvc` | `0.42.2` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows_i686_msvc` | `0.52.6` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows_i686_msvc` | `0.53.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows_x86_64_gnu` | `0.42.2` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows_x86_64_gnu` | `0.52.6` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows_x86_64_gnu` | `0.53.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows_x86_64_gnullvm` | `0.42.2` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows_x86_64_gnullvm` | `0.52.6` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows_x86_64_gnullvm` | `0.53.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows_x86_64_msvc` | `0.42.2` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows_x86_64_msvc` | `0.52.6` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `windows_x86_64_msvc` | `0.53.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `winnow` | `0.5.40` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `winnow` | `0.7.15` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `winnow` | `1.0.4` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `winreg` | `0.55.0` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `wit-bindgen` | `0.57.1` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `writeable` | `0.6.3` | `Unicode-3.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `wry` | `0.55.1` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `x11` | `2.21.0` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `x11-dl` | `2.21.0` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `x509-cert` | `0.2.5` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `x509-tsp` | `0.1.0` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `xattr` | `1.6.1` | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `yoke` | `0.8.3` | `Unicode-3.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `yoke-derive` | `0.8.2` | `Unicode-3.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `zbus` | `5.18.0` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `zbus_macros` | `5.18.0` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `zbus_names` | `4.3.4` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `zerocopy` | `0.8.56` | `BSD-2-Clause OR Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `zerocopy-derive` | `0.8.56` | `BSD-2-Clause OR Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `zerofrom` | `0.1.8` | `Unicode-3.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `zerofrom-derive` | `0.1.7` | `Unicode-3.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `zeroize` | `1.9.0` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `zeroize_derive` | `1.5.0` | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `zerotrie` | `0.2.4` | `Unicode-3.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `zerovec` | `0.11.6` | `Unicode-3.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `zerovec-derive` | `0.11.3` | `Unicode-3.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| `zip` | `4.6.1` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `zmij` | `1.0.23` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `zvariant` | `5.13.1` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `zvariant_derive` | `5.13.1` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| `zvariant_utils` | `3.5.0` | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |

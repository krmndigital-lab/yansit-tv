# NOTICE — Ayna (`com.yansit.tv`)

Ayna — wireless screen-mirroring receiver for Android TV and Google TV
Copyright (C) 2026 KRMN Digital

This program is free software: you can redistribute it and/or modify it under the terms of the
GNU General Public License as published by the Free Software Foundation, either version 3 of the
License, or (at your option) any later version.

This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without
even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU
General Public License for more details.

You should have received a copy of the GNU General Public License along with this program. If not,
see <https://www.gnu.org/licenses/>.

Complete corresponding source code: https://github.com/krmndigital-lab/yansit-tv (see `SOURCE-OFFER.md`).

> **Where this notice must appear:** in the app's About screen (together with the full license
> texts), at the root of the public source repository, and linked from the Google Play listing,
> whose "Open source" paragraph carries the short form.

## 1. Upstream projects

### UxPlay — GNU GPL v3

<https://github.com/FDH2/UxPlay>

The AirPlay receiver in `src/airplay/` is a Rust re-implementation that follows UxPlay's C
sources module by module, as each file header states:

| Ayna module | Follows UxPlay |
|---|---|
| `src/airplay/pairing.rs` | `lib/pairing.c` |
| `src/airplay/pin.rs` | `lib/srp.c`, `lib/raop_handlers.h` |
| `src/airplay/mirror.rs` | `lib/mirror_buffer.c`, `lib/raop_rtp_mirror.c` |
| `src/airplay/audio.rs` | `lib/raop_rtp.c`, `lib/raop_buffer.c` |
| `src/airplay/plistx.rs`, `src/airplay/txt.rs` | `lib/raop_handlers.h` (`/info` reply), AirPlay feature flags |

UxPlay is copyright its authors and contributors. It builds on RPiPlay
(<https://github.com/FD-/RPiPlay>) and shairplay (<https://github.com/juhovh/shairplay>), whose
authors are credited in UxPlay's sources and documentation. Ayna is not affiliated with these
projects.

### shairplay-rust — GNU LGPL v3 or later

The FairPlay (fp-setup / SAP) module in `src/airplay/fairplay/` is vendored from shairplay-rust
under LGPL-3.0-or-later (upstream: `https://github.com/thebracket/shairplay-rust`). It contains a Rust port of
**playfair** (<https://github.com/EstebanKubata/playfair>), the FairPlay decryption code by
EstebanKubata; keep playfair's copyright notice and license terms with that code. LGPL-3.0-or-later
code may be combined into a GPL-3.0-or-later work, and the combined work is distributed under
GPL-3.0-or-later.

## 2. Bundled fonts

| Asset | Project | License |
|---|---|---|
| `assets/fonts/InterVariable-om.ttf`, `assets/fonts/InterVariable-tnum-om.ttf` (subsets) | Inter, © The Inter Project Authors — <https://github.com/rsms/inter> | SIL Open Font License 1.1 |
| `assets/fonts/MaterialSymbolsRounded-kare.ttf` (subset) | Material Symbols, © Google LLC — <https://github.com/google/material-design-icons> | Apache License 2.0 |

## 3. Rust crates compiled into `libyansit.so`

Generated 2026-11-03 from `Cargo.lock` with `cargo metadata --filter-platform
aarch64-linux-android`. It lists normal dependencies, transitively; proc-macro and build-only crates
(which run only at compile time) and dev-dependencies are excluded. The armeabi-v7a library uses the
same set except the crates marked ¹. Every license below is compatible with GPL-3.0. Multi-licensed
crates are used under any of their listed licenses (`self_cell`: under Apache-2.0).

| Crate | Version | License (SPDX, as declared) |
|---|---|---|
| `accesskit` | 0.25.2 | MIT OR Apache-2.0 |
| `adler2` | 2.0.1 | 0BSD OR MIT OR Apache-2.0 |
| `aead` | 0.5.2 | MIT OR Apache-2.0 |
| `aes` | 0.8.4 | MIT OR Apache-2.0 |
| `aes-gcm` | 0.10.3 | Apache-2.0 OR MIT |
| `ahash` | 0.8.12 | MIT OR Apache-2.0 |
| `aho-corasick` | 1.1.5 | Unlicense OR MIT |
| `android_log-sys` | 0.3.2 | MIT OR Apache-2.0 |
| `android_logger` | 0.15.1 | MIT OR Apache-2.0 |
| `arrayvec` | 0.7.8 | MIT OR Apache-2.0 |
| `base64` | 0.23.1 | MIT OR Apache-2.0 |
| `base64ct` | 1.8.3 | Apache-2.0 OR MIT |
| `bitflags` | 2.13.2 | MIT OR Apache-2.0 |
| `block-buffer` | 0.10.4 | MIT OR Apache-2.0 |
| `block-padding` | 0.3.3 | MIT OR Apache-2.0 |
| `bytemuck` | 1.25.2 | Zlib OR Apache-2.0 OR MIT |
| `byteorder-lite` | 0.1.0 | Unlicense OR MIT |
| `bytes` | 1.12.1 | MIT |
| `cbc` | 0.1.2 | MIT OR Apache-2.0 |
| `cesu8` | 1.1.0 | Apache-2.0/MIT |
| `cfg-if` | 1.0.5 | MIT OR Apache-2.0 |
| `cipher` | 0.4.4 | MIT OR Apache-2.0 |
| `color` | 0.3.3 | Apache-2.0 OR MIT |
| `combine` | 4.6.9 | MIT |
| `const-oid` | 0.9.6 | Apache-2.0 OR MIT |
| `cpufeatures` ¹ | 0.2.17 | MIT OR Apache-2.0 |
| `crc32fast` | 1.5.4 | MIT OR Apache-2.0 |
| `crypto-common` | 0.1.7 | MIT OR Apache-2.0 |
| `ctr` | 0.9.2 | MIT OR Apache-2.0 |
| `curve25519-dalek` | 4.1.3 | BSD-3-Clause |
| `der` | 0.7.10 | Apache-2.0 OR MIT |
| `deranged` | 0.5.9 | MIT OR Apache-2.0 |
| `digest` | 0.10.7 | MIT OR Apache-2.0 |
| `ecolor` | 0.37.0 | MIT OR Apache-2.0 |
| `ed25519` | 2.2.3 | Apache-2.0 OR MIT |
| `ed25519-dalek` | 2.2.0 | BSD-3-Clause |
| `egui` | 0.37.0 | MIT OR Apache-2.0 |
| `egui_glow` | 0.37.0 | MIT OR Apache-2.0 |
| `either` | 1.19.0 | MIT OR Apache-2.0 |
| `emath` | 0.37.0 | MIT OR Apache-2.0 |
| `env_filter` | 0.1.4 | MIT OR Apache-2.0 |
| `epaint` | 0.37.0 | MIT OR Apache-2.0 |
| `equivalent` | 1.0.3 | Apache-2.0 OR MIT |
| `euclid` | 0.22.14 | MIT OR Apache-2.0 |
| `fdeflate` | 0.3.7 | MIT OR Apache-2.0 |
| `fearless_simd` | 0.4.1 | Apache-2.0 OR MIT |
| `flate2` | 1.1.10 | MIT OR Apache-2.0 |
| `foldhash` | 0.2.0 | Zlib |
| `font-types` | 0.12.7 | MIT OR Apache-2.0 |
| `generic-array` | 0.14.7 | MIT |
| `getrandom` | 0.2.17 | MIT OR Apache-2.0 |
| `ghash` | 0.5.1 | Apache-2.0 OR MIT |
| `glifo` | 0.3.0 | Apache-2.0 OR MIT |
| `glow` | 0.17.0 | MIT OR Apache-2.0 OR Zlib |
| `guillotiere` | 0.7.0 | MIT/Apache-2.0 |
| `harfrust` | 0.12.0 | MIT |
| `hashbrown` | 0.17.1 | MIT OR Apache-2.0 |
| `image` | 0.25.10 | MIT OR Apache-2.0 |
| `indexmap` | 2.14.2 | Apache-2.0 OR MIT |
| `inout` | 0.1.4 | MIT OR Apache-2.0 |
| `itertools` | 0.15.0 | MIT OR Apache-2.0 |
| `jni` | 0.21.1 | MIT/Apache-2.0 |
| `jni-sys` | 0.3.1 | MIT OR Apache-2.0 |
| `jni-sys` | 0.4.1 | MIT OR Apache-2.0 |
| `khronos-egl` | 6.0.0 | MIT/Apache-2.0 |
| `kurbo` | 0.13.1 | Apache-2.0 OR MIT |
| `libc` | 0.2.191 | MIT OR Apache-2.0 |
| `libloading` | 0.8.9 | ISC |
| `linebender_resource_handle` | 0.1.1 | Apache-2.0 OR MIT |
| `lock_api` | 0.4.14 | MIT OR Apache-2.0 |
| `log` | 0.4.35 | MIT OR Apache-2.0 |
| `memchr` | 2.8.3 | Unlicense OR MIT |
| `miniz_oxide` | 0.8.9 | MIT OR Zlib OR Apache-2.0 |
| `miniz_oxide` | 0.9.1 | MIT OR Zlib OR Apache-2.0 |
| `moxcms` | 0.8.1 | BSD-3-Clause OR Apache-2.0 |
| `ndk` | 0.9.0 | MIT OR Apache-2.0 |
| `ndk-sys` | 0.6.0+11769913 | MIT OR Apache-2.0 |
| `nohash-hasher` | 0.2.0 | Apache-2.0 OR MIT |
| `num-bigint` | 0.4.8 | MIT OR Apache-2.0 |
| `num-conv` | 0.2.2 | MIT OR Apache-2.0 |
| `num-integer` | 0.1.47 | MIT OR Apache-2.0 |
| `num-traits` | 0.2.19 | MIT OR Apache-2.0 |
| `num_enum` | 0.7.6 | BSD-3-Clause OR MIT OR Apache-2.0 |
| `once_cell` | 1.21.4 | MIT OR Apache-2.0 |
| `opaque-debug` | 0.3.1 | MIT OR Apache-2.0 |
| `parking_lot` | 0.12.5 | MIT OR Apache-2.0 |
| `parking_lot_core` | 0.9.12 | MIT OR Apache-2.0 |
| `peniko` | 0.6.1 | Apache-2.0 OR MIT |
| `pkcs8` | 0.10.2 | Apache-2.0 OR MIT |
| `plist` | 1.10.3 | MIT |
| `png` | 0.18.1 | MIT OR Apache-2.0 |
| `polycool` | 0.4.0 | MIT OR Apache-2.0 |
| `polyval` | 0.6.2 | Apache-2.0 OR MIT |
| `powerfmt` | 0.2.1 | MIT OR Apache-2.0 |
| `profiling` | 1.0.18 | MIT OR Apache-2.0 |
| `pxfm` | 0.1.30 | BSD-3-Clause OR Apache-2.0 |
| `quick-xml` | 0.42.0 | MIT |
| `rand_core` | 0.6.4 | MIT OR Apache-2.0 |
| `read-fonts` | 0.41.0 | MIT OR Apache-2.0 |
| `regex` | 1.13.1 | MIT OR Apache-2.0 |
| `regex-automata` | 0.4.18 | MIT OR Apache-2.0 |
| `regex-syntax` | 0.8.11 | MIT OR Apache-2.0 |
| `scopeguard` | 1.2.0 | MIT OR Apache-2.0 |
| `self_cell` | 1.3.0 | Apache-2.0 OR GPL-2.0-only |
| `serde` | 1.0.229 | MIT OR Apache-2.0 |
| `serde_core` | 1.0.229 | MIT OR Apache-2.0 |
| `sha1` | 0.10.7 | MIT OR Apache-2.0 |
| `sha2` | 0.10.9 | MIT OR Apache-2.0 |
| `signature` | 2.2.0 | Apache-2.0 OR MIT |
| `simd-adler32` | 0.3.10 | MIT |
| `skrifa` | 0.44.0 | MIT OR Apache-2.0 |
| `smallvec` | 1.16.4 | MIT OR Apache-2.0 |
| `spki` | 0.7.3 | Apache-2.0 OR MIT |
| `subtle` | 2.6.1 | BSD-3-Clause |
| `thiserror` | 1.0.69 | MIT OR Apache-2.0 |
| `thiserror` | 2.0.21 | MIT OR Apache-2.0 |
| `time` | 0.3.55 | MIT OR Apache-2.0 |
| `time-core` | 0.1.9 | MIT OR Apache-2.0 |
| `typenum` | 1.20.1 | MIT OR Apache-2.0 |
| `unicode-general-category` | 1.1.0 | Apache-2.0 |
| `unicode-properties` | 0.1.5 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| `unicode-segmentation` | 1.13.3 | MIT OR Apache-2.0 |
| `universal-hash` | 0.5.1 | MIT OR Apache-2.0 |
| `uuid` | 1.29.0 | Apache-2.0 OR MIT |
| `vello_common` | 0.2.0 | Apache-2.0 OR MIT |
| `vello_cpu` | 0.2.0 | Apache-2.0 OR MIT |
| `x25519-dalek` | 2.0.1 | BSD-3-Clause |
| `zerocopy` | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT |
| `zeroize` | 1.9.1 | Apache-2.0 OR MIT |
| `zlib-rs` | 0.6.8 | Zlib |
| `zune-core` | 0.5.3 | MIT OR Apache-2.0 OR Zlib |
| `zune-jpeg` | 0.5.15 | MIT OR Apache-2.0 OR Zlib |

¹ arm64-v8a only.

## 4. Google libraries in the Android shell (not under the GPL)

When the Android shell bundles them, the **Google Play Billing Library** and the **Google Mobile
Ads SDK** (with their Google Play services dependencies) are proprietary and used under Google's
terms. They are not part of Ayna's source and are not licensed under the GPL. See
`SOURCE-OFFER.md` → "Maintainer checklist" for the licensing caveat.

## 5. Android platform libraries

Ayna uses Android system libraries (`libmediandk`, AAudio, EGL / OpenGL ES, `liblog`, JNI).
They are part of the operating system, are not distributed with the app, and are System Libraries
in the sense of GPLv3 §1.

## 6. Trademarks

AirPlay is a trademark of Apple Inc., registered in the U.S. and other countries. Miracast is a
registered trademark of Wi-Fi Alliance. DLNA is a trademark of the Digital Living Network Alliance.
Android, Google TV, Google Cast and Chromecast are trademarks of Google LLC. Windows is a trademark
of Microsoft Corporation. Ayna is independent and is not affiliated with, or endorsed by, these
companies. Protocol names describe compatibility only.

## 7. Regenerating this list and the full license texts

```sh
cargo metadata --offline --format-version 1 --filter-platform aarch64-linux-android > meta.json
# walk resolve.nodes from resolve.root over edges whose dep_kinds contain kind=null,
# skipping packages whose targets are "proc-macro"; print name|version|license

# full license texts with copyright lines (add the two Android targets to about.toml):
cargo install cargo-about
cargo about init
cargo about generate about.hbs > THIRD-PARTY-LICENSES.html
```

Ship the full texts of GPL-3.0, LGPL-3.0, OFL-1.1, Apache-2.0, MIT, BSD-2-Clause, BSD-3-Clause,
ISC, Zlib, 0BSD, Unlicense and Unicode-3.0 (with each crate's copyright line) in the app's
About → Licenses screen and in the repository.

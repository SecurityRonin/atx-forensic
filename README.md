# atx-forensic

[![atx-core](https://img.shields.io/crates/v/atx-core.svg?label=atx-core)](https://crates.io/crates/atx-core)
[![Docs.rs](https://img.shields.io/docsrs/atx-core)](https://docs.rs/atx-core)
[![Rust 1.80+](https://img.shields.io/badge/rust-1.80%2B-orange.svg)](https://www.rust-lang.org)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache--2.0-blue.svg)](LICENSE)
[![Sponsor](https://img.shields.io/badge/sponsor-h4x0r-ea4aaa?logo=github-sponsors)](https://github.com/sponsors/h4x0r)

[![CI](https://github.com/SecurityRonin/atx-forensic/actions/workflows/ci.yml/badge.svg)](https://github.com/SecurityRonin/atx-forensic/actions)
[![unsafe forbidden](https://img.shields.io/badge/unsafe-forbidden-success.svg)](https://github.com/rust-secure-code/safety-dance)
[![Security advisories](https://img.shields.io/badge/advisories-clean-success.svg)](deny.toml)

**Read Apple ATX (`AAPL`) texture containers — the iOS image caches behind
PosterBoard snapshots, wallpapers, contact posters, and Animoji avatars — and
decode their ASTC payloads to RGBA, in one `forbid(unsafe)` Rust crate. ATX files
are *what was on screen*; `atx-core` turns the container back into the picture.**

> **Status: validated on real device textures (tier-1).** Decodes **108 real
> `.atx` files** from a genuine iPhone 11 / iOS 17.3 extraction, matching the
> independent iLEAPP reference to **within one LSB per channel on every file**,
> across both payload paths — see [Trust but verify](#trust-but-verify).

## Decode an ATX texture

```toml
[dependencies]
atx-core = "0.1"
```

```rust
use atx_core::{decode, parse, FormatConfidence};

let bytes = std::fs::read("snapshot.atx")?;

// Metadata only — parse the container without decoding pixels.
let atx = parse(&bytes)?;
if let Some(head) = &atx.head {
    println!("{}x{}  pixel-format {:?}", head.width, head.height, head.pixel_format);
}
for w in &atx.warnings {
    eprintln!("warning: {w}");   // fail-loud: malformed chunks are surfaced, never silent
}

// Full decode to RGBA8.
let img = decode(&bytes)?;
println!("{}x{} RGBA — format {:?}", img.width, img.height, img.confidence);
match img.confidence {
    FormatConfidence::Confirmed => {}  // (3,5): ASTC 4x4 asserted by the format
    FormatConfidence::Inferred  => {}  // (1,1)/(3,1): decoded as ASTC 4x4, not format-asserted
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

## What an ATX file is

A chunked `AAPL` container (PNG-style 8-byte signature `AAPL\r\n\x1a\n`, then
`[size u32 LE][tag][payload]` chunks to EOF):

```text
AAPL\r\n\x1a\n   HEAD   FILL   astc/ASTC | LZFS   ...
```

- **`HEAD`** — metadata: width, height, depth, array-layer and mipmap counts, a
  texture UUID, and a pixel-format discriminator pair.
- **payload** — **ASTC**-compressed texture, mostly ASTC 4x4. A `LZFS` chunk wraps
  **LZFSE**-compressed ASTC (seen around avatar/Animoji resources).
- **the catch** — raw `astc`/`ASTC` blocks are **macro-tiled** (32x32-block tiles,
  Morton-ordered, with an X/Y interpretation the format does not flag). Decoded
  linearly they produce a visually shuffled image; `atx-core` de-tiles them. An
  `LZFS` payload decompresses to an already-linear stream — no de-tiling.

The codecs are **reused, never reinvented**: [`lzfse_rust`](https://crates.io/crates/lzfse_rust)
(the fleet's LZFSE decoder) and [`astc-decode`](https://crates.io/crates/astc-decode).
The crate's own value-add is the `AAPL` container parse, the HEAD field layout,
and the Morton de-tiling. The byte layout is reimplemented clean-room from
[abrignoni/iLEAPP](https://github.com/abrignoni/iLEAPP)'s `apple_atx.py` (MIT,
@JamesHabben), the reference cited by the source write-up
([James Habben, 2026-06-26](https://leapps.org/blog-post?post=2026-06-26-decoding-apple-atx-images)).

## Trust but verify

No `unsafe` (`unsafe_code = "forbid"`), no C bindings, paranoid lints (no
`unwrap`/`expect` in production), and a parser that **fails loud** — a bad magic
errors with the offending bytes; malformed chunks after a valid magic degrade to
`Atx::warnings`, never a silent empty result.

Validation is honestly tiered (Doer-Checker), and now **tier-1**:

- **Real artifact + independent oracle.** 108 real `.atx` from a public iPhone 11
  / iOS 17.3 full-file-system image (Josh Hickman's research device) decode to RGBA
  that matches the iLEAPP reference (a different author *and* a different ASTC
  decoder) to **≤1 LSB per channel on all 108** — including the 48 raw
  macro-tiled posters/wallpapers where the Morton de-tile orientation matters. The
  ±1 is rounding between two independent decoders, not a layout error. Full
  methodology, corpus provenance, and per-path results in
  [`docs/validation.md`](docs/validation.md).
- **Scope of the claim.** The container parse, HEAD layout, payload framing, LZFSE
  path, de-tile/orientation, crop, and format classification are oracle-confirmed
  on this corpus (one device, one OS version, all ASTC 4x4). The absolute ASTC
  pixel math is each decoder's own concern; ±1-LSB agreement between two unrelated
  decoders corroborates it.

**Epistemics.** Report the pixel format as *confirmed* vs *inferred* — never an
inference as fact. A file's path (`PosterSnapshots`, `PRBPosterExtensionDataStore`,
…) does not make it the *active* wallpaper: report the image, metadata, and source
path; state what the container holds, not what it means.

## Scope

`atx-core` is the reader/decoder. The `atx-forensic` analyzer half is deferred —
ATX's forensic value is the decoded *content*, not a structural anomaly to audit —
and will be added only if a real auditor emerges.

---

[Privacy Policy](https://securityronin.github.io/atx-forensic/privacy/) · [Terms of Service](https://securityronin.github.io/atx-forensic/terms/) · © 2026 Security Ronin Ltd

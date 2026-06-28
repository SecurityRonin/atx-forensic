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

> **Status: decoder built, pixel-validation pending.** The container parse, HEAD
> layout, payload framing, and Morton de-tile are implemented and cross-checked
> against the iLEAPP reference as an oracle (tier-2). The end-to-end image decode
> is **not yet diffed pixel-for-pixel against a real device texture** — see
> [Trust but verify](#trust-but-verify). Decoded pixels are wired, not yet proven.

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

Validation is honestly tiered (Doer-Checker):

- **Confirmed (tier-2):** the container parse, HEAD offsets, payload framing, and
  the Morton de-tile permutation are cross-checked byte-for-byte against the
  iLEAPP reference as an independent oracle — but on *synthetic* containers built
  to the documented layout.
- **Not yet confirmed:** that the end-to-end decode produces the *visually
  correct* image on a real device texture. Closing this needs genuine `.atx`
  samples from an iOS extraction plus a PNG diff against iLEAPP's output. Until
  then, do not present a decoded image as oracle-confirmed.

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

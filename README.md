# atx-forensic

**Read Apple ATX (`AAPL`) texture containers — the iOS image caches that hold what was on screen.**

ATX files are Apple's "AAPL" texture-image containers, found across iOS UI image
caches: PosterBoard / runtime **snapshots**, **wallpapers**, **contact posters**,
**avatars / Animoji**. `atx-core` parses the container and (build-out in progress)
decodes the ASTC texture payload to RGBA/PNG, so a forensic timeline can recover
the actual images a device displayed.

> **Status: pre-release.** Container parse (8-byte magic, framed chunk walk),
> `HEAD` metadata, payload framing, the Morton de-tile, and the full texture
> **decode** pipeline (LZFSE → ASTC 4x4 → de-tile → RGBA) are implemented and
> tested — clean-room from, and cross-validated against, the iLEAPP reference as
> an independent oracle. **Not yet pixel-validated against a real device sample +
> iLEAPP's PNG output** (the remaining boundary — see [`HANDOFF.md`](HANDOFF.md)
> §5); treat decoded images as unverified on real ASTC content until then. Not
> yet published to crates.io.

## What it does today

```rust
use atx_core::{is_atx, parse, decode, astc4x4_confidence, FormatConfidence};

assert!(is_atx(b"AAPL\r\n\x1a\nrest")); // full 8-byte AAPL signature

// Parse the container: framed chunk walk + HEAD metadata + payload, with
// non-fatal anomalies surfaced as warnings (never a silent empty).
let atx = parse(bytes)?;
if let Some(head) = &atx.head {
    println!("{}x{} pixel_format {:?}", head.width, head.height, head.pixel_format);
}

// Decode the texture to RGBA8 (carries the confirmed-vs-inferred confidence).
let img = decode(bytes)?;
assert_eq!(img.rgba.len() as u32, img.width * img.height * 4);

// Report the pixel format honestly — confirmed vs inferred, never a guess.
assert_eq!(astc4x4_confidence((3, 5)), Some(FormatConfidence::Confirmed));
assert_eq!(astc4x4_confidence((1, 1)), Some(FormatConfidence::Inferred));
assert_eq!(astc4x4_confidence((9, 9)), None); // unknown pair → surface the raw bytes
```

## The format (from the source research)

A chunked `AAPL` container (8-byte `AAPL\r\n\x1a\n` signature, then framed
`[size][tag]` chunks) — `HEAD` (metadata: dimensions, UUID, pixel-format
discriminator), `FILL`, an `astc`/`ASTC` payload (mostly ASTC 4x4), and `LZFS`
(LZFSE-wrapped ASTC). A raw `astc`/`ASTC` payload is macro-tiled in 32×32-block
Morton order (X/Y interpretation chosen by a grid-seam heuristic), so its pipeline
is ASTC blocks → de-tile → ASTC 4x4 → RGBA. An `LZFS` payload is
LZFSE-decompressed to an already-*linear* ASTC stream (no de-tile) → ASTC 4x4 →
RGBA. Source: [Decoding Apple ATX Images in iLEAPP (James Habben,
2026-06-26)](https://leapps.org/blog-post?post=2026-06-26-decoding-apple-atx-images).

Codecs are reused, not reinvented: [`lzfse_rust`](https://crates.io/crates/lzfse_rust)
(the fleet's LZFSE decoder) and [`astc-decode`](https://crates.io/crates/astc-decode).
The new code is the container parse, the `HEAD` layout, and the Morton de-tiling.

## Epistemics

- The pixel format is reported as **confirmed** (`(3,5)`) vs **inferred**
  (`(1,1)`/`(3,1)`) — an inference is never dressed as a confirmed fact.
- **A path is not an assignment.** A file under a PosterBoard-ish path is not, by
  that fact, the *active* wallpaper. `atx-core` reports the image, its metadata,
  and its source path — it states what the container holds, not what it means.

## Build

```
cargo test                                 # parsing + confidence tests
cargo clippy --all-targets -- -D warnings  # Paranoid Gatekeeper lints
```

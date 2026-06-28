# atx-forensic

**Read Apple ATX (`AAPL`) texture containers — the iOS image caches that hold what was on screen.**

ATX files are Apple's "AAPL" texture-image containers, found across iOS UI image
caches: PosterBoard / runtime **snapshots**, **wallpapers**, **contact posters**,
**avatars / Animoji**. `atx-core` parses the container and (build-out in progress)
decodes the ASTC texture payload to RGBA/PNG, so a forensic timeline can recover
the actual images a device displayed.

> **Status: scaffold.** Container recognition, chunk inventory, and the
> pixel-format confidence model are implemented and tested. The texture **decode**
> (LZFSE → ASTC → Morton de-tile) returns `Unimplemented` until it is built out
> against real `.atx` samples and the iLEAPP reference — see
> [`HANDOFF.md`](HANDOFF.md). Not yet published to crates.io.

## What it does today

```rust
use atx_core::{is_atx, parse, astc4x4_confidence, FormatConfidence};

assert!(is_atx(b"AAPL\x00\x01"));

// Inventory the chunks a container carries (heuristic FourCC scan for now).
let chunks = parse(bytes)?;

// Report the pixel format honestly — confirmed vs inferred, never a guess.
assert_eq!(astc4x4_confidence((3, 5)), Some(FormatConfidence::Confirmed));
assert_eq!(astc4x4_confidence((1, 1)), Some(FormatConfidence::Inferred));
assert_eq!(astc4x4_confidence((9, 9)), None); // unknown pair → surface the raw bytes
```

## The format (from the source research)

A chunked `AAPL` container — `HEAD` (metadata: dimensions, UUID, pixel-format
discriminator), `FILL`, an `astc`/`ASTC` payload (mostly ASTC 4x4), and `LZFS`
(LZFSE-wrapped ASTC). The ASTC blocks are macro-tiled in Morton order with an X/Y
swap, so the decode pipeline is `LZFS` → LZFSE-decompress → ASTC 4x4 → Morton
de-tile → RGBA. Source: [Decoding Apple ATX Images in iLEAPP (James Habben,
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

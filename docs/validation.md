# Validation

## Executive Summary

`atx-core` decodes **108 real Apple ATX textures** pulled from a genuine iPhone
file-system extraction, and its output matches an **independent oracle**
(iLEAPP) to within **one least-significant bit per channel on every file**, across
both payload paths. This is a **tier-1** validation: a third party authored both
the artifact (Josh Hickman's public iOS 17 image) and the reference decoder
(abrignoni/iLEAPP), neither of which we control.

| Metric | Result |
|---|---|
| Real `.atx` files decoded (atx-core / oracle) | 108 / 108 |
| Dimensions matching the oracle | 108 / 108 |
| **Max per-channel pixel difference (all files)** | **1 (one LSB)** |
| Byte-identical to the oracle | 3 |
| Decode failures (either engine) | 0 |

The remaining ±1 differences are **rounding between two independent ASTC
decoders** (atx-core's `astc-decode` vs iLEAPP's `astc_decomp_faster`), not a
layout or de-tile error — the images are visually identical. Critically, the **48
raw macro-tiled posters/wallpapers** — the Morton de-tile + orientation-heuristic
path that risks the "jumbled image" bug — agree with the oracle to within 1 LSB on
*every* file, so the de-tile orientation is correct on real device content.

> **Tiering (per the Evidence-Based Rigor policy).** The container parse, HEAD
> field layout, payload framing, LZFSE decompression, Morton de-tile + orientation
> pick, crop, and pixel-format classification are **tier-1** here (real artifact +
> independent oracle). The absolute ASTC entropy→RGBA math is each decoder's own
> responsibility; agreement to ±1 LSB between two unrelated decoders is strong
> independent corroboration of it, not a self-check.

## The artifact (tier-1 source)

- **Image:** Josh Hickman (The Binary Hick) iOS 17 public research image — the
  DFIR community's standard public test device.
- **Device / OS:** iPhone 11 (N104AP), iOS 17.3, Cellebrite UFED full file-system
  extraction, collected 2024-07-28.
- **Origin:** `iOS_17_Public_Image.tar.gz`, nested
  `…/EXTRACTION_FFS 01/EXTRACTION_FFS.zip` →
  `filesystem1/…`. Download:
  <https://digitalcorpora.s3.amazonaws.com/corpora/mobile/iOS17/iOS_17_Public_Image.tar.gz>
  (hashes in the accompanying
  [image-creation doc](https://digitalcorpora.s3.amazonaws.com/corpora/mobile/iOS17/iOS17-ImageCreation.pdf)).
- **In the fleet:** held by `issen` at
  `tests/data/josh-hickman-ios17-biome-segb/` (large artifact, gitignored,
  downloaded manually per the fleet test-data standard). Consumed here by path;
  not redistributed.
- **`.atx` files found:** 108, covering both payload encodings and both the
  forensically interesting "what was on screen" content and Apple system assets.

| Category | Count | Payload | Dimensions | Forensic note |
|---|---|---|---|---|
| Poster / wallpaper snapshots | 48 | raw macro-tiled ASTC | 828×1792, 1792×828 | Lock/home-screen renders under `PRBPosterExtensionDataStore` and `…/Caches/…/PosterSnapshots` (WallpaperKit, EmojiPoster, weather, Photos posters) — *what the screen showed* |
| Animoji (AvatarKit) | 54 | LZFSE-compressed | 145×145, 428×428 | Apple system assets under `AvatarKit.framework/animoji/<creature>/` |
| Cached Animoji | 6 | LZFSE-compressed | 170×170 | App-cached avatar thumbnails |

## The oracle (independent reference)

- **iLEAPP** `leapp_functions/parsers/apple_atx.py` from
  [abrignoni/iLEAPP](https://github.com/abrignoni/iLEAPP) (MIT, @JamesHabben) —
  the reverse-engineered reference cited by the source write-up
  ([James Habben, 2026-06-26](https://leapps.org/blog-post?post=2026-06-26-decoding-apple-atx-images)).
- **Independence runs deeper than authorship:** iLEAPP decodes ASTC with
  `astc_decomp_faster` and LZFSE with `liblzfse` — *different libraries* from
  atx-core's `astc-decode` and `lzfse_rust`. The two pipelines share no decode
  code, so agreement is genuine cross-corroboration rather than two wrappers over
  one decoder.

## Method

For each `.atx`, decode with both engines to RGBA8 and compare:

1. **atx-core** — `cargo run --release --example decode_atx -- <file> <out.rgba>`
   (`atx_core::decode` → `DecodedImage.rgba`).
2. **iLEAPP** — `apple_atx.decode_atx_file(<file>).image.pixels`.
3. **Compare** — equal dimensions, then per-byte difference: count differing
   channels, max absolute difference, percentage differing.

Driver: [`tools/atx_oracle_diff.py`](../tools/atx_oracle_diff.py).

## Results

- **108 / 108** files decoded by both engines; **0** failures on either side.
- **108 / 108** dimensions matched the oracle exactly.
- **Max per-channel difference = 1** across all 108 files — on both the 60
  LZFSE-compressed and the 48 raw macro-tiled files.
- **3** files were byte-identical; the remainder differed only by ±1 LSB on a
  fraction of channels (≤19% of bytes, every one of them off by exactly 1).

| Payload path | Files | Dims match | Max diff | Interpretation |
|---|---|---|---|---|
| LZFSE-compressed (`LZFS`) | 60 | 60/60 | 1 LSB | Decompress → linear ASTC 4x4 → decode; no de-tiling needed |
| Raw macro-tiled (`astc`) | 48 | 48/48 | 1 LSB | Morton de-tile + orientation pick → ASTC 4x4 → decode; **de-tile orientation correct on every file** |

Pixel-format discriminators observed: `(3,5)` confirmed ASTC 4x4 ×23, `(1,1)` ×60
and `(3,1)` ×25 inferred — matching `atx-core`'s `FormatConfidence` (Confirmed ×23,
Inferred ×85) and the documented semantics.

## Reproduce

The corpus is gitignored (real device data); the check is env-gated and skips
cleanly when the image is absent.

```sh
# 1. Extract the .atx set from the nested FFS zip (writes to /tmp, never ~/src):
IMG=~/src/issen/tests/data/josh-hickman-ios17-biome-segb/iOS_17_Public_Image.tar.gz
tar -xzf "$IMG" -C /tmp/ios17-ffs ".../EXTRACTION_FFS 01/EXTRACTION_FFS.zip"
unzip -o "/tmp/ios17-ffs/.../EXTRACTION_FFS.zip" "filesystem1/*.atx" -d /tmp/atx-samples
chmod -R u+rwX /tmp/atx-samples           # FFS zip entries extract as mode 0000

# 2. Oracle deps (iLEAPP's decoders):
python3 -m pip install pyliblzfse astc_decomp_faster
git clone --depth 1 https://github.com/abrignoni/iLEAPP

# 3. Build the Rust driver and run the diff:
cargo build --release --example decode_atx
python3 tools/atx_oracle_diff.py        # set SAMPLES / ILEAPP / RUST_BIN paths
```

A committed env-gated Rust smoke test (`ATX_CORPUS=/tmp/atx-samples cargo test
--release -- --ignored corpus`) re-decodes every real `.atx` and asserts a
panic-free `Ok` with sane dimensions — a regression backstop that runs locally
when the corpus is present and skips otherwise.

## Limitations

- **One device, one OS version.** iPhone 11 / iOS 17.3. All real samples were
  ASTC 4x4; other ASTC block sizes are untested because none appeared.
- **Pixel-exactness is bounded by independent-decoder rounding.** ±1 LSB is the
  floor set by two different ASTC implementations, not a defect; a future exact
  bound would require pinning both to the same decoder, which would defeat the
  point of an independent oracle.
- **The orientation pick is a heuristic.** The grid-seam tie-break chose the
  correct orientation on all 48 raw-tiled files here, but it is not a format flag;
  a pathological texture could still fool it. atx-core mirrors the oracle's
  heuristic, so the two agree by construction on this axis.

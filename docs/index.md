# atx-forensic

A pure-Rust, `forbid(unsafe)` reader/decoder for Apple **ATX** (`AAPL`)
texture-image containers — the iOS UI image caches behind PosterBoard snapshots,
wallpapers, contact posters, and Animoji avatars. `atx-core` parses the chunked
container and decodes its ASTC (incl. LZFSE-wrapped) payloads to RGBA.

> **Status: decoder built, pixel-validation pending.** The container parse, HEAD
> layout, payload framing, and Morton de-tile are cross-checked against the iLEAPP
> reference as an oracle (tier-2). The end-to-end image decode is not yet diffed
> pixel-for-pixel against a real device texture.

- **`atx-core`** — the reader/decoder: `AAPL` container walk, HEAD metadata, ASTC
  payload framing, LZFSE decompression (`lzfse_rust`), ASTC decode (`astc-decode`),
  and the Morton macro-tile de-tiling. Imports as `atx_core`.
- **`atx-forensic`** (the analyzer half) is deferred — ATX's forensic value is the
  decoded content, not a structural anomaly to audit.

See the repository [README](https://github.com/SecurityRonin/atx-forensic) for the
quick start and the honest validation status.

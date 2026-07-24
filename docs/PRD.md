# atx-core — Design & Scope

*A library design record, reverse-written from the shipped code (`core/src/lib.rs`,
`Cargo.toml`) on 2026-07-24. This is a `-core` reader/decoder crate, not a product —
so this is a DESIGN doc, not a PRD. The load-bearing decisions live as ADRs under
[`docs/decisions/`](decisions/); this doc frames the purpose, boundaries, and
validation posture they sit inside.*

## Purpose

Apple **ATX** (`AAPL`) files are texture-image containers found throughout iOS UI
image caches: PosterBoard / runtime snapshots, wallpapers, contact posters, and
Animoji / avatar resources. Forensically they are *what was on screen*. Nothing in
the Rust ecosystem read them; iLEAPP decodes them in Python. `atx-core` turns an
`.atx` container back into an RGBA image in one `forbid(unsafe)` Rust crate, so the
fleet — issen's iOS analysis in particular — can recover those pictures as a single
static binary with no runtime dependency.

`atx-core` is a KNOWLEDGE-adjacent reader in the fleet's layer model: it decodes a
source format to addressable content (here, pixels) and depends only on two
pure-Rust codecs plus `thiserror`. It is medium-agnostic — it takes `&[u8]`, so a
live file, a FUSE-mounted path, or bytes carved from an image all feed the same
`parse`/`decode`.

## What it does

- **`parse(&[u8]) -> Result<Atx, AtxError>`** — validate the 8-byte `AAPL` magic
  (loud on failure), walk the framed `[size u32 LE][tag][payload]` chunk list, parse
  the `HEAD` metadata (width/height/depth/array-layers/mipmaps/UUID/pixel-format), and
  locate the texture payload. Malformed chunks after a valid magic degrade to
  `Atx::warnings`.
- **`decode(&[u8]) -> Result<DecodedImage, AtxError>`** — the full pipeline to RGBA8.
  `LZFS` payloads: LZFSE-decompress → linear ASTC 4x4 → decode. Raw `astc`/`ASTC`
  payloads: Morton macro-tile de-tile (dual orientation, grid-seam tie-break) → decode
  → crop to HEAD dimensions. The result carries a `FormatConfidence` (confirmed vs
  inferred).
- **`is_atx` / `astc4x4_confidence`** — cheap magic check and the discriminator →
  confidence mapping, as pure functions.

The byte layout is reimplemented clean-room from abrignoni/iLEAPP's `apple_atx.py`
(MIT); the codecs (`lzfse_rust`, `astc-decode`) are reused, never reinvented. The
crate's own value-add is the `AAPL` container parse, the HEAD field layout, and the
Morton de-tiling. See [ADR 0002](decisions/0002-reuse-codecs-value-add-is-the-container.md),
[ADR 0004](decisions/0004-clean-room-byte-layout-from-ileapp.md), and
[ADR 0005](decisions/0005-morton-detile-dual-orientation-heuristic.md).

## Artifact family

Apple `AAPL` texture containers on iOS, holding ASTC-compressed (mostly ASTC 4x4)
textures, optionally LZFSE-wrapped:

- PosterBoard / SpringBoard snapshots (`PosterSnapshots`, `output.layerStack`, …)
- Wallpapers and contact posters (`PRBPosterExtensionDataStore`, …)
- Avatar / Animoji resource caches

Validated on one device family (iPhone 11 / iOS 17.3, all ASTC 4x4); other block
sizes and OS versions are unproven, not unsupported.

## Scope boundaries (non-goals)

- **No analyzer.** The `atx-forensic` anomaly-audit member is deferred — ATX's
  forensic value is the decoded content, not a structural anomaly to grade
  ([ADR 0001](decisions/0001-reader-only-workspace-analyzer-deferred.md)). The slot
  is reserved; it will be built only if a real auditor emerges.
- **No content interpretation / assignment inference.** The crate reports the image,
  its metadata, and its source path. It never asserts that a file is the *active*
  wallpaper — path is not assignment
  ([ADR 0006](decisions/0006-fail-loud-and-format-confidence-epistemics.md)).
- **Not the ASTC/LZFSE codec owner.** Absolute pixel math belongs to the reused
  decoders; this crate owns the container and the de-tile.
- **No image-format or filesystem knowledge.** It takes bytes; locating and
  extracting the `.atx` from an image is orchestration's job.

## Robustness & security posture

`forbid(unsafe)`, pure-Rust (no C bindings), panic-free by lint
(`unwrap_used`/`expect_used` denied), bounds-checked integer reads, a
`MAX_IMAGE_PIXELS` geometry cap against overflow/alloc bombs, and a `cargo-fuzz`
target on the parse pipeline. A bad magic is a loud bootstrap failure carrying the
offending bytes; misses after a valid magic degrade to `warnings`, never a silent
empty result. See [ADR 0003](decisions/0003-forbid-unsafe-panic-free-fuzzed.md) and
[ADR 0006](decisions/0006-fail-loud-and-format-confidence-epistemics.md).

## Validation approach

Honestly tiered (Doer-Checker), and tier-1: 108 real `.atx` files from a public
iPhone 11 / iOS 17.3 full-file-system image decode to RGBA matching the independent
iLEAPP oracle (a different author *and* a different ASTC decoder) to ≤1 LSB per
channel on every file — including the 48 raw macro-tiled posters/wallpapers where the
de-tile orientation matters. The container framing, HEAD offsets, and de-tile
permutation are additionally cross-checked byte-for-byte against the reference on
synthetic containers (tier-2). Full methodology, corpus provenance, and per-path
results in [`docs/validation.md`](validation.md); harness `tools/atx_oracle_diff.py`;
regression backstop the env-gated `core/tests/corpus.rs` (`ATX_CORPUS=…`).

The scope of the tier-1 claim is one device, one OS version, all ASTC 4x4; the
absolute ASTC pixel math is corroborated by two-decoder agreement, not owned here.

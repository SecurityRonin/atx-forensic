# 5. Morton macro-tile de-tiling with a dual-orientation grid-seam heuristic

Date: 2026-07-24
Status: Accepted

## Context

A raw `astc`/`ASTC` payload is not a linear ASTC block stream. The blocks are
**macro-tiled** in 32x32-block (128 px) tiles, Morton-ordered within each tile.
Decoded linearly they produce a visually shuffled image (the blog's "jumbled
image" bug). The Morton X/Y interpretation — which of the two interleaved bit
lanes is X and which is Y — is **not flagged anywhere in the format**, so a
parser cannot read the correct orientation from the file. An `LZFS` payload, by
contrast, decompresses to an already-linear stream (padded to the 4x4 block
grid) and needs no de-tiling.

## Decision

De-tile the raw path with a 5-bit Morton deinterleave (`morton_5bit`) that
scatters macro-tiled blocks into linear raster order, applied **only** to the raw
`astc`/`ASTC` path — never to the LZFSE path. Because the orientation is
unflagged, decode **both** X/Y orientations and keep the one with the smaller
brightness jump across the 128-px macro-tile seams: `grid_seam_score` computes
the mean absolute luma step across the tile seams (ITU-R 601-2 weights, matching
PIL's "L" mode), and the lower-scoring orientation wins. Crop to the HEAD
dimensions afterward.

Evidence: `core/src/lib.rs` `morton_5bit`, the de-tile scatter loop,
`grid_seam_score`, `MACRO_BLOCKS`/128-px seam stride; `README.md` "the catch";
`HANDOFF.md` §4 (including the correction that de-tiling is raw-path-only, the
blog's single-pipeline framing being slightly off). The orientation heuristic is
called out in `HANDOFF.md` §5/§6 as a heuristic, not a format flag.

## Consequences

Real macro-tiled posters and wallpapers decode to the correct image: all 48 raw
macro-tiled files in the tier-1 corpus match the iLEAPP oracle to ≤1 LSB/channel,
confirming the de-tile orientation on real content (`docs/validation.md`). The
grid-seam pick is a heuristic — it could in principle mis-choose on a pathological
image with genuine seam-aligned brightness edges; the tier-1 corpus shows it does
not on real device textures, and the honest scope of that claim is documented
rather than asserted as universal.

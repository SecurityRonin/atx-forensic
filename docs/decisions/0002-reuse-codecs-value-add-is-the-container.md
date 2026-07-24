# 2. Reuse the ASTC and LZFSE codecs; the value-add is the AAPL container

Date: 2026-07-24
Status: Accepted

## Context

An ATX texture payload is ASTC-compressed, sometimes wrapped in an LZFSE stream.
ASTC block decoding and LZFSE decompression are both solved, non-trivial codecs.
The Research-First discipline (`CLAUDE.core.md`) requires searching the ecosystem
for a correct, maintained implementation before writing one, and reimplementing a
mature codec is exactly the "reinvent an inferior wheel" failure it exists to
prevent. The fleet already depends on a standard LZFSE decoder.

## Decision

Reuse two third-party, pure-Rust codecs and confine the crate's own code to the
container layer:

- **LZFSE → `lzfse_rust = "0.2"`** — the fleet's standard LZFSE decoder
  (`apfs-forensic`, `dmg`, `hfsplus-forensic` already depend on it; pure-Rust, so
  it holds `forbid(unsafe)` — see ADR 0003).
- **ASTC → `astc-decode = "0.3"`** (wwylele) — a software ASTC decoder.

The crate's own value-add is precisely the `AAPL` container parse, the `HEAD`
field layout, and the Morton macro-tile de-tiling (ADR 0005) — the parts no
existing crate provides.

Evidence: workspace `Cargo.toml` lines 14-19 and their comments; `README.md`
"The codecs are reused, never reinvented"; `HANDOFF.md` §3. The "prefer our own
crates" fleet rule (`ronin-issen/CLAUDE.md`) yields here because no
SecurityRonin ASTC/LZFSE crate exists and `lzfse_rust` is already the fleet's
adopted LZFSE dependency — this is the Research-First reuse case, not a
gratuitous third-party pull.

## Consequences

The codec math is maintained upstream and shared across the fleet. Correctness of
the absolute ASTC pixel values is each decoder's concern; the crate validates its
own layers (container framing, HEAD, de-tile orientation, crop) against an
independent oracle whose decoder is *different* (`astc-decode` vs the reference's
Python `astc_decomp`), so ±1-LSB agreement corroborates the pixel math without
this crate owning it (see `docs/validation.md`). A codec bug upstream is an
upstream fix, not a local rewrite.

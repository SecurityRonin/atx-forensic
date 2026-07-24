# 4. Clean-room byte layout from the iLEAPP reference (8-byte magic, HEAD offsets, little-endian)

Date: 2026-07-24
Status: Accepted

## Context

ATX is an undocumented Apple format. The Research-First discipline requires
locating the authoritative reference before implementing a parser rather than
coding from a guess. The community reference is abrignoni/iLEAPP's
`apple_atx.py` (MIT, @JamesHabben), cited by the source write-up
(James Habben, "Decoding Apple ATX Images in iLEAPP", 2026-06-26). An early
scaffold guessed the container framing from the blog prose and got it wrong.

## Decision

Reimplement the byte layout clean-room from the iLEAPP reference (MIT — legal to
read and cite; reimplemented, not copied), and cross-check against it as an
independent oracle. Concretely:

- The magic is the **8-byte** PNG-style signature `AAPL\r\n\x1a\n`, and `is_atx`
  gates on all 8 bytes — the scaffold's 4-byte `AAPL` guess was confirmed wrong
  by the reference's `AAPL_MAGIC`.
- Chunks are framed `[size u32 LE][tag][payload]` from offset 8, `size`
  excluding the 8-byte header.
- HEAD fields sit at the reference's offsets: width `0x18`, height `0x1C`, depth
  `0x20`, array `0x28`, mipmaps `0x2C`, uuid `0x3C`, pixel-format `0x4C`/`0x50`;
  HEAD requires `>= 0x54` bytes.
- All integers are **little-endian** (`u32_le`).
- The pixel-format discriminator maps `(3,5)` → confirmed ASTC 4x4, `(1,1)` and
  `(3,1)` → inferred, per the iLEAPP findings (see ADR 0006).

Evidence: `core/src/lib.rs` module docs and the `Head`/`u32_le`/`parse_head`
field offsets; `MAGIC` constant; commit `1c01ae9` (GREEN framed parse) after
`6c2d416` ("cross-check de-tile against iLEAPP reference oracle"); `HANDOFF.md`
§4-§5 (the 4-byte→8-byte correction and the clean-room provenance). `deny.toml`
notes the MIT oracle is an external tool, never linked.

## Consequences

The framing, HEAD offsets, and endianness are grounded in a real
reverse-engineered reference and cross-validated against it byte-for-byte on
synthetic containers (tier-2) and against 108 real device textures (tier-1, see
`docs/validation.md`). Field meanings the reference did not settle remain
inferences and are labelled as such. If Apple revises the format, the offsets are
in one place (`parse_head`) to revise.

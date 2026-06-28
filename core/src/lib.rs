//! `atx-core` — reader/decoder for Apple **ATX** (`AAPL`) texture-image containers.
//!
//! ATX files are Apple "AAPL" texture containers found throughout iOS UI image
//! caches: PosterBoard / runtime **snapshots**, **wallpapers**, **contact
//! posters**, **avatars / Animoji**. Forensically they are *what was on screen*.
//!
//! ## Container layout (clean-room from the iLEAPP reference)
//!
//! The byte-level framing below is reimplemented clean-room from
//! [abrignoni/iLEAPP](https://github.com/abrignoni/iLEAPP)'s
//! `leapp_functions/parsers/apple_atx.py` (MIT, @JamesHabben, 2026-06-25), the
//! authoritative reverse-engineered reference cited by the source write-up
//! ([James Habben, 2026-06-26](https://leapps.org/blog-post?post=2026-06-26-decoding-apple-atx-images)).
//!
//! ```text
//! AAPL\r\n\x1a\n      8-byte signature (PNG-style)
//! [size u32 LE][tag] chunk header (size = payload bytes, NOT incl. the 8-byte header)
//!   payload[size]
//! ...                repeated to EOF
//! ```
//!
//! Chunk tags: `HEAD` (metadata), `FILL`, `astc`/`ASTC` (raw ASTC payload),
//! `LZFS` (LZFSE-compressed ASTC).
//!
//! ## Decode pipeline
//!
//! - **`LZFS`** payload: LZFSE-decompress (`lzfse_rust`) → a *linear* ASTC 4x4
//!   stream (padded to the 4x4 block grid) → `astc-decode` → RGBA8. No macro
//!   de-tiling — the compressed stream is already linear.
//! - **raw `astc`/`ASTC`** payload: the ASTC blocks are **macro-tiled** in 32x32
//!   block (128 px) tiles, Morton-ordered within each tile. De-tile to a linear
//!   block stream, then `astc-decode` → RGBA8. The Morton X/Y interpretation is
//!   not flagged by the format, so both orientations are decoded and the one with
//!   the smaller brightness jump across the 128-px macro-tile seams is kept (the
//!   reference's grid-seam heuristic).
//!
//! Codecs are REUSED, never reinvented: `lzfse_rust` (the fleet's LZFSE) and
//! `astc-decode`. The new value-add is the AAPL container parse, the HEAD field
//! layout, and the Morton de-tiling.
//!
//! ## Validation status (Doer-Checker)
//!
//! The container parse and the Morton math are validated here against fixtures
//! built to the documented byte layout and against hand-derived values (tier-2).
//! The **end-to-end image decode is NOT yet pixel-validated against a real device
//! sample + the iLEAPP oracle** (PNG diff) — that remains the build-out boundary
//! recorded in HANDOFF.md §5. The decode wires validated codecs and the
//! clean-room framing/de-tile, but visual correctness on real ASTC textures is
//! unconfirmed. Do not present a decoded image as oracle-confirmed.
//!
//! **Epistemics**: report the pixel format as *confirmed* vs *inferred*; never
//! claim a file is the *active* wallpaper just because of its path. State what the
//! container holds, not what it means.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use thiserror::Error;

/// Every ATX file begins with this 8-byte signature (`AAPL` + a PNG-style
/// `\r\n\x1a\n` guard). Confirmed from the iLEAPP reference (`AAPL_MAGIC`).
pub const MAGIC: &[u8; 8] = b"AAPL\r\n\x1a\n";

/// Errors from reading or decoding an ATX container.
///
/// Bad magic is a *bootstrap* failure (the buffer is not an ATX container at all)
/// and fails loud. Per-artifact misses after a validated magic — a missing HEAD,
/// a truncated chunk — degrade to [`Atx::warnings`] rather than an error, so the
/// chunk inventory and any metadata still reach the caller.
#[derive(Debug, Error)]
pub enum AtxError {
    /// The buffer does not begin with the 8-byte `AAPL` magic.
    #[error("not an ATX file: expected AAPL magic, found {found:02x?}")]
    NotAtx {
        /// The leading bytes actually present (up to 8).
        found: Vec<u8>,
    },
    /// No `HEAD` chunk was present, so no metadata could be parsed.
    #[error("ATX has no HEAD chunk")]
    NoHead,
    /// No `astc`/`ASTC`/`LZFS` texture payload chunk was present.
    #[error("ATX has no texture payload chunk (astc/ASTC/LZFS)")]
    NoPayload,
    /// The pixel-format discriminator is not a recognized ASTC 4x4 mapping. The
    /// raw pair is surfaced so the analyst can identify it.
    #[error("unsupported ATX pixel format {pixel_format:?} (not a known ASTC 4x4 discriminator)")]
    UnsupportedPixelFormat {
        /// The raw `(a, b)` discriminator pair from HEAD.
        pixel_format: (u32, u32),
    },
    /// HEAD declared dimensions that cannot form an image.
    #[error("invalid ATX dimensions: {width}x{height}")]
    InvalidDimensions {
        /// Declared width.
        width: u32,
        /// Declared height.
        height: u32,
    },
    /// The texture payload was smaller than the declared geometry requires.
    #[error("ATX texture payload too small: got {got} bytes, expected at least {expected}")]
    PayloadTooSmall {
        /// Bytes actually present.
        got: usize,
        /// Bytes the padded ASTC geometry requires.
        expected: usize,
    },
    /// LZFSE decompression of an `LZFS` payload failed.
    #[error("LZFSE decompression failed: {0}")]
    Decompress(String),
}

/// The chunk FourCC tags ATX containers carry.
pub mod fourcc {
    /// Metadata chunk.
    pub const HEAD: &[u8; 4] = b"HEAD";
    /// Fill chunk.
    pub const FILL: &[u8; 4] = b"FILL";
    /// Raw ASTC payload (lowercase form).
    pub const ASTC_LOWER: &[u8; 4] = b"astc";
    /// Raw ASTC payload (uppercase form).
    pub const ASTC_UPPER: &[u8; 4] = b"ASTC";
    /// LZFSE-compressed ASTC payload.
    pub const LZFS: &[u8; 4] = b"LZFS";
}

/// A located chunk within the container, from the framed `[size][tag]` walk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkRef {
    /// The 4-byte tag.
    pub tag: [u8; 4],
    /// Byte offset of the chunk header (its `size` field) within the container.
    pub offset: usize,
    /// Declared payload size in bytes (excludes the 8-byte chunk header).
    pub size: u32,
    /// Byte offset of the chunk payload (`offset + 8`).
    pub payload_offset: usize,
}

/// `HEAD` metadata, parsed at the byte offsets confirmed from the iLEAPP reference.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Head {
    /// HEAD flags word (offset `0x00`).
    pub flags: u32,
    /// Texture width in pixels (offset `0x18`).
    pub width: u32,
    /// Texture height in pixels (offset `0x1C`).
    pub height: u32,
    /// Texture depth (offset `0x20`).
    pub depth: u32,
    /// Array layer count (offset `0x28`).
    pub array_layers: u32,
    /// Mipmap level count (offset `0x2C`).
    pub mipmaps: u32,
    /// 16-byte texture UUID (offset `0x3C`).
    pub texture_uuid: [u8; 16],
    /// The pixel-format discriminator pair (offsets `0x4C`, `0x50`).
    pub pixel_format: (u32, u32),
}

/// A located texture payload chunk and its inner framing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Payload {
    /// The payload chunk tag (`astc`, `ASTC`, or `LZFS`).
    pub tag: [u8; 4],
    /// The 4-byte inner declared size that prefixes the payload data.
    pub declared_size: u32,
    /// Byte offset of the payload data within the container (after the inner size).
    pub data_offset: usize,
    /// Length of the payload data in bytes.
    pub data_len: usize,
    /// Whether the payload is LZFSE-compressed (the `LZFS` tag).
    pub compressed: bool,
}

/// How confidently the pixel format is known — the source research's honest
/// distinction (never present an inference as a confirmed fact).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatConfidence {
    /// Discriminator `(3, 5)`: confirmed ASTC 4x4.
    Confirmed,
    /// Discriminator `(1, 1)` or `(3, 1)`: inferred ASTC 4x4 from a matching
    /// payload type + size (decoded successfully, but not asserted by the format).
    Inferred,
}

/// Map a pixel-format discriminator pair to an ASTC-4x4 confidence, per the
/// iLEAPP findings. Returns `None` for an unrecognized pair — surface the raw
/// pair to the analyst rather than guessing a format.
#[must_use]
pub fn astc4x4_confidence(discriminator: (u32, u32)) -> Option<FormatConfidence> {
    match discriminator {
        (3, 5) => Some(FormatConfidence::Confirmed),
        (1, 1) | (3, 1) => Some(FormatConfidence::Inferred),
        _ => None,
    }
}

/// Whether `bytes` begins with the full 8-byte ATX `AAPL` magic.
#[must_use]
pub fn is_atx(bytes: &[u8]) -> bool {
    bytes.get(..MAGIC.len()) == Some(MAGIC.as_slice())
}

/// A parsed ATX container: its chunk inventory, optional HEAD metadata, optional
/// texture payload, and any non-fatal parse warnings (truncation, trailing bytes).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Atx {
    /// Every chunk located by the framed walk, in file order.
    pub chunks: Vec<ChunkRef>,
    /// The parsed `HEAD` metadata, if a well-formed HEAD chunk was present.
    pub head: Option<Head>,
    /// The located texture payload, if an `astc`/`ASTC`/`LZFS` chunk was present.
    pub payload: Option<Payload>,
    /// Non-fatal anomalies surfaced during the walk (fail-loud, never silent).
    pub warnings: Vec<String>,
}

/// Parse an ATX container: validate the magic (loud on failure), walk the framed
/// chunk list, and parse HEAD + locate the texture payload. Malformed chunks
/// after a valid magic degrade to [`Atx::warnings`].
pub fn parse(_bytes: &[u8]) -> Result<Atx, AtxError> {
    unimplemented!("RED: framed walk pending")
}

/// A decoded ATX image (RGBA8).
#[derive(Debug, Clone)]
pub struct DecodedImage {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// RGBA8 pixels, row-major, `width * height * 4` bytes.
    pub rgba: Vec<u8>,
    /// How confidently the source pixel format was identified.
    pub confidence: FormatConfidence,
}

/// Decode the texture payload to RGBA8.
///
/// NOTE (Doer-Checker): this wires validated codecs (`lzfse_rust`, `astc-decode`)
/// and the clean-room framing/de-tile, but the end-to-end result is **not yet
/// pixel-validated against a real device sample + the iLEAPP oracle** (HANDOFF §5).
pub fn decode(_bytes: &[u8]) -> Result<DecodedImage, AtxError> {
    unimplemented!("RED: decode pipeline pending")
}

/// Decode an index into a 5-bit Morton (Z-order) `(x, y)` pair: even bits form
/// `x`, odd bits form `y`. Pure function — the de-tiling primitive.
#[must_use]
pub fn morton_5bit(_index: u32) -> (u32, u32) {
    unimplemented!("RED: morton decode pending")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a framed ATX container: 8-byte magic + `[size u32 LE][tag][payload]`
    /// per chunk, matching the documented layout (tier-2 fixture construction).
    fn container(chunks: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        for (tag, payload) in chunks {
            out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            out.extend_from_slice(tag.as_slice());
            out.extend_from_slice(payload);
        }
        out
    }

    /// An 0x54-byte HEAD payload with fields at the documented offsets.
    fn head_payload(width: u32, height: u32, pixel_format: (u32, u32)) -> Vec<u8> {
        let mut p = vec![0u8; 0x54];
        let put = |p: &mut [u8], off: usize, v: u32| {
            p[off..off + 4].copy_from_slice(&v.to_le_bytes());
        };
        put(&mut p, 0x00, 0xABCD); // flags
        put(&mut p, 0x18, width);
        put(&mut p, 0x1C, height);
        put(&mut p, 0x20, 1); // depth
        put(&mut p, 0x28, 1); // array_layers
        put(&mut p, 0x2C, 1); // mipmaps
        for (i, b) in (0..16u8).enumerate() {
            p[0x3C + i] = b; // recognizable UUID bytes
        }
        put(&mut p, 0x4C, pixel_format.0);
        put(&mut p, 0x50, pixel_format.1);
        p
    }

    /// A payload chunk body: `[declared_size u32 LE][data]`.
    fn payload_body(declared_size: u32, data: &[u8]) -> Vec<u8> {
        let mut p = declared_size.to_le_bytes().to_vec();
        p.extend_from_slice(data);
        p
    }

    #[test]
    fn magic_gates_atx_on_full_8_bytes() {
        assert!(is_atx(b"AAPL\r\n\x1a\nrest"));
        assert!(
            !is_atx(b"AAPL\x00\x01\x02\x03"),
            "4-byte AAPL prefix is not ATX"
        );
        assert!(!is_atx(b"AAPL\r\n\x1a"), "7 bytes is too short");
        assert!(!is_atx(b"PK\x03\x04"));
    }

    #[test]
    fn parse_rejects_non_atx_loudly_with_the_bytes() {
        let err = parse(b"\x89PNG\r\n\x1a\n").unwrap_err();
        match err {
            AtxError::NotAtx { found } => assert_eq!(found, b"\x89PNG\r\n\x1a\n"),
            other => panic!("expected NotAtx, got {other:?}"),
        }
    }

    #[test]
    fn framed_walk_locates_chunks_with_size_and_payload_offset() {
        let buf = container(&[
            (fourcc::HEAD, vec![0u8; 0x54]),
            (fourcc::ASTC_LOWER, payload_body(16, &[0u8; 16])),
        ]);
        let atx = parse(&buf).unwrap();
        assert_eq!(atx.chunks.len(), 2);
        let head = &atx.chunks[0];
        assert_eq!(&head.tag, fourcc::HEAD);
        assert_eq!(head.offset, MAGIC.len());
        assert_eq!(head.size, 0x54);
        assert_eq!(head.payload_offset, MAGIC.len() + 8);
        let astc = &atx.chunks[1];
        assert_eq!(&astc.tag, fourcc::ASTC_LOWER);
        assert_eq!(astc.size, 20); // 4-byte declared size + 16 data
        assert!(atx.warnings.is_empty());
    }

    #[test]
    fn framed_walk_warns_on_chunk_past_eof() {
        // size claims 999 bytes but only a few follow.
        let mut buf = MAGIC.to_vec();
        buf.extend_from_slice(&999u32.to_le_bytes());
        buf.extend_from_slice(fourcc::HEAD);
        buf.extend_from_slice(&[0u8; 4]);
        let atx = parse(&buf).unwrap();
        assert!(atx.chunks.is_empty());
        assert!(atx.warnings.iter().any(|w| w.contains("EOF")));
    }

    #[test]
    fn head_parses_fields_at_documented_offsets() {
        let buf = container(&[(fourcc::HEAD, head_payload(390, 844, (3, 5)))]);
        let head = parse(&buf).unwrap().head.expect("HEAD should parse");
        assert_eq!(head.width, 390);
        assert_eq!(head.height, 844);
        assert_eq!(head.depth, 1);
        assert_eq!(head.array_layers, 1);
        assert_eq!(head.mipmaps, 1);
        assert_eq!(head.flags, 0xABCD);
        assert_eq!(head.pixel_format, (3, 5));
        assert_eq!(
            head.texture_uuid,
            [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]
        );
    }

    #[test]
    fn head_too_small_degrades_to_warning_not_panic() {
        let buf = container(&[(fourcc::HEAD, vec![0u8; 8])]); // < 0x54
        let atx = parse(&buf).unwrap();
        assert!(atx.head.is_none());
        assert!(atx.warnings.iter().any(|w| w.contains("HEAD")));
    }

    #[test]
    fn payload_locates_inner_size_and_data() {
        let data = vec![0xAAu8; 32];
        let buf = container(&[
            (fourcc::HEAD, head_payload(4, 4, (3, 5))),
            (fourcc::LZFS, payload_body(64, &data)),
        ]);
        let payload = parse(&buf).unwrap().payload.expect("payload");
        assert_eq!(&payload.tag, fourcc::LZFS);
        assert_eq!(payload.declared_size, 64);
        assert_eq!(payload.data_len, 32);
        assert!(payload.compressed);
        assert_eq!(
            &buf[payload.data_offset..payload.data_offset + payload.data_len],
            &data[..]
        );
    }

    #[test]
    fn pixel_format_confidence_is_honest() {
        assert_eq!(
            astc4x4_confidence((3, 5)),
            Some(FormatConfidence::Confirmed)
        );
        assert_eq!(astc4x4_confidence((1, 1)), Some(FormatConfidence::Inferred));
        assert_eq!(astc4x4_confidence((3, 1)), Some(FormatConfidence::Inferred));
        assert_eq!(
            astc4x4_confidence((9, 9)),
            None,
            "unknown pair: surface raw, never guess"
        );
    }

    #[test]
    fn morton_5bit_matches_hand_derived_values() {
        // even bits -> x, odd bits -> y.
        assert_eq!(morton_5bit(0b0), (0, 0));
        assert_eq!(morton_5bit(0b1), (1, 0)); // bit0 -> x bit0
        assert_eq!(morton_5bit(0b10), (0, 1)); // bit1 -> y bit0
        assert_eq!(morton_5bit(0b11), (1, 1));
        assert_eq!(morton_5bit(0b100), (2, 0)); // bit2 -> x bit1
                                                // index 1023 = all 10 bits set -> x=31, y=31
        assert_eq!(morton_5bit(1023), (31, 31));
    }

    #[test]
    fn decode_rejects_non_atx() {
        assert!(matches!(decode(b"nope"), Err(AtxError::NotAtx { .. })));
    }

    #[test]
    fn decode_surfaces_unsupported_pixel_format_with_bytes() {
        let buf = container(&[
            (fourcc::HEAD, head_payload(4, 4, (9, 9))),
            (fourcc::ASTC_LOWER, payload_body(16, &[0u8; 16])),
        ]);
        match decode(&buf) {
            Err(AtxError::UnsupportedPixelFormat { pixel_format }) => {
                assert_eq!(pixel_format, (9, 9));
            }
            other => panic!("expected UnsupportedPixelFormat, got {other:?}"),
        }
    }

    #[test]
    fn decode_lzfs_path_is_structurally_wired() {
        // Tier-2 STRUCTURAL test: validates pipeline plumbing (decompress, decode,
        // crop, rgba size, confidence) on a 4x4 image — NOT visual correctness,
        // which needs a real sample + iLEAPP oracle (HANDOFF §5).
        let astc_block = [0u8; 16]; // one 4x4 ASTC block
        let mut compressed = Vec::new();
        lzfse_rust::encode_bytes(&astc_block, &mut compressed).unwrap();
        let buf = container(&[
            (fourcc::HEAD, head_payload(4, 4, (3, 5))),
            (
                fourcc::LZFS,
                payload_body(astc_block.len() as u32, &compressed),
            ),
        ]);
        let img = decode(&buf).unwrap();
        assert_eq!((img.width, img.height), (4, 4));
        assert_eq!(img.rgba.len(), 4 * 4 * 4);
        assert_eq!(img.confidence, FormatConfidence::Confirmed);
    }

    #[test]
    fn decode_raw_astc_path_is_structurally_wired() {
        // Tier-2 STRUCTURAL test: a 4x4 image pads to a 128x128 macro tile
        // (32x32 blocks = 1024 ASTC blocks). Validates de-tile + decode + crop
        // plumbing; not visual correctness.
        let blocks = 32 * 32;
        let payload = payload_body(0, &vec![0u8; blocks * 16]);
        let buf = container(&[
            (fourcc::HEAD, head_payload(4, 4, (1, 1))),
            (fourcc::ASTC_UPPER, payload),
        ]);
        let img = decode(&buf).unwrap();
        assert_eq!((img.width, img.height), (4, 4));
        assert_eq!(img.rgba.len(), 4 * 4 * 4);
        assert_eq!(img.confidence, FormatConfidence::Inferred);
    }
}

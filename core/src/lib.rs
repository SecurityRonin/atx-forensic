//! `atx-core` — reader/decoder for Apple **ATX** (`AAPL`) texture-image containers.
//!
//! ATX files are Apple "AAPL" texture containers found throughout iOS UI image
//! caches: PosterBoard / runtime **snapshots**, **wallpapers**, **contact
//! posters**, **avatars / Animoji**. Forensically they are *what was on screen*.
//! Layout (from the iLEAPP write-up, James Habben, 2026-06-26):
//!
//! ```text
//! AAPL (magic)  HEAD  FILL  astc/ASTC  LZFS  END
//! ```
//!
//! - `HEAD` carries metadata: width, height, depth, array layers, mipmap count,
//!   a texture UUID, and a pixel-format discriminator pair.
//! - the payload is **ASTC**-compressed texture (mostly ASTC 4x4); `LZFS` chunks
//!   wrap **LZFSE**-compressed ASTC.
//! - the ASTC blocks are **macro-tiled** (Morton order, with an X/Y swap) — not a
//!   linear ASTC stream — so decode = (LZFSE?) → ASTC → Morton de-tile → RGBA.
//!
//! Codecs are REUSED, never reinvented: `lzfse_rust` (the fleet's LZFSE) and
//! `astc-decode`. The new value-add is the AAPL container parse, the HEAD field
//! layout, and the Morton de-tiling. See HANDOFF.md for the build-out plan.
//!
//! **Epistemics** (carried from the source research): report the pixel format as
//! *confirmed* vs *inferred*; never claim a file is the *active* wallpaper just
//! because of its path. State what the container holds, not what it means.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use thiserror::Error;

/// Every ATX file begins with this 4-byte magic.
pub const MAGIC: &[u8; 4] = b"AAPL";

/// Errors from reading or decoding an ATX container.
#[derive(Debug, Error)]
pub enum AtxError {
    /// The buffer does not begin with the `AAPL` magic.
    #[error("not an ATX file: expected AAPL magic, found {found:02x?}")]
    NotAtx {
        /// The leading bytes actually present.
        found: Vec<u8>,
    },
    /// The container ended before a required structure was read.
    #[error("truncated ATX: {0}")]
    Truncated(&'static str),
    /// A path that is implemented only once real samples are available.
    #[error("not yet implemented: {0}")]
    Unimplemented(&'static str),
}

/// The chunk FourCC tags ATX containers carry. (`END` is intentionally absent: it
/// is 3 characters in the write-up, so its 4-byte framing — `"END "` vs `"END\0"`
/// vs a different terminator — must be confirmed from a real sample first.)
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

/// A located chunk within the container.
///
/// HEURISTIC for now: the exact chunk framing (header layout, size encoding) is
/// not yet reversed, so [`chunk_index`] locates tags by scanning for the FourCC
/// bytes rather than walking a length-prefixed list. Enough to inventory which
/// chunks a file carries; replace with a framed walk once the layout is confirmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkRef {
    /// The 4-byte tag.
    pub tag: [u8; 4],
    /// Byte offset of the tag within the container.
    pub offset: usize,
}

/// `HEAD` metadata — the target shape. The field BYTE OFFSETS are **not yet
/// known**; they must be reversed from iLEAPP's `apple_atx.py` + real samples
/// (HANDOFF) before [`Head`] can be populated. Left as the documented goal.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Head {
    /// Texture width in pixels.
    pub width: u32,
    /// Texture height in pixels.
    pub height: u32,
    /// Texture depth.
    pub depth: u32,
    /// Array layer count.
    pub array_layers: u32,
    /// Mipmap level count.
    pub mipmaps: u32,
    /// 16-byte texture UUID.
    pub texture_uuid: [u8; 16],
    /// The pixel-format discriminator pair.
    pub pixel_format: (u32, u32),
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

/// Whether `bytes` begins with the ATX `AAPL` magic.
#[must_use]
pub fn is_atx(bytes: &[u8]) -> bool {
    bytes.get(..4) == Some(MAGIC.as_slice())
}

/// Inventory the known chunk tags by scanning for their FourCC markers
/// (heuristic — see [`ChunkRef`]).
#[must_use]
pub fn chunk_index(bytes: &[u8]) -> Vec<ChunkRef> {
    const TAGS: &[&[u8; 4]] = &[
        fourcc::HEAD,
        fourcc::FILL,
        fourcc::ASTC_LOWER,
        fourcc::ASTC_UPPER,
        fourcc::LZFS,
    ];
    let mut out = Vec::new();
    let Some(last) = bytes.len().checked_sub(4) else {
        return out; // shorter than a single tag
    };
    for i in 0..=last {
        let Some(window) = bytes.get(i..i + 4) else {
            continue; // cov:unreachable: i <= len-4 keeps the slice in range
        };
        for tag in TAGS {
            if window == tag.as_slice() {
                out.push(ChunkRef {
                    tag: **tag,
                    offset: i,
                });
            }
        }
    }
    out
}

/// Parse the container into its chunk inventory. Validates the magic; the framed
/// chunk walk and [`Head`] field parse are the build-out (HANDOFF).
pub fn parse(bytes: &[u8]) -> Result<Vec<ChunkRef>, AtxError> {
    if !is_atx(bytes) {
        return Err(AtxError::NotAtx {
            found: bytes.get(..4).unwrap_or(bytes).to_vec(),
        });
    }
    Ok(chunk_index(bytes))
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

/// Decode the texture payload to RGBA8. NOT YET IMPLEMENTED — pending real
/// samples + the iLEAPP `apple_atx.py` reference to fix the framing/offsets. The
/// pipeline is: `LZFS` → LZFSE-decompress (`lzfse_rust`) → ASTC 4x4 decode
/// (`astc-decode`) → **Morton block de-tile** (with X/Y swap) → RGBA. See HANDOFF.
pub fn decode(_bytes: &[u8]) -> Result<DecodedImage, AtxError> {
    Err(AtxError::Unimplemented(
        "ASTC decode + Morton de-tile pending real samples + iLEAPP reference (HANDOFF)",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magic_gates_atx() {
        assert!(is_atx(b"AAPL\x00\x01"));
        assert!(!is_atx(b"PK\x03\x04"));
        assert!(!is_atx(b"AAP")); // too short
    }

    #[test]
    fn parse_rejects_non_atx_loudly_with_the_bytes() {
        let err = parse(b"\x89PNG").unwrap_err();
        match err {
            AtxError::NotAtx { found } => assert_eq!(found, b"\x89PNG"),
            other => panic!("expected NotAtx, got {other:?}"),
        }
    }

    #[test]
    fn chunk_index_locates_known_tags() {
        // AAPL + HEAD@4 + astc@8 + LZFS@12 (framing bytes omitted — heuristic scan).
        let buf = b"AAPLHEADastcLZFS";
        let chunks = parse(buf).unwrap();
        let tags: Vec<[u8; 4]> = chunks.iter().map(|c| c.tag).collect();
        assert!(tags.contains(fourcc::HEAD));
        assert!(tags.contains(fourcc::ASTC_LOWER));
        assert!(tags.contains(fourcc::LZFS));
        assert_eq!(
            chunks
                .iter()
                .find(|c| &c.tag == fourcc::HEAD)
                .unwrap()
                .offset,
            4
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
    fn decode_is_honestly_unimplemented() {
        assert!(matches!(decode(b"AAPL"), Err(AtxError::Unimplemented(_))));
    }
}

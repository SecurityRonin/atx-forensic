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
//! ## Validation status (Doer-Checker) — tier-1
//!
//! In-crate unit tests cover the container parse and Morton math against the
//! documented byte layout. Beyond that, the end-to-end decode is **tier-1
//! validated on real device textures**: 108 real `.atx` files from a public
//! iPhone 11 / iOS 17.3 full-file-system image decode to RGBA matching the
//! independent iLEAPP reference (a different author *and* a different ASTC
//! decoder) to within one LSB per channel on every file, across both the
//! LZFSE-compressed and raw macro-tiled paths. See `docs/validation.md` for the
//! corpus, oracle, and per-path results.
//!
//! **Epistemics**: report the pixel format as *confirmed* vs *inferred*; never
//! claim a file is the *active* wallpaper just because of its path. State what the
//! container holds, not what it means.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use thiserror::Error;

/// Every ATX file begins with this 8-byte signature (`AAPL` + a PNG-style
/// `\r\n\x1a\n` guard). Confirmed from the iLEAPP reference (`AAPL_MAGIC`).
pub const MAGIC: &[u8; 8] = b"AAPL\r\n\x1a\n";

/// Bytes per ASTC block (all block footprints are 128-bit).
const ASTC_BLOCK_BYTES: usize = 16;
/// ASTC 4x4 block width in pixels.
const ASTC_BLOCK_WIDTH: u32 = 4;
/// ASTC 4x4 block height in pixels.
const ASTC_BLOCK_HEIGHT: u32 = 4;
/// Upper bound on declared geometry (width x height). A crafted HEAD can carry
/// arbitrary `u32` dimensions; reject implausibly large ones loudly before any
/// padding/byte-count math so the decode cannot overflow or attempt a wild
/// allocation. Mirrors the iLEAPP reference's `MAX_IMAGE_PIXELS` guard.
const MAX_IMAGE_PIXELS: u64 = 100_000_000;
/// Macro-tile edge in ASTC blocks (the de-tiling tile is 32x32 blocks).
const MACRO_BLOCKS: u32 = 32;
/// Macro-tile edge in pixels (32 blocks x 4 px = 128) — also the grid-seam step.
const MACRO_TILE_PX: u32 = MACRO_BLOCKS * ASTC_BLOCK_WIDTH;

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
    /// The ASTC decoder failed to consume the (de-tiled) block stream.
    #[error("ASTC decode failed: {0}")]
    AstcDecode(String),
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
pub fn parse(bytes: &[u8]) -> Result<Atx, AtxError> {
    if !is_atx(bytes) {
        return Err(AtxError::NotAtx {
            found: bytes.get(..MAGIC.len()).unwrap_or(bytes).to_vec(),
        });
    }
    let mut warnings = Vec::new();
    let chunks = walk_chunks(bytes, &mut warnings);
    let head = parse_head(bytes, &chunks, &mut warnings);
    let payload = parse_payload(bytes, &chunks, &mut warnings);
    Ok(Atx {
        chunks,
        head,
        payload,
        warnings,
    })
}

/// Read a little-endian `u32` at `off`, bounds-checked (no panic on truncation).
fn u32_le(bytes: &[u8], off: usize) -> Option<u32> {
    bytes
        .get(off..off + 4)?
        .try_into()
        .ok()
        .map(u32::from_le_bytes)
}

/// Whether a tag is one of the texture payload chunks.
fn is_payload_tag(tag: [u8; 4]) -> bool {
    &tag == fourcc::ASTC_LOWER || &tag == fourcc::ASTC_UPPER || &tag == fourcc::LZFS
}

/// Walk the framed `[size u32 LE][tag][payload]` chunk list from after the magic.
/// Stops (with a warning) at the first chunk that would extend past EOF.
fn walk_chunks(bytes: &[u8], warnings: &mut Vec<String>) -> Vec<ChunkRef> {
    let mut out = Vec::new();
    let mut offset = MAGIC.len();
    while offset + 8 <= bytes.len() {
        let Some(size) = u32_le(bytes, offset) else {
            break; // cov:unreachable: offset+8 <= len keeps the u32 in range
        };
        let Some(tag_slice) = bytes.get(offset + 4..offset + 8) else {
            break; // cov:unreachable: offset+8 <= len keeps the tag in range
        };
        let Ok(tag) = <[u8; 4]>::try_from(tag_slice) else {
            break; // cov:unreachable: slice is exactly 4 bytes
        };
        let payload_offset = offset + 8;
        let end = payload_offset + size as usize;
        if end > bytes.len() {
            warnings.push(format!(
                "Chunk {} at offset {offset} extends beyond EOF",
                String::from_utf8_lossy(&tag)
            ));
            return out;
        }
        out.push(ChunkRef {
            tag,
            offset,
            size,
            payload_offset,
        });
        offset = end;
    }
    if offset != bytes.len() {
        warnings.push(format!(
            "{} trailing byte(s) after last complete chunk",
            bytes.len() - offset
        ));
    }
    out
}

/// Parse the `HEAD` chunk's fields at the documented offsets. Degrades to a
/// warning (returning `None`) if HEAD is absent or too small.
fn parse_head(bytes: &[u8], chunks: &[ChunkRef], warnings: &mut Vec<String>) -> Option<Head> {
    let Some(head) = chunks.iter().find(|c| &c.tag == fourcc::HEAD) else {
        warnings.push("No HEAD chunk found".to_string());
        return None;
    };
    if (head.size as usize) < 0x54 {
        warnings.push(format!(
            "HEAD chunk too small for documented ATX header: {} bytes",
            head.size
        ));
        return None;
    }
    let p = bytes.get(head.payload_offset..head.payload_offset + head.size as usize)?;
    let texture_uuid: [u8; 16] = p.get(0x3C..0x4C)?.try_into().ok()?;
    Some(Head {
        flags: u32_le(p, 0x00)?,
        width: u32_le(p, 0x18)?,
        height: u32_le(p, 0x1C)?,
        depth: u32_le(p, 0x20)?,
        array_layers: u32_le(p, 0x28)?,
        mipmaps: u32_le(p, 0x2C)?,
        texture_uuid,
        pixel_format: (u32_le(p, 0x4C)?, u32_le(p, 0x50)?),
    })
}

/// Locate the first texture payload chunk and read its inner `[declared_size][data]`
/// framing. Degrades to a warning (returning `None`) if absent or too small.
fn parse_payload(bytes: &[u8], chunks: &[ChunkRef], warnings: &mut Vec<String>) -> Option<Payload> {
    let Some(chunk) = chunks.iter().find(|c| is_payload_tag(c.tag)) else {
        warnings.push("No astc, ASTC, or LZFS texture payload chunk found".to_string());
        return None;
    };
    if chunk.size < 4 {
        warnings.push(format!(
            "{} chunk too small to include an inner size",
            String::from_utf8_lossy(&chunk.tag)
        ));
        return None;
    }
    let declared_size = u32_le(bytes, chunk.payload_offset)?;
    Some(Payload {
        tag: chunk.tag,
        declared_size,
        data_offset: chunk.payload_offset + 4,
        data_len: chunk.size as usize - 4,
        compressed: &chunk.tag == fourcc::LZFS,
    })
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
/// Tier-1 validated: output matches the independent iLEAPP oracle to within one
/// LSB per channel on 108 real iOS 17.3 device textures (see `docs/validation.md`).
pub fn decode(bytes: &[u8]) -> Result<DecodedImage, AtxError> {
    let atx = parse(bytes)?;
    let head = atx.head.ok_or(AtxError::NoHead)?;
    let payload = atx.payload.ok_or(AtxError::NoPayload)?;

    if head.width == 0
        || head.height == 0
        || u64::from(head.width) * u64::from(head.height) > MAX_IMAGE_PIXELS
    {
        return Err(AtxError::InvalidDimensions {
            width: head.width,
            height: head.height,
        });
    }
    let confidence =
        astc4x4_confidence(head.pixel_format).ok_or(AtxError::UnsupportedPixelFormat {
            pixel_format: head.pixel_format,
        })?;

    let data = bytes
        .get(payload.data_offset..payload.data_offset + payload.data_len)
        .ok_or_else(|| AtxError::Decompress("payload slice out of bounds".to_string()))?;

    let rgba = if payload.compressed {
        decode_lzfs(data, head.width, head.height)?
    } else {
        decode_macro_tiled(data, head.width, head.height)?
    };

    Ok(DecodedImage {
        width: head.width,
        height: head.height,
        rgba,
        confidence,
    })
}

/// Round `value` up to the next multiple of `multiple`.
fn round_up(value: u32, multiple: u32) -> u32 {
    // Saturating so a pathological `value` can never overflow-panic; callers gate
    // real geometry on `MAX_IMAGE_PIXELS`, and a saturated size is caught by the
    // downstream payload-length check rather than aborting.
    value.div_ceil(multiple).saturating_mul(multiple)
}

/// Bytes a padded `width` x `height` ASTC 4x4 texture occupies.
fn astc_byte_count(width: u32, height: u32) -> usize {
    let blocks_w = round_up(width, ASTC_BLOCK_WIDTH) / ASTC_BLOCK_WIDTH;
    let blocks_h = round_up(height, ASTC_BLOCK_HEIGHT) / ASTC_BLOCK_HEIGHT;
    (blocks_w as usize)
        .saturating_mul(blocks_h as usize)
        .saturating_mul(ASTC_BLOCK_BYTES)
}

/// `LZFS` path: LZFSE-decompress to a *linear* ASTC 4x4 stream, decode, crop.
fn decode_lzfs(data: &[u8], width: u32, height: u32) -> Result<Vec<u8>, AtxError> {
    let mut astc = Vec::new();
    lzfse_rust::decode_bytes(data, &mut astc).map_err(|e| AtxError::Decompress(e.to_string()))?;
    let padded_w = round_up(width, ASTC_BLOCK_WIDTH);
    let padded_h = round_up(height, ASTC_BLOCK_HEIGHT);
    let expected = astc_byte_count(padded_w, padded_h);
    let astc = astc.get(..expected).ok_or(AtxError::PayloadTooSmall {
        got: astc.len(),
        expected,
    })?;
    let rgba = astc_to_rgba(astc, padded_w, padded_h)?;
    Ok(crop_rgba(&rgba, padded_w, padded_h, width, height))
}

/// Raw `astc`/`ASTC` path: the blocks are macro-tiled (32x32-block, Morton order).
/// The X/Y interpretation is unflagged, so decode both orientations and keep the
/// one with the smaller brightness jump across the 128-px macro-tile seams.
fn decode_macro_tiled(data: &[u8], width: u32, height: u32) -> Result<Vec<u8>, AtxError> {
    let padded_w = round_up(width, MACRO_TILE_PX);
    let padded_h = round_up(height, MACRO_TILE_PX);
    let blocks_w = padded_w / ASTC_BLOCK_WIDTH;
    let blocks_h = padded_h / ASTC_BLOCK_HEIGHT;
    let expected = blocks_w as usize * blocks_h as usize * ASTC_BLOCK_BYTES;
    if data.len() < expected {
        return Err(AtxError::PayloadTooSmall {
            got: data.len(),
            expected,
        });
    }

    let mut best: Option<(f64, Vec<u8>)> = None;
    for swap_xy in [false, true] {
        let linear = detile_blocks(data, blocks_w, blocks_h, swap_xy);
        let padded_rgba = astc_to_rgba(&linear, padded_w, padded_h)?;
        let cropped = crop_rgba(&padded_rgba, padded_w, padded_h, width, height);
        let score = grid_seam_score(&cropped, width, height);
        let replace = match &best {
            Some((best_score, _)) => score < *best_score,
            None => true,
        };
        if replace {
            best = Some((score, cropped));
        }
    }
    best.map(|(_, rgba)| rgba)
        .ok_or_else(|| AtxError::AstcDecode("no de-tile candidate produced".to_string()))
}

/// Scatter macro-tiled, Morton-ordered ASTC blocks into linear raster block order.
fn detile_blocks(src: &[u8], blocks_w: u32, blocks_h: u32, swap_xy: bool) -> Vec<u8> {
    let mut linear = vec![0u8; blocks_w as usize * blocks_h as usize * ASTC_BLOCK_BYTES];
    let mut src_off = 0usize;
    let mut macro_y = 0;
    while macro_y < blocks_h {
        let mut macro_x = 0;
        while macro_x < blocks_w {
            for morton in 0..MACRO_BLOCKS * MACRO_BLOCKS {
                let (mut local_x, mut local_y) = morton_5bit(morton);
                if swap_xy {
                    core::mem::swap(&mut local_x, &mut local_y);
                }
                let block_x = macro_x + local_x;
                let block_y = macro_y + local_y;
                let dst = (block_y * blocks_w + block_x) as usize * ASTC_BLOCK_BYTES;
                if let (Some(d), Some(s)) = (
                    linear.get_mut(dst..dst + ASTC_BLOCK_BYTES),
                    src.get(src_off..src_off + ASTC_BLOCK_BYTES),
                ) {
                    d.copy_from_slice(s);
                }
                src_off += ASTC_BLOCK_BYTES;
            }
            macro_x += MACRO_BLOCKS;
        }
        macro_y += MACRO_BLOCKS;
    }
    linear
}

/// Decode a linear ASTC 4x4 block stream to an RGBA8 buffer of `width` x `height`.
fn astc_to_rgba(astc: &[u8], width: u32, height: u32) -> Result<Vec<u8>, AtxError> {
    let row = width as usize;
    let mut rgba = vec![0u8; row * height as usize * 4];
    astc_decode::astc_decode(
        astc,
        width,
        height,
        astc_decode::Footprint::ASTC_4X4,
        |x, y, color| {
            let idx = (y as usize * row + x as usize) * 4;
            if let Some(px) = rgba.get_mut(idx..idx + 4) {
                px.copy_from_slice(&color);
            }
        },
    )
    .map_err(|e| AtxError::AstcDecode(e.to_string()))?;
    Ok(rgba)
}

/// Crop the top-left `dst_w` x `dst_h` region out of a `src_w`-wide RGBA8 buffer.
fn crop_rgba(src: &[u8], src_w: u32, _src_h: u32, dst_w: u32, dst_h: u32) -> Vec<u8> {
    let (sw, dw, dh) = (src_w as usize, dst_w as usize, dst_h as usize);
    let mut out = vec![0u8; dw * dh * 4];
    for y in 0..dh {
        let src_row = y * sw * 4;
        let dst_row = y * dw * 4;
        if let (Some(s), Some(d)) = (
            src.get(src_row..src_row + dw * 4),
            out.get_mut(dst_row..dst_row + dw * 4),
        ) {
            d.copy_from_slice(s);
        }
    }
    out
}

/// Mean absolute luma step across the 128-px macro-tile seams of an RGBA8 image.
/// Lower means smoother seams — the reference's tie-breaker between the two
/// Morton X/Y orientations. Luma uses the ITU-R 601-2 weights PIL's "L" mode uses.
fn grid_seam_score(rgba: &[u8], width: u32, height: u32) -> f64 {
    let (w, h) = (width as usize, height as usize);
    let luma = |x: usize, y: usize| -> i32 {
        let i = (y * w + x) * 4;
        match rgba.get(i..i + 3) {
            Some(p) => {
                (i32::from(p[0]) * 299 + i32::from(p[1]) * 587 + i32::from(p[2]) * 114) / 1000
            }
            None => 0,
        }
    };
    let step = MACRO_TILE_PX as usize;
    let mut total = 0.0f64;
    let mut count = 0u32;
    let mut x = step;
    while x < w {
        for y in 0..h {
            total += f64::from((luma(x, y) - luma(x - 1, y)).unsigned_abs());
            count += 1;
        }
        x += step;
    }
    let mut y = step;
    while y < h {
        for x in 0..w {
            total += f64::from((luma(x, y) - luma(x, y - 1)).unsigned_abs());
            count += 1;
        }
        y += step;
    }
    if count == 0 {
        0.0
    } else {
        total / f64::from(count)
    }
}

/// Decode an index into a 5-bit Morton (Z-order) `(x, y)` pair: even bits form
/// `x`, odd bits form `y`. Pure function — the de-tiling primitive.
#[must_use]
pub fn morton_5bit(index: u32) -> (u32, u32) {
    let mut x = 0;
    let mut y = 0;
    for bit in 0..5 {
        x |= ((index >> (bit * 2)) & 1) << bit;
        y |= ((index >> (bit * 2 + 1)) & 1) << bit;
    }
    (x, y)
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
    fn oversized_dimensions_error_not_overflow_panic() {
        // A crafted HEAD carries arbitrary u32 dimensions. cargo-fuzz found that
        // u32::MAX dims overflowed the padding math (`round_up`); decode must fail
        // loud with InvalidDimensions, never panic. Regression for the
        // parse_decode fuzz crash.
        for (w, h) in [(u32::MAX, u32::MAX), (u32::MAX, 1), (100_000, 100_000)] {
            let buf = container(&[
                (fourcc::HEAD, head_payload(w, h, (3, 5))),
                (fourcc::ASTC_LOWER, payload_body(64, &[0u8; 64])),
            ]);
            assert!(
                matches!(decode(&buf), Err(AtxError::InvalidDimensions { .. })),
                "{w}x{h} must error loudly, not panic/overflow"
            );
        }
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
    fn detile_permutation_matches_ileapp_oracle() {
        // Oracle: iLEAPP `_macro_tiled_payload` on a single 128x128 macro tile
        // (32x32 blocks) where source block i carries byte value (i & 0xFF). The
        // value landing at linear block L is the cross-checked permutation. These
        // golden arrays were produced by the reference (tier-2 independent oracle),
        // not chosen by us. See tests/oracle in the build-out notes.
        let mut payload = Vec::new();
        for i in 0..32 * 32u32 {
            payload.extend_from_slice(&[(i & 0xFF) as u8; 16]);
        }
        let cases: [(bool, [u8; 16]); 2] = [
            (
                false,
                [0, 1, 4, 5, 16, 17, 20, 21, 64, 65, 68, 69, 80, 81, 84, 85],
            ),
            (
                true,
                [
                    0, 2, 8, 10, 32, 34, 40, 42, 128, 130, 136, 138, 160, 162, 168, 170,
                ],
            ),
        ];
        for (swap, expected) in cases {
            let linear = detile_blocks(&payload, 32, 32, swap);
            let got: Vec<u8> = (0..16).map(|l| linear[l * ASTC_BLOCK_BYTES]).collect();
            assert_eq!(
                got, expected,
                "swap={swap} de-tile diverges from iLEAPP oracle"
            );
        }
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

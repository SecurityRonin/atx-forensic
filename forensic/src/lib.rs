//! `atx-forensic` — anomaly analyzer for Apple **ATX** (`AAPL`) texture
//! containers, and the reader it grades over.
//!
//! Emits [`forensicnomicon::report`] observations over an [`Atx`] parsed by the
//! [`atx_core`] reader. Every finding is **self-validating** — it is computed
//! from the container's own bytes (a chunk size field checked against the file
//! length, the declared HEAD geometry re-costed against the payload it must
//! fill, the pixel-format discriminator run through the reader's own mapping) —
//! so it needs no external oracle, and each is an *observation*
//! ("consistent with"), never a conclusion.
//!
//! Where the reader deliberately normalizes an anomaly away — a chunk whose size
//! runs past EOF is dropped from [`Atx::chunks`] with only a string warning —
//! the analyzer re-walks the raw frame itself to *see* it, exactly as ADR-0008
//! anticipates a `-forensic` crate going lower than its `-core` reader.
//!
//! The reader surface is re-exported, so `atx_forensic::` resolves the reader
//! types too.
#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub use atx_core::*;

use forensicnomicon::report::{Category, Observation, Severity};

/// ASTC 4x4 block edge, in pixels (all ATX textures are ASTC 4x4).
const ASTC_BLOCK_PX: u32 = 4;
/// Bytes per ASTC block (128-bit blocks).
const ASTC_BLOCK_BYTES: u64 = 16;
/// Macro-tile edge in pixels — the raw `astc`/`ASTC` payload is padded to whole
/// 128-px macro-tiles before de-tiling (mirrors `atx_core`'s decode geometry).
const MACRO_TILE_PX: u32 = 128;

/// A graded anomaly observed in an ATX container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AtxAnomalyKind {
    /// A chunk header declares a payload size that runs past the end of the
    /// file. The reader drops such a chunk from its inventory; here the raw
    /// frame is re-walked so the truncation is surfaced with its exact framing.
    /// Consistent with a truncated or corrupted container.
    ChunkOverrunsEof {
        /// The 4-byte chunk tag (shown verbatim).
        tag: [u8; 4],
        /// Byte offset of the chunk header within the container.
        offset: usize,
        /// The size the header declares for the chunk payload.
        declared_size: u32,
        /// Bytes actually available after the header (fewer than declared).
        bytes_available: usize,
    },
    /// A raw (`astc`/`ASTC`) texture payload holds fewer bytes than the padded
    /// ASTC geometry declared in HEAD requires. Consistent with a truncated
    /// payload or an edited HEAD. (Compressed `LZFS` payloads are exempt — their
    /// stored length is the *compressed* size and legitimately differs.)
    PayloadSmallerThanGeometry {
        /// Declared texture width from HEAD.
        width: u32,
        /// Declared texture height from HEAD.
        height: u32,
        /// Bytes the padded ASTC 4x4 geometry requires.
        expected_bytes: u64,
        /// Bytes the payload actually carries.
        actual_bytes: usize,
    },
    /// HEAD carries a pixel-format discriminator that the reader's own mapping
    /// ([`astc4x4_confidence`]) does not recognize as an ASTC 4x4 format. The
    /// raw pair is shown so the analyst can identify it. Consistent with a
    /// non-ASTC container, a newer format variant, or a corrupted HEAD.
    UnrecognizedPixelFormat {
        /// The raw `(a, b)` discriminator pair from HEAD.
        discriminator: (u32, u32),
    },
}

impl AtxAnomalyKind {
    /// Severity — the single source of truth.
    #[must_use]
    pub fn severity(&self) -> Severity {
        match self {
            // A size field pointing past EOF, or a payload too small for its own
            // declared geometry, is a structural contradiction worth attention —
            // but truncation is a common benign artifact of partial extraction.
            AtxAnomalyKind::ChunkOverrunsEof { .. }
            | AtxAnomalyKind::PayloadSmallerThanGeometry { .. } => Severity::Medium,
            // An unmapped discriminator is a not-yet-identified format, not
            // evidence of tampering on its own.
            AtxAnomalyKind::UnrecognizedPixelFormat { .. } => Severity::Info,
        }
    }

    /// Analytical lens.
    #[must_use]
    pub fn category(&self) -> Category {
        match self {
            AtxAnomalyKind::ChunkOverrunsEof { .. }
            | AtxAnomalyKind::PayloadSmallerThanGeometry { .. } => Category::Structure,
            AtxAnomalyKind::UnrecognizedPixelFormat { .. } => Category::Provenance,
        }
    }

    /// Stable machine-readable code (published contract; never reused/renamed).
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            AtxAnomalyKind::ChunkOverrunsEof { .. } => "ATX-CHUNK-OVERRUNS-EOF",
            AtxAnomalyKind::PayloadSmallerThanGeometry { .. } => {
                "ATX-PAYLOAD-SMALLER-THAN-GEOMETRY"
            }
            AtxAnomalyKind::UnrecognizedPixelFormat { .. } => "ATX-PIXELFORMAT-UNRECOGNIZED",
        }
    }

    /// Human-readable note (observation, not a conclusion).
    #[must_use]
    pub fn note(&self) -> String {
        match self {
            AtxAnomalyKind::ChunkOverrunsEof {
                tag,
                offset,
                declared_size,
                bytes_available,
            } => format!(
                "chunk {} at offset {offset} declares a {declared_size}-byte payload but only \
                 {bytes_available} byte(s) remain in the file — consistent with a truncated or \
                 corrupted container",
                tag_display(*tag)
            ),
            AtxAnomalyKind::PayloadSmallerThanGeometry {
                width,
                height,
                expected_bytes,
                actual_bytes,
            } => format!(
                "raw ASTC payload holds {actual_bytes} byte(s) but the declared {width}x{height} \
                 geometry requires at least {expected_bytes} — consistent with a truncated payload \
                 or an edited HEAD"
            ),
            AtxAnomalyKind::UnrecognizedPixelFormat { discriminator } => format!(
                "HEAD pixel-format discriminator {discriminator:?} is not a known ASTC 4x4 mapping \
                 — the pixel format could not be identified; consistent with a non-ASTC container, \
                 a newer format variant, or a corrupted HEAD"
            ),
        }
    }
}

/// Render a 4-byte tag as its ASCII form when printable, else as hex — the
/// offending value is always shown verbatim, never elided.
fn tag_display(tag: [u8; 4]) -> String {
    if tag.iter().all(u8::is_ascii_graphic) {
        String::from_utf8_lossy(&tag).into_owned()
    } else {
        format!("{tag:02x?}")
    }
}

/// A graded finding: an [`AtxAnomalyKind`] plus its derived severity/code/note.
#[derive(Debug, Clone)]
pub struct AtxAnomaly {
    pub kind: AtxAnomalyKind,
    severity: Severity,
    code: &'static str,
    note: String,
}

impl AtxAnomaly {
    #[must_use]
    pub fn new(kind: AtxAnomalyKind) -> Self {
        let severity = kind.severity();
        let code = kind.code();
        let note = kind.note();
        Self {
            kind,
            severity,
            code,
            note,
        }
    }
}

impl Observation for AtxAnomaly {
    fn severity(&self) -> Option<Severity> {
        Some(self.severity)
    }
    fn code(&self) -> &'static str {
        self.code
    }
    fn note(&self) -> String {
        self.note.clone()
    }
    fn category(&self) -> Category {
        self.kind.category()
    }
}

/// Read a little-endian `u32` at `off`, bounds-checked (never panics).
fn u32_le(bytes: &[u8], off: usize) -> Option<u32> {
    bytes
        .get(off..off.checked_add(4)?)?
        .try_into()
        .ok()
        .map(u32::from_le_bytes)
}

/// Bytes a padded `width` x `height` raw ASTC 4x4 macro-tiled texture occupies —
/// the same geometry `atx_core` costs the payload against before decoding.
fn raw_astc_expected_bytes(width: u32, height: u32) -> u64 {
    let padded_w = u64::from(width.div_ceil(MACRO_TILE_PX).saturating_mul(MACRO_TILE_PX));
    let padded_h = u64::from(height.div_ceil(MACRO_TILE_PX).saturating_mul(MACRO_TILE_PX));
    let blocks_w = padded_w / u64::from(ASTC_BLOCK_PX);
    let blocks_h = padded_h / u64::from(ASTC_BLOCK_PX);
    blocks_w
        .saturating_mul(blocks_h)
        .saturating_mul(ASTC_BLOCK_BYTES)
}

/// Re-walk the framed `[size u32 LE][tag][payload]` chunk list from after the
/// magic, flagging the first chunk whose declared size runs past EOF. This is
/// the anomaly the reader normalizes away (it drops the chunk); the analyzer
/// looks lower to surface it.
fn scan_truncated_chunk(bytes: &[u8]) -> Option<AtxAnomalyKind> {
    let mut offset = MAGIC.len();
    while offset.checked_add(8)? <= bytes.len() {
        let size = u32_le(bytes, offset)?;
        let tag_slice = bytes.get(offset + 4..offset + 8)?;
        let tag = <[u8; 4]>::try_from(tag_slice).ok()?;
        let payload_offset = offset + 8;
        let end = payload_offset.saturating_add(size as usize);
        if end > bytes.len() {
            return Some(AtxAnomalyKind::ChunkOverrunsEof {
                tag,
                offset,
                declared_size: size,
                bytes_available: bytes.len() - payload_offset,
            });
        }
        offset = end;
    }
    None
}

/// Grade an ATX container, returning every anomaly it exhibits.
///
/// A well-formed container returns an empty vector — every check is computed
/// from the container's own bytes, so genuine files are true negatives. Returns
/// empty for a buffer that is not an ATX container at all (nothing to grade).
#[must_use]
pub fn analyze(bytes: &[u8]) -> Vec<AtxAnomaly> {
    let Ok(atx) = parse(bytes) else {
        return Vec::new();
    };
    analyze_parsed(&atx, bytes)
}

/// Grade an already-parsed [`Atx`] plus the raw bytes it came from (needed for
/// the independent truncation re-walk).
#[must_use]
pub fn analyze_parsed(atx: &Atx, bytes: &[u8]) -> Vec<AtxAnomaly> {
    let mut out = Vec::new();

    // Structure: a chunk whose size field runs past EOF (truncation).
    if let Some(kind) = scan_truncated_chunk(bytes) {
        out.push(AtxAnomaly::new(kind));
    }

    // Structure: a raw ASTC payload too small for the declared HEAD geometry.
    if let (Some(head), Some(payload)) = (atx.head.as_ref(), atx.payload.as_ref()) {
        if !payload.compressed && head.width > 0 && head.height > 0 {
            let expected = raw_astc_expected_bytes(head.width, head.height);
            if (payload.data_len as u64) < expected {
                out.push(AtxAnomaly::new(
                    AtxAnomalyKind::PayloadSmallerThanGeometry {
                        width: head.width,
                        height: head.height,
                        expected_bytes: expected,
                        actual_bytes: payload.data_len,
                    },
                ));
            }
        }
    }

    // Provenance: a HEAD discriminator the reader's mapping cannot identify.
    if let Some(head) = atx.head.as_ref() {
        if astc4x4_confidence(head.pixel_format).is_none() {
            out.push(AtxAnomaly::new(AtxAnomalyKind::UnrecognizedPixelFormat {
                discriminator: head.pixel_format,
            }));
        }
    }

    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod metadata_tests {
    use super::*;

    fn all_kinds() -> [AtxAnomalyKind; 3] {
        [
            AtxAnomalyKind::ChunkOverrunsEof {
                tag: *b"HEAD",
                offset: 8,
                declared_size: 999,
                bytes_available: 4,
            },
            AtxAnomalyKind::PayloadSmallerThanGeometry {
                width: 64,
                height: 64,
                expected_bytes: 4096,
                actual_bytes: 100,
            },
            AtxAnomalyKind::UnrecognizedPixelFormat {
                discriminator: (7, 9),
            },
        ]
    }

    #[test]
    fn every_anomaly_variant_exposes_metadata() {
        for kind in all_kinds() {
            // Direct accessors — every match arm of severity/category/code/note.
            let _ = kind.severity();
            let _ = kind.category();
            assert!(!kind.code().is_empty());
            assert!(!kind.note().is_empty());

            // The `Observation` impl delegates; exercise each method through it.
            let obs = AtxAnomaly::new(kind.clone());
            assert!(Observation::severity(&obs).is_some());
            assert_eq!(Observation::code(&obs), kind.code());
            assert_eq!(Observation::note(&obs), kind.note());
            let _ = Observation::category(&obs);
        }
    }

    #[test]
    fn a_non_graphic_chunk_tag_renders_as_hex() {
        // tag_display's non-ASCII branch: a tag with non-printable bytes must be
        // shown verbatim as hex, never lost to a lossy UTF-8 decode.
        let kind = AtxAnomalyKind::ChunkOverrunsEof {
            tag: [0x00, 0x01, 0xff, 0x7f],
            offset: 0,
            declared_size: 10,
            bytes_available: 2,
        };
        let note = kind.note();
        assert!(note.contains("ff"), "a non-graphic tag must render as hex");
    }

    #[test]
    fn analyze_returns_empty_for_non_atx_bytes() {
        // The early return when the buffer is not an ATX container at all.
        assert!(analyze(&[0u8, 1, 2, 3]).is_empty());
    }
}

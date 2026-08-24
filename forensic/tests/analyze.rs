//! Validate the ATX analyzer.
//!
//! No `.atx` fixtures are committed (the real corpus is a large, gitignored
//! device extraction — see `tests/data/README.md`). So the true-negative and the
//! positive controls run against **synthetic AAPL containers** built to the
//! documented byte layout (the same construction `atx-core`'s own unit tests
//! use), and an additional env-gated sweep asserts zero findings across the real
//! corpus when it is present.
//!
//! - A well-formed container MUST yield zero findings (a false positive fails).
//! - Each check has a positive control proving it can go red.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use atx_forensic::{analyze, fourcc, AtxAnomalyKind, MAGIC};

/// Build a framed ATX container: 8-byte magic + `[size u32 LE][tag][payload]`
/// per chunk (matches the documented layout and atx-core's own test builder).
fn container(chunks: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    for (tag, payload) in chunks {
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(tag.as_slice());
        out.extend_from_slice(payload);
    }
    out
}

/// A 0x54-byte HEAD payload with fields at the documented offsets.
fn head_payload(width: u32, height: u32, pixel_format: (u32, u32)) -> Vec<u8> {
    let mut p = vec![0u8; 0x54];
    let put = |p: &mut [u8], off: usize, v: u32| {
        p[off..off + 4].copy_from_slice(&v.to_le_bytes());
    };
    put(&mut p, 0x18, width);
    put(&mut p, 0x1C, height);
    put(&mut p, 0x4C, pixel_format.0);
    put(&mut p, 0x50, pixel_format.1);
    p
}

/// A raw payload chunk body: `[declared_size u32 LE][data]`.
fn payload_body(data: &[u8]) -> Vec<u8> {
    let mut p = (data.len() as u32).to_le_bytes().to_vec();
    p.extend_from_slice(data);
    p
}

/// A well-formed 128x128 ASTC-4x4 container: confirmed discriminator (3,5) and a
/// raw payload exactly the size the geometry requires (32x32 blocks x 16 = 16384).
fn clean_container() -> Vec<u8> {
    container(&[
        (fourcc::HEAD, head_payload(128, 128, (3, 5))),
        (fourcc::ASTC_LOWER, payload_body(&vec![0u8; 16384])),
    ])
}

#[test]
fn a_well_formed_container_is_clean_no_false_positives() {
    let findings = analyze(&clean_container());
    assert!(
        findings.is_empty(),
        "expected no anomalies on a well-formed container, got {:?}",
        findings.iter().map(|f| f.kind.clone()).collect::<Vec<_>>()
    );
}

#[test]
fn a_chunk_past_eof_is_caught() {
    // A HEAD chunk that declares 999 bytes but is followed by only 4 — the reader
    // drops it; the analyzer's independent re-walk must surface the truncation.
    let mut buf = MAGIC.to_vec();
    buf.extend_from_slice(&999u32.to_le_bytes());
    buf.extend_from_slice(fourcc::HEAD);
    buf.extend_from_slice(&[0u8; 4]);
    let findings = analyze(&buf);
    assert!(
        findings
            .iter()
            .any(|f| matches!(f.kind, AtxAnomalyKind::ChunkOverrunsEof { .. })),
        "a chunk running past EOF must trip the truncation check"
    );
}

#[test]
fn a_payload_smaller_than_geometry_is_caught() {
    // HEAD declares 128x128 (needs 16384 raw bytes) but the payload carries 64.
    let buf = container(&[
        (fourcc::HEAD, head_payload(128, 128, (3, 5))),
        (fourcc::ASTC_LOWER, payload_body(&[0u8; 64])),
    ]);
    let findings = analyze(&buf);
    assert!(
        findings
            .iter()
            .any(|f| matches!(f.kind, AtxAnomalyKind::PayloadSmallerThanGeometry { .. })),
        "a raw payload smaller than the declared geometry must be caught"
    );
}

#[test]
fn a_compressed_payload_is_exempt_from_the_geometry_check() {
    // An LZFS payload's stored length is the *compressed* size and legitimately
    // differs from the geometry cost — it must NOT trip the size check.
    let buf = container(&[
        (fourcc::HEAD, head_payload(128, 128, (3, 5))),
        (fourcc::LZFS, payload_body(&[0u8; 64])),
    ]);
    let findings = analyze(&buf);
    assert!(
        !findings
            .iter()
            .any(|f| matches!(f.kind, AtxAnomalyKind::PayloadSmallerThanGeometry { .. })),
        "a compressed LZFS payload must be exempt from the raw geometry check"
    );
}

#[test]
fn an_unrecognized_pixel_format_is_caught() {
    let buf = container(&[
        (fourcc::HEAD, head_payload(128, 128, (9, 9))),
        (fourcc::ASTC_LOWER, payload_body(&vec![0u8; 16384])),
    ]);
    let findings = analyze(&buf);
    assert!(
        findings.iter().any(|f| matches!(
            f.kind,
            AtxAnomalyKind::UnrecognizedPixelFormat {
                discriminator: (9, 9)
            }
        )),
        "an unmapped discriminator must be caught and shown verbatim"
    );
}

/// Env-gated Tier-2 sweep: every real `.atx` in `$ATX_CORPUS` decodes cleanly
/// (the corpus is tier-1 validated), so the analyzer must find zero anomalies on
/// all of them. Skips cleanly when the corpus is absent (like an oracle binary).
#[test]
fn real_corpus_is_clean_when_present() {
    let Ok(dir) = std::env::var("ATX_CORPUS") else {
        eprintln!("skip: set ATX_CORPUS=/path/to/atx-samples to run the real-corpus sweep");
        return;
    };
    let mut checked = 0usize;
    for entry in std::fs::read_dir(&dir).expect("read ATX_CORPUS dir") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("atx") {
            continue;
        }
        let bytes = std::fs::read(&path).expect("read .atx");
        let findings = analyze(&bytes);
        assert!(
            findings.is_empty(),
            "{}: real device texture should be clean, got {:?}",
            path.display(),
            findings.iter().map(|f| f.kind.clone()).collect::<Vec<_>>()
        );
        checked += 1;
    }
    assert!(
        checked > 0,
        "ATX_CORPUS set but no .atx files found in {dir}"
    );
    eprintln!("real-corpus sweep: {checked} .atx files, all clean");
}

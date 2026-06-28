# atx-forensic / atx-core — handoff

Pickup doc for the next session building out `atx-core`: a reader/decoder for
Apple **ATX** (`AAPL`) texture-image containers. Read this first — it is the
design record, the format notes, and the build-out plan with its hard
validation boundary.

---

## 1. Mission, and the honest framing

ATX files are Apple "AAPL" texture containers that hold iOS UI image caches —
PosterBoard / runtime **snapshots**, **wallpapers**, **contact posters**,
**avatars / Animoji**. Forensically: *what was on screen*. `atx-core` parses the
container and decodes the texture payload to RGBA/PNG so issen's iOS analysis can
recover those images.

**Not first.** iLEAPP already decodes ATX (Python — James Habben, June 2026). Our
value is a Rust single static binary + fleet reuse + feeding issen, **not**
novelty. The iLEAPP source is the clean-room reference (see §5).

## 2. What ATX is (from the source research)

Source: [Decoding Apple ATX Images in iLEAPP — James Habben, 2026-06-26](https://leapps.org/blog-post?post=2026-06-26-decoding-apple-atx-images).

Chunked `AAPL` container:

```text
AAPL (magic)  HEAD  FILL  astc/ASTC  LZFS  END
```

- **HEAD** — metadata: width, height, depth, array-layer count, mipmap count, a
  texture UUID, and a pixel-format discriminator pair.
- **payload** — **ASTC**-compressed texture, mostly **ASTC 4x4**. A `LZFS` chunk
  wraps **LZFSE**-compressed ASTC (seen around avatar/Animoji resources).
- **the catch** — the ASTC blocks are **macro-tiled** (Morton order, *with an
  X/Y swap*), NOT a linear ASTC stream. Decoding the blocks linearly yields a
  *visually shuffled* image (the blog's "jumbled image" bug). Morton de-tiling
  with the X/Y interpretation swapped fixes it.
- **pixel-format discriminator** — `(3, 5)` = **confirmed** ASTC 4x4; `(1, 1)`
  and `(3, 1)` = **inferred** ASTC 4x4 (decoded successfully + payload type/size
  matched, but not asserted by the format).

## 3. Architecture + naming (settled)

- **Repo `atx-forensic` (workspace).** Member **`atx-core`** = the reader/decoder
  (this scaffold). **`atx-forensic`** (the analyzer half of Pattern A) is
  **deferred** — ATX has no obvious structural anomaly to audit (its forensic
  value is the decoded *content*, not anomalies). Add it only if a real auditor
  emerges; the issen side is *wiring* (decode `**/*.atx` → timeline images), not
  an analyzer. (Same YAGNI logic as timeglyph's no-`-core`-split.)
- **Codecs are REUSED, never reinvented** (verified third-party, already in the
  fleet dep graph):
  - LZFSE → **`lzfse_rust = "0.2"`** — the fleet's standard LZFSE decoder
    (apfs-forensic, dmg, hfsplus-forensic depend on it; pure-Rust, holds
    `forbid(unsafe)`).
  - ASTC → **`astc-decode = "0.3"`** (wwylele) — software ASTC decoder.
  - **The new, ours** code = the AAPL container parse, the `HEAD` field layout,
    and the **Morton de-tiling**. That is the whole value-add; keep it tight.

## 4. What's DONE

Compiles; `cargo test` green (14 tests); `cargo clippy --all-targets -- -D
warnings` clean; Paranoid Gatekeeper lints on (no unwrap/expect, forbid unsafe).

The iLEAPP reference (prerequisite 1 of §5) **has been obtained and read** — the
byte layout below is now clean-room reimplemented from it, not guessed, and
cross-validated against it as an independent oracle (§5).

- `MAGIC` — the **8-byte** signature `AAPL\r\n\x1a\n` (the scaffold's 4-byte
  `AAPL` guess was confirmed wrong by the reference's `AAPL_MAGIC`). `is_atx`
  gates on all 8 bytes.
- `parse` — a **framed** chunk walk: from offset 8, each chunk is
  `[size u32 LE][tag][payload]`. Returns an `Atx { chunks, head, payload,
  warnings }`. Bad magic fails loud (`NotAtx` + offending bytes); malformed
  chunks after a valid magic degrade to `warnings` (past-EOF, trailing bytes).
- `Head` parse — fields at the reference's offsets (width `0x18`, height `0x1C`,
  depth `0x20`, array `0x28`, mipmaps `0x2C`, uuid `0x3C`, pixel-format
  `0x4C`/`0x50`; HEAD `>= 0x54` bytes).
- `Payload` — inner `[declared_size u32][data]`; `LZFS` ⇒ compressed.
- `decode` — full pipeline. `LZFS`: LZFSE-decompress (`lzfse_rust`) → *linear*
  ASTC 4x4 → `astc-decode`. Raw `astc`/`ASTC`: 32×32-block **Morton de-tile**
  (both X/Y orientations, grid-seam tie-break) → `astc-decode`. Crops to HEAD
  dims; carries `FormatConfidence`. Fail-loud errors surface the offending value
  (pixel format, sizes).
- `morton_5bit` + `astc4x4_confidence` — pure functions, fully tested.
- Deps wired: `lzfse_rust`, `astc-decode`, `thiserror`.

**Correction to §2's mental model:** the Morton de-tile applies **only to the raw
`astc`/`ASTC` path**. An `LZFS` payload decompresses to an already-*linear* ASTC
stream (padded to the 4×4 block grid) — no macro de-tiling. (Confirmed from the
reference; the blog's "LZFS → … → Morton" single-pipeline framing was slightly
off.)

## 5. What's NEXT — and the remaining validation boundary (Doer-Checker)

Prerequisite 1 (the iLEAPP reference) is **done**: `apple_atx.py` and
`apple_atx_images.py` in [abrignoni/iLEAPP](https://github.com/abrignoni/iLEAPP)
(LICENSE confirmed **MIT** — legal to read + cite; reimplemented clean-room, not
copied) gave the authoritative byte layout, now implemented (§4). The framed
walk, HEAD offsets, payload framing, Morton de-tile, and full decode pipeline are
built and **cross-validated against that reference as an independent oracle**:
the same synthetic container bytes feed the Python reference and the Rust parser,
and HEAD fields, chunk framing, the past-EOF warning, and the de-tile permutation
(both X/Y orientations, byte-for-byte) all agree. This is tier-2 — the reference
is a real reverse-engineered implementation, but the *scenarios are synthetic*.

**The remaining boundary is prerequisite 2 — real `.atx` samples + the pixel
oracle.** What is NOT yet validated: that the end-to-end decode produces the
*visually correct* image on a real device texture (the blog's "snaps into place").
The container parse and de-tile permutation are oracle-confirmed; the ASTC pixel
math is `astc-decode`'s (its own validation); but the full chain on genuine
macro-tiled ASTC has not been diffed against iLEAPP's PNG output. To close it:

1. **Real `.atx` samples.** Pull genuine files from an iOS file-system extraction
   (PosterBoard/SpringBoard snapshot paths, avatar/Animoji caches). Per the fleet
   test-data standard: large/real samples gitignored + documented in
   `tests/data/README.md`, validated env-gated.
2. **Pixel oracle diff (env-gated test).** Decode a real `.atx`, render PNG, and
   **diff against iLEAPP's decoded PNG for the same file**. Reconcile every
   mismatch — especially the grid-seam orientation pick, which is a heuristic, not
   a format flag. A self-encoded round-trip proves nothing here.

Likely points needing a real sample to confirm: the grid-seam tie-break choosing
the right orientation on real content; the `astc-decode` (Rust) vs PIL/`astc_decomp`
(Python) decoders agreeing pixel-for-pixel; and any HEAD field whose meaning the
synthetic fixture got nominally right but a real device populates differently.

## 6. Epistemics (mandatory — same discipline as the issen report work)

- Report the pixel format as **confirmed vs inferred** (the scaffold's
  `FormatConfidence`); never present an inference as confirmed.
- **Path is not assignment**: a file appearing under a PosterBoard-ish path
  (`PosterSnapshots`, `PRBPosterExtensionDataStore`, `output.layerStack`, …) does
  NOT mean it is the *active* wallpaper. Report the image + metadata + source
  path; do not assert current assignment. State what the container holds.
- When decode fails, still report the metadata + chunk list + the raw
  discriminator (fail-loud, show the bytes — never a silent empty).

## 7. How to pick up

```
cd ~/src/atx-forensic
cargo test                                 # 5 green
cargo clippy --all-targets -- -D warnings  # clean
```

Add-a-capability pattern: (1) get the iLEAPP reference + a real sample; (2) RED —
write a test asserting the parsed `HEAD`/decoded pixels for that sample; (3) GREEN
— implement the framing/offset/decode; (4) diff PNG vs iLEAPP; (5) commit RED then
GREEN. Reuse `lzfse_rust`/`astc-decode` for all codec math; the only new code is
container parse + Morton de-tile.

Before publish (fleet standard). **Done:** `deny.toml`, `README.md` (SecurityRonin
standard), MkDocs `docs/` (+ `privacy.md`/`terms.md`), `LICENSE` (Apache-2.0),
`SECURITY.md`, `.pre-commit-config.yaml`, `renovate.json`, CI (`ci.yml`: fmt,
clippy, test matrix, MSRV 1.80, cargo-deny, docs). **Still TODO:** fuzz target
(one per parsed structure, invariant = no panic), tag-driven `release.yml`, 100%
line-coverage gate — all gated on closing the §5 pixel-validation boundary first.
Settle the crate name within 72h of first publish.

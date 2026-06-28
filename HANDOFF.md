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

## 4. What's DONE (this scaffold)

Compiles; `cargo test` green (5 tests); `cargo clippy --all-targets -- -D
warnings` clean; Paranoid Gatekeeper lints on (no unwrap/expect, forbid unsafe).

- `MAGIC` (`AAPL`) + `is_atx` + `parse` (validates magic, fails loud with the
  offending bytes).
- `chunk_index` — a **heuristic** FourCC scan (locates HEAD/FILL/astc/ASTC/LZFS
  by tag bytes; framing not yet known).
- `astc4x4_confidence((u32,u32))` — the **confirmed/inferred** discriminator
  mapping (the epistemics piece), fully implemented + tested.
- Data model: `Head`, `ChunkRef`, `FormatConfidence`, `DecodedImage`, `AtxError`.
- `decode` — honestly returns `Unimplemented` (no fabricated offsets).
- Deps wired: `lzfse_rust`, `astc-decode`, `thiserror`.

## 5. What's NEXT — and the HARD validation boundary (Doer-Checker)

**You cannot finish this from the blog alone.** The blog gives the *shape*, not
the byte-level framing (chunk header layout, size encoding, `END` framing, `HEAD`
field offsets). Do NOT fabricate offsets. Two prerequisites, both mandatory:

1. **The iLEAPP reference.** Read `leapp_functions/parsers/apple_atx.py` and
   `scripts/artifacts/apple_atx_images.py` in
   [abrignoni/iLEAPP](https://github.com/abrignoni/iLEAPP) (**MIT** — confirm the
   LICENSE, then it is legal to read as a reference and cite; clean-room
   re-implement in Rust, do not copy verbatim). This is the authoritative byte
   layout.
2. **Real `.atx` samples.** Pull genuine files from an iOS file-system extraction
   (PosterBoard/SpringBoard snapshot paths, avatar/Animoji caches). Per the fleet
   test-data standard: large/real samples gitignored + documented in
   `tests/data/README.md`, validated env-gated.

Then build out (strict TDD — RED test then GREEN, separate commits):

- **Framed chunk walk** — replace the heuristic `chunk_index` with a real walk of
  the chunk headers (FourCC + size). Confirm the `END` framing.
- **`HEAD` parse** — the field byte offsets → populate `Head` (width/height/…/
  UUID/discriminator).
- **Decode pipeline** — for the payload chunk:
  `LZFS` → `lzfse_rust` decompress → ASTC 4x4 via `astc-decode` →
  **Morton block de-tile (with X/Y swap)** → RGBA8 → (optional) PNG. Carry the
  `FormatConfidence` through to `DecodedImage`.
- **Morton de-tile** is the subtle bit — it is *macro-tiling* of ASTC *blocks*,
  and the X/Y local interpretation is swapped. Expect to iterate against a real
  sample until the image "snaps into place" (the blog's words).

**Validation:** decode a real `.atx`, render PNG, and **diff against iLEAPP's
decoded PNG for the same file** (the oracle). Reconcile every mismatch. A
self-encoded round-trip proves nothing here.

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

Before publish (fleet standard): `deny.toml`, fuzz target (one per parsed
structure, invariant = no panic), `README.md` (SecurityRonin standard), MkDocs
`docs/`, `LICENSE` (Apache-2.0), tag-driven `release.yml`, 100% line-coverage
gate. Settle the crate name within 72h of first publish.

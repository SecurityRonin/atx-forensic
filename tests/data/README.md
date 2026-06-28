# Test data provenance

This crate validates against **real device `.atx` textures**, not committed
fixtures. The corpus is large and gitignored (fleet test-data standard); tests
read it in place, env-gated, and skip cleanly when it is absent.

## iOS 17 PosterBoard / Animoji `.atx` set (tier-1)

- **Source:** Josh Hickman (The Binary Hick) iOS 17 public research image — the
  DFIR community's standard public test device.
- **Identity:** iPhone 11 (N104AP), iOS 17.3 (build 21D50); Cellebrite UFED full
  file-system extraction, collected 2024-07-28.
- **Original download:**
  <https://digitalcorpora.s3.amazonaws.com/corpora/mobile/iOS17/iOS_17_Public_Image.tar.gz>
  (hashes in
  <https://digitalcorpora.s3.amazonaws.com/corpora/mobile/iOS17/iOS17-ImageCreation.pdf>).
- **Fleet location (consumed by path, not redistributed):**
  `~/src/issen/tests/data/josh-hickman-ios17-biome-segb/iOS_17_Public_Image.tar.gz`
  — owned by `issen`, documented in that repo's `tests/data/README.md`. Large
  artifact: gitignored, downloaded manually.
- **Contents used:** 108 real `.atx` files inside the nested
  `…/EXTRACTION_FFS 01/EXTRACTION_FFS.zip` at `filesystem1/…` — 48 raw
  macro-tiled poster/wallpaper snapshots (`PRBPosterExtensionDataStore`,
  `…/PosterSnapshots`) and 60 LZFSE-compressed AvatarKit Animoji textures.
- **License / redistribution:** publicly published research image; **not**
  redistributed here — referenced by path only. Decoded outputs are ephemeral
  (`/tmp`), never committed.
- **Used by:** the oracle diff (`tools/atx_oracle_diff.py`, the tier-1 result in
  `docs/validation.md`) and the env-gated regression test `core/tests/corpus.rs`
  (`ATX_CORPUS=/tmp/atx-samples cargo test --release -- --ignored corpus`).

### Extracting the working set

The `.atx` files live two zips deep; extract to `/tmp` (never under `~/src`),
and note the FFS zip entries unpack as mode `0000`:

```sh
IMG=~/src/issen/tests/data/josh-hickman-ios17-biome-segb/iOS_17_Public_Image.tar.gz
tar -xzf "$IMG" -C /tmp/ios17-ffs ".../EXTRACTION_FFS 01/EXTRACTION_FFS.zip"
unzip -o "/tmp/ios17-ffs/.../EXTRACTION_FFS.zip" "filesystem1/*.atx" -d /tmp/atx-samples
chmod -R u+rwX /tmp/atx-samples
```

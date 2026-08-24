# atx-forensic

Forensic analyzer for Apple **ATX** (`AAPL`) texture containers — the iOS UI
image caches behind PosterBoard snapshots, wallpapers, contact posters, and
Animoji avatars. Emits [`forensicnomicon::report`] observations over the
[`atx-core`](../core) reader; every finding is self-validating (computed from the
container's own bytes) and is an *observation* ("consistent with"), never a
conclusion.

## Findings

| code | severity | what it observes |
|---|---|---|
| `ATX-CHUNK-OVERRUNS-EOF` | Medium | a chunk header declares a payload that runs past the end of the file (the reader drops it; the analyzer re-walks the raw frame to see it) — consistent with truncation/corruption |
| `ATX-PAYLOAD-SMALLER-THAN-GEOMETRY` | Medium | a raw `astc`/`ASTC` payload is smaller than the padded ASTC 4x4 byte count the declared HEAD `WxH` requires (compressed `LZFS` payloads exempt) — consistent with a truncated payload or edited HEAD |
| `ATX-PIXELFORMAT-UNRECOGNIZED` | Info | a HEAD pixel-format discriminator that `atx-core`'s own mapping does not recognize as ASTC 4x4, shown verbatim — the format could not be identified |

## Validation

Tier-2. The true-negative and the positive controls run against synthetic AAPL
containers built to the documented byte layout; an env-gated sweep
(`ATX_CORPUS=/path/to/atx-samples`) additionally asserts zero findings across the
real device corpus (108 tier-1-validated `.atx` files) when present. A
well-formed container yields zero findings; each check has a positive control
proving it can go red.

---

Part of the [`atx-forensic`](https://github.com/SecurityRonin/atx-forensic)
workspace. Every parser in the fleet is a reader (`-core`) plus an analyzer
(`-forensic`), per ADR-0008.

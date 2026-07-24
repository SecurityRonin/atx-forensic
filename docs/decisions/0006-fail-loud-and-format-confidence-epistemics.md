# 6. Fail-loud parsing and confirmed-vs-inferred format epistemics

Date: 2026-07-24
Status: Accepted

## Context

Two forensic hazards shape the API. First, silent wrong output: a bad magic, an
unrecognized pixel-format discriminator, or a truncated chunk must surface the
offending value, never a silent empty result (the global Fail-Loud / Show-the-
unrecognized-value disciplines). Second, over-claiming: the pixel format is
sometimes asserted by the container and sometimes only inferred from a matching
payload, and a file's *path* (`PosterSnapshots`, `PRBPosterExtensionDataStore`,
…) says where it was cached, not that it was the *active* wallpaper. Forensic
epistemics forbid presenting an inference as a fact.

## Decision

Split failures by dependency depth and label confidence structurally:

- **Bootstrap vs per-artifact.** A bad magic is a bootstrap failure — the buffer
  is not an ATX container — and fails loud with `AtxError::NotAtx { found }`
  carrying the leading bytes. Misses *after* a valid magic (missing HEAD,
  truncated chunk, trailing bytes) degrade to `Atx::warnings`, so the chunk
  inventory and any metadata still reach the caller. Every error variant carries
  the offending value: `UnsupportedPixelFormat { pixel_format }`,
  `InvalidDimensions { width, height }`, `PayloadTooSmall { got, expected }`.
- **`FormatConfidence` in the return type.** `decode` returns
  `Confirmed` for discriminator `(3,5)` and `Inferred` for `(1,1)`/`(3,1)` — the
  caller cannot forget the distinction because it is a field, not a doc note.
- **State what the container holds, not what it means.** The crate reports image,
  metadata, and source path; it never asserts current wallpaper assignment.

Evidence: `core/src/lib.rs` `AtxError` variants and their `{…}` payloads, the
`parse` bootstrap-vs-warning split, `walk_chunks`/`parse_head` warning pushes,
`FormatConfidence` + `astc4x4_confidence`; `README.md` "Trust but verify" /
"Epistemics"; `HANDOFF.md` §6 (path-is-not-assignment). Grounded in the global
Robustness (fail-loud, show-the-value) and Expert-Witness epistemic-layer
disciplines.

## Consequences

A malformed file yields a diagnosable error naming the exact byte/value or a
partial parse with warnings — an investigator can identify an unknown
discriminator themselves. Consumers surface confidence honestly. The cost is a
richer error enum and a two-variant confidence type callers must handle rather
than a single opaque success.

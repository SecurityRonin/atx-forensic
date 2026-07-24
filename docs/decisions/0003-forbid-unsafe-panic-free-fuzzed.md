# 3. `forbid(unsafe)`, panic-free bounded reader, fuzzed against overflow

Date: 2026-07-24
Status: Accepted

## Context

`atx-core` parses untrusted, attacker-controllable files: an `.atx` recovered
from a device is exactly the kind of input a crafted length field, a truncated
chunk, or a lying HEAD dimension must never turn into a crash or a wild
allocation. The fleet's Paranoid Gatekeeper standard (`ronin-issen/CLAUDE.md`)
mandates a never-panic, never-OOB posture for every `*-core` parser, and the
global unsafe law makes `forbid(unsafe)` the default and the goal — a provable
"zero places a crafted input can corrupt memory."

## Decision

Enforce the posture statically and dynamically:

- **`unsafe_code = "forbid"`** at the workspace root — achievable (not the
  weaker `deny` + bounded allow) because both codecs chosen in ADR 0002 are
  pure-Rust with no C bindings and no mmap.
- **Panic-free lints**: `clippy::unwrap_used` and `expect_used` denied in
  production; `correctness`/`suspicious` denied. Integer field reads route
  through a bounds-checked `u32_le` that returns `None` (never panics) past EOF;
  the chunk walk stops with a warning at the first chunk that would extend past
  EOF.
- **A `MAX_IMAGE_PIXELS` geometry cap** (100 000 000 px, mirroring the iLEAPP
  reference's guard) rejects oversized HEAD dimensions with a loud
  `InvalidDimensions` error before the padding/byte-count math can overflow.
- **A `cargo-fuzz` target** driving the parse pipeline, smoke-run in CI.

Evidence: workspace `Cargo.toml` lines 22-31 (`unsafe_code = "forbid"`,
`unwrap_used`/`expect_used = "deny"`); `core/src/lib.rs` `u32_le`, `walk_chunks`
past-EOF guard, `MAX_IMAGE_PIXELS`/`InvalidDimensions`; the `fuzz/` member; and
commit `4db91b8` ("fix(atx-core): reject oversized HEAD dimensions (cargo-fuzz
overflow) + fuzz target") — a fuzzer found the overflow, and the cap is the fix.

## Consequences

Malformed evidence degrades to a typed error or a partial `Atx` with populated
`warnings`, never a crash. The `forbid(unsafe)` posture earns the
`unsafe forbidden` README badge honestly (the mmap crates in the fleet cannot).
Bounds-checked reads are more verbose than a quick `unwrap`, and the
`cov:unreachable` markers on the walk's guard arms document arms that are
provably dead under the `offset + 8 <= len` invariant but kept for defence in
depth.

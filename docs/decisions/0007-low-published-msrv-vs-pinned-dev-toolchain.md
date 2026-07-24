# 7. Low published-library MSRV (1.80) separate from the pinned dev toolchain

Date: 2026-07-24
Status: Accepted

## Context

`atx-core` is a published library others may link, so its declared MSRV is a
downstream-facing compatibility promise, not a dev-environment fact. The fleet
MSRV policy (`CLAUDE.core.md` "Rust MSRV & Toolchain Policy" and the personal
fleet specifics) separates the two: develop on the current pinned stable, but
promise only what a published library truly needs, keeping the MSRV low and
CI-verified as a trust signal.

## Decision

Declare `rust-version = "1.80"` in `[workspace.package]` (inherited by
`atx-core`) while pinning the dev/CI toolchain to the current fleet stable in
`rust-toolchain.toml` (`channel = "1.96.0"`, with `rustfmt`/`clippy`/
`llvm-tools-preview` carried on the pinned toolchain itself so fmt/clippy/coverage
resolve correctly in CI and locally). The two numbers are deliberately different:
1.80 is the compatibility floor, 1.96.0 is what we build with.

Evidence: `Cargo.toml` line 7 (`rust-version = "1.80"`); `rust-toolchain.toml`
(`channel = "1.96.0"` + `components`); the `Rust 1.80+` README badge; commit
`d79224f` ("pin rustfmt/clippy components in rust-toolchain.toml; real MSRV +
nightly fuzz"). Grounded in the fleet's published-library low-MSRV rule.

## Consequences

The crate stays consumable by toolchains back to 1.80, widening its crates.io
audience, at the cost of forgoing newer-Rust features in library code. Raising
1.80 later would be a near-breaking change requiring an explicit reason; the CI
MSRV job keeps the promise honest.

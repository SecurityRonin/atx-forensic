# 1. Reader-only workspace; the `atx-forensic` analyzer half is deferred

Date: 2026-07-24
Status: Accepted

## Context

The fleet's single-format crate shape (Pattern A in `ronin-issen/CLAUDE.md`,
"Crate naming grammar") is a workspace named `<x>-forensic` with two members:
`<x>-core` (the reader) and `<x>-forensic` (the anomaly analyzer). ATX does not
fit that mould cleanly. Its forensic value is the decoded *content* — the image
that was on screen (PosterBoard snapshots, wallpapers, contact posters, Animoji
avatars) — not a structural anomaly to audit. The container itself carries no
obvious tampering/clearing signal an analyzer would grade.

## Decision

Ship one workspace member for now: `atx-core`, the reader/decoder. The
`atx-forensic` analyzer crate is deferred, not built. The repo keeps the
`atx-forensic` workspace name (Pattern A) so the analyzer slot is reserved, but
`Cargo.toml` declares `members = ["core"]` only. Issen's ATX handling is *wiring*
(decode `**/*.atx` → timeline images), which lives in orchestration, not an
analyzer.

Evidence: `Cargo.toml` line 2 (`members = ["core"]`); `HANDOFF.md` §3 ("the
analyzer half of Pattern A is **deferred** — ATX has no obvious structural
anomaly to audit"); `README.md` "Scope" section; the same YAGNI logic the
constitution applies to timeglyph's no-split. Grounded in
`ronin-issen/CLAUDE.md` "Crate-structure standard" and the global "Scope
Fidelity — YAGNI" discipline.

## Consequences

The published surface is a single lean library crate; downstream consumers
`use atx_core::…` with no analyzer dependency. If a real structural auditor ever
emerges (e.g. a discriminator or geometry inconsistency worth grading), it slots
into the reserved `forensic/` member without a repo rename. The deferral is a
documented gap, not a silent omission — the README states it plainly.

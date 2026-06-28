# Security Policy

## Threat model

`atx-core` is a **parser of untrusted input**. The `.atx` files it reads come from
seized iOS file-system extractions — attacker-influenced data by definition. The
crate's first-order security property is therefore that **no crafted container can
cause memory unsafety, a panic, or silent wrong output** in a process that ingests
it.

Hardening in place:

- `unsafe_code = "forbid"` workspace-wide — no `unsafe`, no C bindings; the codec
  dependencies (`lzfse_rust`, `astc-decode`) are pure-Rust.
- Paranoid lints (`clippy::unwrap_used` / `expect_used` denied in production code),
  so length/offset/size fields are range-checked rather than unwrapped.
- **Fail-loud parsing:** a bad magic errors with the offending bytes; malformed
  chunks after a valid magic degrade to `Atx::warnings`, never a silent empty
  result. Unsupported pixel-format discriminators surface the raw pair.
- Supply chain gated by [`deny.toml`](deny.toml) (yanked-crate deny, permissive
  licences only, no unknown registries/git sources), enforced in CI.

**Planned, not yet in place:** a `cargo-fuzz` target per parsed structure
(invariant: no panic on arbitrary input) and an end-to-end pixel-oracle diff
against iLEAPP on real device textures. Treat decode output as wired-but-unproven
until those land — see the README's *Trust but verify* section.

## Reporting a vulnerability

For an actual security issue in this crate — memory safety, a parser panic on
crafted input, a supply-chain concern — email **albert@securityronin.com** with
details and a reproducer. Please do not open a public issue for security reports.

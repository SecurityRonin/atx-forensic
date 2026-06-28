//! Fuzz the two public entry points against arbitrary/adversarial bytes.
//!
//! Invariant: neither `parse` nor `decode` may panic on any input — `atx-core`
//! reads attacker-influenced device files (see `SECURITY.md`), so a crafted
//! container must surface as `Err`/`warnings`, never an abort. Seed the corpus
//! with real `.atx` to drive mutation through the HEAD/payload/de-tile paths
//! (see `docs/validation.md` for sourcing samples).
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = atx_core::parse(data);
    let _ = atx_core::decode(data);
});

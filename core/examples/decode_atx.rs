//! Decode an ATX file to raw RGBA8 — a thin driver over [`atx_core::decode`] used
//! by the validation harness (`docs/validation.md`) and for ad-hoc inspection.
//!
//! ```text
//! cargo run --example decode_atx -- <input.atx> <output.rgba>
//! ```
//!
//! On success it writes `width * height * 4` RGBA8 bytes to `<output.rgba>` and
//! prints `OK <width> <height> <confidence>` to stdout. On failure it prints
//! `ERR <message>` and exits non-zero (fail-loud — never a silent empty file).

use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let (Some(input), Some(output)) = (args.next(), args.next()) else {
        eprintln!("usage: decode_atx <input.atx> <output.rgba>");
        std::process::exit(2);
    };

    let bytes = std::fs::read(&input)?;
    match atx_core::decode(&bytes) {
        Ok(img) => {
            std::fs::write(&output, &img.rgba)?;
            println!("OK {} {} {:?}", img.width, img.height, img.confidence);
            Ok(())
        }
        Err(e) => {
            println!("ERR {e}");
            std::process::exit(1);
        }
    }
}

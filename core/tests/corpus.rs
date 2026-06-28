//! Env-gated regression test against a real `.atx` corpus.
//!
//! Set `ATX_CORPUS` to a directory tree of real `.atx` files (see
//! `docs/validation.md` for sourcing the iOS 17 PosterBoard/Animoji set) and run:
//!
//! ```sh
//! ATX_CORPUS=/tmp/atx-samples cargo test --release -- --ignored corpus
//! ```
//!
//! It asserts every real container decodes panic-free to a non-empty RGBA buffer
//! with dimensions matching the decoded width*height. It is a backstop, not the
//! oracle diff — the pixel-for-pixel cross-check against iLEAPP lives in
//! `tools/atx_oracle_diff.py`. Skips cleanly when `ATX_CORPUS` is unset.

use std::path::{Path, PathBuf};

fn collect_atx(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_atx(&path, out);
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("atx"))
        {
            out.push(path);
        }
    }
}

#[test]
#[ignore = "requires ATX_CORPUS pointing at real .atx samples"]
fn corpus_decodes_panic_free() {
    let Ok(root) = std::env::var("ATX_CORPUS") else {
        eprintln!("ATX_CORPUS unset — skipping real-corpus regression test");
        return;
    };

    let mut files = Vec::new();
    collect_atx(Path::new(&root), &mut files);
    assert!(
        !files.is_empty(),
        "no .atx files found under ATX_CORPUS={root}"
    );

    let (mut ok, mut failed) = (0_usize, Vec::new());
    for path in &files {
        let bytes = std::fs::read(path).unwrap();
        match atx_core::decode(&bytes) {
            Ok(img) => {
                assert!(img.width > 0 && img.height > 0, "{path:?}: zero dimensions");
                assert_eq!(
                    img.rgba.len(),
                    img.width as usize * img.height as usize * 4,
                    "{path:?}: RGBA length does not match dimensions"
                );
                ok += 1;
            }
            // A real container that fails to decode is a regression, not a skip:
            // record it so the panic-free invariant report names the offender.
            Err(e) => failed.push(format!("{path:?}: {e}")),
        }
    }

    assert!(
        failed.is_empty(),
        "{} of {} real .atx failed to decode:\n{}",
        failed.len(),
        files.len(),
        failed.join("\n")
    );
    eprintln!("decoded {ok}/{} real .atx files panic-free", files.len());
}

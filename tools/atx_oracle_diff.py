#!/usr/bin/env python3
"""Validation harness: decode every real .atx with both atx-core (Rust) and the
iLEAPP reference (oracle), then compare pixel output. See docs/validation.md.

Paths come from the environment so no machine-specific values are committed:

    SAMPLES   directory tree of real .atx files          (default /tmp/atx-samples)
    ILEAPP    path to iLEAPP/leapp_functions/parsers      (default ./iLEAPP/...)
    RUST_BIN  built decode_atx example binary             (default target/release/...)
    OUT       scratch dir for intermediate rgba + report  (default /tmp)

Oracle deps: `pip install pyliblzfse astc_decomp_faster`.
"""
import json
import os
import subprocess
import sys

SAMPLES = os.environ.get("SAMPLES", "/tmp/atx-samples")
ILEAPP = os.environ.get("ILEAPP", os.path.abspath("iLEAPP/leapp_functions/parsers"))
RUST_BIN = os.environ.get("RUST_BIN", os.path.abspath("target/release/examples/decode_atx"))
OUT = os.environ.get("OUT", "/tmp")
sys.path.insert(0, ILEAPP)
import apple_atx  # noqa: E402


def find_atx(root):
    out = []
    for base, _, files in os.walk(root):
        out += [os.path.join(base, f) for f in files if f.lower().endswith(".atx")]
    return sorted(out)


def rust_decode(path, out_rgba):
    p = subprocess.run([RUST_BIN, path, out_rgba], capture_output=True, text=True)
    line = (p.stdout or "").strip()
    if p.returncode == 0 and line.startswith("OK"):
        _, w, h, conf = line.split(None, 3)
        with open(out_rgba, "rb") as fh:
            return {"ok": True, "w": int(w), "h": int(h), "conf": conf, "rgba": fh.read()}
    return {"ok": False, "err": line or (p.stderr or "").strip()}


def oracle_decode(path):
    r = apple_atx.decode_atx_file(path)
    h = r.header
    return {
        "header": None if not h else {"w": h.width, "h": h.height, "pf": [h.pixel_format_a, h.pixel_format_b]},
        "payload": None if not r.payload else {"kind": r.payload.kind, "compressed": r.payload.compressed},
        "warnings": list(r.warnings),
        "image": None if not r.image else {"w": r.image.width, "h": r.image.height, "rgba": r.image.pixels},
    }


def compare(a, b):
    if a is None or b is None or len(a) != len(b):
        return {"comparable": False}
    if a == b:
        return {"comparable": True, "identical": True, "pct_diff": 0.0, "max_diff": 0}
    diffs = [abs(x - y) for x, y in zip(a, b) if x != y]
    return {"comparable": True, "identical": False,
            "pct_diff": round(100 * len(diffs) / len(a), 4), "max_diff": max(diffs), "n_diff": len(diffs)}


def main():
    files = find_atx(SAMPLES)
    print(f"found {len(files)} .atx files under {SAMPLES}", file=sys.stderr)
    if not files:
        print("no samples — set SAMPLES to a directory of real .atx files", file=sys.stderr)
        return 1
    rows = []
    for i, path in enumerate(files):
        rel = path.split("filesystem1/", 1)[-1]
        rust = rust_decode(path, os.path.join(OUT, "_atx_r.rgba"))
        try:
            orac = oracle_decode(path)
        except Exception as ex:  # noqa: BLE001
            orac = {"error": str(ex), "header": None, "payload": None, "warnings": [], "image": None}
        cmp = compare(rust.get("rgba"), (orac.get("image") or {}).get("rgba")) if rust.get("ok") else {"comparable": False}
        rows.append({
            "rel": rel,
            "payload": (orac.get("payload") or {}).get("kind"),
            "pf": (orac.get("header") or {}).get("pf"),
            "rust_ok": rust.get("ok"), "rust_err": rust.get("err"),
            "rust_dim": [rust.get("w"), rust.get("h")] if rust.get("ok") else None,
            "rust_conf": rust.get("conf"),
            "oracle_dim": [orac["image"]["w"], orac["image"]["h"]] if orac.get("image") else None,
            "oracle_err": orac.get("error"), "cmp": cmp,
        })
        if (i + 1) % 20 == 0:
            print(f"  {i+1}/{len(files)}", file=sys.stderr)
    comparable = [r for r in rows if r["cmp"].get("comparable")]
    max_diff = max((r["cmp"]["max_diff"] for r in comparable), default=None)
    print(f"decoded {sum(r['rust_ok'] for r in rows)}/{len(rows)} (rust); "
          f"max per-channel diff vs oracle = {max_diff}", file=sys.stderr)
    with open(os.path.join(OUT, "atx_validation.json"), "w") as fh:
        json.dump(rows, fh, indent=2)
    return 0


if __name__ == "__main__":
    sys.exit(main())

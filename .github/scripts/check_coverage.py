#!/usr/bin/env python3
"""Fail if any *production* line is uncovered, except those annotated
`// cov:unreachable` (a provably-unreachable defensive arm kept for robustness).

Reads an lcov.info (from `cargo llvm-cov --lib --lcov`). Test code is excluded:
coverage of `#[cfg(test)]`/`mod tests` is not the invariant — production lines are.

Usage: check_coverage.py lcov.info
"""
import sys
from pathlib import Path

MARKER = "// cov:unreachable"


def test_module_start(src_lines: list[str]) -> int:
    """1-based line of the `mod tests` / `#[cfg(test)]` boundary, or len+1 if none."""
    for i, line in enumerate(src_lines, start=1):
        s = line.strip()
        if s.startswith("#[cfg(test)]") or s.startswith("mod tests"):
            return i
    return len(src_lines) + 1


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: check_coverage.py <lcov.info>", file=sys.stderr)
        return 2

    violations: list[str] = []
    current: str | None = None
    uncovered: dict[str, list[int]] = {}

    for raw in Path(sys.argv[1]).read_text().splitlines():
        if raw.startswith("SF:"):
            current = raw[3:]
            uncovered.setdefault(current, [])
        elif raw.startswith("DA:") and current is not None:
            line_s, count_s = raw[3:].split(",")[:2]
            if int(count_s) == 0:
                uncovered[current].append(int(line_s))

    for src, lines in uncovered.items():
        if not lines:
            continue
        p = Path(src)
        if not p.exists():
            continue
        src_lines = p.read_text().splitlines()
        boundary = test_module_start(src_lines)
        for ln in lines:
            if ln >= boundary:
                continue  # test code is not gated
            text = src_lines[ln - 1] if 0 < ln <= len(src_lines) else ""
            if MARKER not in text:
                violations.append(f"{src}:{ln}: uncovered -> {text.strip()}")

    if violations:
        print("Uncovered production lines (annotate with `// cov:unreachable` "
              "only if provably unreachable):", file=sys.stderr)
        for v in violations:
            print(f"  {v}", file=sys.stderr)
        return 1

    print("Coverage gate: all production lines covered or annotated.")
    return 0


if __name__ == "__main__":
    sys.exit(main())

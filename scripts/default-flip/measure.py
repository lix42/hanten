#!/usr/bin/env python3
"""Measure the `nf-core/default-flip` default change (pipeline_version 7 -> 8).

For one real frame per roll it runs three conversions and prints derived numbers
only (report fields and file facts — never pixels):

* **before** — the pre-flip default (`--old`, a pipeline_version 7 build): the
  removed chain's `gain-map-hdr` JPEG.
* **before, new flow** — the same build with `--new-flow`: the chain the flip made
  the default.
* **after** — this build's default (`--new`).

The film base is measured once per roll from its unexposed frame (the centre 40 %,
as `scripts/real-scan-verify/harness.sh` freezes it) and stated in every run, so the
comparison measures the pipeline rather than the base.

    python3 scripts/default-flip/measure.py --old <v7 binary> --new target/release/hanten

Build the v7 binary with `scripts/reference-snapshot/build.sh <pre-flip commit>`.
"""
from __future__ import annotations

import argparse
import filecmp
import json
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def run(binary: str, *args: str) -> tuple[dict, str]:
    proc = subprocess.run([binary, *args], capture_output=True, text=True, check=False)
    if proc.returncode != 0:
        sys.exit(f"{binary} {' '.join(args)} exited {proc.returncode}:\n{proc.stderr}")
    return json.loads(proc.stdout) if proc.stdout.strip() else {}, proc.stderr


def centre_region(binary: str, frame: Path) -> str:
    decode = run(binary, "inspect", str(frame))[0]["decode"]
    w, h = decode["width"], decode["height"]
    return f"{int(w * 0.3)},{int(h * 0.3)},{int(w * 0.4)},{int(h * 0.4)}"


def rolls(assets: Path) -> list[tuple[str, str, str]]:
    """(roll, unexposed frame, first real frame) per roll the manifest can freeze."""
    out = subprocess.run(
        [sys.executable, "-m", "nctool", "manifest", "roles", "--asset-root", str(assets)],
        capture_output=True, text=True, check=True,
        env={"PYTHONPATH": str(ROOT / "scripts/analysis")},
    ).stdout
    rows = []
    for line in out.splitlines():
        roll, unexposed, _leader, reals = line.split("|")
        rows.append((roll, unexposed, reals.split()[0]))
    return rows


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--old", required=True, help="a pipeline_version 7 build")
    parser.add_argument("--new", required=True, help="this build")
    parser.add_argument("--assets", default=str(ROOT.parent / "nc-assets"))
    args = parser.parse_args()
    assets = Path(args.assets)

    print("| roll/frame | before: mean RGB (JPEG) | after: mean RGB (TIFF) | clipped before → after | after = before `--new-flow` | peak memory before → after |")
    print("|---|---|---|---|---|---|")
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        for roll, unexposed, frame in rolls(assets):
            base_frame = assets / "rolls" / roll / unexposed
            region = centre_region(args.new, base_frame)
            b = run(args.new, "estimate", "--base-region", region, str(base_frame))[0]["film_base"]
            base = f"{b['r']},{b['g']},{b['b']}"
            src = str(assets / "rolls" / roll / frame)
            before, _ = run(args.old, "convert", src, "-o", str(tmp / "b.jpg"), "--film-base", base)
            run(args.old, "convert", src, "-o", str(tmp / "bn.tiff"), "--film-base", base,
                "--new-flow", "--report", "none")
            after, _ = run(args.new, "convert", src, "-o", str(tmp / "a.tiff"), "--film-base", base)
            same = filecmp.cmp(tmp / "bn.tiff", tmp / "a.tiff", shallow=False)
            def clipped(report):
                loss = report["loss"]
                return (loss["clipped_low"] + loss["clipped_high"]) / loss["total_samples"]
            fmt = lambda m: ", ".join(f"{v:.4f}" for v in m)
            peak = lambda r: r["memory"]["estimated_peak_bytes"] / 1e9
            print(
                f"| `{roll}/{frame}` | {fmt(before['output_stats']['mean'])} "
                f"| {fmt(after['output_stats']['mean'])} "
                f"| {clipped(before):.3%} → {clipped(after):.3%} "
                f"| {'yes' if same else '**no**'} | {peak(before):.2f} → {peak(after):.2f} GB |"
            )


if __name__ == "__main__":
    main()

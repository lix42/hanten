"""The chart table: every labelled colour-target patch, read from its scan.

One extraction every calibration consumer reads (`analysis/chart-extraction`),
instead of each re-reading the chart frames its own way. For each roll with `charts`
in the manifest it measures the film base once (`hanten measure-base` on the roll's
`unexposed` frame — the Dmin a conversion of that roll uses), then for each chart
entry projects the patches from its corners (`nctool.charts`) and reads each patch's
middle half from the **scan**: a render clips a bracket's over end.

Per patch and channel it records the median transmission (the decoder's scale: a
16-bit sample over 65535), the base-relative density `D = −log10(T / base)`
(design-spec §7.2; `SCAN_FLOOR` below as in `algo::fixed`), the density's p10-p90
spread, and the pixel count. Decoded or rendered values are left to the consumers
that want them. The table is written beside `manifest.json` as `charts.json`; it has
no timestamp and fixed rounding, so a rerun on the same inputs is byte-identical.

It refuses rather than guess: a scan whose bytes no longer match the manifest's
checksum, or a chart entry whose check did not pass under the current rules and
target layout (re-run `charts import`), stops the run with nothing written.
"""
from __future__ import annotations

import math
import os
import sys

from . import charts as _charts

TABLE_VERSION = 1
TABLE_NAME = "charts.json"
SCAN_FLOOR = 1 / 65535  # as `algo::fixed`: a zero sample is one code value
# How far a neutral's density may fall from one bracket frame to the next before it is
# flagged: the placement check's ORDER_STOPS (0.1 stop) in density, since a shoulder
# patch flattens and grain moves it by about that much.
ORDER_TOLERANCE = 0.1 * math.log10(2)


def measure_film_base(nc: str, path: str) -> dict:
    """`hanten measure-base` on an unexposed frame: `{"rgb", "hanten", "uniformity",
    "warnings"}`; ValueError when the run or its report is not a usable base."""
    from .roll import _float3, _run_json
    report, err = _run_json([nc, "measure-base", path, "--report", "json"],
                            "hanten measure-base")
    if err:
        raise ValueError(err)
    rgb = _float3(report, "film_base")
    if rgb is None or not all(v > 0 for v in rgb):
        raise ValueError(f"hanten measure-base {path}: no positive per-channel film_base")
    if report.get("film_base_source") != "effective_area":
        raise ValueError(f"hanten measure-base {path}: base read from "
                         f"{report.get('film_base_source')!r}, not the unexposed frame's "
                         "effective area")
    identity = report.get("identity", {})
    return {"rgb": rgb,
            "hanten": {k: identity.get(k) for k in
                       ("nc_version", "git_commit", "git_dirty", "pipeline_version")},
            "uniformity": report.get("area_uniformity", {}).get("verdict"),
            "warnings": [w.get("message", w) if isinstance(w, dict) else w
                         for w in report.get("warnings", [])]}


def patch_stats(scan, h, rect, base: list[float]) -> dict | None:
    """The middle half of one patch: median transmission and density, the density's
    p10-p90 spread, per channel; None when it lies off the scan."""
    import numpy as np
    s = _charts.sample(scan, h, _charts.shrink(rect, _charts.INNER))
    if s is None:
        return None
    t = np.maximum(s / 65535.0, SCAN_FLOOR)
    d = -np.log10(t / np.asarray(base, dtype=np.float64))
    p10, p50, p90 = np.percentile(d, [10, 50, 90], axis=0)
    return {"transmission": [round(float(v), 6) for v in np.median(t, axis=0)],
            "density": [round(float(v), 4) for v in p50],
            "spread": [round(float(v), 4) for v in p90 - p10],
            "pixels": int(len(s))}


def patch_role(target: dict, pid: str) -> tuple[str, int | None]:
    """("neutral", its step from the light end) | ("saturated", None) | ("other", None)."""
    if pid in target["neutrals"]:
        return "neutral", target["neutrals"].index(pid)
    return ("saturated" if pid in target.get("saturated", []) else "other"), None


def stale_reason(entry: dict, target: dict) -> str | None:
    """Why an entry's stored check does not vouch for its corners today, or None."""
    chk = entry.get("check") or {}
    if chk.get("pass") is not True:
        return "its check did not pass"
    if chk.get("rules") != _charts.CHECK_VERSION:
        return f"it was checked under rules {chk.get('rules')}, now {_charts.CHECK_VERSION}"
    if chk.get("target_version") != target.get("version"):
        return (f"it was checked against layout version {chk.get('target_version')}, "
                f"now {target.get('version')}")
    return None


def chart_rows(scan, entry: dict, base: list[float]) -> dict:
    """One chart entry's table: its identity, check, and every patch's numbers."""
    target = _charts.load_target(entry["target"])
    if why := stale_reason(entry, target):
        raise ValueError(f"{entry['target']} on card {entry['card']!r}: {why}; re-run "
                         "`nctool manifest charts import` with its labels")
    h = _charts.target_to_frame(target, entry["corners"])
    reference = (target.get("reference") or {}).get("values", {})
    patches = []
    for p in target["patches"]:
        stats = patch_stats(scan, h, p["rect"], base)
        if stats is None:
            raise ValueError(f"{entry['target']} patch {p['id']} lies off the scan")
        role, step = patch_role(target, p["id"])
        row = {"id": p["id"], "name": p["name"], "role": role}
        if step is not None:
            row["neutral_step"] = step
        if p["id"] in reference:
            row["reference_lab"] = reference[p["id"]]
        patches.append(row | stats)
    return {"target": entry["target"], "card": entry["card"],
            "target_version": target.get("version"), "check": entry.get("check"),
            "patches": patches}


def verified_path(asset_root: str, frame: dict) -> str:
    """The frame's path, after checking its bytes still match the manifest's sha256 —
    a consumer keys on that checksum, so it must describe the pixels read."""
    from .manifest import sha256
    path = os.path.join(asset_root, frame["file"])
    if not frame.get("sha256"):
        raise ValueError(f"{frame['file']} has no sha256 in the manifest; run "
                         "`nctool manifest generate`")
    if sha256(path) != frame["sha256"]:
        raise ValueError(f"{frame['file']} no longer matches its manifest sha256; run "
                         "`nctool manifest generate` (and re-check its charts)")
    return path


def build_table(asset_root: str, manifest: dict, film_base) -> dict:
    """The table for every roll whose frames carry `charts`. `film_base(path)` measures
    a roll's base (`measure_film_base` bound to a binary). ValueError when a roll with
    charts has no single unexposed frame (density needs one), a scan's bytes changed,
    or an entry's check is stale."""
    rolls = {}
    for name in sorted(manifest.get("rolls", {})):
        roll = manifest["rolls"][name]
        frames = [fr for fr in roll.get("frames", []) if fr.get("charts")]
        if not frames:
            continue
        unexposed = [fr for fr in roll.get("frames", []) if fr.get("role") == "unexposed"]
        if len(unexposed) != 1:
            raise ValueError(f"roll {name} has charts but {len(unexposed)} unexposed frames; "
                             "density needs exactly one")
        base_frame = unexposed[0]
        base = film_base(verified_path(asset_root, base_frame))
        out_frames = {}
        for fr in sorted(frames, key=lambda f: f["file"]):
            scan = _charts.read_scan(verified_path(asset_root, fr))
            stem = os.path.splitext(os.path.basename(fr["file"]))[0]
            try:
                charts = [chart_rows(scan, e, base["rgb"])
                          for e in sorted(fr["charts"], key=lambda e: (e["target"], e["card"]))]
            except ValueError as e:
                raise ValueError(f"{fr['file']}: {e}") from None
            out_frames[stem] = {"file": fr["file"], "sha256": fr["sha256"],
                                "role": fr.get("role"), "shot": fr.get("shot"),
                                "charts": charts}
        rolls[name] = {"stock": roll.get("stock"), "conditions": roll.get("conditions"),
                       "film_base": base | {"frame": base_frame["file"],
                                            "sha256": base_frame["sha256"]},
                       "frames": out_frames}
    return {"table_version": TABLE_VERSION,
            "note": ("Per-patch numbers for every labelled colour target, read from the scans "
                     "(nctool manifest charts table). transmission: median of a 16-bit sample "
                     "over 65535; density: -log10(transmission / film_base); spread: the "
                     "density's p10-p90; all per channel, over each patch's middle half."),
            "rolls": rolls}


def neutral_falls(table: dict) -> list[str]:
    """Every place a neutral patch's density falls, by more than `ORDER_TOLERANCE`, as
    its bracket's exposure rises — it should rise, since more light means more density.
    A frame without a `bracket` or a numeric `exposure_ev` is in no series, and two
    frames at one exposure (a repeat) are not compared."""
    falls = []
    for name, roll in table["rolls"].items():
        series: dict = {}
        for stem, fr in roll["frames"].items():
            shot = fr.get("shot") or {}
            ev = shot.get("exposure_ev")
            if (not isinstance(ev, (int, float)) or isinstance(ev, bool)
                    or not shot.get("bracket")):
                continue
            for chart in fr["charts"]:
                for p in chart["patches"]:
                    if p["role"] == "neutral":
                        key = (shot["bracket"], chart["target"], chart["card"], p["id"])
                        series.setdefault(key, []).append((ev, stem, p["density"]))
        for (bracket, target, card, pid), points in sorted(series.items()):
            points.sort()
            for (ev0, s0, d0), (ev1, s1, d1) in zip(points, points[1:]):
                if ev1 == ev0:
                    continue
                for c, ch in enumerate("RGB"):
                    if d1[c] < d0[c] - ORDER_TOLERANCE:
                        falls.append(f"{name} bracket {bracket} {target} card {card!r} {pid} "
                                     f"{ch}: density {d0[c]:.3f} at {ev0:+g} EV ({s0}) > "
                                     f"{d1[c]:.3f} at {ev1:+g} EV ({s1})")
    return falls


def write_table(path: str, table: dict) -> None:
    import json
    from .manifest import write_atomic
    write_atomic(path, json.dumps(table, indent=2, ensure_ascii=False, allow_nan=False)
                 + "\n", ".charts.")


def cmd_table(args) -> int:
    """Build the chart table and write it beside the manifest (or `--out`)."""
    from . import manifest as _manifest
    from .metrics import MetricsError, require_dependencies
    root = os.path.abspath(args.asset_root)
    try:
        require_dependencies("charts table")
        data, err = _manifest.load_manifest(os.path.join(root, "manifest.json"))
        if err or not data:
            raise ValueError(err or f"no manifest at {root}")
        explicit = args.nc if args.nc is not None else os.environ.get("NC")
        nc, err = _manifest.resolve_nc(explicit, "--nc" if args.nc is not None else "$NC")
        if err or not nc:
            raise ValueError(err or "no hanten binary found (build it, or pass --nc)")
        table = build_table(root, data, lambda p: measure_film_base(nc, p))
        falls = neutral_falls(table)
        out = args.out or os.path.join(root, TABLE_NAME)
        if not args.dry_run:
            write_table(out, table)
    except (OSError, ValueError, MetricsError) as e:
        print(f"error: {e}; nothing was written", file=sys.stderr)
        return 2
    n_frames = sum(len(r["frames"]) for r in table["rolls"].values())
    n_patches = sum(len(c["patches"]) for r in table["rolls"].values()
                    for f in r["frames"].values() for c in f["charts"])
    print(f"{len(table['rolls'])} roll(s), {n_frames} frame(s), {n_patches} patch rows")
    for name, roll in table["rolls"].items():
        for w in roll["film_base"]["warnings"]:
            print(f"WARNING: {name} film base: {w}", file=sys.stderr)
    for f in falls:
        print(f"WARNING: neutral falls with exposure: {f}", file=sys.stderr)
    if not falls:
        print("every neutral's density rises with exposure across its bracket")
    print("(dry run — the table was not written)" if args.dry_run else f"wrote {out}")
    return 0

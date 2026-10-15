"""Colour targets in roll frames — the asset manifest's per-frame `charts`.

A chart entry says where one target sits in a frame: `target` (a definition in
`targets/`), `card` (which physical card), `corners` (the patch array's four outer
corners as fractions of the scan, the target's own top left first, then clockwise in
the target's orientation), `source`, and the `check` its placement passed. Patch
positions are never stored: they are projected from the corners through the target's
geometry, so a refined geometry moves no label (it only needs a new check).

The placement check reads the **scan** — the pixels extraction reads, with nothing
clipped — and asks whether the projected patches land on real patches: each patch's
middle is uniform, the strips between patches read as dark as the card's surround,
the neutral series runs in order, and the saturated patches show colour (a grid slipped
by a whole patch puts a saturated column on the neutral surround). It prints derived numbers only.

The corners may come in any order: the check tries each turn and mirror image of
them and keeps the one that passes, which is what fixes the target's top left.
`import` folds the labelling tool's labels file into the manifest; it re-runs the
check, stores the corners in the order that passed, and refuses a label that fails.
"""
from __future__ import annotations

import datetime
import json
import os
import sys

from .conditions import is_number, is_text

TARGETS_DIR = os.path.join(os.path.dirname(__file__), "targets")
ENTRY_FIELDS = {"target", "card", "corners", "source", "check"}
# `oriented`: the user chose which way up (the anchor click), so the check verifies
# that orientation only instead of searching.
LABEL_FIELDS = {"target", "card", "corners", "oriented"}

# The check's rules, versioned so a stored result names the rules it passed.
CHECK_VERSION = 1
INNER = 0.5          # a patch's sampled middle, as a fraction of each side
STRIP = 0.4          # a gap strip's width, as a fraction of the gap
STRIP_SPAN = 0.6     # its length, as a fraction of the patch side it runs along
UNIFORM_STOPS = 0.5  # widest p10-p90 spread a patch's middle may have, any channel
# How much brighter (scene) than the black patch a gap may read. Well-placed labels on
# 2026-10-12-Gold200 read at most 0.27; a label a little off reads 0.6-0.8 where its
# strips clip the patches, while the patch middles still pass.
GAP_STOPS = 0.5
# The share of strips that may read bright before the gap test fails. A grid off by a
# fraction of a patch lifts many strips at once; camera shake or flare lifts a few (a
# 1 s exposure, 2132: one or two of ~50), without touching the patch middles. Those
# few are reported as warnings, not failures.
GAP_SHARE = 0.1
ORDER_STOPS = 0.1    # how far a neutral step may run backwards (noise)
# Least channel spread a saturated patch shows past the neutrals'. Catches a patch on
# the neutral surround (a grid slipped a whole patch: 0.01-0.04 on 2127) without
# failing a weak real one (D2 purple: 0.30 at -2 EV); the neutral order, not this,
# tells a target from the same target upside down.
CHROMA_STOPS = 0.15


# ------------------------------------------------------------------ targets

def target_ids() -> list[str]:
    return sorted(os.path.splitext(f)[0] for f in os.listdir(TARGETS_DIR)
                  if f.endswith(".json"))


def load_target(target_id: str) -> dict:
    """A target definition; ValueError when there is none."""
    path = os.path.join(TARGETS_DIR, f"{target_id}.json")
    if target_id not in target_ids():
        raise ValueError(f"unknown target {target_id!r} (known: {', '.join(target_ids())})")
    with open(path) as f:
        return json.load(f)


# ------------------------------------------------------------------ geometry

def _solve(a: list[list[float]], b: list[float]) -> list[float]:
    """Gaussian elimination with partial pivoting; ValueError when singular."""
    n = len(b)
    m = [row[:] + [b[i]] for i, row in enumerate(a)]
    for c in range(n):
        p = max(range(c, n), key=lambda r: abs(m[r][c]))
        if abs(m[p][c]) < 1e-12:
            raise ValueError("degenerate corners (three in a line, or two the same)")
        m[c], m[p] = m[p], m[c]
        for r in range(n):
            if r != c:
                f = m[r][c] / m[c][c]
                m[r] = [x - f * y for x, y in zip(m[r], m[c])]
    return [m[i][n] / m[i][i] for i in range(n)]


def homography(src: list[list[float]], dst: list[list[float]]) -> list[list[float]]:
    """The 3x3 projective map taking the four `src` points to the four `dst`."""
    a, b = [], []
    for (x, y), (u, v) in zip(src, dst):
        a.append([x, y, 1, 0, 0, 0, -u * x, -u * y])
        a.append([0, 0, 0, x, y, 1, -v * x, -v * y])
        b += [u, v]
    h = _solve(a, b)
    return [h[0:3], h[3:6], h[6:8] + [1.0]]


def project(h: list[list[float]], pt: list[float]) -> list[float]:
    x, y = pt
    w = h[2][0] * x + h[2][1] * y + h[2][2]
    return [(h[0][0] * x + h[0][1] * y + h[0][2]) / w,
            (h[1][0] * x + h[1][1] * y + h[1][2]) / w]


def target_to_frame(target: dict, corners: list[list[float]]) -> list[list[float]]:
    """The homography from target units to frame fractions."""
    w, h = target["size"]
    return homography([[0, 0], [w, 0], [w, h], [0, h]], corners)


def rect_quad(rect: list[float]) -> list[list[float]]:
    x, y, w, h = rect
    return [[x, y], [x + w, y], [x + w, y + h], [x, y + h]]


def shrink(rect: list[float], f: float) -> list[float]:
    """`rect` scaled by `f` about its centre."""
    x, y, w, h = rect
    return [x + w * (1 - f) / 2, y + h * (1 - f) / 2, w * f, h * f]


def gap_strips(target: dict) -> list[tuple[str, list[float]]]:
    """(name, rect) of a strip in the middle of each gap between neighbouring patches,
    in target units, and one just outside each side on the array's outer edge (the
    card's surround — what catches a grid slipped by a whole patch). A side faces a
    neighbour when another patch overlaps the strip's span across it; the nearest such
    patch sets the gap. An outer strip sits at the median gap."""
    inner = _inner_strips(target)
    w, h = target["size"]
    if not inner:
        return inner
    gaps = sorted(d for _, _, d in inner)
    d = gaps[len(gaps) // 2]
    out = [(name, rect) for name, rect, _ in inner]
    for p in target["patches"]:
        x, y, pw, ph = p["rect"]
        sx = x + pw * (1 - STRIP_SPAN) / 2
        sy = y + ph * (1 - STRIP_SPAN) / 2
        a = d * (1 - STRIP) / 2
        sides = {"left": (x <= 1e-6, [x - a - d * STRIP, sy, d * STRIP, ph * STRIP_SPAN]),
                 "top": (y <= 1e-6, [sx, y - a - d * STRIP, pw * STRIP_SPAN, d * STRIP]),
                 "outer-right": (x + pw >= w - 1e-6,
                                 [x + pw + a, sy, d * STRIP, ph * STRIP_SPAN]),
                 "outer-below": (y + ph >= h - 1e-6,
                                 [sx, y + ph + a, pw * STRIP_SPAN, d * STRIP])}
        out += [(f"{p['id']}|{side}", rect) for side, (on_edge, rect) in sides.items()
                if on_edge]
    return out


def _inner_strips(target: dict) -> list[tuple[str, list[float], float]]:
    """(name, rect, gap) for the strip right of and below each patch with a neighbour."""
    out = []
    patches = target["patches"]
    for p in patches:
        x, y, w, h = p["rect"]
        for side in ("right", "below"):
            horizontal = side == "right"
            lo, span = (y, h) if horizontal else (x, w)
            s0 = lo + span * (1 - STRIP_SPAN) / 2
            s1 = s0 + span * STRIP_SPAN
            edge = x + w if horizontal else y + h
            gaps = []
            for q in patches:
                qx, qy, qw, qh = q["rect"]
                q_lo, q_span, q_near = (qy, qh, qx) if horizontal else (qx, qw, qy)
                if q_near > edge and q_lo <= s0 and q_lo + q_span >= s1:
                    gaps.append(q_near - edge)
            if not gaps or min(gaps) > 0.5 * min(w, h):
                continue
            d = min(gaps)
            a = edge + d * (1 - STRIP) / 2
            rect = ([a, s0, d * STRIP, s1 - s0] if horizontal
                    else [s0, a, s1 - s0, d * STRIP])
            out.append((f"{p['id']}|{side}", rect, d))
    return out


def _cross(o, a, b) -> float:
    return (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])


def is_convex(quad: list[list[float]]) -> bool:
    """Four points in order forming a convex quadrilateral (either winding: a scan
    may be mirrored)."""
    turns = [_cross(quad[i], quad[(i + 1) % 4], quad[(i + 2) % 4]) for i in range(4)]
    return all(t > 0 for t in turns) or all(t < 0 for t in turns)


# ------------------------------------------------------------------ schema

def check_label(e: object, fields: set[str] = LABEL_FIELDS) -> list[str]:
    """Every way `e` breaks a chart label's shape; empty when well formed."""
    if not isinstance(e, dict):
        return [f"not an object ({type(e).__name__})"]
    errs = [f"unknown key {k!r}" for k in sorted(set(e) - fields)]
    if e.get("target") not in target_ids():
        errs.append(f"target {e.get('target')!r} is not one of {', '.join(target_ids())}")
    if not is_text(e.get("card")):
        errs.append("card must be a non-empty string")
    c = e.get("corners")
    if not (isinstance(c, list) and len(c) == 4 and all(
            isinstance(p, list) and len(p) == 2 and all(is_number(v) for v in p) for p in c)):
        errs.append("corners must be four [x, y] points")
    elif not all(0 <= v <= 1 for p in c for v in p):
        errs.append("corners must lie inside the frame (fractions 0-1)")
    elif not is_convex(c):
        errs.append("corners must be in order around a convex quadrilateral")
    if "oriented" in e and e["oriented"] is not True:
        errs.append("oriented, when present, must be true")
    return errs


def check_entry(e: object) -> list[str]:
    """`check_label` plus the manifest's `source` and a passed `check`."""
    errs = check_label(e, ENTRY_FIELDS)
    if isinstance(e, dict):
        if not is_text(e.get("source")):
            errs.append("source must be a non-empty string")
        chk = e.get("check")
        if not (isinstance(chk, dict) and chk.get("pass") is True):
            errs.append("check must be a placement check that passed")
    return errs


def check_frame_charts(fr: dict) -> list[str]:
    """`check_entry` over a frame's `charts`, which must itself be a list, with one
    entry per (target, card)."""
    if "charts" not in fr:
        return []
    cs = fr["charts"]
    if not isinstance(cs, list):
        return [f"charts is not a list ({type(cs).__name__})"]
    errs = [f"chart {i}: {e}" for i, c in enumerate(cs) for e in check_entry(c)]
    keys = [(c["target"], c["card"]) for c in cs if isinstance(c, dict)
            and isinstance(c.get("target"), str) and isinstance(c.get("card"), str)]
    errs += [f"chart {t} on card {c!r} appears twice" for t, c in sorted(set(keys))
             if keys.count((t, c)) > 1]
    return errs


# ------------------------------------------------------------------ the check

def read_scan(path: str):
    """The scan's colour channels as stored (IR dropped); `_region` converts only the
    pixels it samples, since a full frame in float64 is gigabytes."""
    import tifffile
    img = tifffile.imread(path)
    if img.ndim != 3 or img.shape[2] < 3:
        raise ValueError(f"{path}: not an RGB scan (shape {img.shape})")
    return img[:, :, :3]


def _mask(shape: tuple[int, int], quad_px: list[list[float]]):
    """Pixels whose centres lie inside the convex quad, as (rows, cols)."""
    import numpy as np
    xs = [p[0] for p in quad_px]
    ys = [p[1] for p in quad_px]
    x0, x1 = max(int(min(xs)), 0), min(int(max(xs)) + 1, shape[1])
    y0, y1 = max(int(min(ys)), 0), min(int(max(ys)) + 1, shape[0])
    if x1 <= x0 or y1 <= y0:
        return np.empty(0, int), np.empty(0, int)
    yy, xx = np.mgrid[y0:y1, x0:x1]
    px, py = xx + 0.5, yy + 0.5
    sign = np.sign(_cross(quad_px[0], quad_px[1], quad_px[2]))
    inside = np.ones(px.shape, bool)
    for i in range(4):
        (ax, ay), (bx, by) = quad_px[i], quad_px[(i + 1) % 4]
        inside &= sign * ((bx - ax) * (py - ay) - (by - ay) * (px - ax)) >= 0
    return yy[inside], xx[inside]


def _median(xs: list[float]) -> float:
    s = sorted(xs)
    return (s[len(s) // 2] + s[(len(s) - 1) // 2]) / 2


def _region(scan, h, rect) -> "tuple[list[float], list[float], int] | None":
    """(median log2 transmission per channel, p10-p90 spread per channel, pixels), or
    None when the region lies outside the scan."""
    import numpy as np
    rows, cols = scan.shape[:2]
    quad = [project(h, p) for p in rect_quad(rect)]
    yy, xx = _mask((rows, cols), [[x * cols, y * rows] for x, y in quad])
    if len(yy) == 0:
        return None
    v = np.log2(np.maximum(scan[yy, xx].astype(np.float64), 1.0))
    p10, p50, p90 = np.percentile(v, [10, 50, 90], axis=0)
    return [float(x) for x in p50], [float(x) for x in p90 - p10], int(len(yy))


def check_placement(scan, target: dict, corners: list[list[float]]) -> dict:
    """The placement check of one target in one scan. Scene-brighter means *lower*
    transmission on a negative, so the neutral series (white to black) must rise."""
    h = target_to_frame(target, corners)
    regions = {p["id"]: _region(scan, h, shrink(p["rect"], INNER)) for p in target["patches"]}
    outside = [pid for pid, r in regions.items() if r is None]
    if outside:
        return {"pass": False, "rules": CHECK_VERSION, "target_version": target.get("version"),
                "worst_spread": None, "worst_gap": None, "neutral_steps": [],
                "bright_gaps": 0, "warnings": [], "patches": {},
                "failures": [f"{', '.join(outside)} lie outside the scan"]}
    patches, failures, med_of = {}, [], {}
    for p in target["patches"]:
        med, spread, n = regions[p["id"]]
        med_of[p["id"]] = med
        patches[p["id"]] = {"level": round(sum(med) / 3, 3), "spread": round(max(spread), 3),
                            "pixels": n}
        if max(spread) > UNIFORM_STOPS:
            failures.append(f"{p['id']} not uniform (spread {max(spread):.2f} stops)")
    neutral_levels = [patches[i]["level"] for i in target["neutrals"]]
    # The film's channel balance (its mask) read off the neutrals, so chroma is what a
    # patch has beyond it.
    offsets = [_median([med_of[i][c] - sum(med_of[i]) / 3 for i in target["neutrals"]])
               for c in range(3)]
    for pid in target.get("saturated", []):
        rel = [med_of[pid][c] - offsets[c] for c in range(3)]
        patches[pid]["chroma"] = round(max(rel) - min(rel), 3)
        if patches[pid]["chroma"] < CHROMA_STOPS:
            failures.append(f"{pid} shows no colour (chroma {patches[pid]['chroma']:.2f} stops)")
    black = max(neutral_levels)
    gaps, bright = {}, []
    for name, rect in gap_strips(target):
        region = _region(scan, h, rect)
        if region is None:  # an outer strip past the scan's edge says nothing
            continue
        level = sum(region[0]) / 3
        gaps[name] = round(level - black, 3)
        if level < black - GAP_STOPS:
            bright.append(f"gap {name} reads {black - level:.2f} stops brighter than black")
    warnings = []
    if len(bright) > GAP_SHARE * len(gaps):
        failures.append(f"{len(bright)} of {len(gaps)} gaps read bright: the grid is off")
        failures += bright
    else:
        warnings = bright
    steps = [round(b - a, 3) for a, b in zip(neutral_levels, neutral_levels[1:])]
    for (a, b), step in zip(zip(target["neutrals"], target["neutrals"][1:]), steps):
        if step < -ORDER_STOPS:
            failures.append(f"neutrals out of order at {a}->{b} ({step:+.2f} stops)")
    worst_gap = min(gaps.values()) if gaps else None
    return {"pass": not failures, "rules": CHECK_VERSION,
            "target_version": target.get("version"),
            "worst_spread": max(p["spread"] for p in patches.values()),
            "worst_gap": worst_gap, "neutral_steps": steps, "bright_gaps": len(bright),
            "failures": failures, "warnings": warnings, "patches": patches}


def orientations(corners: list[list[float]]) -> list[list[list[float]]]:
    """The label's corners as given, then every other way they could name the target's
    corners: the three other turns, then the four mirror images (a scan may be flipped)."""
    turns = [corners[i:] + corners[:i] for i in range(4)]
    flipped = corners[:1] + corners[1:][::-1]
    return turns + [flipped[i:] + flipped[:i] for i in range(4)]


def find_placement(scan, target: dict, corners: list[list[float]],
                   oriented: bool = False) -> dict:
    """The check of the one orientation of `corners` that passes, with the corners in
    the target's own order (`corners`, `reoriented`). The labelling page lets the
    corners be clicked in any order, so which one is the target's top left is the
    scan's to say: a wrong turn runs the neutrals backwards or puts a saturated patch
    on grey. When none passes, the result is the given orientation's. When several
    pass, nothing is guessed: the result fails with every passing orientation in
    `candidates`, and the user names the target's `anchor` patch to choose one; the
    label then says `oriented`, and only its own order is checked."""
    quads = [corners] if oriented else orientations(corners)
    results = [check_placement(scan, target, quad) | {"corners": quad, "reoriented": i > 0}
               for i, quad in enumerate(quads)]
    passing = [r for r in results if r["pass"]]
    if len(passing) == 1:
        return passing[0]
    if not passing:
        return results[0]
    anchor = target["anchor"]
    return results[0] | {
        "pass": False, "candidates": [r["corners"] for r in passing],
        "failures": [f"{len(passing)} ways up pass; click {anchor['prompt']} ({anchor['patch']}) "
                     "to say which"]}


def stored_check(result: dict, date: str) -> dict:
    """The summary a manifest entry keeps; per-patch numbers stay in the labels file."""
    return {k: result[k] for k in ("pass", "rules", "target_version", "worst_spread",
                                   "worst_gap", "bright_gaps", "neutral_steps")} | {"date": date}


# ------------------------------------------------------------------ labels file

def load_labels(path: str) -> dict:
    """The labelling tool's file: `{"roll", "frames": {stem: [label…]}}`; ValueError
    on any malformed label."""
    with open(path) as f:
        data = json.load(f)
    if not isinstance(data, dict) or not is_text(data.get("roll")) \
            or not isinstance(data.get("frames"), dict):
        raise ValueError(f"{path}: expected {{\"roll\": …, \"frames\": {{…}}}}")
    for stem, labels in data["frames"].items():
        if not isinstance(labels, list):
            raise ValueError(f"{path}: frame {stem}: not a list of labels")
        for i, e in enumerate(labels):
            if errs := check_label(e):
                raise ValueError(f"{path}: frame {stem} label {i}: " + "; ".join(errs))
    return data


def frame_entry(frames: list[dict], stem: str) -> dict | None:
    for fr in frames:
        if os.path.splitext(os.path.basename(fr["file"]))[0] == stem:
            return fr
    return None


def _open(asset_root: str, labels_path: str):
    """(manifest path, manifest, labels, roll entry) or raise ValueError."""
    from . import manifest as _manifest
    mpath = os.path.join(os.path.abspath(asset_root), "manifest.json")
    data, err = _manifest.load_manifest(mpath)
    if err or not data:
        raise ValueError(err or f"no manifest at {mpath}")
    labels = load_labels(labels_path)
    roll = data.get("rolls", {}).get(labels["roll"])
    if not isinstance(roll, dict):
        raise ValueError(f"unknown roll {labels['roll']!r}")
    return mpath, data, labels, roll


def check_frames(asset_root: str, labels: dict, roll: dict,
                 stems: list[str]) -> dict[str, list[dict]]:
    """{stem: [result per label]} for the named frames; ValueError for a frame the
    roll does not have."""
    out = {}
    for stem in stems:
        fr = frame_entry(roll.get("frames", []), stem)
        if fr is None:
            raise ValueError(f"roll {labels['roll']!r} has no frame {stem!r}")
        found = labels["frames"].get(stem, [])
        scan = read_scan(os.path.join(asset_root, fr["file"])) if found else None
        out[stem] = [find_placement(scan, load_target(e["target"]), e["corners"],
                                    e.get("oriented", False))
                     | {"target": e["target"], "card": e["card"]}
                     for e in found]
    return out


def cmd_check(args) -> int:
    """Print the placement check of every label (or `--frame`'s) as JSON. Exit 1 means
    a label failed and the JSON is complete; anything else operational is exit 2."""
    from .metrics import MetricsError, require_dependencies
    try:
        require_dependencies("chart check")
        _, _, labels, roll = _open(args.asset_root, args.labels)
        stems = [args.frame] if args.frame else sorted(labels["frames"])
        results = check_frames(os.path.abspath(args.asset_root), labels, roll, stems)
    except (OSError, ValueError, MetricsError) as e:
        print(f"error: {e}", file=sys.stderr)
        return 2
    json.dump({"roll": labels["roll"], "frames": results}, sys.stdout, indent=2,
              allow_nan=False)
    print()
    return 0 if all(r["pass"] for rs in results.values() for r in rs) else 1


def cmd_import(args) -> int:
    """Check every label and fold the frames into the manifest. Each listed frame loses
    its entries from the same `--source` (and any for the same target and card) and
    gains the file's, so a label removed or re-carded in the tool goes from the manifest
    too; a frame listed with no labels keeps only other sources'. Nothing is written if
    any label fails."""
    from . import manifest as _manifest
    from .metrics import MetricsError, require_dependencies
    try:
        require_dependencies("chart import")
        mpath, data, labels, roll = _open(args.asset_root, args.labels)
        stems = sorted(labels["frames"])
        results = check_frames(os.path.abspath(args.asset_root), labels, roll, stems)
    except (OSError, ValueError, MetricsError) as e:
        print(f"error: {e}; nothing was written", file=sys.stderr)
        return 2
    failed = [f"{s} {r['target']}: " + "; ".join(r["failures"])
              for s, rs in results.items() for r in rs if not r["pass"]]
    if failed:
        print("error: placement check failed, nothing was written:\n  "
              + "\n  ".join(failed), file=sys.stderr)
        return 1
    date = args.date or datetime.date.today().isoformat()
    for stem in stems:
        fr = frame_entry(roll["frames"], stem)
        if not isinstance(fr.get("charts", []), list):
            print(f"error: {fr['file']} has a `charts` that is not a list; fix it by hand",
                  file=sys.stderr)
            return 2
        new = [{"target": e["target"], "card": e["card"], "corners": r["corners"],
                "source": args.source, "check": stored_check(r, date)}
               for e, r in zip(labels["frames"][stem], results[stem])]
        replaced = {(e["target"], e["card"]) for e in new}
        kept = [c for c in fr.get("charts", []) if not isinstance(c, dict) or (
            c.get("source") != args.source
            and (c.get("target"), c.get("card")) not in replaced)]
        if kept or new:
            fr["charts"] = kept + new
        else:
            fr.pop("charts", None)
        print(f"{fr['file']}: {len(new)} chart(s)")
    data["schema_version"] = _manifest.SCHEMA
    if args.dry_run:
        print("(dry run — manifest.json not written)")
        return 0
    _manifest.write_manifest(mpath, data)
    print("wrote", mpath)
    return 0

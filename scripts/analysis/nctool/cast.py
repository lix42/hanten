"""The colour cast a conversion leaves on a roll's chart frames — `manifest charts cast`.

`render` measures the roll on its pictures only (`role: real`, so the chart frames are an
independent check), then renders every chart frame through a grid of configurations.
`measure` samples each labelled patch of those renders, and of any outside producer's
exports of the same frames, as CIELAB (D50, display white L* 100), and summarises:

- **neutral cast**: a*/b* of the grey patches, by rendered lightness band and by bracket
  frame. Counted only between L* 15 and 95, where a cast is measurable;
- **colour patches**: hue error and chroma ratio against the target's reference values, on
  the frame whose mid-grey renders nearest the reference's (lightness changes chroma, so an
  over-bright frame is not compared);
- **density**: each channel's density above base after the decode's scale, against green's,
  from `charts.json` — the decode-side view, where a constant per-channel offset (one gain)
  and a slope error look different. In Lab both rise with lightness.

The procedure and how to read the numbers: the `chart-cast` skill. Results:
`docs/reports/chart-cast.md`.
"""
from __future__ import annotations

import concurrent.futures
import json
import math
import os
import subprocess
import sys

from . import charts as _charts

D50 = (0.96422, 1.0, 0.82521)
NEUTRAL_L = (15.0, 95.0)
BANDS = {"dark": (15.0, 40.0), "mid": (40.0, 70.0), "light": (70.0, 95.0)}
SATURATED_C = 20.0   # least reference chroma whose hue error is counted
DENSITY_BINS = 0.2   # green density bin width of the density residuals
DENSITY_RANGE = (0.2, 1.0)  # green density above base the offset is averaged over
DESTINATION = ["--range", "sdr", "--gamut", "display-p3", "--container", "tiff"]

#: The grid `render` expands by default. `extra` adds named configurations by hand.
DEFAULT_GRID = {
    "placement": ["roll", "hybrid", "frame"],
    "highlight_desaturation": ["0.8", "0"],
    "white_balance_form": ["gain", "tilt"],
    "midtone_neutral": ["on", "off", "red", "blue"],
    "balance_off": True,
    "direct": True,
    "extra": {},
}


# ------------------------------------------------------------------ the grid

def expand_grid(grid: dict) -> dict[str, dict]:
    """Configuration name → `{"recipe": form or None, "args": [...]}`. `recipe` names the
    roll recipe (measured under that white-balance form) the run reads; None reads none."""
    configs: dict[str, dict] = {}
    if grid.get("direct"):
        configs["direct"] = {"recipe": None, "args": ["--rendering", "direct"]}
    forms = grid.get("white_balance_form", [])
    for p in grid.get("placement", []):
        for hd in grid.get("highlight_desaturation", []):
            common = ["--placement", p, "--highlight-desaturation", hd]
            if grid.get("balance_off") and forms:
                configs[f"{p}.hd{hd}.wb-off"] = {
                    "recipe": forms[0], "args": ["--neutral-balance", "off", *common]}
            for form in forms:
                for mn in grid.get("midtone_neutral", []):
                    configs[f"{p}.hd{hd}.{form}.mn-{mn}"] = {
                        "recipe": form, "args": ["--midtone-neutral", mn, *common]}
    for name, spec in grid.get("extra", {}).items():
        if name in configs:
            raise ValueError(f"extra configuration {name!r} repeats a grid name")
        configs[name] = {"recipe": spec.get("recipe"), "args": list(spec.get("args", []))}
    return configs


def roll_frames(manifest: dict, roll: str) -> dict[str, list[dict]]:
    """The roll's frames by role, each list in file order."""
    if roll not in manifest.get("rolls", {}):
        raise ValueError(f"no roll {roll!r} in the manifest")
    out: dict[str, list[dict]] = {}
    for f in sorted(manifest["rolls"][roll]["frames"], key=lambda f: f["file"]):
        out.setdefault(f.get("role") or "real", []).append(f)
    return out


def stem(frame: dict) -> str:
    return os.path.splitext(os.path.basename(frame["file"]))[0]


def graft_tones(recipe: dict, with_charts: dict, names: list[str]) -> dict:
    """`recipe` (measured on the pictures) with the `roll.frames` entries of `names` taken
    from `with_charts` (measured with the chart frames included): placement reads a frame's
    own tones, and the pictures-only measurement has none for the chart frames."""
    out = json.loads(json.dumps(recipe))
    src = with_charts["params"]["roll"]["frames"]
    dst = out["params"]["roll"].setdefault("frames", {})
    for n in names:
        if n not in src:
            raise ValueError(f"the measurement with the chart frames has no entry for {n}")
        dst[n] = src[n]
    return out


# ------------------------------------------------------------------ render

def _run(cmd: list[str], stdout_path: str | None = None) -> None:
    with open(stdout_path or os.devnull, "w") as out:
        proc = subprocess.run(cmd, stdout=out, stderr=subprocess.PIPE, text=True)
    if proc.returncode != 0:
        raise RuntimeError(f"{os.path.basename(cmd[0])} {cmd[1]} failed "
                           f"({proc.returncode}): {proc.stderr.strip()[-400:]}")


def prepare_recipes(nc: str, root: str, frames: dict, work: str, forms: list[str]) -> dict:
    """Measure the roll per white-balance form; returns form → recipe path."""
    pictures = [os.path.join(root, f["file"]) for f in frames.get("real", [])]
    chart = [os.path.join(root, f["file"]) for f in frames.get("chart", [])]
    if not pictures or not chart:
        raise ValueError("the roll needs both picture (real) and chart frames")
    ref = []
    for role, flag in (("unexposed", "--unexposed"), ("leader", "--leader")):
        if frames.get(role):
            ref += [flag, os.path.join(root, frames[role][0]["file"])]
    rdir = os.path.join(work, "recipes")
    os.makedirs(rdir, exist_ok=True)
    out = {}
    for form in forms:
        mine = os.path.join(rdir, f"roll-{form}.json")
        both = os.path.join(rdir, f"roll-with-charts-{form}.json")
        for inputs, path in ((pictures, mine), (pictures + chart, both)):
            _run([nc, "measure-roll", *inputs, *ref, "--white-balance-form", form,
                  "--out", path])
        with open(mine) as a, open(both) as b:
            grafted = graft_tones(json.load(a), json.load(b),
                                  [os.path.basename(p) for p in chart])
        path = os.path.join(rdir, f"charts-{form}.json")
        with open(path, "w") as f:
            json.dump(grafted, f, indent=1)
        out[form] = path
    return out


def render_commands(nc: str, root: str, frames: dict, work: str, configs: dict,
                    recipes: dict) -> list[tuple[list[str], str]]:
    """(convert command, report path) per configuration and chart frame."""
    base = None
    if recipes:
        with open(next(iter(recipes.values()))) as f:
            base = json.load(f)["params"]["calibration"]["film_base"]["explicit"]
    cmds = []
    for name, spec in configs.items():
        odir = os.path.join(work, "renders", name)
        os.makedirs(odir, exist_ok=True)
        if spec["recipe"] is None:
            if base is None:
                raise ValueError(f"{name}: no measured film base to render without a recipe")
            pre = ["--film-base", ",".join(str(x) for x in base)]
        else:
            if spec["recipe"] not in recipes:
                raise ValueError(f"{name}: no recipe measured under {spec['recipe']!r}")
            pre = ["--params", recipes[spec["recipe"]]]
        for f in frames["chart"]:
            out = os.path.join(odir, stem(f))
            cmds.append(([nc, "convert", os.path.join(root, f["file"]), *pre, *spec["args"],
                          *DESTINATION, "-o", out + ".tif", "--report", "json"], out + ".json"))
    return cmds


# ------------------------------------------------------------------ measure

def read_linear(path: str):
    """A rendered image as linear RGB and its RGB → XYZ (D50) matrix, from its ICC profile."""
    import numpy as np
    from pathlib import Path
    from . import metrics, reference
    icc, enc = reference.read_export(Path(path))
    name, _ = reference.resolve_space(icc, None)
    space = metrics.SPACES[name]
    lin = metrics._decode_transfer(enc, space.transfer)
    m = metrics._mat_mul(metrics.bradford_adaptation("d65", "d50"),
                         metrics.rgb_to_xyz_d65(space))
    return lin, np.asarray(m, dtype=np.float64), name


def patch_rows(lin, matrix, frame_stem: str, entries: list[dict]) -> list[dict]:
    """Every neutral and reference-valued patch of `entries` (a frame's chart entries) in
    `lin`, as the median of its middle half, in CIELAB D50."""
    import numpy as np
    from . import cie
    rows = []
    for entry in entries:
        t = _charts.load_target(entry["target"])
        h = _charts.target_to_frame(t, entry["corners"])
        ref = (t.get("reference") or {}).get("values", {})
        for p in t["patches"]:
            pid = p["id"]
            role = "neutral" if pid in t["neutrals"] else "colour" if pid in ref else None
            if role is None:
                continue
            s = _charts.sample(lin, h, _charts.shrink(p["rect"], _charts.INNER))
            if s is None:
                raise ValueError(f"{frame_stem} {entry['target']} {pid}: outside the image")
            rgb = np.median(s, axis=0)
            lab = cie.xyz_to_lab(matrix @ rgb, white=D50)
            rows.append({"frame": frame_stem, "target": entry["target"], "patch": pid,
                         "role": role, "lab": [round(float(x), 3) for x in lab],
                         "ref": ref.get(pid)})
    return rows


def _cast(labs: list) -> dict:
    a = [x[1] for x in labs]
    b = [x[2] for x in labs]
    return {"n": len(labs), "a": sum(a) / len(a), "b": sum(b) / len(b),
            "c": sum(math.hypot(x, y) for x, y in zip(a, b)) / len(a)}


def _hue(a: float, b: float) -> float:
    return math.degrees(math.atan2(b, a)) % 360


def summarize(rows: list[dict]) -> dict:
    """Neutral cast overall and by band; colour accuracy on the exposure-matched frame."""
    neutral = [r["lab"] for r in rows if r["role"] == "neutral"]
    s: dict = {}
    counted = [x for x in neutral if NEUTRAL_L[0] <= x[0] < NEUTRAL_L[1]]
    if counted:
        s["all"] = _cast(counted)
    for band, (lo, hi) in BANDS.items():
        sel = [x for x in neutral if lo <= x[0] < hi]
        if sel:
            s[band] = _cast(sel)
    # The reference neutral nearest mid-grey (L* 50) picks the exposure-matched frame.
    refs = [r for r in rows if r["role"] == "neutral" and r["ref"]]
    if refs:
        mid = min({r["patch"]: r["ref"][0] for r in refs}.items(), key=lambda kv: abs(kv[1] - 50))
        lights = {r["frame"]: r["lab"][0] for r in refs if r["patch"] == mid[0]}
        frame = min(lights, key=lambda f: abs(lights[f] - mid[1]))
        s["matched"] = {"frame": frame, "patch": mid[0], "l": lights[frame], "ref_l": mid[1],
                        **colour_accuracy([r for r in rows if r["frame"] == frame])}
    return s


def colour_accuracy(rows: list[dict]) -> dict:
    """Mean |hue error| (degrees) and median chroma ratio of the saturated reference
    patches, mean ΔE00 and mean ΔL* of every reference colour patch."""
    import numpy as np
    from . import cie
    col = [r for r in rows if r["role"] == "colour" and r["ref"]]
    if not col:
        return {}
    lab = np.array([r["lab"] for r in col])
    ref = np.array([r["ref"] for r in col])
    dh, cr = [], []
    for l, rf in zip(lab, ref):
        c_ref = math.hypot(rf[1], rf[2])
        if c_ref >= SATURATED_C:
            dh.append((_hue(l[1], l[2]) - _hue(rf[1], rf[2]) + 180) % 360 - 180)
            cr.append(math.hypot(l[1], l[2]) / c_ref)
    return {"hue_error": float(np.mean(np.abs(dh))) if dh else None,
            "chroma_ratio": float(np.median(cr)) if cr else None,
            "delta_e": float(cie.delta_e_2000(lab, ref).mean()),
            "delta_l": float((lab[:, 0] - ref[:, 0]).mean())}


def per_frame(rows: list[dict]) -> dict:
    """The neutral cast of each frame (L* 15–95)."""
    out = {}
    for f in sorted({r["frame"] for r in rows}):
        sel = [r["lab"] for r in rows if r["frame"] == f and r["role"] == "neutral"
               and NEUTRAL_L[0] <= r["lab"][0] < NEUTRAL_L[1]]
        if sel:
            out[f] = _cast(sel)
    return out


def density_residuals(table_frames: dict, scale: list[float]) -> dict:
    """Red and blue density minus green, after `scale`, over every grey patch of the
    roll's chart table frames: by green density bin, the mean over `DENSITY_RANGE` (the
    offset that would cancel it, negated), and each channel's fitted slope against green."""
    pts = [[d * k for d, k in zip(p["density"], scale)]
           for f in table_frames.values() for c in f["charts"]
           for p in c["patches"] if p["role"] == "neutral"]
    if not pts:
        return {}
    bins = {}
    for r, g, b in pts:
        lo = math.floor(g / DENSITY_BINS) * DENSITY_BINS
        bins.setdefault(round(lo, 3), []).append((r - g, b - g))
    sel = [(r - g, b - g) for r, g, b in pts if DENSITY_RANGE[0] <= g < DENSITY_RANGE[1]]

    def slope(i: int) -> float:
        gs = [p[1] for p in pts]
        cs = [p[i] for p in pts]
        mg, mc = sum(gs) / len(gs), sum(cs) / len(cs)
        return (sum((x - mg) * (y - mc) for x, y in zip(gs, cs))
                / sum((x - mg) ** 2 for x in gs))

    return {
        "scale": list(scale),
        "bins": {f"{lo:.1f}": {"n": len(v), "red": sum(x[0] for x in v) / len(v),
                               "blue": sum(x[1] for x in v) / len(v)}
                 for lo, v in sorted(bins.items())},
        "offset": {"range": list(DENSITY_RANGE), "n": len(sel),
                   "red": -sum(x[0] for x in sel) / len(sel) if sel else None,
                   "blue": -sum(x[1] for x in sel) / len(sel) if sel else None},
        "slope": {"red": slope(0), "blue": slope(2)},
    }


# ------------------------------------------------------------------ commands

def _load(root: str) -> dict:
    from . import manifest as _manifest
    data, err = _manifest.load_manifest(os.path.join(root, "manifest.json"))
    if err or not data:
        raise ValueError(err or f"no manifest at {root}")
    return data


def cmd_render(args) -> int:
    from . import manifest as _manifest
    root = os.path.abspath(args.asset_root)
    try:
        frames = roll_frames(_load(root), args.roll)
        explicit = args.nc if args.nc is not None else os.environ.get("NC")
        nc, err = _manifest.resolve_nc(explicit, "--nc" if args.nc is not None else "$NC")
        if err or not nc:
            raise ValueError(err or "no hanten binary found (build it, or pass --nc)")
        grid = DEFAULT_GRID
        if args.grid:
            with open(args.grid) as f:
                grid = {**DEFAULT_GRID, **json.load(f)}
        configs = expand_grid(grid)
        work = os.path.abspath(args.work)
        recipes = prepare_recipes(nc, root, frames, work,
                                  sorted({c["recipe"] for c in configs.values()} - {None}))
        cmds = render_commands(nc, root, frames, work, configs, recipes)
        with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
            list(pool.map(lambda c: _run(*c), cmds))
    except (OSError, ValueError, RuntimeError) as e:
        print(f"error: {e}", file=sys.stderr)
        return 2
    print(f"rendered {len(configs)} configurations x {len(frames['chart'])} chart frames "
          f"into {os.path.join(work, 'renders')}")
    return 0


def _sources(work: str, extra: list[str]) -> dict[str, str]:
    out = {}
    rdir = os.path.join(work, "renders")
    if os.path.isdir(rdir):
        for d in sorted(os.listdir(rdir)):
            if os.path.isdir(os.path.join(rdir, d)):
                out[f"hanten:{d}"] = os.path.join(rdir, d)
    for s in extra:
        name, sep, path = s.partition("=")
        if not sep or not name or not path:
            raise ValueError(f"--source {s!r}: expected NAME=DIR")
        out[name] = path
    return out


def measure(root: str, roll: str, work: str, extra: list[str]) -> dict:
    """The cast file: per-patch rows, summaries and per-frame casts per source."""
    from .chart_table import TABLE_NAME
    frames = roll_frames(_load(root), roll)
    chart = frames.get("chart", [])
    if not chart:
        raise ValueError(f"roll {roll} has no chart frames")
    out = {"roll": roll, "frames": {stem(f): (f.get("shot") or {}).get("exposure_ev")
                                    for f in chart}, "sources": {}}
    for name, d in _sources(work, extra).items():
        rows, spaces = [], set()
        for f in chart:
            lin, m, space = read_linear(os.path.join(d, stem(f) + ".tif"))
            spaces.add(space)
            rows += patch_rows(lin, m, stem(f), f.get("charts", []))
        out["sources"][name] = {"space": sorted(spaces), "summary": summarize(rows),
                                "per_frame": per_frame(rows), "patches": rows}
    table = os.path.join(root, TABLE_NAME)
    recipe = os.path.join(work, "recipes", "roll-gain.json")
    if os.path.exists(table) and os.path.exists(recipe):
        with open(table) as t, open(recipe) as r:
            scale = json.load(r)["params"]["reconstruction"]["scale"]
            tframes = json.load(t)["rolls"].get(roll, {}).get("frames", {})
        out["density"] = density_residuals(tframes, scale)
    return out


def _fmt(x) -> str:
    return "     -" if x is None else f"{x:+6.1f}"


def print_table(cast: dict, file=sys.stdout) -> None:
    print(f"{'source':38} {'C*':>5} {'a*':>6} {'b*':>6} | {'dark b*':>6} {'mid b*':>6} "
          f"{'lt b*':>6} | {'dark a*':>6} {'mid a*':>6} {'lt a*':>6} | {'|dh|':>5} {'Cr':>5}",
          file=file)
    for name, src in cast["sources"].items():
        s = src["summary"]
        al = s.get("all", {})
        band = lambda b, k: _fmt(s.get(b, {}).get(k))
        m = s.get("matched", {})
        print(f"{name:38} {al.get('c', float('nan')):5.1f} {_fmt(al.get('a'))} "
              f"{_fmt(al.get('b'))} | {band('dark', 'b')} {band('mid', 'b')} "
              f"{band('light', 'b')} | {band('dark', 'a')} {band('mid', 'a')} "
              f"{band('light', 'a')} | {m.get('hue_error') or float('nan'):5.1f} "
              f"{m.get('chroma_ratio') or float('nan'):5.2f}", file=file)
    d = cast.get("density")
    if d:
        o = d["offset"]
        print(f"density after scale {d['scale']}: offset to cancel red {o['red']:+.4f}, "
              f"blue {o['blue']:+.4f} (n {o['n']}); slope against green red "
              f"{d['slope']['red']:.3f}, blue {d['slope']['blue']:.3f}", file=file)


def cmd_measure(args) -> int:
    from .metrics import MetricsError, require_dependencies
    root = os.path.abspath(args.asset_root)
    try:
        require_dependencies("charts cast measure")
        cast = measure(root, args.roll, os.path.abspath(args.work), args.source or [])
        out = args.out or os.path.join(args.work, "cast.json")
        with open(out, "w") as f:
            json.dump(cast, f)
    except (OSError, ValueError, MetricsError) as e:
        print(f"error: {e}", file=sys.stderr)
        return 2
    print_table(cast)
    print(f"wrote {out}")
    return 0

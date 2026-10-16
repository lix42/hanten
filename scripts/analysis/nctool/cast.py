"""The residual cast a conversion leaves on a roll's chart frames — `manifest charts cast`.

`render` measures the roll on its pictures only (`role: real`, so the chart frames are an
independent check), then renders every chart frame through a grid of configurations.
`measure` samples each labelled patch of those renders, and of any outside producer's
exports of the same frames, as CIELAB (D50, display white L* 100), and summarises:

- **greys**: a*/b* of the grey patches, by rendered lightness band and by bracket
  frame. Counted only between L* 15 and 95, where a residual cast is measurable;
- **colour patches**: CIELAB hue error and chroma ratio against the target's reference
  values, on the card whose D4 grey patch renders nearest the reference's L* (lightness
  changes chroma, so an over-bright frame is not compared);
- **density**: each channel's density above base after the decode's scale, against green's,
  from `charts.json` — the decode-side view, where a constant per-channel offset (one gain)
  and a density-scale error look different. In CIELAB both rise with lightness. The fitted
  density scale against green is fitted over `DENSITY_RANGE`, the straight part; the
  shoulder's separately from `SHOULDER_FROM`, where that range ends.

The procedure and how to read the numbers: the `chart-cast` skill. Results:
`docs/reports/chart-cast.md`.
"""
from __future__ import annotations

import concurrent.futures
import json
import math
import os
import re
import shutil
import subprocess
import sys

from . import chart_table as _chart_table
from . import charts as _charts
from . import manifest as _manifest

D50 = (0.96422, 1.0, 0.82521)
NEUTRAL_L = (15.0, 95.0)
BANDS = {"dark": (15.0, 40.0), "mid": (40.0, 70.0), "light": (70.0, 95.0)}
SATURATED_C = 20.0   # least reference chroma whose hue error is counted
DENSITY_BINS = 0.2   # green density bin width of the density residuals
SHOULDER_FROM = 0.9  # green density above base where the shoulder starts (Gold 200's +3/+4)
DENSITY_RANGE = (0.2, SHOULDER_FROM)  # the straight part: offset and scale are fitted here
DESTINATION = ["--range", "sdr", "--gamut", "display-p3", "--container", "tiff"]
CONFIGS_NAME = "configs.json"  # what `render` produced, in the work folder

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


def check_stems(chart: list[dict]) -> None:
    """Refuse chart frames whose file stems repeat: their renders would share a path."""
    stems = [stem(f) for f in chart]
    dup = sorted({s for s in stems if stems.count(s) > 1})
    if dup:
        raise ValueError(f"chart frames share a file stem ({', '.join(dup)}), so their "
                         "renders would overwrite each other")


def _field(doc: dict, keys: tuple, what: str):
    """`doc[keys[0]][keys[1]]…`, or ValueError naming `what` and the missing key."""
    cur = doc
    for k in keys:
        if not isinstance(cur, dict) or k not in cur:
            raise ValueError(f"{what} has no {'.'.join(keys)}")
        cur = cur[k]
    return cur


def graft_frames(recipe: dict, with_charts: dict, names: list[str]) -> dict:
    """`recipe` (measured on the pictures) with the whole `roll.frames` entries of `names`
    taken from `with_charts` (measured with the chart frames included). Those entries are
    per-frame placement, which the pictures-only measurement has none of for the chart
    frames; the roll's colour (`roll.white_balance`, the midtone line) stays the pictures'."""
    out = json.loads(json.dumps(recipe))
    src = _field(with_charts, ("params", "roll", "frames"), "the measurement with the charts")
    roll = _field(out, ("params", "roll"), "the pictures-only measurement")
    if not isinstance(roll.get("frames"), dict):
        roll["frames"] = {}
    for n in names:
        if n not in src:
            raise ValueError(f"the measurement with the chart frames has no entry for {n}")
        roll["frames"][n] = src[n]
    return out


def balance_divergence(recipe: dict, with_charts: dict) -> tuple[list, list, float]:
    """Both measurements' `roll.white_balance` and their largest per-channel difference
    (a fraction of the pictures-only gain)."""
    a = _field(recipe, ("params", "roll", "white_balance"), "the pictures-only measurement")
    b = _field(with_charts, ("params", "roll", "white_balance"),
               "the measurement with the charts")
    if not (isinstance(a, list) and isinstance(b, list) and len(a) == len(b) == 3):
        raise ValueError(f"roll.white_balance is not three gains in both ({a}, {b})")
    return a, b, max(abs(y / x - 1) for x, y in zip(a, b))


# ------------------------------------------------------------------ render

def _run(cmd: list[str], stdout_path: str | None = None) -> str:
    """Run `cmd`; its stderr when it succeeds, RuntimeError when it fails."""
    with open(stdout_path or os.devnull, "w") as out:
        proc = subprocess.run(cmd, stdout=out, stderr=subprocess.PIPE, text=True)
    if proc.returncode != 0:
        raise RuntimeError(f"{os.path.basename(cmd[0])} {cmd[1]} failed "
                           f"({proc.returncode}): {proc.stderr.strip()[-400:]}")
    return proc.stderr.strip()


def check_destination_build(nc: str) -> str:
    """`nc`'s `--version` banner; ValueError if it does not take destination flags
    (`DESTINATION`)."""
    try:
        banner = subprocess.run([nc, "--version"], capture_output=True, text=True,
                                timeout=15).stdout.strip()
    except (OSError, subprocess.SubprocessError) as e:
        raise ValueError(f"{nc} --version failed: {e}") from e
    version = _manifest.banner_pipeline_version(banner)
    if version is None:
        raise ValueError(f"{nc} --version states no pipeline_version, so whether it takes "
                         "destination flags is unknown")
    if _manifest.output_interface(version) != "destination":
        raise ValueError(f"{nc} predates destinations (pipeline_version < "
                         f"{_manifest.DESTINATION_PIPELINE}); `cast render` passes "
                         f"{' '.join(DESTINATION)}")
    return banner


def prepare_recipes(nc: str, root: str, frames: dict, work: str, forms: list[str],
                    log=None) -> dict:
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
            err = _run([nc, "measure-roll", *inputs, *ref, "--white-balance-form", form,
                        "--out", path, "--force"])
            for line in err.splitlines():
                print(f"hanten measure-roll ({os.path.basename(path)}): {line}", file=sys.stderr)
        with open(mine) as a, open(both) as b:
            mine_r, both_r = json.load(a), json.load(b)
        grafted = graft_frames(mine_r, both_r, [os.path.basename(p) for p in chart])
        wa, wb, diff = balance_divergence(mine_r, both_r)
        print(f"{form}: roll.white_balance on the pictures {wa}, with the chart frames {wb} "
              f"(largest difference {100 * diff:.1f} %); the chart frames' placement "
              "entries come from the latter", file=log or sys.stdout)
        path = os.path.join(rdir, f"charts-{form}.json")
        with open(path, "w") as f:
            json.dump(grafted, f, indent=1)
        out[form] = path
    return out


def _recipe_field(path: str, keys: tuple):
    with open(path) as f:
        return _field(json.load(f), keys, path)


def render_commands(nc: str, root: str, frames: dict, work: str, configs: dict,
                    recipes: dict) -> list[tuple[list[str], str]]:
    """(convert command, report path) per configuration and chart frame."""
    base = None
    if recipes:
        base = _recipe_field(next(iter(recipes.values())),
                             ("params", "calibration", "film_base", "explicit"))
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


def check_work(work: str) -> None:
    """Refuse a work folder with renders this tool did not record."""
    if (os.path.exists(os.path.join(work, "renders"))
            and not os.path.exists(os.path.join(work, CONFIGS_NAME))):
        raise ValueError(f"{work} has renders/ but no {CONFIGS_NAME}: pick an empty folder "
                         "or an earlier `charts cast` work folder")


def reset_work(work: str, roll: str) -> None:
    """Clear an earlier run's renders from `work` and mark its record incomplete until
    this run finishes."""
    check_work(work)
    shutil.rmtree(os.path.join(work, "renders"), ignore_errors=True)
    write_record(work, {"roll": roll, "complete": False})


def write_record(work: str, rec: dict) -> None:
    os.makedirs(work, exist_ok=True)
    with open(os.path.join(work, CONFIGS_NAME), "w") as f:
        json.dump(rec, f, indent=1)


#: A path or file name (kept whole), else a number (masked).
_FILE_OR_NUMBER = re.compile(r"\S*[/.][A-Za-z]\S*|(\d+(?:\.\d+)?)")


def warning_summary(stderrs: list[str]) -> list[str]:
    """The renders' stderr lines grouped by wording, numbers shown as `#` where they vary
    (file names kept), each with how many lines it stands for."""
    groups: dict[str, list[str]] = {}
    for line in (x for e in stderrs for x in e.splitlines() if x):
        groups.setdefault(_FILE_OR_NUMBER.sub(lambda m: "#" if m.group(1) else m.group(0),
                                              line), []).append(line)
    return [f"{len(v)}x {k if len(set(v)) > 1 else v[0]}" for k, v in sorted(groups.items())]


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
        if why := _chart_table.stale_reason(entry, t):
            raise ValueError(f"{frame_stem}: {entry['target']} on card {entry.get('card')!r}: "
                             f"{why}; re-run `nctool manifest charts import` with its labels")
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
            rows.append({"frame": frame_stem, "target": entry["target"],
                         "card": entry.get("card"), "patch": pid, "role": role,
                         "lab": [round(float(x), 3) for x in lab], "ref": ref.get(pid)})
    return rows


def _cast(labs: list) -> dict:
    a = [x[1] for x in labs]
    b = [x[2] for x in labs]
    return {"n": len(labs), "a": sum(a) / len(a), "b": sum(b) / len(b),
            "c": sum(math.hypot(x, y) for x, y in zip(a, b)) / len(a)}


def _hue(a: float, b: float) -> float:
    return math.degrees(math.atan2(b, a)) % 360


def summarize(rows: list[dict]) -> dict:
    """Grey a*/b* overall and by band; colour accuracy on the lightness-matched card."""
    neutral = [r["lab"] for r in rows if r["role"] == "neutral"]
    s: dict = {}
    counted = [x for x in neutral if NEUTRAL_L[0] <= x[0] < NEUTRAL_L[1]]
    if counted:
        s["all"] = _cast(counted)
    for band, (lo, hi) in BANDS.items():
        sel = [x for x in neutral if lo <= x[0] < hi]
        if sel:
            s[band] = _cast(sel)
    # The reference neutral nearest L* 50 picks the lightness-matched card.
    refs = [r for r in rows if r["role"] == "neutral" and r["ref"]]
    if refs:
        mid, ref_l = min({(r["target"], r["patch"]): r["ref"][0] for r in refs}.items(),
                         key=lambda kv: abs(kv[1] - 50))
        lights = {(r["frame"], r["card"]): r["lab"][0] for r in refs
                  if (r["target"], r["patch"]) == mid}
        frame, card = min(lights, key=lambda k: abs(lights[k] - ref_l))
        s["matched"] = {"frame": frame, "card": card, "target": mid[0], "patch": mid[1],
                        "l": lights[(frame, card)], "ref_l": ref_l,
                        **colour_accuracy([r for r in rows
                                           if r["frame"] == frame and r["card"] == card])}
    return s


def colour_accuracy(rows: list[dict]) -> dict:
    """Mean |CIELAB hue error| (degrees) and median CIELAB chroma ratio of the saturated
    reference patches, mean ΔE00 and mean ΔL* of every reference colour patch."""
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
    """Each frame's grey a*/b* (L* 15–95)."""
    out = {}
    for f in sorted({r["frame"] for r in rows}):
        sel = [r["lab"] for r in rows if r["frame"] == f and r["role"] == "neutral"
               and NEUTRAL_L[0] <= r["lab"][0] < NEUTRAL_L[1]]
        if sel:
            out[f] = _cast(sel)
    return out


def _scale_fit(pts: list, i: int) -> float | None:
    """Channel `i`'s least-squares density scale against green, or None with no spread in
    green."""
    if not pts:
        return None
    gs = [p[1] for p in pts]
    cs = [p[i] for p in pts]
    mg, mc = sum(gs) / len(gs), sum(cs) / len(cs)
    den = sum((x - mg) ** 2 for x in gs)
    if den == 0:
        return None
    return sum((x - mg) * (y - mc) for x, y in zip(gs, cs)) / den


def density_residuals(table_frames: dict, scale: list[float]) -> dict:
    """Red and blue density minus green, after `scale`, over every grey patch of the
    roll's chart table frames: by green density bin; over `DENSITY_RANGE` the mean (the
    offset that would cancel it, negated) and each channel's fitted density scale against
    green; and that scale from `SHOULDER_FROM` up, the shoulder."""
    pts = [[d * k for d, k in zip(p["density"], scale)]
           for f in table_frames.values() for c in f["charts"]
           for p in c["patches"] if p["role"] == "neutral"]
    if not pts:
        return {}
    lo, hi = DENSITY_RANGE
    bins: dict[int, list] = {}
    for r, g, b in pts:
        # The epsilon keeps a value on a bin edge (0.6 / 0.2 = 2.9999…) in its own bin.
        bins.setdefault(math.floor(g / DENSITY_BINS + 1e-9), []).append((r - g, b - g))
    straight = [p for p in pts if lo <= p[1] < hi]
    shoulder = [p for p in pts if p[1] >= SHOULDER_FROM]
    sel = [(r - g, b - g) for r, g, b in straight]
    return {
        "scale": list(scale),
        "bins": {f"{i * DENSITY_BINS:.1f}": {"n": len(v), "red": sum(x[0] for x in v) / len(v),
                                             "blue": sum(x[1] for x in v) / len(v)}
                 for i, v in sorted(bins.items())},
        "offset": {"range": list(DENSITY_RANGE), "n": len(sel),
                   "red": -sum(x[0] for x in sel) / len(sel) if sel else None,
                   "blue": -sum(x[1] for x in sel) / len(sel) if sel else None},
        "scale_fit": {"range": list(DENSITY_RANGE), "n": len(straight),
                      "red": _scale_fit(straight, 0), "blue": _scale_fit(straight, 2)},
        "shoulder": {"from": SHOULDER_FROM, "n": len(shoulder),
                     "green": [min(p[1] for p in shoulder), max(p[1] for p in shoulder)]
                     if shoulder else None,
                     "red": _scale_fit(shoulder, 0), "blue": _scale_fit(shoulder, 2)},
    }


# ------------------------------------------------------------------ commands

def _load(root: str) -> dict:
    data, err = _manifest.load_manifest(os.path.join(root, "manifest.json"))
    if err or not data:
        raise ValueError(err or f"no manifest at {root}")
    return data


def cmd_render(args) -> int:
    root = os.path.abspath(args.asset_root)
    try:
        frames = roll_frames(_load(root), args.roll)
        check_stems(frames.get("chart", []))
        explicit = args.nc if args.nc is not None else os.environ.get("NC")
        nc, err = _manifest.resolve_nc(explicit, "--nc" if args.nc is not None else "$NC")
        if err or not nc:
            raise ValueError(err or "no hanten binary found (build it, or pass --nc)")
        banner = check_destination_build(nc)
        grid = DEFAULT_GRID
        if args.grid:
            with open(args.grid) as f:
                grid = {**DEFAULT_GRID, **json.load(f)}
        configs = expand_grid(grid)
        work = os.path.abspath(args.work)
        check_work(work)
        recipes = prepare_recipes(nc, root, frames, work,
                                  sorted({c["recipe"] for c in configs.values()} - {None}))
        reset_work(work, args.roll)
        cmds = render_commands(nc, root, frames, work, configs, recipes)
        with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
            stderrs = list(pool.map(lambda c: _run(*c), cmds))
        scale = (_recipe_field(recipes[sorted(recipes)[0]], ("params", "reconstruction", "scale"))
                 if recipes else None)
        write_record(work, {"roll": args.roll, "complete": True, "build": banner,
                            "configurations": list(configs), "density_scale": scale})
    except (OSError, ValueError, KeyError, RuntimeError) as e:
        print(f"error: {e}", file=sys.stderr)
        return 2
    if lines := warning_summary(stderrs):
        print(f"hanten convert stderr over {len(cmds)} renders:",
              file=sys.stderr)
        for line in lines:
            print(f"  {line}", file=sys.stderr)
    print(f"rendered {len(configs)} configurations x {len(frames['chart'])} chart frames "
          f"into {os.path.join(work, 'renders')}")
    return 0


def _sources(work: str, configurations: list[str], extra: list[str]) -> dict[str, str]:
    out = {f"hanten:{c}": os.path.join(work, "renders", c) for c in sorted(configurations)}
    for s in extra:
        name, sep, path = s.partition("=")
        if not sep or not name or not path:
            raise ValueError(f"--source {s!r}: expected NAME=DIR")
        if name in out:
            raise ValueError(f"--source {name!r} repeats a source name")
        out[name] = path
    return out


def read_configs(work: str, roll: str) -> dict:
    """What `render` recorded in `work`: its roll, configurations and density scale."""
    path = os.path.join(work, CONFIGS_NAME)
    if not os.path.exists(path):
        raise ValueError(f"no {path}: run `nctool manifest charts cast render` into this "
                         "work folder first")
    with open(path) as f:
        rec = json.load(f)
    if rec.get("roll") != roll:
        raise ValueError(f"{path} records roll {rec.get('roll')!r}, not {roll!r}")
    if rec.get("complete") is not True:
        raise ValueError(f"{path}: the last `render` into this folder did not finish; "
                         "run it again")
    return rec


def check_table(table_roll: dict, frames: list[dict], roll: str) -> None:
    """Refuse a chart table whose roll no longer matches the manifest: the frames with
    charts, their bytes, their chart entries' checks and the film base frame."""
    from .chart_table import TABLE_NAME
    fix = f"; rebuild it with `nctool manifest charts table`"
    stale = f"{TABLE_NAME} is stale for roll {roll}"
    tframes = table_roll.get("frames", {})
    want = {stem(f): f for f in frames if f.get("charts")}
    if set(want) != set(tframes):
        raise ValueError(f"{stale}: it holds frames {sorted(tframes)}, the manifest's charts "
                         f"are on {sorted(want)}{fix}")
    for name, f in want.items():
        t = tframes[name]
        if (t.get("file"), t.get("sha256")) != (f["file"], f.get("sha256")):
            raise ValueError(f"{stale}: {f['file']} has other bytes or another path{fix}")
        # The table sorts a frame's charts by (target, card); the manifest keeps import order.
        have = {(c.get("target"), c.get("card")): c.get("check") for c in t.get("charts", [])}
        if have != {(c["target"], c.get("card")): c.get("check") for c in f["charts"]}:
            raise ValueError(f"{stale}: {f['file']}'s chart entries changed{fix}")
    base = [f for f in frames if f.get("role") == "unexposed"]
    tb = table_roll.get("film_base") or {}
    if base and (tb.get("frame"), tb.get("sha256")) != (base[0]["file"], base[0].get("sha256")):
        raise ValueError(f"{stale}: its film base frame changed{fix}")


def measure(root: str, roll: str, work: str, extra: list[str]) -> dict:
    """The cast file: per-patch rows, summaries and per-frame residual casts per source."""
    from .chart_table import TABLE_NAME
    from .reference import ReferenceError as _ReferenceError
    frames = roll_frames(_load(root), roll)
    chart = frames.get("chart", [])
    if not chart:
        raise ValueError(f"roll {roll} has no chart frames")
    check_stems(chart)
    rec = read_configs(work, roll)
    out = {"roll": roll, "build": rec.get("build"), "frames": {stem(f): (f.get("shot") or {}).get("exposure_ev")
                                    for f in chart}, "sources": {}}
    for name, d in _sources(work, rec.get("configurations", []), extra).items():
        rows, spaces = [], set()
        for f in chart:
            path = os.path.join(d, stem(f) + ".tif")
            try:
                lin, m, space = read_linear(path)
            except _ReferenceError as e:
                raise ValueError(f"{name}: {path}: {e}") from e
            if lin.shape[:2] != (f.get("height"), f.get("width")):
                raise ValueError(
                    f"{name}: {path} is {lin.shape[1]}x{lin.shape[0]}, the scan "
                    f"{f.get('width')}x{f.get('height')}: the chart corners are fractions of "
                    "the scan, so export at its size, uncropped and unrotated")
            spaces.add(space)
            rows += patch_rows(lin, m, stem(f), f.get("charts", []))
        out["sources"][name] = {"space": sorted(spaces), "summary": summarize(rows),
                                "per_frame": per_frame(rows), "patches": rows}
    table = os.path.join(root, TABLE_NAME)
    scale = rec.get("density_scale")
    if os.path.exists(table) and scale:
        with open(table) as t:
            troll = json.load(t).get("rolls", {}).get(roll, {})
        check_table(troll, [f for fs in frames.values() for f in fs], roll)
        out["density"] = density_residuals(troll.get("frames", {}), scale)
    return out


def _num(x, fmt: str, width: int) -> str:
    return f"{'-':>{width}}" if x is None else format(x, fmt)


def print_table(cast: dict, file=sys.stdout) -> None:
    if cast.get("build"):
        lines = cast["build"].splitlines()
        keep = [lines[0]] + [l.split(" (")[0] for l in lines[1:]
                             if l.startswith(("pipeline_version:", "commit:"))]
        print("build: " + "; ".join(keep), file=file)
    print(f"{'source':38} {'C*':>5} {'a*':>6} {'b*':>6} | {'dark b*':>6} {'mid b*':>6} "
          f"{'lt b*':>6} | {'dark a*':>6} {'mid a*':>6} {'lt a*':>6} | {'|dh|':>5} {'Cr':>5} "
          f"{'dL':>5}", file=file)
    for name, src in cast["sources"].items():
        s = src["summary"]
        al = s.get("all", {})
        band = lambda b, k: _num(s.get(b, {}).get(k), "+6.1f", 6)
        m = s.get("matched", {})
        dl = m["l"] - m["ref_l"] if m else None
        print(f"{name:38} {_num(al.get('c'), '5.1f', 5)} {band('all', 'a')} "
              f"{band('all', 'b')} | {band('dark', 'b')} {band('mid', 'b')} "
              f"{band('light', 'b')} | {band('dark', 'a')} {band('mid', 'a')} "
              f"{band('light', 'a')} | {_num(m.get('hue_error'), '5.1f', 5)} "
              f"{_num(m.get('chroma_ratio'), '5.2f', 5)} {_num(dl, '+5.1f', 5)}", file=file)
    print("|dh|, Cr: CIELAB hue error and chroma ratio on the matched card, whose D4 grey "
          "patch renders dL from the reference's L*", file=file)
    d = cast.get("density")
    if not d:
        print("density: not read (no charts.json, or no roll measurement in the grid)",
              file=file)
        return
    o, sl, sh = d["offset"], d["scale_fit"], d["shoulder"]
    lo, hi = o["range"]
    print(f"density after scale {d['scale']}, green {lo}-{hi} above base (n {o['n']}): "
          f"offset to cancel red {_num(o['red'], '+.4f', 7)}, blue {_num(o['blue'], '+.4f', 7)}; "
          f"density scale against green red {_num(sl['red'], '.3f', 5)}, "
          f"blue {_num(sl['blue'], '.3f', 5)}", file=file)
    span = f", green {sh['green'][0]:.2f}-{sh['green'][1]:.2f}" if sh.get("green") else ""
    print(f"shoulder, green {sh['from']} and above (n {sh['n']}{span}): density scale against "
          f"green red {_num(sh['red'], '.3f', 5)}, blue {_num(sh['blue'], '.3f', 5)}", file=file)


def cmd_measure(args) -> int:
    from .metrics import MetricsError, require_dependencies
    from .reference import ReferenceError as _ReferenceError
    root = os.path.abspath(args.asset_root)
    try:
        require_dependencies("charts cast measure")
        cast = measure(root, args.roll, os.path.abspath(args.work), args.source or [])
        out = args.out or os.path.join(args.work, "cast.json")
        with open(out, "w") as f:
            json.dump(cast, f)
    except (OSError, ValueError, KeyError, MetricsError, _ReferenceError) as e:
        print(f"error: {e}", file=sys.stderr)
        return 2
    print_table(cast)
    print(f"wrote {out}")
    return 0

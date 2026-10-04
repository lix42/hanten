"""The file set and pre-checks for `analysis/viewer-interoperability`.

`viewer set` renders every destination the benchmark's fixtures set names (one per
ready `destination::ROWS` row, plus the film master) from each input of
`viewer.json`, into a directory outside the repository, and writes beside them
`viewer-set.json` (what each file is and what a viewer should show) and `rubric.md`
(the manual checklist, one table per reader and display setting). A roll input renders under the recipe
`hanten measure-roll` writes for its roll, as a user would, measured on every run.

`viewer check <dir>` runs the decoders that need no person on every gain-map JPEG of
a set: Apple ImageIO (`scripts/iso-decoder-oracle/`) and libultrahdr's
`ultrahdr_app`, the reference decoder behind Android's. Each must find the gain map,
read three channel gains equal to the ones nc's report states, and decode the file
at its own size. It checks parsing, not whether a viewer shows the HDR rendition;
that is the rubric.

The procedure and the rubric live in `scripts/viewer-interop/README.md`.

Exit codes: `0` done (`check`: every check passed); `1` a conversion or a check
failed; `2` usage or an operational failure.
"""

from __future__ import annotations

import json
import math
import os
import re
import shutil
import struct
import subprocess
import sys
import tempfile

from . import acceptance as _acceptance
from . import compare as _compare
from . import manifest as _manifest

CONFIG = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
                      "viewer.json")
CONFIG_SCHEMA = 1
SET_SCHEMA = 1
CHECK_SCHEMA = 1
SET_FILE = "viewer-set.json"
RUBRIC_FILE = "rubric.md"
CHECK_FILE = "checks.json"
# The decoders print gains to six figures; this is well above that and far below
# any difference between channels.
GAIN_TOLERANCE = 1e-4
# A decoder that has not answered in this long is hung, and the file fails.
DECODER_TIMEOUT_S = 300
RUBRIC_ITEMS = ("opens", "rendition", "fallback", "geometry", "no gross fault")


class ViewerError(Exception):
    """An operational failure: no verdict (exit 2)."""


class Failed(Exception):
    """A conversion failed (exit 1)."""


# ---------------------------------------------------------------------------
# Configuration and cases
# ---------------------------------------------------------------------------

def load_config(path: str) -> dict:
    cfg, err = _compare.load_json(path)
    if err:
        raise ViewerError(err)
    if cfg.get("schema_version") != CONFIG_SCHEMA:
        raise ViewerError(f"{path}: schema_version {cfg.get('schema_version')!r} is not "
                          f"{CONFIG_SCHEMA}")
    expect = cfg.get("expect", {})
    missing = sorted(set(_acceptance.ENCODINGS) - set(expect))
    if missing:
        raise ViewerError(f"{path}: no `expect` entry for {', '.join(missing)}")
    readers = cfg.get("readers", {})
    for rid, r in readers.items():
        settings = r.get("settings") if isinstance(r, dict) else None
        if not isinstance(settings, dict) or not settings or not r.get("label") or \
                not all(isinstance(v, bool) for v in settings.values()):
            raise ViewerError(f"{path}: readers.{rid} needs a `label` and `settings` "
                              "(each display setting: whether HDR is on)")
    for enc, e in expect.items():
        if not (isinstance(e, dict) and isinstance(e.get("hdr"), bool)
                and isinstance(e.get("fallback_gated"), bool)
                and isinstance(e.get("rendition"), str) and isinstance(e.get("fallback"), str)
                and isinstance(e.get("readers"), list)):
            raise ViewerError(f"{path}: expect.{enc} needs `hdr`, `rendition`, `fallback`, "
                              "`fallback_gated` and `readers`")
        unknown = sorted(set(e.get("readers", [])) - set(readers))
        if unknown:
            raise ViewerError(f"{path}: expect.{enc} names unknown reader(s) "
                              f"{', '.join(unknown)}")
    for name, spec in cfg.get("inputs", {}).items():
        if not re.fullmatch(r"[a-z0-9][a-z0-9-]*", name):
            raise ViewerError(f"{path}: input name {name!r} is not filename-safe")
        kinds = [k for k in ("acceptance_input", "roll") if k in spec]
        if len(kinds) != 1 or ("roll" in spec) != ("frame" in spec):
            raise ViewerError(f"{path}: input {name!r} needs exactly one of "
                              "`acceptance_input` or `roll` + `frame`")
    d = cfg.get("destinations")
    if not isinstance(d, dict) or not all(k in d for k in ("benchmark_set", "input",
                                                           "prefix", "flags")):
        raise ViewerError(f"{path}: `destinations` needs `benchmark_set`, `input`, "
                          "`prefix` and `flags`")
    return cfg


def destinations(cfg: dict, benchmark_path: str) -> list[dict]:
    """`[{id, args}]`: the benchmark cases of one input that state a destination."""
    d = cfg["destinations"]
    bench, err = _compare.load_json(benchmark_path)
    if err:
        raise ViewerError(err)
    cases, err = _compare.resolve_cases(bench, d["benchmark_set"], asset_root="")
    if err:
        raise ViewerError(err)
    source = os.path.join(_compare.repo_root(), d["input"])
    out = []
    for c in cases:
        block = c["blocks"].get("destination")
        if c["input"] != source or block is None:
            continue
        if not any(flag in block["args"] for flag in d["flags"]):
            continue
        if block.get("recipe"):
            raise ViewerError(f"case {c['name']!r}'s destination block names a recipe; "
                              "viewer set renders only its args")
        if not c["name"].startswith(d["prefix"]):
            raise ViewerError(f"case {c['name']!r} does not start with {d['prefix']!r}")
        out.append(dict(id=c["name"][len(d["prefix"]):], args=list(block["args"])))
    if not out:
        raise ViewerError(f"no {d['benchmark_set']!r} case of {d['input']} states a "
                          "destination")
    return out


def _roll_frames(assets: dict, roll: str) -> list[dict]:
    rolls = assets.get("rolls", {})
    if roll not in rolls:
        raise ViewerError(f"roll {roll!r} is not in the asset manifest")
    return rolls[roll].get("frames", [])


def _stem(frame: dict) -> str:
    return os.path.splitext(os.path.basename(frame["file"]))[0]


def roll_recipe(nc: str, assets: dict, asset_root: str, roll: str, out_dir: str) -> str:
    """The roll's `measure-roll` recipe, written to `<out>/recipes/`. Measured on every
    run, so a set never renders under another build's measurement."""
    path = os.path.join(out_dir, "recipes", f"{roll}.json")
    frames = _roll_frames(assets, roll)
    by_role = lambda role: [os.path.join(asset_root, f["file"]) for f in frames
                            if f.get("role") == role]
    real, unexposed, leader = by_role("real"), by_role("unexposed"), by_role("leader")
    if not real or len(unexposed) != 1 or len(leader) != 1:
        raise ViewerError(f"roll {roll!r} needs real frames, one unexposed frame and one "
                          "leader in the asset manifest")
    os.makedirs(os.path.dirname(path), exist_ok=True)
    proc = subprocess.run([nc, "measure-roll", *real, "--unexposed", unexposed[0],
                           "--leader", leader[0], "--out", path, "--force"],
                          capture_output=True, text=True)
    if proc.returncode != 0:
        raise Failed(f"measure-roll {roll}: hanten exited {proc.returncode}\n"
                     f"{proc.stderr.strip()}")
    return path


def input_argv(cfg: dict, name: str, nc: str, asset_root: str, assets: dict | None,
               out_dir: str) -> tuple[str, list[str]]:
    """`(input path, args)` for one input; destination args are added per case."""
    spec = cfg["inputs"][name]
    if "acceptance_input" in spec:
        acc = _acceptance.load_manifest(_acceptance.MANIFEST)
        src = acc.get("inputs", {}).get(spec["acceptance_input"])
        if src is None:
            raise ViewerError(f"acceptance.json has no input {spec['acceptance_input']!r}")
        return os.path.join(_compare.repo_root(), src["input"]), list(src["args"])
    if assets is None:
        raise ViewerError(f"input {name!r} needs the asset manifest "
                          f"({os.path.join(asset_root, 'manifest.json')})")
    frames = [f for f in _roll_frames(assets, spec["roll"]) if _stem(f) == spec["frame"]]
    if len(frames) != 1:
        raise ViewerError(f"roll {spec['roll']!r} has no single frame {spec['frame']!r}")
    recipe = roll_recipe(nc, assets, asset_root, spec["roll"], out_dir)
    return os.path.join(asset_root, frames[0]["file"]), ["--params", recipe]


# ---------------------------------------------------------------------------
# Image facts (stdlib only)
# ---------------------------------------------------------------------------

def jpeg_size(data: bytes) -> tuple[int, int]:
    """`(width, height)` from the first frame header of a JPEG's primary image."""
    if data[:2] != b"\xff\xd8":
        raise ValueError("not a JPEG")
    i = 2
    while i + 4 <= len(data):
        if data[i] != 0xFF:
            raise ValueError(f"no marker at offset {i}")
        marker = data[i + 1]
        if marker in (0xD8, 0x01) or 0xD0 <= marker <= 0xD7:
            i += 2
            continue
        length = struct.unpack(">H", data[i + 2:i + 4])[0]
        if marker in (0xC0, 0xC1, 0xC2):
            h, w = struct.unpack(">HH", data[i + 5:i + 9])
            return w, h
        if marker == 0xDA:
            break
        i += 2 + length
    raise ValueError("no frame header before the scan")


def tiff_size(f) -> tuple[int, int]:
    """`(width, height)` from the first IFD of a TIFF open for reading."""
    def read(at: int, n: int) -> bytes:
        f.seek(at)
        data = f.read(n)
        if len(data) != n:
            raise ValueError("truncated TIFF")
        return data
    order = {b"II": "<", b"MM": ">"}.get(read(0, 2))
    if order is None or struct.unpack(order + "H", read(2, 2))[0] != 42:
        raise ValueError("not a classic TIFF")
    ifd = struct.unpack(order + "I", read(4, 4))[0]
    count = struct.unpack(order + "H", read(ifd, 2))[0]
    entries = read(ifd + 2, 12 * count)
    found = {}
    for k in range(count):
        e = entries[12 * k:12 * k + 12]
        tag, typ = struct.unpack(order + "HH", e[:4])
        if tag in (256, 257):
            fmt = {3: "H", 4: "I"}.get(typ)
            if fmt is None:
                raise ValueError(f"tag {tag} has type {typ}")
            found[tag] = struct.unpack(order + fmt, e[8:8 + struct.calcsize(fmt)])[0]
    if set(found) != {256, 257}:
        raise ValueError("no ImageWidth/ImageLength")
    return found[256], found[257]


def image_size(path: str) -> tuple[int, int]:
    """A JPEG's or TIFF's own dimensions; `ValueError` if it is neither, or malformed."""
    with open(path, "rb") as f:
        head = f.read(2)
        if head == b"\xff\xd8":
            f.seek(0)
            return jpeg_size(f.read(1 << 20))
        return tiff_size(f)


# ---------------------------------------------------------------------------
# viewer set
# ---------------------------------------------------------------------------

def _outside_repo(out: str) -> str:
    out = os.path.realpath(out)
    repo = os.path.realpath(_compare.repo_root())
    if out == repo or out.startswith(repo + os.sep):
        raise ViewerError(f"{out} is inside the repository ({repo}); the set holds "
                          "rendered photographs and must go to a directory outside it")
    return out


def render(nc: str, path: str, args: list[str], dest: dict, out_base: str) -> dict:
    argv = [nc, "convert", path, "-o", out_base, *args, *dest["args"]]
    proc = subprocess.run(argv, capture_output=True, text=True)
    if proc.returncode != 0:
        raise Failed(f"{os.path.basename(out_base)}: hanten exited {proc.returncode}\n"
                     f"{proc.stderr.strip()}")
    try:
        return json.loads(proc.stdout)
    except json.JSONDecodeError as e:
        raise Failed(f"{os.path.basename(out_base)}: hanten's stdout is not JSON ({e})")


def entry(cfg: dict, name: str, dest: dict, report: dict, out_dir: str) -> dict:
    encoding, gamut, _ = _acceptance.encoding_of(report)
    output = report["output"]
    try:
        width, height = image_size(output)
    except (ValueError, struct.error) as e:
        raise Failed(f"{output}: {e}")
    gm = (report.get("chain") or {}).get("gain_map")
    return dict(file=os.path.relpath(output, out_dir), input=name, destination=dest["id"],
                args=dest["args"], encoding=encoding, gamut=gamut,
                width=width, height=height, bytes=os.path.getsize(output),
                sha256=_manifest.sha256(output),
                gain_map_max=gm["max"] if gm else None,
                expect=cfg["expect"][encoding])


def expectation(e: dict, hdr_on: bool) -> tuple[str, str]:
    """`(item 2, item 3)` for one encoding under one display setting. With HDR on there
    is no fallback to see; with it off an HDR rendition cannot show, so item 3 is the
    check."""
    if hdr_on:
        return e["rendition"], "n/a (HDR on)"
    fallback = e["fallback"] + (" **(gated)**" if e["fallback_gated"] else "")
    if e["hdr"]:
        return "n/a (HDR off): see fallback", fallback
    return e["rendition"], fallback


def rubric(cfg: dict, record: dict) -> str:
    ident = record["identity"]
    lines = [
        "# Viewer rubric",
        "",
        f"Set built by hanten {ident.get('nc_version')} "
        f"({ident.get('git_commit')}{', dirty' if ident.get('git_dirty') else ''}), "
        f"pipeline_version {ident.get('pipeline_version')}.",
        "",
        "Per file, answer each item yes / no / n/a, with a note on anything other than "
        "yes. Items: 1 opens without repair or error; 2 shows the intended rendition; "
        "3 falls back as expected with HDR off (or on an SDR-only reader); 4 "
        "orientation, dimensions and crop are right; 5 no channel swap, inversion, "
        "all-black or all-white render, or edge artifact. Item 3 passes or fails only "
        "where its column says **(gated)**; elsewhere it records what is shown.",
        "",
    ]
    for rid, reader in cfg["readers"].items():
        files = [f for f in record["files"] if rid in f["expect"]["readers"]]
        if not files:
            continue
        for setting, hdr_on in reader["settings"].items():
            lines += [f"## {reader['label']} — {setting}", "",
                      "Viewer version: · OS version: · Display: ", "",
                      "| file | size | rendition (2) | fallback (3) | 1 | 2 | 3 | 4 | 5 | notes |",
                      "|---|---|---|---|---|---|---|---|---|---|"]
            for f in files:
                rendition, fallback = expectation(f["expect"], hdr_on)
                lines.append(f"| `{f['file']}` | {f['width']}×{f['height']} | {rendition} "
                             f"| {fallback} |  |  |  |  |  |  |")
            lines.append("")
    return "\n".join(lines)


def build_set(nc: str, cfg: dict, dests: list[dict], names: list[str], asset_root: str,
              out_dir: str) -> dict:
    assets = None
    if any("roll" in cfg["inputs"][n] for n in names):
        assets, err = _compare.load_json(os.path.join(asset_root, "manifest.json"))
        if err:
            raise ViewerError(err)
    files, identity = [], None
    for name in names:
        path, args = input_argv(cfg, name, nc, asset_root, assets, out_dir)
        os.makedirs(os.path.join(out_dir, name), exist_ok=True)
        for dest in dests:
            report = render(nc, path, args, dest,
                            os.path.join(out_dir, name, f"{name}-{dest['id']}"))
            # The build, not the run: `params_hash` differs per file.
            ident = {k: v for k, v in (report.get("identity") or {}).items()
                     if k != "params_hash"}
            if identity is not None and ident != identity:
                raise ViewerError("the hanten binary changed during the run")
            identity = ident
            files.append(entry(cfg, name, dest, report, out_dir))
    return dict(schema_version=SET_SCHEMA, identity=identity, files=files)


def _write(path: str, text: str) -> None:
    fd, tmp = tempfile.mkstemp(dir=os.path.dirname(path), prefix=".viewer.")
    with os.fdopen(fd, "w") as f:
        f.write(text)
    os.replace(tmp, path)


def cmd_set(args) -> int:
    try:
        out_dir = _outside_repo(args.out)
        # A set's rubric.md may hold the answers by now: never overwrite it unasked.
        if os.path.exists(os.path.join(out_dir, SET_FILE)) and not args.force:
            raise ViewerError(f"{out_dir} already holds a set, and its {RUBRIC_FILE} may "
                              "hold answers; copy it elsewhere and pass --force to rebuild")
        cfg = load_config(args.config)
        # Nothing an earlier set wrote may outlive it, even if this run fails: not its
        # records, and not the files of an input this run does not render.
        for name in (SET_FILE, RUBRIC_FILE, CHECK_FILE):
            if os.path.exists(os.path.join(out_dir, name)):
                os.remove(os.path.join(out_dir, name))
        for name in [*cfg["inputs"], "recipes"]:
            shutil.rmtree(os.path.join(out_dir, name), ignore_errors=True)
        names = list(dict.fromkeys(args.input or cfg["inputs"]))
        unknown = sorted(set(names) - set(cfg["inputs"]))
        if unknown:
            raise ViewerError(f"no such input: {', '.join(unknown)}")
        nc, err = _manifest.resolve_nc(args.nc, "--nc")
        if err or nc is None:
            raise ViewerError(err or "no hanten binary; pass --nc")
        dests = destinations(cfg, args.benchmark)
        os.makedirs(out_dir, exist_ok=True)
        record = build_set(nc, cfg, dests, names, args.asset_root, out_dir)
        _write(os.path.join(out_dir, SET_FILE), json.dumps(record, indent=2) + "\n")
        _write(os.path.join(out_dir, RUBRIC_FILE), rubric(cfg, record) + "\n")
    except (ViewerError, _acceptance.AcceptanceError, OSError, ValueError) as e:
        print(f"viewer set: {e}", file=sys.stderr)
        return 2
    except Failed as e:
        print(f"viewer set: {e}", file=sys.stderr)
        return 1
    print(f"{len(record['files'])} files in {out_dir}; checklist: "
          f"{os.path.join(out_dir, RUBRIC_FILE)}")
    return 0


# ---------------------------------------------------------------------------
# viewer check
# ---------------------------------------------------------------------------

def _gains_match(found: list[float], stated: list[float]) -> bool:
    return len(found) == 3 and len(stated) == 3 and all(
        math.isfinite(a) and abs(a - b) <= GAIN_TOLERANCE * max(1.0, abs(b))
        for a, b in zip(found, stated))


def parse_apple(text: str) -> dict:
    """The facts the ImageIO oracle prints about one file."""
    present = re.search(r"ISO 21496-1 gain map \([^)]*\): (PRESENT|ABSENT)", text)
    hdr = re.search(r"HDR decode: (\d+)x(\d+)", text)
    return dict(iso=present.group(1) if present else None,
                gain_map_max_log2=[float(v) for v in re.findall(r"GainMapMax = (-?[\d.]+)", text)],
                hdr_size=[int(hdr.group(1)), int(hdr.group(2))] if hdr else None)


def _run(argv: list[str]) -> tuple[int | None, str]:
    """`(exit status, stdout + stderr)`; the status is `None` when the decoder hung."""
    try:
        proc = subprocess.run(argv, capture_output=True, text=True,
                              timeout=DECODER_TIMEOUT_S)
    except subprocess.TimeoutExpired:
        return None, ""
    return proc.returncode, proc.stdout + proc.stderr


def check_apple(oracle: str, path: str, f: dict) -> dict:
    rc, text = _run([oracle, path])
    facts = parse_apple(text)
    gains = [2.0 ** v for v in facts["gain_map_max_log2"]]
    reasons = []
    if rc != 0:
        reasons.append("timed out" if rc is None else f"exited {rc}")
    if facts["iso"] != "PRESENT":
        reasons.append(f"ISO gain map {facts['iso'] or 'not reported'}")
    if not _gains_match(gains, f["gain_map_max"]):
        reasons.append(f"channel gains {gains} are not the report's {f['gain_map_max']}")
    if facts["hdr_size"] != [f["width"], f["height"]]:
        reasons.append(f"HDR decode size {facts['hdr_size']}, file is "
                       f"{f['width']}x{f['height']}")
    return dict(decoder="apple-imageio", ok=not reasons, reasons=reasons,
                gains=gains)


def parse_ultrahdr_probe(text: str) -> dict:
    boost = re.search(r"--maxContentBoost ([^\n]+)", text)
    return dict(ultra_hdr=bool(re.search(r"Ultra HDR Image: Yes", text)),
                max_content_boost=[float(v) for v in
                                   re.findall(r"-?\d+(?:\.\d+)?(?:e-?\d+)?", boost.group(1))]
                if boost else [])


def check_ultrahdr(app: str, path: str, f: dict, scratch: str) -> dict:
    reasons = []
    rc, text = _run([app, "-m", "1", "-j", path, "-P"])
    facts = parse_ultrahdr_probe(text)
    if rc != 0 or not facts["ultra_hdr"]:
        reasons.append(f"probe did not find a gain map (exit {rc})")
    if not _gains_match(facts["max_content_boost"], f["gain_map_max"]):
        reasons.append(f"channel gains {facts['max_content_boost']} are not the report's "
                       f"{f['gain_map_max']}")
    raw = os.path.join(scratch, "decoded.raw")
    # Linear half-float RGBA: 8 bytes a pixel.
    rc, _ = _run([app, "-m", "1", "-j", path, "-o", "0", "-O", "4", "-z", raw])
    size = os.path.getsize(raw) if os.path.exists(raw) else None
    if rc != 0:
        reasons.append(f"decode exited {rc}")
    elif size != f["width"] * f["height"] * 8:
        reasons.append(f"decoded {size} bytes, expected {f['width']}x{f['height']}x8")
    if os.path.exists(raw):
        os.remove(raw)
    return dict(decoder="libultrahdr", ok=not reasons, reasons=reasons,
                gains=facts["max_content_boost"])


def tool_version(argv: list[str], pattern: str) -> str | None:
    try:
        _, text = _run(argv)
    except OSError:
        return None
    m = re.search(pattern, text)
    return m.group(1) if m else None


def cmd_check(args) -> int:
    set_dir = os.path.realpath(args.set)
    # An earlier verdict must not survive a run that writes none.
    if os.path.exists(os.path.join(set_dir, CHECK_FILE)):
        os.remove(os.path.join(set_dir, CHECK_FILE))
    record, err = _compare.load_json(os.path.join(set_dir, SET_FILE))
    if err:
        print(f"viewer check: {err}", file=sys.stderr)
        return 2
    if record.get("schema_version") != SET_SCHEMA:
        print(f"viewer check: {SET_FILE} schema_version is not {SET_SCHEMA}", file=sys.stderr)
        return 2
    oracle = args.oracle
    app = args.ultrahdr or shutil.which("ultrahdr_app")
    if not oracle or not os.access(oracle, os.X_OK):
        print("viewer check: build the ImageIO oracle and pass --oracle "
              "(scripts/iso-decoder-oracle/README.md)", file=sys.stderr)
        return 2
    if not app or not os.access(app, os.X_OK):
        print("viewer check: no ultrahdr_app; `brew install libultrahdr` or pass --ultrahdr",
              file=sys.stderr)
        return 2
    files = [f for f in record.get("files") or [] if isinstance(f, dict)
             and f.get("encoding") == "gain-map-jpeg"]
    if not files:
        print("viewer check: the set has no gain-map JPEG", file=sys.stderr)
        return 2
    for f in files:
        gains = f.get("gain_map_max")
        if not (isinstance(f.get("file"), str) and isinstance(f.get("sha256"), str)
                and isinstance(f.get("width"), int) and isinstance(f.get("height"), int)
                and isinstance(gains, list) and len(gains) == 3):
            print(f"viewer check: {SET_FILE} has a malformed gain-map entry: {f}",
                  file=sys.stderr)
            return 2
    results = []
    with tempfile.TemporaryDirectory() as scratch:
        for f in files:
            path = os.path.join(set_dir, f["file"])
            if not os.path.isfile(path):
                results.append(dict(file=f["file"], ok=False, checks=[],
                                    reasons=["missing from the set"]))
                continue
            if _manifest.sha256(path) != f["sha256"]:
                results.append(dict(file=f["file"], ok=False,
                                    checks=[], reasons=["file changed since the set was built"]))
                continue
            checks = [check_apple(oracle, path, f), check_ultrahdr(app, path, f, scratch)]
            results.append(dict(file=f["file"], ok=all(c["ok"] for c in checks), checks=checks))
    out = dict(schema_version=CHECK_SCHEMA, set_identity=record.get("identity"),
               decoders=dict(libultrahdr=tool_version([app], r"lib version: (v[\d.]+)"),
                             macos=tool_version(["sw_vers", "-productVersion"], r"([\d.]+)")),
               ok=all(r["ok"] for r in results), results=results)
    _write(os.path.join(set_dir, CHECK_FILE), json.dumps(out, indent=2) + "\n")
    for r in results:
        why = "; ".join(x for c in r.get("checks", []) for x in c["reasons"]) or \
            "; ".join(r.get("reasons", []))
        print(f"{'PASS' if r['ok'] else 'FAIL'} {r['file']}" + (f": {why}" if why else ""))
    return 0 if out["ok"] else 1

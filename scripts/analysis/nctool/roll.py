"""Manifest-driven roll calibration, conversion, and analysis.

This module turns the manual workflow in ``docs/using-nc.md`` into one command:
measure Dmin from the manifest's unexposed frame, freeze it into a partial recipe,
and run ``hanten roll`` over every real frame.
The durable ``tags.json`` and ``roll-report.json`` can be normalized into a
deterministic ``analysis.json`` artifact. Ordinary diff tools can then compare
configurations without opening their image pixels again.
"""
from __future__ import annotations

import datetime as _datetime
import hashlib
import json
import math
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

from . import compare as _compare
from . import manifest as _manifest

TAG_SCHEMA = 1
ANALYSIS_SCHEMA = 1
CONFIG_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]*$")


def _load_object(path: Path) -> tuple[dict | None, str | None]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        return None, f"cannot read {path}: {error}"
    if not isinstance(value, dict):
        return None, f"{path}: expected a JSON object"
    return value, None


def _write_json(path: Path, value: dict) -> None:
    """Atomically write a JSON artifact beside the conversion outputs."""
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=path.parent, prefix=f".{path.name}.", suffix=".tmp")
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as stream:
            json.dump(value, stream, indent=2, sort_keys=True)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(tmp, path)
    except Exception:
        try:
            os.remove(tmp)
        except OSError:
            pass
        raise


def _asset_manifest(asset_root: Path) -> tuple[dict | None, str | None]:
    path = asset_root / "manifest.json"
    data, error = _manifest.load_manifest(str(path))
    if error:
        return None, error
    if not data:
        return None, f"no manifest.json at {asset_root}; run `nctool manifest generate` first"
    return data, None


def _roll_frames(data: dict, roll: str) -> tuple[dict | None, str | None]:
    spec = data.get("rolls", {}).get(roll)
    if not isinstance(spec, dict):
        available = ", ".join(sorted(data.get("rolls", {}))) or "(none)"
        return None, f"unknown roll {roll!r}; available: {available}"
    frames = spec.get("frames")
    if not isinstance(frames, list):
        return None, f"roll {roll!r} has no frames list"
    by_role: dict[str, list[dict]] = {"unexposed": [], "leader": [], "real": []}
    for frame in frames:
        if not isinstance(frame, dict) or not isinstance(frame.get("file"), str):
            return None, f"roll {roll!r} contains a malformed frame entry"
        role = frame.get("role", "real")
        if role not in by_role:
            return None, f"roll {roll!r} frame {frame['file']} has unsupported role {role!r}"
        by_role[role].append(frame)
    if len(by_role["unexposed"]) != 1:
        return None, (f"roll {roll!r} needs exactly one unexposed frame; "
                      f"found {len(by_role['unexposed'])}")
    if not by_role["real"]:
        return None, f"roll {roll!r} has no real frames to convert"
    return by_role, None


def _region(raw: str | None, frame: dict, label: str,
            fraction: float = .8) -> tuple[str | None, str | None]:
    if raw is None:
        width, height = frame.get("width"), frame.get("height")
        if not isinstance(width, int) or not isinstance(height, int) or width <= 0 or height <= 0:
            return None, f"{label} frame lacks valid manifest dimensions"
        margin = (1 - fraction) / 2
        return (f"{round(margin * width)},{round(margin * height)},"
                f"{round(fraction * width)},{round(fraction * height)}"), None
    try:
        values = [int(item) for item in raw.split(",")]
    except ValueError:
        values = []
    if len(values) != 4 or any(value < 0 for value in values[:2]) or any(value <= 0 for value in values[2:]):
        return None, f"invalid {label} region {raw!r}; expected X,Y,W,H with positive W/H"
    return ",".join(map(str, values)), None


def _strip_jsonc(text: str) -> str:
    """`text` with its `//` and `/* */` comments outside strings blanked: JSONC, which
    `hanten profile` writes, read as JSON. An unclosed `/*` raises `ValueError`, as
    `--params` refuses it."""
    out, i, in_string = [], 0, False
    while i < len(text):
        c = text[i]
        if in_string:
            out.append(c)
            if c == "\\" and i + 1 < len(text):
                out.append(text[i + 1])
                i += 1
            elif c == '"':
                in_string = False
        elif c == '"':
            in_string = True
            out.append(c)
        elif text.startswith("//", i):
            end = text.find("\n", i)
            i = len(text) if end == -1 else end
            continue
        elif text.startswith("/*", i):
            end = text.find("*/", i + 2)
            if end == -1:
                line = text.count("\n", 0, i) + 1
                raise ValueError(f"the comment opened on line {line} is never closed")
            # Line breaks stay, so a parse error's line still points into the text.
            out.append(" " + "\n" * text.count("\n", i, end))
            i = end + 2
            continue
        else:
            out.append(c)
        i += 1
    return "".join(out)


def _run_json(argv: list[str], label: str, jsonc: bool = False
              ) -> tuple[dict | None, str | None]:
    try:
        proc = subprocess.run(argv, capture_output=True, text=True, check=False)
    except OSError as error:
        return None, f"{label} could not start {argv[0]!r}: {error}"
    if proc.returncode != 0:
        detail = proc.stderr.strip() or proc.stdout.strip() or "no diagnostic"
        return None, f"{label} failed (exit {proc.returncode}): {detail}"
    try:
        value = json.loads(_strip_jsonc(proc.stdout) if jsonc else proc.stdout)
    except ValueError as error:  # json.JSONDecodeError, or an unclosed comment
        return None, f"{label} emitted invalid JSON: {error}"
    if not isinstance(value, dict):
        return None, f"{label} emitted a non-object JSON report"
    return value, None


def _defaults_command(nc: str) -> str:
    """The subcommand that prints the build's complete default recipe: ``profile``
    (JSONC, without the measured ``calibration`` and ``roll``), or ``params`` on a build
    from before the rename (the reference build). Asked of the binary, like
    `_base_command`: ``params`` exits 2 on a renamed build."""
    try:
        proc = subprocess.run([nc, "profile", "--help"], capture_output=True,
                              text=True, check=False)
    except OSError:
        return "profile"  # the default recipe query reports the start failure itself
    return "profile" if proc.returncode == 0 else "params"


def _base_command(nc: str) -> str:
    """The subcommand that measures the film base: ``measure-base``, or ``estimate``
    on a build from before the rename (the reference build). Read off the binary by
    asking it, never off its id: ``estimate --help`` exits 2 on a renamed build."""
    try:
        proc = subprocess.run([nc, "measure-base", "--help"], capture_output=True,
                              text=True, check=False)
    except OSError:
        return "measure-base"  # the Dmin step reports the start failure itself
    return "measure-base" if proc.returncode == 0 else "estimate"


def _recipe_input(path: str | None) -> tuple[dict | None, dict | None, str | None]:
    """The `--recipe` file's recipe, and its envelope's `meta` (None when bare)."""
    if path is None:
        return {}, None, None
    value, error = _load_object(Path(path))
    if error:
        return None, None, error
    assert value is not None
    meta = value.get("meta") if set(value) == {"meta", "params"} else None
    value, error = _unwrap_envelope(value, str(path))
    if error:
        return None, None, error
    return json.loads(json.dumps(value)), meta, None


def _unwrap_envelope(value: dict, label: str) -> tuple[dict | None, str | None]:
    """The recipe in a `{meta, params}` envelope — a preset build's sidecar, or any
    recipe document a build from `core/recipe-replay-fidelity` on writes — or `value`
    itself when it is a bare recipe (the builds between)."""
    if set(value) != {"meta", "params"}:
        return value, None
    # As `hanten` refuses it, so a malformed one fails before the Dmin measurement.
    if not isinstance(value.get("meta"), dict):
        return None, f"{label}: envelope `meta` must be an object"
    params = value.get("params")
    if not isinstance(params, dict):
        return None, f"{label}: envelope `params` must be an object"
    return params, None


def _deep_merge(base: dict, overlay: dict) -> dict:
    """Recipe merge: objects recurse; every other overlay value replaces.

    A differing `type` replaces the whole object, so when nc stops serializing a
    tag (as it did `reconstruction.type`), normalize old recipes' spelling of it
    before merging, or an old recipe reads as a variant switch."""
    result = json.loads(json.dumps(base))
    for key, value in overlay.items():
        if isinstance(value, dict) and isinstance(result.get(key), dict):
            # Tagged recipe objects have disjoint key sets. Switching exponential to
            # characteristic while retaining the exponential's gamma/anchor would
            # create a recipe nc correctly rejects as mixed-curve input.
            old_type, new_type = result[key].get("type"), value.get("type")
            result[key] = (json.loads(json.dumps(value))
                           if new_type is not None and old_type != new_type
                           else _deep_merge(result[key], value))
        else:
            result[key] = json.loads(json.dumps(value))
    return result


def _drop_retired_curve_tag(defaults: dict, partial: dict) -> None:
    """Drop a partial recipe's `reconstruction.curve.type = "exponential"` when the
    build's own defaults carry no curve tag.

    the default recipe stopped writing the tag when the curve became the one exponential
    (`nf-retire/characteristic`), so the old spelling would read as a variant switch in
    `_deep_merge` and replace the whole default curve. The reference build still writes
    it, and there a stated tag is a real selector, so it is kept."""
    # A malformed section is left for `_freeze_recipe` to refuse with its own message.
    default_rec, rec = defaults.get("reconstruction"), partial.get("reconstruction")
    default_curve = default_rec.get("curve") if isinstance(default_rec, dict) else None
    curve = rec.get("curve") if isinstance(rec, dict) else None
    if (isinstance(default_curve, dict) and "type" not in default_curve
            and isinstance(curve, dict) and curve.get("type") == "exponential"):
        curve.pop("type")


def _adopt_the_builds_version(defaults: dict, partial: dict) -> None:
    """Let a version 2 partial recipe with no `look.contrast` number take the build's
    `recipe_version`.

    Version 3 changed only that key — a slope in 2, a multiplier on the base slope in
    3 — and the default recipe writes it as `1.0`. Merged under the partial's stated 2,
    that default would read as an old slope and `hanten` would refuse it, though the
    partial never stated one. A partial that states a number keeps its version, so
    `hanten` refuses it with the conversion."""
    look = partial.get("look")
    stated = look.get("contrast") if isinstance(look, dict) else None
    later = defaults.get("recipe_version")
    if (partial.get("recipe_version") == 2 and isinstance(later, int) and later > 2
            and not isinstance(stated, (int, float))):
        partial["recipe_version"] = later


#: The destination axes the convenience flags may set, as the recipe
#: `output.display` names them.
DISPLAY_AXES = ("range", "transfer", "gamut", "container")


def _output_override(args) -> tuple[object, str | None]:
    """What the destination flags ask of the recipe's `output`: `"film-master"`, a
    dict of the display axes stated, or `None` when no flag was passed — with an
    error when they contradict each other."""
    axes = {axis: getattr(args, axis, None) for axis in DISPLAY_AXES}
    axes = {axis: value for axis, value in axes.items() if value}
    if getattr(args, "film_master", False):
        if axes:
            return None, ("--film-master writes no display destination; drop "
                          + ", ".join(f"--{axis}" for axis in axes))
        return "film-master", None
    return axes or None, None


def _freeze_recipe(base: dict, dmin: list[float],
                   film_type: str | None, output: object = None,
                   exposure: float | None = None) -> tuple[dict | None, str | None]:
    """Overlay measured calibration and the convenience flags on a partial recipe.

    `output` and `exposure` write destination-build keys (`output`,
    `scene_correction.exposure`), so they need a base from a build that takes
    destinations; on a preset build's recipe (the reference build) they are refused
    and the partial `--recipe` states that build's own keys instead.
    """
    recipe = json.loads(json.dumps(base))
    # The measurement lives in the `calibration` section (design-spec §8): a roll
    # calibration is a recipe with nothing else, a pipeline profile is a recipe with
    # no `calibration` at all.
    calibration = recipe.setdefault("calibration", {})
    if not isinstance(calibration, dict):
        return None, "recipe `calibration` must be an object"
    calibration["film_base"] = {"explicit": dmin}
    # The default recipe from a build before `nf-retire/dmax-machinery` (the reference
    # build) still writes the retired reference at its old default; drop it so the
    # frozen recipe names only what this build reads. A stated one is left for
    # `hanten` to refuse.
    if calibration.get("dmax") == "fixed":
        del calibration["dmax"]

    reconstruction = recipe.setdefault("reconstruction", {})
    if not isinstance(reconstruction, dict):
        return None, "recipe `reconstruction` must be an object"
    # The curve is left as the merge resolved it: the default recipe writes the build's own
    # (with its tag on the reference build, without on this one).
    curve = reconstruction.get("curve")
    if curve is not None and not isinstance(curve, dict):
        return None, "recipe `reconstruction.curve` must be an object"

    if film_type:
        input_cfg = recipe.setdefault("input", {})
        if not isinstance(input_cfg, dict):
            return None, "recipe `input` must be an object"
        input_cfg["film_type"] = film_type
    if (output is not None or exposure is not None) and not _manifest.is_destination_recipe(recipe):
        return None, ("--film-master, --range, --transfer, --gamut, --container and "
                      "--exposure set keys of recipe_version 2 and later, and this "
                      "build's recipe is not one (it takes output presets); state its "
                      "output in --recipe instead")
    if output == "film-master":
        recipe["output"] = "film-master"
    elif output is not None:
        current = recipe.get("output")
        display = current.get("display") if isinstance(current, dict) else None
        # Axes stated over a film master start a display destination afresh.
        display = dict(display) if isinstance(display, dict) else {}
        display.update(output)
        recipe["output"] = {"display": display}
    if exposure is not None:
        scene = recipe.setdefault("scene_correction", {})
        if not isinstance(scene, dict):
            return None, "recipe `scene_correction` must be an object"
        scene["exposure"] = exposure
    return recipe, None


def _float3(report: dict, key: str) -> list[float] | None:
    value = report.get(key)
    if isinstance(value, dict):
        value = [value.get("r"), value.get("g"), value.get("b")]
    if (isinstance(value, list) and len(value) == 3
            and all(isinstance(item, (int, float)) and math.isfinite(item) for item in value)):
        return [float(item) for item in value]
    return None


def _config_id(recipe: dict) -> str:
    payload = json.dumps(recipe, sort_keys=True, separators=(",", ":")).encode()
    return "config-" + hashlib.sha256(payload).hexdigest()[:12]


def _relative(path: Path, root: Path) -> str:
    try:
        return path.resolve().relative_to(root.resolve()).as_posix()
    except ValueError:
        return str(path.resolve())


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _build_mismatch_hint(mode: str, error: str) -> str:
    """The other `--dmin-mode`, only when the failure says the build lacks this one.

    Any other failure (a `--strict` warning, an empty area, a degenerate base) is the
    frame's, and naming a mode there would be a remedy that cannot work.
    """
    if mode == "grid" and "--grid was removed" in error:
        return " (this build measures the effective area instead: pass --dmin-mode area)"
    # An older build's sourceless `estimate` runs the rebate search, whose refusal
    # names it.
    if mode == "area" and "auto film-base detection" in error:
        return " (this build predates the effective-area measurement: pass --dmin-mode grid)"
    return ""


def cmd_convert(args) -> int:
    output, error = _output_override(args)
    if error:
        print(f"error: {error}", file=sys.stderr)
        return 2
    exposure = getattr(args, "exposure", None)
    root = Path(args.asset_root).resolve()
    data, error = _asset_manifest(root)
    if error:
        print(f"error: {error}", file=sys.stderr)
        return 2
    assert data is not None
    roles, error = _roll_frames(data, args.roll)
    if error:
        print(f"error: {error}", file=sys.stderr)
        return 2
    assert roles is not None
    command = _defaults_command(args.nc)
    defaults, error = _run_json([args.nc, command], "default recipe query",
                                jsonc=command == "profile")
    if not error:
        assert defaults is not None
        defaults, error = _unwrap_envelope(defaults, "default recipe query")
    if error:
        print(f"error: {error}", file=sys.stderr)
        return 2
    partial, meta, error = _recipe_input(args.recipe)
    if error:
        print(f"error: {error}", file=sys.stderr)
        return 2
    assert defaults is not None and partial is not None
    # The default recipe no longer writes `reconstruction.type`, so a recipe still
    # spelling the old `"density"` tag would read as a variant switch in the merge and
    # replace the whole default reconstruction. nc accepts the tag; drop it here.
    reconstruction = partial.get("reconstruction")
    if isinstance(reconstruction, dict) and reconstruction.get("type") == "density":
        reconstruction.pop("type")
    _drop_retired_curve_tag(defaults, partial)
    _adopt_the_builds_version(defaults, partial)
    base = _deep_merge(defaults, partial)
    # Everything but the measurement is known now, so a recipe the flags cannot be
    # frozen into is refused before the Dmin estimate spends its time.
    _, error = _freeze_recipe(base, [0.0, 0.0, 0.0], args.film_type, output, exposure)
    if error:
        print(f"error: {error}", file=sys.stderr)
        return 2

    unexposed = roles["unexposed"][0]
    # `area` measures the frame's effective area and takes no region; `grid` (builds
    # before `film-base/holder-masked-measurement`) and `region` read one.
    if args.dmin_mode == "area":
        if args.dmin_region is not None:
            print("error: --dmin-region applies to --dmin-mode grid or region; `area` "
                  "measures the frame's effective area", file=sys.stderr)
            return 2
        dmin_region = None
    else:
        dmin_region, error = _region(args.dmin_region, unexposed, "Dmin")
        if error:
            print(f"error: {error}", file=sys.stderr)
            return 2
        assert dmin_region

    operational = ["--max-memory", args.max_memory]
    strict = ["--strict"] if args.strict_estimate else []
    film_type = ["--film-type", args.film_type] if args.film_type else []
    unexposed_path = root / unexposed["file"]
    # Only the frames this run reads: a manifest `leader` is no longer measured, so a
    # missing or drifted one must not fail the run.
    all_frames = roles["unexposed"] + roles["real"]
    sources = []
    for frame in all_frames:
        path, label = root / frame["file"], frame.get("role", "frame")
        if not path.is_file():
            print(f"error: {label} frame is missing: {path}", file=sys.stderr)
            return 2
        expected = frame.get("sha256")
        if not isinstance(expected, str) or not expected:
            print(f"error: manifest frame has no sha256: {frame['file']}; regenerate it first",
                  file=sys.stderr)
            return 2
        try:
            actual = _sha256(path)
        except OSError as hash_error:
            print(f"error: cannot checksum {path}: {hash_error}", file=sys.stderr)
            return 2
        if actual != expected:
            print(f"error: manifest checksum drift for {frame['file']}; run "
                  "`nctool manifest generate` before converting", file=sys.stderr)
            return 1
        sources.append({"file": frame["file"], "role": frame.get("role", "real"),
                        "sha256": actual})

    source = [] if dmin_region is None else ["--base-region", dmin_region]
    grid = ["--grid"] if args.dmin_mode == "grid" else []
    dmin_report, error = _run_json(
        [args.nc, _base_command(args.nc), str(unexposed_path), *source, *grid, *film_type, *strict,
         *operational], "Dmin estimation")
    if error:
        print(f"error: {error}{_build_mismatch_hint(args.dmin_mode, error)}",
              file=sys.stderr)
        return 1
    assert dmin_report is not None
    # An older build's `estimate` with no source ran the rebate search instead, so the
    # report must say it measured the area.
    if args.dmin_mode == "area" and dmin_report.get("film_base_source") != "effective_area":
        print("error: the build did not measure the effective area (its `estimate` "
              "predates it): pass --dmin-mode grid", file=sys.stderr)
        return 1
    dmin = _float3(dmin_report, "film_base")
    if dmin is None:
        print("error: Dmin report has no finite three-channel `film_base`", file=sys.stderr)
        return 1

    recipe, error = _freeze_recipe(base, dmin, args.film_type, output, exposure)
    if error:
        print(f"error: {error}", file=sys.stderr)
        return 2
    assert recipe is not None
    config = args.config or _config_id(recipe)
    if not CONFIG_RE.fullmatch(config):
        print("error: --config must contain only letters, digits, dot, underscore, or hyphen",
              file=sys.stderr)
        return 2
    out_dir = (Path(args.out_dir).resolve() if args.out_dir else
               root / "converted" / "nc" / config / args.roll)
    if out_dir.exists() and any(out_dir.iterdir()):
        print(f"error: output directory is not empty: {out_dir}", file=sys.stderr)
        return 2
    out_dir.mkdir(parents=True, exist_ok=True)
    recipe_path = out_dir / "recipe.json"
    calibration_path = out_dir / "calibration.json"
    report_path = out_dir / "roll-report.json"
    tags_path = out_dir / "tags.json"
    calibration = {
        "schema_version": 1,
        "roll": args.roll,
        "dmin": {"frame": unexposed["file"], "region": dmin_region,
                 "mode": args.dmin_mode, "value": dmin, "report": dmin_report},
    }
    # An enveloped `--recipe` keeps its `meta`, so `hanten roll` checks the
    # `pipeline_version` it was written under; a bare one stays bare.
    _write_json(recipe_path, recipe if meta is None else {"meta": meta, "params": recipe})
    _write_json(calibration_path, calibration)

    real_paths = [str(root / frame["file"]) for frame in roles["real"]]
    argv = [args.nc, "roll", *real_paths, "--out-dir", str(out_dir),
            "--params", str(recipe_path), "--report-file", str(report_path),
            "--max-memory", args.max_memory]
    if args.strict_roll:
        argv.append("--strict")
    try:
        proc = subprocess.run(argv, capture_output=True, text=True, check=False)
    except OSError as run_error:
        print(f"error: roll conversion could not start {args.nc!r}: {run_error}",
              file=sys.stderr)
        return 2
    if proc.stdout.strip():
        print(proc.stdout, end="", file=sys.stderr)
    if proc.stderr:
        print(proc.stderr, end="", file=sys.stderr)
    roll_report, report_error = _load_object(report_path)
    if report_error:
        print(f"error: roll conversion produced no usable report: {report_error}", file=sys.stderr)
        return proc.returncode or 1
    assert roll_report is not None
    tags = {
        "schema_version": TAG_SCHEMA,
        "kind": "nctool-roll-conversion",
        "created": _datetime.datetime.now(_datetime.timezone.utc).isoformat(),
        "config": config,
        "roll": args.roll,
        "asset_root_manifest_generated": data.get("generated"),
        "source_frames": sources,
        "output_dir": _relative(out_dir, root),
        "recipe_file": _relative(recipe_path, root),
        "calibration_file": _relative(calibration_path, root),
        "report_file": _relative(report_path, root),
        "identity": roll_report.get("identity"),
        "recipe": recipe,
        "calibration": {"dmin": calibration["dmin"] | {"report": None}},
        "summary": roll_report.get("summary"),
    }
    tags["calibration"]["dmin"].pop("report")
    _write_json(tags_path, tags)
    if proc.returncode != 0:
        print(f"error: hanten roll exited {proc.returncode}; tags preserve the failed run",
              file=sys.stderr)
        return 1
    print(json.dumps(tags, indent=2, sort_keys=True))
    return 0


def _tag_path(root: Path, roll: str, ref: str) -> Path:
    path = Path(ref)
    if path.is_file() or path.name == "tags.json" or os.sep in ref:
        return path.resolve()
    return root / "converted" / "nc" / ref / roll / "tags.json"


class DepthError(ValueError):
    """The roll's output depth cannot be stated: the report names no build, no frame
    resolved a destination, or one resolved a depth this reader does not know."""


def _frame_depth(frame: dict) -> str | None:
    """The depth a destination build's frame resolved (`chain.destination`), or
    `None` when it resolved none (a frame that failed first)."""
    chain = _manifest.chain_block(frame)
    if "destination" not in chain:
        return None
    depth = _compare.depth_for_destination(chain["destination"])
    if depth is None:
        raise DepthError("a frame resolved a destination of unknown depth ("
                         + json.dumps(chain["destination"], sort_keys=True) + ")")
    return depth


def _preset_depth(recipe: dict) -> str:
    """A preset build's depth, fixed for the whole roll by the recipe's preset."""
    output = recipe.get("output") if isinstance(recipe.get("output"), dict) else {}
    preset = output.get("preset", "gain-map-hdr")
    if preset in ("gain-map-hdr", "ultra-hdr-v1"):
        return "u8"
    if preset in ("hdr-pq", "hdr-hlg"):
        return "u10"
    if preset in ("film-master", "hdr-linear-tiff"):
        return "f32"
    # Retired before the reference build's successors; still read from its runs.
    if preset in ("legacy", "custom") and output.get("depth") == "f32":
        return "f32"
    return "u16"


def _depths(recipe: dict, report: dict) -> tuple[str, list[str | None]]:
    """The roll's output depth and each report frame's.

    Which source states it follows the build's `pipeline_version`
    (`manifest.output_interface`): a destination build's frames each state the
    destination they resolved, since its recipe may leave the axes to nc; a preset
    build's depth is the recipe's preset. The roll's depth is `mixed` when its frames
    differ; each frame's is then the one to read.

    Raises `DepthError` rather than recording no depth, like
    `metrics.space_for_run`: `output_depth` says which units the frames' means are
    in, and a missing one would let two artifacts' means be compared across units.
    """
    frames = [frame for frame in report.get("frames", []) if isinstance(frame, dict)]
    identity = report.get("identity") if isinstance(report.get("identity"), dict) else {}
    pipeline_version = identity.get("pipeline_version")
    if not isinstance(pipeline_version, int) or isinstance(pipeline_version, bool):
        raise DepthError("the report states no identity.pipeline_version, so whether "
                         "its frames' depth follows a destination or a preset is unknown")
    if _manifest.output_interface(pipeline_version) == "preset":
        depth = _preset_depth(recipe)
        return depth, [depth if frame.get("status") == "ok" else None for frame in frames]
    per_frame = [_frame_depth(frame) for frame in frames]
    resolved = sorted({depth for depth in per_frame if depth is not None})
    if not resolved:
        raise DepthError("no frame resolved a destination (did every frame fail?), so "
                         "the roll's depth cannot be stated")
    return (resolved[0] if len(resolved) == 1 else "mixed"), per_frame


def _clip(frame: dict) -> float | None:
    loss = frame.get("loss")
    if not isinstance(loss, dict):
        return None
    total = loss.get("total_samples")
    low, high = loss.get("clipped_low"), loss.get("clipped_high")
    if not all(isinstance(value, (int, float)) for value in (total, low, high)):
        return None
    return (low + high) / total if total else 0.0


def _analysis_frame(frame: dict, source_by_name: dict[str, str],
                    depth: str | None) -> dict:
    """Select stable, conversion-relevant fields from one roll report row, plus the
    depth its output was written at (absent when it wrote none)."""
    input_ref = frame.get("input")
    name = Path(input_ref).name if isinstance(input_ref, str) else "(unknown)"
    result = {
        "source": source_by_name.get(name, name),
        "status": frame.get("status"),
    }
    # `dmax` is still read: reports from the reference build carry it. `chain` (or
    # `new_flow`, its name before `nf-core/report-contract`) is a destination build's
    # per-frame rendering facts, the resolved destination among them.
    for key in ("film_base", "dmax", "white_balance", "input_color", "loss",
                "output_stats", "identity", "chain", "new_flow", "warnings", "error"):
        if key in frame:
            result[key] = frame[key]
    if depth is not None:
        result["output_depth"] = depth
    clip_fraction = _clip(frame)
    if clip_fraction is not None:
        result["clip_fraction"] = clip_fraction
    return result


def cmd_analyze(args) -> int:
    """Write one deterministic, diff-friendly artifact for a converted roll."""
    root = Path(args.asset_root).resolve()
    tag_path = _tag_path(root, args.roll, args.run)
    tag, error = _load_object(tag_path)
    if error:
        print(f"error: {error}", file=sys.stderr)
        return 2
    assert tag is not None
    if tag.get("schema_version") != TAG_SCHEMA or tag.get("kind") != "nctool-roll-conversion":
        print(f"error: {tag_path} is not an nctool roll tag v{TAG_SCHEMA}", file=sys.stderr)
        return 2
    if tag.get("roll") != args.roll:
        print(f"error: tags are for roll {tag.get('roll')!r}, not {args.roll!r}",
              file=sys.stderr)
        return 2
    sources = tag.get("source_frames")
    if not isinstance(sources, list) or not sources:
        print("error: tags have no source-frame checksum inventory", file=sys.stderr)
        return 2
    report_ref = tag.get("report_file")
    if not isinstance(report_ref, str):
        print("error: tags have no report_file", file=sys.stderr)
        return 2
    report_path = Path(report_ref)
    if not report_path.is_absolute():
        report_path = root / report_path
    report, error = _load_object(report_path)
    if error:
        print(f"error: {error}", file=sys.stderr)
        return 2
    assert report is not None

    stable_sources = sorted(
        ({key: source.get(key) for key in ("file", "role", "sha256")}
         for source in sources if isinstance(source, dict)),
        key=lambda source: str(source.get("file")),
    )
    source_by_name = {
        Path(source["file"]).name: source["file"]
        for source in stable_sources if isinstance(source.get("file"), str)
    }
    try:
        depth, frame_depths = _depths(
            tag.get("recipe") if isinstance(tag.get("recipe"), dict) else {}, report)
    except DepthError as depth_error:
        print(f"error: {report_path}: {depth_error}", file=sys.stderr)
        return 2
    frames = [
        _analysis_frame(frame, source_by_name, frame_depth)
        for frame, frame_depth in zip(
            (frame for frame in report.get("frames", []) if isinstance(frame, dict)),
            frame_depths)
    ]
    frames.sort(key=lambda frame: str(frame.get("source")))
    output = {
        "schema_version": ANALYSIS_SCHEMA,
        "kind": "nctool-roll-analysis",
        "roll": args.roll,
        "config": tag.get("config"),
        "source_frames": stable_sources,
        "identity": tag.get("identity"),
        "output_depth": depth,
        "recipe": tag.get("recipe"),
        "calibration": tag.get("calibration"),
        "summary": report.get("summary", tag.get("summary")),
        "frames": frames,
        "note": ("Stable analysis of nc's recorded per-frame conversion facts; it does "
                 "not prove pixel identity or judge visual quality."),
    }
    out_path = Path(args.out).resolve() if args.out else tag_path.with_name("analysis.json")
    try:
        _write_json(out_path, output)
    except OSError as write_error:
        print(f"error: cannot write {out_path}: {write_error}", file=sys.stderr)
        return 2
    print(f"wrote {out_path} ({len(frames)} frames)", file=sys.stderr)
    return 0

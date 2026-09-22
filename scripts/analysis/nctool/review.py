"""Render a described matrix of conversions into a review set for `tools/review-app`.

One entry point, and the matrix is **data**: a JSON file naming the configurations
and the flags each one passes, so changing what is compared never means editing
code. It replaces `scripts/preset-review/generate.py`, which stated its matrix as
a Python list.

Each cell of the matrix is one `hanten convert` of one frame with one configuration.
Beside the rendered image the generator writes that image's **metric record**
(`nctool metrics`), so the review app can draw the tone and cast charts next to
the picture instead of only showing the picture. The record is derived numbers
only — never pixels, never a photograph (CLAUDE.md).

Three properties worth keeping:

* **Every config renders through the path being measured.** The matrix states one
  `output_preset` for the whole set, and both the file suffix and the colour space
  the metrics are read in come from *that name* rather than from a guess about the
  bytes.
* **A cell that fails is reported and skipped**, never silently dropped: the other
  cells still make a reviewable page, and `review.json` simply carries no rendition
  for that config — which the app renders as a visible gap.
* **Output goes to a throwaway directory outside the repo.** The frames are the
  user's own photographs and are never committed; only the matrix is.
"""
from __future__ import annotations

import json
import re
import os
import subprocess
import sys
import tempfile
from pathlib import Path

from . import metrics as _metrics

#: The matrix format this module reads. A future shape bumps it.
SCHEMA = 1

#: The `review.json` schema the app parses (`tools/review-app/SCHEMA.md`).
REVIEW_SCHEMA = 1

#: Suffix per output preset, mirroring `cli::derived_extension`.
#:
#: Not load-bearing in the dangerous direction: nc refuses an `-o` whose suffix
#: its resolved preset does not accept, so a stale entry here fails loudly at the
#: first render rather than writing a mislabelled file.
PRESET_SUFFIX: dict[str, str] = {
    "gain-map-hdr": "jpg",
    "ultra-hdr-v1": "jpg",
    "hdr-pq": "avif",
    "hdr-hlg": "avif",
    "legacy": "tiff",
    "custom": "tiff",
    "film-master": "tiff",
    "display-p3": "tiff",
    "compatibility": "tiff",
    "hdr-linear-tiff": "tiff",
    "hdr-pq-tiff": "tiff",
    "hdr-hlg-tiff": "tiff",
}

#: Ids that are safe to build a filename from, as `roll.py` spells it.
#:
#: Every cell writes `<frame>-<config>.<suffix>` into the output directory, so an
#: id carrying a path separator would place it somewhere else entirely — `..` up
#: into the repository the output check just refused, or an absolute path that
#: discards the output directory altogether, while `review.json` goes on naming
#: the bare filename.
SAFE_ID = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]*$")


def check_id(value: str, what: str, at: str) -> str:
    if not SAFE_ID.match(value) or ".." in value:
        raise ReviewError(
            f"{at}: {what} {value!r} is not filename-safe; every cell writes "
            "<frame>-<config> into the output directory")
    return value


#: Flags the generator supplies itself, refused in a matrix.
#:
#: `nc` takes the **last** occurrence of a `Set` argument, so a config restating one
#: would be silently overridden — and `--output-preset` decides the file suffix and
#: the colour space besides, so the override would not even be consistently ignored.
#: `--report-file` belongs here too: it sends the report to a file *instead of*
#: stdout, which is where the generator reads each cell's resolved recipe from —
#: so a matrix passing it would lose the measurement on every cell that rendered
#: perfectly well, and every cell would overwrite the same report path.
OWNED_FLAGS = ("--output-preset", "-o", "--output", "--report", "--report-file")

#: Placeholders a config's `args` may use, resolved per frame.
#:
#: `dmin` comes from the fixture declaration the metrics already use, so the two
#: cannot drift; `film_stock` is stated per roll in the matrix, because the roll
#: names in the fixtures ("2026-07-24-Gold200") are not the registry ids
#: (`gold-200`) and deriving one from the other is the guess this toolkit refuses
#: to make elsewhere.
PLACEHOLDERS = ("dmin", "film_stock")


class ReviewError(Exception):
    """A malformed matrix, or a run that cannot produce a reviewable set."""


def _write_json(path: Path, value: dict) -> None:
    """Atomically write a JSON artifact into the output directory."""
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=path.parent, prefix=f".{path.name}.", suffix=".tmp")
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as stream:
            json.dump(value, stream, indent=2, sort_keys=True)
            stream.write("\n")
        os.replace(tmp, path)
    except Exception:
        try:
            os.remove(tmp)
        except OSError:
            pass
        raise


def _load_object(path: Path, what: str) -> dict:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ReviewError(f"cannot read {what} {path}: {error}") from error
    if not isinstance(value, dict):
        raise ReviewError(f"{what} {path}: expected a JSON object")
    return value


def _string(value, at: str) -> str:
    if not isinstance(value, str) or not value:
        raise ReviewError(f"{at} must be a non-empty string")
    return value


def _optional_string(value, at: str) -> str | None:
    """An optional string, checked.

    Copied verbatim into `review.json`, where the app's own parser refuses a
    non-string — so an unchecked one here renders every cell and then makes the
    whole set unloadable, which is exactly the twenty-minutes-too-late failure
    this loader exists to prevent.
    """
    if value is None:
        return None
    return _string(value, at)


def _known_keys(record: dict, allowed: set[str], at: str) -> None:
    """Refuse a key this loader does not read.

    The `deny_unknown_fields` rule every recipe struct in this repo follows, and
    for the same reason: `"arg"` for `"args"` loads as *no* arguments, so that
    cell renders the default conversion under a label promising something else —
    five buttons, five labels, identical pixels, exit 0. `"insets"` for `"inset"`
    measures the whole frame, holder included.
    """
    unknown = sorted(set(record) - allowed)
    if unknown:
        raise ReviewError(
            f"{at}: unknown key{'s' if len(unknown) > 1 else ''} "
            f"{', '.join(unknown)}; known: {', '.join(sorted(allowed))}")


def _string_list(value, at: str) -> list[str]:
    if not isinstance(value, list):
        raise ReviewError(f"{at} must be an array of strings")
    return [_string(item, f"{at}[{index}]") for index, item in enumerate(value)]


def placeholders_in(args: list[str], at: str) -> set[str]:
    """The placeholder names one argument list uses, refusing unknown ones.

    Checked when the matrix is read rather than when a frame is rendered, so a
    typo is one error at the top instead of the same error once per frame — or,
    worse, a literal `{film_stok}` handed to `nc` as a flag value.
    """
    used: set[str] = set()
    for arg in args:
        rest = arg
        while "{" in rest:
            head, _, rest = rest.partition("{")
            del head
            name, closed, rest = rest.partition("}")
            if not closed:
                raise ReviewError(f"{at}: unterminated placeholder in {arg!r}")
            if name not in PLACEHOLDERS:
                known = ", ".join(f"{{{p}}}" for p in PLACEHOLDERS)
                raise ReviewError(
                    f"{at}: unknown placeholder {{{name}}} in {arg!r}; known: {known}")
            used.add(name)
    return used


def reject_owned_flags(args: list[str], at: str) -> None:
    """Refuse a matrix that states a flag the generator supplies itself."""
    for arg in args:
        # `-o/tmp/x.jpg` is one token to clap, so splitting on `=` alone misses it —
        # and it would then beat the generator's own `-o`, writing the image
        # somewhere `review.json` does not name.
        name = arg[:2] if arg.startswith("-o") and not arg.startswith("--") else arg.split("=", 1)[0]
        if name in OWNED_FLAGS:
            raise ReviewError(
                f"{at}: {name} is set by the generator, not by the matrix"
                + (" — state it once as output_preset" if name == "--output-preset" else ""))


def expand_args(args: list[str], values: dict[str, str]) -> list[str]:
    """Substitute the per-frame values into one config's arguments."""
    out = []
    for arg in args:
        for name, value in values.items():
            arg = arg.replace("{" + name + "}", value)
        out.append(arg)
    return out


def load_matrix(path: Path) -> dict:
    """Read and validate a review matrix.

    Validation is deliberately front-loaded: a matrix error found after twenty
    minutes of rendering is a matrix error found too late.
    """
    raw = _load_object(path, "matrix")
    _known_keys(raw, {"schema_version", "title", "description", "output_dir",
                      "output_preset", "common_args", "rolls", "frames", "metrics",
                      "configs"}, str(path))
    version = raw.get("schema_version")
    if version != SCHEMA:
        raise ReviewError(
            f"{path}: schema_version must be {SCHEMA}, got {version!r}")

    preset = _string(raw.get("output_preset"), "output_preset")
    if preset not in PRESET_SUFFIX:
        known = ", ".join(sorted(PRESET_SUFFIX))
        raise ReviewError(f"unknown output_preset {preset!r}; known: {known}")

    common = _string_list(raw.get("common_args", []), "common_args")
    placeholders_in(common, "common_args")
    reject_owned_flags(common, "common_args")

    configs_raw = raw.get("configs")
    if not isinstance(configs_raw, list) or not configs_raw:
        raise ReviewError("configs must list at least one configuration")
    configs = []
    seen: set[str] = set()
    for index, entry in enumerate(configs_raw):
        at = f"configs[{index}]"
        if not isinstance(entry, dict):
            raise ReviewError(f"{at} must be an object")
        _known_keys(entry, {"id", "label", "note", "args"}, at)
        cid = check_id(_string(entry.get("id"), f"{at}.id"), "config id", f"{at}.id")
        if cid in seen:
            raise ReviewError(f"configs contains two entries with id {cid!r}")
        seen.add(cid)
        args = _string_list(entry.get("args", []), f"{at}.args")
        reject_owned_flags(args, f"{at}.args")
        configs.append({
            "id": cid,
            "label": _string(entry.get("label", cid), f"{at}.label"),
            "note": _optional_string(entry.get("note"), f"{at}.note"),
            "args": args,
            "needs": placeholders_in(args, at + ".args") | placeholders_in(common, at),
        })

    rolls_raw = raw.get("rolls", {})
    if not isinstance(rolls_raw, dict):
        raise ReviewError("rolls must be an object keyed by roll name")
    rolls = {}
    for name, entry in rolls_raw.items():
        if not isinstance(entry, dict):
            raise ReviewError(f"rolls.{name} must be an object")
        _known_keys(entry, {"film_stock"}, f"rolls.{name}")
        stock = entry.get("film_stock")
        rolls[name] = {"film_stock": _string(stock, f"rolls.{name}.film_stock")
                       if stock is not None else None}

    frames = raw.get("frames")
    if frames is not None:
        frames = _string_list(frames, "frames")

    measure = raw.get("metrics", {})
    if not isinstance(measure, dict):
        raise ReviewError("metrics must be an object")
    _known_keys(measure, {"inset"}, "metrics")
    inset = measure.get("inset", 0.0)
    if not isinstance(inset, (int, float)) or not 0.0 <= inset < 0.5:
        raise ReviewError("metrics.inset must be a fraction in [0, 0.5)")

    return {
        "title": _optional_string(raw.get("title"), "title"),
        "description": _optional_string(raw.get("description"), "description"),
        "output_preset": preset,
        "suffix": PRESET_SUFFIX[preset],
        "common_args": common,
        "configs": configs,
        "rolls": rolls,
        "frames": frames,
        "inset": float(inset),
        "output_dir": _optional_string(raw.get("output_dir"), "output_dir"),
    }


def metrics_space(preset: str) -> tuple[str | None, str]:
    """Whether a preset's output can be measured at all, before anything renders.

    A pre-flight only, so a matrix whose output this toolkit cannot read (AVIF,
    PQ-encoded) says so once up front instead of once per cell. It is **not** the
    answer used to measure — see `cell_space`. Such a matrix still renders a page
    and is still perfectly reviewable by eye; it just has no charts.
    """
    if preset in _metrics.PRESET_UNREADABLE:
        return None, f"{preset} {_metrics.PRESET_UNREADABLE[preset]}"
    space = _metrics.PRESET_SPACES.get(preset)
    if space is None:
        return None, f"no verified colour space for {preset}"
    return space, ""


def cell_space(recipe: dict, expected_preset: str) -> tuple[str | None, str]:
    """The colour space one rendered cell is measured in, from its **own** recipe.

    Resolved per cell from what `nc` reports it resolved, not from the matrix's
    preset name, because the preset name does not determine the space: `legacy`
    and `custom` accept `--output-profile`, which a matrix is free to pass, and
    `space_for_recipe` is what maps it — and what refuses an `f32` output whose
    transfer this toolkit has not verified. Reading the name alone would measure
    ProPhoto pixels as sRGB and report every tone and cast number as if it were
    right, which is the plausible wrong answer the metrics module exists to
    refuse.

    The recipe must **state** the preset the matrix asked for. `space_for_recipe`
    defaults an unstated one to `gain-map-hdr`, so a report that could not be read
    would otherwise resolve Display P3 for a `film-master` render — linear ACEScg
    pixels measured as an encoded display space, every number wrong and every one
    of them plausible.
    """
    output = recipe.get("output") if isinstance(recipe.get("output"), dict) else {}
    preset = output.get("preset")
    if preset != expected_preset:
        return None, (
            f"the render reports output preset {preset!r}, not the matrix's "
            f"{expected_preset!r}; not measuring pixels this run cannot identify")
    try:
        space, _why = _metrics.space_for_recipe(recipe)
    except _metrics.MetricsError as error:
        return None, str(error)
    return space, ""


def _dimensions(record: dict) -> dict:
    """The rendition's pixel size, as `review.json` states it.

    Both or neither: the app refuses half a size, because half of one reserves no
    box. Absent when a cell is not measured, which the schema allows — the page
    then sizes itself from the image, as it did before this existed.
    """
    image = record.get("image") if isinstance(record.get("image"), dict) else {}
    width, height = image.get("width"), image.get("height")
    if isinstance(width, int) and isinstance(height, int) and width > 0 and height > 0:
        return {"width": width, "height": height}
    return {}


def rendition_stem(frame: str, config_id: str) -> str:
    return f"{frame}-{config_id}"


def colliding_stems(frames: list[str], config_ids: list[str]) -> list[str]:
    """Cells whose filenames would land on top of each other.

    `<frame>-<config>` is not injective when either id may contain a hyphen —
    and config ids here routinely do (`chr-generic`). Frame `a-b` with config `c`
    and frame `a` with config `b-c` both write `a-b-c`, so the second render
    overwrites the first while both `review.json` entries point at the surviving
    bytes: a comparison of one rendition with itself, under two labels. Cheaper to
    refuse than to encode around, since a set that trips it is misnamed anyway.
    """
    seen: dict[str, str] = {}
    clashes = []
    for frame in frames:
        for config_id in config_ids:
            stem = rendition_stem(frame, config_id)
            cell = f"{frame}/{config_id}"
            # Compared **case-folded**: the default macOS volume — and Windows —
            # treat `A-c.jpg` and `a-c.jpg` as one file, so an exact-string check
            # would pass while the second render silently replaced the first.
            key = stem.casefold()
            if key in seen:
                clashes.append(f"{seen[key]} and {cell} would both write {stem}")
            else:
                seen[key] = cell
    return clashes


def is_measured(record: dict, digest: str, region: dict, space: str,
                decoder: str | None = None) -> bool:
    """Whether a stored record already describes these exact bytes and region.

    Measuring a 74 MP frame is minutes of work, and a generator run re-renders
    every cell — so the check is against the rendered file's **checksum**, which
    the record already carries, rather than against an mtime a re-render always
    moves.

    The **declared space** is part of that identity, not a detail: the same file
    measured as sRGB and as Display P3 gives different tone and cast numbers, and
    `nctool metrics image --space …` beside the same image is a documented way to
    produce one. Reusing the wrong one charts a record this run did not mean.
    """
    if record.get("schema_version") != _metrics.SCHEMA:
        return False
    if record.get("sha256") != digest:
        return False
    declared = record.get("space") if isinstance(record.get("space"), dict) else {}
    if declared.get("declared") != space:
        return False
    # A JPEG's samples are whatever its decoder says they are — which is why the
    # record names the decoder at all. Reusing across a Pillow or libjpeg upgrade
    # mixes measurements from two decoders inside one set, in whichever cells
    # happened not to be re-rendered.
    image = record.get("image") if isinstance(record.get("image"), dict) else {}
    stored_decoder = image.get("decoder")
    if stored_decoder is not None and decoder is not None and stored_decoder != decoder:
        return False
    stored = record.get("region")
    if not isinstance(stored, dict):
        return False
    return all(stored.get(key) == region.get(key) for key in ("x", "y", "width", "height"))


def build_review(matrix: dict, images: list[dict]) -> dict:
    """The `review.json` document for a finished run."""
    return {
        "schema_version": REVIEW_SCHEMA,
        **({"title": matrix["title"]} if matrix["title"] else {}),
        **({"description": matrix["description"]} if matrix["description"] else {}),
        "configs": [
            {"id": c["id"], "label": c["label"],
             **({"note": c["note"]} if c["note"] else {})}
            for c in matrix["configs"]
        ],
        "images": images,
    }


def _frames_to_render(matrix: dict, fixtures: dict,
                      requested: str | None) -> list[str]:
    known = list(fixtures.get("frames", {}))
    if requested:
        names = [name.strip() for name in requested.split(",") if name.strip()]
    elif matrix["frames"]:
        names = list(matrix["frames"])
    else:
        names = known
    seen: set[str] = set()
    repeated = sorted({name for name in names if name in seen or seen.add(name)})
    if repeated:
        # Two entries for one frame render every cell twice and write two images
        # with the same id, which the app refuses — after the expensive part.
        raise ReviewError(f"frame named more than once: {', '.join(repeated)}")
    for name in names:
        check_id(name, "frame", "frames")
    missing = [name for name in names if name not in known]
    if missing:
        raise ReviewError(
            f"no such frame in the fixtures: {', '.join(missing)} "
            f"(known: {', '.join(known)})")
    if not names:
        raise ReviewError("no frames to render")
    return names


def _render(nc: Path, source: Path, dest: Path, args: list[str]) -> dict:
    """Run one conversion, returning nc's report or raising with its message."""
    cmd = [str(nc), "convert", str(source), "-o", str(dest), "--report", "json", *args]
    result = subprocess.run(cmd, capture_output=True, text=True)
    if result.returncode:
        tail = result.stderr.strip().splitlines()
        message = tail[-1][:200] if tail else "no message"
        raise ReviewError(f"nc exited {result.returncode}: {message}")
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError:
        return {}


def _cast_note(report: dict) -> str | None:
    """The one-line G/R B/R summary the page shows beside the heading.

    Only for a **positive** red mean, which is not the same as a non-zero one:
    the unclamped-float presets (`film-master`, `hdr-linear-tiff`) can report a
    negative mean, and dividing by it prints sign-flipped ratios rather than no
    ratio at all.
    """
    mean = report.get("output_stats", {}).get("mean")
    if not isinstance(mean, list) or len(mean) != 3:
        return None
    if not isinstance(mean[0], (int, float)) or mean[0] <= 0:
        return None
    return f"G/R {mean[1] / mean[0]:.3f} B/R {mean[2] / mean[0]:.3f}"


def cmd_generate(args) -> int:
    """Render a matrix and write the review set."""
    try:
        matrix = load_matrix(Path(args.matrix))
        fixtures = _load_object(Path(args.fixtures), "fixtures")
        frames = _frames_to_render(matrix, fixtures, args.frames)
        clashes = colliding_stems(frames, [c["id"] for c in matrix["configs"]])
        if clashes:
            raise ReviewError("; ".join(clashes))

        # **What was asked is checked before what is installed.** An unbuildable
        # environment is the less specific fault: telling someone to build the
        # binary when their real problem is `--out .` costs them a round trip and
        # then says something else.
        out = Path(args.out or matrix["output_dir"] or "../temp/review").resolve()
        # The frames are the user's own photographs and are never committed, so
        # the one destination this refuses is the repository itself (CLAUDE.md).
        repo = Path(__file__).resolve().parents[3]
        if out == repo or repo in out.parents:
            raise ReviewError(
                f"{out} is inside the repository ({repo}); a review set is rendered "
                "photographs and must go to a throwaway directory outside it")

        # Resolved, because `Path("./fakenc")` normalises to a bare name that
        # `is_file()` accepts and `subprocess` then looks up on PATH instead.
        nc = Path(args.nc).resolve()
        if not nc.is_file():
            raise ReviewError(f"no hanten binary at {nc}; `cargo build --release` first")
        assets = Path(args.asset_root).resolve()
        if not (assets / "manifest.json").is_file():
            raise ReviewError(f"no assets at {assets}")

        out.mkdir(parents=True, exist_ok=True)
    except ReviewError as error:
        print(f"error: {error}", file=sys.stderr)
        return 2

    readable, why_not = metrics_space(matrix["output_preset"])
    measuring = readable is not None and not args.no_metrics
    if readable is None and not args.no_metrics:
        print(f"note: no metrics — {why_not}", file=sys.stderr)
    if measuring:
        # Asked **once**, before anything renders. Measuring is the only part of
        # this toolkit that is not stdlib-only, and a fresh checkout has no venv
        # (it is gitignored) — so without this the setup instructions would print
        # once per cell, seventy-odd lines of it, after every render had already
        # run. The page is still worth having without charts.
        try:
            _metrics.require_dependencies()
        except _metrics.MetricsError as error:
            print(f"note: no metrics — {error}", file=sys.stderr)
            measuring = False

    if matrix["suffix"] not in ("jpg", "avif"):
        print(f"note: {matrix['output_preset']} writes {matrix['suffix'].upper()}, which most "
              "browsers do not display in an <img> (Safari does); the set will render but "
              "most of it will show as broken images", file=sys.stderr)

    fraction = _metrics.inset_fraction(matrix["inset"]) if matrix["inset"] else (
        0.0, 0.0, 1.0, 1.0)

    images: list[dict] = []
    failures: list[str] = []
    for key in frames:
        frame = fixtures["frames"][key]
        roll = frame["roll"]
        source = assets / "rolls" / roll / frame["file"]
        if not source.is_file():
            print(f"{key}: {source} missing, skipped", file=sys.stderr)
            continue
        roll_fixture = fixtures.get("rolls", {}).get(roll, {})
        values = {
            "dmin": ",".join(str(c) for c in roll_fixture.get("dmin", [])),
            "film_stock": (matrix["rolls"].get(roll) or {}).get("film_stock") or "",
        }

        renditions: dict[str, object] = {}
        notes: list[str] = []
        for config in matrix["configs"]:
            cell = f"{key}/{config['id']}"
            # A config needing a value this roll does not state loses only its own
            # cell. That is the difference between a stock with no digitized sheet
            # costing one column and costing the whole frame.
            unmet = [name for name in config["needs"] if not values.get(name)]
            if unmet:
                print(f"{cell}: roll {roll} states no {', '.join(unmet)}, skipped",
                      file=sys.stderr)
                failures.append(cell)
                continue

            dest = out / f"{rendition_stem(key, config['id'])}.{matrix['suffix']}"
            try:
                report = _render(nc, source, dest,
                                 expand_args(matrix["common_args"] + config["args"], values)
                                 + ["--output-preset", matrix["output_preset"]])
            except ReviewError as error:
                print(f"{cell}: {error}", file=sys.stderr)
                failures.append(cell)
                continue

            rendition: dict[str, object] = {"src": dest.name}
            if measuring:
                # The space this cell is measured in comes from the recipe `nc`
                # reports it resolved — provenance, not the matrix's preset name.
                space, why = cell_space(report.get("recipe", {}), matrix["output_preset"])
                record_path = dest.with_name(dest.name + ".metrics.json")
                if space is None:
                    print(f"{cell}: metrics skipped — {why}", file=sys.stderr)
                else:
                    try:
                        record = _measure(dest, record_path, space, fraction, args.force)
                    except ReviewError as error:
                        print(f"{cell}: metrics skipped — {error}", file=sys.stderr)
                    else:
                        rendition["metrics"] = record_path.name
                        rendition.update(_dimensions(record))
            renditions[config["id"]] = rendition

            note = _cast_note(report)
            if note:
                notes.append(f"{config['id']} {note}")
            print(f"{key:4} {config['id']:12} -> {dest.name}", file=sys.stderr)

        # Kept even when every cell failed. The set is the record of what was
        # compared, and a frame that silently vanishes tells a later reader
        # nothing; an empty rendition map draws the gaps the app already has.
        images.append({
            "id": key,
            "label": f"{key} — {roll} · {frame['file']}",
            **({"note": " · ".join(notes)} if notes else {}),
            "renditions": renditions,
        })

    if not images:
        print("error: no frame could be read — no review set written", file=sys.stderr)
        return 1

    _write_json(out / "review.json", build_review(matrix, images))
    rendered = sum(len(image["renditions"]) for image in images)
    print(f"\n{len(images)} frames x {len(matrix['configs'])} configs -> "
          f"{out}/review.json", file=sys.stderr)
    if failures:
        print(f"FAILED cells ({len(failures)}): {', '.join(failures)}", file=sys.stderr)
    print(f"\n  cd tools/review-app && pnpm dev {out}/review.json", file=sys.stderr)
    # The set is written either way — it is the record of what was attempted —
    # but a run that rendered nothing did not produce a comparison, and says so
    # with its exit status rather than only in the lines above.
    return 0 if rendered else 1


def _stored_record(path: Path) -> dict:
    """A previously written record, or an empty one when it cannot be read."""
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError):
        return {}
    return value if isinstance(value, dict) else {}


def _measure(image: Path, record_path: Path, space: str,
             fraction: tuple[float, float, float, float], force: bool) -> dict:
    """Measure one rendered image, reusing a record that already describes it.

    Returns the record — freshly measured or the reused one — because it is also
    the only place the rendition's pixel dimensions are known: `nc`'s report does
    not carry them, and the review file wants them so the page can reserve the
    right box before the image loads. A measurement that cannot run (no numpy, an
    unreadable container) raises instead, so the caller can say *why* the charts
    will be missing.
    """
    try:
        _metrics.require_dependencies()
    except _metrics.MetricsError as error:
        raise ReviewError(str(error)) from error

    # Only JPEG records name a decoder, and `_jpeg_decoder_identity` imports
    # Pillow, so it is asked for once per cell rather than at import time.
    decoder = _metrics._jpeg_decoder_identity() if _metrics._is_jpeg(image) else None
    digest = _metrics.sha256(image)
    if not force and record_path.is_file():
        stored = _stored_record(record_path)
        size = stored.get("image") if isinstance(stored.get("image"), dict) else {}
        width, height = size.get("width"), size.get("height")
        if isinstance(width, int) and isinstance(height, int) and width > 0 and height > 0:
            region = _metrics.resolve_region(width, height, fraction)
            if is_measured(stored, digest, region, space, decoder):
                return stored

    try:
        record = _metrics.measure(image, space, fraction, digest=True)
    except _metrics.MetricsError as error:
        raise ReviewError(str(error)) from error
    _write_json(record_path, record)
    return record

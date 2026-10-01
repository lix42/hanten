"""Patch rectangles on roll frames — the asset manifest's per-frame `patches`.

A patch is a labelled region of a scan that an analysis measures: a neutral
(white / grey) to judge a cast, or a saturated `colour` to judge desaturation.
`kind` keeps the two apart so a consumer never averages a colour patch in as a
neutral. `rect` is `[x, y, w, h]` as fractions of the **source scan** — Hanten
never crops, so a render's frame is the scan's frame, and the review app's
percentages divide straight down.

`import` folds the review app's **Copy all** text into the manifest. Its headings
name an image's *label*, so the review set's `review.json` maps them back to image
ids, and an id to a frame by its serial (`1981` or `p400-0928d-1981`). A label
prefixed `W:` / `G:` / `C:` sets that patch's kind; `--kind` covers the rest.
"""
from __future__ import annotations

import json
import os
import re
import sys

KINDS = ("white", "grey", "colour", "unknown")
LIGHTS = ("sun", "shade")
FIELDS = {"label", "rect", "kind", "light", "source"}
# Copy all prints 0.1 %, and x and w round independently, so an edge-clamped
# rectangle can read up to 0.1 % past the frame.
EDGE_SLACK = 1.5e-3
PREFIX_KIND = {"W": "white", "G": "grey", "C": "colour"}

_HEADING = re.compile(r"^## (.+?)\s*$")
_PATCH = re.compile(
    r'^- (".*") — x ([\d.]+)% y ([\d.]+)% w ([\d.]+)% h ([\d.]+)%\s*$')
_PREFIX = re.compile(r"^([WGC])(?::\s*(.*))?$")


def check_patch(p: object) -> list[str]:
    """Every way `p` breaks the patch schema; empty when it is well formed."""
    if not isinstance(p, dict):
        return [f"not an object ({type(p).__name__})"]
    errs = [f"unknown key {k!r}" for k in sorted(set(p) - FIELDS)]
    if not isinstance(p.get("label"), str) or not p["label"].strip():
        errs.append("label must be a non-empty string")
    if not isinstance(p.get("source"), str) or not p["source"].strip():
        errs.append("source must be a non-empty string")
    if p.get("kind") not in KINDS:
        errs.append(f"kind {p.get('kind')!r} is not one of {', '.join(KINDS)}")
    if "light" in p and p["light"] not in LIGHTS:
        errs.append(f"light {p['light']!r} is not one of {', '.join(LIGHTS)}")
    r = p.get("rect")
    if not (isinstance(r, list) and len(r) == 4
            and all(isinstance(v, (int, float)) and not isinstance(v, bool) for v in r)):
        errs.append("rect must be four numbers [x, y, w, h]")
    else:
        x, y, w, h = r
        if not (w > 0 and h > 0 and x >= 0 and y >= 0
                and x + w <= 1 + EDGE_SLACK and y + h <= 1 + EDGE_SLACK):
            errs.append(f"rect {r} does not lie inside the frame (fractions 0-1, w, h > 0)")
    return errs


def check_frame_patches(fr: dict) -> list[str]:
    """`check_patch` over a frame's `patches`, which must itself be a list."""
    if "patches" not in fr:
        return []
    ps = fr["patches"]
    if not isinstance(ps, list):
        return [f"patches is not a list ({type(ps).__name__})"]
    return [f"patch {i}: {e}" for i, p in enumerate(ps) for e in check_patch(p)]


def clamp_rect(rect: list[float]) -> list[float]:
    """`rect` pulled inside the frame — undoes the 0.1 % rounding at an edge."""
    x, y = min(max(rect[0], 0.0), 1.0), min(max(rect[1], 0.0), 1.0)
    return [x, y, round(min(rect[2], 1.0 - x), 4), round(min(rect[3], 1.0 - y), 4)]


def split_kind(label: str, default: str) -> tuple[str, str]:
    """A `W:` / `G:` / `C:` label prefix → (label, kind); else (label, default)."""
    m = _PREFIX.match(label.strip())
    if not m:
        return label, default
    kind = PREFIX_KIND[m.group(1)]
    return (m.group(2) or "").strip() or kind, kind


def parse_copy_all(text: str) -> dict[str, list[tuple[str, list[float]]]]:
    """The review app's Copy all text → `{heading: [(label, rect)]}`, rect as
    fractions. Notes are ignored; a `- ` line under `Patches:` that does not parse
    raises ValueError, since dropping it would shrink the frame's patch list."""
    out: dict[str, list[tuple[str, list[float]]]] = {}
    heading, in_patches = None, False
    for n, line in enumerate(text.splitlines(), 1):
        if m := _HEADING.match(line):
            heading, in_patches = m.group(1), False
        elif line.strip() == "Patches:":
            in_patches = heading is not None
        elif in_patches and line.startswith("- "):
            m = _PATCH.match(line)
            if not m:
                raise ValueError(f"line {n} is not a patch the review app writes: {line!r}")
            label = json.loads(m.group(1))
            rect = [round(float(v) / 100, 4) for v in m.groups()[1:]]
            out.setdefault(heading, []).append((label, rect))
        elif in_patches and line.strip():
            in_patches = False
    return out


def image_ids_by_heading(review: dict) -> dict[str, str]:
    """Copy all heads a frame with its label, or its id when it has none. Two
    images with one heading cannot be told apart, so that raises ValueError."""
    out: dict[str, str] = {}
    for img in review.get("images", []):
        heading = img.get("label") or img["id"]
        if heading in out:
            raise ValueError(f"review.json has two images headed {heading!r}")
        out[heading] = img["id"]
    return out


def frame_for_image(frames: list[dict], image_id: str) -> dict | None:
    """The roll frame an image id names: its stem, or the id's last `-` field."""
    serial = image_id.rsplit("-", 1)[-1]
    for fr in frames:
        stem = os.path.splitext(os.path.basename(fr["file"]))[0]
        if stem in (image_id, serial):
            return fr
    return None


def resolve_frame(rolls: dict, image_id: str, roll: str | None) -> tuple[dict | None, str]:
    """(frame, "") for the one frame `image_id` names — in `roll`, or in any roll
    when it is None — else (None, why)."""
    names = [roll] if roll else sorted(rolls)
    hits = [(r, fr) for r in names
            if (fr := frame_for_image(rolls[r].get("frames", []), image_id))]
    if len(hits) == 1:
        return hits[0][1], ""
    if not hits:
        return None, f"names no frame of {roll or 'any roll'}"
    return None, ("names a frame in several rolls ("
                  + ", ".join(r for r, _ in hits) + "); pass --roll")


def replace_source(existing: list[dict], new: list[dict], source: str) -> list[dict]:
    """A frame's patches with every one from `source` replaced by `new`."""
    return [p for p in existing if p.get("source") != source] + new


def cmd_import(args) -> int:
    from . import manifest as _manifest

    def fail(msg: str) -> int:
        print(f"error: {msg}; nothing was written", file=sys.stderr)
        return 2

    mpath = os.path.join(os.path.abspath(args.asset_root), "manifest.json")
    data, err = _manifest.load_manifest(mpath)
    if err or not data:
        return fail(err or f"no manifest at {mpath}")
    rolls = data.get("rolls", {})
    if args.roll and not isinstance(rolls.get(args.roll), dict):
        return fail(f"unknown roll {args.roll!r}")
    try:
        with open(args.review) as f:
            ids = image_ids_by_heading(json.load(f))
        if args.notes == "-":
            text = sys.stdin.read()
        else:
            with open(args.notes) as f:
                text = f.read()
        parsed = parse_copy_all(text)
    except (OSError, ValueError) as e:
        return fail(str(e))
    if not parsed:
        return fail("no patches found in the notes (expected the review app's Copy all text)")

    # Group by frame first: two headings naming one frame must add up, not let
    # the second replace the first.
    updates: dict[str, tuple[dict, list[dict]]] = {}
    for heading, rows in parsed.items():
        image_id = ids.get(heading)
        fr, why = resolve_frame(rolls, image_id, args.roll) if image_id else (
            None, "is no image in review.json")
        if fr is None:
            return fail(f"{heading!r} {why}")
        if not isinstance(fr.get("patches", []), list):
            return fail(f"{fr['file']} has a `patches` that is not a list; fix it by hand")
        new = []
        for label, rect in rows:
            label, kind = split_kind(label, args.kind)
            new.append({"label": label, "rect": clamp_rect(rect), "kind": kind,
                        "source": args.source})
        bad = [f"{p['label']!r}: {e}" for p in new for e in check_patch(p)]
        if bad:
            return fail(f"{heading}: " + "; ".join(bad))
        updates.setdefault(fr["file"], (fr, []))[1].extend(new)

    for fr, new in updates.values():
        fr["patches"] = replace_source(fr.get("patches", []), new, args.source)
        print(f"{fr['file']}: {len(new)} patch(es) from {args.source!r}")
    data["schema_version"] = _manifest.SCHEMA
    if args.dry_run:
        print("(dry run — manifest.json not written)")
        return 0
    _manifest.write_manifest(mpath, data)
    print("wrote", mpath)
    return 0

"""The synthetic acceptance chart: a small negative of uniform patches.

The decode-back oracles need content a scanned fixture lacks — a neutral ramp,
saturated colours, and colours that no tone or gamut mapping touches — so this
module writes one (`tests/fixtures/chart-48bit.tif`, via
`python -m nctool acceptance chart`). Each patch is placed at a chosen **film RGB**
value by inverting the fixed decode's documented line (design-spec §7,
`algo::fixed`): `L = 10^(linearization·(D′ − A))`, `D′ = scale·D`,
`D = −log10(T / base)`. The constants are the decode's defaults, so a default change
moves where a patch lands but not what the oracles check: every comparison is against
the canonical buffer of the same render.

The patch list is data (`PATCHES`); the acceptance manifest names patches by `name`
and reads their rectangles from `layout()`, so the two cannot drift.
"""

from __future__ import annotations

import math

# The fixed decode's defaults (`hanten profile`: `reconstruction`), and the film base the
# benchmark fixture cases state.
FILM_BASE = (0.9, 0.55, 0.42)
DENSITY_SCALE = (1.0, 0.84, 0.73)
LINEARIZATION = 1.8
# Mid grey sits `0.62` density above the base (`mid-at-base-offset`).
MID_GREY_DENSITY = 0.62
MID_GREY = 0.18

PATCH = 48
GAP = 8
COLUMNS = 8


def _ramp():
    return [(f"neutral{k:+d}", "neutral", tuple(MID_GREY * 2.0 ** k for _ in range(3)))
            for k in range(-3, 5)]


def _saturated():
    lo, hi = MID_GREY / 4, MID_GREY * 4
    return [
        ("sat-red", "saturated", (hi, lo, lo)),
        ("sat-green", "saturated", (lo, hi, lo)),
        ("sat-blue", "saturated", (lo, lo, hi)),
        ("sat-cyan", "saturated", (lo, hi, hi)),
        ("sat-magenta", "saturated", (hi, lo, hi)),
        ("sat-yellow", "saturated", (hi, hi, lo)),
    ]


def _muted():
    # Near mid grey with modest chroma: inside every destination gamut and on the part
    # of the tone curve every range shares.
    m = MID_GREY
    return [
        ("muted-warm", "muted", (m * 1.25, m, m * 0.8)),
        ("muted-cool", "muted", (m * 0.8, m, m * 1.25)),
        ("muted-green", "muted", (m * 0.85, m * 1.15, m * 0.85)),
        ("muted-magenta", "muted", (m * 1.15, m * 0.85, m * 1.15)),
        ("muted-skin", "muted", (m * 1.3, m * 0.95, m * 0.75)),
        ("muted-dark", "muted", (m * 0.5, m * 0.45, m * 0.4)),
        ("muted-light", "muted", (m * 1.6, m * 1.7, m * 1.9)),
    ]


PATCHES = _ramp() + _saturated() + _muted()


def transmission(film_rgb, anchor: float) -> tuple[float, float, float]:
    """The scan transmission that decodes to `film_rgb` under the default decode."""
    out = []
    for c, value in enumerate(film_rgb):
        d_prime = anchor + math.log10(value) / LINEARIZATION
        d = d_prime / DENSITY_SCALE[c]
        out.append(FILM_BASE[c] * 10.0 ** (-d))
    return tuple(out)


def anchor() -> float:
    """The density that renders to 1.0: mid grey's density plus mid grey's stops."""
    return MID_GREY_DENSITY - math.log10(MID_GREY) / LINEARIZATION


def layout() -> tuple[int, int, dict]:
    """`(width, height, {name: dict(kind, film_rgb, x, y, w, h)})`, patches in rows of
    `COLUMNS` on unexposed film base."""
    rects = {}
    for i, (name, kind, rgb) in enumerate(PATCHES):
        col, row = i % COLUMNS, i // COLUMNS
        rects[name] = dict(kind=kind, film_rgb=list(rgb),
                           x=GAP + col * (PATCH + GAP), y=GAP + row * (PATCH + GAP),
                           w=PATCH, h=PATCH)
    rows = math.ceil(len(PATCHES) / COLUMNS)
    width = GAP + COLUMNS * (PATCH + GAP)
    height = GAP + rows * (PATCH + GAP)
    return width, height, rects


def pixels():
    """The chart's 16-bit RGB samples, `(height, width, 3)` uint16."""
    import numpy as np
    width, height, rects = layout()
    base = np.array(FILM_BASE)
    img = np.empty((height, width, 3), dtype=np.float64)
    img[:] = base
    a = anchor()
    for r in rects.values():
        img[r["y"]:r["y"] + r["h"], r["x"]:r["x"] + r["w"]] = transmission(r["film_rgb"], a)
    return np.clip(np.floor(img * 65535 + 0.5), 0, 65535).astype(np.uint16)


def write(path: str) -> None:
    """Write the chart as an uncompressed, interleaved 48-bit RGB TIFF."""
    import tifffile
    tifffile.imwrite(path, pixels(), photometric="rgb", planarconfig="contig")

"""How a frame was shot and a roll was made — the manifest's per-frame `shot` and
per-roll `conditions`.

Both are hand-entered facts a calibration consumer needs and a scan cannot tell it:
a chart bracket's exposure offsets, the light, the development. `generate` keeps a
frame's `shot` like its `patches` (across a rename too) and a roll's `conditions` by
roll name, and refuses to drop either; `validate` reports one that breaks the shape
below. The value checks here are shared with `nctool.patches`.
"""
from __future__ import annotations

import math

# The protocol's lights (`analysis/calibration-frame-capture`), most even first; a
# patch's `light` takes the same words.
LIGHTS = ("overcast", "sun", "shade")
SHOT_TEXT = {"bracket", "metered", "note"}
SHOT_POSITIVE = {"aperture", "shutter_s"}  # f-number; seconds
SHOT_FIELDS = SHOT_TEXT | SHOT_POSITIVE | {"exposure_ev", "light"}
CONDITION_FIELDS = {"light", "meter", "camera", "lens", "support", "development",
                    "scan", "note"}


def is_number(v: object) -> bool:
    """A finite JSON number (`json.load` also yields NaN and Infinity)."""
    return isinstance(v, (int, float)) and not isinstance(v, bool) and math.isfinite(v)


def is_text(v: object) -> bool:
    return isinstance(v, str) and bool(v.strip())


def _check_light(d: dict) -> list[str]:
    if "light" in d and d["light"] not in LIGHTS:
        return [f"light {d['light']!r} is not one of {', '.join(LIGHTS)}"]
    return []


def check_shot(s: object) -> list[str]:
    """Every way a frame's `shot` breaks its shape; empty when well formed.

    `exposure_ev` is the frame's offset from the metered reading (a bracket's −2 … +4),
    `bracket` names the series it belongs to, `aperture` is the f-number."""
    if not isinstance(s, dict):
        return [f"shot is not an object ({type(s).__name__})"]
    if not s:
        return ["shot is empty"]
    errs = [f"shot: unknown key {k!r}" for k in sorted(set(s) - SHOT_FIELDS)]
    if "exposure_ev" in s and not is_number(s["exposure_ev"]):
        errs.append("shot: exposure_ev must be a number")
    errs += [f"shot: {k} must be a positive number" for k in sorted(SHOT_POSITIVE & set(s))
             if not (is_number(s[k]) and s[k] > 0)]
    errs += [f"shot: {k} must be a non-empty string" for k in sorted(SHOT_TEXT & set(s))
             if not is_text(s[k])]
    return errs + [f"shot: {e}" for e in _check_light(s)]


def check_conditions(c: object) -> list[str]:
    """Every way a roll's `conditions` breaks its shape; empty when well formed."""
    if not isinstance(c, dict):
        return [f"conditions is not an object ({type(c).__name__})"]
    if not c:
        return ["conditions is empty"]
    errs = [f"conditions: unknown key {k!r}" for k in sorted(set(c) - CONDITION_FIELDS)]
    errs += [f"conditions: {k} must be a non-empty string"
             for k in sorted(set(c) & CONDITION_FIELDS - {"light"}) if not is_text(c[k])]
    return errs + [f"conditions: {e}" for e in _check_light(c)]

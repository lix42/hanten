"""Read an ICC profile from its bytes: header, tag table, matrix/TRC tags and `cicp`.

The decode-back oracles (`nctool.acceptance`) decode nc's output through the profile
the *file* carries, read here from ICC.1:2022 rather than through Little CMS, which is
what wrote it. Only what a matrix/TRC display profile and a CICP-signalled profile
need is parsed, plus the `lutAtoBType` form (curves and a matrix, no CLUT) nc's
coded-HDR profiles use.

Stdlib only, so the parser is testable without the metrics venv; the curve
evaluators accept a numpy array or a float.
"""

from __future__ import annotations

import struct

HEADER_BYTES = 128


class IccError(ValueError):
    """The bytes are not a profile this module can read."""


def _s15f16(raw: bytes, at: int) -> float:
    return struct.unpack_from(">i", raw, at)[0] / 65536.0


def _xyz(data: bytes) -> tuple[float, float, float]:
    if data[:4] != b"XYZ ":
        raise IccError(f"expected an XYZType, got {data[:4]!r}")
    return tuple(_s15f16(data, 8 + 4 * i) for i in range(3))


def _sf32(data: bytes) -> list[list[float]]:
    if data[:4] != b"sf32" or len(data) < 8 + 36:
        raise IccError(f"expected a 3x3 s15Fixed16ArrayType, got {data[:4]!r}")
    v = [_s15f16(data, 8 + 4 * i) for i in range(9)]
    return [v[0:3], v[3:6], v[6:9]]


# Parameters of each `para` function type (ICC.1:2022 Table 68), in stored order.
PARA_PARAMS = {0: ("g",), 1: ("g", "a", "b"), 2: ("g", "a", "b", "c"),
               3: ("g", "a", "b", "c", "d"), 4: ("g", "a", "b", "c", "d", "e", "f")}


def _curve(data: bytes) -> dict:
    kind = data[:4]
    if kind == b"para":
        function = struct.unpack_from(">H", data, 8)[0]
        names = PARA_PARAMS.get(function)
        if names is None:
            raise IccError(f"unknown parametricCurveType function {function}")
        return dict(kind="para", function=function,
                    params={n: _s15f16(data, 12 + 4 * i) for i, n in enumerate(names)})
    if kind == b"curv":
        count = struct.unpack_from(">I", data, 8)[0]
        if count == 0:
            return dict(kind="para", function=0, params=dict(g=1.0))
        if count == 1:
            return dict(kind="para", function=0,
                        params=dict(g=struct.unpack_from(">H", data, 12)[0] / 256.0))
        table = struct.unpack_from(f">{count}H", data, 12)
        return dict(kind="table", table=[t / 65535.0 for t in table])
    raise IccError(f"unsupported curve type {kind!r}")


def _text(data: bytes) -> str:
    kind = data[:4]
    if kind == b"mluc":
        count, size = struct.unpack_from(">II", data, 8)
        if count == 0 or size < 12:
            return ""
        length, offset = struct.unpack_from(">II", data, 16 + 4)
        return data[offset:offset + length].decode("utf-16-be")
    if kind == b"desc":
        length = struct.unpack_from(">I", data, 8)[0]
        return data[12:12 + length].rstrip(b"\0").decode("latin-1")
    if kind == b"text":
        return data[8:].rstrip(b"\0").decode("latin-1")
    raise IccError(f"unsupported text type {kind!r}")


def parse(raw: bytes) -> dict:
    """The profile's header facts and the tags the oracles read.

    Keys: `version` (`"4.4"`-style), `device_class`, `colour_space`, `pcs`, `tags`
    (signature list, in table order), `description`, `white` (`wtpt`), `chad`,
    `colorants` (`[rXYZ, gXYZ, bXYZ]`), `trc` (`[r, g, b]` curves), `cicp`
    (`[primaries, transfer, matrix, full_range]`), `lut` (whether an `A2B0` exists) and
    `a2b0` (its bytes, for `eval_lut_atob`).
    Absent tags are `None`.
    """
    if len(raw) < HEADER_BYTES + 4:
        raise IccError(f"{len(raw)} bytes is too short for an ICC profile")
    size = struct.unpack_from(">I", raw, 0)[0]
    if size != len(raw):
        raise IccError(f"the header says {size} bytes, the profile has {len(raw)}")
    if raw[36:40] != b"acsp":
        raise IccError("no 'acsp' signature at byte 36")
    major, minor = raw[8], raw[9] >> 4
    count = struct.unpack_from(">I", raw, HEADER_BYTES)[0]
    tags: dict[str, bytes] = {}
    order = []
    for i in range(count):
        sig, offset, length = struct.unpack_from(">4sII", raw, HEADER_BYTES + 4 + 12 * i)
        if offset + length > len(raw):
            raise IccError(f"tag {sig!r} runs past the end of the profile")
        name = sig.decode("latin-1")
        tags[name] = raw[offset:offset + length]
        order.append(name)

    def get(name, read):
        return read(tags[name]) if name in tags else None

    colorants = None
    if all(t in tags for t in ("rXYZ", "gXYZ", "bXYZ")):
        colorants = [_xyz(tags[t]) for t in ("rXYZ", "gXYZ", "bXYZ")]
    trc = None
    if all(t in tags for t in ("rTRC", "gTRC", "bTRC")):
        trc = [_curve(tags[t]) for t in ("rTRC", "gTRC", "bTRC")]
    cicp = None
    if "cicp" in tags:
        data = tags["cicp"]
        if data[:4] != b"cicp" or len(data) < 12:
            raise IccError("malformed cicp tag")
        cicp = list(data[8:12])
    return dict(
        version=f"{major}.{minor}",
        device_class=raw[12:16].decode("latin-1"),
        colour_space=raw[16:20].decode("latin-1"),
        pcs=raw[20:24].decode("latin-1"),
        tags=order,
        description=get("desc", _text),
        white=get("wtpt", _xyz),
        chad=get("chad", _sf32),
        colorants=colorants,
        trc=trc,
        cicp=cicp,
        lut="A2B0" in tags,
        a2b0=tags.get("A2B0"),
    )


def _inverse3(m):
    a, b, c = m[0]
    d, e, f = m[1]
    g, h, i = m[2]
    det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g)
    if det == 0:
        raise IccError("singular matrix")
    return [[(e * i - f * h) / det, (c * h - b * i) / det, (b * f - c * e) / det],
            [(f * g - d * i) / det, (a * i - c * g) / det, (c * d - a * f) / det],
            [(d * h - e * g) / det, (b * g - a * h) / det, (a * e - b * d) / det]]


def _matmul(a, b):
    return [[sum(a[r][k] * b[k][c] for k in range(3)) for c in range(3)] for r in range(3)]


def rgb_to_xyz(profile: dict) -> list[list[float]]:
    """The linear-RGB → XYZ matrix in the profile's **own** white: the D50-adapted
    colorants carried back through the inverse of `chad` (identity when absent)."""
    if profile.get("colorants") is None:
        raise IccError("the profile has no rXYZ/gXYZ/bXYZ colorants")
    d50 = [[profile["colorants"][c][r] for c in range(3)] for r in range(3)]
    chad = profile.get("chad")
    return d50 if chad is None else _matmul(_inverse3(chad), d50)


def primaries_xy(profile: dict) -> dict:
    """The red, green and blue primaries and the white as CIE 1931 `xy`, in the
    profile's own white (`rgb_to_xyz`)."""
    m = rgb_to_xyz(profile)
    out = {}
    for name, col in (("red", 0), ("green", 1), ("blue", 2)):
        x, y, z = (m[0][col], m[1][col], m[2][col])
        out[name] = (x / (x + y + z), y / (x + y + z))
    white = [sum(m[r]) for r in range(3)]
    out["white"] = (white[0] / sum(white), white[1] / sum(white))
    return out


def curve_decode(curve: dict, encoded):
    """Device value → linear, as the TRC defines it (numpy array or float in)."""
    import numpy as np
    x = np.asarray(encoded, dtype=np.float64)
    if curve["kind"] == "table":
        table = np.asarray(curve["table"], dtype=np.float64)
        return np.interp(np.clip(x, 0, 1), np.linspace(0, 1, len(table)), table)
    p, f = curve["params"], curve["function"]
    g = p["g"]
    if f == 0:
        return np.power(np.clip(x, 0, None), g)
    a, b = p["a"], p["b"]
    base = np.power(np.clip(a * x + b, 0, None), g)
    if f == 1:
        return np.where(x >= -b / a, base, 0.0)
    if f == 2:
        return np.where(x >= -b / a, base + p["c"], p["c"])
    if f == 3:
        return np.where(x >= p["d"], base, p["c"] * x)
    return np.where(x >= p["d"], base + p["e"], p["c"] * x + p["f"])


def curve_encode(curve: dict, linear):
    """Linear → device value: the inverse of `curve_decode`, in binary64. Only the
    monotonic forms nc writes are inverted (`para` 0 and 3, and an increasing table)."""
    import numpy as np
    y = np.asarray(linear, dtype=np.float64)
    if curve["kind"] == "table":
        table = np.asarray(curve["table"], dtype=np.float64)
        if np.any(np.diff(table) < 0):
            raise IccError("a decreasing TRC table has no inverse here")
        return np.interp(y, table, np.linspace(0, 1, len(table)))
    p, f = curve["params"], curve["function"]
    g = p["g"]
    if f == 0:
        return np.power(np.clip(y, 0, None), 1.0 / g)
    if f == 3:
        a, b, c, d = p["a"], p["b"], p["c"], p["d"]
        upper = (np.power(np.clip(y, 0, None), 1.0 / g) - b) / a
        return np.where(y >= c * d, upper, y / c)
    raise IccError(f"no inverse implemented for parametric function {f}")


def _curve_length(data: bytes) -> int:
    """Bytes one `curv` / `para` element occupies, padded to 4 (ICC.1 §10.12)."""
    if data[:4] == b"curv":
        n = 12 + 2 * struct.unpack_from(">I", data, 8)[0]
    else:
        n = 12 + 4 * len(PARA_PARAMS[struct.unpack_from(">H", data, 8)[0]])
    return (n + 3) // 4 * 4


def _curves(tag: bytes, offset: int, count: int) -> list[dict]:
    out = []
    for _ in range(count):
        out.append(_curve(tag[offset:]))
        offset += _curve_length(tag[offset:])
    return out


def eval_lut_atob(tag: bytes, device):
    """Evaluate an `mAB ` (lutAtoBType) tag on device values (`(..., 3)` in [0, 1]):
    M curves, then the matrix with its offset, then B curves (ICC.1:2022 §10.12). A tag
    with A curves or a CLUT is refused, not approximated. Returns the PCS values as
    stored, `[0, 1]`-normalized; an XYZ PCS scales them by `1 + 32767/32768`."""
    import numpy as np
    if tag[:4] != b"mAB ":
        raise IccError(f"expected lutAtoBType, got {tag[:4]!r}")
    if tag[8] != 3 or tag[9] != 3:
        raise IccError(f"a {tag[8]}-in, {tag[9]}-out lutAtoBType is not RGB → XYZ")
    b_at, matrix_at, m_at, clut_at, a_at = struct.unpack_from(">5I", tag, 12)
    if clut_at or a_at:
        raise IccError("A curves or a CLUT in lutAtoBType are not evaluated here")
    y = np.asarray(device, dtype=np.float64)
    if m_at:
        m = _curves(tag, m_at, 3)
        y = np.stack([curve_decode(m[c], y[..., c]) for c in range(3)], axis=-1)
    if matrix_at:
        v = [_s15f16(tag, matrix_at + 4 * i) for i in range(12)]
        y = y @ np.array(v[:9]).reshape(3, 3).T + np.array(v[9:])
    b = _curves(tag, b_at, 3)
    return np.stack([curve_decode(b[c], y[..., c]) for c in range(3)], axis=-1)

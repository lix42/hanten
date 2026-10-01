"""Read an ISO 21496-1 gain-map JPEG and reconstruct its alternate rendition.

Written from the container standards — JPEG markers (T.81), the CIPA DC-007
Multi-Picture Format, and ISO 21496-1's binary metadata — not from nc's writer
(`io::iso_gain_map`), so the gain-map oracle in `nctool.acceptance` is an independent
reading. Parsing is stdlib-only; `reconstruct` needs numpy.
"""

from __future__ import annotations

import struct

ISO_URN = b"urn:iso:std:iso:ts:21496:-1\x00"
MPF_ID = b"MPF\x00"
ICC_ID = b"ICC_PROFILE\x00"

# Markers with no length field.
_STANDALONE = {0x01, *range(0xD0, 0xD8)}


class GainMapError(ValueError):
    """The bytes are not a gain-map JPEG this module can read."""


def segments(jpeg: bytes) -> list[dict]:
    """The marker segments of one JPEG up to its scan, in order: `marker`, `offset`
    (of the `FF`), `payload`. The entropy-coded data after SOS is not walked."""
    if jpeg[:2] != b"\xff\xd8":
        raise GainMapError("not a JPEG (no SOI)")
    out = []
    at = 2
    while at < len(jpeg):
        if jpeg[at] != 0xFF:
            raise GainMapError(f"expected a marker at byte {at}")
        marker = jpeg[at + 1]
        if marker == 0xFF:
            at += 1
            continue
        if marker in _STANDALONE:
            out.append(dict(marker=marker, offset=at, payload=b""))
            at += 2
            continue
        length = struct.unpack_from(">H", jpeg, at + 2)[0]
        out.append(dict(marker=marker, offset=at, payload=jpeg[at + 4:at + 2 + length]))
        at += 2 + length
        if marker == 0xDA:
            return out
    raise GainMapError("no SOS before the end of the data")


def app2(segs: list[dict], ident: bytes) -> list[bytes]:
    """The payloads (after the identifier) of the APP2 segments carrying `ident`."""
    return [s["payload"][len(ident):] for s in segs
            if s["marker"] == 0xE2 and s["payload"].startswith(ident)]


def icc_profile(segs: list[dict]) -> bytes | None:
    """The ICC profile reassembled from its `ICC_PROFILE` chunks (ICC.1 Annex B)."""
    chunks = app2(segs, ICC_ID)
    if not chunks:
        return None
    ordered = sorted(chunks, key=lambda c: c[0])
    if [c[0] for c in ordered] != list(range(1, len(ordered) + 1)) or any(
            c[1] != len(ordered) for c in ordered):
        raise GainMapError("ICC_PROFILE chunks are not numbered 1..n of n")
    return b"".join(c[2:] for c in ordered)


def mpf_entries(jpeg: bytes, segs: list[dict]) -> list[dict]:
    """The MP Entry list of the primary's MPF segment: per image `type`, `size` and
    its absolute `offset` in the file (the first image's is 0)."""
    found = [s for s in segs if s["marker"] == 0xE2 and s["payload"].startswith(MPF_ID)]
    if len(found) != 1:
        raise GainMapError(f"expected one MPF APP2 segment, found {len(found)}")
    seg = found[0]
    tiff = seg["payload"][len(MPF_ID):]
    # Offsets in an MP Entry count from the TIFF header inside the segment.
    base = seg["offset"] + 4 + len(MPF_ID)
    order = tiff[:2]
    if order not in (b"MM", b"II"):
        raise GainMapError(f"MPF byte order {order!r}")
    e = ">" if order == b"MM" else "<"
    if struct.unpack_from(e + "H", tiff, 2)[0] != 42:
        raise GainMapError("MPF TIFF header lacks 42")
    ifd = struct.unpack_from(e + "I", tiff, 4)[0]
    count = struct.unpack_from(e + "H", tiff, ifd)[0]
    entries_at = None
    entries_len = None
    for i in range(count):
        tag, kind, n, value = struct.unpack_from(e + "HHII", tiff, ifd + 2 + 12 * i)
        if tag == 0xB002:
            entries_at, entries_len = value, n
    if entries_at is None:
        raise GainMapError("MPF has no MPEntry tag (0xB002)")
    out = []
    for i in range(entries_len // 16):
        attr, size, offset, _, _ = struct.unpack_from(e + "IIIHH", tiff, entries_at + 16 * i)
        out.append(dict(type=attr & 0x00FFFFFF, size=size,
                        offset=0 if offset == 0 else base + offset))
    return out


def parse_metadata(payload: bytes) -> dict:
    """ISO 21496-1 binary gain-map metadata (big-endian), into floats: the versions,
    flags, `base_hdr_headroom` / `alternate_hdr_headroom` (log2) and per channel
    `gain_map_min` / `gain_map_max` (log2), `gamma`, `base_offset`,
    `alternate_offset`. A version-only payload (4 bytes) parses with `version_only`."""
    if len(payload) < 4:
        raise GainMapError("ISO metadata shorter than its version fields")
    minimum, writer = struct.unpack_from(">HH", payload, 0)
    out = dict(minimum_version=minimum, writer_version=writer)
    if len(payload) == 4:
        return dict(out, version_only=True)
    if minimum != 0:
        raise GainMapError(f"unsupported ISO 21496-1 minimum_version {minimum}")
    flags = payload[4]
    channels = 3 if flags & 0x80 else 1
    out.update(version_only=False, flags=flags, channels=channels,
               use_base_colour_space=bool(flags & 0x40),
               backward_direction=bool(flags & 0x04),
               common_denominator=bool(flags & 0x08))
    at = 5

    def u32():
        nonlocal at
        v = struct.unpack_from(">I", payload, at)[0]
        at += 4
        return v

    def s32():
        nonlocal at
        v = struct.unpack_from(">i", payload, at)[0]
        at += 4
        return v

    def frac(signed, common):
        n = s32() if signed else u32()
        d = common if common is not None else u32()
        if d == 0:
            raise GainMapError("a metadata fraction has denominator 0")
        return n / d

    common = u32() if out["common_denominator"] else None
    out["base_hdr_headroom"] = frac(False, common)
    out["alternate_hdr_headroom"] = frac(False, common)
    per = {k: [] for k in ("gain_map_min", "gain_map_max", "gamma",
                           "base_offset", "alternate_offset")}
    for _ in range(channels):
        per["gain_map_min"].append(frac(True, common))
        per["gain_map_max"].append(frac(True, common))
        per["gamma"].append(frac(False, common))
        per["base_offset"].append(frac(True, common))
        per["alternate_offset"].append(frac(True, common))
    if at != len(payload):
        raise GainMapError(f"{len(payload) - at} trailing bytes after the metadata")
    if channels == 1:
        per = {k: v * 3 for k, v in per.items()}
    out.update(per)
    return out


def read(jpeg: bytes) -> dict:
    """Split a gain-map JPEG: the primary's segments, ICC and ISO version payload,
    the MP entries, and the gain-map image's bytes, segments and ISO metadata."""
    base_segs = segments(jpeg)
    entries = mpf_entries(jpeg, base_segs)
    if len(entries) < 2:
        raise GainMapError(f"MPF lists {len(entries)} image(s); a gain map needs two")
    gm = entries[1]
    gain_map = jpeg[gm["offset"]:gm["offset"] + gm["size"]]
    if gain_map[:2] != b"\xff\xd8":
        raise GainMapError("the second MP entry does not point at a JPEG")
    gm_segs = segments(gain_map)
    base_iso = app2(base_segs, ISO_URN)
    gm_iso = app2(gm_segs, ISO_URN)
    if len(gm_iso) != 1:
        raise GainMapError(f"the gain map carries {len(gm_iso)} ISO 21496-1 segments")
    return dict(
        base_segments=base_segs,
        base_icc=icc_profile(base_segs),
        base_iso=[parse_metadata(p) for p in base_iso],
        entries=entries,
        gain_map_jpeg=gain_map,
        gain_map_segments=gm_segs,
        gain_map_icc=icc_profile(gm_segs),
        metadata=parse_metadata(gm_iso[0]),
    )


def upsample(codes, height: int, width: int):
    """Centre-aligned bilinear upsampling of a `(h, w, c)` map to `(height, width)`,
    edges clamped."""
    import numpy as np
    m = np.asarray(codes, dtype=np.float64)
    mh, mw = m.shape[:2]

    def axis(n_out, n_in):
        pos = (np.arange(n_out) + 0.5) * (n_in / n_out) - 0.5
        pos = np.clip(pos, 0, n_in - 1)
        lo = np.floor(pos).astype(int)
        hi = np.minimum(lo + 1, n_in - 1)
        return lo, hi, pos - lo

    y0, y1, fy = axis(height, mh)
    x0, x1, fx = axis(width, mw)
    top = m[y0][:, x0] * (1 - fx)[None, :, None] + m[y0][:, x1] * fx[None, :, None]
    bottom = m[y1][:, x0] * (1 - fx)[None, :, None] + m[y1][:, x1] * fx[None, :, None]
    return top * (1 - fy)[:, None, None] + bottom * fy[:, None, None]


def log2_gain(codes01, metadata: dict):
    """Normalized map values in [0, 1] (`(..., 3)`) → per-channel log2 gain."""
    import numpy as np
    v = np.clip(np.asarray(codes01, dtype=np.float64), 0, 1)
    gamma = np.asarray(metadata["gamma"])
    lo = np.asarray(metadata["gain_map_min"])
    hi = np.asarray(metadata["gain_map_max"])
    return lo + (hi - lo) * np.power(v, 1 / gamma)


def reconstruct(base_linear, codes01, metadata: dict, display_headroom: float | None = None):
    """The alternate rendition: `(base + k_base)·2^(G·W) − k_alt` per channel, with `W`
    from the display's log2 headroom (default: the alternate's, so `W = 1`).
    `codes01` must already be at the base's resolution."""
    import numpy as np
    base_h = metadata["base_hdr_headroom"]
    alt_h = metadata["alternate_hdr_headroom"]
    h = alt_h if display_headroom is None else display_headroom
    w = 1.0 if alt_h == base_h else float(np.clip((h - base_h) / (alt_h - base_h), 0, 1))
    if metadata.get("backward_direction"):
        raise GainMapError("backward_direction maps are not reconstructed here")
    g = log2_gain(codes01, metadata)
    k_base = np.asarray(metadata["base_offset"])
    k_alt = np.asarray(metadata["alternate_offset"])
    return (np.asarray(base_linear, dtype=np.float64) + k_base) * np.exp2(g * w) - k_alt

"""Reference cells: another producer's export of a frame, as a review-set cell.

A matrix names a producer (`silverfast`, `silverfast-ccr`, `nlp` — the manifest's
`converted/<producer>/` buckets); the manifest pairs its export to the frame by
`source_frame`, never by name or registration. The export is brought to the cell
format Hanten renders when a matrix has references — SDR sRGB JPEG — so toggling
compares like with like. The colour space is read from the export's embedded ICC
profile (its curves and colorants, never its description), or declared by the
matrix when the file carries none.

Conversion and pairing are pure; `review.cmd_generate` orchestrates them.
"""
from __future__ import annotations

import hashlib
import json
from pathlib import Path

from . import icc as _icc
from . import metrics as _metrics

#: Bump when the conversion's pixels change, so stored conversions are redone.
CONVERTER = 3

#: The JPEG every reference cell is written as.
JPEG_QUALITY = 95

#: Spaces an export may be in: the ones `profile_space` can recognise.
EXPORT_SPACES = ("srgb", "linear-srgb", "adobe-rgb", "linear-adobe-rgb",
                 "display-p3", "linear-display-p3", "prophoto", "prophoto-gamma1.8")

#: The destination a matrix with references must render Hanten's cells as.
CELL_DESTINATION = {"range": "sdr", "transfer": "native", "gamut": "srgb",
                    "container": "jpeg"}

#: Largest |decoded difference| at which a TRC counts as a known transfer.
TRC_TOLERANCE = 2e-3

#: Largest xy distance at which a profile's primaries count as a known set.
XY_TOLERANCE = 5e-3


class ReferenceError(Exception):
    """A reference cell that cannot be produced; costs only that cell."""


# -- the matrix ----------------------------------------------------------------


def destination_fault(destination: object) -> str | None:
    """Why a matrix's destination cannot sit beside reference cells, or `None`."""
    display = destination.get("display") if isinstance(destination, dict) else None
    if display == CELL_DESTINATION:
        return None
    wanted = "/".join(CELL_DESTINATION[a] for a in _metrics.DESTINATION_AXES)
    return (f"reference cells are SDR sRGB JPEG, so Hanten's cells must be too: state "
            f'"destination": {{"display": {json.dumps(CELL_DESTINATION)}}} ({wanted})')


# -- pairing -------------------------------------------------------------------


def exports(manifest: dict, producer: str) -> list[dict]:
    """Every manifest entry under `converted/<producer>/`, with its batch key."""
    found = []
    for key, batch in sorted((manifest.get("converted") or {}).items()):
        if not isinstance(batch, dict) or batch.get("producer") != producer:
            continue
        for entry in batch.get("outputs") or []:
            if isinstance(entry, dict):
                found.append({**entry, "batch": key})
    return found


def source_frames(manifest: dict) -> dict[str, dict]:
    """Every roll frame's manifest entry, keyed by its asset-relative path."""
    out = {}
    for roll in (manifest.get("rolls") or {}).values():
        for frame in (roll.get("frames") or []) if isinstance(roll, dict) else []:
            if isinstance(frame, dict) and isinstance(frame.get("file"), str):
                out[frame["file"]] = frame
    return out


def producers(manifest: dict) -> list[str]:
    """The producers the manifest has at least one export from."""
    return sorted({b["producer"] for b in (manifest.get("converted") or {}).values()
                   if isinstance(b, dict) and isinstance(b.get("producer"), str)
                   and b.get("outputs")})


def pair(entries: list[dict], source: str) -> list[dict]:
    """The exports whose `source_frame` is `source`."""
    return [e for e in entries if e.get("source_frame") == source]


def unpaired(entries: list[dict], rolls: set[str]) -> dict[str, list[str]]:
    """Per batch, the exports of these rolls that name no `source_frame`."""
    out: dict[str, list[str]] = {}
    for entry in entries:
        if entry.get("source_frame"):
            continue
        # `converted/<producer>/[<version>/]<roll>/<file>`: the roll is not always parts[2].
        parts = str(entry.get("file", "")).split("/")
        if any(part in rolls for part in parts[2:-1]):
            out.setdefault(entry["batch"], []).append(parts[-1])
    return out


# -- colour space --------------------------------------------------------------


#: Pure power-law TRCs by exponent: read exactly, since a sampled comparison
#: cannot tell gamma 1.8 from ROMM's toed curve.
POWER_TRANSFERS = {1.0: "linear", _metrics.ADOBE_RGB_GAMMA: "adobe-rgb",
                   1.8: "gamma1.8"}


def _transfer_of(curves: list[dict]) -> str | None:
    """The `metrics` transfer the three TRCs all decode as, or `None`."""
    import numpy as np

    if all(c["kind"] == "para" and c["function"] == 0 for c in curves):
        gammas = {round(c["params"]["g"], 2) for c in curves}
        found = {POWER_TRANSFERS.get(g) for g in POWER_TRANSFERS
                 if all(abs(g - x) < 0.01 for x in gammas)}
        return found.pop() if len(found) == 1 else None
    # Tables and toed curves: sampled, with a relative bound near black, where the
    # toes differ most.
    probe = np.concatenate([np.linspace(0.0, 0.05, 41), np.linspace(0.05, 1.0, 65)])
    decoded = [np.asarray(_icc.curve_decode(c, probe)) for c in curves]
    for transfer in ("linear", "srgb", "adobe-rgb", "prophoto", "gamma1.8"):
        expected = _metrics._decode_transfer(probe.astype(np.float32), transfer)
        bound = np.where(probe < 0.05, 0.25 * expected + 1e-5, TRC_TOLERANCE)
        if all(np.all(np.abs(d - expected) <= bound) for d in decoded):
            return transfer
    return None


def _bradford(src, dst):
    """Bradford adaptation from XYZ white `src` to `dst` (`metrics`' matrices)."""
    m = _metrics
    s, d = m._mat_vec(m.BRADFORD, src), m._mat_vec(m.BRADFORD, dst)
    ratio = tuple(tuple(d[i] / s[i] if i == j else 0.0 for j in range(3))
                  for i in range(3))
    return [list(r) for r in m._mat_mul(m._inverse(m.BRADFORD),
                                        m._mat_mul(ratio, m.BRADFORD))]


def _primaries_of(profile: dict) -> tuple[str, str] | None:
    """The `metrics` primaries and white the profile's colorants are, or `None`.

    A v2 profile has no `chad`: its colorants are Bradford-adapted to D50, so they
    are carried back to the media white (`wtpt`) here.
    """
    if profile.get("chad") is None and profile.get("white"):
        wtpt = [v / profile["white"][1] for v in profile["white"]]
        d50 = _metrics._xyz_of(_metrics.WHITE["d50"])
        if max(abs(a - b) for a, b in zip(wtpt, d50)) > 1e-3:
            profile = {**profile, "chad": _bradford(wtpt, d50)}
    xy = _icc.primaries_xy(profile)

    def near(a, b):
        return abs(a[0] - b[0]) <= XY_TOLERANCE and abs(a[1] - b[1]) <= XY_TOLERANCE

    for white_name, white in _metrics.WHITE.items():
        if not near(xy["white"], white):
            continue
        for name, prims in _metrics.PRIMARIES.items():
            if all(near(xy[c], p) for c, p in zip(("red", "green", "blue"), prims)):
                return name, white_name
    return None


def profile_space(raw: bytes) -> str:
    """The `metrics` space an embedded ICC profile defines.

    Raises with what the profile *is* when it matches none of `EXPORT_SPACES`.
    """
    try:
        profile = _icc.parse(raw)
        if profile["colorants"] is None or profile["trc"] is None:
            raise _icc.IccError("not a matrix/TRC RGB profile")
        prims = _primaries_of(profile)
        transfer = _transfer_of(profile["trc"])
    except Exception as error:  # a malformed profile can fail anywhere in the parser
        raise ReferenceError(f"unreadable ICC profile: {type(error).__name__}: "
                             f"{error}") from error
    for name in EXPORT_SPACES:
        space = _metrics.SPACES[name]
        if prims == (space.primaries, space.white) and transfer == space.transfer:
            return name
    raise ReferenceError(
        f"ICC profile {profile.get('description')!r} (primaries {prims}, transfer "
        f"{transfer}) is none of {', '.join(EXPORT_SPACES)}")


def resolve_space(embedded: bytes | None, declared: str | None) -> tuple[str, str]:
    """The export's space and where it came from. The profile wins; a declaration
    that contradicts it is refused rather than obeyed."""
    if embedded is None:
        if declared is None:
            raise ReferenceError("the file has no ICC profile; declare the reference's "
                                 "`space` in the matrix")
        return declared, "declared by the matrix"
    found = profile_space(embedded)
    if declared is not None and declared != found:
        raise ReferenceError(f"the matrix declares {declared}, but the file's ICC "
                             f"profile is {found}")
    return found, "embedded ICC profile"


# -- reading and converting ----------------------------------------------------


def read_export(path: Path, pixels: bool = True):
    """An export's ICC profile bytes and, with `pixels`, its first image as
    encoded float32 (else `None`).

    Extra pages are accepted only as reduced-resolution previews (SilverFast writes
    one); anything else is a layout this does not know.
    """
    import numpy as np
    import tifffile

    try:
        with tifffile.TiffFile(str(path)) as handle:
            page, *rest = handle.pages
            if any(not (int(p.subfiletype) & 1) for p in rest):
                raise ReferenceError(f"{len(handle.pages)} full-resolution TIFF pages")
            if int(page.photometric) != int(tifffile.PHOTOMETRIC.RGB):
                raise ReferenceError(f"photometric {page.photometric.name}, not RGB")
            if int(page.planarconfig) != int(tifffile.PLANARCONFIG.CONTIG):
                raise ReferenceError("planar (separate-plane) samples, not contiguous")
            tag = page.tags.get(34675)
            embedded = bytes(tag.value) if tag is not None else None
            orientation = page.tags.get(274)
            orientation = int(orientation.value) if orientation is not None else 1
            array = page.asarray() if pixels else None
    except ReferenceError:
        raise
    except Exception as error:
        raise ReferenceError(f"cannot read as TIFF ({type(error).__name__}: "
                             f"{error})") from error
    if array is None:
        return embedded, None
    if array.ndim != 3 or array.shape[2] < 3:
        raise ReferenceError(f"expected an RGB image, got shape {array.shape}")
    array = orient(array[:, :, :3], orientation)
    if array.dtype.kind == "u":
        encoded = array.astype(np.float32) / np.float32(np.iinfo(array.dtype).max)
    elif array.dtype.kind == "f":
        encoded = array.astype(np.float32)
    else:
        raise ReferenceError(f"unsupported sample type {array.dtype}")
    return embedded, encoded


def orient(array, orientation: int):
    """Apply a TIFF Orientation tag (1-8), which `asarray` leaves to the reader."""
    import numpy as np

    steps = {1: lambda a: a, 2: np.fliplr, 3: lambda a: np.rot90(a, 2), 4: np.flipud,
             5: lambda a: a.transpose(1, 0, 2), 6: lambda a: np.rot90(a, -1),
             7: lambda a: np.rot90(a, 2).transpose(1, 0, 2),
             8: lambda a: np.rot90(a, 1)}
    if orientation not in steps:
        raise ReferenceError(f"unknown TIFF orientation {orientation}")
    return np.ascontiguousarray(steps[orientation](array))


def to_srgb(encoded, space_name: str):
    """Encoded samples in `space_name` → 8-bit sRGB, the fraction of pixels with
    a channel outside the sRGB gamut or range (clipped, never compressed), and the
    fraction with a non-finite channel (written black)."""
    import numpy as np

    space = _metrics.SPACES[space_name]
    srgb = _metrics.SPACES["srgb"]
    matrix = _metrics._mat_mul(_metrics._inverse(_metrics.rgb_to_xyz_d65(srgb)),
                               _metrics.rgb_to_xyz_d65(space))
    linear = _metrics._decode_transfer(encoded, space.transfer)
    out = linear @ np.asarray(matrix, dtype=np.float32).T
    broken = ~np.all(np.isfinite(out), axis=2)
    out[broken] = 0.0
    nonfinite = float(np.count_nonzero(broken)) / broken.size
    outside = np.any((out < 0.0) | (out > 1.0), axis=2)
    clipped = float(np.count_nonzero(outside)) / outside.size
    np.clip(out, 0.0, 1.0, out=out)
    toe = out <= np.float32(0.0031308)
    low = out[toe] * np.float32(12.92)
    np.power(out, np.float32(1 / 2.4), out=out)
    out *= np.float32(1.055)
    out -= np.float32(0.055)
    out[toe] = low
    out *= np.float32(255.0)
    out += np.float32(0.5)
    return out.astype(np.uint8), clipped, nonfinite


def write_jpeg(pixels, path: Path) -> None:
    """An sRGB JPEG with an embedded sRGB profile, so a browser shows it as sRGB."""
    from PIL import Image, ImageCms

    profile = ImageCms.ImageCmsProfile(ImageCms.createProfile("sRGB")).tobytes()
    tmp = path.with_name(f".{path.name}.tmp")
    Image.fromarray(pixels, "RGB").save(tmp, "JPEG", quality=JPEG_QUALITY,
                                        subsampling=0, icc_profile=profile)
    tmp.replace(path)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as stream:
        for block in iter(lambda: stream.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def pillow_version() -> str:
    import PIL

    return PIL.__version__


def is_converted(stored: dict, source_digest: str, space: str, jpeg: Path) -> bool:
    """Whether a stored conversion record still describes this source and output."""
    return (stored.get("converter") == CONVERTER
            and stored.get("source_sha256") == source_digest
            and stored.get("space") == space
            and stored.get("pillow") == pillow_version()
            and jpeg.is_file() and stored.get("output_sha256") == sha256(jpeg))


def convert(source: Path, jpeg: Path, declared: str | None, stored: dict,
            expected_digest: str | None) -> dict:
    """Bring one export to the cell JPEG, reusing a conversion that still holds.

    Returns the conversion record written beside the JPEG.
    """
    try:
        return _convert(source, jpeg, declared, stored, expected_digest)
    except OSError as error:
        raise ReferenceError(f"{type(error).__name__}: {error}") from error


def _convert(source: Path, jpeg: Path, declared: str | None, stored: dict,
             expected_digest: str | None) -> dict:
    digest = sha256(source)
    if expected_digest and digest != expected_digest:
        raise ReferenceError("the file no longer matches the manifest's sha256; "
                             "regenerate the manifest")
    embedded, _ = read_export(source, pixels=False)
    space, space_from = resolve_space(embedded, declared)
    if is_converted(stored, digest, space, jpeg):
        return stored
    _, encoded = read_export(source)
    height, width = encoded.shape[:2]
    pixels, clipped, nonfinite = to_srgb(encoded, space)
    del encoded
    write_jpeg(pixels, jpeg)
    return {
        "schema_version": 1,
        "converter": CONVERTER,
        "pillow": pillow_version(),
        "source_sha256": digest,
        "space": space,
        "space_from": space_from,
        "width": width,
        "height": height,
        "clipped_fraction": round(clipped, 6),
        "nonfinite_fraction": nonfinite,
        "output_sha256": sha256(jpeg),
    }


def producer_note(spaces: dict[tuple[str, str], int]) -> str | None:
    """The spaces a reference's cells were read in, for its `producer` block."""
    if not spaces:
        return None
    return "; ".join(f"{space} ({source}) x{count}"
                     for (space, source), count in sorted(spaces.items()))

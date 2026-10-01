"""Decode every output encoding independently and check it against what nc meant to
write — the harness `analysis/display-output-acceptance` runs on real scans.

`python -m nctool acceptance run` converts each case of `acceptance.json` with
`--export-pre-encode`, which writes the buffers the destination's encoder received
(the **canonical** buffers). Each output is then decoded without nc — the ICC
profile read by `nctool.icc`, BT.2100 by `nctool.rec2100`, the gain map by
`nctool.gainmap`, JPEG by Pillow's libjpeg — and compared with the
canonical buffers by its encoding's **oracle**. The result is a JSON record of
measured maxima, RMS, metadata diffs, decoder identity and pass/fail.

**The cases are the benchmark's** (`benchmark.json`'s `fixtures` set, destination
blocks only), so there is one case list; the manifest only re-targets them at other
inputs (the synthetic chart, `nctool.chart`) and states what each encoding must be.

**What is pinned, and where.** Bounds and expected signalling are per encoding, in
the manifest. A canonical buffer and an encoded file are checksummed into the result
but never compared against a committed checksum: decode pixels differ by target
(design-spec §8), so the fixtures run regenerates both in the same run. `--golden`
pins them for a single-machine run (the real-scan acceptance), and
`--write-golden` records an update with the old and new values.

**The gain map is checked at its own resolution.** The map is half resolution, so
no full-resolution reconstruction can match the HDR rendition pixel for pixel.
The gate is the map's gains — decoded from the file's metadata and the pre-JPEG
codes — against the gains this module derives from the two renditions, resampled to
the map grid; JPEG loss is bounded separately, and the end-to-end reconstruction
error is reported.

Exit codes: `0` every check passed; `1` a check failed or a case would not convert;
`2` usage or an operational failure.
"""

from __future__ import annotations

import hashlib
import io
import json
import math
import os
import struct
import subprocess
import sys
import tempfile

from . import chart as _chart
from . import compare as _compare
from . import gainmap as _gainmap
from . import icc as _icc
from . import manifest as _manifest

MANIFEST = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
                        "acceptance.json")
MANIFEST_SCHEMA = 1
RESULT_SCHEMA = 1
GOLDEN_SCHEMA = 1

# CIE 1931 xy of each standard's primaries and white, from the standards themselves
# (IEC 61966-2-1, SMPTE EG 432-1, Adobe RGB (1998), Rec. ITU-R BT.2020, SMPTE ST 2065-1
# / S-2014-004) — never from nc's colorimetry, which is what is being checked.
D65 = (0.3127, 0.3290)
STANDARDS = {
    "srgb": dict(red=(0.64, 0.33), green=(0.30, 0.60), blue=(0.15, 0.06), white=D65),
    "display-p3": dict(red=(0.680, 0.320), green=(0.265, 0.690), blue=(0.150, 0.060),
                       white=D65),
    "adobe-rgb": dict(red=(0.64, 0.33), green=(0.21, 0.71), blue=(0.15, 0.06), white=D65),
    "bt2020": dict(red=(0.708, 0.292), green=(0.170, 0.797), blue=(0.131, 0.046),
                   white=D65),
    "acescg": dict(red=(0.713, 0.293), green=(0.165, 0.830), blue=(0.128, 0.044),
                   white=(0.32168, 0.33767)),
}
# The display transfer each SDR gamut is encoded with.
SRGB_CURVE = dict(kind="para", function=3,
                  params=dict(g=2.4, a=1 / 1.055, b=0.055 / 1.055, c=1 / 12.92, d=0.04045))
ADOBE_CURVE = dict(kind="para", function=0, params=dict(g=563 / 256))
LINEAR_CURVE = dict(kind="para", function=0, params=dict(g=1.0))
SDR_CURVES = {"srgb": SRGB_CURVE, "display-p3": SRGB_CURVE, "adobe-rgb": ADOBE_CURVE}
# The `cicp` code points of a Rec.2100 signal (H.273).
CICP_TRANSFER = {"pq": 16, "hlg": 18}
# The ICC profile connection space's illuminant (ICC.1 §7.2.16) and the Bradford cone
# matrix that adapts D65 to it — what a profile's PCS values are expressed in.
PCS_D50 = (0.9642, 1.0, 0.8249)
BRADFORD = ((0.8951, 0.2664, -0.1614), (-0.7502, 1.7135, 0.0367), (0.0389, -0.0685, 1.0296))
# A PCSXYZ value stored as [0, 1] spans XYZ [0, 1 + 32767/32768] (ICC.1 §6.3.4.2).
PCS_XYZ_SCALE = 1 + 32767 / 32768
# TIFF facts every output has: RGB, interleaved, uncompressed, one image.
TIFF_PHOTOMETRIC_RGB = 2
TIFF_PLANAR_CONTIG = 1
TIFF_UNCOMPRESSED = 1
# Tolerances for values a profile stores as s15Fixed16.
XY_TOLERANCE = 1e-3
CURVE_TOLERANCE = 1e-4

# Every encoding an oracle exists for; a destination that maps to none fails loudly.
ENCODINGS = ("film-master", "sdr-tiff", "hdr-linear-tiff", "hdr-pq-tiff",
             "hdr-hlg-tiff", "gain-map-jpeg")
DETERMINISM = ("byte-identical", "decoded")


class AcceptanceError(Exception):
    """An operational failure: the run cannot produce a verdict (exit 2)."""


class ConvertFailed(AcceptanceError):
    """A case would not convert (exit 1): a fault in what is being accepted."""


def _np():
    import numpy as np
    return np


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: str) -> str:
    with open(path, "rb") as f:
        return sha256_bytes(f.read())


# ---------------------------------------------------------------------------
# The manifest and its cases
# ---------------------------------------------------------------------------

def load_manifest(path: str) -> dict:
    with open(path) as f:
        man = json.load(f)
    if man.get("schema_version") != MANIFEST_SCHEMA:
        raise AcceptanceError(f"{path}: schema_version {man.get('schema_version')!r} "
                              f"is not {MANIFEST_SCHEMA}")
    encodings = man.get("encodings", {})
    missing = sorted(set(ENCODINGS) - set(encodings))
    if missing:
        raise AcceptanceError(f"{path}: no `encodings` entry for {', '.join(missing)}")
    unknown = sorted(set(encodings) - set(ENCODINGS))
    if unknown:
        raise AcceptanceError(f"{path}: no oracle for encoding(s) {', '.join(unknown)}")
    for name, spec in encodings.items():
        if spec.get("determinism") not in DETERMINISM:
            raise AcceptanceError(f"{path}: encodings.{name}.determinism must be one of "
                                  f"{', '.join(DETERMINISM)}")
    return man


def resolve(man: dict, benchmark_path: str) -> list[dict]:
    """The benchmark's cases (destination blocks), then their re-targeted copies."""
    bench, err = _compare.load_json(benchmark_path)
    if err:
        raise AcceptanceError(err)
    base, err = _compare.resolve_cases(bench, man["benchmark_set"], asset_root="")
    if err:
        raise AcceptanceError(err)
    cases = []
    for c in base:
        block = c["blocks"].get("destination")
        if block is None:
            continue
        cases.append(dict(name=c["name"], input=c["input"], input_name=None,
                          output_ext=c["output_ext"], recipe=block["recipe"],
                          args=c["args"] + block["args"]))
    out = list(cases)
    root = _compare.repo_root()
    for name, spec in man.get("inputs", {}).items():
        source = os.path.join(root, spec["reuse_cases_of"])
        old, new = spec["rename"]
        picked = [c for c in cases if c["input"] == source]
        if not picked:
            raise AcceptanceError(f"inputs.{name}: no benchmark case reads "
                                  f"{spec['reuse_cases_of']}")
        for c in picked:
            if not c["name"].startswith(old):
                raise AcceptanceError(f"inputs.{name}: case {c['name']!r} does not start "
                                      f"with {old!r}")
            # The source case's own args are input facts (its film base), replaced by
            # this input's; the destination block's args are kept.
            dest_args = c["args"][len(_case_args(bench, man, c["name"])):]
            out.append(dict(c, name=new + c["name"][len(old):],
                            input=os.path.join(root, spec["input"]), input_name=name,
                            args=list(spec["args"]) + dest_args))
    names = [c["name"] for c in out]
    if len(set(names)) != len(names):
        raise AcceptanceError("two acceptance cases share a name")
    return out


def _case_args(bench: dict, man: dict, name: str) -> list:
    for c in bench["sets"][man["benchmark_set"]]["cases"]:
        if c["name"] == name:
            return c.get("args", [])
    raise AcceptanceError(f"benchmark case {name!r} vanished")


def encoding_of(report: dict) -> tuple[str, str | None, str | None]:
    """`(encoding, gamut, transfer)` from the report's resolved `chain.destination`."""
    dest = (report.get("chain") or {}).get("destination")
    if dest == "film-master":
        return "film-master", "acescg", None
    d = (dest or {}).get("display") if isinstance(dest, dict) else None
    if not d:
        raise AcceptanceError(f"the report names no destination ({dest!r})")
    key = (d["range"], d["transfer"], d["container"])
    table = {
        ("sdr", "native", "tiff"): "sdr-tiff",
        ("hdr", "linear", "tiff"): "hdr-linear-tiff",
        ("hdr", "pq", "tiff"): "hdr-pq-tiff",
        ("hdr", "hlg", "tiff"): "hdr-hlg-tiff",
        ("hdr", "native", "jpeg"): "gain-map-jpeg",
    }
    if key not in table:
        raise AcceptanceError(f"no oracle for destination {d}")
    transfer = d["transfer"] if d["transfer"] in CICP_TRANSFER else None
    return table[key], d["gamut"], transfer


# ---------------------------------------------------------------------------
# Running nc
# ---------------------------------------------------------------------------

def convert(nc: str, case: dict, workdir: str, tag: str) -> dict:
    """Convert one case with `--export-pre-encode`; its report and file paths."""
    out = os.path.join(workdir, f"{case['name']}.{tag}.{case['output_ext']}")
    pre = os.path.join(workdir, f"{case['name']}.{tag}.pre.tiff")
    argv = [nc, "convert", case["input"], "-o", out, "--export-pre-encode", pre]
    if case.get("recipe"):
        argv += ["--params", case["recipe"]]
    argv += case["args"]
    proc = subprocess.run(argv, capture_output=True, text=True)
    if proc.returncode != 0:
        raise ConvertFailed(f"case {case['name']!r}: hanten exited {proc.returncode}\n"
                            f"{proc.stderr.strip()}")
    try:
        report = json.loads(proc.stdout)
    except json.JSONDecodeError as e:
        raise ConvertFailed(f"case {case['name']!r}: hanten's stdout is not JSON ({e})")
    if report.get("pre_encode_exported") != pre:
        raise ConvertFailed(f"case {case['name']!r}: the report does not name the "
                            "pre-encode export")
    return dict(report=report, output=out, pre_encode=pre)


def preflight(nc: str) -> None:
    """Refuse a binary that is not hanten, or one with no `--export-pre-encode` (any
    build before this harness, the reference build included), before converting."""
    if not _manifest.is_nc(nc):
        raise AcceptanceError(f"{nc} is not a hanten binary (no hanten/nc --version banner)")
    proc = subprocess.run([nc, "convert", "--help"], capture_output=True, text=True)
    if "--export-pre-encode" not in proc.stdout:
        raise AcceptanceError(f"{nc} has no `convert --export-pre-encode`, so there is no "
                              "canonical buffer to check against; build a newer hanten")


def read_pre_encode(path: str) -> dict:
    """`{buffer: array}` from a `--export-pre-encode` file, page order kept in `_order`."""
    import tifffile
    pages = {}
    order = []
    with tifffile.TiffFile(path) as t:
        for p in t.pages:
            desc = json.loads(p.description)
            if desc.get("nc_pre_encode") != 1:
                raise AcceptanceError(f"{path}: a page is not an nc pre-encode page")
            pages[desc["buffer"]] = p.asarray()
            order.append((desc["buffer"], desc["space"]))
    pages["_order"] = order
    return pages


# ---------------------------------------------------------------------------
# Measurements
# ---------------------------------------------------------------------------

def diff_stats(found, reference) -> dict:
    """Per-channel max |Δ|, RMS, mean signed Δ and 99th percentile |Δ| over the samples
    finite in both; `non_finite_mismatch` counts samples finite in only one."""
    np = _np()
    f = np.asarray(found, dtype=np.float64)
    r = np.asarray(reference, dtype=np.float64)
    if f.ndim == 2:
        f, r = f[..., None], r[..., None]
    f, r = f.reshape(-1, f.shape[-1]), r.reshape(-1, r.shape[-1])
    finite = np.isfinite(f) & np.isfinite(r)
    d = np.where(finite, f - r, 0.0)
    a = np.abs(d)
    return dict(max=a.max(axis=0).tolist(), rms=np.sqrt((d ** 2).mean(axis=0)).tolist(),
                mean=d.mean(axis=0).tolist(),
                p99=np.percentile(a, 99, axis=0).tolist(),
                nonzero_fraction=float((a > 0).any(axis=1).mean()),
                non_finite_mismatch=int((np.isfinite(f) != np.isfinite(r)).sum()))


def _check(name: str, ok: bool, **fields) -> dict:
    return dict(check=name, passed=bool(ok), **fields)


def _within(stats: dict, bounds: dict) -> tuple[bool, list[str]]:
    """Compare `stats` against `bounds` (`max`, `rms`: scalars or per-channel lists)."""
    fails = []
    if stats.get("non_finite_mismatch"):
        fails.append(f"{stats['non_finite_mismatch']} sample(s) finite in only one of the two")
    for key in ("max", "rms"):
        if key not in bounds:
            continue
        limit = bounds[key]
        limits = limit if isinstance(limit, list) else [limit] * len(stats[key])
        for c, (v, lim) in enumerate(zip(stats[key], limits)):
            if not v <= lim:
                fails.append(f"{key}[{c}] = {v:.6g} > {lim}")
    return not fails, fails


def metadata_check(items: list[tuple[str, object, object, bool]]) -> dict:
    """A `metadata` check from `(field, expected, found, ok)` rows."""
    diffs = [dict(field=f, expected=e, found=v) for f, e, v, ok in items if not ok]
    return _check("metadata", not diffs, fields={f: v for f, _, v, _ in items},
                  diffs=diffs)


def _close(a, b, tol) -> bool:
    if a is None or b is None:
        return False
    if isinstance(a, (list, tuple)):
        return len(a) == len(b) and all(_close(x, y, tol) for x, y in zip(a, b))
    return abs(a - b) <= tol


def profile_rows(profile: dict | None, space: str, curve: dict | None) -> list:
    """Metadata rows for a matrix/TRC profile: primaries, white, and every TRC."""
    if profile is None:
        return [("icc", "present", None, False)]
    rows = [("icc.colour_space", "RGB ", profile["colour_space"],
             profile["colour_space"] == "RGB ")]
    try:
        found = _icc.primaries_xy(profile)
    except _icc.IccError as e:
        return rows + [("icc.primaries", space, str(e), False)]
    for k, xy in STANDARDS[space].items():
        rows.append((f"icc.{k}", list(xy), [round(v, 5) for v in found[k]],
                     _close(found[k], xy, XY_TOLERANCE)))
    if curve is not None:
        for c, trc in enumerate(profile["trc"] or [None] * 3):
            ok = (trc is not None and trc["kind"] == "para"
                  and trc["function"] == curve["function"]
                  and set(trc["params"]) == set(curve["params"])
                  and all(abs(trc["params"][k] - v) <= CURVE_TOLERANCE
                          for k, v in curve["params"].items()))
            rows.append((f"icc.trc[{c}]", curve["params"],
                         trc and trc.get("params"), ok))
    return rows


def tiff_rows(tiff: dict, dtype: str, shape: list) -> list:
    """Metadata rows every TIFF output shares: its sample type, size and layout."""
    return [("tiff.dtype", dtype, tiff["dtype"], tiff["dtype"] == dtype),
            ("tiff.shape", shape, tiff["shape"], tiff["shape"] == shape),
            ("tiff.photometric", TIFF_PHOTOMETRIC_RGB, tiff["photometric"],
             tiff["photometric"] == TIFF_PHOTOMETRIC_RGB),
            ("tiff.planar", TIFF_PLANAR_CONTIG, tiff["planar"],
             tiff["planar"] == TIFF_PLANAR_CONTIG),
            ("tiff.compression", TIFF_UNCOMPRESSED, tiff["compression"],
             tiff["compression"] == TIFF_UNCOMPRESSED),
            ("tiff.pages", 1, tiff["pages"], tiff["pages"] == 1)]


def _tiff(path: str):
    import tifffile
    with tifffile.TiffFile(path) as t:
        page = t.pages[0]
        tags = page.tags
        icc_raw = tags["InterColorProfile"].value if "InterColorProfile" in tags else None
        facts = dict(dtype=str(page.dtype), shape=list(page.shape),
                     photometric=int(page.photometric), planar=int(page.planarconfig),
                     compression=int(page.compression), pages=len(t.pages))
        return page.asarray(), (_icc.parse(icc_raw) if icc_raw else None), facts


def quantize(values, bits: int):
    """Clamp to [0, 1], scale to `2^bits − 1` and round half away from zero."""
    np = _np()
    from . import rec2100
    return rec2100.round_half_away(np.clip(values, 0, 1) * ((1 << bits) - 1))


def encode_curve(curve: dict, linear):
    np = _np()
    lin = np.asarray(linear, dtype=np.float64)
    return np.stack([_icc.curve_encode(curve, lin[..., c]) for c in range(3)], axis=-1)


def decode_curve(curve: dict, encoded):
    np = _np()
    e = np.asarray(encoded, dtype=np.float64)
    return np.stack([_icc.curve_decode(curve, e[..., c]) for c in range(3)], axis=-1)


# ---------------------------------------------------------------------------
# The oracles: (output path, canonical buffers, case facts, spec) -> checks
# ---------------------------------------------------------------------------

def oracle_lossless_float(path, pre, facts, spec, patches) -> list[dict]:
    """Film master and HDR linear float TIFF: the file is the canonical buffer."""
    np = _np()
    pixels, profile, tiff = _tiff(path)
    buffer = "film-master" if facts["encoding"] == "film-master" else "hdr-linear"
    canonical = pre[buffer]
    rows = tiff_rows(tiff, "float32", list(canonical.shape))
    rows += profile_rows(profile, facts["gamut"], LINEAR_CURVE)
    if facts["encoding"] == "hdr-linear-tiff":
        rows.append(("icc.cicp", None, profile and profile["cicp"],
                     profile is not None and profile["cicp"] is None))
    checks = [metadata_check(rows)]
    if pixels.shape != canonical.shape:
        return checks + [_check("pixels", False, error="shape differs from canonical")]
    stats = diff_stats(pixels, canonical)
    stats["bit_identical"] = bool(np.array_equal(pixels.view(np.uint32),
                                                 canonical.astype(np.float32).view(np.uint32)))
    ok, fails = _within(stats, spec["bounds"])
    # A zero bound cannot see a flipped sign of zero or a rewritten NaN; bit identity can.
    if spec["bounds"].get("bit_identical") and not stats["bit_identical"]:
        ok, fails = False, fails + ["the samples are not bit-identical"]
    checks.append(_check("pixels", ok, metrics=stats, bounds=spec["bounds"], fails=fails))
    return checks


def oracle_sdr_tiff(path, pre, facts, spec, patches) -> list[dict]:
    """16-bit SDR TIFF: the standard transfer applied to the canonical linear buffer
    and quantized, against the stored codes."""
    np = _np()
    codes, profile, tiff = _tiff(path)
    canonical = pre["sdr-linear"].astype(np.float64)
    curve = SDR_CURVES[facts["gamut"]]
    rows = tiff_rows(tiff, "uint16", list(canonical.shape))
    rows += profile_rows(profile, facts["gamut"], curve)
    checks = [metadata_check(rows)]
    if codes.shape != canonical.shape:
        return checks + [_check("pixels", False, error="shape differs from canonical")]
    encoded = encode_curve(curve, canonical)
    reference = quantize(encoded, 16)
    stats = diff_stats(codes, reference)
    stats["clipped"] = dict(low=int((encoded < 0).sum()), high=int((encoded > 1).sum()))
    stats["reported_loss"] = facts["report"].get("loss")
    ok, fails = _within(stats, spec["bounds"])
    checks.append(_check("pixels", ok, metrics=stats, bounds=spec["bounds"], fails=fails,
                         units="16-bit codes"))
    return checks


def _signal(linear, transfer: str, hdr: dict):
    from . import rec2100
    if transfer == "pq":
        return rec2100.pq_signal(linear, hdr["reference_white_nits"])
    return rec2100.hlg_signal(linear, hdr["reference_white_nits"], hdr["peak_nits"],
                              hdr["hlg_system_gamma"])


def _rgb_to_pcs(space: str):
    """RGB → XYZ in the PCS's D50 white, by the Bradford adaptation from D65."""
    np = _np()
    b = np.asarray(BRADFORD)
    m = _xy_matrix(space)
    d65 = m @ np.ones(3)
    cat = np.linalg.inv(b) @ np.diag((b @ np.asarray(PCS_D50)) / (b @ d65)) @ b
    return cat @ m


def lut_check(profile: dict | None, facts: dict, bounds: dict) -> dict:
    """The coded-HDR profile's `A2B0`, the path a CICP-unaware ICC reader decodes
    through, evaluated on a grid of codes against BT.2100: PQ to `Y = L / 203 cd/m²`,
    HLG (scene-referred) to scene light with reference white at `Y = 1`."""
    np = _np()
    from . import rec2100
    if not profile or not profile.get("a2b0"):
        return _check("icc_lut", False, error="the profile has no A2B0")
    levels = np.linspace(0, 1, 9)
    grid = np.stack(np.meshgrid(levels, levels, levels, indexing="ij"), -1).reshape(-1, 3)
    device = np.concatenate([np.repeat(np.linspace(0, 1, 257)[:, None], 3, 1), grid])
    try:
        got = _icc.eval_lut_atob(profile["a2b0"], device) * PCS_XYZ_SCALE
    except (_icc.IccError, struct.error) as e:
        return _check("icc_lut", False, error=str(e))
    hdr = facts["hdr"]
    if facts["transfer"] == "pq":
        light = rec2100.pq_eotf(device) / hdr["reference_white_nits"]
    else:
        white = rec2100.hlg_signal(np.ones(3), hdr["reference_white_nits"], hdr["peak_nits"],
                                   hdr["hlg_system_gamma"])
        light = rec2100.hlg_inverse_oetf(device) / rec2100.hlg_inverse_oetf(white)[0]
    # PCSXYZ holds no negative component; a saturated red's adapted Z is one.
    want = np.maximum(light @ _rgb_to_pcs("bt2020").T, 0)
    excess = np.abs(got - want) / np.maximum(bounds["absolute"],
                                             bounds["relative"] * np.abs(want))
    worst = float(excess.max())
    return _check("icc_lut", worst <= 1.0, metrics=dict(
        worst_over_bound=worst, max_absolute=float(np.abs(got - want).max())),
        bounds=bounds, samples=len(device))


def oracle_coded_tiff(path, pre, facts, spec, patches) -> list[dict]:
    """PQ/HLG 16-bit TIFF: BT.2100 applied to the canonical linear BT.2020 buffer in
    binary64 and quantized, against the stored codes."""
    np = _np()
    codes, profile, tiff = _tiff(path)
    canonical = pre["hdr-linear"].astype(np.float64)
    want_cicp = [9, CICP_TRANSFER[facts["transfer"]], 0, 1]
    rows = tiff_rows(tiff, "uint16", list(canonical.shape))
    rows += [("icc.cicp", want_cicp, profile and profile["cicp"],
              profile is not None and profile["cicp"] == want_cicp)]
    checks = [metadata_check(rows), lut_check(profile, facts, spec["bounds"]["lut"])]
    if codes.shape != canonical.shape:
        return checks + [_check("pixels", False, error="shape differs from canonical")]
    reference = quantize(_signal(canonical, facts["transfer"], facts["hdr"]), 16)
    stats = diff_stats(codes, reference)
    ok, fails = _within(stats, spec["bounds"])
    checks.append(_check("pixels", ok, metrics=stats, bounds=spec["bounds"], fails=fails,
                         units="16-bit codes"))
    return checks


def _patch_means(img, patches: dict, kinds: tuple, inset: int):
    np = _np()
    out = {}
    for name, r in patches.items():
        if r["kind"] not in kinds:
            continue
        cut = img[r["y"] + inset:r["y"] + r["h"] - inset, r["x"] + inset:r["x"] + r["w"] - inset]
        out[name] = cut.reshape(-1, cut.shape[-1]).astype(np.float64).mean(axis=0)
    return out


def patch_checks(decoded, reference, patches: dict | None, bounds: dict, inset: int,
                 chroma) -> list[dict]:
    """The ramp and saturated-patch bounds of a lossy encoding: each patch's mean
    error, and on the neutral ramp the codec's chroma shift (`chroma` maps a mean
    error triple to its chroma part)."""
    if not patches or "patches" not in bounds:
        return []
    np = _np()
    b = bounds["patches"]
    dec = _patch_means(decoded, patches, ("neutral", "saturated"), inset)
    ref = _patch_means(reference, patches, ("neutral", "saturated"), inset)
    rows = {}
    fails = []
    for name in dec:
        err = dec[name] - ref[name]
        kind = patches[name]["kind"]
        mean_err = float(np.abs(err).max())
        row = dict(kind=kind, mean_error=mean_err)
        limit = b["neutral_mean"] if kind == "neutral" else b["saturated_mean"]
        if mean_err > limit:
            fails.append(f"{name}: mean error {mean_err:.4g} > {limit}")
        if kind == "neutral":
            shift = float(chroma(err))
            row["chroma_shift"] = shift
            if shift > b["neutral_chroma"]:
                fails.append(f"{name}: chroma shift {shift:.4g} > {b['neutral_chroma']}")
        rows[name] = row
    return [_check("patches", not fails, patches=rows, bounds=b, fails=fails)]


def _rgb_chroma(err):
    np = _np()
    return np.abs(err - err.mean()).max()


def _jpeg_rgb(data: bytes):
    np = _np()
    from PIL import Image
    with Image.open(io.BytesIO(data)) as im:
        if im.mode != "RGB":
            raise AcceptanceError(f"a JPEG decoded as {im.mode}, not RGB")
        return np.asarray(im).astype(np.float64)


def oracle_gain_map(path, pre, facts, spec, patches) -> list[dict]:
    """ISO 21496-1 gain-map JPEG: metadata against the renditions, the map's gains at
    its own resolution, both JPEGs' codec loss, and the end-to-end reconstruction
    (reported)."""
    np = _np()
    with open(path, "rb") as f:
        data = f.read()
    sdr = pre["sdr-linear"].astype(np.float64)
    hdr = pre["hdr-linear"].astype(np.float64)
    codes = pre["gain-map-codes"].astype(np.float64)
    h, w = sdr.shape[:2]
    g = facts["gain_map"]
    offset = g["offset"]
    try:
        parsed = _gainmap.read(data)
    except _gainmap.GainMapError as e:
        return [_check("metadata", False, diffs=[dict(field="container", expected="ISO "
                                                      "21496-1 gain-map JPEG", found=str(e))])]
    md = parsed["metadata"]
    profile = _icc.parse(parsed["base_icc"]) if parsed["base_icc"] else None
    curve = SDR_CURVES[facts["gamut"]]
    # The ratio is taken on the SDR base as stored (clamped to white). A non-finite or a
    # clearly negative sample is a fault; fit gamut may leave an ulp below zero where
    # two channels tie on the cube's black face, which the encoder clamps by design.
    floor = -g["negative_tolerance"]
    bad = int((~np.isfinite(sdr)).sum() + (~np.isfinite(hdr)).sum()
              + (sdr < floor).sum() + (hdr < floor).sum())
    gains = np.log2((np.maximum(hdr, 0) + offset) / (np.clip(sdr, 0, 1) + offset))
    flat = gains.reshape(-1, 3)
    headroom = math.log2(facts["hdr"]["peak_nits"] / facts["hdr"]["reference_white_nits"])
    entries = parsed["entries"]
    rows = [("renditions.invalid_samples", 0, bad, bad == 0),
            ("mpf.types", g["mpf_types"], [e["type"] for e in entries],
             [e["type"] for e in entries] == g["mpf_types"]),
            ("base.iso_version", [[0, 0]],
             [[m["minimum_version"], m["writer_version"]] for m in parsed["base_iso"]],
             [m.get("version_only") for m in parsed["base_iso"]] == [True]
             and [[m["minimum_version"], m["writer_version"]]
                  for m in parsed["base_iso"]] == [[0, 0]]),
            ("gain_map.icc", None, parsed["gain_map_icc"] and "present",
             parsed["gain_map_icc"] is None),
            ("iso.channels", 3, md["channels"], md["channels"] == 3),
            ("iso.use_base_colour_space", True, md["use_base_colour_space"],
             md["use_base_colour_space"] is True),
            ("iso.backward_direction", False, md["backward_direction"],
             md["backward_direction"] is False),
            ("iso.base_hdr_headroom", 0.0, md["base_hdr_headroom"],
             _close(md["base_hdr_headroom"], 0.0, g["rational_tolerance"])),
            ("iso.alternate_hdr_headroom", headroom, md["alternate_hdr_headroom"],
             _close(md["alternate_hdr_headroom"], headroom, g["rational_tolerance"])),
            ("iso.gamma", [1.0] * 3, md["gamma"], _close(md["gamma"], [1.0] * 3,
                                                          g["rational_tolerance"])),
            ("iso.base_offset", [offset] * 3, md["base_offset"],
             _close(md["base_offset"], [offset] * 3, g["rational_tolerance"])),
            ("iso.alternate_offset", [offset] * 3, md["alternate_offset"],
             _close(md["alternate_offset"], [offset] * 3, g["rational_tolerance"]))]
    # A flat map — every gain within the manifest's tolerance of 1 — stores a zero
    # window; otherwise each channel's window is its gains' own extremes.
    if np.abs(flat).max() <= g["flat_tolerance_log2"]:
        want_min = want_max = [0.0] * 3
    else:
        want_min = flat.min(axis=0).tolist()
        want_max = flat.max(axis=0).tolist()
    rows += [("iso.gain_map_min", want_min, md["gain_map_min"],
              _close(md["gain_map_min"], want_min, g["window_tolerance"])),
             ("iso.gain_map_max", want_max, md["gain_map_max"],
              _close(md["gain_map_max"], want_max, g["window_tolerance"])),
             ("gain_map.size", [math.ceil(w / 2), math.ceil(h / 2)],
              [codes.shape[1], codes.shape[0]],
              list(codes.shape[:2]) == [math.ceil(h / 2), math.ceil(w / 2)])]
    rows += profile_rows(profile, facts["gamut"], curve)
    checks = [metadata_check(rows)]

    # The map's gains, at its own grid.
    mh, mw = codes.shape[:2]
    canonical_grid = _gainmap.upsample(gains, mh, mw)
    decoded_gains = _gainmap.log2_gain(codes / 255, md)
    step = np.where(np.asarray(md["gain_map_max"]) > np.asarray(md["gain_map_min"]),
                    (np.asarray(md["gain_map_max"]) - np.asarray(md["gain_map_min"])) / 255,
                    np.inf)
    # f32 arithmetic in the encoder against binary64 here: a log2 tolerance absorbs it
    # before the error is counted in steps.
    slack = spec["bounds"]["map_grid_log2_tolerance"]
    steps = np.maximum(np.abs(decoded_gains - canonical_grid) - slack, 0) / step
    grid = dict(max_steps=steps.reshape(-1, 3).max(axis=0).tolist(),
                rms_steps=np.sqrt((steps ** 2).reshape(-1, 3).mean(axis=0)).tolist())
    limit = spec["bounds"]["map_grid_steps"]
    fails = [f"max_steps[{c}] = {v:.6g} > {limit}" for c, v in enumerate(grid["max_steps"])
             if not v <= limit]
    checks.append(_check("gain_map_grid", not fails, metrics=grid,
                         bounds=dict(map_grid_steps=limit, map_grid_log2_tolerance=slack),
                         fails=fails,
                         units="map code steps"))

    # Codec loss: the base against the standard transfer of the canonical SDR, the map
    # against its pre-JPEG codes.
    base_ref = quantize(encode_curve(curve, sdr), 8)
    base_dec = _jpeg_rgb(data)
    map_dec = _jpeg_rgb(parsed["gain_map_jpeg"])
    if base_dec.shape != base_ref.shape or map_dec.shape != codes.shape:
        return checks + [_check("pixels", False, error="decoded size differs")]
    base_stats = diff_stats(base_dec, base_ref)
    map_stats = diff_stats(map_dec, codes)
    b = spec["bounds"]
    ok1, f1 = _within(base_stats, b["base"])
    ok2, f2 = _within(map_stats, b["map"])
    checks.append(_check("base_jpeg", ok1, metrics=base_stats, bounds=b["base"], fails=f1,
                         units="8-bit codes"))
    checks.append(_check("map_jpeg", ok2, metrics=map_stats, bounds=b["map"], fails=f2,
                         units="8-bit codes"))
    checks += patch_checks(base_dec, base_ref, patches, b["base"], facts["inset"],
                           _rgb_chroma)

    # End to end, reported: the file alone, reconstructed, against the HDR rendition.
    rec = reconstruct_hdr(base_dec, map_dec, md, curve)
    white = facts["hdr"]["reference_white_nits"]
    err_nits = np.abs(rec - hdr) * white
    allowed = np.maximum(g["reconstruction_nits"],
                         g["reconstruction_relative"] * np.abs(hdr) * white)
    rel = np.abs(rec - hdr) / np.maximum(np.abs(hdr), 1e-6)
    checks.append(_check("reconstruction", True, reported_only=True, metrics=dict(
        within_spec_bound=float((err_nits <= allowed).mean()),
        max_relative=rel.reshape(-1, 3).max(axis=0).tolist(),
        p99_relative=np.percentile(rel.reshape(-1, 3), 99, axis=0).tolist())))
    return checks


def reconstruct_hdr(base_codes, map_codes, metadata: dict, curve: dict):
    """The alternate rendition a reader rebuilds from the decoded base and map."""
    h, w = base_codes.shape[:2]
    base_linear = decode_curve(curve, base_codes / 255)
    up = _gainmap.upsample(map_codes / 255, h, w)
    return _gainmap.reconstruct(base_linear, up, metadata)


ORACLES = {
    "film-master": oracle_lossless_float,
    "sdr-tiff": oracle_sdr_tiff,
    "hdr-linear-tiff": oracle_lossless_float,
    "hdr-pq-tiff": oracle_coded_tiff,
    "hdr-hlg-tiff": oracle_coded_tiff,
    "gain-map-jpeg": oracle_gain_map,
}
# The pre-encode pages each encoding's export must hold, in order.
PAGES = {
    "film-master": ["film-master"],
    "sdr-tiff": ["sdr-linear"],
    "hdr-linear-tiff": ["hdr-linear"],
    "hdr-pq-tiff": ["hdr-linear"],
    "hdr-hlg-tiff": ["hdr-linear"],
    "gain-map-jpeg": ["sdr-linear", "hdr-linear", "gain-map-codes"],
}


# ---------------------------------------------------------------------------
# Decoding to light, for the cross-encoding comparison
# ---------------------------------------------------------------------------

def _xy_matrix(space: str):
    """RGB → XYZ for a standard's primaries, white normalized to `Y = 1`."""
    np = _np()
    s = STANDARDS[space]
    cols = np.array([[x / y, 1.0, (1 - x - y) / y] for x, y in
                     (s["red"], s["green"], s["blue"])]).T
    wx, wy = s["white"]
    white = np.array([wx / wy, 1.0, (1 - wx - wy) / wy])
    return cols * np.linalg.solve(cols, white)


def decode_light(path: str, facts: dict) -> dict:
    """`{rendition: XYZ image}` decoded from the file alone, each normalized so its
    reference white is `Y = 1`. The film master is scene-referred and not decoded."""
    np = _np()
    from . import rec2100
    enc = facts["encoding"]
    hdr = facts["hdr"]
    white = hdr["reference_white_nits"]
    if enc in ("sdr-tiff", "hdr-linear-tiff"):
        pixels, profile, _ = _tiff(path)
        if profile is None or profile["trc"] is None:
            raise _icc.IccError("no matrix/TRC profile to decode the file through")
        scale = 65535.0 if pixels.dtype.kind == "u" else 1.0
        lin = np.stack([_icc.curve_decode(profile["trc"][c], pixels[..., c] / scale)
                        for c in range(3)], axis=-1)
        rendition = "sdr" if enc == "sdr-tiff" else "hdr"
        return {rendition: lin @ np.asarray(_icc.rgb_to_xyz(profile)).T}
    if enc in ("hdr-pq-tiff", "hdr-hlg-tiff"):
        codes, _, _ = _tiff(path)
        signal = codes / 65535.0
        if facts["transfer"] == "pq":
            nits = rec2100.pq_eotf(signal)
        else:
            nits = rec2100.hlg_display_nits(signal, hdr["peak_nits"], hdr["hlg_system_gamma"])
        return {"hdr": (nits / white) @ _xy_matrix("bt2020").T}
    if enc == "gain-map-jpeg":
        with open(path, "rb") as f:
            data = f.read()
        parsed = _gainmap.read(data)
        if parsed["base_icc"] is None:
            raise _icc.IccError("the gain-map base carries no ICC profile")
        profile = _icc.parse(parsed["base_icc"])
        if profile["trc"] is None:
            raise _icc.IccError("the gain-map base's profile has no TRC")
        m = np.asarray(_icc.rgb_to_xyz(profile)).T
        base = _jpeg_rgb(data)
        curve = profile["trc"][0]
        sdr = decode_curve(curve, base / 255)
        rec = reconstruct_hdr(base, _jpeg_rgb(parsed["gain_map_jpeg"]), parsed["metadata"],
                              curve)
        return {"sdr": sdr @ m, "hdr": rec @ m}
    return {}


def cross_encoding(cases: list[dict], man: dict) -> list[dict]:
    """Per (input, rendering) group with patches: every decoded rendition's patch means
    against the reference case's, in CIELAB / u'v' on reference-white-normalized XYZ."""
    from . import cie
    np = _np()
    spec = man["cross_encoding"]
    groups: dict = {}
    for c in cases:
        if c.get("patches") and c.get("light"):
            groups.setdefault((c["input_name"], c["rendering"]), []).append(c)
    out = []
    for (input_name, rendering), members in sorted(groups.items()):
        ref_case = next((c for c in members if c["encoding"] == spec["reference"]["encoding"]
                         and c["gamut"] == spec["reference"]["gamut"]), None)
        if ref_case is None:
            out.append(dict(input=input_name, rendering=rendering, passed=True,
                            skipped="no reference case in this group", rows=[], fails=[]))
            continue
        patches = members[0]["patches"]
        inset = members[0]["inset"]
        untouched = set(spec["untouched"])
        ref = {n: v for n, v in _patch_means(ref_case["light"]["hdr"], patches,
                                              ("neutral", "saturated", "muted"), inset).items()}
        ref_lab = {n: cie.xyz_to_lab(v) for n, v in ref.items()}
        rows = []
        fails = []
        for c in members:
            bounds = spec.get("allowances", {}).get(c["encoding"], spec["bounds"])
            for rendition, xyz in c["light"].items():
                if c is ref_case and rendition == "hdr":
                    continue
                means = _patch_means(xyz, patches, ("neutral", "saturated", "muted"), inset)
                for name, v in means.items():
                    lab = cie.xyz_to_lab(v)
                    row = dict(case=c["name"], rendition=rendition, patch=name,
                               delta_e_2000=float(cie.delta_e_2000(lab, ref_lab[name])))
                    if name in untouched:
                        if row["delta_e_2000"] > bounds["delta_e_2000"]:
                            fails.append(f"{c['name']} {rendition} {name}: ΔE00 "
                                         f"{row['delta_e_2000']:.4f}")
                        if patches[name]["kind"] == "neutral":
                            try:
                                duv = float(np.hypot(*(cie.uv_prime(v)
                                                       - cie.uv_prime(ref[name]))))
                            except ValueError as e:
                                fails.append(f"{c['name']} {rendition} {name}: {e}")
                                row["delta_uv"] = None
                                rows.append(row)
                                continue
                            row["delta_uv"] = duv
                            if duv > bounds["neutral_delta_uv"]:
                                fails.append(f"{c['name']} {rendition} {name}: Δu'v' "
                                             f"{duv:.6f}")
                    else:
                        chroma_ref = float(np.hypot(ref_lab[name][1], ref_lab[name][2]))
                        dh = float((cie.hue_angle(lab) - cie.hue_angle(ref_lab[name]) + 180)
                                   % 360 - 180)
                        row.update(mapped=True, hue_delta=dh,
                                   chroma_ratio=float(np.hypot(lab[1], lab[2])) /
                                   chroma_ref if chroma_ref else None)
                    rows.append(row)
        out.append(dict(input=input_name, rendering=rendering, reference=ref_case["name"],
                        passed=not fails, fails=fails, bounds=spec["bounds"],
                        allowances=spec.get("allowances", {}), rows=rows))
    return out


# ---------------------------------------------------------------------------
# The run
# ---------------------------------------------------------------------------

def _summary(checks: list[dict]) -> dict:
    """The numbers a golden keeps per check: its metrics' maxima and RMS."""
    out = {}
    for c in checks:
        m = c.get("metrics") or {}
        out[c["check"]] = {k: m[k] for k in ("max", "rms", "max_steps") if k in m}
    return out


def determinism_check(first: dict, second: dict, spec: dict) -> dict:
    """Two runs of one case: byte-identical files, or (`decoded`) identical canonical
    buffers and the same verdicts. For `byte-identical`, `second` holds only digests."""
    same_out = first["output_sha256"] == second["output_sha256"]
    same_pre = first["pre_encode_sha256"] == second["pre_encode_sha256"]
    if spec["determinism"] == "byte-identical":
        ok = same_out and same_pre
    else:
        ok = same_pre and [c["passed"] for c in first["checks"]] == [
            c["passed"] for c in second["checks"]]
    return _check("determinism", ok, determinism_class=spec["determinism"],
                  output_identical=same_out, pre_encode_identical=same_pre)


# What a malformed output raises while being read: a failed check, not a crash.
DECODE_FAULTS = (_icc.IccError, _gainmap.GainMapError, struct.error, ValueError,
                 KeyError, IndexError, OSError)


def facts_for(report: dict, man: dict) -> dict:
    """What an oracle needs to know about a run: its encoding and the pinned policy."""
    encoding, gamut, transfer = encoding_of(report)
    return dict(encoding=encoding, gamut=gamut, transfer=transfer, report=report,
                hdr=man["hdr"], gain_map=man["gain_map"], inset=man["patch_inset"])


def check_output(output: str, pre_encode: str, report: dict, man: dict,
                 patches: dict | None = None) -> list[dict]:
    """Every check of one output against the canonical buffers in `pre_encode`, by the
    oracle its report's destination selects. Takes any file, so a test can hand it a
    corrupted copy."""
    return _checks(output, read_pre_encode(pre_encode), facts_for(report, man), man,
                   patches)


def _checks(output: str, pre: dict, facts: dict, man: dict, patches: dict | None):
    pages = [b for b, _ in pre["_order"]]
    if pages != PAGES[facts["encoding"]]:
        return [_check("pre_encode", False, expected=PAGES[facts["encoding"]], found=pages)]
    spec = man["encodings"][facts["encoding"]]
    try:
        return ORACLES[facts["encoding"]](output, pre, facts, spec, patches)
    except DECODE_FAULTS as e:
        return [_check("decode", False, error=f"{type(e).__name__}: {e}")]


def rerun_hashes(nc: str, case: dict, workdir: str) -> dict:
    """The determinism rerun of a byte-identical encoding: only the two digests."""
    conv = convert(nc, case, workdir, "b")
    return dict(output_sha256=sha256_file(conv["output"]),
                pre_encode_sha256=sha256_file(conv["pre_encode"]))


def run_case(nc: str, case: dict, man: dict, workdir: str, tag: str) -> tuple[dict, dict]:
    """Convert and check one case: its result entry, and what the cross-encoding
    comparison and the record's header need (`light`, `patches`, `identity`)."""
    conv = convert(nc, case, workdir, tag)
    report = conv["report"]
    patches = None
    if case.get("input_name") and man["inputs"][case["input_name"]].get("patches") == "chart":
        patches = _chart.layout()[2]
    facts = facts_for(report, man)
    pre = read_pre_encode(conv["pre_encode"])
    order = pre["_order"]
    checks = _checks(conv["output"], pre, facts, man, patches)
    del pre
    identity = report.get("identity", {})
    chain = report.get("chain") or {}
    entry = dict(
        name=case["name"], input=os.path.relpath(case["input"], _compare.repo_root()),
        input_sha256=sha256_file(case["input"]), args=case["args"],
        rendering=chain.get("rendering"), destination=chain.get("destination"),
        encoding=facts["encoding"], gamut=facts["gamut"], transfer=facts["transfer"],
        params_hash=identity.get("params_hash"),
        pipeline_version=identity.get("pipeline_version"),
        working_mapping=report.get("working_mapping"),
        output_sha256=sha256_file(conv["output"]),
        pre_encode_sha256=sha256_file(conv["pre_encode"]),
        pre_encode_pages=[list(p) for p in order],
        checks=checks,
    )
    light = None
    if patches is not None and facts["encoding"] != "film-master":
        try:
            light = decode_light(conv["output"], facts)
        except DECODE_FAULTS as e:
            checks.append(_check("decode_light", False, error=f"{type(e).__name__}: {e}"))
    return entry, dict(light=light, patches=patches, inset=facts["inset"],
                       identity=identity, input_name=case.get("input_name"))


def compare_golden(cases: list[dict], golden: dict) -> list[dict]:
    """Checks against a recorded golden: the canonical buffer, the file and every
    metadata field the oracles read."""
    out = []
    known = golden.get("cases", {})
    for c in cases:
        g = known.get(c["name"])
        if g is None:
            out.append(dict(case=c["name"], passed=False, diffs=["not in the golden"]))
            continue
        now = golden_entry(c)
        diffs = [k for k in ("pre_encode_sha256", "output_sha256", "metadata", "summary")
                 if now[k] != g.get(k)]
        out.append(dict(case=c["name"], passed=not diffs, diffs=diffs))
    return out


def golden_entry(c: dict) -> dict:
    meta = next((x.get("fields") for x in c["checks"] if x["check"] == "metadata"), None)
    return dict(pre_encode_sha256=c["pre_encode_sha256"], output_sha256=c["output_sha256"],
                metadata=json.loads(json.dumps(meta, default=str)),
                summary=_summary(c["checks"]))


def find_decoders() -> dict:
    """The decoders' versions, for the record."""
    import PIL
    import numpy
    import tifffile
    from PIL import features
    return dict(versions=dict(numpy=numpy.__version__, tifffile=tifffile.__version__,
                              pillow=PIL.__version__,
                              libjpeg=features.version("jpg"),
                              libjpeg_turbo=features.version("libjpeg_turbo")))


def run(nc: str, man: dict, cases: list[dict], golden: dict | None = None) -> dict:
    """Every case twice, its checks, the cross-encoding groups, and the verdict."""
    preflight(nc)
    decoders = find_decoders()
    results = []
    extras = []
    with tempfile.TemporaryDirectory(prefix="nc-acceptance-") as workdir:
        for case in cases:
            first, extra = run_case(nc, case, man, workdir, "a")
            spec = man["encodings"][first["encoding"]]
            second = (rerun_hashes(nc, case, workdir)
                      if spec["determinism"] == "byte-identical"
                      else run_case(nc, case, man, workdir, "b")[0])
            first["checks"].append(determinism_check(first, second, spec))
            first["passed"] = all(c["passed"] for c in first["checks"])
            results.append(first)
            extras.append(extra)
    cross = cross_encoding([dict(name=r["name"], input_name=x["input_name"],
                                 rendering=r["rendering"], encoding=r["encoding"],
                                 gamut=r["gamut"], light=x["light"], patches=x["patches"],
                                 inset=x["inset"]) for r, x in zip(results, extras)], man)
    identity = extras[0]["identity"]
    record = dict(
        schema_version=RESULT_SCHEMA,
        tool="nctool acceptance",
        build={k: identity.get(k) for k in ("nc_version", "git_commit", "git_dirty",
                                             "pipeline_version", "target")},
        decoders=decoders["versions"],
        codec=man.get("codec"),
        cases=results,
        cross_encoding=cross,
    )
    # Whether the oracles passed, apart from the golden: what `--write-golden` needs, so
    # that `--golden G --write-golden G` re-baselines a deliberate change.
    record["checks_passed"] = (all(r["passed"] for r in results)
                               and all(g["passed"] for g in cross))
    if golden is not None:
        record["golden"] = compare_golden(results, golden)
    record["passed"] = (record["checks_passed"]
                        and all(g["passed"] for g in record.get("golden", [])))
    return record


def _json(value) -> str:
    return json.dumps(value, indent=2, sort_keys=True, default=_jsonable) + "\n"


def _jsonable(v):
    if hasattr(v, "tolist"):
        return v.tolist()
    raise TypeError(f"{type(v).__name__} is not JSON-serializable")


def cmd_run(args) -> int:
    from . import metrics as _metrics
    try:
        try:
            _metrics.require_dependencies("acceptance")
        except _metrics.MetricsError as e:
            raise AcceptanceError(str(e)) from e
        man = load_manifest(args.manifest)
        cases = resolve(man, args.benchmark)
        if args.case:
            wanted = set(args.case)
            cases = [c for c in cases if c["name"] in wanted]
            missing = wanted - {c["name"] for c in cases}
            if missing:
                raise AcceptanceError(f"no such case: {', '.join(sorted(missing))}")
        if not cases:
            raise AcceptanceError("no cases to run")
        golden = None
        if args.golden:
            try:
                with open(args.golden) as f:
                    golden = json.load(f)
            except (OSError, json.JSONDecodeError) as e:
                raise AcceptanceError(f"--golden {args.golden}: {e}") from e
            if golden.get("schema_version") != GOLDEN_SCHEMA:
                raise AcceptanceError(f"{args.golden}: not a schema {GOLDEN_SCHEMA} golden")
        record = run(args.nc, man, cases, golden)
        text = _json(record)
        if args.out:
            with open(args.out, "w") as f:
                f.write(text)
        else:
            sys.stdout.write(text)
        if args.write_golden:
            if record["checks_passed"]:
                write_golden(args.write_golden, record)
            else:
                print(f"acceptance: not writing {args.write_golden}: a check failed",
                      file=sys.stderr)
    except ConvertFailed as e:
        print(f"acceptance: {e}", file=sys.stderr)
        return 1
    except (AcceptanceError, OSError, json.JSONDecodeError) as e:
        print(f"acceptance: {e}", file=sys.stderr)
        return 2
    for c in record["cases"]:
        if not c["passed"]:
            failed = [x["check"] for x in c["checks"] if not x["passed"]]
            print(f"FAIL {c['name']}: {', '.join(failed)}", file=sys.stderr)
    for g in record["cross_encoding"]:
        if not g["passed"]:
            print(f"FAIL cross-encoding {g['input']} ({g['rendering']}): "
                  f"{len(g['fails'])} patch(es)", file=sys.stderr)
    return 0 if record["passed"] else 1


def write_golden(path: str, record: dict) -> None:
    """Write the run into the golden. Cases this run did not cover keep their entries;
    a case whose entry changed keeps the values it replaced under `previous`, and an
    unchanged one keeps the `previous` it had."""
    old: dict = {}
    if os.path.exists(path):
        with open(path) as f:
            old = json.load(f).get("cases", {})
    cases = dict(old)
    for c in record["cases"]:
        entry = golden_entry(c)
        before = old.get(c["name"])
        if before is not None:
            current = {k: v for k, v in before.items() if k != "previous"}
            if current != entry:
                entry["previous"] = current
            elif "previous" in before:
                entry["previous"] = before["previous"]
        cases[c["name"]] = entry
    golden = dict(schema_version=GOLDEN_SCHEMA, build=record["build"],
                  decoders=record["decoders"], cases=cases)
    with open(path, "w") as f:
        f.write(_json(golden))


CHART = os.path.join(_compare.repo_root(), "tests", "fixtures", "chart-48bit.tif")


def cmd_chart(args) -> int:
    """Write the synthetic chart, or check that the committed one is current."""
    if args.check:
        import numpy as np
        import tifffile
        if not os.path.isfile(args.out):
            print(f"{args.out} does not exist; write it with `python -m nctool acceptance "
                  "chart`", file=sys.stderr)
            return 2
        committed = tifffile.imread(args.out)
        if not np.array_equal(committed, _chart.pixels()):
            print(f"{args.out} differs from `nctool.chart`; regenerate it with "
                  "`python -m nctool acceptance chart`", file=sys.stderr)
            return 1
        return 0
    _chart.write(args.out)
    return 0

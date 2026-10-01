"""Rec. ITU-R BT.2100 transfers, in binary64.

Written from the standard, not from nc's `pipeline::hdr`, so the PQ/HLG oracles in
`nctool.acceptance` check nc's encoders against an independent reading. The one
nc-specific step is HLG's gamut pull (`hlg_signal`), which design-spec §"Fit gamut"
defines and the encoder repeats after the inverse OOTF.
"""

from __future__ import annotations

import numpy as np

# ST 2084 / BT.2100 Table 4.
PQ_M1 = 2610 / 16384
PQ_M2 = 2523 / 4096 * 128
PQ_C1 = 3424 / 4096
PQ_C2 = 2413 / 4096 * 32
PQ_C3 = 2392 / 4096 * 32
PQ_PEAK_NITS = 10000.0

# BT.2100 Table 5.
HLG_A = 0.17883277
HLG_B = 1 - 4 * HLG_A
HLG_C = 0.5 - HLG_A * np.log(4 * HLG_A)

# BT.2020 non-constant-luminance luma.
BT2020_LUMA = np.array([0.2627, 0.6780, 0.0593])


def pq_inverse_eotf(nits):
    """Absolute display luminance in cd/m² → PQ signal in [0, 1]."""
    y = np.clip(np.asarray(nits, dtype=np.float64) / PQ_PEAK_NITS, 0, None)
    p = np.power(y, PQ_M1)
    return np.power((PQ_C1 + PQ_C2 * p) / (1 + PQ_C3 * p), PQ_M2)


def pq_eotf(signal):
    """PQ signal → absolute display luminance in cd/m²."""
    e = np.power(np.clip(np.asarray(signal, dtype=np.float64), 0, 1), 1 / PQ_M2)
    return PQ_PEAK_NITS * np.power(np.clip(e - PQ_C1, 0, None) / (PQ_C2 - PQ_C3 * e), 1 / PQ_M1)


def hlg_oetf(scene):
    """Normalized scene light in [0, 1] → HLG signal."""
    e = np.clip(np.asarray(scene, dtype=np.float64), 0, None)
    low = np.sqrt(3 * e)
    with np.errstate(invalid="ignore", divide="ignore"):
        high = HLG_A * np.log(np.maximum(12 * e - HLG_B, 1e-300)) + HLG_C
    return np.where(e <= 1 / 12, low, high)


def hlg_inverse_ootf(display, gamma: float):
    """Display light normalized to the peak (`(..., 3)`) → scene light, inverting
    `F_D = Y_S^(γ-1) · E` for a zero-black display."""
    d = np.asarray(display, dtype=np.float64)
    yd = d @ BT2020_LUMA
    with np.errstate(divide="ignore", invalid="ignore"):
        scale = np.where(yd > 0, np.power(np.maximum(yd, 1e-300), (1 - gamma) / gamma), 0.0)
    return d * scale[..., None]


def radial_to_cube(rgb, luminance, ceiling: float = 1.0):
    """Pull each colour toward neutral at constant luminance until it lies in the cube
    `[0, max(ceiling, Y)]`; a colour with `Y <= 0` becomes black."""
    rgb = np.asarray(rgb, dtype=np.float64)
    y = np.asarray(luminance, dtype=np.float64)[..., None]
    upper = np.maximum(ceiling, y)
    delta = rgb - y
    with np.errstate(divide="ignore", invalid="ignore"):
        to_top = np.where(delta > 0, (upper - y) / delta, np.inf)
        to_bottom = np.where(delta < 0, -y / delta, np.inf)
    scale = np.minimum(1.0, np.minimum(to_top, to_bottom).min(axis=-1, keepdims=True))
    out = y + scale * delta
    out = np.clip(out, 0, upper)
    return np.where(y > 0, out, 0.0)


def pq_signal(linear, reference_white_nits: float):
    """Reference-white-relative linear BT.2020 (1.0 = reference white) → PQ signal."""
    return pq_inverse_eotf(np.asarray(linear, dtype=np.float64) * reference_white_nits)


def hlg_signal(linear, reference_white_nits: float, peak_nits: float, gamma: float):
    """Reference-white-relative linear BT.2020 → HLG signal for a `peak_nits`,
    zero-black reference display: normalize to the peak, invert the OOTF, pull into
    the cube, then the OETF."""
    display = np.asarray(linear, dtype=np.float64) * (reference_white_nits / peak_nits)
    scene = hlg_inverse_ootf(display, gamma)
    return hlg_oetf(radial_to_cube(scene, scene @ BT2020_LUMA, 1.0))


def round_half_away(x):
    """Round half away from zero (numpy's `round` is half-to-even)."""
    x = np.asarray(x, dtype=np.float64)
    return np.sign(x) * np.floor(np.abs(x) + 0.5)


def hlg_inverse_oetf(signal):
    """HLG signal → normalized scene light."""
    e = np.clip(np.asarray(signal, dtype=np.float64), 0, 1)
    low = e * e / 3
    high = (np.exp((e - HLG_C) / HLG_A) + HLG_B) / 12
    return np.where(e <= 0.5, low, high)


def hlg_display_nits(signal, peak_nits: float, gamma: float):
    """HLG signal (`(..., 3)`) → display light in cd/m² on a `peak_nits`, zero-black
    reference display: the inverse OETF, then the OOTF `F_D = L_W · Y_S^(γ-1) · E_S`."""
    scene = hlg_inverse_oetf(signal)
    ys = scene @ BT2020_LUMA
    with np.errstate(divide="ignore", invalid="ignore"):
        scale = np.where(ys > 0, np.power(np.maximum(ys, 1e-300), gamma - 1), 0.0)
    return peak_nits * scene * scale[..., None]


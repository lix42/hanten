"""CIELAB, CIEDE2000 and CIE 1976 u'v', as `analysis/display-output-acceptance` pins
them for the cross-encoding oracle.

The white is the **tabulated** D65 2° triple, not one derived from a space's
primaries (`nctool.metrics` derives its own, deliberately; the two coexist).
CIEDE2000 follows Sharma, Wu & Dalal (2005) with `kL = kC = kH = 1`.
"""

from __future__ import annotations

import numpy as np

D65_WHITE = (0.95047, 1.00000, 1.08883)
DELTA = 6 / 29


def _f(t):
    t = np.asarray(t, dtype=np.float64)
    return np.where(t > DELTA ** 3, np.cbrt(t), t / (3 * DELTA ** 2) + 4 / 29)


def xyz_to_lab(xyz, white=D65_WHITE):
    """XYZ (`(..., 3)`, white `Y = 1`) → CIELAB."""
    xyz = np.asarray(xyz, dtype=np.float64)
    fx, fy, fz = (_f(xyz[..., i] / white[i]) for i in range(3))
    return np.stack([116 * fy - 16, 500 * (fx - fy), 200 * (fy - fz)], axis=-1)


def delta_e_2000(lab1, lab2):
    """CIEDE2000 between two CIELAB arrays (`(..., 3)`)."""
    lab1 = np.asarray(lab1, dtype=np.float64)
    lab2 = np.asarray(lab2, dtype=np.float64)
    l1, a1, b1 = lab1[..., 0], lab1[..., 1], lab1[..., 2]
    l2, a2, b2 = lab2[..., 0], lab2[..., 1], lab2[..., 2]
    c1 = np.hypot(a1, b1)
    c2 = np.hypot(a2, b2)
    c_bar = (c1 + c2) / 2
    g = 0.5 * (1 - np.sqrt(c_bar ** 7 / (c_bar ** 7 + 25.0 ** 7)))
    a1p, a2p = (1 + g) * a1, (1 + g) * a2
    c1p, c2p = np.hypot(a1p, b1), np.hypot(a2p, b2)
    h1p = np.where((a1p == 0) & (b1 == 0), 0.0, np.degrees(np.arctan2(b1, a1p)) % 360)
    h2p = np.where((a2p == 0) & (b2 == 0), 0.0, np.degrees(np.arctan2(b2, a2p)) % 360)
    dl = l2 - l1
    dc = c2p - c1p
    dh_raw = h2p - h1p
    both = c1p * c2p
    dh = np.where(both == 0, 0.0,
                  np.where(dh_raw > 180, dh_raw - 360,
                           np.where(dh_raw < -180, dh_raw + 360, dh_raw)))
    d_big_h = 2 * np.sqrt(both) * np.sin(np.radians(dh / 2))
    l_bar = (l1 + l2) / 2
    cp_bar = (c1p + c2p) / 2
    h_sum = h1p + h2p
    hp_bar = np.where(both == 0, h_sum,
                      np.where(np.abs(h1p - h2p) <= 180, h_sum / 2,
                               np.where(h_sum < 360, (h_sum + 360) / 2, (h_sum - 360) / 2)))
    t = (1 - 0.17 * np.cos(np.radians(hp_bar - 30)) + 0.24 * np.cos(np.radians(2 * hp_bar))
         + 0.32 * np.cos(np.radians(3 * hp_bar + 6)) - 0.20 * np.cos(np.radians(4 * hp_bar - 63)))
    d_theta = 30 * np.exp(-(((hp_bar - 275) / 25) ** 2))
    r_c = 2 * np.sqrt(cp_bar ** 7 / (cp_bar ** 7 + 25.0 ** 7))
    s_l = 1 + 0.015 * (l_bar - 50) ** 2 / np.sqrt(20 + (l_bar - 50) ** 2)
    s_c = 1 + 0.045 * cp_bar
    s_h = 1 + 0.015 * cp_bar * t
    r_t = -np.sin(np.radians(2 * d_theta)) * r_c
    return np.sqrt((dl / s_l) ** 2 + (dc / s_c) ** 2 + (d_big_h / s_h) ** 2
                   + r_t * (dc / s_c) * (d_big_h / s_h))


def uv_prime(xyz):
    """CIE 1976 `(u', v')`. A zero denominator is an invalid sample, not a pass."""
    xyz = np.asarray(xyz, dtype=np.float64)
    x, y, z = xyz[..., 0], xyz[..., 1], xyz[..., 2]
    den = x + 15 * y + 3 * z
    if np.any(den == 0):
        raise ValueError("u'v' of a sample with X + 15Y + 3Z = 0 is undefined")
    return np.stack([4 * x / den, 9 * y / den], axis=-1)


def hue_angle(lab):
    """CIELAB hue angle `h_ab` in degrees."""
    lab = np.asarray(lab, dtype=np.float64)
    return np.degrees(np.arctan2(lab[..., 2], lab[..., 1])) % 360

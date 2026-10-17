# Can the content base use the rebate?

Task: `film-base/content-base-rebate` (2026-10-16). Raw output:
`../temp/content-base-rebate/` (`summary.json`, `reports/`, `band.log`).

## Question

`measure-roll --base-content` reads the 99th percentile of every picture frame's
effective area, pooled. The default inset (5% of the shorter edge) cuts away the frame's
border, where the rebate is: the only unexposed film on a picture frame. Does
`--measure-inset 0` bring the content base closer to the roll's measured base, or does the
border's sprocket holes and backlight push it the wrong way?

## Method

13 archive rolls (Plustek OpticFilm 8300i, SilverFast HDRi), the picture frames only (role
`real`, no `--leader`):

- **measured base**: `hanten measure-base base.tif`, the unexposed frame's median;
- **content base** at the default inset and at `--measure-inset 0`: `hanten measure-roll
  <frames> --base-content`;
- **the band** between the two areas, per frame and edge
  (`film_base::rebate_band_probe`, `#[ignore]`d): each pixel is *base-like* (every
  channel within 0.05 density of the measured base), *clearer* (any channel more than 0.05
  clearer: sprocket holes, backlight) or *denser* (picture, holder).

The probe's pooled p99 reproduces the binary's content base to four decimals.

## Result

Error of the content base, in density (positive: denser than the measured base):

| roll | frames | inset 0.05 (R, G, B) | inset 0 (R, G, B) | band base-like | band clearer |
|---|---|---|---|---|---|
| 2026-07-15 Ektar 100 | 3 | +0.029, +0.022, +0.064 | +0.010, +0.009, +0.024 | 6.5% | 0 |
| 2026-07-23 Portra 160 | 4 | −0.004, −0.008, +0.007 | −0.005, −0.009, +0.003 | 9.5% | 0 |
| 2026-07-24 Gold 200 | 3 | +0.016, +0.015, +0.003 | +0.015, +0.014, +0.002 | 4.1% | 0 |
| 2026-09-09 Ektar 100 | 12 | −0.010, +0.001, +0.092 | −0.010, +0.002, +0.094 | 0.04% | 0.04% |
| 2026-09-11 Portra 400 | 11 | −0.046, −0.062, −0.069 | −0.046, −0.062, −0.069 | 9.8% | 3.7% |
| 2026-09-13 Portra 400 | 10 | −0.039, −0.051, −0.051 | −0.038, −0.050, −0.050 | 7.0% | 1.3% |
| 2026-09-14 Ektar 100 | 32 | +0.025, +0.041, +0.088 | +0.023, +0.039, +0.085 | 0.4% | 0.01% |
| 2026-09-18 Gold 200 | 35 | +0.019, +0.017, −0.009 | +0.017, +0.016, −0.010 | 2.4% | 0.1% |
| 2026-09-20 Portra 400 | 34 | −0.004, −0.015, −0.007 | −0.004, −0.014, −0.006 | 2.6% | 0.2% |
| 2026-09-28 Portra 400 dark | 34 | −0.007, −0.019, −0.019 | −0.007, −0.019, −0.019 | 12.7% | 0.01% |
| 2026-09-29 Ektar 100 | 31 | −0.018, −0.016, +0.018 | −0.016, −0.014, +0.021 | 1.1% | 0.02% |
| 2026-10-10 Lomo 100 | 37 | −0.003, −0.007, −0.016 | −0.003, −0.006, −0.016 | 9.6% | 0 |
| 2026-10-12 Gold 200 | 29 | −0.012, −0.015, −0.031 | −0.012, −0.015, −0.031 | 17.3% | 0.04% |

The band is 148–169 px wide and 14–16% of each frame's area. No warning appears or
disappears at inset 0.

**Inset 0 is close to a no-op.** One roll moves (07-15 Ektar, blue +0.064 → +0.024).
Every other roll-channel moves by at most 0.004 density, and none gets worse by more
than 0.003.

**Why it rarely helps.** Most of the band is denser than the p99, so adding it only
dilutes the pool's top 1%. That accounts for the small moves above. Base-like pixels move
a p99 substantially only when it sits *denser* than the base and they make up a real
share of that 1%:

- Rolls that err **clear** (the Portra 400s, Lomo, 10-12 Gold) already have pixels
  clearer than the base above their p99 (deep shadows, noise). The rebate falls below
  those pixels, so it cannot set the p99.
- The rolls with the worst **dense** errors (09-09 and 09-14 Ektar, blue +0.09) show
  almost no rebate in the band: 0.04% and 0.4%. The 09-09 scans were cropped at scan
  time, and nearly all of 09-14's band is picture or holder.
- Where the band *is* base-like on a roll that errs dense (07-15: 6.5% of the band, about
  0.9% of the pool), it moves the p99 most of the way to the base.

**No sprocket holes or backlight on this scanner.** The clearer share is at most 3.7% of
the band (09-11 Portra 400, about 0.5% of the pool). That is too little to reach the p99,
and on a roll whose p99 is already clearer than the base. The Plustek holder covers the
sprocket holes, so the archive cannot test the risk the task named.

## Answers

1. **On rolls that show rebate, does inset 0 move the content base toward the measured
   base?** Yes, when the roll errs dense and the band has enough rebate (07-15). On the
   other 12 rolls the effect is ≤0.004 density. It never made a roll meaningfully worse.
   It does not touch the two worst misses, because their scans show no rebate.
2. **Camera scans:** open. The archive has none. A linear 16-bit TIFF export reads once
   its input is asserted linear scanner-device (`convert`'s `--input-*` flags, a recipe's
   `input` keys for `measure-roll`). RAW and DNG do not read yet (`io/camera-scan-input`).
   A camera scan usually shows the sprocket holes. Holes on both long edges are roughly a
   tenth of a 135 frame's area, far more than the 1% tail. So inset 0 would very likely
   land the p99 on the backlight, which `--base-content`'s 0.9-of-full-scale and
   0.15-density warnings exist to catch.
3. **A rebate search?** Not on this evidence. The rebate is small and unevenly visible
   (0–17% of the band, per roll), and the p99 uses it only on a roll that errs dense. Where
   a scan shows rebate, a base read from the rebate itself (`measure-base --base-region`,
   or a rebate area the user marks in the app) is a direct measurement and needs no p99 to
   find it.

**Recommendation:** keep the default inset for the content base, and do not add a rebate
search. Camera scans are left open with no follow-up task (user decision). They are where
the rebate and the sprocket holes are both in frame.

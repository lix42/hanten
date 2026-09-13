# Scanner Density Calibration


## Goal

Establish what a scanner's numbers mean in **absolute** density, so that densities
published by film manufacturers can be used by reconstruction. Today
`io/input-data-semantics` resolves an input's *transfer* and *meaning* but not its
*absolute normalisation*, which leaves a real gap: a datasheet-derived parameter is
only usable if our density scale can be related to the densitometry the datasheet
used.

## Input from `algo/film-stock-profiles` (2026-09-08, that task's close-out)

This task is now **unblocked and load-bearing**: it owns the largest known colour gap in the
renderer, with measurements to aim at.

- **The gap, quantified.** Inverting each stock's published curve removes blue's
  exposure-dependent cast (drift +1.26 → +0.09 stops per unit corrected density, against
  +1.29 predicted) but leaves **green at +0.40 mean, +1.00 on the Ektar roll** across 21
  frames. Blue transfers; green does not.
- **Added 2026-09-09: a per-channel gain now ships as a default, and it is a placeholder
  this task should replace.** `density.scale` defaults to `[1, 0.90, 0.86]` under the
  parametric curves (`pipeline_version` 4). Two measurements make it this task's business
  rather than the curve's:
  - **The scan's green and blue slopes against red are nearly equal** (1.115 and 1.183),
    where the datasheets say they are far apart (green barely steeper, blue 16%). An excess
    landing on *both* channels against red is not film chemistry — it is the signature of
    something in the scan/decode/base path, i.e. exactly this task's subject.
  - **The gain nulls the corpus mean, not any roll.** Per-roll residuals still span ±0.5
    stop per density on the green–magenta axis (`curve_probe::sigmoid_scale`), and no single
    scale improves it — even the scan-derived corpus mean only moves \|green–magenta\| from
    0.60 to 0.56. A scale-shaped correction has no generic setting worth shipping, which is
    the negative result arguing for the 3×3 below.
  - The `characteristic` curve deliberately keeps the **identity** gain so this residual
    stays visible rather than half-absorbed: its own solved gain (`[1, 0.938, 0.985]`)
    measured *worse* than identity on real frames (0.047 against 0.039).
- **The form to fit is a 3×3 + offset, not per-channel gains.** ACES applies exactly that
  (`CDD → CID`) *before* its per-channel curves, and nc is the same chain minus that stage.
  SMPTE ST 2065-2 NOTE 3 says the conversion between scanner density and a standard density
  metric is "3 × 3 matrix transformations followed by an offset", is **product specific**,
  and is "likely imperfect". A per-channel gain cannot be the whole answer: it cancels in
  nc's base division.
- **No datasheet correction substitutes for a measurement.** Ektar 100's sheet is the corpus
  outlier (it draws R and G nearly parallel, ratio 1.002 against everyone else's 1.02–1.05,
  and disagrees with its own aim table by 11%) — yet replacing its channel relationship with
  the corpus consensus would move its residual only +1.00 → **+0.87**. The dominant term is
  in the scan, not the sheet.
- **What a calibration frame has to contain** (see that task's close-out discussion): a
  *neutral series*, not a single patch — the residual is a **slope**, so one grey card at one
  exposure fits an offset and cannot separate offset from slope. Neutrals alone constrain
  only the matrix diagonal and the offsets; the **off-diagonal terms need coloured patches**,
  because cross-channel contamination is a property of the dye spectra. A transmissive step
  wedge isolates the scanner but is blind to dye cross-talk for the same reason.
- **Sample size matters more than it looks.** Per-frame residuals scatter at sd 0.3–1.6, and
  one fixture roll spans −1.88…+2.03. Resolving a 0.3 stops/density difference needs ~11
  frames of the same condition. Two per-roll/per-stock conclusions were retracted during
  that task for reading n=3–4 too confidently.
- Diagnostic already in-tree: `algo::curve_probe::channel_drift` (asset-gated, `#[ignore]`d)
  reports scan / predicted / residual drift per channel, per stock and per roll, with
  scatter. Re-run it to score a candidate matrix.

### Shooting the calibration frames

Agreed with the user 2026-09-08, ahead of them exposing a set.

- **Target**: an X-Rite ColorChecker Classic is the complete answer — its six-patch neutral
  row fits the matrix diagonal and the offsets, and only its coloured patches can constrain
  the **off-diagonal** terms, since cross-channel contamination is a property of the dye
  spectra. A plain grey card is a genuine first step for the diagonal.
- **Bracket, always: −2, −1, 0, +1, +2 stops** off a metered reading of the grey patch. This
  is what makes a single grey patch usable at all — the residual is a *slope*, and one patch
  at one exposure fits an offset without separating it from the slope.
- **The bracket also makes the fit illuminant-independent where it matters.** A one-stop
  change multiplies exposure by 2 in *every* channel whatever the light, so relative log
  exposures are known exactly and each channel's response shape is recovered absolutely,
  leaving one unknown constant per channel — which the "+ offset" term absorbs and white
  balance handles downstream. Only channel *balance*, not channel *shape*, depends on the
  illuminant.
- **Light, in preference order**: bright overcast (most even, repeatable); direct sun from
  behind the camera with the card tilted 10–15° against sheen (best channel balance,
  ≈5500 K); clear-sky open shade last — it is 7000–12000 K, which starves red into its noisy
  toe, and its colour shifts with any lit surface bouncing in. Record which was used.
- **Geometry**: card flat and square, filling the central ~60% of the frame (vignetting is
  an additive field in density and would corrupt neutrality across the card), f/5.6–f/8, no
  filters or polariser. One extra frame with the card rotated 180° detects uneven light
  rather than leaving it to be assumed.
- **Place the set at the head of a roll that is then shot and scanned normally**, with that
  roll's unexposed frame and leader. It must share development batch *and* scanning session
  with real frames, and SilverFast's per-frame automatic adjustments must be off or locked —
  if the scanner adjusts per frame, a calibration from one frame does not transfer to the
  others, which would void the exercise.
- **One set is a start, not a conclusion.** Per-frame residuals scatter at sd 0.3–1.6, so a
  bracket on **two different rolls** is what separates "the film and scanner" from "that
  roll", the fork this task's whole diagnosis currently sits on.
- A transmissive step wedge would isolate the scanner from the film — useful if that
  distinction ever matters, but blind to dye cross-talk for the same spectral reason a
  neutral target cannot constrain the off-diagonal terms.

## Design

### What a scan value actually is

`io::decode`'s `normalize_u16` maps 16-bit samples to `f32` by dividing by 65535
(`src/io/decode.rs`). So a scan value is a **code-value ratio against full scale**,
not transmitted intensity over a measured reference intensity. Optical density is
`−log10(I/I₀)`, and `I₀` here is unknown: scanner exposure time and per-channel gains
shift the reference level arbitrarily between scans and between channels.

Everything below follows from that. **Absolute density requires a same-settings
open-gate (clear-gate) reference measurement** — a scan of the empty gate with
identical exposure and gain — to supply `I₀`. Without it there is no absolute scale,
only relative differences within one scan.

### Tier 1 — unexposed frame only: a diagnostic, **not** a calibration

The workflow already needs an unexposed frame or rebate for `Dmin`, so this tier is
free. On that frame, report `−log10(scan)` per channel beside the stock's published
nominal `D-min`.

**State plainly what this cannot do.** It cannot test absolute normalisation and
cannot define a correction, because an arbitrary per-channel offset sits between the
two quantities. A perfectly linear scan can disagree with the published `D-min`
purely from exposure and gain settings. Tier 1's value is as a **non-calibrating
diagnostic**: it establishes reproducibility across scans of the same roll, flags
gross anomalies (a channel near clipping, a wildly different scan setting between
frames of one roll), and records what the scanner reported so later work has a
baseline.

**Do not infer a density slope from the cross-channel spread.** The three channel
readings are one point on **three different response curves**, each with its own gain
and spectral sensitivity — not three points sampled from one curve. A compressed or
stretched spread can arise from channel gains alone, before any question of Status M
mismatch, and no individual channel has a second point from which a slope could be
identified. Classifying the spread as "scale compressed" or deriving a correction from
it would corrupt colour. Slope requires a second known density **in each channel**.

### Tier 2 — a calibrated transmission target

To determine offset *and* slope per channel, the second sample must be a **known
density**, which means a **calibrated transmission step wedge** measured through the
same scan settings (or a fully specified sensitometric procedure: controlled exposure
onto the stock, documented process, then densitometry of the result).

**A photographed grey card is not a known density.** The developed negative density of
a photographed card depends on illumination, exposure, processing, and the stock's
characteristic curve — so an unexposed frame plus an ordinary grey-card frame cannot
determine offset and slope, and this task must not promise that it can.

Tier 2 asks for a target most users will not have, so it must be strictly optional and
never a precondition for conversion.

### A mismatch is not fatal

Manufacturer data supplies the *relationship* between landmarks (mid-grey to diffuse
white); a locally measured difference supplies the scale in our own units. Relative
differences are usable without an absolute anchor because the unknown offset cancels.
So an uncalibrated scanner does not block anything — it means deriving the parameter
from the locally measured difference rather than from a published absolute value. The
profile is therefore a **correction to apply when available**, never a gate.

### Output shape

A scanner profile is keyed by scanner + scan settings, a different axis from the film
stock — the two multiply and neither substitutes for the other. Whatever is applied to
pixels must be reported with provenance, and the uncalibrated path stays the default so
existing conversions do not silently change.

Related but distinct:
[scanner ICC before-density experiment](../color/scanner-profile-before-density-experiment.md)
concerns applying a *colour* transform before density conversion; this task concerns the
*density scale*. Do not conflate them.

## Implementation Suggestion

- Run tier 1 as a measurement/report first and look at real numbers before designing any
  correction — but frame the report as a diagnostic, not a verdict on absolute scale.
- The published `D-min` values available today are **chart readings, not Status M
  measurements** (see [film-stock profiles](../algo/film-stock-profiles.md) for why
  single-wavelength sampling of a spectral-density curve is not a Status M density).
  Treat them as nominal and do not build a correction on them until a properly derived
  or manufacturer-tabulated Status M value exists.
- Reuse per-stock reference data from
  [film-stock profiles](../algo/film-stock-profiles.md) rather than keeping a second
  copy — which is why that task is a prerequisite. Duplicating datasheet values across
  two modules is exactly the silent-drift risk the `pipeline/colorimetry/` pattern
  exists to prevent.
- The diagnostic *measurement* on real scans is performed by
  [reference-anchored sigmoid](../algo/reference-anchored-sigmoid.md)'s baseline
  harness, which is why this task depends on it: this task productises the result
  (a reportable, reusable profile), it does not perform the first measurement.

## How to Verify

- Tier 1's logic is covered by a **synthetic committed fixture** (a known scan value in,
  the expected `−log10(scan)` out), so a clean checkout can verify it with no assets.
- On the **external** real rolls — which live in the machine-local, uncommitted
  `../nc-assets` and must be identified by their `manifest.json` entry (roll + frame +
  `sha256`), not assumed present — tier 1 reports measured `−log10(scan)` per channel
  beside the nominal published `D-min`, driven through the
  `scripts/real-scan-verify/` harness. The report **explicitly states** that the
  comparison cannot establish absolute scale without an open-gate reference. This half
  cannot run in CI and must skip with a clear message when the assets are absent.
- The report does not classify the cross-channel spread as a scale error, and contains
  no correction derived from it.
- Tier 2, if implemented, recovers a known offset and slope per channel from a
  calibrated transmission step wedge (verifiable on a synthetic two-density input), and
  the docs do not claim a photographed grey card suffices.
- The default conversion path is byte-identical with no profile selected.
- With a profile applied, the resolved report names it and the correction, and the same
  profile reapplied reproduces the output bit-exactly.
- `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo build`, `cargo test` pass.

## Dependencies

- [Input data semantics and validation](input-data-semantics.md)
- [Film-stock profiles](../algo/film-stock-profiles.md) — supplies the per-stock nominal
  reference densities this task must not duplicate

`algo/reference-anchored-sigmoid` is now **transitive** via `algo/film-stock-profiles`.

---

**2026-09-13 — the calibration shoot is wanted by three tasks; plan it once.** The target this
task specifies (a ColorChecker Classic: neutral series for the diagonal and offsets, coloured
patches for the off-diagonal terms) overlaps two other open needs, and the user has it on
their roadmap:

- [`algo/sigmoid-parameter-calibration`](../algo/sigmoid-parameter-calibration.md) wants a
  **bracketed** roll (one subject at −2 … +2 EV) with a **grey card in frame**;
- [`film-base/dmax-per-channel-reduction`](../film-base/dmax-per-channel-reduction.md) parked
  on 2026-09-13 for want of exactly this — its roll-scoped measurement assumes scene colour is
  uncorrelated with density, which the available rolls (one trip, one palette) violate.

A ColorChecker **plus** a bracket, on a roll that also carries a grey card, satisfies all
three. Shooting for only one of them wastes the other two.

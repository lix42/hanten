# Contrast / Latitude Spike

> **Superseded 2026-09-19 by `nf-look/contrast`** — contrast becomes a look
> knob, so the spike is answered there; its nc-vs-NLP measurements are the
> evidence that carried over. Kept so existing references resolve; see
> `docs/nf-migration.md` for the migration plan.

## Goal

Decide whether nc's tonal latitude should change — and if so, at **which end** and
by **which mechanism**. A spike, not a feature: the deliverable is a decision plus
the evidence for it, and **"change nothing" is an acceptable outcome.**

Filed 2026-09-11 after the first measured nc-versus-Negative Lab Pro numbers (see
`docs/progress/analysis.md`, `analysis/nlp-comparison`). The user's own read: *"we
may raise our contrast value… maybe HDR can help… my guess is we should provide
multiple options, then pick the default by taste."*

## Use the Ektar batch, not the numbers below

The figures in this section come from **three** Gold200 frames whose NLP reference
is cropped to a different aspect ratio and carries a self-contradictory colour
profile. Treat them as the reason this task exists, not as its evidence.

`converted/nlp/2026-09-09-Ektar100/` is strictly better on every axis: one stock,
16-bit Adobe RGB (1998) at gamma 2.1992 with `desc`, primaries and TRC all in
agreement, and **pixel-aligned with its sources** under `rolls/2026-09-09-Ektar100/`
— identical dimensions, so no registration is needed and a per-pixel comparison is
available. Re-measure there first.

**2026-09-15 — the pairing is now 12 frames, not 32.** At the user's request 20 of that
roll's 32 frames were deleted as near-duplicates; the NLP outputs survive, so those 20
have no source to pair with. The sibling rolls are likewise reduced —
`2026-09-11-Portra400` to 11 of 32, `2026-09-13-Portra400` to 10 of 36 — so any
regression planned below has roughly a third of the samples it was scoped for. (Paths
above also predate the roll rename and are corrected here.) Note the declared
space differs per directory: that batch is `--space adobe-rgb`, while
`2026-07-2x`/`2026-08-04` are `--space linear-srgb`.

## What is measured, and what it does not settle

Three Gold200 frames with an NLP reference, nc through `chr-generic` and
`sig-flat`:

- **nc's tonal range is narrower on two of three frames** — `p95 − p5` of
  3.55–4.51 stops against NLP's 8.14 / 3.83 / 7.35. On G2 they are **tied**.
- **NLP's median is higher on all three** (+1.22 / +1.70 / −0.71 against nc's
  −0.71…−1.11).
- **nc is stable where NLP is not**: nc's `p95 − p5` moves **0.96** stops across
  the three frames, NLP's moves **4.31**. This is the one figure that held through
  every re-reading of the data, and it is the signature design-spec §3.8 describes.

What none of that settles is **why**, because the independent variable is missing:
nobody has measured each frame's **scene** range. Without it, "nc is narrower" and
"nc faithfully reproduces a narrower scene" are indistinguishable.

An early attempt to fill that gap from `scripts/analysis/fixtures.json`
patch densities was **wrong and should not be repeated**: the white patch is
flagged `valid: false` on two of the three frames (a window opening; sky above the
roll `Dmax`), and a three-patch spread is not comparable with a `p95 − p5` over 15
million pixels anyway.

## Open questions

- **Measure the scene range first.** The negative's own density distribution, per
  frame, from `hanten inspect` — a distribution, not patches. Everything below is
  guesswork until this exists. Regressing output range against scene range for both
  converters is what actually separates "nc compresses" from "nc carries a narrower
  scene": a slope near 1 means it carries, near 0 means it compresses. Note the sample
  is now **12** pixel-aligned Ektar pairs rather than the 32 this was scoped for (see
  above), so pooling the Portra rolls — 11 and 10 pairs, different stocks — may be
  necessary to say anything with confidence. **Pooling is not free**: each roll shares one
  development and scan recipe, and the two stocks differ in both input distribution and
  response, so a single pooled slope can charge a between-roll or between-stock difference
  to converter compression. Carry a per-roll (or per-stock) term or an interaction, and
  report the per-roll slopes separately. **Do not lean on a cluster-robust interval**:
  three rolls is three clusters, far too few for one to have reliable coverage, and it
  would understate the uncertainty it appears to quantify. With this dataset the per-roll
  results are descriptive evidence — if the three agree, say so and say it is three; an
  inferential claim needs more independent rolls.
- **Is the gap a consequence of §3.8 rather than a defect?** If nc faithfully
  carries each frame's own range while NLP adapts per frame, then the remedy is
  per-frame opt-in, not a global change.

  **2026-09-15 — the normalisation model is back on the table; the rejection above
  tested the wrong statistic.** "NLP normalises each frame to fill the output" was
  dismissed because it predicts a near-zero `p95 − p5` spread against a measured 4.31.
  That follows only if the stretch is fitted to `p5`/`p95`. Fitted to the **extremes**,
  a frame whose content clusters between two outliers keeps a narrow `p95 − p5` while
  one whose content fills the frame keeps a wide one — so a 4.31 spread is **compatible
  with** extreme-fitting rather than evidence against it.

  That is all it is. The model permits a range of spreads and equally permits identical
  ones; it does not predict 4.31, and the input distributions have not been measured. So
  this removes the objection without supplying any support — the model is back on the
  table, not ahead.

  The suggestive observation — **not yet evidence** — is a failure mode the statistics do
  not show: on frames filled by a single surface (all water, or cloudless sky) the user
  reported NLP losing almost all the detail (`rolls/2026-09-09-Ektar100/1612.tif`) while
  every nc config holds it.

  **Do not read that as confirming the model.** A monotone stretch of a narrow input
  interval onto the full output *increases* the separation between its samples; on its
  own it destroys no information. For detail to be lost some further stage has to act —
  clipping at the ends, quantization after the stretch, or a nonlinear step — and which
  one is **unidentified**. Treat the frame as the thing to be explained rather than as
  proof, or the spike will choose a remedy for a mechanism it never located.

  **Test it directly**: regress each converter's output extremes against the negative's
  own extremes. **"Gain rises as the scene range narrows" does not establish per-frame
  adaptation** and must not be the test: if gain is read as `output_range / scene_range`, the
  predictor sits in the denominator and the relationship is close to arithmetic; and a single
  fixed nonlinear transfer curve yields different local gains for frames occupying different
  input intervals. NLP can satisfy that check without adapting at all, so a remedy inferred
  from it would be constrained for the wrong reason. Fit a **common transfer curve across all
  frames** first, then test whether adding a frame-specific scale or effect explains the
  residual variation — per-frame adaptation is the term that earns its place, not the slope.
  Then on the frames that look degraded, measure where the detail actually goes. All **three**
  candidates need a test, not just the two that are easy: end clipping (count samples at
  the endpoints), post-stretch quantization (look for a comb in the histogram), and the
  nonlinearity (measure the transfer curve, or local slope over the flat region — a
  response that flattens water or sky produces neither clipping nor a quantization
  signature, so testing only the first two can leave the failure unexplained while looking
  complete). If a frame-specific term does earn its place, the remedy for nc is bounded
  per-frame adaptation at most — never mapping a frame's own range onto the full output,
  which is the line `--auto-d-max` already draws ("grading, not conversion"). If it does
  not, the common curve is the whole story and no per-frame mechanism is warranted here.

  **Mask the holder first, and pick the endpoint statistic before measuring.** On a scan
  that keeps its border the literal extrema are the opaque holder, not the scene
  (`scripts/analysis/README.md:340-345`), so an unmasked regression measures cropping.
  Both sides need a region covering the **same scene area**, and how you get one differs by
  batch. The primary Ektar pairs are pixel-aligned with identical dimensions (see above), so
  there use **one shared mask** — defining a region independently per side there can select
  different scene pixels and manufacture the very range relationship being measured. Only for
  genuinely differently-cropped batches does the region have to be derived per side, by
  registration or by an intersecting content crop. "Extreme" also needs a definition robust to
  dust and specular pixels.
- **Which end?** On G2 the gap splits 43% shadow / 57% highlight. Raising global
  contrast would treat both, and nc already cannot reach diffuse white — the last
  non-empty luminance bin on five G2 presets is L\* 88–98 against white at 100.
- **Does HDR dissolve it?** More output range means less need to compress. The
  metrics histogram now runs to twice diffuse white precisely so headroom is
  measurable rather than inferred.
- **What is the deliverable shape?** A knob, a new axis on the `--preset` bundles
  (the pattern `algo/conversion-presets` established), or nothing. The user
  expects multiple options with the default chosen by eye — which means a review
  set, not a number.

## Non-goals

No default moves inside this spike. The shared brightness target approved
2026-09-09 stands until a separate decision replaces it. Not a fix for
`analysis/nlp-comparison`, which owns the reference-comparison tooling; this spike
consumes evidence rather than building the harness.

## How to Verify

A spike is done when the decision is recorded with the evidence behind it: the
scene-range measurement exists, each open question above is answered or explicitly
deferred, and the outcome is either a named follow-up task with a chosen mechanism
or a recorded "no change, because…". Any option offered for a taste judgement is
rendered into a review set (`tools/review-app`) rather than argued numerically.

## Dependencies

- [Conversion Metrics & Photographic Analysis](../analysis/conversion-metrics.md)
- [Named conversion presets](conversion-presets.md)
- [Reference comparison: nc vs NLP](../analysis/nlp-comparison.md) — added 2026-09-13:
  the scene-range regression is that harness's job; without it this spike grows a second
  pairing script. Revised 2026-09-15: the dataset is **12** pixel-aligned Ektar pairs, not
  the 32 first scoped (see above), so the harness should expect to pool the Portra rolls —
  11 and 10 pairs of a different stock — rather than treat Ektar alone as sufficient.
  **Resolve each NLP directory's declared space before pooling it.** Only the Ektar and the
  July/August batches have been established; `docs/progress/analysis.md` records that the
  2026-09-11 Portra batch's space "must be established the same way before it is measured",
  and there is no determination on record for 2026-09-13 at all. Reading a batch's own ICC
  profile per directory is the prerequisite — guessing or inheriting one is what produced
  the transfer-function decoding error that invalidated the first round of measurements

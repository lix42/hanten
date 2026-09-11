# Contrast / Latitude Spike

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

`converted/nlp/2026-09-09/2026-09-09-Ektar/` is strictly better on every axis: **32
frames** of one stock, 16-bit Adobe RGB (1998) at gamma 2.1992 with `desc`,
primaries and TRC all in agreement, and **pixel-aligned with its sources** under
`rolls/2026-09-09-Ektar/` — identical dimensions, so no registration is needed and
a per-pixel comparison is available. Re-measure there first. Note the declared
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

An early attempt to fill that gap from `scripts/sigmoid-baseline/fixtures.json`
patch densities was **wrong and should not be repeated**: the white patch is
flagged `valid: false` on two of the three frames (a window opening; sky above the
roll `Dmax`), and a three-patch spread is not comparable with a `p95 − p5` over 15
million pixels anyway.

## Open questions

- **Measure the scene range first.** The negative's own density distribution, per
  frame, from `nc inspect` — a distribution, not patches. Everything below is
  guesswork until this exists. With 32 pixel-aligned Ektar pairs there is enough
  data to regress output range against scene range for both converters, which is
  what actually separates "nc compresses" from "nc carries a narrower scene": a
  slope near 1 means it carries, near 0 means it compresses.
- **Is the gap a consequence of §3.8 rather than a defect?** If nc faithfully
  carries each frame's own range while NLP adapts per frame, then the remedy is
  per-frame opt-in, not a global change. But note NLP's output range varies *more*
  than the scene range plausibly does, so "NLP normalises each frame to fill the
  output" does not describe what it is doing either — that model predicts a
  near-zero spread and the measurement gives 4.31.
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

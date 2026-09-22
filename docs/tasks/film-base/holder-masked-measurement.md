# Rebuild Dmin and Dmax measurement on area x method

## Goal

Migrate `Dmin` and `Dmax` measurement onto **two inputs and nothing else**
(user, 2026-09-16):

- **Area** — either the effective area `holder-depth-mask` resolves, or an area the
  user states.
- **Method** — a percentile over that whole area, or the grid. (The user leans
  whole-area percentile; see open question 1.)

Everything else that currently decides where and how the base is measured goes away,
including the rebate-band search. nc is not shipped, so a breaking change here is
accepted (user, 2026-09-16).

**Pixel change.** The base is the divisor of the whole conversion, so this owes a
`pipeline_version` bump. The area and the estimator ship together for that reason:
split, they cost two bumps and two baselines for one conceptual change.

## What this retires

`FilmBaseSource::Auto` today is a **rebate detector** — `rebate_candidates` +
`select_auto_base` march inward per edge for a thin uniform band sitting behind the
holder (`auto-base-redesign`, design-spec §9). Under area x method there is no such
search: `Auto` becomes "the effective area, measured by the configured method".

Removing it reaches four other tasks, all of which exist to serve the detector:
`auto-base-real-scan-refusal` (why it never fires), `auto-base-neutral-stock`
(hardening it for a neutral base), `white-holder-support` (its holder polarity), and
`core/base-acquisition-planner`'s auto rung. Their disposition is part of this task,
not a silent consequence of it — each is parked with a pointer here.

**`content-fallback` is not retired.** Tier 3 estimates the base from picture content
when there is no unexposed film anywhere on the frame; that is a different question
from where to measure, and the effective area is what it would measure over.

## Why the estimator must change with the area

p97 exists to perform **population selection**: on a rebate strip the region is a
*mixture*, unexposed film is the sub-population with the largest transmission, and
an extreme percentile reaches past the contaminants to land in it.

Masking removes that premise. A masked unexposed frame is a *single* population,
and an extreme percentile then just lands in its noise tail — biased by
construction, roughly +2σ in transmission and therefore an *understated* density,
since `D = −log10(t/t_base)` is strictly decreasing.

Measured on the Gold 200 leader (2026-08-11), p97 sits 0.046 density from p50 —
0.16 stops once `dA/dR = f = 0.5` carries it into the sigmoid anchor, systematic
across the roll and in the "pale" direction the sigmoid work exists to fix.

`reference_dmax` already samples at p = 0.5 for exactly this reason. This task
brings `Dmin` onto the same rule.

**"Whole-area percentile" must not be read as "keep p97".** Which percentile is the
open question below; what the measurement above rules out is an *extreme* one over a
single population. p97 survives only where the region is still a genuine mixture —
an untrusted user rectangle is the one remaining candidate.

## What is known

- **Holder depth is small and asymmetric.** On the unexposed HP5 frame IR clears
  at ~2% of the short edge on the right, ~3% top and bottom, ~5% left. A single
  rectangular crop must take the worst edge; per-edge masking need not, and
  `EdgeHolderMask` already expresses per-edge segments.
- **The no-IR path is a first-class path, not a rare one.** For silver stock IR can
  never separate the holder on a *leader*, so every silver `Dmax` measurement takes
  the static inset alone. It deserves a reported, deliberate value rather than a
  safety net — and its size is the user's knob, owned by `holder-depth-mask`.
- **There is no rebate search here** (user, 2026-09-16). `Dmin` is measured over the
  whole effective area of a reference frame, or over a region the user states.
- **The spread within a leader is grain and scanner noise**, not defects — smooth,
  symmetric, no discontinuity — and the median of ~40k samples is reproducible to
  1.4e-4 density on split halves. The wide distribution does not threaten the
  estimate; only a non-central statistic does.
- Provenance is **per-run**: record whether the holder was masked, and how, in the
  report and the existing output sidecar. No persisted pre-processed input.

## Open questions

1. **Does the grid survive as a method at all?** The user enumerated grid and
   whole-area percentile and leans percentile. If percentile wins, `--grid` goes and
   `tiling-uniformity-validator`'s planned retirement of it stands unchanged; if grid
   stays, that task must not retire the flag it keeps. Settle it here, once.
2. **Which percentile** — median, or a trimmed mean, and trimmed where? On a
   distribution this symmetric they agree to a few thousandths, so pick for
   robustness against the asymmetric case rather than for the symmetric one.
3. ~~The fallback fraction.~~ **Answered 2026-09-16**: the inset and its override
   belong to `holder-depth-mask`; this task neither sizes nor duplicates it.
4. **Memory.** Materialising the whole masked region is ~900 MB of `Vec<f32>` on a
   75 MP frame. A 16-bit histogram per channel gives exact percentiles and a
   trimmed mean in O(1) — expected to be the shape here, which means
   `pipeline::memory` gets *new* numbers rather than the current `12·s` term.
5. **What do the film-base *sources* collapse to?** With no rebate search, `Auto`
   and `Region` differ only in where the area came from, which is the "area" input.
   Whether `FilmBaseSource` keeps three variants, or becomes an area plus a method,
   is a CLI/recipe surface question — and `calibration.film_base` is the one knob with no
   default, so whatever replaces it inherits that rule.

## How to Verify

- A masked unexposed frame and the same frame with the holder manually cropped
  away produce the same base, to within the estimator's reproducibility.
- The holder contributes nothing: a synthetic frame with a deliberately extreme
  holder value yields the same base masked as it does cropped.
- Any surviving mixture path (an untrusted user rectangle) keeps p97; the single-
  population path does not.
- Per-edge asymmetry is exercised: a fixture whose holder is deeper on one edge is
  masked per edge, not to the worst edge everywhere.
- Silver-leader `Dmax` takes the fallback and **says so** in the report.
- The `pipeline_version` bump has its fingerprint row, and the report/sidecar
  record the masking provenance.

## Dependencies

- [Decide IR usability by measurement](ir-usability-detection.md)
- [The effective measurement area](holder-depth-mask.md) — **done 2026-09-17**. Call
  `film_base::effective_area(&image, cfg.measure.inset)`; it returns a per-edge
  rectangle plus `holder: Option<HolderDepths>`, where `None` means the holder was not
  measured and all-zero means measured-and-none. Do not re-derive either. Measured
  holder depth on real scans is **2.5-4% of the shorter edge** (31 frames), which is
  the number to reason from — not the 10-15% figure, which measures something else
- [Conversion versioning and baseline comparison](../core/conversion-versioning.md)
- [Roll-fixed Dmax from a fully-exposed reference frame](dmax-reference.md) — this task
  changes `reference_dmax` sampling, which that task introduced

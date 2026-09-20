# Why `--auto-base` refuses every real scan

> **Retired 2026-09-19** — it investigates why that same detector does not
> fire. Kept so existing references resolve; see `docs/nf-migration.md` for
> the migration plan.

> **Parked 2026-09-16 — the detector this task serves is being retired.**
> `film-base/holder-masked-measurement` rebuilds `Dmin`/`Dmax` on **area x method**,
> and nc no longer searches for a rebate band at all (user, 2026-09-16; not shipped,
> so the breaking change is accepted).
> Its question — *why* the detector never fires — stops being load-bearing once the
> detector goes; what survives is question 1, whether a rebate is visible on these
> scans at all, which is worth knowing for `content-fallback`'s tier ordering.
> Do not start this before that task settles what `FilmBaseSource` becomes. The
> evidence below is kept because it is still the best record of how the detector
> behaved on real scans.

## Goal

Find out why the redesigned auto film-base detector has **never resolved a base on a
real full-size scan**, and fix it if the cause is a defect in the detector rather than
a property of the scans. The outcome is either a detector that succeeds on the real
frames that carry a visible rebate, or a written finding that those scans expose no
rebate and auto-base is correctly refusing — in which case the default-on `auto`
source should say so up front instead of failing after a full decode.

## What is known

- `--auto-base` is the default `film_base.source` mode for `estimate`, and the detector
  was redesigned for exactly the real layout (`dark holder → thin inset rebate →
  picture`) in `film-base/auto-base-redesign` (#23, 2026-07-16).
- It has refused on **every real frame ever tried**: real-scan verification a week
  later (2026-07-23, `docs/reports/real-scan-verification.md` row 2) recorded "fails
  loudly on every frame" and accepted it as correct for the layout; `ir-usability-detection`
  (2026-09-04) re-ran it on 11 frames across 6 rolls, with and without the IR mask,
  and it refused on all 11; the 2026-09 whole-roll work notes "the cropped film holder
  defeats the auto rebate-band detector on every frame". Its only real-scan successes
  are on the two `48bit-full` fixtures cited by the redesign.
- The IR holder mask reads **all-holder on 22 of 25 chromogenic frames** at the 0.5%
  probe depth, so on most frames the RGB-only search runs. That mask is along-edge only
  and has no depth (`film-base/holder-masked-measurement` and
  `algo/auto-anchor-interior-measurement` both record this).
- The RGB search marches inward to `REBATE_SCAN_FRAC = 0.10` of the short dimension.
  Measured holder depth on real frames is 2–5% (HP5, `holder-masked-measurement`) but
  **10–15% of one edge** on others (`analysis/conversion-metrics`). On a frame where
  the holder is deeper than the scan cap, the search never leaves the holder.
- A rebate candidate must also be ≥ `MIN_BAND_STRIPS = 2` strips thick, uniform to
  `MAX_RELATIVE_SPREAD = 0.15` on every channel, continuous to `STRIP_CONTINUITY_TOL`,
  and brighter than the interior median by 5% on every channel. Any one of these can
  be the gate that kills a real, thin, slightly noisy rebate.
- Three later tasks build on this detector (`auto-base-neutral-stock`,
  `white-holder-support`, the auto rung of `core/base-acquisition-planner`), and the
  supported workflow today is measure-once-and-reuse with an explicit base.

## Open questions

1. **Is there a rebate to find?** Per real frame, does a visible unexposed band exist
   inside the holder at all, and at what depth? Measure it (IR plane, or a manual
   strip profile via `nc inspect` / a throwaway `#[ignore]` test printing derived
   numbers) before touching the detector. If the holder covers the rebate on every
   scan, no detector can succeed and the right fix is in the refusal, not the search.
2. **Which gate refuses?** For frames that do carry a rebate, instrument the walk:
   scan depth exhausted inside the holder, band too thin, spread over 0.15, continuity,
   or the interior-brightness margin. The refusal message today names none of these.
3. **Is the scan cap the bug?** A 10% cap against a 10–15% holder is the simplest
   explanation for "every frame". Whether to raise it, make it IR-informed (march past
   the holder's measured depth), or make it a knob is a design call once question 2
   has an answer.
4. **What should a default-on mode do when it cannot work?** Today a bare `estimate`
   decodes the whole frame, then refuses. If refusal is the normal real-world outcome,
   consider a cheaper early verdict or a different default.

## How to Verify

- A per-frame table for the real roll set: rebate visible (yes/no, depth), and the gate
  that refused. Derived numbers only, never sample pixels in context.
- If a defect is found: the detector resolves a base on every real frame with a visible
  rebate, that base agrees with the roll's unexposed-frame base within the estimator's
  reproducibility, and the two `48bit-full` fixtures still resolve the same values.
  Any change to the detector's search is a pixel change for `auto` runs; decide the
  `pipeline_version` question rather than inheriting the contested call parked in
  `version.rs` (2026-09-05).
- If no defect is found: the finding is written into `auto-base-redesign`'s section of
  the film-base log, and the refusal message names the measured reason.
- Full CI gate green.

## Dependencies

- [Robust auto film-base detection](auto-base-redesign.md) — the detector under
  investigation.

Not dependencies, but should not start before this has an answer:
[auto-base-neutral-stock](auto-base-neutral-stock.md) and
[white-holder-support](white-holder-support.md) harden edge cases of this detector, and
[base-acquisition-planner](../core/base-acquisition-planner.md)'s auto rung assumes it
can succeed.

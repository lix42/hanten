# Exclude the holder from content-driven measurement

## Goal

Stop the opaque film holder from dominating every measurement taken over a whole
scan. `DmaxSource::Auto` is the visible casualty — it is unusable on a real
full-frame scan today, which disqualifies every content-driven rendering mode — but
the same contamination reaches every statistic read off the frame.

**Measurement only. The output image is never cropped** (user decision 2026-09-12):
dimensions, aspect ratio and pixel count stay exactly as decoded. What this task
changes is *which pixels a statistic is computed over*, not what is written.

## The defect, measured

`algo::density::auto_dmax` takes the 99.5th percentile of corrected densities over
the whole strided buffer. Corrected density is `D = −log10(scan / base)`
(`src/algo/density.rs:271`), so:

| region | scan value | corrected density | where in the distribution |
|---|---|---|---|
| **holder** | at the `SCAN_EPSILON` floor | enormous | **top** |
| brightest scene highlight | dense negative | roll `Dmax` ≈ 1.28–1.38 | upper |
| **rebate / unexposed base** | ≈ base | **≈ 0** | **bottom** |

The holder therefore *owns* the top percentile. On the three fixture rolls `Auto`
resolved to **2.23–2.37** against roll `Dmax` of 1.28–1.38, and every frame rendered
to 0/255 (`algo/reference-anchored-sigmoid`, 2026-08-03).

**The rebate does not contaminate a high percentile** — it sits at the bottom. That
is why this task does not try to detect it (see "What we deliberately do not do").

## Design — take the effective area

**Both cuts belong to `film-base/holder-depth-mask`** (widened 2026-09-16). They are
sequential, not alternatives: IR removes the holder at whatever depth it measures,
then a static inset removes the rebate from what is left, and the inset runs whether
or not the IR cut did. Its default is 5% of the shorter edge and the user can
override it.

This task consumes that one region. It does not resolve a holder depth, does not
size an inset, and does not carry a second fraction.

**And it no longer wires `auto_dmax` either** (settled 2026-09-16; **shipped
2026-09-17**). `holder-depth-mask` shipped that one consumer, region only, so its
inset knob is not an accepted-and-ignored flag. `resolve_dmax` now takes
`(&DensityImage, DmaxSource, Option<[u32; 4]>)` and the region threads from the
orchestrator through `stages::render*`. `convert` *resolves and reports* the area on
every run (so `--measure-inset` is observable). Two predicates gate the rest, split
2026-09-17 because they answer different questions — **add this task's own condition
to whichever one it belongs to**: `measures_over_region` decides which region a
measurement is taken over (today `DmaxSource::Auto` alone, whatever the placement,
so the reported `dmax` has one meaning), and `region_reaches_a_rendered_pixel`
decides whether the IR plane changed a rendered pixel (`Auto` under a
reference-reading anchor), which is what the "IR preserved but not used" note and
`--strict` turn on.

Measured after that wiring: `Auto` resolves **0.76-1.18** across 12 real frames from
6 rolls, against the 2.23-2.37 that motivated this task. So the *contamination* half
is done; what is left here:

- the **loud-failure range check** — an `Auto` result outside the asserted range must
  be an error, not a black frame. **This is now the task's headline**: the region
  landed without it, so the window below is open today;
- `algo::density::measure_balance_range`, whose `hi` has the same defect (its `lo` is a
  code-read inference, still unverified);
- whether `Auto` is worth keeping at all, and what range a plausibility check asserts.

**The seam leaves a window and this task closes it.** As of 2026-09-17 `Auto` reads a
better region while an out-of-range result still renders black. That was accepted
because `DmaxSource::Fixed` is the default and `Auto` is opt-in (`--auto-d-max`), so
only a user who asked for it is exposed — but the check is the half that makes `Auto`
*safe*, and nothing else will land it. Note the measured range above is now evidence
for what "plausible" should mean, which the original framing lacked.

### What this consumer adds

- **Every frame here is a *picture* frame**, so the silver-leader case that makes IR
  decline is never one of them: `auto_dmax` reads the frame being converted, and the
  roll-wide content `Dmax` direction excludes the leader by definition. A dense B&W
  frame that declines simply takes the inset alone. Where that limitation *is*
  load-bearing is `film-base/holder-masked-measurement`, which measures `Dmax` on a
  leader.
- **The loud-failure requirement is this task's, and it is what makes an under-cut
  safe.** An `Auto` result outside the asserted range must be an error, not a black
  frame, so a holder the inset failed to clear surfaces as a refusal rather than a
  silently wrong render — whatever inset the user chose.
- **Reuse the region, not a copy of the rule.** `film_base::auto_interior_pixels`
  already encodes `[cap, cap, w − 2·cap, h − 2·cap]` (~69% of a 3:2 frame), which
  `pipeline::memory` sizes the film-base phase from. A second, drifting copy of that
  rule is the failure mode.

### What we deliberately do not do

- **No output crop.** Not from IR, not from the inset. The written image keeps holder,
  rebate and picture, at the decoded dimensions.
- **No rebate detection for measurement purposes.** The rebate sits at `D ≈ 0` and does
  not disturb a high percentile. Removing it from the *picture* is the user's manual
  crop, not nc's inference.

## Who consumes this

`auto_dmax` is the visible one, but the region is shared and should have one owner:

- `algo::density::auto_dmax` — high percentile, contaminated by the holder.
- `algo::density::measure_balance_range` (`BalanceRange::Auto` / `--auto-balance-range`)
  — takes **both** `BALANCE_LO_PERCENTILE = 0.005` and `BALANCE_HI_PERCENTILE = 0.995`,
  so its `hi` has the same defect. *Unverified:* whether its `lo` is disturbed by the
  rebate is a code-read inference, not a measurement — check it before scoping it in.
- `algo/content-aware-sigmoid-toe` — a content-derived toe or black point reads the
  **low** end, which is where the rebate lives. Worth measuring before that task starts.
- `film-base/dmax-anchor-reliability`'s roll-wide-content direction inherits the same
  contamination.

## Plumbing

Stages stay pure: the orchestrator resolves the region and passes it in, exactly as it
does the film base. `auto_dmax` already strides for cost, so a region restricts the walk
rather than adding one, and its transient sample buffer is capped at
`AUTO_DMAX_MAX_SAMPLES = 1 << 20` — so no new full-frame allocation and, probably, no
change to `pipeline::memory`. Confirm rather than assume.

Note `resolve_dmax` has two callers — `algo/density.rs` and `algo/sigmoid.rs:296`, the
latter anchoring its S-curve on the *same* resolved `Dmax` rather than inventing a second
measurement. Do not fork that.

## Open questions

- ~~Whether cut 2 is this task's own fraction.~~ **Answered 2026-09-16**: both cuts,
  the inset default and its override belong to `film-base/holder-depth-mask`, which
  resolves one effective area for every measurement path. This task carries no fraction
  of its own.
- **What does a plausibility check compare against?** The task's original framing — "an
  `Auto` anchor above the plausible range must fail loudly" — compares a *scene* statistic
  against a *film-density* range. Those are different quantities (see below), so the check
  must state what range it is really asserting rather than silently borrowing the leader's.
- **Does `Auto` remain worth keeping at all?** It is already demoted to opt-in, and
  `film-base/dmax-anchor-reliability` is weighing whether the default should depend on a
  leader-measured anchor at all. Fixing `Auto` and retiring it are not exclusive, but the
  two tasks should not answer this differently.

## A naming trap worth recording

`Auto`'s "Dmax" is **not the film's Dmax**. The three sources measure different things
into one anchor slot:

- `Explicit` (`--d-max`) — measured once from a fully-exposed **leader**, reused across
  the roll. A property of **the film**.
- `Fixed` (default) — `NOMINAL_DMAX = 1.3`, a scene-independent constant.
- `Auto` (`--auto-d-max`) — the 99.5th percentile of **the frame currently being
  converted**. A property of **the scene**: the brightest content becomes display white.

And the holder is a third thing again — not film density at all, but "no light reached
the sensor". Conflating the three is how a content statistic came to be judged against a
film-density plausibility floor.

## How to Verify

- An `Auto` result outside the asserted range is a loud error on every fixture roll,
  including the frames that used to resolve 2.23–2.37.
- `measure_balance_range`'s `hi` is measured over the region; whether its `lo` needed
  it is answered with a measurement, not a code read.
- **Output dimensions are byte-for-byte unchanged** on every path — the exclusion is
  visible only in resolved statistics and the report, never in the image.
- On a frame where IR is absent or measures unusable, the inset runs alone and the
  report **says so** — distinguishing "no holder was found" from "the holder was not
  measured".
- A frame whose holder wraps the entire border is still cut by IR, not passed through to
  the inset alone — the all-holder case is the majority, not an error.
- An `Auto` result outside the asserted range is a loud error, not a black image.
- `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo build`,
  `cargo test` pass.

## Dependencies

- [Reference-anchored sigmoid calibration and redesign](reference-anchored-sigmoid.md)
- [Robust auto film-base detection](../film-base/auto-base-redesign.md)
- [The effective measurement area](../film-base/holder-depth-mask.md) — resolves the
  per-edge holder depth **and** the static inset into one region (split out of
  `holder-masked-measurement` on 2026-09-13, widened to own both cuts on 2026-09-16).
  This task consumes that region rather than inventing a second one

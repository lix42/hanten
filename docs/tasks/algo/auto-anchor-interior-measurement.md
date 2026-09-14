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

## Design — two cuts, in order

They are **sequential, not alternatives**, and they remove different things:

1. **IR removes the holder**, at whatever depth it measures on this frame.
2. **A fixed fractional inset then removes the rebate** from what is left.

Cut 2 is therefore sized for a thin inset band, not for a holder — which is why
holder depth varying 2–15% across scans does not set its default. Where IR is
absent, cut 2 runs alone and is the only defence; see its note below.

### 1. IR-based holder exclusion (the first cut, when IR permits)

The IR plane separates holder from film independently of image density. Where it can,
it is the better region source than any geometric rule.

- **Consume the existing verdict, don't re-derive it.** `film_base::ir_separability`
  already decides per frame whether IR can separate holder from film, and
  `ir_holder_mask` already keys on it. Ask it; don't ask whether an IR plane exists.
  This is how the code behaves today, not a new burden.
- **No IR, or IR that measures unusable → do nothing here.** Fall through to (2).
  Silver's IR transmission falls with accumulated density, so a dense B&W frame may
  decline — cut 2 then runs alone, and it does not gate this path. The
  limitation is **not** a reason to hold IR-based exclusion back: every consumer here
  measures *picture* frames (`auto_dmax` reads the frame being converted, roll-wide
  content `Dmax` excludes the leader by definition), and the uniformly-opaque silver
  leader that IR genuinely cannot handle is never one of them. Where that case is
  load-bearing is `film-base/holder-masked-measurement`, which measures `Dmax` on a
  leader.
- **The existing mask has no depth, and that is the work.** `EdgeHolderMask` is a list
  of segments *along* each edge at `IR_HOLDER_PROBE_FRAC = 0.005`; the code comment is
  explicit that it "restricts along the edge only, not in depth". Excluding the holder
  from a statistic needs to know how far in it reaches. That is new machinery, not a
  reuse — though the IR plane makes the depth test a simple threshold march, and
  `film-base/auto-base-redesign` already marches inward for the rebate.
- **Do not inherit the all-holder decline.** `ir_holder_mask` returns `None` when the
  mask leaves no film to search *along* any edge — measured on **22 of 25 real
  chromogenic frames**. In the along-edge design that is correct: no film along an edge
  means nothing to search. In a **depth-aware** design the same reading means only "the
  holder wraps the whole border", which is the *normal* case, and the answer is to keep
  marching inward rather than decline. Porting the guard across would send 22 of 25
  frames to cut 2 alone, leaving a 5% inset doing the holder's job — precisely the
  failure this two-cut design avoids.
- **IR present but no holder found** is then a real verdict, not a shrug: the frame most
  likely arrived already cropped. Cut 2 still runs, and the report should record that
  cut 1 found nothing rather than that it was skipped.

### 2. Fixed fractional inset (the second cut — the rebate)

A blind fraction of each edge, with a default and a user override. No detection: the
rebate is not searched for, it is simply inset past. `film-base/auto-base-redesign`
describes it as "a thin inset band", which is what this cut is sized for.

**Default 5%, and tweak it when there is evidence** (user, 2026-09-12). After cut 1
there is no holder left to clear, so the number only has to cover a rebate. It is a
default, not a measurement, and it is deliberately not being derived from one.

**The no-IR case is different, and 5% is probably wrong there — decide it explicitly.**
With no IR plane, cut 1 does not run and this inset is the *only* exclusion, so it is
back to doing the holder's job. And there **is** a measurement, contrary to an earlier
draft of this file: `analysis/conversion-metrics` records that "a 5% inset often does
not clear a real holder … the holder can occupy 10–15% of one edge" — a holder-occupancy
figure, not a remark about that tool's own design. `film-base/holder-masked-measurement`
separately measured 2–5% on the HP5 frame, so real holder depth spans roughly 2–15%
and one number cannot serve both paths.

So the open decision is **whether cut 2 takes a different default when cut 1 did not
run** — 5% after an IR cut, something nearer 10–15% without one — or whether the no-IR
path simply refuses to measure content-driven statistics at all. Do not settle it by
inheriting the IR path's number.

**What keeps either choice safe is the loud-failure requirement** — an `Auto` result
outside the asserted range must be an error, not a black frame — so an under-cut
surfaces as a refusal rather than a silently wrong render. The two design elements are
load-bearing for each other; do not ship any inset default without the range check.

**Align with an existing fraction rather than adding a sixth** if one fits.
`REBATE_SCAN_FRAC = 0.10` and `IR_HOLDER_PROBE_FRAC = 0.005` already exist, and
`film_base::auto_interior_pixels` already encodes the interior rectangle
`[cap, cap, w − 2·cap, h − 2·cap]` (~69% of a 3:2 frame) that `pipeline::memory` sizes
the film-base phase from. A second, drifting copy of that *rule* is the failure mode;
a distinct default *value* for a distinct job is not.

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

- **Is cut 1 shared with `film-base/holder-masked-measurement`, or its own?** That task
  builds the per-edge holder mask plus a fixed-fraction fallback for `Dmin`/`Dmax`
  measurement. Two copies of "where is the holder" is the drift risk; it is a dependency
  for that reason. Note its fallback and this task's cut 2 are **not** the same thing —
  that one substitutes for a missing holder mask, this one removes the rebate — so
  sharing the mask does not mean sharing the fraction.
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

- On each fixture roll, `Auto` resolves **below** the roll's reference `Dmax` and within
  the frame's bright content, not at 2.2+. Confirm with `pipeline::shadow_metrics`.
- A synthetic committed fixture with a deliberately opaque border proves the border is
  excluded, so the regression is caught with no external assets.
- **Output dimensions are byte-for-byte unchanged** on every path — the exclusion is
  visible only in resolved statistics and the report, never in the image.
- On a frame where IR is absent or measures unusable, cut 2 runs alone and the report
  **says so** — distinguishing "cut 1 found no holder" from "cut 1 did not run".
- A frame whose holder wraps the entire border is still cut by IR, not passed through to
  cut 2 — the all-holder case is the majority, not an error.
- An `Auto` result outside the asserted range is a loud error, not a black image.
- `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo build`,
  `cargo test` pass.

## Dependencies

- [Reference-anchored sigmoid calibration and redesign](reference-anchored-sigmoid.md)
- [Robust auto film-base detection](../film-base/auto-base-redesign.md)
- [A depth-aware holder mask](../film-base/holder-depth-mask.md) — builds the per-edge
  holder depth and its fixed-fraction fallback (split out of `holder-masked-measurement`
  on 2026-09-13). This task consumes that primitive rather than inventing a second one

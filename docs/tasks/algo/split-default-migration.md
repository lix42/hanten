# Activate the reconstruction / render split as the default

## Goal

Make the split the shipped default: reconstruction stops shaping tone, the display
operator carries the character. That is a `pipeline_version` bump with a
before/after report — the part `algo/reconstruction-render-curve-split`
deliberately left out of its own scope.

## Input from `algo/film-stock-profiles` (2026-09-08)

- **This task's stated blocker is resolved for named stocks.** The shoulder this migration
  removes was hiding a per-channel error; inverting a stock's published curve removes
  blue's part of it by construction (residual +0.09 against +1.26). `film-base/dmax-per-
  channel-reduction` still owns the no-stock path.
- **There is now a third candidate default**, not just a shoulder-less sigmoid: the
  `characteristic` curve, which carries no slope or anchor at all because it reads both off
  the film. If the migration's goal is "the reconstruction sheds both knees and the display
  operator carries the character", this is the form that does it completely.
- **It changes what the display stage must do.** The characteristic curve hands the display
  scene-referred exposure, so `--display-tone none` is *refused* on both SDR and HDR (the
  reconstruction no longer bounds itself at the render's ceiling). Measured picture content
  reaches p99.99 **+3.64 stops** over diffuse white and never exceeds 4 stops on 21 frames,
  so `reinhard` with `--display-tone-headroom 4` covers it — the shipped default of 6 wastes
  two stops. Whether that headroom default should move is arguably `output/presets`.
- **It un-inerts the HDR path.** Same frame, same tone: MaxCLL 101 nits under the shipped
  sigmoid (half of diffuse white — the known inert-gain-map defect) against 999 under both
  a shoulder-less sigmoid and the characteristic curve.
- **Do not migrate before the green residual is understood** (`io/scanner-density-
  calibration`): +0.40 mean, +1.00 on one roll, and on the worst-affected stocks
  `generic-c41` currently looks better than the matching profile.

## Why

The split is decided, not speculative. `algo/reconstruction-render-curve-split`
reached a positive verdict on seven real frames at matched lightness, over a user
visual verdict from `output/display-tone-mapping`. Everything needed to render it
already ships and is reachable from the CLI today; what has not happened is making
it what `nc convert` does with no flags.

It is a separate task because activation inherits a problem the split itself does
not own — see the blocker below — and because a default migration is its own kind
of work: a version bump, a drift-gate row, a measured report, and a guide update.

## Input from `algo/conversion-presets` (2026-09-09)

The default migration this task owns is now the **last step** of that task, not a separate
piece of work, and two of its obstacles have measured answers:

- **How the default moves without breaking `film-master`.** A preset must not set
  `output.preset`; the non-display presets (`legacy`/`custom`/`film-master`) resolve their
  own tone and exposure. Verified that they refuse `reinhard` outright, and that
  `film-master` refuses any non-default `print_exposure` — so a global default move would
  have made a bare `nc convert --output-preset film-master` fail.
- **Which reconstruction.** `characteristic-generic` is the proposed default, which lands on
  the half of this task's blocker that is still open: `film-base/dmax-per-channel-reduction`
  owns the no-stock path. Read that note above before migrating.

## Open questions

- **How much of the shape moves?** The measured answer is "both knees off", but the
  migration has to decide whether the default anchor placement moves with it. Keeping
  `MidAtDmaxFraction(0.5)` costs a measured **0.21–0.28 EV darker** than the
  lightness-matched anchor (it anchors above the solved one) — inside what
  `print.print_exposure` corrects, so plausibly fine, and it avoids shipping the
  uncalibrated 0.626 offset as a constant.
- **Is the default display tone the same one?** `--display-tone reinhard` at the
  6-stop default is what was reviewed. **Half of that concern is now gone**: since
  2026-09-09 the operator preserves mid-grey by construction
  (`extended-reinhard-mid-preserving-v2`), so it no longer darkens the midtones and
  the review's renditions were re-rendered brighter. What remains is **0.86 stop at
  diffuse white** — still a rendering-intent call the migration has to make, and still
  one no measurement can decide, but it is now a highlight-contrast question rather
  than an overall-brightness one.
- **What does the report have to say?** The split changes what a stage *does*, which
  is CLAUDE.md's "fifth spot" — check the prose claims, not just the values.
- **Does anything downstream assume the old bound?** Reconstruction currently holds
  `lin ≤ 1.0` under a positive shoulder; without it the master and both display
  sources go over-range by design.

## Known vs unknown

**Known:** the rendition is reachable today (`--sigmoid-shoulder 0 --display-tone
reinhard`); `film-master` accepts it at exit 0 and needs no change; the gain map goes
live and its plateau share improves 10–25x; `PIPELINE_FINGERPRINTS` needs a new row
and a historical row must never be edited in place.

**The new row's `render` hash may not be portable, and this is the single most
important thing to read before writing one.** `algo/characteristic-curve-coverage`
(closed 2026-09-10) established by observation — not by argument — that **x86_64 and
macOS return different `f32` results from `log10f`** on two of the fifteen
`stages::golden::pixels()` samples under this curve. The chain has two libm calls
(`log10` in `to_density`, `10^` in the curve), and a 1-ULP difference in the first is
amplified by `ln(10)·d·(1/γ_local)` — up to 62 pixel ULPs on that vector.

The golden there survives it with a **derived per-sample window**
(`stages::golden::reachable_window`, which renders every density a 1-ULP-accurate libm
can return). **A fingerprint row has no window at all** — it hashes raw f32 bits — so it
is a strictly harder bar, and the current vector is known to fail it on at least those
two samples. Do not assume `golden::pixels()` carries over: budget for choosing sample
values whose *rendered* pixels are identical on both targets, and verify by running CI
on both rather than by any margin argument. Two threshold-based arguments were tried
during that task and both were unsound; the progress log records why.

Note also that moving `golden::pixels()` itself is not free — it is shared with every
historical row, whose meaning would shift with it. Adding a separate vector for the new
default's fingerprint is likely the cheaper answer.

## The blocker, and why it is a real one

**`film-base/dmax-per-channel-reduction` must land first.** The sigmoid's shoulder
was *hiding* a model error: measured on the uniformly-exposed leader — a target with
no scene content, so every deviation is model error — Gold reads B/G **1.826**, Portra
R/G **1.676**, Ektar B/G 1.170, i.e. 17–83% off neutral on a grey target. The shoulder
washes highlights toward white and drains the cast along with the detail; shoulder-less,
it survives into the highlights. That task's own analysis calls the per-channel term
"redundant under the exponential, not under the sigmoid, **which is the intended
default**" — a premise this migration overturns. Shipping the split as the default
before it lands means shipping a visible cast on Gold and Portra.

## How to Verify

- A `pipeline_version` bump with its own `PIPELINE_FINGERPRINTS` row, and a
  before/after report under `docs/reports/`.
- Neutrality checked on the leader for each stock, since that is the thing the old
  default was hiding.
- `docs/using-nc.md` updated by running the binary, not by reading the diff.

## Dependencies

- [Reconstruction / render curve split](reconstruction-render-curve-split.md)
- [Per-channel Dmax and the gray-mean reduction](../film-base/dmax-per-channel-reduction.md)
- [Named conversion presets](conversion-presets.md)
- [Pin the characteristic curve against regression](characteristic-curve-coverage.md) —
  **done 2026-09-10.** The curve now carries four property tests over the real
  `algo::reconstruct` plus a 1-ULP golden, so the default can move onto pinned wiring.
  The `PIPELINE_FINGERPRINTS` row was deliberately left to this task; see the
  portability note under *Known vs unknown* before writing one

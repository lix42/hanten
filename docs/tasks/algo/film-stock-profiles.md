# Film-Stock Profiles

## Goal

Let a user name the film stock they shot and have reconstruction use that stock's
published reference densities instead of generic ones — without making stock
selection mandatory. The output is a selectable registry of known stocks carrying
the per-stock numbers reconstruction needs, each traceable to a manufacturer
datasheet, plus a generic C-41 fallback that keeps unknown stocks working.

## Design

**What a profile holds.** Only quantities that are genuinely stock-dependent and
published. From the Kodak datasheet research done under
[reference-anchored-sigmoid](reference-anchored-sigmoid.md):

- the *Judging Negative Exposures* aim densities — grey card and the lightest step
  of a paper grey scale (≈ diffuse white), Status M, red channel, **absolute**
  (base+fog included), each published as a **range** (±0.05 professional, ±0.10
  consumer) whose width is itself a limit on what the profile can promise: ±0.05 is
  ±0.34 EV of exposure placement at contrast 2.07;
- the mid→white difference `Δ` (base-independent, since a difference cancels
  base+fog) — but read it off the **characteristic curve**, not the aim table, and use
  the table as a cross-check; see Constraint 2;
- the **mid-above-base offset** `mid aim − D-min`, which is what a reference-free
  `MidAtBaseOffset` anchor needs;
- the **per-channel structure** — each layer's own `D-min` and gamma. This is the only
  per-channel datum published anywhere (the aim tables are red-only), and it says nc's
  `density.scale = [1,1,1]` default is wrong on every stock measured.

Measured values, method and provenance live in the 2026-09-04 entry of
[`progress/algo.md`](../../progress/algo.md) — 8 colour stocks digitized from the
published curves. Don't restate the tables here; this file tracks the work.

**Constraint 1 — measured roll `film_base` stays authoritative; published `D-min` is
nominal only.** This repo already defines `Dmin` as a property of stock **plus
development plus scanner settings**
([estimate-reuse-output](../film-base/estimate-reuse-output.md)). Base fog and the
characteristic curve shift with processing, storage and the individual roll, so
selecting a stock must never substitute a nominal standard-process base for the
measured one — that would misplace tones on a real roll. Store published `D-min` as a
reference/diagnostic value and keep measured `film_base`, and any measured offset,
authoritative in the render path.

**Constraint 2 — resolved for Kodak still stocks (2026-09-04), with one risk
deliberately deferred.** The 2026-08-02 values came off the *Spectral-Dye-Density*
chart, which is per-wavelength diffuse density — genuinely the wrong quantity, since
Status M is a prescribed **broadband** response. But the **characteristic curve is
plotted in Status M**, the same densitometry as the aim table, so its `D-min` is a
Status M density and no spectral integration is needed. Those sheets are vector art, so
digitizing them is exact (±0.002 between two independent extraction paths), not an
eyeball read. This unblocks the mid-above-base offset.

What remains is a **different** risk, and it is postponed by decision: our scanner is
not a Status M densitometer, so a per-channel *slope* difference between the two scales
would misplace anything taken from the sheets. A per-channel gain cannot cause it (it
cancels in nc's base division). Quantifying it is
[scanner density calibration](../io/scanner-density-calibration.md) and needs one
known-neutral target — a ColorChecker frame, whose neutral row gives six points of the
ramp in a single exposure. Until that exists, datasheet numbers are applied as a
hypothesis judged on rendered results. The **leader cannot stand in**: measured leaders
do not reproduce the published per-channel divergence at all (one fixture roll has red
densest, which no C-41 neutral response gives), and that comparison cannot separate a
non-neutral leader exposure from a scanner-slope error.

**A generic default is viable and matters — but it is not equally good for every
field.** Most users will not know or will not say, so stock selection must be a
*refinement*, never a precondition. A named stock absent from the registry is a loud
error; *no* stock named resolves to generic without complaint. What the measurements
say about the generic values:

- the per-channel **gain** is nearly stock-independent (blue 0.859, range 0.850–0.881
  over eight stocks — a 3.6 % spread on a 14 % correction), so a generic `density.scale`
  captures most of the effect and is a real improvement on `[1,1,1]`;
- the per-channel **offset** is not (−0.101…+0.015, splitting by tier), so it stays 0
  unless a stock or a measurement supplies it;
- `Δ` is genuinely stock-dependent — the legacy NC/VC pair (0.36 vs 0.41 at the same
  speed, corroborated by their curves) proves it — so a generic `Δ` is a fallback, not
  a law. It looks constant across today's line only because today's C-41 stocks are all
  similar-contrast.

**Data shape follows `pipeline/colorimetry/`.** That module is the established
pattern in this repo for reference data that must not drift silently: source data
with provenance, separately pinned literals the runtime reads, and a `#[cfg(test)]`
audit. Reuse the split rather than inventing a second convention. Every number carries
its publication id (E-4046, E-4050, E-4051, E-7022, …) and its **kind**:
`tabulated` (the aim table), `curve-digitized` (read off the Status M characteristic
curve — the right quantity, exact to ~±0.01) or `chart-read` (the superseded
spectral-dye-density reads: a *different quantity*, never to be consumed). The third
kind is load-bearing: folding `curve-digitized` into `chart-read` would forbid the use
that motivates this task.

**Knob shape.** One enum field, per the project convention that mutually exclusive
knobs are never parallel fields — e.g. `FilmStock { Generic (default) | Ektar100 |
Portra160 | Gold200 | … }`. It is a **conversion knob**, so it spans all four
coupled spots (CLI `*Overrides`, recipe `*Params`, a `merge` arm, a `validate`
check) and appears in the resolved report with provenance. Recipe placement follows
design-spec §9; `input.film_type` (`FilmType`) already exists as the *input-medium*
axis and is a different quantity — a stock profile is not a film type. Decide
explicitly whether the two compose or one constrains the other.

**Roll-fixed.** Stock is a property of the roll, so `nc roll` must treat it like
`film_base` and `reconstruction.curve.dmax`: a per-frame override is applied but
raises a loud, `--strict`-promotable warning.

**Status of the build (2026-09-06).** The reconstruction half has shipped as the
`characteristic` density curve: `--film-stock` / `reconstruction.curve.stock`, ten
digitized stocks including a derived `generic-c41`, provenance in the report, roll-fixed
with a `--strict`-promotable per-frame warning, and every parametric knob refused rather
than ignored. It is **opt-in — no default moved**. The registry is now
re-derivable rather than transcribed: the publications live in `docs/datasheets/`, a
committed digitizer produces `curves.json` from them, and a `cargo test` audit pins the
Rust literals to that extraction. What that leaves for this task:

- the **generic fallback for the *parametric* path** — a `density.scale`/`offset` pair for
  users who do not name a stock and do not select the characteristic curve. The measured
  generic gain is solid (blue 0.860, range 0.850–0.887); the offset is not (−0.101…−0.002).
  Coordinate with `film-base/dmax-per-channel-reduction`, which owns where a per-channel
  term belongs;
- whether the characteristic curve should become the **default** — that is
  `algo/split-default-migration`'s call, with a `pipeline_version` bump, and it wants the
  scanner-to-Status M question answered first;
- **B&W**, which cannot use this shape at all (contrast index is set by developer, time and
  temperature, not by the film) — `algo/bw-support`;
- **the green residual** (found by the 2026-09-06 visual review, measured at +0.08…+1.00
  stops per unit density and varying per stock in an order the datasheets do not predict).
  Blue transfers; green does not. The leading cause is the **cross-channel term** ACES
  applies before its curves and nc does not — `io/scanner-density-calibration`, which still
  needs one known-neutral frame. Until then `generic-c41` renders better than the matching
  profile on the sheets whose own halves disagree, which is a reason to fix the matrix
  rather than to prefer the generic;
- **two sheets disagree with themselves** — Ektar 100 by +11%, UltraMax 400 by −11% between
  their aim table and their own curve. Pinned by `aim_table_agrees_with_the_curve`; what to
  *do* about a stock whose published data is internally inconsistent is undecided (ship it,
  warn on it, or withhold it).

## Implementation Suggestion

- Land the registry only *after* `reference-anchored-sigmoid` settles which
  parameters are stock-dependent. Building it earlier risks storing fields nothing
  reads.
- **Settle the reconstruction shape first.** If reconstruction inverts each channel's
  digitized characteristic curve rather than fitting a parametric curve, the registry
  stores curves, not scalars, and the per-channel `scale`/`offset` question disappears.
  That decision changes what this task builds, so it should not run second.
- Prefer the **curve** over the aim table for `Δ`, and cross-check them: `Δ_tab / γ` must
  equal `log10(white/mid reflectance) ≈ 0.69`. It does to ±0.05 density on every stock
  except the two 800-speed sheets, which tabulate `Δ` 0.25 against their own curves'
  0.36 — those aim ranges are ±0.10, so the tabulated difference is consistent but
  uninformative.
- Per-channel values come from the curves, not the aim tables (which are red-only) — do
  not invent numbers no sheet states.
- Coordinate with
  [dense-base Dmax plausibility](../film-base/dense-base-dmax-plausibility.md): that
  task wants the C-41-calibrated plausibility floor made stock-relative (Harman
  Phoenix false-alarms today). It is deliberately *not* dependent on this task — it
  can loosen its floor without a registry — but the two must not solve
  stock-awareness twice.

## How to Verify

- A recipe naming each registry stock round-trips, and the resolved report shows
  the stock plus the reference densities it resolved to, with provenance.
- Naming no stock resolves to the generic C-41 profile. (This bullet used to ask for
  byte-identity with the *pre-task default*; that premise died with the shape change —
  `generic-c41` is a different curve and is meant to render differently. The invariant that
  matters, and holds, is that a **bare `nc convert` is unchanged**, pinned by the unmoved
  `version::PIPELINE_FINGERPRINTS` row.)
- An unknown stock name fails loudly with exit 2 and lists the accepted names.
- A per-frame stock override under `nc roll` raises the roll warning and `--strict`
  promotes it.
- Every registry number carries its publication id **and revision date** and is covered
  by a test, so a typo cannot ship silently; a test asserts no `chart-read` value reaches
  a render path (`curve-digitized` values may).
- `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo build`, `cargo test` pass.

## Dependencies

- [Reference-anchored sigmoid calibration and redesign](reference-anchored-sigmoid.md)

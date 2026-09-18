# Light film holder support

> **Parked 2026-09-16 — the detector this task serves is being retired.**
> `film-base/holder-masked-measurement` rebuilds `Dmin`/`Dmax` on **area x method**,
> and nc no longer searches for a rebate band at all (user, 2026-09-16; not shipped,
> so the breaking change is accepted).
> This one is the clearest casualty: under IR a light holder is still opaque, and
> without IR the holder is cleared by a blind user-sized inset — neither path asks
> about polarity, so `--holder white|black` has nothing left to configure.
> Do not start this before that task settles what `FilmBaseSource` becomes. The
> evidence below is kept because it is still the best record of how the detector
> behaved on real scans.

## Goal

Let film-base auto/border detection work when the film holder is **white** (light)
rather than the assumed dark surround, via an explicit CLI/recipe control.

## Background

Auto detection assumes the frame is surrounded by a near-black holder and that the
unexposed rebate is the bright band just inside it (see `auto-base-redesign`). Some
holders are white/light, which inverts that assumption: a bright surround would be
mistaken for the rebate. The polarity cannot always be inferred reliably, so make it
an explicit knob.

**This is the RGB-only fallback to `ir-holder-detection`.** Since
`ir-usability-detection` (2026-09-04) the IR holder mask is applied whenever the
frame's own IR plane *measures* able to separate holder from film; `--film-type`
gates nothing. A light holder is still opaque, so it reads dark in IR and polarity
stops mattering on that path. What remains for this knob is the genuinely IR-less
set: **HDR 48-bit scans with no IR plane**, scans whose IR page is identified by shape
alone, and any frame whose own film is IR-opaque (a fully-exposed silver-halide frame,
and silver film generally past the density at which accumulated silver blocks IR).

**Re-evaluate before starting** (2026-09-13): `film-base/auto-base-real-scan-refusal`
records that the auto detector has never resolved a base on a real full-size scan. A
polarity knob on a detector that does not fire has no observable effect; wait for that
task's verdict.

## Design

Add a single mutually-exclusive knob (default `black`):

- CLI: `--holder white|black`
- Recipe key: `film_base.holder` (extends the `film_base` section; keep the
  `deny_unknown_fields` contract and the flag↔recipe↔merge↔validate wiring noted
  in `cli-framework`).

Thread it into the auto detector's RGB holder classification so "holder" is classified
by the configured polarity while the rebate remains the bright, uniform, inset band.
The IR mask supersedes it when active. Only affects `auto`; `region`/`explicit` are
unaffected.

## How to Verify

- `--holder white` on a synthetic light-holder `holder → rebate → picture` image
  finds the rebate; the default (`black`) would misfire on it.
- Round-trips through the recipe (`film_base.holder`) and rejects unknown values.
- Default behavior is unchanged for dark-holder scans.

## Dependencies

- [IR-assisted film-holder detection](ir-holder-detection.md) — this is the RGB-only
  fallback for the no-IR path, so it builds on the holder-classification dispatch that
  task establishes (which in turn builds on `auto-base-redesign`, a transitive dependency).

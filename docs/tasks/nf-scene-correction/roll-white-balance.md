# A roll-level white balance

## Goal

A white balance measured once per roll from the roll's own content, removing the cast
that is constant across the roll — film, development, scanner — and keeping the scene's
light. Today scene correction offers only stated gains or a per-frame estimate.

## Why it exists

[`desaturation-band-fit`](../nf-look/desaturation-band-fit.md) found that
[the path to white](../nf-look/path-to-white.md)'s saturation band is placeable **only
behind a roll-level white balance** ([`docs/spike/desaturation-band.md`](../../spike/desaturation-band.md)):

- With none (the new chain's default), Ektar's whites carry as much cast as skin, so no
  band can clean one without flattening the other.
- A per-frame estimate reads a sunset as the cast and removes it before the band can
  protect it.

The intent (user, 2026-09-23) is that there is no correct white balance, only a choice,
and one global gain cannot keep a sunset on the sky while removing it from a cloth; so
keep the scene's light and remove only what a whole roll shares. One sunset frame barely
moves a roll-level statistic.

## Known

- **Not the film base or the leader.** The base is the decode's divisor and already
  neutralises black, where a gain does nothing. The leader renders 1.5–3.5 stops above
  diffuse white and asks for gains the wrong way round.
- **The candidate that worked in the fit:** the pooled per-channel 99th percentile of the
  roll's pixels, dropping any pixel within 0.1 density of the leader on any channel (a
  guard against a fully exposed frame mixed into the roll). With the median of per-frame
  95th-percentile gains (today's `--auto-wb percentile`), Ektar's top 5% is sky, not
  white, and its blue gain comes out 0.79 against the whites' 1.28.
- The existing per-frame estimators (`pipeline/white_balance.rs`) are the statistics to
  build on; a Python replica matched `--auto-wb percentile` to 0.01.

## Open questions

- **Where it runs.** A roll statistic needs every frame, so `roll` measures it, but
  `convert` renders one frame: a measured value carried in the recipe (the way
  `calibration` carries a roll's base) is the obvious shape, not a settled one.
- **The estimator.** No variant clearly won on 29 patches — percentile, pooled vs median
  of per-frame, per-channel percentile vs the colour of the brightest pixels. The leader
  guard is untested: none of the measured rolls holds a blown frame, and its margin must
  stay small (at 0.2 density it removed 16% of Ektar, whose white sits 0.15 below its
  leader).
- **How it relates to the anchor.** Candidate C's white level is the same family of
  statistic (a high percentile of the roll). One measured "roll white" could supply both
  the level and the colour; whether it should is `nf-calibration/anchor-comparison`'s
  business as much as this task's.
- **Portra.** No bright whites were found on its frames, so its gains were never checked
  against marked whites.

## How to Verify

- On rolls with marked whites, the roll white balance lands near the gains those whites
  ask for, and the desaturation band's W/C separation matches the fit's.
- A sunset frame keeps its warmth; its roll's gains move little when it is removed.
- A roll with a fully exposed frame mixed in gives the same gains with and without it.

## Dependencies

- [Scene correction as a named stage](stage.md) — **done**; it owns white balance and
  exposure, and this is a new source for the first.

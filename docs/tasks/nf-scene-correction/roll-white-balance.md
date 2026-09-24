# A roll-level white balance

## Goal

A white balance measured once per roll from the roll's own content, removing the cast
that is constant across the roll — film, development, scanner — and keeping the scene's
light. Before this task, scene correction offered only stated gains or a per-frame
estimate.

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

## Design (decided 2026-09-23)

- **Measured by a roll command, frozen into the recipe.** Rendering is per frame and
  stays pure; a value that needs every frame is measured once and carried, the way `roll`
  already asks for a frozen film base. The command writes explicit gains for
  `scene_correction.white_balance`, so `convert` reproduces a roll frame from the recipe
  alone. A `roll` pre-pass was rejected for exactly that reason.
- **Gains only.** The same measurement could also yield the roll white's *level*, which
  `nf-reconstruction/gamma-split`'s candidates C/D need; that is left for when a consumer
  exists. Share the measurement then, not a parameter — each stage keeps its own value.
- **Per-frame auto white balance retires** on the new chain (`gray-world`, `percentile`,
  `--auto-wb`), with a migration message naming the roll command. The current chain's
  copy leaves with `print.*` (`nf-retire/print-prefix-rename`).
- **Measured at the decode's own output**, before scene correction. The look's contrast is
  a channel-equal power pivoted at mid-grey, so a white the gains make neutral stays
  neutral under any contrast — but the gain *values* are contrast-specific, which is why
  the band fit's numbers (taken under candidate C's per-roll contrast) do not carry.
- **Not the decode's `density.offset`.** A gain is a per-channel density offset only before
  the 3×3; the decode stays fixed and stock-agnostic, and roll cast is scene correction's.
- **The leader guard reads a measurement written fresh.** `nf-retire/dmax-machinery`
  retires the leader-`Dmax` anchor and says a content white's guard must not reuse it.
- **Over the effective area**, which gives `measure.inset` a consumer once the per-frame
  estimate is gone.

## Known

- **Not the film base or the leader as the reference.** The base is the decode's divisor
  and already neutralises black, where a gain does nothing. The leader renders 1.5–3.5
  stops above diffuse white and asks for gains the wrong way round.
- **The estimator (study 2026-09-23, at the fixed decode):** pooled per-channel p99 of the
  roll's picture pixels, excluding any pixel within 0.1 density of the leader's median on
  any channel. It lands within 0.03 of the gains the marked whites ask for on Gold200 and
  Ektar. **The leader guard is not optional:** one fully exposed frame mixed in moves the
  gains 0.4–1.3 stops without it and not at all with it. Dropping a sunset frame moves them
  ≤ 0.03 stops.

## Open questions

- **Small rolls.** On 09-11-Portra400 (11 frames, underexposed) dropping one frame moves
  the gains up to 0.2 stops; whether to warn below some frame count is open.
- **Portra's whites.** No bright whites were found on its frames, so its gains were never
  checked against marked whites.
- **A roll without a scanned leader** cannot be guarded: warn (and let `--strict` refuse)
  rather than guess.

## How to Verify

- On rolls with marked whites, the roll white balance lands near the gains those whites
  ask for.
- A sunset frame keeps its warmth; its roll's gains move little when it is removed.
- A roll with a fully exposed frame mixed in gives the same gains with and without it.
- A recipe or flag asking for a per-frame auto white balance on the new chain is refused
  with a message naming the roll command.

## Dependencies

- [Scene correction as a named stage](stage.md) — **done**; it owns white balance and
  exposure, and this is a new source for the first.

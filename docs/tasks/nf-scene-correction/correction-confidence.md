# How far to trust a roll's corrections

## Goal

Tell the user how far to trust each correction `measure-roll` derives (the roll white
balance, the midtone line), and what to try instead when it is in doubt.

Filed by the user on 2026-10-07 after the sea probe. The intent then: corrections stay on
by default; confident → apply; in doubt → apply, warn and suggest a comparison; sure it is
wrong → turn it off. **Re-scoped 2026-10-09 (user), after the research below: no automatic
turn-off, only warnings**, and the white balance gets a manual switch.

## Why it exists

The probe ("Sea frames probe", <https://claude.ai/artifact/FEa5gytdqhFfUnKvVhjzKa>;
scripts in `../temp/beach-probe/`) showed that a roll dominated by one scene colour fools
the corrections. On 09-14 Ektar the white balance from its 16 sea frames left a neutral-patch
cast of 465 (log2 × 1000) against 110 for the whole roll. The white balance is hit hardest:
one gain reaches every pixel, while the line fades to zero at the roll's white and spares
strongly coloured light.

## Decided

- **Frame count is the only signal that ships** (`pipeline::correction_confidence`): both
  corrections are in doubt under 16 frames, with one advisory note naming
  `--neutral-balance off`. Not a warning, so `--strict` ignores it: a whole roll of 10
  or 12 frames can never reach 16.
- **`--neutral-balance on|off` / `roll.neutral_balance`**: off keeps the gains in the
  recipe and takes the midtone line with them, since it is measured after them; the whites
  stay as measured. Render-time only; `measure-roll` keeps writing the gains (an unwritten
  gain would trip `convert`'s "no roll measurement" warning).
- **Not detected**: a roll of one dominant colour. No statistic inside a roll separated
  harmed subsets from random draws (evidence in the progress log).

## Open

- Detecting a one-colour roll. Needs evidence the archive lacks: real one-colour rolls
  beyond the sea marks, or the ColorChecker rolls as true neutrals. A per-stock prior
  scored best (AUC 0.87) but normal rolls of one stock sit up to ~450 apart.
- A fallback better than neutral gains, which lose to the measured white balance on most
  rolls; `neutrality-gate` may change that.
- Scoring exposure and the white's placement.
- A "measure from frames you pick" suggestion for the GUI.

## How to Verify

- `measure-roll` reports `confidence` per correction; a roll under 16 frames notes it once
  (`confidence.advice`), naming a flag `convert` accepts, and passes `--strict`.
- `--neutral-balance off` renders as a recipe without the gains and the line; a typed
  `--midtone-neutral on` beside it is refused, a recipe's `"on"` spared.
- `docs/using-nc.md` and design-spec §6 state both.

## Dependencies

- [A midtone neutral measured per roll](midtone-neutral.md) — one of the corrections
  graded, and the switch shape
- [A roll-level white balance](roll-white-balance.md) — the correction the probe found
  most exposed
- [Separate taste from quality](../nf-calibration/taste-vs-quality.md) — which
  adjustments are corrections, and the switch shape

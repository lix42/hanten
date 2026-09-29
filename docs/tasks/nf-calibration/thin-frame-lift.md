# Opt-in lift for a thin frame

## Goal

An **opt-in** way to render a frame far thinner than its roll as a full print. The roll's
exposure leaves such a frame dark, which is the default and stays so. With the option, a
per-frame exposure and a steeper slope are solved so the frame's white reaches diffuse
white. This trades more grain for a usable print, within bounds, and the recipe and report
show what was applied.

## Design

What is known:

- **Exposure alone cannot do it.** A thin frame's recorded range, from the base to its
  white, is short: on the 2026-09-28 roll the base sits about 3.5 scene stops under
  mid-grey, and the thinnest frames' whites about 1.1 stops under it. Enough exposure to
  bring that white to diffuse white also brings the base within 2 stops of mid-grey,
  where display black warns and crushes the shadows. So the lift needs **exposure and a
  steeper slope together**: the slope spreads the short range, and the exposure is
  solved from it.
- **Grain rises with the slope.** The user accepts that as the price, within a bound.
- **Evidence, 2026-09-29** (roll `20260928-film-*`: roll exposure +1.4 EV, slope 1.649;
  frames 2005 and 1983, whites −1.23 and −1.06). Hand-solved pairs put each white at
  diffuse white. Each exposure below is the frame's **total**, replacing the roll's +1.4
  (a manifest `params` exposure replaces the flag):
  - slope 2.0 (about ×1.21 the roll's): exposure +2.47 and +2.30. The base landed 2.2 and
    2.6 stops under mid-grey.
  - slope 2.4 (about ×1.46 the roll's): exposure +2.26 and +2.09. The base landed 3.2 and
    3.6 stops under mid-grey.

  Nothing clipped, and display black never warned. **The user's verdict:** grain at 2.4 is
  "definitely fine", and 2.0 is acceptable but not necessary. 2.0's white is fine; 2.4's is
  a little dark but acceptable. The base prediction from the report's display-black numbers
  was within 0.15 stop on all four renders, so the solve can be computed rather than
  searched.
- **It is opt-in and visible.** It uses the machinery the clamps use: `measure-roll`
  already measures each frame's white and knows the base, and would write the solved values
  as `roll.frames` entries, where they can be read and deleted. Without the option, nothing
  changes.

Open:

- **The bounds.** At least ×1.46 on the slope is acceptable by that review. The exposure
  bound, and whether the slope bound should be written against the roll's slope (a
  multiplier) or as an absolute slope.
- **The white target.** At the steeper slope, whether to aim a little above diffuse white:
  2.4's white read slightly dark. `roll-white-rule` also saw a target 0.15 stop above
  diffuse white look "a little better" in some SDR cases.
- **Which frames qualify.** A threshold below the roll (in stops under the roll's
  exposure-corrected white), and what a frame that needs more than the bounds allow gets.
  It could be lifted to the bound and reported, or left alone.
- **How to pick the slope and exposure within the bounds.** The minimum slope that keeps
  the base clear of display black's warning, or a fixed ratio.
- **Colour.** Lifted, 1983 grew bluer (written blue 0.52 against red and green about
  0.30). The user judged this to be the scene, not a toe cast. So no cast appeared on
  these frames, but a thinner frame on another roll could still show one.
- **Total or delta.** Whether a lifted frame's exposure in `roll.frames` replaces the roll's
  or adds to it (on 2005: +2.26 total, or +0.86 over the roll). `roll-exposure` makes the
  stated exposure add to the roll's, so the entry's merge must be pinned either way.
- **Keys and flags.** How the option is spelled on `measure-roll`, and how a lifted frame's
  entry is marked in `roll.frames`, so it reads as a choice, not a measurement.

## How to Verify

- On the 2026-09-28 roll, the option lifts 1983, 1984, 2000 and 2005 (and 1992 if it
  qualifies) to renders comparable with the hand-solved slope-2.4 pairs, and leaves every
  other frame's entry untouched.
- Without the option, `measure-roll --out` writes byte-identically to the build before this
  task (after `roll-exposure`).
- A frame that the bounds stop from being fully lifted is reported by name.
- A review round on at least one more roll with thin frames confirms the bounds.

## Dependencies

- [A measured roll exposure](roll-exposure.md) — the lift is measured on top of the roll's
  exposure, and "thinner than the roll" is relative to it
- [`measure-roll` places the roll's white](roll-white-rule.md) — the per-frame whites and
  the clamp entries it extends

# Raise the roll's brightness target to 0

## Goal

Move `measure-roll`'s level target (`roll_white::LEVEL_TARGET_STOPS`) from −0.6 to 0
scene stops, so the roll exposure puts the median frame's log-average luma at mid-grey,
where a light meter would. A global fix: every roll, not only poorly developed ones.

## Design

Known:

- **Evidence (2026-10-02/03, the colour-cast spike, `../temp/roll-neutral-spike/`):**
  on each roll's 8 kept frames, target 0 puts Hanten's midtones near SilverFast CCR's on 09-18,
  09-20 and 09-29 (median L* within +4.5 / −0.7 / +2.7; −0.6 sits 6–12 L* darker). Over **every**
  frame of 09-18 and 09-29, CCR's midtones are still ~10 L* brighter (52.6 vs 62.1, 49.5 vs 58.8):
  CCR levels each frame, lifting dim frames most — a per-frame question, not the target's. Review
  (`../temp/r1-target-desat/`): target 0 beat today on almost every frame of 09-18 Gold
  (good roll; 1813 the exception) and on 09-29 Ektar (poor development).
- The bright end stays lower than CCR's (L* 78–81 vs 88–93): Hanten maps the roll, CCR
  each frame, so the narrower contrast is by design.
- The frame-lift cap (+0.3 EV) needs no change: at target 0 most frames no longer
  qualify for a small lift.
- −0.6 was chosen by `roll-exposure`'s review: it beat −1.0 and −0.8; against −0.3 it
  split 8–10 **by frame** — low-key frames wanted brighter, bright frames −0.6. The same
  split shows here: on 09-29's brightest frames (2028, 2037) today ≈ target 0, today
  keeping more cloud detail.
- Decided (user, 2026-10-07): the target stays a constant (`--exposure` already moves a
  roll); bright frames and re-placing the white (measured at exposure 0, so target 0 puts
  most frames' whites above diffuse white) go to `display-white` and
  `envelope-hybrid-placement`.

Open:

- How the thin lift's thresholds read at the new exposure (`thin-lift-confirmation`).

## How to Verify

- `measure-roll` on the archived rolls: roll exposures move by +0.6 EV unless bounded;
  frame lifts re-derive.
- The `docs/using-nc.md` text that states the target. No drift-gate row or
  `pipeline_version` bump: the target is a measuring command's constant, which changes
  what a new measurement writes but not how a written recipe replays (`version.rs`).

## Dependencies

- [Roll exposure](roll-exposure.md) — the target this moves, and the review that chose it
- [Separate taste from quality](taste-vs-quality.md) — the recipe format the lifts are
  measured into

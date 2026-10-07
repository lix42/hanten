# How far to trust a roll's corrections

## Goal

Tell the user how far to trust each correction `measure-roll` derives (the roll white
balance, the midtone line, and any other correction in `taste-vs-quality`'s sense), and
what to try instead when it is in doubt. Some rolls cannot be corrected automatically; the
tool should say so rather than guess.

Filed by the user on 2026-10-07 after the sea probe. The intent:

- Every correction stays **on by default**. The user can always turn one off, and the
  planned GUI previews the choice.
- **Confident**: apply it, so one click gives the roll-optimized result.
- **In doubt**: apply it, warn, and suggest what to compare in the preview.
- **Sure it is wrong**: turn it off automatically, and say so.
- The CLI carries this as a score in the JSON report, plus a suggested alternative.

## Why it exists

The probe ("Sea frames probe", <https://claude.ai/artifact/FEa5gytdqhFfUnKvVhjzKa>;
scripts in `../temp/beach-probe/`) showed that a roll dominated by one scene colour fools
the corrections, and that no guard can be built from colour statistics alone. It used the
frames the user marked as sea on 09-09, 09-14 and 09-18:

- **Sea frames pull every correction warm.** The method reads the water as the film's
  grey.
- **The roll white balance is hit hardest.** On 09-14 Ektar, the white balance measured
  on its 16 sea frames left a grey about 24% less blue than the whole-roll measurement,
  twice the largest shift from a random 16-frame draw. The median cast on the neutral
  patches rose from 110 to 465 (log2 × 1000).
- **The midtone line is pulled less, and does not protect near white**, where it fades
  out.
- **Small rolls are noisy even without a scene bias.** A random half of 09-14 moves the
  line by up to about 200.

## Known

- **`--rendering direct` is not yet a safe fallback.** It applies white balance 1 and no
  line.
  - It leaves a patch cast of 340–500 on every probed roll, about three times today's
    white balance on the good rolls (105–110).
  - It beats a fooled white balance only in the worst case: 367 against 465 on 09-14's
    sea frames.
  - Its cast differs between rolls of one stock (Ektar 09-09 against 09-14), so a fixed
    per-stock gain would not fix it.

  Calibration against the ColorChecker scans (`neutrality-gate`,
  `scanner-density-calibration`) improves direct, but probably will not remove that
  per-roll spread.
- **A labelled set can be made from the rolls already scanned.** Subsets of mostly one
  scene colour are known-bad, random draws are known-fine, small draws test the noise,
  and the marked neutral patches give the ground truth. The probe's script builds all
  three kinds.

## Open

- **The signals.** Candidates from the probe:
  - the white balance and the line disagreeing;
  - the frames' votes scattering (the spike's detection ratio);
  - the frame count;
  - one hue dominating the votes.

  None is shown yet to separate bad from good.
- **The thresholds** between confident, in doubt and sure it is wrong, calibrated on the
  labelled set rather than guessed.
- **What "off" falls back to, per correction.** Direct is worse than a fooled white
  balance on most rolls. Options include a white balance from a subset of frames the
  user picks, a per-stock default, or the line without the white balance.
- **Which corrections get a score.** The white balance and the line first. Then
  exposure and the white's placement, and whether a preference (the lifts) needs one at
  all.
- **The report shape.** One entry per correction, the suggestion as a flag or recipe
  value the user's command accepts, and what the GUI needs to render the alternative.
- **How a stated value interacts.** A user-stated `--white-balance` presumably gets no
  score.

## How to Verify

- On the labelled set:
  - the sea-dominated subsets score in doubt or sure it is wrong;
  - whole mixed rolls and random draws of ten frames or more score confident.
- Every suggestion the report makes is accepted by the command it names.
- `docs/using-nc.md` documents the score, its tiers and the automatic turn-off.

## Dependencies

- [A midtone neutral measured per roll](midtone-neutral.md) — one of the corrections
  scored, and the source of the vote-based signals
- [A roll-level white balance](roll-white-balance.md) — the correction the probe found
  most exposed
- [Separate taste from quality](../nf-calibration/taste-vs-quality.md) — which
  adjustments are corrections, and the switch shape the automatic turn-off uses

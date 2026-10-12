# Hanten against SilverFast CCR on the Lomography 100 roll

## Goal

Find why Hanten leaves a blue (slightly green) cast on most frames of
`2026-10-10-LomoColor100` that SilverFast's Color Cast Removal does not, and decide what
would remove it without losing what Hanten already does better there.

## Design

Known (user review, 2026-10-11; the notes are in this epic's progress log):

- Over 36 frames the user ranked CCR above Hanten on 18, Hanten above CCR on 11, and the
  two even on 7. The overall remark: Hanten's contrast and exposure are the best of the
  three, but it has much more cast than CCR, mostly blue, maybe a little green; where the
  cast is small or does not matter, Hanten looks best. Per-frame reasons were given only
  for 2056–2061.
- Hanten's blacks are lighter than CCR's, which keeps shadow detail: wanted, not a fault.
- On 2056 Hanten keeps a blue sky CCR loses, so a fix that neutralises harder than the
  roll's light is a regression.
- 18 white patches on 12 frames are in the archive's manifest: 7 white cloth or wall, 11
  cloud. Cloud may be slightly blue itself (the same doubt as
  [`midtone-neutral-fit`](midtone-neutral-fit.md)), so read the cloth and wall first.
- RGB scans without IR (only 2060 has an IR plane), so the holder is measured on 2060 at most.
  Roll exposure +2.04 EV; frame slopes 1.95–3.5 under `--placement frame`. Colour uses the
  roll's slope, not the frame's, so the steep slopes do not scale the cast.

Open:

- Where the cast lives: the white balance, the midtone line, or the fixed decode's
  per-channel density gain (a stock it was not calibrated on), and whether it varies with
  tone. Highlight desaturation pulls near-white patches toward neutral, so measure with it
  off (`--highlight-desaturation 0`) or before it.
- What CCR does that Hanten does not: per frame or per roll, and at which tones. Its
  exports keep the scan's dimensions, so patches compare region for region.
- Whether this is one roll's development or a stock trait; there is no other roll of it.
- If the midtone line is the cause, how it interacts with `midtone-neutral-fit`, which
  tunes the same line.

## How to Verify

- Each patch's cast measured on Hanten, CCR and NLP, with the cause located to a stage.
- A candidate change re-rendered on the whole roll and reviewed by the user beside CCR;
  2056's sky and the shadow detail kept.

## Dependencies

- [A roll-level white balance](roll-white-balance.md) — the gains under suspicion
- [A midtone neutral measured per roll](midtone-neutral.md) — the line under suspicion

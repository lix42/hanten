# Hanten — nf-calibration Progress Log

Execution log for the `nf-calibration` epic: what was done and how, key decisions, what
works, what doesn't. TASKS.md holds the authoritative status; this file is the
narrative beside it.

One `##` section per task in this epic, named by the bare task name. Read this
whole file before starting a task in this epic, and read other epics' `Epic
summary` sections when you depend on them. Append entries — don't rewrite earlier
ones.

## Epic summary

> **Since `nf-core/default-flip` (2026-09-27, `pipeline_version` 8)** the chain this summary calls the new flow is the only one: read "under `--new-flow`" as the default, and "the current chain" as the removed one (the reference build).

The numbers rather than the machinery: an early `scale` ladder (runs against today's binary, no colorchecker), the `scale`/`gamma` review loop, the offset question, the neutrality release gate, and what a user would run.

The epic was created on 2026-09-19 with the new-flow migration plan
(`docs/nf-migration.md`). **`scale-ladder` is done** (2026-09-20).

**`anchor-comparison` is done (2026-09-25): the roll's white is content-referenced,
placed through contrast, never through the decode's anchor.** `d` stays 0.62 and mid-grey
stays pinned. Rule, provisional values from nine rolls: each frame's white is its red p97
(**`measure-roll` reads each pixel's brightest channel instead**, `roll-white-rule`
2026-09-26 — red under-reads blue- and green-lit highlights);
the roll's white is its brightest frame's white at or under **+2.0** scene stops above
mid-grey, raised to at least **+1.5**; a frame above +2.0 is clamped to it; the solved
`look.contrast` spans whole contrast 2.23–2.97 (2.23 on a clamped frame). A warning fires near the **leader**
(film saturation), never merely because the cap bound. It was chosen by review **with a
black point in the chain**, which the new chain lacked: every placement looked pale
without one. Black landed on 2026-09-26 as `--display-black` (`fit_range.display_black`,
default 6 stops below mid-grey; `nf-display-stages/parametric-operator`), keyed on where
the film base renders, so judge any white placement with it at its default. Implementation is
`roll-white-rule`: `hanten measure-roll` reports the white and the `look.contrast` placing
it, measured over the shared effective area in film RGB before the working-space matrix;
a white target 0.15–0.31 stop above diffuse white was reviewed and not adopted. The warning's margin is
`saturation-margin`. Every round was judged on SDR; the values stay provisional until
`white-rule-hdr` reviews the HDR rendition. Fit range stays reinhard
(`nf-display-stages/parametric-shoulder`), so the rule's cap and floor stand as chosen. Ranked rule > fixed anchor > content white with solved contrast
(noise up to 5.4× the scan's floor) > level move (mid-grey up to L\* 82). Levels here
are **scene stops**: red density through the fixed linearization, 1 stop ≈ 0.167
density.

**`roll-section` is done (2026-09-27): the roll's measurements are their own recipe
section**, `roll.white_balance` and `roll.white_stops` (the white, not a contrast;
`look.contrast` unset means the roll's, else the default). `measure-roll` writes it; the
gains multiply a stated white balance, and a stated contrast wins. A rendering decides
whether it applies (`--rendering default` does, `direct` does not). The fallback when a
roll has no measurement is `no-roll-defaults`'.

**`scale-gamma-loop` is done (2026-09-27): nothing moved.** One review round on blue
under `--rendering direct --range sdr` could not tell 0.68 / 0.73 / 0.78 apart;
`[1, 0.84, 0.73]` and the linearization 1.8 (the datasheets' `1/0.55`, not reviewable by
eye) stand. Blue splits by roll as green does (July ~0.72, four September rolls ~0.78;
09-09 confounded by skylight), so no global
value closes it. The calibrated re-run is `neutrality-gate`'s, against the calibration
frames; the look's default contrast is `no-roll-defaults`'.

## anchor-comparison

**Status:** done
**Updated:** 2026-09-25

- 2026-09-21: filed to carry the rendered half of `nf-reconstruction/anchor-spike`,
  which costed four white placements from the scans but could not rank them: under the
  fixed anchor the three rolls measured land 0.55–1.28 stops short of white, so a
  per-channel highlight operator has nothing to act on and a render would compare four
  configurations of which one is inert. Waits on `nf-look/path-to-white`. A verdict of
  "keep the fixed anchor and move `d` instead" is a complete outcome.
- 2026-09-23: the "0.55–1.28 stops short" above is **0.55–1.73** once 09-11's calibration
  frame is excluded — see the correction in [`white-placement.md`](../spike/white-placement.md).
  09-11 alone would ask candidate C for gamma 6.61, which makes it the natural test roll
  for D's ceiling.
- 2026-09-24: from `nf-look/contrast` (user). **`W` is held at red p97 for the
  renders**; the final percentile, and whether it is a code constant or a recipe value,
  stays this task's to settle or hand on explicitly. **Where C/D's per-roll contrast
  lands is settled**: it is `look.contrast` itself, one knob, with the decode's anchor
  unchanged — only how it reaches the recipe (`measure-roll` or by hand) stays open.
- 2026-09-25: **started; two review rounds, and the shortlist changed shape.** Sets and
  scripts in `../temp/anchor-cap/` (`review.json` = cap round, `review-probe.json` = black
  probe); rendered by hand on the new chain, since the generator cannot pass `--new-flow`.
  Only `look.contrast` varies within a roll (total gamma `= 0.745 / (W − d)`, mid-grey
  pinned at `d = 0.62`); white balance is each roll's `measure-roll` gains, held.

  **Units.** The user reasons in stops, so every level here is in **scene stops above
  mid-grey**: red density through the decode's fixed linearization (1 stop = 0.167
  density). Not rendered stops — those depend on the contrast being solved.

  **Two measurement traps, both hit.** (1) `decode-side.json`'s per-frame `q` is the
  *channel mean*; the spike's `W` is *red*. Mixing them made frames appear to exceed their
  leader. (2) `estimate`'s film base is a **transmission**: density is
  `−log10(T / base)`, not `−log10 T − base`. Re-measured on red, all nine negative rolls
  (`remeasure-fixed.json`): no frame's white exceeds its leader, and the spike's `W`
  reproduces. The spike's roll `W` was the **p90 over frames** of per-frame red p97 — a
  cross-frame percentile the user rejected, since it makes the roll's white depend on
  roll size.

  **Replaced D with a clamp on the roll's white** (user). The roll's white is its
  brightest frame's white (per-frame red p97, which keeps the headroom), bounded by a cap
  above and a floor below. Frames whose white sits above the cap are handled one of two
  ways: **V1** clamps the roll's white to the cap; **V2** skips them and takes the
  brightest frame left (the cap if none is). A cap binding must **warn** (user).

  **Cap round** (6 rolls, 35 frames): uncapped, V1/V2 at +2.5/+3.0, V1 at leader −1.0.
  The user preferred V2 +2.5 on most frames, V1 +2.5 on some; uncapped and +3.0 were
  flat. Uncapped white lands at +2.2 to +4.7, which is gamma 0.94–2.05, pale on every roll.
  1121 (07-23) judged overexposed; 1868 and 1902 (09-20) bright scenes; 1151 ambiguous.

  **The pale look was mostly the missing black point.** The new chain places no black
  (`--black-point` is refused under `--new-flow`; `flare-removal` and the operator's toe
  are unbuilt), so the only thing darkening shadows was contrast: the darkest 1% of
  pixels sat at L\* 12–26. Every roll's darkest frame bottoms out at the **film base**
  (red p0.5 at −3.6 to −3.8 stops, against mid-grey's 3.7 above base), which the straight
  line renders at **L\* 12–15** under V1. The **black probe** moved that level to L\* ≈ 2
  (one value per roll and arm, applied to the JPEG, not a pipeline stage). Verdict (user):
  **with black added, both white rules improved** (V1+black > V1, V2+black > V2), and on
  09-14 and 09-18 the earlier wish for more contrast was met by the black alone. The
  **two-point pin** (base → L\* 2, `W` → white, mid-grey floating; gamma 2.48–2.67 on every
  roll) put mid-grey at L\* ≈ 36 and was **too dark** — **mid-grey stays pinned**.
  **Leader − 0.5 with skip** found the overexposed frames (skipped 1121, 1151, 1816) but left
  09-20 flat — it neither places the white nor fixes flatness.

  **Where V1 beat V2 (both with black): almost exactly the frames V2 skipped** — 1121,
  1151, 1632 (skin), 1868 (a toss-up); 1902, barely over the cap, went the other way. The
  one other V1 win, 1137, is on 07-24 (3 frames), where skipping 1151 makes V2 jump to gamma
  3.39: the jump to the next frame on a short roll. Hence **V3** (user-agreed): the roll's
  contrast from V2, and each skipped frame clamped to the cap as in V1, with a warning.
  Only warned frames render differently from their roll.

  **What the leader is still good for.** Its distance from a frame's white says whether the
  frame nears film saturation (1121 0.13 stop under, 1151 0.40) or is a bright scene
  within latitude (1868 1.81 under). That separates the warning's two meanings, and
  may justify a different highlight treatment for the saturated kind.

  **Consequences.** (a) A black point is needed, and it is not this task's to build:
  `nf-display-stages/parametric-operator` / `nf-scene-correction/flare-removal`. Its
  natural reference is where the film base renders, which is already measured. Until it
  exists, **every white rule must be judged with the probe's black added**, or the white
  is tuned to compensate for the missing black. (b) The cap value is still an estimate (+2.5
  was the only value tested with black); the floor is untested. Next round: V2 vs V3 at
  +2.0/+2.5/+3.0 with black, and single dark frames under a floor ladder.
- 2026-09-25: **round 3 (all with the probe's black) — cap, V3 and floor chosen** (user).
  Sets `review-r3-cap.json`, `review-r3-floor.json`; `scripts/round3.py`.

  **Cap +2.0, V3.** +3.0 lost wherever it differed; +2.0 and +2.5 were close, with a lean to
  +2.0 (09-18, 09-14). V3 was safer on the frames near film saturation (1121: "v3 is much
  safer at highlight"; 1151), V2 better on skipped frames that are ordinary bright scenes
  (1632, 1902) — the split the leader distance predicts. **The warning keys on the leader,
  not the cap** (user): +2.0 binds on every roll measured, so a cap warning would fire on
  every roll. Warn when a frame's white is within a margin of its leader ("near film
  saturation"); the cap itself is silent.

  **Floor +1.5.** Single dark frames treated as a roll of one (09-13 1698/1696, 09-11
  1658/1662, 09-09 1612, 09-14 1719, 09-18 1797, 09-20 1904): floor +1.0 (gamma 4.45) was
  worst on every frame except control 1810; +1.5 (2.97) beat +2.0 (2.23) and the frame's
  render within its own roll.

  **The rule this gives, on all nine rolls** — roll white = brightest frame white at or
  under +2.0, raised to at least +1.5; frames above +2.0 clamped to it:

  | roll | `W` | gamma | leader − frame < 0.5 |
  |---|---|---|---|
  | 07-15 Ektar100 | +1.95 | 2.29 | — |
  | 07-23 Portra160 | +1.91 | 2.33 | 1121 (0.14) |
  | 07-24 Gold200 | +1.50 floor | 2.97 | 1151 (0.39) |
  | 09-09 Ektar100 | +1.87 | 2.39 | — |
  | 09-11 Portra400 | +1.52 | 2.93 | — |
  | 09-13 Portra400 | +1.65 | 2.69 | — |
  | 09-14 Ektar100 | +1.98 | 2.25 | — |
  | 09-18 Gold200 | +1.50 floor | 2.97 | 1816 (0.27) |
  | 09-20 Portra400 | +1.97 | 2.26 | — |

  Two things this table raises, open: (1) **cap − floor is 0.5 stop**, so the rule can move a
  roll's white by at most half a stop and its contrast only across 2.25–2.97. A fixed white
  near +1.75 (gamma 2.54) is within ±0.25 stop of it on every roll, so the fixed option is the
  null to beat before the per-roll measurement earns a place. (2) A 0.5-stop leader margin
  also flags **1816**, which the user never judged overexposed; Gold200's leader sits only
  ~1.5 stops above its content on both Gold rolls, so the margin may be stock-dependent.
- 2026-09-25: **round 4, the null test — the per-roll rule beats a fixed white** (user).
  `review-r4.json`, `scripts/round4.py`: 8 frames on five rolls chosen where the two differ
  most (floor rolls 09-18/07-24/09-11 steeper under the rule, cap rolls 09-14/09-20 flatter),
  the rule against one fixed white at +1.75 (gamma 2.54), both with the probe's black. "They
  look different… maybe 1 or 2 frames the fixed one is better, but the rule is more
  consistent." So the per-roll measurement earns its place, narrow band and all. **1816**:
  the rule's highlight is better — the V3 clamp to the cap (gamma 2.23 against 2.54) helps
  it, whether or not it is near saturation, so the leader margin (0.5 stop) stays
  provisional rather than confirmed by this frame.
- 2026-09-25: **the verification numbers, and the ranking.** `scripts/verify.py`, 160
  16-bit renders measured in memory, on every frame with marked patches on 09-18/09-14/
  09-20 plus three 09-11 frames. Noise is the three-way report's measure (q25 of
  `|I(x+1) − I(x)|` over the median, red, 1200 px central crop) as a multiple of the scan's
  own floor. C\* is the mean over the band fit's marked patches. Mid-grey is where a datasheet
  mid lands, scene-referred (analytic). A = today's default; B = the spike's `W` to white by
  exposure at gamma 2.0; C = the spike's candidate C; rule = the chosen rule; rule-bk = with
  the probe's black.

  | roll | option | gamma | noise × floor (max) | white C\* | colour C\* | mid L\* |
  |---|---|---|---|---|---|---|
  | 09-18 Gold200 | A | 2.00 | 0.88 (1.03) | 3.8 | 8.4 | 49.5 |
  | | B | 2.00 | 0.77 (0.91) | 4.3 | 10.3 | **72.1** |
  | | C | 4.15 | **1.98 (2.45)** | 7.8 | 17.3 | 49.5 |
  | | rule | 2.23–2.97 | 1.32 (1.63) | 5.5 | 11.7 | 49.5 |
  | | rule-bk | 2.23–2.97 | 1.36 (1.67) | 5.5 | 11.8 | 49.5 |
  | 09-14 Ektar100 | A | 2.00 | 0.88 (1.34) | 4.0 | 11.0 | 49.5 |
  | | B | 2.00 | 0.83 (1.27) | 4.2 | 11.6 | 58.4 |
  | | C | 2.57 | 1.15 (1.76) | 5.3 | 14.9 | 49.5 |
  | | rule | 2.23–2.25 | 1.00 (1.52) | 4.5 | 12.7 | 49.5 |
  | | rule-bk | 2.23–2.25 | 1.14 (1.88) | 4.6 | 12.9 | 49.5 |
  | 09-20 Portra400 | A | 2.00 | 0.87 (1.01) | 13.7 | 31.1 | 49.5 |
  | | B | 2.00 | 0.84 (0.98) | 14.6 | 32.5 | 54.9 |
  | | C | 2.32 | 1.00 (1.18) | 16.1 | 37.1 | 49.5 |
  | | rule | 2.26 | 0.97 (1.15) | 15.6 | 36.0 | 49.5 |
  | | rule-bk | 2.26 | 1.14 (1.32) | 15.9 | 36.5 | 49.5 |
  | 09-11 Portra400 | A | 2.00 | 1.07 (1.28) | — | — | 49.5 |
  | | B | 2.00 | 0.90 (1.08) | — | — | **81.6** |
  | | C | 6.61 | **5.43 (6.31)** | — | — | 49.5 |
  | | rule | 2.93 | 1.75 (2.14) | — | — | 49.5 |
  | | rule-bk | 2.93 | 1.81 (2.22) | — | — | 49.5 |

  Readings. **Noise tracks the slope**, as the three-way report found: C costs 2× the floor
  on Gold200 and 5.4× on the underexposed 09-11. The rule's floor holds 09-11 to 1.75×. The
  black probe adds ~0.05–0.15×, most of it the measure's own normalisation (the black
  lowers the median it divides by). **B's mid-grey floats to L\* 55–82**: the washed-out
  midtone the spike predicted, worst on the underexposed roll. **Marked whites' C\* rises
  with contrast, in proportion to colour C\***. A → C on 09-18 doubles both (3.8 → 7.8,
  8.4 → 17.3, gamma ×2.07). Contrast is a per-channel power, so it multiplies residual cast
  with saturation, and highlight desaturation at its provisional band does not take it back.
  The rule's whites carry 1.1–1.45× A's chroma. 09-20's two white patches sit at C\* ~14–16
  under every option, so they are not neutral surfaces.

  **Ranking: rule (with a black point) > A > C > B.** The rule won every review round it
  was in. A is roll-consistent but pale and leaves the highlight operator nearly inert. C
  pins both ends but pays in noise, unboundedly on an underexposed roll. B loses mid-grey.
  "A, and move `d`" was not needed as the verdict: the rule's band (gamma 2.25–2.97) is what
  moving away from A bought, and the fixed +1.75 white lost to it in review.

  **The task's open questions, answered or handed on.** *Which percentile defines `W`*: each
  frame's red p97 over a 12% inset, with the roll taking its brightest frame under the cap —
  no cross-frame percentile. Moving onto `measure-roll`'s ACEScg pooling and re-checking
  the cap and floor there goes to the follow-up that implements it. *Code constant or recipe
  value, and how the contrast reaches the recipe*: handed to that follow-up, which has
  `measure-roll` compute it and the recipe carry the value, as white balance does. *Whether
  an underexposed roll is lifted*: only as far as the floor (+1.5, gamma ≤ 2.97); below it the
  roll renders dark, which is the user's call ("if a whole roll is underexposed, we should
  render them as underexposed").
- 2026-09-25: **done.** Follow-ups filed (user-approved): `nf-calibration/roll-white-rule`
  (implement the rule in `measure-roll`; depends on this task and on
  `nf-display-stages/parametric-operator`, which now also places black) and
  `nf-calibration/saturation-margin` (the leader margin, and frames near saturation).
  Cross-references were appended to `path-to-white` (band re-fit), `scene-range-mapping`
  (the floor as a noise budget), `parametric-operator` and `flare-removal`. The claims that this
  task would decide the anchor were updated where they are current guidance: guide,
  spec, open tasks, the `nf-look`, `nf-reconstruction`, `nf-retire` and `nf-calibration`
  epic summaries, `types.rs`, `look.rs`, `fixed.rs`. The band re-fit is owned by
  `nf-look/desaturation-band-refit`; a frame clamped to the cap is disclosed in the report,
  not warned about (both from a code review of this close-out). What a dependent
  needs: **`d` and the anchor rule do not move**; the per-roll value is `look.contrast`;
  judge anything that places white with a black point in the chain.

## roll-white-rule

**Status:** done
**Updated:** 2026-09-27

- 2026-09-25: filed from `anchor-comparison`. Goal: `measure-roll` places the roll's white
  by the rule that task chose by review.
- 2026-09-26: **implemented** (`pipeline::roll_white`, `run_measure_roll`). Decisions
  (user):
  - **A frame's white is measured before the leader guard.** The guard drops pixels
    within 0.1 density (≈0.6 stop) of the leader — the highlights of a frame near
    saturation — so a guarded white lands lower, can escape the clamp and set the roll's
    contrast, and can **never** come within the 0.5-stop margin: the warning could not
    fire. The cap already keeps a blown frame from raising the roll's white; the guard now
    serves the white balance only.
  - **Domain: film red, before the working-space 3×3** — the reviewed quantity (red's
    density scale is 1, so it is red density through the linearization). ACEScg red
    before white balance was tried first: it mixes in green, blue and the roll's
    uncorrected cast, and moved frames −0.48 to +0.58 stop off the review **by stock**
    (Gold200/Portra160 ~−0.2, Ektar ~+0.25) at the review's own inset. `decode_for_roll_white`
    reads the film RGB just before mapping it; no extra full-frame buffer.
  - **Area: the shared effective area** (holder cut + `measure.inset`), not the review's
    12% inset. The scans are hand-cropped (user), so the 5–12% band is picture.
  - **Constants in code** (`WHITE_PERCENTILE`, `WHITE_CAP_STOPS`, `WHITE_FLOOR_STOPS`,
    `SATURATION_MARGIN_STOPS`), stated in the report: they parametrize a measurement, and
    the render's knob, `look.contrast`, is what the recipe carries. `calibration` gains
    no white.
  - **Reuse**: the flag gains `--contrast`, the fragment `look.contrast`, and a
    `roll --frames` manifest carries each clamped frame's contrast. **No leader, no
    saturation check**; the no-leader warning says so.
  - **The contrast needs no linearization**: at the decode's output a neutral `W` scene
    stops up sits at `0.18·2^W`, so `look.contrast = log2(1/0.18) / W` (1.237 at the cap,
    1.649 at the floor); the whole contrast is that × 1.8.

  **Verification on the nine rolls** (`measure-roll --leader`, the review's bases;
  area per the user's rule for these scans: holder + 5% where a holder is found, else a
  1% inset — holders were found only on 07-15, 07-23, 07-24 and one 09-14 frame):

  | roll | review `W` | `measure-roll` `W` | whole contrast | clamped | near leader (< 0.5) |
  |---|---|---|---|---|---|
  | 07-15 Ektar100 | +1.95 | **+1.50 floor** | 2.97 | 971, 991 | — |
  | 07-23 Portra160 | +1.91 | +1.93 | 2.31 | 1121 | 1121 (0.22) |
  | 07-24 Gold200 | +1.50 floor | +1.50 floor | 2.97 | 1151 | 1151 (0.38) |
  | 09-09 Ektar100 | +1.87 | +1.85 | 2.41 | 1632, 1635 | 1635 (0.27) |
  | 09-11 Portra400 | +1.52 | +1.61 | 2.77 | — | — |
  | 09-13 Portra400 | +1.65 | +1.61 | 2.77 | — | — |
  | 09-14 Ektar100 | +1.98 | +1.87 | 2.38 | 1737–1739 | — |
  | 09-18 Gold200 | +1.50 floor | +1.50 floor | 2.97 | 1815, 1816 | 1815 (0.28), 1816 (0.30) |
  | 09-20 Portra400 | +1.97 | +1.98 | 2.25 | 1868, 1894, 1902 | — |

  Per-frame medians sit within ±0.12 stop of the review; the spread is the area. At the
  review's 12% inset the same code reproduces it (roll `W` within 0.07 on eight rolls; 07-15
  −0.23). What moved is **content the 12% inset excluded**: 07-15's 971 and 991 read
  0.2–0.35 stop brighter and cross the cap, dropping the 3-frame roll to the floor; 1635's
  white is +0.53 at 12% and +3.48 at the edge (bright picture content 5–9% in), and it
  now warns. 1815 warns too; 1816 (warned) was not judged overexposed in review. Awaits the
  user's call on whether these shifts stand.
- 2026-09-26: **the white takes each pixel's brightest channel, not red** (user). Red
  under-reads a blue- or green-lit highlight, and the solved contrast then pushes it past
  white — the case per-frame work would expose. Only the measured `W` changes: the contrast
  is `log2(1/0.18) / W` whatever the channel, so the cap and floor keep their units. Red vs
  brightest channel, same area rule as above (leader distance in the same measure):

  | roll | `W` red → max | whole contrast | clamped red → max | frame shift median / max |
  |---|---|---|---|---|
  | 07-15 Ektar100 | +1.50 → +1.56 | 2.97 → 2.85 | 2 → 2 | +0.01 / +0.08 |
  | 07-23 Portra160 | +1.93 → +1.93 | 2.31 | 1 → 1 | +0.03 / +0.17 |
  | 07-24 Gold200 | +1.50 → +1.50 | 2.97 | 1 → 1 | +0.00 / +0.01 |
  | 09-09 Ektar100 | +1.85 → +2.00 | 2.41 → 2.23 | 2 → 5 | +0.61 / +2.15 |
  | 09-11 Portra400 | +1.61 → +1.81 | 2.77 → 2.46 | 0 → 0 | +0.20 / +0.60 |
  | 09-13 Portra400 | +1.61 → +1.62 | 2.77 → 2.75 | 0 → 0 | +0.29 / +0.99 |
  | 09-14 Ektar100 | +1.87 → +1.97 | 2.38 → 2.26 | 3 → 5 | +0.75 / +1.57 |
  | 09-18 Gold200 | +1.50 → +1.50 | 2.97 | 2 → 2 | +0.04 / +0.39 |
  | 09-20 Portra400 | +1.98 → +1.98 | 2.25 | 3 → 3 | +0.07 / +0.40 |

  The saturation warnings are the same set. **Ektar moves most**, expected (user): its
  roll cast leaves neutrals ~0.25 stop lower in red than green (red gain ≈ 1.18), and the
  rest is scene content — 0.5–2 stops on most 09-14 frames, the blue/green highlights this
  change is for. The roll's white moves ≤ 0.2 stop, since the cap and floor bound it.
  **Sunset colour is unaffected**: an orange highlight's brightest channel is red anyway,
  white balance is a separate roll-level measurement, and highlight desaturation gives no
  pull past its band's saturation limit however bright the pixel. Luminance was considered
  and rejected: it weights blue at 5% (`ACESCG_LUMA`), so a blue sky still under-reads, and
  it exists only after the 3×3. Near-neutral-only pixels were rejected: they fail exactly
  on a blue- or green-dominated highlight. Next: a review round on 09-11, 09-09 and 09-14,
  red vs brightest channel.
- 2026-09-27: **two review rounds; the implementation stands** (user). Sets in
  `../temp/white-channel/` (`review.json`, `review-r2.json`; `scripts/` holds the render
  scripts, the rendering binary and its source patch). Both hold white balance, film base
  and display black fixed and vary only `look.contrast`.
  - **Round 1, red vs brightest channel** (16 frames on 09-09, 09-11, 09-14): "the diff is
    very small", a little worse under the brightest channel on **09-11**, whose roll white
    rose +1.61 → +1.81 (contrast 2.77 → 2.46).
  - **Round 2, a global white-target offset** (18 frames, two per roll on all nine): the
    roll's white rendered at diffuse white, 0.15 stop above it, or 0.31 stop above it
    (every contrast ×1.06 / ×1.125; +0.31 restores 09-11's 2.77). No constant restores 09-11
    alone: its white sits between the floor and the cap, so only a global lever reaches it.
    Verdict: **+0.31 worse**; 0 and +0.15 hard to tell, +0.15 "a little better" in some
    cases. **Kept at 0** — not worth changing the reviewed placement now. If a later round
    wants more contrast, +0.15 is the value to retry.
  - The shifts from measuring over the shared effective area (07-15, 09-14 and the new
    1635/1815 warnings, above) stand with the implementation.
- 2026-09-27: **done.** Landed: `pipeline::roll_white` (`frame_white`, `leader_peak`,
  `place_roll_white`, `contrast_for`, the four constants) and `run_measure_roll`, which
  reads each frame's film RGB before the working-space map (no extra full-frame buffer).
  The report gains per-frame `white_stops`, `leader_distance_stops`, `white_role` and
  `holder_applied`, a `white` section (`stops`, `bound`, `contrast`, `whole_contrast`,
  `clamped`, `rule`), and reuse forms with `--contrast`, `look.contrast` and, when a frame
  is clamped below the roll's contrast, a `roll --frames` manifest. Verified: unit tests on
  the rule's four cases and a guarded highlight; binary tests from `measure-roll` through
  `convert` and `roll --frames`, the all-above-the-cap roll, and the leader path's exit
  codes; the nine rolls (tables above); two review rounds. Code review fixed the leader
  path's exit codes, stale help text and the diagnosis order. **For dependents:**
  - The white is placed at `scene_correction.exposure` 0; an exposure moves it by
    `exposure · contrast` stops (documented in the guide).
  - The roll's cast leaks into the white (measured before white balance; ~0.25 stop on
    Ektar) — accepted.
  - HDR headroom and the gain-map rendition under the rule were **never reviewed**; the
    values stay provisional. `nf-display-stages/parametric-shoulder` re-opens where the
    white renders and is the natural place to look at HDR.
  - `saturation-margin` inherits 1816 warning (not judged overexposed) and the new 1635
    and 1815 warnings.
  - A white target 0.15 stop above diffuse white was "a little better" in some cases;
    +0.31 was worse. Retry +0.15 if a later round wants more contrast.

## saturation-margin

**Status:** not started
**Updated:** 2026-09-25

- 2026-09-25: filed from `anchor-comparison`. Goal: the leader margin the saturation
  warning keys on, and whether frames near saturation need their own treatment.

## no-roll-defaults

**Status:** not started
**Updated:** 2026-09-27

- 2026-09-27: filed while re-planning `nf-destinations/direct-preset`. Goal: the values a
  render uses without a roll measurement, first the default whole contrast (2.0 today,
  kept for continuity; the user suggests about 2.5; the rolls measure 2.23–2.97).

## roll-section

**Status:** done
**Updated:** 2026-09-27

- 2026-09-27: filed while re-planning `nf-destinations/direct-preset`. Goal: a `roll` recipe
  section for what `measure-roll` measures (the gains and the roll's white in stops), out
  of `scene_correction` and `look`, so `--rendering default` can apply it and `direct`
  can leave it out.
- 2026-09-27: implemented (the middle PR of a three-PR stack), awaiting review.
  - **`recipe::RollSection`** — `roll.white_balance`, `roll.white_stops`, flags
    `--roll-white-balance` / `--roll-white` (new-flow only). Unset values are written as
    `null`, never left out: `cli::merge_json` reads a one-key object as an enum switch, and
    a roll's per-frame `{"roll": {"white_stops": …}}` must merge, not replace (pinned by
    `a_frames_own_roll_white_keeps_the_rolls_gains`).
  - **Presence, not value.** "The roll's contrast unless `look.contrast` is stated" needs an
    unset contrast, so the recipe's look keys are their own type, `recipe::LookKeys`, with
    `contrast: Option<f32>`; the stage still receives a resolved `LookSection`
    (`Recipe::shared_params`), and never picks a fallback itself — which is what lets
    `direct` pin its own base later. The default document now writes `"contrast": null`.
  - **Resolution** (`Recipe::resolved_scene_correction`, `resolved_contrast`): the roll's
    gains multiply the stated white balance (identity by default, so they reach the stage
    exactly); the contrast is stated, else `contrast_for(white_stops)`, else the default.
    Validation checks each stated value by its own name, then the product, naming both
    factors; a contrast from the white that fails the whole-contrast rule names
    `--roll-white`, not `--contrast`.
  - **`measure-roll`** writes `--roll-white-balance … --roll-white W`, a `{"roll": …}`
    fragment, and a `--frames` manifest giving each clamped frame the cap as its white.
    Verified: the fragment renders byte-identically to the same values stated as
    `--white-balance` / `--contrast` (`measure_roll_gains_reach_convert_…`).
  - Report: `new_flow.roll` states both values, the derived contrast, and
    `white_balance_applied` / `contrast_applied` (false for the film master, and for the
    contrast when `look.contrast` is stated). The film master does not refuse the section:
    a measurement is not a stage asked for.
  - The `--auto-wb` and per-frame-mode remedies point at `roll.white_balance`.
  - Docs: `using-nc.md` §5/§11 (the `measure-roll` example's reuse values are derived from
    the example's own numbers, not re-run on the 35-frame roll), design-spec §8–§9.
- 2026-09-27: review round (#180). **No value is read as unset by its value.** An
  earlier fix read `look.contrast` at exactly its old serialized default (1.1111112) as
  unset, so a roll's white would apply to recipes older builds wrote; it broke replay —
  `--roll-white 1.7 --contrast 1.1111112 --dump-params d.json` rendered at 1.1111112 and
  `--params d.json` at 1.455 — since a value cannot tell an old default from a choice.
  Removed. Instead `Recipe::roll_overlap_warnings` warns, on a rendered run, where a
  stated `scene_correction.white_balance` (not the identity) multiplies
  `roll.white_balance`, or a stated `look.contrast` overrides `roll.white_stops`, and
  names the migration for a recipe an earlier build or `measure-roll` wrote (drop the
  gains; set the contrast to `null`). Recipe keys only, so one message serves `convert`
  and `roll`. Also: a whole-contrast fault from the roll's white names the remedy in the
  white's direction (the contrast is inverse to it), and its contrast fault drops the
  "1 is the identity" hint; `--roll-white-balance` / `--roll-white` conflict with
  `--film-master` at the parser, as the destination axes do (the recipe section is still
  spared). Deferred: a roll-level warning when a per-frame override sets
  `roll.white_balance`.
- 2026-09-27: review round 2. **The overlap warnings key on provenance.** A typed
  `--white-balance` or `--contrast` beside a roll value is a documented adjustment, and
  warning on it made `--strict` refuse a legitimate workflow; only a value a recipe file
  stated can be a leftover. `roll_overlap_warnings` takes `TypedStyle` (which of the two
  were typed) and skips the film master; `convert` pushes it once per run, `roll` once
  for the shared recipe (no per-frame push, and a per-frame override does not warn). The
  white-balance warning now says "the white balance applied", since the stage also
  applies `2^exposure`. Typed roll flags under a **recipe's** film master are refused
  first in `validate_convert` (`reject_roll_flags_under_a_recipe_film_master`), ahead of
  the suffix rule; a typed `--film-master` still conflicts at the parser.
- 2026-09-27: review round 3. `reject_roll_flags_under_a_recipe_film_master` moved out of
  `validate_convert` to straight after `recipe::merge` in `run_convert`, before
  `recipe::validate`: a bad `--roll-white` got a remedy no white satisfies, and
  `--roll-white 1.7 --contrast 1.3` got the film master's look refusal first. Pinned
  through the binary, with the losing wording asserted absent. `using-nc.md` §11 notes
  that a `--dump-params` recipe stating a contrast or white balance beside the roll's
  warns on replay (a file cannot say who chose it); typing the flag keeps it quietly.
- 2026-09-27: **done**, merged as #180. What dependents need: the roll's measurements
  live in the recipe's `roll` section (`roll.white_balance`, `roll.white_stops`; flags
  `--roll-white-balance`, `--roll-white`), stored as measurements — the contrast is
  derived at render time (`roll_white::contrast_for`) unless `look.contrast` is stated,
  and the gains multiply `scene_correction.white_balance`. `measure-roll`'s reuse output
  writes the section. Nothing is read as unset by its value, so `--dump-params` replays;
  a recipe value beside a roll measurement warns instead (typed flags never do). Whether
  the section applies is the rendering's (`nf-destinations/direct-preset`: `default`
  applies it, `direct` leaves it out).

## white-rule-hdr

**Status:** not started
**Updated:** 2026-09-27

- 2026-09-27: filed (user) from `nf-display-stages/parametric-shoulder`, which kept
  reinhard and so did not look at HDR, the hand-off `roll-white-rule` gave it. Goal:
  review the white rule on an HDR rendition so its cap, floor and target stop being
  provisional.

## roll-exposure

**Status:** not started
**Updated:** 2026-09-29

- 2026-09-29: filed (user) after converting the thin 2026-09-28 roll. Its whites bound
  at the floor and the render was dark; `--exposure 1.4` fixed it, and the user wants
  the darkest frames left dark. Goal: `measure-roll` measures one neutral exposure per
  roll and writes it as `roll.exposure`.

## thin-frame-lift

**Status:** not started
**Updated:** 2026-09-29

- 2026-09-29: filed (user) after hand-lifting frames 2005 and 1983 of the 2026-09-28 roll
  with a `roll --frames` manifest. Slope 2.0 and 2.4 (×1.21 and ×1.46 the roll's), with the
  exposure solved to put each white at diffuse white, both passed. Grain at 2.4 is fine,
  and its white a little dark. Goal: an opt-in, bounded per-frame lift that `measure-roll`
  writes to `roll.frames`.

## scale-ladder

**Status:** done
**Updated:** 2026-09-20

- 2026-09-19: filed after the plan review as `nf-look/path-to-white-spike`. Goal: can a look-stage operator reproduce the knee'd sigmoid's whites?.
- 2026-09-19: moved here and redefined as a `density.scale` ladder. Three reasons.
  (1) `sigmoid-flat` *is* the exponential (`toe = shoulder = 0`), so Appendix E
  already compared the exponential against the knee'd render — but the two presets
  differ in four ways at once (shoulder, anchor 0.28 vs 0.5, `print_exposure`,
  `display_tone`), so nothing in it isolates the shoulder. (2) The scale is the
  higher-leverage question and was sequenced last, behind the whole chain being
  built. (3) The git tag removed the "run it while the sigmoid still exists"
  urgency, so it no longer needs to gate `nf-core/stage-skeleton`.
- 2026-09-19: **green gets measured, not judged.** The user reports they are not
  sensitive to light green, which is the axis `src/types.rs:698` documents as
  unresolved (green splits by scan date, July 0.86–0.90 vs September ~0.77; blue is
  solid at 0.68–0.78). So the eye decides red/blue and preference; green is read off
  neutral surfaces. On the one clean neutral measured — `ektar0909-1608`'s white flag
  — the knee'd render is G/R 1.024, B/R 1.027: a mild *cool/cyan* cast, not green,
  and still the most neutral of the four. But it is one patch on one frame, so the
  knee'd render is a comparison target here, not ground truth.
- 2026-09-19: **first step run — the green split is real, and on one roll green
  alone cannot fix it.** Six cells (2 frames x green 0.77/0.84/0.90, blue held at
  0.73, exponential decode) in **17.4 s** total, ~2.9 s per cell including
  measurement — the all-cores work (#125) makes a ladder cheap enough to iterate.
  Set: `../temp/scale-ladder-probe/`, scripts beside it.

  Whole-frame grey-world means were useless as expected: they put the null at
  ~0.80 (September) vs ~0.85 (July) by one statistic and ~0.845 vs ~0.86 by
  another — the statistic chose the answer, which is Part 3's identifiability
  problem, not a measurement bug. The marked neutral patches
  (`../temp/neutral-patches/patches.json`) cover both frames, so the numbers below
  are read off genuine neutral surfaces instead.

  | patch | roll | light | a* null | b* null | gap |
  |---|---|---|---|---|---|
  | `ektar0715-971` cloud | July | shade | 0.866 | 0.873 | 0.007 |
  | `ektar0909-1608` white flag | Sept | sun | 0.811 | 0.854 | 0.043 |
  | `ektar0909-1608` cloud | Sept | sun | 0.792 | 0.877 | 0.086 |

  Two findings. **(1) The scan-date split reproduces** — July nulls at 0.866,
  September at 0.79-0.81, and the shipped 0.84 fits neither: it leaves a* +7.2
  (red) on the July patch and -7.0 / -11.0 (green) on the September ones. Opposite
  directions by roll, which is what one global value costs today. **(2) The
  September roll cannot be made neutral by green at all** — its a* and b* nulls sit
  0.043 and 0.086 apart, so no green value zeroes both, while the July patch's nulls
  agree to 0.007. That points at blue (or the green/blue relationship) differing on
  that roll too, consistent with the developer change the split is attributed to.
  `src/types.rs:698`'s "blue is the solid half" may hold per roll and still not hold
  across rolls.

  Caveats: three patches on two frames, and they are drawn from the same 31 that set
  the shipped value — so this is a re-measurement through the exponential render
  path, not independent data. The July patch is the only one in **shade** and its
  `g=0.77` cell clipped (the null is interpolated between two unclipped cells, so it
  stands). The September patches erring **green** at the shipped value is the
  direction the reviewer reports not seeing.
- 2026-09-20: **widened to every patched frame — the unit is the roll, not the scan
  date, and there is no offset signature.** 14 frames x 4 green values (0.72–0.93,
  blue held) in **2m26s**, ~2.6 s a cell. Five of the 19 patched frames no longer
  have sources under `../nc-assets` (three 2026-09-09-Ektar100, two
  2026-09-11-Portra400), which cost that Ektar roll most of its sample. Set:
  `../temp/scale-ladder/`, with `analyse.py` beside it. 22 patches, 5 rolls;
  interpolation excludes cells whose median clipped.

  | roll | n | green null | within-roll spread |
  |---|---|---|---|
  | 2026-09-11-Portra400 | 8 | 0.783 | 0.030 |
  | 2026-09-09-Ektar100 | 4 | 0.839 | **0.104** |
  | 2026-07-15-Ektar100 | 1 | 0.865 | — |
  | 2026-07-23-Portra160 | 5 | 0.877 | 0.027 |
  | 2026-07-24-Gold200 | 3 | 0.886 | 0.032 |

  **(1) Per-roll nulls are tight; roll-to-roll they span 0.103.** Four of five rolls
  agree internally to <=0.032, which is what makes a per-roll correction
  well-defined. The exception is 2026-09-09-Ektar100, and its spread is not noise —
  it splits by *frame*, 1608 (sun) ~0.80 against 1627 (shade) ~0.875.

  **(2) The scan-date story is too simple.** July's three rolls cluster
  (0.865/0.877/0.886) but September's two do not (0.783 vs 0.839), and the clusters
  overlap. Stock and scan date are confounded in this set — Portra400 is both the
  lowest null and September-only — so neither can be isolated here.

  **(3) The shipped 0.840 is a well-chosen compromise.** Patch-weighted optimum is
  0.835, roll-weighted 0.850. At 0.840 the worst roll carries -0.057 in green scale;
  at 0.850, -0.067. So retuning the global value buys almost nothing — the cost is
  the spread, not the centre.

  **(4) No offset signature, which cuts against the NLP hypothesis.** If the decode
  error had an offset component, a patch's null would drift with its lightness
  consistently. It does not: the slope is +0.027 per 10 L* on Portra160 (r=+0.97),
  **-0.024 on Ektar0909** (r=-0.94), and +0.001 on Portra400 (r=+0.05) — the
  best-sampled roll, 8 patches over L* 50–80, showing essentially nothing. Opposite
  signs across rolls means the within-roll correlations are picking up scene and
  illuminant differences, not a tone-dependent decode error. On this evidence the
  error is well modelled as a pure slope, and NLP's red shadows are more likely its
  own per-frame fitting than a term missing from ours.

  **(5) The a*/b* disagreement is not clean evidence about blue.** Median gap 0.033,
  8 of 16 patches over 0.03 — but the large gaps land on cars and clouds while walls
  and cloth sit at 0.001–0.007. A surface with a real colour produces exactly this,
  so the gap may measure patch quality rather than the decode. Separating the two
  needs the ColorChecker.
- 2026-09-20: **outside evidence corroborates the shipped value; no tweak is
  available now, and the tuning pass should wait for the anchor.** Three commercial
  converters measured against the same negatives
  (`docs/reports/three-way-gold200.md`) put green at 0.83–0.91 against nc's 0.840,
  and blue between 0.44 and 0.76 — a range too wide to override our own 31-patch
  measurement, which called blue solid at 0.68–0.78 on every roll. Neither value has
  a reason to move.

  **The optimum is anchor-independent, which is why one pass suffices.** With
  `out_c = 10^(gamma·(scale_c·D_c − A))` and a *scalar* anchor `A`, a neutral patch
  needs `scale_R·D_R = scale_G·D_G = scale_B·D_B` — `A` cancels. Moving the anchor
  changes **where the error is visible**, not what the scale should be. (That holds on
  the straight part; a shoulder compresses per channel by level, so a highlight anchor
  would *mask* scale error at the top and leave it in the midtones — an argument for
  measuring the scale in the midtones, not for measuring it twice.)

  And the widened ladder already showed there is nothing to win: patch-weighted
  optimum 0.835 against the shipped 0.840, roll-weighted 0.850, while the roll-to-roll
  spread is 0.103. Retuning the centre moves the worst-case residual from 0.057 to
  0.067 — the wrong direction. So: **do not tune now.** One pass, after
  `nf-reconstruction/anchor-rule` settles and with the ColorChecker frames in hand.
- 2026-09-20: **done.** The question is answered, negatively: the split is real and
  per-roll, and the shipped `[1, 0.84, 0.73]` already sits at the optimum of what one
  global value can do. The HDR check this task listed is moot — it applied to a
  *winning* scale, and nothing moved. The ColorChecker pass is
  [neutrality-gate](../tasks/nf-calibration/neutrality-gate.md)'s, and the post-chain
  tuning is [scale-gamma-loop](../tasks/nf-calibration/scale-gamma-loop.md)'s, which
  already depends on this task and inherits both the value and the method caution.

## scale-gamma-loop

**Status:** done
**Updated:** 2026-09-27

- 2026-09-19: created with the new-flow plan. Goal: tune `scale` and `gamma` by review.
- 2026-09-26: two dependencies added (user). `nf-destinations/direct-preset`: the
  rendering the loop holds fixed is the direct destination, which did not exist yet.
  `nf-display-stages/parametric-operator`: it changes that rendering's operator and adds
  its black point, so rounds judged before it would be re-judged.
- 2026-09-27: **scope set (user): one `scale` round now, `gamma` not tuned, then close.**
  The calibration frames need time to prepare, so this round checks what the new chain
  shows today; the calibrated pass runs once they exist (`neutrality-gate`).
  - **The linearization stays 1.8.** It is the datasheets' slope, not a C-41 process
    constant: `1/0.55`, inside the digitized stocks' red film gamma 0.53–0.61 (1.64–1.89;
    the generic 0.541 would be 1.85; ACES's generic film 0.55). Two reasons it is not reviewable: on a
    neutral it enters only as the product with `look.contrast`, so a round varying it at a
    pinned contrast judges contrast, and holding the product fixed leaves only its effect
    through the 3×3 on colour. And whether 1.8 truly linearizes depends on scanner density
    matching datasheet density (design-update Appendix B), which only the bracket measures.
  - **The look's default 1.11 is `no-roll-defaults`'** (user), not this task's.
- 2026-09-27: **round S1, blue scale — no change** (user). Set `../temp/scale-loop/`
  (`review.json`; `scripts/round1.py`, the rendering binary, `nulls.py`): 18 frames, two per
  roll on all nine rolls (marked-patch frames where a roll has them), blue 0.68 / 0.73
  (shipped) / 0.78, red 1 and green 0.84 held. Held rendering `--rendering direct --range
  sdr` (Display P3 TIFF → JPEG). Blue was chosen as the judged axis; green is the one the
  user does not see, so it was left to the ladder's measurement.

  Blue at which each unclipped marked patch reads b\* = 0 (per-roll median; 09-13 has no
  marked patches):

  | roll | patches | blue null |
  |---|---|---|
  | 07-15 Ektar100 | 1 | 0.707 |
  | 07-23 Portra160 | 4 | 0.725 |
  | 07-24 Gold200 | 2 | 0.727 |
  | 09-09 Ektar100 | 4 | 0.642 (0.53 shaded cloth … 0.72 sunlit flag) |
  | 09-11 Portra400 | 3 | 0.780 |
  | 09-14 Ektar100 | 4 | 0.778 |
  | 09-18 Gold200 | 5 | 0.789 |
  | 09-20 Portra400 | 2 | 0.762 |

  **Blue splits by roll, as green does** (`scale-ladder`): July at the shipped 0.73, four
  September rolls at 0.76–0.79; pooled 0.745–0.757. Green at 0.84 shows the ladder's split
  too (July patches magenta, September green). Under `direct` there is no white balance,
  so a patch's level carries its light and the roll's cast — 09-09's shade/sun gap is
  skylight — and only a within-roll trend with L\* is the scale's signal (09-18 1810 and
  09-14 1713 yellow as L\* rises, favouring higher blue there). Verdict: **hard to tell by
  eye; `[1, 0.84, 0.73]` stays.** One global value cannot close a between-roll spread;
  that needs the known-neutral frames.
- 2026-09-27: **the direct rendering works as the held rendering** (user): with the roll
  unapplied and desaturation off, a candidate's cast shows and nothing masks it.
- 2026-09-27: **done.** No value moved, so no `pipeline_version` bump or fingerprint row.
  **For dependents:** `scale` `[1, 0.84, 0.73]` and linearization 1.8 stand as picks; the
  per-roll split (green and blue) is real and one global value cannot remove it. The
  re-run against the calibration frames — `scale` from a bracketed neutral's slope, and
  whether 1.8 linearizes — is `neutrality-gate`'s. `offset-question` and
  `user-calibration-procedure` inherit the values unchanged.

## offset-question

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: does `density.offset` earn a value?.

## neutrality-gate

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: the neutrality release gate.

## user-calibration-procedure

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: what a user would actually run.

---
name: chart-cast
description: >-
  Measure the colour cast Hanten's settings — and other tools' conversions — leave on a
  roll's ColorChecker frames: render the chart frames through a grid of settings
  (placement, white-balance form, midtone line, highlight desaturation), sample every
  patch as CIELAB, compare with NLP / SilverFast / SilverFast CCR exports, and record the
  result in docs/reports/chart-cast.md. Use when a roll with chart frames arrives, when
  asked "how neutral is it", "what cast does setting X leave", "check the colour against
  the chart", or before changing a white-balance, midtone-line or decode default.
---

# Measure a roll's chart cast

A chart frame turns "is it neutral?" from taste into a number: its grey patches should
read a\* = b\* = 0, and its Classic patches have published values. This skill runs the
whole comparison for one roll and adds it to the running record.

The pixels are the user's photographs: work goes under `../temp/`, never into the repo,
and the only place frames or charts of them may be published is a **private** claude.ai
Artifact. Never read a render into context; `nctool` prints derived numbers.

## Before you start

- **The chart frames are labelled.** Each has `role: "chart"` and `charts` entries
  (corners per target) in `../nc-assets/manifest.json`. If not: label them in
  `tools/review-app` (`pnpm label`), `nctool manifest charts import`, then
  `nctool manifest charts table` (writes `charts.json`, which the density check reads).
- **The roll has pictures** (`role: "real"`, ideally ten or more), its unexposed frame
  and its leader: the roll is measured on the pictures only, so the chart is an
  independent check of what the roll's measurement does.
- `cargo build --release`, and the venv (`scripts/analysis/CLAUDE.md`).
- Outside exports, if any, sit at `../nc-assets/converted/<producer>/<roll>/<frame>.tif`
  **at the scan's size**: the chart corners are fractions of the scan, so a cropped or
  rotated export samples the wrong patches. `measure` reports each file's ICC space;
  a neutral series that does not run in order across a frame is the sign of a misfit.

## 1. Render the grid

```sh
PYTHONPATH=scripts/analysis .venv/bin/python -m nctool manifest charts cast render \
  --roll <roll> --work ../temp/chart-cast-<roll> --nc target/release/hanten
```

The default grid (`nctool.cast.DEFAULT_GRID`) is every placement × highlight
desaturation 0.8 / 0 × roll balance off, or gain / tilt with the midtone line on, off,
red only, blue only — plus `--rendering direct`: 55 configurations, under a minute for
seven frames. A `--grid` JSON overrides keys; its `extra` adds named runs, e.g. a
decode offset:

```json
{"extra": {"hybrid.offset.wb-off": {"recipe": "gain",
  "args": ["--density-offset", "0,0,0.0447", "--neutral-balance", "off"]}}}
```

Ask the user before adding axes; the grid already answers the standing questions.

## 2. Measure

```sh
PYTHONPATH=scripts/analysis .venv/bin/python -m nctool manifest charts cast measure \
  --roll <roll> --work ../temp/chart-cast-<roll> \
  --source NLP=../nc-assets/converted/nlp/<roll> \
  --source SF=../nc-assets/converted/silverfast/<roll> \
  --source SF-CCR=../nc-assets/converted/silverfast-ccr/<roll>
```

It prints one row per source and writes `<work>/cast.json` (every patch, the
summaries, the per-frame casts, the density residuals).

## 3. Read the numbers

Say what each number measures in plain words before quoting it.

- **Grey C\*** is how coloured the greys are overall (0 is neutral); **b\*** is
  yellow (+) / blue (−), **a\*** magenta (+) / green (−). Greys are counted only between
  L\* 15 and 95.
- **A cast that rises with lightness in Lab is not a slope error.** A constant colour
  ratio (one gain) gives a larger a\*/b\* on a lighter patch. Decide slope versus
  offset from the **density** line (`density after scale …`): a near-constant
  channel-minus-green across the bins is an offset, which one gain removes; a fitted
  slope away from 1 is a slope. The over end (+3/+4 frames) bends on the shoulder and
  says nothing about the straight part.
- **Check the a\* terms separately.** On 2026-10-12-Gold200 the midtone line's red and
  blue terms pushed a\* opposite ways, so "line on" looked neutral in a\* only because
  two overshoots cancelled. The red/blue rows show it.
- **Colour accuracy is on the matched frame**, the one whose mid-grey renders nearest
  the card's. Lightness changes chroma, so other frames' hue and chroma mislead; a
  source that never gets near (hybrid placement and NLP brighten every frame) stays confounded.
- **Placement moves lightness, not cast**, and **highlight desaturation** only pulls
  pixels already nearly neutral, so it neither hides nor fixes a cast of C\* 5 or more.
- **The roll's balance never sees the chart.** It is fitted on the pictures, so it
  removes their average light too; a chart shot in different light lands off by that.
  Re-measuring with a decode offset does not change this: the roll's gain absorbs it.
- **A fitted offset is in-sample.** An offset derived from these patches makes them
  neutral by construction; what it tests is whether one constant holds across the
  bracket.
- **Outside tools may balance each frame on their own**, which flatters them on a chart
  frame in a way that does not carry over to pictures. Say so; the files do not tell.
- **Light.** The roll's `conditions.light` says what lit the card. Overcast is bluer than
  the film's balance, so a few b\* of blue can be the true light.

## 4. Report and record

- Publish the charts as a **private** claude.ai Artifact (the user comments there):
  the cast across the bracket, the cast by tone (a\* and b\*), and the full table. Treat
  the page as a draft until the user has read it.
- Add the roll to [`docs/reports/chart-cast.md`](../../../docs/reports/chart-cast.md):
  a new section per roll (conditions, build, the headline rows, what it showed) and an
  update to the running summary at the top. Numbers only — no images, no frame paths
  beyond the roll id.
- Log the finding to the tasks it bears on (`tasks log`), e.g.
  `nf-calibration/stock-neutral-offset`, `nf-calibration/chart-default-review`,
  `nf-scene-correction/midtone-neutral-fit`.

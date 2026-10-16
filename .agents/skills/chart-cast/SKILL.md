---
name: chart-cast
description: >-
  Measure the residual cast Hanten's settings — and other tools' conversions — leave on a
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
desaturation 0.8 / 0 × roll colour correction off (`--neutral-balance off`), or gain /
tilt with the midtone line on, off, red only, blue only — plus `--rendering direct`: 55
configurations, about 50 s for seven frames (`--jobs 6`, an 11-core machine). Once the roll is
measured it clears `<work>`'s earlier renders (it refuses a folder with renders it did
not record). A `--grid` JSON overrides keys; its `extra` adds named
runs, e.g. a decode offset:

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

It reads only the configurations `render` recorded (`<work>/configs.json`), refuses an
export not at the scan's size, prints one row per source and writes `<work>/cast.json`
(every patch, the summaries, the per-frame residual casts, the density residuals).

## 3. Read the numbers

Say what each number measures in plain words before quoting it.

- **Grey C\*** (CIELAB chroma) is how coloured the greys are overall (0 is neutral in
  CIELAB, a\* = b\* = 0); **b\*** is
  yellow (+) / blue (−), **a\*** magenta (+) / green (−). Greys are counted only between
  L\* 15 and 95.
- **A residual cast that rises with lightness in CIELAB is not a density-scale error.** A
  constant colour ratio (one gain) gives a larger a\*/b\* on a lighter patch. Decide scale
  versus offset from the **density** lines (`density after scale …`): a near-constant
  channel-minus-green across the bins is an offset, which one gain removes; a fitted
  density scale against green (over the straight part, 0.2–0.9 green density) away from 1
  is a scale error. The `shoulder` line fits from 0.9 up, where the over end (+3/+4
  frames) bends.
- **Check the a\* terms separately.** On 2026-10-12-Gold200 the midtone line's red and
  blue terms pushed a\* opposite ways, so "line on" looked neutral in a\* only because
  two overshoots cancelled. The red/blue rows show it.
- **Colour accuracy is on the matched frame**, the one whose D4 grey patch renders nearest
  the card's L\* (`dL` says how far it lands). Lightness changes chroma, so other frames'
  CIELAB hue and chroma mislead; a source that never gets near (hybrid placement and NLP brighten
  every frame) stays confounded.
- **Placement moves lightness, not the residual cast**, and **highlight desaturation**
  only pulls pixels already near neutral in graded ACEScg, so it neither hides nor fixes a
  residual cast of C\* 5 or more.
- **The roll colour correction never sees the chart.** It is fitted on the pictures, so
  it removes their average light too; a chart shot in different light lands off by that.
  Re-measuring with a decode offset does not change this: the roll white balance absorbs
  it.
- **The chart frames' placement comes from a second measurement.** The pictures-only
  measurement has no per-frame entries for them, so `render` takes their whole
  `roll.frames` entries (tones, white stops, small lift) from one that includes them, and
  prints how far its `roll.white_balance` lands from the pictures-only one.
- **Only `role: chart` frames are measured.** A chart inside a picture (`role: real`,
  planned in `analysis/calibration-frame-capture`) is not, and that picture votes in the
  roll's measurement like any other.
- **A fitted offset is in-sample.** An offset derived from these patches makes them
  neutral by construction; what it tests is whether one constant holds across the
  bracket.
- **Outside tools may white-balance each frame on their own**, which flatters them on a chart
  frame in a way that does not carry over to pictures. Say so; the files do not tell.
- **Light.** The roll's `conditions.light` says what lit the card. Overcast is bluer than
  the daylight the film is balanced for, so a few b\* of blue can be the true light.

## 4. Report and record

- Publish the charts as a **private** claude.ai Artifact (the user comments there):
  the residual cast across the bracket, and by tone (a\* and b\*), and the full table. Treat
  the page as a draft until the user has read it.
- Add the roll to [`docs/reports/chart-cast.md`](../../../docs/reports/chart-cast.md):
  a new section per roll (conditions, build, the headline rows, what it showed) and an
  update to the running summary at the top. Numbers only — no images, no frame paths
  beyond the roll id.
- Log the finding to the tasks it bears on (`tasks log`), e.g.
  `nf-calibration/stock-neutral-offset`, `nf-calibration/chart-default-review`,
  `nf-scene-correction/midtone-neutral-fit`.

# Capture the calibration frames

## Goal

Produce, scan and register the calibration frames three other tasks are waiting on:
a neutral **series** (not a single patch) plus coloured patches, shot to a protocol
that makes relative log exposures known by construction. Without them, "is nc's
colour neutral?" cannot be answered by measurement — only asserted.

This is an **asset-acquisition** task. Most of the work is photographic, not code;
the code half is registering the result in `manifest.json` and measuring it.

## Why it is its own task

**Four** tasks name these frames as a precondition, each in its own words, and none of
them owns producing them:

- `io/scanner-density-calibration` — needs them for the 3×3 + offset fit. Its tier 1
  is deliberately non-calibrating, so its checkbox can go green without them.
- `nf-calibration/scale-gamma-loop` — needs a bracketed roll and a grey card in
  frame, which is the same shoot.
- `film-base/dmax-per-channel-reduction` — **parked 2026-09-13** for want of exactly
  this (its route 3). Added here after #115 merged; it is the one consumer that was
  not visible when this task was filed on 2026-09-12.
- `nf-calibration/neutrality-gate` — the release gate is neutrality checked against a
  **known-neutral reference, not the leader**. That gate is evidence, not a task, so
  the graph could not see it.

`io/scanner-density-calibration` and `nf-calibration/scale-gamma-loop` each carry a
dated note reaching the same conclusion independently (#115, 2026-09-13: "plan it once",
"shooting for only one of them wastes the other two"). Those notes are the reasoning;
this task is the owner.

Leaving the precondition implicit meant the graph reported work as executable when
the thing actually blocking it was a roll of film that did not exist.

## The protocol

Agreed with the user 2026-09-08, ahead of exposing a set. This is the canonical copy —
`io/scanner-density-calibration` points here rather than restating it, and keeps only a
short note on why each requirement exists for the fit it performs.

- **Target:** an X-Rite ColorChecker Classic. The six-patch neutral row fits the
  matrix diagonal and the offsets; only the coloured patches can constrain the
  **off-diagonal** terms, since cross-channel contamination is a property of the dye
  spectra. A plain grey card is a genuine first step for the diagonal alone.
- **Bracket, always: −2, −1, 0, +1, +2 stops** off a metered reading of the grey
  patch. This is what makes a single patch usable at all — the residual is a
  *slope*, and one patch at one exposure fits an offset without separating it from
  the slope.
- **Light**, in preference order: bright overcast (most even, repeatable); direct sun
  from behind the camera with the card tilted 10–15° against sheen (best channel balance,
  ≈5500 K); clear-sky open shade last — 7000–12000 K starves red into its noisy toe, and
  its colour shifts with any lit surface bouncing in. Record which was used.
- **Geometry:** card flat and square, filling the central ~60% of the frame — vignetting
  is an additive field in density and would corrupt neutrality across the card — at
  f/5.6–f/8, no filters or polariser. One extra frame with the card rotated 180° detects
  uneven light rather than leaving it assumed.
- **Place the set at the head of a roll** that is then shot and scanned normally,
  with that roll's unexposed frame and leader. It must share development batch *and*
  scanning session with real frames, and SilverFast's per-frame automatic
  adjustments must be off or locked — a per-frame-adjusting scanner makes a
  calibration from one frame untransferable to the others, which voids the exercise.
- **Two different rolls**, not one. Per-frame residuals scatter at sd 0.3–1.6 and one
  fixture roll spans −1.88…+2.03, so a single roll cannot separate "the film and
  scanner" from "that roll" — the fork the whole diagnosis sits on.
- **Different subject matter between them — variety is the requirement, not count**
  (from `film-base/dmax-per-channel-reduction`'s 2026-09-13 parking note). Both existing
  whole rolls are one Hawaii trip: blue-dominant, little red, with the bright subjects
  carrying the colour. A cross-roll comparison meant to separate "scanner property" from
  "roll property" is confounded when two rolls share a photographer and a palette, so
  more of the same trip adds n without removing the confound. The target itself removes
  the scene from the measurement, but the surrounding frames should still vary.

## Scope

In: the shoot, development, scanning, registering the frames in `manifest.json` with
their roles and conditions, and a first neutrality measurement against them.

Out: the 3×3 + offset fit itself (`io/scanner-density-calibration`), the sigmoid
parameter values (`nf-calibration/scale-gamma-loop`), the parametric per-channel gain
(`film-base/dmax-per-channel-reduction`), and any default move
(`nf-calibration/neutrality-gate`). This task produces evidence; those consume it.

## Open questions

- **What "role" a calibration frame gets in the manifest.** The existing roles
  describe a roll's frames (leader, unexposed, picture); a bracketed target frame is
  a fourth kind and needs its exposure offset and lighting recorded alongside it.
- **Whether one measurement command serves all three consumers**, or each wants its
  own read of the same frames. Worth resolving before writing any of them.
- **How much scatter survives the protocol.** The bracket removes exposure ambiguity
  by construction, but ~11 frames of one condition were needed to resolve a 0.3
  stops/density difference on ordinary pictures. Whether a controlled target needs
  the same n is unknown and is the first thing the frames themselves will answer.

## How to Verify

- The frames are in `../nc-assets` and registered in `manifest.json`
  (`python -m nctool manifest validate` passes), identified by roll + frame +
  `sha256` like every other asset.
- Both rolls' conditions are recorded — light, bracket offsets, development batch,
  scan session — not left to recall.
- A neutrality measurement runs against them and reports a number, whatever that
  number is. "We measured it and the residual is still there" is a complete outcome.
- The measurement is asset-gated and skips with a clear message when `../nc-assets`
  is absent, like every other real-scan check.

## Dependencies

- [Asset manifest](asset-manifest.md) — the inventory the frames are registered in

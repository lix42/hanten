# Warn when the curve's endpoint is unreachable

## Goal

Report — before decoding — when a resolved curve places its black endpoint so high
that the render cannot approach black. The failure this exists to catch is a
washed-out image from a configuration that passes every other check: the encode-side
loss counter sees highlights leaving the top of the range and is structurally blind
to a black end that never reaches the bottom.

## Design

This **supersedes [`algo/curve-endpoint-validation`](../algo/curve-endpoint-validation.md)**,
which was written against the sigmoid and a reference-anchored world. Two things
survive from it, and they are the whole design:

- **Read the endpoint off the renderer's own curve**, never a re-derived closed form.
  A hand-rolled duplicate is precisely how such a check goes stale, and the original
  file's own worked example is a closed form that silently stops describing the curve.
  Expose a narrow endpoint helper rather than making the curve public.
- **Warning tier, never an error.** It goes in the report and `--strict` promotes it.
  A deliberately flat diagnostic render is legitimate, and refusing to render the
  configuration that demonstrates the bug would be self-defeating.

What does **not** survive: the white check (the reference-free anchor never reads a
reference density, so there is nothing to place white against), the `Dmax` deferral
table, and the sigmoid-specific thresholds. The black endpoint on the new flow is a
function of the resolved curve alone — no pixels, no image statistics.

Two details to decide rather than inherit: the endpoint is **per channel**, so the
aggregation has to be defined (a single lifted channel is a coloured black, which
argues for worst-channel); and the message must say it is about **curve placement**,
not displayed black, since later stages still move where black lands.

## Open questions

- **Does the check still earn its keep?** With a fixed anchor and a fixed calibrated
  gamma the endpoint is fixed too, so on the shipped defaults it can never fire. The
  honest scope may be "only when a calibration term is overridden". Decide this first;
  the answer may be a short rule, or none at all.
- **Where the threshold comes from.** The superseded file's calibration fixtures are
  sigmoid configurations and do not transfer.
- **Does it overlap `algo/density-safety-bounds`?** That bounds each parameter; this
  is about their joint effect. If both land, coordinate the wording.

## How to Verify

- The shipped defaults emit **nothing** — the falsifiable control; a rule that always
  fires is worthless.
- A configuration that lifts the black endpoint trips it, and the message names the
  knob that would fix it and the channel at fault.
- `--strict` promotes it to a non-zero exit, using the IR-free fixture
  `tests/fixtures/hdr-48bit.tif` plus a no-override control run.
- **No pixel change:** `PIPELINE_FINGERPRINTS` untouched, no `pipeline_version` bump,
  and the four CI gates pass.

## Dependencies

- [One anchor rule, with a value for `d`](anchor-rule.md)

# Spike: can a look-stage operator reproduce the knee'd sigmoid's whites?

## Goal

Answer, before any migration code is written, whether a per-channel operator at the
**look** stage can reproduce the behaviour that made `sigmoid-knees` the best-reviewed
render — the whites reading clean. If it cannot, the design needs revisiting while the
sigmoid is still in the tree.

## Design

The behaviour is measured, not guessed: `sigmoid-knees`'s per-channel shoulder pulls
the channels together as pixels brighten, B/R falling 1.65 → 1.27 across one frame's
deciles against 1.76 → 1.48 for the shoulder-less render (design-update Appendix E).
That shoulder sits **inside reconstruction**, which the new design deletes as a
stage-boundary violation, and the plan promises the behaviour back as
[highlight desaturation](path-to-white.md) — an operator that does not exist yet.

This spike tests the promise against today's code, so it needs none of the migration:

- The candidate acts per channel on ACEScg **before** the SDR/HDR branch, which is
  reachable today inside `render_split::display_source`. A throwaway patch is the
  point — the shipped operator is `nf-look/path-to-white`'s, written fresh.
- Render the Appendix E frames through `sigmoid-flat` + the candidate, beside shipped
  `sigmoid-flat` and `sigmoid-knees`, with one nc configuration otherwise.
- Measure with `nctool metrics` and review in `tools/review-app`, matched for
  brightness — an unmatched comparison is evidence for a different claim.

**Why it gates the rest.** `path-to-white` sits at depth 5 in the graph, so without
this spike the plan discovers whether its central promise holds *after* the sigmoid is
retired and the defaults have flipped. Running it first costs a day and either
de-risks the whole `nf-look` epic or changes the design before 46 tasks are underway.

## Open questions

- **What "approaches white" is measured on** — luminance, the max channel, or distance
  from the neutral axis. The three disagree exactly where saturated highlights live.
- **Whether the pull preserves hue** or is allowed to shift it, as a per-channel curve
  does. The knee'd sigmoid shifts hue; that may be part of what the eye likes.
- **Whether the mechanism is even chroma-toward-white.** If the numbers move but the
  picture still reads wrong, the eye is responding to something else — the most
  valuable outcome this spike can produce.

## How to Verify

A written answer in `docs/progress/nf-look.md`, with the measured decile trend beside
the two references and a visual verdict. Three outcomes are all complete:

- the trend moves toward the knee'd render's **and** the user ranks it level — the
  promise holds, and `path-to-white` inherits the parameterisation;
- the trend moves but the look does not — the mechanism is not chroma-toward-white,
  recorded before anything depends on it;
- it cannot be made to work — the design is revisited while the sigmoid still exists.

## Dependencies

None — it runs against today's binary, deliberately, so that it can run first.

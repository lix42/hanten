# A depth-aware holder mask

## Goal

Give the pipeline one function that answers "how far in from each edge does the
opaque film holder reach on this frame", using the IR plane where it measures usable
and a stated fixed fraction where it does not. Four tasks need exactly this region
and none should own a copy of it.

## Why it is its own task

Split out of `holder-masked-measurement` on 2026-09-13. The existing
`EdgeHolderMask` marks segments *along* each edge at a 0.5% probe depth and, by its
own comment, "restricts along the edge only, not in depth". The consumers waiting on
a depth:

- `holder-masked-measurement` masks the holder before estimating `Dmin`/`Dmax`, then
  changes the estimator (a `pipeline_version` bump);
- `algo/auto-anchor-interior-measurement` excludes the holder from every whole-frame
  statistic (its first cut), then insets past the rebate;
- `tiling-uniformity-validator` tiles the masked region;
- `dmax-anchor-reliability`'s roll-wide content direction needs the same region.

Bundled with the estimator change, the primitive was held behind a pixel change and a
version bump none of the other consumers needs. On its own it changes no pixels.

## What is known

- Holder depth is small and **asymmetric**: on the unexposed HP5 frame IR clears at
  ~2% of the short edge on the right, ~3% top and bottom, ~5% left. On other frames the
  holder occupies **10–15% of one edge** (`analysis/conversion-metrics`). One number
  cannot serve every frame, which is why this is measured, not configured.
- The IR plane makes the depth test a threshold march inward per edge;
  `auto-base-redesign` already marches inward for the rebate.
- `film_base::ir_separability` already decides per frame whether IR can separate
  holder from film. Consume that verdict; do not re-derive it.
- **Do not inherit the all-holder decline.** `ir_holder_mask` returns `None` when no
  film is found *along* any edge, which is 22 of 25 real chromogenic frames. In a
  depth-aware design that reading means "the holder wraps the whole border", the normal
  case, and the answer is to keep marching inward.
- When IR is absent or unusable, the fallback is a fixed fraction. Measured need is
  2–5%; `REBATE_SCAN_FRAC` is 10%. Reuse a fraction that exists or justify a new one;
  the consumers' *inset* for the rebate is a different thing and stays with them.
- Provenance is per run: the report says whether the mask came from IR or the fallback,
  and how deep.

## Open questions

1. One depth per edge, or a per-segment profile along each edge? Per edge is what the
   consumers need; per segment is what the existing mask has.
2. Whether `film-base/auto-base-real-scan-refusal` finds the auto detector's 10% scan
   cap is the reason it never fires; if so, this depth is what that detector should
   march past too.

## How to Verify

- A synthetic frame with a deliberately deep, asymmetric holder yields the per-edge
  depths the fixture encodes; a frame whose holder wraps the entire border is still
  measured, not declined.
- On a frame where IR is absent or measures unusable, the fallback runs and the report
  says so, distinguishing "no holder found" from "not measured".
- Output pixels and dimensions are byte-identical on every path.

## Dependencies

- [Decide IR usability by measurement](ir-usability-detection.md) — the per-frame
  verdict this consumes.

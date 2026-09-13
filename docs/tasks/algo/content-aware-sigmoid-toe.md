# Content-Aware Sigmoid Toe (Optional)

## Goal

Offer an explicit convenience mode that derives sigmoid-toe placement from
image content without changing the reference-anchored product default or
silently correcting exposure.

## Design

Add one tagged toe-source choice rather than parallel booleans:

- `reference` (default): the fixed Dmin-origin, Dmax-anchored behavior from
  `reference-anchored-sigmoid`;
- `frame`: derive the statistic independently for one frame;
- `roll`: derive once from an explicitly selected roll calibration set and
  freeze the result for every frame.

The content statistic, percentile rules, sample exclusions, bounds, and fallback
must be deterministic, exposed through CLI and recipe JSON, and fully reported.
No mode may change white balance, Dmin, Dmax, exposure, or shoulder placement as
a hidden side effect.

Because frame-local fitting changes relative exposure, `frame` is advanced
`custom` output only and is rejected by `film-master` and normal product
presets. A future implementation may permit `roll` with film-master only if the
derived value is frozen, serialized, reusable, and acceptance proves that
cross-frame exposure is preserved.

This task is optional and blocks no product output. Its existence must not cause
the required reference task to accumulate content-analysis code.

## Implementation Suggestion

- Reuse the roll planner and manifest/report machinery rather than scanning
  sibling files from an algorithm stage.
- Separate statistic acquisition from the pure sigmoid evaluation; the latter
  receives only a resolved toe value.
- Make fallback to `reference` loud and reportable; `--strict` promotes it.
- Include explicit provenance (`reference`, `frame-auto`, `roll-auto`,
  `recipe`, or `cli`) in the resolved report.

## How to Verify

- Merge and validation tests cover every source, flags-win behavior, illegal
  preset combinations, and unknown recipe fields.
- Repeated runs over identical inputs are bit-identical on one build/architecture.
- A roll-order test proves `roll` resolution is independent of traversal order
  and applies one serialized value to all frames.
- Under/overexposed fixtures demonstrate that `frame` changes tone only when
  explicitly selected and cannot masquerade as a film master.
- The default recipe and all named product presets remain byte-identical when
  this task lands with `toe_source = reference`.

## Dependencies

- [Exclude the holder from content-driven measurement](auto-anchor-interior-measurement.md) —
  **hard prerequisite.** Content-driven anchoring is currently unusable: `DmaxSource::Auto`
  takes the 99.5th percentile over the whole scan, so the nearly-opaque film holder owns it
  (measured 2.23–2.37 against roll Dmax 1.28–1.38) and every frame renders black. Any
  content-derived toe placement inherits that defect until the holder is excluded.
  **What that task will and will not hand over** (rescoped 2026-09-12): it removes the
  holder, and it deliberately does **not** detect the rebate — which sits at `D ≈ 0`,
  harmless to a high percentile but squarely on the **low** end this task reads for a toe or
  black point. Its second cut is a blind fixed inset (5% default) that happens to clear the
  rebate; whether that is enough for a toe statistic is **this** task's question to measure,
  not an assumption to inherit. It also crops nothing: output dimensions are unchanged.
- [Reference-anchored sigmoid reconstruction](reference-anchored-sigmoid.md)
- [Roll conversion](../core/roll-conversion.md)
- [Output presets and guidance](../output/presets.md)

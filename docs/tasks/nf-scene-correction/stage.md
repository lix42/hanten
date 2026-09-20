# Scene correction as a named stage

## Goal

White balance and exposure become a real stage: scene-referred, linear, resolved
once, reported, with its own recipe section. It is the first rendering stage the
new flow runs after reconstruction.

## Design

- **Written fresh.** Today these are a fused arithmetic expression inside
  `render_split::apply_shared_controls` (WB → exposure → black point →
  `linear_range`, one loop body). That file is context for *what* the arithmetic
  does, not a template for the stage boundary — see CLAUDE.md's migration rule.
- The stage owns **white balance** and **exposure**. Both act on working-space
  channels, *after* the 3×3, so neither is the decode's `offset` or anchor:
  design-update Part 1 measures the two white-balance bases ≈2.6 % apart on a
  neutral. Exposure is where brightness is set now that the anchor is a
  reference-free convention.
- **Flare/fog removal joins this stage** and `linear_range` may — both are
  separate tasks below, and this one only has to leave room for them.
- White balance has an auto mode today; the resolved gains and their
  **provenance** (stated vs estimated) are report fields, not just numbers.
- The `print.*` recipe prefix retires with the legacy path; this stage picks its
  own section name, and the flag spellings (`--print-exposure`) are open with it.

**Open: where a positive-scanned input enters the chain.** A positive has no
reconstruction to run, so the new flow needs a named entry point for it — this
stage, or the working space just ahead of it.
`docs/tasks/io/positive-input-mode.md` carries over and is written against
whichever seam this settles; it currently names the old shared-controls entry.
Answer it here rather than leaving that task pointing at a retired function.

## How to Verify

- An identity configuration is bit-exact through the stage.
- A stated white balance and exposure reach the render and reproduce a
  hand-computed value on a synthetic frame.
- Auto white balance still resolves, and the report distinguishes the two
  provenances.

## Dependencies

- [The new flow's stage skeleton](../nf-core/stage-skeleton.md)
- [The fixed decode](../nf-reconstruction/fixed-decode.md)

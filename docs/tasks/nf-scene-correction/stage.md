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

## Outcome (2026-09-22)

- **Knobs:** `scene_correction.white_balance` (`{"explicit": [r, g, b]}`,
  `"gray-world"`, `"percentile"` — `--white-balance` / `--auto-wb`, spelled as on the
  current chain) and `scene_correction.exposure` in stops (`--exposure`, new;
  `--print-exposure` is refused under `--new-flow` naming it, and `--exposure` without
  the flag naming `--print-exposure`). Both fold into one per-channel gain on linear
  ACEScg, after the 3×3.
- **Auto white balance estimates over the effective area**, not the whole frame, so
  it is a consumer of the measurement region: an empty region is a refusal under it.
  The estimators moved to `pipeline/white_balance.rs`; the current chain reaches them
  through a whole-frame adapter and its output is byte-identical (checked
  same-machine, both fixtures, both modes, `legacy` and `display-p3`).
- **Reported** in `new_flow.scene_correction` — the gains applied, `provenance`
  (`stated` / `estimated`, with the estimator and region) and the exposure — and the
  stage's `applied` string is derived from those resolved values. Stating an
  estimate's gains reproduces the frame byte for byte.
- **The positive-input question is answered: a positive enters at the working space**
  (`AcesCgImage`), ahead of scene correction, because a slide needs white balance and
  exposure as much as a negative does. `io/positive-input-mode` is retargeted.

## How to Verify

- An identity configuration is bit-exact through the stage.
- A stated white balance and exposure reach the render and reproduce a
  hand-computed value on a synthetic frame.
- Auto white balance still resolves, and the report distinguishes the two
  provenances.

## Dependencies

- [The new flow's stage skeleton](../nf-core/stage-skeleton.md)
- [The fixed decode](../nf-reconstruction/fixed-decode.md)

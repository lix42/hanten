# Extract the chart patches

## Goal

One extraction that reads every labelled chart and writes a per-patch table — roll, frame,
target, patch, the scan's value per channel and its spread, with the frame's shot facts and
the roll's conditions — that every calibration consumer reads, instead of each consumer
re-reading the frames its own way.

## Known

- Agreed with the user 2026-10-14: one extraction, many consumers. The consumers are
  `calibration-frame-capture`'s.
- Reads the scan, not a render: a bracket's over end is clipped by any render (on
  `2026-10-12-Gold200` +3/+4 lose 6% and 21% under `--placement frame`).
- Each frame carries two neutral series: the Classic's six and the Creative target's
  bottom row (⅓-stop steps by design).
- Patch positions are projected from the chart entries at read time; nothing per patch is
  stored in the manifest.
- Never reads pixels into an agent's context; asset-gated like every real-scan check.

## Open

- The table's units (transmission, density, or both) and whether a producer's output
  (NLP, SilverFast, CCR, Hanten) is a column or a separate read.
- What else a consumer needs per frame (aperture: 2132 and 2133 were shot wider than f/8).

## How to Verify

- The table covers every labelled chart on the archive and reruns byte-identical.
- The neutral series rise monotonically with exposure across the 2026-10-12-Gold200 bracket.

## Dependencies

- [Label the charts in a frame](chart-labeling-tool.md) — the chart entries it reads

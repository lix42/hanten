---
title: Measure the cast settings and tools leave on the chart frames
status: done
deps: [analysis/chart-labeling-tool, analysis/chart-extraction]
summary: "`nctool manifest charts cast`, the `chart-cast` skill and `docs/reports/chart-cast.md`: every chart patch through a grid of settings and beside outside tools"
---
# Measure the cast settings and tools leave on the chart frames

## Goal

Turn "is it neutral?" into a number for every chart roll: render a roll's chart frames
through a grid of settings, sample every labelled patch, set other tools' conversions of
the same frames beside them, and keep the result as a running record.

## Done (2026-10-15)

- `nctool manifest charts cast render` / `measure` (`scripts/analysis/nctool/cast.py`):
  the roll measured on its pictures only, the chart frames rendered through
  `DEFAULT_GRID`, every patch as CIELAB D50, and the chart table's density residuals.
- The `chart-cast` skill: the procedure and how to read the numbers.
- `docs/reports/chart-cast.md`: the record, opened with `2026-10-12-Gold200`.

## How to Verify

- `NCTOOL_REQUIRE_DEPS=1 … -m unittest nctool.test_cast` passes.
- On `2026-10-12-Gold200`, `render` + `measure` reproduce the report's rows.

## Dependencies

- [Label the charts in a frame](chart-labeling-tool.md) — the corners every patch is
  projected from
- [Extract the chart patches](chart-extraction.md) — `charts.json`, which the density
  check reads

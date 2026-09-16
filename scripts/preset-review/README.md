# preset-review

The visual-review matrix for the five conversion presets
([`algo/conversion-presets`](../../docs/tasks/algo/conversion-presets.md), filed out of
[`algo/film-stock-profiles`](../../docs/tasks/algo/film-stock-profiles.md)).

`presets.matrix.json` is **data**, not a script: it names the five configurations and the
flags each one passes. The renderer is `nctool review generate`, which is shared with every
other matrix.

```sh
cargo build --release
# Once per checkout: `.venv/` is gitignored, so a fresh one does not have it.
uv venv --python 3.12 && uv pip install -r scripts/analysis/requirements.txt

PYTHONPATH=scripts/analysis .venv/bin/python -m nctool review generate \
  scripts/preset-review/presets.matrix.json                    # writes ../temp/preset-review/
… --frames P3,G2                                              # a subset
… --out /tmp/review                                           # elsewhere
… --no-metrics                                                # render only, no charts
```

It writes a `review.json` for [`tools/review-app`](../../tools/review-app/README.md) and
prints the command to open it. Frames and per-roll `Dmin` come from
`scripts/sigmoid-baseline/fixtures.json` — the same declaration the metrics use, so the two
cannot drift.

**Measuring is the one part that needs the venv**, because it reads pixels; the rest of
`nctool` is stdlib-only. A plain `python3` still renders the whole set — it says once, before
anything renders, that it will have no charts, and writes a `review.json` with no `metrics`
keys, which the app shows as pictures with "no measurement" under each. `--no-metrics` is the
same thing asked for on purpose.

**Each button is `nc convert --preset <name>` and nothing else.** The generator used to
state the expansion by hand, which made it the mechanism's acceptance test — and the test
passed on 2026-09-10: 12 of 15 renditions byte-identical, the three that differed being
`chr-aim`, where the script's constants were rounded to three decimals and `nc` derives the
value exactly. See `docs/progress/algo.md`.

**Every preset carries the exposure that keeps brightness steady when you switch** — scene
mid-grey 0.18 delivered at 0.4525 (1.33 stop up) on `portra-400`, the target approved on
2026-09-15. It removes exposure as a variable so the comparison is about the reconstruction
and the tone; it is not a claim that the presets should look alike, and mid-grey is not
promised to land identically on another stock. `nc` owns those numbers now, so a
recalibration cannot leave this set rendering the previous ones;
`pipeline::stages::midtone_placement::presets_land_the_calibration_target_on_the_calibration_stock`
fails if a bundle drifts off it on the calibration stock, and prints the per-stock spread.

A config's `args` may not restate what the generator supplies — `--output-preset`, `-o`,
`--report` — because `nc` takes the last occurrence of such a flag and the override would be
silent. The preset is stated once, as `output_preset`; each cell is then *measured* in the
space its own resolved recipe reports, not in whatever that preset's name usually implies.

**Which configs take a `--film-stock` is stated by their own `args`**, through the
`{film_stock}` placeholder, and the roll → registry-id mapping is in the matrix's `rolls`
block. Nothing is derived from a name: the fixtures call a roll `2026-07-24-Gold200` while
the registry calls the stock `gold-200`. A roll whose stock states no usable aim delta
(`portra-800`, `ultramax-800`) loses only its `chr-aim` cell, reported as a failed cell,
rather than costing the whole frame.

**`metrics.inset` trims the film holder and rebate out of the statistics.** 0.18 per edge
clears them on the three rolls checked (P3, G2, E1: 0.000% of pixels below L\* 5), which is
the check — a luminance histogram with a hard spike at the bottom of the L\* axis is
measuring the holder, not the picture, and the app draws that histogram. Re-check it on a
roll framed more tightly; a holder can occupy 10-15% of an edge, and more on some scans.

**Needs `../nc-assets`, so it is deliberately not in CI**, and it writes to a throwaway
directory *outside* the repo — the frames are the user's own photographs and are never
committed. Only the matrix is. The records beside each rendition hold derived numbers and
never pixels; a cell whose render fails is reported and skipped rather than silently
dropped from the page.

The convention that is easy to get backwards — the aim-matched density scale being a
*reciprocal*, since `--density-scale` multiplies the *scan's* density where the aim factor
scales the *table's* — lives in `algo::film_stock::aim_red_scale`, checked by its own tests
rather than by a comment.

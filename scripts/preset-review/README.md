# preset-review

The visual-review set for the five conversion presets
([`algo/conversion-presets`](../../docs/tasks/algo/conversion-presets.md), filed out of
[`algo/film-stock-profiles`](../../docs/tasks/algo/film-stock-profiles.md)).

```sh
cargo build --release
python3 scripts/preset-review/generate.py            # writes ../temp/preset-review/
NC_PRESET_FRAMES=P3,G2 python3 scripts/preset-review/generate.py   # a subset
NC_PRESET_OUT=/tmp/review python3 scripts/preset-review/generate.py
```

It writes a `review.json` for [`tools/review-app`](../../tools/review-app/README.md) and
prints the command to open it. Frames and per-roll `Dmin` come from
`scripts/sigmoid-baseline/fixtures.json`.

**Each button is `nc convert --preset <name>` and nothing else.** The script used to state
the expansion by hand, which made it the mechanism's acceptance test — and the test passed
on 2026-09-10: 12 of 15 renditions byte-identical, the three that differed being `chr-aim`,
where the script's constants were rounded to three decimals and `nc` derives the value
exactly. See `docs/progress/algo.md`.

**Every preset is calibrated to one brightness, not to its own taste** — scene mid-grey
0.18 delivered at 0.223 (0.31 stop up), the target approved on 2026-09-09. `nc` owns those
numbers now, so a recalibration cannot leave this set rendering the previous ones;
`pipeline::stages::midtone_placement::every_preset_lands_the_shared_brightness_target`
fails if any bundle drifts off the shared target.

A roll whose stock states no usable aim delta (`portra-800`, `ultramax-800`) loses only
its `chr-aim` cell — reported as a failed rendition — rather than being skipped entirely
as it was when the script carried its own table of aim constants.

**Needs `../nc-assets`, so it is deliberately not in CI**, and it writes to a throwaway
directory *outside* the repo — the frames are the user's own photographs and are never
committed. Only the script is. It prints derived numbers (per-preset G/R and B/R means)
and never pixels; a frame whose render fails is reported and skipped rather than silently
dropped from the page.

The convention that is easy to get backwards — the aim-matched density scale being a
*reciprocal*, since `--density-scale` multiplies the *scan's* density where the aim factor
scales the *table's* — now lives in `algo::film_stock::aim_red_scale`, checked by its own
tests rather than by a comment.

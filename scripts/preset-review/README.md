# preset-review

The visual-review set for the five proposed conversion presets
([`algo/conversion-presets`](../../docs/tasks/algo/conversion-presets.md), filed out of
[`algo/film-stock-profiles`](../../docs/tasks/algo/film-stock-profiles.md)).

```sh
cargo build --release
python3 scripts/preset-review/generate.py            # writes ../temp/preset-review/
NC_PRESET_FRAMES=P3,G2 python3 scripts/preset-review/generate.py   # a subset
NC_PRESET_OUT=/tmp/review python3 scripts/preset-review/generate.py
```

It writes a `review.json` for [`tools/review-app`](../../tools/review-app/README.md) and
prints the URL to open. Frames and per-roll `Dmin` come from
`scripts/sigmoid-baseline/fixtures.json`.

**`--preset` does not exist yet.** Each preset is rendered through the plain flags it will
expand to, which makes this set the mechanism's acceptance test: regenerating through
`--preset` must produce identical files.

**Every preset is calibrated to one brightness, not to its own taste** — scene mid-grey
0.18 delivered at 0.223 (0.31 stop up), the target approved on 2026-09-09. The
`--print-exposure` values therefore differ per preset, and `sig-knees` cannot use that
knob at all: `--display-tone none` is bounded by the render's ceiling, so a scalar gain
after the curve is refused and its brightness comes from `--anchor-mid-fraction` instead.
`pipeline::stages::midtone_placement` pins both facts and fails if the calibration drifts.

**Needs `../nc-assets`, so it is deliberately not in CI**, and it writes to a throwaway
directory *outside* the repo — the frames are the user's own photographs and are never
committed. Only the script is. It prints derived numbers (per-preset G/R and B/R means)
and never pixels; a frame whose render fails is reported and skipped rather than silently
dropped from the page.

The extraction conventions that are easy to get backwards — the aim-matched density scale
being a *reciprocal*, and `--print-exposure` not being comparable across tone operators —
are recorded in `generate.py`'s module docstring. Read it before changing a constant.

# `nctool` analysis toolkit

`nctool` is the repository's Python command line for asset inventory, repeatable
roll conversion, and conversion analysis.

Run it from the repository root:

```sh
PYTHONPATH=scripts/analysis python3 -m nctool --help
```

### Dependencies

Every command except `metrics` and `acceptance` uses only the Python standard
library, and stays that way. Those two read output pixels, which needs `numpy`,
`tifffile` and `Pillow` (the last for JPEG):

```sh
uv venv --python 3.12
uv pip install -r scripts/analysis/requirements.txt
PYTHONPATH=scripts/analysis .venv/bin/python -m nctool metrics image --help
```

**[uv](https://docs.astral.sh/uv/) is how the environment is made**, here and in
CI — one shape, and about twenty times faster than `venv` + `pip` (3.8 s to 0.2 s
warm, measured). What it produces is an ordinary virtual environment in `.venv/`,
so everything downstream is unchanged and `python3 -m venv .venv && .venv/bin/pip
install -r …` still works if you have no uv.

**The Python version is stated, not inherited.** `uv venv` otherwise takes
whichever interpreter it finds first — which is not the system one, and differs
between machines and between the two CI runner images. uv fetches 3.12 itself, so
nothing has to be installed first.

`.venv/` is gitignored, as a virtual environment always should be: it holds
compiled, platform-specific wheels and absolute paths, so it is build output
rather than source. A fresh checkout therefore runs the two lines above once.

The import is lazy, so a checkout without the venv still runs every other
command. The metrics tests skip when the packages are absent; CI installs them
and sets `NCTOOL_REQUIRE_DEPS=1`, which turns a missing install into a failure
instead of a silent skip.

The default asset root is `../nc-assets`. Override it with `--asset-root` or the
`NC_ASSET_ROOT` environment variable.

## Asset manifest

```sh
PYTHONPATH=scripts/analysis python3 -m nctool manifest generate \
  --asset-root ../nc-assets --nc target/release/hanten
PYTHONPATH=scripts/analysis python3 -m nctool manifest validate \
  --asset-root ../nc-assets
PYTHONPATH=scripts/analysis python3 -m nctool manifest roles \
  --asset-root ../nc-assets
PYTHONPATH=scripts/analysis python3 -m nctool manifest patches import notes.md \
  --review ../temp/<set>/review.json --roll <roll> --source '<set> <date>' --kind white
```

- `generate` inventories source rolls, samples, and converted outputs; obtains
  derived metadata from `hanten inspect`; and streams files through SHA-256. Existing
  human fields such as roles, stock names, notes and patches are preserved. It
  refuses to drop a missing frame's patches without `--drop-patches`, and
  `--carry-from <manifest>` takes a restored frame's fields from another manifest.
- `validate` reports checksum drift, missing files, misplaced/orphaned TIFFs, and
  integrity gaps. It never deletes or moves anything.
- `roles` emits the unexposed/leader/real-frame grouping consumed by the legacy
  real-scan harness.
- `patches import` folds the review app's **Copy all** text into frames' `patches`
  (schema in `nctool/patches.py`), replacing each listed frame's patches from the
  same `--source`.

`generate_manifest.py` is a compatibility wrapper for older callers. New code
should use `python -m nctool manifest generate`. `manifest.sample.json` is a
trimmed schema example; the live manifest belongs at the asset root.

## Manifest-driven roll conversion

This command automates the calibrate-once/apply-many workflow from
`docs/using-nc.md`:

```sh
PYTHONPATH=scripts/analysis python3 -m nctool roll convert Ektar \
  --nc target/release/hanten \
  --config default-p3 \
  --strict-estimate
```

It performs these operations:

1. Finds the roll's single `unexposed` frame and all `real` frames in
   `manifest.json`.
2. Verifies every source frame against its manifest SHA-256, so stale asset bytes
   cannot be attributed to a configuration change.
3. Measures Dmin from the unexposed frame. By default (`--dmin-mode area`) that is
   `hanten measure-base` with no source: the median over the frame's effective area, which
   takes no region. `--dmin-mode region` reads the center 80% (`x=10%`, `y=10%`,
   `width=80%`, `height=80%`, or `--dmin-region`) at p97; `--dmin-mode grid` is the
   five-cell grid, for a build that predates the effective-area measurement (the
   reference build). No `Dmax` is measured: the roll reference density
   retired with the placements that read it (`nf-retire/dmax-machinery`).
4. Reads the tested binary's complete default recipe (`hanten profile`; `hanten
   params` on the reference build), overlays the optional partial recipe, then
   freezes the measurements. This pins defaults such as the curve's anchor placement
   instead of letting a later build reinterpret an underspecified recipe.
5. Runs `hanten roll` over the real frames with that shared recipe.
6. Writes `recipe.json`, `calibration.json`, `roll-report.json`, and `tags.json`
   beside the converted images.

The default destination is:

```text
<asset-root>/converted/nc/<config>/<roll>/
```

If `--config` is omitted, a stable ID is derived from the frozen recipe. A
non-empty destination is refused so a new run cannot silently mix with or
overwrite an old configuration.

Use `--recipe FILE` for the full configuration surface. It accepts a partial nc
recipe, bare or in the `{meta, params}` envelope hanten writes; the measured Dmin
deliberately replaces any film base in it. An enveloped one keeps its `meta` in
`recipe.json`, so `hanten roll` checks the `pipeline_version` it was written under. `--film-type` is a convenience
override, and so are the destination flags `--film-master` or `--range`,
`--transfer`, `--gamut`, `--container` (the recipe `output`) and `--exposure`
(`scene_correction.exposure`). Those write keys of `recipe_version` 2 and later, so they need a
build that takes destinations; against a preset build (the reference build) they
are refused before anything is measured, and its output goes in `--recipe`
instead. `--strict-estimate` is recommended for calibration; `--strict-roll` is
separate because on the reference build a frozen explicit base on an IR scan
legitimately emits its unused-IR warning.

### Tags

`tags.json` is a small index for the run. It records the configuration ID, source
roll, source-frame checksums, frozen recipe, calibration frames/regions/values, build identity, report
path, and roll summary. `calibration.json` retains the complete `hanten measure-base`
reports (`estimate` on a build from before the rename, which `nctool roll` detects by
asking the binary). The roll report carries each frame's identity (with the hash of the recipe
the frame ran) and, on a destination build, its resolved destination (`chain`, named
`new_flow` by earlier builds); a destination build writes no per-image sidecar, so
`recipe.json` plus that report is the whole record.

After a successful TIFF-producing conversion, regenerate the asset manifest so
its converted bucket includes the new TIFFs:

```sh
PYTHONPATH=scripts/analysis python3 -m nctool manifest generate \
  --asset-root ../nc-assets --nc target/release/hanten
```

The current manifest schema inventories TIFF artifacts only. A gain-map JPEG or
HDR AVIF run is still fully described by its `tags.json`, roll report, and
optional `analysis.json`, but `manifest generate` will not add those container
files to `manifest.json` yet.

## Analyze a converted roll, then compare with `diff`

```sh
PYTHONPATH=scripts/analysis python3 -m nctool roll analyze Ektar default-p3
PYTHONPATH=scripts/analysis python3 -m nctool roll analyze Ektar adobe
diff -u \
  ../nc-assets/converted/nc/default-p3/Ektar/analysis.json \
  ../nc-assets/converted/nc/adobe/Ektar/analysis.json
```

The run operand is a configuration ID or an explicit path to `tags.json`.
`analyze` writes `analysis.json` beside that tag by default; use `--out FILE` to
choose another destination. The artifact contains:

- the frozen recipe, calibration, build identity, and source checksums;
- the output depth, from the destination each frame resolved (or, for a preset
  build, from the recipe's preset);
- stable per-frame film-base, input-semantics, output-statistics, clipping,
  identity, rendering-facts (`chain`, or an earlier build's `new_flow`), status, and
  warning fields;
- deterministic key and frame ordering.

It deliberately omits timestamps, elapsed time, memory/machine facts, and
absolute input/output paths, which would create irrelevant diffs. It does not
reread pixels, so equal analysis files mean the recorded conversion facts agree;
they do not prove that output files are byte- or pixel-identical.

## Measure a converted image's own pixels

Every command above derives its numbers from `nc`'s JSON report, so they exist
only for nc outputs. `metrics` reads the output *image*, so an NLP conversion, a
SmartConvert TIFF, or an export edited by hand can be measured on the same
footing:

```sh
PYTHONPATH=scripts/analysis .venv/bin/python -m nctool metrics image \
  /Volumes/blackbox/full-assets/converted/nlp/2026-07-23-Portra160/1102.tif \
  --space linear-srgb --inset 0.05
```

It reads **TIFF and JPEG**, dispatched on the file's magic bytes rather than its
extension.

`--space` is required and never inferred. A file's samples do not say whether
they are transfer-encoded — the NLP TIFF exports are 32-bit float with a *linear*
sRGB profile, nc writes transfer-encoded u16, and SmartConvert writes u16 with
no profile at all — so guessing produces a plausible wrong table rather than an
error. Supported: `srgb`, `linear-srgb`, `display-p3`, `linear-display-p3`,
`adobe-rgb`, `linear-adobe-rgb`, `prophoto`, `prophoto-gamma1.8`,
`linear-prophoto`, `linear-bt2020`, `linear-acescg`, `film-rgb` — which covers the usual
Lightroom export choices. The two ProPhoto entries are deliberate: `prophoto` is
ISO 22028-2 as specified, with the linear toe, and is right for a third-party
export; `prophoto-gamma1.8` is the pure power law nc's retired
`--output-profile prophoto` wrote (the reference build still writes it). They agree above encoded 0.03125 and diverge sharply
below it, so the wrong one silently rewrites the deep-shadow statistics. PQ and HLG are
recognized and refused with a reason: they are absolute or display-referred, so
comparing them with an SDR rendition needs a reference-white normalization this
command does not implement yet.

`film-rgb` is for `hanten convert --export-film-rgb`: the fixed decode before the
NC film RGB v1 3×3, whose channels are the dye layers and have no primaries. Its
record has only `channels` (each channel's key, stop percentiles and spread, in the
file's own channels) — no `tone`, `color` or `bands`, which need primaries. `metrics
image --channels` adds the same block to any other space, so an export compares with
its film master (`linear-acescg`) field for field. `metrics roll` refuses `film-rgb`:
a roll tracks tone and colour axes.

### JPEG input

8-bit JPEG is read as well as TIFF, which is what makes an NLP or Lightroom JPEG
export measurable without a re-render. Two things ride in the record because a
lossy read needs them: `image.bits_per_sample` (8 bits is 256 levels, and the
tone metrics are logarithmic, so a JPEG's deep-shadow percentiles and `toe_span`
are quantization-limited — cross-checked against a 16-bit TIFF of the same
content, the key and p95 agreed exactly while p0.1 moved ~0.015 stops), and
`image.decoder`, naming the Pillow/libjpeg build that produced the samples, since
a JPEG's pixels are whatever its decoder says they are.

`--jpeg-image sdr|hdr` chooses which rendition of a **gain-map** JPEG to measure.
`sdr` is the default and reads the base image — which is also all a plain JPEG
has. The record marks `gain_map_present` so the base of a dual-image file is never
mistaken for the rendition an HDR-aware viewer shows. `hdr` is **not implemented**
and says so: reconstructing it means applying the gain map with its ISO 21496-1 /
Ultra HDR metadata, and a reconstruction that is subtly wrong yields plausible
wrong numbers rather than an error. Measure nc's own `--transfer linear --gamut
bt2020` render (a linear BT.2020 float TIFF) of the same source instead.

`--inset F` trims that fraction off each edge and `--region x,y,w,h` takes an
explicit rectangle; both are **fractions**, because the images being compared do
not share dimensions. Use them to keep the film holder and rebate out of the
statistics until `film-base/ir-holder-detection` can supply a mask — and check
the inset actually clears the holder, which can occupy 10-15% of an edge.

It reads every sample in the region rather than subsampling, so peak memory
scales with the frame: ~1.4 GB at 18.7 MP, ~5.7 GB extrapolated to a 10368x7200
scan. Runtime is ~3.0 s at 18.7 MP.

The record reports endpoint occupancy on the stored (encoded) samples, then,
after decoding to linear light:

- **tone**, in log2 stops relative to 0.18 — the key (geometric mean), a
  percentile vector, contrast spreads, toe and shoulder spans, band occupancy,
  and an L\*-binned histogram for luminance and each channel;
- **colour**, in CIELAB — per-channel balance in stops, mean cast and chroma,
  neutral share, chroma percentiles, six hue sectors, and the cast of each tone
  band separately.

### Tone bands

The bands are cut in **CIELAB lightness** — every 15 L\* up to 75, then diffuse
white (L\* 100), then an overflow band above it. `record.bands` states the cut,
in both L\* and stops, inside every record it applies to.

Lightness rather than stops because equal steps of lightness are unequal steps of
exposure, and a cut even in stops is even in nothing a viewer sees. Until schema
2 the edges were -4 / -2 / +2 stops and diffuse white, after Zones III and VII;
across 33 real renders (six frames x five `hanten convert --preset` bundles plus
three Negative Lab Pro references) that put a median 83% of the frame — 95% at
worst — in `mid` alone, while `highlight` spanned 0.47 stops and read 0.00 on
four of the five renders of one frame. The lightness cut's largest band holds a
median 46% and 56% at worst. On the five renders of one frame the old band vector
spread 2.1 percentage points from preset to preset, so the five presets were
effectively one reading; the new one spreads 24.0.

`above_diffuse_white` is an **overflow bin, not a seventh of the range**. An SDR
rendition essentially cannot populate it, and on a float or HDR output it is the
only place in the tone stage where headroom above display white appears.

`color.cast_by_tone_band` is the one to read first on a negative conversion. The
characteristic fault is **crossover** — the cast drifting one way in the shadows
and the other as the frame brightens — and a whole-frame cast averages exactly
that out to nothing. On one measured nc-versus-NLP pair the nc render went from
`b* = -0.7` in deep shadow to `-32.2` in midtones where NLP moved `-0.1` to
`-3.8`; the whole-frame means alone would have understated it.

The rollup's `crossover_a` / `crossover_b` axes difference the **`shadow` and
`mid`** bands specifically — not shadow and highlight. Across those 33 renders
the smallest `shadow` was 4.2% of the region and the smallest `mid` 5.7%, while
`highlight` legitimately empties on a dark frame, which would make the axis
vanish exactly where a render is darkest.

Each `cast_by_tone_band` entry carries its own denominator — `pixels`, and
`sparse` when the band holds under 0.1% of the region. Sparse entries are kept
rather than dropped, because a band set that varies frame to frame cannot be
diffed, but **a sparse band's cast is not a measurement**: on the old cut the
largest colour excursion in one measured record, a\* = -19.5 in `highlight`, was
the colour of a **single pixel** out of 15.1 million, printed beside a `mid` cast
resting on 91.7% of the frame with nothing to tell the two apart. The
rollup's `crossover_a` / `crossover_b` are withheld outright when either
contributing band is sparse.

### The histogram

`tone.histogram` is the record's only list-valued field, and the one thing in it
a review tool can draw rather than read. Four series — `luminance`, `r`, `g`, `b`
— each 200 counts, one per L\* unit, plus `above_range` for anything past the top
and separate counters for samples with no lightness at all (non-positive,
non-finite). Those four numbers partition the region. It states its own domain in
the record — `domain`, `lstar_range`, `bins`, `bin_width_lstar`, and the
`mid_grey_bin` / `diffuse_white_bin` reference lines a chart wants — so a
consumer never has to infer the bins from the shape of the data, and never has to
re-derive the L\* formula to place white.

`luminance` uses the **declared space's own luma weighting**, the same one
`tone.percentiles_stops` is built from, so the histogram and the percentile curve
describe one quantity and cannot disagree. That is also why luminance is emitted
rather than left to be derived at draw time: luma is a weighted sum of linear
channel values and is **not** recoverable from three independent per-channel
histograms.

The axis runs to **twice diffuse white in lightness**, not to diffuse white.
L\* 200 is 6.46x diffuse white (+5.17 stops), which covers nc's own 1000/203 HDR
ceiling (L\* 181.4) with margin, so a `film-master` or `hdr-linear-tiff` render's
headroom can be *drawn* rather than reduced to one overflow number. It also keeps
white inside the axis rather than at its edge, which is what makes the commoner
SDR question readable: how far short of diffuse white the highlights stop. On the
five preset renders of one frame the last non-empty luminance bin sits at L\* 88 /
92 / 88 / 92 / 98 — 12, 8, 12, 8 and 2 L\* short of white — against a
`diffuse_white_bin` of 100.

L\* and not the stored code values: those describe the file's encoding as much as
the picture, which is the whole reason this command decodes to linear light
first. L\* and not stops: stops give black an unbounded tail no chart can draw.
And because the bands are cut on the same axis, every band edge falls exactly on
a bin edge — one chart can shade the bands over the bars without interpolating.

The channel series apply the same L\* curve to one channel. That is a level, not
a colorimetric lightness — only `luminance` is that — but it is the one monotone
mapping that puts all four series on one axis, which is what makes a cast read as
a shape rather than as `color.balance_stops`' one number per channel.

It costs ~0.6 s at 18.7 MP and no measurable memory: it streams in row blocks, so
only the 200 accumulators per series survive a block. A record grows to ~8 KB.

### Reading the record

`color.balance_support` states what fraction of the region each channel's
geometric mean rests on. When they disagree — a channel crushed to black over
part of the frame — `r_over_g` / `b_over_g` are **omitted** rather than reported
against different pixel sets, which once made a heavily cast frame read as
perfectly balanced.

Two things the numbers mean, which are easy to misread:

- `endpoints.at_or_above_white` is an **upper bound** on what the producer
  clipped: a sample that legitimately landed on the endpoint is indistinguishable
  from one clamped to it. On an nc output the report's `loss.*` counters are the
  independent check, and the two agree to rounding when clipping is what happened.
- `tone.shoulder_span_stops` of 0 means p95, p99 and p99.9 are the same value —
  the top of the distribution is one flat step. On an uncropped scan that is
  usually **the film holder**, not the render: the holder blocks all light, so it
  is maximum density in the negative and renders to white. On one measured frame,
  tightening the inset from 0 to 0.15 took the top-code population from 9.5% to
  0% and the shoulder span from 0.000 to 0.417. Measure a region before concluding
  anything about highlights.

Colorimetry is not restated here: the primaries, white points and Bradford matrix
are transcribed from `src/pipeline/colorimetry/definitions.rs`, and the tests
re-read that file and the generated `derived-artifacts.txt` and fail if the
Python drifts from either. To support a new space, define it there first — which
is how `definitions::ADOBE_RGB` came to exist before nc rendered to it, and why
`definitions::PROPHOTO` stays although nc no longer does.

### A whole roll at once

```sh
PYTHONPATH=scripts/analysis .venv/bin/python -m nctool metrics roll Ektar default-p3 \
  --inset 0.08 --markdown docs/reports/ektar-default-p3.md
```

The run operand is a configuration ID or a path to `tags.json`, as for `roll
analyze`. It measures every successfully converted frame and writes
`metrics.json` beside the tag, with per-frame records embedded plus a spread
table; `--markdown` also renders the table, and `metrics table <metrics.json>`
re-renders it later without re-reading pixels.

The colour space is **resolved from the run's recorded provenance** here rather
than declared — not a guess at the pixels — and an under-determined one is refused
rather than defaulted. On a build that takes destinations it is the destination the
roll report says every frame resolved (`chain.destination`), since the frozen
recipe may leave its axes to nc; the frames must agree:

| destination (gamut, transfer) | space | notes |
|---|---|---|
| `display-p3`, `native` | `display-p3` | a gain-map JPEG is read as its **SDR base**, per `--jpeg-image` |
| `adobe-rgb`, `native` | `adobe-rgb` | |
| `srgb`, `native` | `srgb` | a gain-map JPEG as above |
| `display-p3` / `adobe-rgb` / `srgb` / `bt2020`, `linear` | `linear-display-p3` / `linear-adobe-rgb` / `linear-srgb` / `linear-bt2020` | |
| `"film-master"` | `linear-acescg` | |

`pq`/`hlg` transfers and the AVIF container are refused with the reason. On a
build that takes presets (the reference build) it is the frozen recipe's preset:

| preset | space | notes |
|---|---|---|
| `legacy`, `custom` (default profile) | `srgb` | retired before the reference build's successors |
| `legacy`, `custom` + `--output-profile` | that profile's space | `prophoto` resolves to `prophoto-gamma1.8` |
| `compatibility` | `srgb` | |
| `display-p3` | `display-p3` | |
| `gain-map-hdr`, `ultra-hdr-v1` | `display-p3` | the JPEG's **SDR base**, per `--jpeg-image` |
| `film-master` | `linear-acescg` | |
| `hdr-linear-tiff` | `linear-bt2020` | |

Everything else is refused with the reason: `hdr-pq`/`hdr-hlg` write AVIF, the
coded HDR TIFFs are PQ/HLG encoded, an `--output-profile` path has no primaries
here, and an f32 `legacy` TIFF's transfer was never established. `--space`
overrides all of it.

A gain-map JPEG (an HDR Display P3 destination, or a preset build's default
`gain-map-hdr`) measures as its **SDR base**. That is a real rendition, not a
fallback — it is what a non-HDR viewer shows — but it is not what an HDR-aware
viewer shows, and the per-frame records mark `gain_map_present` accordingly.

A frame that fails to measure is recorded in `skipped` and the command exits 1 —
the rest of the roll is still measured and written, but a partial roll never
reports success.

Read the spread, not the mean: frame 3 is a backlit portrait and frame 11 a
shaded street, so averaging their exposures describes the subjects. What the
spread is **not** is attributable — one frozen recipe served every frame, so
variation combines scene content with how well that calibration fits, and those
cannot be separated from one roll's numbers. The extremes are named so you can
look at those frames. There is deliberately no outlier rule and no verdict.

## Render a review set — `review generate`

Comparing conversions **by eye** goes through `tools/review-app`, and this is what
produces what it reads:

```sh
PYTHONPATH=scripts/analysis .venv/bin/python -m nctool review generate \
  <matrix.json> --out ../temp/<set>
```

The matrix is **data** (the `render-review-set` skill writes one per set): it names
the configurations and the flags each one passes, with `{dmin}` standing for each
frame's film base. Every cell is one `hanten convert`; beside each
rendition the command writes that image's metric record, so the app can draw the
tone and cast charts next to the picture. Frames and per-roll `Dmin` come from
`scripts/analysis/fixtures.json` — the same declaration the metrics read,
so the two cannot drift.

Four rules it holds to, each of which has a reason rather than a preference:

- **The matrix states the output once per interface its builds speak.** A build
  at `pipeline_version` 8 or later takes a **destination**: the matrix's
  `destination` is the recipe `output` value with all four axes stated,
  `{"display": {"range", "transfer", "gamut", "container"}}`, or `"film-master"`,
  and the generator passes the destination flags. An earlier build — the reference
  build — takes a **preset**, stated as `output_preset`, passed as
  `--output-preset`. Which a build gets is read off its own `--version` banner,
  never from its name; a build axis mixing the two states both, and a build whose
  interface the matrix does not state is refused before anything renders. A config
  may not restate these or any other flag the generator supplies (`-o`,
  `--report`), because `nc` takes the last occurrence of such a flag and the
  override would be silent. Every destination axis is stated rather than left to
  `nc`'s derivation, so the suffix and the metrics' colour space are read off the
  matrix, keyed on the container and on (gamut, transfer).
- **Each cell is measured in the space its own render reports**, not in whatever
  the output's name usually implies — a destination cell by the
  `chain.destination` it resolved, a preset cell by its resolved recipe (the
  reference build's `legacy` and `custom` accept `--output-profile`, and measuring
  ProPhoto pixels as sRGB yields a table where every number is wrong and every
  number looks reasonable).
- **A cell that fails costs only itself.** A roll that states no film stock loses
  the one column that needs it; a failed render leaves its config without a
  rendition, which the app draws as a visible gap.
- **Re-measuring is keyed to the image's checksum**, not its mtime — a run
  re-renders every cell, so an mtime always looks new while the bytes rarely are.

It needs `../nc-assets` and the venv, so it is not in CI, and it refuses an output
directory inside the repository: the frames are the user's own photographs and are
never committed.

## Compare two builds

The older `compare run|diff` workflow answers a different question: how one fixed
benchmark behaves under two `nc` builds.

```sh
PYTHONPATH=scripts/analysis python3 -m nctool compare run \
  --nc /path/to/baseline/nc --out before.json
PYTHONPATH=scripts/analysis python3 -m nctool compare run \
  --nc /path/to/candidate/nc --out after.json
PYTHONPATH=scripts/analysis python3 -m nctool compare diff before.json after.json
```

Cases come from `benchmark.json`. The default `fixtures` set is self-contained: the
HDRi fixture through every ready destination (all four axes stated), the film master,
`--rendering direct` and the product default, and the HDR 48-bit fixture (the other
input format) through the default and the film master.
Two runs of one build over it diff to zero, and CI checks exactly that. The `rolls`
set resolves real scans and checksums through the asset manifest.

A case carries one block per output interface it applies to: `destination` for a build
at `pipeline_version` 8 or later, `preset` for the reference build
(`scripts/reference-snapshot/`). Each block holds that interface's `args` and an
optional `recipe`; the case's own `args` go to every build it runs on. Which interface
a build speaks is read off its `--version` banner, as `review generate` does, so a
reference-build record and a current one share case names and `diff` pairs them. A
case with no block for the build's interface is not run, and the record lists it in
`not_run`. This is how a destination the reference cannot write (Adobe RGB, the linear
P3/sRGB/Adobe RGB TIFFs, the sRGB gain map, `direct`) stays out of a reference run
instead of being rendered as some other preset under its name. `diff` reports it as
`not-run`, not `missing`. A preset block that renders a display image states the
reference config, `--preset sigmoid-knees`, so the preset blocks run on the reference
build alone: a build at `pipeline_version` 6 or 7 refuses that preset, and its own
checkout's nctool and set are the ones to benchmark it with.

The pre-migration pipeline is compared by re-running the reference build over this
set; records made before 2026-10-01 are superseded.

The `params_hash` is read from the report's `identity`; a destination build from
before `nf-core/report-contract` reports it only in telemetry, so there a missing
telemetry record fails the case instead of merely losing its timings.
Run records include build identity, pipeline version, input digest, parameter
hash, output depth, means, clipping counts, telemetry timings, and the cases not run. Timing changes
are informational and never decide the deterministic-statistics verdict; a stage
only one record times (a schema-8 `algorithm` beside schema-9 stages) diffs as `null`.

## Decode-back acceptance — `acceptance run`

Decodes every output encoding **without nc** and checks it against the buffers nc's
encoder received — the gate `analysis/display-output-acceptance` runs on real scans.

```sh
PYTHONPATH=scripts/analysis .venv/bin/python -m nctool acceptance run \
  --nc target/debug/hanten --out result.json
```

Each case is converted with `hanten convert --export-pre-encode`, which writes those
buffers (the **canonical** ones: the linear rendition before any transfer, and a gain
map's codes before its JPEG). The output is then decoded from the standards — its ICC
profile parsed by `nctool.icc`, BT.2100 by `nctool.rec2100`, ISO 21496-1 and MPF by
`nctool.gainmap`, JPEG by Pillow — and its encoding's oracle compares the two:

| Encoding | Pixels | Bound |
|---|---|---|
| film master, HDR linear TIFF | the file's f32 against the canonical buffer | bit-identical (written verbatim) |
| SDR TIFF | the standard transfer of the canonical buffer, quantized to 16 bits | 1 code |
| PQ / HLG TIFF | BT.2100 of the canonical buffer in binary64, quantized to 16 bits; the profile's `A2B0` against BT.2100 | 1 code; 0.2 % or 0.1 cd/m² |
| gain-map JPEG | the map's gains at its own grid; each JPEG against its pre-JPEG codes | ½ map step; measured JPEG max/RMS |

Every case also gets a **metadata** check (the TIFF's layout; the profile's primaries,
white, TRC and `cicp` against the standard; the gain map's MPF, ISO fields and window
against the renditions) and a **determinism** check (two runs, byte-identical). A file
that cannot be read at all is a failed `decode` check, not a crash. An HDR TIFF also
gets a **content_light** check: its report's `max_cll_nits` / `max_fall_nits` against
CTA-861.3 on the canonical buffer (the peak and mean of each pixel's largest channel,
within the half-nit rounding), and their absence for HLG. The gain map is
gated at the map's resolution because the map is half resolution: no full-resolution
reconstruction matches the HDR rendition pixel for pixel, so that error is reported
(`reconstruction`), not gated.

**The cases are the benchmark's** (`benchmark.json`'s `fixtures` set), re-targeted at
`acceptance.json`'s `inputs` — today the synthetic chart `tests/fixtures/chart-48bit.tif`
(`nctool.chart`: a neutral ramp, saturated and muted patches). The chart drives the
patch bounds of the lossy encodings and the **cross-encoding** check: every rendition's
patch means as XYZ (reference white at `Y = 1`) against the BT.2020 HDR linear TIFF's, at
ΔE00 ≤ 0.5 and neutral Δu'v' ≤ 10⁻⁴. 8-bit quantization alone exceeds that, so the gain
map has a measured allowance in the manifest. After editing `nctool.chart`, regenerate
the chart with `acceptance chart` (a test fails until you do).

Nothing compares against a committed checksum: decode pixels differ by target, so the
run makes the canonical buffer and the file together. For a single-machine run (the
real scans), `--write-golden PATH` records each case's buffer and file hashes, metadata
and metric summary, keeping a changed entry's old values under `previous`, and
`--golden PATH` fails a case that moved. Exit `0` passed, `1` a check failed or a case
would not convert, `2` usage or an operational failure (including a build without
`--export-pre-encode`). `--write-golden` keeps the cases a run did not cover and writes
only when every oracle passed; a golden mismatch alone does not stop it, so `--golden G
--write-golden G` re-baselines a deliberate change and keeps the old values under
`previous`. Determinism reruns a byte-identical encoding for its digests only.

## Viewer file set and pre-checks — `viewer set`, `viewer check`

`viewer set --out DIR` renders every destination of the benchmark's `fixtures` set from
each input of `viewer.json` (the chart and two real frames), with `viewer-set.json` and a
`rubric.md` checklist; `viewer check DIR --oracle PATH` decodes each gain-map JPEG with
Apple ImageIO and libultrahdr. Stdlib only. The procedure, and why, is
[`../viewer-interop/README.md`](../viewer-interop/README.md).

## Datasheet digitization — `digitize_datasheets.py`

Not part of `nctool`, and not stdlib-only: it reads the vector characteristic curves in
`docs/datasheets/` and writes `src/film_stock/curves.json`, the intermediate that
`film_stock::curves`'s pinned Rust literals are audited against.

```sh
python3 scripts/analysis/digitize_datasheets.py            # rewrite curves.json
python3 scripts/analysis/digitize_datasheets.py --check    # verify, change nothing
```

It needs poppler (`brew install poppler`) and is run **by hand**, never in CI — the same
split as `pipeline/colorimetry/`: extraction needs a toolchain, while "the literals match
the extraction" is a plain `cargo test`. The file's own module docstring carries the
extraction traps; read it before editing.

## Tests

The CI command is:

```sh
NCTOOL_REQUIRE_DEPS=1 PYTHONPATH=scripts/analysis python3 -m unittest discover \
  -s scripts/analysis -p "test_*.py"
```

The tests are hermetic: they use temporary asset manifests, committed tiny TIFF
fixtures, and images synthesized in the test itself, rather than the Drive-hosted
scans. `NCTOOL_REQUIRE_DEPS=1` makes a missing `numpy`/`tifffile` a failure
instead of letting the metrics tests skip while the run still prints `ok`; leave
it unset locally if you have not made the venv. The harness tests, `compare`'s
end-to-end test (the `fixtures` set, run twice), the acceptance tests and `viewer`'s
end-to-end test additionally need `cargo build` to have produced `target/debug/hanten`.

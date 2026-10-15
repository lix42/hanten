---
name: render-review-set
description: >-
  Build a visual review set — an image set converted through two or more
  configurations, optionally beside another tool's output (SilverFast, SilverFast
  CCR, Negative Lab Pro) — write its review.json, and offer to serve it
  in tools/review-app. Use when asked to compare destinations, parameters or builds by
  eye, to "render a review set", "build a comparison", "see how X looks across the
  roll", to judge a default before changing it, or to put an outside reference beside
  Hanten's renders.
---

# Render a review set

Comparing renders by eye goes through [`tools/review-app`](../../../tools/review-app/README.md),
never a one-off HTML page. Every configuration of a frame occupies **one grid cell**, so
switching between them cannot move the picture by a pixel — which is what makes highlight
differences visible at all.

A review set has four inputs. Settle each with the user before rendering anything, because a
wrong one costs a full re-render:

| Input | What it is | Where it comes from |
|---|---|---|
| **Image set** | which frames | `../nc-assets/manifest.json` (`role: "real"`), a named subset, or arbitrary files |
| **Config set** | what to compare | one matrix entry per configuration |
| **References** | another tool's output | the matrix's `references`, paired through `../nc-assets/converted/<producer>/` |
| **Destination** | where it lands | a fresh folder under `../temp/`, never inside the repo |

## Before you start

- `cargo build --release` — the generator shells out to the binary.
- Venv, for measuring only: `uv venv --python 3.12 .venv && uv pip install --python .venv -r
  scripts/analysis/requirements.txt`. The generator is stdlib-only apart from measuring, so a
  set still renders without it — but then run `python3` in place of `.venv/bin/python`, since
  the command in step 4 names an interpreter that will not exist, and pass `--no-metrics`.
  A set built that way has no charts, and **no reference cells**: converting another
  tool's export (step 5) needs the venv even with `--no-metrics`.
- `../nc-assets` must resolve (machine-local symlink).
- **The frames are the user's own photographs**: output goes outside the repo and is never
  committed or published anywhere public (a private claude.ai Artifact is allowed). `nctool review generate` refuses an output directory inside it.

## 1. The image set

`../nc-assets/manifest.json` is the inventory. Keep `role: "real"` frames —
`unexposed` / `leader` / `calibration` / `chart` are reference frames, not pictures. Frame counts go
stale fast, so derive them at run time rather than trusting any written-down number.

Narrow the set with the matrix's `frames` list, or `--frames a,b,c` on the command line.
Prefer a whole roll when judging a default, a handful when iterating on one parameter.

**Sources outside `../nc-assets/rolls/`** are not addressable by the generator, which builds
each path as `rolls/<roll>/<file>`. For those, run `hanten convert` directly and hand-write
`review.json` — the schema is [`tools/review-app/SCHEMA.md`](../../../tools/review-app/SCHEMA.md)
and a rendition is just a path.

## 2. The film base, per roll

`dmin` is the one fact the manifest does not carry, and every conversion needs it. Measure it
once per roll from that roll's unexposed frame:

```sh
./target/release/hanten measure-base ../nc-assets/rolls/<roll>/base.tif --report json
# take film_base_flag / the film_base object
```

It reads the median over the frame's effective area (holder and inset cut). **Never silently
reuse a value that came with a warning** — a "not uniform" one means the frame is not
unexposed film; a "film holder was not measured" one (every 48-bit scan) means the area
rests on the inset alone. Say so. (A reference build predates this and needs `--grid`.)

## 3. The config set

The matrix is **data**, so comparing something new is a JSON edit, not code. Write copies;
never edit `scripts/analysis/fixtures.json` in place — it describes a curated study and
its roll names predate the asset rename.

Two files, built by a short script kept beside the set. The frame lists come from
`manifest.json`; the `dmin` values do **not** — the manifest does not carry them, so the script
has to merge in the per-roll measurements from step 2. Miss that and `review generate` skips
every cell whose `common_args` reference `{dmin}`.

- **fixtures copy** — `{"rolls": {<roll>: {"dmin": [r,g,b]}}, "frames": {<key>: {"roll",
  "file"}}}`. Nothing else per frame is read. Keys must be filename-safe and unique across
  rolls: `<short-roll-tag>-<serial>` (`g200-1137`, `ektar0909-1605`).
  **`file` is relative to the roll directory, not to the asset root** — the generator builds
  `<assets>/rolls/<roll>/<file>`, so copying the manifest's own `rolls/<roll>/<serial>.tif`
  produces a doubled path and every frame is reported missing and skipped.
- **matrix copy** — `schema_version` (must be `1`; the loader refuses the document
  otherwise), `output_dir`, the output — `destination` for a current build, `output_preset`
  for the reference build, both when a build axis mixes them (see below) — `common_args`
  (usually `["--film-base", "{dmin}"]`), `metrics.inset`, and one entry per configuration
  under `configs`. `{dmin}` is the one placeholder.
- **a build axis, when comparing two binaries** — an optional top-level `builds`, one
  `{"id", "label", "nc"}` per binary (`note` optional). Every config is then rendered by
  every build, as cells with the composed id `<config>@<build>` and the label
  `<config> · <build>`; a config may state its own `"builds": ["after"]` to opt out of one,
  which is what to do when a flag only the new binary accepts would otherwise fail on every
  frame. `--nc` is refused beside a `builds` block — repoint one arm with
  `--build <id>=<path>` instead, which is the flag for the rebuild loop. A build may add
  `"expect_commit": "<hex>"`: its binary must then be a clean build of that commit — checked
  on `--version` before anything renders, and on every cell's report — or the run stops. Use
  it on a reference arm, where a wrong path otherwise renders
  and is merely *labelled* with the wrong commit. The pre-migration reference arm is built by
  [`scripts/reference-snapshot/`](../../../scripts/reference-snapshot/README.md).

Rules, each with a reason:

- **The output is stated once per interface, and each build gets the one it speaks.** A
  build at `pipeline_version` 8 or later takes a `destination`: the recipe `output` value
  with **every** axis stated (`{"display": {"range": "sdr", "transfer": "native", "gamut":
  "display-p3", "container": "tiff"}}`) or `"film-master"`, rendered with the destination
  flags. An earlier build — the reference build — takes an `output_preset`, rendered with
  `--output-preset`. The generator reads each build's `pipeline_version` off its own
  `--version` banner, never off its id or label, so a matrix comparing the reference arm
  with a current one states **both** (`"output_preset": "display-p3"` beside the SDR
  Display P3 TIFF destination writes the same container on both arms). A build whose
  interface the matrix does not state is refused before anything renders (exit 2).
- A config may not restate any flag the generator owns — `--output-preset`, the destination
  flags (`--range`, `--transfer`, `--gamut`, `--container`, `--film-master`), `-o` /
  `--output`, `--report`, `--report-file`. `hanten` takes the **last** occurrence, so an
  output override would be silent, and redirecting the report to a file stops the generator
  reading the resolved output back from stdout. The loader rejects them by name, so a config
  that restates one fails before anything renders. A flag only one arm has (`--exposure` on
  a current build, `--preset` on the reference) goes in a config that opts out of the other
  arm with `"builds"`.
- A cell that fails costs only itself; the app draws the gap.
- Re-measuring is keyed to the image's **checksum**, not mtime, because a rerun re-renders
  everything.
- `metrics.inset` should clear the film holder — check it, since a luminance histogram with a
  hard spike at the bottom is measuring the holder, not the picture.

## 4. Render

```sh
PYTHONPATH=scripts/analysis .venv/bin/python -m nctool review generate \
  <matrix copy> --fixtures <fixtures copy> --nc target/release/hanten --out ../temp/<set>
```

`--no-metrics` renders without charts; `--force` re-measures. Budget ~3 s per cell on
17–50 MP frames, so a few hundred cells is tens of minutes — run it in the background.

**With a build axis, keep the config list short.** The cells are the product, and only ten
have a number key: two builds over five configs already spends all of them.

Three things the generator checks for you, none of which needs a flag. **The first two are
not build-axis features** — a matrix with no `builds` is one unnamed build, the `--nc`
binary, and both rules apply to it verbatim:

- Every binary is verified **before anything renders** — it exists, its `--version`
  really is this project's CLI, and it states a `pipeline_version`, which picks the output
  interface it renders through. That includes a plain `--nc` binary, which used to fail at
  the first render instead. The **pre-rename `nc` banner is accepted**, which matters because
  the reference arm of a before/after is usually the pre-rename reference build, which
  prints it. A render reporting another `pipeline_version` than the banner stated aborts the
  run like any other identity change.
- A binary that reports **one identity and then another** aborts the run — a build by name,
  or the `--nc` binary by path. It writes no `review.json`, and it *deletes* an earlier run's
  `review.json` from the output directory **when this run overwrote a cell that file names**
  (otherwise it leaves it and says so). The binary changed underneath, so every cell already
  rendered under it is suspect too — including the ones overwritten in place, which a stale
  set would go on attributing to the previous binary while the app refreshed onto the new
  pixels.
- Two builds that are the **same file** are a note, not a refusal — that is how you check the
  set is honest, since their cells must come out byte-identical. Two *different* files that
  report the same identity (two dirty builds of one commit) are also a note: the labels
  cannot tell them apart, and nothing can. (This one *is* build-axis-only; one binary cannot
  collide with itself.)

**Smoke-test unusual flag combinations on one frame first.** A configuration Hanten refuses
(`--display-tone-headroom 0` on a frame with highlights above white, say) is worth finding in 15 seconds rather than after
a full run.

## 5. References from another tool

SilverFast (with and without CCR) and NLP exports go in the matrix as `references`,
and the generator turns each into a cell. The rules are in
[`scripts/analysis/README.md`](../../../scripts/analysis/README.md) ("Reference
cells"):

```json
"destination": {"display": {"range": "sdr", "transfer": "native", "gamut": "srgb", "container": "jpeg"}},
"references": [
  {"id": "sf",  "label": "SF",  "producer": "silverfast"},
  {"id": "ccr", "label": "CCR", "producer": "silverfast-ccr"},
  {"id": "nlp", "label": "NLP", "producer": "nlp"}
]
```

- **Hanten's cells must be SDR sRGB JPEG** when there are references, and the loader
  refuses anything else. A gain-map cell would show its HDR rendition next to a plain
  SDR export.
- **Pairing is the manifest's `source_frame`.** Read the run's notes: exports that
  name no `source_frame` are listed before rendering (some NLP batches have it `null`
  on most entries), and frames with no export are gaps with a note. Fix the manifest,
  never the matrix.
- **An export with no ICC profile needs `"space"`** on its reference (SilverFast's
  `2026-09-20-Portra400` has none, and its space is recorded as inferred in the
  manifest note). Otherwise its cells fail and say why.
- **Charts appear only for exports with the scan's exact dimensions.** A re-cropped
  export gets the picture and a note but no charts, because the same fractional inset
  would measure different scene content.
- Converted cells are rendered **into the set**, with a `.reference.json` beside each.
  A rerun converts again only when the source, its space, the converter or the Pillow
  version changes, or the JPEG no longer matches its recorded sha256.

An export **not in the manifest** (a one-off, a producer with no bucket) is still
manual: convert it to an sRGB JPEG from its own ICC profile, check one file against
`sips -m` first, measure it with `nctool metrics image --space srgb --out
<set>/<frame>-<producer>.metrics.json`, and merge it as in step 6.

## 6. Assemble `review.json`

The generator writes one review file per run, references included. Merge in anything rendered
elsewhere — other configurations, an export not in the manifest — writing every path
**relative to the review file**, so sibling
folders are referenced rather than copied. Verify every `src` and `metrics` path resolves
before handing it over; a broken path is a silent gap.

**Declare a `configs` entry for every merged rendition, before adding its paths.** The
generator builds `configs` from the matrix, so it names only the matrix's configs and references. A
rendition keyed by an id that is not declared there makes the app refuse the **whole set**
(`… not one of the declared configs`), and quietly reusing an existing id is worse than the
error — it replaces that Hanten rendition rather than sitting beside it, so the cell you wanted
to compare against is the one you lose. Add `{"id": "nlp", "label": "NLP"}` first.

**With a build axis the generator's ids are composed**, `<config>@<build>` — so a merged
rendition sits beside `chr-generic@after`, not beside `chr-generic`, which names no cell in
that set. Key the merged entry against the composed id it belongs with, or give it an id of
its own; a bare config name there declares a cell nothing renders.

The order of `configs` sets the `1`–`9` keys, and only ten cells have one — the same ceiling
step 4 warns about, counted over the **merged** list rather than the matrix's, so a merge is
what spends the last of them.

Split large sets per roll (`review-<roll>.json`) alongside the combined one: a reviewer works
through one roll at a time.

## 7. Ask before serving

**Do not start the dev server unprompted.** Report what was built — frames, configurations,
failures, where it is — then ask whether to start it or just hand over the path:

```sh
cd tools/review-app
corepack enable pnpm   # once per machine
pnpm install           # once per checkout — `pnpm dev` does not install, and `vp` is local
pnpm dev ../../../temp/<set>/review.json
```

Serving costs memory and only one set is visible at a time, so the user may prefer the path,
a specific roll, or a different set first. If you do start it, confirm which port it bound
(another server may hold the usual one) and that it is serving *this* set.

The app **watches the set**: re-rendering updates the page in place, keeping the selected
config and scroll position. Keys: `1`–`9`/`0` select a config, `f` toggles fit/fullsize.
`pnpm` is pinned by `packageManager` — **never run `pnpm self-update`** in the repo.

## Layout

One folder per kind of output, so configurations never mix:

```
../temp/
  README.md          # index of every folder — keep it current
  <set>/             # one rendering configuration set
    review.json  review-<roll>.json
    <frame>-<config>.<jpg|tiff|avif>[.metrics.json]
    scripts/         # the matrix, the fixtures copy, and the scripts that built them
  <producer>-srgb/   # converted outside references, shared by every set
  notes/             # what was observed, frame by frame
```

Keep each set's scripts beside it: a set nobody can rebuild is a set nobody can trust six
weeks later.

## Traps

- **Never commit a review set or publish it anywhere public.** The images are personal
  photographs; a private claude.ai Artifact is the one allowed exception.
- **An SDR set's browser-displayable cell is the SDR JPEG** (`--range sdr --container jpeg`, ICC
  embedded), not a gain-map JPEG, which shows its HDR rendition. A TIFF cell does not
  display in a browser.
- **Nothing checks that a metric record describes the pixels beside it.** Re-render by hand
  and the charts go on describing the previous render; `nctool review generate` re-measures on
  checksum change, which is why it is the way to rebuild a set.
- **Explicit flags do not freeze a render.** Stating every flag pins those *values*, not
  the omitted defaults and not the algorithm inside the `--nc` binary, so the same matrix
  re-run after a pipeline change can produce different pixels. **A set records which binary made it only when it declares `builds`**: each config
  then carries a `producer` block, derived from what that binary reported rather than typed
  into the matrix, and the app shows it under the picture and in the button's tooltip. A
  matrix with no build axis still carries none, and a current build writes no `<image>.json`
  sidecar either (only the reference build does), so nothing in the set names the commit. So
  for a set meant to be trusted months later, either give it a `builds` block or write the
  commit and `hanten --version` into the set's own `scripts/` folder yourself.
- **Deleting source frames breaks later reruns, not the existing set.** Rendered JPEGs and
  their records survive; the generator simply skips the missing sources.

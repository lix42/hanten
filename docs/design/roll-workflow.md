# The roll workflow: measuring, composing and converting from the CLI

**Status:** target design, decided 2026-09-28; not shipped. Where this document and a
task file disagree, this document wins — change it first, then the task.
`docs/using-nc.md` §4 describes what ships today.

**Implemented by:** [`core/measure-base`](../tasks/core/measure-base.md),
[`nf-core/subcommands`](../tasks/nf-core/subcommands.md),
[`core/recipe-composition`](../tasks/core/recipe-composition.md),
[`core/profile-authoring`](../tasks/core/profile-authoring.md),
[`core/roll-measure-mode`](../tasks/core/roll-measure-mode.md),
[`core/auto-calibration`](../tasks/core/auto-calibration.md).

## The problem

Converting a roll today takes four commands (one optional) joined by three `jq` steps:

```sh
hanten inspect frame.tif                                         # optional
hanten estimate unexposed.tif --grid > estimate.json
jq '{recipe_version: 2, calibration}' estimate.json > base.json  # jq
hanten measure-roll frames/*.tif --leader leader.tif --params base.json > measure.json
jq -n --slurpfile b base.json --slurpfile m measure.json \
  '$b[0] + $m[0].reuse.recipe' > roll.json                       # jq
jq '.reuse.frames' measure.json > frames.json                    # jq: the clamps, when a frame is clamped
hanten roll --frames frames.json --out-dir out/ --params roll.json
```

The measuring commands print a report and leave the user to extract the recipe from
it; `--params` takes one file, so the parts must be merged by hand; and `roll` has no
override flags, so every change is a file edit.

## Principles

- **Plan → frozen recipe → deterministic apply.** Measurement resolves to explicit
  values; conversion replays them. No conversion re-measures a roll value.
- **A report is evidence, a recipe is input.** Every command's report stays on stdout
  (`--report none`, `--report-file`). A command that measures something a later run
  needs also writes it as a **recipe file**. `--params` reads only recipes, never a
  report — recipes are `deny_unknown_fields`, and reading reports would make every
  report field part of the recipe contract.
- **No `jq` between steps.** Every hand-off is a file a command wrote and another
  command reads.
- **Configuration is layered, not merged by hand.** Each measurement and each look is
  its own recipe file; commands compose them.

## Three kinds of configuration

| Part | Recipe section | Origin | Reused |
|---|---|---|---|
| **film base** | `calibration` | measured, once per roll, from the unexposed frame | this roll only |
| **roll white balance and white** | `roll` | measured, once per roll, over its picture frames | this roll only |
| **look** — scene correction, look, destination | everything else | chosen | across rolls |

A roll's measurements do not transfer: two rolls share them only if stock, development,
scanner *and* exposure all match. So the measured files exist to re-render **this**
roll, and the look file is the only one meant for other rolls.

## Commands

| Command | Does | Writes |
|---|---|---|
| `inspect` | what is this file (unchanged) | report |
| `measure-roll` | **everything a roll shares**: the film base (`--unexposed`), white balance and white | report; `--out` → `{"recipe_version": 2, "calibration": {…}, "roll": {…}}`, plus the per-frame clamps |
| **`measure-base`** (was `estimate`) | the film base alone, from one frame or a region — for a single-frame `convert`, or a roll with no unexposed frame | report; `--out` → `{"recipe_version": 2, "calibration": {…}}` |
| **`profile`** (was `params`) | author a look from flags, no image | `--out` → annotated look file |
| `roll` | convert frames from layered recipes and flags — or, with `--measure-roll` (implied by `--unexposed`), measure the roll first | images; report; opt-in recipe (`--save-recipe`) |

The workflow becomes:

```sh
hanten measure-roll frames/*.tif --unexposed unexposed.tif --leader leader.tif --out roll.json
hanten roll frames/*.tif --out-dir out/ --params roll.json --exposure 0.3
```

or, in one command:

```sh
hanten roll frames/*.tif --unexposed unexposed.tif --leader leader.tif \
    --out-dir out/ --exposure 0.3 [--save-recipe roll-recipe.json]
```

### `measure-roll`: one stop for the roll

`measure-roll` measures everything a roll shares and writes it as one recipe file. It
already takes every picture frame and the leader; `--unexposed` adds the film base, so
the roll's measurements come from one command and live in one file.

- **One base measurement, not two.** `--unexposed` runs exactly `measure-base`'s code
  and reports the same evidence (the grid cells, their spread, the warnings).
- **`--unexposed` measures the whole unexposed frame** with the reference-frame method
  that `film-base/holder-masked-measurement` and `film-base/tiling-uniformity-validator`
  settle (today the grid). A user naming another base source (a region, auto) uses
  `measure-base` and passes its file with `--params`. (Automatic calibration's cascade
  may still reach region and auto measurement — below.)
- **A measured base is not overridden.** `--film-base` over a recipe's base layers as
  usual (the flag wins); `--unexposed`, a measurement, is refused beside either. The
  unexposed file is refused among the picture frames, like the leader. A base measured
  per frame stays refused: every frame's decode depends on it.
- **The per-frame clamps** (`reuse.frames`, a frame whose white is above the cap)
  travel in the same file, so the user never passes a manifest by hand; their shape
  inside the recipe is open (see below).

### `measure-base`

`estimate` measures the film base and nothing else — everything else in its report is
evidence for that one measurement (source, grid cells, effective area), and the
general "what is this file" command is `inspect`. So it is **renamed, not split**. The
old name is a removed command: it exits 2 naming `measure-base`, never an alias.

It is no longer a step of the roll workflow. It serves a single-frame `convert` (its
report keeps `film_base_flag`) and a roll with no unexposed frame (`--base-region` on a
frame with a visible rebate, whose `--out` file then feeds `measure-roll --params`).

### Layering and precedence

`--params` is repeatable and takes `-` for stdin. Every converting command follows one
chain; later wins:

```text
defaults  <  --params A  <  --params B  <  …  <  individual flags
```

Flags win **by source, not by value** — an explicit `--white-balance 1,1,1` means
neutral gains, not "fall back to the recipe". `roll` gains every override flag
`convert` has, so a one-off change needs no file.

A frame's own override (the `--frames` manifest's `params`) resolves the same config
a single `convert` of that frame would. An override that changes a roll-wide value
warns (`--strict` refuses it), and a restatement does not; a per-frame
`roll.white_stops` is **not** roll-wide — it is how a clamp is expressed.

### `roll`'s measure mode

**`roll`'s requirements do not change.** It needs what it needs today — a film base;
the `roll` section and every style knob are optional, with today's defaults — and
refuses only when a required value has no source.

**Measure mode** is entered by `--measure-roll`: before converting, `roll` measures the
roll's white balance, white and clamps over its frames, exactly as `measure-roll` does
with the same inputs. `--unexposed U` is one more source for the base, and implies
measure mode.

| Base from | plain `roll` | `roll --measure-roll` |
|---|---|---|
| a `--params` recipe's `calibration`, or `--film-base`¹ | applies it, as today | measures the roll over that base, then converts |
| `--unexposed U` | — | measures the base and the roll, then converts |
| none | refused, as today | refused |

¹ `roll` has no `--film-base` today; it arrives with `roll`'s override flags
(`core/recipe-composition`), as do the `--roll-*` flags below.

- **Measure mode adds no measurement of its own**: its images are byte-identical to
  `measure-roll` then `roll` over the same inputs.
- **A measured value is stated once.** A layer *states* a value when it holds it
  non-null. In measure mode, a recipe stating a `roll` value or a `--roll-*` flag is
  refused (the section is being measured), and `--unexposed` beside a stated base is
  refused. A look layer (`--params my-look.json`) and style flags are fine.
- **`--leader` is optional, as in `measure-roll`**: without it the run warns that no
  frame is checked for saturation and the white-balance guard is off, and `--strict`
  refuses. Outside measure mode it guards nothing and is refused.
- The unexposed file and the leader are refused among the frames.
- `measure-roll` stays the command for measuring without converting — to read the
  numbers first, or to keep the file.

So a roll with no unexposed frame is still one command after its base:
`measure-base frame.tif --base-region … --out base.json`, then
`roll frames/*.tif --params base.json --measure-roll --leader L --out-dir out/`.

**`--save-recipe PATH` is opt-in**, and it writes the resolved run — the measured
sections, the clamps, every layer and every flag — so `roll --params PATH` over the
same frames replays it byte for byte. It is for the edit-and-rerun loop and for
re-rendering a subset of the roll with the whole roll's numbers (measuring a subset
gives different ones). Without measure mode it still collapses several layers and
flags into one file. Without the flag nothing is lost: the report carries the measured
values, and re-running over the same full frame set measures the same numbers.

### Automatic calibration

When the unexposed frame and leader are not named, an **opt-in** mode of
`measure-roll` (and so of `roll`'s measure mode) finds them among the inputs. Its
cascade may fall back to region and automatic base measurement on the picture frames
(a *named* `--unexposed` always measures its whole frame); it prefers cross-frame agreement,
records which source won and how confident it is, and drops **loudly** to single-frame conversion when nothing is
trustworthy. Content-based base estimation is never an automatic rung. Detail:
[`core/auto-calibration`](../tasks/core/auto-calibration.md).

### Authored files

Files a user edits are **JSONC** (JSON plus comments), a superset of JSON, so every
existing recipe stays valid and the machine contracts (the report) stay plain JSON.
Comments are **generated from the schema, not preserved**: a round trip discards them,
so Hanten writes an annotated file once and never rewrites a user's file in place. A
measured fragment may be plain JSON.

## Open questions

Each is owned by the task named; its answer is recorded here.

1. ~~The one-shot's name, or a mode of `roll`?~~ **Resolved 2026-09-28: a mode of
   `roll`**, entered by `--measure-roll`, which `--unexposed` implies (above).
2. **Does a `roll` flag beat a frame's manifest `params`?** The chain above has no
   place for the per-frame layer. A roll-wide flag that overrode a clamp would undo it,
   so the per-frame layer probably sits above the flags. — `core/recipe-composition`
3. **The clamps' shape in the recipe**: a per-frame table beside the `roll` section is
   a recipe schema change. — `core/measure-base`
4. **`--out` over an existing file**: refuse unless forced? — `core/measure-base`,
   `core/profile-authoring`
5. **Does `--dump-params` go?** It was to be deleted as a duplicate of the output
   sidecar, but no sidecar has been written since `nf-core/default-flip`, so it is the
   only recipe *file* a `convert` leaves (the report echoes the recipe). —
   `core/profile-authoring`
6. **Is a profile complete or partial?** Complete survives a default change; partial
   composes and reads better. — `core/profile-authoring`
7. **`--strict` across measure mode's phases**: does a measurement warning refuse
   before any frame is written? — `core/roll-measure-mode`
8. **Does `--unexposed` take a region** of a part-exposed frame, for a roll whose only
   unexposed film is half a frame? — `film-base/half-frame-calibration`
9. **`roll --frames manifest.json` in measure mode**: measuring over the manifest's frames
   is clear; how the clamps it measures combine with the manifest's own per-frame
   `params` is not. — `core/roll-measure-mode`
10. **Does a `null` in a later layer override an earlier value?** A complete look file
    (`profile`, `--dump-params`, `--save-recipe`) holds `calibration.film_base` and the
    `roll` values as `null`; layered after `roll.json`, a replacing merge would erase
    the measurement. — `core/recipe-composition`

## Order

`nf-core/subcommands` and `core/measure-base` (which also adds `measure-roll
--unexposed`) can start now. `core/recipe-composition` follows `subcommands` (it layers
on the same frame resolution); `roll`'s measure mode needs both. Automatic calibration needs
`measure-base` and the rebuilt base measurement (`film-base/holder-masked-measurement`).

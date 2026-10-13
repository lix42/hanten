# The roll workflow: measuring, composing and converting from the CLI

**Status:** decided 2026-09-28, and shipped. The measuring commands (`measure-base`,
`measure-roll --unexposed`, `--out`) shipped with `core/measure-base`, layering and
`roll`'s override flags with `core/recipe-composition`, `profile` with
`core/profile-authoring`, and `roll`'s measure mode with `core/roll-measure-mode`; open
question 8 remains. Where this document and a task
file disagree, this document wins — change it first, then the task. `docs/using-nc.md` §4
describes what ships today.

**Implemented by:** [`core/measure-base`](../tasks/core/measure-base.md),
[`nf-core/subcommands`](../tasks/nf-core/subcommands.md),
[`core/recipe-composition`](../tasks/core/recipe-composition.md),
[`core/profile-authoring`](../tasks/core/profile-authoring.md),
[`core/roll-measure-mode`](../tasks/core/roll-measure-mode.md),
[`core/auto-calibration`](../tasks/core/auto-calibration.md).

## The problem

Before `core/measure-base`, converting a roll took four commands (one optional) joined by
three `jq` steps:

```sh
hanten inspect frame.tif                                         # optional
hanten estimate unexposed.tif > estimate.json
jq '{recipe_version: 2, calibration}' estimate.json > base.json  # jq
hanten measure-roll frames/*.tif --leader leader.tif --params base.json > measure.json
jq -n --slurpfile b base.json --slurpfile m measure.json \
  '$b[0] + $m[0].reuse.recipe' > roll.json                       # jq
jq '.reuse.frames' measure.json > frames.json                    # jq: the clamps, when a frame is clamped
hanten roll --frames frames.json --out-dir out/ --params roll.json
```

The measuring commands printed a report and left the user to extract the recipe from
it — `--out` now writes it; `--params` took one file, so a look and a measurement had
to share one; and `roll` had no override flags, so every change was a file edit.

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
| **roll white balance, midtone line, white and exposure** | `roll` | measured, once per roll, over its picture frames | this roll only |
| **look** — scene correction, look, destination | everything else | chosen | across rolls |

A roll's measurements do not transfer: two rolls share them only if stock, development,
scanner *and* exposure all match. So the measured files exist to re-render **this**
roll, and the look file is the only one meant for other rolls.

## Commands

| Command | Does | Writes |
|---|---|---|
| `inspect` | what is this file (unchanged) | report |
| `measure-roll` | **everything a roll shares**: the film base (`--unexposed`, or `--base-content` with no unexposed film), white balance, midtone line, white and exposure | report; `--out` → `{"recipe_version": 3, "calibration": {…}, "roll": {…}}`, plus the per-frame clamps |
| **`measure-base`** (was `estimate`) | the film base alone, from one frame or a region — for a single-frame `convert`, or a roll with no unexposed frame | report; `--out` → `{"recipe_version": 3, "calibration": {…}}` |
| **`profile`** (was `params`) | author a look from flags, no image | annotated look file (JSONC), to stdout or `--out` |
| `roll` | convert frames from layered recipes and flags — or, with `--measure-roll` (implied by `--unexposed` and `--base-content`), measure the roll first | images; report; opt-in recipe (`--save-recipe`) |

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
  and reports the same evidence (the effective area, the spread warning, the other
  warnings).
- **`--unexposed` measures the whole unexposed frame** with the reference-frame
  method: the per-channel median over its effective area (`estimate FRAME` with no
  source flag; `film-base/tiling-uniformity-validator` may add a gradient check). A
  user stating a region uses `measure-base` and passes its file with `--params`.
  (Automatic calibration's cascade may still reach region measurement — below.)
- **A measured base is not overridden.** `--film-base` over a recipe's base layers as
  usual (the flag wins); `--unexposed`, a measurement, is refused beside either. The
  unexposed file is refused among the picture frames, like the leader. A base measured
  per frame stays refused: every frame's decode depends on it.
- **The per-frame clamps** (a frame whose white is above the cap) travel in the same
  file as `roll.frames`, so the user never passes a manifest by hand (open question 3).
- **The file renders alone**: `--out` also carries the `reconstruction` the
  measurement ran under, stated even at its default since the gains hold only under
  that decode, and the `input` and `measure` sections when they are not the defaults.
- **The report no longer carries the recipe** (`reuse.recipe`, `reuse.frames`, and
  `measure-base`'s `calibration` object): they existed to be extracted with `jq`. The
  `convert` flags (`reuse.flag`, `film_base_flag`) stay, and so do the measured values.

### `measure-base`

`estimate` measures the film base and nothing else — everything else in its report is
evidence for that one measurement (source, method, effective area), and the
general "what is this file" command is `inspect`. So it is **renamed, not split**. The
old name is a removed command: it exits 2 naming `measure-base`, never an alias.

It is no longer a step of the roll workflow. It serves a single-frame `convert` (its
report keeps `film_base_flag`) and a roll with no unexposed frame (`--base-region` on a
frame with a visible rebate, whose `--out` file then feeds `measure-roll --params`).

### Layering and precedence

`--params` is repeatable and takes `-` for stdin. Every converting command follows one
chain; later wins:

```text
defaults  <  --params A  <  --params B  <  …  <  the frame's roll.frames entry
          <  individual flags  <  a roll frame's manifest params
```

Flags win **by source, not by value** — an explicit `--white-balance 1,1,1` means
neutral gains, not "fall back to the recipe". `roll` gains every override flag
`convert` has, so a one-off change needs no file. A layer merges key by key; **a
`null` states nothing** (no layer unsets an earlier value), but any stated value wins,
a restated default included — so the measured file goes last; a tagged value that
switches variant (`region` → `explicit`) is replaced whole, and `roll.frames` merges
entry by entry. A look layer states no `calibration` or `roll`, which is what
`hanten profile` writes; a `convert --save-recipe` file is the whole run, table
included.

A frame's own override (the `--frames` manifest's `params`) lands on that frame's
recipe with its flags applied, and resolves the same config a single `convert` of
that frame would. So a manifest clamp survives `--roll-white`; a `roll.frames` clamp,
being recipe, does not. An override that changes a roll-wide value warns (`--strict` refuses
it), and a restatement of the resolved value does not; a per-frame `roll.white_stops`
is **not** roll-wide — it is how a clamp is expressed. A `null` in the override is
refused (Q10).

### `roll`'s measure mode

**`roll`'s requirements do not change.** It needs what it needs today — a film base;
the `roll` section and every style knob are optional, with today's defaults — and
refuses only when a required value has no source.

**Measure mode** is entered by `--measure-roll`: before converting, `roll` measures the
roll's white balance, white and clamps over its frames, as `measure-roll` does with
the same inputs. `--unexposed U` is one more source for the base, and implies
measure mode.

| Base from | plain `roll` | `roll --measure-roll` |
|---|---|---|
| a `--params` recipe's `calibration`, or `--film-base`¹ | applies it, as today | measures the roll over that base, then converts |
| `--unexposed U` | — | measures the base and the roll, then converts |
| none | refused, as today | refused |

¹ `--film-base`, like the `--roll-*` flags below, is one of `roll`'s override flags
(`core/recipe-composition`).

- **Measure mode adds no measurement of its own**: it measures under the decode it
  renders, so the decode, input and measure flags apply to the measurement too. Its
  images are byte-identical to `measure-roll` then `roll` over the same frames, layers
  and style flags, with `measure-roll` measuring under the same decode (`--measure-inset`
  passed to it, the decode and input flags stated in a layer it reads). The measured
  recipe is layered last, as `--params roll.json` would be, and the flags apply over it.
- **A measured value is stated once.** A layer *states* a value when it holds it
  non-null. In measure mode, a recipe stating a measured `roll` value (`roll.frames`
  included) or a `--roll-*` flag is refused (the section is being measured), and
  `--unexposed` beside a stated base is refused. A look layer (`--params my-look.json`)
  and style flags are fine.
- **The roll's switches keep their meaning.** `--small-lift`, `--thin-lift`,
  `--placement`, `--hybrid-span`, `--hybrid-level-keep`, `--midtone-neutral` and
  `--neutral-balance` (and their `roll` keys) decide at render how a measured value applies, in measure mode as outside it. The measurement
  itself runs at `measure-roll`'s defaults; to measure differently (`measure-roll
  --midtone-neutral off`, `--no-small-lift`), run `measure-roll` first.
- **The base it measures over is explicit**: a stated region is refused, as on
  `measure-roll`, since it would measure each frame under a different decode.
- **`--leader` is optional, as in `measure-roll`**: without it the run warns that no
  frame is checked for saturation and the white-balance guard is off, and `--strict`
  refuses. Outside measure mode it guards nothing and is refused. `--unexposed auto`
  and `--leader auto` work as on `measure-roll`, and a frame they find is not converted.
- The unexposed file and the leader are refused among the frames.
- **The report** is `roll`'s, with a `measurement` object holding what `measure-roll`'s
  report would.
- **A measurement failure writes no image** and exits with the measurement's code.
  Under `--strict`, a measurement warning refuses once the report is emitted, before
  any frame is converted (question 7).
- **`--frames` is refused in measure mode** (question 9).
- `measure-roll` stays the command for measuring without converting — to read the
  numbers first, or to keep the file.

So a roll with no unexposed frame is still one command after its base:
`measure-base frame.tif --base-region … --out base.json`, then
`roll frames/*.tif --params base.json --measure-roll --leader L --out-dir out/`. With no
clear film at all, `roll frames/*.tif --base-content --leader L --out-dir out/` estimates
the base from the pictures, as `measure-roll --base-content` does.

**`--save-recipe PATH` is opt-in**, and it writes the resolved run — the measured
sections, the clamps, every layer and every flag — so `roll --params PATH` over the
same frames replays it byte for byte. It is for the edit-and-rerun loop and for
re-rendering a subset of the roll with the whole roll's numbers (measuring a subset
gives different ones). Without measure mode it still collapses several layers and
flags into one file. Without the flag nothing is lost: the report carries the measured
values, and re-running over the same full frame set measures the same numbers.

### Automatic calibration

**Shipped 2026-10-09** as `measure-roll --unexposed auto` and `--leader auto` (each
separately; `roll`'s measure mode takes them too). Every input's effective area is
measured first. The unexposed frame is the clearest flat frame, provided no input is
clearer than it, and other flat frames within 0.01 density of it corroborate it. A denser flat frame is a near-blank picture.
The leader is flat and at least 0.7 density above the roll's base. Found frames leave
the picture pool, so the recipe matches the one that naming them writes. The report's
`references` object records each input's class and evidence, and the base's confidence.

There is no automatic region rung, since nothing searches for a rebate, and no drop to
single-frame conversion, since no per-frame base exists. **Nothing trustworthy is a
refusal** (exit 2) naming `--unexposed FILE` and `measure-base --base-region`. A base
from one frame is used with a warning, which `--strict` refuses. Content-based base
estimation (`--base-content`, which the refusal names last) is never an automatic rung.
Evidence and constants:
`pipeline::reference_frames`.

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
2. ~~Does a `roll` flag beat a frame's manifest `params`?~~ **Resolved 2026-09-28: no —
   the frame's `params` sit above the flags** (above), so a roll-wide flag cannot undo
   a clamp stated there. A clamp in `roll.frames` is recipe, and a flag beats it (3).
3. ~~The clamps' shape in the recipe~~ **Resolved 2026-09-28: `roll.frames`**, a table
   in the `roll` section from a frame's **file name** to its own
   `{"white_stops": …}`, so the recipe survives the scans moving. `convert` and every
   `roll` frame apply their input's entry before any flag; a `--frames` manifest's
   `params` beat it (and may not state the table). A typed `--roll-white` or a manifest's
   white drops a thin frame's slope and exposure with it. `measure-roll --out` refuses two
   inputs sharing a file name.
4. ~~`--out` over an existing file~~ **Resolved 2026-09-28: refused (exit 2, before
   anything is decoded) unless `--force`.** `core/profile-authoring` follows the same
   rule.
5. ~~Does `--dump-params` go?~~ **Resolved 2026-10-09: renamed `convert
   --save-recipe`**, the name `roll` takes in measure mode. It is the only recipe file a
   `convert` leaves, and a profile cannot stand in for it: a profile states no
   `calibration` or `roll`, which a replay needs.
6. ~~Is a profile complete or partial?~~ **Resolved 2026-10-09: complete** — every key
   but `calibration`, `roll` and `scene_correction` (a roll's adjustment: as a recipe
   value it warns beside every measured roll), a key the rendering decides written
   `null`, and stamped like every written recipe, so a `pipeline_version` change warns.
   It restates the decode, so it is layered before the measured file.
7. ~~`--strict` across measure mode's phases~~ **Resolved 2026-10-09: yes** — under
   `--strict` a measurement warning refuses (exit 1) once the report, with its
   `measurement`, is emitted, and before any frame is converted, as `measure-roll
   --strict` refuses before writing its recipe.
8. **Does `--unexposed` take a region** of a part-exposed frame, for a roll whose only
   unexposed film is half a frame? — `film-base/half-frame-calibration`
9. ~~`roll --frames manifest.json` in measure mode~~ **Resolved 2026-10-09: refused**
   (exit 2), naming the two-command route. A manifest's `params` can change the decode
   a frame would be measured under, which needs its own design; measure the roll with
   `measure-roll --out`, then `roll --frames … --params` its file.
10. ~~Does a `null` in a later layer override an earlier value?~~ **Resolved
    2026-09-28: no — a `null` states nothing** in a `--params` layer (above), and a
    frame's `params` refuses one: hand-written there, it is an attempt to unset,
    which the merge cannot do. `hanten params` holds `calibration.film_base` and the
    `roll` values as `null`, and a replacing merge would have erased the measurement
    layered before it. Every value it states still wins, so the measured file goes
    last.

## Order

`nf-core/subcommands` and `core/measure-base` (which also adds `measure-roll
--unexposed`) can start now. `core/recipe-composition` follows `subcommands` (it layers
on the same frame resolution); `roll`'s measure mode needs both. Automatic calibration needs
`measure-base` and the rebuilt base measurement (`film-base/holder-masked-measurement`).

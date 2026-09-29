# Using Hanten

A practical guide to converting film negative scans to positives with `hanten`.

> **Scope.** This is the *user-facing* guide: what to run, in what order, and why.
> For the authoritative design rationale and the full parameter semantics, see
> [`design-spec.md`](design-spec.md). Where the two disagree, the spec wins on
> *intent* — but this document is verified against the binary, so it wins on
> *what the CLI currently accepts*.
>
> **Verified against:** `hanten 0.1.0`, `pipeline_version 8`, on branch
> `core/measure-base` (`estimate` renamed `measure-base`; the measuring commands and
> their `--out` recipes, §4 and §7), after `film-base/holder-masked-measurement` (the
> base measured over the effective area; the `auto` film base retired, §4),
> `telemetry/schema-v2` (telemetry success/failure
> events, §10), `nf-core/report-contract` (the report's `chain` block and recipe, §10),
> `nf-core/default-flip` (the rendering chain of [`design-update.md`](design-update.md)
> became the only one), `nf-calibration/roll-section` and
> `nf-destinations/direct-preset` (§7). The staleness signal is `pipeline_version`: if
> `hanten --version` reports a different one, treat this document as suspect and
> re-verify.
>
> **Before `pipeline_version` 8** there was a second rendering chain, chosen by
> **output presets** (`--output-preset`: `gain-map-hdr`, `display-p3`, `hdr-pq`, …)
> with print controls (`--print-exposure`, `--black-point`, `--auto-wb`,
> `--linear-range`), and the chain described here sat behind `--new-flow`. Those flags,
> the recipes and sidecars that chain wrote, and everything retired before it (the
> sigmoid and `characteristic` curves, the regional balance, the `Dmax` reference,
> `--display-tone`) are documented in the reference build's own guide
> (`git show origin/reserve:docs/using-nc.md`; `scripts/reference-snapshot/README.md`
> builds that binary). Here each is refused with a message naming the replacement.

---

## 1. The mental model

Hanten converts a **negative** scan into a **positive** image. Three properties
shape every workflow below:

- **Deterministic.** Same input + same parameters ⇒ byte-identical output (on one
  build and architecture). There is no hidden per-frame adaptation.
- **Every knob is a CLI flag *and* a recipe key**, and nothing is reachable only
  from code. Passing a flag that doesn't apply to your destination is a **loud
  error**, never a no-op.
- **Calibrate once, apply many.** The film base (`Dmin`) is a property of the
  *roll* — film stock, development, scanner — not of an individual frame. You
  measure it once and reuse it, which is what keeps a whole roll color-consistent;
  the roll's white balance and white are measured once the same way
  (`hanten measure-roll`, §7). **`Dmin` has no default: every `convert` must say
  where the film base comes from**, because it sets the black point and the colour
  balance together.

That last point is the whole workflow:

```
   measure                        freeze                 apply
┌────────────────────┐        ┌───────────────┐        ┌──────────────────────┐
│ hanten inspect     │  ────► │ roll.json     │ ─────► │ hanten roll          │
│ hanten measure-roll│        │ (Dmin, white  │        │ hanten convert       │
│ hanten measure-base│        │  balance,     │        │                      │
│   --out roll.json  │        │  clamps)      │        │                      │
└────────────────────┘        └───────────────┘        └──────────────────────┘
  measure from the              written by --out        one shared recipe
  roll's frames                 (no jq, no copying)     across every frame
```

Every conversion runs one chain: the **fixed decode** turns the negative into film
density and then linear scene values (§6); four **rendering stages** — scene
correction, the look, fit range and fit gamut (§7) — make a picture of it, starting
from one of two **renderings** (`default`, which applies the roll's measurements, or
`direct`, which applies as little as it can); and the **destination** (§8) says what
file it becomes. `--film-master` skips the stages and writes the decode itself.

### Value terms (read this once)

A pixel lives in several **per-channel** domains, and they don't all run the same
direction. As scene luminance rises:

```
transmission ↓     density ↑     positive ↑     output ↑
```

"Bright" and "dark" in this document always describe the **scene**, never a raw
pixel value. The film base is the *highest* transmission on the negative yet
renders to *black* in the positive.

- **`Dmin`** — a per-channel **transmission**: the unexposed film base. This is
  the `--film-base R,G,B` value.
- **The anchor** — a **scalar** in **density** units: the corrected density that
  renders to scene white. It is placed from the film base: mid-grey sits
  `--anchor-mid-offset` (default 0.62) density above it (§6).

They are not two ends of one scale; don't conflate them.

---

## 2. Getting a binary

```sh
cargo build --release      # → target/release/hanten
```

A fresh machine needs CMake, C and C++ compilers, and NASM — the build compiles
libaom (the AVIF encoder) from the `libaom-sys` crate's vendored source. Cargo still
fetches the Rust crates from crates.io, so the build needs network access (or a warm
cargo cache).

```sh
target/release/hanten --version
```

prints the version, the **`pipeline_version`** (the render-behavior identity), the
git commit, and the target triple. Quote it in bug reports — output is only
guaranteed byte-identical within one build and architecture.

> `cargo build` neither installs the binary nor changes `PATH`. Examples below
> write `hanten` for brevity; run `target/release/hanten`, or put it on your
> `PATH`. (The binary was called `nc` until 2026-09-21, which collided with
> netcat; `hanten` does not.)

---

## 3. The six commands

| Command | Purpose | Writes an image? |
|---|---|---|
| `hanten inspect` | **"What is this file?"** — format, dimensions, IR presence, scanner metadata, resolved input semantics, the effective area. | No |
| `hanten measure-base` | **"What is this film's base?"** — measure the film base (`Dmin`) alone, from an unexposed frame or a region. Prints a reuse-ready `--film-base` flag; `--out` writes it as a recipe. (Was `estimate`, which now exits 2 naming it.) | No |
| `hanten params` | Print the full default recipe as JSON — the scaffolding starting point. | No |
| `hanten convert` | Convert one frame. The full parameter surface. | Yes |
| `hanten roll` | Convert many frames from **one shared frozen recipe**. | Yes |
| `hanten measure-roll` | **"What does this roll share?"** — its white balance and white, measured once over its frames (§7), and with `--unexposed` its film base. `--out` writes it all as one recipe for `roll`. | No |

Every command except `params` emits a **JSON report on stdout** on success
(`--report none` to suppress, `--report-file PATH` to redirect); `params` takes no
flags at all and just prints the default recipe. Logs and warnings go to
**stderr**, so stdout stays clean for piping into `jq`.

> **A hard failure emits no report at all** — stdout is empty. A decode error, a
> memory refusal, or a measurement that fails (`measure-base` on a frame with no usable
> signal) exits non-zero before the report is written. Only `roll` is different: it
> aggregates per-frame failures into its report and still emits it. So a script
> must check the exit code, not just parse stdout.

---

## 4. The core workflow

### Step 1 — Inspect the scan

```sh
hanten inspect scan.tif | jq '{decode, input_color, warnings}'
```

Tells you what you actually have: dimensions, bit depth, whether an **IR plane** is
present (HDRi 64-bit input), the scanner make/model/software, the SilverFast XMP
mode metadata, and — importantly — how `hanten` **resolved the input semantics**
(`transfer` and `meaning`) with the evidence behind each.

`inspect` measures no film base. It does report the **effective area** — the region
`hanten` reads measurements over, after the film holder and a border inset are
removed — which is what `measure-base` measures in step 2. Worth a look on any uncropped
scan; see [The measurement region](#the-measurement-region-the-effective-area).

### Step 2 — Measure the roll, once

**This step is mandatory.** `convert` and `roll` refuse to run without a stated
film base — there is no default, because `Dmin` is the divisor of the density
conversion and sets the black point and the colour balance together:

```
usage: no film base selected: pass --film-base R,G,B (a Dmin measured once per roll
       with `hanten measure-base <unexposed-frame>`), or --base-region X,Y,W,H to read
       it from a region of unexposed film. Recipe key: `calibration.film_base`.
```

One command measures everything a roll shares — the film base from its **unexposed
frame**, and its white balance and white over its picture frames (§7) — and writes it
as one recipe:

```sh
hanten measure-roll frames/*.tif --unexposed unexposed.tif --leader leader.tif --out roll.json
```

`roll.json` is the measurement, ready for `--params` (a real Ektar 100 roll):

```json
{
  "recipe_version": 2,
  "calibration": { "film_base": { "explicit": [0.485832, 0.2621805, 0.17726406] } },
  "roll": {
    "white_balance": [0.886392, 1.0, 1.188019],
    "white_stops": 1.9509047,
    "frames": { "971.tif": { "white_stops": 2.0 } }
  }
}
```

`roll.frames` holds the frames whose white is above the roll's cap, keyed by file name
(§7). If your `--params` recipe stated `input`, `measure` or `reconstruction` keys, the
file carries them too, because the gains hold only under the decode they were measured
with — all but `input.export_ir`, one frame's output path, which `roll` refuses.

- `--unexposed` measures the frame exactly as `measure-base` does with no source flag
  (below), and reports that evidence under the report's `unexposed` object.
- A measured base is not overridden: `--unexposed` beside `--film-base`, or beside a
  `--params` recipe stating `calibration.film_base`, exits 2. So does the unexposed
  file named among the frames or as the leader.
- `--out` refuses a file that exists (exit 2, before anything is decoded) unless you
  pass `--force`. A run that fails — including a `--strict` run with a warning —
  writes no file.

The report on stdout is the evidence (the base measurement, each frame's white, the
warnings); it does not carry the recipe.

#### The film base alone: `measure-base`

`measure-base` measures the film base and nothing else, and `--out` writes it as a
recipe (`{"recipe_version": 2, "calibration": {…}}`). Feed that to `measure-roll
--params` and the roll is measured over that base — on the roll above,
`measure-base base.tif --out base.json` followed by `measure-roll … --params
base.json --out roll.json` writes a `roll.json` byte-identical to the one-command form.
It is also the command for a single `convert`: its report prints a paste-ready flag,
`"film_base_flag": "--film-base 0.485832,0.2621805,0.17726406"`.

It measures from one of two sources:

**(a) An unexposed frame** — the intended source. With no source flag, `measure-base`
takes the per-channel **median** over the frame's effective area (§9):

```sh
hanten measure-base unexposed.tif --out base.json
```

Over unexposed film the area is one population — the base plus grain and scanner
noise — so the median is the base, where a high percentile would land in the noise
tail and understate every density. The report says `"film_base_source":
"effective_area"` and `"film_base_percentile": 0.5`. A frame that is not unexposed
film warns (`--strict` fails on it):

```
hanten: warning: the effective area is not uniform (worst per-channel spread
(p90 - p10) / p50 = 1.65 > 0.50): it does not look like unexposed film, so the
median over it is not a film base. …
```

It is a coarse guard — it catches a picture frame, not a leader — so point it at the
right frame. A scan whose holder was not measured also warns — every 48-bit scan
(no IR plane), and an HDRi scan whose IR plane could not do it: the area is then the
inset alone, so raise `--measure-inset` if the holder reaches past it.

**(b) A region of unexposed film** on another frame — the fallback for a roll with
no unexposed frame:

```sh
hanten measure-base scan.tif --base-region 0,0,24,24 --out base.json
```

A drawn rectangle may mix holder, film and picture, so the region is read at its
**97th percentile** (`"film_base_percentile": 0.97`), and `hanten` warns if the
rectangle is not uniform.

`--auto-base` and `--grid` (and a recipe's `"film_base": "auto"`) were removed with
the rebate search and exit 2; `measure-base` with no flag replaces both.

Add `--strict` when scripting: it turns "plausible-looking but bad" into a hard
failure instead of a value your pipeline silently bakes in.

> **Renamed:** `hanten estimate` is now `measure-base`, with the same flags; the old
> name exits 2 naming it. Its report's `calibration` object is gone — `--out` writes
> the recipe instead.
>
> **No `Dmax` step.** Earlier builds measured a roll reference density off the
> light-struck leader (`--d-max-region`); it retired with the placements that read it,
> and the flag now exits 2. The anchor is placed from the film base (§6). The leader
> still matters: pass it to `measure-roll` (§7).

### Step 3 — Convert the roll

```sh
hanten roll frames/*.tif --out-dir positives/ --params roll.json
```

Every frame gets the identical film base, white balance and rendering, so the roll is
color-consistent; a frame in `roll.frames` renders at its own white. Outputs are named
`<input-stem>_positive.<ext>`, the suffix coming from the destination's container
(`.tiff` by default); a roll-level JSON report lands on stdout. A single frame takes
the same file: `hanten convert frame.tif -o out --params roll.json` renders
byte-identically to that frame of the roll, its `roll.frames` entry included.

**Your look goes in the same file, for now.** `roll` takes one `--params` and has no
override flags, so add your choices beside the measurement:

```jsonc
{
  "recipe_version": 2,
  "calibration": { "film_base": { "explicit": [0.485832, 0.2621805, 0.17726406] } },
  "roll": { "white_balance": [0.886392, 1.0, 1.188019], "white_stops": 1.9509047,
            "frames": { "971.tif": { "white_stops": 2.0 } } },
  "scene_correction": { "exposure": 0.3 }
}
```

**The two halves have different lifetimes, and the schema keeps them apart.**
`calibration` and `roll` are what you measured off *this* roll; everything else is the
look, which you reuse across rolls. So a roll calibration is a recipe with nothing but
those two, and a look is a recipe with neither — the second needs a base from a flag,
since `calibration.film_base` has no default.

Omitted sections take their defaults, so a recipe only needs `"recipe_version": 2`
and what you decided. `hanten params` prints the full default document if you want a
scaffold to edit.

> **`--dump-params` writes what you stated**, and it replays byte-identically. A knob
> you left unstated stays `null`, so the rendering still decides it on replay. A
> measured value is frozen only if you stated it: a run with `--base-region` dumps
> the region, not the base it read, so a recipe dumped from it **re-reads the base on
> every frame of the roll** — exactly what `roll` exists to prevent (`roll` warns
> "roll film base is NOT frozen"). Use the file `--out` writes.

---

## 5. Recipes

### Shape

A recipe is one JSON document, versioned as a document, with one section per stage.
Get the current default with:

```sh
hanten params
```

```json
{
  "recipe_version": 2,
  "input":       { "transfer": "auto", "meaning": "auto",
                   "film_type": "unknown", "export_ir": null },
  "calibration": { "film_base": null },
  "roll":        { "white_balance": null, "white_stops": null, "frames": {} },
  "measure":     { "inset": 0.05 },
  "reconstruction": {
    "scale": [1.0, 0.84, 0.73],
    "offset": [0.0, 0.0, 0.0],
    "linearization": 1.8,
    "anchor": { "mid-at-base-offset": 0.62 }
  },
  "rendering": "default",
  "scene_correction": {
    "white_balance": { "explicit": [1.0, 1.0, 1.0] },
    "exposure": 0.0
  },
  "look": {
    "contrast": null,
    "channel_grade": [1.0, 1.0],
    "highlight_desaturation": { "strength": null, "start_stops": null, "band": null }
  },
  "fit_range": { "headroom_stops": null, "display_black": null },
  "fit_gamut": {},
  "output": { "display": {} }
}
```

(Reflowed; the real output is one value per line.) `calibration.film_base` prints as
`null` because it has **no default** — this document is a template to edit, not a
runnable recipe. `convert` and `roll` reject an unstated base.

`input`, `calibration` and `measure` describe the scan (§4, §9), and `roll` holds
what `hanten measure-roll` measured — nothing by default (§7). `reconstruction` is the
fixed decode (§6). `rendering` chooses what the stages start from (§7);
`scene_correction`, `look` and `fit_range` are the rendering stages, and a `null` knob
there is unstated and takes the rendering's value — the look's contrast is the roll's,
else 2.0/1.8, and highlight desaturation, headroom and display black are 0.8, 6 and 6
under `default`. `fit_gamut` is empty for good — its ceiling comes from fit range and
its gamut from the destination. `output` is the destination (§8), with nothing stated
by default: every axis is derived.

### Partial recipes are fine

Omit any section and the defaults fill the gap. This minimal recipe produces a
**byte-identical** result to `--film-base 0.163,0.080,0.0377`:

```json
{
  "recipe_version": 2,
  "calibration": { "film_base": { "explicit": [0.163, 0.080, 0.0377] } }
}
```

### Strictness

Every section uses `deny_unknown_fields`, so a typo is rejected rather than
ignored:

```
usage: invalid recipe t.json: unknown field `exposur`, expected `white_balance` or
       `exposure` at line 1 column 49
```

This means a **misplaced** key fails too — a key must live under the stage section
that owns it (`--export-ir` ⇒ `input.export_ir`, not top level).

### Recipes from earlier builds

**A recipe without `"recipe_version": 2` is refused whole.** Every sidecar and
`--dump-params` file written before `pipeline_version` 8 describes the rendering chain
that version removed, and there is no converter — replaying one here would render a
different picture while claiming to be the same recipe:

```
usage: recipe old.json: a recipe must state `"recipe_version": 2`. A document without
       it — every sidecar and `--dump-params` file written before `pipeline_version` 8
       — describes the rendering chain that version removed, and there is no
       converter. `hanten params` writes the current layout: `input`, `measure` and
       a `region` or `explicit` `calibration.film_base` carry over unchanged (an
       `"auto"` one retired: measure the base with `hanten measure-base
       <unexposed-frame>`), and the rest is a stage section each (`reconstruction`,
       `scene_correction`, `look`, `fit_range`) plus `output`, the destination
```

Carry the film base across by hand; render the old recipe itself with the reference
build. Any other `recipe_version` is refused too (`this build reads only 2`).

A versioned recipe that still carries one of the removed chain's sections or keys is
refused naming it, and where its knobs went, rather than parsed and read by nothing:

```
usage: recipe print.json: `print` is a section of the removed chain's recipe: white
       balance and exposure are `scene_correction.white_balance` and
       `scene_correction.exposure`; the display tone is fit range, whose one operator
       is reinhard and whose headroom is `fit_range.headroom_stops`; the black point's
       surviving half is display black, `fit_range.display_black` (where the film
       base renders); and `linear_range` has no home yet
       (`nf-scene-correction/levels-knob`). Drop it
```

The same holds for `reconstruction.density` (its `scale` and `offset` are
`reconstruction.scale` and `.offset`), `reconstruction.contrast` (split in two — §6),
`output.preset` (a destination is its axes — §8), `calibration.dmax` (the reference
density retired), and a per-frame `white_balance` mode such as `"percentile"` (§7).

### Precedence

**Flags always win over the recipe.** Precedence is by *source*, not value — an
explicit `--white-balance 1,1,1` over a recipe's gains means neutral:
`defaults < --params recipe < flags`.

```sh
hanten convert scan.tif -o out --params roll-recipe.json --exposure 0.5
#                                                        ^ overrides the recipe
```

### No sidecar is written

Earlier builds wrote a `<output>.json` sidecar beside every image; this one does not.
A `convert` report carries the resolved recipe (§10); a `roll` report does not, and
records each frame's `overrides` and `identity.params_hash` instead. To keep a recipe
file, write it with `--dump-params`, which replays byte-identically:

```sh
hanten convert scan.tif -o out --film-base … --contrast 1.3 --dump-params out.json
hanten convert scan.tif -o repro --params out.json
# → repro.tiff is byte-identical to out.tiff
```

A sidecar an earlier build left at the same path is **removed** when a run replaces
its image — it describes the picture just overwritten — and the report names it in
`chain.removed_sidecar`. A file there that is not one of Hanten's sidecars is left
alone, and so is one this run read as its `--params` recipe (with a warning, since it
still pairs by name with an image it no longer describes).

`--params` still accepts the `{"meta": …, "params": …}` envelope those sidecars used,
reading `params` as the recipe (which must itself be version 2). `meta` changes no
pixel, but its `pipeline_version` is checked: a malformed one exits 2, and one that
differs from the running build raises a `--strict`-promotable warning.

### Per-frame overrides in a roll

Use a `--frames` manifest instead of positional inputs when individual frames need
a tweak on top of the shared recipe:

```json
{ "frames": [
    { "input": "a.tif" },
    { "input": "b.tif", "output": "b-brighter",
      "params": { "scene_correction": { "exposure": 1.0 } } }
] }
```

```sh
hanten roll --frames frames.json --out-dir positives/ --params roll-recipe.json
```

An override is merged onto the shared recipe section by section, so it need not state
`recipe_version` (`{"roll": {"white_stops": 2}}`, `{"look": {"contrast": 1.3}}`); a
removed chain's key in it is refused the same way, naming its frame. An explicit manifest `output` goes through the same suffix rule as `convert`
(§8): an extension it states must match the frame's destination, and one it omits is
completed, so `"output": "b-brighter"` writes `b-brighter.tiff` on a default roll.

A frame renders exactly as `convert --params` would with the shared recipe and its
override merged. Some keys describe the *roll*, not the frame:
`calibration.film_base`, `roll.white_balance`, `reconstruction` (every key),
`rendering` and `output`. An override that changes one is applied but warns, naming
both values (and `--strict` turns the warning into a failing exit), because the frame
then renders apart from its siblings — a roll is one piece of film through one
process. What counts is what the frame renders: restating the roll's value does not
warn, nor do gains the frame never applies (under `direct` or the film master), and a
`rendering` change names the destination it derives.
`roll.white_stops` is per frame: it is how `measure-roll` gives a clamped frame its
own white. With `"reconstruction": {"linearization": 1.7}` added to `b.tif`'s
`params`:

```console
$ hanten roll --frames frames.json --out-dir positives/ --params roll-recipe.json --report none
hanten: warning: frame b.tif: its `params` override resolves `reconstruction.linearization` to 1.7, where the roll's is 1.8 — this frame's densities decode differently from the rest of the roll's. Drop what changes it from this frame's `params` to keep the roll consistent.
```

A per-frame `output.display` joins the shared recipe's axes, axis by axis.

---

## 6. The fixed decode

The decode turns the negative into linear scene values: density-domain inversion
(Cineon / negadoctor lineage) through one straight line in density, stock-agnostic and
the same for every frame. Its knobs are calibration, not taste — the look (§7) is
where a picture is made contrasty or warm.

| Flag | Recipe (`reconstruction.`) | |
|---|---|---|
| `--density-scale R,G,B` | `scale` | per-channel density gain — **default `1,0.84,0.73`**, see below |
| `--density-offset R,G,B` | `offset` | per-channel density offset (orange-mask compensation), default `0,0,0` |
| `--density-gamma G` | `linearization` | the film's linearization, default `1.8` |
| `--anchor-mid-offset D` | `anchor` = `{"mid-at-base-offset": D}` | where mid-grey sits above the base, default `0.62` |

The report states what the decode ran, in `chain.decode` (`anchor`,
`anchor_rule`, `linearization`, `scale`, `offset`).

### The decode's slope and the picture's contrast are two knobs

`--density-gamma` is the film's **linearization** — undoing the negative's ≈0.55
density per decade — and `--contrast` (§7) is **print contrast**, a look. With no roll
white and no stated contrast, `1.8 × 2.0/1.8` renders a neutral where the single slope
of 2.0 that earlier builds bundled the two into did. To change how contrasty a picture is, change
`--contrast`; `--density-gamma` is a calibration and moves with `--density-scale`,
never alone. A recipe stating the pre-split `reconstruction.contrast` is refused with
the value that keeps it:

```
usage: recipe old.json: `reconstruction.contrast` split in two: the decode's slope is
       now `reconstruction.linearization`, the film's linearization, and how contrasty
       the picture is is the look's `look.contrast`. Drop the key; to keep a stated 2
       as the whole contrast, write `look.contrast`: 1.1111112 and leave
       `reconstruction.linearization` at its default 1.8
```

### Anchoring — where the line pins a tone

The decode pins **mid-grey** a stated density `D` above the film base and lets white
fall where the slope puts it. Pinning mid rather than white is the substantive outcome
of [`reports/sigmoid-reference-baseline.md`](reports/sigmoid-reference-baseline.md):
raising contrast pivots the line *about the pinned point*, so pinning white
necessarily drags everything below it down. A larger `D` renders the roll **darker**;
a smaller one renders **brighter**.

**The rule reads the film base and nothing else.** Earlier builds could place the
anchor against a roll *reference* density measured from a light-struck leader; that
reference and the placements built on it retired. Their flags are refused, and the
remedy is to drop them:

```
usage: --anchor-mid-fraction was removed: it pinned mid-grey at a fraction of the
       reference density. The roll reference density and the placements that read it
       are gone. Drop the flag: the anchor is placed from the film base, mid-grey
       `--anchor-mid-offset D` density above it (default 0.62, slope
       `--density-gamma`).
```

(`--d-max`, `--fixed-d-max`, `--auto-d-max`, `--no-d-max`, `--anchor-white-at-reference`,
`--anchor-black-floor` and `measure-base --d-max-region` say the same.)

> **Provisional values.** `D = 0.62` is the generic C-41 profile's mid-grey aim above
> the base, rounded and frozen. The anchor stays referenced to the base: a roll's own
> white acts through the look's contrast, from the `roll.white_stops` that
> `measure-roll` measures (§7), not by moving `D`. The per-channel density gain beside it has moved twice
> (`pipeline_version` 4 and 5 — see below). Expect further movement, with a
> `pipeline_version` bump when it happens.

### The density gain

**`--density-scale` does not default to `1,1,1`.** It is `1,0.84,0.73` — a
calibration, not an identity. Green and blue density rise faster than red in a scan,
so with no gain they drift against it across the tone scale, which shows up as a
tone-dependent cast rather than an overall one.

The values come from **31 hand-marked neutral patches** across five rolls: per patch,
the gain that renders it neutral; per roll, the median; the default is the mean of the
five roll medians (green `0.837`, blue `0.733`). Rolls are weighted equally on purpose
— two thirds of the patches come from one scan date, and weighting by patch would let
that date set the default on its own.

**It balances five rolls; it does not fit yours.** Blue is the steady half — every
roll measured wants 0.68–0.78. Green is not: it splits by **scan date** (one group
0.86–0.90, another ~0.77, which tracks a change of developer rather than of film), so
the shipped `0.84` is a compromise that fits neither group exactly and can push a roll
from the higher group slightly green-yellow. The value is also calibrated on one
scanner. A roll that still shows a cast wants its own `--density-scale`;
`io/scanner-density-calibration` is the task that should remove the need to guess.

### Retired curve and balance controls

There is one decode, so nothing selects a curve. `--reconstruction`, `--density-curve`
(at any value, `exponential` included), `--film-stock`, `--preset` and the
`--sigmoid-*` flags are refused, each saying what to do instead:

```
usage: --preset was removed: its bundles (`characteristic-generic`, `-stock`, `-aim`,
       and earlier `sigmoid-knees` / `-flat`) set retired curves with an exposure
       calibrated to them, and no bundle replaces them. Drop the flag, and set a knob
       you want directly — `--exposure`, `--contrast`, `--channel-grade`,
       `--highlight-desaturation` — or collect them in a `--params` recipe. The old
       render is reproducible only from the reference build (`scripts/reference-snapshot/`).
```

**A named look is a recipe file.** Put the knobs you want in a partial recipe with no
`calibration` and pass it with `--params` (§5); `roll` takes it the same way.

The regional balance (`--shadow-balance`, `--highlight-balance`, `--balance-range`,
`--auto-balance-range`) is refused at every value, `0,0,0` included, naming its
successor — the look's per-channel grade (§7), which is not a rename: it acts on the
working space's channels after the 3×3, not on film density, so old balance values do
not translate.

### Nothing is silently ignored

A retired flag is a **usage error** naming what to do instead, never accepted and
dropped: a flag that quietly did nothing would be worse than a failure.

---

## 7. The rendering stages

Four stages make a picture of the decoded scene, in a fixed order: **scene
correction** → **the look** → **fit range** → **fit gamut**. Each is a section of the
recipe, each is always in the chain (its defaults are either the shipped look or the
identity), and the report lists what each applied in `chain.stages`:

```console
$ hanten convert scan.tif -o out --film-base 0.9,0.55,0.42 | jq -c '[.chain.stages[].applied]'
["identity","contrast+highlight-desaturation","reinhard-peak-lifted-v1+log-shift-to-mid-grey-v1","acescg-to-display-p3-matrix+neutral-axis-radial-boundary-v2"]
```

Scene correction reports `"identity"`, `"white-balance"`, `"exposure"` or
`"white-balance+exposure"`; the look the controls that ran joined by `+`, or
`"identity"`; fit range its operator and display black's curve; fit gamut the change of
primaries and the gamut map, named for the destination's gamut. `--film-master` runs
none of them (§8).

The removed chain's print controls are refused, each saying where the knob went:

| Refused | Where it went |
|---|---|
| `--print-exposure` | `--exposure`, scene correction (below) |
| `--black-point` | its surviving half is display black, `--display-black` (below); a flare/fog subtraction in scene correction has not landed (`nf-scene-correction/flare-removal`) |
| `--auto-wb` | none per frame: `hanten measure-roll` measures the roll's white balance once (below) |
| `--linear-range` | an affine levels remap with no home yet (`nf-scene-correction/levels-knob`); refused even at its old default `0,1` |
| `--display-tone`, `--highlight-compress` | fit range's one operator; its parameter is `--display-tone-headroom` |

### The rendering — `default` or `direct`

`--rendering default|direct` (recipe `rendering`) chooses what the stages start from:

| | `default` | `direct` |
|---|---|---|
| the roll's measurements (`roll`) | applied | left out, and reported so (`chain.roll.*_applied: false`) |
| white balance / look contrast | the roll's; without them neutral / 2.0/1.8, and a warning | neutral / 2.0/1.8 |
| highlight desaturation | 0.8 | off |
| display black / headroom | 6 / 6 | 6 / 6, pinned |
| destination, axes unset | SDR Display P3 TIFF | HDR 32-bit float BT.2020 TIFF; a stated axis that rules it out falls back to the lossless 16-bit TIFF (Adobe RGB unless a gamut is stated) |

`default` is Hanten's picture from what was measured; `direct` loses as little as it
can and applies only what the container needs — the handoff to an editor, and (as
`--rendering direct --range sdr`, the form you can judge by eye) the rendering the
calibration loop holds fixed. A knob you state builds on either: `--white-balance`
multiplies the base gains, every other knob replaces its base value. A stated axis is
never overridden, and `direct` decides its container before its range, so it never
takes a lossy container by default — only when you state one, or when your stated axes
leave no lossless row: `--rendering direct --gamut display-p3` is an SDR Display P3 TIFF
(not the gain-map JPEG) and `--transfer native` the Adobe RGB TIFF, while `--container
jpeg`, or `--range hdr --gamut display-p3` (the one row left), is the gain map. The
report states it in `chain.rendering`.

```console
$ hanten convert scan.tif -o d1 --film-base 0.9,0.55,0.42 --rendering direct \
    | jq -c '.output, .chain.destination'
"d1.tiff"
{"display":{"range":"hdr","transfer":"linear","gamut":"bt2020","container":"tiff"}}
```

- **`default` without a roll measurement warns**, since what it renders then is a
  fallback, not a measurement — and that is every plain `convert` until you run
  `measure-roll`:

  ```text
  hanten: warning: no roll measurement: rendered with neutral white balance (no `roll.white_balance`) and the fallback contrast 1.1111112 (no `roll.white_stops`). Run `hanten measure-roll` over the roll and use the recipe it writes (its `roll` section); or state the white balance and contrast you want (`scene_correction.white_balance`, `look.contrast`); or use the `direct` rendering (`rendering`: "direct"), the decode without a roll correction, whose unset destination is the HDR float TIFF
  ```

  A typed `--white-balance` or `--contrast` is a choice — even `--white-balance 1,1,1` —
  and silences its half. `--strict` fails the run on it: without a roll measurement,
  state `--white-balance` and `--contrast`, or pass `--rendering direct`.
- **A recipe value an earlier build could have written unchosen warns under
  `direct`**: highlight desaturation at exactly 0.8, which every earlier recipe stated
  and which would turn the pull back on, and a stated `look.contrast` or white balance
  beside a `roll` section, which an earlier `measure-roll` wrote. Every other old
  default equals `direct`'s base. A deliberate value — any other, or a typed flag —
  never warns, so a `--dump-params` recipe replays under `--strict`, with one carve-out:
  in a recipe that has a `roll` section, a white balance or contrast you typed is
  dumped as a recipe value and warns on replay, since a file cannot say who chose it —
  type the flag again on replay to keep it without the warning:

  ```console
  $ cat olddirect.json
  {"recipe_version": 2, "rendering": "direct",
   "look": {"contrast": 1.1111112,
            "highlight_desaturation": {"strength": 0.8, "start_stops": -1.0, "band": [0.015, 0.025]}},
   "fit_range": {"headroom_stops": 6.0, "display_black": 6.0}}
  $ hanten convert scan.tif -o od --film-base 0.9,0.55,0.42 --params olddirect.json --report none
  hanten: warning: the recipe moves the `direct` rendering's pinned base: `look.highlight_desaturation.strength` 0.8 (direct: 0, off; every recipe an earlier build wrote stated 0.8). If the strength came from a recipe an earlier build wrote, set it to `null` so `direct` renders as pinned; a deliberate adjustment is fine — type it as a flag to keep it without this warning
  ```

  A recipe written entirely by an earlier `measure-roll` — its gains in
  `scene_correction.white_balance`, its contrast in `look.contrast`, and no `roll`
  section — does not warn: `direct` cannot tell those values from a deliberate
  adjustment, and applies them. Move the values into `roll` before rendering it
  `direct`.
- **`--rendering direct` with the film master is refused** — the film master runs no
  rendering for it to choose:

  ```console
  $ hanten convert … --rendering direct --film-master
  usage: --rendering direct (recipe `rendering`) chooses what the rendering stages start from, and --film-master (recipe `output`: `"film-master"`) runs none: it writes the fixed decode's linear ACEScg. Either pass --rendering default, or choose a rendered destination — `direct` alone writes the HDR float TIFF, the rendered output closest to the decode
  ```

  On `roll` the remedy is the key: set `rendering` to `"default"` (or remove it).
- **The roll flags are refused under `direct`**, which leaves the roll out: drop
  `--roll-white-balance` / `--roll-white`, or pass `--rendering default`. A recipe's
  `roll` section is not refused.

### Scene correction

The first stage applies white balance and exposure as per-channel gains on linear
ACEScg — after the decode's 3×3, before the look — and clamps nothing:

| Flag | Recipe key | |
|---|---|---|
| `--white-balance R,G,B` | `scene_correction.white_balance` = `{"explicit": [r, g, b]}` | stated gains, multiplied into the roll's (default `[1, 1, 1]`) |
| `--exposure EV` | `scene_correction.exposure` | a gain of `2^EV` (default `0`) |

The recipe takes only the tagged form — a bare `[r, g, b]` array, which earlier
recipes accepted, is refused. The report states what was applied:

```console
$ hanten convert scan.tif -o out --film-base 0.9,0.55,0.42 \
    --white-balance 1.2,1,0.8 --exposure 0.5 | jq -c '.chain.scene_correction'
{"white_balance":[1.2,1.0,0.8],"exposure":0.5}
```

**There is no per-frame auto white balance.** A frame's own statistics read a sunset
as the cast and remove it, so the gains are measured once per roll with
`hanten measure-roll` (below) and stated in the recipe's `roll` section.
`--white-balance` then multiplies the roll's gains: it adjusts the roll's balance
rather than replacing it, and at its default `1,1,1` the roll's gains apply exactly.
Typed, it never warns; the same gains stated in a recipe beside the roll's do (below),
since they may be an earlier `measure-roll`'s leftover. `--auto-wb` is refused, and so
is a recipe naming a per-frame mode:

```console
$ hanten convert … --auto-wb percentile
usage: --auto-wb was removed: a per-frame estimate reads a sunset as the cast and
removes it, so white balance is measured once per roll. Run `hanten measure-roll`,
then pass the gains it reports as `--roll-white-balance R,G,B` (recipe
`roll.white_balance`).

$ hanten convert … --params wb.json   # "white_balance": "percentile"
usage: recipe wb.json: `scene_correction.white_balance` "percentile" was a per-frame
estimate, and the chain has none: it read a sunset as the cast and removed it. Drop
it, then state the gains `hanten measure-roll` reports for the roll as
`roll.white_balance`: `[r, g, b]`
```

### The look

The stage between scene correction and fit range. It runs three controls, in this
order: **contrast**; the **per-channel grade**, which removes (or
adds) a cast that grows away from mid-grey; then **highlight desaturation**, which
pulls bright surfaces that are nearly neutral the rest of the way to neutral, so a
white that still carries a trace of cast after the roll's white balance reads clean.
Contrast and highlight desaturation are **on by default**; the grade is off.

| Flag | Recipe key | |
|---|---|---|
| `--contrast CONTRAST` | `look.contrast` | print contrast, pivoted at mid-grey; unstated, the roll's (`roll.white_stops`), else `2.0/1.8`; stated, it wins over the roll's; `1` is the identity, must be positive |
| `--channel-grade R,B` | `look.channel_grade` | red and blue exponents pivoted at mid-grey, green fixed at 1; default `1,1` (off); both positive, with the spread over `R,1,B` under 1 |
| `--highlight-desaturation STRENGTH` | `look.highlight_desaturation.strength` | `0`–`1`; unstated, `0.8` (off under `--rendering direct`); `0` is off |
| `--highlight-desaturation-start STOPS` | `look.highlight_desaturation.start_stops` | where the pull begins, in stops below diffuse white (unstated, `-1`) |
| `--highlight-desaturation-band S0,S1` | `look.highlight_desaturation.band` | the saturation band (unstated, `0.015,0.025`) |

- **It only touches near-neutral highlights.** Its strength rises from `start_stops`
  up to diffuse white, and falls to nothing across the band: a pixel whose channels
  differ by more than `S1` (measured as `log10(max/min)` over the whole contrast,
  `--density-gamma` × `--contrast`, so the band means the same density spread
  whichever knob carries a roll's contrast) is left alone. So a sunset, sand or skin keeps its colour; a cast white does not.
- **It assumes the roll's white balance.** "Near-neutral" means near R = G = B, which
  is near white only after `measure-roll`'s gains have removed the roll's cast.
- **Contrast pivots at mid-grey**, on each ACEScg channel: mid-grey stays put and each
  stop away from it becomes `CONTRAST` stops. A neutral stays neutral; saturated colour
  shifts slightly against the pre-split single slope, which acted before the NC film
  RGB 3×3 rather than after it. It runs after scene correction, so `--exposure 1` at
  contrast 1.11 moves the picture 1.11 stops: exposure is in stops of the
  reconstructed scene.
- **The grade is for crossover** — a cast that differs between shadows and
  highlights, which one set of white-balance gains cannot remove. Each of red and blue
  becomes `0.18 · (v / 0.18)^R` (or `^B`), and the pixel's luminance is then put back,
  so mid-grey stays neutral, the cast it adds grows with distance from mid in both
  directions, and neutral contrast is untouched — that stays `--contrast`'s. Below 1
  a channel is pulled down in the highlights and up in the shadows; above 1 the
  reverse. It runs after contrast, so the same values act more strongly on a
  contrastier picture. It does not replace the roll's white balance, which should be
  set first, or the decode's calibrated `--density-scale`.
- **Luminance is kept** by the grade and by highlight desaturation; only chroma moves.
  `--highlight-desaturation 0` turns desaturation off — with `--contrast 1` and the
  grade at `1,1` the look is the exact identity, the way to see the roll's raw cast.
- The report says what ran:

  ```console
  $ hanten convert scan.tif -o out --film-base 0.9,0.55,0.42 \
      | jq -c '{look: .chain.look, stage: .chain.stages[1]}'
  {"look":{"contrast":1.1111112,"channel_grade":[1.0,1.0],"highlight_desaturation":{"strength":0.8,"start_stops":-1.0,"band":[0.015,0.025]}},"stage":{"stage":"look","applied":"contrast+highlight-desaturation"}}
  ```

- An out-of-range value is refused naming the flag and the key:

  ```console
  $ hanten convert … --highlight-desaturation 1.5
  usage: --highlight-desaturation (recipe `look.highlight_desaturation.strength`) must be
  within [0, 1] (0 is off), got 1.5
  ```

### Fit range

Fit range fits the scene's range into the display's. It has one operator, reinhard,
applied to luminance so all three channels scale together and hue is kept;
mid-grey stays where the decode put it. It also places black (below). Its two knobs:

| Flag | Recipe key | |
|---|---|---|
| `--display-tone-headroom STOPS` | `fit_range.headroom_stops` | `0`–`24`; unstated, `6` (under either rendering); `0` is the identity |
| `--display-black STOPS\|off` | `fit_range.display_black` | stops below mid-grey on the display, `(0, 16]` or `off`; unstated, `6` (under either rendering) |

The display's peak is the operator's other argument and belongs to the destination,
not the recipe — `1` for SDR, `1000/203 ≈ 4.93` for HDR. The report names what ran:

```console
$ hanten convert scan.tif -o out --film-base 0.9,0.55,0.42 \
    | jq '.chain.fit_range'
{
  "operator": "reinhard-peak-lifted-v1",
  "headroom_stops": 6.0,
  "white_point": 64.0,
  "display_peak": 1.0,
  "display_black": {
    "setting": 6.0,
    "curve": "log-shift-to-mid-grey-v1",
    "film_base_stops": 3.8510852,
    "shift_stops": -2.1489148
  }
}
```

At `0` the operator reads `"identity"`: reinhard passes the scene through unchanged,
and everything above display white clips at the encode (32% of the samples on
`tests/fixtures/hdr-48bit.tif`, against none at the default). Display black still runs
at `0` unless it is off, so the stage list names its curve then. A pixel with a
non-finite channel is refused (exit 1, naming the pixel) rather than passed to the
encoder.

`--display-tone-headroom 0` is also how to see the decode's highlights unrolled; to
size the headroom for a roll, read `.loss.clipped_high` against `.loss.total_samples`
on a representative frame.

#### Display black

Display black is where the film base — the darkest thing on the film, since
nothing below its threshold is recorded — lands on the display, in stops below
mid-grey. Without it black lands wherever the look's contrast leaves it: at the
default contrast the base renders 3.85 stops under mid-grey (above), which reads as a
pale, lifted black. At the default `6` (about L\* 2.5) it is moved down by the 2.15
stops `shift_stops` reports.

- **Nothing is measured from the image.** Where the base renders is computed from the
  film base you gave and the frame's own decode, white balance, exposure and contrast,
  so a flatter frame gets a larger shift and a steeper one a smaller one, and two
  frames of a roll at different contrasts both land their base at the same depth.
- **Only the shadows move.** The shift is whole at the base and below, and fades to
  nothing at mid-grey: mid-grey, the highlights and white are untouched. Below the base
  the shadows are darkened by the same factor rather than squeezed toward 0, so
  texture there survives. Colour is kept: all three channels scale together.
- **Fewer stops, lighter shadows with more detail; more stops, a deeper black.** It is
  a matter of taste per picture — `6` was chosen by review as the default.
- **A base already that deep is left alone**, never lifted: at a steep contrast the
  report shows `"curve": "identity"` and `"shift_stops": 0.0`. `off` leaves black
  wherever the grade puts it.
- **A base near mid-grey is warned about.** Only a strong `--exposure` gets it there:
  at the default contrast the warning starts at about `--exposure 1.75`. Closer than 2
  stops under mid-grey, the whole shift is squeezed into that narrow band and crushes
  the shadows; at or above mid-grey (about `--exposure 3.75`) black cannot place it and
  is skipped. Either way the report warns, naming
  `--exposure` and `--display-black off` as the remedies.
- **Not the removed `--black-point`**, a subtraction on every channel that crushed
  and tinted the shadows. Removing scanner veil or base fog is a separate, scene-side
  correction that has not landed.

```console
$ hanten convert … --display-black 0
usage: --display-black (recipe `fit_range.display_black`) must be stops below mid-grey
within (0, 16], or `off`, got 0
```

### Fit gamut

The last stage maps colour outside the destination's gamut onto its boundary, keeping
hue: never clipped channel by channel, it moves toward neutral at the same luminance
until it fits. The one exception is a colour whose luminance there is zero or below,
which has no in-gamut rendition and is written black. It has no knobs — its ceiling is
fit range's display peak and its gamut the destination's — so its recipe section stays
empty. What can still clip is content brighter than fit range's headroom, which reaches
the encoder above display white as a neutral: counted in `loss`, and failed by
`--strict`.

### `measure-roll` — a roll's white balance and white, measured once

It measures two things a whole roll shares. The **white balance** removes the cast of
the film, the development and the scanner, and keeps the scene's light: one sunset
frame barely moves a statistic taken over every frame. The **roll's white** sets the
look's contrast, so the roll's highlights reach white with mid-grey held where it is.
Give it the roll's picture frames, its leader, and its film base — measured here from
the unexposed frame with `--unexposed` (§4), or stated explicitly:

```console
$ hanten measure-roll frames/*.tif --leader leader.tif --film-base 0.47095445,0.23244068,0.10803387
{
  "command": "measure-roll",
  "leader": { "median": [0.92210037, 0.72462136, 0.46541658], "guard_density": 0.1,
              "ceiling": [0.6092257, 0.47875258, 0.30749768], "film_peak": 1.0815561, … },
  "frames": [ { "input": "frames/1774.tif", "region": [167, 167, 4579, 3009],
                "holder_applied": false, "sampled": 131072, "kept": 131065, "guarded": 7,
                "unusable": 0, "white_stops": 0.52260643,
                "leader_distance_stops": 2.0644333, "white_role": "under", … }, … ],
  "white_balance": { "gains": [1.0026785, 1.0, 1.2466215], "percentile": 0.99, … },
  "white": { "stops": 1.5, "bound": "floor", "contrast": 1.6492873,
             "whole_contrast": 2.968717,
             "clamped": [ { "input": "frames/1816.tif", "white_stops": 2.297903,
                            "contrast": 1.2369655,
                            "flag": "--roll-white-balance 1.0026785,1,1.2466215 --roll-white 2" }, … ],
             "rule": { "channel": "max", "percentile": 0.97, "cap_stops": 2.0,
                       "floor_stops": 1.5, "saturation_margin_stops": 0.5 } },
  "reuse": { "flag": "--roll-white-balance 1.0026785,1,1.2466215 --roll-white 1.5" },
  "warnings": [ "frames/1816.tif: near film saturation — its white sits 0.29 stop under the leader (margin 0.5 stop); …", … ]
}
```

(Abridged; that is 35 frames of one roll.) Each frame is decoded under the fixed
decode — at its linearization, before the look's contrast — and sampled over its
**effective area** (§9). Freeze the result with `--out roll.json`, which writes the
base, the `roll` section and its clamps as one recipe for `roll --params` or
`convert --params` (§4) — or paste `reuse.flag` on `convert`. A clamped frame converted
by flag takes its own `white.clamped[].flag` instead: `reuse.flag` carries the roll's
white, which would undo the clamp. (By `--params`, the file's `roll.frames` entry does
that for you.)

The recipe keeps these in its **`roll` section**, apart from the style knobs, so a
measured value is never mistaken for a chosen one:

| Flag | Recipe key | |
|---|---|---|
| `--roll-white-balance R,G,B` | `roll.white_balance` | the roll's gains; `--white-balance` multiplies them |
| `--roll-white STOPS` | `roll.white_stops` | the roll's white; the look's contrast renders it at diffuse white unless `--contrast` is stated |
| — | `roll.frames` | `{"<file name>": {"white_stops": …}}`: a frame's own white, in place of the roll's. `convert` and `roll` apply their input's entry, before any flag; a `roll --frames` manifest's `params` beat it, and may not state the table. Keys are file names, not paths (exit 2) |

Each is optional. The report says what applied:

```console
$ hanten convert scan.tif -o out --film-base 0.9,0.55,0.42 \
    --roll-white-balance 1.1,1,0.9 --roll-white 1.7 --white-balance 1.2,1,1 \
    | jq -c '.chain.roll, .chain.scene_correction'
{"white_balance":[1.1,1.0,0.9],"white_stops":1.7,"contrast":1.4552535,"white_balance_applied":true,"contrast_applied":true}
{"white_balance":[1.32,1.0,0.9],"exposure":0.0}
```

With `--contrast` stated, `contrast_applied` is `false`: the stated contrast won. The
film master applies neither value, and reports both as not applied. The roll flags are
refused under it — a typed `--film-master` conflicts with them, and under a recipe's
`"film-master"` the refusal says to drop them or choose a rendered destination — but a
recipe's `roll` section is not.

A style value **a recipe states** beside a roll measurement is kept, and the run warns
(so `--strict` refuses it): it may be a leftover from an earlier build, not a choice.
A typed `--white-balance` or `--contrast` is a choice made now, and never warns. A
`--dump-params` recipe that states a contrast (or white balance) beside the roll's
measurement does warn on replay, since a file cannot say who chose a value; type the
flag on replay to keep it without the warning. `roll` warns once, for the shared recipe,
not per frame:

```console
$ cat old.json
{"recipe_version": 2,
 "roll": {"white_balance": [1.1, 1.0, 0.9], "white_stops": 1.7},
 "scene_correction": {"white_balance": {"explicit": [1.2, 1.0, 1.0]}},
 "look": {"contrast": 1.1111112}}
$ hanten convert scan.tif -o out --film-base 0.9,0.55,0.42 --params old.json --report none
hanten: warning: the recipe's `scene_correction.white_balance` [1.2, 1.0, 1.0] multiplies the roll's gains, `roll.white_balance` [1.1, 1.0, 0.9]: the white balance applied is [1.32, 1.0, 0.9]. A stated white balance is an adjustment on top of the roll's measurement; if it holds gains an earlier `hanten measure-roll` wrote there, drop it — they now live in `roll.white_balance`
hanten: warning: the recipe's `look.contrast` 1.1111112 overrides the roll's contrast 1.4552535 (from `roll.white_stops` 1.7). If it came from a recipe an earlier build wrote (every one stated `look.contrast` 1.1111112) or from an earlier `hanten measure-roll`, set it to `null` to use the roll's
```

**Adopting the `roll` section in a recipe an earlier build wrote:** drop
`scene_correction.white_balance` and set `look.contrast` to `null` if they hold
`measure-roll`'s old output (or, for the contrast, the `1.1111112` every earlier recipe
stated). No value is read as unset for you: a `--dump-params` recipe replays exactly
what it rendered.

**The white balance** equalizes the pooled pixels' per-channel 99th percentile,
green-anchored.

**The roll's white** is measured per frame, in **scene stops** above mid-grey: each
frame's `white_stops` is the 97th percentile of its pixels' **brightest channel**, before
the working-space matrix, which leaves speculars above white. The brightest channel
rather than red, so a blue sky or a green-lit highlight reads as bright as it is. The roll's white is the brightest frame
white at or under the **cap** (+2.0), raised to at least the **floor** (+1.5); `bound`
says which limit set it (`none`, `floor`, or `cap` when every frame is above it).
`contrast` is the look contrast that renders that white at diffuse white with
mid-grey pinned; the recipe stores the white (`roll.white_stops`) and the contrast is
derived from it at render time. `whole_contrast` is it times the decode's
linearization, for comparison only.

- **A frame above the cap is clamped**, not counted: it renders at the cap's contrast
  (`white.clamped`, beside the roll's `contrast`), which is gentler; `--out` records the
  cap as its white in `roll.frames`, and its own `flag` is for `convert`. An ordinary
  bright scene lands here too, so this is reported, not warned about.
- **A frame near its leader warns.** A white within 0.5 stop of the leader
  (`leader_distance_stops`, the same brightest-channel measure) is near film saturation, where the film compresses
  highlights and the decode renders them flat. Without `--leader` nothing is checked.
- **An underexposed roll is lifted only as far as the floor**; below it the roll
  renders dark.
- **The contrast places the white at exposure 0.** `measure-roll` does not read the
  recipe's `scene_correction.exposure`, and the look's contrast expands an exposure too:
  with `--exposure 0.5` at contrast 1.65 the white lands about 0.8 stop past diffuse
  white. That is an exposure doing its job, not a mismeasured white.
- The cap, floor and margin are provisional: they were chosen by review on nine rolls
  with no deliberately bad frames.

- **Pass the leader.** Any pixel within 0.1 density of it is left out of the white
  balance (`guarded`), so a fully exposed frame mixed into the inputs cannot set the
  gains — measured, it would move them 0.4–1.3 stops. A frame's white is measured
  before that guard, so a frame near saturation still shows it; the cap keeps it from
  raising the roll's white. Without `--leader` the run warns, no frame is checked for
  saturation, and `--strict` refuses it before decoding anything (exit 2).
- **Only picture frames, each once.** Every input is pooled as picture; leave out the
  unexposed base, the leader and any calibration frame. A frame named twice is
  refused (exit 2) — it would weigh double — and so are the `--leader` and
  `--unexposed` files among the inputs. With `--out`, two inputs sharing a file name are
  refused too, since `roll.frames` keys by it. A frame that contributes nothing, or whose
  holder cut is not a measurement (the same warning `convert` gives), is warned about
  by name, so `--strict` catches both.
- **The base is measured or explicit** — `--unexposed`, or `--film-base`, or
  `calibration.film_base` in a `--params` recipe (`"recipe_version": 2`, whose
  `reconstruction` it decodes under; its `roll` section, which this measures, and its
  `scene_correction` and `look` are not read). A base estimated per frame would decode
  every frame differently, so anything else is refused (exit 2) naming `--unexposed`
  and `measure-base --out`. `--unexposed` beside either stated form is refused too.

---

## 8. Destinations

**A destination is four separate knobs**, not a preset name — `--range`,
`--transfer`, `--gamut`, `--container` (recipe `output.display`), or `--film-master`
instead of all four (recipe `output`: `"film-master"`). Only these combinations are
written today; the "Suffix" column is what a path may *state*, and the **bold**
spelling is the one `hanten` writes when you leave the suffix off:

| `--range` | `--transfer` | `--gamut` | `--container` | Suffix | writes |
|---|---|---|---|---|---|
| `sdr` *(default)* | `native` | `display-p3` *(default)* / `adobe-rgb` | `tiff` | `.tif` / **`.tiff`** | 16-bit TIFF in the gamut's own curve (sRGB curve / `563/256`) |
| `hdr` | `linear` | `bt2020` | `tiff` | `.tif` / **`.tiff`** | 32-bit float display-linear TIFF (1.0 = 203 cd/m²) |
| `hdr` | `pq` / `hlg` | `bt2020` | `tiff` | `.tif` / **`.tiff`** | Rec.2100 signal as full-range 16-bit TIFF codes |
| `hdr` | `pq` / `hlg` | `bt2020` | `avif` | **`.avif`** | 10-bit 4:4:4 AVIF |
| `hdr` | `native` | `display-p3` | `jpeg` | **`.jpg`** / `.jpeg` | gain-map JPEG: an 8-bit SDR base with a per-channel ISO 21496-1 gain map |

With no destination flag the result is an **SDR Display P3 16-bit TIFF** under the
default rendering, and the HDR float TIFF under `--rendering direct` (§7). An SDR JPEG
(`--container jpeg` alone) is planned (`output/sdr-jpeg-preset`), and refused as not
written yet; sRGB and ProPhoto have no destination yet.

**`--film-master`** writes the fixed decode's linear ACEScg as an unclamped 32-bit
float TIFF with **no** rendering stage, so it refuses any stage you ask for, naming the
stage:

```
usage: --film-master (recipe `output`: `"film-master"`) writes the fixed decode's
       linear ACEScg with no rendering stage, so it cannot apply scene correction
       (--exposure, --white-balance, recipe `scene_correction`) this recipe asks for.
       Either drop the flags and recipe keys that ask for it, or state its identity
       (--exposure 0 --white-balance 1,1,1), or choose a rendered destination
```

Each stage's default and its identity are accepted, since neither asks for anything —
`--exposure 0 --white-balance 1,1,1`, the empty look `--contrast 1
--highlight-desaturation 0`, and `--display-tone-headroom 0 --display-black off` (or
their defaults) — which is how a flag clears a recipe's stage for a master. The film
master and a destination axis are one choice, refused at the parser.

**The gain-map JPEG** renders one graded image twice — an SDR base and an HDR
rendition clamped to the 1000 cd/m² peak — and stores the per-channel ratio between
them as a half-resolution, three-channel gain map. It carries **ISO 21496-1 metadata
only**, in a Multi-Picture Format container Hanten writes itself: no Ultra HDR v1 XMP,
which cannot describe a per-channel map. Apple ImageIO reads it as HDR; a reader that
knows only the Ultra HDR v1 XMP, or no gain maps at all, shows the SDR base.

### Leave an axis unset and it is derived

In the order range, transfer, gamut, container (under `--rendering direct`, the
container first): its default when a destination fits,
else the one value left, else a refusal listing the choices. So `--range hdr` alone is
the gain-map JPEG, `--transfer pq` alone an HDR BT.2020 TIFF and `--gamut adobe-rgb`
alone the Adobe RGB TIFF, while `--gamut bt2020` asks which transfer. A value you
**state** is never overridden — a combination the table lacks is refused, naming the
conflicting pair and a flag that fixes it:

```
usage: no destination combines --range hdr and --gamut adobe-rgb (recipe keys
       `output.display.range`, `.transfer`, `.gamut`, `.container`). Use --range sdr,
       --gamut display-p3, or --gamut bt2020 with --transfer linear|pq|hlg
```

The report records every resolved axis in `chain.destination`
(`{"display": {"range": "hdr", "transfer": "pq", "gamut": "bt2020", "container":
"avif"}}`, or `"film-master"`), which is exactly the recipe `output` that replays it.

### HDR destinations

An HDR destination clamps its rendition to the 1000 cd/m² peak and counts what that
clamped in `chain.peak_clamp` and in `loss`, where `--strict` sees it. Each also
fills a block stating what the encoder wrote and the luminance anchors no container
can carry — `avif` for the AVIF pair, `hdr_coded_tiff` for the PQ/HLG TIFFs,
`hdr_linear_tiff` for the float TIFF (in a `roll` report, on each frame):

```console
$ hanten convert scan.tif -o out --film-base … --transfer pq --container avif | jq -c .avif.rendering
{"reference_white_nits":203.0,"target_peak_nits":1000.0,"linear_headroom":4.9261084,"tone_curve":"reinhard-peak-lifted-v1+log-shift-to-mid-grey-v1","gamut_mapping":"acescg-to-bt2020-matrix+neutral-axis-radial-boundary-v2","linear_domain":"bt2020-linear-relative-to-203-nit-reference-white"}
```

A TIFF or AVIF whose brightest pixel stays at or below reference white is warned
about (`HDR output carries an SDR-range signal`), naming `--exposure` and `--range sdr`
as the remedies. The gain-map JPEG is not: an SDR-range frame makes a **flat** gain
map, which `chain.gain_map.flat` states and which is a correct file (it displays as
its base), so `--strict` passes it. Its `loss` counts both renditions — the SDR base's
clip and the HDR rendition's clamp — over both renditions' samples. `chain.gain_map`
also carries the per-channel gain range at full resolution (`min`, `max`, linear), the
stored map's `width` and `height`, and `base_fit_range` — fit range as the SDR base ran
it, since `chain.fit_range` is the HDR rendition's:

```console
$ hanten convert scan.tif -o out --film-base 0.9,0.55,0.42 --range hdr \
    | jq -c '.chain.gain_map | {min, max, flat, width, height}'
{"min":[0.9999999,1.0,1.0],"max":[1.9145154,1.9127859,1.9116272],"flat":false,"width":251,"height":231}
```

### You do not have to name the container

Leave the suffix off and `hanten` supplies it from the resolved destination:

```console
$ hanten convert scan.tif -o out --film-base 1,1,1 | jq -r .output
out.tiff

$ hanten convert scan.tif -o out --range hdr --film-base 1,1,1 | jq -r .output
out.jpg
```

The report's `output` field always names what was actually written. Four rules make
this predictable:

- **A suffix you state is never rewritten.** `-o out.tif` writes `out.tif`, not
  `out.tiff`; case is preserved as typed.
- **A dot-segment is only a suffix if `hanten` recognises the container.**
  `-o out.v2` and `-o roll-1.2` are stems, so they get `out.v2.tiff` and
  `roll-1.2.tiff`. Only `.tif`, `.tiff`, `.jpg`, `.jpeg` and `.avif` are read as a
  container request.
- **A path that names a directory is refused.** There is nothing to append to, so
  `-o positives/` and `-o positives/.` both exit 2 rather than writing
  `positives.tiff` beside the directory. Name the file inside it
  (`-o positives/out`), or use `hanten roll --out-dir positives/` for a whole roll.
  In a `roll` manifest the same applies to an `"output"` of `"."` — drop the
  `output` key instead and the frame takes its derived name inside `--out-dir`.
- **A stated suffix the destination does not write is an error**, never a silent
  rename. When a destination written today has that container, the refusal offers it,
  as the flags to add on top of what you stated:

```
usage: the output path out.jpg does not end in .tif or .tiff: the destination is
       --range sdr --transfer native --gamut display-p3 --container tiff, which writes
       .tif or .tiff. Hanten never renames a suffix you state — drop .jpg and the path
       is completed for you, or state a destination that writes it: --range hdr
       --container jpeg
```

`-o out.avif` offers `--transfer pq --container avif; --transfer hlg --container
avif`, and with a recipe stating `"gamut": "adobe-rgb"` each offer also carries
`--gamut bt2020`. With a typed `--film-master` the offer says to drop it first; a
recipe's `"film-master"` is replaced by the offered flags themselves. A `roll` frame's
refusal names the recipe keys (`output.display.…`) instead of flags. With `-v`,
`hanten` says on stderr when it completed a path.

### `roll` and the destination

`roll` takes no destination flags: its destination is the shared recipe's `output`.
It derives `<stem>_positive.<ext>` from each frame's own destination, so a default
roll writes `_positive.tiff` and one with `"output": {"display": {"range": "hdr"}}`
writes `_positive.jpg`.

### The output presets — removed

`--output-preset` (and the recipe key `output.preset`) retired with the chain its
presets named, and with them `--out-depth`, `--output-hdr`, `--output-sdr`,
`--output-profile` and `--bigtiff`: every destination resolves its own depth and
profile, and BigTIFF is decided automatically. Each is refused; `--output-preset`
names the preset's counterpart where one exists:

```
usage: --output-preset was removed with the chain its presets named: a destination is
       four separate knobs — --range, --transfer, --gamut, --container (recipe
       `output.display`) — or --film-master. For `hdr-pq`, pass --transfer pq
       --container avif. There is no alias.
```

Each named set resolves to the same destination under either rendering (so
`display-p3` names `--gamut display-p3` rather than "the default").
`gain-map-hdr` and `ultra-hdr-v1`'s nearest is `--range hdr --container jpeg`, a
different file (a per-channel, ISO-only map); `compatibility`'s sRGB has no destination
yet. To reproduce a preset's render, use the reference build.

---

## 9. Input semantics and the IR channel

`hanten` resolves two **independent** axes from the container's evidence, and you can
assert either one:

| Flag | Values | Meaning |
|---|---|---|
| `--input-transfer` | `auto`, `linear` | How samples are **encoded** |
| `--input-meaning` | `auto`, `scanner-device`, `colorimetric` | What they **measure** |

Only `scanner-device` + a linear transfer enters the density path. `colorimetric`
is recognized but unsupported — `convert` rejects it even when explicitly asserted.
`inspect` shows the resolution and the evidence chain under `input_color`.

`--input-profile` is reserved and currently rejected: input-side ICC application
has no validated placement in the pipeline yet.

### IR (HDRi 64-bit input)

The IR plane is decoded and **preserved, but no rendered pixel depends on it**:

- `--export-ir PATH` writes the decoded plane out. **`convert` only** — `roll`
  rejects `input.export_ir`, because one path cannot serve every frame, so IR
  planes have to be exported frame by frame.
- **IR film-holder measurement** runs by itself when the plane can do the job: it
  is the first cut of the [effective area](#the-measurement-region-the-effective-area),
  which `measure-base` and `measure-roll` measure over. Hanten measures the interior IR
  transmission and, if the film reads IR-transparent, marches in from each edge to
  where the opaque holder ends. There is nothing to declare — `--film-type` does
  **not** gate it.

  `--film-type silver|chromogenic|unknown` still exists on `convert`, `measure-base`
  and `inspect` (recipe key `input.film_type`), as a **provenance declaration**:
  it records what stock a run was made from, and nothing reads it. Set it if you
  want the film chemistry captured beside the output: the report echoes it as
  `film_type` (on `roll`, per frame; never `unknown`), and `--dump-params` writes it as
  `input.film_type`. Leave it out otherwise. Planned IR dust removal will need the same
  declaration, which is why it stays.

  `hanten inspect` and `hanten measure-base` report the verdict (the per-edge depths are
  `effective_area.holder`):

  ```sh
  hanten inspect scan.tif | jq -c '.ir_separability'
  ```
  ```json
  {"interior_median":0.67963684,"usable":true}
  ```

  When the film itself is opaque to IR — a fully-exposed silver-halide frame, say
  — holder and film cannot be told apart, so the holder is not measured and
  `inspect` and `measure-base` say so, naming the measurement: the effective area is then
  the inset alone. The same happens for an IR page identified by shape alone (no
  `NewSubfileType=4` marker), which is never trusted.

  Why measured and not declared: silver blocks IR *in proportion to accumulated
  density*, so an **unexposed** silver frame is IR-transparent against an opaque
  holder (~20:1) while its own **leader** is opaque throughout. Film chemistry
  mispredicts both — and on exactly the unexposed and leader frames of a roll.

IR-based dust removal is not implemented.

### The measurement region (the "effective area")

A region `hanten` resolves on every frame it decodes, so that a measurement reads the
picture rather than the film holder: on an uncropped scan the holder is maximum
density, so a whole-frame statistic measures the holder instead. `hanten measure-base`
(its film base, §4) and `hanten measure-roll` (§7) measure over it; a `convert`
resolves and reports it but reads nothing over it. The area is two cuts, in order:

1. **The film holder**, measured per edge from the IR plane — the same separability
   verdict above. Nothing to configure.
2. **A static inset** of what is left, `--measure-inset FRAC` (recipe key
   `measure.inset`), default `0.05` of the shorter edge.

The inset is **not** a fallback for the first cut: it runs either way. Where the
holder could not be measured, it is simply the only cut.

Where the holder **was** measured, the applied inset is floored at one holder-probe
step — 0.5% of the shorter edge, which is the resolution the holder cut itself has.
The march stops at the start of the first band whose median reads film, and a film
median only means the holder covers less than half that band, so up to half a band
of it can sit inboard of any reported depth (a measured `0` included). The floor
absorbs that band. It binds only near zero — a 3600 px frame insets 180 px against
an 18 px step — and `inset` is the **applied** value, so `--measure-inset 0` on a
measured frame reads back as the step, not as 0:

```sh
hanten inspect --measure-inset 0 scan.tif | jq -c '.effective_area | {region, inset}'
```
```json
{"region":[90,108,5040,3402],"inset":18}
```

Where the holder was *not* measured there is no measurement resolution to respect,
and the fraction you state is exact.

Every command that decodes reports the result — `inspect`, `measure-base`, `convert`,
and each frame of a `roll` (under its own `effective_area` key). `convert` and `roll` resolve it on every run, so `--measure-inset` and the
recipe key are never silently ignored:

```sh
hanten inspect scan.tif | jq -c '.effective_area'
```
```json
{"region":[306,270,4662,3078],"holder":{"top":90,"bottom":72,"left":126,"right":36,"capped":{"top":false,"bottom":false,"left":false,"right":false},"converged":true},"holder_applied":true,"inset":180}
```

`region` is `[x, y, w, h]`, in the same convention as `--base-region`.
`holder_applied` is the short answer to "did reading the IR plane change this
rectangle?" — true only when the holder was measured *and* some edge is non-zero.
Read `holder` itself carefully, because the three cases are different answers:

| `holder` | meaning |
|---|---|
| `{"top":90, …}` | measured, and the holder is that deep on each edge |
| `{"top":0,"bottom":0,"left":0,"right":0, …}` | measured, and there is **no** holder — an already-cropped scan |
| `null` | **not measured**: no IR plane, a plane identified by shape alone, or film too IR-opaque to separate |

That last row is the one that should change what you do. The default 5% is sized
for the **rebate**, on the assumption that cut 1 removed the holder first. Where cut
1 did not run, that same 5% has to clear the holder *and* its rebate together — so
on a `null` scan with a visible holder, raise the inset past holder-plus-rebate, not
past the holder alone. (For scale: the IR march measures holder depths of 2.5–4% of
the shorter edge on real scans, which is already most of the default on its own.)
`--measure-inset FRAC` is the flag; here it is on the *measured* frame above, so you
can see the arithmetic (the holder cut is unchanged and the inset goes from 180 px
to 432):

```sh
hanten inspect --measure-inset 0.12 scan.tif | jq -c '.effective_area'
```
```json
{"region":[558,522,4158,2574],"holder":{"top":90,"bottom":72,"left":126,"right":36,"capped":{"top":false,"bottom":false,"left":false,"right":false},"converged":true},"holder_applied":true,"inset":432}
```

Hanten will not guess that number for you — it reports which case the run was in and
leaves the blind cut to you. Values outside `[0, 0.4]` are a usage error (exit 2)
from every command, before the file is read.

Two fields on `holder` say the measurement is not what it looks like. **Both emit a
warning, which `--strict` promotes to a failure** — the fields alone are not the
channel, because a silent field is exactly what let a tenfold over-cut through at
exit 0 while it was being built.

- `capped` — one flag per edge. A capped edge marched as deep as `hanten` looks (25% of
  the shorter edge) without finding film, so its depth is a **floor**, not a
  measurement. The consequence does not stop at that edge: each edge is measured
  over what the *perpendicular* edges' cuts leave, so a truncated depth truncates
  that cut too, and the perpendicular edges then cap as well — at depths that are
  **artifacts of the cap, not floors on their own holder**. A 400×400 frame with a
  120 px top holder and 10 px sides reports `top: 100` (a floor, correctly) and
  `left`/`right` as 100 as well, a tenfold over-cut. So the reading that matters is
  whether a capped edge has a capped *perpendicular* neighbour: with one, treat no
  depth on the frame as measured; without one (a single edge exactly at the cap) the
  other three stand. The warning says which case you are in. No real scan has capped
  — 31 measured IR frames, zero caps, a 6–10× margin.
- `converged: false` — the per-edge march did not settle (the iteration above). Hanten
  then reports the deeper of the last two rounds, which over-cuts rather than leaving
  holder inside the region for a two-round oscillation or a run still settling
  downward; a longer cycle, or one settling upward, could still under-cut. A far
  enough over-cut leaves nothing to measure, which is refused outright on a run that
  measures over the region (exit 2, or a failed frame on a roll) and warned about
  otherwise. No real scan has produced this.

`converged` and `capped` are **not** independent, and `converged: true` is not a
quality verdict on its own: a cap *creates* a stable fixed point, so the worst
answer the march can produce — the tenfold over-cut above — settles and reports
`converged: true`. Read the two together.

Two things this does *not* do:

- **It never crops the image.** Written dimensions, aspect ratio and pixel count are
  exactly as decoded. The effective area changes only which pixels a statistic is
  computed over.
- **It never looks for the rebate.** The inset passes over it blind. That is why
  `measure-base` wants an unexposed frame, where the whole area is unexposed film; on a
  picture frame, give it a region (`--base-region`).

Every command that decodes resolves the area and reports it; a conversion reads
nothing over it. So if the two cuts leave **nothing**, `measure-base` (with no source
flag) refuses (exit 2), but `convert` and `roll` warn rather than refuse, and the report
omits `effective_area` — there is no region to report, and `--measure-inset` has no
effect on that run.

> Every `convert` of a scan carrying an IR plane warns "input carries an IR plane; it
> is preserved but not used in the conversion", which **`--strict` promotes to a
> failure**. The effective area's holder march reads the plane, but no rendered pixel
> depends on it, so it does not count. Either drop `--strict` for those runs, or use
> `--export-ir` so the plane is consumed.

`--export-ir PATH` writes the plane from the decoded image at the destination's depth —
32-bit float beside a float TIFF, 16-bit otherwise.

---

## 10. Reports, warnings, and exit codes

The JSON report on stdout carries the run identity (`nc_version`, the commit,
`pipeline_version`, the target), the resolved input semantics and film base, the
measurement region, the memory decision, `chain` — what the decode, each stage and
the destination ran (§6–§8) — encode loss statistics, `output_stats` (the written
samples' mean, the cross-build comparison basis), and warnings:

```sh
hanten convert scan.tif -o out --film-base … | jq '.loss, .warnings'
```

A `convert` report also carries `recipe`, the resolved recipe exactly as
`--dump-params` would write it — save it and it reloads through `--params` to the same
image — and `identity.params_hash`, a stable hash of those bytes (the telemetry
record's `conversion.params_hash`). Each `roll` frame's `identity.params_hash` hashes
the recipe that frame ran, the shared one plus its overrides; the roll report does not
echo the recipe. No other `identity` carries a hash — not `inspect`, `measure-base`,
`measure-roll`, nor the roll-level one.

```sh
hanten convert scan.tif -o out --film-base … | jq .recipe > out-recipe.json
hanten convert scan.tif -o repro --params out-recipe.json
# → repro.tiff is byte-identical to out.tiff
```

Clipping is reported, never silent:

```json
["output lost 126296 clipped and 0 non-finite of 695772 samples (18.15%)"]
```

Every encoder counts this, not just the TIFF ones — the gain-map JPEG and the
AVIF paths build the same report when they quantize.

Fit range compresses the scene against its headroom, so on both test fixtures **a
default render does not clip**. A clip warning therefore means something pushed samples
past fit range's reach — most often **exposure** (`--exposure 12` clips 100% of a frame)
or a `--display-tone-headroom` too small for the content. Check those before anything
else.

`--strict` promotes warnings **that reach the JSON report** to a hard error
(exit 1), after the report is emitted — the right default for scripts and CI.

> One deliberate exception: a failure to write an opted-in **telemetry**
> destination prints `hanten: warning:` on stderr but is kept out of the report set, so
> it stays fail-soft even under `--strict`. Telemetry must never change a
> conversion's outcome. A script that needs to know telemetry landed has to check
> the file, not the exit code. A telemetry path may not be the input, the
> `--params` recipe, or an output (exit 2); a run that fails before that check (a bad
> recipe, say) writes no event there — `hanten: warning: telemetry: no event
> written: …`.

### Exit codes

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Generic / unexpected error — **including `--strict` with warnings present** |
| 2 | Invalid CLI usage or parameters (bad flag value, a destination not written, wrong suffix, bad recipe, a removed flag) |
| 3 | Input read/decode error |
| 4 | Unsupported variant (e.g. a channel layout not handled yet) |
| 5 | Output write error |
| 6 | Resource limit — estimated peak memory exceeds the budget |

---

## 11. Operational flags

The flags in the tables below are **not** conversion knobs: they never appear in a
recipe and can never perturb a pixel.

| Flag | Purpose |
|---|---|
| `--max-memory BYTES` | Peak-memory budget, checked **before decode**. Accepts `8GiB`, `4096MB`, or raw bytes. Default 6 GiB — a fixed value, so the pass/fail decision is machine-independent. Over budget ⇒ **exit 6**. |
| `--report` / `--report-file` | Report format (`json`, `none`) and destination |
| `-v` / `-vv` / `--quiet` | stderr verbosity — never pollutes stdout |
| `--strict` | Promote warnings to errors |

Two more are **`convert` only** — `roll`, `measure-base` and `inspect` do not accept
them and exit 2 if given one:

| Flag | Purpose |
|---|---|
| `--telemetry` / `--telemetry-file` | Opt-in, fail-soft performance event (JSONL, `schema_version` 10), one per run — failed runs included, once the command line parses. `outcome.status` is `success` or `failure`; a failure names its `stage` (a stage, or `setup` / `preflight` / `finalize`), `error_kind` (`usage`, `decode`, …, or `strict` for a `--strict` promotion) and `exit_code`, and carries only what the run reached — never the error message. `timing_ms` has one field per completed stage, `conversion.params_hash` is the report's. Also `NC_TELEMETRY_LOG`. |
| `--seed N` | Reserved; nothing is stochastic today |

> **Caveat on `--max-memory`:** the budget also caps the TIFF read buffers, so a
> small-but-passing budget can turn a decodable file into an exit-3 decode failure.
> There is also a warning tier above ~70% of detected RAM — the one documented
> exception to machine-independence, since with `--strict` the same run can exit 0
> on a large machine and non-zero on a small one. The *image* is still identical;
> only the exit code differs.

On `roll`, the gate runs **per frame**: a rejected frame is recorded in the report,
its siblings are still written, and the roll exits **1**, not 6.

`--new-flow`, which selected this chain while a second one was the default, was removed
when it became the only one; passing it exits 2 on every command.

---

## 12. Troubleshooting

**"no film base selected"**
Neither `convert` nor `roll` has a default film base — but they take it from
different places. On **`convert`**, pass `--film-base R,G,B` (measured once per
roll) or `--base-region X,Y,W,H`. **`roll` accepts neither flag**: set
`calibration.film_base` in the shared `--params` recipe instead. `hanten measure-base
<unexposed-frame>` is the way to get a value in the first place.

**"the effective area is not uniform … it does not look like unexposed film"**
`measure-base` was given a picture frame. Give it the roll's unexposed frame, or, if the
roll has none, a region of unexposed film on another frame (`--base-region`).

**"base-region … is not uniform (worst per-channel relative spread …)"**
Your rectangle mixes unexposed film with image content. Check the coordinates, or run
`measure-base` on a genuinely unexposed frame.

**Heavy clipping in the report**
Fit range does not clip ordinary content at its default headroom, so something pushed
content past it: a positive `--exposure`, too little `--display-tone-headroom`, or a
low anchor (`--anchor-mid-offset` smaller than the default). Lower the exposure, raise
the headroom, move the anchor up, or write a float output (`--transfer linear`,
`--film-master`) for an unclamped result.

**"no roll measurement: rendered with …"**
Every `convert` under the `default` rendering warns until the roll is measured, and
`--strict` fails on it. Run `hanten measure-roll … --out roll.json` and convert with
`--params roll.json`; or state `--white-balance` and `--contrast`; or render
`--rendering direct` (§7).

**"a recipe must state `"recipe_version": 2`"**
The recipe was written before `pipeline_version` 8, for the removed chain (§5). Start
from `hanten params`, carry `calibration.film_base` across, and render the old recipe
itself with the reference build.

**`--strict` fails on every frame of an IR scan**
Expected — see §9: no conversion reads the IR plane, so it warns, and `--strict`
promotes it. Passing `--film-type` does not change this; it gates nothing. Either
drop `--strict` for those runs, or use `--export-ir` so the plane is consumed.

**Output differs between two machines**
Determinism is scoped to one build and architecture. Transcendental FP and the
lcms2 colour transform differ by ~1 ULP across platforms. Compare
`hanten --version` output — `pipeline_version`, commit, and target must all match.

---

## 13. Not yet available

So you don't go looking:

| Missing | Owning task |
|---|---|
| **Auto-cascade recipe generation** — a planner that produces a roll recipe for you, instead of you measuring and freezing it by hand | [`core/auto-calibration`](tasks/core/auto-calibration.md) |
| **Content-based film-base fallback** (`--base-content`) for cropped scans with no visible rebate | [`film-base/content-fallback`](tasks/film-base/content-fallback.md) |
| **IR dust removal** | roadmap follow-up, no task file yet |

[`docs/TASKS.md`](TASKS.md) is the authoritative status for all of it.

# Using Hanten

A practical guide to converting film negative scans to positives with `hanten`.

> **Scope.** This is the *user-facing* guide: what to run, in what order, and why.
> For the authoritative design rationale and the full parameter semantics, see
> [`design-spec.md`](design-spec.md). Where the two disagree, the spec wins on
> *intent* — but this document is verified against the binary, so it wins on
> *what the CLI currently accepts*.
>
> **Verified against:** `hanten 0.1.0`. Every command and quoted message was re-run on
> `tests/fixtures/` at `pipeline_version 8`, `038e7a3` (`nf-docs/using-nc`), except the few
> marked as from a real roll or scan. At `pipeline_version 9`
> (`nf-calibration/no-roll-defaults`, the look's fallback slope) the sections that slope
> reaches were re-run: §6's `reconstruction.contrast` refusal, §7's rendering table,
> no-roll warning, look report and fit-range report. At `nf-calibration/frame-level-trim`
> the roll section's examples (§5, §7) were re-run. At `algo/density-safety-bounds`
> (`pipeline_version` 9) §10's examples and §12's new entries were re-run, and at
> `output/drop-avif` §2's build prerequisites and §8's destination examples. At
> `core/recipe-replay-fidelity` the recipe files §4, §5 and §10 show were re-run, and at
> `output/content-light-levels` §8's HDR destination examples. At
> `nf-calibration/thin-frame-lift` §5's default recipe and `roll.json` and §7's roll
> examples were re-run (the `measure-roll` ones on the real rolls they name), and again at
> `nf-calibration/taste-vs-quality`, with §8's `direct` roll-flag refusal. At
> `nf-calibration/level-target-zero` §5's `roll.json` and §7's `measure-roll` report were
> re-run on their real rolls, and at `nf-verification/roll-side-exports` §5's default
> recipe, §8's film RGB export (now on `roll` too), §9's IR section and §11's flag tables.
> At `nf-scene-correction/midtone-neutral` (whites now measured after the roll's colour
> correction) §5's default recipe and `roll.json` and §7's scene-correction and roll report
> examples were re-run. At `pipeline_version` 10 (`nf-look/contrast-on-luminance`,
> contrast on luminance and a saturation setting) §5's default recipe, §6's replay
> warning and §7's look section and report were re-run. At `pipeline_version` 11
> (`nf-calibration/span-roll-slope`, the roll's slope from its span) §5's default recipe,
> `roll.json` and version 2 refusal, and §7's no-roll warning, `direct` refusal, roll
> report, thin-lift examples and `measure-roll` report were re-run (`roll.json` and the
> `measure-roll` report on their real rolls). At `core/auto-calibration` §4's
> `measure-roll` section and §7's measure-roll bullets were re-run, the `auto` report on
> its real roll, and at `core/profile-authoring` (`hanten profile`, `--save-recipe`,
> comments in recipes) §3, §4's look file, §5 and §10's recipe examples. The staleness
> signal is `pipeline_version`: if `hanten --version` reports a different one, treat
> this document as suspect and re-verify.
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
  the roll's white balance, midtone line, white and exposure are measured once the same way
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
│   --out roll.json  │        │  exposure,    │        │                      │
│                    │        │  clamps)      │        │                      │
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

A fresh machine needs only a C compiler besides Rust (no CMake or NASM). Cargo
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

## 3. The commands

| Command | Purpose | Writes an image? |
|---|---|---|
| `hanten inspect` | **"What is this file?"** — format, dimensions, IR presence, scanner metadata, resolved input semantics, the effective area. | No |
| `hanten measure-base` | **"What is this film's base?"** — measure the film base (`Dmin`) alone, from an unexposed frame or a region. Prints a reuse-ready `--film-base` flag; `--out` writes it as a recipe. (Was `estimate`, which now exits 2 naming it.) | No |
| `hanten profile` | Write a **look** from the conversion flags, with no scan: every setting but what belongs to one roll, as an annotated recipe (§5). (Was `params`, which now exits 2 naming it.) | No |
| `hanten convert` | Convert one frame. The full parameter surface. | Yes |
| `hanten roll` | Convert many frames from **one shared frozen recipe** — the same `--params` layers and flags as `convert`. | Yes |
| `hanten measure-roll` | **"What does this roll share?"** — its white balance, midtone line, white and exposure, measured once over its frames (§7), and with `--unexposed` its film base. `--out` writes it all as one recipe for `roll`. | No |
| `hanten telemetry` | Opt-in upload of anonymous `convert` telemetry: `enable`, `disable`, `status`, `preview`, `flush`, `purge` ([§11](#telemetry-upload)). | No |

Every image command (`inspect`, `measure-base`, `measure-roll`, `convert`, `roll`)
emits a **JSON report on stdout** on success (`--report none` to suppress,
`--report-file PATH` to redirect); `profile` writes its look to stdout or `--out`,
and `telemetry` has its own output (§11). Logs and warnings go to
**stderr**, so stdout stays clean for piping into `jq`.

> **A hard failure emits no report at all** — stdout is empty. A decode error, a
> memory refusal, or a measurement that fails (`measure-base` on a frame with no usable
> signal) exits non-zero before the report is written. Only `roll` is different: it
> aggregates per-frame failures into its report and still emits it. So a script
> must check the exit code, not just parse stdout.

> **A reader that stops early is not a failure.** Under `hanten … | head` (or
> `2>&1 | head`) the run finishes: the image and a `--out` recipe are still written,
> and the exit code is the run's own — `--strict` and a failed `roll` frame still
> exit 1. Under `-v`, stderr notes the dropped report (`hanten: stdout's reader
> closed; the report was not read`). Any other failure to write the report or
> the profile to stdout, such as a full disk behind `>`, exits 5.

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
       it from a region of unexposed film. Recipe key: `calibration.film_base`, which
       `hanten measure-roll <frames> --unexposed <unexposed-scan> --out roll.json`
       writes for `--params`.
```

One command measures everything a roll shares — the film base from its **unexposed
frame**, and its white balance, midtone line, white, dark end and exposure over its picture frames (§7) — and writes it
as one recipe:

```sh
hanten measure-roll frames/*.tif --unexposed unexposed.tif --leader leader.tif --out roll.json
```

`roll.json` is the measurement, ready for `--params` (a real Ektar 100 roll; `meta`
abridged):

```json
{
  "meta": { "nc_version": "0.1.0", "pipeline_version": 11, … },
  "params": {
    "recipe_version": 3,
    "calibration": { "film_base": { "explicit": [0.485832, 0.2621805, 0.17726406] } },
    "roll": {
      "white_balance": [0.886392, 1.0, 1.188019],
      "neutral_balance": null,
      "white_stops": 1.7573359,
      "dark_stops": -3.6460953,
      "exposure": 1.0854688,
      "frame_exposure": null,
      "small_lift": null,
      "thin_slope": null,
      "thin_exposure": null,
      "thin_lift": null,
      "midtone_line": null,
      "midtone_neutral": null,
      "frames": { "971.tif": { "white_stops": 2.0, "exposure": null,
                               "thin_slope": null, "thin_exposure": null } }
    },
    "reconstruction": {
      "scale": [1.0, 0.84, 0.73], "offset": [0.0, 0.0, 0.0],
      "linearization": 1.8, "anchor": { "mid-at-base-offset": 0.62 }
    }
  }
}
```

Like every recipe file Hanten writes, it is the recipe (`params`) beside the build that
wrote it (`meta`, §5 "No sidecar is written"). `roll.frames` holds, keyed by file name, the frames whose white is
above the roll's cap, the low-key frames given a small lift (`"exposure"`; none on
this roll), and the thin frames given a `"thin_slope"` and `"thin_exposure"` beside it
(§7). `roll.midtone_line` is `null` here because a three-frame roll is too short to
measure it (§7). `reconstruction` is the decode the gains were measured through, written
even at its defaults, because the gains hold only under it. If your `--params` recipe
stated `input` or `measure` keys, the file carries them too.

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

#### Finding the reference frames: `auto`

`--unexposed auto` and `--leader auto` find those frames among the inputs instead, so a
whole folder can go in as is. Each is separate: name one and find the other.

```sh
hanten measure-roll frames/*.tif --unexposed auto --leader auto --out roll.json
```

Every input's effective area is measured first, as `measure-base` measures it with no
source flag, so each frame is decoded twice. Then:

- **The unexposed frame** is flat (an area spread below 0.30) and the clearest such
  frame, and no input is clearer than it by more than 0.01 density on every channel.
  Any flat frame within 0.01 density of it on every channel is the same film and
  **corroborates** it, and the base is their per-channel median. A flat frame that is
  denser than that is a near-blank picture and stays a picture. A base from **one** frame
  is used, but with a warning that it is uncorroborated; `--strict` refuses it (exit 2)
  before the pictures are measured. Naming
  that frame with `--unexposed` (and leaving it out of the frames) clears the warning.
  Without an unexposed frame among the inputs, the clearest flat frame is the leader or a
  flat picture; the pictures beat it, so the run is refused.
- **The leader** is flat (spread at most 0.5) and at least 0.7 density above the roll's
  base on every channel. When several frames qualify, all are left out and the least
  dense one guards. If none qualifies, the run warns, as it does without `--leader`, and
  `--strict` refuses it (exit 2) before the pictures are measured.
- **Found frames leave the picture pool**, so the `--out` file is the one that naming
  them writes. It was byte-identical on 10 of the 11 trimmed archive rolls; the eleventh
  folder holds a calibration frame, which `auto` does not recognise and measures as a
  picture. A frame whose area gives no base is no candidate, and is measured as a
  picture.
- **If no frame is flat enough, or a clearer input beats the candidate, the run is
  refused** (exit 2): there is no per-frame base to fall back to. On the Ektar 100 roll
  with its unexposed frame left out, the leader is the clearest flat frame:

  ```
  usage: --unexposed auto found no unexposed frame among the inputs: the clearest flat
         frame, …/leader.tif, is no film base: …/989.tif is 0.89 density clearer on every
         channel, and unexposed film is the clearest thing on a roll (a leader or a flat
         picture, with no unexposed frame among the inputs). Name the roll's
         unexposed frame with --unexposed <file>, leaving it out of the frames; or, with
         none on the roll, measure a region of clear film (`hanten measure-base <frame>
         --base-region X,Y,W,H --out base.json`) and pass --params base.json in place of
         --unexposed
  ```

The report's `references` object records the evidence: each input's `class`
(`unexposed`, `leader` or `picture`), `area_median`, `area_spread` and
`density_above_base` (the last three absent for a frame whose area gave no base). Under `unexposed` it lists the `frames` the base came from, the
`confidence`, the `spread_density` across those frames, the `rejected` near-blank
pictures, and each frame's `measurements` as `measure-base` reports them. Under
`leader` it lists every leader-like frame. On the real Ektar 100 roll above (abridged):

```json
"references": {
  "inputs": [
    { "input": "…/971.tif",    "class": "picture",   "area_spread": 1.6501793,
      "density_above_base": [0.39922413, 0.53584325, 0.61948985] },
    { "input": "…/base.tif",   "class": "unexposed", "area_spread": 0.096840866,
      "density_above_base": [0.0, 0.0, 0.0] },
    { "input": "…/leader.tif", "class": "leader",    "area_spread": 0.23025207,
      "density_above_base": [1.2481698, 1.2628708, 1.290577] }, …
  ],
  "unexposed": { "frames": ["…/base.tif"], "confidence": "uncorroborated",
                 "spread_density": 0.0, "rejected": [], "max_spread": 0.3,
                 "agreement_density": 0.01, "measurements": [ … ] },
  "leader": { "frames": ["…/leader.tif"], "min_density": 0.7 }
}
```

#### The film base alone: `measure-base`

`measure-base` measures the film base and nothing else, and `--out` writes it as a
recipe (`params` is `{"recipe_version": 3, "calibration": {…}}`). Feed that to `measure-roll
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
"effective_area"` and `"film_base_percentile": 0.5`, and `area_spread` is that worst
per-channel `(p90 - p10) / p50` (0.097 on the Ektar roll's unexposed frame). A frame
that is not unexposed film warns (`--strict` fails on it):

```
hanten: warning: the effective area is not uniform (worst per-channel spread
(p90 - p10) / p50 = 1.08 > 0.50): it does not look like unexposed film, so the
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
color-consistent; a frame in `roll.frames` renders at its own white and lift. Outputs are named
`<input-stem>_positive.<ext>`, the suffix coming from the destination's container
(`.tiff` by default); a roll-level JSON report lands on stdout. A single frame takes
the same file: `hanten convert frame.tif -o out --params roll.json` renders
byte-identically to that frame of the roll, its `roll.frames` entry included.

**Your look is its own file.** `--params` is repeatable, and `roll` takes every flag
`convert` does, so the measurement and your choices stay apart. `hanten profile` writes
the look from the same flags, with no scan (§5, "A look: `hanten profile`"):

```sh
hanten profile --contrast 1.2 --out look.jsonc
hanten roll frames/*.tif --out-dir positives/ --params look.jsonc --params roll.json
hanten roll frames/*.tif --out-dir positives/ --params roll.json --exposure 0.3   # one-off
```

The two have different lifetimes: `calibration` and `roll` are what you measured off
*this* roll, and the look is what you reuse across rolls, so a look file states no
`calibration` or `roll` section — nor `scene_correction`, the white balance and exposure
you adjust that roll's measurement by, so those go on the command line. Put the measured file last: a later layer wins every
value it states (§5, "Precedence").

> **`--save-recipe` writes what you stated**, and it replays byte-identically. A knob
> you left unstated stays `null`, so the rendering still decides it on replay. A
> measured value is frozen only if you stated it: a run with `--base-region` saves
> the region, not the base it read, so a recipe saved from it **re-reads the base on
> every frame of the roll** — exactly what `roll` exists to prevent (`roll` warns
> "roll film base is NOT frozen"). Use the file `--out` writes.

---

## 5. Recipes

### Shape

A recipe is one JSON document, versioned as a document, with one section per stage.
The whole layout, at the defaults (`--save-recipe` writes it with your base; `profile`
and the measuring commands write parts of it):

```json
{
  "meta": { "nc_version": "0.1.0", "git_commit": "…", "git_dirty": false,
            "pipeline_version": 11, "target": "aarch64-apple-darwin" },
  "params": {
    "recipe_version": 3,
    "input":       { "transfer": "auto", "meaning": "auto",
                     "film_type": "unknown" },
    "calibration": { "film_base": null },
    "roll":        { "white_balance": null, "neutral_balance": null,
                     "white_stops": null, "dark_stops": null, "exposure": null,
                     "frame_exposure": null, "small_lift": null,
                     "thin_slope": null, "thin_exposure": null, "thin_lift": null,
                     "midtone_line": null, "midtone_neutral": null,
                     "frames": {} },
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
      "contrast": 1.0,
      "saturation": 1.0,
      "channel_grade": [1.0, 1.0],
      "highlight_desaturation": { "strength": null, "start_stops": null, "band": null }
    },
    "fit_range": { "headroom_stops": null, "display_black": null },
    "fit_gamut": {},
    "output": { "display": {} }
  }
}
```

(Reflowed.) The recipe is `params`; `meta` is the build that wrote it, which a replay
checks and never applies ("No sidecar is written", below). A recipe you write by hand
can be the bare `params` object. `calibration.film_base` is `null` here because it has
**no default**: `convert` and `roll` reject an unstated base.

`input`, `calibration` and `measure` describe the scan (§4, §9), and `roll` holds
what `hanten measure-roll` measured — nothing by default (§7). `reconstruction` is the
fixed decode (§6). `rendering` chooses what the stages start from (§7);
`scene_correction`, `look` and `fit_range` are the rendering stages, and a `null` knob
there is unstated and takes the rendering's value — highlight desaturation, headroom
and display black are 0.8, 6 and 6 under `default`. The look's `contrast` is a
multiplier, so its default `1.0` keeps the base slope: a thin frame's (`roll.thin_slope`), else the roll's, else the fallback ≈1.414 (§7). `fit_gamut` is empty for good — its ceiling comes from fit range and
its gamut from the destination. `output` is the destination (§8), with nothing stated
by default: every axis is derived.

### A look: `hanten profile`

`hanten profile` takes the conversion flags and writes the recipe they describe, with
no scan — every section above but the three that belong to one roll: `calibration` and
`roll`, its measurements, and `scene_correction`, the adjustment made on top of them:

```sh
hanten profile --contrast 1.2 --out look.jsonc
```

```jsonc
// A Hanten look profile, written by `hanten profile`. It writes every setting but what
// belongs to one roll (`calibration`, `roll`, `scene_correction`); a null takes the
// rendering's value, which a later build may move. `meta` names the build that wrote it.
// …
{
  "meta": { … },
  "params": {
    "recipe_version": 3,  // the recipe layout this file is written in
    "input": {  // how the scan's samples are read
      "transfer": "auto",  // how samples are encoded; --input-transfer (auto | linear)
      …
    "look": {  // contrast and colour
      "contrast": 1.2,  // multiplier on the base slope, > 0; 1 keeps it; --contrast
```

- **Every key is written.** One the rendering decides is `null` (highlight
  desaturation, the display tone, an unstated destination axis), and takes the
  rendering's value, which a later build may move; `contrast` and `saturation`
  multiply the roll's slope. Like every written recipe it is stamped, so a later
  `pipeline_version` warns on replay. It restates the decode (`reconstruction`), so
  layer it **before** a roll's measured file.
- **The comments are for you**: one per key, its units, values and flag. `--params`
  reads them (JSONC, `//` and `/* */`, in any recipe) and keeps none, so `profile` never
  overwrites the file: `--out` over an existing file exits 2 unless `--force`. Without
  `--out` the profile goes to stdout.
- **Checked without a scan.** A value out of range or a contradiction is refused here
  (exit 2), as `convert` would refuse it: `--range sdr --transfer pq` gets `no destination
  combines --range sdr and --transfer pq`. So is a look no film base could render, probed
  at a base thinner than any measured, and `--input-meaning colorimetric` (exit 4). What
  depends on your base or pixels — clipping, a render your base cannot hold — is checked
  when it converts.
- **What belongs to one roll is refused**, naming where it goes; `profile --help` does
  not list those flags. A white balance or exposure in a look would land as a recipe
  value, which beside a roll's measurement warns as a stale adjustment:

```
usage: hanten profile writes a look, and --roll-white sets a roll value (`roll`), which
       belongs to one roll: pass it to `convert` or `roll`, or write the roll's file
       with `hanten measure-roll <frames> --out roll.json` and layer it after the look
       (--params look.jsonc --params roll.json)
usage: hanten profile writes a look, and --exposure sets an adjustment to one roll's
       measured white balance and exposure (`scene_correction`): pass it to `convert` or
       `roll`, where it adjusts that roll's measurement
```

### Partial recipes are fine

Omit any section and the defaults fill the gap. This minimal recipe produces a
**byte-identical** result to `--film-base 0.163,0.080,0.0377`:

```json
{
  "recipe_version": 3,
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
that owns it (`--film-type` ⇒ `input.film_type`, not top level).

### Recipes from earlier builds

**A recipe without `"recipe_version"` is refused whole.** Every sidecar and
`--dump-params` file written before `pipeline_version` 8 describes the rendering chain
that version removed, and there is no converter — replaying one here would render a
different picture while claiming to be the same recipe:

```
usage: recipe old.json: a recipe must state `"recipe_version": 3`. A document without
       it — every sidecar and `--dump-params` file written before `pipeline_version` 8
       — describes the rendering chain that version removed, and there is no
       converter. In the current layout (`hanten profile` writes a look's sections,
       `hanten measure-base --out` the base's), `input`, `measure` and a `region` or
       `explicit` `calibration.film_base` carry over unchanged (an `"auto"` one
       retired: measure the base with `hanten measure-base <unexposed-frame>`), and
       the rest is a stage section each
       (`reconstruction`, `scene_correction`, `look`, `fit_range`) plus `output`, the
       destination
```

Carry the film base across by hand; render the old recipe itself with the reference
build. A version this build does not read is refused too (`this build reads 3 and 2`).

**A version 2 recipe is read, except a number in `look.contrast`.** Version 3 changed
only that key: in version 2 it was the slope itself, and now it multiplies the base
slope (§7), so the old number would render differently. It is refused with the
multiplier that keeps its tone scale — worked out against the recipe's own rendering and
roll; colour now follows `look.saturation` — and `null` (what version 2 wrote for
unset) is read as `1`. A version 2 roll states a white without the dark end the roll's
slope now needs (§7), so the multiplier waits on that:

```
usage: recipe old.json: `look.contrast` 1.4552535 in a `recipe_version` 2 recipe is the
       slope itself; since version 3 it is a multiplier on the base slope (1 keeps it),
       on luminance only: colour follows `look.saturation`. The roll's white needs its
       dark end now (`roll.dark_stops`: re-run `hanten measure-roll`), which sets the base
       slope; to keep the tone scale, state 1.4552535 over that base (the report's
       `chain.look.base_slope`) and `"recipe_version": 3`. If nobody chose the value
       (every recipe an earlier build wrote stated 1.1111112, and an earlier `hanten
       measure-roll` wrote the roll's contrast there), drop it and state
       `"recipe_version": 3` to use the base
```

A versioned recipe that still carries one of the removed chain's sections or keys is
refused naming it, and where its knobs went, rather than parsed and read by nothing:

```
usage: recipe print.json: `print` is a section of the removed chain's recipe: white
       balance and exposure are `scene_correction.white_balance` and
       `scene_correction.exposure`; the display tone is fit range, whose one operator
       is reinhard and whose headroom is `fit_range.headroom_stops`; the black point's
       surviving half is display black, `fit_range.display_black` (where the film
       base renders); and `linear_range`, the levels remap, retired: its gain is
       `scene_correction.exposure` and its black is `fit_range.display_black`. Drop it
```

The same holds for `reconstruction.density` (its `scale` and `offset` are
`reconstruction.scale` and `.offset`), `reconstruction.contrast` (split in two — §6),
`output.preset` (a destination is its axes — §8), `calibration.dmax` (the reference
density retired), and a per-frame `white_balance` mode such as `"percentile"` (§7).

### Precedence

`--params` is repeatable; `convert` and `roll` resolve this chain, later winning
(`measure-roll` layers its `--params` the same way):

```text
defaults < --params A < --params B < … < the frame's roll.frames entry < flags
         (< a roll frame's manifest params)
```

```sh
hanten convert scan.tif -o out --params look.json --params roll.json --exposure 0.5
#                                                                    ^ overrides both
```

- **Layers merge key by key.** A later layer replaces only the keys it states; one
  that switches a tagged value (`{"region": …}` → `{"explicit": …}`) replaces it whole.
  `roll.frames` merges entry by entry.
- **A `null` states nothing**, so a recipe's unset knobs erase nothing, and a layer
  cannot unset what an earlier one stated. **Any stated value wins**, a restated
  default included (a profile states `reconstruction`, `input` and `measure`): put the
  measured file last (`--params look.jsonc --params roll.json`). `roll.json` states its decode, so a look's `reconstruction` under it
  does not apply; to change the decode, measure the roll again under it.
- **A `--save-recipe` file is the whole run** — its base, its `roll` values and its
  `roll.frames` table — for replaying *that* run, not a look: layered under another
  roll's file, its table's entries survive (tables merge, and absence states nothing).
  Write a look with `hanten profile`. For the same reason `--save-recipe` may
  overwrite a `--params` file only when it is the sole layer.
- **Flags win over every layer, by *source*, not value** — an explicit
  `--white-balance 1,1,1` over a recipe's gains means neutral. That includes a frame's
  `roll.frames` entry: `--roll-white` renders every frame at that white.
- **`--params -` reads the recipe from stdin**, once; a second `-` exits 2.
- A fault in a layer is named against its file, and each layer must load on its own
  (a `recipe_version`, known keys).

### No sidecar is written

Earlier builds wrote a `<output>.json` sidecar beside every image; this one does not.
A `convert` report carries the resolved recipe (§10); a `roll` report does not, and
records each frame's `overrides` and `identity.params_hash` instead. To keep a recipe
file, write it with `--save-recipe` (was `--dump-params`, which now exits 2 naming
it), which replays byte-identically. It is written only when the run succeeds: a
`--strict` refusal still writes the image and the report but not the recipe, so a file
already at that path describes an earlier run.

```sh
hanten convert scan.tif -o out --film-base … --contrast 1.3 --save-recipe out.json
hanten convert scan.tif -o repro --params out.json
# → repro.tiff is byte-identical to out.tiff
```

A sidecar an earlier build left at the same path is **removed** when a run replaces
its image — it describes the picture just overwritten — and the report names it in
`chain.removed_sidecar`. A file there that is not one of Hanten's sidecars is left
alone, and so is one this run read as its `--params` recipe (with a warning, since it
still pairs by name with an image it no longer describes).

Every recipe file Hanten writes — `--save-recipe`, `hanten profile`, `measure-base
--out`, `measure-roll --out` — has the shape those sidecars had, `{"meta": …, "params":
…}`: `params` is the recipe, and `meta` is the `identity` of the build that wrote it
(§10). Only a file from a build before `pipeline_version` 8 counts as a sidecar, so a
`--save-recipe` file named `<output>.json` is not removed.

`meta` changes no pixel, but its `pipeline_version` is checked. A value a recipe leaves
unset takes this build's default, so a file written under another `pipeline_version`
may not render as it did there, and its replay warns (`--strict` exits 1):

```
hanten: warning: recipe old.json was produced by pipeline_version 9, but this build is
         pipeline_version 10 — the parameters still apply, but the default conversion
         behavior changed between them, so the output will not match the original
```

A malformed `meta` exits 2. A bare recipe — one you wrote, or a report's `recipe` —
records no version, and gets no check.

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
hanten roll --frames frames.json --out-dir positives/ --params roll.json
```

An override is merged onto the frame's resolved recipe — the `--params` layers, its
`roll.frames` entry *and* the flags — section by section, so it wins over a flag (a
`roll.white_stops` stated here survives `--roll-white`), and it need not state
`recipe_version` (`{"roll": {"white_stops": 2}}`) — except beside a `look.contrast`
number, whose meaning changed with version 3 (`{"recipe_version": 3, "look":
{"contrast": 1.3}}`; without it, exit 2); a `null` in it is refused (exit 2, naming the frame and key): an override cannot unset
a shared value, so omit the key to keep the roll's. A
removed chain's key in it is refused the same way, naming its frame. An explicit manifest `output` goes through the same suffix rule as `convert`
(§8): an extension it states must match the frame's destination, and one it omits is
completed, so `"output": "b-brighter"` writes `b-brighter.tiff` on a default roll.

A frame renders exactly as `convert --params` would with the shared recipe and its
override merged. Some keys describe the *roll*, not the frame:
`calibration.film_base`, `roll.white_balance`, `roll.midtone_line`, `roll.dark_stops`, `roll.exposure`,
`reconstruction` (every key), `rendering` and `output`. (A frame may turn the midtone
line off, `"roll": {"midtone_neutral": "off"}`, or the white balance with it,
`"roll": {"neutral_balance": "off"}`, without a warning.) An override that changes one is applied but warns, naming
both values (and `--strict` turns the warning into a failing exit), because the frame
then renders apart from its siblings — a roll is one piece of film through one
process. What counts is what the frame renders: restating the roll's value does not
warn, nor do gains the frame never applies (under `direct` or the film master), and a
`rendering` change names the destination it derives.
`roll.white_stops` is per frame: it is how `measure-roll` gives a clamped frame its
own white. With `"reconstruction": {"linearization": 1.7}` added to `b.tif`'s
`params`:

```console
$ hanten roll --frames frames.json --out-dir positives/ --params roll.json --report none
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
`anchor_rule`, `reads_reference` — always `false`, see Anchoring below — `linearization`, `scale`,
`offset`).

### The decode's slope and the picture's contrast are two knobs

`--density-gamma` is the film's **linearization** — undoing the negative's ≈0.55
density per decade — and the look's slope (§7) is how contrasty the picture is. With no
roll white, the whole slope is `1.8 × 1.414 ≈ 2.54`, steeper than the single slope of
2.0 that earlier builds bundled the two into. To change how contrasty a picture is, change
`--contrast`; `--density-gamma` is a calibration and moves with `--density-scale`,
never alone. A recipe stating the pre-split `reconstruction.contrast` is refused with
the value that keeps it:

```
usage: recipe old.json: `reconstruction.contrast` split in two: the decode's slope is
       now `reconstruction.linearization`, the film's linearization, and how contrasty
       the picture is is the look's `look.contrast`. Drop the key; to keep a stated 2
       as the whole slope, write `look.contrast` 0.78597355 (over the fallback slope 1.413675)
       and leave `reconstruction.linearization` at its default 1.8
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
> white acts through the look's slope, from the `roll.white_stops` that
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
["identity","contrast+saturation+highlight-desaturation","reinhard-peak-lifted-v1+log-shift-to-mid-grey-v1","acescg-to-display-p3-matrix+neutral-axis-radial-boundary-v2"]
```

Scene correction reports what ran joined by `+`, in order — `midtone-neutral`,
`white-balance`, `exposure` (e.g. `"midtone-neutral+white-balance+exposure"`) — or
`"identity"`; the look the controls that ran joined by `+`, or
`"identity"`; fit range its operator and display black's curve; fit gamut the change of
primaries and the gamut map, named for the destination's gamut. `--film-master` runs
none of them (§8).

The removed chain's print controls are refused, each saying where the knob went:

| Refused | Where it went |
|---|---|
| `--print-exposure` | `--exposure`, scene correction (below) |
| `--black-point` | display black, `--display-black` (below); a scene-side flare/fog subtraction was judged not needed (`nf-scene-correction/flare-removal`) |
| `--auto-wb` | none per frame: `hanten measure-roll` measures the roll's white balance once (below) |
| `--linear-range` | retired: its gain is `--exposure` and its black `--display-black` (`nf-scene-correction/levels-knob`); refused even at its old default `0,1` |
| `--display-tone`, `--highlight-compress` | fit range's one operator; its parameter is `--display-tone-headroom` |

### The rendering — `default` or `direct`

`--rendering default|direct` (recipe `rendering`) chooses what the stages start from:

| | `default` | `direct` |
|---|---|---|
| the roll's measurements (`roll`) | applied | left out, and reported so (`chain.roll.*_applied: false`) |
| white balance / base slope | the roll's; without them neutral / ≈1.414, and a warning | neutral / ≈1.414, pinned |
| saturation over the base slope's colour | 1.15 | 1, pinned |
| highlight desaturation | 0.8 | off |
| display black / headroom | 6 / 6 | 6 / 6, pinned |
| destination, axes unset | SDR Display P3 TIFF | HDR 32-bit float Adobe RGB TIFF; a stated gamut keeps the float TIFF in it, and a stated axis that rules it out falls back to the lossless 16-bit TIFF |

`default` is Hanten's picture from what was measured; `direct` loses as little as it
can and applies only what the container needs — the handoff to an editor, and (as
`--rendering direct --range sdr`, the form you can judge by eye) the rendering the
calibration loop holds fixed. A knob you state builds on either: `--white-balance`
multiplies the base gains, and `--contrast` and `--saturation` their base slopes; every
other knob replaces its base value. A stated axis is
never overridden, and `direct` decides its container before its range, so it never
takes a lossy container by default — only when you state one, or when your stated axes
leave no lossless row: `--rendering direct --gamut display-p3` is the Display P3 float
TIFF and `--transfer native` the Adobe RGB 16-bit TIFF. `--container jpeg` or `--range
hdr --transfer native` names a gain map but is refused, asking for its gamut: `direct`'s
Adobe RGB has none, so add `--gamut display-p3` or `--gamut srgb`. The report states
the rendering in `chain.rendering`.

```console
$ hanten convert scan.tif -o d1 --film-base 0.9,0.55,0.42 --rendering direct \
    | jq -c '.output, .chain.destination'
"d1.tiff"
{"display":{"range":"hdr","transfer":"linear","gamut":"adobe-rgb","container":"tiff"}}
```

- **`default` without a roll measurement warns**, since what it renders then is a
  fallback, not a measurement — and that is every plain `convert` until you run
  `measure-roll`:

  ```text
  hanten: warning: no roll measurement: rendered with neutral white balance (no `roll.white_balance`) and the fallback slope 1.413675 (no `roll.white_stops`). Run `hanten measure-roll` over the roll and use the recipe it writes (its `roll` section); or state the white balance you want (`scene_correction.white_balance`), the roll's white and dark end (`roll.white_stops` and `roll.dark_stops`, which set the base slope) and exposure (`roll.exposure`), or choose your own slope (`look.contrast` and `look.saturation` off 1); or use the `direct` rendering (`rendering`: "direct"), the decode without a roll correction, whose unset destination is the HDR float TIFF
  ```

  A typed `--white-balance`, `--contrast` or `--saturation` is a choice — even the
  identity, though each multiplies the fallback — as is a recipe value off the identity.
  `--white-balance` silences its half; the slope's half lasts until both multipliers are
  chosen, naming the one still unchosen (`for tone` or `for colour`). `--strict` fails
  the run on it: without a roll measurement, state `--white-balance`, `--contrast` and `--saturation`, or pass
  `--rendering direct`.
- **A recipe value an earlier build could have written unchosen warns under
  `direct`**: highlight desaturation at exactly 0.8, which every earlier recipe stated
  and which would turn the pull back on, and a stated white balance beside a `roll`
  section, which an earlier `measure-roll` wrote. Every other old default equals
  `direct`'s base. A deliberate value — any other, or a typed flag — never warns, so a
  `--save-recipe` recipe replays under `--strict`, with one carve-out: in a recipe that
  has a `roll` section, a white balance you typed is saved as a recipe value and warns
  on replay, since a file cannot say who chose it — type the flag again on replay to
  keep it without the warning:

  ```console
  $ cat olddirect.json
  {"recipe_version": 3, "rendering": "direct",
   "look": {"highlight_desaturation": {"strength": 0.8, "start_stops": -1.0, "band": [0.015, 0.025]}},
   "fit_range": {"headroom_stops": 6.0, "display_black": 6.0}}
  $ hanten convert scan.tif -o od --film-base 0.9,0.55,0.42 --params olddirect.json --report none
  hanten: warning: the recipe moves the `direct` rendering's pinned base: `look.highlight_desaturation.strength` 0.8 (direct: 0, off; every recipe an earlier build wrote stated 0.8). If the strength came from a recipe an earlier build wrote, set it to `null` so `direct` renders as pinned; a deliberate adjustment is fine — type it as a flag to keep it without this warning
  ```

  A recipe written entirely by an earlier `measure-roll` — its gains in
  `scene_correction.white_balance`, and no `roll` section — does not warn: `direct`
  cannot tell those gains from a deliberate adjustment, and applies them. Move them into
  `roll` before rendering it `direct`. (Its contrast in `look.contrast` is a version 2
  number, refused with its conversion — §5.)
- **`--rendering direct` with the film master is refused** — the film master runs no
  rendering for it to choose:

  ```console
  $ hanten convert … --rendering direct --film-master
  usage: --rendering direct (recipe `rendering`) chooses what the rendering stages start from, and --film-master (recipe `output`: `"film-master"`) runs none: it writes the fixed decode's linear ACEScg. Either pass --rendering default, or choose a rendered destination — `direct` alone writes the HDR float TIFF, the rendered output closest to the decode
  ```

  On `roll` the remedy is the key: set `rendering` to `"default"` (or remove it).
- **The roll flags are refused under `direct`**, which leaves the roll out: drop
  `--roll-white-balance` / `--roll-white` / `--roll-dark` / `--roll-exposure` / `--roll-frame-exposure` /
  `--roll-thin-slope` / `--roll-thin-exposure` / `--roll-midtone-line` / `--small-lift on`
  / `--thin-lift on` / `--midtone-neutral on` / `--neutral-balance on`, or pass
  `--rendering default`. A recipe's `roll` section is not refused, nor is
  `--small-lift off`, `--thin-lift off`, `--midtone-neutral off` or
  `--neutral-balance off`, which ask for nothing (under the film master too).

### Scene correction

The first stage applies white balance and exposure as per-channel gains on linear
ACEScg — after the decode's 3×3, before the look — and clamps nothing. Before the gains
it removes the roll's **midtone cast** when the recipe carries one (`roll.midtone_line`,
measured by `measure-roll`, below); the report's `chain.scene_correction` then also
states the line.

| Flag | Recipe key | |
|---|---|---|
| `--white-balance R,G,B` | `scene_correction.white_balance` = `{"explicit": [r, g, b]}` | stated gains, multiplied into the roll's (default `[1, 1, 1]`) |
| `--exposure EV` | `scene_correction.exposure` | a gain of `2^EV` (default `0`), added to the roll's exposure |

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

The stage between scene correction and fit range. It runs four controls, in this
order: **contrast**, on luminance; **saturation**, on colour; the **per-channel grade**,
which removes (or adds) a cast that grows away from mid-grey; then **highlight
desaturation**, which pulls bright surfaces that are nearly neutral the rest of the way
to neutral, so a white that still carries a trace of cast after the roll's white balance
reads clean. Contrast, saturation and highlight desaturation are **on by default**; the
grade is off.

| Flag | Recipe key | |
|---|---|---|
| `--contrast CONTRAST` | `look.contrast` | a multiplier on the base slope: `1.2` is 20% more contrast, `0.9` flatter; default `1`, which keeps the base; must be positive |
| `--saturation SATURATION` | `look.saturation` | a multiplier on the default colour: `1.2` is 20% more saturated, `0.9` less; default `1`, which keeps the default; must be positive |
| `--channel-grade R,B` | `look.channel_grade` | red and blue exponents pivoted at mid-grey, green fixed at 1; default `1,1` (off); both positive, with the spread over `R,1,B` under 1 |
| `--highlight-desaturation STRENGTH` | `look.highlight_desaturation.strength` | `0`–`1`; unstated, `0.8` (off under `--rendering direct`); `0` is off |
| `--highlight-desaturation-start STOPS` | `look.highlight_desaturation.start_stops` | where the pull begins, in stops below diffuse white (unstated, `-1`) |
| `--highlight-desaturation-band S0,S1` | `look.highlight_desaturation.band` | the saturation band (unstated, `0.015,0.025`) |

- **It only touches near-neutral highlights.** Its strength rises from `start_stops`
  up to diffuse white, and falls to nothing across the band: a pixel whose channels
  differ by more than `S1` (measured as `log10(max/min)` over `--density-gamma` × the
  saturation slope, so the band means the same density spread on a flat roll and a
  saturated one) is left alone. So a sunset, sand or skin keeps its colour; a cast white does not.
- **It assumes the roll's white balance.** "Near-neutral" means near R = G = B, which
  is near white only after `measure-roll`'s gains have removed the roll's cast.
- **Contrast is the slope, pivoted at mid-grey, on luminance**: mid-grey stays put and
  each stop of a pixel's luminance away from it becomes `slope` stops, where slope 1
  reproduces the scene's own contrast. The whole pixel is scaled, so its colour does not
  change with contrast. The slope is a **base** times `--contrast`: the roll's
  (`roll.white_stops` and `roll.dark_stops`, below), a thin frame's (`roll.thin_slope`), else the fallback
  ≈1.414 — the slope a roll whose white sat 1.75 stops above mid-grey would get — and
  under `direct` its pinned ≈1.414. So `--contrast 1.2` is 20% more than the roll's, on
  every roll — the way to carry one taste across rolls. It runs after scene correction,
  so `--exposure 1` at slope 1.41 moves the picture 1.41 stops: exposure is in stops of
  the reconstructed scene.
- **Saturation is the colour's slope**: each pixel's ratios between channels are raised
  to the power `saturation slope` (a channel twice green's becomes 2^s times it) and its
  luminance is kept, so neutrals and the tone scale do not move. The saturation slope is
  the same base slope × 1.15 × `--saturation` — except that a thin frame's slope never
  reaches colour: there the base is the roll's. The 1.15 is the default,
  chosen by review: 15% more colour than the base slope gave when contrast still acted
  on each channel. `direct` uses 1 there instead. Whatever `--contrast` is, colour stays
  put.
- **The grade is for crossover** — a cast that differs between shadows and
  highlights, which one set of white-balance gains cannot remove. Each of red and blue
  becomes `0.18 · (v / 0.18)^R` (or `^B`), and the pixel's luminance is then put back,
  so mid-grey stays neutral, the cast it adds grows with distance from mid in both
  directions, and neutral contrast is untouched — that stays `--contrast`'s. Below 1
  a channel is pulled down in the highlights and up in the shadows; above 1 the
  reverse. It runs after contrast, so the same values act more strongly on a
  contrastier picture. It does not replace the roll's white balance, which should be
  set first, or the decode's calibrated `--density-scale`.
- **Luminance is kept** by saturation, the grade and highlight desaturation; only chroma
  moves. `--highlight-desaturation 0` turns desaturation off — with the grade at `1,1`
  the look is then the two slopes alone, which keep a neutral neutral: the way to see the
  roll's raw cast.
- The report says what ran:

  ```console
  $ hanten convert scan.tif -o out --film-base 0.9,0.55,0.42 \
      | jq -c '{look: .chain.look, stage: .chain.stages[1]}'
  {"look":{"contrast":1.0,"base_slope":1.413675,"base_from":"fallback","saturation":1.0,"saturation_base_slope":1.6257261,"saturation_base_from":"fallback","slope":1.413675,"saturation_slope":1.6257261,"channel_grade":[1.0,1.0],"highlight_desaturation":{"strength":0.8,"start_stops":-1.0,"band":[0.015,0.025]}},"stage":{"stage":"look","applied":"contrast+saturation+highlight-desaturation"}}
  ```

  `base_from` is `roll`, `thin`, `fallback` or `direct`; `slope` is `base_slope` ×
  `contrast`; `saturation_base_slope` is the colour's base (× 1.15 under `default`),
  `saturation_base_from` where its slope came from (a thin frame's colour is the white's,
  `roll`, or `fallback`), and `saturation_slope` is it × `saturation`.

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
    "film_base_stops": 4.9630156,
    "shift_stops": -1.0369847
  }
}
```

At `0` the operator reads `"identity"`: reinhard passes the scene through unchanged,
and everything above display white clips at the encode (36% of the samples on
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
default contrast the base renders 4.96 stops under mid-grey (above), which reads as a
lifted black. At the default `6` (about L\* 2.5) it is moved down by the 1.04 stops
`shift_stops` reports.

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
  at the default contrast the warning starts at about `--exposure 2.15`. Closer than 2
  stops under mid-grey, the whole shift is squeezed into that narrow band and crushes
  the shadows; at or above mid-grey (about `--exposure 3.7`) black cannot place it and
  is skipped. Either way the report warns, naming
  `--exposure` and `--display-black off` as the remedies.
- **Not the removed `--black-point`**, a subtraction on every channel that crushed
  and tinted the shadows. There is no scene-side subtraction either: base fog is
  already in the measured film base, and scanner veil is a highlight and
  scanner-calibration question (`nf-scene-correction/flare-removal`).

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

### `measure-roll` — a roll's white balance, midtone line, white and exposure, measured once

It measures what a whole roll shares. The **white balance** removes the cast of
the film, the development and the scanner, and keeps the scene's light: one sunset
frame barely moves a statistic taken over every frame. The **midtone line** removes the
cast a poor development leaves in the midtones, which the white balance — exact only at
the roll's brightest percent — cannot (below). The **roll's white and dark
end** set the look's base slope, which spreads the span between them over a fixed range
with mid-grey held where it is.
The **roll's exposure** brings an under- or over-exposed roll to a normal level.
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
                "unusable": 0, "white_stops": 0.64702564,
                "decoded_white_stops": 0.52260643,
                "leader_distance_stops": 2.0644333, "level_stops": -1.7618053,
                "white_role": "under", "spread_stops": 3.7179117,
                "dark_stops": -3.5714598, "base_share": 0.38004303, "lift_ev": 0.0, … }, … ],
  "white_balance": { "gains": [1.0026785, 1.0, 1.2466215], "percentile": 0.99, … },
  "midtone_neutral": { "kind": "correction", "mode": "auto",
                       "line": { "red": [-0.023591928, -0.024393905],
                                 "blue": [-0.044650972, -0.06559556],
                                 "bands": [-2.75, 2.25], "fade_end_stops": 1.114628 },
                       "frames": 35, "bands": [ … ], "off_because": null,
                       "min_frames": 10, "min_bands": 3, "fade_stops": 1.0,
                       "gate_log2": [0.6, 0.9] },
  "confidence": { "white_balance": { "tier": "confident", "frames": 35, "confident_from": 16 },
                  "midtone_neutral": { "tier": "confident", "frames": 35,
                                       "confident_from": 16 } },
  "white": { "stops": 1.5, "bound": "floor", "dark_stops": -3.5391304, "slope": 1.7957463,
             "clamped": [ { "input": "frames/1815.tif", "white_stops": 2.2324193,
                            "slope": 1.63365,
                            "flag": "--roll-white-balance 1.0026785,1,1.2466215 --roll-white 2 --roll-dark -3.5391304 --roll-exposure 1.0553794 --roll-midtone-line -0.023591928,-0.024393905,-0.044650972,-0.06559556,-2.75,2.25,1.114628" }, … ],
             "rule": { "channel": "max", "percentile": 0.97, "cap_stops": 2.0,
                       "floor_stops": 1.5, "saturation_margin_stops": 0.5 } },
  "exposure": { "ev": 1.0553794, "level_stops": -1.0553794, "bounded": false,
                "target_stops": 0.0, "bound_ev": 3.0 },
  "small_lift": { "kind": "taste", "written": true, "lifted": 17, "bound_ev": 0.3,
                  "full_stops": 0.9, "none_stops": 1.5, "flat_spread_stops": 1.0 },
  "thin_lift": { "kind": "taste", "written": true, "lifted": 0, "base_stops": -3.707272,
                 "white_stops": 0.9,
                 "base_share": 0.3, "near_base_stops": 1.0, "lift_stops": 1.0,
                 "slope_bound": 2.4 },
  "reuse": { "flag": "--roll-white-balance 1.0026785,1,1.2466215 --roll-white 1.5 --roll-dark -3.5391304 --roll-exposure 1.0553794 --roll-midtone-line -0.023591928,-0.024393905,-0.044650972,-0.06559556,-2.75,2.25,1.114628" },
  "warnings": [ "frames/1816.tif: near film saturation — its white sits 0.29 stop under the leader (margin 0.5 stop); …", … ]
}
```

(Abridged; that is 35 frames of one roll.) Each frame is decoded under the fixed
decode — at its linearization, before the look's slope — and sampled over its
**effective area** (§9). Freeze the result with `--out roll.json`, which writes the
base, the `roll` section, its clamps and its lifts as one recipe for `roll --params` or
`convert --params` (§4) — or paste `reuse.flag` on `convert`. A clamped or lifted frame
converted by flag takes its own `frames[].flag` instead: `reuse.flag` carries the roll's
white and no lift. (By `--params`, the file's `roll.frames` entry does that for you.)

The recipe keeps these in its **`roll` section**, apart from the style knobs, so a
measured value is never mistaken for a chosen one:

| Flag | Recipe key | |
|---|---|---|
| `--roll-white-balance R,G,B` | `roll.white_balance` | the roll's gains; `--white-balance` multiplies them |
| `--roll-white STOPS` | `roll.white_stops` | the roll's white, in scene stops above mid-grey |
| `--roll-dark STOPS` | `roll.dark_stops` | the roll's dark end, in scene stops from mid-grey. With the white it sets the look's base slope, which spreads the span between them over 9.049 stops (below), and `--contrast` multiplies; the colour's base slope too, which `--saturation` multiplies. The two are stated together: one without the other is refused (exit 2) wherever the roll applies. Either, typed or in a `roll` manifest's `params`, drops a frame's thin lift (slope and exposure) and keeps its small lift; a later `--params` layer's does not |
| `--roll-exposure EV` | `roll.exposure` | the roll's exposure, a neutral gain of `2^EV`; `--exposure` adds to it |
| `--roll-frame-exposure EV` | `roll.frame_exposure` | this frame's small lift, added to the roll's exposure: the lift `measure-roll` writes for a low-key frame. A thin lift replaces it |
| `--small-lift on\|off` | `roll.small_lift` | whether the small lift applies |
| `--roll-thin-slope SLOPE` | `roll.thin_slope` | a thin frame's base slope, in place of the roll's (`--contrast` multiplies it) |
| `--roll-thin-exposure EV` | `roll.thin_exposure` | the exposure solved with the thin slope, added to the roll's in place of the small lift; refused without `roll.thin_slope` (exit 2) |
| `--thin-lift on\|off` | `roll.thin_lift` | whether the thin lift applies; off, the frame renders its small lift |
| `--roll-midtone-line RS,RO,BS,BO,LO,HI,END` | `roll.midtone_line` = `{"red": [RS, RO], "blue": [BS, BO], "bands": [LO, HI], "fade_end_stops": END}` | the roll's midtone line (below); refused without `roll.white_balance` (exit 2) |
| `--midtone-neutral on\|off` | `roll.midtone_neutral` | whether the midtone line applies; off keeps it in the recipe |
| `--neutral-balance on\|off` | `roll.neutral_balance` | whether the roll's white balance applies; off keeps the gains in the recipe and turns the midtone line off with them, since it is measured after them. A typed `--midtone-neutral on` against a typed or recipe off is refused (exit 2); a recipe's `"on"` is spared, and a `roll` manifest frame's own off wins over the flag |
| — | `roll.frames` | `{"<file name>": {"white_stops": …, "exposure": …, "thin_slope": …, "thin_exposure": …}}`, any `null`: a frame's own white, in place of the roll's, and its lifts. `convert` and `roll` move their input's entry into `roll.white_stops`, `roll.frame_exposure`, `roll.thin_slope` and `roll.thin_exposure`, before any flag; a `roll --frames` manifest's `params` beat it, and may not state the table. Keys are file names, not paths (exit 2) |

A lift is one frame's: `roll` refuses `roll.frame_exposure`, `roll.thin_slope` or
`roll.thin_exposure` in the shared recipe or as a flag (exit 2), since it would lift every
frame alike; per frame it goes in `roll.frames` or a manifest's `params`. A typed
`--roll-frame-exposure` (or a manifest's `roll.frame_exposure`) on a frame whose thin lift
applies from the recipe is refused (exit 2), since the thin lift replaces it; stated with a
thin value, as in a frame's `flag`, it is the `--thin-lift off` fallback. Likewise a typed
`--roll-thin-slope` (or a manifest's `roll.thin_slope`) with no thin exposure, over a
small lift, is refused (exit 2): it would drop that lift's exposure, so the message names
it — add `--roll-thin-exposure <that value>` to keep it, or `--small-lift off` to drop
it. Each switch is
unset (`null`) by default, which is on, so a measured file layered last never undoes an
earlier `"off"`; `off` keeps the lift in the recipe.

Before 2026-10-02 a thin frame's lift was `roll.frame_slope` (`--roll-frame-slope`) or an
entry's `slope`, with the exposure beside it. At `null`, as every file then wrote them,
they are dropped; any other value is refused (exit 2), naming the rename that renders as
before — the slope to `thin_slope` and the exposure that rendered beside it (a frame's
`roll.frames` entry `exposure` where it has one) to `thin_exposure` — or re-run
`measure-roll --out`. The old switch, `--frame-lift` / `roll.frame_lift`, and
`measure-roll --no-frame-lift` switched both lifts; they are refused too (a `null` key
is dropped), naming both switches: `--frame-lift off` → `--small-lift off --thin-lift
off`, `--frame-lift on` → `--small-lift on --thin-lift on` (it beat a recipe's `off`), the
keys likewise, `--no-frame-lift` → `--no-small-lift --no-thin-lift`. One old key is
named per run (a recipe with several old thin entries takes a run per entry); a retired
slope's message also migrates the switch beside it.

Each is optional. The report says what applied:

```console
$ hanten convert scan.tif -o out --film-base 0.9,0.55,0.42 \
    --roll-white-balance 1.1,1,0.9 --roll-white 1.7 --roll-dark -3.75 --roll-exposure 0.4 \
    --white-balance 1.2,1,1 --exposure 0.2 | jq -c '.chain.roll, .chain.scene_correction'
{"white_balance":[1.1,1.0,0.9],"white_stops":1.7,"dark_stops":-3.75,"slope":1.660367,"exposure":0.4,"frame_exposure":null,"thin_slope":null,"thin_exposure":null,"white_balance_applied":true,"slope_applied":true,"exposure_applied":true,"frame_exposure_applied":false,"thin_lift_applied":false,"midtone_line":null,"midtone_neutral_applied":false,"taste_applied":[]}
{"white_balance":[1.32,1.0,0.9],"exposure":0.6}
```

With `--roll-midtone-line` too, `chain.roll` states the line with
`"midtone_neutral_applied": true`, `chain.scene_correction` carries it as `midtone_line`,
and the stage list reads `midtone-neutral+white-balance+exposure`.

`slope_applied` stays `true` with `--contrast` stated: the contrast multiplies the
roll's slope (`chain.look.base_from` is `roll`). The film master and `direct` apply
none of the values, and report each as not applied. The roll flags are
refused under it — a typed `--film-master` conflicts with them, and under a recipe's
`"film-master"` the refusal says to drop them or choose a rendered destination — but a
recipe's `roll` section is not.

A white balance **a recipe states** beside the roll's gains is kept, and the run warns
(so `--strict` refuses it): it may be gains an earlier `measure-roll` wrote there, not
a choice. An exposure a recipe states beside the roll's does the same, since it may be
one chosen by hand before the roll's was measured:

```console
$ hanten convert scan.tif -o out --film-base 0.9,0.55,0.42 --params old-exp.json --report none
hanten: warning: the recipe's `scene_correction.exposure` 1.4 adds to the roll's exposure, `roll.exposure` 0.4: the exposure applied is 1.8 EV. A stated exposure is an adjustment on top of the roll's measurement; if it is one chosen by hand before the roll's was measured, drop it
```

A typed `--white-balance` or `--exposure` is a choice made now, and never warns. A
`--save-recipe` recipe that states one beside the roll's gains does warn on replay,
since a file cannot say who chose a value; type the flag on replay to keep it without
the warning. A contrast beside the roll's white never warns: it multiplies the roll's
slope, which is what it is for. `roll` warns once, for the shared recipe, not per
frame:

```console
$ cat old.json
{"recipe_version": 3,
 "roll": {"white_balance": [1.1, 1.0, 0.9], "white_stops": 1.7},
 "scene_correction": {"white_balance": {"explicit": [1.2, 1.0, 1.0]}}}
$ hanten convert scan.tif -o out --film-base 0.9,0.55,0.42 --params old.json --report none
hanten: warning: the recipe's `scene_correction.white_balance` [1.2, 1.0, 1.0] multiplies the roll's gains, `roll.white_balance` [1.1, 1.0, 0.9]: the white balance applied is [1.32, 1.0, 0.9]. A stated white balance is an adjustment on top of the roll's measurement; if it holds gains an earlier `hanten measure-roll` wrote there, drop it — they now live in `roll.white_balance`
```

**Adopting the `roll` section in a recipe an earlier build wrote:** drop
`scene_correction.white_balance` if it holds `measure-roll`'s old output. No value is
read as unset for you: a `--save-recipe` recipe replays exactly what it rendered. (A
version 2 recipe's `look.contrast` is refused with its conversion — §5.)

**The white balance** equalizes the pooled pixels' per-channel 99th percentile,
green-anchored.

**The midtone line** is the cast left after the white balance, as a line against scene
brightness. Per half-stop band, each frame votes the most common colour of its pixels
there; the roll's value per band is the median vote, and a line through the bands
weighted by frames voting gives red and blue a correction. At render it applies in full
up to a stop below `fade_end_stops`, fades to nothing there, stays flat outside `bands`,
and spares strongly coloured light (a pixel 0.6 → 0.9 log2 off the line's cast, such as
a sunset-lit cloud) — the **tint gate**. On a roll from an exhausted developer it is
the difference between violet midtones and neutral ones; on a well-developed roll it is
small. It is written only on a roll of **10 frames or more** with at least 3 bands voted
by 3 frames: `off_because` says `too-few-frames` or `too-few-bands` otherwise, and `asked`
(no band measured) under `--midtone-neutral off`. `--midtone-neutral on` writes it on a
shorter roll too, of 3 frames or more, if enough bands count. A roll dominated by one
scene colour (sand, sea) can read that colour as the cast; re-run `measure-roll
--midtone-neutral off`, which also measures the whites without the line, and use its
recipe alone: a `null` in a later `--params` layer does not clear an earlier line. At
render, `--midtone-neutral off` (recipe `roll.midtone_neutral` `"off"`) drops only the
line and keeps it in the recipe: the whites stay as measured after it.

**How far to trust them.** `confidence` grades the white balance and the line by how many
frames they were measured from: `confident` from 16 frames (`confident_from`), else
`in-doubt`, with one note (`confidence.advice`, and a `hanten: note:` line). It is advice, not
a warning, so `--strict` ignores it: a whole short roll has no more frames to give. A random 10 frames
of a roll moved its white balance as much as the cast a whole roll leaves on its neutral
patches; 16 frames, within it. Nothing detects a roll dominated by one colour (sand,
sea), which misleads the white balance most: compare a frame with `--neutral-balance off`
(recipe `roll.neutral_balance` `"off"`), which keeps the gains in the recipe and turns
the line off with them; the whites stay as measured after both.

**The roll's white** is measured per frame, in **scene stops** above mid-grey: each
frame's `white_stops` is the 97th percentile of its pixels' **brightest channel**, after
the roll's white balance and midtone line, in the decode's film RGB (before the
working-space matrix), which leaves speculars above white. `decoded_white_stops` is the
same before that correction. The brightest channel
rather than red, so a blue sky or a green-lit highlight reads as bright as it is. The roll's white is the brightest frame
white at or under the **cap** (+2.0), raised to at least the **floor** (+1.5); `bound`
says which limit set it (`none`, `floor`, or `cap` when every frame is above it).

**The roll's dark end** is measured per frame too: each frame's `dark_stops` is the 1st
percentile of its luma in linear ACEScg, in scene stops, and the roll's
(`white.dark_stops`) is the 10th percentile of those over its picture frames, so its
darkest content sets it but not its single darkest frame. `slope` is the look's slope at
`--contrast 1`: it spreads the span from the dark end to the white over 9.049 stops,
`9.049 / (stops − dark_stops)`, with mid-grey pinned, held between 1.237 (the cap's)
and 3.5 (`nf-calibration/span-roll-slope`); a roll with no span — its dark end at or
above its white — takes 3.5. On the eleven archive rolls it measures 1.56–1.74. The
recipe stores the two ends (`roll.white_stops`, `roll.dark_stops`), and the slope is
derived from them at render time. The white is not placed at a target: it renders where
the slope and the roll's exposure put it, L\* 91–98 on those rolls (SDR); the dark end
renders where display black puts it.

- **A frame above the cap is clamped**, not counted: it renders at the cap's span to
  the roll's dark end (`white.clamped`, beside the roll's `slope`), which is gentler;
  `--out` records the cap as its white in `roll.frames`, and its own `flag` is for
  `convert`. An ordinary bright scene lands here too, so this is reported, not warned
  about.
- **A frame near its leader warns.** A white within 0.5 stop of the leader
  (`leader_distance_stops`, from `decoded_white_stops`: saturation is the film's, before any correction) is near film saturation, where the film compresses
  highlights and the decode renders them flat. Without `--leader` nothing is checked.
- **The white and dark end are measured at exposure 0.** `measure-roll` does not read
  the recipe's `scene_correction.exposure` or `look.contrast`. The look's slope expands
  an exposure too: with the roll's exposure plus `--exposure` at 0.5 and slope 1.65, the
  white renders `1.65 · (stops + 0.5)` stops above mid-grey. That is an exposure doing
  its job, not a mismeasured white.
- A recipe written before `pipeline_version` 11 states the white alone, which placed it
  at diffuse white; it is refused (exit 2), naming `measure-roll` to re-run or the
  `--roll-dark` to add. `direct` and the film master apply no roll, so they spare it.
- The cap, floor and margin are provisional: they were chosen by review on nine rolls
  with no deliberately bad frames, on whites measured before the colour correction. So
  are the span's 9.049 stops and the 3.5 maximum (`nf-calibration/hybrid-slope-bounds`).

**The roll's exposure** is one neutral gain for the whole roll. Each frame's
`level_stops` is the log-average of its luma in linear ACEScg, in scene stops from
mid-grey, over the pixels with positive luma; the exposure (`exposure.ev`) brings the
**median** frame level to mid-grey (`target_stops` 0), where a light meter would put it, so one night scene or one bright frame does not set it. A frame darker than its roll stays
dark, beyond a small lift (below) — it renders what is on the film. The exposure is limited to ±3 EV (`bound_ev`); a
roll that needs more warns (`bounded: true`), and `measure-roll --strict` refuses it.
That usually means wrong inputs: check they are this roll's picture frames under its
film base. If the roll really is that far off, add `--exposure` when converting
(`convert`, `roll`), which adds to `roll.exposure`. The target was chosen by review over
−0.6 stop, which left midtones darker than SilverFast's; it measures +0.62 to +2.34 EV on
the eleven archive rolls.

**The roll's white balance, midtone line, exposure and white are corrections; the two
lifts below are preferences** (taste). A correction restores what the roll recorded and
has no switch — except the white balance and the midtone line, which can misread a roll's
scene as its cast.
A preference is on by default because review preferred it, and each has its own off
switch at render that keeps its value in the recipe, so a frame or a roll can be
compared with and without it and turned back on. The report's `chain.roll.taste_applied`
names the preferences applied, by their switch's key; `measure-roll`'s `small_lift` and
`thin_lift` sections say `"kind": "taste"`.

**A low-key frame gets a small lift** on top of the roll's exposure (`lift_ev`), keyed on
where its white renders after that exposure (its `white_stops` plus `exposure.ev`): the
whole +0.3 EV at or under +0.9 stop (`full_stops`), none at or over +1.5 (`none_stops`),
linear between. A bright frame is never darkened. `--out` writes each non-zero lift to
`roll.frames`, and `small_lift.lifted` counts them. Review preferred the lift on 72 of the
101 frames it gave on ten rolls and its absence on 7 (two of those 7 were flat frames,
which are no longer lifted; below), judged at a target 0.6 stop darker; at mid-grey 45 of
those 178 frames are lifted.
Turn it off when measuring (`measure-roll --no-small-lift`: lifts are reported, not
written) or when rendering, without re-measuring — the lift stays in the recipe:

```console
$ hanten convert scan.tif -o out --film-base 0.9,0.55,0.42 --roll-exposure 0.4 \
    --roll-frame-exposure 0.25 --small-lift off | jq -c '.chain.roll | {exposure, frame_exposure, exposure_applied, frame_exposure_applied, taste_applied}'
{"exposure":0.4,"frame_exposure":0.25,"exposure_applied":true,"frame_exposure_applied":false,"taste_applied":[]}
```

With the lift on, the exposure applied is the sum (`chain.scene_correction.exposure`
0.65). `roll.small_lift` is the same switch in a recipe, so a `--frames` manifest can
turn one frame's lift off.

**A flat frame gets no lift**: one whose luma spans under a stop from its p5 to its p95
(`spread_stops`, against `small_lift.flat_spread_stops`) — one surface filling the frame,
like a close-up of water — reports `"flat": true` and `lift_ev` 0. On the ten reviewed
rolls two frames were flat, and review preferred no lift on both.

**A thin frame gets a bigger lift** in place of the small one. A frame
qualifies when its white renders at or under +0.9 stop after the roll's exposure
(`thin_lift.white_stops`) and at least 30% of its luma sits within a stop of the film
base (`base_share`, `thin_lift.base_share`): its shadows are on the base. That holds for
an underexposed frame and for a night scene alike. Its white then rises about a stop
(`thin_lift.lift_stops`) with the film base held about where its small lift renders it
(the pair is solved from that render) — a steeper slope and the exposure that keeps the
base still, in place of the small lift.
"About": the white is read in film RGB, the base as luma before the roll's gains. The
slope is bounded at 2.4 (`slope_bound`): grain rises with it. A frame the bound holds
under a full stop is listed in `thin_lift.bounded`, and a thin frame no thin lift fits —
it has its small lift only — in `thin_lift.unlifted`; disclosed, not warned about, so
`--strict` passes. Review found the lift brighter, not better: it is on because viewers
tend to like brighter, so it is the preference most worth trying off.
Each qualifying frame's lift is reported as `frames[].thin_lift` (`slope`, `exposure`,
`lift_stops`, `bounded`); it is written as the frame's `roll.frames` `thin_slope` and
`thin_exposure`, beside its small lift, and `thin_lift.lifted` counts them (5 of 178 on
the ten rolls). `measure-roll --no-thin-lift` leaves it out; `--no-small-lift` writes it
alone, unchanged. `--thin-lift off` turns it off at render, and the frame renders its
small lift; `--small-lift off` leaves it alone:

```console
$ hanten convert scan.tif -o out --film-base 0.9,0.55,0.42 --roll-white 1.5 --roll-dark -3.75 \
    --roll-exposure 0.4 --roll-thin-slope 2.0 --roll-thin-exposure 0.7 \
    | jq -c '[(.chain.roll | {slope, thin_lift_applied, frame_exposure_applied, taste_applied}), .chain.scene_correction.exposure, .chain.look.base_from]'
[{"slope":2.0,"thin_lift_applied":true,"frame_exposure_applied":false,"taste_applied":["thin_lift"]},1.1,"thin"]
$ hanten convert scan.tif -o out --film-base 0.9,0.55,0.42 --roll-white 1.5 --roll-dark -3.75 \
    --roll-exposure 0.4 --roll-frame-exposure 0.3 --roll-thin-slope 2.0 --roll-thin-exposure 0.7 \
    --thin-lift off \
    | jq -c '[(.chain.roll | {slope, thin_lift_applied, frame_exposure_applied, taste_applied}), .chain.scene_correction.exposure, .chain.look.base_from]'
[{"slope":1.723619,"thin_lift_applied":false,"frame_exposure_applied":true,"taste_applied":["small_lift"]},0.70000005,"roll"]
```

A roll section with gains, or a white and dark end, but no `roll.exposure` — a `roll.json` written
before `measure-roll` measured the exposure, or roll flags typed without
`--roll-exposure` — renders at exposure 0 and warns (so `--strict` refuses it). Re-run
`measure-roll`, or state `roll.exposure` / `--roll-exposure` (`0` keeps the render):

```console
$ hanten convert scan.tif -o out --film-base 0.9,0.55,0.42 --params before.json --report none
hanten: warning: the roll section has no `roll.exposure`, so the roll renders at exposure 0 and an under-exposed roll stays dark. Run `hanten measure-roll` over the roll (a `roll.json` written before it measured the exposure has none), or state `roll.exposure` / `--roll-exposure` (0 keeps this render)
```

- **Pass the leader.** Any pixel within 0.1 density of it is left out of the white
  balance (`guarded`), so a fully exposed frame mixed into the inputs cannot set the
  gains — measured, it would move them 0.4–1.3 stops — and a frame it empties is left out
  of the exposure. A frame's white is measured before that guard, so a frame near saturation still shows it; the cap keeps it from
  raising the roll's white. Without `--leader` the run warns, no frame is checked for
  saturation, and `--strict` refuses it before decoding anything (exit 2).
- **Only picture frames, each once.** Every input is pooled as picture; leave out the
  unexposed base, the leader and any calibration frame, or pass the leader and unexposed
  frame with them and use `auto` (§4). A frame named twice is
  refused (exit 2) — it would weigh double — and so are the `--leader` and
  `--unexposed` files among the inputs. With `--out`, two inputs sharing a file name are
  refused too, since `roll.frames` keys by it. A frame that contributes nothing, or whose
  holder cut is not a measurement (the same warning `convert` gives), is warned about
  by name, so `--strict` catches both.
- **The base is measured or explicit** — `--unexposed`, or `--film-base`, or
  `calibration.film_base` in a `--params` recipe (a versioned one, whose
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
| `sdr` *(default)* | `native` | `display-p3` *(default)* / `adobe-rgb` / `srgb` | `tiff` | `.tif` / **`.tiff`** | 16-bit TIFF in the gamut's own curve (sRGB curve / `563/256` / sRGB curve) |
| `hdr` | `linear` | `display-p3` / `adobe-rgb` / `srgb` / `bt2020` | `tiff` | `.tif` / **`.tiff`** | 32-bit float display-linear TIFF (1.0 = 203 cd/m²) |
| `hdr` | `pq` / `hlg` | `bt2020` | `tiff` | `.tif` / **`.tiff`** | Rec.2100 signal as full-range 16-bit TIFF codes |
| `hdr` | `native` | `display-p3` / `srgb` | `jpeg` | **`.jpg`** / `.jpeg` | gain-map JPEG: an 8-bit SDR base with a per-channel ISO 21496-1 gain map |

With no destination flag the result is an **SDR Display P3 16-bit TIFF** under the
default rendering, and the HDR float TIFF under `--rendering direct` (§7). An SDR JPEG
(`--container jpeg` alone, or with `--gamut srgb`) is planned (`output/sdr-jpeg-preset`),
and refused as not written yet; ProPhoto has no destination.

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
`--exposure 0 --white-balance 1,1,1`, the look's `--contrast 1 --saturation 1
--channel-grade 1,1 --highlight-desaturation 0`, and `--display-tone-headroom 0 --display-black off` (or
their defaults) — which is how a flag clears a recipe's stage for a master. The film
master and a destination axis are one choice, refused at the parser.

**`--export-film-rgb PATH`** (beside any destination) also writes the
fixed decode *before* the NC film RGB v1 3×3 into ACEScg — the dye layers' values,
unmixed, which is what a per-channel decode measurement wants. It is a 32-bit float
TIFF with **no ICC profile**: the channels have no primaries, so a viewer shows it
untagged. Measure it with `nctool metrics --space film-rgb`, whose `channels` block
compares field for field with a film master's measured with `--channels`. The report names it in
`film_rgb_exported`. Through the pinned 3×3 it is the film master to the bit.

On `roll` the flag takes **no path**: each frame writes `<input-stem>_film-rgb.tiff`
beside its own output (an explicit `--frames` output's folder included), byte for byte
what that frame's `convert --export-film-rgb` writes, and its report entry names it in
`film_rgb_exported`. Put the flag after the inputs; a path after it is refused (exit 2)
rather than read as another scan. An export name that collides with any output, input
or other export — case-insensitively — is refused before any frame renders:

```
usage: output for scans/f02.tif (out/f01_film-rgb.tiff) collides with film RGB export
       for scans/f01.tif: a frame's film RGB export is <input-stem>_film-rgb.tiff in
       its output's folder, so rename that output, or give the two frames' outputs
       different folders
```

Renaming `f02`'s output is the simple fix. A manifest that lists one scan twice
collides on the two exports whatever its outputs are named, so only folders separate
them (`"one/a.tiff"`, `"two/b.tiff"`); a frame whose own output takes its export's
name is told to rename that output. A clash with an input or `--report-file` names
no export remedy.

**`--export-pre-encode PATH`** (`convert` only, beside any destination) also writes
what the destination's encoder receives — the linear rendition before any transfer,
and a gain map's codes before its JPEG — as an untagged TIFF with one page per buffer. Each page's `ImageDescription` is a JSON
object naming its `buffer` and `space`:

| Destination | Pages |
|---|---|
| `--film-master` | `film-master` (ACEScg, as written) |
| SDR TIFF | `sdr-linear` (linear, in the destination gamut, 1.0 = display white) |
| HDR linear, PQ or HLG | `hdr-linear` (linear, 1.0 = the 203 cd/m² reference white, clamped to the peak) |
| gain-map JPEG | `sdr-linear`, `hdr-linear`, then `gain-map-codes` (the 8-bit, half-resolution map before its JPEG) |

The report names it in `pre_encode_exported`. It is the reference `nctool acceptance`
checks an independent decode of the output against; the output itself is unchanged.

**The gain-map JPEG** renders one graded image twice — an SDR base and an HDR
rendition clamped to the 1000 cd/m² peak — and stores the per-channel ratio between
them as a half-resolution, three-channel gain map. It carries **ISO 21496-1 metadata
only**, in a Multi-Picture Format container Hanten writes itself: no Ultra HDR v1 XMP,
which cannot describe a per-channel map. Apple ImageIO reads it as HDR on either base;
a reader that
knows only the Ultra HDR v1 XMP, or no gain maps at all, shows the SDR base.

### Leave an axis unset and it is derived

In the order range, transfer, gamut, container (under `--rendering direct`, the
container first): its default when a destination fits,
else the one value left, else a refusal listing the choices. So `--range hdr` alone is
the Display P3 gain-map JPEG, `--transfer pq` alone an HDR BT.2020 TIFF and `--gamut
adobe-rgb` alone the Adobe RGB TIFF, while `--gamut bt2020` asks which transfer.
`--transfer linear` alone asks for the gamut: the float TIFF is written in four, and
the gamut default would quietly pick Display P3 (under `--rendering direct` it is
Adobe RGB). A value you **state** is never overridden — a combination the table lacks
is refused, naming the conflicting pair and a flag that fixes it:

```
usage: no destination combines --range sdr and --gamut bt2020 (recipe keys
       `output.display.range`, `.transfer`, `.gamut`, `.container`). Use --range hdr
       with --transfer linear|pq|hlg, --gamut display-p3, --gamut adobe-rgb, or
       --gamut srgb
```

The report records every resolved axis in `chain.destination`
(`{"display": {"range": "hdr", "transfer": "pq", "gamut": "bt2020", "container":
"tiff"}}`, or `"film-master"`), which is exactly the recipe `output` that replays it.

**AVIF was removed** ([`design/avif-removal.md`](design/avif-removal.md)). `--container
avif`, the recipe's `"container": "avif"` and an `.avif` output path are each refused
(exit 2), naming the 16-bit PQ/HLG TIFF that carries the same signal and the gain-map
JPEG as the compact HDR file.

### HDR destinations

An HDR destination clamps its rendition to the 1000 cd/m² peak and counts what that
clamped in `chain.peak_clamp` and in `loss`, where `--strict` sees it. Each also
fills a block stating what the encoder wrote and the luminance anchors no container
can carry — `hdr_coded_tiff` for the PQ/HLG TIFFs, `hdr_linear_tiff` for the float
TIFF, whose `pixel_contract`, `linear_domain` and embedded profile name its gamut (in
a `roll` report, on each frame):

```console
$ hanten convert scan.tif -o out --film-base … --transfer pq | jq -c '.hdr_coded_tiff | {reference_white_nits,target_peak_nits,tone_curve,cicp,max_cll_nits,max_fall_nits}'
{"reference_white_nits":203.0,"target_peak_nits":1000.0,"tone_curve":"reinhard-peak-lifted-v1+log-shift-to-mid-grey-v1","cicp":[9,16,0],"max_cll_nits":608,"max_fall_nits":167}
```

`max_cll_nits` and `max_fall_nits` are the frame's measured MaxCLL and MaxFALL as
CTA-861.3 defines them: the peak and the mean of each pixel's **largest** linear
channel in cd/m², not its luminance, so a saturated highlight counts at its brightest
channel. They are measured in the file's own primaries, so one frame's values differ
by gamut. The HLG TIFF omits them: HLG is a relative signal whose
peak the display decides. Builds before
`output/content-light-levels` reported luminance under the same names.

A TIFF whose MaxCLL is at or below reference white is warned
about (`HDR output carries an SDR-range signal`), naming `--exposure` and `--range sdr`
as the remedies. The gain-map JPEG is not: an SDR-range frame makes a **flat** gain
map, which `chain.gain_map.flat` states and which is a correct file (it displays as
its base), so `--strict` passes it. Its `loss` counts both renditions — the SDR base's
clip and the HDR rendition's clamp — over both renditions' samples. `chain.gain_map`
also carries the per-channel gain range at full resolution (`min`, `max`, linear), the
stored map's `width` and `height`, and `base_fit_range` — fit range as the SDR base ran
it, since `chain.fit_range` is the HDR rendition's:

```console
$ hanten convert tests/fixtures/hdr-48bit.tif -o out --film-base 0.9,0.55,0.42 --range hdr \
    | jq -c '.chain.gain_map | {min, max, flat, width, height}'
{"min":[0.9435548,0.9725493,1.0],"max":[3.0689256,3.0031552,2.95945],"flat":false,"width":251,"height":231}
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
  `roll-1.2.tiff`. Only `.tif`, `.tiff`, `.jpg` and `.jpeg` are read as a
  container request, and `.avif` is refused (AVIF was removed) rather than read as a
  stem.
- **A path that names a directory is refused**, before anything renders and whatever
  its suffix: `-o positives/`, `-o positives/.` and `-o out.tiff/` all exit 2 rather
  than writing `positives.tiff` beside the directory or failing at the write. Name the file inside it
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
       --container jpeg; --range hdr --gamut srgb --container jpeg
```

With a recipe stating `"gamut": "adobe-rgb"`, each offer for `-o out.jpg` restates
the gamut (`--range hdr --gamut display-p3 --container jpeg; --range hdr --gamut srgb
--container jpeg`). With a typed `--film-master` the offer says to drop it first; a
recipe's `"film-master"` is replaced by the offered flags themselves. A `roll` frame's
refusal names the recipe keys (`output.display.…`) instead of flags. With `-v`,
`hanten` says on stderr when it completed a path.

### `roll` and the destination

`roll` takes the destination flags, as `convert` does, over the recipes' `output`.
It derives `<stem>_positive.<ext>` from each frame's own destination, so a default
roll writes `_positive.tiff` and one with `--range hdr` (or
`"output": {"display": {"range": "hdr"}}`) writes `_positive.jpg`.

### The output presets — removed

`--output-preset` (and the recipe key `output.preset`) retired with the chain its
presets named, and with them `--out-depth`, `--output-hdr`, `--output-sdr`,
`--output-profile` and `--bigtiff`: every destination resolves its own depth and
profile, and BigTIFF is decided automatically. Each is refused; `--output-preset`
names the preset's counterpart where one exists:

```
usage: --output-preset was removed with the chain its presets named: a destination is
       four separate knobs — --range, --transfer, --gamut, --container (recipe
       `output.display`) — or --film-master. For `hdr-pq`, the nearest is --transfer
       pq: the same Rec.2100 signal as a 16-bit TIFF, since Hanten no longer writes
       AVIF (`docs/design/avif-removal.md`). There is no alias.
```

Each named set resolves to the same destination under either rendering (so
`display-p3` names `--range sdr --gamut display-p3` rather than "the default", and
`compatibility` `--range sdr --gamut srgb`). `gain-map-hdr` and `ultra-hdr-v1`'s nearest
is `--range hdr --gamut display-p3 --container jpeg`, a different file (a per-channel,
ISO-only map). To reproduce a preset's render, use the reference build.

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

**SilverFast positive mode.** A negative scanned in SilverFast's positive mode (XMP
`Negative=No`, often with the scanner's IT8 profile embedded) converts like one scanned
in negative mode: in HDR/HDRi raw mode the samples are the same linear transmission,
and the embedded profile is recorded but not applied. `input_color.evidence` records the
tag. Measure the film base from an unexposed frame as usual. Slide (positive) film is
not supported, and nothing in the file tells the two apart, so `convert`, `roll` and
`measure-roll` check each picture frame against the base and warn when it does not look
like a negative (see Troubleshooting). The check catches a slide only when its base came
from the slide's black unexposed film: read from its clear leader, the slide converts
silently as a negative. It is also blind on a channel whose base is at or above about
two-thirds of full scale, as a B&W negative's can be on every channel.

### IR (HDRi 64-bit input)

The IR plane is decoded, read only by the film-holder measurement below, and dropped
before the render: **no rendered pixel depends on it**. It raises no warning, and
`--strict` passes on an HDRi scan. It is not exported: `--export-ir` and `input.export_ir` were removed and
exit 2 (a `null` key, as older dumps wrote it, is dropped).

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
  `film_type` (on `roll`, per frame; never `unknown`), and `--save-recipe` writes it as
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
resolves and reports it, and reads it only for the polarity warning. The area is two
cuts, in order:

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
measured frame reads back as the step, not as 0 (this example and the two below are
from real uncropped scans; the HDRi fixture is cropped, so its holder measures `0`, and
the 48-bit fixtures have no IR plane, so theirs is `null`):

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
warning, which `--strict` promotes to a failure.**

- `capped` — one flag per edge. A capped edge marched as deep as `hanten` looks (25% of
  the shorter edge) without finding film. No film holder is that deep (real scans
  measure 2.5–4%, and none of 31 measured IR frames has capped), so a cap means the
  IR read something else — IR-dark film or debris — and the depth reported there is
  the cap, not a measurement. Each edge is measured over what the *perpendicular*
  edges' cuts leave, so the edges perpendicular to a capped one can cap with it: a
  400×400 frame with a 120 px IR-dark band at the top and 10 px sides reports
  `top`, `left` and `right` all as 100. The error runs one way only: the region is
  **over-cut**, losing area but holding no holder. If a frame's holder really is
  deeper than the cap, raise the inset to cover the rest — it is added on top of
  the cap.
- `converged: false` — the per-edge march did not settle (the iteration above). Hanten
  then reports the deeper of the last two rounds, which over-cuts rather than leaving
  holder inside the region for a two-round oscillation or a run still settling
  downward; a longer cycle, or one settling upward, could still under-cut. A far
  enough over-cut leaves nothing to measure, which is refused outright on a run that
  measures over the region (exit 2, or a failed frame on a roll) and warned about
  otherwise. No real scan has produced this.

`converged` and `capped` are **not** independent, and `converged: true` is not a
quality verdict on its own: a cap *creates* a stable fixed point, so a capped frame
settles and reports `converged: true`. Read the two together.

Two things this does *not* do:

- **It never crops the image.** Written dimensions, aspect ratio and pixel count are
  exactly as decoded. The effective area changes only which pixels a statistic is
  computed over.
- **It never looks for the rebate.** The inset passes over it blind. That is why
  `measure-base` wants an unexposed frame, where the whole area is unexposed film; on a
  picture frame, give it a region (`--base-region`).

Every command that decodes resolves the area and reports it; a conversion reads it
only for the polarity warning (below), never for a rendered pixel. So if the two cuts
leave **nothing**, `measure-base` (with no source flag) refuses (exit 2), but `convert`
and `roll` warn rather than refuse, skip the polarity check, and the report omits
`effective_area` — there is no region to report, and `--measure-inset` has no effect on
that run.

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

A `convert` report also carries `recipe`, the resolved recipe `--save-recipe` would
write as its `params` — save it and it reloads through `--params` to the same image —
and `identity.params_hash`, a stable hash of that recipe as Hanten pretty-prints it
(the telemetry record's `conversion.params_hash`). Each `roll` frame's `identity.params_hash` hashes
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

Every encoder counts this, not just the TIFF ones — the gain-map JPEG builds the same
report when it quantizes.

A channel written as 0 everywhere is in range, so no loss counter sees it; it warns on
its own (`--exposure=-100` on a fixture, with the roll stated):

```json
["no written sample is above 0 in the red, green and blue channels: the frame renders black there"]
```

Fit range compresses the scene against its headroom, so on both test fixtures **a
default render does not clip**. A clip warning therefore means something pushed samples
past fit range's reach — most often **exposure** (`--exposure 12` clips 100% of a frame)
or a `--display-tone-headroom` too small for the content. Check those before anything
else.

`--strict` promotes warnings **that reach the JSON report** to a hard error
(exit 1), after the report is emitted — the right default for scripts and CI.

> One deliberate exception: a failure to write an opted-in **telemetry**
> destination prints `hanten: warning:` on stderr (a closed stdout pipe is not a
> failure — `-v` notes it) but is kept out of the report set, so
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
| 5 | Output write error — including the report or the profile on stdout, unless its reader closed early |
| 6 | Resource limit — estimated peak memory exceeds the budget |

---

## 11. Operational flags

The flags in the tables below are **not** conversion knobs: they never appear in a
recipe and can never perturb a pixel. `profile` takes none of them, and `inspect` has no
`--strict`.

| Flag | Purpose |
|---|---|
| `--max-memory BYTES` | Peak-memory budget, checked **before decode**. Accepts `8GiB`, `4096MB`, or raw bytes. Default 6 GiB — a fixed value, so the pass/fail decision is machine-independent. Over budget ⇒ **exit 6**. |
| `--report` / `--report-file` | Report format (`json`, `none`) and destination |
| `-v` / `-vv` / `--quiet` | stderr verbosity — never pollutes stdout |
| `--strict` | Promote warnings to errors |
| `--export-film-rgb` | The decode before the 3×3, untagged f32 TIFF: `PATH` on `convert`, a switch naming each frame's file on `roll` ([§8](#8-destinations)); refused elsewhere |

These are **`convert` only** — every other command exits 2 if given one:

| Flag | Purpose |
|---|---|
| `--telemetry` / `--telemetry-file` | Opt-in, fail-soft performance event (JSONL, `schema_version` 11), one per run — failed runs included, once the command line parses. `outcome.status` is `success` or `failure`; a failure names its `stage` (a stage, or `setup` / `preflight` / `finalize`), `error_kind` (`usage`, `decode`, …, or `strict` for a `--strict` promotion) and `exit_code`, and carries only what the run reached — never the error message. `timing_ms` has one field per completed stage, `conversion.params_hash` is the report's, and a finished frame's `outcome.clipped` / `non_finite` counts come with their denominator, `outcome.total_samples`. Also `NC_TELEMETRY_LOG`. |
| `--seed N` | Reserved; nothing is stochastic today |
| `--export-pre-encode PATH` | The buffers the destination's encoder receives, untagged TIFF pages ([§8](#8-destinations)) |

> **Caveat on `--max-memory`:** the budget also caps the TIFF read buffers, so a
> small-but-passing budget can turn a decodable file into an exit-3 decode failure.
> There is also a warning tier above ~70% of detected RAM — the one documented
> exception to machine-independence, since with `--strict` the same run can exit 0
> on a large machine and non-zero on a small one. The *image* is still identical;
> only the exit code differs.

On `roll`, the gate runs **per frame**: a rejected frame is recorded in the report,
its siblings are still written, and the roll exits **1**, not 6.

### Telemetry upload

`--telemetry` never uploads by itself. Uploading is a separate, persistent opt-in:

```bash
hanten telemetry enable            # shows what is sent, asks, then turns it on
hanten telemetry enable --yes      # the same without a prompt (needed off a terminal)
hanten telemetry enable --queue ~/nc-tel.jsonl --yes   # collect into another file
```

`enable` prints the field manifest — a random per-event ID, the day, version, OS,
CPU architecture and core-count bucket, the outcome, per-stage timings, and coarse
image and output facts; never a path, pixel, parameter value, error text or any
identity — then selects one **queue**: `--queue PATH`, else the `--telemetry` log
(`NC_TELEMETRY_LOG` or the default path). From then on every `convert` appends its
event there — a command line clap refuses too, as a `parse` failure — and a
short-lived background process uploads it once the run has finished: nothing waits
on the network, and the run's output, report and exit code are unchanged. Events of
the current schema already in the queue are uploaded too; older records are dropped.
Uploading empties the queue file, so once it is the `--telemetry` log that log stops
being a local history: point `NC_TELEMETRY_LOG` elsewhere to keep one. The queue and
its hidden sibling spool (`.<name>.nc-telemetry-spool`) are capped at 25 MiB, and
records expire after 30 days.

**Panic reporting.** If a consented `convert` panics (an internal bug, exit 101),
it leaves one panic event in the spool before Rust prints its usual message, and a
background upload starts right away, as after a normal run. The event holds the
stage the run was in and up to 32 Hanten function names
(`nc::pipeline::look::apply`, …), never the panic message, a file path, a line
number or an address. A process reports its first panic only, and nothing is
recorded while 64 panic events already wait in the spool or on a filesystem without
hard links (some network or FAT volumes). `--telemetry` alone never reports panics,
and the panic's stderr and exit code are the same either way.
This is not crash reporting: a run killed by a signal (a segfault, `kill -9`),
aborted, stopped by the out-of-memory killer or forced to quit reports nothing. The
queued count is `panic_ready` in `hanten telemetry status`.

| Command | What it does |
|---|---|
| `hanten telemetry status` | JSON on stdout: consent (`never_enabled`, `active`, `inactive`, `unreadable`, or `needs_reconsent` after an upgrade that uploads more fields: `enable` again), the queue and spool paths, what is queued, and upload counters with the last success and last error. |
| `hanten telemetry preview` | The request bodies that would be sent now, one per line on stdout, exactly as sent. Sends nothing. |
| `hanten telemetry flush` | Upload now, in the foreground; prints a JSON summary and exits 1 if the upload failed (the queue is kept for a retry). |
| `hanten telemetry disable` | Stops collecting and uploading. Waits for an upload already in flight (at most 10 s), but not for a running `convert`, which may still queue its event. Keeps the queue. |
| `hanten telemetry purge [--yes]` | Only while disabled: deletes everything queued, waiting for consented `convert`s still running. |

Consent is a file in the config directory (`$XDG_CONFIG_HOME/nc`, else
`%APPDATA%\nc` on Windows, else `~/.config/nc`). Enabling again while enabled is a
no-op; selecting another queue needs `disable` first, and is refused while the old
queue still holds records (`flush` it after enabling it again, or `purge` it). Disabling
or purging cannot delete events already uploaded: they carry no identity to find them
by, and the service keeps them 180 days.

`NC_TELEMETRY=0` turns automatic collection and upload off for one process; a
`--telemetry` event is then not written to the upload queue either (it warns). Where
uploads go is fixed when the binary is built (`NC_TELEMETRY_ENDPOINT`); a build made
with `none` has nothing to enable. Set at run time, the same variable can only narrow
that — to `none`, a `file:<path>` that receives the request bodies, or a loopback
test server. A `--telemetry-file` naming the upload queue is not written (it warns).

`--new-flow`, which selected this chain while a second one was the default, was removed
when it became the only one; passing it exits 2 on every command.

---

## 12. Troubleshooting

**"no film base selected"**
Neither `convert` nor `roll` has a default film base. Pass `--film-base R,G,B`
(measured once per roll), or a `--params` recipe stating `calibration.film_base` (the
file `hanten measure-roll … --out` writes). `--base-region X,Y,W,H` also works, but on
`roll` it re-reads the base on every frame. `hanten measure-base <unexposed-frame>`
is the way to get a value in the first place.

**"the effective area is not uniform … it does not look like unexposed film"**
`measure-base` was given a picture frame. Give it the roll's unexposed frame, or, if the
roll has none, a region of unexposed film on another frame (`--base-region`).

**"this does not look like a negative under the film base: … transmits more than 1.5x the base"**
On a negative the film base is the most transparent film there is, yet more than 1% of
the effective area lets through over 1.5 times its light on some channel. Either the
base is not this roll's (a leader, another roll, or a region over the picture), the area
shows backlight past the film's edge (the end of a strip, a partial frame), or the scan
is a slide, which Hanten does not support. Re-measure the base on this roll's unexposed
frame, or raise `--measure-inset` past the bare backlight. The image is still written
(`--strict` fails the run on it). The absence of this warning proves nothing: see
§9, *SilverFast positive mode*, for what it cannot see.

**"base-region … is not uniform (worst per-channel relative spread …)"**
Your rectangle mixes unexposed film with image content. Check the coordinates, or run
`measure-base` on a genuinely unexposed frame.

**"the film base renders to luminance 0 after the decode, scene correction and the look …"**
**"the densest sample a scan can hold … overflows f32 …"**
The stated values do not combine into a render, though each is legal alone: the film
base would grade to nothing display black can place black against, or the densest
sample a scan can hold (a zero sample) would overflow. Checked before anything is
decoded, on `convert` and `roll`, every `roll.frames` entry included. The message ends
with each knob whose default alone would render:

```text
usage: the film base renders to luminance 0 after the decode, scene correction and the look, and display black needs a positive, finite one to place black against. It renders with --contrast (recipe `look.contrast`) at its default
```

A base read from a region is checked as if it were 1, and the message says so.

**Heavy clipping in the report**
Fit range does not clip ordinary content at its default headroom, so something pushed
content past it: a positive `--exposure`, too little `--display-tone-headroom`, or a
low anchor (`--anchor-mid-offset` smaller than the default). Lower the exposure, raise
the headroom, move the anchor up, or write a float output (`--transfer linear` with a
`--gamut`, or `--film-master`) for an unclamped result.

**"no roll measurement: rendered with …"**
Every `convert` under the `default` rendering warns until the roll is measured, and
`--strict` fails on it. Run `hanten measure-roll … --out roll.json` and convert with
`--params roll.json`; or state `--white-balance`, `--contrast` and `--saturation`; or render
`--rendering direct` (§7).

**"a recipe must state `"recipe_version": 3`"**
The recipe was written before `pipeline_version` 8, for the removed chain (§5). Start
from `hanten profile` for the look, carry `calibration.film_base` into its own file
(`--film-base`, or `hanten measure-base --out`), and render the old recipe itself with
the reference build.

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
| **Content-based film-base fallback** (a `measure-roll` option) for cropped scans with no visible rebate | [`film-base/content-fallback`](tasks/film-base/content-fallback.md) |
| **IR dust removal** | roadmap follow-up, no task file yet |

[`docs/TASKS.md`](TASKS.md) is the authoritative status for all of it.

# Using Hanten

A practical guide to converting film negative scans to positives with `hanten`.

> **Scope.** This is the *user-facing* guide: what to run, in what order, and why.
> For the authoritative design rationale and the full parameter semantics, see
> [`design-spec.md`](design-spec.md). Where the two disagree, the spec wins on
> *intent* — but this document is verified against the binary, so it wins on
> *what the CLI currently accepts*.
>
> **Verified against:** `hanten 0.1.0`, `pipeline_version 5`, built at commit
> `1033b108e90a` plus the new chain's recipe (`nf-core/recipe-schema`, §11). The staleness signal
> is `pipeline_version`: if `hanten --version` reports a different one, treat this
> document as suspect and re-verify.
>
> **Known issue:** under the default render the gain map is inert (no HDR
> headroom) — see the callout in §8.

---

## 1. The mental model

Hanten converts a **negative** scan into a **positive** image. Three properties
shape every workflow below:

- **Deterministic.** Same input + same parameters ⇒ byte-identical output (on one
  build and architecture). There is no hidden per-frame adaptation unless you
  explicitly ask for it. (The transitional `--new-flow` selector in §11 is the one
  flag outside that promise: it chooses a whole rendering chain.)
- **Every knob is a CLI flag *and* a recipe key**, and nothing is reachable only
  from code. Passing a flag that doesn't apply to your selected *curve* or *preset*
  is a **loud error**, never a no-op. The exception is `--reconstruction simple`
  **on the `legacy` / `custom` path**, which has no print stage: `--print-exposure`,
  `--black-point`, `--highlight-compress` and `--white-balance` are accepted there
  and silently do nothing (verified — the output is byte-identical). On a display
  preset — including the default — they all reach the render whatever the
  reconstruction, so they *do* change the picture: white balance, exposure and the
  black point run in the shared display stage, and `--highlight-compress` places the
  knee inside each display renderer. `--auto-wb` is the exception on both paths: it
  still requires `--reconstruction density` and is a usage error under `simple`, even
  on a display preset — pass explicit `--white-balance` gains there.
- **Calibrate once, apply many.** The film base (`Dmin`) and the reference density
  (`Dmax`) are properties of the *roll* — film stock, development, scanner — not
  of an individual frame. You measure them once and reuse them, which is what
  keeps a whole roll color-consistent. **`Dmin` has no default: every `convert`
  must say where the film base comes from**, because it sets the black point and
  the colour balance together.

That last point is the whole workflow:

```
   plan                            freeze                    apply
┌──────────────────┐          ┌───────────────┐        ┌──────────────────────┐
│ hanten inspect   │  ──────► │ recipe.json   │ ─────► │ hanten convert       │
│ hanten estimate  │          │ (Dmin, Dmax,  │        │ hanten roll          │
│                  │          │  print knobs) │        │                      │
└──────────────────┘          └───────────────┘        └──────────────────────┘
  measure from a                reuse-ready forms         one shared recipe
  reference frame               printed by estimate       across every frame
```

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
- **`Dmax`** — a **scalar** in **density** units: the roll's *reference* density.
  This is `--d-max D`.

They are not two ends of one scale; don't conflate them.

> **`Dmax` is a reference density, not automatically "display white".** Which tone
> the reference *places* is a separate control:
>
> - Under the **sigmoid** curve (the default), `Dmax` and the placement are
>   deliberately split. The default anchor puts **mid-grey at 0.5·Dmax** and lets
>   display white float above *that placement* rather than being pinned. See §6.
> - Under the **exponential** curve, `Dmax` maps to display white *by default* — that
>   curve's default anchor is `white-at-dmax`. The anchor flags apply there too, so it
>   is a default, not a property of the curve.
>
> Some CLI help text still calls `--d-max` a "display-white anchor" — that wording
> predates the split and is accurate only for the exponential curve.

---

## 2. Getting a binary

```sh
cargo build --release      # → target/release/hanten
```

A fresh machine needs CMake, C and C++ compilers, libclang (bindgen), and NASM —
the build compiles pinned libultrahdr, libjpeg-turbo, and libaom from vendored
source. Only those **native** libraries are vendored: cargo still fetches the Rust
crates from crates.io, so the build needs network access (or a warm cargo cache).

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

## 3. The five commands

| Command | Purpose | Writes an image? |
|---|---|---|
| `hanten inspect` | **"What is this file?"** — format, dimensions, IR presence, scanner metadata, resolved input semantics, candidate rebate regions. | No |
| `hanten estimate` | **"What number do I freeze?"** — measure the film base (`Dmin`), and optionally `Dmax`. Prints **reuse-ready** flag and recipe forms. | No |
| `hanten params` | Print the full default recipe as JSON — the scaffolding starting point. | No |
| `hanten convert` | Convert one frame. The full parameter surface. | Yes |
| `hanten roll` | Convert many frames from **one shared frozen recipe**. | Yes |

Every command except `params` emits a **JSON report on stdout** on success
(`--report none` to suppress, `--report-file PATH` to redirect); `params` takes no
flags at all and just prints the default recipe. Logs and warnings go to
**stderr**, so stdout stays clean for piping into `jq`.

> **A hard failure emits no report at all** — stdout is empty. A decode error, a
> memory refusal, or a measurement that fails (`estimate` finding no rebate band)
> exits non-zero before the report is written. Only `roll` is different: it
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

`inspect` is non-fatal by design: if a rebate band is detectable it suggests a
`Dmin`, and if selection *refuses* it still reports the candidate rectangles it
found:

```sh
hanten inspect scan.tif | jq '.base_candidates'
```

Confirm one of those rectangles and pass it to step 2 as `--base-region` — that
saves measuring coordinates by hand on a scan where auto-detection won't commit.

`inspect` also reports the **effective area** — the region `hanten` reads measurements
over, after the film holder and a border inset are removed. Worth a look on any
uncropped scan; see [The measurement region](#the-measurement-region-the-effective-area).

### Step 2 — Measure the film base

**This step is mandatory.** `convert` and `roll` refuse to run without a stated
film base — there is no default, because `Dmin` is the divisor of the density
conversion and sets the black point and the colour balance together:

```
usage: no film base selected: pass --film-base R,G,B (a Dmin measured once per
       roll, e.g. with `hanten estimate`), --base-region X,Y,W,H to sample an
       unexposed border, or --auto-base to detect the rebate band …
```

`estimate` and `inspect` are the deliberate exceptions — `estimate` exists to
*produce* a base, so it still resolves an unstated source to `auto`, and `inspect`
always runs the detector. Requiring a base there would make the measure-once
workflow circular.

Three sources, in descending order of reliability:

**(a) An unexposed reference frame** — the best option. Use `--grid` to sample five
cells (corners + center) and cross-check them:

```sh
hanten estimate unexposed-leader.tif --grid
```

Disagreement between cells warns loudly — that diagnoses light leaks, illumination
falloff, or dust *before* it silently poisons a whole roll.

**(b) A known border region** on a normal frame:

```sh
hanten estimate scan.tif --base-region 0,0,24,24
```

`hanten` checks the rectangle for uniformity and warns if it looks like it mixes rebate
with image content.

**(c) Auto-detection** — scans inward for the unexposed rebate band behind the
film holder. Still one flag; what's gone is arriving there by omission:

```sh
hanten estimate scan.tif --auto-base
```

> **Real scans are laid out `dark holder → thin inset rebate → picture`** — the
> rebate is *not* the outer margin. Auto-detection is therefore best-effort and
> **fails loudly** rather than guessing. Prefer (a) or (b) for production work.

Either way, `estimate` hands you the result in **reuse-ready form**:

```json
{
  "film_base": { "r": 0.16311894, "g": 0.080109864, "b": 0.037720304 },
  "film_base_flag": "--film-base 0.16311894,0.080109864,0.037720304",
  "calibration": {
    "film_base": { "explicit": [0.16311894, 0.080109864, 0.037720304] }
  }
}
```

Copy `film_base_flag` straight onto a command line, or take the whole
`calibration` object as a recipe — `jq '{calibration}'` writes one directly. That
is the intended handoff — no manual transcription of floats.

Add `--strict` when scripting: it turns "plausible-looking but bad" into a hard
failure instead of a value your pipeline silently bakes in.

### Step 3 — (Optional) Measure `Dmax`

The reference density defaults to a **fixed nominal** value (1.3 density),
scene-independent and reused across the roll. You can instead measure it from a
**fully-exposed** reference frame — the light-struck roll leader — though see the
reliability caveat in §6 before relying on the result:

```sh
hanten estimate leader.tif \
  --film-base 0.163,0.080,0.0377 \
  --d-max-region 100,100,80,80
```

which reports:

```json
{ "dmax": 0.39084455,
  "d_max_flag": "--d-max 0.39084455",
  "calibration": { "film_base": { "explicit": [0.163, 0.08, 0.0377] },
                   "dmax": { "explicit": 0.39084455 } } }
```

`calibration` carries whatever this run resolved — here both halves, because the
base was supplied and the reference measured. It is already in recipe shape, so
`hanten estimate … | jq '{calibration}' > roll-cal.json` writes a reusable roll
calibration with nothing to edit.

> **Reusing it on a parametric curve mis-anchors the render.** The measured value is
> a *raw* density; `sigmoid` / `exponential` subtract the anchor from the corrected
> density, whose default per-channel scale is not the identity. `convert` warns when
> you combine the two — see the domain caveat in §6.

> **When a fully-exposed leader is beyond the scanner's visible-light range.**
> An error saying a channel's transmission is `0` or at/below the scan floor
> refers to the raw negative scan, before inversion: the film is opaque there,
> so it would become a bright scene value after conversion. This can be a valid
> leader whose density exceeds what the scanner recorded, not necessarily a
> holder-selection mistake. The exact `Dmax` is then unknown—zero transmission
> establishes only a lower bound—so the current `hanten estimate` exits **1** and
> emits no reuse-ready value. To convert today, either retain the fixed nominal
> reference (omit `--d-max`, or use `--fixed-d-max`) or supply a deliberately
> chosen positive `--d-max`; do not pass transmission `0` as a density. A
> machine-readable clipped-reference handoff and documented fallback policy are
> tracked in [`film-base/clipped-dmax-reference`](tasks/film-base/clipped-dmax-reference.md).

### Step 4 — Write the recipe

`roll` is configured **only** by recipe — it has no `--film-base` — so a roll needs
a recipe file. `estimate`'s `calibration` object *is* the measured half; add your
parameter choices beside it:

```jsonc
{
  "calibration": { "film_base": { "explicit": [0.163, 0.080, 0.0377] },
                   "dmax": { "explicit": 0.391 } },
  "reconstruction": { "curve": { "type": "sigmoid" } }
}
```

**The two halves have different lifetimes, and the schema keeps them apart.**
`calibration` is what you measured off *this* roll; everything else is the look,
which you reuse across rolls. So a roll calibration is a recipe with nothing but
`calibration`, and a look is a recipe with no `calibration` at all — the second
needs a base from a flag, since `calibration.film_base` has no default.

Omitted sections take their defaults, so a recipe only needs to carry what you
decided. `hanten params` prints the full default document if you want a scaffold to
edit.

> **`--dump-params` does not freeze your measurements.** It writes the resolved
> *config*, which for a measured value is the **mode**, not the number — a run
> with `--auto-base --auto-wb percentile` dumps `"auto"` and `"percentile"`, and
> the report's measured values are nowhere in it. Two different scans with the
> same flags produce identical dumps. So a recipe dumped from an auto run
> **re-measures on every frame of the roll**, which is exactly what `roll` exists
> to prevent. Only explicit values freeze. (The automatic `<output>.json` sidecar
> has the same content, and reloads through `--params` unchanged.)

### Step 5 — Apply to the whole roll

```sh
hanten roll frames/*.tif --out-dir positives/ --params roll-recipe.json
```

Every frame gets the identical film base, `Dmax`, and print controls, so the roll
is color-consistent. Outputs are named `<input-stem>_positive.<ext>`, the suffix
coming from the resolved preset; a roll-level
JSON report lands on stdout.

---

## 5. Recipes

### Shape

The recipe *is* the resolved config. Get the current default with:

```sh
hanten params
```

```json
{
  "reconstruction": {
    "schema_version": 1,
    "type": "density",
    "density": {
      "scale": [1.0, 0.84, 0.73],
      "offset": [0.0, 0.0, 0.0],
      "shadow_balance": [0.0, 0.0, 0.0],
      "highlight_balance": [0.0, 0.0, 0.0],
      "balance_range": "auto"
    },
    "curve": { "type": "sigmoid", "contrast": 2.0686874, "toe": 0.2,
               "shoulder": 0.6,
               "anchor": { "mid-at-dmax-fraction": 0.5 } }
  },
  "input":       { "transfer": "auto", "meaning": "auto",
                   "film_type": "unknown", "export_ir": null },
  "calibration": { "film_base": null, "dmax": "fixed" },
  "measure":     { "inset": 0.05 },
  "print":     { "print_exposure": 0.0, "black_point": 0.0,
                 "white_balance": { "explicit": [1.0, 1.0, 1.0] },
                 "display_tone": "shoulder",
                 "highlight_compress": 0.0, "linear_range": [0.0, 1.0] },
  "output":    { "preset": "gain-map-hdr", "depth": "u16",
                 "output_profile": null, "bigtiff": "auto" }
}
```

`calibration.film_base` prints as `null` because it has **no default** — this
document is a template to edit, not a runnable recipe. `convert` and `roll` reject
an unstated base.

### Partial recipes are fine

Omit any section and serde defaults fill the gap. This minimal recipe produces a
**byte-identical** result to the full flag form:

```json
{
  "calibration": { "film_base": { "explicit": [0.163, 0.080, 0.0377] },
                   "dmax": { "explicit": 0.391 } },
  "reconstruction": { "curve": { "type": "sigmoid" } }
}
```

> **Gotcha:** the tagged objects need their tag. If you include
> `reconstruction.curve` at all, you **must** include its `"type"` — omitting it
> fails with `reconstruction.curve is missing 'type'`. Omitting the whole `curve`
> object is fine.

The `curve` object above is the **sigmoid** shape. Selecting the exponential curve
resolves a different, smaller set of keys:

```json
{ "type": "exponential", "gamma": 2.0, "anchor": "white-at-dmax" }
```

The reference density is **not** a curve key: it is a measurement of the roll, so
it lives at `calibration.dmax` (`"fixed"` | `"auto"` | `"none"` |
`{"explicit": <d>}`). A recipe that still spells it `reconstruction.curve.dmax`
is rejected with a migration error naming the new path.

### Strictness

Every recipe struct uses `deny_unknown_fields`, so a typo is rejected rather than
ignored:

```
usage: invalid recipe t.json: unknown field `exposure`,
       expected one of `print_exposure`, `black_point`, `white_balance`,
       `display_tone`, `highlight_compress`, `linear_range`
```

This means a **misplaced** key fails too — a key must live under the stage section
that owns it (`--export-ir` ⇒ `input.export_ir`, not top level).

Removed legacy forms produce a **migration error** explaining the replacement, never
a silent alias. Those are a top-level `algorithm` or sibling
`density`/`sigmoid`/`simple` sections, and the two paths the roll measurements moved
from — a top-level `film_base` section, and `reconstruction.curve.dmax`:

```
usage: recipe roll.json: top-level `film_base` is no longer supported — the roll's
       measured values moved into their own `calibration` section, and the `source`
       wrapper went with them. Replace `"film_base": {"source": {"explicit": [r, g, b]}}`
       with `"calibration": {"film_base": {"explicit": [r, g, b]}}` …
```

### Precedence

**Flags always win over the recipe.** Precedence is by *source*, not value — an
explicit `--white-balance 1,1,1` over a recipe's `auto` mode means neutral gains,
not re-estimation. With a [`--preset`](#-preset--pick-a-look-by-name) the full chain is
`defaults < --params recipe < --preset < flags`.

```sh
hanten convert scan.tif -o out.jpg --params roll-recipe.json --d-max 0.5
#                                                          ^ overrides the recipe
```

### Sidecars: every conversion is reproducible

**Every** written image gets a `<output>.json` sidecar automatically — no flag
needed:

```json
{
  "meta":   { "nc_version": "0.1.0", "git_commit": "e4a56bb2540d",
              "pipeline_version": 5, "target": "aarch64-apple-darwin",
              "params_hash": "18b95264170ab67a" },
  "params": { ...the exact recipe... }
}
```

`params` is the recipe body. `meta` is provenance and no part of it changes a
pixel — but it is not entirely ignored either: the loader parses
`meta.pipeline_version`, rejects a malformed one (exit 2), and emits a
`--strict`-promotable warning when it differs from the running build, so an
archived replay tells you the render may have moved.
Feed the sidecar straight back to reproduce the conversion exactly:

```sh
hanten convert scan.tif -o repro.jpg --params out.jpg.json
# → byte-identical to out.jpg
```

### Per-frame overrides in a roll

Use a `--frames` manifest instead of positional inputs when individual frames need
a tweak on top of the shared recipe:

```json
{ "frames": [
    { "input": "a.tif" },
    { "input": "b.tif", "output": "b-brighter.jpg",
      "params": { "print": { "print_exposure": 1.0 } } }
] }
```

```sh
hanten roll --frames frames.json --out-dir positives/ --params roll-recipe.json
```

An explicit manifest `output` goes through the same suffix rule as `convert`: an
extension it states must match the resolved preset's container — `.jpg` under the
default, `.tiff` under `legacy`/`display-p3`/`film-master`, `.avif` under
`hdr-pq`/`hdr-hlg` — and one it omits is completed from that container, so
`"output": "chosen"` writes `chosen.jpg` on a default roll.

Some keys describe the *roll*, not the frame: the whole `calibration` section, the
anchor placement, `curve.stock` and `output.preset`. Overriding one per frame is applied
but warns loudly (and `--strict` turns the warning into a failing exit), because the
frame then renders on a different rule from its siblings — a roll is one piece of film
through one process.

An override that changes `curve.type` **re-resolves the two knobs whose right value is
per-curve**: the anchor placement and the per-channel `density.scale` both take the new
curve's default. The roll's measured `calibration` is untouched by a curve switch, in
either direction — it is a separate section, not a curve knob. Each reset warns if it
discarded a value the recipe had stated; restate it inside the override to keep it.

---

## 6. Reconstruction and curves

### `--preset` — pick a look by name

The five settings below (curve, per-channel gain, print exposure, display tone) only
mean anything **together**: the exposure that lands one brightness runs from 1.59 to
2.17 depending on the reconstruction, because the curves place mid-grey differently.
`--preset` names a bundle so you don't have to carry four coupled numbers.

Each bundle's exposure is solved so that **switching preset changes the look, not the
brightness** — what you compare is then the reconstruction and the display tone. That
calibration is done on `portra-400`; on another stock the parametric presets drift with
how closely they model it, by up to about half a stop. Presets are not meant to render
alike, so a brightness difference between two of them on your film is part of what they
offer, not something to correct.

| `--preset` | Reconstruction | Display tone | Needs |
|---|---|---|---|
| `characteristic-generic` | `characteristic`, the averaged generic C-41 profile | `reinhard` | — |
| `characteristic-stock` | `characteristic`, the roll's own published response | `reinhard` | `--film-stock` |
| `characteristic-aim` | `characteristic-stock` + the aim-matched red density scale | `reinhard` | `--film-stock` |
| `sigmoid-knees` | `sigmoid` with its toe and shoulder | `none` | — |
| `sigmoid-flat` | `sigmoid` with neither knee | `reinhard` | — |

```sh
hanten convert scan.tif -o out.jpg --film-base 0.9,0.55,0.42 --preset characteristic-generic
hanten convert scan.tif -o out.jpg --film-base 0.9,0.55,0.42 --preset characteristic-stock --film-stock portra-400
```

All five render scene mid-grey at the same brightness, so what you are comparing
between them is the reconstruction and the tone, not "one is brighter".

**A preset is a set of starting values, and individual flags still win over it.** The
precedence chain is `defaults < --params recipe < --preset < flags` — the preset sits
*above* the recipe, because `hanten params` writes every key explicitly and a preset
underneath one would have nothing left to set. The report separates the two directions:

```json
"conversion_preset": {
  "name": "characteristic-generic",
  "replaced":   ["reconstruction.curve"],      // the preset won over the recipe
  "overridden": ["print.print_exposure"]       // a flag won over the preset
}
```

**A preset never touches the roll's measured `calibration`.** A preset names a *look*,
and writes no `calibration` key at all, so a measured film base and `Dmax` survive it
untouched and never appear in `replaced`. Everything in `curve` — contrast, knees,
anchor, stock — is the look, and the preset does replace it.

**It is a command-line shorthand, not a recipe key.** `--dump-params` writes the
*expanded* values, so a recipe replays identically on any build — including one whose
preset definitions have since moved. A recipe that names a preset is rejected as an
unknown field, and `hanten roll` takes the dumped recipe rather than a preset name:

```sh
hanten convert scan.tif -o out.jpg --film-base 0.9,0.55,0.42 \
   --preset characteristic-stock --film-stock ektar-100 --dump-params roll.json
hanten roll frames/ --out-dir out/ --params roll.json
```

Six combinations are refused rather than quietly doing something else:

- **`--film-stock` beside a preset that has no stock** (`characteristic-generic`,
  either sigmoid) — otherwise it would silently render a different bundle at the wrong
  exposure. Use `characteristic-stock`.
- **`characteristic-stock` / `-aim` with `--film-stock generic-c41`** — that profile is
  an average of nine sheets, not one film's response. Use `characteristic-generic`.
- **`characteristic-aim` with `portra-800` or `ultramax-800`** — their datasheets
  tabulate an aim delta their own curves contradict, so there is no correction to
  derive. Use `characteristic-stock` for those.
- **`--print-exposure` with `sigmoid-knees`** — that bundle renders with
  `--display-tone none`, which relies on the reconstruction staying inside the render's
  ceiling, and `--print-exposure` is a gain applied *after* the curve, so any positive
  value pushes it past reference white and the frame is refused. Brighten it with
  `--anchor-mid-fraction` instead (the preset resolves `0.28`; **lower is brighter**).
- **A preset with `--reconstruction simple`** (or a recipe resolving `simple`) — the
  direct inversion has no curve stage, so there is nothing for a bundle to configure.
- **A preset with `--output-preset legacy` / `custom` / `film-master`** — see below.

A preset never *sets* `--output-preset`, but the two are not freely combinable: a
conversion preset is a reconstruction **and display** bundle, so it needs an output preset
that renders a display image. `display-p3`, `compatibility`, `gain-map-hdr`,
`ultra-hdr-v1`, `hdr-pq`, `hdr-hlg`, `hdr-linear-tiff`, `hdr-pq-tiff` and `hdr-hlg-tiff`
all work. The three that run no display stage — `legacy`, `custom`, `film-master` — refuse
any `--preset` with a single message. For a film master, set the reconstruction knobs
directly instead:

```sh
hanten convert scan.tif -o master.tif --film-base 0.9,0.55,0.42 \
   --output-preset film-master --density-curve characteristic --film-stock ektar-100
```

Two reconstruction types, selected with `--reconstruction`:

| Type | What it does |
|---|---|
| `density` *(default)* | Density-domain inversion (Cineon / negadoctor lineage). What you want for colour negative. |
| `simple` | Direct channel inversion `1 − scan/Dmin` — **a debugging baseline**, not a production path. No density correction, curve, or `Dmax`, so it isolates decode plus film base. |

> **`simple` is not the B&W path**, despite what `--reconstruction`'s help text
> says. B&W film is still a density medium with its own characteristic curve, so
> B&W support (`algo/bw-support`) runs through `density` too — what it adds is a
> *mono colour model* that pools R,G,B into one gray, not a different
> reconstruction.
>
> The difference between the two is which domain the inversion happens in.
> `simple` is affine in **transmission** (`1 − t/Dmin`); `density` goes through
> the log domain and, with neutral correction, reduces to a **power law**
> (`positive ∝ (t/Dmin)^(−gamma)`). Film density is logarithmic in exposure — that
> is what a characteristic curve *is* — so the power law is the inversion that
> corresponds to something physical. `1 − t/Dmin` corresponds to none: it
> saturates toward 1 as the negative gets denser, compressing highlights by
> accident rather than by a tone decision.

Under `density`, three curves, selected with `--density-curve`:

| Curve | Knobs | Defaults |
|---|---|---|
| `sigmoid` *(default)* | `--sigmoid-contrast` (mid-density slope), `--sigmoid-toe` / `--sigmoid-shoulder` (knee widths in log10 density; `0` disables), plus the anchor flags below | `contrast 2.0686874`, `toe 0.2`, `shoulder 0.6` |
| `exponential` | `--density-gamma` — the straight line's slope, plus the anchor flags below | `gamma 2.0`, `anchor white-at-dmax` |
| `characteristic` | `--film-stock` — and nothing else | `generic-c41` |

Each curve takes **different recipe keys**, and mixing them is rejected — a
sigmoid-only key under an exponential curve fails with *"`toe` is a sigmoid-curve key,
but the curve type is "exponential" (its knobs are `gamma`, `dmax` and `anchor`)"*.
`anchor` is **not** one of those: it is shared by the two parametric curves (see below).

### `characteristic` — invert the film's own published curve

The other two curves *model* the film with a slope and an anchor you choose. This one
**reads** it: each dye layer's measured density-to-log-exposure relation, digitized from
the manufacturer's characteristic curve, inverted per channel. Mid-grey lands at 0.18 by
construction, so there is nothing to anchor and no contrast to pick.

```sh
hanten convert scan.tif -o out.tif --output-preset legacy \
  --film-base 0.5,0.25,0.15 --density-curve characteristic --film-stock portra-400
```

Ten stocks ship, all digitized from Kodak publications kept in
[`docs/datasheets/`](datasheets/): `generic-c41` *(the default)*, `ektar-100`,
`portra-160`, `portra-160vc`, `portra-400`, `portra-400vc`, `portra-800`, `gold-200`,
`ultramax-400`, `ultramax-800`. Naming a stock is a **refinement, never a requirement** —
omit it and you get `generic-c41`, the average of the nine measured stocks, which renders
correctly on any C-41 film. A stock that is named but unknown is a loud error listing the
accepted spellings, because a silent fallback would hide a typo behind a plausible render.

The report says which publication the numbers came from, so a datasheet value is
checkable:

```json
"curve": {
  "type": "characteristic",
  "out_of_table": { "below": [0.00012215113, 8.5596905e-05, 1.6293963e-05],
                    "above": [0.058473945, 0.051922057, 0.0] },
  "stock": { "name": "portra-400", "publication": "E-4050", "revision": "2025-01",
             "aims": [0.82, 1.18], "d_min": [0.2192, 0.646, 0.8665] },
  "dmax": { "policy": "none", "value": null, "provenance": "default" },
  "anchor": null, "anchor_value": null
}
```

`anchor` and `dmax` are `null` on purpose: this curve resolves no reference density and
follows no placement rule, and reporting one would name a knob the render never read. A
`calibration.dmax` stated beside it is accepted and carried — so one roll calibration
still composes with a stock-curve look — and a warning says it was not read.
`aims` are the sheet's published *Judging Negative Exposures* densities, `[grey card,
paper white]` (Status M, red channel) — the most directly checkable numbers on it if you
own a densitometer. `out_of_table` is the fraction of the frame that fell past either end
of the published curve, per channel; see the extrapolation note at the end of this
section. The `d_min` is **diagnostic only** — your measured `--film-base` is what the
render divides by. Its usefulness is as a check: the *differences* between those three
numbers are the stock's orange-mask signature, so comparing them against your own base's
differences tells you whether the stock you declared is the film you scanned.

Why it exists: every C-41 stock measured has a blue layer 12–19 % steeper than its red
one, so one contrast applied to all three channels leaves a colour cast that **grows with
density** — measured at +1.26 stops per unit corrected density across 21 real frames,
against +1.29 predicted by the datasheets. Inverting each channel's own curve removes it
(residual +0.09). It also inverts the film's toe rather than adding a second one.

**Known issue — a residual green cast.** The blue cast a single scalar contrast leaves is
removed (measured residual +0.09 stops per unit density, against +1.26 before), but a green
one remains, and its size depends on the stock: +0.08 for `gold-200`, +0.18 `portra-400`,
+0.48 `portra-160`, +1.00 `ektar-100`. On the badly-affected stocks `--film-stock
generic-c41` currently looks *better* than naming the stock, because averaging nine curves
dilutes any one sheet's error. The cause is most likely a missing cross-channel term (ACES
applies a 3×3 before its curves; Hanten does not yet) and it is tracked by
`io/scanner-density-calibration`. Ektar's own sheet also disagrees with itself by 11 %
between its aim table and its curve, which is a second, smaller factor for that stock.

**It needs a display tone curve.** Unlike the default sigmoid, this curve does not bound
itself at the render's ceiling — it hands the display scene-referred exposure, and measured
picture content reaches p99.99 **+3.64 stops** over diffuse white. So `--display-tone none`
in practice *refuses* a render of ordinary picture content: the per-pixel range check
rejects the frame (verified: "pixel 14 sits above reference white"), while `shoulder`
(the default) and `reinhard` both render. It is not a rejected *combination* — nothing
validates the pair — so content dark enough to stay inside the ceiling still renders
(`--print-exposure=-3` on the test fixture exits 0). If you want the reconstruction to
shed its knees and the display operator to carry the character, `reinhard` is the
pairing — see §7.

Two further limits. The published curves are for *typical* processing, not your
roll, so a heavily pushed or badly stored film will not match. And densities outside the
published range are **extrapolated** along its end slope, not read off it; `convert`'s
report gives the per-channel fractions in `reconstruction_result.curve.out_of_table`. A
`roll` frame entry carries no `reconstruction_result` block, so on a roll only the
above-20 % warning surfaces — measure a representative frame with `convert` if you want
the numbers.

Expect a few per cent there on a full-frame scan and ignore it: the holder and rebate
around the picture are denser than any exposed frame, so they sit past the end of every
curve. Measured across twelve frames, that border is 5–7 % of the frame and **none of it is
inside the picture area**. The figure is a poor check on the stock you declared, too —
rendering one frame under every profile moved it only between 5.75 % and 6.55 %. Only a
much larger fraction (the warning fires above 20 %) means the image itself is being
extrapolated.

### Anchoring — where a curve pins a tone

Both curves separate **which density is the reference** (`dmax`) from **which tone
gets pinned, and where** (`anchor`). This split is the substantive outcome of
[`reports/sigmoid-reference-baseline.md`](reports/sigmoid-reference-baseline.md),
and it exists because the two knobs otherwise fight: raising contrast pivots the
line *about the pinned point*, so pinning white necessarily drags everything below
it down.

The `--anchor-*` flags work on **either curve** — placement is independent of curve
shape:

| Anchor | Flag | Recipe (`reconstruction.curve.anchor`) |
|---|---|---|
| Mid-grey at a fraction of the reference *(sigmoid default, F = 0.5)* | `--anchor-mid-fraction F` | `{"mid-at-dmax-fraction": 0.5}` |
| Display white *at* the reference *(exponential default)* | `--anchor-white-at-reference` | `"white-at-dmax"` |
| The **film base** at output FLOOR | `--anchor-black-floor FLOOR` | `{"black-at-base": 0.005}` |
| Mid-grey at density D **above the base** | `--anchor-mid-offset D` | `{"mid-at-base-offset": 0.5}` |

Raising `F` renders the roll **darker**; lowering it renders **brighter**.

`--sigmoid-mid-fraction` and `--sigmoid-white-at-d-max` are the original spellings of
the first two and still work.

**Switching curves resets the placement.** `--density-curve` (and a `roll` per-frame
override that sets `curve.type`) leaves the roll's `calibration` untouched — it is a
separate section — but takes the new
curve's *default* anchor, because the right placement differs per curve — the two
defaults in the table are deliberately different, and `white-at-dmax` on the sigmoid is
the diagnostic described below. If your recipe pinned a non-default placement, that is a
loud, `--strict`-promotable warning naming what was dropped; restate the rule with an
`--anchor-*` flag (or a `curve.anchor` key beside the new `type`) to keep it.

**The last two rules never read the reference**, which matters because the reference
is measured from a fully-exposed leader — film saturation, not a diffuse white — and
it varies between rolls of the same stock far more than the film base does. Under
`--anchor-white-at-reference` that variation reaches the picture at full strength;
under `--anchor-mid-fraction 0.5`, at half; under the two base-derived rules, not at
all. `FLOOR` is linear light against the reference white, not an sRGB code value:
`0.005` encodes to about 16/255.

`--anchor-white-at-reference` is retained as an explicit **diagnostic**, not a
recommended setting on the sigmoid: at a photographic contrast it renders midtones
roughly 2.5–3.6 stops dark. It is kept reachable so the original defect can be
reproduced on demand, and it is the exponential's default because that curve's role
is to be the predictable straight-line reference.

> **Provisional values.** The measurement behind these defaults filters *methods*
> rather than tuning parameters, so the numbers are not final. The mid fraction
> `F = 0.5` rests on a chart read that is not a true Status M density (measured
> α ≈ 0.48–0.57 across three stocks). The per-stock datasheet anchor — the better form
> on the evidence — now ships as the `characteristic` curve above, but it is **opt-in**:
> the sigmoid's own `contrast`/`toe`/`shoulder`/`anchor` still describe what a bare
> `hanten convert` does. What *has* moved is the per-channel density gain beside them
> (`--density-scale`, `pipeline_version` 4 and again 5 — see below); the curve shape has not.
> Expect further movement, with a `pipeline_version` bump when it happens.

### Density correction (before the curve)

| Flag | Effect |
|---|---|
| `--density-scale R,G,B` | Per-channel density gain — **default `1,0.84,0.73`**, see below |
| `--density-offset R,G,B` | Per-channel density offset — **orange-mask compensation** |
| `--shadow-balance R,G,B` | Per-channel offset applied to the positive's **shadows** |
| `--highlight-balance R,G,B` | Per-channel offset applied to the positive's **highlights** |
| `--balance-range LO,HI` | Fix the regional-balance tone anchors (default: measured per frame) |

**`--density-scale` does not default to `1,1,1`.** It is `1,0.84,0.73` — a
calibration, not an identity. Green and blue density rise faster than red in a scan,
so with no gain they drift against it across the tone scale, which shows up as a
tone-dependent cast rather than an overall one.

The values come from **31 hand-marked neutral patches** across five rolls: per patch,
the gain that renders it neutral; per roll, the median; the default is the mean of the
five roll medians (green `0.837`, blue `0.733`). Rolls are weighted equally on purpose
— two thirds of the patches come from one scan date, and weighting by patch would let
that date set the default on its own. It replaced `1,0.90,0.86` at `pipeline_version` 5.

Two things to know before relying on it:

- **The default is per-curve, and you do not have to manage it.** `sigmoid` and
  `exponential` apply one scalar contrast to every channel, so they have no per-channel
  film model of their own and take `1,0.84,0.73`. The `characteristic` curve carries each
  stock's published per-channel structure already, so the same gain would correct it twice
  — on ten reference frames that moves the channel means *away* from neutral
  (`|G/R − 1| + |B/R − 1|` rises from 0.04 to 0.19) — and it therefore defaults to
  `1,1,1`. Selecting a curve re-resolves the gain unless you state one: `--density-scale`
  always wins, and switching away from a gain you had stated warns rather than dropping it
  in silence.
- **It balances five rolls; it does not fit yours.** Blue is the steady half — every
  roll measured wants 0.68–0.78. Green is not: it splits by **scan date** (one group
  0.86–0.90, another ~0.77, which tracks a change of developer rather than of film), so
  the shipped `0.84` is a compromise that fits neither group exactly and can push a roll
  from the higher group slightly green-yellow. The value is also calibrated on one
  scanner. A roll that still shows a cast wants its own `--density-scale`;
  `io/scanner-density-calibration` is the task that should remove the need to guess.

A positive balance value brightens that channel in that region. `0,0,0` (default)
skips the regional pass entirely and is bit-exact with the unbalanced output.

Negative values are common for the balance flags and a leading `-` is accepted.

For **roll consistency**, measure the range once and freeze it. The range is only
*measured* when the regional pass runs — that is, when the two balances **differ**
— so set them first:

```sh
hanten convert scan.tif -o out.jpg --film-base … \
  --shadow-balance=-0.05,0,0 --highlight-balance 0.05,0,0
```

Read the reported top-level `balance_range` (e.g. `[-0.368, 0.494]`), then pass it
as `--balance-range LO,HI` on the rest. With the neutral default the pass
short-circuits and no range is reported at all.

### The `Dmax` reference density — four mutually exclusive choices

Where the reference density comes from. (What it *places* is the anchor, above.)

| Flag | Behavior |
|---|---|
| *(none)* / `--fixed-d-max` | Fixed nominal reference (1.3 density), reused across the roll. **Default.** Darker frames render darker — faithful relative exposure. |
| `--d-max D` | Explicit roll-fixed reference — your measured calibration. |
| `--auto-d-max` | Measure per frame, over the [effective area](#the-measurement-region-the-effective-area) rather than the whole scan (so the film holder no longer owns the top percentile) — always, whatever the anchor placement, so the reported `dmax` has one meaning. **Per-frame exposure normalization**: brightens underexposed frames and breaks roll consistency. Grading, not conversion. Inert *on the pixels* under the two base-derived anchors, which never read the reference — so it is neither warned about nor rejected there, though the measurement is still taken and reported. |
| `--no-d-max` | No reference. Scene-referred output (base → 1.0, detail above) **under the default `white-at-dmax` placement** — it resolves the reference to 0, so any other `--anchor-*` rule still derives an anchor from the slope (`--anchor-mid-fraction 0.5` there pins mid-grey 0.37 above the base and clips ~99.9% of the frame). **Exponential only**: the sigmoid needs an anchor and rejects it (exit 2), so pair it with `--density-curve exponential`. |

> **A caveat on measuring `Dmax` from a leader** (§4 step 3). The baseline report
> found leader-measured `Dmax` untrustworthy on three independent counts:
> same-stock rolls differed by a full stop (0.295 density) while their bases agreed
> to 0.0005; real frame content measured *above* the leader value; and grain
> sensitivity makes "fully exposed" ill-posed, so a leader is a uniform field at an
> *uncontrolled* level. Blue is the least uniform channel in every leader measured.
> Treat a measured `Dmax` as better than nothing, not as a calibration you can
> trust across rolls.

> **A measured `Dmax` is in a different density domain than the parametric curves
> render in.** `estimate --d-max-region` reports the **raw** base-relative density
> `D = -log10(t/base)`, but `sigmoid` / `exponential` subtract the anchor from the
> *corrected* density `D' = scale·D + offset`, and their default `density.scale` is
> the non-identity scanner calibration `1,0.84,0.73`. So a measured value reused as
> `--d-max` is systematically high — about 14% at that gain, roughly 0.62 stop darker
> on every frame at the default anchor placement. `hanten convert` warns
> (`--strict`-promotable) whenever an explicit `--d-max` is combined with a
> non-identity scale/offset or a non-neutral regional balance. Your options: keep the
> default `--fixed-d-max` (a nominal already defined in the corrected domain), render
> with `--density-scale 1,1,1` so the measured domain *is* the render domain, or scale
> the measured number yourself. `--density-curve characteristic` is unaffected — its
> default scale is the identity.

### Nothing is silently ignored

Cross-curve and cross-type flags are **usage errors**:

```sh
hanten convert … --density-curve sigmoid --density-gamma 1.8
# usage: --density-gamma sets the exponential curve's gamma, but the resolved
#        curve is sigmoid — its mid-density slope is --sigmoid-contrast

hanten convert … --reconstruction simple --density-gamma 1.8
# usage: --density-gamma configures density reconstruction, but the resolved
#        reconstruction is `simple`

hanten convert … --film-stock ektar-100
# usage: --film-stock ektar-100 selects a published film response, but the resolved
#        curve is sigmoid — a stock has nothing to configure there.
#        Pass --density-curve characteristic

hanten convert … --reconstruction simple --film-stock portra-400
# usage: --film-stock configures density reconstruction, but the resolved
#        reconstruction is `simple` (the direct inversion has no density
#        correction and no curve); pass --reconstruction density
```

**A `--*d-max` flag is not in that family.** It sets `calibration.dmax`, a roll
measurement rather than a curve knob, so every reconstruction accepts one — and a
look that will not read it (`characteristic`, `simple`) says so instead of failing:

```sh
hanten convert … --density-curve characteristic --d-max 1.3
# hanten: warning: calibration.dmax is set, but this conversion does not read it:
#         the resolved curve is characteristic — it reads its slope and its
#         mid-grey placement off the stock's published response, so it consults no
#         reference. The value is carried in the recipe … and it did not affect
#         these pixels.
```

That is what lets one roll calibration compose with any look; `--strict` promotes
the warning if you want the stricter contract.

This is deliberate: a flag that quietly did nothing would be worse than a failure.

---

## 7. Print / tone controls

| Flag | Effect |
|---|---|
| `--print-exposure F` | Overall positive exposure |
| `--black-point F` | Paper black / shadow floor |
| `--white-balance R,G,B` | Explicit highlight / neutral gains |
| `--auto-wb MODE` | Estimate gains per frame — `gray-world` (≈ NLP Auto-AVG) or `percentile` (≈ NLP Auto-Neutral, more robust to a dominant scene colour) |
| `--highlight-compress F` | Highlight roll-off — where the display shoulder's knee sits |
| `--display-tone MODE` | `shoulder` (default), `none`, or `reinhard` — see below. **Display presets only**; `reinhard` is narrower still |
| `--display-tone-headroom STOPS` | Specular headroom above reference white, `reinhard` only (default `6` = a white point of 64) |
| `--linear-range LOW,HIGH` | Affine black/white placement, applied last — **display presets only**, which includes the default (see §8) |

`--white-balance` and `--auto-wb` are the two faces of one setting and are mutually
exclusive. The report tells you what was actually used:

```sh
hanten convert scan.tif -o out.jpg --film-base … --auto-wb percentile \
  | jq '.white_balance'
# [1.2501934, 1.0, 0.6589681]
```

Feed that back as an explicit `--white-balance` to freeze it across a roll.

### `--display-tone` — choosing the display tone curve

Display presets normally apply a Hermite shoulder that rolls highlights off to
display white, with `--highlight-compress` moving its knee earlier. The default
sigmoid *already* places every tone below reference white, so that shoulder
compresses highlights a second time. `--display-tone none` skips it:

```sh
hanten convert scan.tif -o out.tiff --output-preset display-p3 \
  --film-base … --display-tone none
```

Measured over ten reference frames on the shipped default reconstruction, this
reduced the share of the frame at absolute white on **every** frame (mean 6.5% →
4.9%) and improved highlight separation most on the frames where it was worst.
Midtones and shadows are untouched — only values above the knee change.

The report says which one ran, in a block every preset emits:
`.output_render.display_tone` is `"shoulder"`, `"none"`, or an object like
`{"reinhard":{"headroom_stops":6.0}}` on a display preset, and absent on
`legacy` / `custom` / `film-master`, which have no display tone stage.

On the HDR presets the per-preset block also states what the renderer *applied*, next
to the luminance anchors no container can carry — `.avif.rendering` for
`hdr-pq`/`hdr-hlg`, `.hdr_coded_tiff` and `.hdr_linear_tiff` for the TIFF pair:

```console
$ hanten convert scan.tif -o out.avif --output-preset hdr-pq --film-base … \
    --display-tone reinhard --report json | jq .avif.rendering
{
  "reference_white_nits": 203.0,
  "target_peak_nits": 1000.0,
  "linear_headroom": 4.9261084,
  "tone_curve": "extended-reinhard-mid-preserving-v2",
  "gamut_mapping": "bt2020-neutral-axis-radial-boundary-v1",
  "linear_domain": "bt2020-linear-relative-to-203-nit-reference-white"
}
```

`shoulder_start` joins it only for a curve that *has* a knee, so its absence does not
mean "no tone ran" — `tone_curve` is the field that says which one did.

**In a recipe, the operator's name alone is enough.** `print.display_tone` accepts
the bare `"reinhard"` and the empty `{"reinhard":{}}` as well as the explicit
`{"reinhard":{"headroom_stops":6.0}}`; the first two resolve the documented default
of 6 stops, exactly as `--display-tone reinhard` does. Reports and `--dump-params`
always write the explicit object, so a round trip normalizes to one form.

What it does *not* skip: gamut mapping and the transfer encode still run, so this
is "no tone curve", not "raw pixels out". And it needs a reconstruction bounded by
the render's own ceiling — the default sigmoid (`--sigmoid-shoulder` above 0) with
neutral print gains is. **The two ceilings differ**: the SDR presets stop at
reference white, the HDR ones at the 1000-nit mastering peak (≈4.93x reference
white), so the same overshoot can be refused on `display-p3` and render cleanly on
`hdr-pq` — that headroom is exactly what an HDR rendition exists to carry. If
anything exceeds its branch's ceiling, the render **fails** naming the pixel rather
than clipping it quietly:

```
error: SDR display rendering applied no display tone curve, but pixel 9 sits above
reference white (luminance 1.1606276). …
```

**"Neutral print gains" is part of the requirement, not a footnote — and the check
is late.** The shared print controls run *before* the display render, so a
non-neutral one can lift samples past reference white even under a bounded sigmoid:
`--print-exposure 0.3`, `--white-balance 1.3,1,1`, `--auto-wb percentile` and
`--linear-range 0,0.5` each trip the error above on the same frame that renders
cleanly with neutral gains. Nothing is rejected up front, because the real condition
is a pixel value rather than a flag combination — so `hanten` renders the whole frame
first and *then* exits 1, writing no file. On a large scan, prove `--display-tone
none` out with neutral print controls before adding grading on top.

Two rules follow from the knob being display-only: `legacy`, `custom` and
`film-master` reject it (they apply no display tone curve at all), and passing a
*non-default* `--highlight-compress` beside `none` is a usage error — a knee width
describes nothing when there is no knee. `--highlight-compress 0` is the default
and asks for nothing, so it is accepted.

#### `reinhard` — a real operator for a reconstruction that overshoots

`none` suits a reconstruction already bounded at the render's ceiling. `reinhard`
is for the opposite case: it compresses *globally* against a stated white point, so
content several stops above diffuse white stays distinguishable instead of landing
flat on the ceiling.

```sh
hanten convert scan.tif -o out.tiff --output-preset display-p3 \
  --film-base … --display-tone reinhard --display-tone-headroom 6
```

`--display-tone-headroom` is **display-referred**: how many stops above reference
white content may sit and still be told apart, so `W = 2^stops`. `6` stops is a
white point of 64 — the value measured to beat the shipped sigmoid on both clipped
fraction *and* highlight separation on all seven reference frames, with brightness
matched so the comparison is not just "one render is darker".

What to know:

- **It is not bounded, by design — so the headroom has to be sized to the
  reconstruction.** Content above the white point still exceeds the ceiling; that
  loss is *counted* at the encode step and reported in `.loss` rather than refused,
  which is the opposite policy from `none`, which relies on the range check being
  the whole rule. Counted is not free: the loss raises a warning, and under
  `--strict` that warning is a **failure** (exit 1). A reconstruction that overshoots
  by more than the stated headroom therefore does not quietly land flat — it lands
  in `.loss`, and `--strict` turns it into a refusal. Read `.loss.clipped_high`
  against `.loss.total_samples` on a representative frame and raise
  `--display-tone-headroom` until the fraction is what you intend; the default `6`
  is sized for the shipped sigmoid, not for an arbitrary curve.
- **`0` stops is the exact identity, on every preset.** `W = 1` makes the operator
  `v`, so `--display-tone reinhard --display-tone-headroom 0` renders
  **byte-identically** to `--display-tone none` — verified on `display-p3`,
  `hdr-linear-tiff` and `hdr-pq-tiff`. On the SDR presets the two still differ in
  range policy (`none` refuses an overshoot, this counts it), which is the only
  reason both exist at that setting; on the HDR presets, where this tone is
  range-checked, they match in that too.
- **A tone switch does not carry the headroom.** `--display-tone none` (or
  `shoulder`) over a recipe that pinned `headroom_stops` resolves the named
  operator, dropping the stated headroom — the flags-win reset that makes such a
  recipe re-runnable at all. That is legitimate, so it is a **warning**, not an
  error, and `--strict` promotes it; restate
  `--display-tone reinhard --display-tone-headroom <stops>` to keep the value.
  Re-naming `reinhard` itself *preserves* it.
- **Taken by every display preset** — the two SDR ones, all five single-rendition HDR
  ones, and the gain-map pair. `legacy`, `custom` and `film-master` apply no display
  tone curve at all and refuse it by name.
- **Mid-grey is preserved; diffuse white still costs about 0.86 stop.** The operator
  carries an input gain solved so that scene mid-grey (`0.18`) comes out at `0.18` at
  *every* headroom, so choosing this tone no longer darkens the midtones and there is no
  exposure correction to remember. What remains is the compression above mid-grey, and that
  is intrinsic to Reinhard rather than a bug: at the default 6 stops reference white `1.0`
  renders `0.550`, i.e. **0.86 stop at diffuse white**, and raising
  `--display-tone-headroom` does not recover it (0.858 stop at `W = 16`, 0.864 at
  `W = 64`) — the compression is what buys the headroom. It relaxes only at the very
  bottom of the range: 0.54 stop at 1 stop of headroom and **0.00 at `W = 1`**, the
  identity case above. Below mid-grey the curve *lifts* slightly (0.09 renders 0.099).
  This applies on every preset, SDR and HDR alike — the operator is global, not a
  highlight knee.
- **The HDR branches apply a different shape, not the same curve at a bigger ceiling.**
  They lift highlights toward the 1000-nit peak over an *asymptotic* base, which is what
  keeps the result **strictly inside** that peak so nothing clips on the way out.
- **The gain-map presets take it too, and what made that safe is worth knowing.** A gain
  map stores the ratio between the HDR rendition and the SDR base **as stored** — and the
  encode clamps the base at white. `reinhard`'s SDR half deliberately runs past white, so
  ratioing against the *rendered* base stored a gain short by whatever was clamped, and a
  decoder reconstructed those highlights up to **23% dark** in a file that looked
  structurally perfect. The fix was the ratio (`min(sdr, 1)`), never a relaxed check, so
  the two renditions now agree as far as the container can express.

`--highlight-compress` is a *knee* width, so it is a usage error beside `reinhard`
for the same reason it is beside `none`: there is no knee to place.

**On the default `gain-map-hdr` preset, `none` makes the gain map inert by
construction.** The examples above use `display-p3`, but the default renders *both*
branches: skipping the shoulder makes the SDR and HDR renditions carry the same
luminance, so their ratio is exactly 1.0 everywhere. Hanten still writes a valid
gain-map JPEG and exits 0 without a warning. That is the §8 known issue in its
sharpest form — the shipped default already decodes at 1.0x — so it costs nothing
today, but if you are reaching for `--display-tone none` *because* you want HDR
headroom, an SDR preset is the honest container until a reconstruction that exceeds
reference white exists to fill it.

---

## 8. Output presets

> ### ⚠️ Known issue: the default gain map is inert
>
> Under the **default sigmoid**, the HDR rendition peaks at *exactly* the 203-nit
> reference white, so `gain-map-hdr` writes a structurally valid gain-map JPEG
> whose `GainMapMax` decodes as **1.0x** — no HDR headroom. Viewers show it as a
> normal SDR image.
>
> This is a **rendering** property, not a container defect. The
> reference-anchored sigmoid pins mid-grey at half the reference density and rolls
> its shoulder so diffuse white lands *at* reference white, so by construction
> nothing exceeds it. The `exponential` curve on the same frame reaches 4.87x,
> because its default placement pins white *at* `Dmax` and has no shoulder, so
> contrast pushes values past reference white. The film is not the limitation — negative stock has
> wide latitude; the *print rendering* decides whether output exceeds diffuse
> white, and today's default declines to.
>
> So HDR is a rendering-intent choice rather than a correctness gap, and it is
> deliberately deprioritised. **Changing container does not help**: `hdr-pq` and
> `hdr-hlg` consume the same shared display source and the same HDR renderer, so
> under the default curve they too report their brightest pixel at or below 203
> nits with none of the 1000-nit headroom used.
>
> **The shoulder gates whether there is any headroom; the anchor sizes it.** The
> figures below are `GainMapMax`, measured 2026-08-28 on
> `tests/fixtures/hdr-48bit.tif` with `--film-base 1,1,1` — a *unity* base, so the
> reference-derived ones move on a real scan (see the caveat after the table).
>
> The sigmoid's shoulder runs during *reconstruction* and removes every above-white
> value before either display branch sees it, so the two renditions are identical
> and their ratio is 1.0 by construction. Remove it and headroom appears — and only
> then does the anchor decide how much content exceeds white:
>
> | config (all `--output-preset gain-map-hdr`) | `GainMapMax` |
> |---|---|
> | sigmoid, `--sigmoid-shoulder 0.6` (default) or `0.2` | 1.000x |
> | sigmoid, `--sigmoid-shoulder 0` | 4.866x |
> | `exponential`, default `white-at-dmax` at the nominal `Dmax` 1.3 | 4.866x |
> | `exponential --anchor-black-floor 0.005` | 4.866x |
> | `exponential --anchor-mid-offset 0.5` | 4.866x |
> | `exponential --d-max 1.5` | 3.738x |
> | `exponential --d-max 2.0` | 1.003x |
>
> The first three shoulder-less rows agree at 4.866x because all three **saturate
> the ceiling**, not because the anchor is irrelevant — raising it walks the number
> back, as the last two rows show.
>
> **Caveat: a realistic film base moves the reference-derived rows.** Re-measured
> with `--film-base 0.9,0.55,0.42`, the exponential's default reads **2.620x** and
> `--d-max 1.5` reads **1.052x**; the two base-derived anchors and `--d-max 2.0` are
> unchanged, as is every sigmoid row. That is the whole point of the base-derived
> rules — they carry no reference term — but it means the unity-base numbers above
> are a controlled comparison, not what your scan will report.
>
> **And the 4.87x is not usable headroom.** It is 98.8% of the 4.926x ceiling, with
> *zero* separation among everything above reference white — the speculars arrive as
> one flat blob rather than as detail. So do not reach for the `exponential` curve
> or `--sigmoid-shoulder 0` expecting good HDR; you get a live gain map carrying a
> clipped highlight. A real fix is tracked in
> [`output/display-tone-mapping`](tasks/output/display-tone-mapping.md).
>
> **`--display-tone reinhard` (§7) is the fix, and it now reaches every display
> preset** — the gain-map pair and the AVIF ones included, each with its own ceiling.
> It is what holds content several stops over diffuse white instead of flattening it,
> so a gain map built over it carries information rather than a plateau. What has
> *not* moved is the **default**: the shipped `shoulder` still removes the above-white
> content, so you have to ask for this tone (and a shoulder-less reconstruction) to
> see it.

`--output-preset` (recipe key `output.preset`) is an **atomic** policy choice: a
named preset resolves container, bit depth, and colour profile itself. `custom` is
the deliberate exception — see below.

Twelve names are accepted, **every one resolves a container**, and there is no
planned-but-unaccepted tier left, so an unknown name always means a typo. The
"Suffix" column is what a path may *state*; the **bold** spelling is the one
`hanten` writes when you leave the suffix off:

| Preset | Container | Suffix | Depth | Contents |
|---|---|---|---|---|
| `gain-map-hdr` *(default)* | JPEG | **`.jpg`** / `.jpeg` | u8 base | SDR base + gain map, packaged **dual-dialect**: ISO 21496-1 segments *and* the legacy Ultra HDR v1 XMP/MPF. |
| `ultra-hdr-v1` | JPEG | **`.jpg`** / `.jpeg` | u8 base | The **same pixels** as `gain-map-hdr`, legacy XMP/MPF only — no ISO claim. |
| `legacy` | TIFF | `.tif` / **`.tiff`** | u16 / f32 | The transitional path: print controls run before the output ICC transform. |
| `custom` | TIFF | `.tif` / **`.tiff`** | u16 / f32 | Same bytes as `legacy`; the difference is **provenance** — it says the combination was chosen. |
| `film-master` | TIFF | `.tif` / **`.tiff`** | f32 | Unclamped **linear ACEScg**, straight from the NC film RGB v1 mapping. Bypasses every print/display control. |
| `display-p3` | TIFF | `.tif` / **`.tiff`** | u16 | Modern-pipeline SDR render, losslessly stored in **Display P3**. |
| `compatibility` | TIFF | `.tif` / **`.tiff`** | u16 | The same SDR render in **sRGB**, for broad compatibility. |
| `hdr-pq` | AVIF | **`.avif`** | 10-bit | 4:4:4 Rec.2100 **PQ**. |
| `hdr-hlg` | AVIF | **`.avif`** | 10-bit | 4:4:4 Rec.2100 **HLG**. |
| `hdr-linear-tiff` | TIFF | `.tif` / **`.tiff`** | f32 | Display-linear **BT.2020**, no transfer applied — HDR interchange. |
| `hdr-pq-tiff` | TIFF | `.tif` / **`.tiff`** | u16 | The same signal as `hdr-pq`, as losslessly stored Rec.2100 **PQ** codes. |
| `hdr-hlg-tiff` | TIFF | `.tif` / **`.tiff`** | u16 | The same signal as `hdr-hlg`, as losslessly stored Rec.2100 **HLG** codes. |

> **The default writes a JPEG.** `hanten convert scan.tif -o out.tiff` *fails* —
> with no `--output-preset`, `hanten` resolves `gain-map-hdr` and wants `.jpg`. For a
> TIFF, name the preset: `--output-preset legacy` (or `display-p3`,
> `compatibility`, `film-master`, …).

`gain-map-hdr` and `ultra-hdr-v1` are **one render packaged twice** — identical
pixels, differing only in metadata dialect. Only the dual-dialect default decodes
as HDR on Apple platforms; `ultra-hdr-v1` exists for readers that predate ISO
21496-1.

### You do not have to name the container

Leave the suffix off and `hanten` supplies it from the resolved preset — that is
what the preset is *for*:

```console
$ hanten convert scan.tif -o out --film-base 1,1,1 | jq -r .output
out.jpg

$ hanten convert scan.tif -o out --output-preset display-p3 --film-base 1,1,1 | jq -r .output
out.tiff
```

The report's `output` field always names what was actually written, so a script
never has to map a preset name to a container. The sidecar follows the completed
path too (`out.jpg.json`).

Four rules make this predictable:

- **A suffix you state is never rewritten.** `-o out.jpeg` writes `out.jpeg`, not
  `out.jpg`; case is preserved as typed.
- **A dot-segment is only a suffix if `hanten` recognises the container.**
  `-o out.v2` and `-o roll-1.2` are stems, so they get `out.v2.jpg` and
  `roll-1.2.jpg`. Only `.tif`, `.tiff`, `.jpg`, `.jpeg` and `.avif` are read as a
  container request.
- **A path that names a directory is refused.** There is nothing to append to, so
  `-o positives/` and `-o positives/.` both exit 2 rather than writing
  `positives.jpg` beside the directory. Name the file inside it
  (`-o positives/out`), or use `hanten roll --out-dir positives/` for a whole roll.
  In a `roll` manifest the same applies to an `"output"` of `"."` — drop the
  `output` key instead and the frame takes its derived name inside `--out-dir`.
- **A stated suffix the preset refuses is still an error**, never a silent rename:

```console
$ hanten convert scan.tif -o out.tiff --output-preset hdr-pq --film-base 1,1,1
usage: output preset `hdr-pq` requires an output path ending in .avif — or no suffix at all, which Hanten completes for you

$ hanten convert scan.tif -o out.tiff --film-base 1,1,1
usage: the output path out.tiff does not end in .jpg or .jpeg: with no --output-preset, Hanten writes `gain-map-hdr` (see --help for the other presets, e.g. `display-p3` for a 16-bit TIFF). Hanten never renames a suffix you state — drop it and the path is completed for you
```

With `-v`, `hanten` says on stderr when it completed a path.

### Preset interaction rules

- An **atomic** preset rejects `--out-depth` / `--output-profile` / `--bigtiff`
  (from a flag *or* the recipe), because it resolves those itself. For
  `--output-profile` and `--bigtiff`, a value equal to the documented default —
  like `--bigtiff auto` — is accepted. **`--out-depth` is rejected by flag
  presence**, so even `--out-depth u16`, which *is* the default, errors alongside
  a named preset.
- **`custom` is the one named preset that is not atomic.** It accepts the
  depth/profile/container selectors, resolving the same branch and the same bytes
  as `legacy`. Use it to record that the combination was a decision.
- **`--out-depth` replaces the old `--output-hdr` / `--output-sdr` pair**:
  `u16` (default, archival) or `f32`. Only `legacy` and `custom` consult it.
- `f32` there is the **transitional print-rendered** float TIFF in the selected
  output space. It is not `film-master` (unclamped linear ACEScg, no print
  controls) and not `hdr-linear-tiff` (display-linear BT.2020) — three different
  f32 TIFFs.
- `film-master` additionally rejects `--auto-d-max` / `--auto-balance-range` and
  every non-default downstream control — it bypasses them, so accepting them
  would be a lie. The `--auto-d-max` half is **conditional on the anchor**: under
  `--anchor-black-floor` / `--anchor-mid-offset` the measured reference is discarded,
  so nothing frame-local reaches the master and the combination is accepted. Same rule
  for `roll`: it does not call such a recipe "Dmax not frozen", because it is.
- `--linear-range` is consumed **only** by a display preset — which the default
  now is, so it works out of the box. On `legacy` / `custom` it stays a loud error
  rather than a silently ignored knob.

### `roll` takes every preset

The old `convert`-only restriction is gone. `roll` derives
`<stem>_positive.<ext>` from each frame's own resolved preset, so a
`gain-map-hdr` roll writes `_positive.jpg` and an `hdr-pq` roll writes
`_positive.avif`. An explicit manifest `output` path goes through the same rule
`convert` uses — checked when it states a suffix, completed when it does not.

### Depth, profile and container knobs

These are consulted by `legacy` and `custom` only.

| Flag | Values |
|---|---|
| `--out-depth` | `u16` (default, archival) or `f32` (written verbatim, values above 1.0 preserved) |
| `--output-profile` | `sRGB`, `prophoto`, `acescg`, `display-p3`, or a path to an ICC file |
| `--bigtiff` | `auto` (default — promote only when needed), `on`, `off` |

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

The IR plane is decoded and **preserved but not acted on** by default, with one
exception that needs nothing from you:

- `--export-ir PATH` writes the decoded plane out. **`convert` only** — `roll`
  rejects `input.export_ir`, because one path cannot serve every frame, so IR
  planes have to be exported frame by frame.
- **IR-assisted film-holder detection** runs by itself when the plane can do the
  job. Hanten measures the interior IR transmission and, if the film reads
  IR-transparent, masks the opaque holder off before the auto rebate search.
  There is nothing to declare — `--film-type` does **not** gate it.

  `--film-type silver|chromogenic|unknown` still exists on `convert`, `estimate`
  and `inspect` (recipe key `input.film_type`), as a **provenance declaration**:
  it records what stock a run was made from, and nothing reads it. Set it if you
  want the film chemistry captured in the recipe or report you keep beside the
  output; leave it out otherwise. Planned IR dust removal will need the same
  declaration, which is why it stays.

  `hanten inspect` and `hanten estimate` report the verdict, and `inspect` adds the
  per-edge mask when it passes:

  ```sh
  hanten inspect scan.tif | jq -c '.ir_separability'
  ```
  ```json
  {"interior_median":0.67963684,"usable":true}
  ```

  ```sh
  hanten inspect scan.tif | jq -c '.holder_mask[0].segments[0]'
  ```
  ```json
  {"span":[0,20],"class":"film","ir":0.6301823}
  ```

  When the film itself is opaque to IR — a fully-exposed silver-halide frame, say
  — holder and film cannot be told apart, so detection falls back to RGB-only and
  says so, naming the measurement. The same happens for an IR page identified by
  shape alone (no `NewSubfileType=4` marker), which is never trusted for
  detection, and for a holder that wraps all four edges: masking it away would
  leave nothing to search, so the RGB-only search runs instead.

  Why measured and not declared: silver blocks IR *in proportion to accumulated
  density*, so an **unexposed** silver frame is IR-transparent against an opaque
  holder (~20:1) while its own **leader** is opaque throughout. Film chemistry
  mispredicts both — and on exactly the two frames you calibrate `Dmin` and
  `Dmax` from.

IR-based dust removal is not implemented.

### The measurement region (the "effective area")

A region `hanten` resolves on every frame it decodes, so that a measurement reads the
picture rather than the film holder: on an uncropped scan the holder is maximum
density, so a whole-frame statistic measures the holder instead. Today one
measurement is taken over it (`--auto-d-max`; see the end of this section) —
moving the rest onto it is separate work. The area is two cuts, in order:

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

Every command that decodes reports the result — `inspect`, `estimate`, `convert`,
and each frame of a `roll` (under its own `effective_area` key, beside that frame's
`dmax`). `convert` and `roll` resolve it on every run, so `--measure-inset` and the
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
- **It never looks for the rebate.** The inset passes over it blind. Where a
  measurement needs unexposed film, give it a region (`--base-region`) or measure a
  reference frame.

Every command that decodes resolves the area and reports it. Whether anything
**measures over** it is a separate question, and today only `--auto-d-max` does —
every other `Dmax` source is a constant or a roll-fixed calibration and reads no
pixels. So a default conversion is byte-identical to before, and `--auto-d-max` now
measures the picture instead of the holder: across 12 real frames from 6 rolls it
resolves 0.76–1.18, against a roll `Dmax` of 1.28–1.38 and the 2.23–2.37 it used to
return. That holds whatever the anchor placement does with the number, so the
reported `dmax` has one meaning.

Whether the measurement then reaches a **pixel** is narrower again: the two
base-derived placements (`--anchor-black-floor`, `--anchor-mid-offset`) discard the
measured reference, so under those the output is byte-identical to a non-auto run
even though the reported `dmax` was measured over the area. That distinction is what
the `--strict` note below turns on.

If the two cuts leave **nothing** to measure, a run that measures over the region is
refused (exit 2, or a failed frame on a roll); a run that does not is warned instead,
and its report omits `effective_area` — there is no region to report, and
`--measure-inset` has no effect on that run.

The `Dmin` / `Dmax` *estimators* still measure as they always have; moving them onto
the effective area is separate work, which is why the opening of this section says
one measurement rather than all of them.

> A scan carrying an IR plane that nothing consumes emits an "IR preserved but
> not used" warning, which **`--strict` promotes to a failure**. Two things consume
> the plane, and either one silences it:
>
> - **Film-base holder detection**, when it actually masked something: the base
>   source must be `auto`, the plane marker-verified and measured usable, *and* the
>   resulting mask must leave some film to search (a holder wrapping all four edges
>   falls back to RGB-only).
> - **The effective measurement area**, when the holder march actually moved the
>   rectangle (`holder_applied: true`) *and* the region reaches a rendered pixel —
>   today `--auto-d-max` with a reference-reading anchor. The area is resolved on
>   every run, but reading the plane and finding no holder, or measuring a reference
>   a base-derived anchor then discards, leaves the plane genuinely unused by the
>   render.
>
> So a frozen explicit `--film-base` with the default anchor — the recommended roll
> workflow — still warns. Either drop `--strict` for those runs, or use
> `--export-ir` so the plane is consumed.

---

## 10. Reports, warnings, and exit codes

The JSON report on stdout carries the run identity, the effective recipe, the
resolved film base and `Dmax`, the white balance actually used, encode loss
statistics, and warnings:

```sh
hanten convert scan.tif -o out.jpg --film-base … | jq '.loss, .warnings'
```

Clipping is reported, never silent:

```json
["output lost 126296 clipped and 0 non-finite of 695772 samples (18.15%)"]
```

Every encoder counts this, not just the TIFF ones — the gain-map JPEG and the
AVIF paths build the same report when they quantize.

Under the **default sigmoid the curve alone cannot clip**: its shoulder approaches
display white asymptotically and never reaches it. So a clip warning means
something downstream pushed samples past 1.0 — most often a **print control**
(`--print-exposure 12` clips 100% of a default-curve frame), or the `exponential`
curve, which hits white exactly at `Dmax` and exceeds it above. Check the print
controls before reaching for the curve or `Dmax`.

`--strict` promotes warnings **that reach the JSON report** to a hard error
(exit 1), after the report is emitted — the right default for scripts and CI.

> One deliberate exception: a failure to write an opted-in **telemetry**
> destination prints `hanten: warning:` on stderr but is kept out of the report set, so
> it stays fail-soft even under `--strict`. Telemetry must never change a
> conversion's outcome. A script that needs to know telemetry landed has to check
> the file, not the exit code.

### Exit codes

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Generic / unexpected error — **including `--strict` with warnings present** |
| 2 | Invalid CLI usage or parameters (bad flag value, unsupported preset, wrong suffix, bad recipe) |
| 3 | Input read/decode error |
| 4 | Unsupported variant (e.g. a channel layout not handled yet) |
| 5 | Output write error |
| 6 | Resource limit — estimated peak memory exceeds the budget |

---

## 11. Operational flags

The flags in the tables below are **not** conversion knobs: they never appear in a
recipe and can never perturb a pixel. (The transitional `--new-flow` selector at the
end of this section is CLI-only too, but for a different reason — it chooses a whole
rendering chain, so it *does* change the render.)

| Flag | Purpose |
|---|---|
| `--max-memory BYTES` | Peak-memory budget, checked **before decode**. Accepts `8GiB`, `4096MB`, or raw bytes. Default 6 GiB — a fixed value, so the pass/fail decision is machine-independent. Over budget ⇒ **exit 6**. |
| `--report` / `--report-file` | Report format (`json`, `none`) and destination |
| `-v` / `-vv` / `--quiet` | stderr verbosity — never pollutes stdout |
| `--strict` | Promote warnings to errors |

Two more are **`convert` only** — `roll`, `estimate` and `inspect` do not accept
them and exit 2 if given one:

| Flag | Purpose |
|---|---|
| `--telemetry` / `--telemetry-file` | Opt-in, fail-soft performance record (JSONL). Also `NC_TELEMETRY_LOG`. |
| `--seed N` | Reserved; nothing is stochastic today |

> **Caveat on `--max-memory`:** the budget also caps the TIFF read buffers, so a
> small-but-passing budget can turn a decodable file into an exit-3 decode failure.
> There is also a warning tier above ~70% of detected RAM — the one documented
> exception to machine-independence, since with `--strict` the same run can exit 0
> on a large machine and non-zero on a small one. The *image* is still identical;
> only the exit code differs.

On `roll`, the gate runs **per frame**: a rejected frame is recorded in the report,
its siblings are still written, and the roll exits **1**, not 6.

### `--new-flow` — the migration selector (transitional)

Hanten is migrating to the design in [`design-update.md`](design-update.md): a fixed
decode followed by named rendering stages. `--new-flow` (on `convert` and `roll`)
selects that chain. It is **scaffolding with an expiry** — when the new chain
becomes the default the flag is removed, and passing it will be a migration error.

It is CLI-only like the flags above — a recipe naming `new_flow` is rejected as an
unknown field — but **it is not in their "can never perturb a pixel" class**:
choosing a chain is a choice of pixels. That is precisely why it must stay out of
the recipe rather than merely out of the image.

**It renders a minimal picture, not a finished one.** The fixed decode feeds the new
chain, whose first three stages (scene correction, look, fit range) are still
identity passes; fit gamut only converts into Display P3 primaries. The result goes
to **one destination, a Display P3 16-bit TIFF** — there is no other, and no way to
choose one:

```console
$ hanten convert scan.tif -o out --film-base 0.9,0.55,0.42 --new-flow
hanten: warning: output lost 68772 clipped and 0 non-finite of 695772 samples (9.88%)
```

That writes `out.tiff`. Expect the clipping warning on most frames: with no fit range
yet, anything above display white is clipped at the 16-bit encode (counted, and failed
by `--strict`). Whether the picture *looks* right is not what this flow promises yet.

- **The suffix is judged against that destination**, on `convert` and on a `roll`
  manifest's explicit `output`: `.tif`/`.tiff` is kept as typed, a missing suffix is
  completed to `.tiff`, and anything else is refused:

  ```console
  $ hanten convert scan.tif -o out.jpg --film-base 0.9,0.55,0.42 --new-flow
  usage: the output path out.jpg does not end in .tif or .tiff: under --new-flow,
  Hanten writes its one destination, a Display P3 16-bit TIFF. Hanten never renames
  a suffix you state — drop it and the path is completed for you
  ```

- **No sidecar is written**, and the report carries no `recipe` echo and no
  `identity.params_hash`: all three were built around the *current* chain's config,
  which does not describe what ran. The new chain has its own recipe (below);
  carrying it in these is `nf-core/report-contract`'s to decide. A sidecar an earlier run left at
  the same path is **removed** — it describes the image just replaced — and the report
  names it in `new_flow.removed_sidecar`. A file there that is not one of Hanten's
  sidecars is left alone, and so is one this run read as its `--params` recipe (with a
  warning, since it still pairs by name with an image it no longer describes).
- **The report is provisional.** The current chain's sections (`reconstruction_result`,
  `output_render`, `dmax`, `white_balance`, …) are absent; a `new_flow` block states
  what ran instead — the decode's resolved `anchor`, `contrast`, `scale` and `offset`,
  each stage with what it `applied` (`"identity"` for the first three,
  `"acescg-to-display-p3-matrix"` for fit gamut), the `destination`
  (`display-p3-u16-tiff`) and `"sidecar_written": false`. Its final shape is
  `nf-core/report-contract`'s to decide.

What *is* live is the availability rule: a knob the new chain cannot honour is
refused (exit 2) rather than accepted and ignored, and the message says whether the
counterpart is missing **yet** or for good:

```console
$ hanten convert … --new-flow --sigmoid-shoulder 0.4
usage: --sigmoid-shoulder has no meaning under `--new-flow`: the new flow has no
counterpart for it yet — one arrives with the fit-range stage: …

$ hanten convert … --new-flow --reconstruction simple
usage: --reconstruction simple has no meaning under `--new-flow`: the new flow has
no counterpart for it, and will not gain one: … Use `--reconstruction density`, …
```

A knob can be refused by the **flag** you typed, or — in a recipe — by the new
chain's recipe schema, which has no key for it (below).

**The reconstruction knobs are fully classified**, because the fixed decode that
strands them has landed. It is one decode for every negative: a straight line in
density against log exposure, with one reference-free anchor rule. So these are
refused:

| Refused | Why |
|---|---|
| `--d-max`, `--fixed-d-max`, `--auto-d-max`, `--no-d-max` | the anchor rule never reads a reference density, so nothing resolves one. A `Dmax` measured from a leader is film *saturation* — neither diffuse white nor the density this decode pins |
| `--anchor-white-at-reference`, `--anchor-mid-fraction`, `--anchor-black-floor` | the decode has one anchor rule |
| `--density-curve sigmoid`, `--density-curve characteristic`, `--sigmoid-contrast` | the curve is no longer a choice the decode offers |
| `--film-stock` | per-stock normalization becomes an optional **rendering** step |
| `--shadow-balance`, `--highlight-balance` (non-zero) | a grade; it moves to the look stage's per-channel control |
| `--preset` | a preset sets knobs on both sides of the decode/rendering boundary |
| `--balance-range`, `--auto-balance-range` | they shape the regional balance's tone ramp, which is itself refused |

And these still work, because they *are* the fixed decode's own calibration and
anchor: `--density-scale`, `--density-offset`, `--density-gamma` (the decode's
contrast) and `--anchor-mid-offset`. So does `--density-curve exponential`, which
names what the new flow already decodes with, and a zero `--shadow-balance` /
`--highlight-balance` / `--sigmoid-toe` / `--sigmoid-shoulder` — an identity value
asks for nothing this flow cannot do. (It is *not* spared in order to let one recipe
be re-used on either chain: the new chain's recipe has no key for any of them, so
there is no pinned value for a flag to clear.)

**A recipe for the new chain is its own document.** It states
`"recipe_version": 2` and has one section per stage; `hanten params --new-flow`
prints the defaults:

```console
$ hanten params --new-flow
{
  "recipe_version": 2,
  "input": { … },
  "calibration": { "film_base": null },
  "measure": { "inset": 0.05 },
  "reconstruction": {
    "scale": [1.0, 0.84, 0.73],
    "offset": [0.0, 0.0, 0.0],
    "contrast": 2.0,
    "anchor": { "mid-at-base-offset": 0.62 }
  },
  "scene_correction": {},
  "look": {},
  "fit_range": {},
  "fit_gamut": {}
}
```

(Abridged; the real output is one value per line.) `input` and `measure` are the
current chain's sections unchanged. `calibration` holds the film base only — the
fixed decode reads no reference density. `reconstruction` spells the four decode
knobs above (`--density-gamma` is `contrast` here). The four rendering stages are
empty and refuse any key until their stage gains one. There is no `output` section:
the new chain writes one fixed destination. `--dump-params` under `--new-flow` writes this
document with your values resolved, and it reloads under the flag unchanged.

The version is what tells the two chains' recipes apart, and each refuses the other's
by name rather than parsing it and reading nothing:

```console
$ hanten convert … --new-flow --params v1.json    # no "recipe_version"
usage: recipe v1.json: under `--new-flow` a recipe must state `"recipe_version": 2`
— without it the document describes the current chain, whose sections this chain
does not read. `hanten params --new-flow` writes the new layout; or run without
`--new-flow`, where a recipe with no `recipe_version` is read

$ hanten convert … --params v2.json               # "recipe_version": 2, no flag
usage: recipe v2.json: states `recipe_version`, so it describes the new rendering
chain — pass `--new-flow` to read it. The current chain's recipe carries no version
```

Any other `recipe_version` reads on neither chain, and says so rather than sending
you to `--new-flow` for a second refusal (`states `recipe_version` 1, which no chain
reads — the current chain's recipe carries no `recipe_version` at all (remove it),
and the new chain (`--new-flow`) reads only 2`).

A versioned recipe that still carries one of the current chain's sections or keys is
refused naming it, and where its knobs went:

```console
$ hanten convert … --new-flow --params old.json   # "reconstruction": {"density": {…}}
usage: recipe old.json: `reconstruction.density` belongs to the current chain's
recipe, not the new one's: `density.scale` and `density.offset` are
`reconstruction.scale` and `reconstruction.offset`; the regional balances have no
counterpart yet (`nf-look/per-channel-grade`). Drop it — the current chain reads
it only in a recipe with no `recipe_version`

$ hanten convert … --new-flow --params print.json # "print": {…}
usage: recipe print.json: `print` is a section of the current chain's recipe, not
the new one's: the print controls are split across the rendering stages — white
balance and exposure to `scene_correction` (`nf-scene-correction/stage`), the
display tone to `fit_range` (`nf-display-stages/fit-range`) — and none has a key
there yet. Drop it — the current chain reads it only in a recipe with no
`recipe_version`
```

So every print and output **flag** is refused as well, each naming the stage that
will carry it:

| Refused | Where it goes |
|---|---|
| `--print-exposure`, `--white-balance`, `--auto-wb` | the scene-correction stage |
| `--black-point` | split in two — flare/fog in scene correction, display black in fit range — which is why it is not a rename |
| `--linear-range` | an affine levels remap needing a stage and a name; retiring it outright is a listed outcome |
| `--display-tone`, `--display-tone-headroom` | the fit-range stage, which is an identity pass today |
| `--highlight-compress` | the knee width of the `shoulder` tone specifically (`none` and `reinhard` refuse a non-default value outright); fit range compresses against the display's peak and has no knee width to set |
| `--output-preset`, `--out-depth`, `--output-profile`, `--bigtiff` | the new flow's destination set (`nf-destinations/preset-set`) — it writes one destination today, so there is no output policy to choose |
| `--telemetry`, `--telemetry-file` | the new chain's report and telemetry shape — the record would name the current chain's preset and timing buckets |

Unlike the decode's knees, **no value is spared here** — `--white-balance 1,1,1` and
`--highlight-compress 0` resolve the documented defaults and are still refused. An
identity value is normally left alone so a flag can clear what a recipe pinned, and
the new chain's recipe has no `print` section, so there is nothing to clear.

What survives untouched is everything before the seam: `--film-base`,
`--base-region`, `--auto-base`, `--measure-inset`, `--input-transfer`,
`--input-meaning` and `--film-type`. Decode, film base and the measurement region
are shared by both chains.

`--export-ir` works too: the IR plane is written from the decoded image at the
destination's depth, 16-bit.

On `roll` the flag applies to every frame: each is written as
`<stem>_positive.tiff`, with no sidecars and no `recipe` in the roll report. `roll`
takes no conversion flags, so its knobs come from the shared recipe and the
per-frame overrides. The shared recipe must be the new chain's document, and each
override is merged onto it and rendered with it, so an override uses the new
sections too (`{"reconstruction": {"contrast": 1.8}}`) and one naming a
current-chain key is refused the same way — naming its frame — at exit 2.

---

## 12. Troubleshooting

**"no film base selected"**
Neither `convert` nor `roll` has a default film base — but they take it from
different places. On **`convert`**, pass `--film-base R,G,B` (measured once per
roll), `--base-region X,Y,W,H`, or `--auto-base`. **`roll` accepts none of those
flags**: set `calibration.film_base` in the shared `--params` recipe instead.
`estimate` still defaults to auto, so `hanten estimate scan.tif` remains the way to
get a value in the first place.

**"auto film-base detection found no uniform unexposed rebate band"**
The scan has no detectable rebate — it's cropped, or the holder covers it. Measure
the base from a reference frame and pass `--film-base`, or point at a known region
with `--base-region`. Content-based estimation is planned but not shipped.

**"base-region … is not uniform (worst per-channel relative spread …)"**
Your rectangle mixes rebate with image content. Check the coordinates against
`hanten inspect`, or use `estimate --grid` on a genuinely unexposed frame.

**Heavy clipping in the report**
You are on the `exponential` curve — the default sigmoid cannot clip highlights. Display
white is probably placed too low, pushing content past it. Raise `--d-max`, move the
anchor up (`--anchor-black-floor` resolves a higher anchor than `--anchor-mid-offset`
does), switch to the default sigmoid, or use `--no-d-max` with an f32 output for a
scene-referred float result. Note a *low* anchor on the exponential is severe: measured
on real frames it blows ~21% of the frame to flat white, because that curve has no
shoulder to roll highlights off with.

**"reconstruction.curve is missing `type`"**
A partial recipe that includes the `curve` object must include its tag. Add the
type you actually intended — normally `"sigmoid"`, the default. Adding
`"type": "exponential"` also makes it parse, but it silently switches you to the
other curve and changes your pixels.

**`--strict` fails on every frame of an IR scan**
Expected — see §9: an unconsumed IR plane warns, and `--strict` promotes it. The
plane is consumed only when the base source is `auto` *and* the plane is
marker-verified *and* it measures able to separate holder from film, so a frozen
explicit `--film-base` — the recommended roll workflow — still warns. Passing
`--film-type` does not change this; it gates nothing. Either drop `--strict` for
those runs, or use `--export-ir` so the plane is consumed.

**Output differs between two machines**
Determinism is scoped to one build and architecture. Transcendental FP and the
lcms2 colour transform differ by ~1 ULP across platforms. Compare
`hanten --version` output — `pipeline_version`, commit, and target must all match.

---

## 13. Not yet available

So you don't go looking:

| Missing | Owning task |
|---|---|
| **Auto-cascade recipe generation** — a planner that produces a roll recipe for you, instead of you measuring and freezing it by hand | [`core/base-acquisition-planner`](tasks/core/base-acquisition-planner.md) |
| **Content-based film-base fallback** (`--base-content`) for cropped scans with no visible rebate | [`film-base/content-fallback`](tasks/film-base/content-fallback.md) |
| **Named conversion presets** (`--preset`) — selecting a whole reconstruction + display bundle by name instead of assembling the flags. Today each configuration is 3–5 coupled flags whose values only make sense together | [`algo/conversion-presets`](tasks/algo/conversion-presets.md) |
| **IR dust removal** | roadmap follow-up, no task file yet |

[`docs/TASKS.md`](TASKS.md) is the authoritative status for all of it.

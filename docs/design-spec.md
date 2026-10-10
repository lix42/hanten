# Hanten — High-Level Design Spec

> Language: Rust. **This file is the design.** [`design-update.md`](design-update.md)
> keeps the evidence behind it: how the decode is evaluated and tuned (its Part 3) and
> the measurements (its appendices). [`using-nc.md`](using-nc.md) is what the binary
> accepts today, and wins on that; this file wins on intent.

## 1. Purpose

**Hanten** (the binary is `hanten`) is a command-line tool that reads a **film
negative scan** (SilverFast HDR/HDRi format first) and produces a **positive
image**. Every step of the conversion is controlled by explicit CLI parameters so
that an automated agent — or a human — can drive the full pipeline reproducibly.

### What "AI-friendly" means here

This was the key clarification that reshaped the design. "AI-friendly" does **not**
mean "use AI/ML models to process the image" (auto-crop, generative restoration,
etc.). It means:

- **Every parameter of the conversion is exposed as a CLI flag.** A negative
  converter naturally has many knobs (film base, decode calibration, white balance,
  exposure, contrast, grading, display fitting, destination). All of them are
  addressable from the command line.
- **The tool is deterministic and scriptable.** The same inputs and parameters
  always produce the same output. No hidden state, no interactive prompts in the
  conversion path.
- **Machine-readable I/O.** Parameters can be loaded from / dumped to a JSON
  "recipe" file, and the tool can emit JSON reports (estimated values, warnings,
  metadata) so an agent can read results and adjust on the next call.

The deterministic core owns the image science. Any future ML assistance (see
§12 Roadmap) is strictly opt-in and sits *around* this core, never replacing it.

## 2. Scope

### In scope

- Read SilverFast **HDR (48-bit RGB)** and **HDRi (64-bit RGB + infrared)** scans.
- Parse the IR channel. Its one reader is the holder cut of the measurement area
  (§6.1); the decode drops it after that.
- Convert negative → positive in **32-bit float linear buffers** whose domain is
  always explicit: scanner measurement RGB through `Dmin`/density, the typed
  `FilmRgbImage` after the decode, and typed linear ACEScg after the NC film RGB v1
  mapping, which every rendering stage works in.
- **One reconstruction: the fixed, stock-agnostic decode** (§7). The curves it
  replaced are listed in §7.3.
- **A staged rendering chain** — scene correction, look, fit range, fit gamut (§6) —
  into a **destination set** (§5): SDR TIFF, HDR float or PQ/HLG TIFF, a gain-map
  JPEG, and the unrendered film master. AVIF was removed
  ([`design/avif-removal.md`](design/avif-removal.md)).
- **Measure once per roll** what a roll shares — its film base, white balance, white
  and exposure — and freeze it into a recipe (`measure-base`, `measure-roll`).
- All conversion parameters controllable via CLI flags and/or a JSON recipe file;
  JSON report output.

### Out of scope — see §12 Roadmap

- IR-based dust/scratch removal.
- Black & white film, incl. single-channel gray scans (`io/gray-primary-decode`,
  `algo/bw-support`).
- Camera RAW (Bayer/X-Trans) input, DNG processing.
- ML/AI assistance of any kind (auto-crop, neutral-patch detection, inpainting).
- GUI, scanner ICC profiling workflow.
- Output beyond the destination set (§5): the SDR JPEG is a planned row; PNG/EXR and
  HEIC gain maps are roadmap items.

## 3. Design principles

1. **Separate capture from rendering.** The scan is an archival record of
   transmitted light, not just an image to invert. The pipeline keeps a clean
   linear capture representation separate from everything that renders it.
2. **Reconstruction is a fixed decode; rendering owns the picture.** Every negative
   is decoded the same way, as a traditional print sees it: stock-agnostic, losing
   nothing it can avoid losing, reporting what it could not recover. Every choice
   about how the picture should look — exposure, white balance, contrast, grading,
   per-stock normalization — belongs to the rendering stages after it, never to the
   decode. This is the single most important architectural rule for colour
   fidelity (§7).
3. **Float-first, explicit-domain internal pipeline.** Image buffers are linear
   `f32`, but "linear" does not imply one colour space: values are scanner
   measurements before the decode, NC film RGB after it, and linear ACEScg from the
   working-space mapping on. No stage clamps; bit-depth reduction and range
   clamping happen only at the final encode, which counts what it clamped.
4. **Deterministic and reproducible.** Same inputs + same params ⇒ identical output.
5. **Every knob is a flag.** No conversion behavior is reachable only through code.
6. **Pure functions over classes.** Each pipeline stage is a pure function
   `(input, params) -> output`, deterministic in its image output and free of
   filesystem access. The CLI layer is the only orchestrator. *One narrow
   exception:* a stage clock reads a monotonic wall clock to fill the telemetry
   record's per-stage timings — a report-only channel that leaves the pixels
   untouched.
7. **Fail loudly, never silently.** Bad input, clipped data, or impossible
   parameters produce explicit errors/warnings with non-zero exit codes — never
   a quietly wrong image.
8. **One recipe per roll, not per frame.** What a roll shares — `Dmin`, its white
   balance, its white (which sets the contrast) and its exposure — is measured once
   per roll and then *frozen*: Hanten does not auto-optimize each frame to its own
   content. Frames from one roll stay comparable, and a difference between two of
   them is a difference in the scene rather than in the tool's reaction to it. This
   is deliberate and it is the largest behavioural difference from per-frame
   converters — measured against Negative Lab Pro on three frames of one roll,
   Hanten's `p95 − p5` moves **0.96 stops** where NLP's moves **4.3–4.5** (see
   `docs/progress/analysis.md`, `analysis/nlp-comparison`). A frame's own statistics
   read a sunset as a cast, which is why per-frame auto white balance retired. Per-frame
   adjustment returns only as bounded preferences, each reported and each with its own
   off switch (§6, "Corrections and preferences").

## 4. Input formats

### SilverFast HDR / HDRi (TIFF-family)

SilverFast HDR/HDRi files are TIFF-family containers holding high-bit-depth,
linear (raw-ish) scanner data:

| Variant | Channels | Bit depth | On-disk layout |
|---|---|---|---|
| HDR   | R, G, B            | 48-bit (16/ch) | Single IFD: 3-sample chunky RGB, no IR. |
| HDRi  | R, G, B + IR       | 64-bit (16/ch) | IFD0 = 3-sample RGB (as HDR); a 1-sample grayscale IR plane in a later IFD. High-res scans also embed a reduced-resolution RGB preview IFD between them. |

The tool reads both. On HDRi input the IR plane is parsed and kept until the decode;
on HDR input there simply is no IR channel.

**On-disk layout (verified against real sample files, 2026-06):** these are
uncompressed little-endian ClassicTIFFs, `PlanarConfiguration=1` (chunky), 16-bit
**unsigned** samples (no `SampleFormat` tag). The IR channel is **not** a 4th
sample interleaved into the RGB pixels — HDRi files carry it as a **separate IFD**
(`NewSubfileType=4`, `Photometric=BlackIsZero`, `SamplesPerPixel=1`,
`BitsPerSample=16`) at the same dimensions as IFD0. High-resolution scans also
embed a **reduced-resolution RGB preview** IFD (`NewSubfileType` bit 0) between the
RGB image and the IR plane, so the IR plane is not always the second IFD; the
decoder skips previews (by their reduced dimensions) and locates the IR plane by
its full-resolution grayscale shape. So it distinguishes HDR from HDRi
**structurally** — by the presence of that IR image — not from metadata: the
`Silverfast:HDRScan="Yes"` XMP flag is present on *both* variants and cannot be
used to detect IR.

**Caveat (carried from research):** there is still no published low-level spec for
the SilverFast layout; the above is reverse-engineered from sample scans. The
reader degrades gracefully — recognized-but-unhandled layouts return an
`Unsupported` error, and what was found is logged via the JSON report.

### Internal representation

After decode, the image is normalized to **linear `f32` scanner RGB measurement
coordinates** in `[0,1]` (plus an optional `f32` IR plane). These values are not
silently Rec.709, sRGB, ACEScg, or another colorimetric working space. The input
semantic resolver (`pipeline::input_semantics`, task `input-data-semantics`)
verifies the transfer encoding and measurement meaning as two independent axes
before Dmin/density — only a supported linear transfer paired with scanner-device
meaning enters the pipeline, and ambiguity fails loudly (§9 Input/decode);
nothing in the negative algorithm needs to know the on-disk container.

### Terminology & value domains

**Read this before using "high", "low", "bright", or "dark" anywhere in the code
or docs.** A pixel passes through several value *spaces* between scan and output,
and one runs **backwards** relative to the others, so an unqualified "high value"
is ambiguous. Everything below is **per channel** (RGB) — each pixel carries three
values in every space; the lone exception is the anchor `A` (a scalar, below). The IR
plane is a separate single channel, carried but not consumed (§6.1).

| Space | Meaning | "Higher" means | Range (`f32`) | Where in code |
|---|---|---|---|---|
| **transmission** (raw scan value) | fraction of light the film passes | more transparent film, thinner negative, brighter pixel *in the raw scan* — a **darker** scene | `[0, 1]` (= `u16`/65535) | `io::decode`, `LinearImage.rgb` |
| **film base / `Dmin`** | the unexposed film's transmission — the per-channel *relative* maximum transmission | (the ceiling of transmission) | `(0, 1]` | `FilmBase`, `film_base::estimate`, `film_base::measure_area` |
| **density `D` / `D′`** | `D = −log10(scan / Dmin)`, log-scale opacity; `D′ = scale·D + offset` (per-channel corrected density, §7.2) | **denser** negative — a **brighter** scene | `D`: `0` at base, `≈ [0, 6]` (slightly `< 0` if a pixel out-transmits the base); `D′` shifted by the offset | inside `algo::fixed::decode`, fused per sample (never a buffer) |
| **NC film RGB v1** (`FilmRgbImage`) | the fixed decode's positive: how much light each dye layer received, declared linear Rec.709/D65 (§7.5) | **brighter** positive — a **brighter** scene | unclamped `f32`; mid-grey `0.18` at the anchor rule's placement | `algo::FilmRgbImage`, `algo::fixed::decode` |
| **linear ACEScg** (`AcesCgImage`) | NC film RGB v1 mapped into linear ACEScg/D60; keeps film/lens/development/scanner character and is not physical scene recovery. The working space of every rendering stage up to fit gamut | **brighter** rendered value | unclamped `f32`; the datasheets' diffuse white decodes to ≈0.80 here, and reaches `1.0` only after the look's contrast (`algo::fixed::DIFFUSE_WHITE`) | `pipeline::working_space`, then the stage modules of `pipeline::chain` |
| **display-referred positive** | after fit range and fit gamut: display-linear, in the destination's gamut, `1.0` = reference white (203 cd/m²) | **brighter** rendered value | `≥ 0`; content past fit range's white point exceeds the peak and is clamped at the encode (at the HDR hand-off on an HDR destination), counted | `pipeline::fit_gamut::DisplayReferredImage` |
| **output sample** (terminal) | the written image value | brighter | destination-defined integer or float encoding | `io::encode`, `io::iso_gain_map` |

**The one rule.** As the depicted **scene luminance rises**:
`transmission ↓ · density ↑ · positive ↑ · output ↑`. Transmission is the only
axis that falls.

**"bright" / "dark" / "highlight" / "shadow" always mean the *scene's*
luminance** — never a raw pixel value. A scene highlight is the **densest**
negative and the **lowest** transmission; a scene shadow (including the unexposed
base) is the **thinnest** negative and the **highest** transmission. So in any
mixed or ambiguous context never call a high-transmission value "bright" — say
"high-transmission". (A module working *purely* in the raw-scan transmission
domain may adopt a local "'bright' = raw-scan transmission" convention, stated
with an explicit §4 cross-reference.) When
naming a numeric value, name its space: "high density", "high transmission",
"bright positive".

**The film-base paradox.** The unexposed base is at once the **highest
transmission**, **zero density** (`D = 0`), and renders to **near-black positive** —
it depicts scene black. *Brightest in the scan = darkest in the positive.*

**`Dmin`** is the film base **transmission** — the divisor the conversion anchors
on. It is the per-channel *relative* maximum (no **genuine picture** pixel
out-transmits it — dust, specular highlights, hot pixels, or noise can, which is
why `D` can dip `< 0` and why the decode floors the scan at `SCAN_FLOOR`),
**not** a value near 1: the orange mask and scanner gain pull channels down (real
Ektar base ≈ `[0.53, 0.26, 0.16]`, blue near the bottom). Named for minimum
*density* but stored as a transmission.

**The anchor `A`** is the corrected density `D′` that renders to positive `1.0`. It
is a **scalar** pooled across channels (a per-channel anchor would apply three gains
in `10^(lin·(D′ − A))`, i.e. a white balance, which is scene correction's job) and
is **derived from the film base**, never measured: `A = d + 0.745/linearization` with
mid-grey at `d` above the base (§7.2). It is reported, not an input.

**`Dmax`** (historical) named the roll-fixed **reference density** the anchor used to
be placed from — a nominal constant, a value measured from a light-struck leader, or a
per-frame percentile. `nf-retire/dmax-machinery` retired it: **it is no longer an input
anywhere**, and nothing measures, reads or reports it. ⚠️ Distinct from classic
photographic film `Dmax` (the negative's physical maximum density), from diffuse white
(a datasheet reference number, `d + 0.36`; `1.0` in the graded image, after the look), and from a roll's content white (not
measured) — `src/algo/fixed.rs`'s "Five quantities" table keeps them apart.

**Domain glossary.** *rebate* — the unexposed film strip between holder and
picture; maximum transmission, zero density. *holder* — the opaque scanner carrier;
near-zero transmission (`< 0.05`). *effective area* — a frame minus its measured
holder and a static inset (§9 `measure`); `measure-base` reads an unexposed frame's
`Dmin` over it. *base-region* — a user rectangle sampled for `Dmin`
(`FilmBaseSource::Region`). *scene white / scene black* — the brightest /
darkest depicted scene luminance (highest / lowest `D′`). *display (paper) white /
black* — the output extremes (`1.0` / `0.0`). *uniform / spread* — a stated region is
uniform when every channel's relative spread `(p97 − p10) / p97 ≤ 0.15`; an effective
area passes when `(p90 − p10) / p50 ≤ 0.5`, a coarse "is this a picture?" guard.
"Spread" is that figure.


## 5. Output: the destination set

**A destination is four separate axes, not a name per combination**
(`crate::destination`): the dynamic **range**, the **transfer** the samples are stored
with, the **gamut** they are rendered into, and the file **container** — or the
**film master**, which runs no rendering and so is not on the axes at all.

| Axis | Values | Flag | Recipe key |
|---|---|---|---|
| range | `sdr`, `hdr` | `--range` | `output.display.range` |
| transfer | `native` (the gamut's own curve), `linear`, `pq`, `hlg` | `--transfer` | `output.display.transfer` |
| gamut | `display-p3`, `adobe-rgb`, `srgb`, `bt2020` | `--gamut` | `output.display.gamut` |
| container | `tiff`, `jpeg` | `--container` | `output.display.container` |

Most combinations are nothing the code can write, so the set is **one table**
(`destination::ROWS`), and everything that has to agree with it reads it: resolution,
the refusals and their remedies, and the container an output path is judged against.

| Range | Transfer | Gamut | Container | Writes |
|---|---|---|---|---|
| sdr | native | display-p3, adobe-rgb, srgb | tiff | one SDR rendition, 16-bit integer, the gamut's own curve |
| hdr | linear | display-p3, adobe-rgb, srgb, bt2020 | tiff | one HDR rendition, display-linear 32-bit float, no transfer |
| hdr | pq, hlg | bt2020 | tiff | a Rec.2100 signal as full-range 16-bit codes |
| hdr | native | display-p3, srgb | jpeg | an 8-bit SDR base plus a per-channel ISO 21496-1 gain map to the HDR rendition |
| sdr | native | display-p3, srgb | jpeg | *planned* (`output/sdr-jpeg-preset`); refused naming the task |
| — | — | — | tiff | **film master** (`--film-master`, `"output": "film-master"`): the decode's linear ACEScg, unclamped 32-bit float, ACEScg profile, no rendering stage |

**Unset axes are derived, stated ones never overridden.** An axis left unset is
derived from the table, in the order range, transfer, gamut, container: its default
when a row consistent with everything decided so far has it, else the one value
left, else a refusal listing the choices. So `--transfer pq` alone is an HDR BT.2020
TIFF, and `--transfer linear` alone asks for the gamut. A stated value is what the
user asked for: `--gamut display-p3 --transfer pq` is refused rather than
reinterpreted. Derivation ignores whether a row is ready, so a command names the
same row on every build, and a planned row is refused *after* resolution. The
report records every resolved axis (`chain.destination`), which replays exactly.
A removed value — `avif` — is refused by name as a flag, a recipe value or an output
suffix (`Axis::REMOVED`), so an old recipe never reads as a typo.

The rendering (§6) supplies the defaults:

- **`default`** — SDR, `native`, Display P3, TIFF: the **product default** since
  `pipeline_version` 8. `--range hdr` alone resolves to the gain-map JPEG.
- **`direct`** — HDR, `linear`, Adobe RGB, TIFF, with the **container derived
  first**, so a lossy container comes only from a stated `--container` or from stated
  axes that leave no lossless row. `--rendering direct --range sdr` is the Adobe RGB
  16-bit TIFF.

**The output path.** `-o` is required and is never silently renamed. A suffix it
states must be one the resolved container accepts, or the run is a usage error
naming the accepted suffixes. A suffix it omits is **completed** with the
container's canonical spelling (`tiff`, `jpg`) — appended, never
substituted. A trailing dot-segment counts as a suffix only when *some* container
accepts that spelling, so `out.v2` is a stem (`out.v2.tiff`), while `out.jpg` under
a TIFF destination is refused. A path with nothing to append to — one naming a
directory — is refused rather than completed (`cli::resolve_output_path`,
`Unappendable`). `roll` derives `<stem>_positive.<ext>` from each frame's own
destination; an explicit manifest path goes through the same rule as `convert`.

**Metadata.** Each file embeds the profile its pixels are in: the gamut's ICC
profile on the SDR TIFF, the gain-map base and the linear float TIFF; a
`cicp`-tagged A2B profile on the PQ/HLG TIFF; the ACEScg profile
on the film master. The renderer already produces pixels in the destination's
primaries, so the encode applies only the transfer, never a second gamut transform.
**No sidecar is written** (since `pipeline_version` 8): the report carries the
resolved `recipe`, which reloads through `--params`, and its `identity`. A sidecar an
earlier build left beside the output is removed, and the report says so
(`chain.removed_sidecar`). The HDR TIFF reports' `hdr_linear_tiff` /
`hdr_coded_tiff` blocks are **authoritative** for reference white, peak and
headroom, which no ICC profile can express. The recipe is deliberately not embedded
in the image container; the report carries it. Per-destination encoding details are in §9
"Output / encode".

## 6. Pipeline architecture

The conversion is a linear sequence of pure-function stages, each with its own
parameters, orchestrated by `cli::convert_frame`:

```text
decode ─ input semantics ─ film base ─ fixed decode ─ NC film RGB v1 → linear ACEScg
  │  (stage 1)  (stage 1b)   (stage 2)     (§7)            (§7.5)
  │
  ├─ film master ─────────────────────────────────────── encode (f32 ACEScg TIFF)
  │
  └─ scene correction → look ─┬─ fit range(peak 1) → fit gamut ─┐
                              └─ fit range(peak P) → fit gamut ─┴─ encode → package
     └──────── rendering, scene-referred ──┘ └── display-referred ──┘   └── output ──┘
```

| Stage | Job | Module |
|---|---|---|
| **decode** | SilverFast HDR/HDRi → linear `f32` scanner RGB (+ IR) | `io::decode` |
| **input semantics** | prove the samples are linear scanner measurements, or refuse (§9) | `pipeline::input_semantics` |
| **film base** | the per-roll `Dmin`, stated or read from a stated region (§9) | `pipeline::film_base` |
| **fixed decode** | density → the negative as a print sees it; `FilmRgbImage` (§7) | `algo::fixed` |
| **NC film RGB v1** | the pinned 3×3 into linear ACEScg; `AcesCgImage` (§7.5) | `pipeline::working_space` |
| **scene correction** | photographic corrections toward what the scene was: the roll's midtone neutral, white balance, exposure. Scene-referred, linear | `pipeline::scene_correction` |
| **look** | creative and optional: contrast, saturation, the per-channel grade, highlight desaturation; later print emulation and per-stock normalization. Scene-referred | `pipeline::look` |
| **fit range** | fit the scene's range into the display's, with the display's peak as the one per-destination argument; place display black (*tone mapping*: "tone" means brightness, not colour) | `pipeline::fit_range` |
| **fit gamut** | change primaries into the destination's gamut and move out-of-gamut colour to its boundary, keeping hue | `pipeline::fit_gamut` |
| **encode** | transfer function, quantization, counting clamped and non-finite samples, warning on a channel written as 0 everywhere | `pipeline::color`, `io::encode`, `io::jpeg` |
| **package** | container, ICC profile or CICP, metadata; for the gain map, the map itself | `io::*`, `pipeline::gain_ratio`, `pipeline::gain_encode`, `io::iso_gain_map` |

**The order is carried by the types.** Each boundary is a type only the stage before
it can mint — `FilmRgbImage`, `AcesCgImage`, then `SceneReferredImage`,
`GradedImage`, `RangeFittedImage` and `DisplayReferredImage`, each in the module of
the stage that produces it — so a chain composed out of order does not compile, and a display stage cannot be
handed the master. Crossing a boundary moves the buffer, so a type per stage costs no
allocation.

**Constraints the order carries:**

- **Everything that shapes contrast or colour sits before the SDR/HDR split.** A gain
  map requires its two renditions to agree below diffuse white; if contrast or
  character lived in fit range, whose argument is the display's peak, the midtones
  would disagree. So the chain splits after the look, and the branches differ in one
  argument: the peak. Fit range runs once per branch, but its headroom and display
  black are fixed above the split, with the film base graded once for both.
- **The branch contract** (`pipeline::chain`, checked bit for bit by
  `chain::contract::check`): below diffuse white the two renditions are identical
  except where the SDR cube binds a saturated colour the HDR display can show — real
  colour a per-channel gain map carries. Above diffuse white fit range lifts toward
  the peak. A single-rendition destination renders one branch through the same
  function, so skipping a branch skips a call, not a code path.
- **Fit range and fit gamut are separate but coupled.** Fit gamut's ceiling is
  `max(peak, Y)`, the peak being fit range's, so they stay adjacent.
- **Encode and package are separate.** The transfer encode (`hdr::encode_transfer`)
  produces the signal; the container only stores it. The gain map is the one thing
  spanning the boundary: it needs both renditions, then is built into the package.
- **Contrast lives in the look.** The decode keeps only the calibrated linearization
  of the film (§7.2); print contrast is a look knob. Print emulation is a look too,
  and per-stock normalization an optional one.

**Rules every stage keeps.** 32-bit float in a linear working space; no stage clamps
— range clamping happens only at the encode, which counts clamped and non-finite
samples into the report's `loss` for a warning. Per-pixel maps go through
`pipeline::pixels`, and floating-point reductions run in a fixed order, or output
stops being byte-identical. Every standards-based matrix, luma vector and transfer
constant lives in `pipeline::colorimetry` (`docs/colorimetry-maintenance.md`).

### The rendering stages

The formulas and each knob's range are in §9; this is what each stage is for.

- **Scene correction** applies the roll's white balance and exposure, and the
  user's adjustments to them, as gains on ACEScg after the 3×3. White balance is a
  measurement per **roll**, never per frame (§3.8): a frame's own statistics read a
  sunset as a cast. Before the gains it removes the cast a poor development leaves in
  the midtones, which grows away from the roll's p99 where the gains are exact — one line
  per roll against scene stops (`nf-scene-correction/midtone-neutral`). There is no
  flare or fog subtraction: base fog is in the measured
  film base, lens glare is part of the photograph, and display black does the black
  point's one job (`nf-scene-correction/flare-removal`).
- **The look** owes four controls the old chain never had as controls:
  - **Contrast** — the print-contrast half of the old single `gamma`, as a slope
    pivoted at mid-grey, on luminance only. Its base is the roll's, set by the roll's white (§8 `roll`), or
    a thin frame's own slope (`roll.thin_slope`), and the knob multiplies it, so one taste carries across rolls as one number.
  - **Saturation** — colour's own slope: log channel ratios stretched, luminance kept,
    so contrast and colour move apart. Its base is the base slope (never a thin frame's)
    times the rendering's saturation, and the knob multiplies it (§9 `look`).
  - **A per-channel grade pivoted at mid-grey** — the photographer-facing counterpart
    of the decode's `density.scale`. Acting on working-space channels after the 3×3,
    it corrects a cast that grows with brightness, which white balance cannot. Pivoted
    so a neutral mid stays neutral, with luminance restored so it never moves neutral
    contrast. It replaced the regional shadow/highlight balance, and does not replace
    the calibration.
  - **Highlight desaturation (a path to white)** — chroma pulled toward neutral as a
    near-neutral pixel approaches diffuse white, as paper's per-channel curves do. A
    luminance-preserving operator cannot converge channels, so without it a cast
    survives to display white. It is a look — parameterised, and off under `direct`,
    because it hides residual cast, which is right for a print and wrong for a
    diagnostic. Anchored to diffuse white, which is scene-referred and common to both
    branches.
- **Fit range is reinhard**, mid-grey preserving, with an HDR lift above diffuse white
  toward the peak (`nf-display-stages/fit-range`). The bounded tones (`shoulder`,
  `none`) retired: both existed for reconstructions already bounded at white. With
  mid-grey, the rendered white and the peak pinned, any smooth shoulder stays within
  about 0.15 stop of reinhard, and a brighter white lost in review
  (`nf-display-stages/parametric-shoulder`). Below mid-grey reinhard is nearly a gain,
  so it leaves the dark end to **display black**: a shift in stops on luminance, whole
  at the film base and below and fading to nothing at mid-grey, keyed on where the
  decoded film base renders after the frame's grade. It is neither a subtraction
  (which crushed frames to code 0) nor a toe inside the operator (which lost on colour
  and shadow contrast) (`nf-display-stages/parametric-operator`).
- **Fit gamut** is one radial map toward neutral at constant luminance, against the
  cube `[0, max(peak, Y)]`, so highlights desaturate toward white instead of ringing.
  Under reinhard it was near-inert where it matters on the removed chain's SDR renderer
  (`docs/reports/gamut-map-share.md`); the HDR branch is unmeasured (`nf-calibration/white-rule-hdr`).
  It has no knob.

### Two renderings: `direct` and `default`

A render has three kinds of input:

```text
decode   fixed, plus calibration.film_base
roll     measured once per roll by `hanten measure-roll`
style    the user's taste
```

The measured values live in their own recipe section, `roll` (§8), so a measured
value is never mistaken for a chosen one. `--rendering` chooses the base every stage
knob starts from, and each has one principle (`crate::rendering`):

- **`direct` loses as little information as possible and applies only what the
  container needs.** It does not apply the roll section. It is the handoff to an
  external editor and — stated SDR — the rendering the decode's calibration loop
  holds fixed (`design-update.md` Part 3).
- **`default` is what our code produces from measured values**: the roll section
  applied, the preferences that won review on (each reported, each with its own off
  switch: "Corrections and preferences" below), and today's defaults for everything
  else.

| | `direct` | `default` |
|---|---|---|
| roll section | not applied, and reported as not applied | applied |
| white balance | identity | the roll's gains |
| base slope (`look.contrast` and `look.saturation` multiply it) | ≈1.414, pinned | the roll's, else the fallback with a warning: the roll's white placed as if +1.75 stops, ≈1.414 |
| saturation over the base slope's colour (`look.saturation` multiplies it) | 1, pinned | 1.15 |
| highlight desaturation | off | 0.8 |
| display black | 6 stops below mid-grey, pinned | 6 |
| fit range | reinhard at 6 stops of headroom, pinned | 6 stops |
| unset destination axes | HDR, `linear`, Adobe RGB, TIFF, container first (§5) | SDR, `native`, Display P3, TIFF |

- **Display black is on in `direct`** because it does not compress: a monotone
  stretch below mid-grey loses almost nothing at 16 bits. Its reference is each
  frame's decoded film base, so in the calibration loop it evens out part of a shadow
  difference between two decode candidates.
- **`direct` defaults to HDR** (range matters more than gamut): the linear float TIFF
  has no transfer and no quantization. It differs from the film master by what
  rendering does — the contrast, fit range at the HDR peak, display black, the gamut
  map — and it clamps at the peak, counted. Its SDR form is the one viewed by eye.
- **"Pinned" means `direct`'s own constants** (`rendering::DIRECT`), never today's
  defaults read through, so moving a default does not move the rendering the
  calibration loop holds. The resolver destructures every stage section without
  `..`, so a new stage knob fails to compile until it has a `direct` value, and
  `direct_is_pinned` fails on any change to `DIRECT`. When either happens: decide the
  knob's `direct` value by the principle (needed to land in the container: the
  gentlest fixed value; a choice of taste: its identity; information-preserving: it
  may stay), update `DIRECT`, the test and the table above together, and if
  `direct`'s output moved, log it as a dated entry in `nf-calibration`'s progress,
  since review rounds before and after no longer compare like for like. A task that
  changes a stage decides `direct`'s part as its own work.

### Corrections and preferences

Every value Hanten sets on its own is one of three kinds
(`nf-calibration/taste-vs-quality`). A **correction** brings the scan back to what the
film recorded, on the whole roll alike; it has no switch, since without it the scan is
wrong rather than differently liked (`direct` leaves out the whole roll section). The
exception is a correction that can read a roll's scene as its cast — the white balance and
the midtone neutral, on a roll dominated by one colour — which has a switch, so a misread
roll can be rendered without it. Nothing detects the misreading
(`nf-scene-correction/correction-confidence`): `measure-roll` notes only when a roll has
too few frames to trust either (`confidence` in its report; advice, outside `--strict`). A
**guard** bounds a measurement, or keeps a preference off a frame it would harm. A
**preference** is a choice a viewer may not share: on by default only when it won
review, and each has its own switch at render that keeps the measured value in the
recipe. Review chose the constants of every kind, so "reviewed" does not separate them;
what does is whether the adjustment restores the roll or reacts to one frame.

| adjustment | kind | why | off at render |
|---|---|---|---|
| roll white balance (`roll.white_balance`) | correction | removes the roll's cast — stock, light, scanner — measured over every frame, so a sunset keeps its colour | `--neutral-balance off`, `roll.neutral_balance` (the midtone line goes off with it) |
| midtone neutral (`roll.midtone_line`) | correction | removes the cast a poor development leaves in the midtones, which the white balance, exact at the roll's p99, cannot; no bad frame on four reviewed rolls | `--midtone-neutral off`, `roll.midtone_neutral` |
| the midtone line's data floor (10 frames, 3 voted bands) and tint gate | guard | keep the line off a roll too short to measure it, and off strongly coloured light (a sunset-lit cloud) | — |
| roll exposure (`roll.exposure`) | correction | brings a thin or dense roll to a normal level with one gain, so frames keep their relative levels | — |
| roll white and dark end (`roll.white_stops`, `roll.dark_stops`) | correction | spread the roll's measured span, highlights (after its colour correction) to its dark end, over a fixed range of the look's stops, mid-grey pinned | — |
| the white's cap and floor; a frame clamped to the cap | guard | keep a blown frame from flattening the roll, and an underexposed roll from being stretched | — |
| flat frame (`roll_white::FLAT_SPREAD_STOPS`) | guard | keeps both lifts off one surface filling the frame | — |
| small lift (`roll.frame_exposure`) | preference | brightens a low-key frame by up to +0.3 EV; preferred on 72 of 103 frames, but leaving a dark frame dark is a choice | `--small-lift off`, `roll.small_lift` |
| thin lift (`roll.thin_slope`, `roll.thin_exposure`) | preference | raises a thin frame's white about a stop with the film base held about where its small lift renders it; review found it brighter, not better | `--thin-lift off`, `roll.thin_lift` |

What a preference carries, so a GUI can preview it with and without, and toggle it:

- **Its value stays in the recipe while off.** `measure-roll --out` writes every
  preference it measured, a thin frame's small lift beside its thin one, so turning the
  thin lift off shows the small one, not none.
- **A measurement never states a switch** (unset is on), so a `measure-roll` file
  layered last keeps an earlier `"off"`. Both switches are frame-local: a `roll
  --frames` manifest may turn one frame's off.
- **The report names it.** `chain.roll.taste_applied` lists the preferences applied, by
  their switch's key; `measure-roll`'s `small_lift` and `thin_lift` sections are
  `"kind": "taste"`.

**Explicit knobs build on the base, under either rendering.** `--white-balance`
multiplies the base gains, and `--contrast` and `--saturation` their base slopes, each
with identity 1; every other knob replaces its base value.

### The film master is the reconstruction output

The film master is the fixed decode → NC film RGB v1 → unclamped f32 TIFF with an
ACEScg profile. It runs no rendering stage, so it is the artifact on which a
reconstruction is measured, and it refuses any stage the recipe asks for (§9).

- **Numbers can be read from it directly; visual review can't**: it is linear,
  exceeds 1.0 and has no white balance. Comparing decodes by eye needs one fixed
  rendering held constant across them — `direct`.
- **The 3×3 treats the dye-layer channels as Rec.709** (§7.5). Neutrality checks
  survive, since white maps to white; per-layer slope measurements get slightly mixed.
  The cleanest measurement point is `FilmRgbImage`, before the matrix:
  `--export-film-rgb` writes it as an untagged f32 TIFF (§9).

### 6.1 IR channel handling

The IR plane (when present) is decoded beside RGB, and **the fixed decode drops it**:
no rendering stage reads it, and the chain does not carry 4 B/px for no reader. It
raises no warning and is not exported (`--export-ir` retired). Its one reader is the
**effective area** (§9 `measure`), which runs before the decode: where a marker-verified
plane **measures able to separate holder from film on that frame**, the opaque
scanner holder (dark in IR) is cut from the frame's edges, since IR-transparent film
(base, rebate, picture) reads bright. That area is what `measure-base`, `inspect` and
`measure-roll` measure over; it reaches a conversion only as a stated base.

The usability verdict is **measured, not declared** (`ir-usability-detection`).
The interior IR transmission is sampled and compared against a threshold
(2.5x the holder classifier's); below it, film and holder cannot be told apart, the
holder is not measured, and the effective area is the inset alone — a report
warning says so. `--film-type` does not gate this: silver-halide blocks IR *in
proportion to accumulated density*, so an unexposed silver frame is IR-transparent
against an opaque holder (measured ~20:1) while its own fully-exposed leader is
opaque throughout — the declared chemistry mispredicts both. An IR page identified
by shape alone is likewise not trusted to measure the holder.

The broader dust-removal stage that *consumes* the IR plane for defect inpainting
is a deliberate follow-up (§12).

*Why IR is powerful and why we defer it:* the color dye image is transparent to
infrared while physical defects (dust, scratches, hair) are opaque to it, so the
IR channel is a near-clean defect map. Acting on it requires a separate
mask + inpainting stage with its own parameters, and it does **not** work for
traditional silver B&W film (silver blocks IR like dust) or reliably for
Kodachrome. So nc reads the plane now and adds the consuming stage later, carrying
the plane as far as that stage needs it.


## 7. Reconstruction: the fixed decode

> **By default nc shows what the negative really holds; it does not optimize the
> photograph. Reconstruction is a fixed, stock-agnostic decode of the negative, the
> way a traditional print sees it. It loses nothing it can avoid losing and reports
> what it could not recover. Every choice about how the picture should look,
> including per-stock normalization, belongs to rendering.**

### 7.1 The model, and why

**A traditional colour print.** C-41 stocks are designed to a common
printing-density aim, so one paper prints them all. The per-stock compensation a lab
applies is **filtration and exposure — a gain**, not a per-channel slope and not a
curve inversion; that is the invariance the decode copies. The lab's per-roll or
per-frame adjustments, colour filtration and exposure time, are rendering's scene
correction. **Stock differences reach the print** — Ektar's negative is steeper than
Portra 400's, about half a stop of extra contrast over a four-stop scene — and the
paper does not undo them, so neither does the decode. **The negative's toe is never
inverted**: the paper prints the compressed shadows as they are. (Cinema's precedent
is split: Cineon → linear is a straight line, ADX a generic curve plus a matrix.)

**Why not per-stock inversion as the default.** Inverting each stock's datasheet
curve normalizes tone character, erasing the difference a print shows between stocks
— optimizing, not showing. It inverts a toe whose slope is ≈0.02–0.03 against ≈0.6
mid-scale, amplifying noise and film-base error 20–35× where the film recorded almost
nothing. And it needs per-stock data that cannot exist for every stock, developer and
scanner. Per-stock inversion survives as an **optional normalization in rendering**;
because the fixed decode is invertible, nothing is lost by moving it there.

**What reconstruction does not decide:** the scene illuminant (white balance), the
absolute exposure (which needs a light-meter reading the negative does not carry),
and the look.

**Known limits.** Saturated regions — near the base and on the film shoulder —
carry no information: the decode stays monotonic, invents nothing, and the problem is
reported. The decode needs a density calibration, and its target is **Status M**
(§7.4).

**Pick a reconstruction by what its output means.** Any invertible reconstruction
can be compensated by some rendering to give the same final image (the converting
transform is generally not a 1D curve, since the 3×3 sits between them, but it
exists while nothing between the stages clamps). So while rendering is free to be
redesigned, neither the final image nor information content can justify a choice of
reconstruction; with rendering *held fixed*, the final image can, which is what makes
a review set evidence (`design-update.md` Part 3). The fixed decode is chosen because
its output means one thing for every stock: the negative, as a print sees it.

### 7.2 The decode

```text
D_c   = −log10(scan_c / base_c)                measurement
D′_c  = scale_c · D_c + offset_c               calibration
out_c = 10^(linearization · (D′_c − A))        the curve, A the anchor
```

`algo::fixed::decode` runs the three steps fused, per sample, in one pass, and
returns the typed `FilmRgbImage`. **Nothing is lost by construction**: every step is
strictly increasing and unclamped in f32, so no choice of `scale`, `offset`, the
linearization or the anchor destroys information — each is undoable downstream. That
is why these settings are conventions and calibrations rather than accuracy
questions. The exceptions are a dead-pixel floor on the scan (`SCAN_FLOOR`) and
non-finite samples, passed through for the encoder to count.

**Polarity.** `D ≥ 0` grows with the film's optical density: the unexposed base
(scene black) is `D = 0`, a scene highlight large `D`. A positive must brighten as `D`
grows, so the curve is `10^(+linearization·D′)`, as in darktable `negadoctor`.

**The anchor rule: `mid-at-base-offset(d)`.** `A = d + 0.745/linearization`, so
mid-grey (18%) renders at density `d` above the film base (`0.745 = −log10(0.18)`).
At the defaults (`d = 0.62`, linearization 1.8) `A ≈ 1.034`. The rule is
**reference-free**: it reads only the film base, which the measurement step divides
out, so no leader or reference density enters the render and nothing is measured per
frame. Mid-grey is what "exposed correctly" means, and pinning mid makes contrast and
exposure independent: contrast pivots about mid instead of moving the whole image.
The anchor is reported (`chain.decode.anchor`), never an input. `AnchorRule` stays an
enum with one variant, so a future rule adds a variant instead of changing what a
number means.

**Which parameters are really rendering.** Expanding the curve:

```text
out_c = 10^(−lin·A) × 10^(lin·offset_c) × (10^(D_c))^(lin · scale_c)
        └ scalar gain ┘ └ per-channel gain ┘ └─ per-channel exponent ─┘
```

| Knob | Effect | Rendering counterpart |
|---|---|---|
| anchor `A` | one gain on all channels | **exactly exposure** — a scalar commutes with the 3×3 |
| `offset[3]` | a per-channel gain, constant at every brightness | white balance, but in film-layer space *before* the 3×3, so not the same operator (≈2.6 % apart on a neutral) |
| `scale[3]` | a per-channel exponent: how fast each channel grows with exposure | the pivoted per-channel grade — same symptom, different basis |
| linearization | overall slope | the look's contrast on luminance, its saturation slope on colour |

A counterpart acts after the 3×3, so it addresses the symptom rather than the error.
Only the products `linearization · scale_c` enter, pinned by the convention
`scale_r = 1`: "measure `scale` from neutrals, the slope from a bracket" is one
measurement split by a convention.

**Decisions.** What is decided is the *rule*; the numbers filling it are current
picks and are **expected to move** — `scale` by visual review, all of them by the
bracketed calibration frames. Moving one moves every default pixel, so it costs a
`pipeline_version` bump and a drift-gate row, planned rather than a regression.

- **`d` is a calibration, not a brightness knob** (0.62, hand-frozen as `generic-c41`'s
  mid aim; stocks measure 0.54–0.70). Brightness is set in rendering: a roll's level
  by its measured exposure (`roll.exposure`), and a low-key frame's by a small lift on
  top (`roll.frame_exposure`), and a thin frame's by a steeper slope and its own
  exposure (`roll.thin_slope`, `roll.thin_exposure`).
- **`gamma` split in two** (`nf-reconstruction/gamma-split`). Linearizing the film
  (≈1/0.55 ≈ 1.8) is calibration and stays as `reconstruction.linearization`; at 1.8
  the decode's output is scene-linear (double the exposure, double the value), which
  the film master, the roll's gains and `scale` rely on. Print contrast is the look's
  slope. The single `2.0` the removed chain shipped bundled both.
- **`d` and the linearization are fixed nominal values, not per-stock ones.** Both
  are per stock in the datasheet registry (`d` 0.542–0.699, i.e. 1.06 stops at gamma 2;
  red film gamma 0.53–0.61, `film_stock/`), and choosing either per stock would be per-stock exposure and contrast normalization
  inside a decode declared stock-agnostic. Fixed values let film speed and stock
  contrast show through, which is the faithful behaviour.
- **`density.scale` is one global value**, `[1, 0.84, 0.73]` (§9): the decode's
  calibration of the common per-channel slope error, never varied per stock, roll or
  frame. It aims at *acceptable on any stock*, not neutral on every frame; the
  remainder is the look's grade. A per-roll value in the decode would stop it being
  fixed.
- **`offset` defaults to `[0, 0, 0]` — a pick, not a closed question.** It is a real
  term (the gap between rebate density and where the three layers correspond to equal
  exposure; the datasheets carry it), but the two candidate values tried lost a
  review, and identifying one needs density varied at a single illuminant (§13).
- **No reference density.** The leader `Dmax`, the three placements that read it,
  and their flags retired (`nf-retire/dmax-machinery`); the reference build keeps
  them for comparison.

### 7.3 Retired reconstructions

Each is a migration error naming its replacement, and the reference build
(`scripts/reference-snapshot/`) still renders it.

- **`simple`** (`1 − scan/Dmin`) — an affine inversion, not a decode of anything a
  print sees (`nf-retire/sigmoid-and-simple`).
- **The sigmoid** — its shoulder was a rendering fused into the decode: it compressed
  everything above diffuse white before either display branch saw it, so the gain map
  decoded as 1.0x (`nf-retire/sigmoid-and-simple`, `pipeline_version` 6). With both
  knees off it is the exponential, which is how the fixed decode was derived.
- **The `characteristic` curve** — a stock's published curve, inverted
  (`--film-stock`, `--preset`) (`nf-retire/characteristic`). Its data stays,
  test-only, as the evidence for the decode's constants (`film_stock/`).
- **The regional shadow/highlight balance** — per-channel density offsets ramped by
  tone, unbounded and so able to fold two densities onto one; replaced by the look's
  grade (`nf-retire/regional-balance`).
- **The `Dmax` anchor machinery** — above (`nf-retire/dmax-machinery`).

### 7.4 Calibration: the part that is a measurement

- **Film base** (`Dmin`), per roll, measured from unexposed film (§9 film base).
  Dividing by it fixes the **offset**, not the **slope**: it makes unexposed film
  neutral, but each channel's density still rises at a different rate with exposure —
  an error that is zero where you normalized and worst in the highlights. Its
  contributors are the film's layer gammas, interimage effects, the scanner's
  channels, and development.
- **Toward Status M.** A print system is balanced in *printing density*, which is not
  one canonical space; Status M is the standardized densitometry that tracks it for
  camera negatives, and what the datasheet corpus is in. So the target is **scanner
  → Status M**, a 3×3 plus offset (`io/scanner-density-calibration`). What an
  achievable measurement fits is the **whole chain** — film × development × scanner —
  since a chart shot on film cannot separate them. Today's crude form is the
  per-channel `density.scale`, fitted from 31 hand-marked neutral patches over five
  rolls. What remains unexplained is green, not blue: blue's drift matches the
  datasheets, Ektar's green does not (`design-update.md` Appendix B).
- **It depends on development**, not only on stock or scanner: green splits by
  developer on one scanner, so no single value fits every roll. The decode ships the
  common ground, and the roll's remainder is rendering's.
- **We cannot measure a user's chain, only our own**, so whatever we fit ships as a
  **default prior**: the most transferable value we can justify, not the best fit to
  our rolls. A user who wants better needs a calibration *procedure*, a product
  feature (`nf-calibration/user-calibration-procedure`).
- **A per-channel gain cannot be the end state**: interimage effects make each layer's
  slope depend on the others' exposure, which is why the standard model is a matrix
  in density — and it is per stock.
- **The marked patches can anchor a level, not a slope**: their density range comes
  from illumination rather than exposure. The bracketed calibration frames
  (`analysis/calibration-frame-capture`) vary density at one illuminant, which is the
  point of them, and are the release gate (`nf-calibration/neutrality-gate`).

### 7.5 Colour, and the NC film RGB v1 contract

What the decode must keep is **character**, and not everything that differs between
stocks is character:

| Kind | Example | Where it belongs |
|---|---|---|
| neutral balance | grey prints grey | not character: every stock is designed neutral under its rated light |
| illuminant cast | tungsten light on daylight film | a scene fact; kept by the decode, decided by white balance |
| colour rendering | Ektar's saturation and hue shifts | real character, kept automatically: the layers saw the scene through the stock's sensitivities |
| tone character | Ektar prints punchier than Portra | real character; kept by the fixed decode, removed only by the optional per-stock normalization |

**NC film RGB v1** is the contract between the decode and rendering. After the
decode, "R" means how much light the red-sensitive layer received — channels defined
by the film's own sensitivities, not by any colour space. NC film RGB v1 **declares
those channels to be Rec.709 primaries with D65 white**, then applies the pinned
standard transform and Bradford adaptation into linear ACEScg/D60
(`pipeline::working_space::map_nc_film_rgb_v1`). The declaration is a convention,
not a measurement:

- **Neutrals are unaffected** — white maps to white, and any 3×3 whose rows each sum
  to 1 preserves the neutral axis. Saturation and hue depend on the declaration; a
  real characterization would come from spectral data or a ColorChecker.
- **It is the negative as a print sees it, not physical scene recovery.** The output
  is scene-linear in exposure at the calibrated linearization, and carries the film's,
  lens's, development's and scanner's character.
- **It is one pinned mapping**, a total pure matrix transform with no knob, shared by
  every conversion and every destination. The `convert` report names it (`working_mapping:
  "nc-film-rgb-v1"`); a different mapping is a new identifier (`v2`), never a silent
  change to v1.
- **Only the mapper can mint `AcesCgImage`**, so no named output can merely tag
  `FilmRgbImage` with a colour space.
- Optional measured correction profiles (`color/optional-color-correction-profiles`)
  would be explicitly selected, recorded with their identity and provenance, and
  block nothing; absence is a bit-identical no-op.

Where the 3×3 should ultimately live — reconstruction output in film-layer space,
with the move into a working space becoming scene correction's — is open (§13).

### 7.6 Interfaces

```rust
// Only algo::fixed mints a FilmRgbImage, so no raw scan or density buffer can
// impersonate film RGB downstream.
pub fn decode(image: &LinearImage, base: &FilmBase, params: &DecodeParams)
    -> Result<(FilmRgbImage, DecodeReport)>;

// The working-space boundary: total (non-finite samples pass through, counted at
// encode), so it returns the value directly. Only this mints AcesCgImage.
pub fn map_nc_film_rgb_v1(film: FilmRgbImage) -> AcesCgImage;

// The rendering chain (pipeline::chain). `film_base` is the decoded film base, one
// pixel through the same decode and 3×3, graded beside the frame for display black.
pub fn render(image: AcesCgImage, film_base: AcesCgImage, params: &ChainParams,
    clock: &mut impl StageClock) -> Result<Rendered>;
pub fn render_pair(image: AcesCgImage, film_base: AcesCgImage, shared: &SharedParams,
    gamut: DestinationGamut, hdr_peak: DisplayPeak, clock: &mut impl StageClock)
    -> Result<RenderedPair>;
```

## 8. CLI design

A single binary (`hanten`) with subcommands. The agent-facing surface is
optimized for scripting: flags for everything, JSON in/out, stable exit codes,
no interactive prompts (but `telemetry enable` / `purge` on a terminal, which
`--yes` skips).

### Subcommands

| Command | Purpose |
|---|---|
| `hanten convert` | The main pipeline: negative file → positive image at the resolved destination (§5; the SDR Display P3 16-bit TIFF by default). |
| `hanten roll` | Convert a batch of frames from one shared, frozen recipe. Per-frame outputs into `--out-dir` + a roll-level JSON report. Each frame runs the same core as `convert`. |
| `hanten inspect` | Read a scan and emit a JSON report of format, channels, bit depth, input colour, the IR usability verdict and the effective area. No `Dmin`: that is `measure-base`'s job. No output image. |
| `hanten measure-base` | Measure the film base (`Dmin`) alone; emit JSON with a reuse-ready `--film-base` flag, and with `--out` write `{"recipe_version": 3, "calibration": {…}}` for `--params`, in the §8 envelope. With no source flag it measures an unexposed frame: the per-channel median over its effective area, warning when the area is too uneven to be unexposed film; `--base-region` reads a stated rectangle instead (§9 film base). Was `estimate`, which now exits 2 naming it. |
| `hanten measure-roll` | Measure a roll's white balance, midtone line, white and exposure once, for its recipe (`nf-scene-correction/roll-white-balance`, `nf-calibration/roll-white-rule`, `nf-calibration/roll-exposure`, `nf-scene-correction/midtone-neutral`): decode every picture frame with the roll's explicit film base, pool the effective areas' pixels, and report the green-anchored gains that equalize their per-channel p99. On a roll of 10 frames or more it fits the midtone line (`--midtone-neutral auto|on|off`; `on` fits a shorter roll of 3 frames or more, if enough bands count) from the unguarded sample, after the gains, of every frame the leader guard did not empty. Each frame's white is the p97 of its pixels' brightest film-RGB channel **after the roll's colour correction** (the gains and the line, mapped back through the 3×3), in scene stops; the roll's white is the brightest at or under a cap (+2.0), raised to a floor (+1.5); a frame above the cap is clamped to the cap and disclosed. Each frame's dark end is the p1 of its ACEScg luma in scene stops, and the roll's is the p10 over its picture frames; the look's slope spreads the span from the roll's dark end to its white over 9.049 stops, mid-grey pinned, held within the cap's slope and 3.5 (`nf-calibration/span-roll-slope`); a clamped frame's spreads the cap's span instead. The roll's exposure is measured independently of the white, which stays measured at exposure 0: it brings the median of the frames' log-average ACEScg luma (over pixels with positive luma) to mid-grey, within ±3 EV (a bound that binds warns). Reported as a reuse-ready `--roll-white-balance … --roll-white … --roll-dark … --roll-exposure …` flag (with `--roll-midtone-line …` when a line is written); `--out` writes the whole measurement as one recipe — `calibration`, the `roll` section (`nf-calibration/roll-section`) with `roll.frames` giving each clamped frame, by file name, the cap as its white and each lifted frame its lifts (a low-key frame's small lift, and a thin frame's slope and exposure beside it; none on a flat frame; `--no-small-lift` leaves out the small lift, `--no-thin-lift` the thin one, which is solved from the small-lifted render either way), the decode (`reconstruction`) it measured through, and the input and measure sections when stated — in the §8 envelope, that `roll --params` renders alone. `--unexposed` measures the film base first, exactly as `measure-base` does with no source flag, and is refused beside any other statement of the base (`core/measure-base`). `--leader` leaves out any pixel within 0.1 density of the leader from the gains, so a fully exposed frame cannot set them, leaves a frame it empties out of the exposure, and warns on a frame whose decoded white is within 0.5 stop of it (near film saturation); without it the run warns and nothing is checked for saturation. `--unexposed auto` and `--leader auto` find those frames among the inputs and leave them out of the pictures, recording each input's class in the report's `references`; a roll with no trustworthy unexposed frame is refused (`core/auto-calibration`). `confidence` grades the white balance and the line by frame count: `confident` from 16 frames, else `in-doubt`, with one advisory note (`confidence.advice`; not a warning, so `--strict` ignores it) (`nf-scene-correction/correction-confidence`). |
| `hanten telemetry` | Opt-in upload of anonymous `convert` telemetry: `enable`, `disable`, `status`, `preview`, `flush`, `purge` (§9 telemetry, `docs/telemetry-strategy.md`). |
| `hanten profile` | Write a **look** from the conversion flags, with no scan: every recipe key but what belongs to one roll — `calibration`, `roll` and `scene_correction` (whose flags it refuses: a recipe white balance or exposure would warn beside every measured roll) — as annotated JSONC in the §8 envelope, to stdout or `--out PATH` (refused over an existing file unless `--force`). Checked against every rule that needs no pixels, and the render probe at a base thinner than any measured, so a look no base could render is refused; what depends on the base or the scan is checked when it converts. A key the rendering decides is written `null`. It restates the decode, so it is layered before a roll's measured file. `hanten params` is its removed name. |

### Recipes (JSON in/out)

- `--params recipe.json` — load a recipe from JSON, or `-` for stdin (once).
  **Repeatable**: the recipes layer in order, `defaults < --params A < --params B < …
  < flags`, a later layer winning key by key. A `null` states nothing, but any stated
  value wins, a restated default included, so the measured file goes last (a look
  layer states no `calibration` or `roll`, which is what `hanten profile` writes; a
  `--save-recipe` file is the whole run, its `roll.frames` table included); a tagged
  value that switches variant (`region` → `explicit`) is replaced whole, and
  `roll.frames` merges as a table, entry by entry. Flags win **by
  source**: an explicit `--white-balance 1,1,1` means neutral gains, not "fall back to
  the recipes". A frame's `roll.frames` entry applies before the flags, so
  `--roll-white` or `--roll-dark` beats it, and a thin frame's lift (slope and exposure) with it; on
  `roll`, a frame's manifest `params` land after the flags, so they win over both, a
  white there likewise (`docs/design/roll-workflow.md`).
- `--save-recipe out.json` — write the resolved recipe (defaults + layers + flags):
  every layer collapsed into one file, which `--params` replays. An unstated knob stays
  `null`, so the rendering decides it on replay too. Written only when the run
  succeeds: a `--strict` refusal still writes the image and report but not the recipe,
  so one already at that path describes an earlier run. `--dump-params` is its removed
  name.
- **A recipe file may carry comments**: `--params` reads JSONC (`//` and `/* */`
  outside strings), a superset of JSON. Comments are never read back or preserved, so
  Hanten never rewrites a file in place (`docs/design/roll-workflow.md`, "Authored
  files").

**The document is versioned as a whole** (`crate::recipe`): `"recipe_version": 3` at
top level, one section per stage in chain order, the destination last:

```json
{
  "recipe_version": 3,
  "input": { "transfer": "auto", "meaning": "auto", "film_type": "unknown" },
  "calibration": { "film_base": {"explicit": [0.163, 0.080, 0.0377]} },
  "roll": { "white_balance": [1.002, 1.0, 1.277], "white_stops": 1.5, "dark_stops": -3.79, "exposure": 0.455, "frames": {} },
  "measure": { "inset": 0.05 },
  "reconstruction": {
    "scale": [1.0, 0.84, 0.73],
    "offset": [0.0, 0.0, 0.0],
    "linearization": 1.8,
    "anchor": {"mid-at-base-offset": 0.62}
  },
  "rendering": "default",
  "scene_correction": { "white_balance": {"explicit": [1.0, 1.0, 1.0]}, "exposure": 0.0 },
  "look": {
    "contrast": 1.0,
    "saturation": 1.0,
    "channel_grade": [1.0, 1.0],
    "highlight_desaturation": {"strength": null, "start_stops": null, "band": null}
  },
  "fit_range": {"headroom_stops": null, "display_black": null},
  "fit_gamut": {},
  "output": {"display": {"gamut": "adobe-rgb"}}
}
```

- **One section per stage, and an identity stage still has one** (`fit_gamut` has no
  knob). Every struct is `deny_unknown_fields`. Each stage's keys are in §9, and each
  key lands with the task that ships its knob — never written here ahead of the code.
- **The version is the chain declaration.** `recipe_version` is required; a document
  without it was written for the chain `nf-core/default-flip` removed (every sidecar and
  `--dump-params` file before `pipeline_version` 8) and is refused whole. This build
  writes `3` and also reads `2`, which differs only in `look.contrast`: the slope itself
  in 2, a multiplier in 3 (`nf-look/contrast-definition`). A version 2 recipe stating a
  number there is refused with the multiplier that keeps it — bit for bit where one
  exists, else within one `f32` step, and the message says which; its `null` reads as
  `1`. A
  per-frame override need not state a version, except beside a `look.contrast` number.
- **Retired keys are refused by name** (`recipe::check_body`), each with where its knob
  went — the removed chain's `print` and `output.preset`, its `reconstruction` and
  `calibration` keys, and older ones (top-level `algorithm`/`density`/`film_base`,
  `input.color`, …). This closes the gap `deny_unknown_fields` cannot: it rejects an
  *unknown* key but is blind to a known, meaningless one. No aliases. A retired key's
  old default, which every earlier file serialized, is dropped on load only where
  replaying it renders the same.
- **Every recipe document hanten writes is an envelope** `{ "meta": …, "params": {…} }`
  — `--save-recipe`, `hanten profile`, `measure-base --out`, `measure-roll --out` —
  with `meta` the build's `identity` (below). `--params` reads it and a bare recipe
  alike: `meta` is provenance and never applied, and a `meta.pipeline_version` other
  than this build's warns (`--strict`-promotable) that the default render changed
  underneath the parameters (the replay contract, below). Earlier builds' sidecars
  have the same shape.
- **`recipe_version` and `params` are reserved** and never recipe keys.

**The `calibration` section.** A key belongs here when it is (a) measured from the
film, (b) fixed across the roll, and (c) an input to the decode. The section is
deliberately **open**: a later measurement joins it with its own task. Today it is the
film base alone: `calibration.film_base`, `{"region": [x, y, w, h]}` or
`{"explicit": [r, g, b]}`, with **no default** — `convert` and `roll` refuse an
unstated one (§9). A pipeline profile is then "a recipe with no `calibration` or `roll`
section", and a roll calibration "a recipe with nothing else".

**The `roll` section** (`nf-calibration/roll-section`) holds what `hanten measure-roll`
measured, kept apart from the style knobs so a measured value is never mistaken for a
chosen one: the roll's white-balance gains, its white in scene stops (which sets the
look's base slope), its exposure, and `frames`, a per-file table of frame-local values
(the white of a frame clamped to the cap, a frame's lift: an exposure, or a thin
frame's slope and exposure). The `default` rendering applies it and
`direct` does not (§6). Its keys are in §9.

**Recipe warnings, not refusals** (`Recipe::recipe_warnings`). A file cannot say who
chose a value, and a `--save-recipe` recipe must replay as it rendered, so nothing is
read as unset by its value and nothing is refused; the run warns once instead, and a
typed style flag (a choice made now) never does. The warnings catch a value nobody chose:
`default` without a roll measurement (what fell back); a recipe white balance or
exposure beside the roll's, which it multiplies or adds to; a `roll` section with gains
or a white but no exposure; and, under `direct`, exactly two leftovers of earlier
builds — highlight desaturation at the old default `0.8`, and a recipe white balance
beside a `roll` section (old `measure-roll` output). `direct`'s list is narrow on
purpose, so a saved recipe of a deliberate adjustment replays under `--strict` — with
one carve-out: a white balance typed beside a `roll` section is written into it and
warns on replay unless the flag is typed again. Any other value that moves `direct`'s
pinned base does not warn. The missing-exposure warning ignores typing: typed
`--roll-white` or `--roll-white-balance` without `--roll-exposure` warns too. A contrast beside `roll.white_stops`
multiplies the roll's slope, as intended, and never warns. A white without its dark end, or
the reverse, is refused where the roll applies (every `roll` section written before
`pipeline_version` 11 has the white alone); `direct` and the film master spare it.

### Target: the roll workflow

**The target CLI workflow — measuring, layered recipes, `roll`'s measure mode — is
[`docs/design/roll-workflow.md`](design/roll-workflow.md)**, the single source for the
tasks that build it. The structural half has shipped: the measurements live in their
own `calibration` and `roll` sections, described above.

### Reports & determinism

- `--report json` — emit a machine-readable result (resolved values, what each stage
  applied, warnings, output path) to stdout or `--report-file`.
- `--seed <n>` — fix any stochastic step (none today, reserved).
- Stable, documented **exit codes** (see §11).

**What a `convert` report carries** (`cli::Report`): `command`, `identity` (below), `input`,
`output` (the completed path, §5), `working_mapping` (`"nc-film-rgb-v1"`), `chain`,
`recipe` (the resolved recipe — a `--save-recipe` file's `params`, so it reloads through
`--params` to this run), `memory`, `input_color`, `film_base` with its source and percentile,
`film_type` (when declared), `effective_area`, `loss` (clamped and non-finite samples at the encode),
`output_stats`, `warnings`, `elapsed_ms`, and the destination's own block where it has one
(`hdr_linear_tiff`, `hdr_coded_tiff`). **`chain` records what ran**, so a consumer never
re-derives it from the recipe:

```json
{
  "chain": {
    "decode": { "anchor": 1.0337375, "anchor_rule": "mid-at-base-offset", "reads_reference": false,
                "linearization": 1.8, "scale": [1.0, 0.84, 0.73], "offset": [0.0, 0.0, 0.0] },
    "stages": [
      { "stage": "scene_correction", "applied": "identity" },
      { "stage": "look", "applied": "contrast+saturation+highlight-desaturation" },
      { "stage": "fit_range", "applied": "reinhard-peak-lifted-v1+log-shift-to-mid-grey-v1" },
      { "stage": "fit_gamut", "applied": "acescg-to-display-p3-matrix+neutral-axis-radial-boundary-v2" }
    ],
    "rendering": "default",
    "scene_correction": { "white_balance": [1.0, 1.0, 1.0], "exposure": 0.0 },
    "look": { "contrast": 1.0, "base_slope": 1.413675, "base_from": "fallback",
              "saturation": 1.0, "saturation_base_slope": 1.6257261,
              "saturation_base_from": "fallback",
              "slope": 1.413675, "saturation_slope": 1.6257261, "channel_grade": [1.0, 1.0],
              "highlight_desaturation": { "strength": 0.8, "start_stops": -1.0, "band": [0.015, 0.025] } },
    "fit_range": { "operator": "reinhard-peak-lifted-v1", "headroom_stops": 6.0, "white_point": 64.0,
                   "display_peak": 1.0,
                   "display_black": { "setting": 6.0, "curve": "log-shift-to-mid-grey-v1",
                                      "film_base_stops": 4.96, "shift_stops": -1.04 } },
    "destination": { "display": { "range": "sdr", "transfer": "native", "gamut": "display-p3",
                                  "container": "tiff" } }
  }
}
```

A stage at its identity reports `"applied": "identity"`. `chain.roll` states the
`roll` section and whether each value was applied; `chain.peak_clamp` counts what an
HDR rendition clamped at its peak; `chain.gain_map` gives the gain map's extent and
size, `flat` when every gain is within rounding of 1 (a fact about the frame, not a
warning). The film master's `chain` has the decode and the destination, and no stages.
A `roll` report carries `identity` for the shared recipe, roll-level `warnings`, a
`frames` array (each with its own `identity`, `chain`, `film_base`, `status` and, on
failure, `error`) and a `summary`.

**Conversion identity (`identity`, every report).** Three independent layers that
make an output attributable, all **operational** metadata in the same class as
`--report` and the telemetry flags: no CLI flag, no recipe key, and never a
changed output pixel.

```json
{
  "identity": {
    "nc_version": "0.1.0",
    "git_commit": "9d612efc2138",
    "git_dirty": false,
    "pipeline_version": 8,
    "target": "aarch64-apple-darwin",
    "params_hash": "3575c9feb5d42b2b"
  }
}
```

- `nc_version` / `git_commit` / `git_dirty` / `target` — **build identity**: which
  binary. Captured by `build.rs`; `git_commit` and `git_dirty` are **omitted**
  (never the string `"unknown"`) when the build tree had no usable git, so a
  source-tarball build degrades honestly instead of claiming a clean checkout.
  `git_dirty: true` means the commit alone does not identify the source.
- `pipeline_version` — the **behavioral** version, an integer **independent of
  semver** that bumps *only* when **default** conversion behavior changes. `0` is the
  Step-1 baseline in `docs/reports/v0-baseline.md`; `6` made the default curve the
  exponential at the fixed decode's configuration; `7` made extended Reinhard the one
  display tone; `8` is current: the staged chain is the only one, and the default is
  the SDR Display P3 16-bit TIFF (`docs/reports/default-flip.md`). `version.rs` carries
  the full table. This is the axis a version comparison is keyed on.
- **What the drift gate does and does not cover.** A golden drift test
  (`version::PIPELINE_FINGERPRINTS`) pairs each version with three fingerprints —
  the default **render** (the fixed decode and the ACEScg mapping over five near-base
  pixels; it stops at scene correction's input, since every rendering stage after it
  makes libm calls a windowless hash cannot absorb), the **film-base estimate** (a
  stated region of the frozen scan in `pipeline::film_base::golden`), and the default
  **recipe values**. Change a default in those stages and the test fails until the
  version and the fingerprints are updated together. It does **not** cover container
  decode, stage-1b input semantics, the rendering stages' arithmetic (the stage
  goldens' and `pipeline::chain_golden`'s), the lcms2 output transform or embedded
  ICC bytes (both differ by target, so no cross-platform hash of them exists),
  encode/quantization, or the effective-area measurement `measure-base` makes. A
  change confined to those can move output with every test green;
  `scripts/real-scan-verify/` and `nctool compare` are the tools for that half.
- `params_hash` — a stable 64-bit FNV-1a hash of the canonical resolved-recipe
  JSON: the report's `recipe` as hanten pretty-prints it, which is the `params` body
  `--save-recipe` writes, dedented, so an agent can reproduce it and identical
  configurations are detectable across frames and versions. Omitted for
  `inspect`/`measure-base`, which resolve no full recipe. `hanten roll` stamps one
  `identity` for the **shared** frozen recipe; a per-frame override changes that
  frame's own hash, which is why each roll frame also reports its own `identity`.

**The replay contract.** A replayed recipe either reproduces its render or says that
it may not. Values it states are applied as stated; values it leaves unset come from
this build's defaults and the rendering's base (`crate::rendering`), which a default
move changes. So every recipe document hanten writes records the `pipeline_version` it
was written under, and a replay under another one warns, with no claim about which
values moved. Two rules keep a written document honest:

- **A measurement states what it was measured through.** `measure-roll --out` always
  writes the decode (`reconstruction`) its gains were measured at, default or not, so
  a moved decode default cannot render them under another.
- **The label is only as good as the bump.** The warning keys on `pipeline_version`
  alone, so a moved default the drift gate does not cover, and nobody bumped for,
  replays silently; and a recipe hanten did not write (hand-written, a report's
  `recipe`) records no version and gets no check.

**Comparison basis (`output_stats`, `convert` and each roll frame).** Report-only,
alongside `loss`:

```json
{ "output_stats": { "mean": [0.512, 0.487, 0.443] } }
```

`mean` is the per-channel mean of the samples **as written**, and it is the numeric
basis `nctool compare` diffs across two builds (per-channel mean ΔRGB is the
difference of two runs' means, so no output is ever re-read or shipped). Its units
follow the output depth: a u16 destination reports the quantized value scaled back to
`[0, 1]` (exact integer accumulation, so it is reproducible on every target given
identical pixels); an f32 destination (the film master, the linear HDR TIFF) reports
the verbatim, **unclamped** float mean over the *finite* samples, so it may exceed
`1.0` and one `NaN` cannot swallow the statistic (`loss.non_finite` is where that
fault is reported). A u16 mean and an f32 mean are therefore not comparable, and
`compare` refuses to subtract them. Only the mean is recorded; ΔE2000 / SSIM need
real pixel access and belong to §12 item 7's QA harness.

`hanten --version` prints the same build identity (semver, `pipeline_version` with a
one-line description of its default render, commit with a `-dirty` marker — or
`(dirty unknown)` when cleanliness could not be read, target) so an output can be
attributed without running a conversion.

**Memory preflight block.** Every command that decodes a scan reports what the
preflight decided before it allocated anything (§9 Global, `--max-memory`; §11
exit 6). Byte counts are exact; the per-phase fields are the accounted
full-frame buffers that are simultaneously live in that phase, and
`estimated_peak_bytes` is the peak of them plus a calibrated allowance for
allocator slack and fixed costs — the number the gate compares:

```json
{
  "memory": {
    "estimated_peak_bytes": 2194546688,
    "accounted_bytes": 1791590400,
    "decode_bytes": 1343692800,
    "film_base_bytes": 1642291200,
    "render_bytes": 1343692800,
    "encode_bytes": 1791590400,
    "budget_bytes": 6442450944,
    "budget_source": "default",
    "decision": "ok",
    "detected_total_ram_bytes": 19327352832
  }
}
```

(A 10368x7200 HDRi `convert` at `u16`, default budget, with a `--base-region` of
half the frame. The `film_base_bytes` figure is the decoded image plus the
three `f32` channel vectors that rectangle is gathered into, which the later phases
keep counting; an explicit
`--film-base` gathers nothing, and neither does `measure-base`'s effective-area median
(a fixed-size histogram), so there the phase is the decoded image alone.)

`budget_source` is `default|flag`, `decision` is `ok|warn` (a rejected run emits
no report at all), and `detected_total_ram_bytes` is omitted when the platform
can't report it (which also disables the warn tier). `render_bytes`/`encode_bytes`
are `0` on `inspect`/`measure-base`, which decode, measure, and stop — so for them the
**film-base** phase is the peak when a `--base-region` is gathered, and decode
otherwise. `hanten roll` reports the same
block **per frame** (frames may differ in dimensions, and the gate runs per
frame), not once for the roll — including for a frame that passed the gate and then
failed for another reason, whose entry carries both its `memory` block and its
`error`.


### Example invocations

```bash
# Measure the roll once. --unexposed measures the film base from a lens-cap frame
# (wind past the light-struck leader, shoot one with the cap on, scan it; never the
# auto-burned wind-on frames, which are fogged); the leader guards the white balance
# against a fully exposed frame. --out writes all of it as one recipe.
hanten measure-roll frames/*.tif --unexposed blank.tif --leader leader.tif --out roll.json
# → { "unexposed": { "film_base": …, "film_base_source": "effective_area", "film_base_percentile": 0.5, … },
#     "white_balance": { "gains": [1.002, 1.0, 1.277], "percentile": 0.99, … },
#     "white": { "stops": 1.5, "bound": "floor", "dark_stops": -3.79, "slope": 1.71, … },
#     "exposure": { "ev": 0.455, "level_stops": -1.055, "bounded": false, … },
#     "midtone_neutral": { "kind": "correction", "mode": "auto", "line": { "red": [...],
#         "blue": [...], "bands": [-2.75, 2.25], "fade_end_stops": 1.9 }, "off_because": null, … },
#     "confidence": { "white_balance": { "tier": "confident", "frames": 24, "confident_from": 16 },
#         "midtone_neutral": { … } },
#     "reuse": { "flag": "--roll-white-balance 1.002,1,1.277 --roll-white 1.5 --roll-dark -3.79 --roll-exposure 0.455 --roll-midtone-line …" } }
# roll.json: { "recipe_version": 3, "calibration": { "film_base": { "explicit": [...] } },
#   "roll": { "white_balance": [...], "neutral_balance": null, "white_stops": 1.5,
#             "dark_stops": -3.79, "exposure": 0.455,
#             "frame_exposure": null, "small_lift": null,
#             "thin_slope": null, "thin_exposure": null, "thin_lift": null,
#             "midtone_line": { … }, "midtone_neutral": null,
#             "frames": { "f07.tif": { "white_stops": 2.0, "exposure": null, … },   # clamped
#                         "f12.tif": { "white_stops": null, "exposure": 0.3, … } } } } # lifted
# (A thin frame also gets a `thin_slope` and `thin_exposure`, which replace its small
# lift. `--no-small-lift` / `--no-thin-lift` leave one out; `convert`/`roll
# --small-lift off` / `--thin-lift off` turn a written one off.)

# Convert the roll from that one frozen recipe: SDR Display P3 16-bit TIFFs,
# out/<stem>_positive.tiff. A look is its own layer, and a flag beats both.
hanten roll frames/*.tif --params roll.json -o out/
hanten roll frames/*.tif --params my-look.json --params roll.json --exposure 0.3 -o out/

# Per-frame overrides via a manifest: each frame may carry its own output path and a
# partial recipe deep-merged onto the shared one for that frame only. It beats the
# recipe's own per-frame table, `roll.frames`.
#   frames.json: { "frames": [
#     { "input": "frame01.tiff" },
#     { "input": "frame02.tiff", "params": { "scene_correction": { "exposure": 0.15 } } } ] }
hanten roll --frames frames.json -o out/ --params roll.json
# A frame's failure is recorded (status "failed" + error) and the roll continues; the
# process then exits non-zero. Same batch + same recipe ⇒ byte-identical output per frame.

# One frame. The film base has no default, so every conversion states one — here in
# the recipe. `-o out` completes to out.tiff; a stated suffix is checked, never renamed.
hanten convert frame12.tif -o out --params roll.json --report json

# The same frame as an HDR gain-map JPEG, a PQ TIFF, and a linear float TIFF.
hanten convert frame12.tif -o out.jpg --params roll.json --range hdr
hanten convert frame12.tif -o out-pq.tiff --params roll.json --transfer pq
hanten convert frame12.tif -o out.tiff --params roll.json --transfer linear --gamut bt2020

# For an external editor: the `direct` rendering (the roll section unapplied, pinned
# values, HDR linear Adobe RGB float TIFF by default), or its SDR form.
hanten convert frame12.tif -o edit.tiff --params roll.json --rendering direct
hanten convert frame12.tif -o edit.tiff --params roll.json --rendering direct --range sdr

# The film master: the decode's unclamped linear ACEScg, no rendering stage. A
# rendering knob beside it is refused, naming the stage.
hanten convert frame12.tif -o master.tiff --params roll.json --film-master

# Only the film base, from an unexposed frame's effective area (per-channel median),
# or from a stated rectangle of unexposed film (p97, with a uniformity warning).
hanten measure-base blank.tif --out roll-cal.json
hanten measure-base frame01.tif --base-region 200,0,300,3600 --out roll-cal.json
# → { "film_base": { "r": 0.553, "g": 0.271, "b": 0.159 }, "film_base_source": "effective_area",
#     "film_base_percentile": 0.5, "film_base_flag": "--film-base 0.553,0.271,0.159", … }
# roll-cal.json: {"recipe_version": 3, "calibration": {"film_base": {"explicit": [0.553, 0.271, 0.159]}}}

# Inspect only; let an agent read the JSON and decide parameters.
hanten inspect in.tif --report json
```


## 9. Parameter reference (grouped by stage)

Every conversion flag has a recipe key (for example, `--exposure` ⇒
`scene_correction.exposure`), and flags win over the recipe (§8). Names are binding and
unknown keys are rejected (`deny_unknown_fields`). The **operational** flags
(`--report`, `--telemetry*`, `--max-memory`, `--export-film-rgb`, `--export-pre-encode`) are
the exception: they touch no parameter at all, so they have no recipe key. Retired flags
and keys are listed at the end of this section.

### Input / decode
- `--export-film-rgb <path>` (operational, no recipe key) — write the
  fixed decode's output **before** the NC film RGB v1 3×3 as an f32 TIFF with no ICC
  profile, since the dye layers have no primaries (`nf-verification/film-rgb-export`).
  Sent through the pinned mapper it equals the film master bit for bit. The cleanest
  point for per-layer measurement (`nctool metrics --space film-rgb`). Reported as
  `film_rgb_exported`. On `roll` it is a switch with no path: each frame writes
  `<input-stem>_film-rgb.tiff` beside its output, guarded like an output, and its
  entry reports `film_rgb_exported` (`nf-verification/roll-side-exports`).
- `--export-pre-encode <path>` (`convert` only; operational, no recipe key) — write what
  the destination's encoder receives, one untagged TIFF page per buffer: the linear
  rendition before any transfer (both, for the gain map) and the gain map's codes
  before its JPEG. The reference `nctool acceptance` checks an independent decode of the
  output against (`analysis/display-acceptance-harness`). Reported as
  `pre_encode_exported`.
- `--film-type <silver|chromogenic|unknown>` ⇒ `input.film_type` (default
  `"unknown"`) — the declared film chemistry. **Provenance only: it gates nothing.**
  IR-assisted film-holder detection (§6.1) is enabled by *measuring* the IR plane,
  not by this declaration. Kept as a shared input-medium axis for the deferred IR
  dust-removal stage (§12 item 1) and `bw-support`; accepted on `convert`,
  `measure-base`, and `inspect`, and echoed back as the report's `film_type` (per frame
  on `roll`; omitted for `unknown`) so a declaration is never parsed and dropped.
  `hanten inspect` and `hanten measure-base` report `ir_separability` (the measured
  interior IR transmission and the verdict) on any scan carrying an IR plane, and
  the measured holder depths inside `effective_area` (§9 `measure`). Where the
  holder was not measured — no IR plane, shape-only provenance, or a frame whose own
  film is IR-opaque — `measure-base` warns that the effective area is the inset alone.
- Input color is resolved as **two independent axes** before Dmin/density — the
  transfer encoding and the measurement meaning — never a single combined
  assertion. Each is a mutually-exclusive assertion with its own recipe key; the
  two never conflict (they describe different facts), so each flag replaces only
  its own axis:
  - `--input-transfer <auto|linear>` ⇒ `input.transfer` (default `"auto"`) —
    how the samples are *encoded*. `linear` asserts a linear transfer (no
    inverse-transfer decoding); it does **not** prove scanner-device provenance.
  - `--input-meaning <auto|scanner-device|colorimetric>` ⇒ `input.meaning`
    (default `"auto"`) — what the pixel axes *are*. Only `scanner-device` (with a
    supported linear transfer) enters Dmin/density without a source→working
    transform. `colorimetric` is recognized but **unsupported** (no inverse
    transfer/reconstruction path exists yet); `convert` rejects it even when
    asserted (an override cannot make it supported).
  - `auto` on either axis resolves from container evidence and **fails loudly in
    `convert`** when it stays ambiguous — nothing is silently labelled linear
    Rec.709 for lacking an ICC. `hanten inspect` still reports the evidence so the
    file is diagnosable.
  Resolution and precedence (deterministic, `pipeline::input_semantics`):
  an explicit assertion outranks a descriptive tag which outranks the
  absence-of-evidence default; authoritative container structure (SilverFast
  HDR/HDRi raw mode) proves *both* a linear transfer and scanner-device meaning.
  Raw-mode provenance is **detected from SilverFast's XMP mode metadata** (TIFF
  tag 700), not assumed from "we decoded it" and not keyed on spoofable signals:
  the decoder accepts any 3-channel 16-bit chunky RGB TIFF, so a file is treated
  as SilverFast raw mode only when its XMP carries `Silverfast:Company =
  "LaserSoft Imaging"` **and** `Silverfast:HDRScan = Yes` (grounded in the real
  sample scans). The `Software` string and IR-plane presence are deliberately
  **not** provenance — a processed export keeps the `Software` tag, and a generic
  RGB16 + Gray16 multipage forges an IR-like plane; both are rejected. The XMP
  `Silverfast:Gamma` feeds the transfer axis (`Gamma ≈ 1` corroborates linear; a
  non-linear gamma on a raw-mode scan makes the transfer ambiguous). A gamma value
  that is **present but uninterpretable** (e.g. a locale-formatted `"2,2"`) is
  treated as ambiguous, **not** linear (transfer → `unknown`, with a decode
  warning naming the value) — nc does not guess the locale. A tag-700 packet that
  is present but yields **no recognizable SilverFast metadata** (malformed, or an
  unrecognized namespace/layout — e.g. a future scanner) emits a warning and
  establishes no provenance rather than silently dropping it. A **generic /
  colorimetric / processed RGB16 TIFF** (e.g. one carrying an sRGB ICC) therefore
  resolves `meaning: unknown` and is **rejected by `convert`** (exit 4, with an
  error suggesting `--input-transfer linear --input-meaning scanner-device` if the
  user knows it is a raw scan) — never silently converted as a raw negative.
  Gamma 1 establishes **only** the transfer axis (never raw-mode provenance or
  meaning). An explicit assertion that contradicts authoritative structure (e.g.
  `--input-meaning colorimetric` on a raw-mode scanner scan) **fails** rather than
  overriding it (exit 2); an explicit assertion that overrides a descriptive tag
  is honored and records the displaced tag. Every explicit override is reported
  with its CLI-vs-recipe provenance. A descriptive gamma tag that contradicts
  raw-mode linear semantics makes the transfer **ambiguous** (rejected by
  `convert`, explained by `inspect`) unless an explicit `--input-transfer linear`
  resolves it.
  - An **embedded scanner ICC** (TIFF tag 34675) is retained and reported as
    device-characterization metadata (a safe class/space/PCS/version/description
    summary — never a raw byte dump), but it is **never applied before density**
    and does not by itself establish either axis.
  - IR remains measurement data — never color-transformed, bit-identical before
    and after input resolution.
  - The removed combined key `input.color` (and the `--assume-linear` flag) is
    rejected with a pinned migration error — it must never silently assert both
    axes.
  - `--input-profile <icc>` stays **rejected for normal conversion** (exit 4):
    input-side ICC application has no validated placement and is reserved for the
    deferred `scanner-profile-before-density-experiment`.
  - A SilverFast **positive-mode** scan (XMP `Silverfast:Negative = No`) in raw mode
    holds the same linear scanner transmission as a negative-mode scan, so a negative
    scanned that way converts as one; the tag is recorded as evidence. A slide carries
    the same tags, so each picture frame is checked against its base instead: more than
    1% of the effective area above 1.5x the base warns that it does not look like a
    negative (`film_base::polarity_warning`). A slide read against its clear leader
    passes, and a channel whose base is ≥ ~0.667 of full scale cannot fire. Slide film
    is `io/slide-film-input`.
  `hanten inspect`, the `convert` report, **and each `hanten roll` frame report** expose
  the resolved `input_color`: both axes with per-axis evidence, whether an ICC is
  embedded plus the safe summary, and `transfer_decoded` (whether any
  inverse-transfer decoding was performed — always `false` today, since nc
  accepts only already-linear samples). `meaning` is always a flat string
  (`scanner-device` / `colorimetric` / `unknown`); the colorimetric detail rides
  in a sibling `meaning_reference` field so consumers can key `meaning` uniformly.
  In roll mode a shared recipe (or per-frame override) asserting the
  unconditionally-unsupported `input.meaning: colorimetric` is rejected up front
  (exit 4), before any frame is decoded.

### Measurement region (`measure`)
Every statistic nc reads off a frame — `Dmin`, a roll's white balance
(`hanten measure-roll`), content exposure and contrast, tiling uniformity — is read over
the **effective area**, not the whole scan. An uncropped scan carries an opaque film
holder that is maximum density, so a whole-frame statistic measures the holder rather
than the picture (a per-frame density percentile taken that way once rendered every
frame black).

The area is **two cuts, in order — never one or the other**:

1. **The film holder**, measured per edge from the IR plane by marching inward
   until IR reads film. Only where `film_base::ir_separability` measures the plane
   able to separate holder from film *on this frame*; declared chemistry takes no
   part in it. No IR plane, a plane identified by shape alone, or film too
   IR-opaque to separate (routine for exposed silver stock) all mean **not
   measured** — a different report from "measured, and there is no holder", which
   is what an already-cropped scan yields.
2. **A static inset** of what remains, recipe key `measure.inset` /
   `--measure-inset FRAC`, default `0.05` of the **original** frame's shorter
   dimension (so the pixel count does not move with the holder measurement).
   Accepted range `[0, 0.4]`; outside it is a usage error (exit 2) from every
   command, checked before the decode. Where cut 1 **ran**, the applied inset is
   floored at one holder-probe step (0.5% of the shorter edge): a march resolves a
   depth only to the start of the first band whose median reads film, so up to half
   a band of holder can sit inboard of any measured depth — including a measured
   zero. The floor absorbs that band, binds only near `0` (a 3600 px frame insets
   180 px against an 18 px step), and is reported, so a stated `0` on a measured
   frame is deliberately not honoured exactly. Where cut 1 did not run there is no
   measurement resolution to respect and the stated fraction is exact.

The inset is **not a fallback** for cut 1 — it runs either way, and where cut 1 did
not run it is simply the only cut. Because it is then sized for a rebate rather than
a holder, it may under-clear the holder and its rebate together: the only directly
measured holder depth is 2.5–4% of the shorter edge (the IR march across 31 real
frames). The 10–15% figure recorded in `analysis/conversion-metrics` is a holder
*occupancy* share of a rendered frame, not a depth, and bounds nothing here. That is deliberate: nc declines to guess a depth it could not measure, and
the user raises the fraction instead. The report records which case a run was in.

**nc never searches for a rebate**, for measurement or otherwise: the inset passes
over it blind. Where a measurement needs unexposed film, it is taken over the whole
effective area of a reference frame or over a region the user states.

**The image is never cropped.** The effective area changes only *which pixels a
statistic is computed over*; written dimensions, aspect ratio and pixel count are
exactly as decoded.

**Every command that decodes resolves the area and reports it**, under the report
key `effective_area` — the resolved rectangle, the per-edge holder depths (with a
per-edge `capped` and the frame-wide `converged`), `holder_applied`, and the applied
inset — for a `roll`, inside each frame's entry.
`convert` and every roll frame resolve it unconditionally so `--measure-inset` and
the `measure.inset` recipe key (including a per-frame override) are observable rather
than accepted-and-ignored. A `capped` edge or `converged: false` also emits a
`--strict`-promotable warning: both mean the reported rectangle is not a
measurement. The march stops at 25% of the shorter edge on the premise that no film
holder is that deep, so a cap is an IR misread and errs only toward an over-cut (the
edges perpendicular to it can cap with it); a holder that really is deeper is
covered by raising the inset, which is added on top of the cap.

**On `convert` the area feeds only the polarity warning** (`film_base::polarity_warning`),
which an *empty* region skips — so an empty region is a warning there (with no reported
area), never a refusal. `measure.inset` stays live for `hanten measure-base` and
`hanten measure-roll`, which measure over the area. A marched holder moves no rendered
pixel, but it can change whether the polarity warning fires, and so a `--strict` exit.

### Film base / Dmin (stage 2)
The base source is a single mutually-exclusive choice, recipe key
`calibration.film_base` — `{"explicit": [r, g, b]}` or `{"region": [x, y, w, h]}`,
**required, with no default**. `convert` and `roll` reject a config that does not
state one (exit 2, naming `--film-base` measured with `hanten measure-base
<unexposed-frame>`, `--base-region`, and the recipe `measure-roll --unexposed --out`
writes). The retired `"auto"` value and
`--auto-base` flag are refused (exit 2) with a message naming the same route.

Why it is required: `Dmin` is the divisor of the density conversion, so it sets
the black point and the colour balance together. Defaulting silently meant the
single most consequential parameter of a conversion was one nobody had decided.

**A base is an area read by a method** (`pipeline::film_base`), and nc never
searches a frame for one (§9 `measure`):

- **The effective area of an unexposed frame**, read at the per-channel **median**
  — `hanten measure-base FRAME` with no source flag. With the holder cut away the area
  is one population (unexposed film plus grain and scanner noise); a high percentile
  would land in its noise tail and read the base too transparent, understating every
  density. The median holds until half the area is contaminated. It is a
  **measurement, not a source**: no conversion runs it; its result reaches one as an
  explicit base. Over a picture it returns a plausible, wrong base, so the report
  warns (`--strict` fails) when the area's worst per-channel spread
  `(p90 − p10) / p50` exceeds 0.5 — unexposed frames measure 0.06–0.29, picture frames
  0.87–2.26. It is a coarse "is this a picture?" guard, not a uniformity verdict: a
  leader passes. An empty area is a usage error (exit 2).
- **A stated region** — `--base-region x,y,w,h` ⇒ `{ "region": [x, y, w, h] }`,
  read at the per-channel **p97**. A hand-drawn rectangle may mix holder, rebate
  and picture, and the base is its most transparent sub-population, which a high
  percentile reaches past the rest (below the maximum, so a hot pixel cannot become
  it). A non-uniform rectangle keeps its value but raises a **uniformity warning**
  (`--strict` promotes it) — a mixed rectangle otherwise yields a plausible-looking
  bad base with no signal.
- **An explicit value** — `--film-base R,G,B` ⇒ `{ "explicit": [r, g, b] }`.

The report's `film_base_source` is `"effective_area"`, `{"region": […]}` or
`{"explicit": […]}`, and `film_base_percentile` (`0.5` / `0.97`) names the method;
it is absent for an explicit base. The two flags conflict (passing both is a usage
error); whichever is given replaces a recipe's source. `inspect` measures no base.

**How to obtain `Dmin`.** `Dmin` is a property of the *film stock + development +
scanner settings*, not of an individual frame, so measure it **once per roll** and
reuse it (recipe / `--film-base`) — the base is then identical across frames,
keeping the roll color-consistent. The sources, in decreasing reliability:

1. **A dedicated unexposed frame (best).** Recommended shooting workflow: after
   loading a roll and winding past the light-struck leader (the frame counter
   reaching 1), take a deliberate exposure with the lens cap on, then scan that
   blank frame alongside the roll. Do **not** rely on the 1–2 auto-burned
   wind-on frames — that leader area was exposed while loading with the back
   open, so it is fogged film, denser than clean base, and would bake a wrong
   `Dmin` into the whole roll. Measure it with `hanten measure-base` and freeze the
   result into the roll recipe (§8 example).
2. **Unexposed film on a picture frame.** Point `--base-region` at a visible
   rebate patch, located by hand (UI-assisted picking is a roadmap item, §12). Real
   scans are laid out `dark film holder → thin unexposed rebate → exposed picture`,
   the rebate a narrow band behind the holder, possibly on only some edges — so the
   rectangle is small and easily mixed, which is what p97 and the uniformity
   warning are for.
3. **Content-based estimation (last resort, opt-in, not built).** When the scan is
   cropped to the image with no unexposed film visible, a per-channel high
   percentile of the *exposed content* approximates the base (the thinnest area of
   a negative is the scene's deepest black, close to true base). It would be an
   **explicit opt-in** `hanten measure-roll` option owned by
   `film-base/content-fallback`, pooled over the roll's picture frames and frozen as an
   explicit base, its source recorded in the report — never a silent fallback. When the assumption fails (foggy/high-key
   scenes), blacks wash out and pick up a cast — recoverable downstream as a global
   cast (`reconstruction.offset` / white balance).

**When no base is stated**, `convert` **fails loudly** with an actionable message
naming the recovery flags — an agent can catch the exit code and re-run with an
explicit choice. Estimator selection is never silent. **A degenerate resolved
base** (a zero / negative / non-finite channel — e.g. a `--base-region` on the dark
holder, or an all-black frame) is rejected at the estimation stage (exit 1) rather
than left to poison the density divide or be echoed back by `hanten measure-base` as a
trustworthy `Dmin`. A neutral base `[1,1,1]` is representable but not recommended:
it forfeits the per-channel orange-mask neutralization. Note the failure geometry is
forgiving: because `D = -log10(scan/base)`, a base error is a *constant per-channel
density offset* — a global cast/exposure error correctable downstream
(`reconstruction.offset`, white balance) — never a shadow/highlight crossover.


### Roll measurements (`roll`)

What `hanten measure-roll` measured (§8, `nf-calibration/roll-section`). All optional;
unset they are written as `null`, never left out, so a roll's one-key per-frame
override merges instead of replacing the section. `default` applies them, `direct` and
the film master apply none — a recipe's `roll` section is carried and reported as
unapplied, but a typed `--roll-*` flag beside `--rendering direct` or `--film-master`
is refused (exit 2), since it asks for something the run will not do.

- `--roll-white-balance R,G,B` ⇒ `roll.white_balance` — the roll's gains, finite and
  positive, multiplied into `scene_correction.white_balance`.
- `--roll-white STOPS` ⇒ `roll.white_stops` and `--roll-dark STOPS` ⇒ `roll.dark_stops`
  — the roll's white in scene stops above mid-grey (finite and positive) and its dark end
  in scene stops from mid-grey (finite): measurements, not a slope, stated together or not
  at all where the roll applies. The look's base slope spreads the span between them over
  9.049 stops, `9.049 / (white_stops − dark_stops)`, held within the cap's slope
  (`log2(1/0.18) / 2`) and 3.5 — a span that is not positive takes 3.5 — with mid-grey
  pinned (`nf-calibration/span-roll-slope`), unless a thin lift applies
  (`roll.thin_slope`), and `look.contrast` multiplies it. A longer span is a flatter
  picture; the white renders where the slope and the roll's exposure put it, not at a
  target. Typed, or in a `roll` manifest's `params`, either drops a frame's thin lift
  (slope and exposure) and keeps its small lift; a later `--params` layer's does not.
- `--roll-exposure EV` ⇒ `roll.exposure` — the roll's measured exposure, a neutral
  gain added to `scene_correction.exposure` (`2^EV` must be normal), so a frame darker
  than its roll stays dark, beyond a bounded lift. It does not move `white_stops`, which is measured at
  exposure 0.
- `--roll-frame-exposure EV` ⇒ `roll.frame_exposure` — this frame's **small lift**, a
  preference (§6): a **delta** added to `roll.exposure`, the lift `measure-roll` gives a
  low-key frame (`nf-calibration/frame-level-trim`), 0 to +0.3 EV, keyed on where the
  frame's white renders after the roll's exposure. A delta, so it keeps its meaning when
  the roll is re-measured. A thin lift replaces it while that applies. A **flat** frame —
  its luma spans under a stop from p5 to p95, one surface filling it — gets no lift.
- `--small-lift on|off` ⇒ `roll.small_lift` — whether the small lift applies.
- `--roll-thin-slope SLOPE` ⇒ `roll.thin_slope` and `--roll-thin-exposure EV` ⇒
  `roll.thin_exposure` — a **thin** frame's lift, a preference (§6;
  `nf-calibration/thin-frame-lift`): a base slope in place of the roll's (`look.contrast`
  multiplies it), and the exposure solved with it, a delta on
  `roll.exposure` in place of the small lift. A frame is thin when its white sits low
  after the roll's exposure and its shadows on the film base. The pair raises the
  frame's white about a stop with the film base held about where its small lift renders
  it (the white is read in film RGB, the base as luma before the roll's gains), within a
  slope bound (2.4); solved from that render, the pair is the same with the small lift
  off. A frame the bound holds, or one no thin lift fits (it has its small
  lift only), is listed in the report's `thin_lift` section, not warned about. A slope,
  not a white, because it is chosen, not measured. The exposure applies only beside the
  slope, and is refused without one.
- `--thin-lift on|off` ⇒ `roll.thin_lift` — whether the thin pair applies; off, the
  frame renders its small lift.
- `--roll-midtone-line RS,RO,BS,BO,LO,HI,END` ⇒ `roll.midtone_line = {"red": [RS, RO],
  "blue": [BS, BO], "bands": [LO, HI], "fade_end_stops": END}` — the roll's **midtone
  neutral**, a correction (§6; `nf-scene-correction/midtone-neutral`). Per band of scene
  brightness `measure-roll` takes each frame's densest cluster of `log2 r/g`, `log2 b/g`
  after the roll's gains, the median over frames, and a line weighted by frames voting;
  scene correction divides red and blue by `2^(slope · s + offset)` at the pixel's scene
  stop `s` (after the roll's gains), in full up to a stop below `END`, fading to zero at
  it, flat outside `[LO, HI]`, and fading out as the pixel's colour lies 0.6 → 0.9 (log2)
  from the line's cast (the tint gate); luminance is restored. Roll-wide: a `frames` entry
  cannot carry one. Every value finite, `LO ≤ HI`, and refused without `roll.white_balance`,
  which it is measured after.
- `--midtone-neutral on|off` ⇒ `roll.midtone_neutral` — whether the line applies. Frame-local.
  Off drops only the line: the whites stay measured after it (`measure-roll
  --midtone-neutral off` measures them without it).
- `--neutral-balance on|off` ⇒ `roll.neutral_balance` — whether `roll.white_balance`
  applies. Frame-local. Off keeps the gains in the recipe and takes the line with them,
  which is measured after them; the whites stay as measured. A typed `--midtone-neutral
  on` is refused against a typed or recipe off; a recipe's `"on"` (the default) is
  spared, and a `roll` manifest frame's own off wins over the flag.

Each switch is unset (`null`) by default, which is on, so `measure-roll`'s file layered
last keeps an earlier `"off"`. Off keeps the measured values in the recipe, so one frame
or a whole roll can be compared without them and turned back on. Typed `off` is spared by
the roll-flag refusals under `direct` and the film master: it asks for nothing. `roll`
refuses a frame's lift stated for the whole roll (a flag, or the shared recipe), which
would lift every frame alike. A small lift typed (or in a manifest's `params`) on a frame
whose thin lift applies from elsewhere is refused: the thin lift replaces it. Stated with
a thin value, as `measure-roll`'s `frames[].flag` and a saved recipe carry it, it is the
fallback `--thin-lift off` renders. The mirror: a thin slope typed (or in a manifest's
`params`) without a thin exposure, over a small lift that would apply, is refused naming
that lift's value as the thin exposure to keep it (or `--small-lift off` to drop it),
since the slope would drop it silently. A recipe file's values are never refused, so a
saved recipe replays. `roll.frame_slope`,
`--roll-frame-slope` and an entry's `slope` (before 2026-10-02) carried the thin lift,
with the exposure beside them as its exposure: a `null` key, as every file then wrote it,
is dropped; any other value is refused, naming the rename to `thin_slope` and
`thin_exposure` that renders as before. `roll.frame_lift` / `--frame-lift` and
`measure-roll --no-frame-lift` (before 2026-10-02) switched both lifts: a `null` key is
dropped, and any other value is refused naming both switches (`"off"` →
`small_lift` and `thin_lift` `"off"`, `"on"` → both `"on"`, since it beat an earlier
`"off"`; `--no-frame-lift` → `--no-small-lift --no-thin-lift`). Retired keys are
diagnosed one at a time, a recipe with several old thin entries one entry per run; a
retired slope's message migrates the switch beside it too, so following it never meets
the switch's refusal.

- `roll.frames` (no flag) maps a **file name** to that frame's own
  `{"white_stops": …, "exposure": …, "thin_slope": …, "thin_exposure": …}` (any may be
  `null`) — the clamps and lifts `measure-roll --out` writes. `convert` and each `roll`
  frame move their input's entry into `roll.white_stops`, `roll.frame_exposure`,
  `roll.thin_slope` and `roll.thin_exposure` before any flag, and a `--frames`
  manifest's `params` beat it; a manifest may not state `roll.frames` itself.

The report's `chain.roll` states each value (`midtone_line` among them), the `slope`
derived, whether each was applied (`white_balance_applied`, `slope_applied`,
`exposure_applied`, `frame_exposure_applied`, `thin_lift_applied`,
`midtone_neutral_applied`), and `taste_applied`, the preferences
applied by their switch's key (`small_lift`, `thin_lift`); `chain.look.base_from` is
`thin` when a thin slope set the look's. A rendered run
warns once where a recipe file's `scene_correction.white_balance` (not the identity)
or non-zero `scene_correction.exposure` sits beside the roll's — a possible leftover
(`Recipe::roll_overlap_warnings`) — and where a section with gains or a white has no
`exposure`, naming `measure-roll` and `roll.exposure`. A typed style flag never warns; typed `--roll-*` flags do not silence the
missing-exposure warning.

### Reconstruction (`reconstruction`, the fixed decode)
- `--density-scale R,G,B` ⇒ `reconstruction.scale` — the per-channel density gain
  (§7.2). **Default `[1, 0.84, 0.73]`** (`algo::fixed::DENSITY_SCALE`, since
  `pipeline_version` 5): green and blue density rise faster than red in a scan, so
  without a gain they drift against it across the tone scale. Calibrated from 31
  hand-marked neutral patches over five rolls — each roll's median nulling scale,
  averaged with equal weight per roll. Every roll measured wants blue 0.68–0.78; green
  splits by scan date (July rolls 0.86–0.90, September ~0.77), so `0.84` is a
  compromise rather than a fit. It is a **calibration**, so it is nulled deliberately in
  tests of the `D = −log10(scan / base)` definition.
- `--density-offset R,G,B` ⇒ `reconstruction.offset` — per-channel density offset,
  default `[0, 0, 0]` (§7.2).
- `--density-gamma <f>` ⇒ `reconstruction.linearization` (default `1.8`) — the film's
  linearization, finite and positive. It is the calibrated half of the single `gamma`
  the removed chain shipped; print contrast is `look.contrast`. The pre-split
  `reconstruction.contrast` is refused by name, with the `look.contrast` multiplier
  that would keep it.
- `--anchor-mid-offset <d>` ⇒ `reconstruction.anchor = {"mid-at-base-offset": d}`
  (default `0.62`), strictly positive. The placement divides by the linearization, so
  validation **resolves the rule** and rejects a non-finite anchor, and separately one
  whose exponent overflows f32 (a silently black frame; `2e38` reaches it). A retired
  placement (`white-at-dmax`, `mid-at-dmax-fraction`, `black-at-base`) is refused naming
  this one.

The report's `chain.decode` states every resolved parameter and the derived `anchor`.

**Whether the values combine into a render** is one more value rule
(`recipe::validate_render`, on `convert` and `roll`, every `roll.frames` entry
included), since legal values can multiply to zero or overflow. The film base and the
corners of the reachable scan range (each channel at the scan floor or at 1) run through
the decode, scene correction and the look; a sample that overflows f32, or a base that
grades to a luminance display black cannot place black against, is a usage error naming
each stated knob whose default alone would render. A base read from a region is taken
at 1. A combination that renders but writes a channel as 0 everywhere is a warning at
the encode instead.

### Rendering (`rendering`)
- `--rendering default|direct` ⇒ `rendering` (default `"default"`) — the base every
  stage knob starts from (§6, `crate::rendering`). A knob written `null` is unstated and
  takes its rendering's value; a stated one builds on it (the white balance multiplies
  the base gains, and `look.contrast` and `look.saturation` their base slopes; every
  other knob replaces its base value). `direct` with the film master is refused. The report states it in
  `chain.rendering`.

### Scene correction (`scene_correction`)
Per-channel gains on linear ACEScg, after the NC film RGB v1 3×3 and before the look,
after the roll's midtone line (`roll.midtone_line`, above) where one applies. A stated
white balance does not move the line, which is keyed on the roll's own gains.
- `--white-balance R,G,B` ⇒ `scene_correction.white_balance = {"explicit": [r, g, b]}`
  (finite and positive; default neutral) — multiplies the roll's gains, so every frame
  applies the same ones and this key adjusts them. Nothing else: a roll's gains are
  measured once by `hanten measure-roll` and stated in `roll`. The per-frame
  `"gray-world"` / `"percentile"` modes and `--auto-wb` retired
  (`nf-scene-correction/roll-white-balance`) — a frame's own statistics read a sunset as
  the cast — and are refused by name.
- `--exposure <stops>` ⇒ `scene_correction.exposure` (default `0`) — adds to
  `roll.exposure` and the frame's `roll.frame_exposure`; the sum is applied as `2^EV`,
  and each gain times that resolved gain must be a normal `f32`.

The report's `chain.scene_correction` states the gains and the exposure applied.

### Look (`look`)
The creative stage, scene-referred and linear, between scene correction and fit range.
Each control is **its own key** — not one CDL-style object, whose slope and offset
would restate white balance and a flare subtraction. At every control's identity the
stage is a bit-exact identity. Controls run in the order listed.
- `--contrast <f>` ⇒ `look.contrast` (`nf-look/contrast-definition`) — a multiplier on
  the base slope, finite and positive, default `1`. The look applies the **slope**,
  `base × contrast`, pivoted at mid-grey **on luminance** (`nf-look/contrast-on-luminance`):
  `Y′ = 0.18 · (Y / 0.18)^slope` on ACEScg luminance and `out = in · Y′ / Y`, so a
  pixel's colour ratios do not move with contrast; slope 1 reproduces the scene's
  contrast, and a pixel whose luminance is not finite and positive passes through. The
  base is a thin frame's slope
  (`roll.thin_slope`, while `roll.thin_lift` is on), else the applied roll's span slope
  (`roll.white_stops` and `roll.dark_stops`, above), else the fallback, `log2(1/0.18) / 1.75`,
  a white placed at diffuse white from
  +1.75 (`look::DEFAULT_SLOPE` ≈ 1.414, whole slope 2.54; `nf-calibration/no-roll-defaults`
  chose it by review over 2.0 and the white rule's floor), and `direct`'s pinned ≈ 1.414.
  On a neutral the look's slope is a steeper decode exactly. Scene correction runs
  first, so an exposure of `e` stops leaves the look as `e · slope` stops. The **whole
  slope**, `linearization · slope`, is internal and must be a normal `f32`; no report
  states it, since it moves when the linearization is recalibrated even when the picture
  does not.
- `--saturation <f>` ⇒ `look.saturation` (`nf-look/contrast-on-luminance`) — a multiplier
  on the rendering's colour, finite and positive, default `1`. The look applies the
  **saturation slope**, `base × rendering saturation × saturation`, where the base is the
  contrast's except that a thin frame's slope never reaches colour (there it is the
  white's, else the fallback), and the rendering saturation is `default`'s **1.15**
  (`look::DEFAULT_SATURATION`, by review on three rolls against held colour, ×1.3 and
  SilverFast CCR) or `direct`'s pinned 1: `q_c = (x_c / Y)^s`, then
  `out = q · Y / Y(q)`, so log ratios between channels scale by `s` and luminance is
  kept. At a rendering saturation of 1 the chroma is what a per-channel power at the
  base slope gave. A pixel is saturated whole or not at all,
  as the grade is. `linearization · saturation slope` is what highlight desaturation's
  band divides by and must be a normal `f32`.
- `--channel-grade R,B` ⇒ `look.channel_grade` (`nf-look/per-channel-grade`) = `[r, b]` —
  red and blue exponents of a power pivoted at mid-grey,
  `p_c = 0.18 · (x_c / 0.18)^g_c` with `g = [r, 1, b]` (green fixed at 1: a common
  exponent under the restore would be a saturation knob), then the ACEScg luminance
  restored, `out = p · Y(x) / Y(p)`, so it never moves neutral contrast and a neutral
  mid-grey stays exactly neutral while a cast grows away from it. Both exponents finite
  and positive and the spread over `[r, 1, b]` under 1, which keeps it monotone in
  exposure; `[1, 1]` is the identity. A pixel is graded only when all three channels and
  both luminances are finite and positive and the result is finite; any other pixel
  passes through whole, bit for bit.
- `look.highlight_desaturation` (`nf-look/path-to-white`) =
  `{strength, start_stops, band: [s0, s1]}` — `--highlight-desaturation`,
  `--highlight-desaturation-start`, `--highlight-desaturation-band`. Per pixel on
  scene-referred ACEScg: `rgb ← rgb + strength · b · w · (Y − rgb)`, where `b` is a
  smoothstep in stops from `start_stops` (default −1) up to diffuse white, held above
  it, and `w` a linear band over `s = log10(max/min) / (linearization · saturation
  slope)` — the negative's density spread, the same at any contrast or saturation — full
  pull at
  `s ≤ s0`, none at `s ≥ s1` (unset: `0.015, 0.025`). Luminance is kept. `strength` is
  in `[0, 1]`, unset `0.8` under `default` and `0` under `direct`; `0` is off, a
  bit-exact identity. It assumes a roll-level white balance ahead of it.

The report's `chain.look` states `contrast`, `base_slope`, `base_from` (`roll`,
`thin`, `fallback` or `direct`), `saturation`, `saturation_base_slope`,
`saturation_base_from` (as `base_from`, never `thin`), the resulting
`slope` and `saturation_slope`, and the section as run.

### Fit range (`fit_range`)
Fits the scene's range into the display's, with the display's **peak** as the
operator's one per-destination argument — the destination states it, never the
recipe (`1.0` for SDR, `1000/203 ≈ 4.926` for HDR). One operator, reinhard:
`Y′ = r(Y)·(1 + (P − 1)·s(Y))` on ACEScg luminance, all three channels scaled by
`Y′/Y`, where `r` is the mid-grey-preserving extended reinhard
`u·(1 + u/W²)/(1 + u)` over `u = gain·Y` at `W = 2^headroom_stops`, the gain solved so
scene mid-grey (`0.18`) is preserved exactly, and `s` a smoothstep in stops from
diffuse white (`1.0`) to `W`. So `P = 1` is exactly reinhard, and every peak agrees bit
for bit below diffuse white. Preserving mid-grey costs the family its property of
mapping `W` to white — no member can do both — so `W` is the curve's scale, and content
above it exceeds the peak on every branch, clamped and counted at the encode. A
non-finite sample is refused, naming the pixel; a pixel with luminance ≤ 0 is scaled
by the curve's limit at black, so the scale is continuous there.

- `--display-tone-headroom <stops>` ⇒ `fit_range.headroom_stops` — finite, `0`–`24`
  (a value rule, so `roll` and per-frame overrides refuse it before any frame is
  decoded), unset `6` (`W = 64`); `0` is the identity. Beyond ~8 stops the operator
  converges on plain reinhard and extra headroom buys nothing.
- `--display-black <stops|off>` ⇒ `fit_range.display_black`
  (`nf-display-stages/parametric-operator`) — where the film base renders, in stops
  below mid-grey on the display, `(0, 16]` or `"off"`, unset `6`. On reinhard's output
  `y`, before the peak's lift: `y′ = y · 2^(shift · (1 − smoothstep(u)))`, `u` running
  over `log2 y` from the base's rendered level `b` to mid-grey,
  `shift = log2(0.18 · 2^−stops / b)`. `b` is the decoded film base
  (`algo::fixed::decode_film_base`) graded with the frame (`chain::render`), so it
  follows each frame's contrast and nothing is measured from the image. Mid-grey and
  above are untouched, the slope is 1 below the base, a base already at or below the
  target is left alone, and the channels scale together. A base rendering less than 2
  stops under mid-grey (only a strong exposure reaches it) is warned about: the shift
  crushes the narrow band there, and at or above mid-grey it is skipped.

The report's `chain.fit_range` names the operator (`reinhard-peak-lifted-v1`, or
`identity` at zero headroom) with its headroom, white point and display peak, and
`display_black` with its setting, curve (`log-shift-to-mid-grey-v1` or `identity`),
where the base rendered without it (`film_base_stops`) and the shift applied
(`shift_stops`).

### Fit gamut (`fit_gamut`)
No knob; the section is empty. It changes primaries into the destination's gamut
(pinned matrices from `pipeline::colorimetry`, never an installed ICC or CMM) and maps
out-of-gamut colour radially toward neutral at constant luminance, against the cube
`[0, max(peak, Y)]` — the ceiling follows the pixel above the peak, so highlights
desaturate toward white instead of gaining a hard ring. Content above the peak renders
neutral at its own luminance and is clamped, counted: at the hand-off to the HDR
encoder on an HDR destination (`chain.peak_clamp`, folded into `loss`), otherwise at
the encode. A pixel whose destination luminance is `≤ 0` renders black. The radial
intersection is computed in binary64 and sets its limiting channel to the exact cube
boundary. The stage list names it after the destination's gamut,
`acescg-to-<gamut>-matrix+neutral-axis-radial-boundary-v2`.

### Output / encode

**How artifacts reach disk (`io/transactional-output-writes`).** Every file `nc`
writes — the primary output, the film RGB export, `--save-recipe`,
`--report-file` — is written to a **same-directory temp**, flushed, **fsynced**, and
only then renamed onto its final path. Two guarantees follow, and one deliberately
does not:

- **No truncated file ever appears at a final path.** The final path holds either the
  previous content or nothing — unconditionally, including on `SIGINT`/`SIGKILL` and
  power loss, because the final path is never opened for writing. Overwrite remains
  **atomic replace**: `nc` keeps overwriting its own output rather than refusing, a
  **symlinked** target is followed so the *referent* is replaced and the link survives,
  and an existing file's **permissions are carried onto** the replacement so a `0600`
  output does not silently widen to `0644` (mode only — not ACLs or xattrs). The staging
  temp is created at the target's mode too, so a killed run cannot leave a wider-than-final
  copy of the pixels behind.
- **Three targets are refused rather than replaced**, because `rename` is more permissive
  than the `File::create` it replaced: an existing **read-only** file (rename needs write
  permission on the *directory*, so a deliberate `0400` output would otherwise be silently
  overwritten), a **non-regular** file (FIFO, socket, device node — `create` opened those;
  a rename destroys them), and **two artifacts that resolve to the same file** (possible
  when a symlinked output points at another artifact's path, which the up-front collision
  check cannot see because it compares the paths as given). Each is exit 5 with a message
  naming the path and the reason. A read-only or non-regular target, or a directory, is
  refused when its file is staged and again before the rename.
- **Hard links are reported, not refused.** An atomic replace necessarily breaks them — the
  other names keep the previous file's bytes — and writing through the shared inode instead
  *is* the non-atomic behaviour this removes. So a target with `nlink > 1` converts and emits
  a warning (report + stderr, `--strict`-promotable) rather than failing or going quiet.
- **Temp cleanup is narrower than that.** Ordinary error paths remove the staging file;
  a signal that kills the process does **not** run destructors, so `SIGINT`/`SIGKILL`
  can leave an inert `*.nctmp` beside the output. No signal handler or startup
  scavenging is installed, so the guarantee is stated for ordinary error paths only.
- **One conversion's artifacts commit together.** The side exports and the primary
  are all staged before any is renamed, so a failure in a later one
  leaves *no* primary output. The renames are
  pre-checked (a target occupied by a directory fails before anything is promoted) and
  the **primary is renamed last**, because its presence is what reads as success.
- **Not a multi-file transaction.** POSIX `rename` is atomic per *file*; a set cannot
  be flipped as one unit. A crash between two renames, or a rename failure no cheap
  check predicts, can still leave one final path updated and another not. This is
  inherent and stated rather than papered over.

`--save-recipe` and `--report-file` are staged individually but *not* held back to
join that set: the former is staged before the frame and committed only once the run has
passed. So the run fails before the decode when the recipe's path cannot be written (a
missing directory; a directory, a read-only or a non-regular file there: exit 5) or
resolves to the output, the IR or film RGB export or `--report-file` through a symlink
(exit 2).
`--report-file` must land even when `--strict` then fails the run (and under `roll` it is a roll-level
artifact no single frame's set could hold). Telemetry is never part of the set: its
event is written last, best-effort. Directory fsync (power-loss
durability for the rename itself) is out of scope: the temp+rename pattern already
covers a full disk, a permissions error, a crash and `SIGINT`, and the remaining gain
would cost a Unix-only code path for output that is reproducible by re-running.

- `-o, --output <path>` (required on `convert`; `roll` takes `--out-dir`) — suffix
  checked or completed, never renamed (§5).
- `--range`, `--transfer`, `--gamut`, `--container` ⇒ `output.display.{range,
  transfer, gamut, container}` — the destination's four axes (§5), each optional and
  derived when unset. Or `--film-master` ⇒ `"output": "film-master"`. One enum field,
  never parallel bools: a recipe cannot state both.
- **The film master refuses any stage the recipe asks for** — scene correction, the
  look or fit range, one rule per stage naming it — after merge, on the *resolved*
  value, whatever its source. Each rule spares its default and its identity, so a flag
  resetting a recipe value (`--exposure 0`) re-exports a graded roll recipe as a
  master. The `--roll-*` flags are refused beside `--film-master` too. There is no ignore mode;
  the rendered float output is the linear HDR TIFF.
- BigTIFF promotion is always automatic: a file too large for classic TIFF is written
  as BigTIFF, and the report says so.

**Per destination:**

- **SDR TIFF** — 16-bit integer, lossless, the gamut's own transfer curve and ICC
  profile, `1.0` = the 203 cd/m² reference white.
- **Gain-map JPEG** (`nf-destinations/gain-map-destination`, `io::iso_gain_map`) — an
  8-bit SDR base in Display P3 or sRGB, and a half-resolution **per-channel** gain map
  to the HDR rendition, described by **ISO 21496-1 metadata only**, in a Multi-Picture
  Format container nc writes itself; the gain map's MP Type is `050000`. It carries no
  Exif, so its base is not the CIPA DC-007 baseline ISO 21496-1 C.4.3 asks for
  (DC-007 §4.2.1, §5.1); no reader tested needs one. The Ultra HDR v1 XMP dialect cannot describe a per-channel map and
  is not written, so neither is libultrahdr used. Every APP segment sits before `SOF0`, where readers stop looking.
  The gain is taken against the base **as stored**, since that is what a decoder
  multiplies (`pipeline::gain_ratio`). Adobe RGB has no gain-map row. Apple ImageIO
  reads it as HDR with three distinct channel entries; a change to the container needs
  the manual `scripts/iso-decoder-oracle/` check (macOS), since exiftool accepts files
  no decoder parses. Other viewers are `analysis/viewer-interoperability`'s, Android
  `analysis/android-gain-map-check`'s.
- **Linear HDR TIFF** — the HDR rendition's display-linear samples, clamped at the
  peak and counted (`chain.peak_clamp`), written verbatim as 32-bit float in Display P3, Adobe RGB, sRGB or BT.2020, with a synthesized
  linear ICC profile of that gamut: `1.0` is the 203 cd/m² reference white, the peak
  `1000/203 ≈ 4.926108`. Because the ICC PCS stops at the media white, no profile can
  state the luminance mapping, so the report's `hdr_linear_tiff` block is
  authoritative for reference white, peak, headroom and the frame's measured
  content-light levels — CTA-861.3's MaxCLL and MaxFALL, the peak and frame mean of
  each pixel's largest linear component in the stored primaries (so they differ by
  gamut), never luminance. The profile carries **no** `cicpTag`: H.273's full-range flag
  describes a bounded code range, and these samples exceed 1.0 by design. It is not the
  film master (linear ACEScg *before* any rendering).
- **PQ / HLG TIFF** — the Rec.2100 signal as **full-range 16-bit TIFF code
  values**. Lossless *relative to the quantized signal*: quantized once with one pinned
  rounding rule (`round`, half away from zero), every code stored exactly, the max and
  RMS quantization error reported in code units. A sample outside `[0, 1]` is
  **rejected, not clipped** — the transfer guarantees the domain. **16 bits is TIFF's
  quantization, not one of BT.2100's own depths** (10 and 12), and the report says so.
  TIFF has no CICP tag, so the signalling lives in the embedded ICC profile's `cicpTag`
  (ICC.1:2022 §9.2.17/§10.3): `9-16-0-1` for PQ and `9-18-0-1` for HLG, with
  **MatrixCoefficients 0** because the data is RGB. Only a CICP-aware colour-managed reader honours it, so these are
  **limited-interoperability interchange, never "display-ready"**. The PQ profile is an
  extended-range A2B (`lutAtoBType`) whose PCS is `Y = L / 203`, unclipped to ≈49.26;
  the HLG profile is scene-referred, since HLG's OOTF is not per-channel separable, and
  the display-referred contract (1000-nit peak, zero black, system gamma 1.2) lives in
  the report's `hdr_coded_tiff` block, which carries the content-light levels for PQ
  only.
- **Film master** — the fixed decode's linear ACEScg, unclamped 32-bit float, with the
  ACEScg profile and no output transform (§6).

### Retired flags and keys

Each is a usage error (exit 2) naming its replacement — a migration error, never an
alias, on flags and recipe keys alike. The reference build
(`scripts/reference-snapshot/`) reproduces what they did.

- **The print stage and the presets** (`nf-core/default-flip`): `--output-preset` and
  `output.preset` (→ the four axes or `--film-master`), `--print-exposure` (→
  `--exposure`), `--black-point` (→ `--display-black`), `--auto-wb` (→ `measure-roll`
  and `--roll-white-balance`), `--linear-range` (retired: its gain is `--exposure`,
  its black `--display-black` — `nf-scene-correction/levels-knob`), the whole `print`
  section, `--new-flow`, and the sidecar. Earlier still, `--output-hdr` and
  `--output-sdr`, the names before the presets. Earlier:
  `--out-depth`, `--output-profile`, `--bigtiff` with the `legacy` and `custom` presets
  (`nf-retire/legacy-custom`), and `--display-tone` / `--highlight-compress` with the
  bounded display tones (`nf-retire/display-tones`).
- **The reconstruction menu** (§7.3): `--algorithm`, `--reconstruction`,
  `--density-curve`, `--film-stock`, `--preset`, the `--sigmoid-*` flags; the
  reference-density flags `--d-max`, `--fixed-d-max`, `--auto-d-max`, `--no-d-max`,
  `--anchor-mid-fraction`, `--anchor-white-at-reference`, `--anchor-black-floor` and
  `measure-base --d-max-region`; the regional balance's `--shadow-balance`,
  `--highlight-balance`, `--balance-range`, `--auto-balance-range`; and `simple`'s
  `--invert-white-balance`, `--clip-low`, `--clip-high`.
- **Film base and input**: `--auto-base` and `"auto"` (a base is always stated),
  `--assume-linear` and `input.color` (→ the two input axes), the `estimate`
  subcommand (→ `measure-base`), and `measure-base --grid` (→
  `film-base/tiling-uniformity-validator`). `--export-ir` and `input.export_ir`
  retired with no replacement (`nf-verification/roll-side-exports`); a `null` key, as
  older dumps wrote it, is dropped.

### Global
- `--params <json>` (repeatable), `--save-recipe <path>`
- `--report json|none`, `--report-file <path>`
- `--strict` — promote report warnings (clipping, non-finite samples, a channel
  written black everywhere, a non-uniform film-base area or region, …) to a failing exit (see §11); on `convert`, `roll`, `measure-base` and `measure-roll`
- `--max-memory <bytes>` — peak-memory budget for the run (`8GiB`, `512MB`, or raw
  bytes). Every command that decodes a scan (`convert`, `roll`, `inspect`,
  `measure-base`, `measure-roll`) estimates its peak allocation from a **metadata-only header probe
  before decoding** and fails with exit 6 when it would exceed the budget. `roll`
  gates **per frame**, and follows its usual per-frame error handling: the frame's
  resource error is recorded in its report entry, sibling frames are still
  converted and written, and the roll exits **1** ("frames failed"), not 6.
  Default **6 GiB** — deliberately a fixed constant, not a
  fraction of detected RAM, so the pass/fail decision is the same on every
  machine. An estimate that fits the budget but exceeds ~70% of detected physical
  RAM warns instead — `--strict`-promotable on `convert`/`roll`/`measure-base`, and
  report-only on `inspect`, which has no `--strict`. Like `--report`/`--strict`/telemetry
  this is **operational**: not a recipe key, never in the recipe, and it can
  never change an output byte. The estimate, its per-phase breakdown, the budget,
  and the decision ride out in the JSON report's `memory` block.
  **Second effect to know about:** the budget also caps the `tiff` crate's read
  buffers (`min(4 GiB, budget)`), so a budget that admits the run but sits below a
  single plane's read buffer turns a decodable file into a decode failure (exit 3)
  rather than a resource error. A passing preflight makes that nearly unreachable —
  the estimate is a multiple of the read buffer — but it is the one way this
  operational flag changes an outcome other than the gate's own verdict.
- `-v/--verbose`, `--quiet`

**Roll (batch, `hanten roll` only — orchestration flags, NOT recipe keys).** `hanten roll`
converts many frames from one shared recipe, resolved like `convert`'s — the
layered `--params` and every conversion flag `convert` takes — and adds no new
conversion knobs. Its own flags are operational (like `--report`): `--out-dir <dir>` (per-frame outputs `<stem>_positive.<ext>`, the
suffix following each frame's resolved destination),
positional `inputs` (files and directories — a directory is expanded to its
`.tif`/`.tiff` files, sorted; shell globs are expanded by the shell, not by nc)
**or** `--frames <manifest.json>` (explicit per-frame `input`/`output`/partial-recipe
`params` overrides, deep-merged onto that frame's resolved recipe — its `roll.frames`
entry, then the flags — for that frame only, so they win over both).
`calibration.film_base` has no default, so a roll states one, by `--film-base` or in
a `--params` layer, or exits 2. `Dmin` is measured once for the roll (`hanten
measure-roll --unexposed`, or `hanten measure-base`) and frozen as an explicit base,
which is the only source that keeps every frame on one base — see the roll-fixed
invariant warnings below.
The roll report's top-level `identity` stamps the shared recipe once; each
frame reports the *resolved* base it used — a redundant echo when the
recipe pins an explicit base, but meaningful under a `region` base that
resolves per frame. Frame-local knobs are the per-frame `params` overrides. Roll-fixed
invariant violations are **loud, `--strict`-promotable warnings** rather than hard
errors, so a deliberate best-effort batch remains usable: (1) a shared
`calibration.film_base` other than `explicit` re-estimates Dmin per frame; (2) a
per-frame override that resolves a **roll-wide** value differently from the shared
recipe — `calibration.film_base`, the applied `roll.white_balance`, `roll.midtone_line`, `roll.dark_stops` or `roll.exposure`, any
`reconstruction` key, `rendering`, or a stated `output` — warns, naming both values
(`cli::ROLL_WIDE`). A restatement does not; `frames[].overrides` records it.
`roll.white_stops` is frame-local: it is how a clamped frame states its own white.
Determinism: same batch + same recipe ⇒ byte-identical output per frame.

**Telemetry (operational, `convert` only — NOT recipe keys).** Opt-in
performance + context telemetry. These are operational flags like `--report`, so
they are **not** conversion knobs: they never enter the recipe and never affect
the output bytes (telemetry on or off ⇒ byte-identical output).
- `--telemetry` — append one JSON event for this run to the local JSONL log
  (default `$XDG_DATA_HOME/nc/telemetry.jsonl`, else `$HOME/.local/share/nc/…` on
  Unix / `%APPDATA%\nc\…` on Windows; override with the `NC_TELEMETRY_LOG` env
  var). Create-append; one object per line.
- `--telemetry-file <path>` — also write the event to `<path>` (`-` = stdout;
  overwrites a one-off file). May be combined with `--telemetry` (event lands in
  both sinks). Telemetry is collected iff at least one of these flags is present.
- **Every run that parses writes one event**: a success, or a failure naming the
  stage it ended in and its error kind and exit code — never the error's text. A
  `--strict` promotion is a failure of kind `strict`. A run that fails before the
  write-target guard writes its event only if the sink is clear of the input and
  every output it knew of. A command line clap rejects writes none, except under
  upload consent (below).
- **Upload (`hanten telemetry`, opt-in, persistent).** `hanten telemetry enable`
  shows the upload field manifest and, once confirmed, selects one queue (the log
  above, or `--queue PATH`). From then on every `convert` — a refused command line
  too, as a `parse` failure — appends its event there, and a detached helper
  uploads its privacy projection (`contracts/telemetry/upload-v1/`) after the run's
  outcome is fixed. `disable`, `purge`, `status`, `preview` and `flush` manage it;
  `NC_TELEMETRY=0` turns it off for one process. Records of an older local schema
  are dropped, not uploaded. The endpoint is fixed at build time
  (`NC_TELEMETRY_ENDPOINT`; `none` builds a binary that uploads nothing). Its
  consent, locks and queue rules are `docs/telemetry-strategy.md`.
- **Best-effort:** a telemetry failure is warned on stderr and never changes the
  exit code (`--strict` does not promote it) — the one deliberate deviation from
  the fail-loudly rule, since telemetry is non-critical observability. A
  `--telemetry-file` **or**
  `--telemetry` log path (`NC_TELEMETRY_LOG` or the default path) that would *collide* with the
  input, the `--params` recipe, or the output/export/report-file is still a loud
  usage error (a config mistake, caught up front — an odd log path must never
  silently append into the scan).

**Telemetry event shape (`schema_version` 11).** The uploader reads queued events
back to project them (`telemetry::spool`). A success:
```json
{
  "schema_version": 11,
  "event_id": "5f0c3a9e81d24b7c9e0a6d3f2b1c8e47",
  "event": "conversion",
  "command": "convert",
  "timestamp_ms": 1790633724299,
  "nc_version": "0.1.0",
  "target": "aarch64-apple-darwin",
  "cpu_count": 11,
  "stage": "finalize",
  "image": {
    "format": "hdri", "width": 502, "height": 462, "megapixels": 0.231924,
    "bit_depth": 16, "channels": 3, "ir_present": true,
    "input_bytes": 2017230, "output_bytes": 1392366
  },
  "timing_ms": {
    "total": 59.9, "decode": 15.3, "film_base": 0.0, "reconstruction": 5.6,
    "scene_correction": 0.0, "look": 1.7, "fit_range": 4.0, "fit_gamut": 7.8,
    "destination": 5.8, "encode": 6.0, "ir_export": 6.2
  },
  "conversion": {
    "destination": { "display": { "range": "sdr", "transfer": "native",
                                  "gamut": "display-p3", "container": "tiff" } },
    "params_hash": "a6bcbaf9b33f4480",
    "film_base_source": { "explicit": [0.9, 0.55, 0.42] },
    "output_depth": "u16"
  },
  "outcome": { "status": "success", "error_kind": "none", "exit_code": 0,
               "warnings": 1, "total_samples": 695772, "clipped": 0, "non_finite": 0 }
}
```
A failure carries only what the run reached — here a scan refused right after decode:
```json
{ "schema_version": 11, "event_id": "…", "event": "conversion", "command": "convert",
  "timestamp_ms": …, "nc_version": "0.1.0", "target": "…", "cpu_count": 11,
  "stage": "decode",
  "image": { "format": "hdr", …, "output_bytes": null },
  "timing_ms": { "total": 20.4, "decode": 14.9 },
  "conversion": { … },
  "outcome": { "status": "failure", "error_kind": "unsupported", "exit_code": 4,
               "warnings": 1 } }
```
`event_id` is 128 random bits, new for every event: the upload's deduplication key,
never a correlation across events. `stage` is a `crate::stage::StageKind` name, or
`parse` (a command line clap refused; written only under upload consent), `setup`
(recipe, validation, output path, the write-target guard), `preflight` (a
frame's checks before its first stage) or `finalize` (the report and the `--strict`
gate, where every success ends); a check between two stages belongs to the one before
it. `error_kind` is `none` exactly for a success, else `usage`, `decode`,
`unsupported`, `write`, `resource`, `other` — the error's §11 category — or `strict`.
`image` is absent before decode, `conversion` before the destination resolved,
`total_samples` / `clipped` / `non_finite` unless the frame finished (`total_samples`, the
samples the encoder examined, is their denominator), and `output_bytes` is `null` unless the
output was written.

`timing_ms` has one field per stage (`crate::stage::StageKind`), present once that
stage **completes** — a failed stage's time counts only toward `total`, which also covers
recipe load, validation, the memory preflight and the commit. The four chain stages are
absent for the film master, which runs none, and `ir_export` always (retired with
`--export-ir`); a
gain map's `fit_range` and `fit_gamut` sum its two renditions (the copy that splits
them counts only toward `total`), and `scene_correction` and `look` include the film
base's one-pixel grade. The history of the shape is `telemetry::SCHEMA_VERSION`'s
rustdoc.
`conversion.destination` is the resolved recipe `output`, every axis stated — without
it two f32 TIFFs (the film master, a linear HDR TIFF) are indistinguishable.
`conversion.output_depth` names the **primary** artifact's depth (`u8` for the gain-map
JPEG), not the optional IR TIFF's.
`params_hash` is a stable hash (`Recipe::params_hash`) of the resolved recipe — the
`params` body `--save-recipe` writes, dedented — the same value as the report's
`identity.params_hash`, so identical
conversions share a hash without the record carrying the recipe. The value shown is
**illustrative**: it covers the whole recipe and changes whenever any key is added,
removed, or re-defaulted. Nothing asserts it as a constant.

## 10. Code architecture (Rust)

Pure functions per stage; the CLI is the only orchestrator. One binary crate, `nc`,
with binary `hanten`:

```
src/
├── main.rs              # clap entry → cli
├── cli.rs               # subcommands, recipe load/merge, validation, orchestration, report
├── recipe.rs            # the recipe (recipe_version 3), one section per stage
│   └── compose.rs       #   layering repeated --params, then the flags
├── rendering.rs         # --rendering default|direct, and direct's pinned base
├── destination.rs       # the destination set: four axes, one table
├── stage.rs             # stage names and per-stage timing
├── types.rs             # LinearImage, FilmBase, shared params and errors
├── version.rs           # build identity, pipeline_version, the drift gate, params hash
├── telemetry.rs         # opt-in JSONL record (never perturbs output)
│   ├── upload.rs        #   the privacy-minimized upload projection
│   ├── consent.rs       #   persistent upload consent and its locks
│   ├── spool.rs         #   the upload queue: rotation, batches, quarantine, caps
│   ├── drain.rs         #   one upload pass under the request lease
│   ├── net.rs           #   the endpoint and one HTTPS request
│   ├── managed.rs       #   per-`convert` collection and the detached helper
│   ├── maintenance.rs   #   `hanten telemetry …` (and the lock order)
│   └── durable.rs       #   crash-safe, no-follow files and cross-process locks
├── algo/
│   ├── mod.rs           # FilmRgbImage, the typed reconstruction output
│   └── fixed.rs         # the fixed decode
├── film_stock/          # test-only: digitized stock curves, evidence for the decode's constants
├── io/
│   ├── decode.rs        # SilverFast HDR/HDRi (TIFF) → LinearImage (+IR)
│   ├── encode.rs        # 16-bit / f32 TIFF with ICC
│   ├── jpeg.rs          # baseline JPEG for the gain-map container
│   ├── iso_gain_map.rs  # the gain-map JPEG: MPF container + ISO 21496-1 metadata
│   └── staged.rs        # write to a temp beside the target, fsync, rename
└── pipeline/
    ├── input_semantics.rs  # transfer + measurement-meaning resolver (stage 1b)
    ├── film_base.rs        # Dmin measurement, IR holder cut, effective area
    ├── working_space.rs    # NC film RGB v1 → linear ACEScg
    ├── chain.rs            # the rendering chain composed; the SDR/HDR branch contract
    ├── working_image.rs    # the buffer every chain boundary carries
    ├── scene_correction.rs # midtone neutral, white balance, exposure
    ├── midtone_neutral.rs  # the roll's midtone line: measured, and removed per pixel
    ├── correction_confidence.rs  # measure-roll: how far to trust the white balance and line
    ├── look.rs             # contrast, saturation, per-channel grade, highlight desaturation
    ├── fit_range.rs        # reinhard to the display's peak, display black
    ├── fit_gamut.rs        # into the destination's gamut, radial to its boundary
    ├── hdr.rs              # the HDR hand-off: peak clamp, PQ/HLG transfer
    ├── gain_ratio.rs       # per-channel gain between a gain map's renditions
    ├── gain_encode.rs      # the gain-map image
    ├── roll_white.rs       # measure-roll: the roll's white balance, exposure, and white after its colour correction
    ├── white_balance.rs    # white-balance statistics
    ├── color.rs            # output transfer transforms (lcms2) and ICC blobs
    ├── colorimetry/        # every standards-based matrix, luma vector and transfer constant
    ├── pixels.rs           # parallel per-pixel map drivers (byte-identical to a loop)
    ├── memory.rs           # peak-memory sizing model + budget preflight
    ├── chain_golden.rs     # (test) per-pixel goldens for the chain's stages
    └── branch_probe.rs, shadow_metrics.rs  # (test) diagnostic probes
```

The tree is the shipped module set, not a proposal; re-check it whenever a module is
added. Each module's `//!` docs hold its traps (CLAUDE.md, "Where the detail lives").

### Crates

| Concern | Crate(s) |
|---|---|
| CLI parsing | `clap` |
| TIFF decode/encode | `tiff` (custom handling for scanner extras) |
| ICC color management | `lcms2` (rust-lcms2) |
| JPEG | `jpeg-encoder` |
| Metadata read | `tiff` (tags), `roxmltree` (SilverFast XMP) |
| Recipe / report JSON | `serde`, `serde_json` |
| Parallelism | `rayon` |

## 11. Error handling & exit codes

| Code | Meaning |
|---|---|
| 0 | Success. |
| 1 | Generic / unexpected error. |
| 2 | Invalid CLI usage or parameters. |
| 3 | Input read/decode error (unreadable or unsupported file). |
| 4 | Unsupported variant (e.g. channel layout we can't handle yet). |
| 5 | Output write error. |
| 6 | Resource limit — the run's estimated peak memory exceeds its budget. |

Warnings (e.g. clipped highlights/shadows, IR present but ignored, BigTIFF
auto-promoted) are surfaced in the JSON report and on stderr, without failing the
run unless `--strict` is set.

**Input-semantic resolution** (§9 Input/decode) maps to these codes: an
ambiguous or unsupported input (transfer/meaning that cannot reach a supported
linear + scanner-device resolution — including an asserted `colorimetric`
meaning) is an **unsupported** input, exit 4; an explicit assertion that
contradicts authoritative container structure, the removed combined `input.color`
recipe key, and the deprecated `--assume-linear` flag are **usage** errors, exit
2; `--input-profile` (reserved, not applied) is unsupported, exit 4. `hanten inspect`
never fails on ambiguity — it reports the per-axis evidence so the file stays
diagnosable.

**Output write failures** map to exit **5**, and since
`io/transactional-output-writes` that exit carries a stronger promise: no truncated
artifact is left at a final path, and a failure while writing any of one conversion's
artifacts leaves *no* primary output rather than an orphaned one (§9 Output/encode).
A run that fails through an ordinary error path also leaves no `*.nctmp` staging files;
a run killed by a signal may leave one, since destructors do not run then.

**A reader that goes away is not a failure.** When stdout's or stderr's reader
closes the pipe (`hanten … | head`), the write is dropped and the run carries on —
the `--out` recipe, the `--strict` and failed-frame gates and telemetry all come
after the report — so the exit code is the run's own. `SIGPIPE` stays ignored for the
same reason: restoring it would kill the run at the report. Any other failure to
write the report or `params` to stdout is exit 5 (`cli::print_stdout`).

**Memory preflight** (§9 Global, `--max-memory`) maps to exit **6**: before any
input is decoded, every command that reads a scan estimates the run's peak
allocation from a metadata-only header probe and compares it against the budget.
Over budget is a **resource** error, deliberately distinct from *unsupported*
(exit 4) — the input is fine; it is this run on this budget that cannot proceed,
so an agent can retry with a larger `--max-memory` (or on a bigger machine)
rather than discard the file. `measure-roll` gates each frame it decodes, and a refusal
keeps exit 6. On `convert`, `inspect`, and `measure-base` no image
or report is produced on that path, and no `--save-recipe` file. On **`roll`** the same rejection is
one frame's error: it is recorded in that frame's report entry, the roll continues
(sibling frames are converted and written), the report is emitted, and the roll
exits **1** — the batch-level "frames failed" code, as for any per-frame error.
An estimate that fits the budget but exceeds ~70% of detected physical RAM
is a `--strict`-promotable **warning**, not a failure. A malformed
`--max-memory` value is a usage error (exit 2).

Determinism note: the *image output* is unaffected by any of this, and the
pass/fail decision is machine-independent because the default budget is a fixed
constant. The **warning** tier is the one deliberately environment-dependent
piece — so under `--strict` the same input can exit differently on a small
machine than on a large one.

A **degenerate resolved film base** (a zero / negative / non-finite channel)
maps to exit 1 (generic error) on every measurement — a stated region
(`film_base::estimate`) and `measure-base`'s effective area (`film_base::measure_area`)
share the finite-and-positive guard. This is unconditional, distinct from the
`--strict`-only promotion of the non-uniformity warnings.


## 12. Roadmap

Deferred work, recorded so it isn't lost. Items graduate into tracked tasks in
[TASKS.md](TASKS.md). Numbers are stable because other documents cite them, so a
shipped or retired item keeps its number and shrinks to one line.

1. **IR-based dust & scratch removal.** Consume the IR channel (decoded, but dropped
   at the fixed decode today, so the stage carries it on) to build a defect mask and inpaint defects. Parameters: IR threshold, mask
   dilation/morphology, inpainting method/strength. Must handle the known limits —
   disable/guard for silver B&W film and Kodachrome. New stages: `defect_mask`,
   `inpaint`. New flags under an `--ir-*` namespace.
2. **Additional reconstruction models** — *superseded*: there is one decode (§7).
   What remains is in rendering — per-stock normalization and print emulation as look
   controls — plus, possibly, a model for camera-scanned negatives (item 4).
3. **Black & white film.** Input: a single-channel gray primary
   (`io/gray-primary-decode`) and plain 16-bit RAW scans. Rendering:
   `algo/bw-support`, with where mono pools in the chain `nf-reconstruction/mono-decode`.
   B&W negatives have no orange mask and no IR defect channel (silver blocks IR), so
   item 1 must be disabled for them.
4. **Camera RAW input.** Bayer/X-Trans and DNG ingestion (e.g. `rawler`/LibRaw)
   to support camera-scanning workflows.
5. **More output formats.** The SDR JPEG (`output/sdr-jpeg-preset`), PNG for proofs,
   EXR for HDR interchange. HEIC gain maps are deferred pending a portable
   final-standard encoder and an approved HEVC licensing/packaging policy.
6. **Roll workflow & automatic calibration.** Shipped: `hanten roll` (one frozen
   recipe, per-frame manifest overrides, one roll report), `measure-base` and
   `measure-roll` (§8). Open: `roll`'s measure mode (`core/roll-measure-mode`), the
   acquisition cascade that *generates* the recipe (`core/auto-calibration`), reusable
   pipeline profiles (`core/profile-authoring`), and bounded per-frame lifts
   (`nf-calibration/thin-frame-lift`, `frame-level-trim`).
7. **Optional color-correction QA harness.** Target-based fitting and ΔE2000 /
   SSIM regression testing against controlled negatives may support explicitly
   selected correction profiles. It is not part of the default film-preserving
   pipeline and is distinct from blindly applying a conventional positive-scanner
   ICC before density.
8. **Auto film-base detection** — *shipped, then retired*: nc no longer searches for a
   rebate (§9 film base). The opt-in content-based source is `film-base/content-fallback`.
9. **Light film holders.** The IR holder cut reads opacity, not colour, so a white
   holder that blocks IR is cut like a dark one. A `--holder white|black` control
   (`measure.holder`) would matter only to an RGB holder measure, which nc does not
   have. Not under `calibration`: a holder is a property of the scanner setup, not a
   measurement of the roll.
10. **Reuse-ready base measurement** — *shipped* as `measure-base` and its `--out`.
11. **UI-assisted film-base picking.** Once a UI layer exists: visual region
    picking for the reference frame, and feedback when a chosen region fails the
    uniformity check.
12. **Crash reporting & opt-in telemetry.** The **local, opt-in telemetry
    record** has **shipped** as the `perf-telemetry` task, and `telemetry/schema-v2`
    made it a typed success/failure event: an embedded, opt-in JSON event per
    `hanten convert` (outcome + image + per-stage timing + run context) written
    to a local JSONL log and/or one-off file (`--telemetry` / `--telemetry-file`,
    `NC_TELEMETRY_LOG`; see §9), best-effort and byte-identical-output-preserving.
    **Upload has shipped** (`telemetry/upload`, `hanten telemetry`, §9), and so
    has panic reporting (`telemetry/panic-hook`): one sanitized event for a
    consented `convert` that panics.
    The `telemetry/strategy` spike is **complete**; its approved
    [design note](telemetry-strategy.md) fixes the remaining shape. The client
    keeps custom JSON (no embedded OTel SDK/Collector) and sends a separately
    versioned, allowlisted upload projection to an nc-owned Cloudflare Worker +
    D1 service. Persistent `hanten telemetry enable` consent opts into automatic
    `convert` success/failure/panic collection and detached, crash-safe queue
    draining from exactly one consent-stored active JSONL plus its derived private
    sibling spool and immutable generation. Collection consent is an
    invocation-start snapshot: disable stops new snapshots/helpers and waits for
    bounded network requests, but an already-running convert may finish one local
    queued event afterward. Inactive-only purge waits those invocations. Other
    commands are out of v1; explicit per-run telemetry does not independently
    enable upload or install the panic hook. Active queue retargeting is rejected;
    inactive retarget requires the old queue empty. Purge preserves the private
    spool and stable lock inodes while clearing its data. Active same-path enable
    is a no-op; inactive same-path enable waits old invocations and the old helper
    before publishing a fresh generation and launching one replacement.
    `NC_TELEMETRY=0` disables automatic collection/networking. Upload carries no
    persistent identity, `params_hash`, exact paths/timestamps/dimensions/sizes,
    messages, recipe/parameter values, or raw backtraces. The implementation is
    split into `telemetry/schema-v2`, `telemetry/upload-schema`,
    `telemetry/ingestion-service`,
    `telemetry/upload`, and `telemetry/panic-hook`; the latter is deliberately
    described as sanitized Rust **panic reporting**, not general native-crash
    capture. The anonymous endpoint cannot prove event provenance, so results are
    advisory/opt-in/unverified rather than exact population rates. V1 runs on a
    paid Cloudflare account with application ceilings bounding the worst case
    (`services/telemetry-ingest/README.md`); any change whose worst case can pass
    $10/month requires explicit approval. Note: the original
    LAB-benchmark `perf-instrumentation` task is **parked** (prototype on
    `prototype/perf-bench-instrumentation`); `perf-telemetry` is the real-world
    successor.
13. **Roll workflow** — merged into item 6.
14. **Roll-fixed `Dmax`** — *shipped, then retired* (§7.3).
15. **IR-assisted film-holder detection** — *shipped*: the holder cut of the effective
    area (§6.1, §9 `measure`).
16. **Conversion versioning & baseline comparison** — *shipped*: `identity` (§8), and
    `nctool compare` over `scripts/analysis/benchmark.json`.
17. **Stdout broken-pipe safety** — *shipped* (§11): a reader that closes stdout or
    stderr early ends neither the run nor its exit code.
18. **Input data semantics** — *shipped* (§4, §9 Input / decode).
19. **Conventional scanner ICC before density — deferred experiment.** Compare
    `scanner RGB → Dmin/log density` against applying the same scanner ICC to image
    and Dmin first, using only a defined linear destination and controlled target
    error. `--input-profile` stays rejected unless this validates a supported path.
    Tracked: `color/scanner-profile-before-density-experiment`.
20. **Film-preserving reconstruction and working space** — *shipped* as the fixed decode
    and NC film RGB v1 (§7). Optional correction profiles remain open
    (`color/optional-color-correction-profiles`).
21. **Display P3 SDR output** — *shipped*: an SDR destination row (§5).
22. **Display HDR rendering and AVIF** — HDR rendering *shipped* (§5, §9); AVIF
    shipped and was then **removed** (`output/drop-avif`,
    [`design/avif-removal.md`](design/avif-removal.md)).
23. **ISO gain-map HDR** — *shipped* as the per-channel, ISO-only gain-map JPEG (§9).
    Open: viewer checks (`analysis/viewer-interoperability`; Android in
    `analysis/android-gain-map-check`), and
    cross-device acceptance (`analysis/display-output-acceptance`).

## 13. Open questions

Each names the task that owns it, or says it has none.

- **Exponential vs `generic-c41`**, or a toe-limited form (invert only where the slope
  carries information), as the fixed decode. The two target different densitometries,
  so a clean comparison needs the calibration below; the ADX precedent argues for
  `generic-c41`. No task yet.
- **How to get to Status M**: the 3×3 + offset, fitted per roll or per developer, and
  from which frames. Interimage effects are per stock and cross-channel, so a
  per-scanner matrix cannot be the whole answer (`io/scanner-density-calibration`,
  `analysis/calibration-frame-capture`).
- **Whether the green residual is film, scanner or developer.** Blue's drift matches
  the sheets (+1.26 measured, +1.29 predicted); Ektar's green does not (+1.26 against
  +0.22), and the shipped `scale` carries it (`io/scanner-density-calibration`).
- **Whether `offset` earns a non-zero default** (`nf-calibration/offset-question`).
- **How to evaluate a reconstruction**: a bracketed neutral series, whose slope must
  equal the stock's own contrast; colour patches measure consistency, not true colour
  (`nf-calibration/neutrality-gate`).
- **Whether nc ships a user-facing calibration workflow**, and in which stage its
  result lands (`nf-calibration/user-calibration-procedure`,
  `color/optional-color-correction-profiles`). Without one, every user inherits a
  prior fitted on one scanner and two developers.
- **Where the NC film RGB v1 matrix belongs**: reconstruction output in film-layer
  space, with the move into a working space becoming scene correction's (a generic
  assumption or a per-stock characterization)? That would change what the film master
  holds. No task yet.
- **Where per-stock normalization lives** — scene correction or the look. §7 settles
  that it is optional and in rendering, not which stage. No task yet.
- **Other outputs the old chain had** (ProPhoto, arbitrary ICC paths): keep or drop.
  No task yet.
- **The roll's white rule in HDR** (`nf-calibration/white-rule-hdr`) and the saturation
  warning's margin (`nf-calibration/saturation-margin`).
- **Rebuilding the pipeline as decode → roll → style** (`nf-core/three-step-pipeline`).

Resolved, kept as a record: the SilverFast HDRi layout (§4); linear ACEScg for the
film master (§6); the recipe travels in the report, not the image container (§5);
reinhard over a parametric shoulder (§6); a separate contrast and per-channel grade
rather than one CDL object (§9 Look); `direct` as a rendering rather than a preset
(§6); one black point, fit range's display black (§6); the fallback slope without a
roll measurement, a white +1.75 stops up (§9 Look).

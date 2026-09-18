# Design update

**Status:** agreed direction from a design discussion, 2026-09-16/17. **Not yet applied**
to `design-spec.md`, `TASKS.md`, any task file or CLAUDE.md. Where this document
contradicts them, it describes the intended design and they describe the current one.
Folding it in (spec revision, task changes) is follow-up work and has not been planned.
Git history keeps earlier versions of this document. Part 1's first version set a
different goal ("estimate scene exposure, per stock"); it was replaced on 2026-09-17 for
the reasons recorded below.

Parts are appended as each discussion settles. **Part 1** covers reconstruction,
**Part 2** rendering and output, **Part 3** how the decode is evaluated and tuned, and
**Part 4** is a placeholder for the same question about rendering.

# Part 1: Reconstruction

## Why this was needed

The split (`algo/reconstruction-render-curve-split`, verdict 2026-09-02) was justified by
its results: better highlights and a working HDR rendition at matched lightness. It never
stated what reconstruction's output **is**. Without that goal there is no principled way
to decide which methods belong in reconstruction or which knobs belong in rendering, and
no target to measure a reconstruction against. The current contract makes the gap
explicit: NC film RGB v1 is "film-rendering intent, not physical scene recovery". A stage
that promises nothing physical cannot be measured.

## The goal

> **By default nc shows what the negative really holds; it does not optimize the
> photograph. Reconstruction is a fixed, stock-agnostic decode of the negative, the way a
> traditional print sees it. It loses nothing it can avoid losing and reports what it
> could not recover. Every choice about how the picture should look, including
> per-stock normalization, belongs to rendering.**

### The model: a traditional colour print

- **One paper for every negative.** RA-4 paper has one fixed curve for all C-41 films;
  every C-41 stock is designed to print on it. The paper does not change per stock.
- **The lab adjusts two things per roll or frame:** colour filtration and exposure time.
  Those are rendering's scene correction (white balance, exposure).
- **Stock differences reach the print.** Ektar prints punchier than Portra because its
  negative is steeper (mid-scale slope, roughly: Ektar R/G/B 0.64/0.60/0.76, Portra 400
  0.52/0.57/0.63). The paper does not undo that, so neither does the decode.
- **The negative's toe is never inverted.** The paper prints the compressed shadows as
  they are.
- **Cinema does the same**, as far as known (unverified in detail): Cineon/ADX scans are
  decoded with a generic straight line in log space, and the look comes from a separate
  print-film emulation.

### Why not per-stock inversion as the default

Inverting each stock's own datasheet curve (`characteristic`) was the first goal written
here. It was dropped as the **default** because:

- **It normalizes tone character.** A perfect per-stock inversion returns every stock to
  the same scene contrast, erasing the difference a print shows between Ektar and Portra.
  That is optimizing, not showing.
- **It inverts the toe.** The datasheet toe is nearly flat: slope ≈0.02–0.03 against ≈0.6
  mid-scale, so inverting it amplifies noise and film-base error **20–35×** where the film
  recorded almost no information.
- **It needs per-stock data** that cannot exist for every stock, developer and scanner.
  The fixed decode needs none.

Per-stock inversion still has a place: as an **optional normalization in rendering**
("show this frame as if every stock had the same response"). Because the fixed decode is
invertible, that step can move there without losing anything (see the key argument).

### What reconstruction does not decide

- **Scene illuminant / white balance** (the lab's filtration).
- **Absolute exposure** (the lab's exposure time). Which value "should" be mid-grey
  needs a light-meter reading the negative does not carry.
- **Look.** Contrast, toe and shoulder shaping, paper or print emulation, per-stock
  normalization.

### Known limits

- **Saturated regions carry no information.** Near the base and on the film shoulder the
  negative recorded little; the decode must stay monotonic, must not invent data, and
  must report the problem.
- **A print system is balanced in printing density** (how the paper sees the dyes). That
  is neither the datasheets' Status M nor the scanner's channels. Applying one straight
  line to raw scanner density leaves a per-channel slope error: measured as blue running
  +1.26 stops per unit density before correction. So the decode needs a **scanner →
  printing-density calibration**. That calibration is a measurement and stays in
  reconstruction (see "Calibration" below).

## Key argument: pick reconstruction by what its output means

Any **invertible** reconstruction can be compensated by some rendering to give the same
final image. For example, a straight-line decode followed by a newly designed rendering
transform can reproduce characteristic + reinhard exactly:

- Both reconstructions are strictly increasing per-channel functions of density, so each
  can be converted into the other.
- The converting transform is generally **not a 1D curve**. The NC film RGB → ACEScg 3×3
  matrix sits between the stages, and a per-channel nonlinearity doesn't commute with a
  matrix that mixes channels. So it must undo the matrix first or be a 3D transform.
  White balance and exposure would also have to move after it.
- This holds only while nothing between the stages loses information (no clamping).

Consequence: **the final image cannot justify a choice of reconstruction**, and neither
can information content, since every invertible reconstruction keeps the same
information. Reconstruction is chosen for **what its intermediate image means**, and
rendering for the result. The fixed decode is chosen because it means one thing for
every stock: the negative, as a print sees it.

## The decode's parameters, and which of them are really rendering

```text
D_c   = −log10(scan_c / base_c)            measurement
D′_c  = scale_c · D_c + offset_c           calibration
out_c = 10^(gamma · (D′_c − A))            the curve (A = anchor)
```

**Nothing is lost, by construction.** Every step is strictly increasing, unclamped, in f32,
so no parameter choice destroys information — each is undoable downstream. (Two exceptions,
neither about parameters: a dead-pixel floor on the scan, and non-finite samples passed
through as NaN for the encoder to count.) That is why these settings are conventions and
opinions rather than accuracy questions, and why they cannot be judged by "how much
survived".

Expanding the curve shows what each knob really is:

```text
out_c = 10^(−gamma·A) × 10^(gamma·offset_c) × (10^(D_c))^(gamma · scale_c)
        └─ scalar gain ─┘ └ per-channel gain ┘ └── per-channel exponent ──┘
```

| Knob | Effect | Rendering equivalent |
|---|---|---|
| **anchor `A`** | one gain on all channels | **Exactly exposure** — a scalar commutes with the 3×3. |
| **`offset [3]`** | a per-channel gain, constant at every brightness | **White balance**, in film-layer space. |
| **`scale [3]`** | a per-channel *exponent*: the rate each channel grows with exposure | **None.** No gain, white balance or luminance curve reproduces it. |
| **`gamma`** | overall contrast | None today; rendering has no contrast control yet. |
| `shadow_balance` / `highlight_balance` | per-channel offsets by tone region | A grade. |

So only `scale` (and `gamma`'s look half) do something rendering cannot. Offset and anchor
are duplicates of scene correction, and tuning them here while "holding rendering fixed" is
really tuning the final image and attributing it to reconstruction.

**The anchor behaves differently here than under the sigmoid.** On the straight line the
anchor factors out as a pure gain: all four placement rules give the same shape and differ
only in brightness. Under the knee'd sigmoid the knees sit relative to the anchor against a
fixed 0–1 range, so moving it changes the *shape* — which is why `sigmoid-knees` takes its
brightness from the anchor and refuses `--print-exposure`.

### Decisions

- **One anchor rule: `mid-at-base-offset(d)`**, with `d` the film's mid-above-base density
  (≈0.62; stocks measure 0.54–0.70). Mid-grey is what "exposed correctly" means, it is
  reference-free (no leader, no roll-to-roll error), and pinning mid makes contrast and
  exposure independent: changing `gamma` pivots about mid instead of moving the whole image.
  Pinning black instead would leave midtone brightness depending on contrast, and the base is
  not scene black anyway — it is fog, with real shadows above it.
- **`d` is a calibration, not a brightness knob.** It is a pure gain (at gamma 2.0, 0.1
  density ≈ 0.66 stop, 1 stop ≈ 0.15 density), i.e. the same lever as `--print-exposure`.
  Brightness is set in rendering.
- **`Dmax` leaves the default path.** This rule never reads the reference, so `--d-max`,
  `--auto-d-max` and `estimate --d-max-region` stop mattering for the decode, and the four
  `--anchor-*` flags collapse to one number. The reference stays alive for the `sigmoid-knees`
  comparison only.
- **`offset` stays `[0, 0, 0]`; the base is the same axis.** Multiplying the base by `k`
  equals an offset of `scale · log10(k)`, so the two cannot be fitted independently. Colour
  balance beyond the base belongs to white balance in rendering.
- **`gamma` is two things and splits.** Linearizing the film (≈1/0.55 ≈ 1.8) is calibration
  and stays; print contrast is a look and moves to rendering. Today's single 2.0 bundles
  both — roughly linearization plus ≈1.15× print contrast.

## Methods under this goal

| Method | Role |
|---|---|
| exponential (≡ sigmoid with `toe = shoulder = 0`, bit-exact) | **Default candidate.** A straight line in density against log exposure, with the toe passed through as recorded. |
| `generic-c41` characteristic | **Alternative candidate** for the fixed decode. It is stock-agnostic but inverts an averaged toe. Needs a comparison against the exponential, or a toe-limited form. |
| per-stock `characteristic` | **Leaves reconstruction** and becomes an optional per-stock normalization in rendering. |
| sigmoid with toe/shoulder | **Leaves reconstruction:** a decode with a rendering fused on top. Kept for now as the **visual reference** for the migration (see below). Retired from the product later. |
| `simple` | **Remove.** `1/T` is not a decode of anything a print sees. |

### Calibration: the part that is a measurement

- **Film base** (Dmin) per roll.
- **Scanner → printing density.** Target: a 3×3 + offset (`io/scanner-density-calibration`).
  Today's crude form is the per-channel `density.scale`, re-calibrated on 2026-09-16
  (`pipeline_version` 5) to **`[1, 0.84, 0.73]`** from 31 hand-marked neutral patches over
  five rolls.
- **It depends on development, not only on stock or scanner.** Green splits by date/developer:
  July rolls (CineStill) want ≈0.86–0.90 and September rolls ≈0.77, on the same scanner. One
  shared default cannot fit both, so the calibration should be per roll or per developer, frozen
  into the roll recipe.
- Neutral patches are all bright (flags, cloud, snow), so they fit one density level each.
  That cannot separate a slope from an offset; the bracketed calibration frames
  (`analysis/calibration-frame-capture`) can.

### Why dividing by the base is not enough

Dividing the scan by the film base fixes the **offset**, not the **slope**. It makes
unexposed film neutral (`D = 0` in every channel), which removes the mask's constant part.
What remains is that each channel's density **rises at a different rate** with exposure: an
error that is zero where you normalized and grows with density, so it is worst in the
highlights. Three contributors, in descending confidence: the film's own layer gammas differ
(our tables give blue/red mid-slope ratios ≈1.19 Ektar, ≈1.21 Portra 400 — they look parallel
only because the plot is logarithmic); the scanner's channels each integrate a band
overlapping more than one dye, so what they report is not the dye's own density, and that
cross term depends on the dye set; and development, which splits green by date on one scanner.

**The size of the correction is not a datasheet number.** The sheets say blue runs 12–19 %
steeper, which a scale near 0.86 would null. Our scans need **0.73**, i.e. ≈37 %. The sheet
and the scanner disagree by about a factor of two, so `[1, 0.84, 0.73]` is an empirical fit to
this scanner and these developers and cannot be derived from a publication. Closing that gap
is `io/scanner-density-calibration`.

### What NLP's white-balance step does, and why ours differs

Negative Lab Pro's workflow starts by white-balancing on the film base in Lightroom. That is
**the same operation** as nc's base division, in another domain: a per-channel gain on the
negative is a per-channel offset in density. (Not exactly — Lightroom's white balance runs
through the camera profile and its matrix — but close.) Crucially it also fixes only the
constant and leaves the slope.

NLP's next step, fitting each channel's range onto the output range per frame, **is** a
per-channel slope + offset correction, derived from the frame's own content. That is how it
reaches neutral-looking results without datasheets, and why it collapses on a frame filled by
one surface. nc's roll-consistency principle rules that out, so nc's equivalent is the same
two numbers **measured per roll** and frozen into the recipe. The choice is not "datasheet vs
content"; it is "fitted per frame vs measured per roll".

### Knobs currently in reconstruction, sorted

- **Measurement, keep:** film base; `density.scale` (scanner → printing density);
  `gamma`'s linearization half; the anchor's `d` (mid-above-base).
- **Convention, frozen:** the anchor rule (`mid-at-base-offset`) and `offset = [0, 0, 0]`.
- **Rendering, move out:** sigmoid `toe` / `shoulder`; per-stock curves; `gamma`'s print-
  contrast half; `shadow_balance` / `highlight_balance` (a grade — see Part 2's per-channel
  control, which subsumes them).
- **Tuned by review:** `scale` and `gamma` are the two knobs a visual review can settle;
  `#120` and `#124` were the first two rounds of exactly that.

### Removal constraints

- The default is still the knee'd sigmoid, and `legacy` depends on it, so removal comes
  **after** `algo/split-default-migration`.
- nc is unreleased, so removal is cheap. Follow the precedent of the removed `algorithm`
  key: old recipes get a migration error, with no aliases.

## Reference for the migration

As of `pipeline_version` 5 and the 2026-09-15 brightness target, **`--preset sigmoid-knees`
(scale `[1, 0.84, 0.73]`, anchor mid-fraction 0.28) is the best result reviewed so far.** It
stays available as the reference that the fixed decode plus rendering is judged against,
even though the knee'd sigmoid leaves the product later.

`sigmoid-flat` (the exponential with reinhard, exposure +2.17) improved with the same
calibration. In review it shows more highlight contrast and reads **slightly red** against
`sigmoid-knees`, especially in the highlights. Unexplained so far. The first suspect is that
the knee'd sigmoid's shoulder runs **per channel**, pulling each channel toward white, so it
hides a residual that reinhard (one factor per pixel on luminance) leaves visible.

## Colour: what "character" is, and what the decode must keep

### Datasheet facts

- **The flat left end of a curve is base + fog, i.e. Dmin**: film that got no image
  exposure, matching the unexposed rebate in a scan. nc's tables store density above Dmin.
- **B > G > R density is the orange mask.** Density measures how much light is blocked;
  blocking blue most and red least looks orange. Scan and sheet agree in order: Ektar scan
  base ≈ D `[0.28, 0.59, 0.80]`, sheet Dmin `[0.21, 0.63, 0.84]`.
- **The x-axis position encodes film speed only** (absolute lux-seconds). nc discards it
  and places each stock by its published grey-card aim density.
- **Curves are measured on a neutral exposure under the rated light**, so all three layers
  received the same exposure at every point.

### Kinds of character

| Kind | Example | Where it belongs |
|---|---|---|
| **Neutral balance** | grey prints grey | Not character. Every stock is designed neutral under its rated light, and the lab keeps greys neutral. |
| **Illuminant cast** | tungsten light on daylight film | A scene fact. Kept by the decode; white balance decides. |
| **Colour rendering** | Ektar's saturation and hue shifts | Real character, from spectral sensitivities and dye interactions. Kept automatically: the layers saw the scene through the stock's sensitivities. |
| **Tone character** | Ektar prints punchier than Portra | Real character. Kept by the fixed decode; removed only by the optional per-stock normalization. |

Greys staying grey while other colours change per stock is possible mathematically (any 3×3
whose rows each sum to 1 preserves the neutral axis) and is how colour film is designed.

### Dye layers and the NC film RGB v1 3×3

Colour negative film has three emulsion layers sensitive to blue, green and red light. Each
forms the complementary dye (yellow, magenta, cyan). After decoding, "R" means how much light
the red-sensitive layer received. Those channels are defined by the film's own
sensitivities, not by any standard colour space.

Converting one RGB space into another is a 3×3 linear mix, and it can only be computed from
known primaries. **NC film RGB v1 declares the film's channels to be Rec.709 primaries with
D65 white**, then applies the standard matrix into ACEScg. Nothing measured supports that
declaration. Neutrals are unaffected (white maps to white); saturation and hue of colours
depend on it. A real characterization would come from spectral data or a ColorChecker.

### Correction to an earlier claim

The "Ektar green cast is a drift, not a hue" argument used `curve_probe::channel_drift`.
That probe groups **ordinary picture pixels** by red density and reads the green/red ratio
across the groups; it uses no grey patch. A constant scene colour cancels, but scene colour
that correlates with brightness does not. Hawaii frames (bright blue sky and sea over mid-tone
foliage) are exactly that case, so the result is suggestive, not shown. Two points still
stand: the Ektar sheet predicts +0.22 green drift where scans show +1.26, and `generic-c41`
renders Ektar better than Ektar's own sheet.

## The sigmoid shoulder is a contract violation, not a rendering option

The sigmoid's shoulder runs **in reconstruction** and compresses everything above diffuse
white before either display branch sees it. SDR and HDR therefore receive identical input,
and the default gain map decodes as 1.0x. With the shoulder off, the same frame reaches
4.87x.

CLAUDE.md currently calls this "not a blocker … a rendering-intent option". Under this
goal it is a violation of the stage boundary: reconstruction discarded range that rendering
was entitled to. The cause is historical: the curve was designed when nc had one SDR output
and reconstruct-plus-print was a single step. The fix is the migration already planned
(`algo/split-default-migration`). What changes is the framing, not the plan.

## Handed to rendering

- **Exposure and white balance:** the anchor gain and any per-channel constant, which the
  decode deliberately leaves at its conventions.
- **Contrast:** `gamma`'s print-contrast half.
- **A per-channel grade** for a cast that varies with brightness — the tunable counterpart of
  `scale`, which cannot be tuned per stock in the decode (Part 2).
- **Per-stock normalization** (optional): the per-stock characteristic inversion, applied
  on top of the fixed decode.
- **Print or paper emulation** (look): a fixed paper curve applied forward, as a print does.
- **Reinhard vs sigmoid is not a family choice.** Classic Reinhard `x/(1+x)` is exactly a
  sigmoid: logistic in log exposure, and nc's own shoulder formula with width 1. The
  extended Reinhard nc ships (`v(1 + v/W²)/(1+v)`) has no upper limit, so it is not
  strictly a sigmoid. The characteristic presets use reinhard because it was the **only
  display tone that works** on unbounded input (`none` refuses it, `shoulder` plateaus it).
  A sigmoid display operator was never built, so it was never compared.
- **`shoulder` and `none` exist for bounded reconstructions.** `shoulder` is linear to 0.75,
  cubic to 1.0, and flat above. Once reconstruction is unbounded, both lose their job.

## Open questions

- **Exponential vs `generic-c41`** as the fixed decode, or a toe-limited form (invert only
  where the slope carries information).
- **How to get printing density:** the 3×3 + offset, fitted per roll or per developer, and
  from which frames.
- **Where the NC film RGB v1 matrix belongs:** reconstruction output in film-layer space,
  with the conversion into a working colour space moving to scene correction (a generic
  assumption or a per-stock characterization)? That would change what `film-master` holds.
- **Why `sigmoid-flat` reads red** against `sigmoid-knees`.
- **How to evaluate a reconstruction** under this goal: a bracketed neutral series works for
  any stock (greys are designed neutral), with slope equal to the stock's own contrast;
  colour patches measure consistency, not true colour. Depends on
  `analysis/calibration-frame-capture`.
- **The design-spec revision:** principle 2 in §3, the NC film RGB v1 contract in §4/§6,
  and §7's framing of the curves as alternatives.
- **Task impact:** which existing tasks this re-scopes (`split-default-migration` targets
  `characteristic-generic`; retiring the sigmoid knees and `simple`; the
  `shadow_balance` / `highlight_balance` decision; CLAUDE.md's HDR framing). To be planned,
  not assumed.

# Part 2: Rendering and output

## The stages

Rendering is a chain of named stages, each with one job, followed by two output stages:

```text
reconstruction (exposure estimate)
  → scene correction → look → fit range → fit gamut    rendering
  → encode → package                                   output
```

| Stage | Job | Today |
|---|---|---|
| **scene correction** | Photographic corrections toward what the scene was: white balance, exposure, flare/fog removal. Scene-referred, linear. | `render_split::display_source` (`apply_shared_controls`): WB → exposure → black point → `linear_range` |
| **look** | Creative and optional: contrast, per-channel colour grading, saturation, print or paper emulation, per-stock normalization. Scene-referred. | Doesn't exist as a stage |
| **fit range** | Fit the scene's dynamic range into the display's range, with parameters from the display's peak (SDR vs HDR). The industry term is *tone mapping*: "tone" means brightness levels, not colour. | `print.display_tone` in `pipeline::sdr` / `pipeline::hdr` |
| **fit gamut** | Move out-of-gamut colour to the display's boundary, keeping hue. | `gamut_map` (`neutral-axis-radial-boundary-v1`) |
| **encode** | Transfer function (sRGB, PQ or HLG), quantization, counting clipped samples | `color`, `io::encode`, `io::avif` |
| **package** | Container (TIFF / AVIF / gain-map JPEG), ICC profile or CICP, metadata | `io::*` |

Constraints the order carries:

- **Look comes before the SDR/HDR branch.** A gain map requires the two renditions to
  agree below diffuse white. If contrast or character lived in fit range, whose
  parameters depend on the display's peak, the midtones would disagree. Only fit range
  and later stages may differ per branch.
- **Fit range and fit gamut are separate but coupled.** The gamut ceiling follows the
  luminance fit range produced (`sdr.rs:266`), so they stay adjacent.
- **Encode and package are separate.** `hdr-pq` and `hdr-pq-tiff` write the same encoded
  signal in two containers. The gain map is the one thing spanning the boundary: it needs
  both renditions, then gets built into the package.
- **Print emulation is a look; per-stock normalization is optional.** The fixed decode
  keeps each stock's tone character, as a print does. Paper and print-film curves are
  published datasheets too.
- **Contrast lives here.** The decode keeps only the calibrated linearization of the film;
  print contrast is a look knob (Part 1).

## Decisions

- **Rename to match the stages.** Stage names replace mechanical ones
  (`apply_shared_controls`). The `print.*` recipe prefix is a leftover from the legacy
  print path and is renamed. `print.linear_range` is an affine levels remap, not fit range,
  so it needs a new name and a stage to live in.
- **Fit range uses reinhard.** `shoulder` and `none` are retired: both exist for
  reconstructions already bounded at white, and `shoulder` flattens everything above 1.0.
  Their knob `highlight_compress` goes with them. They're retired after
  `algo/split-default-migration`, since `shoulder` is still the default tone. "Fit range"
  rather than "highlight compression", because reinhard compresses the whole range: it holds
  mid-grey and costs ≈0.86 stop at diffuse white.
- **Retire `legacy` and `custom`.** This removes the second implementation of the print
  controls: `density::render_print` runs them on film RGB before the NC film RGB v1 mapping,
  while the display presets run them on ACEScg.
  - **Features that exist only on legacy move into the new pipeline:** Adobe RGB output is
    a **must have** (today reachable only via an ICC path on legacy; see
    `output/adobe-rgb-gamut`), and a rendered float TIFF (`--out-depth f32`) is good to have.
  - **The drift gate, the golden vectors, the legacy-injected integration tests and the
    benchmark's legacy cases are not preserved across the redesign.** A regression gate
    measuring legacy would only pin the design being replaced. Remove them, and build new
    gates on the new stages once the work is done.

## Two controls the look stage owes

### A per-channel grade with a pivot

Reconstruction's `scale` corrects a cast that grows with brightness, but it acts on the film
layers *before* the 3×3, so changing one channel there moves all three output channels. That
is accurate and unpredictable to tune by hand. The look stage should carry the
photographer-facing counterpart, acting on working-space channels directly:

```text
out_c = mid · (in_c / mid)^k_c        mid = 0.18
```

- **The pivot matters.** Without it a per-channel power moves neutral everywhere; pivoted at
  mid-grey, a neutral mid stays neutral and the cast grows away from mid in both directions —
  which is the thing white balance cannot do.
- **Scene-referred, before the SDR/HDR branch**, or the two renditions disagree in the
  midtones and the gain map breaks. It needs a guard for values at or below zero, which a
  wide-gamut linear space contains.
- **It subsumes `shadow_balance` / `highlight_balance`.** Per-channel adjustment by tone
  region is the same family; the standard forms are ASC-CDL (slope, offset, power per
  channel) and per-channel tone curves. One control, not three.
- **It does not replace the calibration.** It is a grade on top; without a measured per-roll
  `scale` every roll needs hand-grading.

### A "direct" preset for external editing

A render that does as little as possible, for a workflow that continues in Lightroom or
Photoshop: identity scene correction, empty look, Adobe RGB, and only the fit range needed to
land in the container. Two properties it must have, and both come from reconstruction's
conventions rather than from rendering:

- **Mid-grey lands mid** when the frame was exposed correctly — the `mid-at-base-offset`
  anchor.
- **The cast stays within an acceptable range on any stock** — the `scale` calibration.

"Minimal" cannot mean "no tone": Adobe RGB ends at 1.0 and a real decode exceeds it, so a
gentle compression is still a choice, just a fixed and documented one. This is the preset that
makes Adobe RGB a must-have output (Part 2 decisions).

## `film-master` is the reconstruction output

`film-master` is reconstruction → NC film RGB v1 (a 3×3 to ACEScg) → unclamped f32 TIFF with
an ACEScg profile. It runs no rendering stage at all, so it is the artifact on which a
reconstruction is measured. Caveats:

- **It contains whatever reconstruction was configured.** Today's default is the knee'd
  sigmoid, so the intended decode must be named explicitly. `--preset characteristic-generic`
  is refused on `film-master`, because it also sets rendering knobs.
- **The 3×3 treats the dye-layer channels as Rec.709.** Neutrality checks survive, since
  white maps to white. Per-layer slope measurements get slightly mixed. The cleanest
  measurement point is `FilmRgbImage`, before the matrix, which nc can't export today.
- **Numbers can be read from it directly. Visual review can't:** it's linear, exceeds 1.0
  and has no white balance. Comparing reconstructions by eye needs one fixed reference
  rendering held constant across them.

## Open questions

- **A parametric fit-range operator.** Classic Reinhard is one member of the sigmoid family.
  An operator with contrast, shoulder and display-peak parameters could hold both mid-grey
  and diffuse white, which reinhard can't. Add it only if it beats reinhard at matched
  lightness.
- **How contrast and the per-channel grade are spelled** — one CDL-style object, or separate
  knobs — and whether the "direct" preset is a named output preset or a rendering profile.
- **The black point is two jobs:** a small flare/fog subtraction (scene correction) and
  display black / toe (fit range). Today it's one linear subtraction, and 0.019 crushed
  0.69–8.66% of frames to code 0.
- **Other legacy-only outputs** (ProPhoto, arbitrary ICC paths): keep or drop, undecided.
- **Where per-stock normalization lives** (scene correction or look). Part 1 settled that it
  is optional and in rendering, not which stage.

# Part 3: Evaluating and tuning the decode

Because the decode is invertible, "how much information survived" cannot grade it, and the
final image cannot either — rendering can compensate any of it. What is left to judge is the
two knobs rendering **cannot** reproduce: `scale` (a cast that grows with brightness) and
`gamma` (contrast). Everything else — exposure, white balance, the anchor — is a convention
here and a control there.

## What is measurable, and what is opinion

| Question | How it settles |
|---|---|
| `scale` vs `offset` | **Measurable.** A neutral surface must give `D′_r = D′_g = D′_b`. Across several densities, the residual's **slope** is `scale` and its **level** is the offset (equivalently the base). |
| `gamma`'s linearization half | **Measurable, but only with a bracket:** if exposure doubles, the reconstructed value must double. That measures the film's own slope. |
| `gamma`'s print-contrast half | **Opinion.** No neutral reference constrains it, and it belongs to rendering anyway. |
| Whether a per-channel gain suffices | **Measurable:** whatever is left after the best `scale` is the evidence for how much of a 3×3 is needed. |
| Model stability | **Measurable without any reference:** fit `scale` per frame; the better model is the one whose fitted value has the smallest spread across a roll, and across rolls sharing a developer. |

## The limits of the data we have

- **The bracketed grey card is postponed, deliberately.** It needs a card bought, frames shot
  on several stocks, developed and scanned. `analysis/calibration-frame-capture` holds the
  protocol and is a **release gate, not a blocker**: work continues on visual review until the
  frames exist.
- **The 31 marked patches are all bright, and only approximately neutral.** In ordinary
  photographs the only findable neutral is white, and the eye cannot tell a slightly tinted
  grey from a pure one. Cloud and snow carry the sky's blue, and sunlight and shade shift them
  further. So the patches can anchor a level, not a slope.

## The interim method: compare against other converters

Four producers on the same frames: **NLP**, **SilverFast with CCR**, **SilverFast without
CCR**, and nc's tuned `sigmoid-knees` as the in-house reference. It is not ground truth, but
it makes nc comparable to its competitors, and three independent converters disagreeing with
nc *in the same direction* is evidence where one disagreeing is not.

- **The SilverFast pair is the most valuable part.** CCR off is a fixed decode with a stock
  profile — nc's philosophy. CCR on is per-frame cast removal. **Their difference is an
  independent per-frame measurement of the cast**, on frames with no grey card. Whether nc's
  residual tracks it, frame by frame, is testable today.
- **Do not chase adaptation.** NLP and CCR-on both neutralize per frame, which nc rejects by
  principle (and which is why NLP collapses on a frame filled by one surface). The useful
  reading: where the references agree with each other, treat it as evidence about the scene;
  where they differ from nc **the same way on every frame**, that is a fixed error in the
  decode, i.e. `scale` / `offset`; where they differ **per frame in different directions**,
  that is their adaptation. A table across many frames separates those; the eye on one frame
  cannot.
- **A consensus reference is cheap.** The references are images, so `nctool metrics` reads
  them. Measured on the marked patches, agreement between the three is a usable interim
  neutral — grey-world-biased, but better than assuming cloud is white — and the patches sit
  at different densities across frames, which is what the slope/level split needs.

### What to hold fixed, or the comparison means nothing

- Match brightness before judging colour (the +1 stop review showed preferences move with it).
- One nc rendering config across every nc variant in the set.
- Same output space, same viewer.
- **Judge colour more than tone.** NLP and SilverFast bake their own looks, so a contrast
  comparison mostly compares looks, while a cast comparison transfers.

### A test available now

Pick frames where the same near-neutral surface appears in **both shadow and highlight** (a
white flag in sun and in shade, lit and shadowed snow) and measure the channel residual at
both densities. Flat across brightness → an offset or white-balance issue, and `scale` is the
wrong knob. Growing with density → `scale` is right, and its slope is the value. Weaker than a
bracket, but it needs no new shoot, and it is the most direct lead on why `sigmoid-flat` reads
red against `sigmoid-knees`.

## Tuning order

`scale` first, then `gamma`: the cast is the open question, and contrast is easier to judge
once the cast is settled. Two or three candidates per review set keeps a frame's toggle
manageable. `#120` and `#124` were the first two rounds of this loop.

# Part 4: Evaluating and tuning the rendering — not yet planned

Deliberately empty. The same questions (what is measurable, what is opinion, what to compare
against) apply to the rendering stages, but they cannot be answered before the decode is
settled: every rendering judgement made on top of a moving decode has to be redone. Recorded
here so the gap is visible rather than forgotten.

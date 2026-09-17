# Design update

**Status:** agreed direction from a design discussion on 2026-09-16. **Not yet applied**
to `design-spec.md`, `TASKS.md`, any task file or CLAUDE.md. Where this document
contradicts them, it describes the intended design and they describe the current one.
Folding it in (spec revision, task changes) is follow-up work and has not been planned.

Parts are appended as each discussion settles. **Part 1** covers reconstruction and
**Part 2** rendering and output.

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

> **Reconstruction produces the best available estimate of relative per-channel
> exposure at the film plane. It loses nothing it can avoid losing, and reports what it
> could not recover. Every choice about how the picture should look belongs to
> rendering.**

"Per-channel" means per dye layer: the three layers' sensitivities are not a
colorimetric observer, so mapping them into a standard colour space is itself an
estimate with a residual.

### What reconstruction cannot know, so rendering owns it

- **Scene illuminant / white balance.** The film recorded the light as it was; deciding
  what should look neutral is a choice.
- **Absolute exposure.** Which value "should" be mid-grey needs a light-meter reading
  the negative does not carry. `characteristic` anchors itself by assuming exposure at
  box speed. That is a convention, not a measurement.
- **Look.** Contrast, toe, shoulder, print or paper emulation, "normalized" versus
  "stock-favoured".

### Known limits of the estimate

- **Saturated regions carry no information.** Near the base (film toe) and on the film
  shoulder, inverting the curve amplifies noise and cannot recover detail.
  Reconstruction must stay monotonic, must not invent data, and must report the problem.
  `characteristic` already extrapolates past its table and reports that instead of
  clamping.
- **Datasheet curves come from a densitometer (Status M); the scanner is not one.**
  Green measured at scale 0.900 against the sheet's 0.977. That scanner error term is
  `io/scanner-density-calibration`'s job.
- **An unknown stock needs a generic or fitted curve**, which is a less accurate estimate
  of the same quantity.

## Key argument: pick reconstruction by what its output means

Any **invertible** reconstruction can be compensated by some rendering to give the same
final image. For example, flat (exponential) reconstruction followed by a newly designed
rendering transform can reproduce characteristic + reinhard exactly:

- Both reconstructions are strictly increasing per-channel functions of density, so each
  can be converted into the other.
- The converting transform is generally **not a 1D curve**. The NC film RGB → ACEScg 3×3
  matrix sits between the stages, and a per-channel nonlinearity doesn't commute with a
  matrix that mixes channels. So it must undo the matrix first or be a 3D transform.
  White balance and exposure would also have to move after it.
- This holds only while nothing between the stages loses information (no clamping).

Consequence: **the final image cannot justify a choice of reconstruction**, and neither
can information content, since every invertible reconstruction keeps the same
information. Reconstruction is chosen for **what its intermediate image means**:
something measurable against a physical target. Rendering is chosen for the result.

## Reconstruction methods under this goal

Methods are **estimators of one quantity**, ranked by accuracy, not alternatives offered
as taste. Several may exist while evidence is missing. The end state is one default,
plus fallbacks used only when data is missing.

| Method | Estimates exposure? | Fate |
|---|---|---|
| `characteristic` (per-stock or generic curve) | **Yes, the best.** Inverts each layer's published curve, the same shape as ACES's ADX → ACES transform. | The reconstruction. |
| exponential (≡ sigmoid with `toe = shoulder = 0`, bit-exact) | **Yes, cruder.** Assumes the film response is a straight line (in density vs. log exposure; in light it's a power law of transmission). | Last-resort fallback. |
| sigmoid with toe/shoulder | **No.** An estimator with a rendering fused on top. | Remove from reconstruction. |
| `simple` | **No.** `1/T` is exposure raised to the film's gamma. | Remove. |

### Fallback order

1. A curve fitted to **this roll** from calibration frames (future; captures the actual
   development).
2. The stock's published curve.
3. `generic-c41`: the average of the digitized stocks, tested to sit inside their spread.
4. Exponential: only for material that isn't C-41-shaped (pushed or expired film,
   cross-processing, possibly B&W), and even there a fitted curve should beat it.

Why `generic-c41` ranks above the exponential: every measured C-41 stock has a toe and a
per-channel slope structure. The straight-line model is known to be wrong in the toe for
all of them. Its per-channel scale only removes the corpus's *average* slope drift;
per-roll residuals still span ±0.5 stop per unit density. **How much** worse it is on real
exposure is not measured yet; that needs calibration frames.

### Knobs currently in reconstruction, sorted

- **Measurement, keep:** film base, `density.scale` (a scanner calibration), the stock's
  curve.
- **Rendering, move out:** sigmoid `toe` / `shoulder`; `contrast` wherever it departs from
  the film's actual gamma.
- **Ambiguous, decide later:** `shadow_balance` / `highlight_balance`. A correction for
  crossover between dye layers belongs in reconstruction; a grade belongs in rendering.
  Today they are used as both.
- **Anchor placement** on the parametric curves is an exposure convention; `characteristic`
  needs none.

### Removal constraints

- The default is still the knee'd sigmoid, and the `legacy` path depends on it, so removal
  comes **after** `algo/split-default-migration`, not before.
- nc is unreleased, so removal is cheap. Follow the precedent of the removed `algorithm`
  key: old recipes get a migration error, with no aliases.

## The sigmoid shoulder is a contract violation, not a rendering option

The sigmoid's shoulder runs **in reconstruction** and compresses everything above diffuse
white before either display branch sees it. SDR and HDR therefore receive identical input,
and the default gain map decodes as 1.0x. With the shoulder off, the same frame reaches
4.87x.

CLAUDE.md currently calls this "not a blocker … a rendering-intent option". Under this
goal it is a violation of the stage boundary: reconstruction discarded range that rendering
was entitled to. The cause is historical. The curve was designed when nc had one SDR output
and reconstruct-plus-print was a single step, so landing white at white was the goal. The
fix is the migration already planned (`algo/split-default-migration`). What changes is the
framing, not the plan.

## Handed to rendering

- **Stock-favoured rendering** re-applies a film or print curve forward on top of the
  exposure estimate. This loses nothing, because the estimate is clean.
- **Reinhard vs sigmoid is not a family choice.** Classic Reinhard `x/(1+x)` is exactly a
  sigmoid: logistic in log exposure, and nc's own shoulder formula with width 1. The
  extended Reinhard nc ships (`v(1 + v/W²)/(1+v)`) has no upper limit, so it is not
  strictly a sigmoid. The three characteristic presets use reinhard because it was the
  **only display tone that works** on unbounded input (`none` refuses it, `shoulder`
  plateaus it). A sigmoid display operator was never built, so it was never compared.
- **`shoulder` and `none` exist for bounded reconstructions.** `shoulder` is linear to 0.75,
  cubic to 1.0, and flat above. Once reconstruction is characteristic, both lose their job.

## Open questions

- **How to evaluate a reconstruction** against this goal: bracketed neutral series,
  ColorChecker, cross-roll consistency, and the rule "a defect a global rendering control
  can fix is not a reconstruction defect". Discussed briefly and parked. It depends on
  `analysis/calibration-frame-capture`.
- **The design-spec revision:** principle 2 in §3, the NC film RGB v1 contract in §4/§6,
  and §7's framing of the curves as alternatives.
- **Task impact:** which existing tasks this re-scopes (retiring the sigmoid knees and
  `simple`, the `shadow_balance` / `highlight_balance` decision, CLAUDE.md's HDR framing).
  To be planned, not assumed.

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
| **look** | Creative and optional: contrast, saturation, print or paper emulation, stock-favoured rendering. Scene-referred. | Doesn't exist as a stage |
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
- **Stock-favoured rendering is a look.** It re-applies a film or print curve forward on
  the exposure estimate. Paper and print-film curves are published datasheets too.

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

## `film-master` is the reconstruction output

`film-master` is reconstruction → NC film RGB v1 (a 3×3 to ACEScg) → unclamped f32 TIFF with
an ACEScg profile. It runs no rendering stage at all, so it is the artifact on which a
reconstruction is measured. Caveats:

- **It contains whatever reconstruction was configured.** Today's default is the knee'd
  sigmoid, so characteristic must be named explicitly. `--preset characteristic-generic`
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
- **The black point is two jobs:** a small flare/fog subtraction (scene correction) and
  display black / toe (fit range). Today it's one linear subtraction, and 0.019 crushed
  0.69–8.66% of frames to code 0.
- **Other legacy-only outputs** (ProPhoto, arbitrary ICC paths): keep or drop, undecided.
- **Where "keep vs normalize the stock's colour" lives** (scene correction or look). Tied to
  the reconstruction-evaluation discussion that follows Part 2.

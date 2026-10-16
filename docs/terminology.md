# Hanten terminology

The one definition of every value term Hanten uses: what each number means, which
space it lives in, and which word names it. Design-spec §4 keeps a summary and links
here; the `value-terms` skill points here too.

Every term is tagged **public** (a user sees it: a flag, recipe key, report field,
stage id, help text or message) or **internal** (a Rust identifier, a constant or a
doc comment). Flags, keys, fields and stage ids are the scripting contract: one that
breaks these rules is listed as a [known misnomer](#known-public-misnomers) and kept
until a migration task renames it. Help and message *wording* follows this doc
directly, with its tests and `docs/using-nc.md` in the same change.

## Rules

- **One word per meaning.** Where a word has several meanings below, write it with
  its qualifier — "tone slope", not "slope". A bare word is allowed only inside the
  module that owns the meaning.
- **Name the space.** Every number belongs to one [value space](#value-spaces) or
  [kind of stops](#stops-four-kinds). "High", "bright" and "1.0" are ambiguous until
  the space is named.
- **The multiplier family.** Each word names one kind of operation:
  *gain* multiplies a linear value; *scale* and *offset* act on density; *slope* is a
  power (a multiplier on stops); *contrast* is the user's knob on the tone slope;
  *gamma* is a transfer exponent or film gamma; *exposure* is a change in EV.
- **Colour words name their space.** "Neutral", "luminance", "hue" and "chroma" depend
  on it: R=G=B in film RGB is not R=G=B in ACEScg or in the destination primaries.
- **Spelling.** Prose and messages write "colour" and "grey". Identifiers keep
  whatever they already spell (`input_color`, `pipeline::color`, `gray-world`).
- **Retired words.** `Dmax` (a reference density) is retired: nothing measures, reads
  or reports it. Don't reuse the word.

## Value spaces

**Read this before using "high", "low", "bright" or "dark".** A pixel passes through
several spaces between scan and output, and one runs **backwards**. Everything is
**per channel** except the anchor `A` (a scalar). The IR plane is a separate
channel, read only by holder detection and dropped at the decode.

| Space | Meaning | "Higher" means | Range (`f32`) | Kind | Where |
|---|---|---|---|---|---|
| **transmission** | the raw scan value, `u16 / 65535`: the fraction of light the film passes | thinner negative — a **darker** scene | `[0, 1]` | internal | `io::decode`, `Scan` |
| **film base** (alias `Dmin`) | the unexposed film's per-channel transmission; the divisor of density | (the ceiling of transmission) | `(0, 1]` | public: `--film-base`, `calibration.film_base` | `FilmBase`, `pipeline::film_base` |
| **density `D`** | `−log10(scan / film base)` — 0 at the base. Say "density above base" when the reference matters | denser negative — a **brighter** scene | `≤ log10(65535 · film base)`, at most ≈4.8 (the scan is floored at 1/65535); slightly `< 0` where a pixel out-transmits the base | public: `density_above_base`, `spread_density` | `algo::fixed::decode`, measurements |
| **corrected density `D′`** | `density scale · D + density offset`. At the base `D′` is the offset, not 0 | brighter scene | `D`, scaled and offset per channel | internal | inside `algo::fixed::decode`, never a buffer |
| **film RGB** (NC film RGB v1) | the decode's linear positive, one channel per dye layer. The anchor lands at 1.0, mid-grey at 0.18, diffuse white at ≈0.80 at the default decode | brighter scene | unclamped | public: `working_mapping: nc-film-rgb-v1`, `--export-film-rgb` | `algo::FilmRgbImage` |
| **scene-linear ACEScg** | film RGB through the v1 mapping, before the look. Keeps film, lens, development and scanner character; not a physical scene recovery | brighter | unclamped | public: the `--film-master` output | `pipeline::working_space` |
| **graded ACEScg** | after the look. Diffuse white is 1.0 here | brighter | unclamped | internal | look → fit range |
| **display-linear** | after fit range and fit gamut, in the destination's gamut. 1.0 is reference white (203 cd/m² in an HDR destination) | brighter | `≥ 0`; content past the white point exceeds the peak and is clamped to it and counted (SDR at the integer encode, HDR at `hdr::clamp_to_peak`) | public: `--transfer linear`, `reference_white_sample` | `pipeline::fit_gamut::DisplayReferredImage` |
| **output sample** | the written code value | brighter | destination-defined | public: `loss.clipped_*` | `io::encode`, `io::iso_gain_map` |

**The one rule.** As **scene luminance rises**: `transmission ↓ · density ↑ ·
positive ↑ · output ↑`. Transmission is the only axis that falls.

**"Bright", "dark", "highlight" and "shadow" mean the scene's luminance**, never a raw
value. A scene highlight is the densest negative and the lowest transmission. In a
mixed context never call a high-transmission value "bright" — say
"high-transmission". (A module working purely in transmission may adopt a local
"bright = high transmission" convention, stated with a link here.)

**The film-base paradox.** The unexposed base is at once the highest transmission,
zero density, and near-black in the positive. *Brightest in the scan = darkest in the
positive.*

**The film base** is the per-channel *relative* maximum transmission: no genuine
picture pixel out-transmits it (dust, specular glints, hot pixels and noise can,
which is why `D` dips below 0). It is not near 1: the orange mask and scanner gain
pull channels down (Ektar ≈ `[0.53, 0.26, 0.16]`). It sets the black level and its
colour. `Dmin` is kept as an alias only — it is named for a density but holds a
transmission — so user-facing text leads with "film base".

**Glossary.** *rebate* — the unexposed strip between holder and picture.
*holder* — the opaque scanner carrier (transmission `< 0.05`). *effective area* — a
frame minus its measured holder and a static inset; `measure-base` reads an
unexposed frame's film base over it. *base region* — a user rectangle sampled for
the film base. *scene white / scene black* — the brightest / darkest depicted
luminance.

## Stops: four kinds

A stops figure states its reference and its sign.

| Kind | Meaning | Sign | Kind | Public spellings |
|---|---|---|---|---|
| **scene stops** | `log2(value / 0.18)` on the decode's output; the linearization is already in. The statistic varies: the roll white is in film RGB (see the whites below), a dark end or level is ACEScg luminance | dark is negative | public | `roll.white_stops`, `roll.dark_stops`, `envelope_stops` (`[dark end, white]`), `level_stops` |
| **look stops** | scene stops × the tone slope | dark is negative | public | `roll.hybrid_span_stops`, `placement.frame_white_look_stops` |
| **display stops below mid-grey** | distance below mid-grey after fit range | **positive is darker** — the opposite of `roll.dark_stops` | public | `fit_range.dark_end`, `film_base_below_mid_stops`, `frame_dark_below_mid_stops` |
| **stops from diffuse white** | relative to graded 1.0 | signed | public | `fit_range.highlight_range_stops` (+6), `highlight_desaturation.start_stops` (−1) |

**EV** is an exposure change: `log2` of a neutral gain (`--exposure`, `lift_ev`).

**Density to stops** needs its factor stated. In transmission, 1 density is
`1 / log10 2 ≈ 3.32` stops. In the decode's output, 1 `D′` is
`linearization / log10 2 ≈ 5.98` scene stops at the default linearization 1.8.

## Operations: the multiplier family

### Gain, scale, offset

| Term | Meaning | Stage · space | Kind | Public spellings |
|---|---|---|---|---|
| **white-balance gains** | per-channel multipliers: the stated ones and the roll's measured, green-anchored ones | scene correction · ACEScg | public | `--white-balance`, `--roll-white-balance`, `white_balance.gains` |
| **exposure gain** | `2^EV`, a neutral gain | scene correction · ACEScg | internal | — |
| **gain map** | the ISO 21496-1 image of `(HDR + o) ÷ (SDR + o)`, with a stated offset `o`. Never a bare "gain" | gain-map destination | public | `chain.gain_map.*` |
| **reinhard input gain** | the pre-gain that holds mid-grey in place | fit range | internal | — |
| **density scale** | per-channel multiplier on `D`; the bundled one has red = 1. Not "density gain" | decode · `D` | public | `--density-scale`, `reconstruction.scale`, `chain.decode.scale` |
| **density offset** | per-channel additive term on `D` | decode · `D` | public | `--density-offset`, `reconstruction.offset` |
| **midtone-line intercept** | the constant in `log2(c/g) = slope·s + intercept` | scene correction | public (positional in `--roll-midtone-line`) | — |

### Slope — always qualified

A slope is a power on linear values: a multiplier on stops or decades.

| Term | Meaning | Stage | Kind | Public spellings |
|---|---|---|---|---|
| **linearization** | the decode's straight-line slope in `10^(lin·(D′ − A))`; it undoes film gamma. Default 1.8 | decode | public | `--linearization`, `reconstruction.linearization`, `chain.decode.linearization` |
| **tone slope** | the look's luminance power about mid-grey: base slope × contrast | look | public | `chain.look.slope`; look stage `tone-slope` |
| **base slope** | the tone slope before the contrast knob; from roll, fallback, direct, thin or placement | look | public | `chain.look.base_slope`, `base_from` |
| **roll slope** | the roll's span (dark end to white, scene stops) mapped onto 9.049 look stops, clamped to ≈1.237–3.5 | measure-roll | public | `chain.roll.slope` |
| **fallback slope** | the base slope when no roll is measured (≈1.414); `direct` pins it | look | public | `base_from: fallback` |
| **thin slope, placement slope** | per-frame replacements of the base slope | roll | public | `roll.thin_slope`, `frames[].hybrid.slope` |
| **saturation slope** | the power on colour ratios: the roll's or fallback slope (never a frame's own) × rendering saturation × the saturation knob | look | public | `chain.look.saturation_slope`; look stage `saturation-slope` |
| **whole slope** | linearization × tone slope (≈2.54) — what the retired single "gamma" meant | validation | public (usage messages) | — |
| **colour slope** | linearization × saturation slope, used by highlight desaturation | look | internal | — |
| **midtone-line slope** | the cast's change per scene stop, as a log2 ratio | scene correction | public (positional) | — |
| **local slope** | the log-log slope of the fit-range curve at a point | fit range | public (the shadow-contrast warning) | — |
| **film gamma** | a stock's `dD / dlog10 E`, ≈0.53–0.61. Sensitometry only; never a bare "slope" | — | internal | — |

### Contrast, gamma, exposure, lift

| Term | Meaning | Kind | Public spellings |
|---|---|---|---|
| **contrast** | **only** the user's knob: a multiplier on the base slope, default 1. Never the slope itself, never linearization | public | `--contrast`, `look.contrast` |
| **gamma** | **only** a transfer exponent (the input `Gamma` tag, the HLG system gamma, the gain-map gamma) or film gamma | public | `hlg_system_gamma` |
| **exposure** | a change in EV in scene correction, from three sources — stated, roll and per-frame; the report gives their sum. In sensitometry write "log exposure" | public | `--exposure`, `--roll-exposure`, `roll.frame_exposure`, `chain.scene_correction.exposure` |
| **lift** | **only** a per-frame roll correction: small lift (EV) and thin lift (slope + EV). Fit range's dark floor *raises* the dark end; the operator id `reinhard-peak-lifted-v1` is an identifier | public | `--small-lift`, `--thin-lift`, `lift_ev` |

## Reference points

| Term | Meaning | Space | Kind | Public spellings |
|---|---|---|---|---|
| **mid-grey** | 0.18: the look's pivot and the origin of scene stops | film RGB, ACEScg | internal | — |
| **mid-grey density above base `d`** | the corrected density where mid-grey lands, default 0.62: its height above the film base while the density offset is 0 (the default). The anchor rule's input. A convention, not a per-stock value | `D′` | public | `--anchor-mid-offset`, `reconstruction.anchor_rule` |
| **anchor `A`** | the corrected density that renders to film RGB 1.0, derived as `d + log10(1/0.18) / linearization` ≈ 1.034. A scalar (a per-channel anchor would be a white balance). Reported, never an input; not scene white | `D′` | public | `chain.decode.anchor` |
| **diffuse white** | graded ACEScg 1.0, a convention. The datasheets' diffuse white (`d + 0.36`) decodes to ≈0.80 in film RGB at the default decode; where it grades depends on the tone slope | graded | public | `--highlight-desaturation-start` help |
| **reference white** | display-linear 1.0; 203 cd/m² in an HDR destination (an SDR file states no absolute level). Diffuse white renders below it (≈0.55 at the default highlight range) | display | public | `reference_white_nits`, `reference_white_sample` |
| **highlight range** | the stops above diffuse white that fit range compresses into the display: a knob, default 6. Not "headroom" in prose | stops from diffuse white | public | `--highlight-range`, `fit_range.highlight_range_stops` |
| **white point** | reinhard's `W = 2^highlight range` (64): the graded value that lands about at the display peak (reference white for SDR) | graded | public | `chain.fit_range.white_point` |
| **HDR headroom** | the destination's peak over reference white, **in stops** (ISO 21496-1's term): 0 for SDR, `log2(1000/203)` ≈ 2.30 at 1000 nits. A bare "headroom" means this | display | public | — |
| **display peak** | the same, as a ratio: 1.0 for SDR, ≈4.93 for HDR. *Peak nits* is the absolute form | display | public | `chain.fit_range.display_peak`, `target_peak_nits`, `hdr_linear_tiff.peak_sample` |
| **dark end** | where fit range places the frame's darkest 1 % (target or floor) or the film base (the dark base), in display stops below mid-grey | display | public | `--dark-target`, `--dark-floor`, `--dark-base` |
| **dark base** | the film base's placement on the display: one setting of the dark end. Sets no black level | display stops below mid-grey | public | `--dark-base`, `fit_range.dark_end` `{"base": STOPS}` |
| **roll white, tilt white, fade end** | three whites measured from a roll: the brightest channel's p97 in film RGB; green's p99 in ACEScg; the luminance of the per-channel p99s after balance. Never a bare "the roll's white" | scene stops | public | `roll.white_stops`, `roll.white_balance_stops`, `midtone_line.fade_end_stops` |
| **content white** | a roll's bright end in picture density. Not measured and not shipped; the roll white replaces it | — | internal | — |

**Not interchangeable.** The film base, the anchor, diffuse white, the roll white
and the retired leader `Dmax` are five different numbers from five different places;
only the film base is an input to the decode. The anchor rule never reads a
reference density, which keeps a leader measurement's roll-to-roll error (0.295
density between two rolls of one stock) out of the render.

## Colour

### Spaces, primaries and whites

| Term | Meaning | Kind |
|---|---|---|
| **film RGB** | one channel per dye layer, declared Rec.709/D65 by convention, not by measurement. Not "working-space channels" | public |
| **NC film RGB v1 mapping** | the pinned 3×3 from film RGB to ACEScg; a versioned identifier. Say "the v1 mapping" for the matrix, "film RGB" for the data | public |
| **working space** | linear ACEScg: AP1 primaries at the ACES white, which is about D60 but is not CIE D60 — write "~D60" | public |
| **destination gamut** | the output's primaries and D65 white: one of four. "Destination" means all four axes | public: `--gamut` |
| **white chromaticity** | a white point as an xy coordinate: D65, ACES white, D50 (the ICC PCS). The public "white point" is reinhard's `W` | internal |
| **chromatic adaptation** | Bradford: D65 ↔ ACES white (in the v1 mapping), and D65 → the ICC PCS white (Little CMS's, except in the authored PQ / HLG profiles, which carry a pinned one) | internal |

### Luminance — one word, three spaces

"Luminance" is the one word for a linear Y and always names its space. "Luma" is a
gamma-encoded sum, which Hanten never computes. The vectors are the `*_LUMINANCE`
constants in `pipeline::colorimetry::pinned`.

| Term | Meaning | Used by | Kind |
|---|---|---|---|
| **ACEScg luminance** (AP1 Y) | `dot(rgb, ACESCG_LUMINANCE)` | look, fit range, scene stops, roll measures, midtone line | public |
| **destination luminance** | the Y row of the destination gamut | fit gamut's ceiling and gamut map, the HLG encode | public |
| **display luminance** | absolute, in cd/m²: reference white 203, peak 1000 | HDR encode | public: `reference_white_nits`, `target_peak_nits` |
| **content light level** | MaxCLL / MaxFALL: per-pixel `max(R,G,B)`, not a luminance | HDR report | public: `max_cll_nits`, `max_fall_nits` |

Fit range compresses ACEScg luminance; fit gamut works in destination luminance.

### Corrections — removing what film, development and scanner added

| Term | Meaning | Kind | Public spellings |
|---|---|---|---|
| **roll white balance** | green-anchored gains `[g/r, 1, g/b]` equalising the pooled per-channel p99, as a flat *gain* or a *tilt* ramping from the film base to the tilt white. The gains move ACEScg luminance too | public | `--roll-white-balance`, `--white-balance-form` |
| **stated white balance** | the user's gains, multiplied with the roll's | public | `--white-balance` |
| **midtone line** | the development cast as a function of scene stop, `log2(c/g) = slope·s + intercept`, corrected on R and B with ACEScg luminance restored, limited to near-neutrals by the tint gate. "Midtone neutral" names the switch | public | `--roll-midtone-line`, `--midtone-neutral` |
| **roll colour correction** | the roll white balance and the midtone line together | public | `--colour-correction`, `roll.colour_correction` (its switch) |
| **cast** | an unwanted colour shift, always qualified: *roll cast*, *development cast*, *residual cast*. An *illuminant cast* is a scene fact and is kept | internal | — |

### Preferences — the look's colour

| Term | Meaning | Kind | Public spellings |
|---|---|---|---|
| **saturation** (the knob) | the user's multiplier on the saturation slope, default 1 | public | `--saturation`, `look.saturation` |
| **rendering saturation** | the 1.15 that `default` applies over the base slope (`direct` applies 1) | public | help text |
| **saturation slope** | the power on colour ratios, `(x/Y)^s`, ACEScg luminance kept | public | `chain.look.saturation_slope` |
| **channel grade** | a per-channel power about 0.18 on R and B, luminance restored: a tone-dependent tint | public | `--channel-grade` |
| **highlight desaturation** | a pull toward ACEScg luminance approaching diffuse white, gated by the **chroma band** on `log10(max/min)` of graded ACEScg over the colour slope — not the negative's density | public | `--highlight-desaturation{,-start,-band}` |

### Fitting into the destination

| Term | Meaning | Kind |
|---|---|---|
| **change of primaries** | the pinned 3×3 from ACEScg to the destination gamut | public (stage id) |
| **gamut map** | moves out-of-gamut colour radially toward the neutral axis at constant destination luminance, into the cube. The hue it keeps is a **direction in linear destination RGB**, not a perceptual hue | public (stage id) |
| **HLG boundary map** | the HLG encode applies the gamut map a second time, in scene-linear BT.2020 after the inverse OOTF, where the OOTF's inverse can leave a sample outside the cube (design-spec §5) | internal |
| **rendering intent** | the ICC intent: relative colorimetric | internal |

### Overloaded colour words

- **neutral** — R=G=B in a named space. Not identity values, not "neutral gain".
- **saturation** — colourfulness. Density near the film's maximum is **film
  saturation**, always with "film".
- **balance** — "white balance" means the roll and stated gains only. The film base
  sets the *base colour*.
- **hue** — there is no perceptual hue anywhere: write "linear-RGB hue direction".
- **chroma** — name the measure: the midtone log2 ratio to green, the highlight
  chroma band, or fit gamut's `rgb − Y`.
- **mask, intent, colorimetric** — always qualified: the orange mask vs the holder
  mask; the ICC intent; the `colorimetric` input meaning.

## Other overloaded words

| Word | Meanings | Reading |
|---|---|---|
| **tilt** | the white-balance form `tilt` (a tone-dependent gain). A spatial density change across an area is a **gradient** (`area_uniformity.gradient`) | "tilt" is the WB form only |
| **spread** | region spread `(p97−p10)/p97`; area spread `(p90−p10)/p50` (a coarse "is this a picture?" guard, not a uniformity verdict); `spread_stops` (p95−p5 luminance); `spread_density` | always qualified |
| **grade** | `channel_grade`; the graded image | a confidence level is a **tier** |
| **range** | the `--range` axis; the fit range stage; CICP full range | "dynamic range" or "fit range" in prose |
| **linear** | a linear transfer | encoding only; "linearization" is the decode slope |
| **reference** | reference white; reference frames; the reference build | always qualified |
| **anchor** | `A` only | "green-anchored" etc. become "based on" or "relative to" in new prose |

## Known public misnomers

None is open. A public name that breaks these rules is listed here with the task that
will migrate it, since each changes the scripting contract; until then prose names the
right concept and quotes the public name once. Internal identifiers that contradict this
doc are `core/internal-term-renames`.

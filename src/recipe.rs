//! The recipe (`nf-core/recipe-schema`).
//!
//! One document, versioned **as a document**: a recipe states `"recipe_version": 2` at
//! top level, and that marker is what makes it self-describing. A document without it
//! was written for the chain `nf-core/default-flip` removed (every sidecar and
//! `--dump-params` file before `pipeline_version` 8), and is refused whole; one with it
//! that still carries the removed chain's keys is refused by name ([`check_body`]).
//! That closes the gap `deny_unknown_fields` cannot: it rejects an *unknown* key, and is
//! blind to a **known but meaningless** one, such as a whole `print` section loaded
//! into a chain that has no print stage.
//!
//! The sections follow the chain, one per stage, and an identity stage still has one:
//!
//! ```text
//! input · calibration · measure      the decode's input and the film base
//! roll                               what `hanten measure-roll` measured for the roll
//! reconstruction                     the fixed decode — algo::fixed::DecodeParams
//! rendering                          which base the stages start from (crate::rendering)
//! scene_correction · look ·          one per rendering stage with its knobs; fit
//! fit_range · fit_gamut                gamut has none (its ceiling and target are not
//!                                      the recipe's), so its section stays empty
//! ```
//!
//! **No per-section `schema_version`.** The removed chain's tagged `reconstruction`
//! carried one because it once had to tell several shapes apart inside a single
//! object; here the document version does that for every section at once.
//!
//! **`output` is the destination, not a stage**: the four axes of the destination set
//! (`crate::destination`) under `output.display`, or `"film-master"`. It is written after
//! the stages, and each axis is optional — an unset one is derived from the destination
//! table, so the section says only what the user chose. Each key lands with the task that
//! ships its knob — design-spec §9 states the shape, not keys written ahead of the code.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::algo::fixed::{AnchorRule, DecodeFault, DecodeParams, LINEARIZATION};
use crate::destination::{self, Change, DisplayAxes, Fault, OutputSection, Resolved};
use crate::pipeline::chain::{ChainParams, DisplayTarget, SharedParams};
use crate::pipeline::fit_gamut::DestinationGamut;
use crate::pipeline::fit_range::{
    DisplayBlack, DisplayBlackFault, DisplayPeak, MAX_DISPLAY_BLACK_STOPS,
};
use crate::pipeline::look::{
    ChannelGradeFault, ContrastFault, DEFAULT_CONTRAST, DesaturationFault, HighlightDesaturation,
    IDENTITY_CHANNEL_GRADE, LookParams, LookSection, MAX_START_STOPS,
};
use crate::pipeline::roll_white;
use crate::pipeline::scene_correction::{SceneCorrectionParams, SceneFault, WhiteBalance};
use crate::rendering::{Base, Rendering};
use crate::types::{FilmBaseSource, InputParams, MeasureParams, NcError, Result};

/// The only document version this build reads.
pub const RECIPE_VERSION: u32 = 2;

/// The top-level key carrying [`RECIPE_VERSION`].
///
/// Reserved beside `params` (the sidecar envelope's key). The removed chain's recipe
/// never had a field of this name, which is what tells the two apart.
pub const VERSION_KEY: &str = "recipe_version";

/// A recipe: every conversion knob, one section per stage.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    /// Required on load, so a document that does not say which chain it describes
    /// is never read as this one. [`check_body`] refuses its absence with a
    /// migration message before serde would report a bare "missing field".
    pub recipe_version: RecipeVersion,
    #[serde(default)]
    pub input: InputParams,
    #[serde(default)]
    pub calibration: Calibration,
    #[serde(default)]
    pub roll: RollSection,
    #[serde(default)]
    pub measure: MeasureParams,
    #[serde(default)]
    pub reconstruction: DecodeParams,
    /// `direct` or `default` (`--rendering`): the base every stage knob starts from.
    #[serde(default)]
    pub rendering: Rendering,
    #[serde(default)]
    pub scene_correction: SceneCorrectionParams,
    #[serde(default)]
    pub look: LookKeys,
    #[serde(default)]
    pub fit_range: FitRangeSection,
    #[serde(default)]
    pub fit_gamut: FitGamut,
    #[serde(default)]
    pub output: OutputSection,
}

/// The document version — a type with exactly one value, so a recipe cannot
/// deserialize into a version this build does not read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecipeVersion;

impl Serialize for RecipeVersion {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_u32(RECIPE_VERSION)
    }
}

impl<'de> Deserialize<'de> for RecipeVersion {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let v = u64::deserialize(d)?;
        if v == u64::from(RECIPE_VERSION) {
            Ok(RecipeVersion)
        } else {
            Err(serde::de::Error::custom(format!(
                "`{VERSION_KEY}` is {v}; this build reads only {RECIPE_VERSION}"
            )))
        }
    }
}

/// Fit range's section: the headroom and display black, each unset meaning the
/// rendering's base (`crate::rendering`).
///
/// Its own type rather than [`FitRangeParams`], for the reason [`FitGamut`] is: the
/// stage's other parameter, the display's peak, is the **destination's** to state, and
/// [`Recipe::chain_params`] adds it. The headroom is shared by every rendition of a
/// frame, the peak is not (`pipeline::chain`'s branch contract).
///
/// [`FitRangeParams`]: crate::pipeline::fit_range::FitRangeParams
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FitRangeSection {
    /// In stops above diffuse white: reinhard's white point is `2^headroom_stops`, and
    /// `0` is the identity.
    pub headroom_stops: Option<f32>,
    /// Where the film base renders, in stops below mid-grey on the display, or
    /// `"off"` (`pipeline::fit_range`'s display black).
    pub display_black: Option<DisplayBlack>,
}

impl FitRangeSection {
    /// Whether the user asked for a fit — a headroom, or a display black, that is
    /// neither its default nor its identity (`0`, `"off"`). The one predicate a
    /// destination that runs no fit range (`film-master`) reads to refuse
    /// ([`destination()`]), as the look's is [`LookSection::asks_for_a_look`]: the
    /// defaults are spared because every recipe carries them, the identities because
    /// they ask for nothing such a destination does not already do, and refusing them
    /// would kill the flags-win reset.
    pub fn asks_for_a_fit(&self) -> bool {
        let headroom = self
            .headroom_stops
            .is_some_and(|h| h != crate::types::DEFAULT_HEADROOM_STOPS && h != 0.0);
        let black = self
            .display_black
            .is_some_and(|b| b != DisplayBlack::default() && b != DisplayBlack::Off);
        headroom || black
    }
}

/// Fit gamut's recipe section: empty, and refuses any key. The map has no knob — its
/// ceiling is fit range's output and its target the destination's — and no off switch
/// (decided 2026-09-23, `nf-display-stages/gamut-map-share`).
///
/// Its own type rather than [`FitGamutParams`], because the stage's one parameter
/// today — the target gamut — is the **destination's** to state, not the recipe's:
/// [`Recipe::chain_params`] adds it. A recipe key for it would be a second way to
/// choose primaries the destination already fixes.
///
/// [`FitGamutParams`]: crate::pipeline::fit_gamut::FitGamutParams
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FitGamut {}

/// The roll calibration: the film base alone.
///
/// The section stays open, as design-spec §8 describes it; a later measurement joins it
/// with its own task. (The reference density `dmax` left it with
/// `nf-retire/dmax-machinery`.)
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Calibration {
    /// Required at `convert`/`roll` time with no default — `cli::validate_shared`
    /// refuses an unstated one.
    pub film_base: Option<FilmBaseSource>,
}

/// What `hanten measure-roll` measured, kept apart from the style knobs so a rendering
/// can apply it or leave it out. Applied in [`Recipe::shared_params`].
///
/// Unset values are written as `null`, never left out: `cli::merge_json` reads a one-key
/// object as an enum switch, and a per-frame `{"roll": {"white_stops": …}}` must merge.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RollSection {
    /// The roll's white-balance gains, green-anchored, as `measure-roll` reports them.
    pub white_balance: Option<[f32; 3]>,
    /// The roll's white, in scene stops above mid-grey — the measurement, not the
    /// contrast derived from it, so this section holds no contrast value
    /// (`nf-look/contrast-definition`).
    pub white_stops: Option<f32>,
    /// The frames whose own white differs from the roll's (one clamped to the cap),
    /// keyed by **file name** so the recipe still applies after the scans move.
    /// [`Recipe::for_frame`] applies an entry; a `roll --frames` manifest's `params`
    /// beat it.
    pub frames: BTreeMap<String, FrameRoll>,
}

/// One frame's own roll values ([`RollSection::frames`]).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameRoll {
    /// The frame's white, in place of the roll's `white_stops`.
    pub white_stops: f32,
}

impl RollSection {
    /// The look contrast the roll's white renders at, if it has one.
    pub fn contrast(&self) -> Option<f32> {
        self.white_stops.map(roll_white::contrast_for)
    }
}

/// The recipe's `look` keys: the stage's [`LookSection`] with the rendering-dependent
/// keys optional, since "unset" and "stated at the default" resolve differently. The
/// stage only ever receives a resolved section ([`LookKeys::resolve`]).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LookKeys {
    /// `look.contrast`, `--contrast`: the rendering contrast ([`LookSection::contrast`]).
    pub contrast: Option<f32>,
    /// `look.channel_grade`, `--channel-grade` ([`LookSection::channel_grade`]).
    pub channel_grade: [f32; 2],
    /// `look.highlight_desaturation`, `--highlight-desaturation*`: each key unset takes
    /// the rendering's base (off under `direct`).
    pub highlight_desaturation: DesaturationKeys,
}

impl Default for LookKeys {
    fn default() -> Self {
        Self {
            contrast: None,
            channel_grade: IDENTITY_CHANNEL_GRADE,
            highlight_desaturation: DesaturationKeys::default(),
        }
    }
}

impl LookKeys {
    /// The stage's section: `contrast` when none is stated, and `desaturation`'s value
    /// for each unstated desaturation key. Destructured without `..`, so a new look knob
    /// does not compile until it has a base (`crate::rendering`'s module docs).
    pub fn resolve(&self, contrast: f32, desaturation: HighlightDesaturation) -> LookSection {
        let LookKeys {
            contrast: stated_contrast,
            channel_grade,
            highlight_desaturation:
                DesaturationKeys {
                    strength,
                    start_stops,
                    band,
                },
        } = *self;
        LookSection {
            contrast: stated_contrast.unwrap_or(contrast),
            channel_grade,
            highlight_desaturation: HighlightDesaturation {
                strength: strength.unwrap_or(desaturation.strength),
                start_stops: start_stops.unwrap_or(desaturation.start_stops),
                band: band.unwrap_or(desaturation.band),
            },
        }
    }
}

/// The recipe's `look.highlight_desaturation` keys — the stage's
/// [`HighlightDesaturation`], each optional so an unset one takes the rendering's base.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DesaturationKeys {
    pub strength: Option<f32>,
    pub start_stops: Option<f32>,
    pub band: Option<[f32; 2]>,
}

/// Where the look's resolved contrast came from — the report's
/// `chain.roll.contrast_applied`, whose knob a contrast fault names, and whether the
/// `default` rendering fell back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContrastSource {
    /// `look.contrast` was stated, and wins over the roll's.
    Stated,
    /// The roll's white, through [`roll_white::contrast_for`].
    Roll,
    /// Neither: the rendering's base ([`Base::contrast`]).
    Base,
}

/// What the roll section held and what the run applied of it — the report's
/// `chain.roll`, present whenever the section states a value.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct RollReport {
    /// The section's gains, as stated.
    pub white_balance: Option<[f32; 3]>,
    /// The section's white, as stated.
    pub white_stops: Option<f32>,
    /// The look contrast `white_stops` renders at ([`roll_white::contrast_for`]).
    pub contrast: Option<f32>,
    /// Whether the gains reached scene correction (not under `direct` or the film master).
    pub white_balance_applied: bool,
    /// Whether the look's contrast is the roll's (a stated `look.contrast` wins).
    pub contrast_applied: bool,
}

/// Which style knobs this invocation typed as flags: [`Recipe::recipe_warnings`] never
/// warns about those. `roll` takes no flags and passes the default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TypedStyle {
    /// `--white-balance` was typed.
    pub white_balance: bool,
    /// `--contrast` was typed.
    pub contrast: bool,
    /// `--highlight-desaturation` (the strength; no warning reads the start or band).
    pub highlight_desaturation_strength: bool,
}

impl TypedStyle {
    /// What `convert`'s flags typed.
    pub fn of(args: &crate::cli::ConvertArgs) -> Self {
        Self {
            white_balance: args.scene.white_balance.is_some(),
            contrast: args.look.contrast.is_some(),
            highlight_desaturation_strength: args.look.highlight_desaturation.is_some(),
        }
    }
}

/// The removed chain's sections this recipe does not have, and where each one's
/// knobs go. `reconstruction` is not here: the name survives with a different
/// shape, and [`check_body`] diagnoses its old keys one by one.
const SECTIONS_WITH_NO_COUNTERPART: &[(&str, &str)] = &[(
    "print",
    "white balance and exposure are `scene_correction.white_balance` and \
         `scene_correction.exposure`; the display tone is fit range, whose one \
         operator is reinhard and whose headroom is `fit_range.headroom_stops`; the \
         black point's surviving half is display black, `fit_range.display_black` \
         (where the film base renders); and `linear_range` has no home yet \
         (`nf-scene-correction/levels-knob`)",
)];

/// The removed chain's `output` keys: a destination is its axes here.
const OLD_OUTPUT_KEYS: &[&str] = &["preset", "depth", "hdr", "output_profile", "bigtiff"];

/// The removed chain's `reconstruction` keys, and each one's fate here.
const OLD_RECONSTRUCTION_KEYS: &[(&str, &str)] = &[
    (
        "schema_version",
        "the document's `recipe_version` versions every section at once",
    ),
    (
        "type",
        "there is one fixed decode, so no reconstruction type to choose",
    ),
    (
        "curve",
        "there is one curve: its slope is `reconstruction.linearization` (print contrast \
         is `look.contrast`) and its placement \
         `reconstruction.anchor` (`{\"mid-at-base-offset\": <d>}`)",
    ),
    (
        "density",
        "`density.scale` and `density.offset` are `reconstruction.scale` and \
         `reconstruction.offset`; the regional balances are replaced by the look's \
         per-channel grade, `look.channel_grade`",
    ),
];

/// Keys retired before the removed chain was: each names where its knob lives now.
/// A path is top-level (`["density"]`) or one level down (`["input", "color"]`).
const RETIRED_KEYS: &[(&[&str], &str)] = &[
    (
        &["film_base"],
        "the film base is `calibration.film_base` (`{\"region\": [x, y, w, h]}` \
         or `{\"explicit\": [r, g, b]}`)",
    ),
    (
        &["algorithm"],
        "there is one fixed decode, so no algorithm to choose",
    ),
    (
        &["density"],
        "`density.scale` and `density.offset` are `reconstruction.scale` and \
         `reconstruction.offset`",
    ),
    (
        &["sigmoid"],
        "there is one curve: its slope is `reconstruction.linearization` (print contrast \
         is `look.contrast`) and its placement \
         `reconstruction.anchor`",
    ),
    (
        &["simple"],
        "there is one fixed decode, so no algorithm to choose",
    ),
    (
        &["calibration", "dmax"],
        "the roll reference density retired with the placements that read it; the fixed \
         decode's anchor rule is reference-free",
    ),
    (
        &["input", "color"],
        "it conflated transfer encoding with measurement meaning; use the independent \
         keys `input.transfer` (auto|linear) and `input.meaning` \
         (auto|scanner-device|colorimetric)",
    ),
];

/// Refuse a recipe body written for the removed chain, before serde sees it.
///
/// Run on the raw JSON, because the failure is about presence: a missing marker, or
/// a key the removed chain read that means nothing here. `whole` is `true`
/// for a whole recipe and `false` for a `roll` per-frame overlay, which is a partial
/// document merged onto an already-versioned one and so need not restate the
/// version (it may, and must then state the right one).
///
/// One diagnosis per call, the first found: a user fixing a recipe removes keys one
/// at a time, and a list reads as though all of them had to go before anything else
/// could be checked.
pub fn check_body(body: &serde_json::Value, whole: bool, context: &str) -> Result<()> {
    let usage = |m: String| Err(NcError::Usage(format!("{context}: {m}")));
    match body.get(VERSION_KEY) {
        None if whole => {
            return usage(format!(
                "a recipe must state `\"{VERSION_KEY}\": {RECIPE_VERSION}`. A document without \
                 it — every sidecar and `--dump-params` file written before `pipeline_version` \
                 8 — describes the rendering chain that version removed, and there is no \
                 converter. `hanten params` writes the current layout: `input`, `measure` and \
                 a `region` or `explicit` `calibration.film_base` carry over unchanged (an \
                 `\"auto\"` one retired: measure the base with `hanten measure-base \
                 <unexposed-frame>`), and the rest is a stage \
                 section each (`reconstruction`, `scene_correction`, `look`, `fit_range`) \
                 plus `output`, the destination"
            ));
        }
        Some(v) if v.as_u64() != Some(u64::from(RECIPE_VERSION)) => {
            return usage(format!(
                "`{VERSION_KEY}` is {v}; this build reads only {RECIPE_VERSION}"
            ));
        }
        _ => {}
    }
    for (path, why) in RETIRED_KEYS {
        let found = match path {
            [key] => body.get(key),
            [section, key] => body.get(section).and_then(|s| s.get(key)),
            _ => unreachable!("a retired key is one or two levels deep"),
        };
        if found.is_some() {
            return usage(format!(
                "`{}` is not a recipe key any more: {why}",
                path.join(".")
            ));
        }
    }
    for (section, why) in SECTIONS_WITH_NO_COUNTERPART {
        if body.get(section).is_some() {
            return usage(format!(
                "`{section}` is a section of the removed chain's recipe: {why}. Drop it"
            ));
        }
    }
    // The removed chain's `output` keys. The section name is shared, so it is diagnosed
    // key by key rather than refused whole.
    if let Some(key) = body
        .get("output")
        .and_then(|o| o.as_object())
        .and_then(|o| OLD_OUTPUT_KEYS.iter().find(|k| o.contains_key(**k)))
    {
        return usage(format!(
            "`output.{key}` belongs to the removed chain's recipe: a destination is its \
             axes, `output.display` with `range`, `transfer`, `gamut` and `container` (each \
             optional — an unset one is derived), or `\"film-master\"`. Drop it"
        ));
    }
    let old_key = |section: &str, table: &[(&'static str, &'static str)]| {
        let fields = body.get(section)?.as_object()?;
        table
            .iter()
            .find(|(key, _)| fields.contains_key(*key))
            .map(|(key, why)| (*key, *why))
    };
    // Retired by `film-base/holder-masked-measurement` with the rebate search it named.
    // Named here rather than left to serde, whose "unknown variant" names no remedy.
    if body
        .get("calibration")
        .and_then(|c| c.get("film_base"))
        .and_then(|f| f.as_str())
        == Some("auto")
    {
        return usage(format!(
            "`calibration.film_base` \"auto\": {} and state the `calibration` object it \
             reports (`{{\"film_base\": {{\"explicit\": [r, g, b]}}}}`), or read a region of \
             unexposed film with `{{\"region\": [x, y, w, h]}}`",
            crate::cli::AUTO_BASE_RETIRED
        ));
    }
    // Retired by `nf-scene-correction/roll-white-balance`. Named here rather than left
    // to serde, whose "unknown variant" says nothing about where the mode went.
    if let Some(mode) = body
        .get("scene_correction")
        .and_then(|s| s.get("white_balance"))
        .and_then(|w| w.as_str())
        .filter(|m| ["gray-world", "percentile"].contains(m))
    {
        return usage(format!(
            "`scene_correction.white_balance` \"{mode}\" was a per-frame estimate, and the \
             chain has none: it read a sunset as the cast and removed it. Drop it, \
             then state the gains `hanten measure-roll` reports for the roll as \
             `roll.white_balance`: `[r, g, b]`"
        ));
    }
    // Retired by `nf-reconstruction/gamma-split`, which split the one slope in two.
    // Refused at every value, the old default included: no single new key replays it.
    if let Some(v) = body.get("reconstruction").and_then(|r| r.get("contrast")) {
        let remedy = match v.as_f64() {
            // In f32, as the recipe holds it, so the stated value prints as written.
            // Only a look value validation accepts: a stated slope so small or so large
            // that the quotient leaves the normal f32 range falls to the generic remedy.
            Some(gamma) if (gamma as f32 / LINEARIZATION).is_normal() && gamma > 0.0 => {
                format!(
                    "to keep a stated {} as the whole contrast, write `look.contrast`: {} \
                     and leave `reconstruction.linearization` at its default \
                     {LINEARIZATION}",
                    gamma as f32,
                    gamma as f32 / LINEARIZATION,
                )
            }
            _ => format!(
                "state `look.contrast` for the print contrast, and leave \
                 `reconstruction.linearization` at its default {LINEARIZATION}"
            ),
        };
        return usage(format!(
            "`reconstruction.contrast` split in two: the decode's slope is now \
             `reconstruction.linearization`, the film's linearization, and how contrasty \
             the picture is is the look's `look.contrast`. Drop the key; {remedy}"
        ));
    }
    if let Some((key, why)) = old_key("reconstruction", OLD_RECONSTRUCTION_KEYS) {
        return usage(format!(
            "`reconstruction.{key}` belongs to the removed chain's recipe: {why}. Drop it"
        ));
    }
    Ok(())
}

/// Apply the command-line flags, flags winning over the recipe.
///
/// Every removed flag is refused before this runs (`cli`'s removed-flag check), so it
/// has nothing to set. `every_flag_reaches_the_recipe` holds that each conversion flag
/// has an arm here.
pub fn merge(mut r: Recipe, args: &crate::cli::ConvertArgs) -> Recipe {
    // Input color: transfer and meaning are independent axes — each flag replaces the
    // recipe's value on its own axis. The deprecated `--assume-linear` /
    // `--input-profile` flags are refused before this runs
    // (`cli::reject_deprecated_input_flags`).
    let input = &args.input_opts;
    if let Some(t) = input.input_transfer {
        r.input.transfer = t;
    }
    if let Some(m) = input.input_meaning {
        r.input.meaning = m;
    }
    if let Some(t) = input.film_type {
        r.input.film_type = t;
    }
    if let Some(p) = &input.export_ir {
        r.input.export_ir = Some(p.clone());
    }
    // The film base: the three source flags are mutually exclusive (clap-enforced), and
    // whichever is given replaces the recipe's source entirely.
    if let Some(src) = crate::cli::film_base_source_override(&args.film_base) {
        r.calibration.film_base = Some(src);
    }
    // The measurement region's static inset. The holder half of the effective area is
    // measured, never configured.
    if let Some(f) = args.measure.measure_inset {
        r.measure.inset = f;
    }
    // The decode's own knobs. `--density-gamma` is the decode's linearization — the
    // calibrated half of `gamma`; print contrast is `--contrast`, the look's
    // (`nf-reconstruction/gamma-split`).
    if let Some(v) = args.density.density_scale {
        r.reconstruction.scale = v;
    }
    if let Some(v) = args.density.density_offset {
        r.reconstruction.offset = v;
    }
    if let Some(v) = args.density.density_gamma {
        r.reconstruction.linearization = v;
    }
    if let Some(d) = args.anchor.anchor_mid_offset {
        r.reconstruction.anchor = AnchorRule::MidAboveBase(d);
    }
    // The roll's measurements (`hanten measure-roll`); whether they apply is the
    // rendering's.
    if let Some(gains) = args.roll.roll_white_balance {
        r.roll.white_balance = Some(gains);
    }
    if let Some(stops) = args.roll.roll_white {
        r.roll.white_stops = Some(stops);
    }
    // Scene correction. `--auto-wb` never reaches here: it is a removed flag, since the
    // chain has no per-frame estimate.
    if let Some(gains) = args.scene.white_balance {
        r.scene_correction.white_balance = WhiteBalance::Explicit(gains);
    }
    if let Some(stops) = args.scene.exposure {
        r.scene_correction.exposure = stops;
    }
    if let Some(v) = args.look.contrast {
        r.look.contrast = Some(v);
    }
    if let Some(v) = args.look.channel_grade {
        r.look.channel_grade = v;
    }
    let desat = &mut r.look.highlight_desaturation;
    if let Some(v) = args.look.highlight_desaturation {
        desat.strength = Some(v);
    }
    if let Some(v) = args.look.highlight_desaturation_start {
        desat.start_stops = Some(v);
    }
    if let Some(v) = args.look.highlight_desaturation_band {
        desat.band = Some(v);
    }
    if let Some(stops) = args.display.display_tone_headroom {
        r.fit_range.headroom_stops = Some(stops);
    }
    // The destination. `--film-master` and the axis flags are exclusive at the parser,
    // so at most one arm fires. An axis flag over a recipe's `"film-master"` starts
    // from no stated axes: the flag chose a rendered destination, and the recipe stated
    // none of its axes.
    let d = &args.destination;
    if d.film_master {
        r.output = OutputSection::FilmMaster;
    } else if d.any_axis() {
        let mut axes = match r.output {
            OutputSection::Display(axes) => axes,
            OutputSection::FilmMaster => DisplayAxes::default(),
        };
        axes.range = d.range.or(axes.range);
        axes.transfer = d.transfer.or(axes.transfer);
        axes.gamut = d.gamut.or(axes.gamut);
        axes.container = d.container.or(axes.container);
        r.output = OutputSection::Display(axes);
    }
    if let Some(black) = args.display.display_black {
        r.fit_range.display_black = Some(black);
    }
    if let Some(rendering) = args.rendering.rendering {
        r.rendering = rendering;
    }
    r
}

/// How a validation message names a knob: by the flag and the recipe key on
/// `convert`, which accepts both, and by the key alone on `roll`, which accepts no
/// conversion flags — naming a flag there hands the user a remedy they cannot type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KnobNames {
    FlagAndKey,
    KeyOnly,
}

/// A knob as a validation message names it — one spelling rule for every section.
fn knob_name(names: KnobNames, section: &str, flag: &str, key: &str) -> String {
    match names {
        KnobNames::FlagAndKey => format!("{flag} (recipe `{section}.{key}`)"),
        KnobNames::KeyOnly => format!("`{section}.{key}`"),
    }
}

/// The value rules this recipe's own sections carry: the decode's, which live in
/// [`DecodeParams::check`] and are rendered here as a usage error. The input, film-base
/// and measurement sections are checked by `cli::validate_shared`.
pub fn validate(r: &Recipe, names: KnobNames) -> Result<()> {
    let name = |flag: &str, key: &str| knob_name(names, "reconstruction", flag, key);
    let d = &r.reconstruction;
    let message = match d.check() {
        Ok(_) => {
            // The roll's own values first: each is also a factor in what the stages
            // below receive, so a bad one must be named as itself.
            validate_roll(&r.roll, names)?;
            validate_roll_frames(&r.roll, d.linearization, names)?;
            validate_scene_correction(r, names)?;
            let (contrast, source) = r.resolved_contrast();
            let contrast_name = match source {
                ContrastSource::Roll => knob_name(names, "roll", "--roll-white", "white_stops"),
                ContrastSource::Stated | ContrastSource::Base => {
                    knob_name(names, "look", "--contrast", "contrast")
                }
            };
            validate_look(&r.resolved_look(), &contrast_name, source, names)?;
            validate_whole_contrast(d.linearization, contrast, &contrast_name, source, names)?;
            validate_fit_range(&r.fit_range, names)?;
            return destination(r, names).map(|_| ());
        }
        Err(DecodeFault::Offset { channel, value }) => format!(
            "{} must be finite on every channel, got {value} on channel {channel}",
            name("--density-offset", "offset")
        ),
        Err(DecodeFault::Scale { channel, value }) => format!(
            "{} must be finite and positive on every channel, got {value} on channel {channel}",
            name("--density-scale", "scale")
        ),
        Err(DecodeFault::Linearization(v)) => format!(
            "{} must be finite and positive, got {v}",
            name("--density-gamma", "linearization")
        ),
        Err(DecodeFault::MidAboveBase(v)) => format!(
            "{} must be finite and positive, got {v}",
            name("--anchor-mid-offset", "anchor")
        ),
        Err(DecodeFault::Anchor {
            anchor,
            linearization,
        }) => {
            let AnchorRule::MidAboveBase(mid) = d.anchor;
            // Two routes, and opposite remedies: a tiny linearization overflows the
            // anchor itself (`0.745 / linearization`), while a huge offset overflows only
            // the exponent `linearization · anchor`, where a larger one makes it worse.
            let remedy = if anchor.is_finite() {
                format!("Use a smaller {}", name("--anchor-mid-offset", "anchor"))
            } else {
                format!("Use a larger {}", name("--density-gamma", "linearization"))
            };
            format!(
                "the decode's anchor is not usable at {} {linearization:e} and {} {mid:e}: \
                 it derives an anchor of {anchor:e}, whose exponent overflows f32 and would \
                 render every sample as exactly 0.0. {remedy}",
                name("--density-gamma", "linearization"),
                name("--anchor-mid-offset", "anchor"),
            )
        }
    };
    Err(NcError::Usage(message))
}

/// The look's value rules ([`LookSection::check_contrast`],
/// [`LookSection::check_channel_grade`], [`HighlightDesaturation::check`]), rendered as
/// a usage error naming the knob the way `names` says the command spells it.
///
/// [`HighlightDesaturation::check`]: crate::pipeline::look::HighlightDesaturation::check
fn validate_look(
    p: &LookSection,
    contrast_name: &str,
    source: ContrastSource,
    names: KnobNames,
) -> Result<()> {
    if let Err(ContrastFault(v)) = p.check_contrast() {
        // A white is finite and positive by its own rule, so only a tiny one reaches
        // here, as an infinite contrast: the contrast is `log2(1/0.18)` over the white.
        let hint = match source {
            ContrastSource::Roll => {
                "the contrast is log2(1/0.18) over the white, so use a larger white"
            }
            ContrastSource::Stated | ContrastSource::Base => "1 is the identity",
        };
        return Err(NcError::Usage(format!(
            "{contrast_name} must give a finite, positive contrast ({hint}), got {v}"
        )));
    }
    if let Err(ChannelGradeFault([r, b])) = p.check_channel_grade() {
        return Err(NcError::Usage(format!(
            "{} must be two finite, positive exponents whose spread with green's 1 is \
             under 1 (1,1 is the identity; a wider spread can fold the tone scale), got \
             [{r}, {b}]",
            knob_name(names, "look", "--channel-grade", "channel_grade")
        )));
    }
    let name = |flag: &str, key: &str| {
        knob_name(
            names,
            "look",
            flag,
            &format!("highlight_desaturation.{key}"),
        )
    };
    let message = match p.highlight_desaturation.check() {
        Ok(()) => return Ok(()),
        Err(DesaturationFault::Strength(v)) => format!(
            "{} must be within [0, 1] (0 is off), got {v}",
            name("--highlight-desaturation", "strength")
        ),
        Err(DesaturationFault::Start(v)) => format!(
            "{} must be negative and at most {MAX_START_STOPS} stops below diffuse white — \
             it is a highlight operator, and midtone cast is the grade's — got {v}",
            name("--highlight-desaturation-start", "start_stops")
        ),
        Err(DesaturationFault::Band([s0, s1])) => format!(
            "{} must be finite with 0 <= s0 < s1, got [{s0}, {s1}]",
            name("--highlight-desaturation-band", "band")
        ),
    };
    Err(NcError::Usage(message))
}

/// The whole contrast, `linearization · look.contrast` — the divisor highlight
/// desaturation normalises its saturation measure by — must be a normal positive f32.
/// Each factor can pass its own rule while the product overflows to infinity or
/// underflows to zero or a subnormal, which the stage cannot use. Keyed on the product
/// whether or not desaturation is on: a whole contrast outside f32 describes no usable
/// picture, and one rule is easier to state than a conditional one. Runs after both
/// factors' own rules, so each is already finite and positive.
fn validate_whole_contrast(
    linearization: f32,
    contrast: f32,
    contrast_name: &str,
    source: ContrastSource,
    names: KnobNames,
) -> Result<()> {
    let total = linearization * contrast;
    if total.is_normal() {
        return Ok(());
    }
    let gamma = knob_name(names, "reconstruction", "--density-gamma", "linearization");
    let overflows = total.is_infinite();
    let what = if overflows { "overflows" } else { "underflows" };
    let (toward, away) = if overflows {
        ("smaller", "larger")
    } else {
        ("larger", "smaller")
    };
    // The roll's contrast is inversely proportional to its white, so the white moves
    // the other way from the linearization.
    let remedy = match source {
        ContrastSource::Roll => format!("Use a {toward} {gamma} or a {away} {contrast_name}"),
        ContrastSource::Stated | ContrastSource::Base => {
            format!("Use a {toward} value for either")
        }
    };
    Err(NcError::Usage(format!(
        "the whole contrast, {gamma} {linearization:e} times the look contrast {contrast:e} \
         from {contrast_name}, {what} f32 (highlight desaturation divides by it). {remedy}"
    )))
}

/// The roll section's value rules: gains finite and positive, as scene correction's
/// are; a white finite and positive, since the contrast is `log2(1/0.18)` over it.
fn validate_roll(p: &RollSection, names: KnobNames) -> Result<()> {
    if let Some(gains) = p.white_balance
        && let Some((channel, value)) = gains
            .iter()
            .copied()
            .enumerate()
            .find(|&(_, g)| !g.is_finite() || g <= 0.0)
    {
        return Err(NcError::Usage(format!(
            "{} must be finite and positive on every channel, got {value} on channel {channel}",
            knob_name(names, "roll", "--roll-white-balance", "white_balance")
        )));
    }
    if let Some(stops) = p.white_stops
        && !(stops.is_finite() && stops > 0.0)
    {
        return Err(NcError::Usage(format!(
            "{} must be finite and positive — stops above mid-grey, as `hanten \
             measure-roll` reports them — got {stops}",
            knob_name(names, "roll", "--roll-white", "white_stops")
        )));
    }
    Ok(())
}

/// `roll.frames`' rules, each entry named as itself: a file-name key, and a white
/// finite and positive whose whole contrast at `linearization` fits f32. Public so
/// `convert` can judge the table as stated, before [`Recipe::for_frame`] moves its own
/// entry into `roll.white_stops`.
pub fn validate_roll_frames(p: &RollSection, linearization: f32, names: KnobNames) -> Result<()> {
    for (name, frame) in &p.frames {
        if name.is_empty() || Path::new(name).file_name() != Some(name.as_ref()) {
            return Err(NcError::Usage(format!(
                "recipe `roll.frames` keys are file names, not paths: got {name:?}"
            )));
        }
        let stops = frame.white_stops;
        if !(stops.is_finite() && stops > 0.0) {
            return Err(NcError::Usage(format!(
                "recipe `roll.frames.\"{name}\".white_stops` must be finite and positive — \
                 stops above mid-grey, as `hanten measure-roll` reports them — got {stops}"
            )));
        }
        validate_whole_contrast(
            linearization,
            roll_white::contrast_for(stops),
            &format!("recipe `roll.frames.\"{name}\".white_stops`"),
            ContrastSource::Roll,
            names,
        )?;
    }
    Ok(())
}

/// Scene correction's value rules ([`SceneCorrectionParams::check`]), rendered as a
/// usage error naming the knob the way `names` says the command spells it.
///
/// The stated section is checked first, so a bad stated value is named as itself; then
/// the section the stage receives, with the roll's gains multiplied in
/// ([`Recipe::resolved_scene_correction`]) — only a product can fail there, and the
/// message names both factors.
fn validate_scene_correction(r: &Recipe, names: KnobNames) -> Result<()> {
    let name = |flag: &str, key: &str| knob_name(names, "scene_correction", flag, key);
    scene_correction_fault(&r.scene_correction, names)?;
    // A product can fail either way: as a gain no longer finite, or as a gain whose
    // product with the exposure's is not a normal f32.
    let product = match r
        .roll
        .white_balance
        .map(|_| r.resolved_scene_correction().check())
    {
        Some(Err(SceneFault::WhiteBalance { channel, value })) => Some((channel, value)),
        Some(Err(SceneFault::Combined { channel, gain })) => Some((channel, gain)),
        _ => None,
    };
    if let Some((channel, gain)) = product {
        return Err(NcError::Usage(format!(
            "{} times {} times the exposure gain from {} is {gain:e} on channel {channel}, \
             which is not a normal f32 — every sample of that channel would render as 0 or \
             inf. Move the white balance or the exposure toward neutral",
            knob_name(names, "roll", "--roll-white-balance", "white_balance"),
            name("--white-balance", "white_balance"),
            name("--exposure", "exposure"),
        )));
    }
    Ok(())
}

/// [`SceneCorrectionParams::check`] on the stated section, as a usage error.
fn scene_correction_fault(p: &SceneCorrectionParams, names: KnobNames) -> Result<()> {
    let name = |flag: &str, key: &str| knob_name(names, "scene_correction", flag, key);
    let message = match p.check() {
        Ok(()) => return Ok(()),
        Err(SceneFault::WhiteBalance { channel, value }) => format!(
            "{} must be finite and positive on every channel, got {value} on channel \
             {channel}",
            name("--white-balance", "white_balance")
        ),
        Err(SceneFault::Exposure(stops)) => format!(
            "{} must be finite, with a gain 2^EV that is a normal f32 (roughly -126 to \
             +127 stops), got {stops}",
            name("--exposure", "exposure")
        ),
        Err(SceneFault::Combined { channel, gain }) => format!(
            "{} times the exposure gain from {} is {gain:e} on channel {channel}, which \
             is not a normal f32 — every sample of that channel would render as 0 or \
             inf. Move the white balance or the exposure toward neutral",
            name("--white-balance", "white_balance"),
            name("--exposure", "exposure"),
        ),
    };
    Err(NcError::Usage(message))
}

/// Fit range's value rules — the headroom's, [`crate::types::headroom_fault`], and
/// display black's, [`DisplayBlack::check`] —
/// rendered as a usage error for this recipe's keys.
fn validate_fit_range(p: &FitRangeSection, names: KnobNames) -> Result<()> {
    // Only stated values: an unset one is the rendering's base, a constant.
    if let Some(Err(DisplayBlackFault(v))) = p.display_black.map(|b| b.check()) {
        return Err(NcError::Usage(format!(
            "{} must be stops below mid-grey within (0, {MAX_DISPLAY_BLACK_STOPS}], or \
             `off`, got {v}",
            knob_name(names, "fit_range", "--display-black", "display_black")
        )));
    }
    let Some(fault) = p.headroom_stops.and_then(crate::types::headroom_fault) else {
        return Ok(());
    };
    let name = knob_name(
        names,
        "fit_range",
        "--display-tone-headroom",
        "headroom_stops",
    );
    Err(NcError::Usage(crate::types::headroom_fault_message(
        fault, &name,
    )))
}

/// Where the recipe renders to — its `output` section resolved against the destination
/// table (`crate::destination`), or the film master. [`validate`] runs this, and the run
/// reads the value it returns.
///
/// `film-master` runs no rendering stage, so a stage the user asked for is refused,
/// naming the stage: **one** rule per stage, never one per knob — scene correction
/// keyed on [`SceneCorrectionParams::asks_for_a_correction`], the look on
/// [`LookSection::asks_for_a_look`], fit range on [`FitRangeSection::asks_for_a_fit`]. Each
/// spares its default, which every recipe carries, and its identity, which renders
/// exactly what the film master does (refusing that would kill the flags-win reset).
/// Fit gamut has no knob to ask with. Every stage asked for is named in one refusal, in
/// chain order, so removing one does not uncover the next.
pub fn destination(r: &Recipe, names: KnobNames) -> Result<Destination> {
    match &r.output {
        OutputSection::FilmMaster if r.rendering == Rendering::Direct => {
            // The remedy works whichever of the recipe or a flag stated `direct`: a flag
            // overrides the recipe's rendering, and `roll` takes no flags.
            let (rendering, master, back) = match names {
                KnobNames::FlagAndKey => (
                    "--rendering direct (recipe `rendering`)",
                    "--film-master (recipe `output`: `\"film-master\"`)",
                    "pass --rendering default",
                ),
                KnobNames::KeyOnly => (
                    "`rendering` \"direct\"",
                    "`output` \"film-master\"",
                    "set `rendering` to \"default\" (or remove it)",
                ),
            };
            Err(NcError::Usage(format!(
                "{rendering} chooses what the rendering stages start from, and {master} \
                 runs none: it writes the fixed decode's linear ACEScg. Either {back}, or \
                 choose a rendered destination — `direct` alone writes the HDR float TIFF, \
                 the rendered output closest to the decode"
            )))
        }
        OutputSection::FilmMaster => {
            let asked = stages_the_master_cannot_run(r, names);
            if asked.is_empty() {
                return Ok(Destination::FilmMaster);
            }
            let master = match names {
                KnobNames::FlagAndKey => "--film-master (recipe `output`: `\"film-master\"`)",
                KnobNames::KeyOnly => "`output` \"film-master\"",
            };
            let (drop, identity) = match (names, asked.len()) {
                (KnobNames::FlagAndKey, 1) => {
                    ("the flags and recipe keys that ask for it", "its identity")
                }
                (KnobNames::FlagAndKey, _) => (
                    "the flags and recipe keys that ask for them",
                    "each one's identity",
                ),
                (KnobNames::KeyOnly, 1) => ("the keys that ask for it", "its identity"),
                (KnobNames::KeyOnly, _) => ("the keys that ask for them", "each one's identity"),
            };
            let (stages, identities): (Vec<&str>, Vec<&str>) = asked.into_iter().unzip();
            Err(NcError::Usage(format!(
                "{master} writes the fixed decode's linear ACEScg with no rendering stage, \
                 so it cannot apply {} this recipe asks for. Either drop {drop}, or state \
                 {identity} ({}), or choose a rendered destination",
                stages.join(" and "),
                identities.join("; "),
            )))
        }
        OutputSection::Display(axes) => destination::resolve(axes, &r.base().axes)
            .map(Destination::Display)
            .map_err(|fault| NcError::Usage(fault_message(axes, &fault, names))),
    }
}

/// The rendering stages `r` asks for that the film master does not run, in chain order,
/// each as `(the stage as the message names it, its identity as the command states it)`.
fn stages_the_master_cannot_run(r: &Recipe, names: KnobNames) -> Vec<(&'static str, &'static str)> {
    let flags = names == KnobNames::FlagAndKey;
    let pick = |flag: (&'static str, &'static str), key: (&'static str, &'static str)| {
        if flags { flag } else { key }
    };
    let mut asked = Vec::new();
    if r.scene_correction.asks_for_a_correction() {
        asked.push(pick(
            (
                "scene correction (--exposure, --white-balance, recipe `scene_correction`)",
                "--exposure 0 --white-balance 1,1,1",
            ),
            (
                "scene correction (`scene_correction`)",
                "`scene_correction.exposure` 0, `scene_correction.white_balance` \
                 {\"explicit\": [1, 1, 1]}",
            ),
        ));
    }
    // The look as stated, every unstated key at its default: the roll's contrast is a
    // measurement the film master leaves unapplied, not a look the user asked for.
    if r.look
        .resolve(DEFAULT_CONTRAST, HighlightDesaturation::DEFAULT)
        .asks_for_a_look()
    {
        asked.push(pick(
            (
                "the look (--contrast, --channel-grade, --highlight-desaturation*, \
                 recipe `look`)",
                "--contrast 1 --channel-grade 1,1 --highlight-desaturation 0",
            ),
            (
                "the look (`look`)",
                "`look.contrast` 1, `look.channel_grade` [1, 1], \
                 `look.highlight_desaturation.strength` 0",
            ),
        ));
    }
    if r.fit_range.asks_for_a_fit() {
        asked.push(pick(
            (
                "fit range (--display-tone-headroom, --display-black, recipe `fit_range`)",
                "--display-tone-headroom 0 --display-black off",
            ),
            (
                "fit range (`fit_range`)",
                "`fit_range.headroom_stops` 0, `fit_range.display_black` \"off\"",
            ),
        ));
    }
    asked
}

/// A recipe's resolved destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Destination {
    Display(Resolved),
    FilmMaster,
}

/// One axis value as a message names it: the flag on `convert`, the key on `roll`.
fn axis_value(names: KnobNames, flag: &str, key: &str, value: &str) -> String {
    match names {
        KnobNames::FlagAndKey => format!("{flag} {value}"),
        KnobNames::KeyOnly => format!("`output.display.{key}` \"{value}\""),
    }
}

/// A resolved destination as a message names it: every axis (or the film master) as the
/// flags `convert` takes, or as the recipe keys `roll` takes.
pub fn destination_label(destination: Destination, names: KnobNames) -> String {
    match (destination, names) {
        // As the stage refusal names it: the film master may come from the flag or the
        // recipe, and this label does not know which.
        (Destination::FilmMaster, KnobNames::FlagAndKey) => {
            "--film-master (recipe `output`: `\"film-master\"`)".to_string()
        }
        (Destination::FilmMaster, KnobNames::KeyOnly) => "`output` \"film-master\"".to_string(),
        (Destination::Display(d), _) => complete_destination(names, &d.axes()),
    }
}

/// A complete destination as a message names it.
pub fn complete_destination(names: KnobNames, axes: &DisplayAxes) -> String {
    let parts: Vec<String> = axes
        .stated_axes()
        .iter()
        .map(|a| axis_value(names, a.flag, a.key, a.value))
        .collect();
    match names {
        KnobNames::FlagAndKey => parts.join(" "),
        KnobNames::KeyOnly => parts.join(", "),
    }
}

/// The recipe-key footnote a flag message carries, so a recipe author can act on it too.
fn keys_note(names: KnobNames) -> &'static str {
    match names {
        KnobNames::FlagAndKey => {
            " (recipe keys `output.display.range`, `.transfer`, `.gamut`, \
                                  `.container`)"
        }
        KnobNames::KeyOnly => "",
    }
}

/// A destination fault as a usage message. Every remedy it names resolves as written
/// (`crate::destination`'s `every_offered_remedy_resolves`).
fn fault_message(axes: &DisplayAxes, fault: &Fault, names: KnobNames) -> String {
    let stated = complete_destination(names, axes);
    let or_list = |items: &[String]| match items {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{}, or {last}", init.join(", ")),
    };
    let choices = |flag: &str, key: &str, values: &[&str]| match names {
        KnobNames::FlagAndKey => format!("{flag} {}", values.join("|")),
        KnobNames::KeyOnly => format!(
            "`output.display.{key}` {}",
            values
                .iter()
                .map(|v| format!("\"{v}\""))
                .collect::<Vec<_>>()
                .join(" or ")
        ),
    };
    let destinations = |list: &[DisplayAxes]| {
        list.iter()
            .map(|a| match complete_destination(names, a) {
                none if none.is_empty() => "nothing (the default destination)".to_string(),
                some => some,
            })
            .collect::<Vec<_>>()
            .join("; ")
    };
    let note = keys_note(names);
    match fault {
        Fault::Conflict {
            conflicting,
            changes,
            instead,
        } => {
            let named: Vec<String> = conflicting
                .iter()
                .map(|a| axis_value(names, a.flag, a.key, a.value))
                .collect();
            let remedy = if changes.is_empty() {
                format!(
                    "No one change fixes it with the rest of what is stated; state instead \
                     one of: {}",
                    destinations(instead)
                )
            } else {
                let options: Vec<String> = changes
                    .iter()
                    .map(|c: &Change| {
                        let head = axis_value(names, c.flag, c.key, c.value);
                        match &c.then {
                            None => head,
                            Some((flag, key, values)) => {
                                format!("{head} with {}", choices(flag, key, values))
                            }
                        }
                    })
                    .collect();
                format!("Use {}", or_list(&options))
            };
            format!(
                "no destination combines {}{note}. {remedy}",
                named.join(" and "),
            )
        }
        Fault::Ambiguous {
            flag,
            key,
            choices: values,
        } => format!(
            "{stated} leaves {} open: more than one destination fits. State one — {}{note}",
            match names {
                KnobNames::FlagAndKey => (*flag).to_string(),
                KnobNames::KeyOnly => format!("`output.display.{key}`"),
            },
            choices(flag, key, values),
        ),
        Fault::NotYet {
            row,
            arriving_with,
            adding,
            instead,
        } => {
            let full = complete_destination(
                names,
                &DisplayAxes {
                    range: Some(row.range),
                    transfer: Some(row.transfer),
                    gamut: Some(row.gamut),
                    container: Some(row.container),
                },
            );
            let what = if stated == full {
                full
            } else {
                format!("{stated} resolves to {full}, which")
            };
            let ready = if adding.is_empty() {
                format!("Written today, stated instead: {}", destinations(instead))
            } else {
                format!(
                    "Written today, adding to what is stated: {}",
                    destinations(adding)
                )
            };
            format!("{what} is not written yet — it arrives with {arriving_with}. {ready}{note}")
        }
    }
}

impl Recipe {
    /// The recipe's `params_hash` (`version::stable_hash`) over the bytes
    /// `--dump-params` writes, so a dumped recipe hashes to the run it came from. The
    /// report's `identity` and the telemetry record carry the same value.
    pub fn params_hash(&self) -> String {
        // Plain data cannot fail to serialize; the empty fallback keeps telemetry,
        // which must never fail a run, total.
        crate::version::stable_hash(&serde_json::to_string_pretty(self).unwrap_or_default())
    }

    /// One rendition's parameters for `pipeline::chain::render` — the recipe's shared
    /// half ([`Recipe::shared_params`]) plus the destination's peak and gamut, which
    /// only the destination states.
    pub fn chain_params(&self, peak: DisplayPeak, gamut: DestinationGamut) -> ChainParams {
        ChainParams {
            shared: self.shared_params(),
            target: DisplayTarget { peak, gamut },
        }
    }

    /// Everything every rendition of a frame shares (`pipeline::chain`'s branch contract),
    /// each stage resolved against the rendering's base.
    pub fn shared_params(&self) -> SharedParams {
        let (headroom_stops, display_black) = self.resolved_fit_range();
        SharedParams {
            scene_correction: self.resolved_scene_correction(),
            look: LookParams {
                section: self.resolved_look(),
                linearization: self.reconstruction.linearization,
            },
            headroom_stops,
            display_black,
        }
    }

    /// The base this recipe's rendering starts every knob from.
    pub fn base(&self) -> Base {
        self.rendering.base()
    }

    /// This recipe as `input` renders it: the frame's [`RollSection::frames`] entry, if
    /// any, moves into `roll.white_stops`. `convert` and every `roll` frame go through
    /// it, so the two stay byte-identical. The entry is removed, not copied, so a flag
    /// that then beats it is what a `--dump-params` replay renders; the other entries
    /// stay for [`validate`].
    pub fn for_frame(mut self, input: &Path) -> Self {
        let name = input.file_name().and_then(|n| n.to_str());
        if let Some(frame) = name.and_then(|n| self.roll.frames.remove(n)) {
            self.roll.white_stops = Some(frame.white_stops);
        }
        self
    }

    /// The roll section as the rendering applies it: whole, or not at all.
    fn applied_roll(&self) -> RollSection {
        if self.base().applies_roll {
            self.roll.clone()
        } else {
            RollSection::default()
        }
    }

    /// Whether the roll's gains reach scene correction: not under a rendering that leaves
    /// the roll out, nor on the film master, which runs no scene correction.
    pub fn applies_roll_white_balance(&self) -> bool {
        self.output != OutputSection::FilmMaster && self.base().applies_roll
    }

    /// Scene correction as the stage receives it: the applied roll's gains multiplied
    /// into the stated white balance — which is `1,1,1` unless the user set it, so a
    /// roll's gains alone reach the stage exactly. Both renderings start the white
    /// balance and the exposure at the identity.
    pub fn resolved_scene_correction(&self) -> SceneCorrectionParams {
        let SceneCorrectionParams {
            white_balance: WhiteBalance::Explicit(stated),
            exposure,
        } = self.scene_correction;
        let white_balance = match self.applied_roll().white_balance {
            Some(roll) => std::array::from_fn(|c| roll[c] * stated[c]),
            None => stated,
        };
        SceneCorrectionParams {
            white_balance: WhiteBalance::Explicit(white_balance),
            exposure,
        }
    }

    /// The look's contrast as the stage receives it, and where it came from: a stated
    /// `look.contrast` wins, else the applied roll's white, else the rendering's base.
    pub fn resolved_contrast(&self) -> (f32, ContrastSource) {
        match (self.look.contrast, self.applied_roll().contrast()) {
            (Some(stated), _) => (stated, ContrastSource::Stated),
            (None, Some(roll)) => (roll, ContrastSource::Roll),
            (None, None) => (self.base().contrast, ContrastSource::Base),
        }
    }

    /// The look as the stage receives it.
    pub fn resolved_look(&self) -> LookSection {
        self.look.resolve(
            self.resolved_contrast().0,
            self.base().highlight_desaturation,
        )
    }

    /// Fit range's headroom and display black as the stage receives them: stated, else
    /// the rendering's base. Destructured without `..`, like [`LookKeys::resolve`].
    pub fn resolved_fit_range(&self) -> (f32, DisplayBlack) {
        let FitRangeSection {
            headroom_stops,
            display_black,
        } = self.fit_range;
        let base = self.base();
        (
            headroom_stops.unwrap_or(base.headroom_stops),
            display_black.unwrap_or(base.display_black),
        )
    }

    /// The report's `chain.roll`: `None` when the section states nothing. `rendered`
    /// is whether any rendering stage ran — the film master applies neither value, and
    /// `--rendering direct` leaves the section out.
    pub fn roll_report(&self, rendered: bool) -> Option<RollReport> {
        let r = &self.roll;
        let applies = rendered && self.base().applies_roll;
        // Other frames' `frames` entries are not this frame's measurement.
        (r.white_balance.is_some() || r.white_stops.is_some()).then(|| RollReport {
            white_balance: r.white_balance,
            white_stops: r.white_stops,
            contrast: r.contrast(),
            white_balance_applied: applies && r.white_balance.is_some(),
            contrast_applied: applies && self.resolved_contrast().1 == ContrastSource::Roll,
        })
    }

    /// The run's warnings about its recipe, emitted once per run (once per roll, from the
    /// shared recipe). `typed` spares what was given as a flag; empty for the film master.
    /// Why these and not refusals: design-update Part 2, "Recipe warnings, not refusals".
    pub fn recipe_warnings(&self, typed: TypedStyle) -> Vec<String> {
        if self.output == OutputSection::FilmMaster {
            return Vec::new();
        }
        match self.rendering {
            Rendering::Default => {
                let mut w = self.roll_overlap_warnings(typed);
                w.extend(self.fallback_warning(typed));
                w
            }
            Rendering::Direct => self.direct_override_warning(typed).into_iter().collect(),
        }
    }

    /// `default` without a roll measurement: what fell back. A typed flag, even
    /// `--white-balance 1,1,1`, is a choice and silences its half.
    fn fallback_warning(&self, typed: TypedStyle) -> Option<String> {
        let mut fell_back = Vec::new();
        if !typed.white_balance
            && self.roll.white_balance.is_none()
            && self.scene_correction.white_balance == WhiteBalance::Explicit([1.0, 1.0, 1.0])
        {
            fell_back.push("neutral white balance (no `roll.white_balance`)".to_string());
        }
        if self.resolved_contrast().1 == ContrastSource::Base {
            fell_back.push(format!(
                "the fallback contrast {} (no `roll.white_stops`)",
                self.base().contrast
            ));
        }
        (!fell_back.is_empty()).then(|| {
            format!(
                "no roll measurement: rendered with {}. Run `hanten measure-roll` over the \
                 roll and use the recipe it writes (its `roll` section); or state the white \
                 balance and contrast you want (`scene_correction.white_balance`, \
                 `look.contrast`); or use the `direct` rendering (`rendering`: \"direct\"), \
                 the decode without a roll correction, whose unset destination is the HDR \
                 float TIFF",
                fell_back.join(" and "),
            )
        })
    }

    /// `default`: a recipe's style value beside a roll measurement it multiplies or
    /// overrides.
    fn roll_overlap_warnings(&self, typed: TypedStyle) -> Vec<String> {
        let mut warnings = Vec::new();
        let WhiteBalance::Explicit(stated) = self.scene_correction.white_balance;
        if !typed.white_balance
            && let Some(roll) = self.roll.white_balance
            && stated != [1.0, 1.0, 1.0]
        {
            let product: [f32; 3] = std::array::from_fn(|c| roll[c] * stated[c]);
            warnings.push(format!(
                "the recipe's `scene_correction.white_balance` {stated:?} multiplies the \
                 roll's gains, `roll.white_balance` {roll:?}: the white balance applied is \
                 {product:?}. A stated white balance is an adjustment on top of the roll's \
                 measurement; if it holds gains an earlier `hanten measure-roll` wrote there, \
                 drop it — they now live in `roll.white_balance`"
            ));
        }
        if !typed.contrast
            && let (Some(stated), Some(white), Some(roll)) = (
                self.look.contrast,
                self.roll.white_stops,
                self.roll.contrast(),
            )
        {
            warnings.push(format!(
                "the recipe's `look.contrast` {stated} overrides the roll's contrast {roll} \
                 (from `roll.white_stops` {white}). If it came from a recipe an earlier build \
                 wrote (every one stated `look.contrast` 1.1111112) or from an earlier `hanten \
                 measure-roll`, set it to `null` to use the roll's"
            ));
        }
        warnings
    }

    /// `direct`: a recipe value that moves its pinned base and that an earlier build could
    /// have written unchosen — narrow on purpose (design-update, "Recipe warnings").
    fn direct_override_warning(&self, typed: TypedStyle) -> Option<String> {
        // The strength every recipe an earlier build wrote states — a historical value,
        // pinned here rather than read from today's default.
        const OLD_SERIALIZED_STRENGTH: f32 = 0.8;
        let base = self.base();
        let mut moved: Vec<String> = Vec::new();
        let mut old_default = false;
        let mut beside_roll = false;
        if !typed.highlight_desaturation_strength
            && self.look.highlight_desaturation.strength == Some(OLD_SERIALIZED_STRENGTH)
        {
            old_default = true;
            moved.push(format!(
                "`look.highlight_desaturation.strength` {OLD_SERIALIZED_STRENGTH} (direct: {}, \
                 off; every recipe an earlier build wrote stated 0.8)",
                base.highlight_desaturation.strength
            ));
        }
        if self.roll != RollSection::default() {
            let WhiteBalance::Explicit(wb) = self.scene_correction.white_balance;
            if !typed.white_balance && wb != [1.0, 1.0, 1.0] {
                beside_roll = true;
                moved.push(format!(
                    "`scene_correction.white_balance` {wb:?} beside a `roll` section (direct: \
                     [1, 1, 1])"
                ));
            }
            if !typed.contrast
                && let Some(c) = self.look.contrast
                && c != base.contrast
            {
                beside_roll = true;
                moved.push(format!(
                    "`look.contrast` {c} beside a `roll` section (direct: {})",
                    base.contrast
                ));
            }
        }
        // Each case's remedy, never telling a deliberate adjuster to drop the value.
        let mut remedies: Vec<&str> = Vec::new();
        if old_default {
            remedies.push(
                "if the strength came from a recipe an earlier build wrote, set it to `null` \
                 so `direct` renders as pinned",
            );
        }
        if beside_roll {
            remedies.push(
                "if a value beside the `roll` section came from an earlier `hanten \
                 measure-roll`, drop `scene_correction.white_balance` (and set \
                 `look.contrast` to `null`)",
            );
        }
        let remedies = remedies.join("; ");
        let remedies = match remedies.split_at_checked(1) {
            Some((first, rest)) => format!("{}{rest}", first.to_uppercase()),
            None => remedies,
        };
        (!moved.is_empty()).then(|| {
            format!(
                "the recipe moves the `direct` rendering's pinned base: {}. {}; a \
                 deliberate adjustment is fine — type it as a flag to keep it without this \
                 warning",
                moved.join(", "),
                remedies,
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> std::result::Result<Recipe, serde_json::Error> {
        serde_json::from_str(json)
    }

    fn check(json: &str, whole: bool) -> std::result::Result<(), String> {
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        check_body(&v, whole, "recipe r.json").map_err(|e| e.message().to_string())
    }

    /// Top-level keys of a serialized document, in the order they are written.
    /// (A `serde_json::Value` map sorts its keys, so it cannot answer this.)
    fn written_order(text: &str) -> Vec<String> {
        let mut depth = 0usize;
        let mut keys = Vec::new();
        let mut chars = text.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            match c {
                '{' | '[' => depth += 1,
                '}' | ']' => depth -= 1,
                '"' => {
                    let end = text[i + 1..].find('"').unwrap() + i + 1;
                    if depth == 1 && text[end + 1..].trim_start().starts_with(':') {
                        keys.push(text[i + 1..end].to_string());
                    }
                    while chars.peek().is_some_and(|(j, _)| *j <= end) {
                        chars.next();
                    }
                }
                _ => {}
            }
        }
        keys
    }

    #[test]
    fn the_default_document_names_every_stage_in_chain_order_and_round_trips() {
        let text = serde_json::to_string_pretty(&Recipe::default()).unwrap();
        assert_eq!(
            written_order(&text),
            [
                VERSION_KEY,
                "input",
                "calibration",
                "roll",
                "measure",
                "reconstruction",
                "rendering",
                "scene_correction",
                "look",
                "fit_range",
                "fit_gamut",
                "output",
            ]
        );
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        // A stage with no knob is present as an empty object, not absent or `null`. A
        // knob whose base is the rendering's (`crate::rendering`) is written `null`,
        // unstated — the look's contrast and highlight desaturation, fit range's headroom
        // and display black — and so is the roll section; the rest are written at their
        // identity (scene correction, the grade). Keys are written, never left out, so a
        // per-frame override merges. (Fit gamut's map runs at every setting; it simply
        // has nothing for a recipe to set.)
        assert_eq!(json["rendering"], "default");
        assert_eq!(json["fit_gamut"], serde_json::json!({}));
        // Nothing stated: every axis is derived, so a written recipe states none.
        assert_eq!(json["output"], serde_json::json!({"display": {}}));
        assert_eq!(
            serde_json::to_string(&Recipe::default().look).unwrap(),
            r#"{"contrast":null,"channel_grade":[1.0,1.0],"highlight_desaturation":{"strength":null,"start_stops":null,"band":null}}"#
        );
        assert_eq!(
            json["scene_correction"],
            serde_json::json!({"white_balance": {"explicit": [1.0, 1.0, 1.0]}, "exposure": 0.0})
        );
        assert_eq!(
            json["fit_range"],
            serde_json::json!({"headroom_stops": null, "display_black": null})
        );
        assert_eq!(
            json["roll"],
            serde_json::json!({"white_balance": null, "white_stops": null, "frames": {}})
        );
        assert_eq!(json[VERSION_KEY], RECIPE_VERSION);
        let back: Recipe = serde_json::from_str(&text).unwrap();
        assert_eq!(back, Recipe::default());
    }

    #[test]
    fn the_decode_section_spells_the_decode_parameters() {
        // Compared as text: a `Value` holds the f32 defaults widened to f64
        // (`0.8399999737739563`), while the written document spells `0.84`.
        assert_eq!(
            serde_json::to_string(&Recipe::default().reconstruction).unwrap(),
            r#"{"scale":[1.0,0.84,0.73],"offset":[0.0,0.0,0.0],"linearization":1.8,"anchor":{"mid-at-base-offset":0.62}}"#
        );
        // No reference density in the new calibration section.
        let json = serde_json::to_value(Recipe::default()).unwrap();
        assert_eq!(json["calibration"], serde_json::json!({"film_base": null}));
    }

    #[test]
    fn the_look_is_handed_the_decodes_linearization() {
        // Highlight desaturation's measure is normalised by the whole contrast that
        // shaped its input — the decode's half of it the look section cannot state, so
        // `chain_params` must.
        let r = parse(
            r#"{"recipe_version": 2, "reconstruction": {"linearization": 3.1},
                "look": {"contrast": 1.2, "highlight_desaturation": {"strength": 0.5}}}"#,
        )
        .unwrap();
        let look = r
            .chain_params(DisplayPeak::SDR, DestinationGamut::DisplayP3)
            .shared
            .look;
        assert_eq!(look.linearization, 3.1);
        assert_eq!(look.section.contrast, 1.2);
        assert_eq!(look.section.highlight_desaturation.strength, 0.5);
        assert_eq!(
            look.section.highlight_desaturation.band,
            LookSection::default().highlight_desaturation.band,
            "an omitted key takes its default"
        );
    }

    #[test]
    fn the_look_contrast_leaves_the_decode_untouched() {
        // The split's falsifiable point: how contrasty the picture is must not reach the
        // decode. The decode reads `reconstruction` alone, so its output — what
        // `film-master` will carry — is bit-identical across look contrasts, while the
        // linearization, stated beside it, does move it.
        use crate::algo::fixed;
        use crate::types::{FilmBase, LinearImage};
        let scan = LinearImage::new(2, 1, vec![0.5, 0.3, 0.2, 0.05, 0.04, 0.03], None).unwrap();
        let base = FilmBase::from([0.9, 0.55, 0.42]);
        let decoded = |json: &str| {
            let r = parse(json).unwrap();
            let (film, _) = fixed::decode(&scan, &base, &r.reconstruction).unwrap();
            film.rgb().iter().map(|v| v.to_bits()).collect::<Vec<_>>()
        };
        let plain = decoded(r#"{"recipe_version": 2}"#);
        assert_eq!(
            plain,
            decoded(r#"{"recipe_version": 2, "look": {"contrast": 1.6}}"#)
        );
        assert_ne!(
            plain,
            decoded(r#"{"recipe_version": 2, "reconstruction": {"linearization": 2.0}}"#)
        );
    }

    #[test]
    fn a_partial_recipe_takes_the_defaults_it_omits() {
        let r =
            parse(r#"{"recipe_version": 2, "reconstruction": {"linearization": 1.7}}"#).unwrap();
        assert_eq!(r.reconstruction.linearization, 1.7);
        assert_eq!(r.reconstruction.scale, DecodeParams::default().scale);
        assert_eq!(r.look, LookKeys::default());
    }

    #[test]
    fn the_version_is_required_and_exact() {
        assert!(
            parse("{}")
                .unwrap_err()
                .to_string()
                .contains("recipe_version")
        );
        let err = parse(r#"{"recipe_version": 3}"#).unwrap_err().to_string();
        assert!(err.contains("reads only 2"), "{err}");
    }

    #[test]
    fn every_section_rejects_an_unknown_key() {
        for section in [
            "input",
            "calibration",
            "roll",
            "measure",
            "reconstruction",
            "scene_correction",
            "look",
            "fit_range",
            "fit_gamut",
        ] {
            let json = format!(r#"{{"recipe_version": 2, "{section}": {{"nonsense": 1}}}}"#);
            let err = parse(&json).unwrap_err().to_string();
            assert!(err.contains("nonsense"), "{section}: {err}");
        }
        let err = parse(r#"{"recipe_version": 2, "nonsense": {}}"#)
            .unwrap_err()
            .to_string();
        assert!(err.contains("nonsense"), "{err}");
    }

    #[test]
    fn a_retired_per_frame_white_balance_names_the_roll_measurement() {
        for mode in ["gray-world", "percentile"] {
            let body = format!(r#"{{"scene_correction": {{"white_balance": "{mode}"}}}}"#);
            let err = check(&body, false).unwrap_err();
            assert!(
                err.contains(mode)
                    && err.contains("measure-roll")
                    && err.contains("`roll.white_balance`"),
                "{err}"
            );
        }
        // Only the two retired names: any other string is a typo for serde to report,
        // not "a per-frame estimate".
        check(
            r#"{"scene_correction": {"white_balance": "neutral"}}"#,
            false,
        )
        .unwrap();
        // Stated gains are the one form, and pass the schema check.
        check(
            r#"{"scene_correction": {"white_balance": {"explicit": [1.2, 1, 1.1]}}}"#,
            false,
        )
        .unwrap();
    }

    #[test]
    fn the_retired_auto_film_base_is_refused_with_a_remedy_a_recipe_can_take() {
        // A recipe reaches `roll`, which has no flags, so the remedy is a recipe value.
        for whole in [true, false] {
            let body = if whole {
                r#"{"recipe_version": 2, "calibration": {"film_base": "auto"}}"#
            } else {
                r#"{"calibration": {"film_base": "auto"}}"#
            };
            let err = check(body, whole).unwrap_err();
            assert!(err.contains("`calibration.film_base` \"auto\""), "{err}");
            assert!(
                err.contains("hanten measure-base <unexposed-frame>"),
                "{err}"
            );
            assert!(err.contains(r#"{"explicit": [r, g, b]}"#), "{err}");
            // …and never serde's "unknown variant", which names no remedy.
            assert!(!err.contains("unknown variant"), "{err}");
        }
    }

    #[test]
    fn a_body_without_the_marker_is_refused_as_the_removed_chains() {
        let base = r#"{"calibration": {"film_base": {"explicit": [0.5, 0.4, 0.3]}}}"#;
        let err = check(base, true).unwrap_err();
        assert!(err.contains("\"recipe_version\": 2"), "{err}");
        assert!(err.contains("`hanten params`"), "{err}");
        assert!(err.contains("pipeline_version` 8"), "{err}");
        assert!(!err.contains("--new-flow"), "{err}");
        // A per-frame overlay is partial and may omit it…
        check(base, false).unwrap();
        // …but may not state a wrong one.
        let err = check(r#"{"recipe_version": 1}"#, false).unwrap_err();
        assert!(err.contains("reads only 2"), "{err}");
    }

    #[test]
    fn an_old_section_is_refused_by_name_with_where_it_went() {
        let err = check(r#"{"recipe_version": 2, "print": {}}"#, true).unwrap_err();
        assert!(
            err.contains("`print`") && err.contains("scene_correction"),
            "{err}"
        );
        // `output` is shared by name, so the removed chain's keys under it are named
        // one by one, pointing at the destination's axes.
        for key in OLD_OUTPUT_KEYS {
            let body = format!(r#"{{"recipe_version": 2, "output": {{"{key}": "x"}}}}"#);
            let err = check(&body, true).unwrap_err();
            assert!(
                err.contains(&format!("`output.{key}`")) && err.contains("output.display"),
                "{err}"
            );
        }
        check(
            r#"{"recipe_version": 2, "output": {"display": {"gamut": "adobe-rgb"}}}"#,
            true,
        )
        .unwrap();
        let err = check(
            r#"{"recipe_version": 2, "reconstruction": {"density": {"scale": [1, 1, 1]}}}"#,
            true,
        )
        .unwrap_err();
        assert!(err.contains("reconstruction.density") && err.contains("reconstruction.scale"));
        let err = check(
            r#"{"recipe_version": 2, "reconstruction": {"curve": {"type": "exponential"}}}"#,
            true,
        )
        .unwrap_err();
        assert!(err.contains("reconstruction.linearization"), "{err}");
        let err = check(
            r#"{"recipe_version": 2, "calibration": {"dmax": "fixed"}}"#,
            true,
        )
        .unwrap_err();
        assert!(err.contains("calibration.dmax") && err.contains("reference-free"));
        // Overlays get the same diagnosis.
        let err = check(r#"{"print": {"print_exposure": 1}}"#, false).unwrap_err();
        assert!(err.contains("`print`"), "{err}");
        // The chain that read it is gone, so the remedy is to drop it, never to run
        // something else.
        assert!(!err.contains("--new-flow"), "{err}");
    }

    /// No section may be named `params`: it is the envelope's key, and a recipe
    /// carrying one would be read as a sidecar.
    #[test]
    fn no_section_is_named_params() {
        let new = serde_json::to_value(Recipe::default()).unwrap();
        assert!(new.get("params").is_none());
    }

    /// The last default document the removed chain wrote (`pipeline_version` 7,
    /// `hanten params` at 5207460), keys only — the shape every archived sidecar and
    /// `--dump-params` file carries. Frozen: it describes files on disk, not code.
    fn removed_chains_document() -> serde_json::Value {
        serde_json::json!({
            "reconstruction": {
                "schema_version": 1,
                "density": {"scale": [1, 0.84, 0.73], "offset": [0, 0, 0]},
                "curve": {"gamma": 2.0, "anchor": {"mid-at-base-offset": 0.62}}
            },
            "input": {"transfer": "auto", "meaning": "auto", "film_type": "unknown", "export_ir": null},
            "calibration": {"film_base": null},
            "measure": {"inset": 0.05},
            "print": {
                "print_exposure": 0.0,
                "black_point": 0.0,
                "white_balance": {"explicit": [1, 1, 1]},
                "linear_range": [0, 1]
            },
            "fit_range": {"headroom_stops": 6.0},
            "output": {"preset": "gain-map-hdr"}
        })
    }

    /// The migration inventory: every section and key of the removed chain's recipe is
    /// either shared with this one or refused by name, so an archived recipe fails with
    /// where its knob went rather than an opaque "unknown field" — or, in a shared
    /// section, parses a key nothing reads.
    #[test]
    fn every_key_of_the_removed_chains_recipe_is_shared_or_diagnosed() {
        let old = removed_chains_document();
        let new = serde_json::to_value(Recipe::default()).unwrap();
        for (section, fields) in old.as_object().unwrap() {
            if SECTIONS_WITH_NO_COUNTERPART
                .iter()
                .any(|(s, _)| s == section)
            {
                assert!(new.get(section).is_none(), "{section}");
                continue;
            }
            let shared = new
                .get(section)
                .unwrap_or_else(|| panic!("`{section}` is neither shared nor diagnosed"));
            let table: &[(&str, &str)] = match section.as_str() {
                "reconstruction" => OLD_RECONSTRUCTION_KEYS,
                _ => &[],
            };
            for key in fields.as_object().unwrap().keys() {
                let diagnosed = table.iter().any(|(k, _)| k == key)
                    || (section == "output" && OLD_OUTPUT_KEYS.contains(&key.as_str()));
                assert!(
                    shared.get(key).is_some() != diagnosed,
                    "`{section}.{key}` must be exactly one of shared or diagnosed"
                );
            }
        }
    }

    #[test]
    fn validate_refuses_each_unusable_decode_value() {
        let with = |f: fn(&mut DecodeParams)| {
            let mut r = Recipe::default();
            f(&mut r.reconstruction);
            validate(&r, KnobNames::FlagAndKey).map_err(|e| e.message().to_string())
        };
        validate(&Recipe::default(), KnobNames::FlagAndKey).unwrap();
        assert!(
            with(|d| d.scale[1] = 0.0)
                .unwrap_err()
                .contains("reconstruction.scale")
        );
        assert!(
            with(|d| d.offset[2] = f32::NAN)
                .unwrap_err()
                .contains("reconstruction.offset")
        );
        assert!(
            with(|d| d.linearization = -1.0)
                .unwrap_err()
                .contains("reconstruction.linearization")
        );
        assert!(
            with(|d| d.anchor = AnchorRule::MidAboveBase(0.0))
                .unwrap_err()
                .contains("reconstruction.anchor")
        );
        // A linearization small enough that `0.745 / linearization` overflows.
        // The remedy follows the route: here the anchor itself overflows, so a
        // larger linearization is the fix…
        let err = with(|d| d.linearization = 1e-39).unwrap_err();
        assert!(err.contains("anchor is not usable"), "{err}");
        assert!(err.contains("Use a larger --density-gamma"), "{err}");
        // A finite anchor whose exponent still overflows.
        // …and here only the exponent does, where a larger linearization makes it worse.
        let err = with(|d| d.anchor = AnchorRule::MidAboveBase(2e38)).unwrap_err();
        assert!(err.contains("anchor is not usable"), "{err}");
        assert!(err.contains("Use a smaller --anchor-mid-offset"), "{err}");
        assert!(!err.contains("larger"), "{err}");
    }

    #[test]
    fn roll_names_the_key_alone() {
        // `roll` accepts no conversion flags, so naming one there is a remedy the
        // user cannot type.
        let mut r = Recipe::default();
        r.reconstruction.linearization = 0.0;
        let msg = validate(&r, KnobNames::KeyOnly).unwrap_err();
        let msg = msg.message();
        assert!(msg.contains("`reconstruction.linearization`"), "{msg}");
        assert!(!msg.contains("--density-gamma"), "{msg}");
        let mut r = Recipe::default();
        r.look.contrast = Some(0.0);
        let msg = validate(&r, KnobNames::KeyOnly).unwrap_err();
        let msg = msg.message();
        assert!(msg.contains("`look.contrast`"), "{msg}");
        assert!(!msg.contains("--contrast"), "{msg}");
    }

    #[test]
    fn validate_refuses_an_unusable_look_contrast() {
        for bad in [0.0, -1.1, f32::NAN, f32::INFINITY] {
            let mut r = Recipe::default();
            r.look.contrast = Some(bad);
            let msg = validate(&r, KnobNames::FlagAndKey).unwrap_err();
            assert!(
                msg.message()
                    .contains("--contrast (recipe `look.contrast`)"),
                "{bad}: {}",
                msg.message()
            );
        }
        let mut r = Recipe::default();
        r.look.contrast = Some(1.0);
        validate(&r, KnobNames::FlagAndKey).unwrap();
    }

    #[test]
    fn validate_refuses_an_unusable_channel_grade() {
        for bad in [
            [0.0, 1.0],
            [1.0, -0.2],
            [f32::NAN, 1.0],
            [2.0, 1.0],
            [1.5, 0.5],
        ] {
            let mut r = Recipe::default();
            r.look.channel_grade = bad;
            let msg = validate(&r, KnobNames::FlagAndKey).unwrap_err();
            let msg = msg.message();
            assert!(
                msg.contains("--channel-grade (recipe `look.channel_grade`)"),
                "{bad:?}: {msg}"
            );
            // The most specific rule speaks, not a neighbour's.
            assert!(!msg.contains("--contrast"), "{msg}");
            let msg = validate(&r, KnobNames::KeyOnly).unwrap_err();
            assert!(
                msg.message().contains("`look.channel_grade`")
                    && !msg.message().contains("--channel-grade"),
                "{}",
                msg.message()
            );
        }
        let mut r = Recipe::default();
        r.look.channel_grade = [1.4, 0.6];
        validate(&r, KnobNames::FlagAndKey).unwrap();
    }

    #[test]
    fn the_retired_contrast_key_is_refused_with_its_split() {
        // Refused at every value, the old default included: the key was both halves at
        // once, and no single new key replays it. The remedy states the look value
        // that keeps the stated slope as the whole contrast.
        for (json, look) in [
            (
                r#"{"recipe_version": 2, "reconstruction": {"contrast": 2.0}}"#,
                "1.11",
            ),
            (
                r#"{"recipe_version": 2, "reconstruction": {"contrast": 3.6}}"#,
                "2",
            ),
        ] {
            let err = check(json, true).unwrap_err();
            assert!(
                err.contains("reconstruction.linearization") && err.contains("look.contrast"),
                "{err}"
            );
            assert!(err.contains(&format!("`look.contrast`: {look}")), "{err}");
        }
        // A value whose quotient validation would refuse (zero, subnormal or infinite
        // in f32) gets the generic remedy, never a `look.contrast` it would then refuse.
        for json in [
            r#"{"recipe_version": 2, "reconstruction": {"contrast": "steep"}}"#,
            r#"{"recipe_version": 2, "reconstruction": {"contrast": 1e-50}}"#,
            r#"{"recipe_version": 2, "reconstruction": {"contrast": 1e-39}}"#,
            r#"{"recipe_version": 2, "reconstruction": {"contrast": 1e39}}"#,
            r#"{"recipe_version": 2, "reconstruction": {"contrast": -2.0}}"#,
        ] {
            let err = check(json, true).unwrap_err();
            assert!(err.contains("state `look.contrast`"), "{json}: {err}");
            assert!(!err.contains("write `look.contrast`"), "{json}: {err}");
        }
    }

    #[test]
    fn validate_refuses_a_whole_contrast_outside_f32() {
        // Each factor passes its own rule; the product, which highlight desaturation
        // divides by, does not. The remedy follows the direction.
        let with = |linearization: f32, contrast: f32, names| {
            let mut r = Recipe::default();
            r.reconstruction.linearization = linearization;
            r.look.contrast = Some(contrast);
            validate(&r, names).map_err(|e| (e.exit_code(), e.message().to_string()))
        };
        for (linearization, contrast, remedy) in [
            (1e30, 1e10, "smaller"),
            (1e-30, 1e-20, "larger"),
            (1e-30, 1e-9, "larger"), // subnormal, not zero
        ] {
            let (code, msg) = with(linearization, contrast, KnobNames::FlagAndKey).unwrap_err();
            assert_eq!(code, 2, "{msg}");
            assert!(
                msg.contains("--density-gamma (recipe `reconstruction.linearization`)")
                    && msg.contains("--contrast (recipe `look.contrast`)"),
                "{msg}"
            );
            assert!(msg.contains(&format!("Use a {remedy}")), "{msg}");
            let (_, msg) = with(linearization, contrast, KnobNames::KeyOnly).unwrap_err();
            assert!(
                msg.contains("`reconstruction.linearization`") && msg.contains("`look.contrast`"),
                "{msg}"
            );
            assert!(
                !msg.contains("--density-gamma") && !msg.contains("--contrast"),
                "{msg}"
            );
        }
        // The remedy works: bringing either factor back makes the product usable.
        with(1e30, 1.0, KnobNames::FlagAndKey).unwrap();
        with(1.8, 1e10, KnobNames::FlagAndKey).unwrap();
        with(1e-30, 1.0, KnobNames::FlagAndKey).unwrap();
    }

    #[test]
    fn validate_refuses_an_unusable_fit_range_headroom() {
        let with = |stops: f32, names| {
            let mut r = Recipe::default();
            r.fit_range.headroom_stops = Some(stops);
            validate(&r, names).map_err(|e| e.message().to_string())
        };
        with(0.0, KnobNames::FlagAndKey).unwrap();
        with(crate::types::MAX_HEADROOM_STOPS, KnobNames::FlagAndKey).unwrap();
        for bad in [-1.0, f32::NAN, f32::INFINITY] {
            let err = with(bad, KnobNames::FlagAndKey).unwrap_err();
            assert!(
                err.contains("--display-tone-headroom (recipe `fit_range.headroom_stops`)")
                    && err.contains("non-negative"),
                "{bad}: {err}"
            );
        }
        let err = with(25.0, KnobNames::KeyOnly).unwrap_err();
        assert!(err.contains("`fit_range.headroom_stops` is 25"), "{err}");
        assert!(!err.contains("--display-tone-headroom"), "{err}");
    }

    /// `convert` with `extra`, merged over the recipe `json`.
    fn merged(json: &str, extra: &[&str]) -> Recipe {
        use crate::cli::{Cli, Command};
        use clap::Parser;
        let argv = ["hanten", "convert", "in.tif", "-o", "out"]
            .iter()
            .chain(extra)
            .copied();
        let Command::Convert(args) = Cli::try_parse_from(argv).unwrap().command else {
            unreachable!()
        };
        merge(parse(json).unwrap(), &args)
    }

    #[test]
    fn the_roll_section_reaches_the_stages_and_a_stated_contrast_wins() {
        let roll = r#"{"recipe_version": 2,
                       "roll": {"white_balance": [0.8, 1.0, 1.25], "white_stops": 1.6}}"#;
        // The gains multiply the stated white balance; the white sets the contrast.
        let r = merged(roll, &["--white-balance", "1.25,1,1"]);
        let shared = r.shared_params();
        assert_eq!(
            shared.scene_correction.white_balance,
            WhiteBalance::Explicit([0.8 * 1.25, 1.0, 1.25])
        );
        let from_white = roll_white::contrast_for(1.6);
        assert_eq!(shared.look.section.contrast, from_white);
        assert_eq!(r.resolved_contrast(), (from_white, ContrastSource::Roll));
        // Alone, the gains reach the stage exactly: the identity multiplies nothing in.
        let alone = merged(roll, &[]).shared_params();
        assert_eq!(
            alone.scene_correction.white_balance,
            WhiteBalance::Explicit([0.8, 1.0, 1.25])
        );
        // A stated contrast wins — at any value, the default's included.
        let stated = merged(roll, &["--contrast", &DEFAULT_CONTRAST.to_string()]);
        assert_eq!(
            stated.resolved_contrast(),
            (DEFAULT_CONTRAST, ContrastSource::Stated)
        );
        // Neither: the default.
        assert_eq!(
            Recipe::default().resolved_contrast(),
            (DEFAULT_CONTRAST, ContrastSource::Base)
        );
        assert_eq!(
            Recipe::default().shared_params().look.section,
            LookSection::default()
        );
    }

    #[test]
    fn direct_starts_every_knob_from_its_pinned_base_and_leaves_the_roll_out() {
        use crate::rendering::DIRECT;
        let roll = r#""roll": {"white_balance": [0.8, 1.0, 1.25], "white_stops": 1.6}"#;
        let direct = merged(
            &format!(r#"{{"recipe_version": 2, {roll}}}"#),
            &["--rendering", "direct"],
        );
        let p = direct.shared_params();
        assert_eq!(
            p.scene_correction.white_balance,
            WhiteBalance::Explicit([1.0, 1.0, 1.0]),
            "the roll's gains are not applied"
        );
        assert_eq!(p.look.section.contrast, DIRECT.contrast, "nor its white");
        assert!(p.look.section.highlight_desaturation.is_off());
        assert_eq!(p.headroom_stops, DIRECT.headroom_stops);
        assert_eq!(p.display_black, DIRECT.display_black);
        let report = direct.roll_report(true).unwrap();
        assert!(!report.white_balance_applied && !report.contrast_applied);
        // `default` on the same recipe applies the roll, with every other default.
        let default = merged(&format!(r#"{{"recipe_version": 2, {roll}}}"#), &[]);
        let q = default.shared_params();
        assert_eq!(
            q.scene_correction.white_balance,
            WhiteBalance::Explicit([0.8, 1.0, 1.25])
        );
        assert_eq!(q.look.section.contrast, roll_white::contrast_for(1.6));
        assert_eq!(
            q.look.section.highlight_desaturation,
            HighlightDesaturation::DEFAULT
        );
        // A stated knob builds on either base: the white balance multiplies (the
        // identity under `direct`), every other knob replaces.
        let adjusted = merged(
            &format!(r#"{{"recipe_version": 2, {roll}}}"#),
            &[
                "--rendering",
                "direct",
                "--white-balance",
                "1.1,1,1",
                "--contrast",
                "1.3",
                "--highlight-desaturation",
                "0.5",
                "--display-black",
                "off",
            ],
        )
        .shared_params();
        assert_eq!(
            adjusted.scene_correction.white_balance,
            WhiteBalance::Explicit([1.1, 1.0, 1.0])
        );
        assert_eq!(adjusted.look.section.contrast, 1.3);
        assert_eq!(adjusted.look.section.highlight_desaturation.strength, 0.5);
        assert_eq!(adjusted.display_black, DisplayBlack::Off);
        assert_eq!(adjusted.headroom_stops, DIRECT.headroom_stops);
    }

    #[test]
    fn direct_defaults_the_destination_to_the_hdr_float_tiff() {
        use destination::{Container, Gamut, Range, Transfer};
        let resolved = |extra: &[&str]| {
            let r = merged(
                r#"{"recipe_version": 2}"#,
                &[&["--rendering", "direct"][..], extra].concat(),
            );
            match destination(&r, KnobNames::FlagAndKey).unwrap() {
                Destination::Display(d) => (d.range, d.transfer, d.gamut, d.container),
                Destination::FilmMaster => unreachable!(),
            }
        };
        assert_eq!(
            resolved(&[]),
            (Range::Hdr, Transfer::Linear, Gamut::Bt2020, Container::Tiff)
        );
        // Stated SDR: Adobe RGB, its gamut when a row has it.
        assert_eq!(
            resolved(&["--range", "sdr"]),
            (
                Range::Sdr,
                Transfer::Native,
                Gamut::AdobeRgb,
                Container::Tiff
            )
        );
        // The container is decided first, so a stated axis that rules the float TIFF
        // out falls back to the lossless 16-bit TIFF, never the lossy gain-map JPEG — and
        // a stated axis is never overridden by `direct`'s defaults.
        assert_eq!(
            resolved(&["--gamut", "display-p3"]),
            (
                Range::Sdr,
                Transfer::Native,
                Gamut::DisplayP3,
                Container::Tiff
            )
        );
        assert_eq!(
            resolved(&["--transfer", "native"]),
            (
                Range::Sdr,
                Transfer::Native,
                Gamut::AdobeRgb,
                Container::Tiff
            )
        );
        assert_eq!(
            resolved(&["--range", "hdr"]),
            (Range::Hdr, Transfer::Linear, Gamut::Bt2020, Container::Tiff)
        );
        assert_eq!(
            resolved(&["--transfer", "pq"]),
            (Range::Hdr, Transfer::Pq, Gamut::Bt2020, Container::Tiff)
        );
        // Only a stated container reaches a lossy one.
        assert_eq!(
            resolved(&["--container", "jpeg"]),
            (
                Range::Hdr,
                Transfer::Native,
                Gamut::DisplayP3,
                Container::Jpeg
            )
        );
        // `default` keeps the standard defaults and order, where the same stated gamut
        // is the gain map.
        let standard = |extra: &[&str]| {
            let r = merged(r#"{"recipe_version": 2}"#, extra);
            match destination(&r, KnobNames::FlagAndKey).unwrap() {
                Destination::Display(d) => (d.range, d.transfer, d.gamut, d.container),
                Destination::FilmMaster => unreachable!(),
            }
        };
        assert_eq!(
            standard(&[]),
            (
                Range::Sdr,
                Transfer::Native,
                Gamut::DisplayP3,
                Container::Tiff
            )
        );
        assert_eq!(
            standard(&["--range", "hdr", "--gamut", "display-p3"]),
            (
                Range::Hdr,
                Transfer::Native,
                Gamut::DisplayP3,
                Container::Jpeg
            )
        );
    }

    #[test]
    fn direct_is_refused_with_the_film_master_and_default_is_spared() {
        let r = merged(
            r#"{"recipe_version": 2}"#,
            &["--rendering", "direct", "--film-master"],
        );
        let msg = destination(&r, KnobNames::FlagAndKey)
            .unwrap_err()
            .message()
            .to_string();
        assert!(
            msg.contains("--rendering direct") && msg.contains("--film-master"),
            "{msg}"
        );
        // The stage rule's wording is not what fired: this is the more specific one.
        assert!(!msg.contains("cannot apply"), "{msg}");
        let r: Recipe =
            parse(r#"{"recipe_version": 2, "rendering": "direct", "output": "film-master"}"#)
                .unwrap();
        let msg = destination(&r, KnobNames::KeyOnly)
            .unwrap_err()
            .message()
            .to_string();
        assert!(
            msg.contains("`rendering` \"direct\"") && !msg.contains("--"),
            "{msg}"
        );
        assert!(
            msg.contains("set `rendering` to \"default\" (or remove it)"),
            "{msg}"
        );
        // The flag remedy works whether a flag or the recipe stated `direct`.
        for (recipe, flags) in [
            (r#"{"recipe_version": 2}"#, &["--rendering", "direct"][..]),
            (r#"{"recipe_version": 2, "rendering": "direct"}"#, &[][..]),
        ] {
            let r = merged(recipe, &[flags, &["--film-master"]].concat());
            let msg = destination(&r, KnobNames::FlagAndKey)
                .unwrap_err()
                .message()
                .to_string();
            assert!(msg.contains("pass --rendering default"), "{msg}");
            // Passed instead of a typed `--rendering direct`, over the recipe's otherwise.
            let r = merged(recipe, &["--film-master", "--rendering", "default"]);
            assert_eq!(
                destination(&r, KnobNames::FlagAndKey).unwrap(),
                Destination::FilmMaster
            );
        }
        let r = merged(
            r#"{"recipe_version": 2}"#,
            &["--rendering", "default", "--film-master"],
        );
        assert_eq!(
            destination(&r, KnobNames::FlagAndKey).unwrap(),
            Destination::FilmMaster
        );
    }

    #[test]
    fn direct_warns_when_a_recipe_moves_its_pinned_base() {
        // An earlier build wrote every default into a recipe: under `direct`, its
        // highlight desaturation 0.8 would turn the pull back on unnoticed.
        let old = r#"{"recipe_version": 2, "rendering": "direct",
            "look": {"contrast": 1.1111112,
                     "highlight_desaturation": {"strength": 0.8, "start_stops": -1.0,
                                                "band": [0.015, 0.025]}},
            "fit_range": {"headroom_stops": 6.0, "display_black": 6.0}}"#;
        let w = parse(old).unwrap().recipe_warnings(TypedStyle::default());
        assert_eq!(w.len(), 1, "{w:?}");
        // Only what moves the base is named: the contrast, headroom and black match it.
        assert!(
            w[0].contains("`look.highlight_desaturation.strength` 0.8")
                && !w[0].contains("`look.contrast`")
                && !w[0].contains("headroom")
                && w[0].contains("`null`"),
            "{}",
            w[0]
        );
        assert!(!w[0].contains("--"), "recipe keys only: {}", w[0]);
        // Typed, it is a choice made now.
        let typed = TypedStyle {
            highlight_desaturation_strength: true,
            ..TypedStyle::default()
        };
        assert!(parse(old).unwrap().recipe_warnings(typed).is_empty());
        // A deliberate value — anything but the old serialized default — is what a
        // `--dump-params` recipe replays, so it must pass `--strict`: no warning.
        for deliberate in [
            r#"{"recipe_version": 2, "rendering": "direct",
                "look": {"highlight_desaturation": {"strength": 0.5, "start_stops": -2.0,
                                                    "band": [0.01, 0.03]}},
                "fit_range": {"headroom_stops": 4.0, "display_black": "off"}}"#,
            // A contrast or white balance with no roll section is the user's own.
            r#"{"recipe_version": 2, "rendering": "direct", "look": {"contrast": 1.3},
                "scene_correction": {"white_balance": {"explicit": [1.1, 1, 1]}}}"#,
        ] {
            let w = parse(deliberate)
                .unwrap()
                .recipe_warnings(TypedStyle::default());
            assert!(w.is_empty(), "{deliberate}: {w:?}");
        }
        // Beside a roll section, a stated contrast or white balance is what an earlier
        // `measure-roll` wrote, and `direct` would apply it although it leaves the roll
        // out. A contrast at `direct`'s own base moves nothing.
        let roll = r#"{"recipe_version": 2, "rendering": "direct",
            "roll": {"white_balance": [1.2, 1, 0.9], "white_stops": 1.7},
            "look": {"contrast": 1.3},
            "scene_correction": {"white_balance": {"explicit": [1.2, 1, 0.9]}}}"#;
        let w = parse(roll).unwrap().recipe_warnings(TypedStyle::default());
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(
            w[0].contains("`scene_correction.white_balance` [1.2, 1.0, 0.9] beside a `roll`")
                && w[0].contains("`look.contrast` 1.3 beside a `roll` section"),
            "{}",
            w[0]
        );
        let at_base = roll.replace("1.3", "1.1111112");
        let w = parse(&at_base)
            .unwrap()
            .recipe_warnings(TypedStyle::default());
        assert!(!w[0].contains("`look.contrast` 1.1111112"), "{}", w[0]);
        // The remedy never tells a deliberate adjuster to drop the value.
        assert!(
            w[0].contains("If a value beside the `roll` section came from an earlier")
                && w[0].contains("type it as a flag to keep it without this warning"),
            "{}",
            w[0]
        );
        let typed = TypedStyle {
            white_balance: true,
            contrast: true,
            ..TypedStyle::default()
        };
        assert!(parse(roll).unwrap().recipe_warnings(typed).is_empty());
        // `direct` never reads the roll, so it has no fallback to warn about.
        let bare = r#"{"recipe_version": 2, "rendering": "direct"}"#;
        assert!(
            parse(bare)
                .unwrap()
                .recipe_warnings(TypedStyle::default())
                .is_empty()
        );
    }

    #[test]
    fn default_without_a_roll_measurement_says_what_fell_back() {
        let w = Recipe::default().recipe_warnings(TypedStyle::default());
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(
            w[0].contains("no roll measurement")
                && w[0].contains("neutral white balance (no `roll.white_balance`)")
                && w[0].contains(&format!(
                    "the fallback contrast {} (no `roll.white_stops`)",
                    DEFAULT_CONTRAST
                )),
            "{}",
            w[0]
        );
        // All three remedies, as recipe keys.
        assert!(
            w[0].contains("`hanten measure-roll`")
                && w[0].contains("`scene_correction.white_balance`, `look.contrast`")
                && w[0].contains("`rendering`: \"direct\"")
                && w[0].contains("HDR float TIFF")
                && !w[0].contains("--"),
            "{}",
            w[0]
        );
        // A typed flag is a choice, even the identity: it silences its own half.
        let typed = TypedStyle {
            white_balance: true,
            ..TypedStyle::default()
        };
        let w = Recipe::default().recipe_warnings(typed);
        assert!(
            w.len() == 1 && !w[0].contains("white balance (") && w[0].contains("contrast"),
            "{w:?}"
        );
        let typed = TypedStyle {
            white_balance: true,
            contrast: true,
            ..TypedStyle::default()
        };
        let mut r = Recipe::default();
        r.look.contrast = Some(1.3);
        assert!(r.recipe_warnings(typed).is_empty());
    }

    #[test]
    fn the_roll_flags_land_in_the_roll_section() {
        let r = merged(
            r#"{"recipe_version": 2, "roll": {"white_balance": [0.9, 1.0, 1.1], "white_stops": 1.5}}"#,
            &["--roll-white", "1.8"],
        );
        // One flag replaces its own key and leaves the other.
        assert_eq!(
            r.roll,
            RollSection {
                white_balance: Some([0.9, 1.0, 1.1]),
                white_stops: Some(1.8),
                frames: BTreeMap::new(),
            }
        );
        let r = merged(
            r#"{"recipe_version": 2}"#,
            &["--roll-white-balance", "1.2,1,0.8"],
        );
        assert_eq!(r.roll.white_balance, Some([1.2, 1.0, 0.8]));
        assert_eq!(r.roll.white_stops, None);
    }

    #[test]
    fn the_roll_values_are_refused_by_their_own_names() {
        let refusal = |json: &str, names| {
            let r: Recipe = parse(json).unwrap();
            validate(&r, names).unwrap_err().message().to_string()
        };
        for stops in ["0", "-1.5"] {
            let msg = refusal(
                &format!(r#"{{"recipe_version": 2, "roll": {{"white_stops": {stops}}}}}"#),
                KnobNames::FlagAndKey,
            );
            assert!(
                msg.contains("--roll-white (recipe `roll.white_stops`)"),
                "{msg}"
            );
        }
        let msg = refusal(
            r#"{"recipe_version": 2, "roll": {"white_balance": [1, 0, 1]}}"#,
            KnobNames::KeyOnly,
        );
        assert!(
            msg.contains("`roll.white_balance`") && !msg.contains("--"),
            "{msg}"
        );
        // A contrast the roll's white gives, too small to survive the whole contrast:
        // named as the white, not as `--contrast`, which the user never typed.
        let msg = refusal(
            r#"{"recipe_version": 2, "roll": {"white_stops": 1e38},
                "reconstruction": {"linearization": 1e-8}}"#,
            KnobNames::FlagAndKey,
        );
        assert!(msg.contains("from --roll-white"), "{msg}");
        assert!(!msg.contains("--contrast"), "{msg}");
        // The remedy moves the white the other way: the contrast is inverse to it.
        assert!(
            msg.contains(
                "Use a larger --density-gamma (recipe `reconstruction.linearization`) or a \
                 smaller --roll-white (recipe `roll.white_stops`)"
            ),
            "{msg}"
        );
        assert!(!msg.contains("value for either"), "{msg}");
        // Overflow, in `roll`'s key-only spelling: the reverse remedy.
        let msg = refusal(
            r#"{"recipe_version": 2, "roll": {"white_stops": 1e-30},
                "reconstruction": {"linearization": 1e10}}"#,
            KnobNames::KeyOnly,
        );
        assert!(msg.contains("overflows"), "{msg}");
        assert!(
            msg.contains(
                "Use a smaller `reconstruction.linearization` or a larger `roll.white_stops`"
            ),
            "{msg}"
        );
        assert!(
            !msg.contains("value for either") && !msg.contains("--"),
            "{msg}"
        );
        // A white so small its contrast is not finite: no identity hint, which is about
        // a stated contrast.
        let msg = refusal(
            r#"{"recipe_version": 2, "roll": {"white_stops": 1e-45}}"#,
            KnobNames::FlagAndKey,
        );
        assert!(
            msg.contains("--roll-white (recipe `roll.white_stops`) must give a finite")
                && msg.contains("use a larger white"),
            "{msg}"
        );
        assert!(!msg.contains("identity"), "{msg}");
        // A product only the roll's gains make is named with both factors.
        let msg = refusal(
            r#"{"recipe_version": 2, "roll": {"white_balance": [1e30, 1, 1]},
                "scene_correction": {"white_balance": {"explicit": [1e10, 1, 1]}}}"#,
            KnobNames::FlagAndKey,
        );
        assert!(
            msg.contains("--roll-white-balance") && msg.contains("--white-balance"),
            "{msg}"
        );
    }

    #[test]
    fn a_recipe_stated_style_value_that_meets_a_roll_measurement_warns() {
        let warnings_typed = |json: &str, typed| parse(json).unwrap().roll_overlap_warnings(typed);
        let warnings = |json: &str| warnings_typed(json, TypedStyle::default());
        // Both overlaps: the stated gains multiply the roll's, and the stated contrast
        // wins over the roll's white.
        let overlapping = r#"{"recipe_version": 2,
                "roll": {"white_balance": [1.25, 1, 0.5], "white_stops": 1.7},
                "scene_correction": {"white_balance": {"explicit": [2, 1, 2]}},
                "look": {"contrast": 1.1111112}}"#;
        let both = warnings(overlapping);
        assert_eq!(both.len(), 2, "{both:?}");
        assert!(
            both[0].contains("the recipe's `scene_correction.white_balance` [2.0, 1.0, 2.0]")
                && both[0].contains("`roll.white_balance` [1.25, 1.0, 0.5]")
                && both[0].contains("the white balance applied is [2.5, 1.0, 1.0]")
                && both[0].contains("drop it"),
            "{}",
            both[0]
        );
        let roll = roll_white::contrast_for(1.7);
        assert!(
            both[1].contains("the recipe's `look.contrast` 1.1111112 overrides the roll's")
                && both[1].contains(&roll.to_string())
                && both[1].contains("`roll.white_stops` 1.7")
                && both[1].contains("`null`"),
            "{}",
            both[1]
        );
        // Recipe keys only: only a recipe value warns.
        assert!(both.iter().all(|w| !w.contains("--")), "{both:?}");
        // A typed flag is a choice made now: it silences its own warning, not the other.
        let wb_typed = TypedStyle {
            white_balance: true,
            ..TypedStyle::default()
        };
        let only = warnings_typed(overlapping, wb_typed);
        assert_eq!(only, vec![both[1].clone()]);
        let contrast_typed = TypedStyle {
            contrast: true,
            ..TypedStyle::default()
        };
        assert_eq!(
            warnings_typed(overlapping, contrast_typed),
            vec![both[0].clone()]
        );
        // The film master renders nothing, so the run's recipe warnings are empty.
        let master = overlapping.replacen('{', r#"{"output": "film-master","#, 1);
        assert_eq!(
            parse(&master)
                .unwrap()
                .recipe_warnings(TypedStyle::default()),
            Vec::<String>::new()
        );
        // No overlap, no warning.
        for quiet in [
            // No roll section.
            r#"{"recipe_version": 2,
                "scene_correction": {"white_balance": {"explicit": [2, 1, 2]}},
                "look": {"contrast": 1.3}}"#,
            // The roll's gains over the identity; the roll's white, contrast unstated.
            r#"{"recipe_version": 2,
                "roll": {"white_balance": [1.25, 1, 0.5], "white_stops": 1.7},
                "scene_correction": {"white_balance": {"explicit": [1, 1, 1]}},
                "look": {"contrast": null}}"#,
            // A stated value beside the *other* roll measurement.
            r#"{"recipe_version": 2, "roll": {"white_stops": 1.7},
                "scene_correction": {"white_balance": {"explicit": [2, 1, 2]}}}"#,
            r#"{"recipe_version": 2, "roll": {"white_balance": [1.25, 1, 0.5]},
                "look": {"contrast": 1.3}}"#,
        ] {
            assert_eq!(warnings(quiet), Vec::<String>::new(), "{quiet}");
        }
    }

    #[test]
    fn the_film_master_leaves_the_roll_section_unapplied_and_says_so() {
        let r: Recipe = parse(
            r#"{"recipe_version": 2, "output": "film-master",
                "roll": {"white_balance": [0.8, 1.0, 1.25], "white_stops": 1.6}}"#,
        )
        .unwrap();
        // A measurement is not a stage the user asked for, so it is not refused.
        assert_eq!(
            destination(&r, KnobNames::FlagAndKey).unwrap(),
            Destination::FilmMaster
        );
        let report = r.roll_report(false).unwrap();
        assert!(!report.white_balance_applied && !report.contrast_applied);
        assert_eq!(report.contrast, Some(roll_white::contrast_for(1.6)));
        let rendered = r.roll_report(true).unwrap();
        assert!(rendered.white_balance_applied && rendered.contrast_applied);
        // A stated contrast wins, and the report says the roll's did not apply.
        let mut stated = r.clone();
        stated.look.contrast = Some(1.3);
        assert!(!stated.roll_report(true).unwrap().contrast_applied);
        assert_eq!(Recipe::default().roll_report(true), None);
    }

    #[test]
    fn an_axis_flag_over_a_recipe_film_master_states_only_its_own_axes() {
        // The flag chose a rendered destination; the recipe stated none of its axes.
        let r = merged(
            r#"{"recipe_version": 2, "output": "film-master"}"#,
            &["--transfer", "pq"],
        );
        assert_eq!(
            r.output,
            OutputSection::Display(DisplayAxes {
                transfer: Some(destination::Transfer::Pq),
                ..DisplayAxes::default()
            })
        );
    }

    #[test]
    fn the_film_master_flag_replaces_a_recipes_axes() {
        let r = merged(
            r#"{"recipe_version": 2, "output": {"display": {"transfer": "pq", "container": "avif"}}}"#,
            &["--film-master"],
        );
        assert_eq!(r.output, OutputSection::FilmMaster);
        assert_eq!(
            destination(&r, KnobNames::FlagAndKey).unwrap(),
            Destination::FilmMaster
        );
    }

    #[test]
    fn an_axis_flag_joins_the_recipes_other_axes() {
        // Flags win per axis, and the recipe's other axes stay stated: a flag that
        // contradicts one of them is a conflict naming both, never a silent override.
        let r = merged(
            r#"{"recipe_version": 2, "output": {"display": {"gamut": "adobe-rgb"}}}"#,
            &["--transfer", "pq"],
        );
        assert_eq!(
            r.output,
            OutputSection::Display(DisplayAxes {
                transfer: Some(destination::Transfer::Pq),
                gamut: Some(destination::Gamut::AdobeRgb),
                ..DisplayAxes::default()
            })
        );
        let err = destination(&r, KnobNames::FlagAndKey).unwrap_err();
        let msg = err.message();
        assert!(msg.contains("--transfer pq and --gamut adobe-rgb"), "{msg}");
        // The same axis stated twice: the flag wins.
        let r = merged(
            r#"{"recipe_version": 2, "output": {"display": {"gamut": "adobe-rgb"}}}"#,
            &["--gamut", "display-p3"],
        );
        let Destination::Display(d) = destination(&r, KnobNames::FlagAndKey).unwrap() else {
            panic!("expected a rendered destination")
        };
        assert_eq!(d.gamut, destination::Gamut::DisplayP3);
    }

    #[test]
    fn the_film_master_refuses_each_stage_it_does_not_run_by_its_keys_on_roll() {
        let master = || Recipe {
            output: OutputSection::FilmMaster,
            ..Recipe::default()
        };
        assert_eq!(
            destination(&master(), KnobNames::KeyOnly).unwrap(),
            Destination::FilmMaster
        );
        let mut r = master();
        r.scene_correction.exposure = 1.0;
        r.fit_range.headroom_stops = Some(3.0);
        let msg = destination(&r, KnobNames::KeyOnly).unwrap_err();
        let msg = msg.message();
        assert!(
            msg.contains("scene correction (`scene_correction`)")
                && msg.contains("fit range (`fit_range`)"),
            "{msg}"
        );
        assert!(!msg.contains("--"), "a roll names keys, not flags: {msg}");
        // The identity scene correction and fit range are spared.
        let mut r = master();
        r.scene_correction.white_balance = WhiteBalance::Explicit([2.0, 2.0, 2.0]);
        r.scene_correction.exposure = -1.0;
        r.fit_range.headroom_stops = Some(0.0);
        destination(&r, KnobNames::FlagAndKey).unwrap();
        let fit = |headroom: f32, black: DisplayBlack| FitRangeSection {
            headroom_stops: Some(headroom),
            display_black: Some(black),
        };
        assert!(!FitRangeSection::default().asks_for_a_fit());
        assert!(!fit(0.0, DisplayBlack::Off).asks_for_a_fit());
        assert!(fit(5.0, DisplayBlack::default()).asks_for_a_fit());
        assert!(fit(0.0, DisplayBlack::StopsBelowMid(5.0)).asks_for_a_fit());
    }

    #[test]
    fn validate_refuses_an_unusable_display_black() {
        let with = |black: DisplayBlack, names| {
            let mut r = Recipe::default();
            r.fit_range.display_black = Some(black);
            validate(&r, names).map_err(|e| e.message().to_string())
        };
        with(DisplayBlack::Off, KnobNames::FlagAndKey).unwrap();
        with(
            DisplayBlack::StopsBelowMid(MAX_DISPLAY_BLACK_STOPS),
            KnobNames::FlagAndKey,
        )
        .unwrap();
        for bad in [0.0, -1.0, 16.5, f32::NAN, f32::INFINITY] {
            let err = with(DisplayBlack::StopsBelowMid(bad), KnobNames::FlagAndKey).unwrap_err();
            assert!(
                err.contains("--display-black (recipe `fit_range.display_black`)")
                    && err.contains("`off`"),
                "{bad}: {err}"
            );
        }
        let err = with(DisplayBlack::StopsBelowMid(-1.0), KnobNames::KeyOnly).unwrap_err();
        assert!(err.contains("`fit_range.display_black`"), "{err}");
        assert!(!err.contains("--display-black"), "{err}");
    }

    #[test]
    fn display_black_reads_off_from_a_recipe() {
        let r = parse(r#"{"recipe_version": 2, "fit_range": {"display_black": "off"}}"#).unwrap();
        assert_eq!(r.fit_range.display_black, Some(DisplayBlack::Off));
        let r = parse(r#"{"recipe_version": 2, "fit_range": {"display_black": 5}}"#).unwrap();
        assert_eq!(
            r.fit_range.display_black,
            Some(DisplayBlack::StopsBelowMid(5.0))
        );
        assert!(parse(r#"{"recipe_version": 2, "fit_range": {"display_black": "none"}}"#).is_err());
    }

    #[test]
    fn the_destination_states_fit_ranges_peak() {
        // The recipe carries the headroom; the peak comes from the destination, so a
        // recipe cannot name a peak its destination does not have.
        let mut r = Recipe::default();
        r.fit_range.headroom_stops = Some(4.0);
        let p = r.chain_params(DisplayPeak::SDR, DestinationGamut::DisplayP3);
        assert_eq!(p.shared.headroom_stops, 4.0);
        assert_eq!(p.target.peak, DisplayPeak::SDR);
        let err = parse(r#"{"recipe_version": 2, "fit_range": {"peak": 4.9}}"#)
            .unwrap_err()
            .to_string();
        assert!(err.contains("peak"), "{err}");
    }

    #[test]
    fn a_key_retired_earlier_points_at_its_home() {
        // Keys retired before the removed chain was still name their homes here, rather
        // than falling to an opaque "unknown field".
        for (json, needles) in [
            (
                r#"{"recipe_version": 2, "density": {"scale": [1, 1, 1]}}"#,
                &["`density`", "`reconstruction.scale`"][..],
            ),
            (
                r#"{"recipe_version": 2, "film_base": {"source": "auto"}}"#,
                &["`film_base`", "`calibration.film_base`"],
            ),
            (
                r#"{"recipe_version": 2, "algorithm": "density"}"#,
                &["`algorithm`"],
            ),
            (r#"{"recipe_version": 2, "sigmoid": {}}"#, &["`sigmoid`"]),
            (r#"{"recipe_version": 2, "simple": {}}"#, &["`simple`"]),
            (
                r#"{"recipe_version": 2, "input": {"color": "linear"}}"#,
                &["`input.color`", "`input.transfer`"],
            ),
        ] {
            let err = check(json, true).unwrap_err();
            for needle in needles {
                assert!(err.contains(needle), "{json}: {err}");
            }
            // An overlay gets the same diagnosis.
            let overlay = json.replace(r#""recipe_version": 2, "#, "");
            assert!(check(&overlay, false).unwrap_err().contains(needles[0]));
        }
    }

    /// `convert` flags that are not conversion knobs, so they owe the recipe nothing:
    /// operational flags (never a recipe key), and plumbing (paths and the recipe
    /// itself, not settings inside it).
    const NON_KNOB_FLAGS: &[&str] = &[
        // Refused by `cli::reject_deprecated_input_flags` before this merge runs.
        "--input-profile",
        "--output",
        "--params",
        "--dump-params",
        "--strict",
        "--seed",
        "--telemetry",
        "--telemetry-file",
        "--max-memory",
        "--report",
        "--report-file",
        "--verbose",
        "--quiet",
        "--help",
    ];

    /// Every visible `convert` flag that is a conversion knob, read off clap itself —
    /// the alternative is a hand-kept list sitting beside the flags. Hidden flags are
    /// removed ones, which `cli`'s removed-flag check refuses before this merge runs.
    fn convert_knob_flags() -> std::collections::BTreeSet<String> {
        use clap::CommandFactory;
        let cli = crate::cli::Cli::command();
        let convert = cli
            .find_subcommand("convert")
            .expect("`convert` is a subcommand");
        convert
            .get_arguments()
            .filter(|arg| !arg.is_hide_set())
            .filter_map(|arg| arg.get_long().map(|long| format!("--{long}")))
            .filter(|flag| !NON_KNOB_FLAGS.contains(&flag.as_str()))
            .collect()
    }

    type Landed = fn(&Recipe) -> bool;

    /// One command line per knob flag, each setting a non-default value, and the one
    /// field of the recipe it must land in — "the recipe changed" alone would pass a
    /// flag wired to the wrong knob.
    fn knob_samples() -> &'static [(&'static str, &'static [&'static str], Landed)] {
        use crate::destination::{Container, Gamut, Range, Transfer};
        use crate::types::{FilmType, MeaningAssertion, TransferAssertion};
        &[
            ("--input-transfer", &["--input-transfer", "linear"], |r| {
                r.input.transfer == TransferAssertion::Linear
            }),
            (
                "--input-meaning",
                &["--input-meaning", "scanner-device"],
                |r| r.input.meaning == MeaningAssertion::ScannerDevice,
            ),
            ("--film-type", &["--film-type", "silver"], |r| {
                r.input.film_type == FilmType::Silver
            }),
            ("--film-base", &["--film-base", "0.5,0.4,0.3"], |r| {
                matches!(r.calibration.film_base, Some(FilmBaseSource::Explicit(_)))
            }),
            ("--base-region", &["--base-region", "0,0,10,10"], |r| {
                matches!(r.calibration.film_base, Some(FilmBaseSource::Region(_)))
            }),
            ("--export-ir", &["--export-ir", "ir.tiff"], |r| {
                r.input.export_ir.as_deref() == Some("ir.tiff")
            }),
            ("--measure-inset", &["--measure-inset", "0.1"], |r| {
                r.measure.inset == 0.1
            }),
            ("--density-scale", &["--density-scale", "1,0.9,0.8"], |r| {
                r.reconstruction.scale == [1.0, 0.9, 0.8]
            }),
            ("--density-offset", &["--density-offset", "0,0.1,0"], |r| {
                r.reconstruction.offset == [0.0, 0.1, 0.0]
            }),
            ("--density-gamma", &["--density-gamma", "1.9"], |r| {
                r.reconstruction.linearization == 1.9
            }),
            (
                "--anchor-mid-offset",
                &["--anchor-mid-offset", "0.5"],
                |r| r.reconstruction.anchor == AnchorRule::MidAboveBase(0.5),
            ),
            ("--white-balance", &["--white-balance", "1.1,1,0.9"], |r| {
                r.scene_correction.white_balance == WhiteBalance::Explicit([1.1, 1.0, 0.9])
            }),
            ("--exposure", &["--exposure", "-0.5"], |r| {
                r.scene_correction.exposure == -0.5
            }),
            ("--rendering", &["--rendering", "direct"], |r| {
                r.rendering == Rendering::Direct
            }),
            (
                "--roll-white-balance",
                &["--roll-white-balance", "1.1,1,0.9"],
                |r| r.roll.white_balance == Some([1.1, 1.0, 0.9]),
            ),
            ("--roll-white", &["--roll-white", "1.7"], |r| {
                r.roll.white_stops == Some(1.7)
            }),
            ("--contrast", &["--contrast", "1.3"], |r| {
                r.look.contrast == Some(1.3)
            }),
            ("--channel-grade", &["--channel-grade", "1.1,0.9"], |r| {
                r.look.channel_grade == [1.1, 0.9]
            }),
            (
                "--highlight-desaturation",
                &["--highlight-desaturation", "0.5"],
                |r| r.look.highlight_desaturation.strength == Some(0.5),
            ),
            (
                "--highlight-desaturation-start",
                &["--highlight-desaturation-start", "-2"],
                |r| r.look.highlight_desaturation.start_stops == Some(-2.0),
            ),
            (
                "--highlight-desaturation-band",
                &["--highlight-desaturation-band", "0.01,0.03"],
                |r| r.look.highlight_desaturation.band == Some([0.01, 0.03]),
            ),
            ("--range", &["--range", "hdr"], |r| {
                r.output == display(|a| a.range = Some(Range::Hdr))
            }),
            ("--transfer", &["--transfer", "pq"], |r| {
                r.output == display(|a| a.transfer = Some(Transfer::Pq))
            }),
            ("--gamut", &["--gamut", "adobe-rgb"], |r| {
                r.output == display(|a| a.gamut = Some(Gamut::AdobeRgb))
            }),
            ("--container", &["--container", "avif"], |r| {
                r.output == display(|a| a.container = Some(Container::Avif))
            }),
            ("--film-master", &["--film-master"], |r| {
                r.output == OutputSection::FilmMaster
            }),
            (
                "--display-tone-headroom",
                &["--display-tone-headroom", "4"],
                |r| r.fit_range.headroom_stops == Some(4.0),
            ),
            ("--display-black", &["--display-black", "5"], |r| {
                r.fit_range.display_black == Some(DisplayBlack::StopsBelowMid(5.0))
            }),
        ]
    }

    /// A rendered destination with the axes `set` states.
    fn display(set: fn(&mut DisplayAxes)) -> OutputSection {
        let mut axes = DisplayAxes::default();
        set(&mut axes);
        OutputSection::Display(axes)
    }

    /// Every conversion flag reaches its own recipe field — a flag with no arm in
    /// [`merge`] would parse and do nothing, the accepted-and-ignored defect. Driven
    /// over the flag surface clap reports, so a flag added without a sample reds, and
    /// a sample for a flag that no longer exists reds too.
    #[test]
    fn every_flag_reaches_the_recipe() {
        use crate::cli::{Cli, Command};
        use clap::Parser;
        let surface = convert_knob_flags();
        let sampled: std::collections::BTreeSet<String> = knob_samples()
            .iter()
            .map(|(flag, _, _)| flag.to_string())
            .collect();
        assert_eq!(
            surface, sampled,
            "every visible conversion flag needs a sample here, and every sample a flag"
        );
        for (flag, extra, landed) in knob_samples() {
            let argv = ["hanten", "convert", "in.tif", "-o", "out"]
                .iter()
                .chain(extra.iter())
                .copied();
            let Command::Convert(args) = Cli::try_parse_from(argv).unwrap().command else {
                unreachable!()
            };
            let merged = merge(Recipe::default(), &args);
            assert!(
                landed(&merged),
                "{flag} did not land in its field: {merged:?}"
            );
            // Falsifiability: the default does not already satisfy the check.
            assert!(!landed(&Recipe::default()), "{flag}'s check is vacuous");
        }
    }
}

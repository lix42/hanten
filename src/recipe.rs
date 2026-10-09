//! The recipe (`nf-core/recipe-schema`).
//!
//! One document, versioned **as a document**: a recipe states `"recipe_version": 3` at
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

mod compose;

pub(crate) use compose::is_variant_switch;
pub use compose::{compose, merge_json};

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::algo::fixed::{
    self, AnchorRule, DENSITY_OFFSET, DENSITY_SCALE, DecodeFault, DecodeParams, LINEARIZATION,
    MID_ABOVE_BASE, SCAN_FLOOR,
};
use crate::destination::{self, Change, DisplayAxes, Fault, OutputSection, Resolved};
use crate::pipeline::chain::{self, ChainParams, DisplayTarget, SharedParams};
use crate::pipeline::colorimetry::dot;
use crate::pipeline::colorimetry::pinned::ACESCG_LUMA;
use crate::pipeline::fit_gamut::DestinationGamut;
use crate::pipeline::fit_range::{
    DisplayBlack, DisplayBlackFault, DisplayPeak, MAX_DISPLAY_BLACK_STOPS,
};
use crate::pipeline::look::{
    ChannelGradeFault, DesaturationFault, HighlightDesaturation, IDENTITY_CHANNEL_GRADE,
    LookParams, LookSection, MAX_START_STOPS,
};
use crate::pipeline::midtone_neutral::MidtoneLine;
use crate::pipeline::roll_white;
use crate::pipeline::scene_correction::{
    MidtoneCorrection, SceneCorrectionParams, SceneFault, WhiteBalance,
};
use crate::pipeline::working_space::map_nc_film_rgb_v1;
use crate::rendering::{Base, Rendering};
use crate::types::{
    FilmBase, FilmBaseSource, InputParams, LinearImage, MeasureParams, NcError, Result,
};

/// The document version this build writes.
pub const RECIPE_VERSION: u32 = 3;

/// The earlier version this build still reads: version 3 changed only what a number in
/// `look.contrast` means (the slope itself in 2, a multiplier on the base slope in 3,
/// `nf-look/contrast-definition`), so a version 2 document without one reads unchanged,
/// and [`check_body`] refuses one with it, naming the conversion.
pub const V2_RECIPE_VERSION: u32 = 2;

/// The top-level key carrying [`RECIPE_VERSION`].
///
/// Reserved beside `params` (the envelope's key). The removed chain's recipe
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

/// The document version: it serializes as [`RECIPE_VERSION`] and deserializes only a
/// version this build reads. Version 2 is accepted here because [`check_body`] has
/// already refused its one ambiguous key.
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
        if v == u64::from(RECIPE_VERSION) || v == u64::from(V2_RECIPE_VERSION) {
            Ok(RecipeVersion)
        } else {
            Err(serde::de::Error::custom(format!(
                "`{VERSION_KEY}` is {v}; this build reads {RECIPE_VERSION} and {V2_RECIPE_VERSION}"
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
    /// ([`destination()`]), as the look's is [`LookKeys::asks_for_a_look`]: the
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
/// Unset values are written as `null`, never left out: [`merge_json`] reads a one-key
/// object as an enum switch, and a per-frame `{"roll": {"white_stops": …}}` must merge.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RollSection {
    /// The roll's white-balance gains, green-anchored, as `measure-roll` reports them.
    pub white_balance: Option<[f32; 3]>,
    /// The roll's white, in scene stops above mid-grey — the measurement, not the
    /// slope derived from it ([`roll_white::slope_for`]), so a measured value is never
    /// mistaken for a chosen one.
    pub white_stops: Option<f32>,
    /// The roll's exposure in EV, a neutral gain added to `scene_correction.exposure`
    /// (`roll_white::roll_exposure`).
    pub exposure: Option<f32>,
    /// This frame's small lift in EV (`roll_white::small_lift`), added to `exposure` while
    /// [`Self::small_lift`] is on and no thin lift applies — where [`Recipe::for_frame`]
    /// moves a `frames` entry's `exposure`.
    pub frame_exposure: Option<f32>,
    /// Whether `frame_exposure` applies: a taste switch (design-spec §6). Unset (`null`) is
    /// on, so a measured file layered last never undoes an earlier `"off"`.
    pub small_lift: Option<Switch>,
    /// A thin frame's own slope (`roll_white::thin_lift`), in place of the one `white_stops`
    /// places, while [`Self::thin_lift`] is on. A slope, not a white: it is chosen, not
    /// measured.
    pub thin_slope: Option<f32>,
    /// The exposure solved with `thin_slope`, a delta on `exposure` in place of
    /// `frame_exposure`; it applies only beside `thin_slope`.
    pub thin_exposure: Option<f32>,
    /// Whether the thin pair applies: a taste switch like `small_lift`. Off, a thin frame
    /// renders its small lift.
    pub thin_lift: Option<Switch>,
    /// The roll's midtone cast (`pipeline::midtone_neutral`), removed in scene correction
    /// while [`Self::midtone_neutral`] is on. One line for the roll: a `frames` entry
    /// cannot carry one.
    pub midtone_line: Option<MidtoneLine>,
    /// Whether `midtone_line` applies. Unset (`null`) is on, like the lifts' switches.
    pub midtone_neutral: Option<Switch>,
    /// The frames with their own values — a white clamped to the cap, a lift
    /// (`roll_white::small_lift`, `roll_white::thin_lift`) — keyed by **file name** so the
    /// recipe still applies after the scans move. [`Recipe::for_frame`] applies an entry; a `roll --frames`
    /// manifest's `params` beat it.
    pub frames: BTreeMap<String, FrameRoll>,
}

/// One frame's own roll values ([`RollSection::frames`]); each unset one is the roll's.
/// Written with its nulls, like [`RollSection`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FrameRoll {
    /// The frame's white, in place of the roll's `white_stops`.
    pub white_stops: Option<f32>,
    /// The frame's small lift (`roll.frame_exposure`).
    pub exposure: Option<f32>,
    /// The frame's thin lift (`roll.thin_slope`, `roll.thin_exposure`).
    pub thin_slope: Option<f32>,
    pub thin_exposure: Option<f32>,
}

/// `roll.small_lift` / `--small-lift`, `roll.thin_lift` / `--thin-lift` and
/// `roll.midtone_neutral` / `--midtone-neutral`: whether an automatic adjustment applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Switch {
    On,
    Off,
}

impl RollSection {
    /// The slope this frame renders at, if the section sets one: its thin slope while that
    /// applies, else the one its white places.
    pub fn slope(&self) -> Option<f32> {
        self.applied_thin_slope()
            .or(self.white_stops.map(roll_white::slope_for))
    }

    /// The thin slope as it applies: `None` under `--thin-lift off`.
    pub fn applied_thin_slope(&self) -> Option<f32> {
        self.thin_slope
            .filter(|_| self.thin_lift != Some(Switch::Off))
    }

    /// The thin exposure as it applies: only beside an applied thin slope.
    pub fn applied_thin_exposure(&self) -> Option<f32> {
        self.thin_exposure
            .filter(|_| self.applied_thin_slope().is_some())
    }

    /// The small lift as it applies: `None` under `--small-lift off`, or where a thin lift
    /// replaces it.
    pub fn applied_small_exposure(&self) -> Option<f32> {
        self.frame_exposure
            .filter(|_| self.small_lift != Some(Switch::Off) && self.applied_thin_slope().is_none())
    }

    /// The frame's own exposure as it applies: the thin lift's, else the small lift's.
    pub fn applied_frame_exposure(&self) -> Option<f32> {
        self.applied_thin_exposure()
            .or(self.applied_small_exposure())
    }

    /// The midtone line as it applies: `None` under `--midtone-neutral off`.
    pub fn applied_midtone_line(&self) -> Option<MidtoneLine> {
        self.midtone_line
            .filter(|_| self.midtone_neutral != Some(Switch::Off))
    }

    /// The taste adjustments that apply, by their switch's key (design-spec §6).
    pub fn taste_applied(&self) -> Vec<&'static str> {
        [
            ("small_lift", self.applied_small_exposure().is_some()),
            ("thin_lift", self.applied_thin_slope().is_some()),
        ]
        .into_iter()
        .filter_map(|(key, on)| on.then_some(key))
        .collect()
    }

    /// A white stated over this section: it beats a thin slope from an earlier layer, and
    /// the exposure solved with that slope goes with it; the small lift stays. The caller
    /// then sets any thin value stated beside the white.
    pub fn drop_thin_lift(&mut self) {
        self.thin_slope = None;
        self.thin_exposure = None;
    }

    /// Undo what [`Recipe::for_frame`] moved in from `entry`, back to the `stated`
    /// section's values; a value a later layer beat stays.
    fn undo_entry(&mut self, entry: &FrameRoll, stated: &RollSection) {
        if entry.white_stops.is_some() && entry.white_stops == self.white_stops {
            self.white_stops = stated.white_stops;
        }
        if entry.exposure.is_some() && entry.exposure == self.frame_exposure {
            self.frame_exposure = stated.frame_exposure;
        }
        if entry.thin_slope.is_some() && entry.thin_slope == self.thin_slope {
            self.thin_slope = stated.thin_slope;
        }
        if entry.thin_exposure.is_some() && entry.thin_exposure == self.thin_exposure {
            self.thin_exposure = stated.thin_exposure;
        }
    }
}

/// The recipe's `look` keys: the stage's [`LookSection`] with the rendering-dependent
/// keys optional, since "unset" and "stated at the default" resolve differently. The
/// stage only ever receives a resolved section ([`LookKeys::resolve`]).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LookKeys {
    /// `look.contrast`, `--contrast`: a multiplier on the base slope — the roll's, else
    /// the rendering's own ([`Recipe::resolved_slope`]) — so `1` keeps the base, as
    /// `--white-balance` keeps the roll's gains. `null`, what a version 2 recipe wrote
    /// for "unset", reads as 1.
    #[serde(deserialize_with = "null_is_identity")]
    pub contrast: f32,
    /// `look.channel_grade`, `--channel-grade` ([`LookSection::channel_grade`]).
    pub channel_grade: [f32; 2],
    /// `look.highlight_desaturation`, `--highlight-desaturation*`: each key unset takes
    /// the rendering's base (off under `direct`).
    pub highlight_desaturation: DesaturationKeys,
}

impl Default for LookKeys {
    fn default() -> Self {
        Self {
            contrast: 1.0,
            channel_grade: IDENTITY_CHANNEL_GRADE,
            highlight_desaturation: DesaturationKeys::default(),
        }
    }
}

impl LookKeys {
    /// The stage's section at `slope` (already the base times `contrast`), with
    /// `desaturation`'s value for each unstated desaturation key. Destructured without
    /// `..`, so a new look knob does not compile until it has a base
    /// (`crate::rendering`'s module docs).
    pub fn resolve(&self, slope: f32, desaturation: HighlightDesaturation) -> LookSection {
        let LookKeys {
            contrast: _,
            channel_grade,
            highlight_desaturation:
                DesaturationKeys {
                    strength,
                    start_stops,
                    band,
                },
        } = *self;
        LookSection {
            slope,
            channel_grade,
            highlight_desaturation: HighlightDesaturation {
                strength: strength.unwrap_or(desaturation.strength),
                start_stops: start_stops.unwrap_or(desaturation.start_stops),
                band: band.unwrap_or(desaturation.band),
            },
        }
    }

    /// Whether the user asked for a look — the one predicate a destination that runs no
    /// look (`film-master`) reads to refuse, rather than one rule per knob
    /// (`nf-look/stage`; the refusal is `recipe::destination`). Spared: every knob at its
    /// default, and highlight desaturation off — an identity renders what such a
    /// destination does, and refusing it would kill the flags-win reset.
    pub fn asks_for_a_look(&self) -> bool {
        let pull = self
            .resolve(1.0, HighlightDesaturation::DEFAULT)
            .highlight_desaturation;
        self.contrast != 1.0
            || self.channel_grade != IDENTITY_CHANNEL_GRADE
            || !(pull.is_off() || pull == HighlightDesaturation::DEFAULT)
    }
}

/// Reads `null` as 1, the identity: version 2 wrote an unset `look.contrast` as `null`.
fn null_is_identity<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<f32, D::Error> {
    Ok(Option::<f32>::deserialize(d)?.unwrap_or(1.0))
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

/// Where the look's base slope came from — the report's `chain.look.base_from`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SlopeBase {
    /// The applied roll's white, through [`roll_white::slope_for`].
    Roll,
    /// No roll white under `default`: [`Base::slope`], the fallback.
    Fallback,
    /// `direct`, which applies no roll: its pinned [`Base::slope`].
    Direct,
    /// The applied roll's thin slope, `roll.thin_slope` (a thin frame's lift).
    Thin,
}

/// The look's slope and its parts: `base_slope × contrast` ([`Recipe::resolved_slope`]).
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct ResolvedSlope {
    /// `look.contrast`, the multiplier.
    pub contrast: f32,
    pub base_slope: f32,
    pub base_from: SlopeBase,
    /// What the look stage receives. Not serialized: the report's section states it.
    #[serde(skip)]
    pub slope: f32,
}

/// The report's `chain.look`: the knob and the base it multiplied, then the section the
/// stage ran (its `slope` is their product).
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct LookReport {
    #[serde(flatten)]
    pub slope: ResolvedSlope,
    #[serde(flatten)]
    pub section: LookSection,
}

/// What the roll section held and what the run applied of it — the report's
/// `chain.roll`, present whenever the section states a value.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RollReport {
    /// The section's gains, as stated.
    pub white_balance: Option<[f32; 3]>,
    /// The section's white, as stated.
    pub white_stops: Option<f32>,
    /// The slope the section sets ([`RollSection::slope`]): the thin slope while it
    /// applies, else `white_stops`'s ([`roll_white::slope_for`]).
    pub slope: Option<f32>,
    /// The section's exposure, as stated.
    pub exposure: Option<f32>,
    /// This frame's small lift, as stated.
    pub frame_exposure: Option<f32>,
    /// This frame's thin lift, as stated.
    pub thin_slope: Option<f32>,
    pub thin_exposure: Option<f32>,
    /// Whether the gains reached scene correction (not under `direct` or the film master).
    pub white_balance_applied: bool,
    /// Whether the look's base slope is the roll's.
    pub slope_applied: bool,
    /// Whether the exposure reached scene correction.
    pub exposure_applied: bool,
    /// Whether the small lift reached scene correction (not under `--small-lift off`, nor
    /// beside an applied thin lift).
    pub frame_exposure_applied: bool,
    /// Whether the thin lift set the look's base slope and its exposure.
    pub thin_lift_applied: bool,
    /// The section's midtone line, as stated.
    pub midtone_line: Option<MidtoneLine>,
    /// Whether it reached scene correction (not under `--midtone-neutral off`).
    pub midtone_neutral_applied: bool,
    /// The taste adjustments applied, by their switch's key ([`RollSection::taste_applied`]):
    /// each is a choice, off by `roll.<key>` `"off"`.
    pub taste_applied: Vec<&'static str>,
}

/// Which style knobs this invocation typed as flags: [`Recipe::recipe_warnings`] never
/// warns about those.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TypedStyle {
    /// `--white-balance` was typed.
    pub white_balance: bool,
    /// `--contrast` was typed.
    pub contrast: bool,
    /// `--exposure` was typed.
    pub exposure: bool,
    /// `--highlight-desaturation` (the strength; no warning reads the start or band).
    pub highlight_desaturation_strength: bool,
}

impl TypedStyle {
    /// What the conversion flags typed (`convert`'s, or `roll`'s).
    pub fn of(args: &crate::cli::ConversionFlags) -> Self {
        Self {
            white_balance: args.scene.white_balance.is_some(),
            contrast: args.look.contrast.is_some(),
            exposure: args.scene.exposure.is_some(),
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
         (where the film base renders); and `linear_range`, the levels remap, \
         retired: its gain is `scene_correction.exposure` and its black is \
         `fit_range.display_black`",
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
        "there is one curve: its slope is `reconstruction.linearization` (the picture's \
         contrast is `look.contrast`) and its placement \
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
        "there is one curve: its slope is `reconstruction.linearization` (the picture's \
         contrast is `look.contrast`) and its placement \
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
        &["input", "export_ir"],
        "the IR plane is no longer exported; drop the key",
    ),
    (
        &["input", "color"],
        "it conflated transfer encoding with measurement meaning; use the independent \
         keys `input.transfer` (auto|linear) and `input.meaning` \
         (auto|scanner-device|colorimetric)",
    ),
];

/// Why `roll.frame_slope` and a `roll.frames` entry's `slope` retired
/// (`nf-calibration/taste-vs-quality`). Beside a slope the old exposure was the thin
/// lift's, so the remedy renames both: dropping the slope alone renders a small lift. A
/// retired `roll.frame_lift` beside it is migrated in the same message, so the remedy
/// never meets that key's refusal.
fn thin_pair_moved(
    body: &serde_json::Value,
    slope: &str,
    exposure: &str,
    renamed: (&str, &str),
    entries: bool,
) -> String {
    let lift = frame_lift_remedy(body).map_or(String::new(), |r| format!(", and {r}"));
    // A top-level slope rendered beside a frame's entry `exposure` where one took the
    // top-level exposure's place.
    let entries = if entries {
        " (a frame whose `roll.frames` entry states an `exposure` rendered that one: give \
         the entry the same `thin_slope` and its `exposure` as `thin_exposure`)"
    } else {
        ""
    };
    format!(
        "{slope} is not a recipe key any more: a thin frame's lift is its own pair, \
         `thin_slope` and `thin_exposure`, beside the small lift, so each has its own \
         switch (`roll.thin_lift`, `roll.small_lift`). To render as before, rename {slope} \
         to {} and {exposure}, if stated, to {} (beside a slope it was the thin lift's \
         exposure){entries}{lift}; or re-run `hanten measure-roll --out` for the roll",
        renamed.0, renamed.1
    )
}

/// The remedy for a non-null retired `roll.frame_lift`, which switched both lifts
/// (`nf-calibration/taste-vs-quality`): both switches now, `None` when the body has none.
fn frame_lift_remedy(body: &serde_json::Value) -> Option<String> {
    let value = body.pointer("/roll/frame_lift").filter(|v| !v.is_null())?;
    Some(match value.as_str() {
        Some("off") => "replace `roll.frame_lift` \"off\" with `roll.small_lift` \"off\" and \
                        `roll.thin_lift` \"off\" (it turned both lifts off)"
            .into(),
        // Not a no-op: it beat an earlier layer's "off".
        Some("on") => "replace `roll.frame_lift` \"on\" with `roll.small_lift` \"on\" and \
                       `roll.thin_lift` \"on\" (it turned both lifts on)"
            .into(),
        _ => "drop `roll.frame_lift` and state `roll.small_lift` and `roll.thin_lift`".into(),
    })
}

/// Drop the retired keys where an earlier build wrote their default, `null`, which
/// replays identically: `input.export_ir`, `roll.frame_slope`, `roll.frame_lift` and a
/// `roll.frames` entry's `slope`. [`check_body`] refuses any other value; every recipe
/// body runs this first. Returns whether it dropped any.
pub fn strip_retired_nulls(body: &mut serde_json::Value) -> bool {
    let mut stripped = false;
    if let Some(input) = body.get_mut("input").and_then(|i| i.as_object_mut())
        && input.get("export_ir").is_some_and(|v| v.is_null())
    {
        input.remove("export_ir");
        stripped = true;
    }
    let Some(roll) = body.get_mut("roll").and_then(|r| r.as_object_mut()) else {
        return stripped;
    };
    for key in ["frame_slope", "frame_lift"] {
        if roll.get(key).is_some_and(|v| v.is_null()) {
            roll.remove(key);
            stripped = true;
        }
    }
    if let Some(frames) = roll.get_mut("frames").and_then(|f| f.as_object_mut()) {
        for entry in frames.values_mut().filter_map(|e| e.as_object_mut()) {
            if entry.get("slope").is_some_and(|v| v.is_null()) {
                entry.remove("slope");
                stripped = true;
            }
        }
    }
    stripped
}

/// What a migration message converts an old slope against: the base slope a recipe
/// body gives `look.contrast` under its own rendering and roll, how to name it, and the
/// keys to drop first. With `roll.frames` the roll's slope differs per frame while a
/// stated slope overrode them all, so the exact conversion drops the roll's whites and
/// multiplies the fallback.
struct BodyBase {
    slope: f32,
    named: String,
    drop: String,
}

fn body_base(body: &serde_json::Value) -> BodyBase {
    let base = |slope: f32, named: String, drop: String| BodyBase { slope, named, drop };
    if body.get("rendering").and_then(|r| r.as_str()) == Some("direct") {
        let s = Rendering::Direct.base().slope;
        return base(
            s,
            format!("the `direct` rendering's slope {s}"),
            String::new(),
        );
    }
    let fallback = Rendering::Default.base().slope;
    let white = body.pointer("/roll/white_stops").and_then(|w| w.as_f64());
    let frames = body
        .pointer("/roll/frames")
        .and_then(|f| f.as_object())
        .is_some_and(|f| !f.is_empty());
    match white {
        _ if frames => {
            let keys = if white.is_some() {
                "`roll.white_stops` and `roll.frames`"
            } else {
                "`roll.frames`"
            };
            base(
                fallback,
                format!("the fallback slope {fallback}"),
                format!("drop {keys} (the stated slope overrode every frame's white), "),
            )
        }
        Some(w) if w.is_finite() && w > 0.0 => {
            let s = roll_white::slope_for(w as f32);
            base(
                s,
                format!("the roll's slope {s} from `roll.white_stops` {w}"),
                String::new(),
            )
        }
        _ => base(
            fallback,
            format!("the fallback slope {fallback}"),
            String::new(),
        ),
    }
}

/// The `look.contrast` multiplier whose product with `base` comes closest to `slope`,
/// among the quotient and its `f32` neighbours, and whether it hits `slope` exactly.
/// Exact is not always reachable: the products of neighbouring multipliers can step
/// over `slope`. `None` when no usable multiplier comes out (a non-positive or
/// non-finite slope or base).
fn multiplier_for(slope: f32, base: f32) -> Option<(f32, bool)> {
    let q = slope / base;
    if !(q.is_normal() && q > 0.0) {
        return None;
    }
    let k = (-2i32..=2)
        .map(|step| f32::from_bits(q.to_bits().wrapping_add_signed(step)))
        .filter(|k| k.is_normal())
        .min_by(|a, b| {
            (base * a - slope)
                .abs()
                .total_cmp(&(base * b - slope).abs())
        })?;
    Some((k, base * k == slope))
}

/// A version 2 `look.contrast` number: it was the slope, and is now a multiplier.
/// `whole` is `false` for a per-frame override, whose base is the shared recipe's and
/// so not in `body`.
fn v2_contrast_message(body: &serde_json::Value, stated: f32, whole: bool) -> String {
    let meaning = format!(
        "`look.contrast` {stated} in a `{VERSION_KEY}` {V2_RECIPE_VERSION} recipe is the \
         slope itself; since version {RECIPE_VERSION} it is a multiplier on the base slope \
         (1 keeps it)"
    );
    let version = format!("`\"{VERSION_KEY}\": {RECIPE_VERSION}`");
    let b = body_base(body);
    let convert = match multiplier_for(stated, b.slope) {
        Some((k, exact)) if whole => format!(
            "{}, {}state `look.contrast` {k} ({stated} over {}; stacked with other \
             `--params` files, over the base they compose to, the report's \
             `chain.look.base_slope`) and {version}",
            if exact {
                "To render as before"
            } else {
                "To keep that slope (to within one f32 step: no multiplier hits it exactly)"
            },
            b.drop,
            b.named
        ),
        Some(_) => format!(
            "To render as before, state {stated} over the frame's base slope (its report's \
             `chain.look.base_slope`) and {version}"
        ),
        None => format!("State a positive multiplier and {version}"),
    };
    format!(
        "{meaning}. {convert}. If nobody chose the value (every recipe an earlier build wrote \
         stated 1.1111112, and an earlier `hanten measure-roll` wrote the roll's contrast \
         there), drop it and state {version} to use the base"
    )
}

/// A per-frame override stating a `look.contrast` number but no version: the number
/// means a multiplier since version 3 and a slope before, and nothing tells which.
fn unversioned_contrast_message(stated: f32) -> String {
    format!(
        "`look.contrast` {stated} in a per-frame override that states no \
         `{VERSION_KEY}`: since version {RECIPE_VERSION} it is a multiplier on the frame's \
         base slope (1 keeps it), and in version {V2_RECIPE_VERSION} it was the slope \
         itself. State `\"{VERSION_KEY}\": {RECIPE_VERSION}` beside it if {stated} is the \
         multiplier; if it is a slope, divide it by the frame's base slope (its report's \
         `chain.look.base_slope`) first"
    )
}

/// A body's `look.contrast` when it is a number (in `f32`, as the recipe holds it).
fn stated_contrast(body: &serde_json::Value) -> Option<f32> {
    let v = body.get("look")?.get("contrast")?.as_f64()?;
    Some(v as f32)
}

/// Refuse a recipe body written for the removed chain, before serde sees it.
///
/// Run on the raw JSON, because the failure is about presence: a missing marker, or
/// a key the removed chain read that means nothing here. `whole` is `true`
/// for a whole recipe and `false` for a `roll` per-frame overlay, which is a partial
/// document merged onto an already-versioned one and so need not restate the
/// version (it may, and must then state the right one) — unless it states a
/// `look.contrast` number, whose meaning the version decides.
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
                 converter. `hanten params` writes the current layout in its `params`: \
                 `input`, `measure` and a `region` or `explicit` `calibration.film_base` \
                 carry over unchanged (an \
                 `\"auto\"` one retired: measure the base with `hanten measure-base \
                 <unexposed-frame>`), and the rest is a stage \
                 section each (`reconstruction`, `scene_correction`, `look`, `fit_range`) \
                 plus `output`, the destination"
            ));
        }
        Some(v) if v.as_u64() == Some(u64::from(V2_RECIPE_VERSION)) => {
            if let Some(stated) = stated_contrast(body) {
                return usage(v2_contrast_message(body, stated, whole));
            }
        }
        None => {
            if let Some(stated) = stated_contrast(body) {
                return usage(unversioned_contrast_message(stated));
            }
        }
        Some(v) if v.as_u64() != Some(u64::from(RECIPE_VERSION)) => {
            return usage(format!(
                "`{VERSION_KEY}` is {v}; this build reads {RECIPE_VERSION} and \
                 {V2_RECIPE_VERSION}"
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
    if body.pointer("/roll/frame_slope").is_some() {
        return usage(thin_pair_moved(
            body,
            "`roll.frame_slope`",
            "`roll.frame_exposure`",
            ("`roll.thin_slope`", "`roll.thin_exposure`"),
            true,
        ));
    }
    if let Some(name) = body
        .pointer("/roll/frames")
        .and_then(|f| f.as_object())
        .and_then(|f| f.iter().find(|(_, e)| e.get("slope").is_some()))
        .map(|(name, _)| name)
    {
        return usage(thin_pair_moved(
            body,
            &format!("`roll.frames.\"{name}\".slope`"),
            &format!("`roll.frames.\"{name}\".exposure`"),
            ("`thin_slope`", "`thin_exposure`"),
            false,
        ));
    }
    if let Some(remedy) = frame_lift_remedy(body) {
        return usage(format!(
            "`roll.frame_lift` is not a recipe key any more: it switched both a frame's lifts, \
             which now each have their own switch, `roll.small_lift` and `roll.thin_lift`. To \
             render as before, {remedy}"
        ));
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
    // A removed destination value (AVIF, `output/drop-avif`). Named by key here: serde
    // would word it as the flag, which cannot rescue a recipe that fails before merge.
    if let Some(message) = body
        .get("output")
        .and_then(|o| o.get("display"))
        .and_then(crate::destination::removed_in_recipe)
    {
        return usage(message);
    }
    // Retired by `nf-reconstruction/gamma-split`, which split the one slope in two.
    // Refused at every value, the old default included: no single new key replays it.
    if let Some(v) = body.get("reconstruction").and_then(|r| r.get("contrast")) {
        let b = body_base(body);
        // A version 2 document must move to 3 too, or its new multiplier is read as a
        // version 2 slope and refused again; so must a per-frame override, which may
        // state a `look.contrast` number only beside a version.
        let v2 =
            body.get(VERSION_KEY).and_then(|v| v.as_u64()) == Some(u64::from(V2_RECIPE_VERSION));
        let version = if v2 || !whole {
            format!(", and state `\"{VERSION_KEY}\": {RECIPE_VERSION}`")
        } else {
            String::new()
        };
        // In f32, as the recipe holds it, so the stated value prints as written. Only a
        // multiplier validation accepts: anything else falls to the generic remedy. An
        // override's base is the shared recipe's, not in `body`, so it gets no number.
        let multiplier = v
            .as_f64()
            .filter(|_| whole)
            .and_then(|gamma| multiplier_for(gamma as f32 / LINEARIZATION, b.slope))
            .map(|(k, _)| k)
            .filter(|k| (LINEARIZATION * b.slope * k).is_normal());
        let remedy = match (v.as_f64(), multiplier) {
            (Some(gamma), Some(k)) => format!(
                "to keep a stated {} as the whole slope, {}write `look.contrast` {k} (over \
                 {}) and leave `reconstruction.linearization` at its default \
                 {LINEARIZATION}{version}",
                gamma as f32, b.drop, b.named,
            ),
            (Some(gamma), None) if !whole && gamma > 0.0 => format!(
                "to keep a stated {} as the whole slope, write `look.contrast` {} over the \
                 frame's base slope (its report's `chain.look.base_slope`) and leave \
                 `reconstruction.linearization` at its default {LINEARIZATION}{version}",
                gamma as f32,
                gamma as f32 / LINEARIZATION,
            ),
            _ => format!(
                "state `look.contrast`, a multiplier on the base slope, and leave \
                 `reconstruction.linearization` at its default {LINEARIZATION}{version}"
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
pub fn merge(mut r: Recipe, args: &crate::cli::ConversionFlags) -> Recipe {
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
    // The decode's own knobs. `--density-gamma` is the decode's linearization — a
    // calibration; how contrasty the picture is is `--contrast`, the look's.
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
        r.roll.drop_thin_lift();
    }
    if let Some(ev) = args.roll.roll_exposure {
        r.roll.exposure = Some(ev);
    }
    if let Some(ev) = args.roll.roll_frame_exposure {
        r.roll.frame_exposure = Some(ev);
    }
    if let Some(lift) = args.roll.small_lift {
        r.roll.small_lift = Some(lift);
    }
    if let Some(slope) = args.roll.roll_thin_slope {
        r.roll.thin_slope = Some(slope);
    }
    if let Some(ev) = args.roll.roll_thin_exposure {
        r.roll.thin_exposure = Some(ev);
    }
    if let Some(lift) = args.roll.thin_lift {
        r.roll.thin_lift = Some(lift);
    }
    if let Some(line) = args.roll.roll_midtone_line {
        r.roll.midtone_line = Some(line);
    }
    if let Some(switch) = args.roll.midtone_neutral {
        r.roll.midtone_neutral = Some(switch);
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
        r.look.contrast = v;
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

/// How a validation message names a knob: by the flag and the recipe key where both
/// reach it, and by the key alone where no flag does (`measure-roll`, a roll frame's
/// manifest `params`) — naming a flag there hands the user a remedy they cannot type.
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
            validate_roll_frames(&r.roll, d.linearization, r.look.contrast, names)?;
            validate_scene_correction(r, names)?;
            validate_look(r, names)?;
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

/// The look's value rules, rendered as a usage error naming the knob the way `names`
/// says the command spells it: the `contrast` multiplier, then the slope it makes with
/// its base ([`validate_whole_slope`]), then the grade and highlight desaturation
/// ([`LookSection::check_channel_grade`], [`HighlightDesaturation::check`]).
///
/// [`HighlightDesaturation::check`]: crate::pipeline::look::HighlightDesaturation::check
fn validate_look(r: &Recipe, names: KnobNames) -> Result<()> {
    let contrast_name = knob_name(names, "look", "--contrast", "contrast");
    let k = r.look.contrast;
    if !(k.is_finite() && k > 0.0) {
        return Err(NcError::Usage(format!(
            "{contrast_name} must be finite and positive (a multiplier on the base slope; \
             1 keeps it), got {k}"
        )));
    }
    let s = r.resolved_slope();
    let white_name = knob_name(names, "roll", "--roll-white", "white_stops");
    let slope_name = knob_name(names, "roll", "--roll-thin-slope", "thin_slope");
    let (base, knob) = match s.base_from {
        SlopeBase::Roll => (
            format!("the roll's slope {} from {white_name}", s.base_slope),
            Some(SlopeKnob::White(&white_name)),
        ),
        SlopeBase::Thin => (
            format!("the frame's thin slope {slope_name} {}", s.base_slope),
            Some(SlopeKnob::Slope(&slope_name)),
        ),
        SlopeBase::Fallback => (format!("the fallback slope {}", s.base_slope), None),
        SlopeBase::Direct => (
            format!("the `direct` rendering's slope {}", s.base_slope),
            None,
        ),
    };
    validate_whole_slope(
        r.reconstruction.linearization,
        s.slope,
        &format!("{base} times {contrast_name} {k}"),
        Some(&contrast_name),
        knob,
        names,
    )?;
    let p = r.resolved_look();
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

/// The roll knob a slope comes from, named: a white (the slope falls as it rises) or a
/// frame slope (the slope itself).
#[derive(Clone, Copy)]
enum SlopeKnob<'a> {
    White(&'a str),
    Slope(&'a str),
}

/// The whole slope, `linearization × slope` — the divisor highlight desaturation
/// normalises its saturation measure by — must be a normal positive f32. Each factor
/// can pass its own rule while the product overflows to infinity or underflows to zero
/// or a subnormal, which the stage cannot use; a whole slope in range puts the slope in
/// range too. Keyed on the product whether or not desaturation is on: it describes no
/// usable picture either way. `slope_parts` names the slope's factors; the remedy names
/// each knob that can move it — the multiplier and the roll's knob when they are factors.
fn validate_whole_slope(
    linearization: f32,
    slope: f32,
    slope_parts: &str,
    contrast_name: Option<&str>,
    roll_knob: Option<SlopeKnob>,
    names: KnobNames,
) -> Result<()> {
    let whole = linearization * slope;
    if whole.is_normal() {
        return Ok(());
    }
    let gamma = knob_name(names, "reconstruction", "--density-gamma", "linearization");
    let overflows = whole.is_infinite();
    let what = if overflows { "overflows" } else { "underflows" };
    let (toward, away) = if overflows {
        ("smaller", "larger")
    } else {
        ("larger", "smaller")
    };
    let mut remedy = format!("Use a {toward} {gamma}");
    if let Some(contrast) = contrast_name {
        remedy += &format!(" or {contrast}");
    }
    // The roll's slope is inversely proportional to its white; a frame slope is itself.
    match roll_knob {
        Some(SlopeKnob::White(white)) => remedy += &format!(", or a {away} {white}"),
        Some(SlopeKnob::Slope(slope)) => remedy += &format!(", or a {toward} {slope}"),
        None => {}
    }
    Err(NcError::Usage(format!(
        "the whole slope, {gamma} {linearization:e} times the slope {slope:e} \
         ({slope_parts}), {what} f32 (highlight desaturation divides by it). {remedy}"
    )))
}

/// The roll section's value rules: gains finite and positive, as scene correction's
/// are; a white finite and positive, since the slope is `log2(1/0.18)` over it.
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
    if let Some(ev) = p.exposure {
        exposure_fault(ev, &knob_name(names, "roll", "--roll-exposure", "exposure"))?;
    }
    if let Some(ev) = p.frame_exposure {
        exposure_fault(
            ev,
            &knob_name(names, "roll", "--roll-frame-exposure", "frame_exposure"),
        )?;
    }
    let thin_slope = knob_name(names, "roll", "--roll-thin-slope", "thin_slope");
    if let Some(slope) = p.thin_slope {
        slope_fault(slope, &thin_slope)?;
    }
    if let Some(ev) = p.thin_exposure {
        let name = knob_name(names, "roll", "--roll-thin-exposure", "thin_exposure");
        exposure_fault(ev, &name)?;
        if p.thin_slope.is_none() {
            return Err(NcError::Usage(format!(
                "{name} is the exposure solved with a thin frame's slope, and applies \
                 only beside it: state {thin_slope} too, or drop it"
            )));
        }
    }
    if let Some(line) = p.midtone_line {
        let name = knob_name(names, "roll", "--roll-midtone-line", "midtone_line");
        line.check()
            .map_err(|e| NcError::Usage(format!("{name}: {e}")))?;
        if p.white_balance.is_none() {
            return Err(NcError::Usage(format!(
                "{name} is measured after the roll's white balance, and applies only beside \
                 it: state {} too (`hanten measure-roll` writes both), or drop it",
                knob_name(names, "roll", "--roll-white-balance", "white_balance")
            )));
        }
    }
    Ok(())
}

/// A stated frame slope `slope`, named `name`: finite and positive.
fn slope_fault(slope: f32, name: &str) -> Result<()> {
    if slope.is_finite() && slope > 0.0 {
        return Ok(());
    }
    Err(NcError::Usage(format!(
        "{name} must be finite and positive — the look's base slope, as `hanten \
         measure-roll` writes it for a thin frame — got {slope}"
    )))
}

/// A stated exposure `ev`, named `name`: scene correction's rule on its own.
fn exposure_fault(ev: f32, name: &str) -> Result<()> {
    match (SceneCorrectionParams {
        exposure: ev,
        ..SceneCorrectionParams::default()
    })
    .check()
    {
        Err(SceneFault::Exposure(_)) => Err(NcError::Usage(format!(
            "{name} must be finite, with a gain 2^EV that is a normal f32 (roughly -126 to \
             +127 stops), got {ev}"
        ))),
        _ => Ok(()),
    }
}

/// `roll.frames`' rules, each entry named as itself: a file-name key, and a white
/// finite and positive whose whole slope — at `linearization`, times the `contrast`
/// multiplier the frame renders under — fits f32. Public so `convert` can judge the
/// table as stated, before [`Recipe::for_frame`] moves its own entry into
/// `roll.white_stops`.
pub fn validate_roll_frames(
    p: &RollSection,
    linearization: f32,
    contrast: f32,
    names: KnobNames,
) -> Result<()> {
    let contrast_name = knob_name(names, "look", "--contrast", "contrast");
    for (name, frame) in &p.frames {
        if name.is_empty() || Path::new(name).file_name() != Some(name.as_ref()) {
            return Err(NcError::Usage(format!(
                "recipe `roll.frames` keys are file names, not paths: got {name:?}"
            )));
        }
        if *frame == FrameRoll::default() {
            return Err(NcError::Usage(format!(
                "recipe `roll.frames.\"{name}\"` states nothing: give it a `white_stops`, \
                 an `exposure` or a `thin_slope`, or drop the entry"
            )));
        }
        if let Some(ev) = frame.exposure {
            exposure_fault(ev, &format!("recipe `roll.frames.\"{name}\".exposure`"))?;
        }
        if let Some(ev) = frame.thin_exposure {
            let entry = format!("recipe `roll.frames.\"{name}\".thin_exposure`");
            exposure_fault(ev, &entry)?;
            if frame.thin_slope.is_none() {
                return Err(NcError::Usage(format!(
                    "{entry} is the exposure solved with a thin frame's slope, and applies \
                     only beside it: state the entry's `thin_slope` too, or drop it"
                )));
            }
        }
        if let Some(slope) = frame.thin_slope {
            let entry = format!("recipe `roll.frames.\"{name}\".thin_slope`");
            slope_fault(slope, &entry)?;
            validate_whole_slope(
                linearization,
                slope * contrast,
                &format!(
                    "the frame's thin slope {slope} from {entry} times {contrast_name} {contrast}"
                ),
                Some(&contrast_name),
                Some(SlopeKnob::Slope(&entry)),
                names,
            )?;
        }
        let Some(stops) = frame.white_stops else {
            continue;
        };
        if !(stops.is_finite() && stops > 0.0) {
            return Err(NcError::Usage(format!(
                "recipe `roll.frames.\"{name}\".white_stops` must be finite and positive — \
                 stops above mid-grey, as `hanten measure-roll` reports them — got {stops}"
            )));
        }
        let white = format!("recipe `roll.frames.\"{name}\".white_stops`");
        let base = roll_white::slope_for(stops);
        validate_whole_slope(
            linearization,
            base * contrast,
            &format!("the frame's slope {base} from {white} times {contrast_name} {contrast}"),
            Some(&contrast_name),
            Some(SlopeKnob::White(&white)),
            names,
        )?;
    }
    Ok(())
}

/// Whether `r`'s values combine into a render: every sample a scan can hold stays finite
/// through the decode, scene correction and the look, and the film base grades to a
/// positive luminance for display black. Fit range refuses either as a wiring bug.
/// Run by `convert` and `roll` (not `measure-roll`, which renders nothing) after
/// `cli::validate_shared`, for `r` and each `roll.frames` entry. `own` names `r`'s frame
/// and its roll section before [`Recipe::for_frame`]: a fault the frame's entry causes is
/// named as the entry, and the other entries are probed without what it moved in.
///
/// **A probe, not a bound per knob** — legal values can multiply to zero or overflow:
/// the film base and the corners of the reachable scan range (each channel at the scan
/// floor or 1) through the real stages, which are monotone per channel. A base read
/// from a region is taken at 1, where the densest sample decodes densest.
pub fn validate_render(
    r: &Recipe,
    own: Option<(&str, &RollSection)>,
    names: KnobNames,
) -> Result<()> {
    if r.output == OutputSection::FilmMaster {
        return Ok(());
    }
    let own_entry = own.and_then(|(name, stated)| Some((name, stated.frames.get(name)?)));
    render_fault_message(r, own_entry, None, names)?;
    let mut others = r.clone();
    if let (Some((_, entry)), Some((_, stated))) = (own_entry, own) {
        others.roll.undo_entry(entry, stated);
    }
    for (name, entry) in &r.roll.frames {
        let frame = others.clone().for_frame(Path::new(name));
        render_fault_message(&frame, Some((name, entry)), Some(name), names)?;
    }
    Ok(())
}

/// What stops a recipe rendering ([`validate_render`]).
#[derive(Clone, Copy, Debug, PartialEq)]
enum RenderFault {
    /// The film base grades to this luminance, not a positive, finite one.
    Base(f32),
    /// The densest reachable sample renders non-finite.
    Overflow,
}

fn render_fault(r: &Recipe) -> Result<Option<RenderFault>> {
    let base = match r.calibration.film_base {
        Some(FilmBaseSource::Explicit(b)) => b,
        _ => [1.0; 3],
    };
    let mut probe = base.to_vec();
    for corner in 0..8 {
        probe.extend((0..3).map(|c| {
            if corner >> c & 1 == 1 {
                SCAN_FLOOR
            } else {
                1.0
            }
        }));
    }
    let film_base = FilmBase {
        r: base[0],
        g: base[1],
        b: base[2],
    };
    let (film, _) = fixed::decode(
        &LinearImage::new(9, 1, probe, None)?,
        &film_base,
        &r.reconstruction,
    )?;
    let graded = chain::graded_pixels(map_nc_film_rgb_v1(film), &r.shared_params())?;
    if r.resolved_fit_range().1 != DisplayBlack::Off {
        let luminance = dot([graded.rgb[0], graded.rgb[1], graded.rgb[2]], ACESCG_LUMA);
        if !(luminance.is_finite() && luminance > 0.0) {
            return Ok(Some(RenderFault::Base(luminance)));
        }
    }
    Ok(graded
        .rgb
        .iter()
        .any(|v| !v.is_finite())
        .then_some(RenderFault::Overflow))
}

/// A knob the probe reads: whether a recipe states it, and how to reset it.
struct ProbeKnob {
    section: &'static str,
    flag: &'static str,
    key: &'static str,
    stated: fn(&Recipe) -> bool,
    reset: fn(&mut Recipe),
}

const PROBE_KNOBS: [ProbeKnob; 14] = [
    ProbeKnob {
        section: "reconstruction",
        flag: "--density-offset",
        key: "offset",
        stated: |r| r.reconstruction.offset != DENSITY_OFFSET,
        reset: |r| r.reconstruction.offset = DENSITY_OFFSET,
    },
    ProbeKnob {
        section: "reconstruction",
        flag: "--density-scale",
        key: "scale",
        stated: |r| r.reconstruction.scale != DENSITY_SCALE,
        reset: |r| r.reconstruction.scale = DENSITY_SCALE,
    },
    ProbeKnob {
        section: "reconstruction",
        flag: "--density-gamma",
        key: "linearization",
        stated: |r| r.reconstruction.linearization != LINEARIZATION,
        reset: |r| r.reconstruction.linearization = LINEARIZATION,
    },
    ProbeKnob {
        section: "reconstruction",
        flag: "--anchor-mid-offset",
        key: "anchor",
        stated: |r| r.reconstruction.anchor != AnchorRule::MidAboveBase(MID_ABOVE_BASE),
        reset: |r| r.reconstruction.anchor = AnchorRule::MidAboveBase(MID_ABOVE_BASE),
    },
    ProbeKnob {
        section: "look",
        flag: "--contrast",
        key: "contrast",
        stated: |r| r.look.contrast != 1.0,
        reset: |r| r.look.contrast = 1.0,
    },
    ProbeKnob {
        section: "look",
        flag: "--channel-grade",
        key: "channel_grade",
        stated: |r| r.look.channel_grade != IDENTITY_CHANNEL_GRADE,
        reset: |r| r.look.channel_grade = IDENTITY_CHANNEL_GRADE,
    },
    ProbeKnob {
        section: "roll",
        flag: "--roll-white",
        key: "white_stops",
        stated: |r| r.roll.white_stops.is_some(),
        reset: |r| r.roll.white_stops = None,
    },
    ProbeKnob {
        section: "scene_correction",
        flag: "--exposure",
        key: "exposure",
        stated: |r| r.scene_correction.exposure != 0.0,
        reset: |r| r.scene_correction.exposure = 0.0,
    },
    ProbeKnob {
        section: "roll",
        flag: "--roll-exposure",
        key: "exposure",
        stated: |r| r.roll.exposure.is_some(),
        reset: |r| r.roll.exposure = None,
    },
    ProbeKnob {
        section: "roll",
        flag: "--roll-frame-exposure",
        key: "frame_exposure",
        stated: |r| r.roll.applied_small_exposure().is_some(),
        reset: |r| r.roll.frame_exposure = None,
    },
    // The thin pair goes together: its exposure applies only beside its slope.
    ProbeKnob {
        section: "roll",
        flag: "--roll-thin-slope",
        key: "thin_slope",
        stated: |r| r.roll.applied_thin_slope().is_some(),
        reset: |r| r.roll.drop_thin_lift(),
    },
    ProbeKnob {
        section: "roll",
        flag: "--roll-thin-exposure",
        key: "thin_exposure",
        stated: |r| r.roll.applied_thin_exposure().is_some(),
        reset: |r| r.roll.thin_exposure = None,
    },
    ProbeKnob {
        section: "scene_correction",
        flag: "--white-balance",
        key: "white_balance",
        stated: |r| r.scene_correction.white_balance != WhiteBalance::Explicit([1.0; 3]),
        reset: |r| r.scene_correction.white_balance = WhiteBalance::Explicit([1.0; 3]),
    },
    ProbeKnob {
        section: "roll",
        flag: "--roll-white-balance",
        key: "white_balance",
        stated: |r| r.roll.white_balance.is_some(),
        reset: |r| r.roll.white_balance = None,
    },
];

/// [`render_fault`] as a usage error: the fault, and the stated knobs whose default
/// alone clears it — else every stated knob the probe reads.
fn render_fault_message(
    r: &Recipe,
    own: Option<(&str, &FrameRoll)>,
    entry: Option<&str>,
    names: KnobNames,
) -> Result<()> {
    let Some(fault) = render_fault(r)? else {
        return Ok(());
    };
    // The frame's entry, when `r`'s white or lift came from it (a flag beats it).
    let one = |k: &ProbeKnob| match (own, k.section, k.key) {
        (Some((frame, e)), "roll", "white_stops")
            if e.white_stops.is_some() && e.white_stops == r.roll.white_stops =>
        {
            format!("recipe `roll.frames.\"{frame}\".white_stops`")
        }
        (Some((frame, e)), "roll", "frame_exposure")
            if e.exposure.is_some() && e.exposure == r.roll.frame_exposure =>
        {
            format!("recipe `roll.frames.\"{frame}\".exposure`")
        }
        (Some((frame, e)), "roll", "thin_slope")
            if e.thin_slope.is_some() && e.thin_slope == r.roll.thin_slope =>
        {
            format!("recipe `roll.frames.\"{frame}\".thin_slope`")
        }
        (Some((frame, e)), "roll", "thin_exposure")
            if e.thin_exposure.is_some() && e.thin_exposure == r.roll.thin_exposure =>
        {
            format!("recipe `roll.frames.\"{frame}\".thin_exposure`")
        }
        _ => knob_name(names, k.section, k.flag, k.key),
    };
    // The thin slope's reset drops its exposure too, which applies only beside it: named
    // as the pair, and whether it is one.
    let name = |k: &ProbeKnob| match PROBE_KNOBS.iter().find(|e| e.key == "thin_exposure") {
        Some(exposure) if k.key == "thin_slope" && (exposure.stated)(r) => {
            (format!("{} with its {}", one(k), one(exposure)), true)
        }
        _ => (one(k), false),
    };
    let stated: Vec<&ProbeKnob> = PROBE_KNOBS.iter().filter(|k| (k.stated)(r)).collect();
    let mut clears = Vec::new();
    for k in &stated {
        let mut reset = r.clone();
        (k.reset)(&mut reset);
        if render_fault(&reset)?.is_none() {
            clears.push(name(k));
        }
    }
    let remedy = match clears.as_slice() {
        [] if stated.is_empty() => String::new(),
        [] => format!(
            " It renders with {} at their defaults",
            stated
                .iter()
                .map(|k| one(k))
                .collect::<Vec<_>>()
                .join(" and ")
        ),
        [(one, false)] => format!(" It renders with {one} at its default"),
        [(pair, true)] => format!(" It renders with {pair} at their defaults"),
        some if some.iter().any(|(_, pair)| *pair) => format!(
            " It renders with any one of these at default: {}",
            some.iter()
                .map(|(n, _)| n.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        ),
        some => format!(
            " It renders with any one of {} at its default",
            some.iter()
                .map(|(n, _)| n.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };
    let frame = entry.map_or(String::new(), |e| format!("recipe `roll.frames.\"{e}\"`: "));
    let fault = match fault {
        RenderFault::Base(luminance) => format!(
            "the film base renders to luminance {} after the decode, scene correction and \
             the look, and display black needs a positive, finite one to place black \
             against.",
            if luminance == 0.0 {
                "0".into()
            } else {
                format!("{luminance:e}")
            }
        ),
        RenderFault::Overflow => format!(
            "the densest sample a scan can hold (a zero sample, read at the scan floor) \
             overflows f32 in the decode, scene correction or the look{}.",
            match r.calibration.film_base {
                Some(FilmBaseSource::Explicit(_)) => "",
                _ => ", with a film base read from a region taken at 1, where it decodes densest",
            }
        ),
    };
    Err(NcError::Usage(format!("{frame}{fault}{remedy}")))
}

/// Scene correction's value rules ([`SceneCorrectionParams::check`]), rendered as a
/// usage error naming the knob the way `names` says the command spells it.
///
/// The stated section is checked first, so a bad stated value is named as itself; then
/// the section the stage receives, with the roll's gains and exposure folded in
/// ([`Recipe::resolved_scene_correction`]) — only a combination can fail there, and the
/// message names every factor.
fn validate_scene_correction(r: &Recipe, names: KnobNames) -> Result<()> {
    let name = |flag: &str, key: &str| knob_name(names, "scene_correction", flag, key);
    scene_correction_fault(&r.scene_correction, names)?;
    // A gain no longer finite, a gain whose product with the exposure's is not a normal
    // f32, or two exposures whose sum is not.
    let roll = &r.roll;
    let frame = roll.applied_frame_exposure();
    if roll.white_balance.is_none() && roll.exposure.is_none() && frame.is_none() {
        return Ok(());
    }
    // Every exposure the stage sums, each named.
    let exposures = [
        roll.exposure
            .map(|_| knob_name(names, "roll", "--roll-exposure", "exposure")),
        frame.map(|_| match roll.applied_thin_exposure() {
            Some(_) => knob_name(names, "roll", "--roll-thin-exposure", "thin_exposure"),
            None => knob_name(names, "roll", "--roll-frame-exposure", "frame_exposure"),
        }),
        Some(name("--exposure", "exposure")),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" plus ");
    let (channel, gain) = match r.resolved_scene_correction().check() {
        Ok(()) => return Ok(()),
        Err(SceneFault::WhiteBalance { channel, value }) => (channel, value),
        Err(SceneFault::Combined { channel, gain }) => (channel, gain),
        // The sum of the exposures, each checked on its own already.
        Err(SceneFault::Exposure(ev)) => {
            return Err(NcError::Usage(format!(
                "{exposures} is {ev} EV, whose gain 2^EV is not a normal f32 (roughly -126 \
                 to +127 stops). Move the exposure toward 0"
            )));
        }
    };
    let white_balance = match roll.white_balance {
        Some(_) => format!(
            "{} times {}",
            knob_name(names, "roll", "--roll-white-balance", "white_balance"),
            name("--white-balance", "white_balance")
        ),
        None => name("--white-balance", "white_balance"),
    };
    Err(NcError::Usage(format!(
        "{white_balance} times the exposure gain from {exposures} is {gain:e} on channel \
         {channel}, which is not a normal f32 — every sample of that channel would render as \
         0 or inf. Move the white balance or the exposure toward neutral"
    )))
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
/// [`LookKeys::asks_for_a_look`], fit range on [`FitRangeSection::asks_for_a_fit`]. Each
/// spares its default, which every recipe carries, and its identity, which renders
/// exactly what the film master does (refusing that would kill the flags-win reset).
/// Fit gamut has no knob to ask with. Every stage asked for is named in one refusal, in
/// chain order, so removing one does not uncover the next.
pub fn destination(r: &Recipe, names: KnobNames) -> Result<Destination> {
    match &r.output {
        OutputSection::FilmMaster if r.rendering == Rendering::Direct => {
            // The remedy works whichever of the recipe or a flag stated `direct`: a flag
            // overrides the recipe's rendering.
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
    // The look's knobs as stated: the roll's slope is a measurement the film master
    // leaves unapplied, not a look the user asked for.
    if r.look.asks_for_a_look() {
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

/// One axis value as a message names it: the flag, or the key where no flag reaches.
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
            open,
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
            let what = match open {
                // Nothing resolved: the axis is open and none of its values is written.
                Some((flag, key)) => {
                    let axis = match names {
                        KnobNames::FlagAndKey => (*flag).to_string(),
                        KnobNames::KeyOnly => format!("`output.display.{key}`"),
                    };
                    format!(
                        "{stated} leaves {axis} open, and none of its choices is written yet; \
                         the first, {full},"
                    )
                }
                None if stated == full => full,
                None => format!("{stated} resolves to {full}, which"),
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
    /// The recipe's `params_hash` (`version::stable_hash`) over its pretty JSON — the
    /// `params` `--dump-params` writes, dedented — so a dumped recipe hashes to the run
    /// it came from. The report's `identity` and the telemetry record carry the same value.
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
            midtone: self.resolved_midtone(),
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
    /// any, moves into `roll.white_stops`, `roll.frame_exposure`, `roll.thin_slope` and
    /// `roll.thin_exposure`.
    /// `convert` and every `roll` frame go through it, so the two stay byte-identical. The entry is removed, not copied, so a flag
    /// that then beats it is what a `--dump-params` replay renders; the other entries
    /// stay for [`validate`].
    pub fn for_frame(mut self, input: &Path) -> Self {
        let name = input.file_name().and_then(|n| n.to_str());
        if let Some(frame) = name.and_then(|n| self.roll.frames.remove(n)) {
            if let Some(stops) = frame.white_stops {
                self.roll.white_stops = Some(stops);
            }
            if let Some(ev) = frame.exposure {
                self.roll.frame_exposure = Some(ev);
            }
            if let Some(slope) = frame.thin_slope {
                self.roll.thin_slope = Some(slope);
            }
            if let Some(ev) = frame.thin_exposure {
                self.roll.thin_exposure = Some(ev);
            }
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

    /// Whether the roll's gains and exposure reach scene correction: not under a rendering
    /// that leaves the roll out, nor on the film master, which runs no scene correction.
    pub fn applies_roll_to_scene_correction(&self) -> bool {
        self.output != OutputSection::FilmMaster && self.base().applies_roll
    }

    /// Scene correction as the stage receives it: the applied roll's gains multiplied
    /// into the stated white balance, and its exposure and the frame's added to the stated one — each
    /// the identity unless the user set it, so a roll's values alone reach the stage
    /// exactly. Both renderings start the white balance and the exposure at the identity.
    pub fn resolved_scene_correction(&self) -> SceneCorrectionParams {
        let SceneCorrectionParams {
            white_balance: WhiteBalance::Explicit(stated),
            exposure,
        } = self.scene_correction;
        let roll = self.applied_roll();
        let white_balance = match roll.white_balance {
            Some(gains) => std::array::from_fn(|c| gains[c] * stated[c]),
            None => stated,
        };
        SceneCorrectionParams {
            white_balance: WhiteBalance::Explicit(white_balance),
            exposure: [roll.exposure, roll.applied_frame_exposure()]
                .into_iter()
                .flatten()
                .fold(exposure, |sum, ev| ev + sum),
        }
    }

    /// The roll's midtone line as scene correction receives it: the applied roll's line,
    /// keyed on the roll's gains. [`validate`] refuses a line without gains.
    pub fn resolved_midtone(&self) -> Option<MidtoneCorrection> {
        let roll = self.applied_roll();
        roll.applied_midtone_line().map(|line| MidtoneCorrection {
            line,
            roll_gains: roll.white_balance.unwrap_or([1.0; 3]),
        })
    }

    /// The look's slope and its parts: the base — the applied roll's white, else the
    /// rendering's [`Base::slope`] — times the `look.contrast` multiplier.
    pub fn resolved_slope(&self) -> ResolvedSlope {
        let roll = self.applied_roll();
        let (base_slope, base_from) = match (roll.slope(), self.rendering) {
            (Some(s), _) if roll.applied_thin_slope().is_some() => (s, SlopeBase::Thin),
            (Some(s), _) => (s, SlopeBase::Roll),
            (None, Rendering::Default) => (self.base().slope, SlopeBase::Fallback),
            (None, Rendering::Direct) => (self.base().slope, SlopeBase::Direct),
        };
        ResolvedSlope {
            contrast: self.look.contrast,
            base_slope,
            base_from,
            slope: base_slope * self.look.contrast,
        }
    }

    /// The look as the stage receives it.
    pub fn resolved_look(&self) -> LookSection {
        self.look.resolve(
            self.resolved_slope().slope,
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
        (r.white_balance.is_some()
            || r.white_stops.is_some()
            || r.exposure.is_some()
            || r.frame_exposure.is_some()
            || r.thin_slope.is_some()
            || r.thin_exposure.is_some()
            || r.midtone_line.is_some())
        .then(|| RollReport {
            white_balance: r.white_balance,
            white_stops: r.white_stops,
            slope: r.slope(),
            exposure: r.exposure,
            frame_exposure: r.frame_exposure,
            thin_slope: r.thin_slope,
            thin_exposure: r.thin_exposure,
            white_balance_applied: applies && r.white_balance.is_some(),
            slope_applied: applies
                && matches!(
                    self.resolved_slope().base_from,
                    SlopeBase::Roll | SlopeBase::Thin
                ),
            exposure_applied: applies && r.exposure.is_some(),
            frame_exposure_applied: applies && r.applied_small_exposure().is_some(),
            thin_lift_applied: applies && r.applied_thin_slope().is_some(),
            midtone_line: r.midtone_line,
            midtone_neutral_applied: applies && r.applied_midtone_line().is_some(),
            taste_applied: if applies {
                r.taste_applied()
            } else {
                Vec::new()
            },
        })
    }

    /// The run's warnings about its recipe, emitted once per run (once per roll, from the
    /// shared recipe). `typed` spares what was given as a flag; empty for the film master.
    /// Why these and not refusals: design-spec §8, "Recipe warnings, not refusals".
    pub fn recipe_warnings(&self, typed: TypedStyle) -> Vec<String> {
        if self.output == OutputSection::FilmMaster {
            return Vec::new();
        }
        match self.rendering {
            Rendering::Default => {
                let mut w = self.roll_overlap_warnings(typed);
                w.extend(self.unmeasured_exposure_warning());
                w.extend(self.fallback_warning(typed));
                w
            }
            Rendering::Direct => self.direct_override_warning(typed).into_iter().collect(),
        }
    }

    /// `default` without a roll measurement: what fell back. Each half is silenced as
    /// white balance's is: a typed flag, even the identity, is a choice, and so is a
    /// recipe value off the identity — though both multiply the fallback.
    fn fallback_warning(&self, typed: TypedStyle) -> Option<String> {
        let mut fell_back = Vec::new();
        if !typed.white_balance
            && self.roll.white_balance.is_none()
            && self.scene_correction.white_balance == WhiteBalance::Explicit([1.0, 1.0, 1.0])
        {
            fell_back.push("neutral white balance (no `roll.white_balance`)".to_string());
        }
        if !typed.contrast
            && self.look.contrast == 1.0
            && self.resolved_slope().base_from == SlopeBase::Fallback
        {
            fell_back.push(format!(
                "the fallback slope {} (no `roll.white_stops`)",
                self.base().slope
            ));
        }
        (!fell_back.is_empty()).then(|| {
            format!(
                "no roll measurement: rendered with {}. Run `hanten measure-roll` over the \
                 roll and use the recipe it writes (its `roll` section); or state the white \
                 balance you want (`scene_correction.white_balance`) and the roll's white \
                 (`roll.white_stops`, which sets the base slope `look.contrast` multiplies) \
                 and exposure (`roll.exposure`); \
                 or use the `direct` rendering (`rendering`: \"direct\"), \
                 the decode without a roll correction, whose unset destination is the HDR \
                 float TIFF",
                fell_back.join(" and "),
            )
        })
    }

    /// `default`: a roll's gains or white without its exposure render at exposure 0, which
    /// leaves a thin roll dark. Only a stated exposure silences it, typed or not.
    fn unmeasured_exposure_warning(&self) -> Option<String> {
        let r = &self.roll;
        (r.exposure.is_none() && (r.white_balance.is_some() || r.white_stops.is_some())).then(
            || {
                "the roll section has no `roll.exposure`, so the roll renders at exposure 0 \
                 and an under-exposed roll stays dark. Run `hanten measure-roll` over the \
                 roll (a `roll.json` written before it measured the exposure has none), or \
                 state `roll.exposure` / `--roll-exposure` (0 keeps this render)"
                    .to_string()
            },
        )
    }

    /// `default`: a recipe's white balance or exposure beside the roll's value it
    /// multiplies or adds to.
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
        let stated = self.scene_correction.exposure;
        if !typed.exposure
            && let Some(roll) = self.roll.exposure
            && stated != 0.0
        {
            warnings.push(format!(
                "the recipe's `scene_correction.exposure` {stated} adds to the roll's \
                 exposure, `roll.exposure` {roll}{}: the exposure applied is {} EV. A stated \
                 exposure is an adjustment on top of the roll's measurement; if it is one \
                 chosen by hand before the roll's was measured, drop it",
                match (
                    self.roll.applied_thin_exposure(),
                    self.roll.applied_small_exposure()
                ) {
                    (Some(ev), _) => format!(", and this frame's, `roll.thin_exposure` {ev}"),
                    (None, Some(ev)) => format!(", and this frame's, `roll.frame_exposure` {ev}"),
                    (None, None) => String::new(),
                },
                self.resolved_scene_correction().exposure
            ));
        }
        warnings
    }

    /// `direct`: a recipe value that moves its pinned base and that an earlier build could
    /// have written unchosen — narrow on purpose (design-spec §8, "Recipe warnings").
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
                 measure-roll`, drop `scene_correction.white_balance`",
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
    use crate::pipeline::look::DEFAULT_SLOPE;

    fn parse(json: &str) -> std::result::Result<Recipe, serde_json::Error> {
        serde_json::from_str(json)
    }

    fn check(json: &str, whole: bool) -> std::result::Result<(), String> {
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        check_body(&v, whole, "recipe r.json").map_err(|e| e.message().to_string())
    }

    /// `json` with `err`'s suggested multiplier applied, the listed `roll` keys dropped,
    /// and version 3 stated — the migration a v2 refusal names, as a user follows it.
    fn converted(json: &str, err: &str, drop: &[&str]) -> Recipe {
        let k = stated_number(err, "state `look.contrast` ");
        let mut v: serde_json::Value = serde_json::from_str(json).unwrap();
        v["recipe_version"] = 3.into();
        v["look"]["contrast"] = k.into();
        for key in drop {
            v["roll"].as_object_mut().unwrap().remove(*key);
        }
        check(&v.to_string(), true).unwrap();
        serde_json::from_value(v).unwrap()
    }

    /// The number a message states right after `after`.
    fn stated_number(message: &str, after: &str) -> f32 {
        let rest =
            &message[message.find(after).unwrap_or_else(|| panic!("{message}")) + after.len()..];
        let end = rest.find(' ').unwrap_or(rest.len());
        rest[..end].parse().unwrap_or_else(|_| panic!("{message}"))
    }

    #[test]
    fn a_version_2_contrast_is_refused_with_the_multiplier_that_keeps_its_slope() {
        // Version 2's number was the slope itself; read as a multiplier it would render
        // differently in silence. Each case: the recipe, the stated slope, its base.
        let direct = crate::rendering::DIRECT.slope;
        for (json, slope, base) in [
            (
                r#"{"recipe_version": 2, "look": {"contrast": 1.1111112}}"#,
                1.111_111_2,
                DEFAULT_SLOPE,
            ),
            (
                r#"{"recipe_version": 2, "roll": {"white_stops": 1.6}, "look": {"contrast": 1.8}}"#,
                1.8,
                roll_white::slope_for(1.6),
            ),
            (
                r#"{"recipe_version": 2, "rendering": "direct", "roll": {"white_stops": 1.6},
                    "look": {"contrast": 1.3}}"#,
                1.3,
                direct,
            ),
            // Awkward quotients: the multiplier is picked so the product is exact.
            (
                r#"{"recipe_version": 2, "roll": {"white_stops": 1.7}, "look": {"contrast": 1.1111112}}"#,
                1.111_111_2,
                roll_white::slope_for(1.7),
            ),
            (
                r#"{"recipe_version": 2, "roll": {"white_stops": 1.93}, "look": {"contrast": 1.37}}"#,
                1.37,
                roll_white::slope_for(1.93),
            ),
        ] {
            let err = check(json, true).unwrap_err();
            assert!(
                err.contains("is the slope itself")
                    && err.contains("multiplier on the base slope")
                    && err.contains("`\"recipe_version\": 3`")
                    && err.contains("drop it"),
                "{err}"
            );
            // The remedy works: the stated multiplier, in a version 3 recipe, renders the
            // old slope — bit for bit when the message says "as before", else within one
            // f32 step, which it says too.
            let r = converted(json, &err, &[]);
            let got = r.resolved_slope();
            assert_eq!(got.base_slope, base, "{json}");
            if err.contains("To render as before") {
                assert_eq!(got.slope, slope, "{json}: {err}");
            } else {
                let step = f32::from_bits(slope.to_bits() + 1) - slope;
                assert!(
                    (got.slope - slope).abs() <= step && err.contains("within one f32 step"),
                    "{json}: {err}"
                );
            }
        }
        // Both forms are reached: the value earlier builds wrote converts exactly.
        let err = check(
            r#"{"recipe_version": 2, "look": {"contrast": 1.1111112}}"#,
            true,
        )
        .unwrap_err();
        assert!(err.contains("To render as before"), "{err}");
        // With `roll.frames` the old slope overrode every frame's white, so the exact
        // conversion drops the roll's whites: every frame, clamped or not, renders it.
        let frames = r#"{"recipe_version": 2,
            "roll": {"white_stops": 1.6, "frames": {"f07.tif": {"white_stops": 2.0}}},
            "look": {"contrast": 1.8}}"#;
        let err = check(frames, true).unwrap_err();
        assert!(
            err.contains("drop `roll.white_stops` and `roll.frames`")
                && err.contains("over the fallback slope"),
            "{err}"
        );
        let r = converted(frames, &err, &["white_stops", "frames"]);
        let step = f32::from_bits(1.8f32.to_bits() + 1) - 1.8;
        for input in ["f01.tif", "f07.tif"] {
            let frame = r.clone().for_frame(Path::new(input));
            assert!(
                (frame.resolved_slope().slope - 1.8).abs() <= step,
                "{input}: {err}"
            );
        }
        // A number no multiplier can keep gets the general remedy, never one validation
        // would refuse.
        for bad in ["0", "-1.2"] {
            let json = format!(r#"{{"recipe_version": 2, "look": {{"contrast": {bad}}}}}"#);
            let err = check(&json, true).unwrap_err();
            assert!(
                err.contains("State a positive multiplier")
                    && !err.contains("state `look.contrast`"),
                "{err}"
            );
        }
        // Nothing ambiguous reads unchanged: no contrast, or the `null` a version 2 dump
        // wrote for unset.
        for json in [
            r#"{"recipe_version": 2}"#,
            r#"{"recipe_version": 2, "look": {"contrast": null}}"#,
        ] {
            check(json, true).unwrap();
            assert_eq!(parse(json).unwrap().look.contrast, 1.0, "{json}");
        }
        // A per-frame override is held to the same rule; its base is the shared recipe's,
        // so it is pointed at the report rather than handed a number. One stating no
        // version cannot say which meaning it has, so it must state one.
        let overlay = r#"{"recipe_version": 2, "look": {"contrast": 1.3}}"#;
        let err = check(overlay, false).unwrap_err();
        assert!(
            err.contains("is the slope itself") && err.contains("`chain.look.base_slope`"),
            "{err}"
        );
        let err = check(r#"{"look": {"contrast": 1.3}}"#, false).unwrap_err();
        assert!(
            err.contains("states no `recipe_version`")
                && err.contains("`\"recipe_version\": 3` beside it"),
            "{err}"
        );
        check(r#"{"recipe_version": 3, "look": {"contrast": 1.3}}"#, false).unwrap();
        check(r#"{"look": {"contrast": null}}"#, false).unwrap();
        // Version 3 states a multiplier, and a version this build never wrote is refused.
        check(r#"{"recipe_version": 3, "look": {"contrast": 1.3}}"#, true).unwrap();
        let err = check(r#"{"recipe_version": 4}"#, true).unwrap_err();
        assert!(err.contains("this build reads 3 and 2"), "{err}");
        // A version 2 document is written back as version 3.
        let text = serde_json::to_string(&parse(r#"{"recipe_version": 2}"#).unwrap()).unwrap();
        assert!(text.starts_with(r#"{"recipe_version":3,"#), "{text}");
    }

    #[test]
    fn a_look_is_asked_for_only_by_a_knob_off_its_default_and_off_the_identity() {
        let asks = |json: &str| {
            parse(&format!(r#"{{"recipe_version": 3, "look": {json}}}"#))
                .unwrap()
                .look
                .asks_for_a_look()
        };
        // Spared: the defaults, and highlight desaturation off however its inert knobs sit
        // — the flags-win reset. A stated default strength is spared too.
        for spared in [
            "{}",
            r#"{"contrast": 1}"#,
            r#"{"channel_grade": [1, 1]}"#,
            r#"{"highlight_desaturation": {"strength": 0}}"#,
            r#"{"highlight_desaturation": {"strength": 0, "band": [0.02, 0.04]}}"#,
            r#"{"highlight_desaturation": {"strength": 0.8}}"#,
        ] {
            assert!(!asks(spared), "{spared}");
        }
        // Asked: any knob moved off both.
        for asked in [
            r#"{"contrast": 1.3}"#,
            r#"{"contrast": 0.9}"#,
            r#"{"channel_grade": [1.1, 0.9]}"#,
            r#"{"highlight_desaturation": {"strength": 0.5}}"#,
            r#"{"highlight_desaturation": {"band": [0.02, 0.04]}}"#,
        ] {
            assert!(asks(asked), "{asked}");
        }
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
        // unstated — highlight desaturation, fit range's headroom and display black — and
        // so is the roll section; the rest are written at their identity (scene
        // correction, the look's contrast multiplier, the grade). Keys are written, never left out, so a
        // per-frame override merges. (Fit gamut's map runs at every setting; it simply
        // has nothing for a recipe to set.)
        assert_eq!(json["rendering"], "default");
        assert_eq!(json["fit_gamut"], serde_json::json!({}));
        // Nothing stated: every axis is derived, so a written recipe states none.
        assert_eq!(json["output"], serde_json::json!({"display": {}}));
        assert_eq!(
            serde_json::to_string(&Recipe::default().look).unwrap(),
            r#"{"contrast":1.0,"channel_grade":[1.0,1.0],"highlight_desaturation":{"strength":null,"start_stops":null,"band":null}}"#
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
            serde_json::json!({"white_balance": null, "white_stops": null, "exposure": null,
                "frame_exposure": null, "small_lift": null, "thin_slope": null,
                "thin_exposure": null, "thin_lift": null, "midtone_line": null,
                "midtone_neutral": null, "frames": {}})
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
            r#"{"recipe_version": 3, "reconstruction": {"linearization": 3.1},
                "look": {"contrast": 1.2, "highlight_desaturation": {"strength": 0.5}}}"#,
        )
        .unwrap();
        let look = r
            .chain_params(DisplayPeak::SDR, DestinationGamut::DisplayP3)
            .shared
            .look;
        assert_eq!(look.linearization, 3.1);
        assert_eq!(look.section.slope, DEFAULT_SLOPE * 1.2);
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
        let plain = decoded(r#"{"recipe_version": 3}"#);
        assert_eq!(
            plain,
            decoded(r#"{"recipe_version": 3, "look": {"contrast": 1.6}}"#)
        );
        assert_ne!(
            plain,
            decoded(r#"{"recipe_version": 3, "reconstruction": {"linearization": 2.0}}"#)
        );
    }

    #[test]
    fn a_partial_recipe_takes_the_defaults_it_omits() {
        let r =
            parse(r#"{"recipe_version": 3, "reconstruction": {"linearization": 1.7}}"#).unwrap();
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
        let err = parse(r#"{"recipe_version": 4}"#).unwrap_err().to_string();
        assert!(err.contains("reads 3 and 2"), "{err}");
    }

    #[test]
    fn a_replayed_avif_container_is_refused_by_key_with_its_remedy() {
        // A report or sidecar written while AVIF existed replays `"container": "avif"`;
        // a per-frame override may state it too. Refused before serde, by key: the flag
        // cannot rescue a recipe that fails before merge.
        for (json, whole) in [
            (
                r#"{"recipe_version": 3, "output": {"display": {"transfer": "pq", "container": "avif"}}}"#,
                true,
            ),
            (r#"{"output": {"display": {"container": "AVIF"}}}"#, false),
        ] {
            let err = check(json, whole).unwrap_err();
            assert!(err.contains("`output.display.container` \""), "{err}");
            assert!(err.contains("no longer writes AVIF"), "{err}");
            assert!(err.contains(r#"State `"container": "tiff"`"#), "{err}");
            assert!(!err.contains("--container"), "{err}");
            assert!(!err.contains("unknown"), "{err}");
        }
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
            let json = format!(r#"{{"recipe_version": 3, "{section}": {{"nonsense": 1}}}}"#);
            let err = parse(&json).unwrap_err().to_string();
            assert!(err.contains("nonsense"), "{section}: {err}");
        }
        let err = parse(r#"{"recipe_version": 3, "nonsense": {}}"#)
            .unwrap_err()
            .to_string();
        assert!(err.contains("nonsense"), "{err}");
    }

    #[test]
    fn a_roll_frames_white_is_judged_at_the_slope_it_renders_with_the_contrast() {
        // Each frame renders at its own white's slope times the multiplier, so an entry
        // that fits alone can overflow once multiplied — refused by name, not left for
        // the look stage to fail on.
        let json = r#"{"recipe_version": 3, "roll": {"white_stops": 1.6,
            "frames": {"a.tif": {"white_stops": 1e-37}}}, "look": {"contrast": 100}}"#;
        let r = parse(json).unwrap();
        validate(
            &parse(&json.replace("100", "1")).unwrap(),
            KnobNames::FlagAndKey,
        )
        .unwrap();
        let err = validate(&r, KnobNames::FlagAndKey).unwrap_err();
        let msg = err.message();
        assert!(
            msg.contains("`roll.frames.\"a.tif\".white_stops`")
                && msg.contains("--contrast (recipe `look.contrast`) 100")
                && msg.contains("overflows"),
            "{msg}"
        );
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
        // A recipe layer or a frame's manifest `params` may carry it, so the remedy is a
        // recipe value.
        for whole in [true, false] {
            let body = if whole {
                r#"{"recipe_version": 3, "calibration": {"film_base": "auto"}}"#
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
        assert!(err.contains("\"recipe_version\": 3"), "{err}");
        assert!(err.contains("`hanten params`"), "{err}");
        assert!(err.contains("pipeline_version` 8"), "{err}");
        assert!(!err.contains("--new-flow"), "{err}");
        // A per-frame overlay is partial and may omit it…
        check(base, false).unwrap();
        // …but may not state a wrong one.
        let err = check(r#"{"recipe_version": 1}"#, false).unwrap_err();
        assert!(err.contains("reads 3 and 2"), "{err}");
    }

    #[test]
    fn an_old_section_is_refused_by_name_with_where_it_went() {
        let err = check(r#"{"recipe_version": 3, "print": {}}"#, true).unwrap_err();
        assert!(
            err.contains("`print`") && err.contains("scene_correction"),
            "{err}"
        );
        // `output` is shared by name, so the removed chain's keys under it are named
        // one by one, pointing at the destination's axes.
        for key in OLD_OUTPUT_KEYS {
            let body = format!(r#"{{"recipe_version": 3, "output": {{"{key}": "x"}}}}"#);
            let err = check(&body, true).unwrap_err();
            assert!(
                err.contains(&format!("`output.{key}`")) && err.contains("output.display"),
                "{err}"
            );
        }
        check(
            r#"{"recipe_version": 3, "output": {"display": {"gamut": "adobe-rgb"}}}"#,
            true,
        )
        .unwrap();
        let err = check(
            r#"{"recipe_version": 3, "reconstruction": {"density": {"scale": [1, 1, 1]}}}"#,
            true,
        )
        .unwrap_err();
        assert!(err.contains("reconstruction.density") && err.contains("reconstruction.scale"));
        let err = check(
            r#"{"recipe_version": 3, "reconstruction": {"curve": {"type": "exponential"}}}"#,
            true,
        )
        .unwrap_err();
        assert!(err.contains("reconstruction.linearization"), "{err}");
        let err = check(
            r#"{"recipe_version": 3, "calibration": {"dmax": "fixed"}}"#,
            true,
        )
        .unwrap_err();
        assert!(err.contains("calibration.dmax") && err.contains("reference-free"));
        // The levels remap retired rather than moved: the message says so and names
        // the two knobs that cover it.
        let err = check(
            r#"{"recipe_version": 3, "print": {"linear_range": [0.1, 0.9]}}"#,
            true,
        )
        .unwrap_err();
        assert!(
            err.contains("`linear_range`, the levels remap, retired")
                && err.contains("scene_correction.exposure")
                && err.contains("fit_range.display_black"),
            "{err}"
        );
        assert!(!err.contains("no home"), "{err}");
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
                    || (section == "output" && OLD_OUTPUT_KEYS.contains(&key.as_str()))
                    || RETIRED_KEYS
                        .iter()
                        .any(|(path, _)| path[..] == [section.as_str(), key.as_str()]);
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
        // `measure-roll` and a roll frame's manifest entry take no conversion flags, so
        // naming one there is a remedy the user cannot type.
        let mut r = Recipe::default();
        r.reconstruction.linearization = 0.0;
        let msg = validate(&r, KnobNames::KeyOnly).unwrap_err();
        let msg = msg.message();
        assert!(msg.contains("`reconstruction.linearization`"), "{msg}");
        assert!(!msg.contains("--density-gamma"), "{msg}");
        let mut r = Recipe::default();
        r.look.contrast = 0.0;
        let msg = validate(&r, KnobNames::KeyOnly).unwrap_err();
        let msg = msg.message();
        assert!(msg.contains("`look.contrast`"), "{msg}");
        assert!(!msg.contains("--contrast"), "{msg}");
    }

    #[test]
    fn validate_refuses_an_unusable_look_contrast() {
        for bad in [0.0, -1.1, f32::NAN, f32::INFINITY] {
            let mut r = Recipe::default();
            r.look.contrast = bad;
            let msg = validate(&r, KnobNames::FlagAndKey).unwrap_err();
            assert!(
                msg.message()
                    .contains("--contrast (recipe `look.contrast`) must be finite and positive")
                    && msg.message().contains("multiplier"),
                "{bad}: {}",
                msg.message()
            );
        }
        let mut r = Recipe::default();
        r.look.contrast = 1.0;
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
        // once, and no single new key replays it. The remedy states the multiplier that
        // keeps the stated slope as the whole slope, over the recipe's own base.
        for (json, multiplier) in [
            (
                r#"{"recipe_version": 3, "reconstruction": {"contrast": 2.0}}"#,
                2.0 / (LINEARIZATION * DEFAULT_SLOPE),
            ),
            (
                r#"{"recipe_version": 3, "reconstruction": {"contrast": 3.6}}"#,
                3.6 / (LINEARIZATION * DEFAULT_SLOPE),
            ),
            (
                r#"{"recipe_version": 3, "roll": {"white_stops": 1.6},
                    "reconstruction": {"contrast": 3.6}}"#,
                3.6 / (LINEARIZATION * roll_white::slope_for(1.6)),
            ),
        ] {
            let err = check(json, true).unwrap_err();
            assert!(
                err.contains("reconstruction.linearization") && err.contains("look.contrast"),
                "{err}"
            );
            let stated = stated_number(&err, "write `look.contrast` ");
            assert!((stated - multiplier).abs() < 1e-5, "{json}: {err}");
        }
        // In a version 2 document the remedy moves it to 3 too, or the new multiplier
        // would be read as a version 2 slope.
        let err = check(
            r#"{"recipe_version": 2, "reconstruction": {"contrast": 2.4}}"#,
            true,
        )
        .unwrap_err();
        assert!(err.contains("state `\"recipe_version\": 3`"), "{err}");
        let err = check(
            r#"{"recipe_version": 3, "reconstruction": {"contrast": 2.4}}"#,
            true,
        )
        .unwrap_err();
        assert!(!err.contains("recipe_version"), "{err}");
        // In a per-frame override the base is the shared recipe's, so the remedy points
        // at the frame's report instead of computing a number, and states the version an
        // override needs beside a `look.contrast` number. Followed, it passes.
        let err = check(r#"{"reconstruction": {"contrast": 2.0}}"#, false).unwrap_err();
        assert!(
            err.contains("`chain.look.base_slope`")
                && err.contains("state `\"recipe_version\": 3`")
                && !err.contains("over the fallback slope"),
            "{err}"
        );
        check(r#"{"recipe_version": 3, "look": {"contrast": 1.0}}"#, false).unwrap();
        // A value whose quotient validation would refuse (zero, subnormal or infinite
        // in f32) gets the generic remedy, never a `look.contrast` it would then refuse.
        for json in [
            r#"{"recipe_version": 3, "reconstruction": {"contrast": "steep"}}"#,
            r#"{"recipe_version": 3, "reconstruction": {"contrast": 1e-50}}"#,
            r#"{"recipe_version": 3, "reconstruction": {"contrast": 1e-39}}"#,
            r#"{"recipe_version": 3, "reconstruction": {"contrast": 1e39}}"#,
            r#"{"recipe_version": 3, "reconstruction": {"contrast": -2.0}}"#,
        ] {
            let err = check(json, true).unwrap_err();
            assert!(err.contains("state `look.contrast`"), "{json}: {err}");
            assert!(!err.contains("write `look.contrast`"), "{json}: {err}");
        }
    }

    #[test]
    fn validate_refuses_a_whole_slope_outside_f32() {
        // Each factor passes its own rule; the product, which highlight desaturation
        // divides by, does not. The remedy follows the direction.
        let with = |linearization: f32, contrast: f32, names| {
            let mut r = Recipe::default();
            r.reconstruction.linearization = linearization;
            r.look.contrast = contrast;
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
        merge(parse(json).unwrap(), &cli_flags(extra))
    }

    /// `convert`'s conversion flags from `extra`.
    fn cli_flags(extra: &[&str]) -> crate::cli::ConversionFlags {
        use crate::cli::{Cli, Command};
        use clap::Parser;
        let argv = ["hanten", "convert", "in.tif", "-o", "out"]
            .iter()
            .chain(extra)
            .copied();
        let Command::Convert(args) = Cli::try_parse_from(argv).unwrap().command else {
            unreachable!()
        };
        args.knobs
    }

    #[test]
    fn a_roll_exposure_is_checked_alone_then_as_a_sum() {
        let err = |json: &str, extra: &[&str]| {
            validate(&merged(json, extra), KnobNames::FlagAndKey)
                .unwrap_err()
                .message()
                .to_string()
        };
        let msg = err(r#"{"recipe_version": 3}"#, &["--roll-exposure", "200"]);
        assert!(
            msg.starts_with("--roll-exposure (recipe `roll.exposure`) must be finite"),
            "{msg}"
        );
        // Each usable alone; their sum is not, and the message names both.
        let msg = err(
            r#"{"recipe_version": 3, "roll": {"exposure": 100}}"#,
            &["--exposure", "100"],
        );
        assert!(
            msg.starts_with(
                "--roll-exposure (recipe `roll.exposure`) plus --exposure (recipe \
                 `scene_correction.exposure`) is 200 EV"
            ),
            "{msg}"
        );
        // Under `direct` the roll is left out, so only the stated exposure counts.
        let r = merged(
            r#"{"recipe_version": 3, "rendering": "direct", "roll": {"exposure": 100}}"#,
            &["--exposure", "100"],
        );
        validate(&r, KnobNames::FlagAndKey).unwrap();
        assert_eq!(r.resolved_scene_correction().exposure, 100.0);
    }

    #[test]
    fn a_frames_lift_adds_to_the_roll_unless_turned_off_or_left_out() {
        let roll = r#"{"recipe_version": 3, "roll": {"exposure": 1.0,
            "frames": {"a.tif": {"white_stops": null, "exposure": 0.25}}}}"#;
        let frame = |extra: &[&str]| {
            let r = parse(roll).unwrap().for_frame(Path::new("/scans/a.tif"));
            merge(r, &cli_flags(extra))
        };
        // `for_frame` moves the entry's lift into `roll.frame_exposure`, as it moves a white.
        let r = frame(&[]);
        assert_eq!(r.roll.frame_exposure, Some(0.25));
        assert!(r.roll.frames.is_empty());
        assert_eq!(r.resolved_scene_correction().exposure, 1.25);
        let report = r.roll_report(true).unwrap();
        assert!(report.frame_exposure_applied, "{report:?}");
        // Another frame is not lifted.
        let other = merged(roll, &[]).for_frame(Path::new("b.tif"));
        assert_eq!(other.resolved_scene_correction().exposure, 1.0);
        // Off keeps the value and drops it from the sum; `direct` leaves the roll out.
        let off = frame(&["--small-lift", "off"]);
        assert_eq!(off.roll.frame_exposure, Some(0.25));
        assert_eq!(off.resolved_scene_correction().exposure, 1.0);
        assert!(!off.roll_report(true).unwrap().frame_exposure_applied);
        let direct = frame(&["--rendering", "direct"]);
        assert_eq!(direct.resolved_scene_correction().exposure, 0.0);
    }

    #[test]
    fn a_thin_lift_replaces_the_small_one_and_each_switch_turns_off_its_own() {
        let roll = r#"{"recipe_version": 3, "roll": {"white_stops": 1.5, "exposure": 1.0,
            "frames": {"a.tif": {"exposure": 0.25, "thin_slope": 2.0, "thin_exposure": 0.7}}}}"#;
        let frame = |extra: &[&str]| {
            let r = parse(roll).unwrap().for_frame(Path::new("/scans/a.tif"));
            merge(r, &cli_flags(extra))
        };
        let roll_slope = (roll_white::slope_for(1.5), SlopeBase::Roll);
        let shape = |r: &Recipe| {
            let s = r.resolved_slope();
            let report = r.roll_report(true).unwrap();
            (
                (s.base_slope, s.base_from),
                r.resolved_scene_correction().exposure,
                report.taste_applied,
            )
        };
        let r = frame(&[]);
        assert_eq!(
            (r.roll.thin_slope, r.roll.thin_exposure),
            (Some(2.0), Some(0.7))
        );
        assert_eq!(shape(&r), ((2.0, SlopeBase::Thin), 1.7, vec!["thin_lift"]));
        let report = r.roll_report(true).unwrap();
        assert!(
            report.thin_lift_applied && report.slope_applied && !report.frame_exposure_applied,
            "{report:?}"
        );
        // Thin off renders the small lift; small off leaves the thin one alone.
        assert_eq!(
            shape(&frame(&["--thin-lift", "off"])),
            (roll_slope, 1.25, vec!["small_lift"])
        );
        assert_eq!(
            shape(&frame(&["--small-lift", "off"])),
            ((2.0, SlopeBase::Thin), 1.7, vec!["thin_lift"])
        );
        let off = frame(&["--small-lift", "off", "--thin-lift", "off"]);
        assert_eq!(shape(&off), (roll_slope, 1.0, vec![]));
        assert_eq!(off.roll.thin_slope, Some(2.0), "off keeps the value");
        // `look.contrast` multiplies the thin slope, as it does the roll's.
        assert_eq!(
            frame(&["--contrast", "1.1"]).resolved_slope().slope,
            2.0 * 1.1
        );
        // Another frame keeps the roll's.
        let other = merged(roll, &[]).for_frame(Path::new("b.tif"));
        assert_eq!(other.resolved_slope().base_from, SlopeBase::Roll);
        // A typed white beats the whole thin lift, and the small lift stays.
        let white = frame(&["--roll-white", "1.9"]);
        assert_eq!(
            shape(&white),
            (
                (roll_white::slope_for(1.9), SlopeBase::Roll),
                1.25,
                vec!["small_lift"]
            )
        );
        assert_eq!(white.roll.thin_exposure, None);
        // `direct` leaves the roll out, and applies no taste.
        let direct = frame(&["--rendering", "direct"]);
        assert_eq!(direct.resolved_slope().base_from, SlopeBase::Direct);
        assert!(direct.roll_report(true).unwrap().taste_applied.is_empty());
    }

    #[test]
    fn a_thin_lift_is_checked_by_its_own_name() {
        let err = |json: &str, extra: &[&str]| {
            let r = merged(json, extra);
            validate(&r, KnobNames::FlagAndKey)
                .unwrap_err()
                .message()
                .to_string()
        };
        let msg = err(
            r#"{"recipe_version": 3, "roll": {"frames": {"a.tif": {"thin_slope": -1}}}}"#,
            &[],
        );
        assert!(
            msg.starts_with(
                "recipe `roll.frames.\"a.tif\".thin_slope` must be finite and positive"
            ),
            "{msg}"
        );
        let msg = err(r#"{"recipe_version": 3}"#, &["--roll-thin-slope", "0"]);
        assert!(
            msg.starts_with("--roll-thin-slope (recipe `roll.thin_slope`) must be finite"),
            "{msg}"
        );
        // A whole slope that overflows names the thin slope, to be made smaller.
        let msg = err(r#"{"recipe_version": 3}"#, &["--roll-thin-slope", "3e38"]);
        assert!(
            msg.contains("or a smaller --roll-thin-slope (recipe `roll.thin_slope`)"),
            "{msg}"
        );
        // The exposure applies only beside its slope.
        let msg = err(r#"{"recipe_version": 3}"#, &["--roll-thin-exposure", "0.5"]);
        assert!(
            msg.starts_with("--roll-thin-exposure (recipe `roll.thin_exposure`) is the exposure")
                && msg.contains("state --roll-thin-slope (recipe `roll.thin_slope`) too"),
            "{msg}"
        );
        let msg = err(
            r#"{"recipe_version": 3, "roll": {"frames": {"a.tif": {"thin_exposure": 0.5}}}}"#,
            &[],
        );
        assert!(
            msg.starts_with("recipe `roll.frames.\"a.tif\".thin_exposure` is the exposure"),
            "{msg}"
        );
    }

    #[test]
    fn the_retired_frame_slope_names_the_thin_pair() {
        let loaded = |body: &str| -> std::result::Result<Recipe, String> {
            let mut v: serde_json::Value = serde_json::from_str(body).unwrap();
            strip_retired_nulls(&mut v);
            check_body(&v, true, "recipe r.json").map_err(|e| e.message().to_string())?;
            Ok(serde_json::from_value(v).unwrap())
        };
        // The default an earlier build wrote replays as if absent.
        let old = r#"{"recipe_version": 3, "roll": {"frame_slope": null, "white_stops": 1.5,
            "frames": {"a.tif": {"exposure": 0.2, "slope": null}}}}"#;
        let new = r#"{"recipe_version": 3, "roll": {"white_stops": 1.5,
            "frames": {"a.tif": {"exposure": 0.2}}}}"#;
        assert_eq!(loaded(old).unwrap(), parse(new).unwrap());
        // Any other value names the rename, which replays the old render: beside a slope
        // the exposure was the thin lift's.
        let thin = |r: Recipe| {
            let r = r.for_frame(Path::new("a.tif"));
            (
                r.resolved_slope().base_slope,
                r.resolved_scene_correction().exposure,
            )
        };
        for (body, renamed) in [
            (
                r#"{"recipe_version": 3, "roll": {"white_stops": 1.5, "exposure": 1.0,
                    "frames": {"a.tif": {"exposure": 0.6, "slope": 2}}}}"#,
                r#"{"recipe_version": 3, "roll": {"white_stops": 1.5, "exposure": 1.0,
                    "frames": {"a.tif": {"thin_exposure": 0.6, "thin_slope": 2}}}}"#,
            ),
            (
                r#"{"recipe_version": 3, "roll": {"white_stops": 1.5, "exposure": 1.0,
                    "frame_exposure": 0.6, "frame_slope": 2}}"#,
                r#"{"recipe_version": 3, "roll": {"white_stops": 1.5, "exposure": 1.0,
                    "thin_exposure": 0.6, "thin_slope": 2}}"#,
            ),
        ] {
            let msg = loaded(body).unwrap_err();
            assert!(
                msg.contains("is not a recipe key any more")
                    && msg.contains("To render as before, rename")
                    && msg.contains("hanten measure-roll --out"),
                "{msg}"
            );
            assert!(!msg.contains("drop the key"), "{msg}");
            assert_eq!(thin(loaded(renamed).unwrap()), (2.0, 1.6), "{renamed}");
        }
        let msg = loaded(
            r#"{"recipe_version": 3, "roll": {"frames": {"a.tif": {"exposure": 0.6, "slope": 2}}}}"#,
        )
        .unwrap_err();
        assert!(
            msg.contains(
                r#"rename `roll.frames."a.tif".slope` to `thin_slope` and `roll.frames."a.tif".exposure`, if stated, to `thin_exposure`"#
            ),
            "{msg}"
        );
        assert!(!msg.contains("roll.frame_lift"), "{msg}");
        // Beside the retired `frame_lift` "off", which turned both lifts off, the message
        // migrates both, and followed it renders no lift, as before.
        let msg = loaded(
            r#"{"recipe_version": 3, "roll": {"white_stops": 1.5, "exposure": 1.0,
                "frame_lift": "off", "frames": {"a.tif": {"exposure": 0.6, "slope": 2}}}}"#,
        )
        .unwrap_err();
        assert!(
            msg.contains(
                "and replace `roll.frame_lift` \"off\" with `roll.small_lift` \"off\" and \
                 `roll.thin_lift` \"off\""
            ),
            "{msg}"
        );
        assert!(
            !msg.starts_with("recipe r.json: `roll.frame_lift`"),
            "{msg}"
        );
        let renamed = loaded(
            r#"{"recipe_version": 3, "roll": {"white_stops": 1.5, "exposure": 1.0,
                "small_lift": "off", "thin_lift": "off",
                "frames": {"a.tif": {"thin_exposure": 0.6, "thin_slope": 2}}}}"#,
        )
        .unwrap();
        assert_eq!(thin(renamed), (roll_white::slope_for(1.5), 1.0));
    }

    #[test]
    fn the_retired_frame_lift_names_both_switches() {
        let loaded = |body: &str| -> std::result::Result<Recipe, String> {
            let mut v: serde_json::Value = serde_json::from_str(body).unwrap();
            strip_retired_nulls(&mut v);
            check_body(&v, true, "recipe r.json").map_err(|e| e.message().to_string())?;
            Ok(serde_json::from_value(v).unwrap())
        };
        // `null`, the default every earlier file wrote, loads as if absent.
        assert_eq!(
            loaded(r#"{"recipe_version": 3, "roll": {"frame_lift": null}}"#).unwrap(),
            parse(r#"{"recipe_version": 3}"#).unwrap()
        );
        let msg = loaded(r#"{"recipe_version": 3, "roll": {"frame_lift": "off"}}"#).unwrap_err();
        assert!(
            msg.contains("`roll.frame_lift` is not a recipe key any more")
                && msg.contains(
                    "replace `roll.frame_lift` \"off\" with `roll.small_lift` \"off\" and \
                     `roll.thin_lift` \"off\""
                ),
            "{msg}"
        );
        let msg = loaded(r#"{"recipe_version": 3, "roll": {"frame_lift": "on"}}"#).unwrap_err();
        assert!(
            msg.contains(
                "replace `roll.frame_lift` \"on\" with `roll.small_lift` \"on\" and \
                 `roll.thin_lift` \"on\""
            ),
            "{msg}"
        );
        // Followed, the old "off" renders neither lift on a thin frame, as before.
        let r = loaded(
            r#"{"recipe_version": 3, "roll": {"white_stops": 1.5, "exposure": 1.0,
                "small_lift": "off", "thin_lift": "off", "frames": {"a.tif":
                {"exposure": 0.25, "thin_slope": 2, "thin_exposure": 0.7}}}}"#,
        )
        .unwrap()
        .for_frame(Path::new("a.tif"));
        assert_eq!(
            (
                r.resolved_slope().base_from,
                r.resolved_scene_correction().exposure
            ),
            (SlopeBase::Roll, 1.0)
        );
    }

    #[test]
    fn a_frames_lift_is_checked_by_its_own_name_then_in_the_sum() {
        let err = |json: &str, extra: &[&str]| {
            let r = merged(json, extra);
            validate(&r, KnobNames::FlagAndKey)
                .unwrap_err()
                .message()
                .to_string()
        };
        let msg = err(
            r#"{"recipe_version": 3, "roll": {"frames": {"a.tif": {"exposure": 1e9}}}}"#,
            &[],
        );
        assert!(
            msg.starts_with("recipe `roll.frames.\"a.tif\".exposure` must be finite"),
            "{msg}"
        );
        let msg = err(
            r#"{"recipe_version": 3, "roll": {"frames": {"a.tif": {}}}}"#,
            &[],
        );
        assert!(
            msg.starts_with("recipe `roll.frames.\"a.tif\"` states nothing"),
            "{msg}"
        );
        let msg = err(
            r#"{"recipe_version": 3, "roll": {"exposure": 100, "frame_exposure": 20}}"#,
            &["--exposure", "10"],
        );
        assert!(
            msg.starts_with(
                "--roll-exposure (recipe `roll.exposure`) plus --roll-frame-exposure (recipe \
                 `roll.frame_exposure`) plus --exposure (recipe `scene_correction.exposure`) \
                 is 130 EV"
            ),
            "{msg}"
        );
        // Off, the lift leaves the sum, and the same values are usable.
        let r = merged(
            r#"{"recipe_version": 3, "roll": {"exposure": 100, "frame_exposure": 20}}"#,
            &["--exposure", "10", "--small-lift", "off"],
        );
        validate(&r, KnobNames::FlagAndKey).unwrap();
    }

    #[test]
    fn the_roll_section_reaches_the_stages_and_the_contrast_multiplies_its_slope() {
        let roll = r#"{"recipe_version": 3,
                       "roll": {"white_balance": [0.8, 1.0, 1.25], "white_stops": 1.6}}"#;
        // The gains multiply the stated white balance; the white sets the contrast.
        let r = merged(roll, &["--white-balance", "1.25,1,1"]);
        let shared = r.shared_params();
        assert_eq!(
            shared.scene_correction.white_balance,
            WhiteBalance::Explicit([0.8 * 1.25, 1.0, 1.25])
        );
        let from_white = roll_white::slope_for(1.6);
        assert_eq!(shared.look.section.slope, from_white);
        assert_eq!(
            r.resolved_slope(),
            ResolvedSlope {
                contrast: 1.0,
                base_slope: from_white,
                base_from: SlopeBase::Roll,
                slope: from_white,
            }
        );
        // Alone, the gains reach the stage exactly: the identity multiplies nothing in.
        let alone = merged(roll, &[]).shared_params();
        assert_eq!(
            alone.scene_correction.white_balance,
            WhiteBalance::Explicit([0.8, 1.0, 1.25])
        );
        // A stated contrast builds on the roll's slope rather than replacing it.
        let stated = merged(roll, &["--contrast", "1.2"]);
        assert_eq!(
            stated.resolved_slope(),
            ResolvedSlope {
                contrast: 1.2,
                base_slope: from_white,
                base_from: SlopeBase::Roll,
                slope: from_white * 1.2,
            }
        );
        assert_eq!(stated.shared_params().look.section.slope, from_white * 1.2);
        // No roll white: the fallback, which the contrast multiplies the same way.
        let fallback = Recipe::default().resolved_slope();
        assert_eq!(
            (fallback.base_slope, fallback.base_from, fallback.slope),
            (DEFAULT_SLOPE, SlopeBase::Fallback, DEFAULT_SLOPE)
        );
        let flatter = merged(r#"{"recipe_version": 3}"#, &["--contrast", "0.9"]);
        assert_eq!(flatter.resolved_slope().slope, DEFAULT_SLOPE * 0.9);
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
            &format!(r#"{{"recipe_version": 3, {roll}}}"#),
            &["--rendering", "direct"],
        );
        let p = direct.shared_params();
        assert_eq!(
            p.scene_correction.white_balance,
            WhiteBalance::Explicit([1.0, 1.0, 1.0]),
            "the roll's gains are not applied"
        );
        assert_eq!(p.look.section.slope, DIRECT.slope, "nor its white");
        assert_eq!(direct.resolved_slope().base_from, SlopeBase::Direct);
        assert!(p.look.section.highlight_desaturation.is_off());
        assert_eq!(p.headroom_stops, DIRECT.headroom_stops);
        assert_eq!(p.display_black, DIRECT.display_black);
        let report = direct.roll_report(true).unwrap();
        assert!(!report.white_balance_applied && !report.slope_applied);
        // `default` on the same recipe applies the roll, with every other default.
        let default = merged(&format!(r#"{{"recipe_version": 3, {roll}}}"#), &[]);
        let q = default.shared_params();
        assert_eq!(
            q.scene_correction.white_balance,
            WhiteBalance::Explicit([0.8, 1.0, 1.25])
        );
        assert_eq!(q.look.section.slope, roll_white::slope_for(1.6));
        assert_eq!(
            q.look.section.highlight_desaturation,
            HighlightDesaturation::DEFAULT
        );
        // A stated knob builds on either base: the white balance and the contrast
        // multiply, every other knob replaces.
        let adjusted = merged(
            &format!(r#"{{"recipe_version": 3, {roll}}}"#),
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
        assert_eq!(adjusted.look.section.slope, DIRECT.slope * 1.3);
        assert_eq!(adjusted.look.section.highlight_desaturation.strength, 0.5);
        assert_eq!(adjusted.display_black, DisplayBlack::Off);
        assert_eq!(adjusted.headroom_stops, DIRECT.headroom_stops);
    }

    #[test]
    fn direct_defaults_the_destination_to_the_hdr_float_tiff() {
        use destination::{Container, Gamut, Range, Transfer};
        let resolved = |extra: &[&str]| {
            let r = merged(
                r#"{"recipe_version": 3}"#,
                &[&["--rendering", "direct"][..], extra].concat(),
            );
            match destination(&r, KnobNames::FlagAndKey).unwrap() {
                Destination::Display(d) => (d.range, d.transfer, d.gamut, d.container),
                Destination::FilmMaster => unreachable!(),
            }
        };
        assert_eq!(
            resolved(&[]),
            (
                Range::Hdr,
                Transfer::Linear,
                Gamut::AdobeRgb,
                Container::Tiff
            )
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
        // A stated gamut with a float row keeps the float TIFF.
        assert_eq!(
            resolved(&["--gamut", "display-p3"]),
            (
                Range::Hdr,
                Transfer::Linear,
                Gamut::DisplayP3,
                Container::Tiff
            )
        );
        // The container is decided first, so a stated axis that rules the float TIFF
        // out falls back to the lossless 16-bit TIFF, never the lossy gain-map JPEG — and
        // a stated axis is never overridden by `direct`'s defaults.
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
            (
                Range::Hdr,
                Transfer::Linear,
                Gamut::AdobeRgb,
                Container::Tiff
            )
        );
        assert_eq!(
            resolved(&["--transfer", "pq"]),
            (Range::Hdr, Transfer::Pq, Gamut::Bt2020, Container::Tiff)
        );
        // Only a stated container reaches a lossy one, and its gamut is asked for:
        // `direct`'s Adobe RGB has no gain map, and two gamuts do.
        assert_eq!(
            resolved(&["--container", "jpeg", "--gamut", "srgb"]),
            (Range::Hdr, Transfer::Native, Gamut::Srgb, Container::Jpeg)
        );
        let r = merged(
            r#"{"recipe_version": 3}"#,
            &["--rendering", "direct", "--container", "jpeg"],
        );
        let err = destination(&r, KnobNames::FlagAndKey)
            .unwrap_err()
            .to_string();
        assert!(err.contains("--gamut display-p3|srgb"), "{err}");
        // SDR stated too: both JPEGs are unwritten, so nothing is claimed resolved.
        let r = merged(
            r#"{"recipe_version": 3}"#,
            &[
                "--rendering",
                "direct",
                "--range",
                "sdr",
                "--container",
                "jpeg",
            ],
        );
        let err = destination(&r, KnobNames::FlagAndKey)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("leaves --gamut open, and none of its choices is written yet"),
            "{err}"
        );
        assert!(!err.contains("resolves to"), "{err}");
        // `default` keeps the standard defaults and order, where the same stated gamut
        // is the gain map.
        let standard = |extra: &[&str]| {
            let r = merged(r#"{"recipe_version": 3}"#, extra);
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
            r#"{"recipe_version": 3}"#,
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
            parse(r#"{"recipe_version": 3, "rendering": "direct", "output": "film-master"}"#)
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
            (r#"{"recipe_version": 3}"#, &["--rendering", "direct"][..]),
            (r#"{"recipe_version": 3, "rendering": "direct"}"#, &[][..]),
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
            r#"{"recipe_version": 3}"#,
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
        let old = r#"{"recipe_version": 3, "rendering": "direct",
            "look": {"highlight_desaturation": {"strength": 0.8, "start_stops": -1.0,
                                                "band": [0.015, 0.025]}},
            "fit_range": {"headroom_stops": 6.0, "display_black": 6.0}}"#;
        let w = parse(old).unwrap().recipe_warnings(TypedStyle::default());
        assert_eq!(w.len(), 1, "{w:?}");
        // Only what moves the base is named: the headroom and black match it.
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
            r#"{"recipe_version": 3, "rendering": "direct",
                "look": {"highlight_desaturation": {"strength": 0.5, "start_stops": -2.0,
                                                    "band": [0.01, 0.03]}},
                "fit_range": {"headroom_stops": 4.0, "display_black": "off"}}"#,
            // A contrast or white balance with no roll section is the user's own.
            r#"{"recipe_version": 3, "rendering": "direct", "look": {"contrast": 1.3},
                "scene_correction": {"white_balance": {"explicit": [1.1, 1, 1]}}}"#,
        ] {
            let w = parse(deliberate)
                .unwrap()
                .recipe_warnings(TypedStyle::default());
            assert!(w.is_empty(), "{deliberate}: {w:?}");
        }
        // Beside a roll section, a stated white balance is what an earlier `measure-roll`
        // wrote, and `direct` would apply it although it leaves the roll out. A contrast
        // is not: no earlier build wrote the multiplier, so it is the user's.
        let roll = r#"{"recipe_version": 3, "rendering": "direct",
            "roll": {"white_balance": [1.2, 1, 0.9], "white_stops": 1.7},
            "look": {"contrast": 1.3},
            "scene_correction": {"white_balance": {"explicit": [1.2, 1, 0.9]}}}"#;
        let w = parse(roll).unwrap().recipe_warnings(TypedStyle::default());
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(
            w[0].contains("`scene_correction.white_balance` [1.2, 1.0, 0.9] beside a `roll`")
                && !w[0].contains("`look.contrast`"),
            "{}",
            w[0]
        );
        // The remedy never tells a deliberate adjuster to drop the value.
        assert!(
            w[0].contains("If a value beside the `roll` section came from an earlier")
                && w[0].contains("type it as a flag to keep it without this warning"),
            "{}",
            w[0]
        );
        let typed = TypedStyle {
            white_balance: true,
            ..TypedStyle::default()
        };
        assert!(parse(roll).unwrap().recipe_warnings(typed).is_empty());
        // `direct` never reads the roll, so it has no fallback to warn about.
        let bare = r#"{"recipe_version": 3, "rendering": "direct"}"#;
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
                    "the fallback slope {} (no `roll.white_stops`)",
                    DEFAULT_SLOPE
                )),
            "{}",
            w[0]
        );
        // All three remedies, as recipe keys.
        assert!(
            w[0].contains("`hanten measure-roll`")
                && w[0].contains(
                    "`scene_correction.white_balance`) and the roll's white (`roll.white_stops`"
                )
                && w[0].contains("and exposure (`roll.exposure`)")
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
            w.len() == 1 && !w[0].contains("white balance (") && w[0].contains("slope"),
            "{w:?}"
        );
        // The slope's half is silenced the same way — typed, or a recipe value off the
        // identity — though the contrast multiplies the fallback; and by a roll white.
        let contrast_typed = TypedStyle {
            white_balance: true,
            contrast: true,
            ..TypedStyle::default()
        };
        assert!(Recipe::default().recipe_warnings(contrast_typed).is_empty());
        let mut r = Recipe::default();
        r.look.contrast = 1.3;
        assert!(r.recipe_warnings(typed).is_empty());
        let mut r = Recipe::default();
        r.roll.white_stops = Some(1.7);
        r.roll.exposure = Some(0.0);
        assert!(r.recipe_warnings(typed).is_empty());
        // A roll white without an exposure says so, however it was given; only a stated
        // exposure (0 keeps the render) silences it.
        r.roll.exposure = None;
        let w = r.recipe_warnings(typed);
        assert!(
            w.len() == 1
                && w[0].starts_with("the roll section has no `roll.exposure`")
                && w[0].contains("`--roll-exposure`")
                && !w[0].contains("measured before"),
            "{w:?}"
        );
        r.roll.exposure = Some(0.0);
        assert!(r.recipe_warnings(typed).is_empty());
        r.roll.exposure = None;
        r.rendering = Rendering::Direct;
        assert!(
            r.recipe_warnings(typed).is_empty(),
            "direct applies no roll"
        );
    }

    #[test]
    fn the_roll_flags_land_in_the_roll_section() {
        let r = merged(
            r#"{"recipe_version": 3, "roll": {"white_balance": [0.9, 1.0, 1.1], "white_stops": 1.5,
                "exposure": 0.7}}"#,
            &["--roll-white", "1.8"],
        );
        // One flag replaces its own key and leaves the others.
        assert_eq!(
            r.roll,
            RollSection {
                white_balance: Some([0.9, 1.0, 1.1]),
                white_stops: Some(1.8),
                exposure: Some(0.7),
                frames: BTreeMap::new(),
                ..RollSection::default()
            }
        );
        let r = merged(
            r#"{"recipe_version": 3}"#,
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
                &format!(r#"{{"recipe_version": 3, "roll": {{"white_stops": {stops}}}}}"#),
                KnobNames::FlagAndKey,
            );
            assert!(
                msg.contains("--roll-white (recipe `roll.white_stops`)"),
                "{msg}"
            );
        }
        let msg = refusal(
            r#"{"recipe_version": 3, "roll": {"white_balance": [1, 0, 1]}}"#,
            KnobNames::KeyOnly,
        );
        assert!(
            msg.contains("`roll.white_balance`") && !msg.contains("--"),
            "{msg}"
        );
        // A contrast the roll's white gives, too small to survive the whole contrast:
        // named as the white, not as `--contrast`, which the user never typed.
        let msg = refusal(
            r#"{"recipe_version": 3, "roll": {"white_stops": 1e38},
                "reconstruction": {"linearization": 1e-8}}"#,
            KnobNames::FlagAndKey,
        );
        assert!(msg.contains("from --roll-white"), "{msg}");
        // The remedy moves the white the other way: the slope is inverse to it.
        assert!(
            msg.contains(
                "Use a larger --density-gamma (recipe `reconstruction.linearization`) or \
                 --contrast (recipe `look.contrast`), or a smaller --roll-white (recipe \
                 `roll.white_stops`)"
            ),
            "{msg}"
        );
        assert!(!msg.contains("value for either"), "{msg}");
        // Overflow, in `roll`'s key-only spelling: the reverse remedy.
        let msg = refusal(
            r#"{"recipe_version": 3, "roll": {"white_stops": 1e-30},
                "reconstruction": {"linearization": 1e10}}"#,
            KnobNames::KeyOnly,
        );
        assert!(msg.contains("overflows"), "{msg}");
        assert!(
            msg.contains(
                "Use a smaller `reconstruction.linearization` or `look.contrast`, or a larger \
                 `roll.white_stops`"
            ),
            "{msg}"
        );
        assert!(
            !msg.contains("value for either") && !msg.contains("--"),
            "{msg}"
        );
        // A white so small its slope is not finite overflows the same rule.
        let msg = refusal(
            r#"{"recipe_version": 3, "roll": {"white_stops": 1e-45}}"#,
            KnobNames::FlagAndKey,
        );
        assert!(
            msg.contains("overflows")
                && msg.contains("or a larger --roll-white (recipe `roll.white_stops`)"),
            "{msg}"
        );
        // A product only the roll's gains make is named with both factors.
        let msg = refusal(
            r#"{"recipe_version": 3, "roll": {"white_balance": [1e30, 1, 1]},
                "scene_correction": {"white_balance": {"explicit": [1e10, 1, 1]}}}"#,
            KnobNames::FlagAndKey,
        );
        assert!(
            msg.contains("--roll-white-balance") && msg.contains("--white-balance"),
            "{msg}"
        );
    }

    #[test]
    fn a_recipe_white_balance_that_meets_the_roll_gains_warns() {
        let warnings_typed = |json: &str, typed| parse(json).unwrap().roll_overlap_warnings(typed);
        let warnings = |json: &str| warnings_typed(json, TypedStyle::default());
        // The stated gains multiply the roll's. The contrast beside the roll's white is
        // not an overlap: it multiplies the roll's slope, which is what it is for.
        let overlapping = r#"{"recipe_version": 3,
                "roll": {"white_balance": [1.25, 1, 0.5], "white_stops": 1.7},
                "scene_correction": {"white_balance": {"explicit": [2, 1, 2]}},
                "look": {"contrast": 1.2}}"#;
        let only = warnings(overlapping);
        assert_eq!(only.len(), 1, "{only:?}");
        assert!(
            only[0].contains("the recipe's `scene_correction.white_balance` [2.0, 1.0, 2.0]")
                && only[0].contains("`roll.white_balance` [1.25, 1.0, 0.5]")
                && only[0].contains("the white balance applied is [2.5, 1.0, 1.0]")
                && only[0].contains("drop it")
                && !only[0].contains("--"),
            "{}",
            only[0]
        );
        // A typed flag is a choice made now: it silences the warning.
        let wb_typed = TypedStyle {
            white_balance: true,
            ..TypedStyle::default()
        };
        assert!(warnings_typed(overlapping, wb_typed).is_empty());
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
            r#"{"recipe_version": 3,
                "scene_correction": {"white_balance": {"explicit": [2, 1, 2]}}}"#,
            // The roll's gains over the identity, and a version 2 `null` contrast.
            r#"{"recipe_version": 2,
                "roll": {"white_balance": [1.25, 1, 0.5], "white_stops": 1.7},
                "scene_correction": {"white_balance": {"explicit": [1, 1, 1]}},
                "look": {"contrast": null}}"#,
            // A stated white balance beside the *other* roll measurement.
            r#"{"recipe_version": 3, "roll": {"white_stops": 1.7},
                "scene_correction": {"white_balance": {"explicit": [2, 1, 2]}}}"#,
        ] {
            assert_eq!(warnings(quiet), Vec::<String>::new(), "{quiet}");
        }
    }

    #[test]
    fn the_film_master_leaves_the_roll_section_unapplied_and_says_so() {
        let r: Recipe = parse(
            r#"{"recipe_version": 3, "output": "film-master",
                "roll": {"white_balance": [0.8, 1.0, 1.25], "white_stops": 1.6}}"#,
        )
        .unwrap();
        // A measurement is not a stage the user asked for, so it is not refused.
        assert_eq!(
            destination(&r, KnobNames::FlagAndKey).unwrap(),
            Destination::FilmMaster
        );
        let report = r.roll_report(false).unwrap();
        assert!(!report.white_balance_applied && !report.slope_applied);
        assert_eq!(report.slope, Some(roll_white::slope_for(1.6)));
        let rendered = r.roll_report(true).unwrap();
        assert!(rendered.white_balance_applied && rendered.slope_applied);
        // A stated contrast multiplies the roll's slope, so the roll's still applies.
        let mut stated = r.clone();
        stated.look.contrast = 1.3;
        assert!(stated.roll_report(true).unwrap().slope_applied);
        assert_eq!(Recipe::default().roll_report(true), None);
    }

    #[test]
    fn an_axis_flag_over_a_recipe_film_master_states_only_its_own_axes() {
        // The flag chose a rendered destination; the recipe stated none of its axes.
        let r = merged(
            r#"{"recipe_version": 3, "output": "film-master"}"#,
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
            r#"{"recipe_version": 3, "output": {"display": {"transfer": "pq", "container": "tiff"}}}"#,
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
            r#"{"recipe_version": 3, "output": {"display": {"gamut": "adobe-rgb"}}}"#,
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
            r#"{"recipe_version": 3, "output": {"display": {"gamut": "adobe-rgb"}}}"#,
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
        let r = parse(r#"{"recipe_version": 3, "fit_range": {"display_black": "off"}}"#).unwrap();
        assert_eq!(r.fit_range.display_black, Some(DisplayBlack::Off));
        let r = parse(r#"{"recipe_version": 3, "fit_range": {"display_black": 5}}"#).unwrap();
        assert_eq!(
            r.fit_range.display_black,
            Some(DisplayBlack::StopsBelowMid(5.0))
        );
        assert!(parse(r#"{"recipe_version": 3, "fit_range": {"display_black": "none"}}"#).is_err());
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
        let err = parse(r#"{"recipe_version": 3, "fit_range": {"peak": 4.9}}"#)
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
                r#"{"recipe_version": 3, "density": {"scale": [1, 1, 1]}}"#,
                &["`density`", "`reconstruction.scale`"][..],
            ),
            (
                r#"{"recipe_version": 3, "film_base": {"source": "auto"}}"#,
                &["`film_base`", "`calibration.film_base`"],
            ),
            (
                r#"{"recipe_version": 3, "algorithm": "density"}"#,
                &["`algorithm`"],
            ),
            (r#"{"recipe_version": 3, "sigmoid": {}}"#, &["`sigmoid`"]),
            (r#"{"recipe_version": 3, "simple": {}}"#, &["`simple`"]),
            (
                r#"{"recipe_version": 3, "input": {"color": "linear"}}"#,
                &["`input.color`", "`input.transfer`"],
            ),
            (
                r#"{"recipe_version": 3, "input": {"export_ir": "ir.tiff"}}"#,
                &["`input.export_ir`", "no longer exported"],
            ),
        ] {
            let err = check(json, true).unwrap_err();
            for needle in needles {
                assert!(err.contains(needle), "{json}: {err}");
            }
            // An overlay gets the same diagnosis.
            let overlay = json.replace(r#""recipe_version": 3, "#, "");
            assert!(check(&overlay, false).unwrap_err().contains(needles[0]));
        }
    }

    #[test]
    fn a_null_export_ir_replays_as_if_absent() {
        // Every `--dump-params` written before the IR export retired carries it.
        let mut v: serde_json::Value = serde_json::from_str(
            r#"{"recipe_version": 3, "input": {"film_type": "silver", "export_ir": null}}"#,
        )
        .unwrap();
        assert!(strip_retired_nulls(&mut v));
        check_body(&v, true, "recipe r.json").unwrap();
        let r: Recipe = serde_json::from_value(v).unwrap();
        assert_eq!(r.input.film_type, crate::types::FilmType::Silver);
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
        "--export-film-rgb",
        "--export-pre-encode",
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
            ("--roll-exposure", &["--roll-exposure", "-0.4"], |r| {
                r.roll.exposure == Some(-0.4)
            }),
            (
                "--roll-frame-exposure",
                &["--roll-frame-exposure", "0.2"],
                |r| r.roll.frame_exposure == Some(0.2),
            ),
            ("--small-lift", &["--small-lift", "off"], |r| {
                r.roll.small_lift == Some(Switch::Off)
            }),
            ("--roll-thin-slope", &["--roll-thin-slope", "2.1"], |r| {
                r.roll.thin_slope == Some(2.1)
            }),
            (
                "--roll-thin-exposure",
                &["--roll-thin-exposure", "0.6"],
                |r| r.roll.thin_exposure == Some(0.6),
            ),
            ("--thin-lift", &["--thin-lift", "off"], |r| {
                r.roll.thin_lift == Some(Switch::Off)
            }),
            (
                "--roll-midtone-line",
                &[
                    "--roll-midtone-line",
                    "-0.06,0.04,-0.02,0.37,-2.75,1.75,1.87",
                ],
                |r| {
                    r.roll.midtone_line
                        == Some(MidtoneLine {
                            red: [-0.06, 0.04],
                            blue: [-0.02, 0.37],
                            bands: [-2.75, 1.75],
                            fade_end_stops: 1.87,
                        })
                },
            ),
            ("--midtone-neutral", &["--midtone-neutral", "off"], |r| {
                r.roll.midtone_neutral == Some(Switch::Off)
            }),
            ("--contrast", &["--contrast", "1.3"], |r| {
                r.look.contrast == 1.3
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
            ("--container", &["--container", "jpeg"], |r| {
                r.output == display(|a| a.container = Some(Container::Jpeg))
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
            let merged = merge(Recipe::default(), &args.knobs);
            assert!(
                landed(&merged),
                "{flag} did not land in its field: {merged:?}"
            );
            // Falsifiability: the default does not already satisfy the check.
            assert!(!landed(&Recipe::default()), "{flag}'s check is vacuous");
        }
    }

    /// `validate_render`'s message for `convert` with `extra` over an explicit base, or
    /// `None` when the recipe renders.
    fn render_err(json: &str, extra: &[&str]) -> Option<String> {
        let mut flags = vec!["--film-base", "0.9,0.55,0.42"];
        flags.extend(extra);
        let r = merged(json, &flags);
        validate(&r, KnobNames::FlagAndKey).unwrap();
        validate_render(&r, None, KnobNames::FlagAndKey)
            .err()
            .map(|e| e.message().to_string())
    }

    const V3: &str = r#"{"recipe_version": 3}"#;

    #[test]
    fn a_film_base_that_grades_to_nothing_is_refused_naming_the_knob() {
        for (flag, value, key) in [
            ("--density-offset", "-30,-30,-30", "reconstruction.offset"),
            ("--density-offset", "1e6,1e6,1e6", "reconstruction.offset"),
            ("--density-gamma", "100", "reconstruction.linearization"),
            ("--anchor-mid-offset", "30", "reconstruction.anchor"),
            ("--contrast", "100", "look.contrast"),
            ("--roll-white", "0.001", "roll.white_stops"),
        ] {
            let err = render_err(V3, &[&format!("{flag}={value}")]).expect(flag);
            assert!(
                err.starts_with("the film base renders to luminance"),
                "{flag}: {err}"
            );
            assert!(
                err.ends_with(&format!(
                    "It renders with {flag} (recipe `{key}`) at its default"
                )),
                "{flag}: {err}"
            );
        }
    }

    #[test]
    fn a_sample_that_overflows_is_refused_naming_the_knob() {
        let err = render_err(V3, &["--density-scale", "100,100,100"]).unwrap();
        assert!(
            err.starts_with("the densest sample a scan can hold"),
            "{err}"
        );
        assert!(
            err.contains("--density-scale (recipe `reconstruction.scale`)"),
            "{err}"
        );
        // The region clause belongs to a base read from a region only.
        assert!(!err.contains("region"), "{err}");
        // Display black off reads no base, but the overflow is still the render's.
        let err = render_err(
            V3,
            &["--display-black", "off", "--density-offset=1e6,1e6,1e6"],
        )
        .unwrap();
        assert!(err.starts_with("the densest sample"), "{err}");
    }

    #[test]
    fn a_base_from_a_region_is_probed_at_one() {
        let r = merged(
            V3,
            &[
                "--base-region",
                "0,0,10,10",
                "--density-scale",
                "100,100,100",
            ],
        );
        let err = validate_render(&r, None, KnobNames::FlagAndKey).unwrap_err();
        assert!(
            err.message().contains("read from a region taken at 1"),
            "{}",
            err.message()
        );
    }

    #[test]
    fn the_remedy_names_each_knob_that_clears_the_fault_alone() {
        // Either alone at its default renders.
        let err = render_err(
            V3,
            &["--density-scale=2.5,2.5,2.5", "--density-offset=5,5,5"],
        )
        .unwrap();
        assert!(
            err.ends_with(
                "It renders with any one of --density-offset (recipe `reconstruction.offset`), \
                 --density-scale (recipe `reconstruction.scale`) at its default"
            ),
            "{err}"
        );
        // Neither alone renders: both are named, together.
        let err = render_err(V3, &["--density-offset=-20,-20,-20", "--contrast", "60"]).unwrap();
        assert!(
            err.ends_with(
                "It renders with --density-offset (recipe `reconstruction.offset`) and \
                 --contrast (recipe `look.contrast`) at their defaults"
            ),
            "{err}"
        );
    }

    #[test]
    fn a_rendering_recipe_and_the_film_master_pass() {
        assert_eq!(render_err(V3, &[]), None);
        // Extreme but rendering: black output, which the encoder warns about.
        assert_eq!(render_err(V3, &["--exposure=-100"]), None);
        assert_eq!(
            render_err(
                V3,
                &["--display-black", "off", "--density-offset=-30,-30,-30"]
            ),
            None
        );
        // The film master runs no rendering stage.
        assert_eq!(
            render_err(V3, &["--film-master", "--density-scale", "100,100,100"]),
            None
        );
    }

    #[test]
    fn a_roll_entry_is_named_as_itself() {
        let json = r#"{"recipe_version": 3,
            "roll": {"white_stops": 2.5, "frames": {"b.tif": {"white_stops": 0.001}}}}"#;
        let err = render_err(json, &[]).unwrap();
        assert!(
            err.starts_with(r#"recipe `roll.frames."b.tif"`: the film base"#),
            "{err}"
        );
        assert!(
            err.ends_with(
                r#"It renders with recipe `roll.frames."b.tif".white_stops` at its default"#
            ),
            "{err}"
        );
        assert!(!err.contains("--roll-white"), "{err}");
        // `convert` of that frame: the entry moved into `roll.white_stops`, and is still
        // named as the entry; a flag that beats it is named as the flag.
        let stated = merged(json, &["--film-base", "0.9,0.55,0.42"]);
        let own = Some(("b.tif", &stated.roll));
        let frame = stated.clone().for_frame(Path::new("b.tif"));
        let err = validate_render(&frame, own, KnobNames::FlagAndKey).unwrap_err();
        assert!(
            err.message()
                .ends_with(r#"recipe `roll.frames."b.tif".white_stops` at its default"#),
            "{}",
            err.message()
        );
        let mut beaten = frame;
        beaten.roll.frames.clear();
        beaten.roll.white_stops = Some(0.002);
        let err = validate_render(&beaten, own, KnobNames::FlagAndKey).unwrap_err();
        assert!(
            err.message()
                .ends_with("--roll-white (recipe `roll.white_stops`) at its default"),
            "{}",
            err.message()
        );
    }

    #[test]
    fn the_thin_slope_is_named_with_its_exposure() {
        let roll = [
            "--roll-white-balance",
            "1,1,1",
            "--roll-white",
            "1.5",
            "--roll-exposure",
            "0.4",
        ];
        let thin = ["--roll-thin-slope", "2", "--roll-thin-exposure", "100"];
        let err = render_err(V3, &[&roll[..], &thin].concat()).unwrap();
        assert!(
            err.ends_with(
                "It renders with any one of these at default: --roll-thin-slope (recipe \
                 `roll.thin_slope`) with its --roll-thin-exposure (recipe \
                 `roll.thin_exposure`); --roll-thin-exposure (recipe `roll.thin_exposure`)"
            ),
            "{err}"
        );
        assert!(
            !err.contains("--roll-thin-slope (recipe `roll.thin_slope`), "),
            "the slope alone is not a remedy: {err}"
        );
        // Each named remedy renders.
        assert_eq!(render_err(V3, &roll), None);
        assert_eq!(render_err(V3, &[&roll[..], &thin[..2]].concat()), None);
    }

    #[test]
    fn another_entry_is_probed_without_this_frames_values() {
        // `a.tif`'s steep white and `b.tif`'s exposure each render; only together would
        // the densest sample overflow, and no frame renders both.
        let json = r#"{"recipe_version": 3, "roll": {"white_stops": 1.5, "frames": {
            "a.tif": {"white_stops": 0.8}, "b.tif": {"exposure": 16}}}}"#;
        let stated = merged(json, &["--film-base", "0.9,0.55,0.42"]);
        let frame = stated.clone().for_frame(Path::new("a.tif"));
        let mut both = frame.clone();
        both.roll.frame_exposure = Some(16.0);
        assert!(render_fault(&both).unwrap().is_some(), "not vacuous");
        validate_render(&frame, Some(("a.tif", &stated.roll)), KnobNames::FlagAndKey).unwrap();
    }
}

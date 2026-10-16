//! Parameter resolution: the recipe, its applied roll and the rendering's base combined
//! into what each stage receives (design-spec §6, "Two renderings").
//!
//! The roll's measurements and the user's style act through one operator per stage — the
//! roll's gains times the stated white balance, its base slope times `look.contrast`, its
//! exposure plus the stated one — so they meet here, not in stages of their own
//! (`nf-core/three-step-pipeline`). The decode is not resolved here: it supplies the image
//! and the decoded film base (`pipeline::chain::render`'s arguments).
//!
//! Every function is pure. What the run applied of the roll, and where the look's slope
//! came from, is decided here too, and the report states it from here ([`roll_report`],
//! [`ResolvedSlope`]). Parsing, merging and validation stay in [`crate::recipe`]; its
//! validation calls in here, since it judges the resolved values.

use serde::Serialize;

use super::{FitRangeSection, Placement, Recipe, RollSection};
use crate::algo::fixed;
use crate::destination::Gamut;
use crate::destination::OutputSection;
use crate::pipeline::chain::{ChainParams, DisplayTarget, SharedParams};
use crate::pipeline::fit_range::{DarkEnd, DisplayPeak};
use crate::pipeline::look::{LookParams, LookSection};
use crate::pipeline::midtone_neutral::MidtoneLine;
use crate::pipeline::roll_balance::{RollBalance, Tilt, WhiteBalanceForm};
use crate::pipeline::roll_white::{self, FrameStops};
use crate::pipeline::scene_correction::{RollColour, SceneCorrectionParams, WhiteBalance};
use crate::pipeline::working_space::map_nc_film_rgb_v1;
use crate::rendering::Rendering;
use crate::types::FilmBase;

/// Where the look's base slope came from — the report's `chain.look.base_from`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SlopeBase {
    /// The applied roll's span, through [`RollSection::span_slope`].
    Roll,
    /// No roll white under `default`: [`Base::slope`](crate::rendering::Base::slope),
    /// the fallback.
    Fallback,
    /// `direct`, which applies no roll: its pinned
    /// [`Base::slope`](crate::rendering::Base::slope).
    Direct,
    /// The applied roll's thin slope, `roll.thin_slope` (a thin frame's lift).
    Thin,
    /// The frame's own placement ([`RollSection::frame_placement`]).
    Placement,
}

impl SlopeBase {
    /// The base slope's source, in words, for a message.
    pub fn phrase(self) -> &'static str {
        match self {
            SlopeBase::Roll => "the roll's slope",
            SlopeBase::Fallback => "the fallback slope",
            SlopeBase::Direct => "`direct`'s pinned slope",
            SlopeBase::Thin => "the thin slope (`roll.thin_slope`)",
            SlopeBase::Placement => "the frame placement's slope (`roll.placement`)",
        }
    }
}

/// The look's slopes and their parts: `base_slope × contrast` and
/// `saturation_base_slope × saturation` ([`slope`]).
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct ResolvedSlope {
    /// `look.contrast`, the multiplier.
    pub contrast: f32,
    pub base_slope: f32,
    pub base_from: SlopeBase,
    /// `look.saturation`, the multiplier.
    pub saturation: f32,
    /// What `saturation` multiplies: the base slope — except that a frame's own (its
    /// placement's or thin) never reaches colour, so there the roll's span slope — times the
    /// rendering's [`Base::saturation`](crate::rendering::Base::saturation).
    pub saturation_base_slope: f32,
    /// Where `saturation_base_slope`'s slope came from: `base_from`, except a frame's own,
    /// whose colour is the roll's (`roll`) or the fallback's.
    pub saturation_base_from: SlopeBase,
    /// What the look stage receives. Not serialized: the report's section states them.
    #[serde(skip)]
    pub slope: f32,
    #[serde(skip)]
    pub saturation_slope: f32,
}

impl ResolvedSlope {
    /// Every slope and multiplier at 1.
    pub fn identity() -> Self {
        Self {
            contrast: 1.0,
            base_slope: 1.0,
            base_from: SlopeBase::Direct,
            saturation: 1.0,
            saturation_base_slope: 1.0,
            saturation_base_from: SlopeBase::Direct,
            slope: 1.0,
            saturation_slope: 1.0,
        }
    }
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
    /// The section's dark end, as stated.
    pub dark_stops: Option<f32>,
    /// The slope the section sets ([`RollSection::slope`]): the thin slope while it
    /// applies, else the span's ([`RollSection::span_slope`]).
    pub slope: Option<f32>,
    /// The section's exposure, as stated.
    pub exposure: Option<f32>,
    /// This frame's small lift, as stated.
    pub frame_exposure: Option<f32>,
    /// This frame's thin lift, as stated.
    pub thin_slope: Option<f32>,
    pub thin_exposure: Option<f32>,
    /// The roll's envelope, as stated.
    pub envelope_stops: Option<[f32; 2]>,
    /// This frame's tones, as stated.
    pub frame_tones: Option<FrameStops>,
    /// The placement asked for (`roll.placement`; unset is `hybrid`).
    pub placement: Placement,
    /// The frame's own placement (`placement` says which), when it applied: its slope and
    /// exposure (a delta on the roll's) and what bound each.
    pub frame_placement: Option<roll_white::FramePlacement>,
    /// Whether the gains reached scene correction (not under `direct`, the film master or
    /// `--colour-correction off`).
    pub white_balance_applied: bool,
    /// How the gains apply (`roll.white_balance_form`).
    pub white_balance_form: WhiteBalanceForm,
    /// Where a tilt reaches the gains (`roll.white_balance_stops`).
    pub white_balance_stops: Option<f32>,
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
    /// Whether it reached scene correction (not under `--midtone-neutral off` or
    /// `--colour-correction off`).
    pub midtone_neutral_applied: bool,
    /// The line's terms that reached scene correction (`--midtone-neutral red|blue`
    /// keeps one); empty when the line did not.
    pub midtone_terms_applied: Vec<&'static str>,
    /// The taste adjustments applied, by their switch's key ([`RollSection::taste_applied`]):
    /// each is a choice, off by `roll.<key>` `"off"`.
    pub taste_applied: Vec<&'static str>,
}

/// One rendition's parameters for `pipeline::chain::render` — the recipe's shared
/// half ([`shared_params`]) plus the destination's peak and gamut, which
/// only the destination states.
pub fn chain_params(r: &Recipe, peak: DisplayPeak, gamut: Gamut) -> ChainParams {
    ChainParams {
        shared: shared_params(r),
        target: DisplayTarget { peak, gamut },
    }
}

/// Everything every rendition of a frame shares (`pipeline::chain`'s branch contract),
/// each stage resolved against the rendering's base.
pub fn shared_params(r: &Recipe) -> SharedParams {
    let (highlight_range_stops, dark_end) = fit_range(r);
    SharedParams {
        scene_correction: scene_correction(r),
        roll_colour: roll_colour(r),
        look: LookParams {
            section: look(r),
            linearization: r.reconstruction.linearization,
        },
        highlight_range_stops,
        dark_end,
        frame_dark_scene_stops: applied_frame_tones(r).map(|t| t.dark_stops),
    }
}

/// The roll section as the rendering applies it: whole, or not at all.
pub(super) fn applied_roll(r: &Recipe) -> RollSection {
    if r.base().applies_roll {
        r.roll.clone()
    } else {
        RollSection::default()
    }
}

/// Whether the roll's gains and exposure reach scene correction: not under a rendering
/// that leaves the roll out, nor on the film master, which runs no scene correction.
pub fn applies_roll_to_scene_correction(r: &Recipe) -> bool {
    r.output != OutputSection::FilmMaster && r.base().applies_roll
}

/// Scene correction as the stage receives it: the applied roll's gains multiplied
/// into the stated white balance, and its exposure and the frame's added to the stated one — each
/// the identity unless the user set it, so a roll's values alone reach the stage
/// exactly. Both renderings start the white balance and the exposure at the identity.
pub fn scene_correction(r: &Recipe) -> SceneCorrectionParams {
    let SceneCorrectionParams {
        white_balance: WhiteBalance::Explicit(stated),
        exposure,
    } = r.scene_correction;
    let roll = applied_roll(r);
    // A tilt is not a gain: scene correction applies it on its own (`roll_colour`).
    let folded = roll
        .applied_white_balance()
        .filter(|_| roll.white_balance_form() == WhiteBalanceForm::Gain);
    let white_balance = match folded {
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

/// The applied roll's colour correction: its line, keyed on its white balance, and a
/// tilt; none when there is neither (the gain form folds into scene correction's gains).
fn roll_colour(r: &Recipe) -> Option<RollColour> {
    let roll = applied_roll(r);
    let balance = roll_balance(r, &roll)?;
    let line = roll.applied_midtone_line();
    (line.is_some() || matches!(balance, RollBalance::Tilt(_)))
        .then_some(RollColour { line, balance })
}

/// `roll`'s white balance in its form, a tilt anchored at `r`'s decoded film base: the
/// applied roll's for scene correction, a just-measured one for `measure-roll`; `None`
/// without applied gains.
pub fn roll_balance(r: &Recipe, roll: &RollSection) -> Option<RollBalance> {
    let gains = roll.applied_white_balance()?;
    Some(match roll.white_balance_form() {
        WhiteBalanceForm::Gain => RollBalance::Gain(gains),
        WhiteBalanceForm::Tilt => RollBalance::Tilt(Tilt::new(
            gains,
            decoded_base(r),
            // Validation refuses a tilt without it; NaN fails the stage's own check.
            roll.white_balance_stops.unwrap_or(f32::NAN),
        )),
    })
}

/// The film base as the decode renders it, in linear ACEScg: the same for every roll,
/// since the decode measures density from the base (`algo::fixed::decode_film_base`).
/// NaN when the decode's parameters are unusable, which validation refuses first.
fn decoded_base(r: &Recipe) -> [f32; 3] {
    let unit = FilmBase {
        r: 1.0,
        g: 1.0,
        b: 1.0,
    };
    fixed::decode_film_base(&unit, &r.reconstruction)
        .map(map_nc_film_rgb_v1)
        .ok()
        .and_then(|b| <[f32; 3]>::try_from(b.rgb()).ok())
        .unwrap_or([f32::NAN; 3])
}

/// The look's slopes and their parts: the base — the frame's own slope, else the
/// applied roll's span, else the rendering's [`Base::slope`] — times the
/// `look.contrast` multiplier, and the same base without the frame's own slope, times
/// the rendering's [`Base::saturation`] and the `look.saturation` multiplier.
///
/// [`Base::slope`]: crate::rendering::Base::slope
/// [`Base::saturation`]: crate::rendering::Base::saturation
pub fn slope(r: &Recipe) -> ResolvedSlope {
    let roll = applied_roll(r);
    let white_slope = roll.span_slope();
    let (base_slope, base_from) = match (roll.slope(), r.rendering) {
        (Some(s), _) if roll.frame_placement().is_some() => (s, SlopeBase::Placement),
        (Some(s), _) if roll.applied_thin_slope().is_some() => (s, SlopeBase::Thin),
        (Some(s), _) => (s, SlopeBase::Roll),
        (None, Rendering::Default) => (r.base().slope, SlopeBase::Fallback),
        (None, Rendering::Direct) => (r.base().slope, SlopeBase::Direct),
    };
    // Neither a thin slope nor a frame placement's reaches colour: it is the roll's.
    let (colour_base_slope, saturation_base_from) = match (base_from, white_slope) {
        (SlopeBase::Thin | SlopeBase::Placement, Some(w)) => (w, SlopeBase::Roll),
        (SlopeBase::Thin | SlopeBase::Placement, None) => (r.base().slope, SlopeBase::Fallback),
        (from, _) => (base_slope, from),
    };
    let saturation_base_slope = colour_base_slope * r.base().saturation;
    ResolvedSlope {
        contrast: r.look.contrast,
        base_slope,
        base_from,
        saturation: r.look.saturation,
        saturation_base_slope,
        saturation_base_from,
        slope: base_slope * r.look.contrast,
        saturation_slope: saturation_base_slope * r.look.saturation,
    }
}

/// The look as the stage receives it.
pub fn look(r: &Recipe) -> LookSection {
    r.look.resolve(slope(r), r.base().highlight_desaturation)
}

/// Fit range's highlight range and dark end as the stage receives them: stated, else the
/// rendering's base. Destructured without `..`, like [`LookKeys::resolve`].
///
/// [`LookKeys::resolve`]: super::LookKeys::resolve
pub fn fit_range(r: &Recipe) -> (f32, DarkEnd) {
    let FitRangeSection {
        highlight_range_stops,
        dark_end,
    } = r.fit_range;
    let base = r.base();
    (
        highlight_range_stops.unwrap_or(base.highlight_range_stops),
        dark_end.unwrap_or(base.dark_end),
    )
}

/// This frame's measured tones, where the rendering applies the roll.
pub fn applied_frame_tones(r: &Recipe) -> Option<FrameStops> {
    r.roll.frame_tones.filter(|_| r.base().applies_roll)
}

/// Whether a roll's exposure reaches scene correction.
pub fn applies_roll_exposure(r: &Recipe) -> bool {
    r.roll.exposure.is_some() && r.base().applies_roll
}

/// The report's `chain.roll`: `None` when the section states nothing. `rendered`
/// is whether any rendering stage ran — the film master applies neither value, and
/// `--rendering direct` leaves the section out.
pub fn roll_report(recipe: &Recipe, rendered: bool) -> Option<RollReport> {
    let r = &recipe.roll;
    let applies = rendered && recipe.base().applies_roll;
    let line_applied = applies && r.applied_midtone_line().is_some();
    // Other frames' `frames` entries are not this frame's measurement.
    (r.white_balance.is_some()
        || r.white_stops.is_some()
        || r.dark_stops.is_some()
        || r.exposure.is_some()
        || r.frame_exposure.is_some()
        || r.thin_slope.is_some()
        || r.thin_exposure.is_some()
        || r.envelope_stops.is_some()
        || r.frame_tones.is_some()
        || r.midtone_line.is_some())
    .then(|| RollReport {
        white_balance: r.white_balance,
        white_stops: r.white_stops,
        dark_stops: r.dark_stops,
        slope: r.slope(),
        exposure: r.exposure,
        frame_exposure: r.frame_exposure,
        thin_slope: r.thin_slope,
        thin_exposure: r.thin_exposure,
        envelope_stops: r.envelope_stops,
        frame_tones: r.frame_tones,
        placement: r.placement(),
        frame_placement: r.frame_placement().filter(|_| applies),
        white_balance_applied: applies && r.applied_white_balance().is_some(),
        white_balance_form: r.white_balance_form(),
        white_balance_stops: r.white_balance_stops,
        slope_applied: applies
            && matches!(
                slope(recipe).base_from,
                SlopeBase::Roll | SlopeBase::Thin | SlopeBase::Placement
            ),
        exposure_applied: applies && r.exposure.is_some(),
        frame_exposure_applied: applies && r.applied_small_exposure().is_some(),
        thin_lift_applied: applies && r.applied_thin_slope().is_some(),
        midtone_line: r.midtone_line,
        midtone_neutral_applied: line_applied,
        midtone_terms_applied: if line_applied {
            r.midtone_terms().kept()
        } else {
            Vec::new()
        },
        taste_applied: if applies {
            r.taste_applied()
        } else {
            Vec::new()
        },
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::pipeline::look::{DEFAULT_SATURATION, DEFAULT_SLOPE, HighlightDesaturation};
    use crate::recipe::tests::{cli_flags, merged, parse};
    use crate::recipe::{for_frame, merge};

    #[test]
    fn the_look_is_handed_the_decodes_linearization() {
        // Highlight desaturation's measure is normalised by linearization × the
        // saturation slope — the decode's factor the look section cannot state, so
        // `chain_params` must.
        let r = parse(
            r#"{"recipe_version": 3, "reconstruction": {"linearization": 3.1},
                "look": {"contrast": 1.2, "highlight_desaturation": {"strength": 0.5}}}"#,
        )
        .unwrap();
        let look = chain_params(&r, DisplayPeak::SDR, Gamut::DisplayP3)
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
    fn a_frames_lift_adds_to_the_roll_unless_turned_off_or_left_out() {
        let roll = r#"{"recipe_version": 3, "roll": {"exposure": 1.0,
            "frames": {"a.tif": {"white_stops": null, "frame_exposure": 0.25}}}}"#;
        let frame = |extra: &[&str]| {
            let r = for_frame(parse(roll).unwrap(), Path::new("a.tif"));
            merge(r, &cli_flags(extra))
        };
        // `for_frame` moves the entry's lift into `roll.frame_exposure`, as it moves a white.
        let r = frame(&[]);
        assert_eq!(r.roll.frame_exposure, Some(0.25));
        assert!(r.roll.frames.is_empty());
        assert_eq!(scene_correction(&r).exposure, 1.25);
        let report = roll_report(&r, true).unwrap();
        assert!(report.frame_exposure_applied, "{report:?}");
        // Another frame is not lifted.
        let other = for_frame(merged(roll, &[]), Path::new("b.tif"));
        assert_eq!(scene_correction(&other).exposure, 1.0);
        // Off keeps the value and drops it from the sum; `direct` leaves the roll out.
        let off = frame(&["--small-lift", "off"]);
        assert_eq!(off.roll.frame_exposure, Some(0.25));
        assert_eq!(scene_correction(&off).exposure, 1.0);
        assert!(!roll_report(&off, true).unwrap().frame_exposure_applied);
        let direct = frame(&["--rendering", "direct"]);
        assert_eq!(scene_correction(&direct).exposure, 0.0);
    }

    #[test]
    fn a_thin_lift_replaces_the_small_one_and_each_switch_turns_off_its_own() {
        let roll = r#"{"recipe_version": 3, "roll": {"white_stops": 1.5, "dark_stops": -3.75, "exposure": 1.0,
            "frames": {"a.tif": {"frame_exposure": 0.25, "thin_slope": 2.0, "thin_exposure": 0.7}}}}"#;
        let frame = |extra: &[&str]| {
            let r = for_frame(parse(roll).unwrap(), Path::new("a.tif"));
            merge(r, &cli_flags(extra))
        };
        let roll_slope = (roll_white::span_slope(1.5, -3.75), SlopeBase::Roll);
        let shape = |r: &Recipe| {
            let s = slope(r);
            let report = roll_report(r, true).unwrap();
            (
                (s.base_slope, s.base_from),
                scene_correction(r).exposure,
                report.taste_applied,
            )
        };
        let r = frame(&[]);
        assert_eq!(
            (r.roll.thin_slope, r.roll.thin_exposure),
            (Some(2.0), Some(0.7))
        );
        assert_eq!(shape(&r), ((2.0, SlopeBase::Thin), 1.7, vec!["thin_lift"]));
        let report = roll_report(&r, true).unwrap();
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
        assert_eq!(slope(&frame(&["--contrast", "1.1"])).slope, 2.0 * 1.1);
        // The thin slope never reaches colour: saturation keeps the white's base.
        let s = slope(&frame(&["--saturation", "1.2"]));
        assert_eq!(s.saturation_base_slope, roll_slope.0 * DEFAULT_SATURATION);
        assert_eq!(s.saturation_slope, roll_slope.0 * DEFAULT_SATURATION * 1.2);
        // Another frame keeps the roll's.
        let other = for_frame(merged(roll, &[]), Path::new("b.tif"));
        assert_eq!(slope(&other).base_from, SlopeBase::Roll);
        // A typed white beats the whole thin lift, and the small lift stays.
        let white = frame(&["--roll-white", "1.9"]);
        assert_eq!(
            shape(&white),
            (
                (roll_white::span_slope(1.9, -3.75), SlopeBase::Roll),
                1.25,
                vec!["small_lift"]
            )
        );
        assert_eq!(white.roll.thin_exposure, None);
        // `direct` leaves the roll out, and applies no taste.
        let direct = frame(&["--rendering", "direct"]);
        assert_eq!(slope(&direct).base_from, SlopeBase::Direct);
        assert!(roll_report(&direct, true).unwrap().taste_applied.is_empty());
    }

    #[test]
    fn the_hybrid_places_a_measured_frame_in_place_of_its_lifts() {
        let roll = r#"{"recipe_version": 3, "roll": {"white_stops": 1.5, "dark_stops": -3.75,
            "exposure": 1.0, "envelope_stops": [-4.5, 2.4],
            "frames": {"a.tif": {"frame_exposure": 0.25, "thin_slope": 2.0, "thin_exposure": 0.7,
                "tones": {"white_stops": 0.6, "level_stops": -1.8, "dark_stops": -2.9}}}}}"#;
        let frame = |extra: &[&str]| {
            let r = for_frame(parse(roll).unwrap(), Path::new("a.tif"));
            merge(r, &cli_flags(extra))
        };
        let tones = FrameStops {
            white_stops: 0.6,
            level_stops: -1.8,
            dark_stops: -2.9,
        };
        let roll_slope = roll_white::span_slope(1.5, -3.75);
        let expected = roll_white::hybrid_placement(
            tones,
            roll_slope,
            1.0,
            [-4.5, 2.4],
            roll_white::HYBRID_SPAN_STOPS,
            roll_white::HYBRID_LEVEL_KEEP,
        );
        let r = frame(&[]);
        assert_eq!(r.roll.frame_tones, Some(tones));
        assert_eq!(r.roll.frame_placement(), Some(expected));
        let s = slope(&r);
        assert_eq!(
            (s.base_slope, s.base_from),
            (expected.slope, SlopeBase::Placement)
        );
        // Colour stays the roll's, as a thin frame's does.
        assert_eq!(
            (s.saturation_base_slope, s.saturation_base_from),
            (roll_slope * r.base().saturation, SlopeBase::Roll)
        );
        assert_eq!(scene_correction(&r).exposure, 1.0 + expected.exposure);
        let report = roll_report(&r, true).unwrap();
        assert_eq!(report.taste_applied, vec!["placement"]);
        assert_eq!(report.frame_placement, Some(expected));
        assert!(
            report.slope_applied && !report.thin_lift_applied && !report.frame_exposure_applied,
            "{report:?}"
        );
        // The roll's placement renders the lifts; the knobs reach the solve.
        let per_roll = frame(&["--placement", "roll"]);
        assert_eq!(per_roll.roll.frame_placement(), None);
        assert_eq!(slope(&per_roll).base_from, SlopeBase::Thin);
        assert_eq!(
            roll_report(&per_roll, true).unwrap().taste_applied,
            vec!["thin_lift"]
        );
        let level = frame(&["--hybrid-level-keep", "1"])
            .roll
            .frame_placement()
            .unwrap();
        assert_eq!(
            level,
            roll_white::hybrid_placement(
                tones,
                roll_slope,
                1.0,
                [-4.5, 2.4],
                roll_white::HYBRID_SPAN_STOPS,
                1.0
            )
        );
        // `direct` applies no roll, so no placement.
        let direct = frame(&["--rendering", "direct"]);
        assert_eq!(slope(&direct).base_from, SlopeBase::Direct);
        assert!(
            roll_report(&direct, true)
                .unwrap()
                .frame_placement
                .is_none()
        );
    }

    #[test]
    fn the_roll_section_reaches_the_stages_and_the_contrast_multiplies_its_slope() {
        let roll = r#"{"recipe_version": 3,
                       "roll": {"white_balance": [0.8, 1.0, 1.25], "white_stops": 1.6, "dark_stops": -3.75}}"#;
        // The gains multiply the stated white balance; the white and dark end set the roll slope.
        let r = merged(roll, &["--white-balance", "1.25,1,1"]);
        let shared = shared_params(&r);
        assert_eq!(
            shared.scene_correction.white_balance,
            WhiteBalance::Explicit([0.8 * 1.25, 1.0, 1.25])
        );
        let from_white = roll_white::span_slope(1.6, -3.75);
        assert_eq!(shared.look.section.slope, from_white);
        assert_eq!(
            slope(&r),
            ResolvedSlope {
                contrast: 1.0,
                base_slope: from_white,
                base_from: SlopeBase::Roll,
                saturation: 1.0,
                saturation_base_slope: from_white * DEFAULT_SATURATION,
                saturation_base_from: SlopeBase::Roll,
                slope: from_white,
                saturation_slope: from_white * DEFAULT_SATURATION,
            }
        );
        // Alone, the gains reach the stage exactly: the identity multiplies nothing in.
        let alone = shared_params(&merged(roll, &[]));
        assert_eq!(
            alone.scene_correction.white_balance,
            WhiteBalance::Explicit([0.8, 1.0, 1.25])
        );
        // A stated contrast builds on the roll's slope rather than replacing it.
        let stated = merged(roll, &["--contrast", "1.2"]);
        assert_eq!(
            slope(&stated),
            ResolvedSlope {
                contrast: 1.2,
                base_slope: from_white,
                base_from: SlopeBase::Roll,
                saturation: 1.0,
                saturation_base_slope: from_white * DEFAULT_SATURATION,
                saturation_base_from: SlopeBase::Roll,
                slope: from_white * 1.2,
                saturation_slope: from_white * DEFAULT_SATURATION,
            }
        );
        assert_eq!(shared_params(&stated).look.section.slope, from_white * 1.2);
        // No roll white: the fallback, which the contrast multiplies the same way.
        let fallback = slope(&Recipe::default());
        assert_eq!(
            (fallback.base_slope, fallback.base_from, fallback.slope),
            (DEFAULT_SLOPE, SlopeBase::Fallback, DEFAULT_SLOPE)
        );
        let flatter = merged(r#"{"recipe_version": 3}"#, &["--contrast", "0.9"]);
        assert_eq!(slope(&flatter).slope, DEFAULT_SLOPE * 0.9);
        assert_eq!(
            shared_params(&Recipe::default()).look.section,
            LookSection::default()
        );
    }

    #[test]
    fn direct_starts_every_knob_from_its_pinned_base_and_leaves_the_roll_out() {
        use crate::rendering::DIRECT;
        let roll = r#""roll": {"white_balance": [0.8, 1.0, 1.25], "white_stops": 1.6, "dark_stops": -3.75}"#;
        let direct = merged(
            &format!(r#"{{"recipe_version": 3, {roll}}}"#),
            &["--rendering", "direct"],
        );
        let p = shared_params(&direct);
        assert_eq!(
            p.scene_correction.white_balance,
            WhiteBalance::Explicit([1.0, 1.0, 1.0]),
            "the roll's gains are not applied"
        );
        assert_eq!(p.look.section.slope, DIRECT.slope, "nor its white");
        assert_eq!(slope(&direct).base_from, SlopeBase::Direct);
        assert!(p.look.section.highlight_desaturation.is_off());
        assert_eq!(p.highlight_range_stops, DIRECT.highlight_range_stops);
        assert_eq!(p.dark_end, DIRECT.dark_end);
        assert_eq!(p.frame_dark_scene_stops, None, "nor its tones");
        let report = roll_report(&direct, true).unwrap();
        assert!(!report.white_balance_applied && !report.slope_applied);
        // `default` on the same recipe applies the roll, with every other default.
        let default = merged(&format!(r#"{{"recipe_version": 3, {roll}}}"#), &[]);
        let q = shared_params(&default);
        assert_eq!(
            q.scene_correction.white_balance,
            WhiteBalance::Explicit([0.8, 1.0, 1.25])
        );
        assert_eq!(q.look.section.slope, roll_white::span_slope(1.6, -3.75));
        assert_eq!(
            q.look.section.highlight_desaturation,
            HighlightDesaturation::DEFAULT
        );
        // A stated knob builds on either base: the white balance, the contrast and the
        // saturation multiply, every other knob replaces.
        let adjusted = shared_params(&merged(
            &format!(r#"{{"recipe_version": 3, {roll}}}"#),
            &[
                "--rendering",
                "direct",
                "--white-balance",
                "1.1,1,1",
                "--contrast",
                "1.3",
                "--saturation",
                "1.2",
                "--highlight-desaturation",
                "0.5",
                "--dark-base",
                "off",
            ],
        ));
        assert_eq!(
            adjusted.scene_correction.white_balance,
            WhiteBalance::Explicit([1.1, 1.0, 1.0])
        );
        assert_eq!(adjusted.look.section.slope, DIRECT.slope * 1.3);
        assert_eq!(
            adjusted.look.section.saturation_slope,
            DIRECT.slope * DIRECT.saturation * 1.2
        );
        assert_eq!(adjusted.look.section.highlight_desaturation.strength, 0.5);
        assert_eq!(adjusted.dark_end, DarkEnd::Off);
        assert_eq!(adjusted.highlight_range_stops, DIRECT.highlight_range_stops);
    }

    #[test]
    fn the_floor_reads_the_frame_s_tones_where_the_roll_applies() {
        let tones =
            r#""frame_tones": {"white_stops": 1.5, "level_stops": -0.5, "dark_stops": -4.2}"#;
        let r = parse(&format!(r#"{{"recipe_version": 3, "roll": {{{tones}}}}}"#)).unwrap();
        assert_eq!(shared_params(&r).frame_dark_scene_stops, Some(-4.2));
        let r = parse(&format!(
            r#"{{"recipe_version": 3, "rendering": "direct", "roll": {{{tones}}}}}"#
        ))
        .unwrap();
        assert_eq!(shared_params(&r).frame_dark_scene_stops, None);
    }

    #[test]
    fn the_destination_states_fit_ranges_peak() {
        // The recipe carries the highlight range; the peak comes from the destination, so a
        // recipe cannot name a peak its destination does not have.
        let mut r = Recipe::default();
        r.fit_range.highlight_range_stops = Some(4.0);
        let p = chain_params(&r, DisplayPeak::SDR, Gamut::DisplayP3);
        assert_eq!(p.shared.highlight_range_stops, 4.0);
        assert_eq!(p.target.peak, DisplayPeak::SDR);
        let err = parse(r#"{"recipe_version": 3, "fit_range": {"peak": 4.9}}"#)
            .unwrap_err()
            .to_string();
        assert!(err.contains("peak"), "{err}");
    }
}

//! **Whether a frame's black can approach black** with display black off — a report
//! warning, never an error (`nf-reconstruction/curve-endpoint-warning`).
//!
//! The encode's loss counter sees highlights leave the top of the range and is blind to a
//! black end that never reaches the bottom, so a washed-out frame passes every other
//! check. Where the film base lands cannot decide it alone: a thin roll's default render
//! puts the base 2 stops under mid-grey, and a flat frame keeps its darkest content high
//! on purpose (`roll_white::hybrid_placement`'s slope bounds). Two faults wash a frame
//! out, and each shows in a different number:
//!
//! - **Pushed up**: the frame's middle lands above [`MAX_MIDDLE_STOPS`] *and* its dark
//!   end above [`MAX_DARK_STOPS`] — too much exposure. Read from the frame's measured
//!   tones (`roll.frame_tones`), so a centred flat frame never trips it. Without tones
//!   the base itself is the measure: closer than [`MIN_FILM_BASE_STOPS`] under mid-grey.
//!   A roll's exposure alone is left alone there — a thin roll's puts the base at that
//!   threshold by measurement, not by fault — but a stated exposure added on top is not.
//! - **Squeezed flat**: the look's tone slope is under [`MIN_SLOPE`], the flattest any
//!   placement sets — a `look.contrast` under 1, or a stated thin slope, gets there.
//!
//! **A tone lands at `display_stops(slope · (tone + exposure + log2 gain))`**, `gain` the
//! white balance's on luminance: the placement model `roll_white` solves in, with what the
//! render applied. `chain`'s `the_black_endpoint_model_is_the_chain_s_render_of_a_neutral`
//! pins it to the stages. The base is the real grade's
//! (`fit_range::ResolvedDarkEnd::film_base_stops`). Display black places the base itself
//! and has its own warning (`ResolvedDarkEnd::warning`), so this check is skipped there;
//! the floor and the dark target move only the darkest stops, and neither fixes a frame
//! placed too bright, so it runs beside them on the tones as placed.

use crate::pipeline::chain::graded_tone;
use crate::pipeline::colorimetry::dot;
use crate::pipeline::colorimetry::pinned::ACESCG_LUMINANCE;
use crate::pipeline::fit_range::{MIN_FILM_BASE_STOPS, display_stops};
use crate::pipeline::roll_white::{FrameStops, WHITE_CAP_STOPS, slope_for};

/// The highest a frame's middle may land, in display stops above mid-grey, while its dark
/// end is above [`MAX_DARK_STOPS`].
pub const MAX_MIDDLE_STOPS: f32 = 1.5;

/// The highest a pushed-up frame's dark end may land, in display stops from mid-grey.
pub const MAX_DARK_STOPS: f32 = -1.5;

/// The flattest slope a placement sets (`roll_white::span_slope`'s floor).
pub const MIN_SLOPE: f32 = slope_for(WHITE_CAP_STOPS);

/// What the check reads: what the render applied, and the frame's tones.
#[derive(Clone, Copy, Debug)]
pub struct Inputs {
    /// The look's slope (`chain::Rendered::look`).
    pub slope: f32,
    /// Scene correction's exposure in EV and white-balance gains (`chain::Rendered::scene_correction`).
    pub exposure: f32,
    pub white_balance: [f32; 3],
    /// The frame's measured tones, where the roll applies.
    pub tones: Option<FrameStops>,
    /// Whether a roll's exposure applies (`recipe::resolve::applies_roll_exposure`), and
    /// the stated scene correction on top of it: `scene_correction.exposure` and
    /// `.white_balance`.
    pub roll_exposure: bool,
    pub stated_exposure: f32,
    pub stated_white_balance: [f32; 3],
    /// Where the graded film base lands, in stops under mid-grey.
    pub film_base_stops: f32,
    pub headroom_stops: f32,
    /// For the message: the `look.contrast` multiplier, and the slope it multiplies and
    /// where that came from.
    pub contrast: f32,
    pub base_slope: f32,
    pub base_from: &'static str,
}

impl Inputs {
    /// The stated scene correction's gain on luminance, in EV.
    fn stated_gain(&self) -> f32 {
        self.stated_exposure + dot(self.stated_white_balance, ACESCG_LUMINANCE).log2()
    }

    /// Where a tone (scene stops from mid-grey, before exposure) lands on the display, in
    /// stops from mid-grey.
    pub fn land(&self, stops: f32) -> f32 {
        display_stops(
            self.headroom_stops,
            graded_tone(stops, self.slope, self.exposure, self.white_balance),
        )
    }
}

/// What washes a frame out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fault {
    Squeezed {
        slope: f32,
        contrast: f32,
        base_slope: f32,
        base_from: &'static str,
    },
    /// Display stops from mid-grey.
    Pushed { middle: f32, dark: f32 },
    /// Stops under mid-grey.
    Base { stops: f32 },
}

/// Every fault the frame has, the slope's first: each names its own remedy.
pub fn check(i: &Inputs) -> Vec<Fault> {
    let mut faults = Vec::new();
    if i.slope < MIN_SLOPE {
        faults.push(Fault::Squeezed {
            slope: i.slope,
            contrast: i.contrast,
            base_slope: i.base_slope,
            base_from: i.base_from,
        });
    }
    match i.tones {
        Some(tones) => {
            let (middle, dark) = (i.land(tones.level_stops), i.land(tones.dark_stops));
            if middle > MAX_MIDDLE_STOPS && dark > MAX_DARK_STOPS {
                faults.push(Fault::Pushed { middle, dark });
            }
        }
        None if (!i.roll_exposure || i.stated_gain() > 0.0)
            && i.film_base_stops < MIN_FILM_BASE_STOPS =>
        {
            faults.push(Fault::Base {
                stops: i.film_base_stops,
            })
        }
        _ => {}
    }
    faults
}

/// `n` display stops from mid-grey, in words.
fn from_mid(stops: f32) -> String {
    if stops >= 0.0 {
        format!("{stops:.2} stops above mid-grey")
    } else {
        format!("{:.2} stops under mid-grey", -stops)
    }
}

const LOWER_EXPOSURE: &str = "lower `--exposure` (recipe `scene_correction.exposure`)";

impl Fault {
    pub fn message(&self) -> String {
        match *self {
            Fault::Squeezed {
                slope,
                contrast,
                base_slope,
                base_from,
            } => format!(
                "the look's tone slope is {slope:.3} (`--contrast` {contrast:.3} times \
                 {base_from} {base_slope:.3}), flatter than the {MIN_SLOPE:.3} any \
                 placement sets, so the frame's shadows are raised toward mid-grey: raise \
                 `--contrast` (recipe `look.contrast`)"
            ),
            Fault::Pushed { middle, dark } => format!(
                "the frame is placed so bright its black cannot approach black: the curve \
                 puts its middle {} and its dark end (ACEScg luminance p1) {}, before the dark end is \
                 moved. This is where the look places the frame's measured tones, with \
                 the dark base off: {LOWER_EXPOSURE}",
                from_mid(middle),
                from_mid(dark),
            ),
            Fault::Base { stops } => format!(
                "the film base renders {}, where black needs it at least \
                 {MIN_FILM_BASE_STOPS} stops under mid-grey, so with the dark base off \
                 nothing in the frame can approach black. The frame is placed very bright: \
                 {LOWER_EXPOSURE}",
                from_mid(-stops),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::DEFAULT_HEADROOM_STOPS;

    fn inputs() -> Inputs {
        Inputs {
            slope: 1.6,
            exposure: 1.2,
            white_balance: [1.0; 3],
            tones: Some(FrameStops {
                white_stops: 0.5,
                level_stops: -1.2,
                dark_stops: -3.5,
            }),
            roll_exposure: true,
            stated_exposure: 0.0,
            stated_white_balance: [1.0; 3],
            film_base_stops: 3.5,
            headroom_stops: DEFAULT_HEADROOM_STOPS,
            contrast: 1.0,
            base_slope: 1.6,
            base_from: "the roll's slope",
        }
    }

    #[test]
    fn a_placed_frame_has_no_fault() {
        assert_eq!(check(&inputs()), vec![]);
    }

    #[test]
    fn the_floor_is_span_slope_s() {
        assert_eq!(
            MIN_SLOPE,
            crate::pipeline::roll_white::span_slope(10.0, -10.0)
        );
    }

    #[test]
    fn a_flat_frame_centred_is_not_pushed() {
        // 09-14 Ektar100 1719: the maximum slope on a 1.45-stop span; its dark end lands
        // 0.46 stops under mid-grey, its middle 0.88 above.
        let mut i = inputs();
        i.slope = 3.5;
        i.exposure = 0.53;
        i.tones = Some(FrameStops {
            white_stops: 0.79,
            level_stops: -0.28,
            dark_stops: -0.66,
        });
        assert_eq!(check(&i), vec![]);
    }

    #[test]
    fn pushed_needs_both_the_middle_and_the_dark_end_high() {
        let mut i = inputs();
        // Middle up, dark end still deep: a wide frame lifted, not washed out.
        i.tones = Some(FrameStops {
            white_stops: 2.0,
            level_stops: 0.0,
            dark_stops: -5.0,
        });
        assert_eq!(check(&i), vec![]);
        i.exposure += 3.5;
        let faults = check(&i);
        let [Fault::Pushed { middle, dark }] = faults[..] else {
            panic!("{faults:?}")
        };
        assert!(
            middle > MAX_MIDDLE_STOPS && dark > MAX_DARK_STOPS,
            "{faults:?}"
        );
        let message = faults[0].message();
        assert!(message.contains("`--exposure`"), "{message}");
        assert!(!message.contains("--contrast"), "{message}");
    }

    #[test]
    fn the_white_balance_s_luminance_gain_counts_as_exposure() {
        // Uniform gains of 2 are a stop of exposure the tones never saw.
        let mut i = inputs();
        i.exposure += 3.5;
        let pushed = check(&i);
        assert!(matches!(pushed[..], [Fault::Pushed { .. }]), "{pushed:?}");
        i.exposure -= 1.0;
        i.white_balance = [2.0; 3];
        assert_eq!(check(&i), pushed);
    }

    #[test]
    fn squeezed_names_contrast_and_only_contrast() {
        let mut i = inputs();
        i.contrast = 0.5;
        i.slope = 0.8;
        let faults = check(&i);
        assert_eq!(
            faults,
            vec![Fault::Squeezed {
                slope: 0.8,
                contrast: 0.5,
                base_slope: 1.6,
                base_from: "the roll's slope",
            }]
        );
        let message = faults[0].message();
        assert!(message.contains("raise `--contrast`"), "{message}");
        assert!(
            message.contains("`--contrast` 0.500 times the roll's slope 1.600"),
            "{message}"
        );
        assert!(!message.contains("--exposure"), "{message}");
    }

    #[test]
    fn at_the_floor_is_not_squeezed() {
        let mut i = inputs();
        i.slope = MIN_SLOPE;
        assert_eq!(check(&i), vec![]);
    }

    #[test]
    fn without_tones_the_base_decides() {
        let mut i = inputs();
        i.tones = None;
        i.roll_exposure = false;
        i.film_base_stops = MIN_FILM_BASE_STOPS;
        assert_eq!(check(&i), vec![]);
        i.film_base_stops = 1.2;
        let faults = check(&i);
        assert_eq!(faults, vec![Fault::Base { stops: 1.2 }]);
        let message = faults[0].message();
        assert!(message.contains("1.20 stops under mid-grey"), "{message}");
        assert!(message.contains("lower `--exposure`"), "{message}");
        // A base above mid-grey says so.
        i.film_base_stops = -0.4;
        assert!(check(&i)[0].message().contains("0.40 stops above mid-grey"));
    }

    #[test]
    fn a_roll_exposure_without_tones_leaves_the_base_unread() {
        // A thin roll's frame measure-roll did not measure: its measured exposure puts
        // the base near the threshold.
        let mut i = inputs();
        i.tones = None;
        i.film_base_stops = 1.8;
        assert_eq!(check(&i), vec![]);
        // An exposure stated on top is the user's push, and is checked.
        i.stated_exposure = 0.3;
        assert_eq!(check(&i), vec![Fault::Base { stops: 1.8 }]);
        // So is a stated white balance, by its gain on luminance; one that nets out is not.
        i.stated_exposure = 0.0;
        i.stated_white_balance = [8.0; 3];
        assert_eq!(check(&i), vec![Fault::Base { stops: 1.8 }]);
        i.stated_exposure = -3.0;
        assert_eq!(check(&i), vec![]);
    }

    #[test]
    fn with_tones_the_base_is_not_read() {
        // A thin roll's default render: the base at 2.02 stops, the frame placed well.
        let mut i = inputs();
        i.film_base_stops = 1.0;
        assert_eq!(check(&i), vec![]);
    }

    #[test]
    fn both_faults_are_reported_slope_first() {
        let mut i = inputs();
        i.tones = None;
        i.roll_exposure = false;
        i.slope = 0.8;
        i.contrast = 0.5;
        i.film_base_stops = 1.0;
        let faults = check(&i);
        assert!(
            matches!(faults[..], [Fault::Squeezed { .. }, Fault::Base { .. }]),
            "{faults:?}"
        );
    }
}

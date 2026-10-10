//! Which base the rendering stages start from: `--rendering default|direct` (recipe
//! `rendering`). `default` applies the recipe's `roll` section with every other knob at
//! its default; `direct` leaves the roll out and starts from pinned values ([`DIRECT`]).
//! A stated knob builds on either (`Recipe::shared_params`).
//!
//! Design, and the procedure for changing `DIRECT`: design-spec §6,
//! "Two renderings".

use serde::{Deserialize, Serialize};

use crate::destination::{Container, Defaults, Gamut, Range, Transfer};
use crate::pipeline::fit_range::{DEFAULT_DISPLAY_BLACK_STOPS, DisplayBlack};
use crate::pipeline::look::{DEFAULT_SATURATION, DEFAULT_SLOPE, HighlightDesaturation};
use crate::types::DEFAULT_HEADROOM_STOPS;

/// The rendering a run starts from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Rendering {
    /// Our code plus the roll's measurements.
    #[default]
    Default,
    /// As little as possible: only what the container needs.
    Direct,
}

impl Rendering {
    /// What this rendering's knobs start from.
    pub fn base(self) -> Base {
        match self {
            Rendering::Default => DEFAULT,
            Rendering::Direct => DIRECT,
        }
    }
}

/// A rendering's starting values. White balance, exposure and the grade are absent: both
/// renderings start them at the identity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Base {
    /// Whether the recipe's `roll` section is applied.
    pub applies_roll: bool,
    /// The slope `look.contrast` and `look.saturation` multiply when no applied roll white
    /// gives one.
    pub slope: f32,
    /// The colour over the base slope's that `look.saturation` multiplies: the
    /// saturation slope is the base slope (never a thin frame's) times this times the knob.
    pub saturation: f32,
    pub highlight_desaturation: HighlightDesaturation,
    pub headroom_stops: f32,
    pub display_black: DisplayBlack,
    /// The destination axes an unset axis takes (`crate::destination::resolve`).
    pub axes: Defaults,
}

/// `default`: the roll applied, everything else at its default.
const DEFAULT: Base = Base {
    applies_roll: true,
    slope: DEFAULT_SLOPE,
    saturation: DEFAULT_SATURATION,
    highlight_desaturation: HighlightDesaturation::DEFAULT,
    headroom_stops: DEFAULT_HEADROOM_STOPS,
    display_black: DisplayBlack::StopsBelowMid(DEFAULT_DISPLAY_BLACK_STOPS),
    axes: Defaults::STANDARD,
};

/// `direct`: every value written out, so a moved default leaves it alone.
pub const DIRECT: Base = Base {
    applies_roll: false,
    // The slope, not the whole slope, so a moved linearization still shows. The
    // fallback's value (a white +1.75 stops up) as of `nf-calibration/no-roll-defaults`.
    slope: 1.413_675,
    // Held: the colour the slope gave as a per-channel power, with no taste added.
    saturation: 1.0,
    // Off: it hides the residual cast an editor or the loop must see.
    highlight_desaturation: HighlightDesaturation {
        strength: 0.0,
        start_stops: -1.0,
        band: [0.015, 0.025],
    },
    headroom_stops: 6.0,
    // A monotone stretch, so it loses almost nothing.
    display_black: DisplayBlack::StopsBelowMid(6.0),
    // The float HDR TIFF; container first, so no lossy container comes from a default.
    axes: Defaults {
        range: Range::Hdr,
        transfer: Transfer::Linear,
        gamut: Gamut::AdobeRgb,
        container: Container::Tiff,
        linear_gamut: Some(Gamut::AdobeRgb),
        container_first: true,
    },
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_is_pinned() {
        // `direct` moved: follow design-spec §6, "Two renderings".
        let Base {
            applies_roll,
            slope,
            saturation,
            highlight_desaturation,
            headroom_stops,
            display_black,
            axes,
        } = DIRECT;
        assert!(!applies_roll);
        assert_eq!(slope, 1.413_675);
        assert_eq!(saturation, 1.0);
        assert_eq!(
            highlight_desaturation,
            HighlightDesaturation {
                strength: 0.0,
                start_stops: -1.0,
                band: [0.015, 0.025],
            }
        );
        assert!(highlight_desaturation.is_off());
        assert_eq!(headroom_stops, 6.0);
        assert_eq!(display_black, DisplayBlack::StopsBelowMid(6.0));
        assert_eq!(
            axes,
            Defaults {
                range: Range::Hdr,
                transfer: Transfer::Linear,
                gamut: Gamut::AdobeRgb,
                container: Container::Tiff,
                linear_gamut: Some(Gamut::AdobeRgb),
                container_first: true,
            }
        );
    }

    #[test]
    fn default_reads_the_defaults_through() {
        // The other half of the contract: `default` is not pinned, so it follows every
        // default it names.
        let base = Rendering::Default.base();
        assert!(base.applies_roll);
        assert_eq!(base.slope, DEFAULT_SLOPE);
        assert_eq!(base.saturation, DEFAULT_SATURATION);
        assert_eq!(
            base.highlight_desaturation,
            HighlightDesaturation::default()
        );
        assert_eq!(base.headroom_stops, DEFAULT_HEADROOM_STOPS);
        assert_eq!(base.display_black, DisplayBlack::default());
        assert_eq!(base.axes, Defaults::STANDARD);
    }
}

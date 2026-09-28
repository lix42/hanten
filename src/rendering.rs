//! Which base the rendering stages start from — `--rendering direct|default` (recipe
//! `rendering`, `nf-destinations/direct-preset`). Design: `docs/design-update.md`,
//! Part 2, "Two renderings".
//!
//! A render has three kinds of input: the fixed **decode**, the **roll**'s measurements
//! (`hanten measure-roll`, the recipe's `roll` section), and the user's **style**. A
//! rendering decides what the style knobs start from:
//!
//! - **`default`** is what our code produces from measured values alone: the roll section
//!   applied, and every other knob at its default.
//! - **`direct`** loses as little information as possible and applies only what the
//!   container needs: the roll section is left out (and reported so), white balance is
//!   the identity, highlight desaturation is off, and the rest is pinned here. It is the
//!   handoff to an external editor and, stated SDR, the rendering the calibration loop
//!   holds fixed (`nf-calibration/scale-gamma-loop`).
//!
//! A knob the user states builds on the base under either rendering: the white balance
//! multiplies it, and every other knob replaces its base value
//! (`crate::recipe::Recipe::shared_params`).
//!
//! # Keeping `direct` current — read this before changing a stage
//!
//! `direct`'s values are **its own constants** ([`DIRECT`]), never today's defaults read
//! through: moving a default must not move the rendering the calibration loop holds.
//! `direct_is_pinned` fails on any change to them, and on a new rendering knob, since the
//! resolver destructures every stage section without `..` and a new field does not
//! compile until it has a base. When either happens:
//!
//! 1. **Decide the knob's `direct` value by the principle**: is it needed to land in the
//!    container? Then the gentlest fixed value. Is it a choice of taste? Then its
//!    identity. Is it information-preserving, like display black's monotone stretch of
//!    the shadows? Then it may stay.
//! 2. **Update [`DIRECT`] and the pinned test together**, and the table in
//!    `docs/design-update.md`, Part 2, "Two renderings".
//! 3. **If `direct`'s output moved, log it** as a dated entry in
//!    `docs/progress/nf-calibration.md`: review rounds judged before and after the move no
//!    longer compare like for like.
//!
//! A task that changes a stage (e.g. `nf-display-stages/parametric-shoulder` replacing
//! reinhard) decides `direct`'s part as its own work.

use serde::{Deserialize, Serialize};

use crate::destination::{Container, Defaults, Gamut, Range, Transfer};
use crate::pipeline::fit_range::{DEFAULT_DISPLAY_BLACK_STOPS, DisplayBlack};
use crate::pipeline::look::{DEFAULT_CONTRAST, HighlightDesaturation};
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

/// A rendering's starting values: every knob whose base differs between renderings, or
/// that `direct` must pin. The white balance, the exposure and the per-channel grade are
/// not here — both renderings start them at the identity, and only the user moves them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Base {
    /// Whether the recipe's `roll` section is applied.
    pub applies_roll: bool,
    /// The look contrast when none is stated and no applied roll white gives one.
    pub contrast: f32,
    pub highlight_desaturation: HighlightDesaturation,
    pub headroom_stops: f32,
    pub display_black: DisplayBlack,
    /// The destination axes an unset axis takes (`crate::destination::resolve`).
    pub axes: Defaults,
}

/// `default`: the roll applied, everything else at its default.
const DEFAULT: Base = Base {
    applies_roll: true,
    contrast: DEFAULT_CONTRAST,
    highlight_desaturation: HighlightDesaturation::DEFAULT,
    headroom_stops: DEFAULT_HEADROOM_STOPS,
    display_black: DisplayBlack::StopsBelowMid(DEFAULT_DISPLAY_BLACK_STOPS),
    axes: Defaults::STANDARD,
};

/// `direct`: pinned, **not** read from the defaults (module docs). Every value is
/// written out, so a default moving elsewhere leaves this alone.
pub const DIRECT: Base = Base {
    // The decode's own output is left alone; the roll is a correction on top of it.
    applies_roll: false,
    // The bundled decode's rendering contrast, 2.0 / 1.8 (whole 2.0 at the 1.8
    // linearization). Pinned as a rendering contrast: a moved linearization is a decode
    // change, which is what the calibration loop exists to see.
    contrast: 2.0 / 1.8,
    // Off: it hides residual cast, which is the thing an editor or the loop must see.
    highlight_desaturation: HighlightDesaturation {
        strength: 0.0,
        start_stops: -1.0,
        band: [0.015, 0.025],
    },
    // Needed to land in the container: reinhard at 6 stops.
    headroom_stops: 6.0,
    // A monotone stretch of the shadows, not a compression, so it loses almost nothing;
    // kept on at 6 stops below mid-grey.
    display_black: DisplayBlack::StopsBelowMid(6.0),
    // HDR as linear float BT.2020 (the least lost); Adobe RGB when SDR is stated. The
    // container is decided first, so a stated axis that rules the float TIFF out falls
    // back to the lossless 16-bit TIFF. A lossy container never comes from a default:
    // only when it is stated, or when the stated axes leave no lossless row (`--range hdr
    // --gamut display-p3` is the gain map, the one row left).
    axes: Defaults {
        range: Range::Hdr,
        transfer: Transfer::Linear,
        gamut: Gamut::AdobeRgb,
        container: Container::Tiff,
        container_first: true,
    },
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_is_pinned() {
        // A failure here means `direct`'s rendering moved. That is allowed, but not
        // silently: follow "Keeping `direct` current" in this module's docs
        // (src/rendering.rs) — decide the value by the principle, update the table in
        // docs/design-update.md (Part 2, "Two renderings"), and if the output moved, log
        // it in docs/progress/nf-calibration.md.
        let Base {
            applies_roll,
            contrast,
            highlight_desaturation,
            headroom_stops,
            display_black,
            axes,
        } = DIRECT;
        assert!(!applies_roll);
        assert_eq!(contrast, 1.111_111_2);
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
        assert_eq!(base.contrast, DEFAULT_CONTRAST);
        assert_eq!(
            base.highlight_desaturation,
            HighlightDesaturation::default()
        );
        assert_eq!(base.headroom_stops, DEFAULT_HEADROOM_STOPS);
        assert_eq!(base.display_black, DisplayBlack::default());
        assert_eq!(base.axes, Defaults::STANDARD);
    }
}

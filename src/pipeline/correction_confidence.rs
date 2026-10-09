//! **How far to trust a roll's corrections** (`nf-scene-correction/correction-confidence`):
//! the roll white balance ([`super::roll_white`]) and the midtone line
//! ([`super::midtone_neutral`]) are measured from the roll's own frames, so a short roll
//! measures them loosely. `measure-roll` grades each one and advises when it is in doubt
//! (outside `--strict`: a whole short roll has no more frames); it never turns one off.
//!
//! **Frame count is the one signal.** Over the archive rolls, a random draw of 10 frames
//! moved the white balance up to 263 (90th percentile, log2 × 1000) from its whole
//! roll's; 16 frames, 142, within the cast a whole roll leaves on its neutral patches
//! (67–201). Leaving one frame out did not predict a draw's error, and no statistic
//! inside a roll told a roll of one dominant colour (beach, sea) from a roll with that
//! cast. The evidence: `docs/progress/nf-scene-correction.md`, `correction-confidence`.

use serde::Serialize;

/// Fewest frames for a measured correction to be confident.
pub const CONFIDENT_FRAMES: usize = 16;

/// A correction's grade.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tier {
    Confident,
    /// Applied, with advice.
    InDoubt,
}

/// One correction's grade and what decided it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Assessment {
    pub tier: Tier,
    /// The frames it was measured from.
    pub frames: usize,
    /// [`CONFIDENT_FRAMES`].
    pub confident_from: usize,
}

/// Grade a correction measured from `frames` frames.
pub fn assess(frames: usize) -> Assessment {
    Assessment {
        tier: if frames >= CONFIDENT_FRAMES {
            Tier::Confident
        } else {
            Tier::InDoubt
        },
        frames,
        confident_from: CONFIDENT_FRAMES,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_roll_is_in_doubt_below_the_frame_count_and_confident_from_it() {
        assert_eq!(assess(CONFIDENT_FRAMES - 1).tier, Tier::InDoubt);
        assert_eq!(assess(CONFIDENT_FRAMES).tier, Tier::Confident);
        assert_eq!(assess(1).frames, 1);
    }
}

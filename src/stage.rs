//! The stages a conversion runs, named once.
//!
//! The report's `chain.stages`, the telemetry record's `timing_ms` and
//! `telemetry/schema-v2`'s failure events all key on [`StageKind`], so a stage added
//! or renamed here moves all three together. Its serde names are wire strings: a
//! rename is a telemetry schema bump and a report change.
//!
//! Stages stay clock-free. The orchestrator times them by handing the chain a
//! [`StageClock`]; tests pass `Untimed`.

use serde::Serialize;

/// One stage of a conversion, in run order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StageKind {
    /// Reading the scan (`io::decode`).
    Decode,
    /// Estimating the film base and the effective area (`pipeline::film_base`).
    FilmBase,
    /// The fixed decode and the NC film RGB v1 3×3 into ACEScg (`algo::fixed`).
    Reconstruction,
    SceneCorrection,
    Look,
    FitRange,
    FitGamut,
    /// The destination's transfer: the display curve and ICC profile, the Rec.2100
    /// signal, or the gain map.
    Destination,
    /// Writing the container.
    Encode,
    /// Writing the `--export-ir` plane.
    IrExport,
}

#[cfg(test)]
impl StageKind {
    /// Every stage, in run order; `tests::all_lists_every_variant_in_order` keeps it whole.
    pub const ALL: [StageKind; 10] = [
        StageKind::Decode,
        StageKind::FilmBase,
        StageKind::Reconstruction,
        StageKind::SceneCorrection,
        StageKind::Look,
        StageKind::FitRange,
        StageKind::FitGamut,
        StageKind::Destination,
        StageKind::Encode,
        StageKind::IrExport,
    ];
}

/// Times a stage by running it. The one impure seam a stage's caller holds.
pub trait StageClock {
    fn time<T>(&mut self, stage: StageKind, run: impl FnOnce() -> T) -> T;
}

/// A clock that records nothing.
#[cfg(test)]
pub struct Untimed;

#[cfg(test)]
impl StageClock for Untimed {
    fn time<T>(&mut self, _: StageKind, run: impl FnOnce() -> T) -> T {
        run()
    }
}

#[cfg(test)]
mod tests {
    use super::StageKind;

    /// Exhaustive, so a new variant fails to compile here: give it the next index
    /// and add it to `ALL`.
    fn index(stage: StageKind) -> usize {
        match stage {
            StageKind::Decode => 0,
            StageKind::FilmBase => 1,
            StageKind::Reconstruction => 2,
            StageKind::SceneCorrection => 3,
            StageKind::Look => 4,
            StageKind::FitRange => 5,
            StageKind::FitGamut => 6,
            StageKind::Destination => 7,
            StageKind::Encode => 8,
            StageKind::IrExport => 9,
        }
    }

    #[test]
    fn all_lists_every_variant_in_order() {
        for (i, &stage) in StageKind::ALL.iter().enumerate() {
            assert_eq!(index(stage), i, "{stage:?}");
            // Declaration order, so a variant inserted mid-enum is caught too.
            assert_eq!(stage as usize, i, "{stage:?}");
        }
    }

    #[test]
    fn wire_names_are_snake_case() {
        let json = serde_json::to_string(&[
            StageKind::FilmBase,
            StageKind::SceneCorrection,
            StageKind::IrExport,
        ])
        .unwrap();
        assert_eq!(json, r#"["film_base","scene_correction","ir_export"]"#);
    }
}

//! Which rendering chain a run resolves — the `--new-flow` migration selector.
//!
//! **Scaffolding with a written expiry**, not a feature (`docs/nf-migration.md`,
//! `docs/tasks/nf-core/new-flow-flag.md`). The whole file is deleted by
//! `nf-core/default-flip`, when the new chain becomes the only chain and
//! `--new-flow` becomes a removed-flag error on the `--algorithm` precedent.
//!
//! Two things live here, so the migration's surface is one file rather than a
//! scatter of `if` arms across `cli`:
//!
//! - [`Flow`] — the selected chain. An enum rather than a bool because the
//!   orchestrator passes it down to the render seam, and because the flip deletes
//!   a variant rather than inverting a flag.
//! - The **availability tables** — the flags the new flow refuses, and the two
//!   different sentences it refuses them with ([`Availability`]).
//!
//! The knobs the new flow keeps reach the fixed decode through the new chain's own
//! recipe (`crate::recipe`, which outlives this module), and the render itself —
//! decode, `pipeline::chain`, and the one destination — is `cli::convert_frame`'s.
//!
//! `Flow` is *orchestration state*, like an unresolved `calibration.film_base`: it
//! never reaches a stage, and it is never a recipe key (`--new-flow` selects which
//! knobs exist; it does not set one). Unlike `--report` / `--telemetry` /
//! `--max-memory`, though, it is **not** in the "can never change a pixel" class —
//! choosing a chain is exactly a choice of pixels, which is why it must stay out
//! of the recipe rather than merely out of the image.

use crate::cli::ConvertArgs;
use crate::types::{NcError, Result};

/// The rendering chain a run resolves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Flow {
    /// The shipped chain (reconstruction → print → output). The default until
    /// `nf-core/default-flip`.
    #[default]
    Legacy,
    /// The chain from `docs/design-update.md`, selected by `--new-flow`.
    New,
}

impl Flow {
    /// Resolve the `--new-flow` presence flag.
    pub fn from_flag(new_flow: bool) -> Self {
        if new_flow { Self::New } else { Self::Legacy }
    }
}

/// Why a knob is unavailable — and which of three *different* sentences it earns.
///
/// The distinction is the user's next action, so it is modelled rather than
/// written into each message: "wait for the stage that carries it", "this idea is
/// gone" and "it is spelled differently here" are not the same advice, and a single
/// generic string would have to pick one and be wrong for most of the table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Availability {
    /// No counterpart **yet**: the capability is planned, in a stage that has not
    /// landed. `arriving_with` completes "…arrives with {}".
    NotYet { arriving_with: &'static str },
    /// No counterpart **ever**: the new design drops the idea. `reason` completes
    /// "…and will not gain one: {}", and `instead` names a replacement when one
    /// exists.
    Never {
        reason: &'static str,
        instead: Option<&'static str>,
    },
    /// The same capability under **another spelling**, set by the task that built its
    /// stage. `to` is the new flag and `why` completes "…is {to}: {}".
    Renamed { to: &'static str, why: &'static str },
}

/// Whether the command line *names* a curve that has no knees.
///
/// The knee rows below accept a **zero** knee as an identity value, which is what
/// keeps the flags-win reset usable. Beside a typed non-sigmoid curve that reading
/// stops holding: the pair is a contradiction, and `merge` refuses it with "pass
/// `--density-curve sigmoid`" — a curve this flow then refuses, closing a loop in
/// which *neither* message states the action that works (drop the knee flag). So the
/// rows fire on the pair too, pre-`merge`, where the generic "Drop it for now"
/// remedy is the working one. Reads only flags the user typed, never a resolved
/// value, so a recipe-pinned curve cannot trip it.
fn a_curve_without_knees_is_typed(args: &ConvertArgs) -> bool {
    matches!(
        args.density_curve,
        Some(crate::types::DensityCurveType::Exponential)
            | Some(crate::types::DensityCurveType::Characteristic)
    )
}

/// A knob refused by **flag presence**, checked before `merge`.
struct FlagEntry {
    /// The knob as the user typed it.
    knob: &'static str,
    /// The `convert` flags this row classifies, spelled as `--help` spells them.
    ///
    /// Only flags: every *recipe* path is classified by the new chain's own schema
    /// (`crate::recipe`), which refuses a key it does not define at load — by name,
    /// with where it went, when the key belongs to the current chain's recipe. A flag
    /// needs its own row because the flag parser knows nothing of that schema.
    ///
    /// What this field buys is `every_convert_flag_is_classified`, which reads the
    /// flag surface back out of `cli.rs` and fails on a knob no row mentions. Without
    /// it the tables are a list of refusals, in which "considered and kept" and
    /// "nobody looked" are the same state.
    // Read only by that test — the runtime needs the display `knob`, not the ids.
    // Kept beside the row rather than in the test, because a classification that
    // lives apart from the thing it classifies is one that goes stale.
    #[allow(dead_code)]
    covers: &'static [&'static str],
    present: fn(&ConvertArgs) -> bool,
    availability: Availability,
}

/// Shared refusal reasons, so rows that say the same thing cannot drift apart.
///
/// The anchor family needs **two** of them, because "reference-free" does not
/// discriminate all three placements it refuses:
/// [`AnchorPlacement::reads_reference`](crate::types::AnchorPlacement::reads_reference)
/// is `false` for `BlackAtBase` too, so telling a `--anchor-black-floor` user that the
/// decode's rule was chosen for being reference-free names a property their own rule
/// has. For that row the reason is simply that there is one rule.
const ANCHOR_RULE_REASON: &str = "the decode has one anchor rule — mid-grey pinned a fixed density above the film \
     base — so which tone the decode pins is no longer a choice it offers";
const ANCHOR_REFERENCE_REASON: &str = "the decode has one anchor rule — mid-grey pinned a fixed density above the film \
     base — and it is reference-free, so a leader measurement's roll-to-roll error \
     cannot reach the render";
const DMAX_REASON: &str = "the anchor rule never reads a reference density, so nothing in the new flow \
     resolves one; a `Dmax` measured from a leader is film saturation, which is \
     neither diffuse white nor the density this decode pins — it pins mid-grey a fixed \
     density above the film base, which every scan carries, so nothing has to be \
     stated in the reference's place";
const FIT_RANGE_ARRIVES_WITH: &str = "the fit-range stage, which is the new home of every display tone — it exists \
     as an identity pass today, so there is no operator yet for a tone selector or a \
     headroom to configure (`nf-display-stages/fit-range`)";
const DESTINATION_ARRIVES_WITH: &str = "the new flow's destination set: it renders into exactly one destination today \
     (a Display P3 16-bit TIFF), so there is no output policy to choose or describe \
     (`nf-destinations/preset-set`)";
const BALANCE_ARRIVES_WITH: &str = "the look stage's per-channel grade, which subsumes it: adjusting channels by tone \
     region is a grade, and it is also the one term in the old chain that could be \
     non-monotone, so it cannot sit in a decode that loses nothing by construction \
     (`nf-look/per-channel-grade`)";

/// Knobs with no new-flow meaning, keyed on the **flag the user typed**.
///
/// Per CLAUDE.md's recorded tiebreaker: reject a flag when it *forces something the
/// branch cannot produce*, and leave an identity value alone. A non-zero knee width
/// forces a knee the fixed decode has no way to render, while a zero one asks for
/// *less* shaping, toward the straight line the new flow decodes with — zeroing both
/// knees reaches it exactly (`docs/design-update.md`, methods table). Zeroing one
/// alone leaves the other at its default, so it is not by itself a knee-less curve;
/// it is accepted because it asks for nothing this flow cannot do.
///
/// That is the *whole* reason, and the tiebreaker's usual second one does not apply
/// here: "an identity value keeps the flags-win reset usable" presumes a recipe that
/// pinned the knob, and the new chain's recipe (`crate::recipe`) has no knee to pin —
/// its `reconstruction` is the fixed decode's, and a recipe stating the current
/// chain's `curve` is refused at load. Where no recipe can hold the knob there is
/// nothing left to reset, so an identity value has to earn its acceptance on its own
/// — which the zero knee does and `--no-d-max` does not.
///
/// **How a knob the user never typed is handled**, which this table alone cannot do.
/// It sees only the flag, so the same knee stated by a *recipe* or expanded from
/// `--preset sigmoid-knees` is invisible here — and a value rule cannot be added
/// beside it, because the shipped default sigmoid *has* knees (`toe: 0.2`), so
/// "refuse a non-zero resolved knee" would refuse every `--new-flow` run. The fixed
/// decode closes that from the other end: it reads its own [`DecodeParams`], which is
/// the new chain's recipe section `reconstruction` field for field
/// (`nf-core/recipe-schema`), so a recipe stating the current chain's keys there is
/// refused at load and `--preset` is refused by presence. Nothing the user *asks
/// for* in the new flow's **reconstruction** is silently dropped. (An identity flag
/// — a zero knee, `--reconstruction density`, `--density-curve exponential` — sets
/// nothing, because the new recipe has no field for it to set.)
/// The rest of the surface is closed by the same schema: it has no `print` or
/// `output` section, so a recipe stating either is refused by name, and every flag
/// under them has a row below. The shared sections (`input`, `calibration`'s film
/// base, `measure`) are read.
/// The asymmetry that remains here is benign and deliberate: typing
/// `--sigmoid-toe 0.2`, which resolves
/// today's default, is refused while the same resolved config with no flag is simply
/// not consulted.
///
/// [`DecodeParams`]: crate::algo::fixed::DecodeParams
///
/// The ordering face of the same gap is why these are **presence** rules, checked
/// before `merge`: a value rule cannot run until `merge` has resolved a value, so any
/// command line `merge` itself refuses would be diagnosed by the legacy chain first.
///
/// **The inventory is complete**, and `every_convert_flag_is_classified` is what
/// keeps it so: every conversion flag is either refused by a row here, kept by a row
/// in `KEPT_FLAGS`, or named a non-knob — and `crate::recipe`'s tests hold the recipe
/// half, that every section and key of the current chain's recipe is either shared
/// with the new one or refused by name. The reconstruction rows are
/// `nf-reconstruction/fixed-decode`'s, landed with the decode that strands them; the
/// print, output and kept rows are `nf-core/knob-availability-audit`'s. Adding a row
/// changes no message, ordering or call site.
///
/// The one thing the inventory does **not** carry is a renamed-knob mapping table
/// written ahead of the stages. A `NotYet` names the *task* that will carry the knob,
/// not a flag spelling, because the spelling belongs to the task that builds the
/// stage. When that task lands and chooses a new spelling, the row becomes
/// [`Availability::Renamed`] in the same change (`--print-exposure` → `--exposure`),
/// so a rename is only ever stated once the new flag exists.
const FLAG_ENTRIES: &[FlagEntry] = &[
    // The only rule `simple` needs. Its recipe spelling, `reconstruction.type`, is
    // not a key of the new chain's recipe and is refused at load (`crate::recipe`),
    // and the new flow merges its flags into that recipe rather than into the current
    // chain's `reconstruction`, so no resolved value can carry it past this row. The
    // flag has no identity value to protect — its other value is `density`, which
    // names what this flow already does.
    FlagEntry {
        knob: "--reconstruction simple",
        covers: &["--reconstruction"],
        present: |args| {
            matches!(
                args.reconstruction,
                Some(crate::types::ReconstructionType::Simple)
            )
        },
        availability: Availability::Never {
            reason: "`1 - T/T_base` is an affine inversion of transmission, not a decode of \
                     anything a print sees, so the fixed decode has nothing to map it onto",
            instead: Some("`--reconstruction density`"),
        },
    },
    FlagEntry {
        knob: "--sigmoid-toe",
        covers: &["--sigmoid-toe"],
        present: |args| {
            args.sigmoid
                .sigmoid_toe
                .is_some_and(|w| w != 0.0 || a_curve_without_knees_is_typed(args))
        },
        availability: Availability::NotYet {
            arriving_with: "the fit-range stage, which is where a toe belongs: shaping \
                            the approach to black needs the display's range, and \
                            reconstruction does not know it (`nf-display-stages/fit-range`; \
                            whether its operator gains an explicit toe is \
                            `nf-display-stages/parametric-operator`'s to decide)",
        },
    },
    FlagEntry {
        knob: "--sigmoid-shoulder",
        covers: &["--sigmoid-shoulder"],
        present: |args| {
            args.sigmoid
                .sigmoid_shoulder
                .is_some_and(|w| w != 0.0 || a_curve_without_knees_is_typed(args))
        },
        availability: Availability::NotYet {
            arriving_with: "the fit-range stage: compressing highlights inside \
                            reconstruction is what discards the range an HDR rendition \
                            exists to carry (`nf-display-stages/fit-range`)",
        },
    },
    // --- what the fixed decode strands (`nf-reconstruction/fixed-decode`) ----------
    //
    // These rows are the reconstruction half of `nf-core/knob-availability-audit`,
    // landed with the decode that strands them rather than left accepted-and-ignored
    // in the meantime. That audit has since landed the rest — the `print.*` and
    // `output.*` rows below, `KEPT_FLAGS`, and the exhaustiveness test. It carries no
    // renamed-knob mapping table, deliberately; see the rustdoc above.
    //
    // The decode's *surviving* knobs are deliberately absent from this table and stay
    // reachable: `--density-scale`, `--density-offset`, `--density-gamma` and
    // `--anchor-mid-offset` are exactly the calibration and the anchor
    // `algo::fixed::DecodeParams` carries — the new chain's recipe section
    // `reconstruction`, into which `crate::recipe::merge` writes them and from which
    // the decode reads. `--density-gamma` works bare: the new flow does not run the
    // current chain's `merge`, whose sigmoid default used to refuse it unless
    // `--density-curve exponential` came with it.
    FlagEntry {
        knob: "--density-curve",
        covers: &["--density-curve"],
        // `--density-curve exponential` names the curve this flow already decodes
        // with, so it forces nothing and stays accepted — the tiebreaker's identity
        // value. Not to preserve a reset: the new chain's recipe has no curve to pin,
        // so there is none to preserve. The other two
        // select a curve the fixed decode does not have.
        present: |args| {
            matches!(
                args.density_curve,
                Some(crate::types::DensityCurveType::Sigmoid)
                    | Some(crate::types::DensityCurveType::Characteristic)
            )
        },
        availability: Availability::Never {
            reason: "reconstruction is one fixed decode for every negative — a straight \
                     line in density against log exposure — so which curve to use is no \
                     longer a choice the decode offers",
            instead: Some("`--density-curve exponential`, which names what it already does"),
        },
    },
    FlagEntry {
        knob: "--film-stock",
        covers: &["--film-stock"],
        present: |args| args.density.film_stock.is_some(),
        availability: Availability::NotYet {
            arriving_with: "the look stage, as an optional per-stock normalization on top \
                            of the fixed decode: inverting each stock's own curve returns \
                            every stock to the same scene contrast, which is a choice about \
                            how the picture should look rather than a decode of what the \
                            negative holds (`nf-look/stock-data-home`)",
        },
    },
    FlagEntry {
        knob: "--sigmoid-contrast",
        covers: &["--sigmoid-contrast"],
        present: |args| args.sigmoid.sigmoid_contrast.is_some(),
        availability: Availability::Never {
            reason: "it is the sigmoid's slope, and the fixed decode is the straight line",
            instead: Some("`--density-gamma`, this decode's own contrast"),
        },
    },
    // The three placements the one anchor rule replaces. `--anchor-mid-offset` is
    // absent from this table on purpose: it *is* the rule's `d`.
    FlagEntry {
        knob: "--anchor-white-at-reference",
        covers: &["--anchor-white-at-reference"],
        present: |args| args.anchor.anchor_white_at_reference,
        availability: Availability::Never {
            reason: ANCHOR_REFERENCE_REASON,
            instead: Some("`--anchor-mid-offset`, the one rule the decode has"),
        },
    },
    FlagEntry {
        knob: "--anchor-mid-fraction",
        covers: &["--anchor-mid-fraction"],
        present: |args| args.anchor.anchor_mid_fraction.is_some(),
        availability: Availability::Never {
            reason: ANCHOR_REFERENCE_REASON,
            instead: Some("`--anchor-mid-offset`, the one rule the decode has"),
        },
    },
    FlagEntry {
        knob: "--anchor-black-floor",
        covers: &["--anchor-black-floor"],
        present: |args| args.anchor.anchor_black_floor.is_some(),
        availability: Availability::Never {
            reason: ANCHOR_RULE_REASON,
            instead: Some("`--anchor-mid-offset`, the one rule the decode has"),
        },
    },
    // The reference-density family. **All four**, including `--no-d-max`, and that is
    // the tiebreaker applied rather than waived: an identity value is one that asks
    // for nothing *of a knob this flow has*, and the fixed decode has no reference
    // density at all — so "resolve it from nowhere" is a statement about a quantity
    // that does not exist here, not a reset of one. Nothing in the new flow would
    // read what any of them resolved.
    FlagEntry {
        knob: "--d-max",
        covers: &["--d-max"],
        present: |args| args.dmax.d_max.is_some(),
        availability: Availability::Never {
            reason: DMAX_REASON,
            instead: None,
        },
    },
    FlagEntry {
        knob: "--fixed-d-max",
        covers: &["--fixed-d-max"],
        present: |args| args.dmax.fixed_d_max,
        availability: Availability::Never {
            reason: DMAX_REASON,
            instead: None,
        },
    },
    FlagEntry {
        knob: "--auto-d-max",
        covers: &["--auto-d-max"],
        present: |args| args.dmax.auto_d_max,
        availability: Availability::Never {
            reason: DMAX_REASON,
            instead: None,
        },
    },
    FlagEntry {
        knob: "--no-d-max",
        covers: &["--no-d-max"],
        present: |args| args.dmax.no_d_max,
        availability: Availability::Never {
            reason: DMAX_REASON,
            instead: None,
        },
    },
    // The regional balance. Zero is identity and stays accepted; the range flags are
    // consulted only when a balance is non-zero, so alone they force nothing and are
    // deliberately not listed.
    FlagEntry {
        knob: "--shadow-balance",
        covers: &["--shadow-balance"],
        present: |args| args.density.shadow_balance.is_some_and(|b| b != [0.0; 3]),
        availability: Availability::NotYet {
            arriving_with: BALANCE_ARRIVES_WITH,
        },
    },
    FlagEntry {
        knob: "--highlight-balance",
        covers: &["--highlight-balance"],
        present: |args| {
            args.density
                .highlight_balance
                .is_some_and(|b| b != [0.0; 3])
        },
        availability: Availability::NotYet {
            arriving_with: BALANCE_ARRIVES_WITH,
        },
    },
    // A preset sets knobs on **both** sides of the decode/rendering boundary — the
    // curve, `density.scale`, `print_exposure`, `display_tone` — so it cannot be
    // resolved against a chain whose rendering knobs do not exist yet. It is also the
    // one conversion flag with no recipe key, which is why it needs a presence row
    // rather than a value one.
    FlagEntry {
        knob: "--preset",
        covers: &["--preset"],
        present: |args| args.preset.is_some(),
        availability: Availability::NotYet {
            arriving_with: "the look stage's presets, which is where a bundle spanning \
                            decode and rendering can be defined again (`nf-look/look-presets`)",
        },
    },
    // --- the print controls (`nf-core/knob-availability-audit`) --------------------
    //
    // The whole `print.*` family, by the same argument the decode settled: a stage
    // that owns its parameters reads no resolved section, so accepting one of these
    // would be accepting-and-ignoring. `pipeline::chain`'s four stages each carry
    // their own `Params`, so nothing under `print` reaches the new flow — which is
    // why the recipe half needs no value rules either: the new chain's recipe has no
    // `print` section, and refuses one by name at load.
    //
    // **Refused by presence, at every value, and the missing section is why.** The
    // tiebreaker would normally leave an identity value alone to keep the flags-win
    // reset usable — `--highlight-compress 0` resolves the documented default and
    // renders byte-identically. But that exemption exists to
    // let a flag clear a value a *recipe* pinned, and the new chain's recipe has no
    // `print` section, so on this flow there is never a print value to
    // reset. With nothing to protect, presence is the honest rule: each of these names
    // an operation whose stage is an identity pass, or — `--print-exposure` — a knob
    // the new chain spells differently.
    //
    // Every verdict here is `NotYet` except `--print-exposure`'s `Renamed` (its stage
    // has landed, under another spelling) — including the two display tones
    // `nf-retire/display-tones` removes outright. That is not a softer reading of
    // their fate: `Never` is a claim about the *knob*, and what a user needs to know
    // at this gate is that the stage which would carry any tone is empty. Whether
    // `shoulder` survives into it is that retirement's statement to make, not this
    // gate's, and stating it here would be a second place to keep it in step.
    FlagEntry {
        knob: "--print-exposure",
        covers: &["--print-exposure"],
        present: |args| args.print.print_exposure.is_some(),
        availability: Availability::Renamed {
            to: "`--exposure`",
            why: "the new chain has no print stage — exposure is a scene-referred \
                  correction, applied before the look and the display fit \
                  (recipe `scene_correction.exposure`)",
        },
    },
    FlagEntry {
        // Not simply renamed: today's single linear subtraction is **two** jobs, and
        // the new chain splits them — flare/fog removal on scene-referred values, and
        // display black in fit range. So there is no one knob to point at yet, which
        // is exactly what `NotYet` says and what an `instead` would get wrong.
        knob: "--black-point",
        covers: &["--black-point"],
        present: |args| args.print.black_point.is_some(),
        availability: Availability::NotYet {
            arriving_with: "the two stages that split it — a flare/fog subtraction on \
                            scene-referred values, and display black in fit range; today \
                            it is one subtraction doing both, which is why it is not a \
                            rename (`nf-scene-correction/flare-removal`)",
        },
    },
    FlagEntry {
        knob: "--linear-range",
        covers: &["--linear-range"],
        present: |args| args.print.linear_range.is_some(),
        availability: Availability::NotYet {
            arriving_with: "whichever stage takes the affine levels remap, under whatever \
                            name it takes there — retiring it outright is a listed outcome, \
                            so this is the one print knob that may not come back at all \
                            (`nf-scene-correction/levels-knob`)",
        },
    },
    FlagEntry {
        knob: "--display-tone",
        covers: &["--display-tone"],
        present: |args| args.print.display_tone.is_some(),
        availability: Availability::NotYet {
            arriving_with: FIT_RANGE_ARRIVES_WITH,
        },
    },
    FlagEntry {
        knob: "--display-tone-headroom",
        covers: &["--display-tone-headroom"],
        present: |args| args.print.display_tone_headroom.is_some(),
        availability: Availability::NotYet {
            arriving_with: FIT_RANGE_ARRIVES_WITH,
        },
    },
    FlagEntry {
        // `Never`, where the rest of the family is `NotYet`, and the difference is
        // real: this is not a control that needs a stage built for it, it is the knee
        // *width* of the `shoulder` tone. (`none` and `reinhard` both refuse a
        // non-default value outright — `DisplayTone::resolve` says `none` "applies no
        // shoulder to place" — so pairing it with `none` would misname its owner.)
        // Fit range's operator is
        // parameterised by the display's peak, so there is no knee width for this to
        // configure however that stage lands. Zero is the documented default and
        // still refused, because unlike a zeroed knee it does not ask for less
        // shaping — it asks for a curve this flow has no knee on at all.
        knob: "--highlight-compress",
        covers: &["--highlight-compress"],
        present: |args| args.print.highlight_compress.is_some(),
        availability: Availability::Never {
            reason: "it is the knee width of the `shoulder` display tone, which exists \
                     for a reconstruction already bounded at white — the fixed decode is \
                     unbounded, so fit range compresses against the display's peak \
                     instead and has no knee width to set",
            instead: None,
        },
    },
    // --- output (`nf-core/knob-availability-audit`) --------------------------------
    //
    // The new flow renders into **exactly one destination** (a Display P3 16-bit
    // TIFF, `nf-core/minimal-end-to-end`); the set, and how one is selected, is
    // `nf-destinations`'. So these are refused for a different reason than the print
    // family: not "the stage that carries it is empty", but "there is no output policy
    // to choose yet". Nothing under the flow reads `output.*` — not even the memory
    // preflight, which sizes the new flow with its own `RunProfile::NewFlowSdrTiff`.
    FlagEntry {
        knob: "--output-preset",
        covers: &["--output-preset"],
        present: |args| args.output_opts.output_preset.is_some(),
        availability: Availability::NotYet {
            arriving_with: DESTINATION_ARRIVES_WITH,
        },
    },
    // Operational, and refused because the record names the resolved output preset,
    // the reconstruction and curve, and times the legacy chain's buckets (`algorithm` /
    // `color`) — so under `--new-flow` it would describe a chain the run did not take,
    // and a telemetry record's existence reads as a successful run of what it names.
    FlagEntry {
        knob: "--telemetry / --telemetry-file",
        covers: &["--telemetry", "--telemetry-file"],
        present: |args| args.telemetry || args.telemetry_file.is_some(),
        availability: Availability::NotYet {
            arriving_with: "the new chain's report and telemetry shape, which decides \
                            which stages a record times and what it says ran \
                            (`nf-core/report-contract`)",
        },
    },
    // Anchors of the regional balance's tone ramp. The new chain's recipe has no
    // field for them — the balance they shape is refused above, and its successor is a
    // grade in the look — so they are refused at every value: with no recipe to hold
    // the knob, there is no reset for an identity value to protect.
    FlagEntry {
        knob: "--balance-range / --auto-balance-range",
        covers: &["--balance-range", "--auto-balance-range"],
        present: |args| args.density.balance_range.is_some() || args.density.auto_balance_range,
        availability: Availability::NotYet {
            arriving_with: BALANCE_ARRIVES_WITH,
        },
    },
];

/// A knob the new flow **reads** — the other half of the inventory.
///
/// `#[cfg(test)]` because nothing at runtime consults it: a kept knob is kept by
/// *not* being refused, so this table's only job is to make
/// [`every_convert_flag_is_classified`] able to fail. It is still the place to read
/// a verdict from — an absence from [`FLAG_ENTRIES`] is not one.
///
/// A refusal table alone cannot say whether a knob was considered, so every
/// conversion flag not in [`FLAG_ENTRIES`] is listed here with the reason it
/// survives. Nothing consults this at runtime: its job is to make
/// [`every_convert_flag_is_classified`] able to fail, and to give the next reader a
/// verdict rather than an absence.
#[cfg(test)]
struct KeptEntry {
    covers: &'static [&'static str],
    /// Why the new flow keeps it. For a knob that survives under a different name,
    /// this says which stage takes it — a rename is not a removal.
    why: &'static str,
}

/// Every conversion flag the new flow accepts, and why.
#[cfg(test)]
const KEPT_FLAGS: &[KeptEntry] = &[
    // Decode and film base are shared: the seam is taken after both, so these reach
    // the new flow unchanged and mean exactly what they mean today.
    KeptEntry {
        covers: &["--input-transfer", "--input-meaning"],
        why: "stage-1b input semantics, which both flows decode through",
    },
    KeptEntry {
        covers: &["--film-type"],
        why: "provenance only since `ir-usability-detection` — it gates nothing on either flow",
    },
    KeptEntry {
        covers: &["--film-base", "--base-region", "--auto-base"],
        why: "the film base is measured before the seam and is shared by both flows",
    },
    KeptEntry {
        covers: &["--export-ir"],
        why: "the IR plane is written from the decoded image after the render, at the \
              destination's depth — u16 for the one destination the new flow has",
    },
    KeptEntry {
        covers: &["--measure-inset"],
        why: "it resolves the reported `effective_area`, which every decoding run \
              reports on either flow, and which an auto white balance estimates over",
    },
    // The decode's own knobs — the calibration and the anchor that
    // `algo::fixed::DecodeParams` carries, which is also the new recipe's
    // `reconstruction` section; `crate::recipe::merge` sets each one there.
    KeptEntry {
        covers: &["--density-scale", "--density-offset"],
        why: "the decode's own calibration — `nf-calibration/scale-gamma-loop` owns the \
              values, this gate only keeps them reachable",
    },
    KeptEntry {
        covers: &["--density-gamma"],
        why: "the fixed decode's contrast (recipe `reconstruction.contrast`). \
              `nf-look/path-to-white` tunes against it directly, so it must stay reachable",
    },
    // Scene correction (`nf-scene-correction/stage`). `--exposure` is the new chain's
    // own spelling; white balance keeps the current chain's, since the knob means the
    // same thing on both — a per-channel gain on linear ACEScg, after the 3×3.
    KeptEntry {
        covers: &["--white-balance", "--auto-wb"],
        why: "scene correction's white balance (recipe `scene_correction.white_balance`); \
              an auto mode estimates over the effective area",
    },
    KeptEntry {
        covers: &["--exposure"],
        why: "scene correction's exposure (recipe `scene_correction.exposure`) — the new \
              chain's spelling of `--print-exposure`",
    },
    KeptEntry {
        covers: &["--anchor-mid-offset"],
        why: "the fixed decode's anchor (recipe `reconstruction.anchor`), `mid-at-base-offset`'s \
              `d`: every conversion knob is a flag and a recipe key, and \
              `nf-look/path-to-white` tunes against it directly",
    },
];

/// Refuse a **flag** the new flow has no meaning for.
///
/// Runs **before `merge`**, and that placement is load-bearing: a presence rule
/// placed after it is unreachable whenever `merge` refuses the same command line
/// first, and the user then gets a remedy pointing at a knob this flow rejects —
/// the circular-advice defect CLAUDE.md records shipping four times.
pub fn reject_unavailable_flags(flow: Flow, args: &ConvertArgs) -> Result<()> {
    if flow == Flow::Legacy {
        return reject_new_flow_only_flags(args);
    }
    for entry in FLAG_ENTRIES {
        if (entry.present)(args) {
            return Err(refusal(entry.knob, entry.availability));
        }
    }
    Ok(())
}

/// Refuse, on the **current** chain, a flag only the new chain reads — the other
/// direction of the same accepted-and-ignored hole: the current chain's `merge` has no
/// arm for it, so without this it would parse and do nothing.
fn reject_new_flow_only_flags(args: &ConvertArgs) -> Result<()> {
    if args.scene.exposure.is_some() {
        return Err(NcError::Usage(
            "--exposure sets the new chain's scene-correction exposure (recipe \
             `scene_correction.exposure`) and has no meaning without `--new-flow`; the \
             current chain's exposure is `--print-exposure`"
                .into(),
        ));
    }
    Ok(())
}

/// The one generic rejection, built in one place so a new table row needs no
/// wording of its own.
fn refusal(knob: &str, availability: Availability) -> NcError {
    // Verdict and remedy are resolved together: "wait for the stage that carries it"
    // and "this idea is gone" call for different next actions, and a remedy that fits
    // only one of them is the circular-advice defect in miniature.
    let (verdict, remedy) = match availability {
        Availability::NotYet { arriving_with } => (
            // No markdown emphasis: this is a terminal string, and asterisks print
            // as asterisks (the same reason the `--new-flow` help text is plain prose).
            format!(
                "the new flow has no counterpart for it yet — one arrives with \
                 {arriving_with}."
            ),
            "Drop it for now".to_string(),
        ),
        Availability::Never { reason, instead } => (
            format!("the new flow has no counterpart for it, and will not gain one: {reason}."),
            instead.map_or_else(|| "Drop it".to_string(), |i| format!("Use {i}")),
        ),
        Availability::Renamed { to, why } => (
            format!("the new flow's counterpart is {to}: {why}."),
            format!("Use {to}"),
        ),
    };
    // The trailing clause says only what this rule inspected. "…the current chain
    // still accepts it" was an unconditional claim about the *legacy* path made by a
    // rule that looked at the new one, and it is false whenever the same command line
    // is independently invalid there (`--density-curve exponential --sigmoid-toe 0.3`
    // is refused by the curve-mismatch rule; `--sigmoid-toe nan` by the finite check).
    // Sending the user to a branch that then refuses them is the circular-advice
    // defect this module's ordering exists to avoid.
    NcError::Usage(format!(
        "{knob} has no meaning under `--new-flow`: {verdict} {remedy}, or run without \
         `--new-flow`, where this knob has a meaning. (`--new-flow` is transitional \
         scaffolding; see docs/nf-migration.md.)"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// `convert` flags that are **not** conversion knobs, so the inventory owes them
    /// no row.
    ///
    /// Listed by name rather than skipped by a property — `hide = true` would also
    /// skip a hidden *knob*, and "takes no value" would skip `--auto-base`. Adding a
    /// flag to any of these groups therefore still forces a deliberate choice.
    const NON_KNOB_FLAGS: &[&str] = &[
        // Operational: arg-struct only, never a recipe key. `--dump-params` writes the
        // recipe of whichever chain the run selected — under `--new-flow`, the new
        // chain's document (`crate::recipe`).
        "--dump-params",
        "--strict",
        "--seed",
        // Plumbing: paths and the recipe itself, not settings inside it.
        "--output",
        "--params",
        // The selector. CLI-only for a third reason again — it chooses which knobs
        // exist — so it is not one of them.
        "--new-flow",
        // Removed-flag stubs: hidden args that exist only to emit a migration error on
        // the `--algorithm` precedent. Nothing resolves them, on either flow.
        "--algorithm",
        "--assume-linear",
        "--input-profile",
        "--invert-white-balance",
        "--clip-low",
        "--clip-high",
        "--output-hdr",
        "--output-sdr",
        "--out-depth",
        "--output-profile",
        "--bigtiff",
    ];

    /// Argument groups `ConvertArgs` flattens that carry no conversion knob.
    const NON_KNOB_GROUPS: &[&str] = &["MemoryArgs", "ReportArgs"];

    fn body_of<'a>(src: &'a str, name: &str) -> &'a str {
        let needle = format!("pub struct {name} {{");
        let start = src
            .find(&needle)
            .unwrap_or_else(|| panic!("no struct {name}"))
            + needle.len();
        let end = start
            + src[start..]
                .find("\n}\n")
                .unwrap_or_else(|| panic!("unterminated struct {name}"));
        &src[start..end]
    }

    /// The types `ConvertArgs` pulls in with `#[command(flatten)]`.
    fn flattened_groups(convert: &str) -> Vec<&str> {
        convert
            .match_indices("#[command(flatten)]")
            .map(|(i, _)| {
                let rest = &convert[i..];
                let field = rest.find("pub ").expect("a field after the attribute") + 4;
                let colon = rest[field..].find(':').expect("a typed field") + field + 1;
                let end = rest[colon..].find(',').expect("a terminated field") + colon;
                rest[colon..end].trim()
            })
            .collect()
    }

    /// Every `--long` flag declared in one struct body.
    ///
    /// Reads the declarations back out of the source, the way
    /// `a_boundary_type_can_be_minted_only_by_its_own_stage` reads the boundary types:
    /// the alternative is a hand-kept list of flags sitting beside the flags.
    fn flags_in(body: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut idx = 0;
        while let Some(rel) = body[idx..].find("#[arg(") {
            let open = idx + rel + "#[arg".len();
            let mut depth = 0usize;
            let mut end = 0usize;
            for (i, ch) in body[open..].char_indices() {
                match ch {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = open + i;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            assert!(end > open, "unbalanced #[arg(...)] near byte {open}");
            let attr = &body[open + 1..end];
            let after = &body[end..];
            let name = if let Some(p) = attr.find("long = \"") {
                let rest = &attr[p + "long = \"".len()..];
                rest[..rest.find('"').expect("a closed string")].to_string()
            } else if attr.split(',').any(|t| t.trim() == "long") {
                // `#[arg(long)]` — clap derives the flag from the field name.
                let field = after.find("pub ").expect("a field after the attribute") + 4;
                let colon = after[field..].find(':').expect("a typed field") + field;
                after[field..colon].trim().replace('_', "-")
            } else {
                // A positional, or an attribute carrying only `conflicts_with`/`alias`.
                idx = end;
                continue;
            };
            out.push(format!("--{name}"));
            idx = end;
        }
        out
    }

    /// Every conversion flag `convert` accepts.
    fn convert_flag_surface() -> BTreeSet<String> {
        let src = include_str!("cli.rs");
        let convert = body_of(src, "ConvertArgs");
        let mut flags: BTreeSet<String> = flags_in(convert).into_iter().collect();
        let groups = flattened_groups(convert);
        assert!(
            groups.len() >= 10,
            "ConvertArgs' flattened groups were not found — the parser is reading the \
             wrong thing: {groups:?}"
        );
        for group in groups {
            if NON_KNOB_GROUPS.contains(&group) {
                continue;
            }
            flags.extend(flags_in(body_of(src, group)));
        }
        flags
    }

    /// Every id the inventory classifies, refused or kept.
    fn classified_ids() -> Vec<&'static str> {
        FLAG_ENTRIES
            .iter()
            .flat_map(|e| e.covers)
            .chain(KEPT_FLAGS.iter().flat_map(|e| e.covers))
            .copied()
            .collect()
    }

    #[test]
    fn every_convert_flag_is_classified() {
        // The inventory's whole contract. Without it these tables are a list of
        // refusals, in which a knob nobody considered and a knob deliberately kept are
        // the same state — and the failure the task exists to prevent is exactly that
        // silent one.
        let surface = convert_flag_surface();
        let classified: BTreeSet<&str> = classified_ids().into_iter().collect();

        let unclassified: Vec<&String> = surface
            .iter()
            .filter(|f| !classified.contains(f.as_str()) && !NON_KNOB_FLAGS.contains(&f.as_str()))
            .collect();
        for entry in KEPT_FLAGS {
            assert!(
                !entry.why.is_empty(),
                "{:?} is kept with no recorded reason — an absence is not a verdict",
                entry.covers
            );
        }
        assert!(
            unclassified.is_empty(),
            "unclassified conversion flags — give each a row in FLAG_ENTRIES or \
             KEPT_FLAGS, or a line in NON_KNOB_FLAGS: {unclassified:?}"
        );

        // And the other direction, so a renamed or deleted flag cannot leave a row
        // classifying nothing. A stale row is how an inventory starts lying.
        let dead: Vec<&str> = classified
            .iter()
            .copied()
            .chain(NON_KNOB_FLAGS.iter().copied())
            .filter(|id| !surface.contains(*id))
            .collect();
        assert!(
            dead.is_empty(),
            "rows classify flags that do not exist: {dead:?}"
        );
    }

    #[test]
    fn no_flag_is_classified_twice() {
        // Two rows matching one flag would make the diagnosis depend on row order.
        let ids = classified_ids();
        let mut seen = BTreeSet::new();
        for id in &ids {
            assert!(seen.insert(*id), "{id} is classified by two rows");
        }
        for id in &ids {
            assert!(
                !NON_KNOB_FLAGS.contains(id),
                "{id} is both classified and listed as a non-knob"
            );
        }
    }

    /// A kept flag is kept because the new flow *reads* it, so each one must reach the
    /// new chain's recipe — a kept flag with no arm in `recipe::merge` would be the
    /// accepted-and-ignored defect this inventory exists to prevent. Driven over
    /// [`KEPT_FLAGS`] itself, so a row added later without a sample here reds.
    #[test]
    fn every_kept_flag_reaches_the_recipe() {
        use crate::algo::fixed::AnchorRule;
        use crate::cli::{Cli, Command};
        use crate::pipeline::scene_correction::WhiteBalance;
        use crate::recipe::{self, Recipe};
        use crate::types::{FilmBaseSource, FilmType, MeaningAssertion, TransferAssertion};
        use clap::Parser;
        type Landed = fn(&Recipe) -> bool;
        // One command line per kept flag, each setting a non-default value, and the
        // one field it must land in — "the recipe changed" alone would pass a flag
        // wired to the wrong knob.
        let samples: &[(&str, &[&str], Landed)] = &[
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
            ("--auto-base", &["--auto-base"], |r| {
                r.calibration.film_base == Some(FilmBaseSource::Auto)
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
            ("--density-gamma", &["--density-gamma", "1.8"], |r| {
                r.reconstruction.contrast == 1.8
            }),
            (
                "--anchor-mid-offset",
                &["--anchor-mid-offset", "0.5"],
                |r| r.reconstruction.anchor == AnchorRule::MidAboveBase(0.5),
            ),
            ("--white-balance", &["--white-balance", "1.1,1,0.9"], |r| {
                r.scene_correction.white_balance == WhiteBalance::Explicit([1.1, 1.0, 0.9])
            }),
            ("--auto-wb", &["--auto-wb", "percentile"], |r| {
                r.scene_correction.white_balance == WhiteBalance::Percentile
            }),
            ("--exposure", &["--exposure", "-0.5"], |r| {
                r.scene_correction.exposure == -0.5
            }),
        ];
        for entry in KEPT_FLAGS {
            for flag in entry.covers {
                let (_, extra, landed) = samples
                    .iter()
                    .find(|(f, _, _)| f == flag)
                    .unwrap_or_else(|| panic!("kept flag {flag} has no sample here"));
                let argv = ["hanten", "convert", "in.tif", "-o", "out", "--new-flow"]
                    .iter()
                    .chain(extra.iter())
                    .copied();
                let Command::Convert(args) = Cli::try_parse_from(argv).unwrap().command else {
                    unreachable!()
                };
                let merged = recipe::merge(Recipe::default(), &args);
                assert!(
                    landed(&merged),
                    "{flag} did not land in its field: {merged:?}"
                );
                // Falsifiability: the default does not already satisfy the check.
                assert!(!landed(&Recipe::default()), "{flag}'s check is vacuous");
            }
        }
    }

    #[test]
    fn the_two_verdicts_read_differently() {
        // The audit's open question is which sentence a knob gets, so the two must be
        // distinguishable in the output rather than only in the type.
        let not_yet = refusal("--knob", Availability::NotYet { arriving_with: "X" });
        let never = refusal(
            "--knob",
            Availability::Never {
                reason: "Y",
                instead: None,
            },
        );
        assert!(not_yet.message().contains("no counterpart for it yet"));
        assert!(!not_yet.message().contains("will not gain one"));
        assert!(never.message().contains("will not gain one"));
        assert!(!never.message().contains("no counterpart for it yet"));
        // Terminal output, so no markdown survives into either sentence.
        assert!(!not_yet.message().contains('*') && !never.message().contains('*'));
    }

    #[test]
    fn no_table_lists_one_knob_twice() {
        // Two rows matching one knob would make the diagnosis depend on row order.
        let knobs: Vec<&str> = FLAG_ENTRIES.iter().map(|e| e.knob).collect();
        for knob in &knobs {
            assert!(!knob.is_empty(), "FLAG_ENTRIES has an unnamed row");
        }
        let mut sorted = knobs.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), knobs.len(), "duplicate knob in FLAG_ENTRIES");
    }
}

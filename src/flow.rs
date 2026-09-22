//! Which rendering chain a run resolves — the `--new-flow` migration selector.
//!
//! **Scaffolding with a written expiry**, not a feature (`docs/nf-migration.md`,
//! `docs/tasks/nf-core/new-flow-flag.md`). The whole file is deleted by
//! `nf-core/default-flip`, when the new chain becomes the only chain and
//! `--new-flow` becomes a removed-flag error on the `--algorithm` precedent.
//!
//! Three things live here, so the migration's surface is one file rather than a
//! scatter of `if` arms across `cli`:
//!
//! - [`Flow`] — the selected chain. An enum rather than a bool because the
//!   orchestrator passes it down to the render seam, and because the flip deletes
//!   a variant rather than inverting a flag.
//! - The **availability tables** — the knobs the new flow refuses, and the two
//!   different sentences it refuses them with ([`Availability`]).
//! - [`render_not_implemented`] — the seam itself. `nf-core/stage-skeleton` built
//!   the chain behind it (`pipeline::chain`); it stays closed until
//!   `nf-core/minimal-end-to-end` wires the decode (`algo::fixed`) to that chain and a
//!   destination behind it.
//!
//! `Flow` is *orchestration state*, like an unresolved `calibration.film_base`: it
//! never reaches a stage, and it is never a recipe key (`--new-flow` selects which
//! knobs exist; it does not set one). Unlike `--report` / `--telemetry` /
//! `--max-memory`, though, it is **not** in the "can never change a pixel" class —
//! choosing a chain is exactly a choice of pixels, which is why it must stay out
//! of the recipe rather than merely out of the image.

use crate::cli::{ConvertArgs, ResolvedConfig};
use crate::types::{DmaxSource, NcError, Reconstruction, Result};

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

/// Why a knob is unavailable — and which of two *different* sentences it earns.
///
/// The distinction is the user's next action, so it is modelled rather than
/// written into each message: "wait for the stage that carries it" and "this idea
/// is gone" are not the same advice, and a single generic string would have to
/// pick one and be wrong for half the table.
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
    /// Only flags: every *recipe* path under `reconstruction`, `print` or `output` is
    /// classified wholesale by [`UNREAD_RECIPE_SECTIONS`], because the new flow reads
    /// none of those sections. A flag needs its own row because it sets a resolved
    /// value with no recipe section for that witness to see.
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

/// A knob refused by **resolved value**, checked before every other validation
/// rule. Names both spellings, because a resolved value can come from a recipe
/// nobody typed a flag for.
struct ValueEntry {
    knob: &'static str,
    /// The `convert` flags this row classifies — see [`FlagEntry::covers`]. Empty
    /// when a [`FLAG_ENTRIES`] row already owns the flag and this is the value half
    /// of a dual rule (`simple`), non-empty when the value rule is the *only* rule,
    /// so that `every_convert_flag_is_classified` still sees the flag.
    #[allow(dead_code)]
    covers: &'static [&'static str],
    matches: fn(&ResolvedConfig) -> bool,
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
const SCENE_CORRECTION_ARRIVES_WITH: &str = "the scene-correction stage, which is where white balance and exposure land \
     once they are corrections toward what the scene was rather than print controls \
     (`nf-scene-correction/stage`)";
const FIT_RANGE_ARRIVES_WITH: &str = "the fit-range stage, which is the new home of every display tone — it exists \
     as an identity pass today, so there is no operator yet for a tone selector or a \
     headroom to configure (`nf-display-stages/fit-range`)";
const DESTINATION_ARRIVES_WITH: &str = "a destination for the new chain to render into: it has none yet, so there is \
     nothing for an output policy to describe (`nf-core/minimal-end-to-end` wires the \
     first one, `nf-destinations/preset-set` settles the set)";
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
/// pinned the knob, and [`reject_recipe_sections`] refuses any recipe stating
/// `reconstruction` at all. Where a section is refused whole there is nothing left to
/// reset, so an identity value has to earn its acceptance on its own — which the zero
/// knee does and `--no-d-max` does not.
///
/// **How a knob the user never typed is handled**, which this table alone cannot do.
/// It sees only the flag, so the same knee stated by a *recipe* or expanded from
/// `--preset sigmoid-knees` is invisible here — and a value rule cannot be added
/// beside it, because the shipped default sigmoid *has* knees (`toe: 0.2`), so
/// "refuse a non-zero resolved knee" would refuse every `--new-flow` run. The fixed
/// decode closes that from the other end: it reads its own [`DecodeParams`] rather
/// than the resolved `reconstruction`, so a recipe stating that section is refused
/// whole ([`reject_recipe_sections`]) and `--preset` is refused by presence.
/// Nothing the user *asks for* in the new flow's **reconstruction** is silently
/// dropped. (Knobs that merely resolve to a default still land in that section and
/// are read by nothing — a zero knee, `--balance-range`, `reconstruction.type` — which
/// is why the paragraph below says nothing *maps* them yet.)
/// The rest of the surface is closed too, by the same mechanism:
/// `nf-core/knob-availability-audit` put `print` and `output` in
/// [`UNREAD_RECIPE_SECTIONS`] beside `reconstruction` and gave every flag under them
/// a row below. `measure` is the section that is genuinely *read*
/// (`READ_RECIPE_SECTIONS`), so it needed no refusal at all.
/// The asymmetry that remains here is benign and deliberate: typing
/// `--sigmoid-toe 0.2`, which resolves
/// today's default, is refused while the same resolved config with no flag is simply
/// not consulted.
///
/// [`DecodeParams`]: crate::algo::fixed::DecodeParams
///
/// The ordering face of the same gap is why `simple` is listed here as well as in
/// [`VALUE_ENTRIES`]: a value rule cannot run until `merge` has resolved a value, so
/// any command line `merge` itself refuses is diagnosed by the legacy chain first. A
/// flag row pre-empts that.
///
/// **The inventory is complete**, and `every_convert_flag_is_classified` is what
/// keeps it so: every conversion flag is either refused by a row here, kept by a row
/// in `KEPT_FLAGS`, or named a non-knob, and every recipe section is in
/// [`UNREAD_RECIPE_SECTIONS`] or read in full. The reconstruction rows are
/// `nf-reconstruction/fixed-decode`'s, landed with the decode that strands them; the
/// print, output and kept rows are `nf-core/knob-availability-audit`'s. Adding a row
/// changes no message, ordering or call site.
///
/// The one thing the inventory does **not** carry is a renamed-knob mapping table.
/// That is deliberate: a `NotYet` names the *task* that will carry the knob, not a
/// flag spelling, because the spelling belongs to the task that builds the stage
/// (`nf-core/recipe-schema` owns the sections). Inventing one here would be a second
/// source of truth for it, and advice the user cannot act on today.
const FLAG_ENTRIES: &[FlagEntry] = &[
    // Paired with the `simple` row in [`VALUE_ENTRIES`], which is the same dual-rule
    // shape CLAUDE.md records for `OutputPreset::is_atomic` — a value rule for either
    // provenance, plus a presence rule for what the value rule cannot see in time.
    // Here that is *ordering*: `merge` refuses `--reconstruction simple` beside a
    // `--preset` or a `--density-curve` before any value rule runs, so without this
    // row the new flow's own refusal arrives second, behind advice about a chain the
    // user did not select. The flag has no identity value to protect — its other
    // value is `density`, which this flow wants.
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
    // `algo::fixed::DecodeParams` carries. Nothing **maps** them onto it yet —
    // `nf-core/minimal-end-to-end` owns the wiring, and until it lands the seam
    // refuses before any of them could be ignored. One wrinkle for that task:
    // `--density-gamma` beside the resolved *sigmoid* default is refused by `merge`
    // (exit 2) before this flow ever sees it, and one of merge's two remedies is
    // `--sigmoid-contrast` — which the row below refuses. (`merge` used to rank that
    // one first; `nf-core/knob-availability-audit` dropped the ranking, so the two
    // now read as equals and only one of them is dead here.) That row's remedy still
    // names `--density-curve exponential --density-gamma` together rather than the
    // flag alone, so following it works in one step. So the fixed decode's own contrast
    // currently needs the curve named beside it; that dissolves when the new flow's
    // default curve moves (`nf-core/minimal-end-to-end` / `nf-core/default-flip`).
    FlagEntry {
        knob: "--density-curve",
        covers: &["--density-curve"],
        // `--density-curve exponential` names the curve this flow already decodes
        // with, so it forces nothing and stays accepted — the tiebreaker's identity
        // value. Not to preserve a reset: a recipe pinning a sigmoid is refused whole
        // by `reject_recipe_sections`, so there is none to preserve. The other two
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
            // The curve is named alongside the flag on purpose: bare `--density-gamma`
            // beside the resolved sigmoid default is refused by `merge`, one of whose
            // two remedies is `--sigmoid-contrast` — the flag being refused here. See
            // the note above the surviving knobs.
            instead: Some(
                "`--density-curve exponential --density-gamma`, this decode's own contrast",
            ),
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
    // why the recipe half needs no value rules either, only
    // [`UNREAD_RECIPE_SECTIONS`].
    //
    // **Refused by presence, at every value, and the section refusal is why.** The
    // tiebreaker would normally leave an identity value alone to keep the flags-win
    // reset usable — `--white-balance 1,1,1` and `--highlight-compress 0` resolve the
    // documented defaults and render byte-identically. But that exemption exists to
    // let a flag clear a value a *recipe* pinned, and `reject_recipe_sections` refuses
    // any recipe stating `print`, so on this flow there is never a print value to
    // reset. With nothing to protect, presence is the honest rule: each of these names
    // an operation whose stage is an identity pass.
    //
    // Every verdict here is `NotYet`, including the two display tones
    // `nf-retire/display-tones` removes outright. That is not a softer reading of
    // their fate: `Never` is a claim about the *knob*, and what a user needs to know
    // at this gate is that the stage which would carry any tone is empty. Whether
    // `shoulder` survives into it is that retirement's statement to make, not this
    // gate's, and stating it here would be a second place to keep it in step.
    FlagEntry {
        knob: "--print-exposure",
        covers: &["--print-exposure"],
        present: |args| args.print.print_exposure.is_some(),
        availability: Availability::NotYet {
            arriving_with: SCENE_CORRECTION_ARRIVES_WITH,
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
        knob: "--white-balance",
        covers: &["--white-balance"],
        present: |args| args.print.white_balance.is_some(),
        availability: Availability::NotYet {
            arriving_with: SCENE_CORRECTION_ARRIVES_WITH,
        },
    },
    FlagEntry {
        knob: "--auto-wb",
        covers: &["--auto-wb"],
        present: |args| args.print.auto_wb.is_some(),
        availability: Availability::NotYet {
            arriving_with: SCENE_CORRECTION_ARRIVES_WITH,
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
    // The new flow resolves **no destination at all** — that is `nf-destinations`',
    // and `nf-core/minimal-end-to-end` wires the first one. So these are refused for a
    // different reason than the print family: not "the stage that carries it is
    // empty", but "there is nothing yet for a destination policy to apply to".
    //
    // Note what this does *not* claim. The memory preflight still reads
    // `output.preset` to pick a `RunProfile`, so "nothing reads it" would be false
    // today; what is true is that the profile it picks models a legacy render the new
    // flow will not perform, which `nf-core/minimal-end-to-end` owes.
    FlagEntry {
        knob: "--output-preset",
        covers: &["--output-preset"],
        present: |args| args.output_opts.output_preset.is_some(),
        availability: Availability::NotYet {
            arriving_with: DESTINATION_ARRIVES_WITH,
        },
    },
    FlagEntry {
        knob: "--out-depth",
        covers: &["--out-depth"],
        present: |args| args.output_opts.out_depth.is_some(),
        availability: Availability::NotYet {
            arriving_with: DESTINATION_ARRIVES_WITH,
        },
    },
    FlagEntry {
        knob: "--output-profile",
        covers: &["--output-profile"],
        present: |args| args.output_opts.output_profile.is_some(),
        availability: Availability::NotYet {
            arriving_with: DESTINATION_ARRIVES_WITH,
        },
    },
    FlagEntry {
        knob: "--bigtiff",
        covers: &["--bigtiff"],
        present: |args| args.output_opts.bigtiff.is_some(),
        availability: Availability::NotYet {
            arriving_with: DESTINATION_ARRIVES_WITH,
        },
    },
    // Operational, and refused for a reason none of the others share: it writes the
    // resolved *legacy* config, and it writes it before the render seam — so under
    // `--new-flow` it would hand the user a recipe describing a chain the run did not
    // select. Refusing beats emitting a plausible lie.
    FlagEntry {
        knob: "--dump-params",
        covers: &["--dump-params"],
        present: |args| args.dump_params.is_some(),
        availability: Availability::NotYet {
            arriving_with: "the new chain's recipe schema, which decides how a recipe \
                            describes these stages at all (`nf-core/recipe-schema`)",
        },
    },
];

/// Knobs with no new-flow meaning, keyed on the **resolved value**.
///
/// Provisional for the same reason as [`FLAG_ENTRIES`], and settled by the same
/// document (`simple` — "Remove").
const VALUE_ENTRIES: &[ValueEntry] = &[
    ValueEntry {
        knob: "`simple` reconstruction (`--reconstruction simple`, recipe \
               `reconstruction.type`)",
        // The flag is owned by the `FLAG_ENTRIES` row it pairs with.
        covers: &[],
        matches: |cfg| matches!(cfg.reconstruction, Reconstruction::Simple),
        availability: Availability::Never {
            reason: "`1 - T/T_base` is an affine inversion of transmission, not a decode of \
                     anything a print sees, so the fixed decode has nothing to map it onto",
            instead: Some("`--reconstruction density`"),
        },
    },
    // **A value rule, not a flag one, and it is the only knob in a *read* section that
    // needs a rule at all.** `input` is read by the new flow, so
    // `reject_recipe_sections` does not cover `input.export_ir` — and the export is
    // not, as this row first claimed, written before the render: `cli.rs` stages it
    // *after* `stages::render`, past the seam, and takes its bit depth from
    // `cfg.output.depth()` — from `output.preset`, a section this flow refuses. So
    // `--export-ir --new-flow` is accepted today and writes nothing, which is exactly
    // the accepted-and-ignored state this inventory exists to eliminate. Verified: it
    // exits 4 with no IR file, while the same line without the flag writes one.
    //
    // Keyed on the resolved value so it covers both spellings in one row: there is no
    // `merge` refusal to pre-empt, so the flag half would buy nothing.
    ValueEntry {
        knob: "--export-ir (recipe `input.export_ir`)",
        covers: &["--export-ir"],
        matches: |cfg| cfg.input.export_ir.is_some(),
        availability: Availability::NotYet {
            arriving_with: "a render for the export to be staged beside: it is written after \
                            the render, at the bit depth the destination resolves, so the new \
                            flow has nowhere to put it until a destination exists \
                            (`nf-core/minimal-end-to-end`)",
        },
    },
    // The other half-read section, and the row is what keeps `calibration` in
    // `READ_RECIPE_SECTIONS` honest: the base is read, the reference is not.
    //
    // **A value row rather than a section refusal, and rather than flag rows alone.**
    // The four `--*d-max` flags above already cover what the user *types*; this covers
    // what a *recipe* states, which before `core/calibration-recipe-section` was inside
    // `reconstruction` and so already refused whole. Moving the key to its own section
    // put it beyond that witness — the flag would have stayed refused while the recipe
    // key saying the same thing was parsed and read by nothing.
    //
    // Matching a **non-default** resolved value is what makes it a value rule rather
    // than a second presence rule: `"fixed"` is what an unstated reference resolves to,
    // so refusing it would refuse every `--new-flow` run that states a base.
    ValueEntry {
        knob: "recipe `calibration.dmax`",
        // The flag rows above own the flag spellings; this row exists for the recipe
        // provenance they cannot see.
        covers: &[],
        matches: |cfg| cfg.calibration.dmax != DmaxSource::default(),
        availability: Availability::Never {
            reason: DMAX_REASON,
            instead: None,
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
        covers: &["--measure-inset"],
        why: "it bounds the measurement region the film-base search runs in, which the \
              new flow shares — the seam is taken after it",
    },
    // The decode's own knobs — the calibration and the anchor that
    // `algo::fixed::DecodeParams` carries. `nf-core/minimal-end-to-end` owes the
    // wiring that maps them onto it; until then the seam refuses before any of them
    // could be silently ignored.
    KeptEntry {
        covers: &["--density-scale", "--density-offset"],
        why: "the decode's own calibration — `nf-calibration/scale-gamma-loop` owns the \
              values, this gate only keeps them reachable",
    },
    KeptEntry {
        covers: &["--density-gamma"],
        why: "the fixed decode's contrast. `nf-look/path-to-white` tunes against it \
              directly, so it must stay reachable — though not yet *bare*: `merge` \
              refuses it beside the resolved sigmoid default, so it needs \
              `--density-curve exponential` alongside until that default moves",
    },
    KeptEntry {
        covers: &["--anchor-mid-offset"],
        why: "it is the one anchor rule the decode has — `mid-at-base-offset`'s `d`, \
              whose value is `nf-reconstruction/anchor-rule`'s",
    },
    // Accepted because they force nothing on their own, not because the idea
    // survives: both ramp a regional balance that is itself refused above, so with
    // the balances at zero they configure a correction that does not happen. Not to
    // preserve a reset — `balance_range` serializes under `reconstruction.density`,
    // so a recipe stating it is refused whole — but because there is no diagnosis to
    // give: the row that has one fires as soon as a balance is non-zero.
    KeptEntry {
        covers: &["--balance-range", "--auto-balance-range"],
        why: "they only anchor the regional balance's tone ramp, which is refused \
              whenever it is non-zero, so alone they ask for nothing",
    },
];

/// The recipe sections the new flow does not read, refused whole.
///
/// The decode settled the argument ([`reject_recipe_sections`]): a stage that owns
/// its parameters reads no resolved section, so a recipe stating one would parse,
/// validate and then do nothing. `print` and `output` join `reconstruction` for the
/// same reason — `pipeline::chain`'s four stages each carry their own `Params`, and
/// the new flow resolves no destination at all.
///
/// This is also why no `print.*` or `output.*` knob needs a value rule: between this
/// list and the flag rows above, both provenances are covered.
pub const UNREAD_RECIPE_SECTIONS: &[&str] = &["reconstruction", "print", "output"];

/// The recipe sections the new flow **does** read.
///
/// `measure` is read in full. Two are read in part, and both take the same shape:
/// the section stays here and the unread key is refused by a [`VALUE_ENTRIES`] row,
/// because widening this list would reject the rest of the section with it.
/// `input` is read apart from `export_ir`, whose export is staged after the render
/// and so cannot be honoured yet. `calibration` is read apart from `dmax`: the fixed
/// decode divides by `calibration.film_base` like any other, but its anchor rule
/// reads no reference density at all, so refusing the section whole would reject the
/// base with it.
///
/// The complement of [`UNREAD_RECIPE_SECTIONS`], stated rather than inferred so that
/// `every_recipe_section_is_classified` can fail on a section added to the schema
/// and classified nowhere.
#[cfg(test)]
const READ_RECIPE_SECTIONS: &[&str] = &["input", "calibration", "measure"];

/// Refuse a **flag** the new flow has no meaning for.
///
/// Runs **before `merge`**, and that placement is load-bearing: a presence rule
/// placed after it is unreachable whenever `merge` refuses the same command line
/// first, and the user then gets a remedy pointing at a knob this flow rejects —
/// the circular-advice defect CLAUDE.md records shipping four times.
pub fn reject_unavailable_flags(flow: Flow, args: &ConvertArgs) -> Result<()> {
    if flow == Flow::Legacy {
        return Ok(());
    }
    for entry in FLAG_ENTRIES {
        if (entry.present)(args) {
            return Err(refusal(entry.knob, entry.availability));
        }
    }
    Ok(())
}

/// Refuse a **resolved value** the new flow has no meaning for.
///
/// Runs ahead of every rule that reasons about the shipped chain's parameters, for
/// the same reason as above. (`roll`'s own mode rejections still precede it; they
/// diagnose roll-versus-convert facts, not chain parameters, so their remedies
/// cannot name a knob this flow refuses.) This is the only half `roll` can reach:
/// it accepts no conversion flags, so a roll knob — shared recipe or per-frame
/// overlay — is always a resolved value.
pub fn reject_unavailable_values(flow: Flow, cfg: &ResolvedConfig) -> Result<()> {
    if flow == Flow::Legacy {
        return Ok(());
    }
    for entry in VALUE_ENTRIES {
        if (entry.matches)(cfg) {
            return Err(refusal(entry.knob, entry.availability));
        }
    }
    Ok(())
}

/// Refuse a **recipe** that describes the chain the new flow does not run.
///
/// The third provenance, and the one neither table can see. Every section in
/// [`UNREAD_RECIPE_SECTIONS`] would parse, validate and then do nothing — the new
/// flow decodes through [`DecodeParams`](crate::algo::fixed::DecodeParams), renders
/// through `pipeline::chain`'s own per-stage params, and resolves no destination at
/// all. `deny_unknown_fields` catches an *unknown* key and is blind to a **known but
/// meaningless** one, which is the bug class the project forbids.
///
/// Refusing the sections whole is blunt and temporary: `nf-core/recipe-schema`
/// decides how a recipe describes these stages, and until it does the knobs the new
/// flow *does* read stay reachable by flag (see `KEPT_FLAGS`).
///
/// `stated` comes from a raw-JSON witness, not from a comparison against the
/// defaults: a recipe that *writes* the defaults it would otherwise inherit is
/// indistinguishable from one that omitted them once serde has filled the gaps — the
/// same reason `calibration_dmax_present` exists.
///
/// One section per call is deliberate — the first stated one is named rather than
/// all of them, because a user fixing a recipe removes them one at a time and a list
/// reads as though all of them had to go before anything else could be checked.
pub fn reject_recipe_sections(flow: Flow, stated: &[&'static str]) -> Result<()> {
    if flow == Flow::Legacy {
        return Ok(());
    }
    if let Some(section) = stated.first() {
        return Err(refusal(
            &format!("a recipe `{section}` section"),
            Availability::NotYet {
                arriving_with: "the new chain's recipe schema, which decides how a recipe \
                                describes these stages at all — until then the new flow \
                                would parse this section and never read it \
                                (`nf-core/recipe-schema`)",
            },
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

/// The migration seam: `--new-flow` selected a chain that cannot yet render.
///
/// Exit 4 (`Unsupported`), not a usage error: the command line is well-formed and
/// the config resolved: it is *this build* that cannot serve it, which is the same
/// distinction `Resource` draws for the memory gate.
///
/// The chain itself exists as of `nf-core/stage-skeleton` (`pipeline::chain`, every
/// stage an identity pass), and so does the fixed decode that feeds it
/// (`algo::fixed`, `nf-reconstruction/fixed-decode`). What is missing is a
/// destination to write to, and the wiring between the two
/// (`nf-core/minimal-end-to-end`), which is the task that opens this seam. It stays
/// shut until then rather than composing an identity chain and refusing at the
/// encode: on a real 5000 dpi scan that is a full render thrown away to reach the
/// same message.
pub fn render_not_implemented() -> NcError {
    // The tail says only what this rule inspected — the same discipline as
    // `refusal`. "Drop `--new-flow` to convert through the current chain" asserted
    // that the legacy path *accepts* this command line, from a rule that never
    // looked at it, and it is false whenever the same line is independently invalid
    // there. (The example that motivated the rewrite was `--display-tone none
    // --print-exposure 3`, refused by `pipeline::sdr`'s range check on the legacy
    // path; both flags are now refused by presence before the seam, so reaching this
    // message needs a line the availability rows accept — `--density-scale 1,0.9,0.8`
    // beside a legacy-invalid film base, say. The discipline is unchanged.)
    NcError::Unsupported(
        "--new-flow selected the new rendering chain, which cannot render yet — its \
         stages exist but nothing connects them to an output \
         (`nf-core/minimal-end-to-end`). Every other part of the run resolved under \
         `--new-flow`'s own rules: re-run without it to take the current chain, \
         which checks these settings itself."
            .into(),
    )
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
        // Operational: arg-struct only, never a recipe key. (`--dump-params` is the
        // exception that proves the rule — it is operational *and* refused, because it
        // would write a recipe describing a chain the run did not select, so it earns a
        // row in FLAG_ENTRIES instead of a line here.)
        "--strict",
        "--seed",
        "--telemetry",
        "--telemetry-file",
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
            .chain(VALUE_ENTRIES.iter().flat_map(|e| e.covers))
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
        // The dual-rule `simple` pair is not an exception: the flag row owns
        // `--reconstruction` and the value row carries no ids, precisely so this stays
        // a flat uniqueness check.
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

    #[test]
    fn every_recipe_section_is_classified() {
        // The flag half above has a per-knob verdict; the recipe half is decided a
        // section at a time, because a stage that owns its parameters reads none of
        // its old section. So the check is that every section a recipe can carry is
        // named in one of the two lists — a new one is unread by default and would
        // otherwise be accepted-and-ignored.
        let recipe = serde_json::to_value(ResolvedConfig::default()).expect("a recipe serializes");
        let sections: Vec<&str> = recipe
            .as_object()
            .expect("the recipe is an object")
            .keys()
            .map(String::as_str)
            .collect();
        assert!(!sections.is_empty());
        for section in &sections {
            assert!(
                UNREAD_RECIPE_SECTIONS.contains(section) || READ_RECIPE_SECTIONS.contains(section),
                "recipe section `{section}` is classified neither read nor unread"
            );
        }
        for named in UNREAD_RECIPE_SECTIONS.iter().chain(READ_RECIPE_SECTIONS) {
            assert!(
                sections.contains(named),
                "`{named}` is classified but is not a recipe section"
            );
        }
    }

    #[test]
    fn an_unread_section_is_refused_and_a_read_one_is_not() {
        // Falsifiability for the section lists: the refusal must key on which section
        // was stated, not merely on the flow.
        let err = reject_recipe_sections(Flow::New, &["print"]).unwrap_err();
        assert!(err.message().contains("a recipe `print` section"), "{err}");
        assert!(reject_recipe_sections(Flow::New, &[]).is_ok());
        assert!(reject_recipe_sections(Flow::Legacy, &["print"]).is_ok());
    }

    #[test]
    fn legacy_refuses_nothing() {
        // The flag's whole contract on the no-flag path: nothing moves. A table row
        // added by the audit must not start refusing commands that work today.
        let cfg = ResolvedConfig {
            reconstruction: Reconstruction::Simple,
            ..Default::default()
        };
        assert!(reject_unavailable_values(Flow::Legacy, &cfg).is_ok());
    }

    #[test]
    fn a_value_entry_is_refused_under_the_new_flow() {
        let cfg = ResolvedConfig {
            reconstruction: Reconstruction::Simple,
            ..Default::default()
        };
        let err = reject_unavailable_values(Flow::New, &cfg).unwrap_err();
        let msg = err.message();
        assert!(msg.contains("--reconstruction simple"), "{msg}");
        assert!(msg.contains("will not gain one"), "{msg}");
        assert!(msg.contains("--reconstruction density"), "{msg}");
    }

    #[test]
    fn the_default_reconstruction_passes() {
        // Falsifiability for the test above: the refusal must key on the value, not
        // on the flow alone.
        assert!(reject_unavailable_values(Flow::New, &ResolvedConfig::default()).is_ok());
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
        // *Within* a table, two rows matching one knob would make the diagnosis depend
        // on row order. **Across** the two tables is the deliberate dual-rule pattern
        // (`simple` is both), so the check is per table, not over the union.
        for (label, knobs) in [
            (
                "FLAG_ENTRIES",
                FLAG_ENTRIES.iter().map(|e| e.knob).collect::<Vec<_>>(),
            ),
            (
                "VALUE_ENTRIES",
                VALUE_ENTRIES.iter().map(|e| e.knob).collect::<Vec<_>>(),
            ),
        ] {
            for knob in &knobs {
                assert!(!knob.is_empty(), "{label} has an unnamed row");
            }
            let mut sorted = knobs.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(sorted.len(), knobs.len(), "duplicate knob in {label}");
        }
    }

    #[test]
    fn the_dual_rule_pair_agrees_on_its_verdict() {
        // `simple` is refused by both tables, so the two rows must tell the user the
        // same thing — a pair that disagreed would diagnose one knob two ways
        // depending on whether it arrived by flag or by recipe.
        let flag = FLAG_ENTRIES
            .iter()
            .find(|e| e.knob.contains("--reconstruction simple"))
            .expect("the flag row for simple");
        let value = VALUE_ENTRIES
            .iter()
            .find(|e| e.knob.contains("`simple` reconstruction"))
            .expect("the value row for simple");
        assert_eq!(flag.availability, value.availability);
    }
}

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
//! `Flow` is *orchestration state*, like an unresolved `film_base.source`: it
//! never reaches a stage, and it is never a recipe key (`--new-flow` selects which
//! knobs exist; it does not set one). Unlike `--report` / `--telemetry` /
//! `--max-memory`, though, it is **not** in the "can never change a pixel" class —
//! choosing a chain is exactly a choice of pixels, which is why it must stay out
//! of the recipe rather than merely out of the image.

use crate::cli::{ConvertArgs, ResolvedConfig};
use crate::types::{NcError, Reconstruction, Result};

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
    present: fn(&ConvertArgs) -> bool,
    availability: Availability,
}

/// A knob refused by **resolved value**, checked before every other validation
/// rule. Names both spellings, because a resolved value can come from a recipe
/// nobody typed a flag for.
struct ValueEntry {
    knob: &'static str,
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
/// it is accepted because it asks for nothing this flow cannot do, which is also what
/// keeps the flags-win reset usable — clearing a recipe's knee is how one recipe gets
/// re-used on the new chain.
///
/// **How a knob the user never typed is handled**, which this table alone cannot do.
/// It sees only the flag, so the same knee stated by a *recipe* or expanded from
/// `--preset sigmoid-knees` is invisible here — and a value rule cannot be added
/// beside it, because the shipped default sigmoid *has* knees (`toe: 0.2`), so
/// "refuse a non-zero resolved knee" would refuse every `--new-flow` run. The fixed
/// decode closes that from the other end: it reads its own [`DecodeParams`] rather
/// than the resolved `reconstruction`, so a recipe stating that section is refused
/// whole ([`reject_recipe_reconstruction`]) and `--preset` is refused by presence.
/// Nothing the user *asks for* in the new flow's **reconstruction** is silently
/// dropped. (Knobs that merely resolve to a default still land in that section and
/// are read by nothing — a zero knee, `--balance-range`, `reconstruction.type` — which
/// is why the paragraph below says nothing *maps* them yet.)
/// The rest of the surface is not closed — `print.*`, `output.*` and `measure.*` are
/// still accepted under `--new-flow` and read by nothing, which stays
/// `nf-core/knob-availability-audit`'s (see below). The asymmetry that remains here is benign and deliberate: typing
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
/// **The reconstruction half of the inventory is complete; the rest is not.** These
/// rows are `nf-reconstruction/fixed-decode`'s, landed with the decode that strands
/// them rather than left accepted-and-ignored until the audit runs.
/// `nf-core/knob-availability-audit` still owns `print.*`, `output.*`, `measure.*`
/// and the renamed-knob mapping table. Adding a row changes no message, ordering or
/// call site.
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
    // in the meantime. The audit still owns the rest (`print.*`, `output.*`,
    // `measure.*`) and the renamed-knob mapping table.
    //
    // The decode's *surviving* knobs are deliberately absent from this table and stay
    // reachable: `--density-scale`, `--density-offset`, `--density-gamma` and
    // `--anchor-mid-offset` are exactly the calibration and the anchor
    // `algo::fixed::DecodeParams` carries. Nothing **maps** them onto it yet —
    // `nf-core/minimal-end-to-end` owns the wiring, and until it lands the seam
    // refuses before any of them could be ignored. One wrinkle for that task:
    // `--density-gamma` beside the resolved *sigmoid* default is refused by `merge`
    // (exit 2) before this flow ever sees it, and merge's **first** remedy is
    // `--sigmoid-contrast` — which the row below refuses. The two messages would
    // therefore close a loop, which is why that row's remedy names
    // `--density-curve exponential --density-gamma` together rather than the flag
    // alone: following it works in one step. So the fixed decode's own contrast
    // currently needs the curve named beside it; that dissolves when the new flow's
    // default curve moves (`nf-core/minimal-end-to-end` / `nf-core/default-flip`).
    FlagEntry {
        knob: "--density-curve",
        // `--density-curve exponential` names the curve this flow already decodes
        // with, so it forces nothing and stays accepted — the tiebreaker's identity
        // value, and what keeps the flags-win reset usable on a recipe that pinned a
        // sigmoid. The other two select a curve the fixed decode does not have.
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
        present: |args| args.sigmoid.sigmoid_contrast.is_some(),
        availability: Availability::Never {
            reason: "it is the sigmoid's slope, and the fixed decode is the straight line",
            // The curve is named alongside the flag on purpose: bare `--density-gamma`
            // beside the resolved sigmoid default is refused by `merge`, whose first
            // remedy is `--sigmoid-contrast` — the flag being refused here. See the
            // note above the surviving knobs.
            instead: Some(
                "`--density-curve exponential --density-gamma`, this decode's own contrast",
            ),
        },
    },
    // The three placements the one anchor rule replaces. `--anchor-mid-offset` is
    // absent from this table on purpose: it *is* the rule's `d`.
    FlagEntry {
        knob: "--anchor-white-at-reference",
        present: |args| args.anchor.anchor_white_at_reference,
        availability: Availability::Never {
            reason: ANCHOR_REFERENCE_REASON,
            instead: Some("`--anchor-mid-offset`, the one rule the decode has"),
        },
    },
    FlagEntry {
        knob: "--anchor-mid-fraction",
        present: |args| args.anchor.anchor_mid_fraction.is_some(),
        availability: Availability::Never {
            reason: ANCHOR_REFERENCE_REASON,
            instead: Some("`--anchor-mid-offset`, the one rule the decode has"),
        },
    },
    FlagEntry {
        knob: "--anchor-black-floor",
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
        present: |args| args.dmax.d_max.is_some(),
        availability: Availability::Never {
            reason: DMAX_REASON,
            instead: None,
        },
    },
    FlagEntry {
        knob: "--fixed-d-max",
        present: |args| args.dmax.fixed_d_max,
        availability: Availability::Never {
            reason: DMAX_REASON,
            instead: None,
        },
    },
    FlagEntry {
        knob: "--auto-d-max",
        present: |args| args.dmax.auto_d_max,
        availability: Availability::Never {
            reason: DMAX_REASON,
            instead: None,
        },
    },
    FlagEntry {
        knob: "--no-d-max",
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
        present: |args| args.density.shadow_balance.is_some_and(|b| b != [0.0; 3]),
        availability: Availability::NotYet {
            arriving_with: BALANCE_ARRIVES_WITH,
        },
    },
    FlagEntry {
        knob: "--highlight-balance",
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
        present: |args| args.preset.is_some(),
        availability: Availability::NotYet {
            arriving_with: "the look stage's presets, which is where a bundle spanning \
                            decode and rendering can be defined again (`nf-look/look-presets`)",
        },
    },
    // Operational, and refused for a reason none of the others share: it writes the
    // resolved *legacy* config, and it writes it before the render seam — so under
    // `--new-flow` it would hand the user a recipe describing a chain the run did not
    // select. Refusing beats emitting a plausible lie.
    FlagEntry {
        knob: "--dump-params",
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
const VALUE_ENTRIES: &[ValueEntry] = &[ValueEntry {
    knob: "`simple` reconstruction (`--reconstruction simple`, recipe \
           `reconstruction.type`)",
    matches: |cfg| matches!(cfg.reconstruction, Reconstruction::Simple),
    availability: Availability::Never {
        reason: "`1 - T/T_base` is an affine inversion of transmission, not a decode of \
                 anything a print sees, so the fixed decode has nothing to map it onto",
        instead: Some("`--reconstruction density`"),
    },
}];

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
/// The third provenance, and the one neither table can see. The new flow decodes
/// through [`DecodeParams`](crate::algo::fixed::DecodeParams), not through the
/// resolved `reconstruction` object, so a recipe stating that section would parse,
/// validate and then do nothing — `deny_unknown_fields` catches an *unknown* key and
/// is blind to a **known but meaningless** one, which is the bug class the project
/// forbids. Refusing the section whole is blunt and temporary: `nf-core/recipe-schema`
/// decides how a recipe describes these stages, and until it does the decode's own
/// knobs stay reachable by flag (`--density-scale`, `--density-offset`,
/// `--density-gamma`, `--anchor-mid-offset`).
///
/// `stated` is a raw-JSON witness, not a comparison against the default: a recipe
/// that *writes* the defaults it would otherwise inherit is indistinguishable from
/// one that omitted them once serde has filled the gaps — the same reason
/// `curve_dmax_present` exists.
pub fn reject_recipe_reconstruction(flow: Flow, stated: bool) -> Result<()> {
    if flow == Flow::New && stated {
        return Err(refusal(
            "a recipe `reconstruction` section",
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
    // there (`--display-tone none --print-exposure 3`, which the SDR range check
    // refuses).
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

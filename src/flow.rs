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
//! - [`render_not_implemented`] — the seam itself, until `nf-core/stage-skeleton`
//!   puts stages behind it.
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
/// **Known hole, and it is the audit's to close.** This half sees only the flag, so
/// the same knee stated by a *recipe* or expanded from `--preset sigmoid-knees` is
/// not refused here — and the asymmetry runs the other way too: typing
/// `--sigmoid-toe 0.2`, which resolves today's default, is refused while the
/// identical resolved config with no flag reaches the seam.
///
/// The same gap has an *ordering* face, which is why `simple` is listed here as
/// well as in [`VALUE_ENTRIES`]: a value rule cannot run until `merge` has resolved
/// a value, so any command line `merge` itself refuses is diagnosed by the legacy
/// chain first. A flag row pre-empts that; a knob reaching the new flow **only**
/// through a recipe cannot be pre-empted, since the recipe's value is not the
/// resolved one until the flags have won. A value rule cannot simply be added beside it: the shipped
/// default sigmoid *has* knees (`toe: 0.2`), so "refuse a non-zero resolved knee"
/// would refuse every `--new-flow` run before it reached the seam. Deciding what a
/// knob the user never typed earns is exactly
/// `nf-core/knob-availability-audit`'s open question, and it cannot land before
/// `nf-reconstruction/fixed-decode` moves the default.
///
/// **Provisional, and deliberately short.** The real inventory is that audit's
/// product; these exist so the mechanism is exercised through the binary rather
/// than only unit-tested, and their verdicts are ones `docs/design-update.md`
/// already settled ("sigmoid with toe/shoulder — leaves reconstruction"). Adding a
/// row changes no message, ordering or call site.
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
        present: |args| args.sigmoid.sigmoid_toe.is_some_and(|w| w != 0.0),
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
        present: |args| args.sigmoid.sigmoid_shoulder.is_some_and(|w| w != 0.0),
        availability: Availability::NotYet {
            arriving_with: "the fit-range stage: compressing highlights inside \
                            reconstruction is what discards the range an HDR rendition \
                            exists to carry (`nf-display-stages/fit-range`)",
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

/// The migration seam: `--new-flow` selected a chain whose stages do not exist yet.
///
/// Exit 4 (`Unsupported`), not a usage error: the command line is well-formed and
/// the config resolved: it is *this build* that cannot serve it, which is the same
/// distinction `Resource` draws for the memory gate. `nf-core/stage-skeleton`
/// replaces this call with the identity chain.
pub fn render_not_implemented() -> NcError {
    NcError::Unsupported(
        "--new-flow selected the new rendering chain, which has no stages yet \
         (`nf-core/stage-skeleton` fills them). Every other part of the run resolved: \
         drop `--new-flow` to convert through the current chain."
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

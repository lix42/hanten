# Audit the flag surface against a characteristic default

> **Superseded 2026-09-19 by `nf-core/knob-availability-audit`** — it audits
> flags the new flow retires; its method — classify every rule flag-vs-value
> before the default moves — is what carried over. Kept so existing references
> resolve; see `docs/nf-migration.md` for the migration plan.

## Goal

Find out what breaks when `characteristic-generic` becomes the no-flag default, and
fix it — **before** the default actually moves, so the migration is a version bump
rather than a version bump plus a pile of newly-wrong error messages.

Most of this is executable today by passing `--preset characteristic-generic`
explicitly. It does not wait on the calibration frames.

## Why — measured, 2026-09-13

Three validation rules key on the **resolved curve**, not on flag presence, so a
default move flips them for users who typed only the flag on the left:

| command | today | under a characteristic default |
|---|---|---|
| `nc convert --d-max 1.3` | exit 0 | exit 2 |
| `nc convert --auto-d-max` | exit 0 | exit 2 |
| `nc convert --sigmoid-toe 0.2` | exit 0 | exit 2 |

All three say some form of *"the resolved curve is characteristic … Drop the flag, or
pass `--density-curve sigmoid`"* — a remedy that names a curve the user never asked
about, for a default they never chose. `--d-max` is the documented roll-calibration
workflow (`nc estimate --d-max-region`, design-spec §8), so this is not a corner.

The mirror image also exists: `--film-stock portra-400` alone currently fails with
*"the resolved curve is sigmoid — a stock has nothing to configure there"*. After the
move that command succeeds. **The rule itself must stay** — it is still reached by any
explicit parametric selection (verified: `--density-curve sigmoid --film-stock
portra-400` and the `exponential` form both exit 2), and `src/cli.rs:3603` exists
precisely because "on the parametric curves the flag would be a silent no-op, which is
the failure mode the tagged schema exists to prevent". What changes is which *path*
reaches it and whether its remedy still reads correctly once characteristic is the
default. Do not delete it; re-check its reachability and its wording.

## Design

**The organising distinction is presence versus resolved value.** CLAUDE.md already
records it for output-preset atomicity ("rejects a non-default *resolved value* …
but rejects the `--out-depth` **flag** by presence"), and a default move is exactly
where the two diverge:

- a rule keyed **only on a flag being present** does not change when a default moves;
- a rule keyed **only on a resolved value** changes meaning for every user who never
  typed anything;
- a rule keyed on **both** — the dangerous class, and the one this task's own headline
  examples fall into.

`reject_dmax_flag_with_characteristic_curve` (`src/cli.rs:3765`) is the worked example:
its rustdoc says it "lives in `validate_convert` (flags) rather than `validate` (values)",
which reads as presence-keyed, yet its body also requires
`matches!(curve, DensityCurve::Characteristic(_))`. It is a conjunction, and it is rows 1
and 2 of the table above. Classifying it by where it *lives* gives "unaffected" — the
opposite of the measurement. **Classify by every input the condition reads**, not by which
gate the rule sits in.

So the audit is not "run every flag and see", and it is not a two-way sort either: for each
rule, list what its condition reads, and ask whether each of those can now arrive from a
default instead of from the user.

**Two halves, different timing:**

1. **Executable now** — explicit `--preset characteristic-generic` against the rest of
   the flag surface. Every refusal it produces is a refusal the default will produce
   too, minus the ones keyed on `--preset` presence.
2. **Predictable now, verifiable only after the move** — what happens when the user
   typed nothing. Derive it from the classification above; do not guess from the exit
   codes in half 1.

**The `film-master` escape needs confirming, not assuming.** `algo/conversion-presets`
records that a preset must not set `output.preset`, so `legacy` / `custom` /
`film-master` resolve their own bundle. Today `--preset characteristic-generic
--output-preset film-master` is refused (exit 2) while bare `--output-preset
film-master` is exit 0 — so the escape has to survive the default move, or a working
command starts failing. Check it; it is the single most likely regression.

## What to look at

Not exhaustive — the point is the shape, and implementation will find the rest:

- the anchor family (`--d-max` / `--auto-d-max` / `--fixed-d-max` / `--no-d-max`)
  against a curve that resolves **no** reference density and **no** anchor;
- the `--sigmoid-*` family, now configuring a non-resolved curve;
- `--film-stock`, whose no-flag path flips from refusal to success while the rule stays
  live for explicit parametric curves — a reachability change, not a deletion;
- `density.scale`'s default, resolved in **three** places (recipe `Deserialize` off raw
  JSON key presence, the `--density-curve` merge arm, the `roll` planner by hand) — a
  changed default curve reaches all three differently, and `default_scale_for` returns
  `[1,1,1]` for characteristic against `[1, 0.84, 0.73]` for the parametric curves;
- `--display-tone none`, which characteristic refuses at **render** time (exit 1) rather
  than by a validate rule — content-dependent, so it is a refusal the default inherits
  for ordinary pictures;
- `print_exposure` 1.91 and `display_tone` reinhard as *resolved defaults*, against
  `film-master`'s refusal of a non-default exposure and `legacy`/`custom`/`film-master`'s
  refusal of reinhard;
- **rule ordering**, which CLAUDE.md warns has shipped wrong four times: a rule that also
  matches branches it did not mean to will hand out a remedy those branches refuse.
  Changing the default changes which branch every rule lands on.

## Open questions

- **Does any rule's remedy become unfollowable?** A message saying "pass
  `--density-curve sigmoid`" is advice to leave the default — fine as an escape hatch,
  wrong as the primary diagnosis for someone who never left it.
- **Should the anchor family be accepted-and-ignored, refused, or refused only when
  explicit?** Refusing a flag the user typed is right; refusing because a default put
  them on a curve that has no anchor is not obviously right. This is the same
  presence-vs-value tiebreaker, and it needs an answer rather than an accident.
- **How much of this is testable without moving the default?** Half 1 is. Whether half 2
  can be exercised by a test that resolves config as if the default had moved — rather
  than by moving it — decides whether this task ships a regression net or only a report.

## How to Verify

- A written inventory of every rule whose condition reads the resolved curve, each
  classified by **all** its inputs (flag-only / value-only / both), with a decision
  recorded for every rule that changes behaviour.
- Every remedy string that would become wrong is fixed. A rule is deleted **only** if it
  is unreachable from *every* path, not merely from the no-flag one — `--film-stock`'s
  guard is the counter-example, and deleting it would restore a silent no-op.
- `--output-preset film-master`, `legacy` and `custom` still resolve at exit 0 with no
  other flags, and a test pins that.
- Tests drive the **real path** (`merge` / the binary), not the rule directly — a test
  calling a rule in isolation exercises it and never the ordering, which is how one of
  the four ordering defects passed CI.
- `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo build`,
  `cargo test` pass.

## Dependencies

- [Named conversion presets](conversion-presets.md) — ships the bundle this audits
  against. Its Goal line once claimed the default had moved; it had not, and that
  mismatch is what surfaced this task

# Named conversion presets (`--preset`)

## Goal

Give the reconstruction + display intent a **named** form, so a user selects a conversion
by name instead of assembling four to six flags whose values only make sense together.

**Five presets ship. The default did not move** — `--preset` is `Option<String>` with no
default, so a bare `nc convert` still resolves the knee'd sigmoid into `gain-map-hdr`.
Making `characteristic-generic` the no-flag state is `algo/split-default-migration`, which
owns the `pipeline_version` bump it requires. An earlier draft of this line read
"`characteristic-generic` becomes the default", which this task's own *How to verify*
contradicts; corrected 2026-09-12 after it misled a reader. That mismatch also surfaced
`algo/characteristic-default-audit` — the flag-surface work the default move needs, which
nothing owned.

## Why

Every good configuration found so far is a *bundle* of coupled parameters, and the coupling
is the problem. `--print-exposure` alone needs a different value per reconstruction (0.31 to
0.70, measured) because the reconstructions place mid-grey differently; the aim-matched red
scale needs a different value per stock; the per-channel gain already needed a different
value per curve. None of these numbers means anything on its own, and today the only place
they exist together is a review-set generator and this task's progress log.

A name also makes the brightness calibration honest. Instead of five tastes there is **one**
— scene mid-grey rendered 0.31 stop up, the 2026-09-09 review verdict — and each preset's
exposure is whatever lands it there.

## The five

| preset | reconstruction | display tone | brightness from |
|---|---|---|---|
| `characteristic-generic` | characteristic, `generic-c41` | reinhard | `print_exposure` |
| `characteristic-stock` | characteristic, `--film-stock` | reinhard | `print_exposure` |
| `characteristic-aim` | characteristic + derived red scale | reinhard | `print_exposure` |
| `sigmoid-knees` | sigmoid, toe/shoulder | **none** | the **anchor** |
| `sigmoid-flat` | sigmoid, no knees | reinhard | `print_exposure` |

Plus an implicit `master` preset that `legacy` / `custom` / `film-master` resolve, so those
keep working (see the constraint below).

## Known constraints

These are measured, and each one killed a simpler design:

- **`sigmoid-knees` cannot use `--print-exposure` at all.** `--display-tone none` is
  self-policing on the render's ceiling, and `print_exposure` is a scalar gain applied after
  the curve — so any positive value pushes the shoulder past reference white and the frame
  is refused (measured on a real scan: `+0.70` gave luminance 1.6236, exactly `2^0.70`).
  Its brightness must come from the anchor, which moves mid-grey *within* the bounded range.
  `mid-fraction 0.42` lands the shared target.
- **`legacy` / `custom` / `film-master` refuse `reinhard`, and `film-master` refuses any
  non-default `print_exposure`.** So the default preset cannot simply set them globally —
  a bare `nc convert --output-preset film-master` would stop working. A preset must
  therefore *not* set `output.preset`, and the non-display presets resolve their own.
- **`characteristic-aim` must refuse `portra-800` and `ultramax-800`** — no usable aim
  delta, already recorded in `algo::film_stock`.
- **`--display-tone none` is refused under the characteristic curve** (it overshoots by
  design, p99.99 = +3.64 stops), so those three presets cannot offer a linear render.

## Open questions

> **Answered while building (2026-09-10)** — the reasoning and the measurements are in
> `docs/progress/algo.md`. In short: the argument is the existing `--film-stock` flag;
> a preset is a **CLI-only expansion** with no recipe key, so the question of
> re-expansion does not arise; precedence is `defaults < params < preset < flags`, with
> the preset **above** the recipe because the proposed ordering would have been inert
> against any recipe nc writes; and the headroom default did not move. Whether the
> aim-matched scale generalises is still open and is why it is a named option rather
> than a candidate default.


- **How a preset's argument is supplied.** Reusing the existing flags
  (`--preset characteristic-stock --film-stock portra-400`) needs no new parser and keeps
  "every knob is a flag" true; a compact `name:arg` form would need quoting rules. Start
  with the flags and validate presence.
- **Where the expansion happens**, and what a resolved recipe records. It should carry the
  concrete expanded values (so it re-loads identically, as `density.scale` now does) plus
  the preset name as provenance — but whether the name is a recipe key that *re-expands* on
  load, or pure provenance, decides what happens when a stored recipe meets a build whose
  preset definition has moved.
- **Precedence.** Intended: preset → `--params` recipe → flags, i.e. a preset is a named
  set of *defaults*. Needs checking against the flags-win reset rules that already exist.
- **Whether the default headroom moves with the default.** Measured content reaches p99.99
  +3.64 stops, so `reinhard` at 4 stops covers it where the shipped default of 6 wastes two.
  Arguably `output/presets`.
- **Whether the aim-matched scale generalises.** It measured best on the corpus mean but was
  the least consistent per frame (0.020–0.333 against `characteristic-stock`'s 0.066–0.193),
  and the three Ektar frames disagree with each other.

## How to verify

- The preset-review generator already rendered all five from their *expanded* flags, so
  regenerating through `--preset` had to produce **byte-identical** files — the acceptance
  test for the expansion, which existed before the mechanism did. It passed on 2026-09-10 and
  the expanded flags were then deleted; the matrix that replaced them is
  `scripts/preset-review/presets.matrix.json`, rendered by `nctool review generate`.
- `pipeline::stages::midtone_placement::each_candidate_look_needs_its_own_print_exposure`
  prints the calibration table and fails if the spread ever collapses to where one shared
  default would do.
- Making `characteristic-generic` the default is a `pipeline_version` bump with a
  golden recapture and a measured report, as `algo/split-default-migration` describes.

## Dependencies

- [Film stock profiles](film-stock-profiles.md) — ships the characteristic curve, the
  registry and the aim tables the derived scale reads.

Depended on by (not a dependency; the edge runs the other way in `TASKS.md`):
[Make `characteristic-generic` what a bare `nc convert` resolves](split-default-migration.md),
which owns the default migration this task's last step performs. Read its release gate
first (since 2026-09-12 it is the known-neutral reference from
`analysis/calibration-frame-capture`).

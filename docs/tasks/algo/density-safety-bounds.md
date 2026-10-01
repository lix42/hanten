# Density Safety Bounds

> **Re-scoped 2026-10-01** for the new chain (`nf-core/default-flip`). The original
> file described the removed chain (`algo/density.rs`, `render_print`,
> `print_exposure`, the sigmoid bounds); that version is in git. The gap it named
> survives the flip, measured below.

> **Done 2026-10-01 — parts 1 and 2.** Part 1 is `recipe::validate_render`, a probe of
> the film base and the reachable scan range through the real decode and grade; part 2
> is the black-channel warning at the encode. Part 3 moved to
> [`algo/near-black-collapse-warning`](near-black-collapse-warning.md). The open
> questions are answered below.

## Goal

No validation-passing recipe may produce a degenerate image silently, and no user
value may reach an internal-invariant error. Three parts:

1. **No user value reaches an internal error**: each becomes a usage error (exit 2)
   naming the knob. *(doable without real scans)*
2. **A threshold-free collapse warning**: an output channel that quantizes entirely to
   `0` raises a report warning (`--strict` promotes). *(doable without real scans)*
3. **A tuned near-black collapse warning** with a real-scan false-positive guard.
   *(needs `../nc-assets`)*

## What the new chain already refuses

`recipe::validate` refuses an anchor whose exponent overflows f32
(`DecodeFault::Anchor`), an exposure whose gain `2^EV` is not a normal f32
(`exposure_fault`), and a whole slope (`linearization × slope`) that is not a normal
f32 (`validate_whole_slope`). Those are the model for part 1.

## The gap, measured 2026-10-01

On `tests/fixtures/hdr-48bit.tif --film-base 0.9,0.55,0.42`, with the roll stated
(`--roll-white 2.5 --roll-white-balance 1,1,1 --roll-exposure 0`) so no unrelated
warning is present:

| Extra | rc | Output | Warnings | `--strict` |
|---|---|---|---|---|
| *(none)* | 0 | normal | none | 0 |
| `--density-offset=-5,-5,-5` | 0 | 100 % zero samples | none | 0 |
| `--exposure=-100` | 0 | 100 % zero | none | 0 |
| `--roll-exposure=-100` | 0 | 100 % zero | none | 0 |
| `--white-balance=1e-30,1,1` | 0 | red channel 100 % zero | none | 0 |
| `--exposure=-20` | 0 | 31–59 % zero per channel, max code 3 | none | — |

The report's `output_stats.mean` reads `[0, 0, 0]` on the all-black rows, and every
`loss` counter is 0: a `0.0` is a legal in-range sample.

**Internal errors reachable from user values** (exit 1, `NcError::Other`):

| Extra | Error |
|---|---|
| `--density-offset=1e6,1e6,1e6`, `--density-offset=-1e6,…`, `--density-gamma=1000`, `--roll-white 0.001` | `fit range was handed a film base that graded to luminance inf/0; the decoded base is positive and finite by construction` (`pipeline/fit_range.rs`) |
| `--density-scale=1e6,1e6,1e6` | `fit range received a non-finite sample at pixel 0` |

## Design notes

- **Part 1.** Validate the resolved value, not a proxy: the film base graded through
  decode → scene correction → look must be a finite, positive, normal luminance, and
  the message names the knobs that can move it (the `validate_whole_slope` pattern).
  Not per-knob `is_normal()` refusals: individually legal factors can multiply to
  zero, and a per-knob rule rejects legitimate extreme pushes.
  Then decide whether generous magnitude bounds on `reconstruction.offset`, `scale`
  and `linearization` are still needed once that rule exists. A negative offset is
  legal.
- **Part 2.** A whole channel at exactly 0 cannot come from a real scene. One
  legitimate trigger: converting the unexposed frame itself, which display black maps
  to 0. Warning, not error. It catches the single-channel case
  (`--white-balance=1e-30,1,1`) a whole-image test would miss.
- **Part 3.** The tuned form (dynamic-range collapse, near-black fraction) catches the
  `--exposure=-20` row. Its threshold must keep every real frame, the darkest
  included, silent. It may split off as its own task when parts 1–2 land.

## Open questions

- Does part 1 make the magnitude bounds unnecessary, or are they still wanted for
  their messages? **Unnecessary**: the probe refuses every value that reaches an
  internal error, naming it, and a bound would refuse values that render.
- Does the warning apply to the film master, whose output is unclamped float?
  **Yes**: it reads the written samples of every encoder, and a film master whose
  decode underflows to 0 warns.

## How to Verify

- Each row of both tables above becomes either a usage error (exit 2) naming the
  knob, or a report warning that `--strict` promotes; no internal error remains
  reachable from a recipe value. Tests go through `merge` or the binary.
- A realistic recipe renders byte-identically (validation and warnings only).
- Part 3: every real `../nc-assets` frame converts at defaults with no collapse
  warning (`#[ignore]` probe printing derived numbers only).

## Dependencies

- [Density-domain algorithm](density.md) — historical owner of the parameters; the
  decode is now `algo::fixed`.
- [Pipeline orchestration](../core/pipeline-orchestration.md) — `validate`, the
  report and `push_warning`.

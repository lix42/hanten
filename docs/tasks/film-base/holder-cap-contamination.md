# The holder march cap: a premise, not a defect

## Goal

Settle what a holder-march **cap** means. The march stops each edge at
`HOLDER_MARCH_MAX_FRAC` (25% of the shorter edge). Because each edge is measured over
what the **perpendicular** edges' reported depths leave, a capped edge makes its
perpendicular edges cap too: a 120 px IR-dark top band on a 400x400 frame with 10 px
sides reports `{top: 100, bottom: 10, left: 100, right: 100}` at `converged: true`.
`holder-depth-mask` made that loud (per-edge `capped`, a `--strict`-promotable
warning). This task was filed to make the measurement right.

## Resolution (2026-10-01)

**Closed on a premise the user set: no film holder is deeper than 25% of the shorter
edge.** Real holders measure 2.5-4% (31 IR frames, 7 rolls, zero caps). So a cap is
an IR misread — IR-dark film or debris — and every error it causes is an **over-cut**:
the capped edge and the perpendicular edges it inflates cut film, never leave holder,
and the region shrinks but stays clean. If the premise ever fails, that is a special
case the user covers by raising the inset (`--measure-inset` / `measure.inset`),
which is added on top of the cap.

Done: the premise and remedy are stated on `HOLDER_MARCH_MAX_FRAC` and in the capped
warning; `CappedEdges::contaminated()` is removed (it only mattered while a cap could
be a real deep holder); `using-nc.md`, the design spec and the epic summary carry the
same reading. No march change, no pixel change, no report field change.

## Rejected

Recorded so none is re-proposed:

- **Raise `HOLDER_MARCH_MAX_FRAC`.** The empty-region predicate on the short axis is
  `2 * (cap_frac + inset) >= 1`; at 0.35 an ordinary `inset >= 0.15` refuses on
  `measure-base` and `measure-roll`. It also removes the only bound on an ambiguous IR
  read: a locally IR-dark edge would march deeper and report `capped: false`. Past 0.5
  the dead `.min(perpendicular / 2)` clamp becomes load-bearing.
- **Decline (`holder_depths` returns `None`) on a capped perpendicular pair.** `None`
  degrades to the inset-only cut, which leaves *more* holder in the region than the
  cap does — the wrong direction for the p97/p99 whites `measure-roll` reads.
- **Attribute the corner run.** Dropping the run of capped segments next to a capped
  perpendicular edge was simulated and fixes every fixture, but under the premise it
  only tidies the area loss on a frame whose IR was already misread.

## Not covered

A holder the march **misses** — reported as `0` while present, as on the Gold200 frame
in `nf-reconstruction`'s progress log — is a different failure (the unstated all-zero
premise on `EffectiveArea::holder`) and is not this task's.

## Dependencies

`film-base/holder-depth-mask` (done) — the march, the per-edge flags and the
warning are its work.

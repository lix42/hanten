# A beyond-cap holder makes the perpendicular edges' depths artifacts

## Goal

Stop the holder-depth march reporting a plausible rectangle whose numbers are
wrong, on a frame whose holder is deeper than `HOLDER_MARCH_MAX_FRAC` (25% of the
shorter edge). `holder-depth-mask` made the case *loud* — per-edge `capped`, a
`--strict`-promotable warning, and prose that says a capped edge inflates its
perpendicular neighbours. It did not make the measurement right, and that is this
task.

## The mechanism

The march is a fixed point: each edge is measured only over the along-edge
positions the **perpendicular** edges' cuts leave, and the trim is those edges'
reported depths. So the trim can only remove as much holder as the perpendicular
edge *reported*. Once a true depth exceeds the cap, the reported value is the cap,
the residual strip between cap and real holder edge stays inside the perpendicular
edges' extent, every segment there reads holder at every depth, and they cap too.

Measured (`a_beyond_cap_edge_inflates_the_perpendicular_edges_and_says_so`): a
400x400 frame with a 120 px top holder and 10 px sides reports
`{top: 100, bottom: 10, left: 100, right: 100}` — a tenfold over-cut on two edges
discarding 45% of the frame width. The committed 200x200 `[4, 4, 60, 4]` fixture is
the same shape and reported `{50, 50, 50, 4}` undetected until the per-edge flags
landed.

Two things narrow the surface usefully:

- **The cap is the only route.** A mid-edge notch cannot contaminate a
  perpendicular edge (left/right sample only `x in [0, left)` and `[w-right, w)`),
  and a deep-but-uncapped edge produces a trim that exactly covers its own corner.
- **The corrupted case is already distinguishable from the merely-floored one**:
  an edge capped *and* a perpendicular edge capped. That is exactly when a trim was
  truncated. `CappedEdges::contaminated` is the predicate and separates the sweep
  rows with no false positives or negatives; a single edge exactly at the cap
  (`[100, 10, 10, 10]`) is correct on all four and must not be declined.

## Candidate remedies

- **A decline rule**: `holder_depths` returns `None` when `contaminated()` holds.
  `None` means "not measured", degrades to the inset-only cut, and is a state every
  caller already handles — structurally the existing small-frame decline. Cheap, and
  it does not touch the at-cap case. Open: is losing the measurement entirely the
  right trade against reporting one known-bad edge pair, and does the warning then
  still tell the user enough?
- **Per-edge trim from a source other than the capped report** — something that
  bounds the perpendicular extent without depending on the depth that capped. Open
  which signal that is; a corroborating read of the outermost band along the whole
  edge is one idea, pass 1's untrimmed depths are explicitly *not* (they are
  corner-contaminated by design, so they would add a number always wrong in a known
  direction).

## Rejected: raise `HOLDER_MARCH_MAX_FRAC`

Recorded so it is not re-proposed. It trades one silent wrong answer for two:

- It widens the empty-region refusal into ordinary use. The predicate is
  `2 * (cap_frac + inset) >= 1`; at `cap_frac = 0.25` that needs `inset >= 0.25`
  (unreachable at the 5% default), at `0.35` only `inset >= 0.15` — a value
  `using-nc.md` advises on a `null`-holder scan.
- It removes the only bound on an ambiguous IR read. The cap is what stops the
  march on a frame whose *film* is locally IR-dark near an edge (dense content, a
  heavy sky, silver past its IR threshold). Today such an edge caps and says so;
  with a larger cap it marches deeper and reports `capped: false` — an over-cut with
  no flag at all.
- Past 0.5 the currently-dead `.min(perpendicular / 2)` clamp becomes live and
  load-bearing, changing the analysis of a second piece of code.

## Known vs unknown

- **Known:** no real frame has capped — 31 measured IR frames across 7 rolls, zero
  caps, depths 2.5-4% of the shorter edge, a 6-10x margin. So this is a robustness
  gap, not a reachable defect on scans resembling those.
- **Unknown:** whether it is reachable at all on a geometry nc targets. The
  denominator is the *scan's* shorter edge, not the film's, so a tightly cropped
  scan of a single small frame shrinks it while the rail stays put —
  `film-base/half-frame-calibration` is on the roadmap and a half-frame's short
  dimension is ~18 mm, where a 4.5 mm rail is 25%. A misaligned or partially
  inserted frame changes the geometry without changing the equipment.

## How to verify

- The existing beyond-cap fixtures report something honest rather than a confident
  wrong rectangle, and `[100, 10, 10, 10]` (correct on all four edges) still
  measures.
- Whatever replaces the current answer is falsifiable on a sub-cap frame, so the
  rule is not simply always firing.

## Dependencies

`film-base/holder-depth-mask` (done) — the march, the per-edge flags and the
warning are its work.

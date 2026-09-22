# Make `characteristic-generic` what a bare `hanten convert` resolves

> **Superseded 2026-09-19 by `nf-core/default-flip` and
> `nf-calibration/neutrality-gate`** — the default now flips to the new chain
> with the exponential, not to `characteristic-generic`; the flip half and the
> neutrality gate landed in different epics. Kept so existing references
> resolve; see `docs/nf-migration.md` for the migration plan.

## Goal

Make the reconstruction / render split the shipped default: reconstruction stops
shaping tone, the display operator carries the character.

Since `algo/conversion-presets` shipped, that is a narrow and concrete change —
**what a bare `hanten convert` with no flags resolves**. `--preset characteristic-generic`
already expands to the target rendition (`cli::ConversionPreset::expand`); this task
makes it the no-flag state, with the `pipeline_version` bump, drift-gate row,
measured report and guide update that a default move owes.

## What "the default" means now, and what is left

The default has already moved along several axes, and none of them is this one:

- **per curve** — `DensityParams::default_scale_for` gives `[1, 0.84, 0.73]` for the
  parametric curves and `[1, 1, 1]` for `characteristic`;
- **per stock** — `--film-stock` selects a published curve; `characteristic-aim`
  derives a per-stock red scale;
- **per bundle** — five `--preset` bundles, each carrying the exposure that lands scene
  mid-grey +1.33 stop on `portra-400` (a calibration convenience, not a promise that the
  presets render alike);
- **per output preset** — `legacy` / `film-master` / the display presets each resolve
  their own tone and exposure.

What has *not* moved is the no-flag resolution: `--preset` is `Option<String>` with
no default, so a bare `hanten convert` still resolves the knee'd sigmoid into
`gain-map-hdr` — the configuration that writes the structurally valid but inert
1.0x gain map.

## Why the split is decided

`algo/reconstruction-render-curve-split` reached a positive verdict on 2026-09-02:
seven real frames at matched lightness, over a user visual verdict from
`output/display-tone-mapping`. The reconstruction curve is the shipped sigmoid with
**both knees off** (bit-exactly the exponential), and `algo/conversion-presets` then
established `characteristic-generic` as the form that does it completely — it carries
each channel's own published curve, so it needs neither a scalar `Dmax` nor a
per-channel gain.

It also un-inerts HDR: same frame, same tone, MaxCLL 101 nits under the shipped
sigmoid against 999 under the characteristic curve. **That payoff reaches the explicit
gain-map presets, not the default path:** the user decided (2026-08-09, reaffirmed 2026-09-13) that the default
output becomes SDR lossless (`output/display-p3-default`). Prefer landing the two
default moves in one `pipeline_version` bump; the order is that task's open question.

## The release gate — colour, not tone

**This task must not ship until neutrality is checked against a known-neutral
reference frame** (`analysis/calibration-frame-capture`).

The gate is on a *different axis* from the goal. The goal is where tone shaping
happens; the gate is per-channel colour neutrality — green residual +0.40 mean,
+1.00 on the Ektar roll, which no per-channel scale removes. The split does not
create that residual; it exists today. It makes it **more visible**, because the
shoulder this migration removes was compressing the highlights where the cast lives.

A leader cannot settle it: a leader cannot separate a non-neutral exposure from a
scanner-slope error, so it can neither accept nor reject this migration.

**History worth keeping:** this gate was `film-base/dmax-per-channel-reduction` until
2026-09-10, when `algo/film-stock-profiles` disqualified the leader as a per-channel
source and showed the term is a *slope* (carried by `density.scale`, and by each
stock's own tables on `characteristic`) rather than the anchor that task weighs. It
was then an edge on `io/scanner-density-calibration` until 2026-09-12 — but that edge was
"necessary, not sufficient" for a simpler reason than it first looked: that task produces a
**3×3 + offset fit**, while this gate needs the **reference frames** a neutrality measurement
is taken against. Those are different artifacts, and nothing owned producing the second. The
edge now points at the task that does.

## Open questions

- **Does the default anchor placement move with the curve?** Keeping
  `MidAtDmaxFraction(0.5)` costs a measured **0.21–0.28 EV darker** than the
  lightness-matched anchor — inside what `print.print_exposure` corrects, and it avoids
  shipping the uncalibrated 0.626 offset as a constant.
- **Does the default headroom move 6 → 4?** Measured content reaches p99.99 **+3.64
  stops**, so `reinhard` at 4 covers it where the shipped 6 wastes two. Arguably
  `output/presets`.
- **What is the display-tone default?** Since 2026-09-09 the operator preserves
  mid-grey by construction, so the midtone half of this is closed. What remains is
  **0.86 stop at diffuse white** — a rendering-intent call no measurement decides.
- **What does the report have to say?** The split changes what a stage *does*, which is
  CLAUDE.md's "fifth spot" — check the prose claims, not just the values.
- **What survives the flag-surface audit?** `algo/characteristic-default-audit` decides which
  rules change behaviour when the value they read arrives from a default rather than from the
  user. Its answers are inputs here, not questions this task re-opens.
- **Does anything downstream assume the old bound?** Reconstruction currently holds
  `lin ≤ 1.0` under a positive shoulder; without it the master and both display sources
  go over-range by design.

## Known vs unknown

**Known:** the rendition is reachable today and ships as a named preset; `film-master`
accepts it at exit 0 and needs no change; the gain map goes live and its plateau share
improves 10–25x; a historical `PIPELINE_FINGERPRINTS` row must never be edited in place.

**A preset must not set `output.preset`.** The non-display presets (`legacy` / `custom`
/ `film-master`) resolve their own tone and exposure and refuse `reinhard`; `film-master`
refuses any non-default `print_exposure`. A global default move that ignored this would
break a bare `hanten convert --output-preset film-master`.

**The new fingerprint row may not be portable.** The problem and its evidence belong to
`algo/characteristic-fingerprint-vector` (split out 2026-09-13), which delivers the
vector this row hashes; do not write the row from `golden::pixels()` as it stands.

## How to Verify

- A `pipeline_version` bump with its own `PIPELINE_FINGERPRINTS` row, and a
  before/after report under `docs/reports/`.
- **Release gate:** neutrality measured against a **known-neutral reference, not the
  leader**, from `analysis/calibration-frame-capture` — **and a pass criterion applied to
  the number**, not merely the measurement taken.
  A measurement alone cannot hold this gate: `calibration-frame-capture` correctly accepts
  "we measured it and the residual is still there" as a complete outcome, because its job
  is evidence rather than colour. So this task must state what residual it is willing to
  ship under, decide against the measured value, and **record the decision either way**.
  If the answer is "not acceptable", the remedy is `io/scanner-density-calibration`'s 3×3
  + offset fit — named here as the path, deliberately **not** as a dependency edge, since
  the measurement may well show the residual is tolerable under a curve that already
  removes blue's part of it by construction (+1.26 → +0.09).
  **The threshold is unset and this task owns setting it.** Today's numbers are the only
  anchor: green +0.40 mean, +1.00 on the Ektar roll, per-roll spread ±0.5 stop/density.
- A bare `hanten convert --output-preset film-master` still succeeds, and every named
  output preset still resolves.
- Regenerating the preset review set through the new default produces byte-identical
  files to `--preset characteristic-generic` — the expansion acceptance test.
- `docs/using-nc.md` updated by running the binary, not by reading the diff.

## Dependencies

- [Reconstruction / render curve split](reconstruction-render-curve-split.md) — the verdict
- [Named conversion presets](conversion-presets.md) — the mechanism; this migration is the
  last step of that task, not separate work
- [A portable fingerprint vector](characteristic-fingerprint-vector.md) — split out
  2026-09-13: the `render` row's cross-target portability problem, solvable before the frames
- [Pin the characteristic curve against regression](characteristic-curve-coverage.md) —
  **done 2026-09-10.** Four property tests over the real `algo::reconstruct` plus a 1-ULP
  golden, so the default moves onto pinned wiring. The fingerprint row was deliberately left
  to this task; read the portability note above before writing one
- [Audit the flag surface against a characteristic default](characteristic-default-audit.md) —
  the CLI-surface half of this migration, split out because it is executable **now** while this
  task waits on the frames. Three rules key on the resolved curve rather than flag presence, so
  moving the default breaks commands that work today
- [Capture the calibration frames](../analysis/calibration-frame-capture.md) — produces the
  known-neutral reference the release gate names

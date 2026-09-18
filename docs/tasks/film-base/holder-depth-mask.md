# The effective measurement area

## Goal

Give the pipeline one function that answers "which part of this frame may a
measurement be computed over" — the **effective area** — and make it the first step
of every measurement path: `Dmin`, `Dmax`, content exposure and contrast, tiling
uniformity, roll-wide content statistics. Four tasks need exactly this region and
none should own a copy of it.

Exceptions exist only where the **user states a region explicitly**. Nothing in nc
opts itself out.

## The region is two cuts, in order

1. **The holder** — measured from the IR plane where it separates holder from film.
2. **A static inset** of what remains — the rebate, and anything else near the border
   that does not matter.

They are **sequential, not alternatives** (user, 2026-09-12, restated 2026-09-16).
Cut 2 is not a fallback for cut 1: it runs on every path, and where cut 1 did not run
it is simply the only cut.

**Scope note (2026-09-16).** When this task was split out of
`holder-masked-measurement` it disowned cut 2 — "the consumers' inset for the rebate
is a different thing and stays with them" — and modelled the static value as a
*substitute* for a missing IR mask. Both are reversed by the decisions above: the two
values are consumed together on every path, so they are resolved together, here. The
one thing that would justify separating them again is using the holder depth to
auto-crop the output, which is not planned.

## One function, but the answer depends on what is being measured

A user-stated area **replaces** the effective area for the measurement it was given
for — and only for that one. The example that sets the shape (user, 2026-09-16): a
single `convert` where the user points `--base-region` at a rebate strip to measure
`Dmin`, while a content-aware render measures exposure and contrast over the derived
effective area. One frame, two regions, both correct.

So if this ships as one shared function, it takes a **usage** parameter — measuring
the base, measuring content, and whatever follows — and decides per usage whether a
user-stated region overrides the derived one. Resolving a single region per frame and
handing it to everyone cannot express that case.

**Not needed yet**, and it should not be built speculatively: no current caller passes
a per-usage region. What it must not do is *preclude* it. That is a constraint on the
**inputs**, not the return shape (see Deliverable — one rectangle is fine): what to
avoid is a function whose only input is the frame, resolved once and handed to
everyone, because retrofitting a usage then changes every call site at once.

## Nothing is detected except the holder

**The rebate is never searched for** (user, 2026-09-16). Cut 2 insets past it blind.
Where a measurement needs unexposed film — `Dmin` — it is taken either over the whole
effective area of a reference frame, or over a region the user states. There is no
third path that goes looking for a rebate strip.

This **retires the shipped rebate detector**. `FilmBaseSource::Auto` today is
`rebate_candidates` + `select_auto_base` marching inward for a thin uniform band
behind the holder (design-spec §9). nc is not shipped, so the breaking change is
accepted (user, 2026-09-16); `film-base/holder-masked-measurement` owns the removal
and what `Auto` means afterwards.

Likewise, where IR cannot separate the holder, nc does **not** guess its depth from
pixel darkness. It says so, and the static inset is the user's to size.

## Deliverable (settled 2026-09-16, shipped 2026-09-17)

> **Shipped.** `film_base::effective_area` / `holder_depths` / `march_edge_depth`,
> the `measure.inset` knob (`types::{DEFAULT_MEASURE_INSET, MAX_MEASURE_INSET,
> check_measure_inset}`), the `effective_area` report field on
> `inspect`/`estimate`/`convert`, and `auto_dmax` wired region-only. Verified on 31
> real IR frames. Two bugs the measurement caught — a holder ring's corners
> collapsing every edge, and a 6 px sliver segment dragging one edge to the cap —
> are recorded in `docs/progress/film-base.md`. Read that before changing the march.


**It returns a per-edge rectangle** — four depths, `{top, bottom, left, right}`, not a
per-segment mask. Consumers clamp their walk to bounds instead of testing every pixel, so
there is no mask buffer and `pipeline::memory` gets no new term. The cost is accepted:
a holder covering only part of one edge widens that whole edge to its deepest segment.
`EdgeHolderMask`'s along-edge segments stay an *input* to the depth, not the output.

**Three pieces ship together:**

1. The function, its IR march and the static inset.
2. The inset knob — CLI flag *and* recipe key, with the merge arm and validate check.
3. The resolved rectangle and its provenance in the report and `nc inspect`.

Plus **one consumer, so the knob is not accepted-and-ignored**: `algo::density::auto_dmax`
takes the rectangle instead of the whole strided buffer. Region only —
`algo/auto-anchor-interior-measurement` keeps the loud-failure range check,
`measure_balance_range`, and the question of whether `Auto` is worth keeping.

**That seam leaves a known window, accepted deliberately**: `Auto` will read a better
region while an out-of-range result still renders black rather than refusing. What makes
it tolerable is that `DmaxSource::Fixed` is the default and `Auto` is opt-in
(`--auto-d-max`), so the window is reachable only by a user who asked for it — but
`algo/auto-anchor-interior-measurement` must land the check, and this task should not be
read as having made `Auto` safe.

**So "no pixel change" is not quite true any more.** Every default render is
byte-identical, and `version::PIPELINE_FINGERPRINTS` does not move (its golden vectors
resolve `Fixed`). A `--auto-d-max` run *does* change — that is the point of wiring it.

## What is known

- Holder depth is small and **asymmetric**: on the unexposed HP5 frame IR clears at
  ~2% of the short edge on the right, ~3% top and bottom, ~5% left. The IR march
  since measured **2.5–4% of the shorter edge** across 31 real frames, with the four
  edges differing on every one — one number cannot serve every frame, which is why
  cut 1 is measured. (The **10–15% of one edge** figure from
  `analysis/conversion-metrics` is *not* a depth: it is the holder's share of a
  rendered frame's top codes. It does not bound a cut.)
- **Zero is a real answer.** A frame that arrived already cropped has no holder, and
  the report must distinguish "cut 1 measured no holder" from "cut 1 did not run".
- `film_base::ir_separability` already decides per frame whether IR can separate
  holder from film. Consume that verdict; do not re-derive it.
- **B&W follows the same rule as everything else** (user, 2026-09-16): a frame whose
  IR separates cleanly gets its holder cut automatically; a frame whose IR does not
  separate is treated exactly as a frame with no IR plane, and the inset is the
  user's decision. No chemistry-keyed branch — that is what
  `ir-usability-detection` removed. Note the measured consequence: silver blocks IR in
  proportion to accumulated density, so ordinary *exposed* B&W frames often decline
  (HP5 frame 1354 reads interior IR 0.0818 against the 0.25 usability line) while an
  unexposed one separates 20:1. The no-IR path is normal for B&W, not exceptional.
- **Do not inherit the all-holder decline.** `ir_holder_mask` returns `None` when no
  film is found *along* any edge, which is 22 of 25 real chromogenic frames. In a
  depth-aware design that reading means "the holder wraps the whole border", the
  normal case, and the answer is to keep marching inward.
- The IR plane makes the depth test a threshold march inward per edge;
  `auto-base-redesign` already marches inward.
- Provenance is per run: the report says whether the holder was measured, skipped or
  measured as zero, how deep each cut went, and where the inset came from.

## The static inset is a knob

Default **5% of the shorter edge**, overridable by the user (user, 2026-09-12 and
2026-09-16). Per project convention that means a CLI flag *and* a recipe key, with
the merge arm and validate check that go with them.

**Of the *original* frame's shorter edge**, not of what the holder cut left (settled
2026-09-16). A given scan therefore always insets the same pixel count, whatever the
holder measured — the resolved value does not move with an IR measurement, and a user
overriding it is setting something predictable.

**The 5% default is not a claim that 5% is always enough, and that is deliberate.**
After an IR cut there is only a rebate left to clear. Where cut 1 did not run the same
5% has to clear the holder *and* its rebate on a frame nc could not measure, against a
measured holder depth of 2.5–4% — and the answer is the override, not a second default
nc picks on the user's behalf. nc's job is to measure what it can measure and to say
what it did; sizing a blind cut is the user's.

## Open questions

1. Is the inset one fraction for all four edges, or per edge? The default is
   symmetric either way; the question is whether the knob has to be.
2. A warning when cut 1 did not run — no IR plane, or IR that does not separate.
   Agreed in principle (user, 2026-09-16), **explicitly low priority**: it must not
   hold up the region itself, and it should not be `--strict`-promoted without
   thinking about how often the B&W path takes it.
3. ~~Does the effective area itself need a `--strict`-promotable sanity rule?~~
   **Answered yes, 2026-09-17 (ship review).** `capped` and `converged` both mean
   "the reported rectangle is not a measurement", and as `Serialize`-only fields
   nothing on any command read them — the channel that let a 10x over-cut through at
   exit 0. `film_base::effective_area_warnings` now emits both on every command that
   resolves the area, so `--strict` promotes them. An *empty* region stays the loud
   half: a refusal for a run that measures over it, a warning otherwise (there is no
   region to report, so `effective_area` is omitted).

## How to Verify

- A synthetic frame with a deliberately deep, asymmetric holder yields the per-edge
  depths the fixture encodes; a frame whose holder wraps the entire border is still
  measured, not declined. A **beyond-cap** holder is out of scope for getting right
  (`film-base/holder-cap-contamination`), but it must report per-edge `capped` and
  warn rather than pass a confident wrong rectangle.
- The two cuts compose: the same frame measured with and without an IR plane differs
  by cut 1 alone, and the inset applies in both.
- A frame with no holder reports cut 1 as measured-and-zero, distinct from a frame
  where cut 1 did not run.
- The inset override reaches the resolved region from both the flag and the recipe,
  and a value outside the sane range is refused rather than clamped.
- Default renders are byte-identical and `PIPELINE_FINGERPRINTS` does not move; a
  `--auto-d-max` run changes, and the fixture that resolved 2.23–2.37 now lands inside
  the frame's bright content.
- Output **dimensions** are byte-identical on every path — nothing is ever cropped.

## Dependencies

- [Decide IR usability by measurement](ir-usability-detection.md) — the per-frame
  verdict this consumes.

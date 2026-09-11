# Pin the `characteristic` Curve Against Regression

## Goal

Give the `characteristic` density curve a regression pin. Its *tables* are well covered,
its *wiring* not at all — so a refactor between the stages moves every characteristic pixel
with all four gates green.

**Closed 2026-09-10.** Two complementary pins, and one correction to the premise below.
The detail is in `docs/progress/algo.md`; the answers to the open questions are:

- **Which shape?** Both. Four *property* tests in `algo::film_stock::tests` run the real
  `algo::reconstruct` over a synthesized scan (neutral ramp on all ten stocks, published
  mid-grey, the `scale·d + offset` ordering, `out_of_table` against a recount), plus a
  golden in `pipeline::stages::golden` pinned to **1 ULP** rather than bit-for-bit.
- **A bit-exact capture turned out to be *mostly* available.** Two libms can only disagree
  on `10f32.powf` when the true value lies within ~`2^-5` of an f32 ULP from a rounding
  boundary; eleven of the fifteen `golden::pixels()` samples clear that by 2-16x. So which
  values are unsafe is decidable in advance, and `characteristic_golden_values_carry_their_libm_headroom`
  records the four that are not — including the film-base pixel, at 0.0059 ULP.
- **Is it falsifiable?** Measured, not argued: seven deliberate perturbations, each run
  against the full suite. The transposition is the one case the golden provably cannot see
  and the property test can; the counting-pass drift is the one only the new tests see.
- **The drift gate is deliberately untouched** — it covers the *default* render, so the
  row belongs to `algo/split-default-migration`, whose task file now carries the
  portability warning.

## Known at filing (verified 2026-09-10)

**The table layer is covered, synthetically — no assets, libm-safe.**
`algo::film_stock::tests` pins the Rust literals against the extraction, that no variant
can ship tableless, invertibility, the aim tables against the curves, and that a neutral
ramp reconstructs neutral on every stock. **Extraction reproducibility too:**
`scripts/analysis/digitize_datasheets.py --check` re-derives `curves.json` from the PDFs
and reports `ok: … matches the datasheets`. Fidelity of the printed *artwork* to the film
remains unverified, and probably always will be by machine.

**The wiring is not covered.** Nothing asserts what
`to_density → check_tables → apply_curve_per_channel → FilmRgbImage` produces: the curve has
**no golden vector and no `version::PIPELINE_FINGERPRINTS` coverage**, since the gate's
`render` hash measures only the default (sigmoid) path.

## Unknown — the constraint that makes this a task

**A checked-in bit-exact vector is not available.** The inversion runs `10f32.powf`, which
differs ~1 ULP across libm implementations, so a capture would be green on aarch64-darwin
and red on x86_64 CI — the cross-platform rule in `CLAUDE.md`. None was added when the
curve shipped, and that was correct. The open question is what shape of pin works
*without* one.

## Open questions

- Which shape? Candidates, none chosen here: a tolerance-based end-to-end assertion; a
  property that survives ULP noise (neutral in ⇒ neutral out, through the **full** chain
  rather than the table alone); a same-machine before/after harness like the one
  `color::to_output` relies on; or a non-transcendental proxy in the drift gate.
- Is it *falsifiable* — does it fail when a stage is reordered, or does it merely
  re-describe the build? And does it belong beside `pipeline::stages::golden` or in
  `algo::film_stock`, next to the table tests it complements?

## Related

- `algo/conversion-presets` proposes `characteristic-generic` as the **default**, activated
  by `algo/split-default-migration`. At that point this stops being optional: the drift gate
  has to cover the curve and the libm problem becomes load-bearing. Hence the edge into
  `split-default-migration` rather than `conversion-presets` — the default move lives there.
- **Watch item, not a defect:** the v4 `render` fingerprint was captured on aarch64-darwin.
  The mechanism is proven cross-target for rows 0–3, but v4 changed which f32 values flow
  through the sigmoid's `10^`, so that agreement does not transfer by argument. x86_64 CI is
  the first check.

## How to Verify

- The pin fails on a deliberate reordering of the wiring chain, and passes on **both**
  targets — a green local run is not evidence.
- No new asset dependency: `cargo test` stays green without `../nc-assets`, and the four
  CI gates pass.

## Dependencies

- [Film-stock profiles](film-stock-profiles.md)

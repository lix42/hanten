# Negative Converter — algo Progress Log

Execution log for the `algo` epic: what was done and how, key decisions, what
works, what doesn't. TASKS.md holds the authoritative status (the checkboxes);
this file is the narrative beside it.

One `##` section per task in this epic, named by the bare task name (the part
after the `/`). Read this whole file before starting a task in this epic, and
read other epics' `Epic summary` sections when you depend on them. Append
entries — don't rewrite earlier ones.

> **Consolidated 2026-09-13** (user-authorised; see CLAUDE.md's exception to the
> append-only rule). Sections of *done* tasks were rewritten as summaries keeping the
> decisions, gotchas and every measurement an open task cites; the full history is in
> git before that date. Sections of open and parked tasks are unchanged except: the PR
> #70 entry moved verbatim from `sigmoid-parameter-calibration` to
> `reference-anchored-sigmoid`, and `content-aware-sigmoid-toe`'s two sections were
> merged.

## Epic summary

What other epics need to know about `algo`:

- **The shipped surface is `reconstruct(image, base, config) -> (FilmRgbImage,
  ReconstructionReport)` plus `finish_print`** — the old `Converter` trait and
  `AlgoParams` are gone. The recipe is one **tagged `reconstruction` object**
  (`schema_version: 1`) selecting `simple` or `density`, with density carrying a
  tagged `sigmoid` (the default since `pipeline_version` 2, 2026-08-08),
  `exponential`, or — since #105 (built 2026-09-04, closed 2026-09-08, merged
  2026-09-10) — **`characteristic`**, which inverts a named film
  stock's *published* curve per channel instead of modelling it (`--film-stock`, ten
  digitized stocks, `algo/film-stock-profiles`). That third variant resolves **no reference
  density and no anchor placement**: `DensityCurve::anchor()` returns `Option` for that
  reason, `dmax()` reports `None`, and the report emits `null` rather than naming a rule the
  render never read. **The curve is opt-in, but a default did move**: `density.scale` went
  `[1, 1, 1]` → `[1, 0.90, 0.86]` on 2026-09-09 (`pipeline_version` **4**), and its default
  is **per-curve** — `DensityParams::default_scale_for` is the single definition, giving the
  parametric curves that calibration and `characteristic` the identity, because that curve
  already carries each stock's per-channel structure and the same gain would correct it
  twice (measured: channel means move from 0.039 to 0.185 off neutral). Resolved when a
  recipe omits the key, re-resolved on a `--density-curve` switch, and an explicit value
  always wins. Also **`--display-tone none` in practice refuses a render of ordinary
  picture content under this curve** — it hands the display scene-referred exposure
  (p99.99 = +3.64 stops over diffuse white) and the renderer's range check rejects the
  frame, so it needs `shoulder` or `reinhard`. There is no `validate` rule for the pair:
  dark enough content still renders at exit 0. The legacy `algorithm` +
  top-level `density`/`sigmoid`/`simple` keys are **rejected with migration
  errors** — never re-add them as aliases.
- **`--preset` ships five named reconstruction + display bundles** (`algo/conversion-presets`,
  2026-09-10), because every configuration worth shipping is a bundle whose numbers are
  meaningless separately — the `print_exposure` matching one brightness runs 0.31–0.61
  across the shipped presets (0.70 for `sigmoid-knees`, which cannot use the knob at all).
  Four things other epics must key on. **It is a CLI-only
  expansion, not a recipe key**: `--dump-params` writes the expanded values, a recipe
  naming a preset is rejected as an unknown field, and the name survives only as the
  report's `conversion_preset` provenance (with `overridden` and `replaced` lists, since
  flags still win over a preset and a preset can replace a recipe value).
  **Precedence is `defaults < params < preset < flags`** — a preset sits
  *above* the recipe, because nc writes every key explicitly and one layered underneath
  would be inert against any recipe nc produced. **A preset never sets `output.preset`**,
  so the non-display presets keep resolving their own tone and exposure, which is what lets
  a conversion default move without breaking `film-master`. And **`sigmoid-knees` takes its
  brightness from the anchor rather than `print_exposure`** (refused beside that preset),
  because `--display-tone none` cannot carry a scalar gain applied after a bounded curve.
  **No default moved**: `PIPELINE_VERSION` stays 4 and all three fingerprints are
  unchanged; `characteristic-generic` becoming the default is still the
  `algo/split-default-migration` step — whose no-stock blocker was **lifted 2026-09-10**.
  Since **2026-09-12** its release gate is neutrality against a known-neutral reference,
  which `analysis/calibration-frame-capture` produces; `io/scanner-density-calibration`
  is the remedy if that measurement fails, not the gate itself. The CLI-surface half
  of that move is `algo/characteristic-default-audit` (filed 2026-09-13, executable now).
- **The aim-matched red scale is derived at runtime**, not tabulated:
  `algo::film_stock::aim_red_scale(stock)` returns the factor `--density-scale` takes (the
  **reciprocal** of the one that scales the table), and `None` for the derived generic and
  the two 800-speed sheets whose Δ their own curves contradict by +44%. Anything needing it
  reads it there — the three constants that lived in the review script are gone.
- **The characteristic curve's wiring is pinned, and a fingerprint row over it is a
  harder bar than that pin** (`algo/characteristic-curve-coverage`, 2026-09-10). Four
  property tests run the real `algo::reconstruct` over a synthesized scan, plus a golden
  stated within a **derived per-sample window** rather than bit-for-bit. That window is
  what absorbs a libm disagreement, and the disagreement is real: x86_64's `log10f`
  returns a different density from Apple's on two of the fifteen captured samples.
  `reachable_window` renders every density a 1-ULP-accurate libm can return and takes the
  widest excursion — 1 ULP for nine samples, 63 at worst. Whoever
  writes the `PIPELINE_FINGERPRINTS` row when this curve becomes the default
  (`core/conversion-versioning` owns the gate, `algo/split-default-migration` the move)
  gets **no** such window — the gate hashes raw f32 bits — so budget for choosing a new
  vector rather than assuming the shared one carries over.
  `stages::golden::reachable_window` is the tool that decides it — enumeration over what
  a conforming libm can return, after two threshold-based arguments proved unsound.
- **`FilmRgbImage` is the typed boundary out of this epic.** Private fields,
  constructible only inside `algo`, so nothing can mint one that skipped
  reconstruction. `working_space::map_nc_film_rgb_v1` is its only intended
  consumer; its legacy alternative is `finish_print`, the stage-4 bridge that
  presets will displace.
- **A residual green cast is a known, measured gap** (mean +0.40 stops per unit corrected
  density; +1.00 on one roll). Blue's equivalent is fixed. The leading cause is the
  cross-channel term ACES applies before its curves and nc does not, and closing it needs
  one known-neutral frame on film — `io/scanner-density-calibration`. Anything judging
  colour fidelity out of this epic should expect it, and should not read the per-stock
  breakdown as established (frame-to-frame scatter swamps it at n=3–4).
- **Polarity: a *denser* negative renders *brighter*.** Stage 3 is
  `10^(+γ·(D′ − Dmax))`, not the `10^(−…)` in early spec sketches. A regression
  test pins this.
- **Density conversion and print rendering are separate sub-stages** — the core
  fidelity rule. Stage 2 (`to_density` → `regional_balance`) is shared by both
  curves; the curve is stage 3; `render_print` is the shared stage 4. A future
  curve injects itself as the stage-3 closure and reuses the rest.
- **Anchoring is not optional for sigmoid.** Both curves consume the resolved
  scalar `Dmax` from `film-base`; sigmoid *requires* a positive anchor and rejects
  `none`. `DmaxSource::None` is the bit-exact scene-referred escape hatch and is
  a density-only feature.
- **For sigmoid, `Dmax` is the *reference*, not the anchor** (since
  `reference-anchored-sigmoid`, 2026-08-03). `curve.anchor`
  (`AnchorPlacement`) says which tone the reference places: the default
  `MidAtDmaxFraction(0.5)` pins **mid-grey** at half the reference and lets white
  land above it, `WhiteAtDmax` is the old rule kept as a diagnostic. Consequences
  for other epics: the two coincide only under `WhiteAtDmax`, so the paper-black
  floor is `10^(−contrast·A)` and the reduce-to-exponential identity holds only
  there; a per-stock rule is a **third variant**, not a new field; and the default
  sigmoid contrast/shoulder are now `≈2.0687`/`0.6`, derived from manufacturer aim
  densities rather than chosen. The drift fingerprints did not move *for that*
  change — the default recipe still selected `exponential` at the time. **That bump
  has since been taken here, not by `output/presets`:** on 2026-08-08 the default
  curve became the sigmoid, `NOMINAL_DMAX` became `1.3` and the exponential's own
  `gamma` became `2.0`, recorded as `pipeline_version` 2 with its own
  `PIPELINE_FINGERPRINTS` row. `output/presets` owns the default *preset*
  migration; it no longer owes the curve bump.
- **`AnchorPlacement` is carried by *both* curves, and reference-freeness is a property of
  the placement, not of `DmaxSource`** (since `exponential-anchor-placement`, 2026-08-29).
  One curve-neutral `--anchor-*` flag family covers all four placements on both curves
  (`--sigmoid-mid-fraction` / `--sigmoid-white-at-d-max` survive as aliases). Two of the four —
  `black-at-base`, `mid-at-base-offset` — never read the resolved `Dmax`, which is what keeps
  the leader anchor's roll-to-roll error out of the render. Consequences other epics must key
  on: **`AnchorPlacement::reads_reference()` is the single shared predicate** for "does this
  render actually consume the reference", and every gate that asked `DmaxSource` alone was
  asking the wrong object — `film-master`'s provenance (`MasterAnchor`: roll-fixed /
  base-derived / none) and roll's "Dmax is NOT frozen" warning both route through it, so
  `--auto-d-max` with a base-derived placement is correctly neither refused nor warned.
  `curve.anchor` is emitted in the report for both curves. Defaults are unchanged and
  deliberately per-curve: sigmoid `MidAtDmaxFraction(0.5)`, exponential `WhiteAtDmax`.
- **The reconstruction/render split is settled affirmatively (2026-09-02), but no default
  has moved.** `algo/reconstruction-render-curve-split` measured that reconstruction should
  keep the density conversion, contrast and anchor and shed **both knees**, with the character
  supplied by the display operator. Consequences other epics must key on: the shipped default
  is **unchanged** and still the shouldered sigmoid, so anything describing what nc renders
  today is still correct; activation is `algo/split-default-migration`. Its gate stopped being
  `film-base/dmax-per-channel-reduction` on **2026-09-10** (the grey leader those 17-83%
  channel ratios were read off is disqualified as a per-channel source) and, since
  **2026-09-12**, is neutrality measured against a known-neutral reference from
  `analysis/calibration-frame-capture` — not `io/scanner-density-calibration`, whose
  checkbox can go green without that measurement ever being taken. `film-master` needs no work — its contract is the
  configured reconstruction, not a curve shape. And the **anchor is a pure gain exactly when
  the shoulder is off** (`anchor_is_a_pure_gain_only_without_the_shoulder`), which is what
  lets a matched-exposure probe solve a scalar instead of re-rendering; under the shipped
  shoulder that shortcut is wrong by 69-81%.
- **A curve switch resets `anchor` and says so.** `dmax` is a measured roll calibration and
  carries across a `--density-curve` switch; `anchor` is a rendering rule whose right value is
  per-curve, so it resets — and `curve_switch_dropped_anchor` warns (roll-level,
  `--strict`-promotable) when the discarded placement was not the base curve's own default.
- **Mutually exclusive knobs are one enum, never parallel fields** — `WbSource`,
  `BalanceRange`, `DmaxSource`, the tagged `Reconstruction`/`DensityCurve`. This
  is what makes the flags-win merge sound and provenance representable.
- **Auto modes are two-pass and report their result for reuse.** Auto white
  balance renders a neutral analysis pass, estimates, then re-renders through the
  normal slot; the resolved gains ride back in the report and reproduce the output
  **bit-exactly** when fed back as explicit values. Same pattern for
  `balance_range`. The sidecar records the *mode*; the report carries the frozen
  values — freeze from the report.
- **A knob that would be silently ignored is a loud error, not a no-op.** An auto
  WB mode under an algorithm with no print WB stage, a customized `gamma` under
  sigmoid, sigmoid flags under exponential — all rejected or warned after merge.
- **Numerical discipline:** non-finite input propagates through every stage
  untouched so `io::encode`'s non-finite counter still sees it. Never launder
  `NaN` (`f32::max(NaN, 0.0)` returns `0.0` — a real trap here). Extreme-but-finite
  params that would posterize are bounded by explicit caps (`contrast ≤ 50`, knee
  widths ≤ 10) because such output trips *no* counter.
- **Golden fixtures live in `pipeline::stages::golden`** — curated **per-pixel**
  `f32::to_bits` vectors, plus a decoded-pixels hash. Never checksum a whole
  encoded TIFF or post-lcms2 pixels: the embedded ICC and colour transform differ
  by target, so such a gate is green locally and red on CI.


## interface
**Status:** done (2026-06-16; superseded in shape 2026-07-23)

- Shipped the first pluggable surface in `src/algo/mod.rs`: an object-safe
  `Converter` trait, `Algorithm { Simple, Density }` with `FromStr` mapping unknown
  names to `NcError::Usage` (exit 2, never a silent default), and an infallible
  `build(AlgoParams) -> Box<dyn Converter>` whose enum variant *was* the selector, so
  an `--algorithm`-vs-params mismatch was unrepresentable rather than an error case.
- **All of that shape is gone** — `algo/negative-reconstruction-density-curves`
  (2026-07-23) replaced the trait and `AlgoParams` with the pure
  `reconstruct(image, base, config)` and the tagged `reconstruction` recipe object.
  Two decisions survived the replacement and still hold: an unknown selector name is
  a loud usage error, and a selector must not be carried separately from the
  parameter set that already implies it (the tagged enum is that rule, one level up).


## simple
**Status:** done (2026-07-12; scope narrowed 2026-07-23)

- `src/algo/simple.rs`: per channel, `normalized = value / base[c]` (the film base
  divides out, so an unexposed pixel → 1.0), then `positive = 1 − normalized`. No
  density math, no clamping (clamping is the u16 encoder's job), rayon ordered
  collect so the parallel path is deterministic, IR carried through untouched.
- **Fail-loud base guard.** `simple` was the first stage to divide by a
  *runtime-estimated* base (`Region`/`Auto`), which `cli::validate` never sees, so a
  `--base-region` over the dark holder produced `value / 0.0` silently. `convert`
  guards every channel finite-and-positive (`NcError::Other`, exit 1) with a message
  naming `--film-base`/`--base-region`. The deeper fix — rejecting a bad base where
  it is born — landed later in `film_base::estimate` (`film-base/auto-base-redesign`);
  the per-algo guards stay as defense-in-depth.
- 2026-07-23: white balance and the clip range were **removed from `simple`'s
  reconstruction** — it now ends at the unclamped `1 − scan/Dmin` `FilmRgbImage`,
  and `--invert-white-balance` / `--clip-low` / `--clip-high` are rejected with
  migration errors pointing at `print.white_balance` / `print.linear_range` (see
  `negative-reconstruction-density-curves`).
- 2026-08-11 (#95): `simple` is **not** the B&W path; B&W is `algo/bw-support`. It
  remains the debugging reference only.


## density
**Status:** done (2026-07-12, follow-ups 2026-07-13)

`src/algo/density.rs` implements the density-domain converter as two independently
testable pure sub-stages composed by the converter (`to_density` = stages 1–2,
`render` = stages 3–4). The exact equations, kept here because the task file asks for
them and later tasks build on them:

1. transmission → density: `D_c = -log10(max(scan_c, EPS) / base_c)`, `EPS = 1e-6`,
   applied only to *finite* zero/negative/denormal transmission — `NaN`/`±inf` scan
   samples propagate as `NaN` density so `io::encode`'s non-finite counter sees them.
2. density correction: `D'_c = density_scale_c · D_c + density_offset_c` (dividing by
   the per-channel base lands an unexposed sample on `D = 0` in every channel, so
   orange-mask compensation is structural; `scale`/`offset` trim the residual).
3. density → positive: `lin_c = 10^(density_gamma · D'_c)` — **positive sign**. The
   task file and early design-spec §7.2 wrote `10^(−D'·gamma)`, which yields the
   original *negative*; verified against darktable `negadoctor` (denser negative →
   brighter print) and pinned by `convert_is_positive_polarity_denser_is_brighter`.
   `dmax-white-anchor` later made this `10^(γ·(D' − Dmax))`.
4. print render: `lin_c = white_balance_c · 2^print_exposure · lin_c − black_point`,
   then a per-channel highlight soft-clip: identity for `x ≤ 1.0` or `amount ≤ 0`;
   above white `out = 1 + amount·(1 − e^(−(x−1)/amount))`, asymptoting to
   `1 + amount` (`amount = highlight_compress`; the `1.0` threshold is the documented
   definition of "highlight", not a hidden knob). Non-finite passes through unchanged.

Decisions still in force:

- `--highlight-compress` must be `≥ 0` (a negative value was a silent no-op).
- `check_base` guards an estimated/recipe base finite, `> 0` and `≤ 1` at the point
  of consumption (exit 1); an explicit `--film-base` is bounded to `(0, 1]` at the
  CLI (exit 2). A `90`-for-`0.90` typo previously blew out silently.
- `render` consumes its `DensityImage` (in-place, IR moved not cloned).
- **Silent underflow is real and unowned by this task.** A hugely *negative* `D'`
  underflows `10^(γ·D')` to a finite `+0.0` that no counter flags (a hugely positive
  one overflows to `+inf`, which the non-finite counter catches). Recorded at the
  time as an orchestration warning to add; it is `algo/density-safety-bounds`' second
  context block (which also found the same shape in `render_print`'s exposure gain).


## dmax-white-anchor
**Status:** done (2026-07-14; PR #17)

Closed the PR #12 finding that the default u16 encode clipped the whole image: stage
3 renders relative to a display-white anchor, `lin = 10^(γ·(D' − Dmax))`, so scene
white lands at ≈1.0 and the film base at `10^(−γ·Dmax)`. `to_density` untouched;
the sub-stages stay separate.

- **Apply the anchor in the exponent, not as a folded gain.** The first version
  factored it as `10^(γ·D') · 10^(−γ·Dmax)` and overflowed f32 when `γ·D'` alone
  exceeded the pow10 range (γ = 5 with EPS-clamped `D' ≈ 8` rendered scene white as
  `inf`). Regression test added. `None` stays bit-exact (`d − 0.0 == d`).
- **`DmaxSource { Auto | Explicit(f32) | None }`**, one enum, serializing
  `"auto"` / `{"explicit": d}` / `"none"`; CLI `--d-max` / `--auto-d-max` /
  `--no-d-max` conflict at the clap layer. `Auto` was the default here; it has since
  been demoted to opt-in (`film-base/dmax-reference` made the default `Fixed`, and
  the key moved to `reconstruction.curve.dmax`).
- **`Auto` measurement**: `AUTO_DMAX_PERCENTILE = 0.995` of the *finite* corrected
  densities, **scalar, pooled across all channels** (a per-channel anchor would double
  as colour correction). Nearest-rank via `select_nth_unstable_by(round((n−1)·p),
  f32::total_cmp)`, tie-order-independent so deterministic; empty/all-non-finite ⇒
  `0.0`. Measured from a deterministic strided sample capped at 2^20 values
  (`AUTO_DMAX_MAX_SAMPLES`), stride derived from length only and bumped off multiples
  of 3 so interleaved RGB is not single-channel biased.
  **Known since 2026-08-03: it samples the *whole* scan, so the opaque holder owns
  the top percentile on every real full-frame scan** (resolves 2.23–2.37 against roll
  `Dmax` 1.28–1.38) — `algo/auto-anchor-interior-measurement`.
- **`None` is bit-exact with the pre-anchor render** — HDR f32 workflows depend on it;
  pinned by `none_anchor_is_bit_exact_with_pre_anchor_render` with `assert_eq!` on
  f32, not an epsilon.
- The resolved anchor rides back for the report (then `ConvertReport.dmax`, now
  `ReconstructionReport`).
- Two silent-failure findings were dismissed with rationale and are worth keeping:
  an `Auto` anchor ≤ 0 brightening a dim frame is *correct* display-fill behaviour
  (the sigmoid later added its own positivity guard because its toe floor inverts
  there); and a pathological `--density-gamma` × `--d-max` underflowing the gain to a
  finite all-black image that no counter sees — deferred to an orchestration warning,
  i.e. `algo/density-safety-bounds`.
- Real-scan check on `tests/fixtures/hdr-48bit.tif`: default `Auto` clipped 0.49%
  (spot highlights) against 99.9996% with `--no-d-max`; resolved `Dmax ≈ 1.087`.


## sigmoid
**Status:** done (2026-07-14; #27)

The S-curve tone map in density space, originally `--algorithm sigmoid`, now the
`sigmoid` density curve and the product default since `pipeline_version` 2. Reuses
`to_density` (stages 1–2), the resolved `Dmax` anchor and the base guard from
`density`; stage 4 was factored out of `density::render` into the shared
`render_print` (bit-exact refactor, pinned by the existing value tests).

**The curve (design-spec §7.3)**, per channel, in log₁₀-output space, with `A` the
resolved anchor and `c = contrast`:

```text
t = c·(D' − A)                                 the straight line
F = −c·A                                       paper-black floor (the line at D' = 0)
p = F + toe·log10(1 + 10^((t−F)/toe))          toe  FIRST: soft-max with F   (skipped if toe = 0)
v = p − shoulder·log10(1 + 10^(p/shoulder))    shoulder LAST: soft-min with 0 (skipped if shoulder = 0)
lin = 10^v
```

Chosen over a closed-form logistic because with both knees at `0` the expression is
*bit-identical* to the exponential's stage 3 (pinned by `assert_eq!`;
`convert_with_knees_off_matches_exponential_bit_exactly`). Since
`reference-anchored-sigmoid` the anchor `A` is derived from the reference `Dmax` by
an `AnchorPlacement` rule rather than being `Dmax` itself.

Properties and gotchas that still hold:

- **Knee order is load-bearing: toe first, shoulder last.** The reverse order let the
  toe soft-max lift the white asymptote above 1 (≈1.056 at `--d-max 0.1`). With the
  shoulder last and written in the manifestly bounded form
  `−shoulder·log10(1 + 10^(−p/shoulder))`, `v ≤ 0` in f32 by construction, so for
  every **finite** density `lin ≤ 1.0` when `shoulder > 0` — the default u16 encode
  cannot clip highlights (scoped to stage-3 output under neutral print params; the
  print stage can lift samples above 1.0). `shoulder = 0` removes the roll-off and
  highlights can exceed 1.0 like the exponential.
- **`log10(1 + 10^y)` must be the stable `max(y,0) + log10(1 + 10^(−|y|))`** — the
  naive form overflows at `y ≳ 38`. And `f32::max(NaN, 0.0)` returns `0.0`: never
  launder NaN. `s_curve` returns a non-finite `d` verbatim *before* the knees, and
  surfaces a finite-`d` → non-finite knee overflow, so `10^v ≤ 1.0` is guaranteed only
  for finite stage-3 output and corrupt samples still reach the encode counter.
- **Caps close silent-destruction holes** that trip no counter: `SIGMOID_CONTRAST_MAX
  = 50` (an extreme slope collapses the curve into a two-level image) and
  `SIGMOID_KNEE_MAX = 10` for both knees (`shoulder 10000` → all-black, `toe 10000`
  → all-white, all finite and in range). Within-cap extreme params posterizing
  without a warning is an accepted, documented trade-off. These are the analogue
  `algo/density-safety-bounds` wants for `density_scale/offset/gamma`.
- **The anchor is required.** `dmax = none` is a usage error (exit 2) plus a
  fail-loud backstop in the converter; a resolved anchor that is not finite and `> 0`
  (an `Auto` percentile driven negative by a wrong base, or an empty sample) errors at
  exit 1, with `anchor_error` distinguishing corrupt input from a wrong base — with
  `anchor ≤ 0` the toe floor `10^(−c·A) ≥ 1` would render everything above white.
- `density_gamma` is not read by the sigmoid; under the tagged schema a
  `--density-gamma` beside a sigmoid curve is a post-merge usage error (originally a
  `--strict`-promotable warning).
- `--highlight-compress` composes with the shoulder rather than being disabled: with
  the shoulder on and neutral print params nothing exceeds 1.0, so the soft-clip never
  engages.

**Deferred, shared, not sigmoid-specific:** a *tiny-positive* anchor passes the `> 0`
guard yet renders degenerate (→ `algo/density-safety-bounds`); and nothing verifies
that a non-finite sample survives the lcms2 colour transform to reach the encode
counter — a colour/encode robustness gap with no owning task.


## auto-neutral-wb
**Status:** done (2026-07-14)

Two deterministic per-frame estimators behind the stage-4 `print.white_balance`
slot, ≈ NLP's Auto-AVG / Auto-Neutral.

- **`print.white_balance` is one source enum, `WbSource { Explicit([f32;3]) |
  GrayWorld | Percentile }`**, default `Explicit([1,1,1])`, serialized
  `{"explicit": [...]}` / `"gray-world"` / `"percentile"` (a deliberate wire change
  from a bare array). The variant records provenance, so explicit beats auto **by
  source**: `--white-balance 1,1,1` over a recipe's auto mode means neutral gains,
  never re-estimation. CLI `--white-balance R,G,B` vs `--auto-wb
  gray-world|percentile`. `every_auto_wb_source_has_a_cli_flag` is an exhaustive
  `match`, so a recipe-only mode cannot ship silently.
- An auto mode under `simple` (no print WB stage) is a usage error, not a no-op — the
  check *whitelists* `density | sigmoid` so a future path fails loudly by default.
  Explicit gains under `simple` stay inert (a value unused, not a computation dropped).
- **Estimators** (`density::estimate_wb_gains`): strided whole-pixel sample capped at
  2^20, non-finite dropped per sample, each channel fully sorted with `total_cmp`.
  `GrayWorld` = per-channel mean of the central 98% (`AUTO_WB_TRIM = 0.01`);
  `Percentile` = per-channel nearest-rank 95th (`AUTO_WB_PERCENTILE = 0.95`). Gains
  are **green-anchored** (`g = 1.0` exactly): WB corrects colour, not exposure.
  Degenerate channels fail loudly (exit 1), never silently neutral.
- **Estimation reads, application re-renders.** The anchor is resolved once; an
  analysis positive is rendered with a fully neutral print (unit gains, 0 EV, no black
  point, no soft-clip) so the statistic measures exactly what the WB slot multiplies;
  then the real render runs with the resolved gains through the standard slot. Explicit
  gains skip the analysis pass, so the default path's arithmetic is unchanged.
- **Reuse is bit-exact**: report gains → `--white-balance` → byte-identical output,
  pinned in unit and E2E tests. The **report** carries the resolved gains; the
  **sidecar** records the mode (re-running re-estimates) — the hazard
  `core/unfrozen-auto-mode-warning` exists for. `nc estimate` was deliberately not
  extended (Dmin-only contract).
- **Rebate/border pixels are not excluded** from the statistic: they render neutral by
  construction and dilute gains toward 1 rather than casting them. The shared
  measurement region is `algo/auto-anchor-interior-measurement`'s to own.
- 2026-08-08 (sigmoid default): auto-WB is a *weaker* corrector for a **wrong** base
  under the sigmoid — a wrong base leaves a constant per-channel density offset that
  the exponential turns into a constant factor a gain cancels exactly, while the
  sigmoid is nonlinear in that domain. The estimator is unchanged; the effect vanishes
  with a correct base.


## regional-color-balance
**Status:** done (2026-07-17)

Shadow/highlight per-channel balance completing stage 2 between `to_density` and the
curve (`regional_balance` in `algo/density.rs`):

```text
D'_c = B_c + shadow_balance_c·w_lo(D̄) + highlight_balance_c·w_hi(D̄)
w_hi = smoothstep((D̄ − lo)/(hi − lo)),  w_lo = 1 − w_hi
```

`D̄` is the per-pixel **scalar** tone — mean of the *finite* pre-regional corrected
channels (per-channel weighting would misfire on exactly the crossover pixels; a NaN
channel is excluded from the tone but stays NaN). Equal balances degenerate to a
uniform offset.

- **Naming (§9):** "shadow"/"highlight" are the *positive's* regions — low corrected
  density = shadow — and a positive balance value brightens that channel there.
- **`BalanceRange { Auto | Explicit([lo, hi]) }`**: `Auto` measures nearest-rank
  0.5% / 99.5% of `D̄` over a strided sample (cap 2^20 whole pixels), in the same
  domain the ramps consume so non-default `scale`/`offset` cannot make them drift. It
  deliberately does not anchor on an `Auto` `Dmax`, which is measured *after* stage 2.
  Reported as `balance_range` for roll reuse via `--balance-range`.
- **`consults_balance_range` is `shadow_balance != highlight_balance`**: equal
  non-zero balances short-circuit to a tone-independent offset and never measure the
  range. `algo/curve-endpoint-validation`'s per-endpoint deferral keys on this.
- Ordering: before the curve, so an `Auto` `Dmax` resolves from post-balance
  densities, and before print WB. Neutral `[0,0,0]` returns before touching the
  buffer (bit-exact, even `+0.0` would flip `−0.0`). An unmeasurable `Auto` range
  with a requested balance is `NcError::Other` naming `--balance-range`.
- Applies under the sigmoid too (it shares stage 2); without that, `--shadow-balance`
  would have been a silent no-op there.


## negative-reconstruction-density-curves
**Status:** done (2026-07-24; default flip 2026-08-08; warning rounds 2026-08-09)

**2026-07-23/24 — the tagged schema.** `Algorithm::{Simple,Density,Sigmoid}` became
one nested tagged `reconstruction` object (`schema_version: 1`, `type:
simple|density`, density carrying `.density {scale, offset, shadow_balance,
highlight_balance, balance_range}` and one tagged `.curve` — `exponential {gamma,
dmax}` or `sigmoid {contrast, toe, shoulder, dmax}`). `algo::reconstruct` /
`finish_print` replaced the trait; every path returns a private-field `FilmRgbImage`
(`pub(in crate::algo)` constructor — `pub(super)` on a top-level module is
crate-wide, a review catch). Decisions still in force:

- **Golden-first.** Pre-refactor outputs were captured as per-pixel `f32::to_bits`
  vectors for nine configurations plus decoded-pixel hashes before any code moved;
  they live in `pipeline::stages::golden`. A whole-encoded-TIFF hash failed on Linux
  CI because the embedded ICC carries platform-dependent bytes — retargeted to
  `tiff_pixels_hash` (decode back, hash samples + dimensions). Bit-identity was
  independently re-proven by a reviewer at HEAD.
- **Legacy forms are rejected with migration errors, never aliased:** `--algorithm`,
  the top-level `algorithm`/`density`/`sigmoid`/`simple` keys, and `simple`'s
  `--invert-white-balance`/`--clip-*` (whose homes are `print.white_balance` /
  `print.linear_range`). `merge` is fallible: density/curve/Dmax flags with `simple`,
  sigmoid flags under exponential, `--density-gamma` under sigmoid are post-merge
  usage errors keyed on **flag presence**.
- **A curve switch carries `dmax`** (a roll calibration) across variants — in
  `merge`'s `--density-curve` arm and in `merge_json`'s `internally_tagged_switch`
  for roll per-frame overrides, which replaces the tagged object instead of
  deep-merging a rejected union. Later widened: `anchor` is *not* carried (see
  `exponential-anchor-placement`), and the carry is gated on `takes_dmax()` since
  `characteristic` has no `dmax` key (see `film-stock-profiles`, 2026-09-09).
- **Report:** `recipe` (the effective config) and `reconstruction_result` with
  `curve.dmax = {policy, value, provenance}` — policy `fixed|explicit|auto|none`,
  provenance `default|recipe|cli|auto-frame`. Recipe-vs-default provenance is
  witnessed from the raw JSON at load (`LoadedRecipe.curve_dmax_present`), since a
  recipe that wrote `"fixed"` is indistinguishable after defaulting. Telemetry
  `SCHEMA_VERSION` 2 (`conversion.reconstruction` + optional `conversion.curve`).
- `reconstruction.schema_version` versions the wire **shape** and is checked for
  exact equality; behavioural drift is `pipeline_version`'s.

**2026-08-08 — three render defaults moved together (`pipeline_version` 1 → 2):**
`NOMINAL_DMAX` 2.0 → 1.3, default curve exponential → sigmoid, exponential `gamma`
1.0 → 2.0. Baseline in `reports/render-defaults-v2.md`.

- The headline was clipping: four real frames went from 0.00/3.38/4.86/1.98% clipped
  to 0.00% on all four. Partly an accounting artifact — `io::encode` counts `v > 1.0`
  strictly and the sigmoid saturates to exactly `1.0f32` above `D' ≈ 3.1`, so the clip
  counter is not a sufficient measure of highlight preservation under this curve.
- **Measurement lesson worth keeping:** a zsh helper interpolated an unquoted
  parameter (zsh does not word-split), so `--d-max 2.0` was silently dropped and the
  first table published 72% clipping for the wrong anchor. A comparison must assert
  the varied thing changed (the report prints `dmax`); the measurement now lives in
  `scripts/render-defaults-v2/measure.py` with an explicit argv list.
- `NOMINAL_DMAX` 2.0 sat above **every** measured roll (0.90–1.74, median ≈1.34) and
  darkened Ektar 963 by 5.09x in linear terms. 1.3 is the median rounded, still
  *nominal*; `film-base/dmax-anchor-reliability` owns the calibrated value. This
  **superseded** that task's 2026-08-03 "do not settle it yet" (user, 2026-08-08):
  waiting meant shipping a wrong anchor. Harman Phoenix (0.8976) counts as the
  population floor, not an exclusion example.
- Exponential `gamma` 2.0 fixes the black floor (72 → 12/255 on confirmed shadow
  patches) and costs 2.75 EV of midtone — a real partial win whose residual became
  `algo/exponential-anchor-placement`. Lesson: a frame mean cannot separate "floor
  fixed" from "midtones moved"; read percentile metrics.
- Consequences: `film-master` and `hdr-linear-tiff` integration tests now select the
  exponential explicitly (the sigmoid never exceeds 1.0, so they stopped exercising
  their subject — the container); HDR headroom stays absent (`GainMapMax` ≈1.0027x
  under both curves; the shoulder, not the curve family, decides it — see
  `exponential-anchor-placement`); every single-rendition HDR preset emits a
  `--strict`-promotable `hdr::sdr_range_warning` built on the existing `clli`
  measurement when the render peaks at SDR range (201 vs 203 nits on the fixture)
  while advertising 1000 nits — `ultra-hdr-v1` excluded, since low headroom there
  shows as an inert gain map, a different diagnosis. Goldens were **not** rebased:
  reference captures name their configuration (`frozen_reference_curve`,
  `sigmoid_at_reference_anchor_2_0`) and the new default got a fresh golden that pins
  "has not drifted since set", not "matches the reference implementation".

**2026-08-09 — the moved-default warning, four rounds to a structural rule.**
`curve_default_warning` / `unpinned_curve` warn that an archived recipe will render
differently because defaults moved under it. The predicate went under-warned (omitted
`curve` only), over-warned (any present key; but `"dmax":"fixed"` names a *policy*
resolving through the moved `NOMINAL_DMAX`), over-warned again (any `"fixed"`, which is
exactly what this build writes, so a sidecar failed its own `--strict` replay), then a
version-based exemption that fixed the sidecar but not bare `--dump-params` output.
Root cause of all four: the predicate was tuned against hand-written JSON while
**nothing tested the one file nc itself writes**. Final rule: **warn only on shapes
this build cannot produce** (absent `curve` / `anchor` / `gamma` / `dmax`), gated end
to end by `recipe_dumped_by_this_build_replays_clean_under_strict` (dump → replay,
byte-compared). Two residual gaps — an archived bare `"dmax":"fixed"` does not warn,
and CLI overrides that pin the floating values still do — are recorded in the task file
and belong to `core/recipe-replay-fidelity`'s policy, not to more special cases here.
Also from those rounds: `sigmoid_rejects_no_d_max` had been passing on clap's
duplicate-flag rejection (the fourth test that session found passing for the wrong
reason — an exit-code assertion more than one rule can produce; assert the message);
`scripts/analysis/benchmark.json` lost exponential coverage when the default flipped and
gained an explicit `hdri-exponential` case; two shipped doc examples passing
`--density-gamma` exited 2 and were fixed by *running* them.


## bw-support

**Status:** not started
**Updated:** —

- Goal: Convert B&W negatives to clean mono positives through the existing `density` algorithm.


## density-safety-bounds

**Status:** not started
**Updated:** —

- Goal: Close the gap where a validation-passing density recipe can silently produce a degenerate (e.g. finite all-black) image, via bounded `density_scale`/`density_offset`/`density_gamma` ranges at the CLI `validate` boundary plus a post-render degenerate-output warning.
- 2026-07-27 (from the `color/film-master-render-pipeline` review; **no code changed
  here**): a **second** silent-underflow site was confirmed and reproduced — the
  **stage-4 print render**, not the stage-3 tone map this task's original context block
  describes. `render_print`'s `2f32.powf(print.print_exposure)` (`algo/density.rs:478`)
  and `px[c] * wb[c] * exposure_gain` (`:486`) are guarded only by `finite()` /
  `positive()`, so `--print-exposure=-200` writes 100 % zero samples at rc 0 with every
  `loss` counter at 0, no warning, and `--strict` also 0; `--white-balance=1e-45,1,1`
  kills exactly one channel the same way (so a whole-image collapse test would miss it).
  The overflow direction (`--print-exposure 300`) is already loud via `clipped_low`.
  The measurement table, the exact reproduction command, and two implementation notes
  (why a naive `is_normal()` on user-supplied gains is the wrong fix here, and where a
  reference predicate already exists in `pipeline::render_split`) are in the task
  file's second `Context` block — start there rather than rediscovering it.


## reference-anchored-sigmoid
**Status:** done (2026-08-03; PR #70; review follow-ups 2026-08-03/13)

Fixed the shipped sigmoid's measured "pale, compressed shadows" defect. The whole
phased record (fixture freezing, three review rounds of patch selection, the candidate
harness, the review pages) is in git history; what follows is the evidence and the
decisions that later tasks still read. Report: `docs/reports/sigmoid-reference-baseline.md`.

**Direction (2026-07-30/31).** Dmin stays the density origin; the sigmoid owns floor /
toe / midtone placement in a roll-fixed coordinate; film-master and display share one
tonal foundation; the default is reference-driven and preserves under/overexposure.
Terminology: the sigmoid is **Dmax-anchored** (`t = contrast·(D' − Dmax)`), not
"Dmax-normalized".

**Phase 0 — fixtures frozen (2026-08-02)** via `harness.sh freeze` from the manifest:

| Roll | Dmin (r,g,b) | Dmax |
|---|---|---|
| `2026-07-24-Gold200` | 0.6001831, 0.27512017, 0.14776836 | 1.2758015 |
| `Ektar` | 0.51679254, 0.2768597, 0.18973067 | 1.2933096 |
| `Portra160-2026-07-22` | 0.49988556, 0.24776074, 0.14920272 | 1.3816013 |

The older `Portra160.json` / `Portra400.json` recipes were *orphaned* by an asset
reorganisation, not stale — restored folders reproduced their Dmax exactly (1.3352162,
1.7382799). Gold 200 confirmed by the user as Kodak Gold 200 (E-7022).

**The leader `Dmax` is uncontrolled — same-stock pairs, same scanner** (the evidence
`film-base/dmax-anchor-reliability` rests on):

| Stock | pair | base Δ (r / g / b) | leader Dmax Δ |
|---|---|---|---|
| Portra 160 | `Portra160` vs `-2026-07-22` | +0.029 / +0.027 / +0.021 | +0.046 (0.15 stops) |
| Portra 400 | `Portra400` vs `-leica-flaw` | **−0.0005** / +0.023 / +0.021 | **−0.295** (0.98 stops) |

Both quantities cannot be film properties. ±0.03 density is the cross-roll
reproducibility floor for a Dmin-referenced quantity. Leaders are **uniform** (interior
tile range 0.024–0.067, gradients ≤ 0.024), so the case rests on the level, not on a
fogging gradient. In both pairs the later roll carries ~+0.02 more green/blue base
(n = 2, hypothesis only — bounds how far a one-time scanner profile can be trusted).

**Phases 1–3 — what the frames said** (`pipeline::shadow_metrics`, `#[cfg(test)]`,
asset-gated; `scripts/sigmoid-baseline/fixtures.json` holds each patch's rectangle,
user-confirmed semantics and **validity flags** — 2/10 valid diffuse whites, 7/10 mids,
9/10 shadows, 2 frames usable for the datasheet Δ):

- **Diffuse white lands at 41–93% of the leader `Dmax` (median ~66%), never near
  100%**; density headroom above the brightest textured diffuse candidate is 0.09–0.81
  (median ≈0.43). Mid-tone sits at 11–58% of `Dmax` across frames — the genuine
  exposure spread the default must preserve.
- **The exposure deficit is large and path-independent**: with white pinned at `Dmax`
  and contrast 2.0, measured mid-tones need +2.52 … +3.57 EV to reach 0.18. The
  datasheet chain closes on itself: white pinned at *diffuse* white with `contrast =
  0.745/Δ = 2.07` puts a mid-grey Δ below white at exactly 0.18 — so the whole deficit
  is the anchor, and reference-driven forms predict zero compensation.
- **Real content exceeds the leader `Dmax`** (G3 1.3265 vs 1.2758; P3 1.5062 vs
  1.3816): the anchor does not even bound the frame.
- **Reference-driven beats content-driven.** Scoring anchors against uncensored EV
  preferences (−1…+3.5): pin the *measured* brightest diffuse patch — mean |diff| 0.96
  EV; pin the *datasheet* diffuse-white-above-base — **0.63 EV**, with residuals
  **systematic per stock** (Ektar ≈ +0.6, Portra 160 ≈ 0 within 0.16 EV on three
  scenes, Gold 200 ≈ −1.0) — the signature of per-stock constants each off by a fixed
  amount (they rested on chart-read `D-min`, since superseded by curve-digitized values
  in `film-stock-profiles`). Content-driven modes force frames with no true white (fog,
  a sign, specular leaves) too bright — frame-local fitting misbehaving as predicted.
- **Scope reduced by the user: filter forms, do not tune parameters.** Asking for a
  preferred EV per frame *is* frame optimisation; only the central tendency is usable
  (median +1.5 EV at contrast 2.0 = white 0.452 density below `Dmax`, against a measured
  diffuse-white gap of 0.417 — two routes to ~0.42–0.45). Acceptance became qualitative
  gates; parameter tuning is `algo/sigmoid-parameter-calibration`, which needs a
  bracketed roll and a grey card rather than more random frames.
- **Every anchoring form reduces to one anchor `A` plus a contrast**, so eight
  candidates ran through one curve implementation with no new curve code. Verdicts
  (user, on renders): **3 and 8 GO** (mid pinned at `f·Dmax` / at `Dmin + datasheet
  offset`), **5b most likely GO** (black pinned — its first rejection was a parameter
  error, anchor 1.607 above every roll's Dmax), 1 (shipped) maybe not, 2 not.
  Candidate 8 scored best of any shippable form (0.78 EV, 27/255) but could not ship:
  its per-stock offset needed a Status M `D-min` that did not yet exist.
  **A low between-frame spread is not a merit** — a reference-driven anchor applied to
  frames that differ in exposure *should* leave spread; low spread means the anchor is
  correcting exposure.
- **`DmaxSource::Auto` is dominated by the film holder** (found here): 2.23–2.37 on
  every frame against roll Dmax 1.28–1.38, so content-driven candidates rendered 0/255
  — holder contamination, not a verdict on the form. Filed as
  `algo/auto-anchor-interior-measurement`; content-driven anchoring is "explicit-mode
  only" (`algo/content-aware-sigmoid-toe`), never the default.
- **Shoulder ≈ 0.6, with a mechanism.** All eight configs had used the shipped 0.2,
  which was calibrated for a regime where content never exceeded white; moving the
  anchor to diffuse white made the shoulder load-bearing. Local contrast per 0.05
  density on candidate 8 (A = 1.03, c = 2.069):

  | `D′` | region | sh 0.2 | sh 0.6 | sh 1.0 |
  |---|---|---|---|---|
  | 0.67 | mid-grey | 0.0430 | 0.0393 | 0.0308 |
  | 0.85 | upper mid | **0.0995** | 0.0716 | 0.0498 |
  | 1.20 | highlight | 0.0043 | 0.0428 | **0.0507** |
  | 1.40 | curtain | **0.0000** | 0.0117 | **0.0298** |

  Each shoulder starts eating local contrast at `D′` 0.95 (0.2), **0.70** (0.6),
  **0.45** (1.0); mid-grey is 0.67, so 0.6 bends at mid-grey where a print shoulder
  belongs and 1.0 flattens the whole upper half. User verdict 0.6 > 1.0 > 0.2.
  Anchor height governs highlights, contrast governs shadows, the shoulder relaxes the
  conflict. A shoulder selected from how much content is too light is content-adaptive
  and belongs to the explicit mode; a *per-stock* shoulder from datasheet curve shape
  would be legitimate (no data yet — sigmoid-parameter-calibration).
- **G3 and P3 are unresolvable by any single global curve** (sky +0 vs trees +2;
  window +0 vs people +1.5): their scene range exceeds SDR — the HDR question, deferred
  by the user until every candidate is renderable. `sips` cannot downscale a gain-map
  JPEG without destroying the gain map, so HDR review needs full-size files.
- Review previews must come from the **measured** renderer: the first pages rendered
  through the legacy path while bounds were measured on `pipeline::sdr`; corrected to
  `--output-preset ultra-hdr-v1`, whose JPEG base *is* the SDR rendition. A finding
  measured on the legacy path only ("exposure costs 7–26% blown highlights") did not
  transfer; the exposure deficit did.

**Phase 4 — what shipped (2026-08-03), and why it stopped at remedy 2.** §7.3's
equation is unchanged; the defect was never in the curve but in *which tone it pins*,
and recalibration alone could not fix it (steepening a white-pinned line pivots about
white and drags midtones down), so two coupled changes were needed:

- **Defaults recalibrated:** `contrast 1.0 → 0.745/0.36 ≈ 2.0687` (from the
  manufacturers' mid-to-white aim delta; film gamma 0.52 / system gamma 1.07 as
  corroboration), `shoulder 0.2 → 0.6`, `toe` unchanged; derivations in the doc comments.
- **Anchor reparameterized:** `curve.dmax` is the roll's *reference*; `curve.anchor`
  (`AnchorPlacement`, an enum, not bool + f32) says which tone it places, default
  `{"mid-at-dmax-fraction": 0.5}` (candidate 3, `A = f·R + 0.745/contrast`), with
  `white-at-dmax` retained as the diagnostic reproducing the old defect. `f = 0.5`
  halves the fallback's `Dmax` error (`dA/dR = f`).
- Goldens moved and were recaptured with reasoning at the site (base pixel 0.0115 →
  0.00177; the auto-WB golden gain dropped 2.2304 → 1.0574, i.e. WB had been partly
  compensating for the broken curve). Drift fingerprints unmoved — the default recipe
  still selected `exponential`; that bump came 2026-08-08.
- `NOMINAL_DMAX` was deliberately left at 2.0 (user asked to wait for more rolls) —
  **superseded 2026-08-08**, see `negative-reconstruction-density-curves`.

### 2026-08-03 (later) — PR #70 review: four findings, and why one remedy was refused

(Relocated verbatim from `## sigmoid-parameter-calibration` on 2026-09-13; PR #70 was this
task's PR.)

- **The report named a number the render did not use.** `ReconstructionReport.dmax` was
  documented as "the display-white anchor the curve used" and, after the placement split,
  carried the *reference* instead. Now both travel: `dmax` (reference, what a recipe freezes
  back) and `curve_anchor` (derived, what rendered to 1.0 and therefore sets the floor at
  `10^(−contrast·anchor)`). The JSON `reconstruction_result.curve` gained the placement
  *rule* plus `anchor_value`, so that block is self-contained — a consumer no longer has to
  re-derive the anchor from the echoed recipe, which is the opposite of what diagnostics are
  for. Exponential reports both fields equal rather than a null, so consumers need no special
  case.
- **A tiny contrast panicked instead of erroring.** `MID_GREY_OUTPUT_DECADES / contrast`
  overflows below ~2.2e-39, and the `debug_assert` I had left there turned that into exit
  101 in debug and `inf` fed into `s_curve` in release. Now a `validate` usage error naming
  the flag, plus `apply_curve` returning a real error for the programmatic path (the
  defense-in-depth pattern `algo/simple.rs` already uses). Two things worth recording: the
  bound applies **only** to the mid-grey placement, since `WhiteAtDmax` performs no
  division; and `f32::MIN_POSITIVE` is *accepted* on purpose — the quotient is finite there,
  and because `contrast · anchor` is then exactly `MID_GREY_OUTPUT_DECADES` the render is a
  flat mid-grey rather than a broken one. My first test asserted it should fail, which was
  wrong about the arithmetic.
- **The roll consistency check had a fourth hole.** `resolve_frames` probed `film_base`,
  `curve.dmax` and `output.preset`, so a per-frame `curve.anchor` override silently gave one
  frame a different placement *rule* — subtler than a different Dmax number and, by our own
  documentation, a roll-level property. Added as warning (5) of six.
- **Refused: bumping `reconstruction.schema_version`.** The reviewer was right that an
  archived sigmoid recipe now renders differently — real, and it would have been silent. But
  the proposed remedy is wrong for this codebase and the reasoning is worth keeping: that
  constant versions the schema **shape** and the reader checks it for *exact* equality, so
  bumping to 2 would reject every archived recipe outright, including the large majority that
  select `exponential` and are wholly unaffected. The alternative — preserving v1 semantics
  via a per-version default table — is a real design, but it would have to cover `contrast`
  and `shoulder` too (both moved in the same commit with the identical property), and that is
  `core/conversion-versioning` policy, not something to improvise inside an algo task.
  **What is not acceptable is silence**, so it is now a loud, `--strict`-promotable warning
  when a loaded recipe selects sigmoid with no `anchor` — modelled directly on the existing
  `pipeline_version_warning`, which handles the same "parameters still apply, default moved
  underneath them" situation one level up.
- **A gap that is genuinely unowned, flagged rather than filed:** `core/conversion-versioning`
  is scoped to *default* behaviour ("bumps only when default conversion behaviour changes"),
  so nothing currently owns "a non-default path changed and archived recipes for it are
  reinterpreted". The warning covers this instance; the policy question is open.

**2026-08-13 — the shipped `MidAtDmaxFraction(0.5)` has a quantified error** (from
`exponential-anchor-placement`): the mid patch sits at `D′ = 0.513` where `0.5·Dmax`
puts the anchor at 0.650; that 0.137 density is 0.91 stops at contrast 2.0 against
candidate 3's measured 0.93 EV — the shipped default's whole residual is the fraction,
and 0.395 would be correct for these rolls. But (a) it is correct only while `Dmax`
means the leader, and (b) `f` is also the coupling to that unreliable anchor (the two
rolls 0.295 apart swing 0.98 stops at `f = 0.5`, zero at `f = 0`), so re-tuning it
fixes the systematic half and leaves the roll-to-roll half. The calibrated answer is
more likely a change of *reference* than a better fraction.

**Where the artefacts are.** `pipeline::shadow_metrics` (`propose_patches`,
`characterise_reference_frames`, `measure_candidates`; run with `--test-threads=1`
or roll headers interleave), `scripts/sigmoid-baseline/` (fixtures, review-page
builders — the review pages themselves were superseded by
`analysis/comparison-review-tooling`). Output under `../temp`, never published as an
Artifact: the frames are the user's own photographs.


## content-aware-sigmoid-toe

**Status:** not started
**Updated:** 2026-07-31

- 2026-07-30: Parked content-derived toe placement as an optional, explicit
  follow-up rather than part of the product default. The task distinguishes
  per-frame and roll-frozen acquisition, requires complete provenance, and
  forbids frame-local fitting from film-master/normal product presets so nc does
  not silently auto-correct exposure.
- 2026-07-31 (PR review): Added `output/presets` as a prerequisite. This task
  promises named-preset rejection and byte-identity verification, so the preset
  surface must exist before those contracts can be implemented or tested.


## film-stock-profiles
**Status:** done (filed 2026-08-02; built 2026-09-04; closed 2026-09-08; merged 2026-09-10 as #105; `density.scale` default 2026-09-09)

**Goal and the decision everything follows from.** A selectable registry of known
stocks carrying the per-stock reference densities reconstruction needs, from
manufacturer datasheets with provenance, with a generic C-41 fallback so naming a stock
is a refinement, never a precondition. It shipped as a **third density curve**,
`characteristic` (`--density-curve characteristic` + `--film-stock`, recipe
`reconstruction.curve = {"type":"characteristic","stock":…}`), which inverts each dye
layer's *published* density-to-log-exposure relation per channel — so the registry
stores **curves, not scalars**, and resolves no reference density and no anchor.
Opt-in, no default moved for the curve itself.

**Constraints recorded up front (2026-08-02, PR #68 review)** and still in force:
measured roll `film_base` stays authoritative — a published `D-min` is nominal only
(base fog shifts with processing, storage and roll); and single-wavelength
*chart-read* values off the Spectral-Dye-Density chart are **not** Status M densities
and must never reach a render path (provenance kind `chart-read`). Deliberately not a
dependency of `film-base/dense-base-dmax-plausibility` (a false edge would kill
parallelism; coordinate so stock-awareness is not solved twice).

### 2026-09-04 — the datasheet corpus, digitized

- **Corpus — 8 colour stocks digitized from the published curves:** 7 Kodak C-41 sheets (Ektar 100 `E-4046`, Portra 160 `E-4051`, Portra
  400 `E-4050`, Portra 800 `E-4040`, Gold 200 `E-7022`, UltraMax 400 `E-7023`,
  UltraMax 800 `E-7024`) plus the legacy five-stock Portra sheet (160NC/160VC/400NC/
  400VC/800, E-4040 2009-02 — `publication` alone is not an identifier, the
  `(publication, revision)` pair is); 16 B&W sheets (Kodak, Ilford, Kentmere). **No
  data exists for Harman Phoenix** (a fixture roll — "unnamed stock resolves to
  generic" is a first-class path), no Fuji (never publishes the diffuse-white row).
  Aim values are identical across the 2016 and 2025 revisions.
- **Method:** Kodak still sheets are vector art, so curves digitize exactly. Calibrate
  `y` off the **plot frame** (bottom `D = 0`, top `4.0`) — calibrating off the axis
  *label baselines* biases every density by ~0.05. Two independent extraction paths
  (raw content stream, `pdftocairo -svg`) agree to ±0.002. Ilford/Harman/Kentmere are
  raster and Fuji's PDFs encrypted; both need a different route. Take `D-min` from the
  leftmost point with monotonicity enforced (Gold 200's blue dips 0.03 there).
- **Constraint 2 lifted for Kodak still stocks:** the characteristic curve is plotted
  in **Status M** (stated on the plot, same densitometry as the aim table), so its
  `D-min` is a Status M density and `mid aim − D-min` — the `MidAtBaseOffset`
  `reference-anchored-sigmoid` could not ship — is available at ~±0.01. Provenance
  kind `curve-digitized` was added for exactly this.

  | Stock | mid aim | white aim | Δ tab | D-min R/G/B | mid−D-min | Δ curve | γ R/G/B |
  |---|---|---|---|---|---|---|---|
  | Ektar 100 | 0.82 ±.05 | 1.18 ±.05 | 0.36 | 0.209/0.634/0.844 | 0.611 | 0.401 | 0.608/0.589/0.656 |
  | Portra 160 | 0.84 ±.05 | 1.20 ±.05 | 0.36 | 0.200/0.616/0.835 | 0.640 | 0.370 | 0.524/0.536/0.587 |
  | Portra 400 | 0.82 ±.05 | 1.18 ±.05 | 0.36 | 0.220/0.647/0.867 | 0.600 | 0.376 | 0.531/0.555/0.633 |
  | Portra 160VC | 0.87 ±.06 | 1.28 ±.06 | 0.41 | 0.219/0.645/0.860 | 0.651 | 0.391 | 0.552/0.572/0.656 |
  | Portra 400VC | 0.87 ±.06 | 1.28 ±.06 | 0.41 | 0.219/0.646/0.867 | 0.651 | 0.392 | 0.553/0.573/0.655 |
  | Portra 800 (EI 800) | 0.85 ±.10 | 1.10 ±.10 | 0.25 | 0.308/0.706/1.021 | 0.542 | 0.362 | 0.512/0.532/0.594 |
  | Gold 200 | 0.95 ±.10 | 1.35 ±.10 | 0.40 | 0.251/0.657/0.991 | 0.699 | 0.383 | 0.543/0.565/0.611 |
  | UltraMax 400 | 0.90 ±.10 | 1.30 ±.10 | 0.40 | 0.285/0.694/0.980 | 0.615 | 0.355 | 0.503/0.524/0.583 |

  Red channel; `Δ curve` is the density rise over the 0.694 decades between an 18%
  grey card and a ~89% paper white, taken at the mid aim's own exposure.
- **Δ is stock-dependent** (the NC/VC pair: 0.36 vs 0.41 at the same speed, corroborated
  by their curves), so a generic Δ is a fallback, not a law. **Both 800-speed sheets
  tabulate Δ 0.25 against curves saying ~0.36** (±0.10 aim ranges make the table
  uninformative there) — prefer the curve; `Δ_tab / γ ≈ 0.69` holds to ±0.05 on every
  other stock. Aim-table tolerance bounds what any registry can buy: ±0.05 is ±0.34 EV
  at contrast 2.07, ±0.10 is ±0.68 EV.
- **The per-channel structure — the one per-channel datum the sheets carry — says
  `density.scale = [1,1,1]` is wrong on every stock:** blue runs 12–19% steeper than
  red, green 2–5%. A neutral ramp in nc's `D′` (Portra 400):

  | stops vs mid | −4 | −3 | −2 | −1 | 0 | +1 | +2 | +3 | +4 |
  |---|---|---|---|---|---|---|---|---|---|
  | D′_R | 0.035 | 0.133 | 0.287 | 0.442 | 0.600 | 0.762 | 0.926 | 1.093 | 1.262 |
  | D′_G | 0.038 | 0.145 | 0.311 | 0.479 | 0.646 | 0.813 | 0.979 | 1.144 | 1.311 |
  | D′_B | 0.085 | 0.247 | 0.435 | 0.624 | 0.814 | 1.005 | 1.197 | 1.390 | 1.582 |
  | blue error after WB at mid | −0.164 | −0.100 | −0.066 | −0.032 | 0 | +0.029 | +0.057 | +0.083 | +0.106 |

  White balance is one gain (a constant density shift) and can zero one row only: 1.9
  EV of blue swing at contrast 2.07. **`scale` alone cannot fix it** — the channels'
  toes sit at different exposures (`D′_B/D′_R` drifts 1.25–2.43); the (scale, offset)
  pair fitted over −2…+4 stops:

  | stock | scale G | offset G | scale B | offset B |
  |---|---|---|---|---|
  | Ektar 100 | 0.996 | −0.023 | 0.857 | −0.090 |
  | Portra 160 | 0.975 | −0.039 | 0.863 | −0.067 |
  | Portra 400 | 0.976 | −0.025 | 0.850 | −0.088 |
  | Portra 160VC | 0.986 | −0.031 | 0.852 | −0.101 |
  | Portra 400VC | 0.988 | −0.031 | 0.856 | −0.096 |
  | Portra 800 | 0.967 | −0.058 | 0.857 | −0.004 |
  | Gold 200 | 0.971 | −0.035 | 0.887 | −0.002 |
  | UltraMax 400 | 0.956 | −0.045 | 0.862 | −0.012 |
  | **generic** | **0.977** | −0.036 | **0.860** | −0.057 |

  The **gain is nearly stock-independent** (blue 0.850–0.887), the **offset is not**
  (−0.101…−0.002, splitting by tier), so a generic scale is defensible and a generic
  offset is not. Deriving either from roll statistics *per run* is content-derived and
  forbidden for a default; a constant calibrated once from a corpus is not (2026-09-09).
- **The leader cannot validate any of this.** Evaluating each sheet at the leader's
  red `D′`: measured `G−R` / `B−R` do not reproduce the published divergence (Ektar
  predicted B−R +0.322, measured +0.048; Portra 160's leader has **red** densest, which
  no C-41 neutral response gives). The leader's exposing light is not neutral and/or
  the scanner's slopes are not Status M's — either way it is not a neutral reference,
  and an earlier suggestion here to measure per-channel gamma on it was wrong.
- **Curve shape:** the film has a toe and essentially no shoulder in the scanned range
  (local γ/mid γ: Portra 400 0.35 at −4 stops → 1.09 at +6; real rolls reach ~+4 stops
  over mid). So the sigmoid's default shoulder (bending from mid-grey) is print
  character, not film — measured support for `reconstruction-render-curve-split` — and
  nc's toe has the wrong sign (the film compresses shadows; the sigmoid compresses them
  again).
- **User decisions:** the blue highlight cast is gamma, not base offset (it grows with
  density); try the datasheet structure on our scans rather than gating on a
  calibration; the scanner-to-Status-M slope is postponed to
  `io/scanner-density-calibration`; no ColorChecker frame exists, so the numbers ship as
  a hypothesis judged on rendered results.
- **Measured transfer (21 frames, 6 rolls, 4 stocks, `algo::curve_probe::channel_drift`
  — asset-gated, `#[ignore]`d):** bin interior pixels by red corrected density and
  measure how the blue-minus-red log-exposure ratio drifts across bins after
  normalising at the middle bin (a scene-colour bias shifts every bin together and
  cancels).

  | | scalar | curve | datasheet predicts |
  |---|---|---|---|
  | blue drift slope, stops per unit density | **+1.26** | **+0.09** | **+1.29** |
  | ektar-100 (n=3) | +1.22 | +0.13 | +1.23 |
  | portra-160 (n=8) | +1.18 | +0.15 | +1.11 |
  | portra-400 (n=7) | +1.24 | −0.07 | +1.62 |
  | gold-200 (n=3) | +1.54 | +0.29 | +1.07 |

  The scans carry 90% of the divergence the sheets claim and inverting the curves
  removes it. **Read the slope, not the swing** — per-frame slopes scatter −3.7…+4.8
  from real scene colour; only the mean is the film's signature.

### 2026-09-04 → 09-06 — the `characteristic` curve shipped

- **It is the ACES `ADX10 → ACES` film-scan transform with per-stock data:**
  per-channel density → cross-channel matrix → per-channel curve inverse → `10^` →
  matrix, **no tone curve anywhere**. ACES's two free constants (film gamma 0.55,
  mid-grey 0.70 above base) sit inside what our per-stock measurements bracket. nc
  substitutes the stock's own data and **omits the cross-channel matrix** — the
  scanner-to-Status-M correction (ST 2065-2 NOTE 3: "3 × 3 matrix followed by an
  offset", product-specific, "likely imperfect") deferred to
  `io/scanner-density-calibration`.
- **Self-anchoring:** each table's log-exposure axis is shifted so the stock's mid-grey
  aim sits at `log10(0.18)`, so `10^(curve⁻¹(D′))` is relative scene exposure with
  mid-grey at 0.18 by construction. No `dmax`, no `AnchorPlacement`;
  `DensityCurve::anchor()` returns `Option` and the report emits `null`.
- Ten stocks in `algo/film_stock/curves.rs`: the eight measured, `ultramax-800`
  (needed three silent extractor fixes; it digitizes to the same curve as Portra 800 —
  D-min 0.308/0.706/1.021 both, γ within 0.003 — from different publications, which
  is an independent check on the extraction and explains both anomalous Δ 0.25 tables),
  and `generic-c41`, the average (red mid-scale γ 0.541, mid-grey 0.624 above base).
- **Every parametric knob is refused, not ignored**, each naming a remedy the branch
  accepts: `--d-max`/`--auto-d-max` by **flag presence** in `validate_convert` (a
  resolved value cannot tell "asked for fixed" from "left at the default"),
  `--sigmoid-*`, `--anchor-*`, `--density-gamma` (remedy `--density-curve
  exponential`, the curve that has a gamma), and `stock` under either parametric curve.
- **Roll-fixed:** `sets_curve_stock` is the fifth roll-consistency probe — a per-frame
  `curve.stock` override converts but warns, `--strict` promotes.
- **Out-of-table samples extrapolate along the end slope and are counted**, never
  clamped. The warning shipped at 1% and fired on every real scan (5.2–7.2%): **100% of
  those samples lie in the outer 12% of the frame** (holder and rebate, denser than any
  exposed image), **0.00% of the frame sits at the `SCAN_EPSILON` floor** (the holder
  is dense, not clamped — the first proposed fix would have been a no-op), and the
  statistic cannot diagnose a wrong stock or base (seven stock profiles move it
  5.75→6.55%, a 30% wrong base 6.00→6.44%). Threshold now **0.20**, the message states
  the fact, and the per-channel fractions ride unconditionally in
  `reconstruction_result.curve.out_of_table`. The *interior* fraction (0.00% on every
  fixture) is the statistic that would diagnose; it needs
  `algo/auto-anchor-interior-measurement`.
- **Re-derivable registry (2026-09-06):** publications in `docs/datasheets/` (24
  files), `scripts/analysis/digitize_datasheets.py` (`PDF → curves.json`, needs poppler,
  run by hand, `--check` reproduces), and `curves_match_the_digitized_json` audits
  `curves.json → curves.rs` in CI with no poppler. A wrong publication id (`E-4022`,
  invented) for the VC pair was caught by committing the sources. Every stock
  round-trips through an emitted recipe byte-for-byte (serde's `kebab-case` renames
  `Portra400` to `portra400` — hand-written `Serialize`/`Deserialize` against one
  `as_str`/`parse` pair). The aims ride in `curves.json`, the pinned table and the
  report (`stock.aims`).
- Tests are property-based rather than golden bit vectors (a neutral ramp run forward
  through each stock's curves reconstructs on every stock and channel), since a new
  bit-exact vector would be a coin flip on the Linux runner — the pin came later in
  `characteristic-curve-coverage`.
- Clippy's `approx_constant` fires on the digitized `0.78539` (near π/4); the module
  allows the lint with a comment.

### 2026-09-06 — user review: blue fixed, a green cast left

Ten frames × six configs. The characteristic curve keeps detail and fixes the blue
cast; the shoulder display tone loses highlight detail on it. A **green cast** remains,
ranked by the user Ektar worst, Portra slight, Gold 200 clean — with `generic-c41`
*better* than the per-stock profile on the bad stocks. `channel_drift` extended to green
reproduces that ranking exactly:

| stock | green: scan | datasheet predicts | **residual** | blue residual |
|---|---|---|---|---|
| ektar-100 | +1.26 | +0.22 | **+1.00** | +0.13 |
| portra-160 | +0.88 | +0.40 | **+0.48** | +0.15 |
| portra-400 | +0.65 | +0.46 | **+0.18** | −0.07 |
| gold-200 | +0.41 | +0.36 | **+0.08** | +0.29 |

- **This corrects the 2026-09-04 "cast removed" claim**: that measured blue. Green
  does not transfer, and a green/magenta error has no warm/cool reading the eye
  forgives. **Measure the channel that matters perceptually, not the one with the
  biggest number.**
- The digitization is right (Ektar checked through both extraction paths: γ agrees to
  0.004, `D-min` to three decimals; x-axis 37.16 vs 37.29 pt/decade). Ektar's sheet is
  the corpus outlier — it draws red and green nearly parallel (`γ_G/γ_R` 1.002 against
  1.02–1.05) and predicts the *least* green divergence where the scans show the most.
- **`aim_table_agrees_with_the_curve`** checks each sheet's two halves against each
  other over exactly the interval the two aims span: Portra 160 +3%, Portra 400 +6%,
  Gold 200 −5%, VC pair −4/−5%, **Ektar 100 +11%, UltraMax 400 −11%** (the 800s +44/45%,
  exempt). A first, window-sensitive version (±0.35 decade gamma) manufactured a −15%
  and a spurious sub-unity `γ_G/γ_R` — **match intervals when comparing two published
  quantities**. It is a sheet-quality check, **not** a predictor of rendered colour
  (Portra 160 passes and still casts +0.48).
- **Leading hypothesis for green:** the cross-channel `CDD → CID` term — its magnitude
  depends on dye set and mask, which is the stock-dependence seen; blue happens to
  survive per-channel treatment and green does not. Routes to
  `io/scanner-density-calibration` with one known-neutral frame. `generic-c41` renders
  better on Ektar/Portra only because averaging nine curves dilutes any one sheet's
  error — a reason to fit the matrix, not to prefer the generic.

### 2026-09-08 — close-out

**Verified.** Blue's exposure-dependent cast falls from +1.26 to +0.09 stops per unit
density against +1.29 predicted (21 frames, 6 rolls, 4 stocks); a neutral ramp
round-trips on every stock and channel; all ten stocks round-trip through an emitted
recipe byte-for-byte; the user's ten-frame review confirmed the blue fix and kept
detail. Reading the ACES source first is what made this cheap.

**What a dependent task must know.**

- The cross-channel matrix is the one stage deliberately not implemented; the green
  residual (+0.40 mean, +1.00 on the Ektar roll) most likely lives there.
  `io/scanner-density-calibration` owns it and needs a **known-neutral target on
  film** — replacing Ektar's anomalous channel relationship with the corpus consensus
  would move its residual only +1.00 → +0.87.
- **Do not read the per-stock or per-roll residual breakdown as established.**
  Frame-to-frame scatter is sd 0.3–1.6 at n=3–4, one roll spans −1.88…+2.03; only the
  21-frame aggregate and Ektar's roll (sem 0.17) are solid. Two claims were retracted
  for this; resolving a 0.3 difference needs ~11 frames per roll.
- Two sheets disagree with themselves (Ektar +11%, UltraMax 400 −11%); what to do about
  an internally inconsistent stock (ship, warn, withhold) is undecided.
- Anything wanting per-stock *parameters* reads the 2026-09-04 tables above rather than
  re-deriving. B&W cannot use this shape (contrast index is set by development) —
  `algo/bw-support`.

### 2026-09-09 — the sigmoid's `density.scale` default: the datasheet value is the wrong one, but `[1,1,1]` is not the answer either

Measured on the scalar (sigmoid) render path over 21 real frames from six rolls:
`algo::curve_probe::sigmoid_scale`. Drift is the least-squares slope of each channel's
log2 exposure ratio against red density, normalised at the middle bin — a pure tilt in
stops per unit density, 0 neutral, and an offset cannot move it. The scalar path leaves
`contrast · (D'_c − D'_R)`, so with `D'_c = s_c · D_c` each channel's drift is exactly
linear in its own scale: `drift_c(s) = (contrast / log10 2) · (s_c · r_c − 1)` with
`r_c = dD_c/dD_R` the scan's slope ratio; the probe verifies the closed form against
real re-renders (worst disagreement 0.017 stops/density). Measured `r`: **green 1.115,
blue 1.183**, so the corpus nulls at green 0.897, blue 0.845.

| scale | green | blue | **green–magenta** | \|g–m\| | rolls within ¼ |
|---|---|---|---|---|---|
| `[1, 1, 1]` (shipped) | +0.79 | +1.26 | +0.16 | 0.60 | 1/6 |
| `[1, 0.977, 0.860]` (datasheet generic) | +0.61 | +0.12 | +0.55 | 0.73 | 2/6 |
| `[1, 0.90, 0.86]` | **+0.02** | **+0.12** | **−0.04** | 0.53 | 2/6 |
| `[1, 0.897, 0.845]` (corpus null) | −0.00 | +0.00 | −0.00 | 0.52 | 1/6 |
| `[1, 0.905, 0.905]` (equal slopes) | +0.06 | +0.49 | −0.18 | 0.57 | 2/6 |

- **The figure of merit is green–magenta, `(G−R) − (B−R)/2`**, not each channel's
  drift: `G−R` and `B−R` tilting together reads as a colour-temperature drift the eye
  attributes to the light. The datasheet scale shrinks both drifts while making
  green–magenta *worse* (+0.16 → +0.55, worse on 4 of 6 rolls): the sheets correct blue
  well (predicted +1.29 vs measured +1.26, 98%) and green badly (predicted +0.39 vs
  +0.79, 49%), so nulling blue un-masks a green tilt.
- **Recommended and shipped: `[1, 0.90, 0.86]`** — blue 0.860 from the datasheets
  (independently corroborated at 98%), green 0.900 from the scans (the published 0.977
  is the one number known wrong here). Preferred over the raw corpus null because 0.860
  is the better-evidenced blue and the two differ by 0.01 stop/density.
- **What it does not fix:** the green–magenta *mean* goes to zero but the magnitude
  barely moves (0.60 → 0.53), only 2 of 6 rolls land within a quarter stop, because the
  residual is per-roll scatter no constant removes (Ektar +0.65 → +0.41,
  Portra160-2026-07-22 +0.08 → −0.09, Portra160 +0.49 → +0.24, Portra400 +0.50 → +0.26,
  Portra400-leica-flaw −0.32 → −0.47, Gold200 −0.36 → −0.50). A better default, not a fix.
- **It is a scanner calibration wearing a film default's clothes:** the two nulling
  scales came out close (0.897, 0.845) against datasheet values far apart (0.977,
  0.860) — green and blue are *both* ~11–18% steeper than red in our scans where the
  sheets say green barely is. An excess on both channels against red is the signature
  of the scan/decode/base path, not chemistry. `io/scanner-density-calibration` is the
  real fix; label this constant as calibrated on one scanner and one six-roll corpus.
- Two corrections to the first version of this entry: it recommended leaving
  `[1, 1, 1]` (treating the choice as binary when green is a free parameter), and it
  averaged per-frame nulls (`mean(1/r)`, biased high by Jensen — the corpus null is
  `1/mean(r)`). And **the flat level is not a non-issue**: the drift probes normalise
  at the middle bin and define the level away, but a scale is a tilt about `D = 0` and
  moves the level at every non-zero density — on a mid-grey patch
  (`midtone_placement::the_default_gain_shifts_per_channel_level_on_both_curves`) the
  gain moves R/G/B by −0.061/−0.371/−0.565 (characteristic) and −0.080/−0.413/−0.672
  (sigmoid): green ~0.31 and blue ~0.50 stop below red, a strong yellow shift the
  offset half of the pair exists to absorb.

### 2026-09-09 — `density.scale` `[1, 0.90, 0.86]` shipped as `pipeline_version` 4, and it splits the two curves

On a scene-neutral patch (Portra 400's datasheet mid-grey), delivered spread (max
per-channel deviation from the mean):

| curve | identity gain | `[1, 0.90, 0.86]` |
|---|---|---|
| sigmoid | 0.545 | **0.327** |
| characteristic | **0.000** | 0.194 |

The same constant is a 40% improvement on one curve and a pure regression on the
other: the sigmoid has no per-channel film model, so the gain covers film structure
*and* scanner residual; the characteristic curve already removes the film half and is
exactly neutral on a datasheet neutral, so the gain corrects twice. The decomposition is
clean — sigmoid drifts green +0.79 / blue +1.26, characteristic +0.40 / +0.09, and the
difference (+0.39, +1.17) is what the sheets predict (+0.39, +1.29). **So the sheets
predict green correctly**; what they lack is an *additional* scanner residual of about
the same size in green. The characteristic path's own nulling gain would be
`[1, 0.938, 0.985]`, and even that measures worse than identity on real frames (0.047
vs 0.039 on `|G/R−1|+|B/R−1|`, identity vs the shipped gain 0.039 vs 0.185), so the
residual stays visible for `io/scanner-density-calibration` rather than half-absorbed.

**Per-curve default** (`DensityParams::default_scale_for(DensityCurveType)`:
`[1, 0.90, 0.86]` for `sigmoid`/`exponential`, `[1, 1, 1]` for `characteristic`),
following the `anchor` precedent, resolved in **three** places:

1. a recipe omitting `density.scale` — in `Reconstruction`'s `Deserialize`, reading
   key *presence* off the raw JSON (the field is a concrete `[f32; 3]`, so after serde
   an absent key and an explicit `[1,1,1]` are the same value);
2. `--density-curve` — the merge arm resets the gain to the target curve's default,
   and runs **before** the `--density-scale` arm so an explicit gain still wins;
3. a per-frame `roll` overlay — reset by hand in the planner, because the overlay is
   JSON-merged onto the *serialized* shared config where the key is always present
   (`sets_density_scale` keeps the reset from overriding a stated overlay).

`curve_switch_dropped_density_scale` warns when a reset discards a value that was not
its own curve's default. Two roll-path defects the seam hid: switching a frame to
`characteristic` from a manifest was impossible (`internally_tagged_switch` carried
`dmax` unconditionally and `characteristic` rejects the key — now gated on
`takes_dmax()`), and the overlay carried the sigmoid's gain onto the switched curve.
Nothing prevents code from building `Reconstruction::Density { density:
DensityParams::default(), curve: Characteristic }` directly — the two in-crate call
sites state their gain explicitly; a resolving constructor would be tighter if a third
appears.

**What landed:** `PIPELINE_VERSION` 3 → 4 with a new row (`render`
`323499bad6c71237`, `recipe` `72e424ee6a15d53b`, `base` unchanged);
`golden_new_default_is_bit_identical` recaptured (red bit-identical, only green and
blue move); every frozen/reference golden and the tests asserting the density
definition or auto-WB robustness now state `frozen_density()` / an explicit identity
gain — **a probe measuring a shipped default must state the identity, never inherit
it** (moving `DensityParams::default()` off identity silently desynchronised three
`#[ignore]`d probes, including the one cited as evidence, with every gate green;
`curve_probe::identity_gain` names the trap, and changing a default means re-running
the ignored set by hand).

### 2026-09-09 — five conversion presets scoped; two calibration traps

`--preset` was filed as `algo/conversion-presets`; the evidence gathered while defining
the five removed candidate designs. All five are calibrated to **one target** — scene
mid-grey (0.18) delivered at 0.223, the brightness approved this round — and each
preset's `print_exposure` is whatever lands it there
(`midtone_placement::each_candidate_look_needs_its_own_print_exposure` fails if the
spread collapses to one shared default):

| preset | reconstruction | tone | delivers | needs |
|---|---|---|---|---|
| `characteristic-stock` | characteristic, the stock | reinhard | 0.1800 | +0.31 |
| `characteristic-generic` | characteristic, `generic-c41` | reinhard | 0.1702 | +0.39 |
| `sigmoid-flat` | sigmoid, no knees | reinhard | 0.1461 | +0.61 |
| `sigmoid-knees` | sigmoid, toe/shoulder | none | 0.1372 | +0.70 → **impossible** |

- **Trap 1: `sigmoid-knees` cannot use `print_exposure`.** `--display-tone none` is
  self-policing on the render ceiling and `print_exposure` is a scalar gain *after* the
  bounded curve, so any positive value is refused (measured: luminance 1.6236 at +0.70,
  exactly `2^0.70`). The anchor is the knob that works — `mid-fraction 0.42` lands the
  target to 0.027 stop (`the_linear_rendered_sigmoid_takes_its_brightness_from_the_anchor`).
- **Trap 2: the aim-matched red scale is a reciprocal.** `stock_table_variants` prints
  the factor scaling the **table's** red density (Ektar 0.898); `--density-scale`
  multiplies the **scan's**, so the flag takes `1/k = 1.114`. Measured on 21 frames
  (`scale_against_the_characteristic_curve`): identity green–magenta +0.35, 0.898
  table-side **+0.72**, 1.114 as the flag **+0.01** — the flattest of anything on this
  path. `portra-800`/`ultramax-800` have no usable aim delta and must be refused.
- The survey harness had been double-correcting (characteristic cases built with
  `DensityParams::default()`, ~0.06 stop); it now resolves the gain from the curve.
- On whole-image channel means four of the five presets sit within 0.005 of each other
  (`|G/R−1|+|B/R−1|` 0.033–0.038, `sigmoid-flat` 0.063) — the metric is a tie and the
  visual verdict is the whole decision.


## auto-anchor-interior-measurement

**Status:** not started
**Updated:** 2026-08-03

- Goal: make `DmaxSource::Auto` measure the picture area, not the whole scan.
- 2026-08-03 (found by `algo/reference-anchored-sigmoid`): `Auto` takes the 99.5th percentile
  of corrected densities over the **whole frame**, and the nearly-opaque film holder sits at the
  `SCAN_EPSILON` floor, so its corrected density is enormous and it owns the top percentile. On
  the three fixture rolls `Auto` resolved to **2.23–2.37** against roll Dmax 1.28–1.38, and every
  frame rendered to 0/255. The fix is a sampling *region*, not a new statistic — `film_base`
  already locates the rebate by marching inward, so plumb a resolved interior in via the
  orchestrator (as the film base is). An implausible `Auto` result must fail loudly.
- 2026-09-12 (**rescoped after review with the user**): the task is now *Exclude the holder
  from content-driven measurement*.
  - **Measurement only — the output image is never cropped** (user decision). Dimensions,
    aspect ratio and pixel count stay as decoded. An IR-driven crop was considered and
    declined for now: keeping the ratio/pixel count stable matters more than the convenience,
    and it takes `--export-ir` plane alignment, the memory model and design-spec §2's
    auto-crop line out of scope entirely.
  - **The rebate is deliberately not detected.** `D = −log10(scan/base)` puts the rebate at
    `D ≈ 0`, the *bottom* of the distribution, so it cannot disturb a high percentile. The
    user made this point and it is correct; the old "picture area" framing was wider than the
    measured defect required. Manual cropping of the rebate stays the user's.
  - **Two exclusions, not one region rule.** IR-based (consuming the `ir_separability`
    verdict `ir_holder_mask` already keys on, rather than re-deriving "is there an IR
    plane?") plus a fractional-inset fallback for the no-IR path.
  - **The silver-leader IR limitation does not gate this path** (user, and correct — my
    first draft imported the caveat from `film-base/holder-masked-measurement` without
    re-checking that it applied here). Every consumer in this task measures *picture*
    frames: `auto_dmax` reads the frame being converted, and the roll-wide content `Dmax`
    direction excludes the leader by definition. The uniformly-opaque silver leader that
    IR genuinely cannot separate is never one of them, the leader is already judged
    undependable by `film-base/dmax-anchor-reliability`, and a dense B&W frame that
    declines just takes the fallback. Where the limitation *is* load-bearing is
    `holder-masked-measurement`, which measures `Dmax` on a leader.
  - **The existing IR mask has no depth**, which is the real work: `EdgeHolderMask` is
    segments *along* each edge at `IR_HOLDER_PROBE_FRAC = 0.005`, and the code says so
    explicitly. Excluding the holder from a statistic needs inward depth.
  - **Two cuts in order, not two alternatives** (user, correcting my first draft): IR
    removes the holder at whatever depth it measures, *then* a fixed inset removes the
    rebate from what is left. I had modelled them as either/or, both aiming at the holder,
    which made a 5% default look unsafe against a 10–15% holder. It is not: cut 1 already
    took it. **Default 5%, revisit with evidence.**
    **Correction (2026-09-13, review):** I then reconciled the two figures wrongly. I wrote
    that `analysis/conversion-metrics`' 10–15% "describes a single blind inset"; it does
    not — it is a **holder-occupancy measurement**, given as the reason a 5% inset fails.
    So on the **IR path** the user's reasoning holds exactly (cut 1 took the holder, cut 2
    only needs the rebate), but on the **no-IR path** the two figures genuinely conflict and
    the repo has already measured 5% as often insufficient. Real holder depth spans ~2–15%.
    Whether cut 2 takes a different default when cut 1 did not run is now an explicit open
    decision rather than a settled 5%.
  - **The genuinely open case is no IR at all**, where cut 2 runs alone and may under-cut a
    deep holder. Accepted rather than guessed: no measurement exists to pick a better
    number, and guessing large costs picture on every scan that does not need it. What
    makes it safe is the loud-failure requirement — an out-of-range `Auto` must refuse, not
    render black — so the two elements are load-bearing for each other.
  - **Do not port the all-holder decline into the depth-aware detector.** `ir_holder_mask`
    returns `None` when no film remains *along* an edge (22 of 25 real chromogenic frames),
    which is right for an along-edge mask and wrong for a depth-aware one, where the same
    reading just means the holder wraps the border — the normal case. Porting it would send
    those 22 frames to cut 2 alone, leaving 5% doing the holder's job.
  - Gained a dependency on `film-base/holder-masked-measurement` so the per-edge mask +
    fixed-fraction fallback has one owner rather than two drifting copies.
  - Recorded a naming trap: `Auto`'s "Dmax" is a *scene* statistic (this frame's brightest
    content), `Explicit`'s is a *film* property (the leader), and the holder is neither. A
    plausibility check must say which range it asserts instead of borrowing the leader's.


## sigmoid-parameter-calibration

**Status:** not started
**Updated:** 2026-08-03

- Goal: turn `reference-anchored-sigmoid`'s **provisional** parameters into calibrated ones.
- Provisional values and their firmness: contrast ≈2.07 (firmest — derived `0.745/Δ` from
  *tabulated* datasheet Δ); shoulder ≈0.6 (bends at `D′` 0.70 ≈ mid-grey 0.67, judged on ten
  frames); per-stock anchor offsets (**weakest** — from chart-read `D-min` values that are not
  true Status M densities, with systematic per-stock residuals of Ektar ≈+0.6 EV, Portra 160 ≈0,
  Gold 200 ≈−1.0); and the `NOMINAL_DMAX = 2.0` fallback (measured rolls 0.90–1.74, ≈1.35 better).
- **More frames will not fix this.** Per-frame exposure preference *is* frame optimisation, so
  only central tendency is usable. It needs a **bracketed roll** (exposure labels true by
  construction, which makes exposure-preservation verifiable rather than "consistent with"), a
  **grey card in frame** (a real 18% reference under the same illumination as a diffuse white —
  only 2 of 10 existing frames could even approximate the datasheet Δ), and ideally the
  calibrated transmission step wedge.
- 2026-08-13 (**cross-reference from `algo/exponential-anchor-placement`: the shipped
  `MidAtDmaxFraction(0.5)` has a quantified error, so this task starts with a number**).
  The mid patch sits at `D′ = 0.513` where `0.5·Dmax` puts the anchor at 0.650; that
  0.137 density is **0.91 stops** at contrast 2.0, against candidate 3's measured
  **0.93 EV** — i.e. the shipped default's whole residual is the fraction, and 0.395
  would be correct for these rolls. Two cautions before anyone simply re-picks the
  number. **(a)** 0.395 is only correct *while `Dmax` means the leader*; the fraction has
  no stable value because the thing it is a fraction **of** is a saturation density, not
  diffuse white. **(b)** `f` is also the coupling to that unreliable anchor — the two
  rolls 0.295 apart swing 0.98 stops at `f = 0.5` and zero at `f = 0` — so re-tuning the
  fraction fixes the systematic half and leaves the roll-to-roll half untouched. The
  calibrated answer is more likely a change of *reference* (to `Dmin` + offset, `f = 0`)
  than a better fraction. Full derivation in the `exponential-anchor-placement` section;
  the offset half is blocked per the note added to `film-stock-profiles`.

> Consolidation note (2026-09-13): the "PR #70 review: four findings" sub-entry that sat
> here belonged to `reference-anchored-sigmoid` (PR #70 was that task's PR) and was moved
> into that section. The `NOMINAL_DMAX = 2.0` figure above is historical — it became `1.3`
> on 2026-08-08 — and the chart-read `D-min` caveat was lifted on 2026-09-04
> (`film-stock-profiles`, curve-digitized Status M values); the task file is current on both.


## curve-endpoint-validation

**Status:** not started
**Updated:** 2026-08-08

- Goal: a closed-form, pre-decode warning when a resolved density curve cannot
  reach display white or paper black. Covers **both** curves. Ships no pixel change.
- Origin: raised 2026-08-08 by the user while reviewing the sigmoid anchoring
  section of the usage guide. The framing is theirs and it is the useful part:
  *this is a property of the configuration, and a configuration can be good or
  bad* — so it should be checkable, not discovered by eye.
- The argument, and the correction it survived. The initial claim was that the
  default sigmoid lets display white land *above* `Dmax`, which would mean
  fully-exposed film could never render white. **That was wrong** — a misreading of
  "lets white land above it" in `sigmoid.rs`, where "it" is the mid-grey
  placement, not the reference. Computed from the shipped formula
  `A = f·R + 0.745/contrast`, white sits **below** `Dmax` under the defaults:

  | R (Dmax) | A (white pt) | A − R | render @ Dmax |
  |---|---|---|---|
  | 1.2758 Gold 200 | 0.9979 | −0.278 | 0.939 |
  | 1.2933 Ektar | 1.0067 | −0.287 | 0.943 |
  | 1.3816 Portra 160 | 1.0508 | −0.331 | 0.959 |
  | 2.0 nominal | 1.3600 | −0.640 | 0.996 |

  But the *principle* held: `A ≤ R` is not guaranteed, it is a consequence.
  `A ≤ R ⟺ contrast ≥ 0.745/((1−f)·R)`, so at `f = 0.5, R = 1.3` any contrast
  below ~1.15 makes white unreachable. Good and bad configs both exist.
- Both shipped failure modes are the same two formulas at opposite ends:

  | config | A | s_curve(R) | floor | verdict |
  |---|---|---|---|---|
  | shipped mid@0.5, c=2.069 | 0.998 | 0.939 | 0.009 | ok |
  | mid@0.5, c=1.0 | 1.383 | 0.576 | 0.041 | white unreachable |
  | **old defect** white@Dmax, c=1.0 | 1.276 | 0.660 | **0.053** | floor too high |
  | white@Dmax, c=2.0 | 1.276 | 0.660 | 0.003 | ok |

  The 0.053 row reproduces the measured 72/255 shadow patch from
  `reference-anchored-sigmoid` — i.e. this check would have caught that defect from
  config alone, before the blind visual review did.
- **The default exponential curve has the same hole**, which is what widened the
  scope. Its floor is `10^(-gamma*Dmax)`: nominal `Dmax = 2.0` gives 0.010, but a
  leader-measured 1.2758 gives 0.053 and `--d-max 0.391` gives **0.407**. That last
  is a real invocation made while writing the usage guide — nc reported 18.15%
  clipping (an encode-side symptom) and said nothing about the floor.
- Two traps recorded so the implementer does not re-derive them: with
  `shoulder > 0` the sigmoid approaches 1.0 **asymptotically and never reaches it**
  (deliberate — it is what makes u16 highlight clipping impossible), so "must reach
  white" would fail every valid config; and `A ≤ R` is only a proxy, since the
  `white@Dmax` rows have `A = R` exactly yet render `Dmax` to 0.660 because the
  shoulder is already compressing. Test `s_curve(R)`.
- Warning tier, not a hard error: `--sigmoid-white-at-d-max` is retained precisely
  to reproduce the defect on demand, so refusing to render it would be
  self-defeating. `--no-d-max` (anchor 0.0 → floor formula yields 1.0) is the
  intended scene-referred mode and is exempt; `simple` has no curve and is out of scope.
- Boundary vs `algo/density-safety-bounds`: that task is per-parameter bounds plus a
  post-render histogram check; this is a closed-form pre-decode check on the *joint*
  endpoint behaviour. `contrast = 1.0` and `shoulder = 0.6` each pass any
  per-parameter bound — only their combination with the resolved `R` is broken.
- **Spec refined 2026-08-08 after Codex review of PR #83** (six P2 findings, all
  verified against the code and all real — the task file over-claimed in six ways):
  1. **Exponential has no black floor.** `10^(gamma*(D'-Dmax))` runs to 0 as
     `D' → -inf`. The meaningful quantity is the **film base endpoint**, and
     `D'base = density.offset`, *not* 0 (`D' = scale*D + offset`, base at `D = 0`);
     regional balance shifts it further. `10^(-gamma*Dmax)` is only the
     `offset = 0` special case.
  2. **The sigmoid floor must go through the shoulder.** `10^(-contrast*A)` is the
     pre-shoulder value. Equal at `shoulder = 0` and indistinguishable at the
     shipped 0.6 (0.008621 vs 0.008623), but `SIGMOID_KNEE_MAX` is **10**, and the
     naive form over-states the floor 1.75x at shoulder 3 and 5x at shoulder 5 —
     warning on configs that actually render deeper black.
  3. **The white check is reference *placement*, not reachability.** The sigmoid is
     monotonic and asymptotic to 1.0, so denser content always renders nearer white;
     no config makes white unattainable. `s_curve(R)` says the *reference tone*
     renders dim. Phrasing it as "white unreachable" would be false.
  4. **`--no-d-max` is an exponential-only exemption.** On sigmoid it is already a
     hard usage error ("the sigmoid curve needs a display-white anchor … only
     supported by the exponential curve") — verified against the binary. The old
     verification bullet ("emits nothing on either curve") was impossible.
  5. **`DmaxSource::Auto` has no pre-decode value.** `resolve_dmax` computes it
     inside `density::reconstruct` from post-balance densities, so `cli::validate`
     would evaluate a placeholder. Added a per-source table; Auto must be decided
     explicitly (skip or evaluate later), not by omission.
  6. **`core/pipeline-orchestration` added as a dependency** — it owns
     `cli::validate`, warning accumulation, roll handling and `--strict`;
     `density-safety-bounds`, `auto-neutral-wb` and `bw-support` all declare it.
     Still `[x]`, so the task stays immediately executable.
- **Spec corrected again 2026-08-08, review rounds 2–4** (appended rather than
  editing the entry above, which records what was believed at the time):
  - **The black endpoint is the reachable film base, not any idealized floor** —
    superseding item 2 above. Exponential has no floor; the sigmoid has one, but the
    base can sit far above it (shipped parameters: asymptote 0.0086, but
    `offset = +0.5` renders the base at 0.0923). Evaluate the curve at `D′base`,
    which is `density.offset` plus balance, **per channel**.
  - Deferral is **per endpoint**, and only when the balance range is genuinely
    consulted (`consults_balance_range` = `shadow_balance != highlight_balance`).
    Equal non-zero balances are pre-decode; `s_curve(R)` is pre-decode whenever
    `Dmax` is `Fixed`/`Explicit`.
  - The check judges **curve placement**, not the rendered image: `render_print`
    applies WB and exposure, subtracts `black_point`, then soft-clips, so a curve
    black of 0.053 is not the displayed black.
  - `algo/regional-color-balance` added as a dependency.


## exponential-anchor-placement
**Status:** done (filed 2026-08-08 as `exponential-mid-grey-anchor`; renamed and re-scoped 2026-08-12; mechanism 2026-08-18; measured 2026-08-28; merged 2026-08-29 as #98; closed 2026-08-31)

> Renamed from `exponential-mid-grey-anchor` on 2026-08-12 — the stem changed, so the old id
> resolves to nothing; live references were repointed. Filed *after* three sources had cited
> it as existing (a docstring, `reports/render-defaults-v2.md`, a log entry): naming a
> follow-up task in a docstring is a promise that the task file, checklist entry, dependency
> entry and graph node all exist.

**Why, and why the direction changed.** The exponential pinned display white at `Dmax`
with no placement rule, so contrast pivoted the line *around white*: gamma 2.0 took the
black floor 72 → 12/255 and cost 2.75 EV of midtone. The filed proposal copied the
sigmoid's mid pin; the user's proposal was to **pin black at the film base**, which
anchors the reliable measurement (base agrees to 0.0005 across rolls of one stock where
the leader `Dmax` is 0.295 apart) and is candidate 5b from `reference-anchored-sigmoid`.

**The 2.75 EV defect explained (2026-08-12/13) — the cause is the anchor, not the
formula.** A straight line placing mid at 0.18 *and* the base at 8/255 needs mid at
71.4% of the density range; it sits at 39.5%. Shortfall 0.415 density = 2.75 stops at
contrast 2.0 — the reported number reproduced from geometry. **0.415 is the
`Dmax`-to-diffuse-white gap, reached three independent ways** (geometric shortfall
0.415, measured diffuse-white gap 0.417, median exposure preference 0.452): display white
was pinned ~0.42 density too high. Contrast 1.0 only *looked* right on midtones because
two errors cancelled (anchor at `Dmax` −2.83 stops, contrast 1.0 +2.69, net −0.14).

- **`Dmax` is the wrong *quantity*, not merely unreliable:** a leader is film
  *saturation*; diffuse white is a scene object at ~90% reflectance, compressed by the
  film shoulder. Anchoring display white to saturation is a category error. A leader's
  better use — a development-variation signal — is uninvestigated.
- **Contrast is not a scene measurement.** `contrast = target_system_gamma /
  film_gamma`: C-41 ≈ 0.6, display intent ≈ 1.1–1.25, giving `1.2 / 0.6 = 2.00`; the
  datasheet route `0.745/Δ` gives 2.07 at Δ = 0.36. Two routes within 3%; the
  decomposition (how much is film vs target gamma) is not settled. Content-measured
  contrast is ruled out (a frame of a grey object has no range and no contrast should
  invent one). A derived contrast (pin both ends → 2.00 on a 1.3 roll with floor 0.0025)
  is *unexamined*, not rejected — the earlier "adaptive contrast, already rejected" had no
  recorded rationale — but it would route the anchor's unreliability into the slope.
- **The shipped sigmoid default carries the same defect:** `MidAtDmaxFraction(0.5)`
  puts mid at `D′ = 0.650` where it measures 0.513 — 0.91 stops, candidate 3's whole
  residual (0.93 EV); 0.395 would be right for these rolls. `f` is also the coupling
  strength to the unreliable anchor: white-pin swings 1.96 stops across the two rolls
  0.295 apart, mid-pin 0.98, black-pin **zero**. `0.5` was not arbitrary (chosen to halve
  the fallback's error) but the `pipeline_version` 3 default carries ≈0.9 stops of
  systematic midtone error plus ≈1.0 stop of roll-to-roll swing.
- **The placement *rule* must not branch on data availability** ("datasheet present →
  mid-pin, else black-pin"): it would make stock selection structural, and black-pin does
  not escape the missing offset, it defers it — black at floor 0.0025 resolves `A =
  1.301`, mid at `Dmin + 0.513` resolves `A = 0.886`, and the 0.415 between them is the
  picture rendering 2.79 stops dark unless something downstream supplies the same number.
  The right fallback is one mechanism, `Dmin + offset` with `f = 0`, and an offset that
  degrades per-stock → generic → empirical. Deriving a generic offset from the same ten
  frames and scoring it on them is fitting, not prediction.
- Black-pinning the sigmoid is not a gain swap (its toe asymptotes, so placing the base
  means inverting the S-curve); if mid-pin is the universal rule the sigmoid never needs
  a black-pin variant.
- The 2026-08-13 constraint that candidate 8's per-stock offset was blocked on a Status M
  `D-min` was **lifted 2026-09-04** (`film-stock-profiles`, curve-digitized values).

**Mechanism shipped (2026-08-18), no pixel change.** `AnchorPlacement` gained
`BlackAtBase(floor)` (`A = −log10(floor)/contrast`) and `MidAtBaseOffset(offset)`
(`A = offset + 0.745/contrast`); `ExponentialParams` gained `anchor`. Placement is
**shared by both curves** — orthogonal to curve shape, which is what let eight forms run
through one curve. **Default left at `white-at-dmax` on the exponential**, verified
byte-identical to HEAD (`params_hash 55a841428c1e6671`, drift gate unmoved); an archived
exponential recipe without `anchor` resolves exactly what it always did. CLI is one
curve-neutral family — `--anchor-mid-fraction` / `--anchor-white-at-reference` /
`--anchor-black-floor` / `--anchor-mid-offset` — with `--sigmoid-mid-fraction` /
`--sigmoid-white-at-d-max` as aliases (they appear in committed recipes); validation
moved to a shared block so the bounds apply to both curves. `--anchor-black-floor 0.005`
at contrast 2.0 resolves `anchor_value` 1.150515, matching the independent 2026-08-03
retest. The floor's sRGB equivalent is **16/255**, not the 20/255 that had been written
(that figure is the darkest confirmed shadow patch, a different quantity).

**Measured on ten real frames, nineteen configs (2026-08-28) — the rendering premise
failed; the mechanism stays as shipped.** `pipeline::shadow_metrics` extended
(per-candidate curve, toe/shoulder/`black_point`, a film-base probe, highlight metrics);
its arithmetic validates against the frozen report (white@Dmax 2.74 EV / 12 vs recorded
2.75 / 12).

- **The trade is three-way and every single-anchor form sits on one frontier.** Anchor
  1.293 → 0.906 gives |EV| 2.75 → 0.03, base 10 → 38, highlight separation (p90→p99,
  code values) 122 → 19, monotonically, no exceptions. Two-points-not-three, measured.
- **The toe does NOT pull the film base down — refuted.** Widening it 0.2 → 0.4 → 0.6
  moved the base 38 → 41 → 44: a toe is a soft approach to black *from above*.
  "Reconstruction places mid, a toe recovers black" is dead on arrival.
- **A display-stage black point does what the toe cannot:** `print.black_point = 0.019`
  over mid@base+0.508 gives |EV| 0.13 with the base at 1/255, dominating every other form
  on both axes; costs 0.16 stops of midtone; lands in the display stage so `film-master`
  keeps the unclipped rendering. (Later refined by `reconstruction-render-curve-split`:
  0.019 crushes 0.69–8.66% of every frame to code 0; ~0.005 is the largest safe fixed
  value.)
- **`GainMapMax` is controlled by the shoulder and nothing else.** Shoulder 0.6 →
  1.000x, 0.2 → 1.000x, 0.0 → **4.866x**; the exponential reads 4.866x under
  `white-at-dmax` *and* `black-at-base`. The sigmoid's shoulder runs during
  *reconstruction* and strips every above-white value before either display branch sees
  it, so SDR and HDR receive identical input. 4.866x is 98.8% of the 4.926 declared
  headroom — turning the shoulder off saturates the ceiling rather than buying graceful
  HDR (under the *old fixed-ceiling knee*; see the split task). `GainMapMax` lives in the
  second MPF image's XMP (`exiftool -b -GainMapImage`). **The figures are film-base
  dependent**: on `tests/fixtures/hdr-48bit.tif` the exponential default reads 4.866x at
  `--film-base 1,1,1` but 2.620x at `0.9,0.55,0.42`; base-derived anchors and every
  sigmoid row are identical under both. A headroom figure quoted without its film base
  is not reproducible.
- **The exponential is not competitive at any anchor — under the shipped knee.** At the
  sigmoid's own anchor (0.875) it blows 21.4% of every frame to white with zero
  top-decile separation (sigmoid 6.9% / 19 code values); at high anchors it converges on
  the sigmoid. User verdict: X1 (white@Dmax) has the best highlights in the set but is
  2.75 EV too dark; X2 (black@0.005, candidate 5b) is *dominated* by the shipped default.
  **Therefore no default moved**, on evidence rather than caution. **Rescoped
  2026-09-02**: the same straight line under the unbounded display operator at
  lightness-matched anchors measures 3.89–5.95% blown — the pairing failed, not the
  curve (`reconstruction-render-curve-split`). The user decided the exponential is not
  retired.
- **Three metric traps, all in a committed harness:** `sat%` (fixed 0.999 threshold, any
  `black_point` shift deflates it), `flat%` (measured against each frame's own maximum),
  and highlight separation as a **linear ratio** (inverts against visual review — sRGB
  spends more code values per stop higher up). Surviving pair: `blown%` (absolute ≥
  0.999) and separation in **code values**; both together match the eye. Still
  unmeasured: hard clip versus soft roll-off.

**Review rounds (2026-08-28/29) — three real behaviours, each a general lesson:**

- **Validate the resolved value, never a stand-in.** The anchor guard tested the proxy
  `MID_GREY_OUTPUT_DECADES / slope`, which bounds only the mid-grey rules; `black-at-base`
  divides the unbounded `−log10(floor)`, so `--anchor-black-floor 1e-45 --density-gamma
  1e-37` passed with a real anchor of `inf` and rendered an all-black frame at exit 0.
  `validate` now resolves `placement.anchor(reference, slope)` and rejects a non-finite
  result; `density::reconstruct` carries the same guard. **Sharpened in round 4: "does
  any intermediate overflow"** — a finite anchor still overflowed the *product*
  (`--anchor-mid-offset 2e38` at the default gamma 2.0 gives `slope · (d − anchor) =
  −inf`, `10^` of which is exactly `0.0`), so `slope · anchor` is checked in `validate`
  and at both render sites. Walk the arithmetic forward to the pixel. A large offset whose
  product stays *finite* still validates — bounding that is `algo/density-safety-bounds`'
  job, and a test pins the boundary.
- **Diagnose the more specific fault first, and never offer a remedy that does not
  work.** Ordering the anchor guard above the slope-positivity checks told
  `--sigmoid-contrast 0` "too small to place the anchor" and recommended
  `--anchor-white-at-reference`, which then failed the positivity rule.
- **`DmaxSource` describes the *policy*; whether it reaches the pixels is a property of
  the placement.** `master_places_dmax` keyed on `curve.dmax()` alone, so a base-derived
  anchor and an unanchored run emitted identical `film-master` provenance; `film-master`
  hard-rejected `--auto-d-max --anchor-black-floor 0.005` and `roll` warned "Dmax is NOT
  frozen" for a render that never read `Dmax`. All three now route through
  `AnchorPlacement::reads_reference()` (`MasterAnchor`: roll-fixed / base-derived / none).
  `curve.anchor` is emitted for both curves; `unpinned_curve` warns on an exponential
  recipe missing `anchor`. Six doc statements claiming the curve "pins white at `Dmax`
  with no placement rule" were corrected.
- **A curve switch silently discarded a stated `anchor`** (round 3, found by a third
  review engine). Both switch sites carry `dmax` and reset every other field — right
  while `dmax` was the only shared field, wrong once `anchor` became a second one. A roll
  pinning `black-at-base` with a per-frame `{"curve":{"type":"exponential"}}` rendered
  that frame on `white-at-dmax` and none of the roll warnings fired. **Kept the reset,
  made it loud**: `curve_switch_dropped_anchor` warns (roll-level, `--strict`-promotable,
  both CLI and roll paths) when the discarded placement was not the base curve's own
  default — carrying would make `--density-curve sigmoid` over an exponential recipe
  resolve `white-at-dmax` and strip the exponential of its straight line. **A comment
  asserting a closed set is a maintenance obligation**; grep for the assumption's
  *wording*, not its identifier.

**Closed 2026-08-31.** Filed to fix the 2.75 EV defect by pinning black; the mechanism
shipped and the *rendering* premise was refuted — a task whose stated fix is refuted but
whose measurements redirect two downstream tasks is a success, provided the negative
result is written down as loudly. Handed to `reconstruction-render-curve-split` (its
curve open again, its HDR question answered). The A-versus-B question — which stage
places the picture — belongs to that task and was not refiled; the enum carries both.


## reconstruction-render-curve-split
**Status:** done (filed 2026-08-10; started and closed 2026-09-02; review 2026-09-03; merged as #102)

**Filed** out of the `output/presets` review round: the reference-anchored sigmoid does
tone shaping *during reconstruction* (floor, midtone, shoulder), partly collapsing the
separate-sub-stages rule, and it is the same question as HDR headroom (on one Gold 200
frame the sigmoid's HDR rendition peaks at exactly reference white, `GainMapMax` 1.0x,
while the exponential reaches 4.87x). **The film is not the limitation** — negative
stock carries 10–14 stops; the *print rendering* decides whether output exceeds diffuse
white, which is why HDR is deprioritised rather than abandoned.

**Handoff from `exponential-anchor-placement` (2026-08-31):** the modified exponential
could not be the reconstruction curve *under the shipped knee* (21.4% blown at the
sigmoid's anchor); `GainMapMax` answers to the shoulder alone; the toe as a
black-recovery device is refuted; a display-stage black point does what the toe cannot;
and every single-anchor form sits on one measured frontier. Metric warning: `sat%`,
`flat%` and linear-ratio separation in `pipeline::shadow_metrics` all mislead; use
`blown%` and code-value separation.

### 2026-09-02 — chunk A: what reconstruction keeps, measured on seven frames

Scope agreed with the user: **verdict + shape + `film-master` reconciliation**; the
default migration split off (it inherits a calibration and a colour-model fix this task
does not own).

- **The split was already decided elsewhere**: `exponential-anchor-placement` ruled the
  exponential out (under the old knee) and `output/display-tone-mapping` closed on a user
  visual verdict for `s0-reinhard` — `--sigmoid-shoulder 0 --display-tone reinhard`,
  everything else default. So the reconstruction curve is *the shipped sigmoid with its
  shoulder set to zero*; the shoulder was the only genuinely print-side operation.
- **The anchor is a pure gain exactly when the shoulder is off**: `t − floor` is
  `contrast·d`, so the curve factors as `10^(−contrast·anchor) · h(d)`. Measured 5e-7
  relative deviation at `shoulder = 0` with the toe on, against 69–81% at the shipped
  0.6 (`algo::sigmoid::anchor_is_a_pure_gain_only_without_the_shoulder`,
  mutation-verified). Under today's default the exposure anchor and print shoulder
  interact — the stage collapse the fidelity rule names, measured.
- `shadow_metrics::reconstruction_shape_probe`: every candidate matched to the
  benchmark's **mean encoded lightness**, every row asserting the render hit the
  statistic it solved for (guard verified to fail).
- **All five shapes beat the shipped sigmoid on both metrics on all seven frames** —
  35/35 rows on `blown%`, 33/35 on separation (exceptions: the deepest black-point rows
  on P4).
- **The toe is dead.** Toe 0.2 against 0: `blown%` identical to two decimals, separation
  within 0.2 code values, and the floor consistently **lower without it** (E3 27.9 →
  25.9, P3 27.0 → 25.3, P4 22.8 → 20.9). Reconstruction should keep neither knee.
- **So the split's endpoint is the straight line** — `toe = shoulder = 0` is bit-exactly
  the exponential (`convert_with_knees_off_matches_exponential_bit_exactly`). Not a
  contradiction of `exponential-anchor-placement` but a **rescoping**: under the
  unbounded operator at lightness-matched anchors (0.954–1.030) the same curve measures
  **3.89–5.95% blown with 10.9–120.1 code separation**. The curve was never the problem;
  the pairing was.
- **`black_point = 0.019` is refuted at that value, and it took a fifth metric to see
  it** — neither |EV| nor a floor percentile can see *crushing*:

  | black point | floor (code) | crushed% | separation cost |
  | --- | --- | --- | --- |
  | 0     | 20.9–30.7 | 0.00 | — |
  | 0.005 | 10.4–23.2 | **0.00 on 7/7** | 1–4 |
  | 0.010 | 0.0–14.8 | 0.00 on 6/7, **P4 2.07** | 2–7 |
  | 0.019 | 0.0 | **0.69–8.66** | 5–14 |

  A fixed black point cannot be pushed past ≈0.005 without crushing some frame; the
  linear subtraction trades floor against crushing ~1:1, so it is a per-frame grading
  control, not a default. `crushed%` joined the harness (the fifth metric trap after
  `sat%`, `flat%`, linear-ratio and unclamped separation).
- **The anchor placement cannot be adjudicated by this probe.** Matching to a common
  lightness *solves* the anchor, and the anchor is a pure gain, so every placement
  converges on identical pixels (`MidAtBaseOffset(0.626)` and `MidAtDmaxFraction(0.5)`
  agreed to four decimals). The bias column is confounded (the target is matched to a
  `Dmax`-reading render); the uncontaminated spread is identical by construction — 0.07
  EV (Gold), 0.10 (Ektar), 0.32 (Portra), frame-to-frame exposure disagreement no
  single-anchor rule can remove. Adjudication needs a grey card on a bracketed roll
  (`algo/sigmoid-parameter-calibration`'s precondition).

**Shape verdict:** reconstruction keeps the density conversion, the contrast and the
anchor, and sheds **both knees**; character comes from the display operator; black
placement is a display-stage control well below the value previously measured.

### 2026-09-02 — chunk B: `film-master` reconciled; the "sharpest constraint" dissolves

`film-master`'s contract was never a curve shape: `render_split::film_master` is
`aces.into_linear()` — a pure unwrap, no range check, no `PrintParams` — and the branch
already varies with every curve knob. The `TASKS.md` rollup sentence naming the sigmoid's
toe/midtone/shoulder had stated the *current default's* shape as if it were the
definition; reworded. Verified on `tests/fixtures/hdr-48bit.tif` (`--film-base 1,1,1`):

| reconstruction | anchor | clipped hi/lo | non-finite | mean RGB |
| --- | --- | --- | --- | --- |
| default (shoulder 0.6) | 1.010 | 0 / 0 | 0 | 0.65 0.84 0.95 |
| `--sigmoid-shoulder 0` | 1.010 | 0 / 0 | 0 | 3.81 5.15 30.15 |
| that, `--sigmoid-toe 0` | 1.010 | 0 / 0 | 0 | 3.81 5.15 30.15 |

The branch accepts a shoulder-less reconstruction cleanly; the master's values change
materially at the same anchor (the pairing chunk A solves); the toe changes nothing at
this boundary either. The report needs no change — `output_render.content` names
"density curve" generically, so no prose becomes false (the fifth-spot trap avoided).
**After the split `film-master` is the film's density record — density conversion,
contrast and anchor, carried unclamped into ACEScg with no print decision baked in** — a
*better* master by the fidelity rule. The one consequence: it becomes unbounded above
1.0, which is already its documented contract.

### 2026-09-02 — closed: the verdict, and what the split does to HDR

1. **The split holds**, measured on seven frames at matched mean encoded lightness.
2. **`film-master` reconciled** explicitly (chunk B).
3. **HDR** — the whole difference between a live gain map and an inert one (numbers
   from `output/display-tone-mapping`'s HDR review):

| | shipped default | split (`s0` + unbounded tone) |
| --- | --- | --- |
| gain map | inert, `GainMapMax` **1.000x** | live |
| frame on the top gain code | **6.6-15.2%** (shouldered) / 92.68% (default) | **0.26-0.61%** |
| above reference white | 0% by construction | 7-26% |

`GainMapMax` is the wrong instrument (4.87x vs 4.79x for the two live configs, identical
on all four review frames); the plateau share is the row that matters.

**Not done here, deliberately:** no default moved, `pipeline_version` untouched, drift
gate quiet. Activation is `algo/split-default-migration`. (At close-out it was recorded
as blocked on `film-base/dmax-per-channel-reduction`, on the reasoning that the shoulder
hides a 17-83% off-neutral leader reading; that edge was **removed 2026-09-10** when
`film-stock-profiles` disqualified the leader as a per-channel source — see the
migration task's section.) Nothing in `src/` outside a test module changed.

### 2026-09-03 — the fourth quadrant: `shipped sigmoid + --display-tone none`

`output/linear-render`'s pairing had never been benchmarked against the split (each had
only been compared to the common baseline). Added as a row (no exposure matching — it
shares the benchmark's reconstruction, and with `shoulder > 0` the anchor is not a gain).
Seven-frame means:

| config | blown% | code sep | sep vs benchmark | `GainMapMax` |
| --- | --- | --- | --- | --- |
| shipped sigmoid + Hermite (today) | 6.52 | 49.0 | — | **1.0000x** |
| shipped sigmoid + `none` | **4.95** | 49.9 | **+0.9** | **1.0027x** |
| `s0` + reinhard (the split) | **4.93** | 61.5 | **+12.5** | **4.7929x** |

The two tie on `blown%` and are not close on anything else: `none`'s separation is
unchanged from the benchmark on five of seven frames, because the reconstruction shoulder
fuses highlights in density space upstream of everything; skipping the display tone stops
values being pushed to 1.0 but cannot recover collapsed spread. The HDR half is settled by
construction (`shoulder > 0` keeps `lin ≤ 1.0`; the 0.0027 is the SDR branch losing its
shoulder, not headroom). It remains available as a **conservative interim default** —
shipped, byte-identical below the knee, ~80% of the `blown%` win, at the cost of forgoing
highlight separation and HDR. A scheduling option, not a destination.

### 2026-09-03 — review round: one real output bug behind five green gates

- **The `bias EV` column printed the wrong sign** — the comment dropped a minus, and the
  code implemented the comment. Magnitudes and `spread`/`worst` were unaffected, but the
  direction was wrong wherever read: the shipped placement anchors *above* the solved one
  and renders **0.21–0.28 EV darker** (`split-default-migration`'s open question now says
  so). The sign convention is stated at the definition.
- The `linear-render` row `unwrap`ped the one fallible render in the loop
  (`DisplayTone::None` refuses any sample above reference white, and the film-RGB →
  P3-luma functional sums to 1.0000000468, so a near-white pixel can round over); a
  refusal is now a result for that row, not a reason to lose six frames.
- Prose no gate reads: two stale task-owner references (`output`'s epic summary and
  `types.rs`) and this task's `How to Verify` bullet 2 being half-met without saying the
  version bump had moved downstream. One remedy declined — the branch had zero commits, so
  two misfiled entries were moved rather than pointed at (append-only protects what others
  may have read; it is not a reason to make a filing mistake permanent).


## contrast-latitude-spike

**Status:** not started
**Updated:** 2026-09-15

- Goal: decide whether nc's tonal latitude should change — which end, which mechanism,
  or not at all.
- 2026-09-11: Filed out of the first measured nc-versus-NLP numbers, which live in
  `docs/progress/analysis.md` under `analysis/nlp-comparison` (including the correction
  that resolved the reference colour space — the NLP files are **linear** sRGB, and every
  figure derived from the earlier "gamma reading" is superseded). A spike rather than a
  task because the scene range was never measured, so the cause of the gap is open.
  Read the §3.8 principle before starting: the per-roll recipe is deliberate, so a
  narrower range may be the design working rather than failing.
- 2026-09-15: **Evidence that NLP fits each frame's extremes to the output range — and the
  earlier rejection of that model tested the wrong statistic.** Reviewing 43 frames by eye
  against NLP, the user reported that NLP collapses on frames filled by a single surface
  (all water, cloudless sky), losing almost all information —
  `rolls/2026-09-09-Ektar100/1612.tif` — while every
  nc config holds them. The task file had dismissed "NLP normalises each frame" because it
  predicts a near-zero `p95 − p5` spread against the measured 4.31; that inference holds only
  for a stretch fitted to `p5`/`p95`, not to the extremes, where such a spread is **compatible
  with** the model rather than predicted by it. The objection falls away; that is not the same
  as support, since the model permits identical spreads too and the input distributions are
  unmeasured. Recorded in the task file's second open question together with the
  regression that would settle it (output extremes against the negative's own extremes, and
  whether NLP's gain rises as the scene range narrows).
  Corrected on review, 2026-09-16: the single-surface frame is an **observation to explain,
  not positive evidence** for the model. A monotone stretch of a narrow interval onto the
  full output increases sample separation and destroys no information by itself; losing
  detail needs a further stage — end clipping, post-stretch quantization, or a nonlinearity —
  and which one is unidentified. Both must be measured, or the spike picks a remedy for a
  mechanism it never located.
- 2026-09-15 (caveat for whoever measures): the **"32 pixel-aligned Ektar pairs"** this task
  plans to regress are now **12**. At the user's request 20 of the 32 `rolls/2026-09-09-Ektar100`
  frames were deleted as near-duplicates; their NLP outputs survive, so those 20 have no source.
  `2026-09-11-Portra400` is down to 11 of 32 and `2026-09-13-Portra400` to 10 of 36. The paths in
  the task file also predate the roll rename (`converted/nlp/2026-09-09-Ektar100/`,
  `rolls/2026-09-09-Ektar100/`).


## split-default-migration

**Status:** not started
**Updated:** 2026-09-10

- Goal: make the reconstruction/render split the shipped default — a
  `pipeline_version` bump with a before/after report. See
  [the task file](../tasks/algo/split-default-migration.md).
- Filed 2026-09-02 out of `algo/reconstruction-render-curve-split`, which reached a positive
  verdict but scoped the migration out deliberately: it inherits a colour-model fix the split
  does not own. Read that task's four entries above first — chunks A and B carry the measured
  shape (both knees off), the `film-master` reconciliation and the black-point bracket; the
  close-out carries the HDR outcome; and **the fourth-quadrant entry is direct input to this
  task's scheduling**, since `shipped sigmoid + --display-tone none` is an interim default
  that reaches ~80% of the `blown%` win *without* this task's per-channel blocker.
- **The blocker is real, not bookkeeping.** `film-base/dmax-per-channel-reduction` must land
  first: the shoulder this migration removes is what currently *hides* a 17-83% off-neutral
  channel error on the grey leader, so a shoulder-less default ships a visible cast on Gold
  and Portra.
- **2026-09-10 — that blocker is withdrawn; the dependency is removed.** Both halves of the
  bullet above died in `algo/film-stock-profiles`. The grey leader is disqualified as a
  per-channel source (measured leaders do not reproduce the published divergence, and the
  comparison cannot separate a non-neutral leader exposure from a scanner-slope error), and
  the per-channel term turned out to be a **slope** carried by `density.scale` and by
  `characteristic`'s own tables — not the anchor that task weighs. `characteristic-generic`,
  the proposed default, has neither a scalar `Dmax` nor a per-channel gain. What gates this
  migration now is the **green residual** (+0.40 mean, +1.00 on the Ektar roll), which no
  per-channel scale removes. `io/scanner-density-calibration` is the new edge, but it is
  necessary rather than sufficient — its known-neutral tier is optional there — so the
  binding condition is the neutrality release gate in the task file's `How to Verify`.
- 2026-09-12 (**rescoped after review with the user**): retitled *Make
  `characteristic-generic` what a bare `nc convert` resolves*.
  - **The goal did not change; what activation *means* did.** Since `algo/conversion-presets`
    shipped, `--preset characteristic-generic` already expands to the target rendition, so
    this task is making that the no-flag state — not rewiring. The user's observation that
    the default has already moved per curve, per stock, per bundle and per output preset is
    what surfaced this: the only thing left unmoved is the no-flag resolution, and `--preset`
    is `Option<String>` with no default.
  - **The four dependencies are not the same kind of thing.** Three are constructive —
    `reconstruction-render-curve-split` (the verdict), `conversion-presets` (the mechanism),
    `characteristic-curve-coverage` (pinned wiring). The fourth is a **gate on a different
    axis**: the goal is where tone shaping happens, the gate is per-channel colour neutrality.
    The split does not create the green residual — it exists today — it makes it more visible,
    because the shoulder being removed compressed the highlights where the cast lives.
  - **The gate edge now points at `analysis/calibration-frame-capture`.** It pointed at
    `film-base/dmax-per-channel-reduction` until 2026-09-10 and at
    `io/scanner-density-calibration` until today; both were "necessary, not sufficient",
    because a green checkbox on either is reachable without the known-neutral reference ever
    being measured. This closes the gap the task file had flagged and explicitly declined to
    close unilaterally.
  - Nothing about the fingerprint-portability hazard changed; it is still the single most
    important thing to read before writing a `PIPELINE_FINGERPRINTS` row.


## characteristic-curve-coverage
**Status:** done (filed and closed 2026-09-10; #107)

The curve shipped with its *tables* well covered and its wiring barely covered — nothing
asserted what `to_density → check_tables → apply_curve_per_channel → FilmRgbImage`
produces. Closed with two complementary pins, and a correction to the premise.

**The premise was half wrong, and so were the first two attempts to say how.** "A
bit-exact capture is not available because `10f32.powf` differs ~1 ULP across libm" is
true of particular *values*, not of the mechanism. Three designs, each falsified by
evidence:

1. **Threshold from "computed in double, then rounded"** — claimed divergence only within
   `2^-5` of an f32 ULP from a rounding boundary. Wrong arithmetic: one f64 ULP is
   ~`2^-29` of an f32 ULP, so the real figure is ~`2^-27`, seven orders tighter (one
   quantity converted to a relative error twice). Caught in review.
2. **Threshold from glibc's published `powf` error** (0.52 ULP) applied to both libm
   calls. Unsound: a bound for one function does not transfer to another, and neither
   target documents `log10f`. x86_64 CI disagreed with this host on sample 1 (margin
   0.0153) *and* on **sample 9, margin 0.456 ULP — twenty times the threshold that called
   it safe**; the golden passed only because each disagreement fell where the curve
   flattens.
3. **What shipped: enumeration.** `reachable_window` renders every density a libm within
   1 ULP can return (`d.next_down()`, `d`, `d.next_up()` around the correctly-rounded
   value), takes the widest excursion from the captured pixel, and adds one ULP for the
   curve's own `10^`. Any conforming libm is inside by construction.

| samples | window |
|---|---|
| 0-2, 12-14 (near-base shadow, film base) | **1 ULP** |
| 3-5 (midtone) | 4-5 ULP |
| 6-8 (dense highlight) | 7-9 ULP |
| 9, 11 (out-of-range, extrapolated) | 27, 38 ULP |
| 10 (out-of-range green) | **63 ULP** |

Nine of fifteen are effectively bit-exact; the window opens where a 1-ULP density
difference is amplified by `ln(10)·d·(1/γ_local)`. The smallest real fault measured moves
these pixels 115,549 ULPs. `MAX_REASONABLE_WINDOW_ULPS` reports a future vector landing
somewhere steeper (`PORTRA_160_B` has a segment with `1/γ = 767`; `check_tables` bounds
slope by nothing — not worth an invariant today, worth knowing before trusting an
analytic bound). **One rule generalises:** assert *conformance* (within 1 ULP), never
correct rounding — both dead designs asserted the host's libm was correctly rounding.
Three modelling traps, each producing a plausible wrong number: dividing scan by base in
f64 when `to_density` divides in f32 before the `log10` (5 ULPs out); dropping the
identity gain and zero offset as a no-op (`+ offset` normalises the film-base pixel's
`−0.0` to the stored `+0.0`); taking the ULP width from `bits + 1` at a binade boundary.

**px4 is the one to carry forward:** its corrected density is exactly `0.0` (it *is* the
film base), so the rendered value is the constant `10^(table[0].0)` — a property of the
shipped table literal with a 0.006 ULP margin. If this curve becomes the default that
margin reaches every base-density pixel of every frame.

**Pin 1 — properties, in `algo::film_stock::tests`**, running the real
`algo::reconstruct` over a synthesized scan: a neutral ramp round-trips on all ten stocks
(relative error 4.8e-7); the published mid-grey reconstructs to 0.18 with `dmax` and
`curve_anchor` both absent; stages 1-2 are `scale·d + offset` not `scale·(d + offset)`
(needs an explicit non-neutral pair); the reported `out_of_table` fractions match a
recount and out-of-range samples render *outside* the endpoint exposures. Plus an
integration test that an off-table render warns and `--strict` refuses it, with a
sane-base control.

**Pin 2 — bits, in `pipeline::stages::golden`:**
`golden_characteristic_is_correct_within_its_libm_window` and
`the_characteristic_capture_is_correctly_rounded_and_the_host_conforms` (each captured
constant is what a correctly-rounded f64 chain produces; the running host's `log10` is
within 1 ULP). What was there before: one point (mid-grey, one stock, red only, ±0.01)
as a side effect of `midtone_placement`.

**Falsifiability matrix (2026-09-10), measured rather than argued** — each perturbation applied to the shipped code, full `cargo test` run, reverted (new tests in bold):

| perturbation | caught by |
|---|---|
| `scale·(d + offset)` transposed | **the ordering test**; also `to_density_applies_scale_then_offset` + two parametric goldens |
| red's table on all three channels | **ramp, mid-grey, ordering, the golden**; `midtone_placement` |
| shared (red) film base | 24 tests — **all four properties, both new golden tests**, and every parametric golden |
| channels permuted at stage 3 | **ramp, mid-grey, ordering, the golden**; `midtone_placement` |
| clamp instead of extrapolating below | **the `out_of_table` test, both new golden tests**; `out_of_table_extrapolates_and_reports` |
| count pass drifts from the render | **the `out_of_table` test and the golden — nothing else** |
| one table literal moved by 1e-6 | **both new golden tests**; `curves_match_the_digitized_json` |

Re-run after the window design changed, and it earned its keep: the first
`reachable_window` measured distance *from the captured value*, so the table nudge
inflated the window as much as the drift and the golden passed on a frame whose every
pixel had moved 115,523 ULPs. A window that depends on the value under test is not a
window; it now measures the reachable set's own spread. The transposition is the case the
golden provably *cannot* see; the counting drift the case only the new tests see.

**`version::PIPELINE_FINGERPRINTS` is deliberately untouched — the handoff.** The gate
fingerprints the *default* render; adding a column would force values into historical
rows for behaviour those builds never emitted. When `algo/split-default-migration` moves
the default here it bumps `PIPELINE_VERSION` and records a new row, and the margin harness
decides whether that row's `render` hash is portable: the drift gate hashes raw f32 bits
with no window, and px4's 0.006 ULP margin says the answer is not automatically yes.


## conversion-presets
**Status:** done (filed 2026-09-09; shipped 2026-09-10 as #110)

`--preset` names five reconstruction + display bundles (`characteristic-generic`,
`characteristic-stock`, `characteristic-aim`, `sigmoid-knees`, `sigmoid-flat`), all
calibrated to one brightness target. The scoping evidence is the 2026-09-09 entry under
`film-stock-profiles`; this section is the build.

- **Precedence is `defaults < params < preset < flags`.** The proposed
  `preset → --params → flags` (a preset as *defaults* under the recipe) is inert: `nc
  params` / `--dump-params` write every key explicitly, so a preset beneath any recipe nc
  produced has nothing left to set.
- **A preset is not a recipe key.** `--dump-params` writes the expanded values, a recipe
  naming a preset is rejected by `deny_unknown_fields`, and the name survives only as the
  report's `conversion_preset` provenance. Three reasons: a re-expanding key would render an
  archived recipe differently on a build whose definitions moved (the drift
  `PIPELINE_FINGERPRINTS` exists to prevent); a bundle spans `reconstruction` *and*
  `print`, so §9 has no home for it; and `roll` needs no new surface — the dumped recipe
  replays exactly. A documented exception to "every knob is a flag and a recipe key",
  narrower than the operational ones: a preset is not a knob, it only sets knobs that are
  already both.
- **The report carries two diffs.** `conversion_preset.overridden` lists recipe paths
  whose resolved value a *flag* moved off the preset's expansion (so `--preset
  characteristic-aim --density-curve sigmoid` cannot report a preset it did not render).
  **`overridden` is empty by construction exactly when the preset itself replaced a recipe
  value** — so a second list, `replaced` (preset-owned paths where the render differs
  from the loaded recipe, computed only when `--params` was given), is what surfaces a
  recipe pinned to `ektar-100` silently rendered on `generic-c41`.
- **The aim-matched red scale is derived** — `algo::film_stock::aim_red_scale` computes
  `rise / Δ` (the reciprocal, since `--density-scale` multiplies the scan's density),
  sharing `AIM_SEPARATION_DECADES` / `usable_aim_delta` with
  `aim_table_agrees_with_the_curve` so a sheet cannot be correctable by one and unchecked
  by the other. The review script's three hand-written constants are gone.
- **Four refusals, each with a remedy that was run:** `--film-stock` beside a stockless
  preset (otherwise `characteristic-generic --film-stock ektar` silently becomes
  `characteristic-stock` at the wrong exposure); `--film-stock generic-c41` under the
  stock presets; `characteristic-aim` on `portra-800`/`ultramax-800`; `--print-exposure`
  on `sigmoid-knees` — refused *by preset*, not by combination, since the underlying pair
  (`--display-tone none` + positive exposure) has no general rule and must not gain one
  (dark enough content renders at exit 0). Each message prints **that preset's**
  accepted-stock list, not `FilmStock::ALL`.
- **Ordering bugs found by testing, not reasoning.** The preset arm ran before
  `--reconstruction`, so `--preset X --reconstruction simple` rendered a `simple` frame
  carrying the preset's exposure and tone under the preset's name — now it runs after.
  The `--film-stock`-beside-stockless-preset rule lived in `validate_convert`, after
  `merge`'s own `--film-stock` arm had already refused with "pass `--density-curve
  characteristic`", whose remedy then landed on the preset rule — a two-step
  contradictory diagnosis; the rule now runs inside merge's preset arm, and its test goes
  through `merge` and asserts the *losing* rule's wording is absent.
- **`reconstruction.curve` is one recipe path but six knobs, and `dmax` is a roll
  calibration, not a look** (the most serious defect this task produced, found by the ship
  review). `*curve = expansion.curve` replaced the whole object including `dmax`, so
  `--params roll.json --preset sigmoid-flat` resolved `fixed` with zero warnings and
  `overridden: []` on every frame of a roll, while a same-type `--density-curve` switch
  carried it. Fixed by `cli::preset_curve`, which carries `dmax` on exactly the condition
  the `--density-curve` arm uses (both sides `takes_dmax()`); an explicit `--d-max` still
  wins; both diffs share the helper. Pinned by falsifiable tests. CLAUDE.md records the
  trap.
- The curve-switch warnings are suppressed when a preset is named (the replacement is the
  user's own request, and `replaced` reports it) — guarded by a binary-level test that
  also asserts the gain really was replaced.
- **Acceptance:** the preset-review set rendered through `--preset` is byte-identical to
  the flag expansion — **12 of 15** on the three-frame set, then **40 of 50** on the full ten-frame set; all ten exceptions are `chr-aim`, whose
  constant became the exact derivation (Ektar 1.1133202 vs the script's 1.114, etc.). On a
  lossless 16-bit `display-p3` TIFF the true difference averages 0.023 of an 8-bit code
  value (max 8.1, where the inverted curve is steepest); the JPEG figures (up to 19/255)
  are DCT re-quantization. `scripts/preset-review/generate.py` now drives `--preset` and
  states no constants (later replaced by `scripts/preset-review/presets.matrix.json` +
  `nctool review generate`, `analysis/comparison-review-tooling`).
- **No default moved:** `PIPELINE_VERSION` stays 4, all three fingerprints unchanged.
- **Not done, deliberately:** `nc roll` gains no `--preset` (it has no override flags at
  all — `core/recipe-composition`'s scope; the dumped recipe reaches a roll exactly). The
  headroom question (`reinhard` at 4 stops rather than 6) is untouched.
- **For `algo/split-default-migration`:** its constructive deps are all `[x]` and the
  preset machinery including its brightness calibration is in place, so the migration is
  a `pipeline_version` bump plus a golden recapture, not new CLI surface. (The close-out
  on 2026-09-10 named `io/scanner-density-calibration` as the one remaining blocker; since
  2026-09-12 the gate is `analysis/calibration-frame-capture`.) Note #107's warning that
  the fingerprint row gets no per-sample window.
- Three doc/prose fixes of the class no gate reads: a rustdoc claiming a sharing that never
  happened, a citation that established the *direction* cited as per-stock evidence, and
  `using-nc.md` saying "four combinations are refused" where the design lists five.

### 2026-09-15 — the shared brightness target moves up one stop

**Scene mid-grey 0.18 is now delivered at 0.4525 (+1.33 stop), replacing 0.223 (+0.31).** The
first change to it since it was approved on 2026-09-09. Every preset was re-solved against the
new one, so switching preset still changes the look rather than the brightness — on the
calibration stock; see the 2026-09-16 entry below for what that does and does not claim.

**Provenance.** The user reviewed 43 frames x 5 presets rendered one stop above calibration and
judged +1 stop better on all five — hence the target moved rather than one preset leaving it.
Offered anchor 0.30 (the +1.0 stop measured on real-frame median luminance, i.e. the rendering
actually reviewed) against 0.28 (the harness's exact +1.00 on its synthetic mid-grey, 0.13 stop
brighter), the user chose **0.28**.

**New values.** `ConversionPreset::ANCHOR_MID_FRACTION` 0.42 -> **0.28**; `print_exposure`
`characteristic-generic` 0.39 -> **1.91**, `characteristic-stock` 0.31 -> **1.82**,
`characteristic-aim` 0.31 -> **1.59**, `sigmoid-flat` 0.61 -> **2.17**. Delivered: −0.051 /
−0.051 / −0.030 / +0.005 / −0.056 stop off target, **tighter than the previous shipped spread**
(max 0.108).

**Why the exposures rose further than the target did.** Reinhard returns only ~73% of a
post-curve gain — measured here, +0.89 nominal bought +0.652 delivered — so the four toned
presets need roughly 1.5x the nominal move. `sigmoid-knees` takes no exposure knob at all and
moves by the anchor, which is 1:1. The asymmetry is why a single "+1 stop for everyone" flag
sweep does *not* keep the family aligned.

**Caveat worth acting on before this is called settled.** The three characteristic presets now
ship **brighter than anything reviewed by eye**: in the reviewed set they carried a nominal +1.0
and so landed ~0.5 stop below `sigmoid-knees` once Reinhard took its share. Putting all five on
one target lifts them to match. Worth one confirming look.

**Not a `pipeline_version` bump.** Presets are CLI-only expansions; the default recipe is
untouched and `PIPELINE_FINGERPRINTS` is unaffected. All four gates green (755 unit + 191
integration passed, 23 ignored, zero clippy/build warnings).

**2026-09-16 — the target is a calibration convenience, not an invariant: the claim was fixed,
not the constants.** Review asked why `characteristic-aim`'s single exposure was solved only on
`portra-400`, since the same harness delivers 0.3606 on `gold-200` (−0.33 stop) and 0.3162 on
`ultramax-400` (−0.52). It reproduces exactly — but it is **pre-existing and not confined to
that preset**: `ultramax-400` was already −0.408 under the old constants, and `sigmoid-knees` on
`gold-200` is +0.443. `characteristic-stock` is the only one that lands identically on every
stock (0.4367), and only because it inverts the very curve the patch is built from, so the
per-stock spread measures reconstruction accuracy rather than a miscalibration. The user's
ruling: rendering the five presets alike — mid-grey included — **was never a goal**; the only
goal they share is a satisfying result, and that goal itself has room to move (whether a stock's
colour cast is character worth keeping is the live case). So the prose stops claiming an
invariant, no per-stock exposure is introduced, and the test asserts the calibration stock while
**printing** the other eight — the spread is now documented rather than bounded.

`presets_land_the_calibration_target_on_the_calibration_stock` (renamed from
`every_preset_lands_the_shared_brightness_target`) collects every row and asserts once at the
end. A per-row assert hid the other four presets behind the first miss, which is precisely the
table a recalibration needs. Twelve call sites and prose references were updated with the
numbers, including `design-spec.md` §"Named conversion presets", `using-nc.md`, this task's file,
`scripts/preset-review/` (README + matrix) and the test that pinned `sigmoid-flat`'s old 0.61.
Review sets already rendered in `../temp` passed their flags explicitly and are unchanged;
re-running `presets.matrix.json` now renders the new brightness.


## characteristic-default-audit

**Status:** not started
**Updated:** 2026-09-13

- Goal: find and fix what breaks when `characteristic-generic` becomes the no-flag default,
  before the default moves. Executable now — it does not wait on the calibration frames.
- 2026-09-13 (filed, from a user question): `algo/conversion-presets` is `[x]` but its Goal
  claimed "`characteristic-generic` becomes the default", which never shipped — `--preset` is
  `Option<String>` with no default. Verified by running the binary: a bare `nc convert`
  resolves `curve.type: sigmoid` (toe 0.2 / shoulder 0.6), `density.scale [1, 0.90, 0.86]`,
  `display_tone: shoulder`, `print_exposure: 0`, `output_render.preset: gain-map-hdr`,
  `conversion_preset: null`. The Goal line and two copies of it were corrected.
- **Chasing that turned up real work, not just stale prose.** Measured on the binary: three
  validation rules key on the **resolved curve** rather than on flag presence, so a default
  move flips them for users who typed only the flag —
  `--d-max 1.3`, `--auto-d-max` and `--sigmoid-toe 0.2` each go exit 0 → exit 2, with a remedy
  telling the user to "pass `--density-curve sigmoid`" for a default they never chose.
  `--d-max` is the documented roll-calibration workflow (design-spec §8). The mirror case:
  `--film-stock portra-400` alone currently errors with "the resolved curve is sigmoid — pass
  `--density-curve characteristic`" — whose *no-flag* path flips to success, while the guard
  itself stays reachable from any explicit parametric curve (`--density-curve sigmoid
  --film-stock portra-400` is exit 2) and must not be deleted: `src/cli.rs:3603` exists to stop
  the flag being a silent no-op.
- **Decided against reopening `algo/conversion-presets`** (recommendation, user's call): its
  scope was the five bundles plus the expansion, and that shipped and works. Reopening it
  would put the default move under two owners, since `split-default-migration` already holds
  the version bump and fingerprint row.
- **Decided against folding this into `split-default-migration`**: that task is gated on the
  calibration frames, and this half is executable today. Every break found now is cheaper than
  one found during a version bump. Filed as its own task with an edge into the migration.
- The organising distinction is **presence versus resolved value** — the same one CLAUDE.md
  records for output-preset atomicity. A presence-keyed rule is unaffected by a default move;
  a value-keyed rule changes meaning for everyone who typed nothing.
- Likeliest regression to check first: bare `--output-preset film-master` is exit 0 today,
  while `--preset characteristic-generic --output-preset film-master` is exit 2. The
  "a preset must not set `output.preset`" escape has to survive the default move.

## characteristic-fingerprint-vector

**Status:** not started
**Updated:** 2026-09-13

- Goal: a `PIPELINE_FINGERPRINTS` `render` vector that is bit-identical on both CI
  targets under the `characteristic` curve. Split out of `split-default-migration`
  because `characteristic-curve-coverage` observed x86_64 and macOS disagreeing in
  `log10f` on two of the fifteen golden samples, and a fingerprint has no ULP window.

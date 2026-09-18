# Negative Converter — film-base Progress Log

Execution log for the `film-base` epic: what was done and how, key decisions, what
works, what doesn't. TASKS.md holds the authoritative status (the checkboxes);
this file is the narrative beside it.

One `##` section per task in this epic, named by the bare task name (the part
after the `/`). Read this whole file before starting a task in this epic, and
read other epics' `Epic summary` sections when you depend on them. Append
entries — don't rewrite earlier ones.

> **Consolidated 2026-09-13** (user-authorised; see CLAUDE.md's exception to the
> append-only rule). Sections of *done* tasks were rewritten as summaries keeping the
> decisions, gotchas and every measurement an open task cites; the full history is in
> git before that date. Sections of open and parked tasks are unchanged except: the
> retired `grid-verdict-enum` section was folded into `tiling-uniformity-validator`.

## Epic summary

What other epics need to know about `film-base`:

- **`Dmin` and `Dmax` are different quantities that share this code.** `Dmin` is
  a per-channel **transmission** (the film base). `Dmax` is a **scalar** anchor in
  **density** units. Never conflate them (design-spec §4).
- **`film_base.source` has NO default — `convert` and `roll` refuse an unstated
  source (exit 2).** `Dmin` is the divisor of the density conversion, so it sets
  black point and colour balance together; it must be a decision, not an omission.
  `--auto-base` is still one flag and still means what it used to. `roll` accepts
  none of the film-base flags, so its diagnosis points at `film_base.source` in
  the shared `--params` recipe — a roll recipe must carry it. `estimate` resolves
  an unstated source to `auto` and `inspect` always runs the detector, since both
  exist to *produce* a base. Any orchestrator added later must call
  `cli::validate` (or `validate_with_remedy`) rather than reaching for a default
  that no longer exists.
- **`estimate` returns `BaseEstimate { base, warnings, ir_mask_applied }`** and
  **guards the resolved base finite-and-positive on every channel at birth** — a
  region on the dark holder errors loudly here, not silently downstream. The
  per-algo guards in `algo` remain defence-in-depth. Warnings ride to the report
  and `--strict` promotes them. `ir_mask_applied` is a *fact about what stage 2
  did*; callers key the "IR preserved but not used" note on it and must never
  re-derive it from the inputs (that re-derivation silently broke `--strict` on
  22 of 25 real frames once).
- **Real scans are laid out `dark holder → thin inset rebate → picture`** — the
  rebate is *not* the outer margin. Auto detection marches 1-px strips inward and
  takes the first uniform, value-continuous band sitting **immediately behind a
  holder run**; the brightest survivor wins. It uses **no orange/colour
  assumption** (holder-backing + flatness + brightness only), so a near-neutral
  base doesn't break it — **don't hard-code an orange-mask assumption anywhere.**
- **Auto is best-effort and refuses loudly** when it can't find a rebate — and on
  real full-size scans it has refused on **every frame tried** (11 frames, 6 rolls,
  2026-09-04; the cropped film holder defeats the rebate-band detector). The
  supported workflow is measure once from an unexposed reference, then reuse:
  `nc estimate` emits paste-ready `--film-base` / recipe-fragment forms (only when
  the measurement would actually be accepted by `convert`).
- **IR is consumed here, and only here so far.** On a scan with a *marker-verified*
  IR plane that **measures** able to separate holder from film on that frame
  (`ir_separability`, interior IR median vs 2.5x the holder classifier threshold),
  `ir_holder_mask` masks the opaque holder before the RGB rebate search.
  `--film-type` gates nothing since `ir-usability-detection` — chemistry
  mispredicts, because separability tracks the frame's own density (an unexposed
  silver frame separates ~20:1; its own leader is opaque). `FilmType` (recipe key
  `input.film_type`) survives as a provenance axis `algo/bw-support` and IR dust
  removal are expected to use; whether *those* gates should also be measurements is
  open. Known limitation, in the mask rather than the verdict: a thin holder margin
  that is IR-dark only in the shallow probe can hide a rebate behind it; the
  workaround is `--base-region`. The mask restricts **along** each edge only, not in
  depth — depth is `film_base::effective_area`'s, **shipped 2026-09-18**
  (`holder-depth-mask`): the per-edge IR holder march plus a user-sizable static inset
  (`measure.inset` / `--measure-inset`, the recipe's sixth section), returned as a
  rectangle with `holder_applied` and per-edge `capped`. Call it; do not re-derive it.
  Two things constrain callers: **`converged` is not a quality signal on its own** — a
  capped edge inflates its perpendicular edges at a stable fixed point, so read it with
  per-edge `capped` (`film-base/holder-cap-contamination` narrows this) — and **which
  region a measurement gets is `measures_over_region`, while whether the IR plane moved
  a pixel is `region_reaches_a_rendered_pixel`**; a new consumer adds its condition to
  one of those two, never to a merged one. `DmaxSource::Auto` is the only consumer so
  far; `Dmin`/`Dmax` estimation is still unmoved (`holder-masked-measurement`). A holder covering *every* edge (22 of 25 real
  chromogenic frames at the 0.5% probe depth) **is** handled: `ir_holder_mask`
  returns no mask when no edge would yield a film range, so the search falls back
  to RGB-only instead of getting nothing to scan.
- **`Dmax` is roll-fixed, not per-frame.** The default is `Fixed` (the nominal
  `NOMINAL_DMAX`, **1.3** in corrected-density units since 2026-08-08 /
  `pipeline_version` 2 — a rounded median of measured rolls, not a calibration;
  `dmax-anchor-reliability` owns the number); `nc estimate --d-max-region`
  measures a calibrated scalar from a fully-exposed leader frame and emits it as
  `{"explicit": d}`. Per-frame `Auto` is demoted to opt-in and is the marker that a
  run is *not* film-master-compatible. A per-channel Dmax would smuggle in white
  balance, so the anchor is scalar by construction (`reference_dmax` measures per
  channel, then reduces by the gray mean — the per-channel values survive only for
  the plausibility check). **The leader anchor's level is uncontrolled** (same
  stock 0.295 density apart while the base agrees to 0.0005), and the leader is
  disqualified as a per-channel source — see `dmax-anchor-reliability` and
  `dmax-per-channel-reduction` (parked 2026-09-13 on the calibration shoot).
- **Reusing an explicit `Dmax` has a domain caveat:** it is measured against raw
  `D`, so non-default `density_scale`/`offset` or a non-neutral regional balance
  shift the `D′` domain and mis-anchor it. The orchestrator warns; heed it.
- **`--grid` still ships and is slated for retirement** by
  `tiling-uniformity-validator`; nothing outside `estimate` reads it.
- **Known gap:** the reference-Dmax plausibility floor
  (`MIN_PLAUSIBLE_REFERENCE_DMAX = 1.0`) and the base-uniformity check are
  C41-calibrated and **false-alarm on dense/neutral-base stocks** like Harman
  Phoenix (`film-base/dense-base-dmax-plausibility`).


## estimation
**Status:** done (#10, 2026-07-01; film base made a stated choice in #84, 2026-08-09)
**Updated:** 2026-09-13 (consolidated)

- `pipeline/film_base.rs::estimate` resolves a `FilmBaseSource` (`Auto | Region |
  Explicit`, precedence structural in `cli.rs`'s merge) into a `FilmBase`. Per-channel
  **p97** (`SAMPLE_PERCENTILE`, over finite values only via `total_cmp`) is the sampling
  statistic — resists hot pixels/dust while landing on the bright base. Region rects
  are bounds-checked in the stage with u64 math (the CLI cannot see image dimensions).
- **The Step-1 auto heuristic sampled the outer 4% margin as one blob** (holder +
  rebate + picture), so on real scans it always bailed; `auto-base-redesign` replaced
  it. Kept here because that verification is where the real layout was learned:
  - Real scans are `holder (~0.01) → thin bright uniform rebate → picture`; the rebate
    appears only on some edges and can be a few px wide. Measured rebate is consistent
    per stock (`48bit-full/1` bottom and `/2` left both ≈ `[0.53, 0.26, 0.16]`).
  - **Decision (user): the accurate path is a dedicated unexposed reference frame,
    reused across the roll.** Reference `20260630-nikon-844.tif` gives
    `[0.553, 0.271, 0.159]` from a centre region; the sibling picture frame's own edge
    rebate reads `[0.475, 0.236, 0.136]` — a ~14% gap, so a large clean unexposed area
    beats a narrow edge strip (falloff/fog). This is the premise `base-acquisition-planner`
    and `content-fallback` build on.
- **Fail loudly, no silent fallback**: a low-confidence auto result is an error naming
  `--film-base` / `--base-region`.

### 2026-08-08 — the film base becomes a stated choice (#84)

- **`film_base.source` no longer has a default**; `convert`/`roll` reject an unstated
  source with exit 2 (user decision after a defaults review). `--auto-base` is still one
  flag; what is gone is reaching it by silence.
- **The diagnosis is command-aware.** `roll` accepts none of the three film-base flags,
  so `cli::missing_film_base_message` has two spellings behind `FilmBaseRemedy`: `convert`
  is told about the flags, `roll` about `film_base.source` in the shared `--params`
  recipe. Both call sites share the one function.
- **The rule is the *last* check in `validate`, deliberately** — placed first, the
  least-specific diagnosis pre-empted every contradiction rule.
- `estimate` still resolves an unstated source to `auto`; `inspect` never consults
  `FilmBaseParams` (it calls `rebate_candidates` + `select_auto_base` directly). Both
  exist to *produce* a base. `film_base::estimate` takes a resolved `&FilmBaseSource` —
  "unset" is an orchestration state, never a stage input.
- **No pixel moved; `PIPELINE_VERSION` stayed 1.** A bump was tried and reverted:
  `pipeline_version_warning` fires on *any* mismatch with "the output will not match the
  original" and `--strict` promotes it, so replaying an archived v1 sidecar that states a
  base — which renders bit-identically — exited 1 on a false claim. The v1 row's `recipe`
  hash was refreshed in place (the default document now carries `"source": null`) and
  `PIPELINE_BEHAVIOR` dropped its "auto rebate film base" clause; `render`/`base` stay
  never-edit. This precedent was reused by `ir-usability-detection`.
- **Two gotchas for the next default change.** The stage-2 `base` fingerprint pins
  `FilmBaseSource::Auto` **explicitly**, not `FilmBaseParams::default()` — do not
  re-couple the detector's fingerprint to a default. And `run_estimate`'s
  `unwrap_or(FilmBaseSource::Auto)` is now the crate's only surviving default film-base
  choice, and **no fingerprint watches it**: changing it would move every `nc estimate`
  result with the whole gate green (a rustdoc line on `run_estimate` says so).


## auto-base-redesign
**Status:** done (#23, 2026-07-16)
**Updated:** 2026-09-13 (consolidated)

- Goal: robust `--auto-base` on the real `dark holder → rebate → picture` layout,
  replacing the Step-1 margin heuristic. Whole task is pure functions; `cli`/`stages`
  only ferry warnings.
- **Detector** (`rebate_candidates` + `select_auto_base`; `auto_estimate` composes them):
  - Per edge, march 1-px strips inward up to `REBATE_SCAN_FRAC = 10%` of the short side
    (min 3 px). Strips are **trimmed by the scan depth at both ends**, or the
    perpendicular edges' holder margins contaminate every strip.
  - Per strip: per-channel p97 + worst-channel relative spread `(p97−p10)/p97`. Classes:
    **holder** (all channels p97 < `HOLDER_MAX_TRANSMISSION = 0.05`; real holder ≈ 0.01,
    dimmest real rebate channel ≈ 0.14), **uniform** (all-channel spread ≤ 0.15), else
    **other**.
  - A candidate is the **first** run of ≥ `MIN_BAND_STRIPS = 2` uniform, value-continuous
    strips (adjacent-strip step ≤ 10% per channel — what stops the band merging into a
    flat picture region) sitting **immediately behind a contiguous holder run**; the band
    is re-measured as one region and must pass the spread gate again. Bands at depth 0
    (no holder outside) are rejected.
  - Selection: candidates must beat the frame-interior **median** on **every** channel by
    ≥ 5% (`INTERIOR_BRIGHTNESS_MARGIN`); the **brightest** survivor wins.
- **Decisions still load-bearing:**
  - **The anti-bright-surround signal is holder-backing, not mandatory cross-edge
    agreement.** A bright surround bleeding to the edge has no dark holder outside it →
    no candidate → refusal. A real rebate legitimately appears on a **single edge**
    (verified `48bit-full/2`, left only) — `auto-base-neutral-stock` must not "fix" its
    single-band case by requiring corroboration. Cross-edge *disagreement* between
    survivors is a `--strict`-promotable warning (same-edge disagreement too, since
    `ir-holder-detection` let one edge yield several runs).
  - **Brightest wins is physical**: the rebate is Dmin = per-channel max transmission;
    no genuine picture area out-brights clean base, and a uniform dark band behind the
    holder can never out-rank a real rebate.
  - **No orange/colour assumption anywhere** — holder-backing, flatness and brightness are
    colour-independent, so a near-neutral base (Phoenix, R/B ≈ 0.84) passes the same gates.
  - `estimate` returns `BaseEstimate { base, warnings }`; the `--base-region` uniformity
    check *warns* (never alters the value); `guard_base` rejects any zero / negative /
    non-finite channel at birth for every source. Estimation runs in the orchestrator
    *before* the render (stage 2 is not inside `stages::render`), so a render error cannot
    swallow the warning that explains a bad base. `nc estimate --strict` fails on any
    warning, report emitted first.
  - `percentile` is `retain(finite)` + `select_nth_unstable_by` (O(n), deterministic).
  - `inspect` reports `base_candidates` (edge, `--base-region`-ready rect, value, spread)
    even when selection refuses.
- **Scope change 2026-07-15 (authoritative):** the content-based source (`--base-content`,
  `film_base.source = "content"`, its report wiring and tests) was **reassigned to
  `film-base/content-fallback`** and removed from this worktree. The only content-mode
  responsibility left here is the auto-refusal message *suggesting* `--base-content`
  (`RECOVERY_ADVICE`), never implementing or silently falling back to it. The task file
  still lists content mode in its scope and verification; this note wins.
- **Real-scan status — the happy path has not been observed on a full-size scan.** The
  worktree only had the 502×462 picture-interior fixtures (correct refusal). Thresholds
  came from the `estimation` real-scan numbers (holder ≈ 0.01, rebate ≈ [0.53,0.26,0.16],
  picture spread ≫ 0.15). Later evidence: `ir-usability-detection` (2026-09-04) found
  `--auto-base` refuses on all 11 real frames tried across 6 rolls, and the 2026-09-10
  Ektar roll's cropped holder defeats the rebate-band detector on every frame. **Nothing
  currently owns making auto succeed on real scans**; `auto-base-neutral-stock` and
  `white-holder-support` harden it for specific stocks/holders only.
- **Notes for dependents:**
  - `white-holder-support`: the polarity assumption lives in exactly two spots —
    `StripClass::Holder` classification (`HOLDER_MAX_TRANSMISSION`) and the documented
    "holder-backing" rationale. A `film_base.holder = white` knob flips the holder test
    to "very bright on all channels"; selection is unchanged, since the holder sits
    *outside* the band. `Edge`/`RebateCandidate` are public.
  - `auto-base-neutral-stock` (PR #23 review): the mean-brightness tiebreak can prefer a
    coloured edge artefact over a true rebate that is lower in blue; a single
    uncorroborated bright band with a holder present is content estimation in disguise.
    Both are recorded in that task file.
  - `algo/auto-anchor-interior-measurement`: `film_base::auto_interior_pixels` encodes the
    interior rectangle `[cap, cap, w − 2·cap, h − 2·cap]` that `pipeline::memory` sizes the
    film-base phase from; reuse the *rule*, not a copy.

## white-holder-support
**Status:** not started
**Updated:** —

- Goal: support scans made in light/white film holders, where the current
  darker-than-interior assumptions of base estimation don't hold.



## estimate-reuse-output
**Status:** done (#31, 2026-07-17)
**Updated:** 2026-09-13 (consolidated)

- Goal: `nc estimate` output shaped for direct reuse, closing the
  measure-once-reuse-for-the-roll loop.
- **Reuse-ready report fields.** `film_base_flag` (a paste-ready `"--film-base R,G,B"`,
  formatted with `f32` `Display` so parsing back is **bit-identical**) and
  `film_base_recipe` (`{"source":{"explicit":[r,g,b]}}`, parses as a recipe's `film_base`
  section). Both are emitted **only when the measurement passes the same explicit-base
  validation `convert` applies** (each channel in `(0, 1]`); a degenerate measurement is
  still reported as `film_base` with a warning saying why no reuse form was emitted. In
  `Report` they are one `reuse: Option<ReuseReady>` flattened to the two flat keys
  (both-or-neither is unrepresentable; wire shape unchanged). Reuse fields survive a grid
  *disagreement* (the median resists one bad cell; `--strict` is the hard gate).
- **`estimate --grid`** (`film_base::estimate_grid`): a fixed 5-cell grid (corners +
  centre, each 25% × 25% of the rectangle, p97 per cell) over the frame or
  `--base-region`. `GridEstimate { base, cells: [GridCell; 5], spread, tolerance,
  agreement }`: combined `base` is the per-channel **median** across cells; `spread` is
  `(max − min) / max` judged against `GRID_MAX_RELATIVE_SPREAD = 0.05`; disagreement is a
  **diagnostic warning** naming the per-cell evidence, never averaged away. A **degenerate
  combined base hard-errors (exit 1) regardless of `--strict`**, after the report is
  emitted — matching the single-measurement path's birth guard (2026-07-17 review fix;
  fixture `tests/fixtures/black-48bit.tif`).
- **`--grid` is an estimate-only CLI mode, not a recipe key** (like `--report`): `convert`
  never grid-samples, so the four-spot knob wiring deliberately does not apply. It
  clap-conflicts with `--film-base` and `--auto-base`, composes with `--base-region`.
  Under `--grid`, `film_base_source` reports the overall rectangle sampled; the `grid`
  object documents the per-cell method. If grid ever becomes a `convert`-usable source it
  must join `FilmBaseSource` as a variant, not a bool beside it.
- **`--strict` on `estimate`** promotes any estimate warning to exit 1 after the report.
- **Known limitation, deferred then retired:** `GridEstimate.agreement: bool` conflates
  *disagree* with *degenerate*, and `spread = 1.0` is an overloaded sentinel the CLI
  re-derives the case from. The `grid-verdict-enum` task filed for it was **removed
  2026-08-11**; its intent (a self-describing verdict) carries into
  `tiling-uniformity-validator`, which retires `--grid` and `GridEstimate` outright.
- **Follow-ups still without an owner:** `inspect`'s suggested-Dmin output carries no reuse
  fields (estimate-only by scope; trivial to add); the 0.05 grid tolerance is a constant,
  not a flag (moot once `--grid` goes).
- Verified on the committed fixtures: `estimate --base-region 0,0,60,60` emits flag +
  fragment; full-frame `--grid` on a real (non-blank) frame disagrees (spread ≈ 0.42–0.56)
  → warning, exit 0, exit 1 under `--strict`; an e2e round-trip feeds both reuse forms
  back into `convert` and asserts byte-identical outputs.


## dmax-reference
**Status:** done (#39, 2026-07-21; labelled `pipeline_version` 1 by `core/conversion-versioning`)
**Updated:** 2026-09-13 (consolidated)

Made `Dmax` a **roll-fixed calibration** (like `Dmin`) instead of a per-frame
measurement, and changed the default render accordingly.

- **`DmaxSource`** (`types.rs`) is `{ Fixed (default), Explicit, Auto, None }` — one enum,
  wire forms `"fixed"` / `{ "explicit": <d> }` / `"auto"` / `"none"`. `Fixed` resolves
  the nominal `algo::density::NOMINAL_DMAX` (scene-independent, **in corrected-density
  units**, not base transmission + range — mixing the two is a unit error): shipped as
  `2.0`, **lowered to `1.3` on 2026-08-08** by `algo/negative-reconstruction-density-curves`
  (`pipeline_version` 2) because 2.0 sat above every measured roll (0.90–1.74) and darkened
  every default conversion ~5x; 1.3 is a rounded median, not a calibration —
  `dmax-anchor-reliability` owns the number. `Explicit(d)` is the roll-fixed calibrated
  value a roll recipe freezes. `Auto` (`--auto-d-max`, the 99.5th percentile of the frame)
  is **demoted** to opt-in and documented as per-frame exposure normalisation. `None`
  (`--no-d-max`) is the bit-exact scene-referred escape hatch. `--fixed-d-max` exists so a
  recipe's explicit/auto is CLI-overridable back to the default; all four are mutually
  exclusive and wired through Overrides / Params / merge / validate.
- **Plan-phase measurement: `estimate --d-max-region X,Y,W,H`**, the mirror of
  `--base-region`. Point it at the light-struck leader with the roll's `Dmin` as
  `--film-base`. It samples the region's **median** transmission
  (`film_base::sample_region_at`, `p = 0.5` — robust to dust on a near-opaque frame; a
  relative-spread gate on near-zero transmissions is noise-dominated and would
  false-alarm), converts per channel to base-relative density, and
  `algo::density::reference_dmax` reduces to **one scalar by the gray mean**. It returns
  `ReferenceDmax { scalar, per_channel }`; the per-channel values exist only for the
  plausibility check. A per-channel `Dmax` would be three different gains in the exponent
  — a white balance — so the anchor is scalar by construction
  (`reference_derived_dmax_introduces_no_per_channel_correction`). `holder-masked-measurement`
  brings `Dmin` onto the same `p = 0.5` rule; `dmax-per-channel-reduction` questioned the
  gray mean and closed that half as *absorbed* (the term is a slope).
- **Freeze = scalar, provenance = report.** `estimate --d-max-region` emits `d_max_flag`
  (`--d-max <d>`) and `d_max_recipe` (`{ "dmax": { "explicit": <d> } }`) and records the
  region as `dmax_region` provenance only. There is deliberately **no** `{ "reference": … }`
  recipe form: the apply phase re-reads nothing, so one recipe hash yields one output
  (`roll-conversion`'s deterministic-apply contract). Reuse forms are gated on the same
  `(0, 1]` base-usability check as the film-base reuse.
- **`reference_dmax` hardening (review, 2026-07-19/21):** on every channel, a transmission
  that is non-finite or at/below `SCAN_EPSILON` is a **hard error** (a floored channel —
  dead sensor, clipped black, dark holder — must not launder into `D ≈ 6` and freeze a
  black-rendering anchor), and each channel's base-relative density must be `> 0` (a
  coloured/wrong region can average positive while one channel out-transmits the base).
  Hard errors are `NcError::Other` (exit 1), mirroring the `Dmin` guard.
  `clipped-dmax-reference` exists because a genuinely clipped leader (Portra 400 `1229`,
  zero transmission in all three channels) hits this guard and cannot produce a value.
- **Plausibility warning** (`MIN_PLAUSIBLE_REFERENCE_DMAX = 1.0`, `--strict`-promotable,
  never a rejection): `reference_dmax_plausibility_warning` has two mutually exclusive
  shapes — a sub-floor gray mean (frame too thin, not a leader), or a plausible mean with
  the **weakest channel** below the floor (a coloured/wrong region a scalar hides, e.g.
  per-channel densities ≈ `[3.0, 0.004, 0.004]` averaging 1.0). The floor is C41-tuned and
  false-alarms on Phoenix (`0.898` from a correct leader) — `dense-base-dmax-plausibility`.
- **Domain guard** (`cli::explicit_dmax_domain_warning`, pure): an explicit/reference
  `--d-max` is measured against raw `D`, so non-default `density_scale`/`density_offset`
  **or a non-neutral regional balance** shifts the render's `D′` and mis-anchors it;
  `convert` warns (`--strict` promotes). Threading scale/offset into `estimate` was
  infeasible — `estimate` resolves only a film base and builds no `ResolvedConfig`.
- **This changed the default render** (frame-local auto → fixed nominal). No
  `pipeline_version` constant existed yet, so the change was documented as superseding v0
  in design-spec §12 and `core/conversion-versioning` later labelled it **`pipeline_version`
  1**. Real-leader verification (Ektar `1009` / Phoenix `1010`) was deferred to the user and
  done by `analysis/real-scan-verification` (2026-07-23: Phoenix `1010` → 0.898).


## ir-holder-detection
**Status:** done (#56, 2026-07-26); its `--film-type` gate superseded by `ir-usability-detection` (#104, 2026-09-04)
**Updated:** 2026-09-13 (consolidated)

The first real consumer of the decoded IR channel: a pure, segmented IR film-holder
mask that feeds the RGB rebate search.

- **Mask (`film_base::ir_holder_mask`).** Per edge, the along-edge extent is split into
  `IR_HOLDER_SEGMENTS = 24`; each segment's **shallow** near-edge probe band
  (`IR_HOLDER_PROBE_FRAC = 0.5%` of the short dimension, floored at 2 px) is reduced to
  its median IR and classified holder (≤ `IR_HOLDER_MAX_TRANSMISSION = 0.1`) or film.
  Segmenting *along* the edge lets a partially-covered edge split into holder vs film
  runs. `rebate_candidates` then runs the inward scan **once per contiguous film run**;
  with no mask it is the single full-extent scan, byte-identical to before. The mask is
  built **once** by `estimate` and passed in (`ir_mask_applied` reports the outcome —
  see `ir-usability-detection`).
- **Probe depth was the one real design decision.** Probing the whole ~10% rebate-scan
  window washes a real holder band out with the bright film behind it; the holder
  occludes from the very edge inward, so its darkness lives in a shallow band. **The mask
  therefore restricts along the edge only, not in depth** — a thin holder margin that is
  IR-dark only in the shallow probe, with a rebate directly behind it, is excluded and
  auto-base can miss a rebate RGB-only would find (**known limitation**, deliberately
  accepted, workaround `--base-region`/`--film-base`; documented in the module doc). The
  roadmap fix is depth-aware occlusion — which is exactly the "existing mask has no depth"
  work `algo/auto-anchor-interior-measurement` and `holder-masked-measurement` take on.
- **Real-scan numbers (derived only).** IR separates holder from film by ~10–25x: holder
  segments read IR ≈ 0.019–0.084, film ≈ 0.65–0.67, so 0.1 sits with wide margin.
  Phoenix `933` (unexposed): top and right = holder, bottom and left = film. Ektar `1009`
  (leader): **all four edges read holder** — the task file expected "all-film", but a real
  leader is genuinely held in a holder on every edge; its bright fully-exposed film is the
  interior. Neither calibration frame yields rebate candidates (they lack the
  `holder → rebate → picture` structure; use `--base-region`/`--grid` on them).
- **IR provenance gate.** The decoder accepts a same-dimension 16-bit grayscale IFD as IR
  by **shape alone** when the `NewSubfileType=4` marker is absent. Safe while IR was
  merely carried, unsafe once consumed: `LinearImage::ir_verified` (set from the marker)
  gates the mask, a shape-only plane falls back to RGB-only with a `--strict`-promotable
  note.
- **`--film-type (silver | chromogenic | unknown)`**, recipe key `input.film_type`
  (`InputParams`, design-spec §9 input section — deliberately not under `film_base`, since
  IR dust removal will reuse it). Shipped as the **gate** for this mask
  (`FilmType::ir_transparent()`, chromogenic only, default off). **That gate is gone**:
  `ir-usability-detection` showed chemistry mispredicts, deleted `ir_transparent()`, and
  left `FilmType` as provenance only (echoed as `report.film_type` on all three commands).
- `nc inspect` reports `holder_mask` (per-edge segments with span/class/IR). A too-small
  image is a diagnostic warning in `inspect`, never an abort.
- The Codex P1 claiming the mask excludes spans the rebate detector needs was a false
  positive: `edge_candidate`'s depth-0 "holder run" is a dark *RGB* band (can be dense
  film, IR-bright); the IR mask excludes only IR-dark spans, and
  `ir_mask_recovers_the_rebate_on_a_partially_occluded_edge` proves IR *recovers* a rebate
  RGB-only misses.


## auto-base-neutral-stock

**Status:** not started
**Updated:** —

- Goal: Harden auto film-base detection for film stocks whose base is **near-neutral** (e.g. Harman Phoenix, R/B ≈ 0.84) rather than orange — bright but not color-distinctive, so confidence signals keyed on base color can mis-anchor on bright neutral scene content.


## dense-base-dmax-plausibility

**Status:** not started
**Updated:** —

- Goal: Stop `nc estimate` from emitting spurious plausibility warnings on legitimately dense- / atypical-base film stocks.


## content-fallback

**Status:** not started
**Updated:** —

- Goal: Add an explicit, opt-in film-base source that estimates `Dmin` from the exposed image **content** when no unexposed film (dedicated frame, rebate, or holder-inset band) is available to sample — the design-spec §9 acquisition-ladder **Tier 3**.


## dmax-anchor-reliability

**Status:** not started
**Updated:** 2026-08-03

- Goal: establish whether the roll-fixed `Dmax` anchor measures the quantity it is meant to.
  A follow-up on a **completed** contract (`film-base/dmax-reference` built what was specified),
  which is why it is a new task rather than an edit.
- Evidence from `algo/reference-anchored-sigmoid` (2026-08-02/03), all from committed data:
  1. **Same-stock rolls disagree by a full stop while their bases agree.** `Portra400` 1.7383 vs
     `Portra400-leica-flaw` 1.4435 (0.295 apart) while their **red base agrees to 0.0005** — the
     base proves ±0.03 reproducibility is achievable, so both cannot be film properties. The
     Portra 160 pair differs by only 0.046, so the leader is not reliably *wrong*; it is
     **uncontrolled**, which is worse, because one measurement cannot tell you which case it is.
  2. **Real content exceeds the anchor** — G3 `D′` 1.3265 vs Dmax 1.2758; P3 1.5062 vs 1.3816.
  3. **Leaders are uniform**, so it is not a fogging gradient: interior `D′` range 0.024/0.039/
     0.067, gradients ≤0.024. A uniform field at an *uncontrolled level*. Grain sensitivity also
     makes "fully exposed" arguably ill-posed.
- Separately, `NOMINAL_DMAX = 2.0` is a poor no-reference fallback: measured rolls span 0.90–1.74
  (median ≈1.34; ≈1.36 excluding the poor-quality Harman Phoenix), so worst-case error is 1.10
  density and switching between `Fixed` and `Explicit` is a multi-stop jump. ~1.35 is provisional;
  n=7 is too small to fix a shipped constant, and Portra400's own 1.7383 is one of the suspects.
- `algo` candidates 2 and 3 are contingent on this: candidate 3 halves a Dmax error
  (`dA/dDmax = 0.5`, so 0.046 → 0.15 stop but 0.295 → 0.98 stop); candidate 2 passes it in full.
- 2026-09-12 (**a direction was picked**): roll-wide content measurement is the lead
  candidate (user) — measure the anchor across the roll's picture frames, excluding the
  leader and any near-fully-exposed frame, then freeze it as an explicit `Dmax`. It keeps
  the roll-fixed shape that makes `Explicit` right while replacing the uncontrolled part,
  and finding 2 already supports it: real content measures *above* the leader anchor. The
  other three directions stay as alternatives.
  **It inherits the holder defect** — a high percentile over whole frames is what
  `algo/auto-anchor-interior-measurement` measured at 2.23–2.37 against a roll Dmax of
  1.28–1.38. Evaluating the direction does not need the holder fix; producing a number does.
  No dependency edge was added: the task's *establishing* half is genuinely unblocked, and a
  hard edge would have blocked a ready task on a prerequisite only one of its four directions
  needs.

## dmax-per-channel-reduction

**Status:** parked (see 2026-09-13)
**Updated:** 2026-09-13

- Goal: decide whether the gray-mean reduction in `reference_dmax` discards a
  per-channel term that matters, and if so where that term belongs. An
  investigation — "the scalar is justified" is a valid outcome. Ships no pixel change.
- Origin: raised 2026-08-06 while drafting user-facing usage documentation.
  Working through why `Dmin` is per-channel and `Dmax` scalar surfaced the
  assumption underneath: since
  `D_c` is base-relative, a scalar anchor asserts the **highlight end shares the
  base's colour cast**. Not previously stated anywhere.
- The committed data already contradicts it. Recomputed from the leader-uniformity
  table in `reports/sigmoid-reference-baseline.md` (per-channel base-relative
  densities = per-channel `Dmax`):

  | roll | R | G | B | gray mean | fixture Dmax | spread | stops | widest |
  |---|---|---|---|---|---|---|---|---|
  | Gold 200 | 1.2242 | 1.2340 | 1.3628 | 1.2737 | 1.2758 | 0.1386 | 0.46 | B |
  | Ektar | 1.2724 | 1.2865 | 1.3201 | 1.2930 | 1.2933 | 0.0477 | 0.16 | B |
  | Portra 160 | 1.4402 | 1.3297 | 1.3807 | 1.3835 | 1.3816 | 0.1105 | 0.37 | R |

  The gray mean reproduces each fixture `Dmax` to ~4 decimals, which confirms the
  reduction is `(r+g+b)/3`. Direction is **not** consistent (B densest twice, R
  once), so no single constant absorbs it. Gold 200's leader renders
  `R 0.892 / G 0.913 / B 1.228` at `gamma = 1` — a blue "white" with blue clipping.
- Why it may still be fine, and the one case where it is not: for the
  **exponential** curve a per-channel anchor is *exactly* a per-channel gain
  (`10^(γ(D'−Dmax_c))` factorises into the scalar form times a constant), so
  `print.white_balance` / `reconstruction.density.offset` already span it — nothing
  is lost. The **sigmoid** is nonlinear, so a per-channel anchor moves each channel
  to a different toe/shoulder position and no downstream gain reproduces it. The
  sigmoid is the intended default, so the question gets *more* relevant over time,
  not less.
- Cheapest first step is the **ratio-stability** question, and it is a pure re-read
  of existing measurements: `dmax-anchor-reliability` established the leader's
  *level* is uncontrolled, but level and ratio are different claims — the level
  depends on how much light hit the leader, the inter-channel ratio may be a dye
  property. Same-stock sibling pairs (`Portra400` vs `Portra400-leica-flaw`; the
  Portra 160 pair) separate them. Confound to respect: if the three layers differ
  in contrast, a ratio measured at an unknown exposure level is not the ratio at a
  different level.
- Coordinate with `dmax-anchor-reliability` — same leader measurements, different
  axis; neither blocks the other.

### 2026-09-10 — re-scoped after `algo/film-stock-profiles`; whole-roll Ektar measured

The task's original question closed from outside. `algo/film-stock-profiles` (#105) settled
that the per-channel term is a **slope**, not an anchor — shipped as `density.scale`
`[1, 0.90, 0.86]` on the parametric curves (`pipeline_version` 4) and as each stock's own
inverted curve on `characteristic`. It also disqualified the leader as a source: measured
leaders do not reproduce the published per-channel divergence, and that comparison cannot
separate a non-neutral leader exposure from a scanner-slope error. So questions 1–3 as written
are answered, the verdict on the original taxonomy is *absorbed*, and the
`→ algo/split-default-migration` edge was removed the same day (the proposed default,
`characteristic-generic`, has neither a scalar `Dmax` nor a per-channel gain to get wrong).

What the user kept: **is there another way to compute the per-channel scaling**, since the
shipped constant is corpus-calibrated on one scanner and six rolls and no datasheet exists for
every stock. Candidates: the leader (dead) and a whole-roll measurement.

**Stage 0 — the asset.** `rolls/2026-09-09-Ektar`: 34 frames, Kodak Ektar 100, same scanner and
SilverFast build as the entire corpus (Plustek OpticFilm 8300i, 9.2.9), so it is directly
comparable. Its 32 NLP positives moved to `converted/nlp/2026-09-09/2026-09-09-Ektar/` — the
**roll subdirectory is load-bearing**: without it `walk_converted` yields `roll = None`,
`src_roll` falls back to the hardcoded `Portra160-2026-07-22`, and every `source_frame`
resolves `null`. All 32 link correctly.

Frame `1603` is the roll's **leader** and `1604` its **unexposed** base frame (user, and their
readings confirm it: spans of 0.07 and 0.10 density, i.e. uniform fields). The cropped film
holder defeats the *auto* rebate-band detector on every frame, but that is a detector-geometry
limitation, not a missing reference — the pair freezes normally with explicit regions. The
harness `freeze` stage produced `2026-09-09-Ektar.json` and **reproduced the other six
recipes byte-identically**, which is a free reproducibility check on the whole freeze path:

- `Dmin` `[0.3916228, 0.20027466, 0.13084611]` · `Dmax` `1.2826737`

*Worth recording as a trap:* the first pass read `1603`/`1604` as ill-conditioned picture
frames and concluded the roll had no base. A leader and an unexposed frame are exactly what a
"narrow density span" filter flags, so a conditioning filter cannot be trusted to tell a
useless frame from a calibration frame — check the roles first.

**Stage 1 — `curve_probe::whole_roll_scale`** (new, asset-gated, `#[ignore]`d; deliberately
outside `FIXTURES`, which is the calibration corpus behind the shipped default). Roles now
exclude `1603`/`1604`, leaving 32 real frames; 7 more span < 0.35 density and return
ill-conditioned slopes, leaving **25**. One frame clears the span filter and is still an
outlier — `1626`, `r_blue` **−0.91**, which no film produces — so the median is reported beside
the mean. `out_of_range` is 0.000 everywhere.

| n = 25 | green | blue |
|---|---|---|
| mean `r` | 1.2569 | 1.1781 |
| sd | 0.2410 | 0.5210 |
| se | 0.0482 | 0.1042 |
| nulling scale | **0.796** ± 0.031 | **0.849** ± 0.075 |
| corpus (21 frames, 6 rolls) | 1.115 → 0.900 | 1.183 → 0.860 |

**Blue reproduces the corpus (1.178 vs 1.183); green does not (1.257 vs 1.115, ≈3 se.)** The
datasheet-corroborated half transfers across rolls; the scanner-residual half does not. This
roll wants green ≈ 0.80, not the shipped 0.900 — the first per-roll green statement the asset
set can support (`io/scanner-density-calibration` records ~11 frames of one condition as the
threshold, and two earlier per-roll conclusions retracted at n = 3–4).

**Split-half cross-validation**, because the roll's own null scored on its own frames is
circular: fitted **0.20** mean held-out `|green−magenta|` against the current default's
**0.86** — a 4.3× improvement out of sample, with the two folds fitting nearly the same value
(green 0.806 / 0.784, blue 0.868 / 0.829). So ~12 frames already resolve a roll's scale
reproducibly, corroborating the ~11-frame estimate empirically.

**The slope measurement is base-independent, now shown rather than argued.** An earlier run
used the sibling `Ektar` roll's base, which differs from this roll's per-channel and
non-uniformly (ratios 0.758 / 0.723 / 0.690, up to 0.041 density). Every `span >= 0.35`
statistic is **identical to four decimals** across that change. The mechanism: a base error is
a per-channel constant in density, `measure_decoded` subtracts the middle bin from every bin
per channel, and the residual x-shift is a uniform translation that cannot move a
least-squares slope. The immunity is specific to slope — a level measurement is exactly what a
base error corrupts, which is why Stage 2 needed the real base.

**Stage 2 — `curve_probe::whole_roll_white_point`.** Per-channel density relative to red at
p99.5 (the statistic `WbSource::Percentile` already equalises, per frame), with grey-world on
the same frames as a control, 32 frames:

| | mean | sd | se | stops |
|---|---|---|---|---|
| white `g−r` | 0.2461 | 0.0658 | 0.0116 | 1.69 |
| white `b−r` | 0.4262 | 0.0908 | 0.0161 | 2.93 |
| grey `g−r` | 0.2362 | 0.0807 | 0.0143 | 1.62 |
| grey `b−r` | 0.4492 | 0.1267 | 0.0224 | 3.09 |

The roll **does** have a consistent white point: sd is 21–27 % of the mean, a roll-level
constant predicts a held-out frame at RMS 0.065 / 0.089 against 0.255 / 0.436 for no offset,
and the two estimators — which lean on opposite assumptions — agree to 0.010 / 0.023.

**The result that matters is the decomposition.** At the roll's mean near-white red density
0.8184, a channel running `r` times steeper sits `(r − 1)·0.8184` above red for that reason
alone:

| channel | measured | slope part | residual offset |
|---|---|---|---|
| green `g−r` | 0.2461 | 0.2102 | **0.0359** (0.25 stops) |
| blue `b−r` | 0.4262 | 0.1458 | **0.2804** (1.93 stops) |

**Green's white-point difference is 85 % the slope `density.scale` already carries; blue's is
only 34 %.** So a white point adds essentially nothing on green and a large genuine offset on
blue — which is `density.offset`'s documented purpose (orange-mask compensation), still
shipping `[0, 0, 0]`. Independent cross-check: `film-stock-profiles`' datasheet-fitted generic
offset is `[0, −0.036, −0.057]`, and green's residual here is **−0.036 to three decimals**.
Blue's is ~5× larger, which is the direction the same task flagged when it noted `D'_B` is not
a constant multiple of `D'_R` (ratio drifting 1.25–2.43). Treat the green agreement as a
cross-check worth pursuing, not as confirmation: the two fits use different parameterisations.

**What none of this establishes.** One roll, one stock — and Ektar is the corpus outlier on
green (worst residual, +1.00; its sheet disagrees with its own aim table by 11 %), so this is
the most favourable case for a per-roll correction, not a typical one. A second full roll of a
different stock is needed before anything ships. And the white point cannot be *validated*
here at all: "the brightest thing was white" and "the blue layer runs hot" are
indistinguishable without a known neutral, which is precisely the ColorChecker frame
`io/scanner-density-calibration` is waiting on. Self-consistency is all that was measured.

Finally, scope: a per-roll scale or offset is **content-derived**. The recorded line permits a
corpus-calibrated constant pinned in source and forbids a value derived per run from the frame
being converted; per-roll sits between, so it is an opt-in knob at most until the user rules
otherwise. Note also that per-*frame* `--white-balance percentile` already removes the whole
white-point difference by construction — the roll-level version is strictly worse at
neutralising any single frame (held-out RMS 0.065–0.089) and is only preferable because it
does not optimise per frame.

Gates: `fmt` clean, `clippy --all-targets -D warnings` clean, `build` OK, `cargo test` 730
unit + 189 integration, 0 failed (23 ignored, two of them the new probes), `cargo doc` at
exactly the documented 16-link baseline, `nctool` suite OK (76 skipped, no local venv — the
`NCTOOL_REQUIRE_DEPS=1` guard fails for that reason alone, as in #105).

### 2026-09-12 — second whole roll (Portra 400): the Ektar result does not generalise

`rolls/2026-09-11-Portra400` registered — 35 frames, Kodak Portra 400, same scanner and
SilverFast build as the corpus; 32 NLP positives moved to
`converted/nlp/2026-09-11/2026-09-11-Portra400/` (roll subdirectory again, or `source_frame`
falls back to the hardcoded NLP roll and resolves `null`; all 32 link). Leader `1638`,
unexposed `1672`, frozen by the harness: `Dmin [0.350927, 0.16437018, 0.09372091]`,
`Dmax 1.2665207`. The Ektar recipe reproduced byte-identically in the same run.

**A span filter cannot exclude a calibration frame, and assuming it could corrupted the first
run.** Frame `1639` is half leader and half base. It was carried as `real` on the theory that
the density-span filter would drop it — it measured the **largest span on the roll (1.40)**,
because base-to-leader spans the entire density range. What it lacks is *scene* content,
which no span threshold can see. It now carries role `calibration`, which `real_frames`
filters on; `nctool manifest roles` does not know that role and warns while treating it as
real, which is cosmetic for the harness. `MIN_SPAN`'s doc records the trap.

`curve_probe`'s two whole-roll probes now take a roll + stock from `WHOLE_ROLLS` and run
both sets, with a cross-roll block. The point of a second roll is not more n: one roll can
show a fitted gain beats the shipped constant but not *why*. Same scanner, different stock —
if both land on one gain it is a scan-path property and a per-roll knob is the wrong shape.

| span >= 0.35 | n | mean `r_g` | mean `r_b` | scale green | scale blue |
|---|---|---|---|---|---|
| 2026-09-09-Ektar | 25 | 1.2569 | 1.1781 | 0.796 ± 0.031 | 0.849 ± 0.075 |
| 2026-09-11-Portra400 | 27 | 1.1393 | 1.0147 | 0.878 ± 0.039 | 0.985 ± 0.079 |
| shipped default | | 1.1111 | 1.1628 | 0.900 | 0.860 |

**The two rolls do not separate: green 1.7 se, blue 1.3 se.** So there is no evidence for a
per-roll gain — but the standard errors are wide enough (±0.03–0.08) that a real difference
of ~0.1 would be missed, so this is "not detected", not "shown equal".

**What did change is how well the shipped default fits, and the two channels disagree.**
Ektar's green sits **3.4 se** from the shipped 0.900 while Portra's sits **0.6 se** — the
default fits Portra's green well and Ektar's badly. Blue runs the other way: Ektar's 0.849 is
near the shipped 0.860, Portra's is **0.985**, i.e. that roll wants almost no blue gain at all
while the default applies 14%. A single constant cannot be right for both.

**The per-roll case is much weaker on the second roll.** Split-half, held out:

| roll | fitted | current default |
|---|---|---|
| Ektar | 0.20 | 0.86 |
| Portra 400 | 0.53 | 0.81 |

4.3x on Ektar, **1.5x** on Portra. The 2026-09-10 entry called Ektar the most favourable case
— the corpus outlier on green, with a sheet disagreeing with its own aim table by 11% — and
that is what it turned out to be.

**White point (the offset half) — the green residual is gone on both rolls.** Decomposing the
near-white per-channel difference into the part a channel's own slope already produces,
`(r − 1)·w_r`, and the residual:

| roll | channel | measured | slope part | residual | % slope |
|---|---|---|---|---|---|
| Ektar | green | 0.2396 | 0.2270 | 0.0127 | 95% |
| Ektar | blue | 0.3950 | 0.1573 | 0.2377 | 40% |
| Portra 400 | green | 0.1064 | 0.1069 | −0.0004 | 100% |
| Portra 400 | blue | 0.1023 | 0.0113 | 0.0910 | 11% |

Green is **95% and 100% slope** — a per-channel *offset* adds nothing in green on either
roll, which retires the idea that a roll white point supplies the missing green term. Blue
leaves a residual on both, but 0.238 against 0.091 is not one constant.

**And on Portra the roll white point is not even self-consistent in blue**: sd 0.1619 against
a mean of 0.1023 — the failure case the probe's own guidance names, where the aggregate is
scene colour averaged. Held out, a roll-level blue offset scores RMS 0.1652 against 0.1885
for no offset at all: a 1.14x improvement, against 6.4x on Ektar.

**Reading.** Both halves of the roll-scoped idea worked on Ektar and largely failed on Portra
400. That is the shape of a measurement that was fitting one roll's idiosyncrasy, and it lines
up with what `io/scanner-density-calibration` already argues from the corpus: *a scale-shaped
correction has no generic setting worth shipping*, and the form to fit is a 3x3 plus offset.
Nothing here contradicts that; the whole-roll data now says the same thing from a second
direction, and adds that the **green** term specifically is fully accounted for by slope.

Not yet done: the visual review set covers Ektar only (`scripts/scale-review/`), and a
verdict should not be written from the numbers alone given how much of this is the eye's call.
Two rolls also remain a small n for a negative conclusion about a per-stock effect.

Gates: `fmt` clean, `clippy --all-targets -D warnings` clean, `build` OK, `cargo test` 736
unit + 190 integration 0 failed, `cargo doc` at the documented 16-link baseline, manifest
validates clean.

### 2026-09-13 — parked: the method is sound, the sample cannot carry it (user decision)

The roll-scoped measurement is **suspended** until a better sample exists. Current default
`density.scale = [1, 0.90, 0.86]` stands. Recorded here in full because the trap is subtle
and the next person will otherwise re-run it on the same assets and get the same wrong
confidence.

**What the method does.** It never identifies a grey or white patch. Per frame it bins
interior pixels by *red* density, takes the median green-vs-red and blue-vs-red balance per
bin, **subtracts the middle bin** (which deletes the absolute balance, exactly as a white
balance would), and fits the slope of what is left. The output is a *tilt* — does the balance
drift between shadows and highlights — not a level. Over many frames that tilt should average
to the systematic term, because a per-channel slope difference tilts every frame the same way
while scene colour does not.

**That last clause is the whole method, and these rolls break it.** The user's objection,
2026-09-13:

- Both whole rolls (`2026-09-09-Ektar`, `2026-09-11-Portra400`) are from **one Hawaii
  vacation** — heavily blue, some green, little red. Scene colour is not uncorrelated with
  density here; it is themed, and the theme correlates with luminance (sky and water are the
  bright subjects).
- The datasheets cannot referee it. Blue's 98% agreement with the published prediction was
  the one scene-independent check, but Ektar's sheet is the corpus outlier we are *already*
  doubting on green — internally inconsistent with its own aim table by 11%, R and G nearly
  parallel at 1.002 against everyone else's 1.02–1.05. Using the sheets to validate a
  measurement while disputing them elsewhere is circular.

**The consequence is bigger than a caveat on the numbers: the cross-roll test is confounded.**
The 2026-09-12 entry reads "the two rolls do not separate (green 1.7 se, blue 1.3 se)" as
evidence there is no per-roll effect, and therefore that the quantity is a scan-path property.
Two rolls that share a photographer, a trip and a palette produce **exactly that agreement**
from shared scene statistics alone. The comparison cannot distinguish "same scanner" from
"same subject matter", so it settles nothing — and the per-stock difference it did show
(Ektar green 0.796 vs Portra 0.878) is equally unattributable. Treat every conclusion in that
entry as provisional on the sample, not on the arithmetic.

**What would actually answer it**, in the user's order of preference:

1. **Manual review and tweak.** The user's own read is that `[1, 0.90, 0.86]` already looks
   good enough by eye. This is the status quo and needs nothing.
2. **More rolls — with different subject matter**, which is the part that matters. More
   Hawaii rolls add n without removing the confound. What breaks it is variety: indoor,
   overcast, red-dominant, no-sky.
3. **A ColorChecker or grey target, bracketed.** The only option that removes the scene from
   the measurement entirely rather than averaging over it. On the user's roadmap, not ready.
   Shared need with `io/scanner-density-calibration` (which wants a known-neutral frame for
   the 3x3) and `algo/sigmoid-parameter-calibration` (which wants a bracketed roll with a
   grey card) — one shoot could serve all three, and whoever plans it should coordinate.

**Kept, because it is ready to run the moment a better sample lands:**
`curve_probe::whole_roll_scale` and `whole_roll_white_point` over `WHOLE_ROLLS`, both
asset-gated and `#[ignore]`d, plus the two frozen recipes and the review-set generator
`scripts/scale-review/`. Add a roll to `WHOLE_ROLLS` and re-run; nothing else is needed.

**Two traps worth carrying forward regardless of sample:**

- A **density-span filter cannot exclude a calibration frame.** `2026-09-11-Portra400`'s
  `1639` is half leader and half base and has the *largest* span on the roll. Role, not span.
- A **white point is an offset and cannot yield a slope**, and the slope measurement is
  immune to a wrong film base while the offset measurement is not. Do not swap the bases or
  the estimators between the two halves.

### 2026-09-13 (later) — review correction: the headline estimator was the plain mean

Review of the working tree found that every conclusion the two probes draw was built from the
**arithmetic mean**, including frames the log itself called physically impossible. The probes
printed a median and never used it. Corrected; the parked verdict is unchanged, but two
intermediate findings moved and one **reversed**, so the numbers in the 2026-09-12 entry above
are superseded by these.

**What was wrong.** `r = dD_c/dD_r` cannot be negative — density rises with exposure in every
channel of every film — yet Ektar's `1626` (`r_b = −0.91`) and Portra's `1659` (`r_b = −0.11`)
were averaged in. They are now **refused** by `physically_possible`, and the headline is a
`trimmed` estimator (physically possible, then ±2 sd). Both are named functions so the probe,
the review set and this log cannot drift apart.

**That drift had already happened.** `scripts/scale-review/` rendered a gain
`[1, 0.773, 0.790]` described as a "±2 sd trim over 24 frames that `whole_roll_scale` prints".
No trim existed in the probe: an earlier round computed the value out-of-band, and the
rewrite that generalised the probes to two rolls dropped the trimmed block without anything
referencing it — the exact "a re-port loses what nothing references" failure CLAUDE.md
records. The generator now cites what the probe prints, `[1, 0.763, 0.773]`.

| trimmed headline | n | scale green | scale blue |
|---|---|---|---|
| 2026-09-09-Ektar | 23 | **0.763** ± 0.018 | **0.773** ± 0.033 |
| 2026-09-11-Portra400 | 24 / 25 | **0.825** ± 0.015 | **0.915** ± 0.055 |
| shipped default | | 0.900 | 0.860 |

**The reversal: the two rolls *do* separate.** 2026-09-12 reported green 1.7 se / blue 1.3 se
and read that as no per-roll effect. On the trimmed estimator it is **green 2.7 se, blue
2.2 se** — the outlier had been inflating the `se` the comparison divides by. This does *not*
revive the per-roll hypothesis, because the 2026-09-13 entry's confound stands: two rolls from
one trip separate for stock, roll or **scene** reasons indistinguishably. It does mean the
earlier "no evidence of a per-roll difference" sentence was an artefact of the estimator, not
a result.

**The slope/offset split moved, and now over-attributes.** Green reads **114%** slope on Ektar
and **153%** on Portra — above 100%, i.e. the slope term alone exceeds the measured near-white
difference. That is a real limit of the decomposition rather than a finding: the slope is fit
over p2–p98 red density and then **extrapolated** out to the p99.5 near-white point, where
real film shoulders. Treat the split as indicative of direction only. The direction still
holds and is what mattered — green's residual is ≤ 0 on both rolls, so a per-channel *offset*
adds nothing in green; blue keeps a positive residual, now 0.136 (Ektar) and 0.031 (Portra),
smaller than before but still not one constant.

**Two caveats recorded rather than acted on:**

- `2026-09-11-Portra400`'s frozen `Dmin` region was flagged by nc as *not uniform* (worst
  per-channel relative spread 0.30 against a 0.15 threshold; Ektar's is 0.18). The slope half
  is base-immune and that is verified, but the white-point half is base-**sensitive** by
  construction, and the cross-roll blue residual compares two bases of unequal confidence. The
  base is a high percentile rather than a mean, so the bias is likely small — but the Portra
  offset numbers carry that asterisk.
- **The visual-review generator is deliberately not shipped.** One was written and used to
  sanity-check the Ektar numbers, but rebasing onto #114 found that it re-introduces exactly
  the pattern that PR removed — it deleted `scripts/preset-review/generate.py` in favour of a
  data matrix rendered by `nctool review generate`. A scale review cannot be a matrix yet:
  every cell there is an `nc convert`, and this study compares nc against an **outside
  reference** (NLP's existing TIFFs), and needs a common SDR sRGB JPEG so an HDR-decoding nc
  rendition is not set beside NLP's SDR one. The requirement is recorded in the task file;
  extending `nctool review` with a reference-cell kind is the right fix, not another script.

Frame accounting is now printed — how many manifest `real` frames a roll offered, how many
were measured, and the reason each was dropped — because three independent `continue`s could
shrink a roll silently and a partially-measured roll printed the same summary as a complete
one. Both rolls report all 32 measured. The cross-roll block iterates **every pair** and names
the rolls, so the documented "add a roll and re-run" path cannot leave a third roll out of the
verdict at exit 0.

### 2026-09-16 — the shipped gain moved anyway, by a different route

The 2026-09-13 park above still stands **for the slope method**: nothing here rehabilitates a
per-density-bin tilt measured over Hawaii rolls, and that measurement is still suspended.

What changed is the sentence "current default `density.scale = [1, 0.90, 0.86]` stands". It no
longer does — `pipeline_version` 5 ships **`[1, 0.84, 0.73]`**, calibrated from 31 hand-marked
neutral patches over five rolls and shipped on a whole-set visual verdict. That is the park
entry's own **option 1, "manual review and tweak"**, carried out with more rolls and an NLP
reference rather than by eye on one set; the verdict came out the other way this time.

The full record, including why blue transfers across rolls and green splits by scan date, is
under `io/scanner-density-calibration` in `docs/progress/io.md` (2026-09-16). Read it before
re-reading the park above as a statement about the shipped value.


## ir-usability-detection
**Status:** done (#104, 2026-09-04)
**Updated:** 2026-09-13 (consolidated)

- Goal: decide whether IR can separate holder from film by measuring the plane, not by
  trusting `--film-type`.
- The measurement that motivates it (Ilford HP5, silver-halide, IR median transmission,
  2026-08-11):

  | frame | border p05 | interior p05 | interior median | separable |
  |---|---|---|---|---|
  | 1364 unexposed | 0.0229 | 0.4567 | 0.4734 | yes — 20:1 |
  | 1330 half-leader | 0.0194 | 0.0202 | 0.4620 | partly |
  | 1354 regular | 0.0186 | 0.0236 | 0.0818 | no |
  | 1329 leader | 0.0154 | 0.0151 | 0.0163 | no |

  Separability tracks the **frame's density**, not the stock's chemistry: silver blocks
  IR in proportion to accumulated density, so an unexposed frame is IR-transparent against
  an opaque holder while its own leader is opaque throughout. `silver → IR off` was wrong
  for exactly the frame `Dmin` is measured from, and right for the frame `Dmax` is.

### What shipped (2026-09-04)

- **`film_base::ir_separability`** — interior IR median (outer 10% of the short edge
  trimmed, strided sample) against `IR_USABLE_MIN_INTERIOR = 2.5 × IR_HOLDER_MAX_TRANSMISSION
  = 0.25` — gates `ir_holder_mask`. `--film-type` gates nothing; `FilmType::ir_transparent()`
  is deleted; `estimate` / `rebate_candidates` / `ir_holder_mask` take no `FilmType`.
  `inspect` and `estimate` report the verdict as `ir_separability` and echo a declared
  `--film-type` as `report.film_type` (absent when undeclared) so the flag is never
  accepted-and-ignored.
- **Evidence** (IR planes read from `../nc-assets`, derived numbers only; the probe
  reproduced the HP5 table to 4 decimals):

  | set | interior median |
  |---|---|
  | 25 chromogenic frames, 9 rolls, every role, 8 leaders among them | **0.576 – 0.728** |
  | HP5 1364 unexposed / 1330 half-leader | 0.4730 / 0.4602 |
  | HP5 1335 / 1339 (mid-density) | 0.2711 / 0.1238 |
  | HP5 1354 regular / 1341 / 1329 leader | 0.0748 / 0.0607 / 0.0165 |

  Chromogenic dye stays IR-transparent at **any** exposure, even leaders — the threshold
  sits 2.3x below the lowest of 25 frames, which is what makes the demotion safe. Silver is
  a *continuum* (0.0165–0.4730), not a two-mode split; the tightest real margin is 1335 at
  1.08x, and a wrong verdict there fails safe (dense edges classify holder and drop out).
- **The geometry trap.** A predicate built from the numbers the mask already computes is
  impossible: the 0.5% probe depth (18 px on these scans) sits *inside* the 2–5% HP5
  holder and reads 24/24 holder on all four edges of every HP5 frame — 1364 included. The
  verdict needs its own sampling geometry and must read the **interior**.
- **Open questions, answered.** Partially-separable frames use the separable edges (the
  per-segment mask already does this; 1330's opaque half drops out). `--film-type` does
  **not** override the measurement, not even to force it off — a silver declaration would
  re-break the unexposed frame; a user who distrusts the verdict has `--base-region` /
  `--film-base`. Whether the verdict serves IR dust removal is deliberately **not**
  assumed (dust is opaque *against* film; a different question) — `FilmType` is kept for
  that task to decide.
- **`--export-ir` exemption:** the two new fallback notes (unusable plane; shape-only
  plane, now firing undeclared) must respect `export_ir.is_none()` like the note they
  replace — a new warning inherits none of the exemptions its predecessor earned, and
  the first cut broke `convert --strict --export-ir` on every HDRi scan.

### The all-holder fallback, and the fact-not-prediction fix

- **An all-holder mask now falls back to RGB-only.** When the holder covers every edge at
  the probe depth — **22 of the 25 real chromogenic frames** — every segment classifies
  holder, `film_along_ranges` yields nothing on any edge, and auto refused on frames whose
  RGB-only search would have scanned inward *past* the holder. `ir_holder_mask` returns
  `None` when no edge yields a film range; the guard asks `film_along_ranges` itself, not
  "are all segments holder", so it catches the corner-trim case too. (First recorded as
  out of scope; the reviewer's counter — this change makes the failure reachable *by
  default* — decided it.)
- **That fallback silently broke `--strict`, because the orchestrator *predicted*
  consumption**: `convert` computed `auto_base && ir_present && ir_verified && ir_usable`,
  still true on a fallback frame, and suppressed "IR preserved but not used" for a plane
  that was not used — exit 0 with `warnings: null` on 22 of 25 real frames, while `inspect`
  reported it correctly. Fix: `film_base::estimate` returns
  **`BaseEstimate::ir_mask_applied`** and `rebate_candidates` takes the mask as a
  parameter, so it is built once and its outcome is a returned fact. **When a stage gains
  a new way to decline, every caller re-deriving "did the stage do it?" from the inputs
  goes wrong silently, only for the new case. Return the fact.** The consumed-plane CLI
  test now drives the all-holder frame through `convert --strict` and asserts exit 1.
- **`--auto-base` refuses on every real frame tried** (11 frames, 6 rolls, with and
  without the mask). The mask has no observable end-to-end effect on real scans today; its
  surface is `inspect`'s `holder_mask`. This task makes the *gate* correct, which is what
  `holder-masked-measurement` builds on.
- Kept, with reasons: `convert` calls `ir_separability` twice (once for the fallback
  warning's number, once inside the mask); both are bounded ~100k-value strided samples.
  `ir_separability`'s stride uses `sqrt`, and the `golden` determinism rationale now states
  the bar (IEEE-754 requires `sqrt` correctly rounded, unlike libm's `powf`/`log10`) rather
  than denying the case.

### `pipeline_version` stays 3 — a contested call, recorded in `version.rs`

- The stage-2 `base` fingerprint runs on `golden::scan()`, which has **no IR plane**, so
  *no* IR-path change can move any hash; CI staying green is not evidence nothing moved.
  Extending the gate with an IR-carrying frozen scan is unowned (the log assigned it to
  `core/conversion-versioning`, which is closed).
- **Decision (owner, 2026-09-05, against a P1 on PR #104): stay at 3.** The narrow ground:
  `--auto-base` refuses on all 11 real frames tried, the supported workflow is
  measure-once with an explicit base, and a v4 row would carry render/base/recipe hashes
  *identical* to v3's — a label with nothing the drift gate can point at. The review's two
  valid holes are recorded beside the bump triggers in `version.rs`: the written rule names
  "the film-base source **and its detector**" as a trigger and this flipped the detector
  from opt-in to default-on for every HDRi `auto` run; and neither cited precedent is
  parallel (`ir-holder-detection` shipped behind a flag; `estimation` moved no pixel).
  **Whoever next changes the film-base detector should settle it rather than inherit it**
  — the missing evidence is one frame where auto resolves a base *and* the mask changes
  which candidate wins. `holder-masked-measurement` plans a bump regardless.
- Doc traps this left behind: design-spec §12 roadmap item 15 still described the
  film-type gate after §6.1 had been rewritten (fixed); four code comments that merely
  *mentioned* "chromogenic" survived a gate-shaped grep (grep for the negation of the
  claim); `using-nc.md` had stopped saying what `--film-type` is *for* (a provenance
  declaration IR dust removal will need).


## holder-masked-measurement

**Status:** not started
**Updated:** 2026-08-11

- Goal: mask the holder per edge, then estimate the centre of the resulting single
  population. Pixel change, one `pipeline_version` bump.
- Holder depth measured on the unexposed HP5 frame: IR clears at ~2% of the short edge
  right, ~3% top/bottom, ~5% left — small, and **asymmetric**, so a rectangular crop
  must take the worst edge while per-edge masking need not.
- Why the estimator has to move with the mask: p97 exists to select the film
  sub-population out of a *mixture*. Masking makes the region one population, where an
  extreme percentile is just its noise tail — measured 0.046 density from p50 on the
  Gold 200 leader, 0.16 stops through `dA/dR = 0.5`, in the "pale" direction.
  `reference_dmax` already samples at p = 0.5 for this reason.
- Reassurance recorded so it is not re-litigated: the p03–p97 span on a leader is ~0.32
  stops, but it is **grain and scanner noise** — smooth, symmetric, no discontinuity —
  and split-half medians agree to 1.4e-4 density. Wide population, precise centre.
- Fallback is a first-class path: for silver stock IR can never separate on a leader, so
  every silver `Dmax` takes it. Provenance is per-run (user decision 2026-08-11), not a
  persisted pre-processed input.

## tiling-uniformity-validator

**Status:** not started
**Updated:** 2026-08-11

- Goal: replace the 5-cell grid with a coarse tiling in the estimate's own pass, reporting
  within-tile and between-tile variation separately. Covers `Dmax` too. No pixel change.
- The decomposition is what makes it worth doing (4x4 tiles, leader interiors):

  | roll | between-tile | within-tile p05–p95 | ratio |
  |---|---|---|---|
  | Gold 200 | 0.0081 | 0.0830 | 10 : 1 |
  | Portra 160 | 0.0390 | 0.0887 | 2.3 : 1 |

  Same scanner, ~5x difference in real spatial structure, independently reproducing the
  baseline report's finding that Portra 160's leader is the least uniform of the set.
  `(max − min)` over five patches cannot tell a slope from one bad patch, and a pooled
  percentile cannot see either.
- **Retires `--grid`** — once masking and a central estimator are unconditional it selects
  no estimator, and the tiling is free in a pass already being made.
- **`film-base/grid-verdict-enum` was removed** on 2026-08-11 and is absorbed here: its
  goal was replacing `GridEstimate.agreement: bool` and its overloaded spread sentinel
  with a self-describing verdict. That intent carries over to the tiling verdict; the
  task itself made no sense once `--grid` goes. It had no dependents.

## half-frame-calibration

**Status:** not started — **deferred, blocks nothing**
**Updated:** 2026-08-11

- Goal: let one part-unexposed, part-leader frame serve as both calibration
  references, instead of requiring two frames.
- Real case: HP5 `20260808-film-1330`, whose IR statistics show both populations in
  one frame — interior median 0.4620 (transparent, unexposed) against border p05
  0.0194 and interior p05 0.0202 (opaque).
- The caution to carry: the exposed half of a *transition* is where exposure was
  still ramping, so it may not be the film's maximum. `dmax-anchor-reliability`
  already questions a dedicated leader; a half-leader is weaker evidence still.

## clipped-dmax-reference

**Status:** not started
**Updated:** 2026-08-12

- Goal: preserve the estimate-to-recipe-to-convert workflow when a confirmed
  fully-exposed leader is clipped at the scanner's visible-light boundary, while
  keeping the fallback distinguishable from a measured `Dmax`.
- 2026-08-12: Provisional fallback decision is `1.3`. A region-only Portra 400
  sweep across Dmax 1.2–1.9 made 1.2–1.3 look best; 1.3 also matches the shipped
  nominal roll-fixed Dmax. Keep this provisional until the task evaluates more
  clipped leaders and output intents.

## auto-base-real-scan-refusal

**Status:** not started
**Updated:** 2026-09-13

- Goal: explain why the default-on auto film-base detector has never resolved a base
  on a real full-size scan, and fix it if the cause is the detector.
- Evidence so far lives under `## auto-base-redesign`, `## ir-usability-detection`
  (11 of 11 real frames refused, with and without the IR mask, 2026-09-04) and
  `docs/reports/real-scan-verification.md` row 2 (every frame refused, 2026-07-23).
- First question is whether a rebate is visible inside the holder on these scans at
  all; second is which gate refuses. The 10% scan cap against a 10–15% holder is the
  leading hypothesis, not a finding.

## holder-depth-mask

**Status:** done (2026-09-18)
**Updated:** 2026-09-18

- Goal: one per-edge holder-depth primitive (IR-measured where usable, fixed fraction
  otherwise) for `holder-masked-measurement`, `algo/auto-anchor-interior-measurement`
  and `tiling-uniformity-validator`. Split out of `holder-masked-measurement` so the
  mask is not held behind that task's estimator change and version bump. No pixel change.

### 2026-09-16 — widened to own the effective area (user decisions)

Reviewed the depth tasks with the user against what was actually recorded. The
model was right in `algo/auto-anchor-interior-measurement` and had **not** survived
the 2026-09-13 split into this task, which had gone back to modelling the static
value as a *fallback* for a missing IR mask. These decisions supersede every earlier
conflicting record:

- **This task owns the "effective area", not just the holder.** The two values are
  consumed together on every path, so they are resolved together. The scope note the
  split left behind — "the consumers' inset for the rebate is a different thing and
  stays with them" — is reversed. The one thing that would justify separating them
  again is using the holder depth to auto-crop the output, which is not planned.
- **It is the first step of every measurement path** — `Dmin`, `Dmax`, content
  exposure/contrast, tiling, roll-wide content. Exceptions exist only where the user
  states a region explicitly; nothing in nc opts itself out.
- **Two cuts in order, always both.** IR holder cut, then the static inset. The inset
  is not a substitute for a missing IR cut: it runs regardless, and where IR did not
  run it is simply the only cut. `auto-anchor-interior-measurement`'s claim that the
  two fractions are different things and must not be shared is withdrawn.
- **nc never searches for a rebate.** `Dmin` is measured either over the whole
  effective area of a reference frame or over a user-stated region. The blind inset
  removes the rebate; nothing detects it. (See the open item below — the shipped
  `--auto-base` source *is* a rebate detector, and this decision reaches it.)
- **B&W takes the same rule as everything else.** Clear IR separation → cut the holder
  automatically. No clear separation → treat the frame as having no IR, and the inset
  depth is the user's. No chemistry-keyed branch; `ir-usability-detection` already
  removed that. Consequence worth knowing: exposed silver frames often decline (HP5
  1354 reads interior IR 0.0818 against the 0.25 line), so the no-IR path is the
  normal one for B&W rather than an edge case.
- **The 10–15% holder-occupancy measurement does not force a second default.** The
  earlier open decision ("whether cut 2 takes a different default when cut 1 did not
  run") is **closed**: it stays 5% of the shorter edge, and the override is how a
  deeper holder is handled. nc measures what it can measure and reports what it did;
  sizing a blind cut is the user's call, not a number nc guesses.
- **A warning when the holder was not measured** (no IR plane, or no separation) is
  agreed in principle and explicitly **low priority** — it must not hold up the region.

Still open after this pass: the shipped `FilmBaseSource::Auto` detector (`rebate_candidates`
/ `select_auto_base`, design-spec §9) is a rebate-band search, which the no-rebate-detection
decision contradicts. Recorded here and cross-referenced from `holder-masked-measurement`
open question 4; not carried into the `auto-base-*` task files or the design spec pending
the user's call on whether that source is replaced or kept as a legacy option.

### 2026-09-16 (later) — the rebate detector is retired; Dmin/Dmax becomes area x method

The open item above is answered. **nc is not shipped, so a breaking change is
acceptable** (user), and the rebate-band search goes.

- **`Dmin`/`Dmax` measurement is rebuilt on two inputs and nothing else**: an **area**
  (the effective area, or one the user states) and a **method** (a percentile over that
  whole area, or the grid). The user leans whole-area percentile. `holder-masked-measurement`
  is re-scoped to own this — it was already the estimator task, so the migration lands
  there rather than in a new one, and it keeps its `pipeline_version` bump.
- **A user-stated area is per *usage*, not per frame.** The worked example: one `convert`
  where the user points `--base-region` at a rebate strip for `Dmin` while a content-aware
  render measures exposure and contrast over the derived effective area. So a shared
  effective-area function takes a **usage** parameter and decides per usage whether the
  user's region overrides. Explicitly **not needed yet** — the requirement is that the
  signature not preclude it, i.e. do not return one region per frame.
- **Four tasks are downstream of the retirement**, all parked with a pointer rather than
  deleted (their real-scan evidence is worth keeping): `auto-base-real-scan-refusal`
  (its "which gate refuses" question dies with the detector; "is a rebate visible at all"
  survives for `content-fallback`), `auto-base-neutral-stock` and `white-holder-support`
  (both most likely moot — a search that does not happen cannot mis-anchor, and neither
  remaining path asks about holder polarity), and `core/base-acquisition-planner`, whose
  auto rung now means "measure the effective area".
- **`content-fallback` is *not* retired** — estimating the base from picture content when
  no unexposed film exists anywhere is a different question from where to measure.

Two things deliberately left open rather than inferred:

- **Whether the grid survives as a method.** The user enumerated it and preferred
  percentile. It collides with `tiling-uniformity-validator`, which plans to retire
  `--grid` outright on the grounds that it would select no estimator. Recorded as that
  task's open question 1 so it is settled once, in the task that owns the method.
- **"Whole-area percentile" is not "keep p97".** The 2026-08-11 measurement stands:
  over a single population an extreme percentile is its noise tail, 0.046 density /
  0.16 stops in the pale direction. Which percentile is open; an extreme one is not.

### 2026-09-16 (third pass) — scope interview: the deliverable is settled

Four questions put to the user, to find the places where two readings would have built
different things:

- **Return shape: a per-edge rectangle**, `{top, bottom, left, right}` — not a
  per-segment mask. Consumers clamp bounds, so there is no mask buffer and
  `pipeline::memory` owes no new term. Accepted cost: a holder covering part of one edge
  widens that whole edge to its deepest segment. `EdgeHolderMask`'s segments remain an
  *input* to the depth, never the output.
- **Inset basis: the *original* frame's shorter edge**, not the post-holder remainder. A
  scan always insets the same pixel count whatever IR measured, so the resolved value
  does not move with a measurement and an override is predictable.
- **Deliverable: region + knob + report/`inspect` + one consumer.** `auto_dmax` takes the
  rectangle, so the inset flag is not the accepted-and-ignored knob the project forbids.
- **The consumer is wired region-only.** `algo/auto-anchor-interior-measurement` keeps the
  loud-failure range check, `measure_balance_range` and the "does `Auto` survive" question.

**One accepted risk, recorded so it is not rediscovered as a bug.** Between the two tasks
`Auto` reads a better region while an out-of-range result still renders black instead of
refusing. The user took this seam knowingly; what makes it tolerable is that
`DmaxSource::Fixed` is the default (`src/types.rs:485`) and `Auto` is opt-in via
`--auto-d-max`, so only a user who asked for it is exposed. The check is still the half
that makes `Auto` *safe* and nothing but that task will land it.

**"No pixel change" is therefore now qualified.** Default renders stay byte-identical and
`version::PIPELINE_FINGERPRINTS` does not move — its golden vectors resolve `Fixed`, never
`Auto`. A `--auto-d-max` run does change, which is the point of wiring it. Output
*dimensions* are still byte-identical on every path; nothing is ever cropped.

### 2026-09-17 — implemented (#TBD)

**Status:** done. Four chunks, all four gates green at each one, no commits.

- **`film_base::effective_area(image, inset_frac) -> EffectiveArea`** — the per-edge
  rectangle `[x, y, w, h]` plus `holder: Option<HolderDepths>` and the resolved
  `inset`. `holder_depths` marches inward per along-edge segment in
  `holder_probe_depth` steps until IR reads film, and the edge takes its **deepest**
  segment. Built on `ir_separability` + the segment helpers directly, **not** on
  `ir_holder_mask`, whose all-holder `None` is the decline this must not inherit.
  `median_ir_probe` was generalized to `median_ir_band` (a band at arbitrary depth,
  caller-owned scratch buffer) and now delegates to it, so the geometry has one copy.
- **Two bugs found by measuring rather than by reasoning**, both worth keeping:
  - **A holder *ring* makes corner segments read holder for the frame's whole
    height**, so every edge marched to its cap and the rectangle collapsed. Each
    edge must be measured only over the along-edge positions that survive the
    *perpendicular* edges' cuts — which are what we are computing, so it is a fixed
    point. `holder_depths` iterates (up to `HOLDER_MARCH_PASSES = 4`, exiting on the
    first repeat) starting untrimmed; it converges **downward** in two or three
    passes. The existing rebate search has the same problem and solves it the same
    way (`film_along_ranges` trims by the scan depth).
  - **A trailing sliver segment dragged a whole edge to the cap.**
    `edge_holder_segments`' floor-plus-leftover split leaves a remainder segment —
    harmless when each segment is classified independently, fatal under a `max`
    reduction. On Portra160 `1102` a **6 px** trailing segment never cleared and
    reported the left holder as the 900 px cap where the other 24 segments agreed on
    108-126 px: a 7x over-cut discarding 15% of the frame width, with `capped` the
    only hint. `march_edge_depth` now spreads `IR_HOLDER_SEGMENTS` evenly (every
    segment within a pixel of the others) and deliberately does *not* share the
    mask's split. `edge_holder_segments` itself is untouched — changing it would move
    the mask and the film-base path, i.e. pixels.
- **Real-scan verification, 31 IR frames across 7 rolls** (`nc inspect`, derived
  numbers only): **all 31 measured, zero capped**, depths consistent within each
  roll — top ~90, bottom ~72, left/right 18-144 px, i.e. **2.5-4% of the shorter
  edge**. That corroborates the 2-5% HP5 figure and does **not** reproduce the
  10-15% one, which came from a different measurement (top-code share vs inset on a
  *rendered* frame, `analysis/conversion-metrics`) — worth knowing before either
  number is cited as "holder depth". The 2026-09 rolls read `0,0,0,0`: measured, no
  holder, which is the already-cropped verdict and matches
  `auto-base-redesign`'s note that those scans' cropped holder defeats the rebate
  detector. Contrast `ir_holder_mask`, which declines on 22 of 25.
- **The knob is `measure.inset` / `--measure-inset FRAC`**, its own recipe section
  rather than a key under `film_base`, because the area governs every measurement
  path and `deny_unknown_fields` makes a key's section permanent. Default and bound
  live **once** in `types.rs` (`DEFAULT_MEASURE_INSET`, `MAX_MEASURE_INSET`,
  `check_measure_inset`) so `cli::validate` and `film_base::effective_area` cannot
  disagree — the `headroom_stops` precedent. The bound is a *value* rule, so it is in
  `validate`, not `validate_convert`.
  - **`inspect`/`estimate` check the flag before the decode.** They resolve no
    recipe, so `validate` never sees it, and the best-effort `effective_area` call
    would have swallowed a bad value as a warning at **exit 0**. Now exit 2 from all
    three commands, before a 160 MB read.
- **`recipe` fingerprint refreshed in place** to `e194308725fd2cf1`, no version bump:
  the default document gained `"measure": {"inset": 0.05}` while `render` and `base`
  are byte-identical. Precedent is the v1 row's `film_base.source` refresh.
- **`auto_dmax` is wired, region only.** `resolve_dmax` now takes
  `(&DensityImage, DmaxSource, Option<[u32; 4]>)` — the region threads from the
  orchestrator through `stages::render*` → `algo::reconstruct` →
  `density::reconstruct` / `sigmoid::apply_curve`, so stages stay pure. Three shape
  decisions:
  - **The `None` arm is its own path**, not a full-frame rectangle, so the
    unrestricted result is *bit-identical* to the old walk (pinned by a test on raw
    bits). Adding the parameter moved nothing.
  - **A region restricts the walk rather than adding one** — one `step_by` over the
    region's rows chained end to end, so the stride keeps a single phase and the
    visited set is a function of the rectangle and the dimensions alone. The 1 MiB
    sample cap and the ~4 MB transient are unchanged, so `pipeline::memory` owes this
    no new term (asserted, not assumed).
  - **The rectangle is clamped, not trusted** — a region resolved for other
    dimensions must not index out of bounds.
- **Measured: `Auto` resolves 0.76-1.18 across 12 real frames from 6 rolls**, all
  below the roll `Dmax` of 1.28-1.38 and inside the frames' bright content, against
  the recorded **2.23-2.37** it used to return. That is the defect
  `algo/auto-anchor-interior-measurement` was filed for, on its measurement half.
- **`convert` resolves the region only when something consumes it**
  (`consumes_measure_region`: `DmaxSource::Auto` alone today). Resolving it always
  would read the IR plane on every run and then make the "IR preserved but not used"
  note claim the plane was used on runs where the region changed nothing. This keeps
  a default conversion byte-identical **and** keeps that note honest — it is now
  keyed on `measure_area.holder.is_some()`, a **fact about what happened**, beside
  `base.ir_mask_applied`, never a prediction from the inputs. Verified both ways on
  the fixture: default anchor warns and reports no area; `--auto-d-max` reports the
  area and does not warn.
- **"No pixel change" is qualified, and the gate cannot see the difference.**
  Default renders are byte-identical and `PIPELINE_FINGERPRINTS` does not move —
  `golden`'s vectors resolve `Fixed`, never `Auto`. The `--auto-d-max` change is
  verified by the table above and by a committed synthetic opaque-border fixture,
  not by the drift gate.
- Docs: design-spec gained a `measure` section before "Film base / Dmin";
  `using-nc.md` gained "The measurement region (the effective area)" with the
  three-case `holder` table (depths / all-zero / `null`), and its `--auto-d-max` row
  and `--strict`+IR blockquote were corrected — that note now lists **two**
  consumers of the plane.

**What this did *not* do**, so it is not mistaken for finished: the loud-failure
range check on `Auto` stays with `algo/auto-anchor-interior-measurement`, which
leaves a window where `Auto` reads a better region but an out-of-range result still
renders black instead of refusing. The user took that seam knowingly (2026-09-16);
it is tolerable only because `Fixed` is the default and `Auto` is opt-in. `Dmin` /
`Dmax` estimation is untouched — that is `holder-masked-measurement`.


### 2026-09-17 (later) — review round: the honesty fixes, and the version decision

Two review engines against the implementation above. Nine findings, all fixed in the
working tree; the three that changed behaviour were one coupled defect.

- **`convert` now resolves the effective area unconditionally and always reports
  it.** The "resolve only when something consumes it" decision recorded above fixed
  one honesty problem and created three: `--measure-inset` was accepted and silently
  ignored (output byte-identical, no `effective_area`, no warning — against
  `using-nc.md`'s own "Nothing is silently ignored" section); `--strict` stopped
  failing on the "IR preserved but not used" note for `--auto-d-max` configs where
  the plane provably never reached a pixel; and `inspect` carried a measured
  `effective_area.holder` *and* that note in one report. Measured before choosing:
  on an 18.7 MP real frame the marching run is **not slower** than the non-marching
  one — the march is below run-to-run noise, so there was nothing to save.
- **Consumption is now two returned facts, never re-derived from the inputs.**
  `EffectiveArea::holder_applied` (the holder was measured *and* some depth is
  non-zero) comes back from `effective_area`, where the arithmetic happens; the note
  is suppressed only when that **and** `consumes_measure_region` hold. The previous
  guard was `holder.is_some()`, which counts the all-zero "measured, no holder"
  verdict — i.e. both committed fixtures and every 2026-09 roll. This is the
  `BaseEstimate::ir_mask_applied` shape, for the same reason.
- **`consumes_measure_region` gained `&& anchor().reads_reference()`.**
  `dmax() == Auto` alone is not consumption: a reference-free placement
  (`--anchor-black-floor`, `--anchor-mid-offset`) discards the measured anchor. The
  `film-master` rejection and roll's not-frozen warning already carried both
  conditions; this was the third site and had only one.
  - Deliberately **no presence rule** refusing `--measure-inset`: always reporting
    the resolved region is what makes the flag observable (the `IoArgs::film_type`
    precedent), and a presence rule would need ordering against the existing
    dmax/curve rules — the defect that has shipped four times here already.
- **The march's "converges downward in two or three passes" was an unproven claim,
  and is corrected.** The edge depth is a `max` over segments whose partition is
  recomputed from the *trimmed* extent, so a larger trim makes segments narrower,
  which makes a partially-holder segment **more** likely to read holder: a depth can
  *rise* as the trim rises. The map is not monotone and nothing excludes a 2-cycle.
  What is true is the observation — every frame measured so far settles in two or
  three passes. So `HolderDepths` now carries `converged`, and on exhausting
  `HOLDER_MARCH_PASSES` without a repeat the reported depths are the **elementwise
  max of the last two passes**: a cycle degrades to an over-cut (costing measurement
  area) rather than an under-cut (holder left inside the measured region). The merge
  is its own function so the rule is testable — no fixture exhibits a cycle.
- **A frame too small to fit one probe band is now "not measured" rather than
  capped.** `march_edge_depth`'s limit applies `.min(perpendicular / 2)` last, so
  where that is below the probe step the `while` loop never ran, every segment took
  the not-cleared arm, and the edge reported the cap with `capped: true` on **zero**
  bands read — against `capped`'s documented contract of being a floor on the real
  holder. `holder_depths` declines such a frame up front, which is a state every
  caller already handles. Unreachable on real scans; it was dishonest, not wrong.
- **`pipeline_version` stays 4 — recorded as a decision, not as a consequence.**
  The in-place `recipe` refresh is sanctioned and `render`/`base` really are
  byte-identical, but the v1/v3 precedents for that refresh were *new* knobs, where
  no pre-existing recipe could change meaning. Here a frozen recipe carrying
  `reconstruction.curve.dmax: "auto"` renders differently before and after this
  change under the same `pipeline_version: 4` and the same `behavior` string, on a
  path the fingerprints cannot witness (`golden`'s vectors resolve `Fixed`). That is
  a version-**identity** question, not merely the verification gap the entry above
  frames it as. Accepted because nc is unshipped, `Auto` is opt-in, and the prior
  behaviour rendered every real frame black. `core/conversion-versioning` is its
  home if revisited. The comment in `version.rs` says all of this at the row.
- **Two `shadow_metrics` probes were measuring retired behaviour.** Candidates 4 and
  7 are the shipped `DmaxSource::Auto` — that is the stated premise of their rows —
  but took the mechanical `, None` and so kept measuring the whole frame (2.23-2.37)
  where shipped now measures the effective area (0.76-1.18). They print and never
  assert, so they reported plausible numbers at exit 0 for behaviour nc retired:
  the trap this file's own header records twice. `measure_candidates` now resolves
  the area per frame and hands it to exactly those two rows; the other nine `None`
  sites use explicit anchors and correctly keep their meaning.
- **The 10-15% figure is no longer cited as a holder depth.** It is the holder's
  share of a *rendered* frame's top codes (`analysis/conversion-metrics`), not a
  measured depth, and the entry above already flagged that before either number was
  quoted as one. `types.rs`'s `DEFAULT_MEASURE_INSET` doc, the user-visible
  `check_measure_inset` message, `HOLDER_MARCH_MAX_FRAC`'s doc, `using-nc.md`,
  `design-spec.md` and this task's own file now all cite the measured 2.5-4% of the
  shorter edge and describe the other figure as what it is.
- **The `None`-region bit-identity test was a tautology** — it compared
  `resolve_dmax(.., None)` against `auto_dmax_strided`, which *is* the `None` arm. It
  now pins a literal captured from the pre-region code (`2.4`, the fixture border's
  holder density) and cross-checks the full-frame *rectangle* against it.
- Docs: `design-spec.md`'s recipe-section enumeration gained `measure` (six sections
  now, and in a `deny_unknown_fields` spec that enumeration is load-bearing);
  `using-nc.md`'s verbatim `nc params` block gained `"measure": {"inset": 0.05}`; the
  `--measure-inset 0.12` example is re-framed as the arithmetic on the *measured*
  frame it actually shows, with the `null`-scan advice stated separately; and both
  `converged` and `holder_applied` are documented beside `capped`.
- **Out of scope, for the user:** `CLAUDE.md`'s module map wants a line noting
  `effective_area` as a second orchestrator-resolved entry point in `film_base`
  beside `film_base::estimate`.

### 2026-09-17 (third pass) — re-review of the round-1 delta: prose, roll, reachability

Nine findings against the fix round above (three Medium, six Low), **no logic or
behaviour defect**. The two Medium items that follow from round 1's design —
resolving the area unconditionally on `convert`, and reporting consumption as a
returned fact — were re-confirmed and kept; only their prose and their reach moved.

- **`convert`'s "IR preserved but not used" note is reworded, not re-gated.** With
  the area resolved unconditionally, a default-anchor `convert` of an IR frame
  legitimately reports `holder_applied: true` *and* the note, so the round-1
  sentence ("not used in Step 1") contradicted the field one line above it — and the
  identical sentence meant "not measured" on `inspect` and "never reached a pixel"
  here. The note now says "not used in the **conversion** — no rendered pixel
  depends on it, whatever `effective_area.holder` measured"; `inspect`'s keeps the
  round-1 wording, so the two commands' verdicts no longer collide in one sentence.
  The verdict and the exit code are unchanged. `scripts/analysis/nctool`'s fake-nc
  fixture echoes the new text; `scripts/real-scan-verify/harness.sh`'s grep is a
  prefix of both and was not touched.
- **A `roll` frame now carries `effective_area`.** `measure.inset` is accepted from
  the shared recipe *and* from a per-frame override, and was reaching the region
  invisibly — which is precisely what round 1's remedy for the accepted-and-ignored
  flag exists to prevent, so it was fixed for three commands of four. It sits beside
  the frame's `dmax`, which is the value measured over it, and per-frame is where it
  belongs: one shared inset meets a holder depth that is genuinely each frame's own.
  Verified on Portra160 `1102`: the frame entry gains the key, and a per-frame
  `{"measure": {"inset": 0.12}}` override now shows as `inset: 432` against the
  shared `180`, and `roll_frame_report_makes_the_measurement_area_observable` pins
  that pair on the committed fixture (shared `0.05` in the recipe, resolved `55` in
  the frame) so the field cannot quietly go away again.
  The field is boxed (`Option<Box<EffectiveArea>>`) — with it unboxed
  `FrameStatus::Ok` trips `clippy::large_enum_variant`, the same reason
  `input_color` is boxed. The design-spec and `using-nc.md` claim "every command
  that decodes resolves the area and reports it" is now true **unedited**; both were
  widened only to name where a roll reports it.
- **`converged`'s guarantee is scoped to a two-phase cycle.** Max-of-the-last-two
  guarantees the over-cut for a two-phase cycle and for a monotone transient, but a
  period-≥3 cycle can still under-cut: passes 3 and 4 are only two of its phases.
  The merge is deliberately **not** widened to all four passes — pass 1 is the
  untrimmed, corner-contaminated upper bound and would over-cut every ordinary ring
  frame. The rustdoc also now records the consequence: the merge can raise
  `top + bottom` to the frame height, which with the inset trips `effective_area`'s
  empty-region refusal, so a far enough over-cut is a hard refusal (exit 2, or a
  failed roll frame) rather than a degraded measurement. `using-nc.md`'s
  `converged: false` bullet carries the same two caveats; "no real scan has produced
  this" stands.
- **The exhaustion path is now reachable, and tested.** The march loop is extracted
  as `march_to_fixed_point(image, ir, step, passes)`; at `HOLDER_MARCH_PASSES` no
  fixture cycles, so `passes = 1` on the existing ring fixture is what exercises the
  `!converged` merge (pass 1 cannot repeat the all-zero start) and both of its lines.
  The wiring was already correct — this is about it staying correct.
- **The timing measurement behind "resolve unconditionally" is scoped, not
  reversed.** The march measured free on an 18.7 MP frame, and that frame
  **converged**: a capped frame runs up to 4 passes x 4 edges x 24 segments x ~50
  bands, roughly 50x the work. Zero of 31 real frames capped, so the decision
  stands — but the evidence covers the converging case, which is every real frame
  measured so far, not the capped worst case.
- Two rustdoc corrections: `Report::effective_area` was still scoped
  "(`inspect` / `estimate`)", and `film_base::effective_area`'s said it "is called
  per measurement rather than resolved once per frame" when every caller resolves it
  once per frame — the real constraint is only that the signature not *preclude* a
  per-usage region, which it does not.
- `inspect`'s third IR-note branch had a comment describing the all-holder fallback,
  which the round-1 `ir_consumed` disjunct excludes (an all-holder frame marches to a
  ring, so `holder_applied` is true — verified on `1102`: `holder_mask: null`,
  `ir_separability.usable: true`, holder measured, and the note does not fire). The
  comment now says what the branch actually covers ("both readers declined for a
  reason neither note above named", with the effective-area error path warning
  separately) and the branch is **kept** as defence for a future third IR reader.
- `shadow_metrics`' per-frame area resolution no longer ends in `.ok()`: degrading to
  `None` there is exactly the retired whole-frame walk the comment above it guards
  against, and a probe only prints. It now prints a loud line and skips the frame.

### 2026-09-17 (ship review) — a different reviewer, four Mediums the loop missed

The two-engine review loop above converged; a **ship review by a different reviewer**
then found four Medium and seven Low findings. All fixed in the working tree, no
commits. The pattern worth keeping: that reviewer was strong on this project's
conventions and weak on premises, so two of its four recommended remedies were
wrong and are recorded below with what was done instead.

- **The empty-region refusal fired on runs that read no region, with a remedy that
  could not work** (M1). `convert_frame` called `effective_area(...)?`
  unconditionally, *before* asking whether anything consumed the region, so a run
  whose output would be byte-identical failed at exit 2 with nothing written — while
  `inspect`/`estimate` degraded the identical measurement to a warning at exit 0.
  That contradicted the rationale comment directly above the call. Now: a refusal
  only for a run that measures over the region, a warning otherwise, with
  `effective_area` omitted (there is no region to report). The knock-on —
  `--measure-inset` inert on such a run — is correct, because the warning is the
  observable. And the message's "or state a region explicitly" was the **fifth**
  instance of the remedy-that-does-not-work defect CLAUDE.md records: `effective_area`
  takes only the image and the inset, so `--base-region` never reaches it. It now
  names lowering the inset, plus dropping `--auto-d-max` on the fatal path only,
  where that is what measures over the region.
- **A capped edge inflates the perpendicular edges tenfold, at `converged: true`**
  (M2). The trim can only remove as much holder as the perpendicular edge
  *reported*, so once a true depth exceeds `HOLDER_MARCH_MAX_FRAC` the residual strip
  stays inside the perpendicular edges' extent and they cap too — at a **stable**
  fixed point. Measured: 400x400, 120 px top holder, 10 px sides →
  `{100, 10, 100, 100}`, a 10x over-cut discarding 45% of the frame width, exit 0.
  This round fixes the **reporting**, not the algorithm: `capped` is now per edge
  (`CappedEdges`, with `contaminated()` = "a capped edge has a capped perpendicular
  neighbour", the exact condition under which a trim was truncated). The deep fix is
  filed as `film-base/holder-cap-contamination`, which records why **raising the cap
  is rejected** — it widens M1's refusal into ordinary use (the predicate is
  `2*(cap_frac + inset) >= 1`, so 0.35 makes `inset >= 0.15` fail) and removes the
  only bound on an ambiguous IR read.
  - **The committed `[4, 4, 60, 4]` fixture was already a contamination case**,
    reporting `{50, 50, 50, 4}` where top and bottom are 4 px — 12x out on two edges.
    Its test asserted only `d.left == 50` and the frame-wide `capped`, so it passed.
    The per-edge flags surfaced it on the first run. Evidence that M2 was reachable
    from a *committed* fixture, not only from a hand-built one.
- **`capped` and `converged` emitted no warning, so `--strict` could not see either**
  (M4, the task's own open question 3, now closed). Both mean "the reported rectangle
  is not a measurement", which CLAUDE.md's fail-loudly rule puts in a warning, not a
  `Serialize`-only field — and the progress entry above records this exact class of
  bug already happening once during implementation (the 6 px sliver reporting a 900 px
  cap, caught only because someone was reading the numbers).
  `film_base::effective_area_warnings` is a pure function all three commands push, so
  the field and the warning cannot disagree. This is what converts M2 from silently
  wrong to loudly suspect.
- **The reported `dmax` was holder-contaminated under a base-derived anchor** (M3),
  and the reviewer's one-line remedy was wrong. `consumes_measure_region` was one flag
  doing two jobs, and the two questions have different answers:
  - *What region does the measurement use?* → `measures_over_region`, keyed on
    `DmaxSource::Auto` alone. Under `--anchor-black-floor` / `--anchor-mid-offset` the
    region moved and the reported `dmax` did not (1.6848611 at `--measure-inset 0.4`
    and at the default), so one report field meant the effective area under one
    placement and the whole frame under another — and the contaminated value is the
    one the calibrate-once workflow has users copy into `--d-max`, i.e. the 2.23-2.37
    figure this task exists to eliminate.
  - *Did the IR plane change a rendered pixel?* → `region_reaches_a_rendered_pixel`,
    which keeps `Auto && reads_reference()`. **The reviewer proposed dropping that
    conjunct outright**, which would have re-broken the fix two entries above:
    `--strict` stopped failing on runs where the plane provably never reached a pixel,
    and `strict_still_fails_when_an_auto_dmax_anchor_reads_no_reference` pins it. Two
    predicates, two rustdocs saying which question each answers, and both behaviours
    intact. Pixels are byte-identical under the base-derived placements — the same
    fact the old gate relied on, used in the other direction. Accepted consequence:
    `sigmoid::apply_curve`'s finite-and-positive guard now sees the region-restricted
    value, so a genuinely degenerate measurement can newly fail loudly there.
  - Side effect: `shadow_metrics` keys its probe region on `AnchorRule::Auto`, which
    the review noted disagreed with the shipped gate. It now agrees, without a change.
- **Correction to the third-pass entry above** (L1; recorded here rather than edited
  in place, per the append-only rule). That entry, and `HolderDepths::converged`'s
  rustdoc, claimed the last-two-passes max guarantees an over-cut "for a monotone
  transient in either direction". False for a transient settling **upward**:
  `max(p3, p4) = p4` then sits *below* the fixed point, which is an under-cut. The
  honest statement, now in the rustdoc, is a two-phase cycle and a transient settling
  **downward**. (Unlikely in practice for the same reason the merge is not widened:
  pass 1 is measured untrimmed and is the corner-contaminated upper bound, so the
  sequence starts high.)
- **`converged` and `capped` are documented as independent and are not.** A cap
  *creates* a stable fixed point, so the worst answer the march can produce comes back
  `converged: true`. Said in the rustdoc and in `using-nc.md`, where the caveat sat
  under the `converged: false` bullet while the reachable risk is one bullet up.
- Doc fixes: `using-nc.md`'s opening "Every number nc measures off a frame is
  measured over the effective area" contradicted its own closing "the `Dmin`/`Dmax`
  estimators still measure as they always have" 84 lines later — one consumer exists,
  so the guide now says one measurement (the aspiration stays in design-spec §1861,
  which states intent and is correct as written). The `null`-holder advice told users
  to raise the inset "past" 2.5-4% when the default is already 5%; the right
  reasoning is `DEFAULT_MEASURE_INSET`'s — where cut 1 did not run, the same 5% must
  clear the holder **and** its rebate. And `estimate`'s IR-note comment now says its
  scoping to "for the film base" is **deliberate and load-bearing**, so the next
  reader does not "fix" the three commands' divergent rules by adding the march
  disjunct and make that message wrong.
- **A premise that became load-bearing without being stated** (L7, recorded on
  `EffectiveArea::holder`): `ir_separability` samples only the interior (border
  trimmed 10%) *by design*, so `usable: true` asserts nothing about whether an
  IR-opaque holder exists — and the user-facing "measured, and there is no holder"
  verdict therefore rests on the march's own outermost band with no corroboration.
  No plausible failing case could be constructed and the project's own physical
  premise cuts against one (`white-holder-support`: IR opacity is thickness, not
  colour), so this is an unstated assumption, not a defect.
- Report shape changed (`capped` bool → object); `render`, `base` and `recipe`
  fingerprints all unmoved, which the green gate pins. 780 + 197 tests (was 778 + 194).

### 2026-09-18 — close-out

**Landed.** `film_base::effective_area(image, inset_frac)` returns a per-edge
rectangle plus `holder: Option<HolderDepths>`, `holder_applied`, and the resolved
`inset`. Two cuts in order: the IR holder march (per along-edge segment, in
`holder_probe_depth` steps, deepest segment wins the edge), then a static inset from
the **original** shorter edge. `measure.inset` / `--measure-inset` is the knob, the
recipe's sixth top-level section; default and bound live once in `types.rs`
(`DEFAULT_MEASURE_INSET`, `MAX_MEASURE_INSET`, `check_measure_inset`) so the stage
and `cli::validate` cannot disagree. Every decoding command resolves and reports it.

**Verified.** Four Rust gates green (781 + 197, counts read), `nctool` 284 OK,
`cargo doc` at the 16-link baseline, `render`/`base` fingerprints unmoved with only
the sanctioned in-place `recipe` refresh. Real scans: 31 IR frames across 7 rolls all
measured, none capped, holder 2.5-4% of the shorter edge; the 2026-09 rolls read
all-zero (already cropped). `--auto-d-max` resolves 0.76-1.18 across 12 real frames
from 6 rolls, against the 2.23-2.37 it used to return — the defect
`algo/auto-anchor-interior-measurement` was filed for, on its measurement half.

**What a dependent task needs to know.**

- **Call `effective_area`; do not re-derive any of it.** `holder: None` means *not
  measured* (no IR plane, shape-only, or film too IR-opaque — routine for silver);
  all-zero depths mean measured-and-none, the already-cropped case. `holder_applied`
  is the returned fact for "did the IR read move the rectangle", and
  `EffectiveArea` is `Copy`.
- **Read `converged` together with per-edge `capped`, never alone.** A capped edge
  inflates its *perpendicular* edges at a stable fixed point, so the worst answer the
  march produces reports `converged: true`. `film-base/holder-cap-contamination` owns
  the narrowing; raising `HOLDER_MARCH_MAX_FRAC` was considered and rejected (it
  widens the empty-region refusal into ordinary use at `inset >= 0.15`).
  `effective_area_warnings` is the `--strict`-promotable channel for both flags.
- **Two predicates, not one.** `measures_over_region` (`DmaxSource::Auto`) decides
  what the measurement uses; `region_reaches_a_rendered_pixel` (`Auto` *and*
  `anchor().reads_reference()`) decides whether the IR plane moved a pixel. Add a new
  consumer's condition to whichever one it belongs to — collapsing them breaks either
  report honesty or `--strict`, and both have shipped once.
- **The estimators are untouched.** `Dmin`/`Dmax` still measure as they always have;
  moving them onto this region is `film-base/holder-masked-measurement`, which also
  owns the area x method rebuild. `algo/auto-anchor-interior-measurement` still owns
  the loud-failure range check that makes `Auto` safe — the region landed without it.
- **`pipeline_version` was deliberately not bumped** and that is recorded as a
  decision in `version.rs`: a frozen `dmax: "auto"` recipe renders differently across
  this change under the same version label, on a path no fingerprint witnesses.
  `core/conversion-versioning` is its home if revisited.

### 2026-09-17 (later) — review round 3: the inset carries the march's resolution

Three P2 findings from the PR review. Two were prose (the "remedy must actually work"
and "over-general claim" classes, both repeats); one was real behaviour.

- **`march_edge_depth` reports the *start* of the first band whose median reads film**,
  and a film median only means the holder covers less than half that band — so up to
  ~`step/2` of holder can sit inboard of any measured depth, **including a measured
  zero**. At the default the inset absorbs it (180 px against an 18 px step on a
  3600 px frame), which is why an earlier round cleared it; at the documented
  `--measure-inset 0` the residual *is* the whole defence, and a 9-18 px holder ring
  is ~1-2% of the region at `SCAN_EPSILON` density — enough to own the percentile
  `auto_dmax` reads. **Fix: `effective_area` floors the applied inset at one
  `holder_probe_depth` step wherever the march ran**, so the mixed band is absorbed by
  construction. No measured depth moves, so no fixture, doc figure or `--auto-d-max`
  number changes; reporting `depth + step` instead would have moved all of them.
  - **Keyed on `holder.is_some()`, not `holder_applied`** (the review proposed the
    latter). A sub-band holder reads *zero* on the edges it covers, so
    `holder_applied` is exactly false in the case with the thinnest margin — the
    keying would have left the defect open there. Where the holder was not measured
    at all there is no measurement resolution to respect and a stated `0` is exact.
  - A stated `0` is therefore **deliberately not honoured exactly** on a measured
    frame. `EffectiveArea::inset` is documented and reported as the *applied* value,
    which is what makes the floor visible; verified on Portra160 `1121`
    (`--measure-inset 0` → `inset: 18`, default → `180`, unchanged).
  - The empty-region refusal grew a second remedy: where the floor set the inset,
    "lower the fraction" cannot work, so it says so instead.
- **`check_measure_inset`'s over-maximum message** still ended "state a region
  explicitly instead" — the sibling of the `film_base.rs` wording fixed in round 1,
  and the sixth instance of the class. `--base-region` sets the *film-base source* and
  never reaches `effective_area`; there is no explicit measurement region. It now says
  lower the fraction, or drop what measures over the area (`--auto-d-max`), and a
  `cli::validate` test asserts the retired wording's **absence** as well as the new
  one's presence.
- **The `!converged` warning claimed an unconditional over-cut**, which
  `HolderDepths::converged`'s rustdoc had already been corrected away from: the merge
  guarantees that direction only for a two-phase cycle or a march still settling
  downward, and a longer cycle or an upward transient can leave holder inside the
  region. The user-facing copy now says both, and two further copies (the inline
  comment in `holder_depths`, a test's name and comment) were corrected with it.
  `docs/using-nc.md` already carried the honest version.

# Hanten — nf-display-stages Progress Log

Execution log for the `nf-display-stages` epic: what was done and how, key decisions, what
works, what doesn't. TASKS.md holds the authoritative status; this file is the
narrative beside it.

One `##` section per task in this epic, named by the bare task name. Read this
whole file before starting a task in this epic, and read other epics' `Epic
summary` sections when you depend on them. Append entries — don't rewrite earlier
ones.

## Epic summary

Fit range and fit gamut as real stages, shared by both display branches, plus the operator question the shadow end raises.

The epic was created on 2026-09-19 as part of the new-flow migration plan
(`docs/nf-migration.md`).

**Fit range has landed** (`fit-range`, 2026-09-23): one reinhard with the display's
peak as its argument, `Y′ = r(Y)·(1 + (P − 1)·s(Y))` on ACEScg luminance. Every peak
agrees bit for bit below diffuse white (`algo::fixed::DIFFUSE_WHITE = 1.0`); above `W`
the output exceeds the peak on every branch and the encoder counts it — the one open
trade for an HDR destination. Knob `fit_range.headroom_stops` (`--display-tone-headroom`);
the peak is the destination's. Non-finite samples are refused.

**The gamut map's share of highlight desaturation is near zero where it matters**
(`gamut-map-share`, done 2026-09-23; `docs/reports/gamut-map-share.md`). It moves no
marked white under `sigmoid-knees`, the desaturation spike's control, or
`path-to-white`'s hand-set contrast, and every limit it hit was the cube's top, never
P3's primaries. It acts heavily only under the `shoulder` display tone, whose plateau
leaves no chroma room at the cube's top.

So under the `none` and `reinhard` tones in SDR, `fit-gamut` need not shrink the map to
make room for the look's operator. Re-check that if fit range's operator plateaus near
display white, and for HDR, which was not measured.

The map gets no diagnostic off switch. Should one ever be wanted, "off" means an unmapped
float destination, never a per-channel clip.

## fit-range

**Status:** done
**Updated:** 2026-09-23

- 2026-09-19: created with the new-flow plan. Goal: fit range as one stage.
- 2026-09-23: **done.** `pipeline::fit_range` is one operator with the display's peak
  as its argument: `Y′ = r(Y)·(1 + (P − 1)·s(Y))` on ACEScg luminance, channels scaled
  by `Y′/Y`. `r` is the mid-grey-preserving extended reinhard at `W = 2^headroom_stops`
  (bit-identical to `display_tone::extended_reinhard`, pinned while both exist); `s` is
  a smoothstep in stops from diffuse white to `W`. Decisions (user, 2026-09-23):
  - **Both peaks in one function now**, though no HDR destination reaches it yet.
    Chosen over legacy's HDR form (asymptotic base + lift): `P = 1` is exactly reinhard
    and transcendental-free, and **every peak agrees bit for bit below diffuse white**
    (legacy: within 0.03%). The price: `r`'s `v/W²` tail survives, so content above
    `W` exceeds `P` on HDR as it exceeds `1.0` on SDR — `f(W) = P·r(W)`, 1.006·P at six
    stops — clamped and counted at the encode. Legacy dropped the tail to hold HDR
    strictly under its 1000-nit peak (measured: an unbounded base peaked 5.3–17.0
    against 4.93 on seven frames). Revisit in `branch-contract` if an HDR destination
    needs a hard ceiling.
  - **Diffuse white is `algo::fixed::DIFFUSE_WHITE = 1.0`**, the value the decode
    renders its anchor to (≈0.08 stop from the datasheets' diffuse white). The lift
    starts there; `nf-look/path-to-white` reads the same constant.
  - **Knob: `fit_range.headroom_stops`** (default 6, `0`–`24`, `0` the identity) via the
    existing `--display-tone-headroom`, now kept under `--new-flow`. The peak is the
    destination's (`chain_params(peak, gamut)`), never a recipe key. No selector:
    `--display-tone` is refused at every value (`Never`), and `--sigmoid-shoulder`
    became `Never`, pointing at the headroom.
  - **Non-finite samples are refused**, naming the lowest pixel, at every headroom;
    luminance ≤ 0 passes through untouched. Luminance uses a new pinned
    `ACESCG_LUMA` (exact derivation, audited).
  - Report: `new_flow.fit_range` = `{operator, headroom_stops, white_point,
    display_peak}`; the stage list reads `reinhard-peak-lifted-v1`, or `identity` when
    the white point is 1.
  - Goldens (`chain_golden`): SDR bit-exact; HDR windowed over `log2` and asserting
    bit-equality with SDR below white; threaded vectors recaptured at the shipped
    headroom and moved to the seven finite pixels (the NaN pixel's refusal is its own
    test). Mutation-checked: `MID_GREY` 0.1801 reds SDR, HDR and threaded; a linear
    ramp in place of the smoothstep reds HDR.
  - On `hdr-48bit.tif` at the guide's base: 0.12% of samples clipped at the default,
    32.23% at zero headroom — byte-identical to the pre-task binary. The guide's quoted
    9.88% for that command was already stale before this change.
  - Not done here: fit gamut does not yet read the peak (`fit-gamut` adds it to
    `RangeFittedImage` when its ceiling needs it); display black / a toe
    (`parametric-operator`).
- 2026-09-24: review round (`/code-review high`), all fixed:
  - **Luminance ≤ 0 is no longer passed through unscaled.** `f(Y)/Y → gain` as
    `Y → 0⁺` (≈1.22 at six stops), so scale 1 there stepped every channel ~22% where a
    saturated colour's luminance crosses zero. Those pixels now take the limit, the
    mid-grey gain (user decision) — continuous, still unclamped. No golden moved (the
    fixtures hold no such pixel); a unit test pins the continuity.
  - **The headroom rule is `types::headroom_fault`**, one predicate returning which
    rule failed; the current chain's `check_headroom_stops` and the new chain's
    `validate_fit_range` word their own messages from it, and the stage re-checks it.
    `FitRangeParams::check` and the placeholder peak it needed are gone.
  - **"Bit-reproducible across targets" was too strong**: `W = 2^stops` is an `exp2`
    call, exact only at whole stops. The module doc now says so.
  - `--display-tone-headroom`'s help names both chains' recipe keys; the stale "a
    non-finite sample reaches the encoder" comments in `scene_correction`, `fit_gamut`
    and `working_image` now say fit range refuses it; the goldens and tests state the
    HDR peak themselves instead of importing `hdr::LINEAR_HEADROOM`, which retires.
  - Declined: a v2 recipe saved before this renders differently (unversioned new-flow
    output is `nf-verification/fingerprints`' gap); overflow of the scale at huge
    luminance (finite for any f32 at the default); the reinhard duplicate of
    `display_tone` (the migration rule; a test pins them bit-identical).
- 2026-09-24: rebased onto `nf-retire/sigmoid-and-simple` (#151), which removed the
  `--sigmoid-*` flags and `--reconstruction` outright. So the `--sigmoid-toe` /
  `--sigmoid-shoulder` availability rows this task had reworded are gone with them;
  instead the removed-flag message for both knees now names `--display-tone-headroom`
  as the `--new-flow` remedy (it said "not yet available under `--new-flow`"), and its
  test checks the new flow accepts that flag.
- 2026-09-24: rebased onto `nf-scene-correction/roll-white-balance` (#152), which
  retired the new chain's per-frame auto white balance and dropped `chain::render`'s
  measurement region. The threaded auto-white-balance golden went with it; the fit-range
  goldens, the finite-pixel input and the NaN-refusal test carry over unchanged.

## fit-gamut

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: one gamut-mapping implementation.

## parametric-operator

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: a parametric operator with a toe.

## branch-contract

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: the sdr/hdr branch contract.

## gamut-map-share

**Status:** done
**Updated:** 2026-09-23

- 2026-09-22: filed, split out of `nf-look/path-to-white`. Goal: how much of the
  highlight chroma convergence nc already produces is the **gamut map** rather than a
  tone or reconstruction curve. Two things rest on it: `path-to-white` must not double up
  with the map, and the map is one of only two surviving candidates for the knee'd
  sigmoid's clean whites after design-update Appendix F eliminated the other three
  (`sdr.rs:249-266`, radial near luminance 1.0 with a ceiling that follows the rendered
  luminance — which is how the display tone reaches it indirectly although no nc tone can
  converge channels itself). Nothing turns the map off by flag, which is exactly why
  `nf-look/desaturation-spike` could not separate them; `--display-tone shoulder` failed
  as a separator because it drives top-end chroma to 0.0 with 17-29 degrees of rotation,
  i.e. it flattens rather than shapes. Runs against today's binary. Two routes: a
  throwaway patch disabling the map (measures the share) or counting top-end samples out
  of gamut before mapping (bounds it, no patch).
- 2026-09-23: **done.** Report: `docs/reports/gamut-map-share.md`; scripts, job file,
  raw results and the throwaway patch (`probe.diff`) in `../temp/gamut-map-share/`.
  - **Route: a throwaway in-crate probe, not a binary patch.** An env-var patch skipping
    the map would have handed the overshoot to the u16 encode, whose per-channel clip is
    a gamut policy of its own, so it would have measured map-vs-clip. The probe split
    `render_destination_pixel` into tone-scale and map, read both sides in float, and
    asserted its mapped output bit-identical to `sdr::render` (403 of 403 accepted
    renders). The task's "counting needs no patch" was wrong: nothing reports the
    pre-map values, so counting needs the same patch.
  - **Scope:** 92 frames on Gold200 0918, Ektar100 0909, Portra400 0911 and 0920; 41
    marked whites (17 on 0920, marked for this task). Five neutral-patch marks were lost
    to frames since pruned from the assets (Ektar 1615/1620/1623, Portra 1643/1652).
  - **Knees: 0.00% of top-end pixels touched on every roll**, so the knee'd whites are the
    shoulder's. The 2x2 with `--sigmoid-shoulder 0` is not a share: that render is
    refused on every frame, and in float the map greys what overshoots, so the map-first
    ordering credits it 93-130%.
  - **Spike control / C / D (reinhard): mean C\* removed 0-2.6 per roll**, from a few
    bright frames; no marked patch moved (largest 0.001).
  - **`path-to-white`'s hand-set spelling named no display tone**, so it meant `shoulder`,
    under which the map removes 5.3-13.8 on average (worst 52). Corrected there to
    `--display-tone reinhard`.
  - Off switch: none, on either chain (user decision 2026-09-23).

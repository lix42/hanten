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

No stage has landed yet: the epic was created on 2026-09-19 as part of the new-flow
migration plan (`docs/nf-migration.md`). One measurement task is done (below).

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

**Status:** not started
**Updated:** 2026-09-19

- 2026-09-19: created with the new-flow plan. Goal: fit range as one stage.

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

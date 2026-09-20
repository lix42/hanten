# Tune `scale` and `gamma` by review

## Goal

Settle the two knobs the decode genuinely owns — `scale` (a cast that grows with
brightness) and `gamma`'s linearization half — by repeated visual review against a
held-fixed rendering. Common ground, not per-frame neutrality.

## Design

- **Rendering held fixed is what makes a review set evidence about the decode**
  (design-update Part 3). The rendering to hold is the direct destination: scene
  correction identity, look empty. Both knobs act before the 3×3, out of a grade's
  reach.
- **Order: `scale` first, then `gamma`** — the cast is the open question, contrast
  is easier once it is settled, and two or three candidates per review set keeps a
  frame's toggle manageable.
- **The method caution is the important part.** A candidate built from patch
  arithmetic can win on the patches and lose on the picture — the 2026-09-17 offset
  round — and the disagreement is information about the *measurement*, not only the
  candidate. Judging cast on whites also favours any config with a per-channel
  highlight compression, so prefer midtone neutrals.

This supersedes two tasks:

- **[`algo/sigmoid-parameter-calibration`](../algo/sigmoid-parameter-calibration.md).**
  Its parameters are the knee'd sigmoid's and go with it; its argument survives
  intact — per-frame exposure preference cannot select a parameter, because asking
  which EV looks best *is* frame optimisation, so only known references (a
  bracketed roll plus a grey card) make exposure preservation verifiable.
- **[`film-base/dmax-per-channel-reduction`](../film-base/dmax-per-channel-reduction.md),
  partly done.** Its finding is what matters here: the per-channel term is a
  *slope*, carried by `density.scale`, not the anchor it set out to weigh, and the
  leader is disqualified as a source. See `docs/progress/film-base.md` (2026-09-10,
  2026-09-13); do not re-plan that investigation.

Open: how many rounds this is worth before the calibration frames exist, and
whether `scale` stays one global value once a second stock family is in the set.

## How to Verify

- Each round records its candidates, the set judged, and the verdict — including
  rounds that change nothing. Values move on a whole-set verdict, never one frame.
- Any pixel move carries its `pipeline_version` bump and fingerprint row.

## Dependencies

- [The destination set](../nf-destinations/preset-set.md) — the fixed rendering the
  loop holds is a destination
- [The frozen reference build](../nf-verification/reference-snapshot.md)
- [A `density.scale` ladder, before the calibration frames exist](scale-ladder.md)
  — inherits its candidate value and its method caution

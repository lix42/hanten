# Fit the desaturation band on more than one patch

## Goal

Place the saturation band that guards [the path to white](path-to-white.md) on enough
marked patches, across enough rolls, that its numbers are evidence rather than an
example. [The spike](desaturation-spike.md) demonstrated the **mechanism** on a single
saturated patch; it says in as many words that the shape carries and the parameters do
not.

## Why it is its own task

It runs against **today's binary** — the operator is a throwaway patch, the anchor is
set by flag, the patches are marked in the review app — so it does not wait for the
look stage, and `path-to-white` should start with real numbers instead of a shape.

## Design

What the spike left:

- The band is over linear-RGB saturation `(max − min)/max`: full pull below `s0`, off
  above `s1`, linear between, multiplying the brightness term. Placed at **0.30 → 0.45**
  from 12 patches on `2026-09-18-Gold200`, where the whites-with-cast ran **0.137–0.279**
  and the one genuinely coloured surface ("sand beach") sat at **0.463**. One example
  above the band is what the placement rests on.
- **The gap is real but not wide** (0.279 against 0.463), which is why a band and not a
  single knee — and why more saturated examples are the thing that moves it.
- **A perceptual saturation measure may separate the cases better.** Linear-RGB was the
  cheap choice, not a considered one.

What this task adds: more marked patches, chosen to populate the **upper** half of the
range — bright saturated surfaces (sunsets, painted walls, flowers, skin in sun) — on
rolls other than the one it was fitted to, and a placement justified by the resulting
distribution.

**Measure under the anchor the design expects to ship, not the spike's.** The spike
pinned white per frame at p97, a level move. `path-to-white` is being built under a
hand-set candidate C/D contrast ([`docs/spike/white-placement.md`](../../spike/white-placement.md)),
which steepens everything below white and therefore changes which pixels land in the
operator's range. Re-placing the band under the wrong white is the same work twice.

## Open questions

- **Which saturation measure.** Linear-RGB `(max − min)/max` against a perceptual one;
  the second costs more and may separate a bright white from a bright colour better.
- **Whether one band serves every roll**, or the separation is roll-dependent the way
  the decode's cast direction is.
- **What "enough" is.** The failure mode is known — an aggregate cannot see these
  surfaces, one marked patch found the case eight frames of averaging hid — so the count
  that matters is saturated patches, not frames.

## How to Verify

- A distribution, not a pair: the marked patches plotted by saturation, with the
  whites-with-cast and the genuinely-coloured populations visibly separated, and `s0`/`s1`
  placed from that separation.
- The placement holds on a roll it was not fitted to.
- A patch above `s1` keeps its chroma while the whites below `s0` clean identically —
  the same pair check `path-to-white` uses, since either half alone passes a broken
  operator.

## Dependencies

- [Spike: what form should highlight desaturation take?](desaturation-spike.md)
  — **done 2026-09-21.** Settled the form and the band's shape; left its numbers
  fitted to one patch on one roll.

Depended on by [the path to white](path-to-white.md), which ships the values.

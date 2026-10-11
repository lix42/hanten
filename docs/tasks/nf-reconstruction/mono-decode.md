# Where black-and-white fits the new chain

## Goal

Decide, and record, where channel pooling happens for a monochrome negative in the
new chain, and what the NC film RGB v1 3×3 means once there is one image-forming
layer. This is a **gap, not a feature**: the design is written for three dye layers
end to end and says nothing about a B&W scan.

## Design

The whole decode is per channel. `scale` and `offset` are RGB arrays fitted on colour
dye layers; the 3×3 declares the film's channels to be Rec.709 primaries; the look
stage's grade is per channel about mid-grey. A silver B&W negative has none of that
structure, and a chromogenic (C-41) B&W negative has dyes but prints mono — so "B&W"
is not one case. The candidate stages, each with a consequence to weigh:

- **In the decode, before the matrix.** Simplest signal, but it makes the matrix
  meaningless for mono and `film-master` — the artifact a reconstruction is measured
  on — single-valued, and it applies a calibration fitted on colour dyes to a medium
  it was not fitted for.
- **In scene correction.** White balance under mono cannot tint, which is what NLP's
  B&W mode advertises; it can still shift tonality, so mono is not the same as "white
  balance is a no-op".
- **In the look stage.** Treats mono as a rendering choice — right for a *colour*
  scan rendered mono, and it leaves a genuinely monochrome negative going through
  three channels of colour machinery to get there.

Those two cases may want different answers. Saying so explicitly is part of the
deliverable.

## Decision

Recorded in [`docs/design/mono.md`](../../design/mono.md). This task stays `[~]` until
the HP5 measurements land: one frame recorded as one gray channel and as three RGB
channels. They settle the pooling domain and weights, silver's per-channel base, and
IR on silver.

## What depends on this

[`algo/bw-support`](../algo/bw-support.md) is superseded by
[`mono-input`](mono-input.md) (silver, in the decode) and
[`nf-look/mono-mix`](../nf-look/mono-mix.md) (a dye negative rendered mono, in the
look). [`io/gray-primary-decode`](../io/gray-primary-decode.md)'s first open question
is answered: copy the one channel into three.

## Open questions

- *Answered:* does `density.scale` apply to silver? No, the mono decode reads only the
  red entries. Chromogenic B&W takes the colour route for now (pending a scan) and keeps the colour `scale`
  until measured.
- *Answered:* mode or stage? Neither: mono enters in the decode (silver) or in the
  look (a dye negative); after the decode no stage needs a mono branch.
- *Answered:* grayscale encode or R = G = B? Three channels with R = G = B; a
  grayscale encode is a follow-up.
- *Pending the HP5 scans:* whether silver gets its own `scale` default, and whether
  the one-channel and pooled three-channel decodes agree.

## How to Verify

This is a decision task and may ship little or no code. It owes a written answer
naming the stage, the treatment of the 3×3, and whether silver and chromogenic B&W
differ — recorded where the two carried tasks can cite it, with their own open
questions then closed or explicitly deferred. If code lands, the colour path stays
byte-identical.

## Dependencies

- [The fixed, stock-agnostic decode](fixed-decode.md)

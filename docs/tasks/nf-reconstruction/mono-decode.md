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

## What depends on this

Two existing tasks are carried and blocked on the answer:

- [`algo/bw-support`](../algo/bw-support.md) places pooling "post-algorithm,
  pre-output-transform" — a seam the new chain does not have. Its knob shape (one
  enum, weights, validation against the resolved model) is still sound; its placement
  is not.
- [`io/gray-primary-decode`](../io/gray-primary-decode.md) cannot be finished without
  it: its first open question — replicate the single channel into three, or add a
  one-channel variant — is this decision seen from the input end, and it also changes
  `pipeline::memory`'s per-pixel figures.

## Open questions

- Does `density.scale` apply to a silver negative at all, and is the answer the same
  for chromogenic B&W?
- Is mono a **mode** (the chain differs throughout) or a **stage** (one pooling step in
  an unchanged chain)? The first is honest about how much differs; the second is far
  less code. And: grayscale encode, or 3-channel R == G == B as `bw-support` assumed?

## How to Verify

This is a decision task and may ship little or no code. It owes a written answer
naming the stage, the treatment of the 3×3, and whether silver and chromogenic B&W
differ — recorded where the two carried tasks can cite it, with their own open
questions then closed or explicitly deferred. If code lands, the colour path stays
byte-identical.

## Dependencies

- [The fixed, stock-agnostic decode](fixed-decode.md)
